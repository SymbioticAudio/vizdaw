//! Co-Producer's requests, previews and one-time tokens (plan 8a Task 4;
//! Rulings 3, 6 and 7). The session methods live in `session.rs`; this
//! module holds their rows, the preview builder and the pending preview.
//!
//! A preview runs on a clone of the session's `History` and never reaches
//! the host, the engine, the disk or the UI. Its token is tied to the
//! session's version, the present step's serial and the plan's canonical
//! text, and is taken at most once. Every input is hostile.

use crate::control::{ControlArg, ControlValue};
use crate::error::VizError;
use core_control::co_producer::{self as gate, MAX_PLAN, Planned};
use core_control::registry::{self, ArgValue};
use core_control::request::Command as ControlCommand;
use core_model::diff::{self, ChangeItem};
use core_model::history::{History, RequestInfo};
use core_model::selection::ViewFilter;
use serde_json::{Map, Value};
use std::io::Read;
use std::path::Path;

/// What a Co-Producer request is (Ruling 3).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CoProducerRequestRow {
    /// The name as given, trimmed.
    pub name: String,
    /// Its undo label, "Co-Producer: <Title>".
    pub label: String,
    /// A group of it is open now.
    pub group_open: bool,
    /// An earlier group of it made a step: the next is "(continued)".
    pub continued: bool,
}

impl From<RequestInfo> for CoProducerRequestRow {
    fn from(r: RequestInfo) -> Self {
        CoProducerRequestRow {
            name: r.name,
            label: r.label,
            group_open: r.group_open,
            continued: r.continued,
        }
    }
}

/// One line of a preview (Ruling 6).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ChangeItemRow {
    /// What it acts on.
    pub thing: String,
    /// What happens to it.
    pub change: String,
    /// It changes or removes the composer's work, so it needs their yes.
    pub composers_work: bool,
}

/// What a plan would change, and the token to apply it with (Ruling 7).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct PreviewRow {
    /// 32 lowercase hex characters, taken once.
    pub token: String,
    /// Some item is the composer's work.
    pub needs_yes: bool,
    /// What changes, in order.
    pub items: Vec<ChangeItemRow>,
}

/// One planned command of a preview or apply.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct PlannedRow {
    /// Registry ID.
    pub id: String,
    /// Its arguments.
    pub args: Vec<ControlArg>,
}

/// The FFI form of a planned command.
pub fn planned_row(p: Planned) -> PlannedRow {
    PlannedRow {
        id: p.id,
        args: p
            .args
            .into_iter()
            .map(|(name, v)| ControlArg {
                name,
                value: match v {
                    ArgValue::Text(value) => ControlValue::Text { value },
                    ArgValue::Flag(value) => ControlValue::Flag { value },
                    ArgValue::Int(value) => ControlValue::Int { value },
                },
            })
            .collect(),
    }
}

fn model(message: impl Into<String>) -> VizError {
    VizError::Model {
        message: message.into(),
    }
}

/// `args` of registry command `id`, validated by the registry (no unknown,
/// duplicate or missing argument, every value of its type), in registry
/// order.
pub fn validated(id: &str, args: &[ControlArg]) -> Result<Vec<(String, ArgValue)>, VizError> {
    let spec = registry::find(id).ok_or_else(|| model(registry::unknown(id).message))?;
    let mut params = Map::new();
    for a in args {
        // A null `IntOrNull` reaches the app as the registry's `none`
        // (`mixer.set_output`'s master): it is null again here.
        let nullable = spec
            .params
            .iter()
            .any(|p| p.name == a.name && matches!(p.kind, registry::ParamKind::IntOrNull { .. }));
        let v = match &a.value {
            ControlValue::Text { value } if nullable && value == registry::NULL_TEXT => Value::Null,
            ControlValue::Text { value } => Value::from(value.as_str()),
            ControlValue::Flag { value } => Value::from(*value),
            ControlValue::Int { value } => Value::from(*value),
        };
        if params.insert(a.name.clone(), v).is_some() {
            return Err(model(format!("{id}: `{}` is given twice", a.name)));
        }
    }
    registry::validate_args(spec, &params).map_err(|e| model(e.message))
}

/// `rows` checked as the Co-Producer socket checks a plan: 1 to
/// [`MAX_PLAN`] commands, each validated by the registry (no unknown,
/// duplicate or missing argument, every value of its type) with its args
/// in registry order, then admitted by the gate (Ruling 6: previewable,
/// explicit targets, Save without a path, nothing replaced).
pub fn plan_of(rows: &[PlannedRow]) -> Result<Vec<Planned>, VizError> {
    if rows.is_empty() || rows.len() > MAX_PLAN {
        return Err(model(format!("A plan holds 1 to {MAX_PLAN} commands.")));
    }
    let mut plan = Vec::with_capacity(rows.len());
    for r in rows {
        plan.push(Planned {
            id: r.id.clone(),
            args: validated(&r.id, &r.args)?,
        });
    }
    let c = ControlCommand::CoProducerPreview { plan, window: None };
    gate::admit(&c).map_err(|e| model(e.message))?;
    match c {
        ControlCommand::CoProducerPreview { plan, .. } => Ok(plan),
        _ => Err(model("not a plan")),
    }
}

/// Ruling 7's canonical text of a validated plan.
pub fn canonical(plan: &[Planned]) -> String {
    gate::canonical(plan)
}

/// A fresh token: 32 lowercase hex characters from 16 bytes of
/// `/dev/urandom`. No fallback: when it cannot be read, the preview fails.
pub fn fresh_token() -> Result<String, VizError> {
    let mut b = [0u8; 16];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut b))
        .map_err(|e| VizError::Io {
            message: format!("The preview's token could not be made ({e})."),
        })?;
    Ok(b.iter().map(|x| format!("{x:02x}")).collect())
}

/// The session's latest preview (Ruling 7): one at a time, taken once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingPreview {
    /// Its token.
    pub token: String,
    /// The session's version when it was made.
    pub version: u64,
    /// The present step's serial when it was made.
    pub serial: u64,
    /// The plan's canonical text.
    pub canonical: String,
    /// Some item is the composer's work (plan 8b Ruling 11: a take then
    /// needs VizDAW's own approval).
    pub needs_yes: bool,
    /// What it lists, for `co_producer_pending`.
    pub items: Vec<ChangeItemRow>,
}

impl PendingPreview {
    /// The preview's row again.
    pub fn row(&self) -> PreviewRow {
        PreviewRow {
            token: self.token.clone(),
            needs_yes: self.needs_yes,
            items: self.items.clone(),
        }
    }
}

/// Which of VizDAW's own approvals a take has (plan 8b Ruling 11). A take
/// whose preview needs the composer's yes needs `Sheet` or `Standing`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum CoProducerApproval {
    /// None: only a take that needs no yes goes through.
    None,
    /// The composer pressed Allow on VizDAW's approval sheet.
    Sheet,
    /// The asking app has the composer's standing approval.
    Standing,
}

/// Ruling 11's refusal of a take with no approval.
pub const NEEDS_APPROVAL: &str = "VizDAW's own approval is needed for this change.";

/// Ruling 7's refusals.
pub const USED: &str = "This preview was already used or replaced.";
/// The song moved since the preview.
pub const CHANGED: &str = "The song changed since the preview; preview again.";
/// The plan differs from the previewed one.
pub const NOT_PREVIEWED: &str = "These are not the previewed commands.";

/// Whether `pending` (already taken out of the session) lets `token` and
/// `plan` through now, at `version` and `serial`.
pub fn check_take(
    pending: Option<PendingPreview>,
    token: &str,
    plan: &[PlannedRow],
    version: u64,
    serial: u64,
) -> Result<(), VizError> {
    check_pending(pending.as_ref(), token, plan, version, serial)
}

/// [`check_take`]'s checks on a preview still in the session (plan 8b
/// Ruling 11): nothing is taken.
pub fn check_pending(
    pending: Option<&PendingPreview>,
    token: &str,
    plan: &[PlannedRow],
    version: u64,
    serial: u64,
) -> Result<(), VizError> {
    let Some(p) = pending.filter(|p| p.token == token) else {
        return Err(model(USED));
    };
    if p.version != version || p.serial != serial {
        return Err(model(CHANGED));
    }
    match plan_of(plan) {
        Ok(plan) if canonical(&plan) == p.canonical => Ok(()),
        _ => Err(model(NOT_PREVIEWED)),
    }
}

const CO_PRODUCER: &str = "Co-Producer: ";

fn rows(items: Vec<ChangeItem>, composers_work: bool) -> impl Iterator<Item = ChangeItemRow> {
    items.into_iter().map(move |i| ChangeItemRow {
        thing: i.thing,
        change: i.change,
        composers_work,
    })
}

/// Items whose own flag says whether each is the composer's work (Ruling
/// 2): what was Co-Producer's, and loses nothing, needs no yes.
fn own_rows(items: Vec<ChangeItem>) -> impl Iterator<Item = ChangeItemRow> {
    items.into_iter().map(|i| ChangeItemRow {
        composers_work: !i.co_producers,
        thing: i.thing,
        change: i.change,
    })
}

fn id_arg(p: &Planned, name: &str) -> Result<u64, VizError> {
    match p.args.iter().find(|(n, _)| n == name) {
        Some((_, ArgValue::Int(i))) => {
            u64::try_from(*i).map_err(|_| model(format!("{}: `{name}` is out of range", p.id)))
        }
        _ => Err(model(format!("{}: name the track's `{name}`", p.id))),
    }
}

/// The core-model command the track menu's Rename or Delete builds, as
/// `Session::edit` does.
fn track_command(p: &Planned) -> Result<core_model::command::Command, VizError> {
    use crate::edit::{EditCommand, LaneEdit};
    let (seq, lane) = (id_arg(p, "seq")?, id_arg(p, "lane")?);
    let edit = match p.id.as_str() {
        "track.rename" => {
            let name = match p.args.iter().find(|(n, _)| n == "name") {
                Some((_, ArgValue::Text(t))) => t.clone(),
                _ => return Err(model("track.rename: give the track's new `name`")),
            };
            LaneEdit::Rename { seq, lane, name }
        }
        _ => LaneEdit::Remove { seq, lane },
    };
    crate::edit::to_command(&EditCommand::Lane { edit })
}

/// The core-model command the strip's Colour menu builds (plan 8b Ruling
/// 3): `none` clears the colour.
fn colour_command(p: &Planned) -> Result<core_model::command::Command, VizError> {
    use core_model::command::{Command, MixerCmd};
    use core_model::mixer::Swatch;
    let strip = match p.args.iter().find(|(n, _)| n == "strip") {
        Some((_, ArgValue::Int(i))) => {
            u64::try_from(*i).map_err(|_| model("mixer.set_colour: `strip` is out of range"))?
        }
        _ => return Err(model("mixer.set_colour: name the `strip`")),
    };
    let colour = match p.args.iter().find(|(n, _)| n == "colour") {
        Some((_, ArgValue::Text(t))) => match t.as_str() {
            "none" => None,
            "red" => Some(Swatch::Red),
            "orange" => Some(Swatch::Orange),
            "yellow" => Some(Swatch::Yellow),
            "green" => Some(Swatch::Green),
            "teal" => Some(Swatch::Teal),
            "blue" => Some(Swatch::Blue),
            "purple" => Some(Swatch::Purple),
            "pink" => Some(Swatch::Pink),
            other => {
                return Err(model(format!(
                    "mixer.set_colour: `{}` is not a colour",
                    diff::shown_name(other)
                )));
            }
        },
        _ => return Err(model("mixer.set_colour: give the `colour`")),
    };
    Ok(Command::Mixer(MixerCmd::SetColour {
        strip: core_model::ids::StripId(strip),
        colour,
    }))
}

/// The release-fixes changes Co-Producer previews (plan 8e Task 6b).
const FIXES: &[&str] = &[
    "region.set_fade_shape",
    "destination.set_sound",
    "lane.set_output",
];

/// The app's refusal of a fade in on a crossfaded start (release-fixes
/// Decision 9).
const CROSSFADED_START: &str =
    "This region's start is a crossfade: set its shape with region.crossfade.";

fn text_arg<'a>(p: &'a Planned, name: &str) -> Option<&'a str> {
    match p.args.iter().find(|(n, _)| n == name) {
        Some((_, ArgValue::Text(t))) => Some(t),
        _ => None,
    }
}

/// The core-model command the app's handler for one of [`FIXES`] builds
/// on `doc`, refused with the handler's words when the handler would
/// refuse it; `None` when the handler changes nothing (a fade already of
/// that shape).
fn fixes_command(
    doc: &core_model::document::Document,
    p: &Planned,
) -> Result<Option<core_model::command::Command>, VizError> {
    use core_model::command::{AudioCmd, Command, LaneCmd};
    use core_model::event::{EventBody, Fade, FadeShape};
    use core_model::ids::{DestId, EventId, LaneId, OutputPathId, SeqId};
    let id = p.id.as_str();
    match id {
        "region.set_fade_shape" => {
            let (seq, lane) = (id_arg(p, "seq")?, id_arg(p, "lane")?);
            let event = EventId(id_arg(p, "event")?);
            let l = doc
                .lane(SeqId(seq), LaneId(lane))
                .map_err(|_| model(format!("{id}: `lane` names no track of that sequence")))?;
            let ev = l
                .active()
                .and_then(|t| t.events.values().find(|e| e.id == event))
                .ok_or_else(|| {
                    model(format!(
                        "{id}: `event` names no event on that track's active take"
                    ))
                })?;
            let EventBody::AudioRegion {
                fade_in,
                fade_out,
                crossfade,
                ..
            } = &ev.body
            else {
                return Err(model(format!("{id}: `event` must be an audio region")));
            };
            let shape = match text_arg(p, "shape") {
                Some("linear") => FadeShape::Linear,
                Some("equal_power") => FadeShape::EqualPower,
                Some("exponential") => FadeShape::Exponential,
                Some("s_curve") => FadeShape::SCurve,
                _ => return Err(model(format!("{id}: give the fade's `shape`"))),
            };
            let fade_in_edge = match text_arg(p, "fade") {
                Some("in") => true,
                Some("out") => false,
                _ => return Err(model(format!("{id}: give the `fade`, in or out"))),
            };
            if fade_in_edge && *crossfade {
                return Err(model(CROSSFADED_START));
            }
            let with = |f: &Fade| Fade { shape, ..*f };
            let (new_in, new_out) = if fade_in_edge {
                (with(fade_in), *fade_out)
            } else {
                (*fade_in, with(fade_out))
            };
            if (new_in, new_out) == (*fade_in, *fade_out) {
                return Ok(None);
            }
            Ok(Some(Command::Audio(AudioCmd::SetFades {
                event,
                fade_in: new_in,
                fade_out: new_out,
            })))
        }
        "destination.set_sound" => {
            let dest = DestId(id_arg(p, "dest")?);
            let sound = text_arg(p, "sound")
                .and_then(crate::builtin_sound::BuiltinSound::named)
                .ok_or_else(|| model(format!("{id}: give the `sound`")))?;
            crate::builtin_sound::sound_command(doc, dest, sound).map(Some)
        }
        "lane.set_output" => {
            let (seq, lane) = (id_arg(p, "seq")?, id_arg(p, "lane")?);
            let kind = crate::tracks::track_kind(doc, SeqId(seq), LaneId(lane))?;
            if kind != crate::tracks::TrackKindRow::Audio {
                return Err(model("Only an audio track has an output."));
            }
            let output = match p.args.iter().find(|(n, _)| n == "output") {
                Some((_, ArgValue::Text(t))) if t == registry::NULL_TEXT => None,
                Some((_, ArgValue::Int(o))) => {
                    let o = u64::try_from(*o)
                        .map_err(|_| model(format!("{id}: `output` is out of range")))?;
                    if !doc.outputs().contains_key(&OutputPathId(o)) {
                        return Err(model(format!("There is no output {o}.")));
                    }
                    Some(OutputPathId(o))
                }
                _ => return Err(model(format!("{id}: give the `output`, or null"))),
            };
            Ok(Some(Command::Lane(LaneCmd::SetOutput {
                seq: SeqId(seq),
                lane: LaneId(lane),
                output,
            })))
        }
        other => Err(model(format!("Co-Producer cannot preview {other} yet."))),
    }
}

/// Mixer items by plan 8d Ruling 2: the strip an item names decides. It
/// is the composer's work unless that strip was Co-Producer's before the
/// change, or is new (an item "added": an acting step made it). So a send
/// or insert Co-Producer itself put on a composer's strip still asks. With
/// `feeds_theirs` (the change alters what feeds a bus of the composer's)
/// every item but a new strip's is.
fn mixer_rows(items: Vec<ChangeItem>, feeds_theirs: bool) -> impl Iterator<Item = ChangeItemRow> {
    items.into_iter().map(move |i| ChangeItemRow {
        composers_work: i.change != "added" && (feeds_theirs || !i.co_producers),
        thing: i.thing,
        change: i.change,
    })
}

/// The strips whose solo changed from `before` to `after`, as the list
/// names them ("Strip <name>", by the name before), with the solo now.
/// Controller ruling (plan 8d Task 4 review): every solo change asks, on
/// any strip, on or off, since a solo silences every strip outside its
/// reach, the composer's tracks included.
fn solo_changes(
    before: &core_model::document::Document,
    after: &core_model::document::Document,
) -> Vec<(String, bool)> {
    let now = after.mixer().strips();
    before
        .mixer()
        .order()
        .iter()
        .chain(std::iter::once(&before.mixer().master()))
        .filter_map(|id| {
            let (b, a) = (before.mixer().strip(*id).ok()?, now.get(id)?);
            (b.solo != a.solo).then(|| (format!("Strip {}", diff::shown_name(&b.name)), a.solo))
        })
        .collect()
}

/// The running plug-ins' parameter names and values, as `describe_with`
/// reads them.
fn param_lookup(
    host: &dyn crate::mixer_chores::PluginLookup,
) -> impl Fn(core_model::ids::PluginInstanceId, core_model::mixer::ParamId) -> Option<(String, f64)> + '_
{
    move |slot, param| {
        host.params(slot)?
            .into_iter()
            .find(|r| r.id == param.0)
            .map(|r| (r.name, r.value))
    }
}

/// What `plan` would change (Ruling 6), worked out in order on a clone of
/// `history` with Co-Producer acting, as the apply will run it. `package`
/// is the song's file, if saved; `host` resolves plug-ins and reads the
/// running ones' parameters (plan 8d), changing nothing. Nothing outside
/// the clone changes.
pub fn preview_items(
    history: &History,
    package: Option<&Path>,
    plan: &[Planned],
    host: &dyn crate::mixer_chores::PluginLookup,
) -> Result<Vec<ChangeItemRow>, VizError> {
    let mut h = history.clone();
    if let Some(why) = h.acting_refusal() {
        return Err(VizError::Busy {
            message: why.to_string(),
        });
    }
    h.set_acting(true)?;
    let mut out = Vec::new();
    let filter = ViewFilter::default();
    for p in plan {
        match p.id.as_str() {
            "track.rename" | "track.delete" | "mixer.set_colour" => {
                let cmd = if p.id == "mixer.set_colour" {
                    colour_command(p)?
                } else {
                    track_command(p)?
                };
                let before = h.doc().clone();
                h.execute(&cmd, &filter)?;
                out.extend(own_rows(diff::describe(&before, h.doc())));
            }
            id if crate::chores::PREVIEWABLE_CHORES.contains(&id) => {
                // Ruling 3: every note or marker change is the composer's
                // work, even on Co-Producer's own track.
                let cmd = crate::chores::command_for(h.doc(), id, &p.args)?;
                let before = h.doc().clone();
                h.execute(&cmd, &filter)?;
                out.extend(rows(diff::describe(&before, h.doc()), true));
            }
            id if crate::mixer_chores::builds(id) => {
                // Plan 8d Ruling 1: new strips get the EQ the apply gives
                // them, so the clone is armed first.
                crate::bundled::arm(&mut h);
                let cmds = crate::mixer_chores::commands_for(h.doc(), id, &p.args, host)?;
                let before = h.doc().clone();
                for cmd in &cmds {
                    h.execute(cmd, &filter)?;
                }
                let lookup = param_lookup(host);
                let theirs = crate::mixer_chores::feeds_composers_bus(&before, h.doc());
                let soloed = solo_changes(&before, h.doc());
                out.extend(
                    mixer_rows(diff::describe_with(&before, h.doc(), &lookup), theirs).map(
                        |mut r| {
                            // A solo change's strip item asks, whoever's
                            // strip it is.
                            if soloed.iter().any(|(thing, _)| *thing == r.thing) {
                                r.composers_work = true;
                            }
                            r
                        },
                    ),
                );
                out.extend(soloed.into_iter().map(|(thing, on)| {
                    ChangeItemRow {
                        thing,
                        change: if on {
                            "soloed: other tracks are silent while it's on"
                        } else {
                            "unsoloed: other tracks play again"
                        }
                        .into(),
                        composers_work: true,
                    }
                }));
            }
            id if registry::CREATIVE_IDS.contains(&id) => {
                // Plan 8e Ruling 4: the target's strip decides, by 8d
                // Ruling 2; a new automation track is "added". A write
                // over points of a track not Co-Producer's asks (the
                // strip item says so).
                let cmds = crate::creative::commands_for(h.doc(), id, &p.args, host)?;
                let before = h.doc().clone();
                for cmd in &cmds {
                    h.execute(cmd, &filter)?;
                }
                let lookup = param_lookup(host);
                // A solo target asks on any strip, as every solo change
                // does (plan 8d): a solo silences every strip outside its
                // reach, the composer's tracks included.
                let soloed: Vec<String> = cmds
                    .iter()
                    .filter_map(|c| match c {
                        core_model::command::Command::WriteAutomation(w) => match w.target {
                            core_model::automation::AutomationTarget::StripSolo(strip) => before
                                .mixer()
                                .strip(strip)
                                .ok()
                                .map(|s| format!("Strip {}", diff::shown_name(&s.name))),
                            _ => None,
                        },
                        _ => None,
                    })
                    .collect();
                out.extend(
                    mixer_rows(diff::describe_with(&before, h.doc(), &lookup), false).map(
                        |mut r| {
                            if soloed.contains(&r.thing) {
                                r.composers_work = true;
                            }
                            r
                        },
                    ),
                );
                out.extend(soloed.into_iter().map(|thing| ChangeItemRow {
                    thing,
                    change: "soloed: other tracks are silent while it's on".into(),
                    composers_work: true,
                }));
            }
            id if FIXES.contains(&id) => {
                // Plan 8e Task 6b: a fade shape or an output is its
                // track's item, so the track decides (8b Ruling 1); the
                // Built-in Synth's sound is every track's that plays it,
                // so its item always asks.
                let Some(cmd) = fixes_command(h.doc(), p)? else {
                    continue;
                };
                let before = h.doc().clone();
                h.execute(&cmd, &filter)?;
                out.extend(own_rows(diff::describe(&before, h.doc())));
            }
            "edit.undo" | "edit.redo" => {
                let undo = p.id == "edit.undo";
                // An open request group closes first: its label is the one
                // the step gets.
                let label = if undo { h.undo_label() } else { h.redo_label() };
                let Some(label) = label.map(str::to_string) else {
                    return Err(model(if undo {
                        "There is nothing to undo."
                    } else {
                        "There is nothing to redo."
                    }));
                };
                let before = h.doc().clone();
                if undo {
                    h.undo()?;
                } else {
                    h.redo()?;
                }
                let items = diff::describe(&before, h.doc());
                // Ruling 7, P4: undoing Co-Producer's step only restores
                // the composer's song; redoing one needs no yes only when
                // all it does is add.
                let ours = label.starts_with(CO_PRODUCER)
                    && (undo || items.iter().all(|i| i.change == "added"));
                out.push(ChangeItemRow {
                    thing: if undo { "Undo" } else { "Redo" }.to_string(),
                    change: format!("\"{}\"", diff::shown_name(&label)),
                    composers_work: !ours,
                });
                out.extend(rows(items, !ours));
            }
            "file.save" => {
                let name = package.and_then(Path::file_name).ok_or_else(|| {
                    model("The song has no file yet: ask the composer to save it (File › Save).")
                })?;
                out.push(ChangeItemRow {
                    thing: diff::shown_name(&name.to_string_lossy()),
                    change: "saved over the file on disk".to_string(),
                    composers_work: true,
                });
            }
            other => {
                return Err(model(format!("Co-Producer cannot preview {other} yet.")));
            }
        }
    }
    Ok(out)
}
