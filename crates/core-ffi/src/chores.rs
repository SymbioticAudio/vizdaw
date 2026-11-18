//! Co-Producer's note chores and markers (plan 8c Task 2; Rulings 2 and
//! 4): the core-model command each of [`CHORE_IDS`] builds from its
//! registry arguments. The preview (`co_producer::preview_items`) and the
//! apply (`Session::run_chore`) both build through [`command_for`], so
//! what the composer says yes to is what runs. Every argument is hostile:
//! each is read by name and checked again here.
//!
//! Also the one-call export's plan (plan 8c Task 3; Ruling 1,
//! [`export_plan`]): the menu's own command into a new dated folder.

use crate::control::{ControlArg, ControlValue};
use crate::error::VizError;
use core_control::co_producer::{Output, check_output, id_list, output};
use core_control::registry::{ArgValue, GRIDS};
use core_control::request::ExportWhat;
use core_model::command::{Command, SequenceCmd};
use core_model::document::Document;
use core_model::ids::{LaneId, SeqId, StripId};
use core_model::modify::{Modify, Quantize, Transpose, ValueOp};
use core_model::sequence::Marker;
use core_model::time::Tick;
use std::path::Path;

/// The six chores: three note chores, then the marker commands.
pub const CHORE_IDS: &[&str] = &[
    "modify.quantize_notes",
    "modify.transpose_notes",
    "modify.velocity_notes",
    "marker.add",
    "marker.rename",
    "marker.remove",
];

/// The chores a preview can list (all but `marker.add`, which only adds).
pub const PREVIEWABLE_CHORES: &[&str] = &[
    "modify.quantize_notes",
    "modify.transpose_notes",
    "modify.velocity_notes",
    "marker.rename",
    "marker.remove",
];

fn bad(message: impl Into<String>) -> VizError {
    VizError::Model {
        message: message.into(),
    }
}

fn int(id: &str, args: &[(String, ArgValue)], name: &str) -> Result<Option<i64>, VizError> {
    match args.iter().find(|(n, _)| n == name) {
        None => Ok(None),
        Some((_, ArgValue::Int(i))) => Ok(Some(*i)),
        Some(_) => Err(bad(format!("{id}: `{name}` must be a whole number"))),
    }
}

fn need_int(id: &str, args: &[(String, ArgValue)], name: &str) -> Result<i64, VizError> {
    int(id, args, name)?.ok_or_else(|| bad(format!("{id}: give `{name}`")))
}

fn need_text<'a>(
    id: &str,
    args: &'a [(String, ArgValue)],
    name: &str,
) -> Result<&'a str, VizError> {
    match args.iter().find(|(n, _)| n == name) {
        Some((_, ArgValue::Text(t))) => Ok(t),
        _ => Err(bad(format!("{id}: give `{name}` as text"))),
    }
}

fn out_of_range(id: &str, name: &str) -> VizError {
    bad(format!("{id}: `{name}` is out of range"))
}

fn seq_of(id: &str, args: &[(String, ArgValue)]) -> Result<SeqId, VizError> {
    let seq = need_int(id, args, "seq")?;
    u64::try_from(seq)
        .map(SeqId)
        .map_err(|_| out_of_range(id, "seq"))
}

/// A note chore's tracks and its optional `[start, end)`.
type Target = (Vec<LaneId>, Option<(Tick, Tick)>);

/// The note target (Ruling 2): `lanes` and the optional `[start, end)`.
fn target(id: &str, args: &[(String, ArgValue)]) -> Result<Target, VizError> {
    let lanes = id_list(need_text(id, args, "lanes")?)
        .map_err(|m| bad(format!("{id}: `lanes`: {m}")))?
        .into_iter()
        .map(LaneId)
        .collect();
    let range = match (int(id, args, "start")?, int(id, args, "end")?) {
        (None, None) => None,
        (Some(start), Some(end)) if start >= 0 && end > start => Some((Tick(start), Tick(end))),
        (Some(_), Some(_)) => return Err(bad(format!("{id}: `end` must be after `start`"))),
        _ => {
            return Err(bad(format!(
                "{id}: give `start` and `end` together, or neither"
            )));
        }
    };
    Ok((lanes, range))
}

fn note_modify(id: &str, args: &[(String, ArgValue)]) -> Result<Modify, VizError> {
    match id {
        "modify.quantize_notes" => {
            let grid = need_text(id, args, "grid")?;
            let ticks = GRIDS
                .iter()
                .find(|(n, _)| *n == grid)
                .map(|(_, t)| *t)
                .ok_or_else(|| bad(format!("{id}: `grid` is not one of the grids")))?;
            let strength = int(id, args, "strength")?.unwrap_or(100);
            let strength = u8::try_from(strength)
                .ok()
                .filter(|s| (1..=100).contains(s))
                .ok_or_else(|| out_of_range(id, "strength"))?;
            Ok(Modify::Quantize(Quantize::Grid {
                grid: Tick(ticks),
                strength,
            }))
        }
        "modify.transpose_notes" => {
            let semitones = i8::try_from(need_int(id, args, "semitones")?)
                .ok()
                .filter(|s| (-48..=48).contains(s) && *s != 0)
                .ok_or_else(|| out_of_range(id, "semitones"))?;
            Ok(Modify::Transpose(Transpose::Interval { semitones }))
        }
        _ => {
            let amount = need_int(id, args, "amount")?;
            let op = match need_text(id, args, "op")? {
                "set" => u8::try_from(amount)
                    .ok()
                    .filter(|a| (1..=127).contains(a))
                    .map(|a| ValueOp::Set(u32::from(crate::input::upscale_velocity(a)))),
                "add" => ((-126..=126).contains(&amount) && amount != 0)
                    .then_some(ValueOp::Add(amount << 9)),
                "scale" => u32::try_from(amount)
                    .ok()
                    .filter(|a| (10..=400).contains(a))
                    .map(ValueOp::ScalePercent),
                _ => return Err(bad(format!("{id}: `op` is set, add or scale"))),
            };
            Ok(Modify::Velocity(
                op.ok_or_else(|| out_of_range(id, "amount"))?,
            ))
        }
    }
}

/// A 1-based marker `index` of `markers`, as a position: `Model`
/// "Sequence <seq> has N markers." past the end.
fn marker_at(
    id: &str,
    args: &[(String, ArgValue)],
    seq: SeqId,
    markers: &[Marker],
) -> Result<usize, VizError> {
    let index = need_int(id, args, "index")?;
    usize::try_from(index)
        .ok()
        .and_then(|i| i.checked_sub(1))
        .filter(|i| *i < markers.len())
        .ok_or_else(|| {
            let n = markers.len();
            bad(format!(
                "Sequence {} has {n} marker{}.",
                seq.0,
                if n == 1 { "" } else { "s" }
            ))
        })
}

/// The core-model command for one of [`CHORE_IDS`], built from `doc`
/// (markers need it). `Model` for an unknown ID, a missing or wrong
/// argument, a sequence `doc` lacks, a marker index past the end, or a
/// marker that is already there.
pub fn command_for(
    doc: &Document,
    id: &str,
    args: &[(String, ArgValue)],
) -> Result<Command, VizError> {
    if !CHORE_IDS.contains(&id) {
        return Err(bad(format!(
            "{} is not a note or marker chore.",
            core_model::diff::shown_name(id)
        )));
    }
    let seq = seq_of(id, args)?;
    if id.starts_with("modify.") {
        let (lanes, range) = target(id, args)?;
        return Ok(Command::ModifyNotes {
            seq,
            lanes,
            range,
            modify: note_modify(id, args)?,
        });
    }
    let s = doc
        .sequence(seq)
        .map_err(|_| bad(format!("There is no sequence {}.", seq.0)))?;
    let mut markers: Vec<Marker> = s.markers.iter().cloned().collect();
    match id {
        "marker.add" => {
            let tick = need_int(id, args, "tick")?;
            if tick < 0 {
                return Err(out_of_range(id, "tick"));
            }
            let name = need_text(id, args, "name")?.to_string();
            let tick = Tick(tick);
            if markers.iter().any(|m| m.tick == tick && m.name == name) {
                return Err(bad(format!(
                    "There is already a marker \"{}\" there.",
                    core_model::diff::shown_name(&name)
                )));
            }
            // Sorted by tick, a new one after those already at its tick.
            let at = markers
                .iter()
                .position(|m| m.tick > tick)
                .unwrap_or(markers.len());
            markers.insert(at, Marker { tick, name });
        }
        "marker.rename" => {
            let i = marker_at(id, args, seq, &markers)?;
            let name = need_text(id, args, "name")?.to_string();
            if let Some(m) = markers.get_mut(i) {
                m.name = name;
            }
        }
        _ => {
            let i = marker_at(id, args, seq, &markers)?;
            markers.remove(i);
        }
    }
    Ok(Command::Sequence(SequenceCmd::SetMarkers {
        seq,
        markers: markers.into_iter().collect(),
    }))
}

/// The undo label a chore's step gets outside a request: the marker
/// commands' own (Ruling 4), else the modification's (Ruling 2).
pub fn label(id: &str, cmd: &Command) -> String {
    match id {
        "marker.add" => "Add Marker".into(),
        "marker.rename" => "Rename Marker".into(),
        "marker.remove" => "Remove Marker".into(),
        _ => cmd.name().into(),
    }
}

/// Stems with no strip that plays audio (Ruling 1).
pub const NOTHING_TO_BOUNCE: &str = "There is nothing to bounce: no strip plays audio.";

/// One export chore as the app runs it (plan 8c Ruling 1): the new, empty
/// dated folder, and the command the menu runs with its arguments.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ExportPlanRow {
    /// The new dated folder, made empty.
    pub folder: String,
    /// The registry command the menu runs.
    pub id: String,
    /// Its arguments, in registry order. Never `replace`, `dry_run` or
    /// `add_to_project`.
    pub args: Vec<ControlArg>,
}

fn text_arg(name: &str, value: String) -> ControlArg {
    ControlArg {
        name: name.into(),
        value: ControlValue::Text { value },
    }
}

fn shown_path(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

/// Ruling 1's table: the command and its arguments for `what` into
/// `folder`, with `song` the cleaned song name.
fn export_command(
    what: ExportWhat,
    folder: &Path,
    song: &str,
    strips: Option<String>,
) -> Result<(&'static str, Vec<ControlArg>), VizError> {
    let file = |ext: &str| text_arg("path", shown_path(&folder.join(format!("{song}.{ext}"))));
    let into = || text_arg("path", shown_path(folder));
    Ok(match what {
        ExportWhat::Stems => {
            let mut args = vec![text_arg("source", "stems".into())];
            args.extend(strips.map(|s| text_arg("strips", s)));
            args.push(into());
            ("file.bounce", args)
        }
        ExportWhat::Mix => (
            "file.bounce",
            vec![text_arg("source", "mix".into()), file("wav")],
        ),
        ExportWhat::Midi => ("file.export_midi", vec![file("mid")]),
        ExportWhat::Dawproject => ("file.export_dawproject", vec![file("dawproject")]),
        ExportWhat::Aaf => ("file.export_aaf", vec![into()]),
        ExportWhat::OtherDaw => {
            let mut args = vec![into()];
            args.extend(strips.map(|s| text_arg("strips", s)));
            ("file.export_for_daw", args)
        }
        ExportWhat::Selection => {
            return Err(bad(
                "Bounce a selection with export_folder and file_bounce.",
            ));
        }
    })
}

/// The strips the plan names, as `strips`' text: the given ones (each in
/// `doc`), else for stems For Another DAW's defaults for `seq`; none for
/// another DAW's export with none given, nor for any other `what`.
fn plan_strips(
    doc: &Document,
    seq: Option<SeqId>,
    what: ExportWhat,
    strips: Option<&[u64]>,
) -> Result<Option<String>, VizError> {
    let ids: Vec<u64> = match (what, strips) {
        (ExportWhat::Stems | ExportWhat::OtherDaw, Some(given)) => {
            if given.len() > core_control::co_producer::MAX_IDS {
                return Err(bad(format!(
                    "`strips`: give at most {} IDs",
                    core_control::co_producer::MAX_IDS
                )));
            }
            let joined: Vec<String> = given.iter().map(u64::to_string).collect();
            let ids = id_list(&joined.join(",")).map_err(|m| bad(format!("`strips`: {m}")))?;
            for id in &ids {
                if doc.mixer().strip(StripId(*id)).is_err() {
                    return Err(bad(format!("There is no strip {id}.")));
                }
            }
            ids
        }
        (_, Some(_)) => return Err(bad("strips is only for stems and other_daw.")),
        (ExportWhat::Stems, None) => {
            let seq = seq.ok_or_else(|| bad(NOTHING_TO_BOUNCE))?;
            let ids: Vec<u64> = core_io::for_daw::default_strips(doc, seq)
                .map_err(|e| bad(e.to_string()))?
                .into_iter()
                .map(|s| s.0)
                .collect();
            if ids.is_empty() {
                return Err(bad(NOTHING_TO_BOUNCE));
            }
            ids
        }
        _ => return Ok(None),
    };
    let text: Vec<String> = ids.iter().map(u64::to_string).collect();
    Ok(Some(text.join(",")))
}

/// `args` as the registry hands them back.
fn as_rows(args: Vec<(String, ArgValue)>) -> Vec<ControlArg> {
    args.into_iter()
        .map(|(name, v)| ControlArg {
            name,
            value: match v {
                ArgValue::Text(value) => ControlValue::Text { value },
                ArgValue::Flag(value) => ControlValue::Flag { value },
                ArgValue::Int(value) => ControlValue::Int { value },
            },
        })
        .collect()
}

fn refused(e: core_control::ControlError) -> VizError {
    bad(e.message)
}

/// The planned command's output path and its rule.
fn planned_output(id: &str, args: &[(String, ArgValue)]) -> Result<(String, Output), VizError> {
    let rule = output(id, args).ok_or_else(|| bad(format!("{id} writes no file.")))?;
    let path = args
        .iter()
        .find_map(|(n, v)| match (n.as_str(), v) {
            ("path", ArgValue::Text(t)) => Some(t.clone()),
            _ => None,
        })
        .ok_or_else(|| bad(format!("{id} has no path.")))?;
    Ok((path, rule))
}

/// Ruling 1: works out the whole export before it makes anything, so a
/// refusal leaves no folder behind. `parent` (found by 8b Ruling 6's
/// order) must pass the Co-Producer check for a folder to export into;
/// the strips are worked out (given, else the defaults for `seq`); the
/// menu's command is built for a folder "<song> — <what> — <date>" in
/// `parent` and validated by the registry. Only then is the folder made
/// (never replacing one), and the planned path checked again by
/// `check_output` now that it is there; a refusal then removes the new,
/// still empty folder. `Model` for every refusal.
pub(crate) fn export_plan(
    doc: &Document,
    seq: Option<SeqId>,
    what: ExportWhat,
    parent: &Path,
    song: &str,
    date: &str,
    strips: Option<&[u64]>,
) -> Result<ExportPlanRow, VizError> {
    if what == ExportWhat::Selection {
        return Err(bad(
            "Bounce a selection with export_folder and file_bounce.",
        ));
    }
    if !crate::notes::check_date(date) {
        return Err(bad("An export folder's date is \"YYYY-MM-DD\"."));
    }
    check_output(parent, Output::IntoFolder).map_err(refused)?;
    let strips = plan_strips(doc, seq, what, strips)?;
    let name = crate::notes::clean_song(song);
    let planned = parent.join(crate::notes::export_folder_name(song, what, date));
    let (id, args) = export_command(what, &planned, &name, strips.clone())?;
    let checked = crate::co_producer::validated(id, &args)?;
    planned_output(id, &checked)?;
    let folder = crate::notes::make_export_folder(parent, song, what, date)?;
    // The folder made may be numbered: the command names it.
    let (id, args) = export_command(what, &folder, &name, strips)?;
    let made = crate::co_producer::validated(id, &args).and_then(|checked| {
        let (path, rule) = planned_output(id, &checked)?;
        check_output(Path::new(&path), rule).map_err(refused)?;
        Ok(checked)
    });
    match made {
        Ok(checked) => Ok(ExportPlanRow {
            folder: shown_path(&folder),
            id: id.into(),
            args: as_rows(checked),
        }),
        Err(e) => {
            // Only an empty folder is removed: it is the one just made.
            let _ = std::fs::remove_dir(&folder);
            Err(e)
        }
    }
}
