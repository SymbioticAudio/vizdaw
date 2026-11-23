//! Co-Producer's mixer chores (plan 8d Rulings 1 and 3): plug-in
//! references resolved by the Plugin Manager's rules and plan 6g's Multi-Q
//! rules, the list of plug-ins Co-Producer may use, and the core-model
//! commands each mixer edit builds ([`commands_for`]). The preview
//! (`co_producer::preview_items`) and the apply (`Session::run_mixer`)
//! both build through [`commands_for`], so what the composer says yes to
//! is what runs. Every argument is hostile: each is read by name and
//! checked again here.
//!
//! Every naming of Multi-Q (`multi-q`, `clap:<MULTIQ_CLAP_ID>`,
//! `au:<MULTIQ_AU_ID>`) is the bundled CLAP that [`bundled::multiq`]
//! answers: the AU copy exists only for other apps, and Co-Producer never
//! loads it. Any other reference is the catalog's preferred entry, usable
//! only when OK or not yet validated (so not turned off, blocklisted or
//! missing) and of the kind the command needs.

use crate::bundled;
use crate::catalog::PluginKindTag;
use crate::error::VizError;
use crate::guard::ffi_guard;
use crate::snapshot::ParamRow;
use core_control::registry::{
    self, ArgValue, GROUP_COLOURS, GROUP_IDS, MIXER_IDS, NULL_TEXT, VCA_MODE_NAMES,
};
use core_host::PluginKind;
use core_host::bundled::{MULTIQ_AU_ID, MULTIQ_CLAP_ID};
use core_host::catalog::{Catalog, CatalogEntry, PluginStatus};
use core_model::command::{Command, GroupCmd, MixerCmd};
use core_model::diff::shown_name;
use core_model::document::{Document, SlotOwner};
use core_model::groups::{Link, VCA_MODES, group_gains, group_of, linked};
use core_model::ids::{GroupId, PluginInstanceId, StripId, VcaId};
use core_model::mixer::{
    AuxSend, GAIN_FLOOR_DB, GAIN_MAX_DB, Output, ParamId, PluginFormat, PluginRef, Strip,
    StripKind, Swatch, vst3_class_id,
};

/// The most rows [`co_producer_plugins`] gives.
pub const MAX_PLUGINS: usize = 5000;

/// The most parameters `Session::co_producer_params` gives.
pub const MAX_PARAMS: usize = 1000;

/// The bundled Multi-Q's reference text.
const MULTI_Q: &str = "multi-q";

/// The Built-in Synth's reference text (`track.new_plugin_instrument`).
pub const BUILT_IN_SYNTH: &str = "built-in-synth";

fn refuse<T>(message: String) -> Result<T, VizError> {
    Err(VizError::Model { message })
}

/// Whether `format`/`id` names Multi-Q.
fn is_multiq(format: PluginFormat, id: &str) -> bool {
    match format {
        PluginFormat::Clap => id == MULTIQ_CLAP_ID,
        PluginFormat::Au => id == MULTIQ_AU_ID,
        PluginFormat::Vst3 => false,
    }
}

fn kind_text(k: PluginKind) -> &'static str {
    match k {
        PluginKind::Instrument => "an instrument",
        PluginKind::Effect => "an effect",
    }
}

/// Ruling 3. `want` is the kind the command needs.
pub fn resolve_plugin(text: &str, want: PluginKind) -> Result<PluginRef, VizError> {
    if text == BUILT_IN_SYNTH {
        // Not a plug-in: `track.new_plugin_instrument` takes it before
        // anything is resolved.
        return refuse(match want {
            PluginKind::Effect => "Built-in Synth is an instrument, not an effect.".into(),
            PluginKind::Instrument => "Built-in Synth is built in, not a plug-in.".into(),
        });
    }
    let named = if text == MULTI_Q {
        None
    } else if let Some(id) = text.strip_prefix("clap:") {
        Some((PluginFormat::Clap, id))
    } else if let Some(id) = text.strip_prefix("au:") {
        Some((PluginFormat::Au, id))
    } else if let Some(rest) = text.strip_prefix("vst3:") {
        // A VST3 class ID: 32 hexadecimal digits, either case.
        let Some(id) = vst3_class_id(rest) else {
            return refuse(format!("No installed plug-in is {}.", shown_name(text)));
        };
        return crate::catalog::with(|c| from_catalog(c, text, PluginFormat::Vst3, &id, want));
    } else {
        return refuse(format!("No installed plug-in is {}.", shown_name(text)));
    };
    match named {
        Some((format, id)) if !is_multiq(format, id) => {
            if id.is_empty() {
                return refuse(format!("No installed plug-in is {}.", shown_name(text)));
            }
            crate::catalog::with(|c| from_catalog(c, text, format, id, want))
        }
        _ => {
            if want == PluginKind::Instrument {
                return refuse("Multi-Q is an effect, not an instrument.".into());
            }
            bundled::multiq().ok_or_else(|| VizError::Model {
                message: "Multi-Q is not available in this build.".into(),
            })
        }
    }
}

/// The catalog's preferred entry of `format`/`id` when usable as `want`.
fn from_catalog(
    c: &Catalog,
    text: &str,
    format: PluginFormat,
    id: &str,
    want: PluginKind,
) -> Result<PluginRef, VizError> {
    let Some(e) = c.preferred(format, id) else {
        return refuse(format!("No installed plug-in is {}.", shown_name(text)));
    };
    let name = shown_name(&e.plugin.name);
    match c.status_of(e) {
        PluginStatus::Ok | PluginStatus::NotValidated => {}
        PluginStatus::Disabled => {
            return refuse(format!("{name} is turned off in the Plugin Manager."));
        }
        PluginStatus::Missing => return refuse(format!("{name} cannot be used: it is missing.")),
        PluginStatus::Blocklisted(reason) => {
            return refuse(format!(
                "{name} cannot be used: it is blocklisted ({}).",
                shown_name(&reason)
            ));
        }
    }
    match e.kind {
        Some(k) if k == want => Ok(e.plugin.clone()),
        Some(k) => refuse(format!(
            "{name} is {}, not {}.",
            kind_text(k),
            kind_text(want)
        )),
        None => refuse(format!(
            "{name} cannot be used: VizDAW does not know whether it is an instrument or an effect."
        )),
    }
}

/// The reference text for `p`: "multi-q" for the bundled Multi-Q, else
/// "clap:<id>", "au:<id>" or "vst3:<class ID>".
pub fn plugin_ref_text(p: &PluginRef) -> String {
    match p.format {
        PluginFormat::Clap if p.id == MULTIQ_CLAP_ID => MULTI_Q.into(),
        PluginFormat::Clap => format!("clap:{}", p.id),
        PluginFormat::Au => format!("au:{}", p.id),
        PluginFormat::Vst3 => format!("vst3:{}", p.id),
    }
}

/// One plug-in Co-Producer may use (Ruling 3).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CoProducerPluginRow {
    /// The reference text to pass back: `multi-q`, `built-in-synth`,
    /// `clap:<id>` or `au:<id>`.
    pub plugin: String,
    /// Its name, safe to show.
    pub name: String,
    /// Its maker, safe to show; empty for the built-ins.
    pub maker: String,
    /// `au`, `clap` or `built_in`.
    pub format: String,
    /// Instrument or effect.
    pub kind: PluginKindTag,
    /// Its version, as the catalog has it.
    pub version: String,
}

/// The plug-ins Co-Producer may use, and whether the list stopped at
/// [`MAX_PLUGINS`].
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CoProducerPluginsRow {
    /// Built-ins first, then instruments, then effects, each by name.
    pub plugins: Vec<CoProducerPluginRow>,
    /// More were usable than listed.
    pub truncated: bool,
}

/// The plug-ins Co-Producer may use (Ruling 3): the built-ins, then the
/// catalog's usable plug-ins of a known kind, instruments first, each by
/// name ignoring case, then reference; at most [`MAX_PLUGINS`]. `kind`
/// keeps only that kind. Never fails: empty if anything goes wrong.
#[uniffi::export]
pub fn co_producer_plugins(kind: Option<PluginKindTag>) -> CoProducerPluginsRow {
    ffi_guard(|| Ok(plugins(kind))).unwrap_or(CoProducerPluginsRow {
        plugins: Vec::new(),
        truncated: false,
    })
}

fn plugins(kind: Option<PluginKindTag>) -> CoProducerPluginsRow {
    let wants = |k: PluginKindTag| kind.is_none_or(|w| w == k);
    let mut rows = Vec::new();
    if wants(PluginKindTag::Effect) {
        if let Some(mq) = bundled::multiq() {
            rows.push(CoProducerPluginRow {
                plugin: MULTI_Q.into(),
                name: "Multi-Q".into(),
                maker: String::new(),
                format: "built_in".into(),
                kind: PluginKindTag::Effect,
                version: shown_name(&mq.version),
            });
        }
    }
    if wants(PluginKindTag::Instrument) {
        rows.push(CoProducerPluginRow {
            plugin: BUILT_IN_SYNTH.into(),
            name: "Built-in Synth".into(),
            maker: String::new(),
            format: "built_in".into(),
            kind: PluginKindTag::Instrument,
            version: String::new(),
        });
    }
    let mut found: Vec<CoProducerPluginRow> = crate::catalog::with(|c| {
        c.entries()
            .iter()
            .filter(|e| usable(c, e))
            .filter_map(|e| {
                let k = PluginKindTag::from(e.kind?);
                let plugin = plugin_ref_text(&e.plugin);
                // Only what the registry takes back (Ruling 3's shape).
                core_control::registry::plugin_ref(&plugin, false).ok()?;
                wants(k).then(|| CoProducerPluginRow {
                    plugin,
                    name: shown_name(&e.plugin.name),
                    maker: shown_name(&e.plugin.vendor),
                    format: match e.plugin.format {
                        PluginFormat::Au => "au".into(),
                        PluginFormat::Clap => "clap".into(),
                        PluginFormat::Vst3 => "vst3".into(),
                    },
                    kind: k,
                    version: shown_name(&e.plugin.version),
                })
            })
            .collect()
    });
    found.sort_by(|a, b| {
        let rank = |k: PluginKindTag| u8::from(k != PluginKindTag::Instrument);
        rank(a.kind)
            .cmp(&rank(b.kind))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.plugin.cmp(&b.plugin))
    });
    rows.extend(found);
    let truncated = rows.len() > MAX_PLUGINS;
    rows.truncate(MAX_PLUGINS);
    CoProducerPluginsRow {
        plugins: rows,
        truncated,
    }
}

/// Whether `e` is a plug-in [`resolve_plugin`] gives: the preferred entry
/// of its ID, not Multi-Q (listed once, as a built-in), OK or not yet
/// validated, of a known kind.
fn usable(c: &Catalog, e: &CatalogEntry) -> bool {
    let p = &e.plugin;
    !p.id.is_empty()
        && e.kind.is_some()
        && !is_multiq(p.format, &p.id)
        && !(p.format == PluginFormat::Clap && p.id == MULTIQ_AU_ID)
        && !(p.format == PluginFormat::Au && p.id == MULTIQ_CLAP_ID)
        && c.preferred(p.format, &p.id)
            .is_some_and(|x| std::ptr::eq(x, e))
        && matches!(
            c.status_of(e),
            PluginStatus::Ok | PluginStatus::NotValidated
        )
}

/// What [`commands_for`] asks of the plug-in host; the Session's reads the
/// running plug-ins, and tests pass a fake.
pub trait PluginLookup {
    /// `text` resolved as a plug-in of kind `want` (Ruling 3).
    fn resolve(&self, text: &str, want: PluginKind) -> Result<PluginRef, VizError>;
    /// Slot `slot`'s parameters with their present values, hidden ones
    /// included; `None` while its plug-in is not running.
    fn params(&self, slot: PluginInstanceId) -> Option<Vec<ParamRow>>;
}

/// Whether [`commands_for`] builds `id`: one of [`MIXER_IDS`] but
/// `track.new_plugin_instrument`, one of [`GROUP_IDS`], or
/// `mixer.new_bus` (with `strip`).
pub fn builds(id: &str) -> bool {
    id == "mixer.new_bus"
        || GROUP_IDS.contains(&id)
        || (id != "track.new_plugin_instrument" && MIXER_IDS.contains(&id))
}

/// The swatches in Ruling 4's order, as [`GROUP_COLOURS`] names them.
const SWATCHES: [Swatch; 8] = [
    Swatch::Red,
    Swatch::Orange,
    Swatch::Yellow,
    Swatch::Green,
    Swatch::Teal,
    Swatch::Blue,
    Swatch::Purple,
    Swatch::Pink,
];

/// Ruling 4's default colour for a new group when `count` exist: the
/// swatches in order, round again after the eighth.
fn default_swatch(count: usize) -> Swatch {
    SWATCHES
        .get(count % SWATCHES.len())
        .copied()
        .unwrap_or(Swatch::Red)
}

/// The undo step `cmds` (built by [`commands_for`] for `id`, whose
/// registry title is `title`) run as: a group command's own name (so
/// `mixer.new_group` is "New Group", not its menu title "Group"), but
/// always "Set VCA" for `mixer.set_vca` (preflight F11: a mute-only call
/// too), "Move Group ‹name›" for a `mixer.set_strip` whose fader move the
/// group follows, else `title`.
pub fn step_label(doc: &Document, id: &str, title: &str, cmds: &[Command]) -> String {
    if id == "mixer.set_vca" {
        return SET_VCA.to_string();
    }
    if let [Command::Groups(g)] = cmds {
        return g.name().to_string();
    }
    let gains: Vec<StripId> = cmds
        .iter()
        .filter_map(|c| match c {
            Command::Mixer(MixerCmd::SetGain { strip, .. }) => Some(*strip),
            _ => None,
        })
        .collect();
    if id == "mixer.set_strip" && gains.len() > 1 {
        if let Some(g) = gains.first().and_then(|s| group_of(doc.mixer(), *s)) {
            return move_group_label(&g.name);
        }
    }
    title.to_string()
}

/// The undo step of a group move, from `mixer.set_strip` or a gesture:
/// "Move Group ‹name›".
pub(crate) fn move_group_label(name: &str) -> String {
    format!("Move Group {}", shown_name(name))
}

/// `mixer.set_vca`'s one undo step, whatever it sets.
const SET_VCA: &str = "Set VCA";

/// A required group ID.
fn group_arg(id: &str, args: &[(String, ArgValue)]) -> Result<GroupId, VizError> {
    need_id(id, args, "group").map(GroupId)
}

/// A `strips` list ([`core_control::co_producer::id_list`]); absent is
/// empty (Ruling 11), and `required` refuses that.
fn strips_arg(
    id: &str,
    args: &[(String, ArgValue)],
    required: bool,
) -> Result<Vec<StripId>, VizError> {
    match opt_text(id, args, "strips")? {
        None if required => Err(bad(format!("{id}: give `strips`"))),
        None => Ok(Vec::new()),
        Some(t) => core_control::co_producer::id_list(t)
            .map(|ids| ids.into_iter().map(StripId).collect())
            .map_err(|m| bad(format!("{id}: `strips`: {m}"))),
    }
}

/// A `colour` of [`GROUP_COLOURS`].
fn colour_arg(id: &str, args: &[(String, ArgValue)]) -> Result<Option<Swatch>, VizError> {
    opt_text(id, args, "colour")?
        .map(|t| {
            GROUP_COLOURS
                .iter()
                .position(|c| *c == t)
                .and_then(|i| SWATCHES.get(i).copied())
                .ok_or_else(|| {
                    bad(format!(
                        "{id}: `colour` is one of {}",
                        GROUP_COLOURS.join(", ")
                    ))
                })
        })
        .transpose()
}

/// A required `name`, as given (the model trims and checks it).
fn name_arg(id: &str, args: &[(String, ArgValue)]) -> Result<String, VizError> {
    Ok(need_text(id, args, "name")?.to_string())
}

/// A required VCA ID.
fn vca_arg(id: &str, args: &[(String, ArgValue)]) -> Result<VcaId, VizError> {
    need_id(id, args, "vca").map(VcaId)
}

/// The [`GroupCmd`]s of group or VCA command `id`, in order: one, but
/// `mixer.set_vca`'s level, mute, solo and mode, those given.
fn group_commands(
    doc: &Document,
    id: &str,
    args: &[(String, ArgValue)],
) -> Result<Vec<GroupCmd>, VizError> {
    if id != "mixer.set_vca" {
        let one = match vca_command(doc, id, args)? {
            Some(c) => c,
            None => group_command(doc, id, args)?,
        };
        return Ok(vec![one]);
    }
    let vca = vca_arg(id, args)?;
    let mode = opt_text(id, args, "mode")?
        .map(|t| {
            VCA_MODE_NAMES
                .iter()
                .position(|m| *m == t)
                .and_then(|i| VCA_MODES.get(i).copied())
                .ok_or_else(|| bad("A VCA's automation mode is Off, Read, Touch or Latch."))
        })
        .transpose()?;
    let mut out = Vec::new();
    out.extend(level(id, args, "gain_db")?.map(|gain_db| GroupCmd::SetVcaLevel { vca, gain_db }));
    out.extend(opt_flag(id, args, "mute")?.map(|mute| GroupCmd::SetVcaMute { vca, mute }));
    out.extend(opt_flag(id, args, "solo")?.map(|solo| GroupCmd::SetVcaSolo { vca, solo }));
    out.extend(mode.map(|mode| GroupCmd::SetVcaMode { vca, mode }));
    if out.is_empty() {
        return Err(bad(format!(
            "{id}: give at least one of gain_db, mute, solo and mode"
        )));
    }
    Ok(out)
}

/// The one [`GroupCmd`] of VCA command `id` but `mixer.set_vca` (Ruling
/// 4's defaults: "VCA N" and the swatch by count, N one more than the
/// VCAs); `None` for a group command.
fn vca_command(
    doc: &Document,
    id: &str,
    args: &[(String, ArgValue)],
) -> Result<Option<GroupCmd>, VizError> {
    Ok(Some(match id {
        "mixer.new_vca" => {
            let count = doc.mixer().vcas().len();
            GroupCmd::NewVca {
                members: strips_arg(id, args, false)?,
                name: match opt_text(id, args, "name")? {
                    Some(n) => n.to_string(),
                    None => format!("VCA {}", count + 1),
                },
                colour: colour_arg(id, args)?.unwrap_or_else(|| default_swatch(count)),
            }
        }
        "mixer.rename_vca" => GroupCmd::RenameVca {
            vca: vca_arg(id, args)?,
            name: name_arg(id, args)?,
        },
        "mixer.set_vca_colour" => GroupCmd::SetVcaColour {
            vca: vca_arg(id, args)?,
            colour: colour_arg(id, args)?.ok_or_else(|| bad(format!("{id}: give `colour`")))?,
        },
        "mixer.assign_vca" => GroupCmd::AssignVca {
            vca: vca_arg(id, args)?,
            strips: strips_arg(id, args, true)?,
        },
        "mixer.unassign_vca" => GroupCmd::UnassignVca {
            vca: vca_arg(id, args)?,
            strips: strips_arg(id, args, true)?,
        },
        "mixer.delete_vca" => GroupCmd::DeleteVca {
            vca: vca_arg(id, args)?,
        },
        _ => return Ok(None),
    }))
}

/// The one [`GroupCmd`] of group command `id` (Ruling 4's defaults: "Group
/// N" and the swatch by count, N one more than the groups).
fn group_command(
    doc: &Document,
    id: &str,
    args: &[(String, ArgValue)],
) -> Result<GroupCmd, VizError> {
    Ok(match id {
        "mixer.new_group" => {
            let count = doc.mixer().groups().len();
            GroupCmd::NewGroup {
                members: strips_arg(id, args, false)?,
                name: match opt_text(id, args, "name")? {
                    Some(n) => n.to_string(),
                    None => format!("Group {}", count + 1),
                },
                colour: colour_arg(id, args)?.unwrap_or_else(|| default_swatch(count)),
            }
        }
        "mixer.rename_group" => GroupCmd::RenameGroup {
            group: group_arg(id, args)?,
            name: name_arg(id, args)?,
        },
        "mixer.set_group_colour" => GroupCmd::SetGroupColour {
            group: group_arg(id, args)?,
            colour: colour_arg(id, args)?.ok_or_else(|| bad(format!("{id}: give `colour`")))?,
        },
        "mixer.add_to_group" => GroupCmd::AddToGroup {
            group: group_arg(id, args)?,
            strips: strips_arg(id, args, true)?,
        },
        "mixer.remove_from_group" => GroupCmd::RemoveFromGroup {
            group: group_arg(id, args)?,
            strips: strips_arg(id, args, true)?,
        },
        "mixer.set_group_links" => {
            let (mute, solo) = (opt_flag(id, args, "mute")?, opt_flag(id, args, "solo")?);
            if mute.is_none() && solo.is_none() {
                return Err(bad(format!("{id}: give at least one of mute and solo")));
            }
            GroupCmd::SetGroupLinks {
                group: group_arg(id, args)?,
                mute,
                solo,
            }
        }
        "mixer.delete_group" => GroupCmd::DeleteGroup {
            group: group_arg(id, args)?,
        },
        _ => {
            let shown: String = id.chars().take(64).collect();
            return Err(bad(format!(
                "{} is not a group command.",
                shown_name(&shown)
            )));
        }
    })
}

fn bad(message: impl Into<String>) -> VizError {
    VizError::Model {
        message: message.into(),
    }
}

fn arg<'a>(args: &'a [(String, ArgValue)], name: &str) -> Option<&'a ArgValue> {
    args.iter().find(|(n, _)| n == name).map(|(_, v)| v)
}

fn opt_int(id: &str, args: &[(String, ArgValue)], name: &str) -> Result<Option<i64>, VizError> {
    match arg(args, name) {
        None => Ok(None),
        Some(ArgValue::Int(i)) => Ok(Some(*i)),
        Some(_) => Err(bad(format!("{id}: `{name}` must be a whole number"))),
    }
}

/// A required ID (strip, slot, bus) or 1-based index: at least 1.
fn need_id(id: &str, args: &[(String, ArgValue)], name: &str) -> Result<u64, VizError> {
    let v = opt_int(id, args, name)?.ok_or_else(|| bad(format!("{id}: give `{name}`")))?;
    u64::try_from(v)
        .ok()
        .filter(|v| *v >= 1)
        .ok_or_else(|| bad(format!("{id}: `{name}` is out of range")))
}

/// A strip ID: as [`need_id`], but 0 (a new song's master) too, so the
/// master's own refusals answer it.
fn need_strip(id: &str, args: &[(String, ArgValue)]) -> Result<StripId, VizError> {
    let v = opt_int(id, args, "strip")?.ok_or_else(|| bad(format!("{id}: give `strip`")))?;
    u64::try_from(v)
        .map(StripId)
        .map_err(|_| bad(format!("{id}: `strip` is out of range")))
}

fn opt_flag(id: &str, args: &[(String, ArgValue)], name: &str) -> Result<Option<bool>, VizError> {
    match arg(args, name) {
        None => Ok(None),
        Some(ArgValue::Flag(b)) => Ok(Some(*b)),
        Some(_) => Err(bad(format!("{id}: `{name}` must be true or false"))),
    }
}

fn opt_text<'a>(
    id: &str,
    args: &'a [(String, ArgValue)],
    name: &str,
) -> Result<Option<&'a str>, VizError> {
    match arg(args, name) {
        None => Ok(None),
        Some(ArgValue::Text(t)) => Ok(Some(t)),
        Some(_) => Err(bad(format!("{id}: `{name}` must be text"))),
    }
}

fn need_text<'a>(
    id: &str,
    args: &'a [(String, ArgValue)],
    name: &str,
) -> Result<&'a str, VizError> {
    opt_text(id, args, name)?.ok_or_else(|| bad(format!("{id}: give `{name}` as text")))
}

/// A fader or send level in dB (Ruling 1's decimal, −144 to 12).
fn level(id: &str, args: &[(String, ArgValue)], name: &str) -> Result<Option<f64>, VizError> {
    opt_text(id, args, name)?
        .map(|t| {
            registry::decimal(t, GAIN_FLOOR_DB, GAIN_MAX_DB)
                .map_err(|m| bad(format!("{id}: `{name}` {m}")))
        })
        .transpose()
}

fn strip_of(doc: &Document, strip: StripId) -> Result<&Strip, VizError> {
    doc.mixer()
        .strip(strip)
        .map_err(|_| bad(format!("This song has no strip {}.", strip.0)))
}

/// `bus`, which must be a bus strip: "Strip <n> is not a bus." otherwise.
fn bus_of(doc: &Document, bus: u64) -> Result<StripId, VizError> {
    let s = strip_of(doc, StripId(bus))?;
    if s.kind != StripKind::Bus {
        return Err(bad(format!("Strip {bus} is not a bus.")));
    }
    Ok(s.id)
}

fn slot_of(
    doc: &Document,
    id: &str,
    args: &[(String, ArgValue)],
) -> Result<(PluginInstanceId, SlotOwner), VizError> {
    let n = need_id(id, args, "slot")?;
    let slot = PluginInstanceId(n);
    let owner = doc
        .find_slot(slot)
        .ok_or_else(|| bad(format!("This song has no slot {n}.")))?;
    Ok((slot, owner))
}

/// An insert slot: "Slot <n> is not an insert." for an EQ or an
/// instrument's own slot.
fn insert_of(
    doc: &Document,
    id: &str,
    args: &[(String, ArgValue)],
) -> Result<(PluginInstanceId, StripId), VizError> {
    match slot_of(doc, id, args)? {
        (slot, SlotOwner::Insert(strip, _)) => Ok((slot, strip)),
        (slot, _) => Err(bad(format!("Slot {} is not an insert.", slot.0))),
    }
}

fn inserts_refusal(s: &Strip) -> VizError {
    bad(format!(
        "Strip {} has {} inserts.",
        shown_name(&s.name),
        s.inserts.len()
    ))
}

/// A 1-based send `index` of `s`, as a position.
fn send_at(id: &str, args: &[(String, ArgValue)], s: &Strip) -> Result<usize, VizError> {
    let i = need_id(id, args, "index")?;
    usize::try_from(i - 1)
        .ok()
        .filter(|i| *i < s.sends.len())
        .ok_or_else(|| {
            bad(format!(
                "Strip {} has {} sends.",
                shown_name(&s.name),
                s.sends.len()
            ))
        })
}

/// "Bus N" with the lowest N ≥ 1 no bus is named (plan 6g-2 Ruling 8),
/// as the app's `MixerModel.nextBusName` gives it.
pub fn next_bus_name(doc: &Document) -> String {
    let taken: std::collections::BTreeSet<&str> = doc
        .mixer()
        .strips()
        .values()
        .filter(|s| s.kind == StripKind::Bus)
        .map(|s| s.name.as_str())
        .collect();
    (1u64..)
        .map(|n| format!("Bus {n}"))
        .find(|n| !taken.contains(n.as_str()))
        .unwrap_or_else(|| "Bus".into())
}

/// A whole parameter value as text: at most 6 decimals, trailing zeros
/// dropped.
fn shown_value(v: f64) -> String {
    let t = format!("{v:.6}");
    let t = t.trim_end_matches('0').trim_end_matches('.');
    if t == "-0" { "0".into() } else { t.into() }
}

/// `mixer.set_param`'s one pair (Ruling 1): the plug-in running, the
/// parameter listed and not hidden, the value within its range, and whole
/// for a stepped one.
fn param_pair(
    id: &str,
    args: &[(String, ArgValue)],
    slot: PluginInstanceId,
    host: &dyn PluginLookup,
) -> Result<(ParamId, f64), VizError> {
    let param = opt_int(id, args, "param")?.ok_or_else(|| bad(format!("{id}: give `param`")))?;
    let param = u64::try_from(param).map_err(|_| bad(format!("{id}: `param` is out of range")))?;
    let value = need_text(id, args, "value")?;
    let rows = host.params(slot).ok_or_else(|| {
        bad(format!(
            "The plug-in in slot {} is not running yet; read plugin_params again in a moment.",
            slot.0
        ))
    })?;
    let row = rows
        .iter()
        .find(|r| r.id == param && !r.hidden)
        .ok_or_else(|| bad(format!("Slot {} has no parameter {param}.", slot.0)))?;
    let v = registry::decimal(value, -1_000_000.0, 1_000_000.0)
        .map_err(|m| bad(format!("{id}: `value` {m}")))?;
    let name = shown_name(&row.name);
    if !(row.min.is_finite() && row.max.is_finite() && (row.min..=row.max).contains(&v)) {
        return Err(bad(format!(
            "{name} is {} to {}.",
            shown_value(row.min),
            shown_value(row.max)
        )));
    }
    if row.stepped && v.fract() != 0.0 {
        return Err(bad(format!("{name} takes whole values.")));
    }
    Ok((ParamId(param), v))
}

/// The core-model commands for one of [`MIXER_IDS`] but
/// `track.new_plugin_instrument`, one of [`GROUP_IDS`] (its
/// [`GroupCmd`]s: one, or `mixer.set_vca`'s in the order level, mute,
/// solo, mode), or for `mixer.new_bus` with `strip`, in order;
/// `mixer.set_strip` moves the strip's group and its linked switches too
/// unless `alone` (groups-vcas-transport Task 2); Ruling 1's refusals, all found from `doc` (and `host`) before
/// any command is built. `args` are the registry's, read by name and
/// checked again. A routing cycle is the model's own refusal when the
/// command runs.
pub fn commands_for(
    doc: &Document,
    id: &str,
    args: &[(String, ArgValue)],
    host: &dyn PluginLookup,
) -> Result<Vec<Command>, VizError> {
    if !builds(id) {
        let shown: String = id.chars().take(64).collect();
        return Err(bad(format!("{} is not a mixer edit.", shown_name(&shown))));
    }
    if GROUP_IDS.contains(&id) {
        return Ok(group_commands(doc, id, args)?
            .into_iter()
            .map(Command::Groups)
            .collect());
    }
    let master = doc.mixer().master();
    let mix = |c: MixerCmd| Command::Mixer(c);
    let cmds = match id {
        "mixer.set_strip" => {
            let strip = strip_of(doc, need_strip(id, args)?)?.id;
            let gain = level(id, args, "gain_db")?;
            let pan = opt_int(id, args, "pan")?
                .map(|p| {
                    (-100..=100)
                        .contains(&p)
                        .then(|| p as f64 / 100.0)
                        .ok_or_else(|| bad(format!("{id}: `pan` is -100 to 100")))
                })
                .transpose()?;
            let (mute, solo) = (opt_flag(id, args, "mute")?, opt_flag(id, args, "solo")?);
            // The group follows (its fader by the same dB, its linked
            // switches), unless `alone`.
            let alone = opt_flag(id, args, "alone")?.unwrap_or(false);
            let m = doc.mixer();
            let switch = |link| {
                if alone {
                    vec![strip]
                } else {
                    linked(m, strip, link)
                }
            };
            let mut out = Vec::new();
            if let Some(gain_db) = gain {
                let moves = if alone {
                    vec![(strip, gain_db)]
                } else {
                    group_gains(m, strip, gain_db)
                };
                out.extend(
                    moves
                        .into_iter()
                        .map(|(strip, gain_db)| mix(MixerCmd::SetGain { strip, gain_db })),
                );
            }
            out.extend(pan.map(|pan| mix(MixerCmd::SetPan { strip, pan })));
            if let Some(mute) = mute {
                out.extend(
                    switch(Link::Mute)
                        .into_iter()
                        .map(|strip| mix(MixerCmd::SetMute { strip, mute })),
                );
            }
            if let Some(solo) = solo {
                out.extend(
                    switch(Link::Solo)
                        .into_iter()
                        .map(|strip| mix(MixerCmd::SetSolo { strip, solo })),
                );
            }
            if out.is_empty() {
                return Err(bad(format!(
                    "{id}: give at least one of gain_db, pan, mute and solo"
                )));
            }
            out
        }
        "mixer.set_output" => {
            let strip = strip_of(doc, need_strip(id, args)?)?.id;
            if strip == master {
                return Err(bad("Master has no output to set."));
            }
            let output = match arg(args, "bus") {
                Some(ArgValue::Text(t)) if t == NULL_TEXT => Output::Master,
                Some(ArgValue::Int(_)) => Output::Bus(bus_of(doc, need_id(id, args, "bus")?)?),
                _ => return Err(bad(format!("{id}: give `bus`, or null for master"))),
            };
            vec![mix(MixerCmd::SetOutput { strip, output })]
        }
        "mixer.add_send" => {
            let strip = strip_of(doc, need_strip(id, args)?)?.id;
            if strip == master {
                return Err(bad("Master has no sends."));
            }
            let target = bus_of(doc, need_id(id, args, "bus")?)?;
            let send = AuxSend {
                level_db: level(id, args, "level_db")?.unwrap_or(0.0),
                pre_fader: opt_flag(id, args, "pre_fader")?.unwrap_or(false),
                ..AuxSend::new(target)
            };
            vec![mix(MixerCmd::AddSend { strip, send })]
        }
        "mixer.set_send" => {
            let s = strip_of(doc, need_strip(id, args)?)?;
            let index = send_at(id, args, s)?;
            let level_db = level(id, args, "level_db")?;
            let pre_fader = opt_flag(id, args, "pre_fader")?;
            let enabled = opt_flag(id, args, "enabled")?;
            if level_db.is_none() && pre_fader.is_none() && enabled.is_none() {
                return Err(bad(format!(
                    "{id}: give at least one of level_db, pre_fader and enabled"
                )));
            }
            let old = s
                .sends
                .get(index)
                .copied()
                .ok_or_else(|| bad(format!("{id}: no such send")))?;
            let send = AuxSend {
                level_db: level_db.unwrap_or(old.level_db),
                pre_fader: pre_fader.unwrap_or(old.pre_fader),
                enabled: enabled.unwrap_or(old.enabled),
                ..old
            };
            vec![mix(MixerCmd::EditSend {
                strip: s.id,
                index,
                send,
            })]
        }
        "mixer.remove_send" => {
            let s = strip_of(doc, need_strip(id, args)?)?;
            let index = send_at(id, args, s)?;
            vec![mix(MixerCmd::RemoveSend { strip: s.id, index })]
        }
        "mixer.add_insert" => {
            let s = strip_of(doc, need_strip(id, args)?)?;
            let index = opt_int(id, args, "index")?
                .map(|i| {
                    usize::try_from(i)
                        .ok()
                        .and_then(|i| i.checked_sub(1))
                        .filter(|i| *i <= s.inserts.len())
                        .ok_or_else(|| inserts_refusal(s))
                })
                .transpose()?;
            let plugin = host.resolve(need_text(id, args, "plugin")?, PluginKind::Effect)?;
            vec![mix(MixerCmd::AddInsert {
                strip: s.id,
                index,
                plugin,
            })]
        }
        "mixer.remove_insert" => {
            let (slot, _) = insert_of(doc, id, args)?;
            vec![mix(MixerCmd::RemoveInsert { slot })]
        }
        "mixer.move_insert" => {
            let (slot, strip) = insert_of(doc, id, args)?;
            let s = strip_of(doc, strip)?;
            let index = usize::try_from(need_id(id, args, "index")? - 1)
                .ok()
                .filter(|i| *i < s.inserts.len())
                .ok_or_else(|| inserts_refusal(s))?;
            vec![mix(MixerCmd::MoveInsert { slot, strip, index })]
        }
        "mixer.set_bypass" => {
            let (slot, _) = slot_of(doc, id, args)?;
            let bypass =
                opt_flag(id, args, "bypass")?.ok_or_else(|| bad(format!("{id}: give `bypass`")))?;
            vec![mix(MixerCmd::SetBypass { slot, bypass })]
        }
        "mixer.set_eq" => {
            let strip = strip_of(doc, need_strip(id, args)?)?.id;
            let plugin = host.resolve(need_text(id, args, "plugin")?, PluginKind::Effect)?;
            vec![mix(MixerCmd::SetEq {
                strip,
                plugin: Some(plugin),
            })]
        }
        "mixer.set_param" => {
            let (slot, _) = slot_of(doc, id, args)?;
            let pair = param_pair(id, args, slot, host)?;
            vec![mix(MixerCmd::SetPluginParams {
                slot,
                params: vec![pair],
            })]
        }
        "mixer.remove_bus" => {
            let s = strip_of(doc, need_strip(id, args)?)?;
            if s.id == master {
                return Err(bad("Master is not a bus."));
            }
            if s.kind != StripKind::Bus {
                return Err(bad(format!("Strip {} is not a bus.", s.id.0)));
            }
            vec![mix(MixerCmd::RemoveBus { strip: s.id })]
        }
        _ => {
            // `mixer.new_bus` with `strip`, as `new_bus_from` builds it:
            // the bus, then a post-fader 0 dB send to it.
            if arg(args, "strip").is_none() {
                return Err(bad(
                    "mixer.new_bus needs no preview: run it with command.run.",
                ));
            }
            let source = strip_of(doc, need_strip(id, args)?)?.id;
            if source == master {
                return Err(bad("Master has no sends."));
            }
            let name = match opt_text(id, args, "name")? {
                Some(n) if n.trim().is_empty() => return Err(bad("A bus needs a name.")),
                Some(n) => n.trim().to_string(),
                None => next_bus_name(doc),
            };
            // The ID the bus gets: the document's next strip.
            let bus = StripId(doc.counters().strip);
            vec![
                mix(MixerCmd::AddBus { name, index: None }),
                mix(MixerCmd::AddSend {
                    strip: source,
                    send: AuxSend::new(bus),
                }),
            ]
        }
    };
    Ok(cmds)
}

/// One feed into a bus: its source, and for a send its level (as bits),
/// tap and switch.
type Feed = (StripId, Option<(u64, bool, bool)>);

/// What feeds strip `bus` in `doc`: each source's output to it, and each
/// send to it with its level, tap and switch.
fn feeds(doc: &Document, bus: StripId) -> Vec<Feed> {
    doc.mixer()
        .strips()
        .values()
        .flat_map(|s| {
            let out = (s.output == Output::Bus(bus)).then_some((s.id, None));
            let sends = s
                .sends
                .iter()
                .filter(move |x| x.target == bus)
                .map(move |x| (s.id, Some((x.level_db.to_bits(), x.pre_fader, x.enabled))));
            out.into_iter().chain(sends)
        })
        .collect()
}

/// Whether the change from `before` to `after` changes what feeds a bus
/// of the composer's (one in `before` and not Co-Producer's): an output
/// routed to it or away from it, or a send to it added, set or removed.
/// Such a change alters what the composer's bus plays, so it needs their
/// yes even when every strip it names is Co-Producer's (the owner's rule
/// on the composer's routing and sound; stricter than Ruling 2 alone).
pub fn feeds_composers_bus(before: &Document, after: &Document) -> bool {
    before
        .mixer()
        .strips()
        .values()
        .filter(|s| s.kind == StripKind::Bus && !s.co_producer)
        .any(|b| feeds(before, b.id) != feeds(after, b.id))
}
