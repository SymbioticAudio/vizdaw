//! Co-Producer's mixer session methods (plan 8d Tasks 3–4), kept out of
//! `session.rs`: that file gains only `mod co_producer_mixer;` and the
//! one-line hooks these tasks name.

use super::{Session, edit_open_busy, records_busy};
use crate::control::ControlArg;
use crate::error::VizError;
use crate::guard::ffi_guard;
use crate::host;
use crate::mixer_chores::{
    BUILT_IN_SYNTH, MAX_PARAMS, PluginLookup, commands_for, plugin_ref_text, resolve_plugin,
    step_label,
};
use crate::plugins;
use crate::snapshot::{DocSnapshot, ParamRow};
use core_control::registry::{self, ArgValue, GROUP_IDS, MIXER_IDS};
use core_host::PluginKind;
use core_model::command::{Command, MixerCmd};
use core_model::diff::shown_name;
use core_model::ids::PluginInstanceId;
use core_model::mixer::{ParamId, PluginRef};
use core_model::selection::ViewFilter;

/// Slot `slot`'s parameters in session `session`'s plug-in host, hidden
/// ones included; `None` while it is not running. Reads only.
fn running_params(session: u64, slot: PluginInstanceId) -> Result<Option<Vec<ParamRow>>, VizError> {
    plugins::with(session, |h| {
        let inst = h.and_then(|h| h.instance(slot))?;
        let infos = inst.info().params.clone();
        Some(
            infos
                .into_iter()
                .map(|i| ParamRow {
                    id: i.id.0,
                    value: inst.param_value(i.id).unwrap_or(i.default),
                    name: i.name,
                    module: i.module,
                    min: i.min,
                    max: i.max,
                    default: i.default,
                    stepped: i.stepped,
                    automatable: i.automatable,
                    hidden: i.hidden,
                    read_only: i.read_only,
                })
                .collect(),
        )
    })
}

/// The live [`PluginLookup`] of session `.0`: references resolve by
/// Ruling 3, parameters are the running plug-ins' (a busy host answers
/// as not running). It changes nothing.
pub(crate) struct LiveLookup(pub(crate) u64);

impl PluginLookup for LiveLookup {
    fn resolve(&self, text: &str, want: PluginKind) -> Result<PluginRef, VizError> {
        resolve_plugin(text, want)
    }

    fn params(&self, slot: PluginInstanceId) -> Option<Vec<ParamRow>> {
        running_params(self.0, slot).ok().flatten()
    }
}

fn bad(message: impl Into<String>) -> VizError {
    VizError::Model {
        message: message.into(),
    }
}

/// A loaded plug-in's parameters for Co-Producer (plan 8d Ruling 3).
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct CoProducerParamsRow {
    /// The slot asked about.
    pub slot: u64,
    /// Its plug-in's reference text (`multi-q`, `clap:<id>`, `au:<id>`).
    pub plugin: String,
    /// The plug-in is running, so its parameters are known.
    pub running: bool,
    /// Its parameters with their current values, without hidden ones, at
    /// most [`MAX_PARAMS`]; empty while not running.
    pub params: Vec<ParamRow>,
}

/// Co-Producer's reads of the mixer (plan 8d Task 3).
#[uniffi::export]
impl Session {
    /// Slot `slot`'s plug-in and parameters (Ruling 3): `Err` "This song
    /// has no slot <n>." for a slot this document lacks. Reads the host as
    /// [`Session::params`] does, and changes nothing.
    pub fn co_producer_params(&self, slot: u64) -> Result<CoProducerParamsRow, VizError> {
        ffi_guard(|| {
            let inner = self.lock_inner();
            let id = PluginInstanceId(slot);
            let Some(p) = inner.doc().plugin_slot(id) else {
                return Err(VizError::Model {
                    message: format!("This song has no slot {slot}."),
                });
            };
            let plugin = plugin_ref_text(&p.plugin);
            let params = running_params(self.id, id)?.map(|rows| {
                rows.into_iter()
                    .filter(|r| !r.hidden)
                    .take(MAX_PARAMS)
                    .collect::<Vec<_>>()
            });
            Ok(CoProducerParamsRow {
                slot,
                plugin,
                running: params.is_some(),
                params: params.unwrap_or_default(),
            })
        })
    }
}

/// Co-Producer's mixer edits (plan 8d Task 4).
#[uniffi::export]
impl Session {
    /// Runs one of [`MIXER_IDS`] or [`GROUP_IDS`] as one step named by
    /// [`step_label`] (its title, Ruling 1; a group command's own name; a
    /// group's fader move "Move Group ‹name›"), as the preview built it
    /// ([`commands_for`]): `args` are validated by
    /// the registry and checked again; a refusal changes nothing. Inside a
    /// Co-Producer call the step is Co-Producer's, and joins the request's.
    /// `mixer.set_param` sends the value to the running plug-in first, as
    /// the typed-value path does. `track.new_plugin_instrument` adds the
    /// destination and its track in the same step (`seq` absent: the
    /// selected sequence; `after` absent: the end). `Busy` while an edit
    /// is open (and, for a new track, while recording).
    pub fn run_mixer(&self, id: String, args: Vec<ControlArg>) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            if !MIXER_IDS.contains(&id.as_str()) && !GROUP_IDS.contains(&id.as_str()) {
                let shown: String = id.chars().take(64).collect();
                return Err(bad(format!(
                    "{} is not a mixer command.",
                    shown_name(&shown)
                )));
            }
            let title = registry::find(&id)
                .map(|spec| spec.title)
                .ok_or_else(|| bad(format!("{id} is not a mixer command.")))?;
            let args = crate::co_producer::validated(&id, &args)?;
            if id == "track.new_plugin_instrument" {
                return self.new_plugin_instrument(title, &args);
            }
            self.run_mixer_edit(&id, title, &args)
        })
    }
}

impl Session {
    /// [`Session::run_mixer`] for a mixer edit.
    fn run_mixer_edit(
        &self,
        id: &str,
        title: &str,
        args: &[(String, ArgValue)],
    ) -> Result<DocSnapshot, VizError> {
        let host = LiveLookup(self.id);
        let (cmds, mut next) = {
            let inner = self.lock_inner();
            if inner.edit_label.is_some() || inner.history.in_transaction() {
                return Err(edit_open_busy());
            }
            let cmds = commands_for(inner.doc(), id, args, &host)?;
            (cmds, inner.doc().clone())
        };
        // Running plug-ins the edit removes (an insert, a replaced EQ, a
        // bus's chain) are captured first, so Undo restores their state.
        for c in &cmds {
            if let Ok(n) = core_model::command::apply(&next, c) {
                next = n.doc;
            }
        }
        self.capture_removed(&next)?;
        // The value reaches the running plug-in first (`set_param`, then
        // the step, as the typed-value path does); a refusal puts it back.
        let mut sent: Option<(PluginInstanceId, ParamId, f64)> = None;
        if let [Command::Mixer(MixerCmd::SetPluginParams { slot, params })] = cmds.as_slice() {
            if let [(param, value)] = params.as_slice() {
                let was = host
                    .params(*slot)
                    .and_then(|rows| rows.into_iter().find(|r| r.id == param.0))
                    .map(|r| r.value);
                plugins::with(self.id, |p| {
                    if let Some(p) = p {
                        p.set_param(*slot, *param, *value);
                    }
                })?;
                sent = was.map(|w| (*slot, *param, w));
            }
        }
        let put_back = || {
            if let Some((slot, param, was)) = sent {
                let _ = plugins::with(self.id, |p| {
                    if let Some(p) = p {
                        p.set_param(slot, param, was);
                    }
                });
            }
        };
        let mut inner = self.lock_inner();
        if inner.edit_label.is_some() || inner.history.in_transaction() {
            put_back();
            return Err(edit_open_busy());
        }
        // Built again under the lock the step runs under.
        let cmds = match commands_for(inner.doc(), id, args, &host) {
            Ok(c) => c,
            Err(e) => {
                put_back();
                return Err(e);
            }
        };
        let before = inner.doc().clone();
        let label = step_label(&before, id, title, &cmds);
        let filter = ViewFilter::default();
        let h = &mut inner.history;
        crate::bundled::arm(h);
        h.begin(label)?;
        let mut notes = Vec::new();
        for c in &cmds {
            match h.execute(c, &filter) {
                Ok(r) => notes.extend(r.warnings),
                Err(e) => {
                    let _ = h.abort();
                    put_back();
                    return Err(e.into());
                }
            }
        }
        h.commit()?;
        inner.version += 1;
        inner.reload_host(self.id);
        self.mark(&mut inner);
        self.release_vanished(&mut inner);
        self.resync_plugin_automation(&mut inner, Some(&before));
        self.send_mix_changes(&mut inner, &before);
        inner.snapshot_with(notes)
    }

    /// `track.new_plugin_instrument` (Ruling 1): `built-in-synth` makes the
    /// track `track.new_instrument` makes; anything else resolves as an
    /// instrument (Ruling 3) and gets its own destination.
    fn new_plugin_instrument(
        &self,
        title: &str,
        args: &[(String, ArgValue)],
    ) -> Result<DocSnapshot, VizError> {
        let int = |name: &str| -> Result<Option<u64>, VizError> {
            match args.iter().find(|(n, _)| n == name) {
                None => Ok(None),
                Some((_, ArgValue::Int(i))) => u64::try_from(*i).map(Some).map_err(|_| {
                    bad(format!(
                        "track.new_plugin_instrument: `{name}` is out of range"
                    ))
                }),
                Some(_) => Err(bad(format!(
                    "track.new_plugin_instrument: `{name}` must be a whole number"
                ))),
            }
        };
        let plugin = match args.iter().find(|(n, _)| n == "plugin") {
            Some((_, ArgValue::Text(t))) => t.as_str(),
            _ => return Err(bad("track.new_plugin_instrument: give `plugin` as text")),
        };
        let (seq, after) = (int("seq")?, int("after")?);
        // The Built-in Synth is no plug-in: it is checked before anything
        // is resolved.
        let resolved = if plugin == BUILT_IN_SYNTH {
            None
        } else {
            Some(resolve_plugin(plugin, PluginKind::Instrument)?)
        };
        let monitor = crate::audio::default_monitor();
        let mut inner = self.lock_inner();
        if inner.edit_label.is_some() || inner.history.in_transaction() {
            return Err(edit_open_busy());
        }
        records_busy(&inner)?;
        let seq = match seq {
            Some(s) => crate::snapshot::seq_id(inner.doc(), s)?,
            None => inner
                .selected_seq()
                .ok_or_else(|| bad("The song has no sequence for the track."))?,
        };
        let index = crate::tracks::index_below(inner.doc(), seq, after);
        let mut notes = Vec::new();
        let h = &mut inner.history;
        crate::bundled::arm(h);
        h.begin(title)?;
        let built = match resolved {
            None => crate::tracks::build_track(
                h,
                seq,
                crate::tracks::TrackKindRow::Instrument,
                index,
                monitor,
                &mut notes,
            ),
            Some(p) => {
                crate::tracks::build_plugin_instrument_track(h, seq, p, index, monitor, &mut notes)
            }
        };
        match built {
            Ok(_) => h.commit()?,
            Err(e) => {
                h.abort()?;
                return Err(e);
            }
        }
        inner.version += 1;
        host::clear_input_failure_of(self.id);
        inner.reload_host(self.id);
        self.mark(&mut inner);
        inner.snapshot_with(notes)
    }
}
