//! Theirs: the returning file as a project of its own (plan 6e Ruling 1).

use crate::IoError;
use crate::dawproject::import::{DpImport, FROM_VIZDAW, Origins, SlotStates, apply_tagged};
use crate::dawproject::record::{
    EventAt, Identity, Projection, projection_with_identity, returned,
};
use crate::report::Report;
use core_model::command::{Command, LaneCmd, SequenceCmd, SettingsCmd};
use core_model::document::Document;
use core_model::error::ModelError;
use core_model::format::AudioFormat;
use core_model::history::History;
use core_model::ids::{AnyId, SeqId};
use core_model::selection::ViewFilter;
use std::sync::atomic::{AtomicBool, Ordering};

/// The scratch project's one transaction.
const LABEL: &str = "Import DAWproject";

/// The returning file as a project of its own.
pub struct Theirs {
    /// The scratch project the file was imported into.
    pub doc: Document,
    /// Its one sequence.
    pub seq: SeqId,
    /// Its projection, in the file's frame ([`returned`]).
    pub projection: Projection,
    /// Who each projection row is in the scratch project.
    pub identity: Identity,
    /// What each new object was in the project the file came from.
    pub origins: Origins,
    /// The new plug-in slots and the state each captured.
    pub states: SlotStates,
    /// The import's report, without [`FROM_VIZDAW`].
    pub report: Report,
    /// The mapped import, kept for `apply_tracks` (Task 6).
    pub imp: DpImport,
}

fn stop(cancel: &AtomicBool) -> Result<(), IoError> {
    if cancel.load(Ordering::Relaxed) {
        Err(IoError::Cancelled)
    } else {
        Ok(())
    }
}

fn refused(e: ModelError) -> IoError {
    IoError::Refused(e.to_string())
}

/// A new project at `format` with one sequence and no tracks, as a new
/// project starts before an import.
fn empty_project(format: AudioFormat) -> Result<(History, SeqId), ModelError> {
    let mut h = History::new(Document::new());
    let view = ViewFilter::default();
    if h.doc().settings().format != format {
        h.execute(
            &Command::Settings(SettingsCmd::SetFormat {
                format,
                swaps: Vec::new(),
            }),
            &view,
        )?;
    }
    let r = h.execute(
        &Command::Sequence(SequenceCmd::New {
            name: "Sequence 1".into(),
        }),
        &view,
    )?;
    let seq = r
        .created
        .iter()
        .find_map(|id| match id {
            AnyId::Seq(s) => Some(*s),
            _ => None,
        })
        .ok_or_else(|| ModelError::invalid("no sequence was created"))?;
    let lanes: Vec<_> = h.doc().sequence(seq)?.lanes.iter().map(|l| l.id).collect();
    for lane in lanes {
        h.execute(&Command::Lane(LaneCmd::Remove { seq, lane }), &view)?;
    }
    Ok((h, seq))
}

/// `ids` (one per point of `before`) for the points of `after`, a
/// subsequence of `before` ([`returned`] drops hold points).
fn kept_ids(before: &[(i64, f64)], ids: &[EventAt], after: &[(i64, f64)]) -> Vec<EventAt> {
    let mut out = Vec::with_capacity(after.len());
    let mut next = after.iter().peekable();
    for (p, id) in before.iter().zip(ids) {
        if next.peek().is_some_and(|q| *q == p) {
            out.push(*id);
            next.next();
        }
    }
    out
}

/// Applies `imp` (with `apply_tagged`, in one transaction) to a new empty
/// project at `format`, captures each of its states into it
/// (`History::capture_state`, as 6d's round trip does, so device state
/// hashes are `blob_id`s), then projects it (`projection_with_identity`,
/// then `returned(.., origin)`, `origin` the snapshot's). The report drops
/// `import::FROM_VIZDAW`. Looks at `cancel` before each stage.
pub fn theirs(
    imp: DpImport,
    format: AudioFormat,
    origin: i64,
    cancel: &AtomicBool,
) -> Result<Theirs, IoError> {
    stop(cancel)?;
    let (mut h, seq) = empty_project(format).map_err(refused)?;
    let mut report = Report::default();
    h.begin(LABEL).map_err(refused)?;
    // On a refusal the scratch project is dropped with its open
    // transaction.
    let (states, origins) = apply_tagged(&mut h, &imp, seq, &mut report).map_err(refused)?;
    h.commit().map_err(refused)?;
    stop(cancel)?;
    for (slot, bytes) in &states {
        h.capture_state(*slot, bytes).map_err(refused)?;
    }
    report.items.retain(|i| i.what != FROM_VIZDAW);
    stop(cancel)?;
    let doc = h.doc().clone();
    let (raw, mut identity) = projection_with_identity(&doc, seq)?;
    let projection = returned(&raw, origin);
    for ((before, after), ids) in raw
        .tracks
        .iter()
        .zip(&projection.tracks)
        .zip(&mut identity.tracks)
    {
        for ((b, a), at) in before
            .automation
            .iter()
            .zip(&after.automation)
            .zip(&mut ids.automation)
        {
            *at = kept_ids(&b.1, at, &a.1);
        }
    }
    stop(cancel)?;
    Ok(Theirs {
        doc,
        seq,
        projection,
        identity,
        origins,
        states,
        report,
        imp,
    })
}
