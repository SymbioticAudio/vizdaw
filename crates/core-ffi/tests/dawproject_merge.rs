//! The DAWproject merge from the app (plan 6e Task 8; spec §7.8; Rulings
//! 2, 12, 13 and 15; Review Focus 3): the merge job and its three steps,
//! the swap's check, the snapshots and the Conflict row. Each test saves
//! 6d's project (`common::dawproject`) with a notes track "Keys" on the
//! built-in synth, exports it, edits the returned file with `core-io`'s
//! schema reader and writer (as Task 4's harness does) and merges it back.
// `im` maps nest deeply; sending a `Session` to a thread needs more depth.
#![recursion_limit = "256"]

mod common;

use common::dawproject::*;
use core_engine::fixtures::Q;
use core_io::dawproject::merge::text::{BY_NAME, CONFLICT_SETTING, SEQUENCE_GONE, SNAPSHOT_CHOSEN};
use core_io::dawproject::record::SNAPSHOT_DAMAGED;
use core_io::dawproject::{
    DpClip, DpMarker, DpMetaData, DpNamed, DpProject, DpStructureItem, DpTimeline, DpTrack,
    read_metadata, read_project, write_metadata, write_project,
};
use core_io::limits::PackageLimits;
use core_io::report::Report;
use core_io::xml::Tree;
use core_io::zip::{EntryMethod, ZipReader, ZipWriter};
use core_model::command::{Command, LaneCmd};
use core_model::document::Document;
use core_model::event::{EventBody, EventSpec};
use core_model::lane::LaneInput;
use core_model::mixer::StripKind;
use core_model::time::Tick;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use vizdaw_ffi::dawproject::{
    CHANGED_SINCE, NOT_FROM_HERE, NOT_FROM_VIZDAW, RECORDING_APPLY, needs_snapshot,
};
use vizdaw_ffi::{
    DawprojectMergeRequestRow, EditCommand, JobKindRow, JobRow, JobStateRow, LaneEdit,
    MergeStepRow, RecordSpecRow, ReportKindRow, ReportRow, SequenceEdit, Session, SnapshotRow,
    VizError,
};

// ---------------------------------------------------------------- the fixture

/// The date every merge is made on.
const DATE: &str = "2026-11-13";

/// The hand-written fixtures of `core-io`.
const FIXTURES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../core-io/tests/data/dawproject"
);

/// 6d's project with "Keys" (the built-in synth; notes 60 at tick 0 and 64
/// at 2 beats, a beat long), armed on a MIDI input when `armed`.
fn merge_project_with(armed: bool) -> Project {
    project_with(|b, seq, _, _| {
        let synth = b.dest("Synth", "builtin:synth", 0).unwrap();
        let keys = b.lane(seq, "Keys", Some(synth)).unwrap();
        let note = |pitch: u8| EventBody::Note {
            pitch,
            velocity: 0x8000,
            release_velocity: 0,
            duration: Tick(Q),
            attribute: None,
            per_note: Default::default(),
        };
        b.events(
            seq,
            keys,
            vec![
                EventSpec::new(Tick(0), note(60)),
                EventSpec::new(Tick(2 * Q), note(64)),
            ],
        )
        .unwrap();
        if armed {
            b.run(Command::Lane(LaneCmd::SetInput {
                seq,
                lane: keys,
                input: Some(LaneInput::Midi {
                    port: Some("Keys".into()),
                    channel: None,
                }),
            }))
            .unwrap();
            b.run(Command::Lane(LaneCmd::SetFlags {
                seq,
                lane: keys,
                mute: None,
                solo: None,
                arm: Some(true),
            }))
            .unwrap();
        }
    })
}

fn merge_project() -> Project {
    merge_project_with(false)
}

/// Exports `p`'s sequence to `out/<name>.dawproject`.
fn exported_as(p: &Project, name: &str) -> PathBuf {
    let dest = p.out.join(format!("{name}.dawproject"));
    export(&p.s, req(p.seq, &dest));
    dest
}

fn exported(p: &Project) -> PathBuf {
    exported_as(p, "x")
}

fn mreq(path: &Path, step: MergeStepRow) -> DawprojectMergeRequestRow {
    DawprojectMergeRequestRow {
        path: text(path),
        snapshot: None,
        step,
        date: DATE.into(),
    }
}

/// Starts the merge `r` and waits for its job to end.
fn merge(s: &Session, r: DawprojectMergeRequestRow) -> JobRow {
    let j = wait_job(s, s.merge_dawproject(r).unwrap());
    assert_eq!(j.kind, JobKindRow::MergeDawproject);
    j
}

fn doc_of(s: &Session) -> Document {
    core_model::file::from_json(&s.debug_document_json()).unwrap()
}

fn items(r: &ReportRow, kind: ReportKindRow, what: &str) -> u64 {
    r.items
        .iter()
        .filter(|i| i.kind == kind && i.what == what)
        .map(|i| i.count)
        .sum()
}

fn failed_with(j: &JobRow, message: &str) {
    assert_eq!(j.state, JobStateRow::Failed, "{j:?}");
    assert_eq!(j.message.as_deref(), Some(message), "{j:?}");
}

/// The files and folders under the project's `Media/` (hidden ones too),
/// once the overview worker is idle.
fn media_listing(p: &Project) -> Vec<String> {
    idle_until(&p.s, |s| s.debug_overviews_idle());
    without_empty_folders(listing(&p.pkg.join("Media")))
}

/// `all` without the folders that hold nothing: the shared folders
/// (`.incoming`, `.overviews`) stay, emptied, until close (review I2).
fn without_empty_folders(all: Vec<String>) -> Vec<String> {
    all.iter()
        .filter(|f| {
            !f.ends_with('/')
                || all
                    .iter()
                    .any(|g| g.len() > f.len() && g.starts_with(f.as_str()))
        })
        .cloned()
        .collect()
}

// ---------------------------------------------------------------- the returned file

/// What [`returned`] takes out of the file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Strip {
    /// Nothing: the file as VizDAW wrote it.
    Nothing,
    /// The record and the metadata's export line: the file lost its export
    /// ID (its tags stay, so it still came from VizDAW).
    ExportId,
}

fn parse(bytes: &[u8]) -> Tree {
    Tree::parse(bytes, &PackageLimits::default(), &AtomicBool::new(false)).unwrap()
}

/// `meta` without the export line its comment ends with.
fn without_export_line(meta: &mut DpMetaData) {
    use core_io::dawproject::units::EXPORT_LINE;
    if let Some(c) = meta.fields.last_mut() {
        if let Some(text) = c.take() {
            let kept: Vec<&str> = text
                .split('\n')
                .filter(|l| !l.starts_with(EXPORT_LINE))
                .collect();
            let kept = kept.join("\n");
            *c = (!kept.is_empty()).then_some(kept);
        }
    }
}

static RETURNS: AtomicUsize = AtomicUsize::new(0);

/// The file `src` as the other DAW sends it back: its project and metadata
/// read with `core-io`'s schema reader, edited by `edit`, written again;
/// `strip` takes out what it says; `entries` replaces or adds entries.
fn returned(
    src: &Path,
    strip: Strip,
    entries: &[(&str, Vec<u8>)],
    edit: impl FnOnce(&mut DpProject, &mut DpMetaData),
) -> PathBuf {
    let z = ZipReader::open(src, &PackageLimits::default()).unwrap();
    let read = |name: &str| {
        let e = z.find(name).unwrap_or_else(|| panic!("no {name}"));
        z.read(e, u64::MAX, None, &AtomicBool::new(false)).unwrap()
    };
    let mut report = Report::default();
    let mut project = read_project(&parse(&read("project.xml")), &mut report).unwrap();
    let mut meta = read_metadata(&parse(&read("metadata.xml")), &mut report);
    edit(&mut project, &mut meta);
    if strip == Strip::ExportId {
        without_export_line(&mut meta);
    }
    let n = RETURNS.fetch_add(1, Ordering::Relaxed);
    let path = src.with_file_name(format!("Return {n}.dawproject"));
    let mut w = ZipWriter::new(std::fs::File::create(&path).unwrap());
    let mut names: Vec<String> = z.entries().iter().map(|e| e.name.clone()).collect();
    for (name, _) in entries {
        if !names.iter().any(|n| n == name) {
            names.push((*name).to_owned());
        }
    }
    for name in names {
        let bytes = if let Some((_, b)) = entries.iter().find(|(n, _)| *n == name) {
            b.clone()
        } else if name == "project.xml" {
            write_project(&project).into_bytes()
        } else if name == "metadata.xml" {
            write_metadata(&meta).into_bytes()
        } else if name == "vizdaw/record.json" && strip == Strip::ExportId {
            continue;
        } else {
            read(&name)
        };
        let method = if name.ends_with(".xml") || name.ends_with(".json") {
            EntryMethod::Deflated
        } else {
            EntryMethod::Stored
        };
        w.add(&name, method, &bytes).unwrap();
    }
    w.finish().unwrap();
    path
}

/// The track named `name` in the file's structure.
fn track<'a>(p: &'a mut DpProject, name: &str) -> &'a mut DpTrack {
    fn find<'a>(t: &'a mut DpTrack, name: &str) -> Option<&'a mut DpTrack> {
        if t.named.name.as_deref() == Some(name) {
            return Some(t);
        }
        t.children.iter_mut().find_map(|c| find(c, name))
    }
    p.structure
        .iter_mut()
        .find_map(|i| match i {
            DpStructureItem::Track(t) => find(t, name),
            DpStructureItem::Channel(_) => None,
        })
        .unwrap_or_else(|| panic!("no track {name}"))
}

/// The clips of the track whose XML ID is `id`.
fn clips<'a>(p: &'a mut DpProject, id: &str) -> &'a mut Vec<DpClip> {
    let lanes = p.arrangement.as_mut().unwrap().lanes.as_mut().unwrap();
    for item in &mut lanes.items {
        if let DpTimeline::Lanes(l) = item
            && l.head.track.as_deref() == Some(id)
        {
            for inner in &mut l.items {
                if let DpTimeline::Clips(c) = inner {
                    return &mut c.clips;
                }
            }
        }
    }
    panic!("no clips on {id}")
}

/// The XML ID of the track `name`.
fn track_id(p: &mut DpProject, name: &str) -> String {
    track(p, name).named.id.clone().unwrap()
}

/// Theirs' edits: Keys' first note moves to half a beat, Keys' strip goes
/// to half its level (−6 dB) and a marker "Bridge" is added at beat 4.
fn three_edits(p: &mut DpProject, _: &mut DpMetaData) {
    let keys = track_id(p, "Keys");
    let clip = clips(p, &keys).first_mut().unwrap();
    let Some(DpTimeline::Notes(n)) = clip.content.as_deref_mut() else {
        panic!("Keys' clip holds notes")
    };
    n.notes[0].time = 0.5;
    half_level(p, "Keys");
    add_marker(p);
}

fn half_level(p: &mut DpProject, name: &str) {
    let ch = track(p, name).channel.as_mut().unwrap();
    ch.volume.as_mut().unwrap().value = Some(0.5);
}

fn add_marker(p: &mut DpProject) {
    p.arrangement.as_mut().unwrap().markers.push(DpMarker {
        named: DpNamed {
            name: Some("Bridge".into()),
            ..DpNamed::default()
        },
        time: 4.0,
    });
}

/// A mono 16-bit WAV of `frames` at 48 kHz, unlike the fixture's audio
/// (macOS: converting it needs the system importer).
#[cfg(target_os = "macos")]
fn new_wav(frames: u32) -> Vec<u8> {
    let data = frames * 2;
    let mut b = Vec::new();
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&48_000u32.to_le_bytes());
    b.extend_from_slice(&96_000u32.to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data.to_le_bytes());
    for f in 0..frames {
        let v = ((f * 53) % 2000) as i16 - 1000;
        b.extend_from_slice(&v.to_le_bytes());
    }
    b
}

/// The new audio entry [`add_region`] plays.
#[cfg(target_os = "macos")]
const NEW_AUDIO: &str = "audio/new.wav";

/// Theirs adds a region on "Mono" at beat 4 playing [`NEW_AUDIO`].
#[cfg(target_os = "macos")]
fn add_region(p: &mut DpProject, _: &mut DpMetaData) {
    let mono = track_id(p, "Mono");
    let cs = clips(p, &mono);
    let mut clip = cs[0].clone();
    clip.named.comment = None;
    clip.time = 4.0;
    let Some(DpTimeline::Audio(a)) = clip.content.as_deref_mut() else {
        panic!("Mono's clip holds audio")
    };
    a.file.path = NEW_AUDIO.into();
    a.head.named.name = Some("new".into());
    cs.push(clip);
}

/// The returned file with [`add_region`] and its audio.
#[cfg(target_os = "macos")]
fn with_new_audio(src: &Path) -> PathBuf {
    returned(
        src,
        Strip::Nothing,
        &[(NEW_AUDIO, new_wav(4_800))],
        add_region,
    )
}

// ---------------------------------------------------------------- what a merge did

fn keys_lane(d: &Document, seq: u64) -> core_model::ids::LaneId {
    let s = d.sequence(core_model::ids::SeqId(seq)).unwrap();
    s.lanes.iter().find(|l| l.name == "Keys").unwrap().id
}

fn keys_strip(d: &Document) -> core_model::ids::StripId {
    d.mixer()
        .order()
        .iter()
        .copied()
        .find(|s| {
            d.mixer()
                .strip(*s)
                .is_ok_and(|x| matches!(x.kind, StripKind::BuiltinSynth))
        })
        .unwrap()
}

/// Keys' note starts, its strip's level and the marker names.
fn edits_of(d: &Document, seq: u64) -> (Vec<i64>, f64, Vec<(i64, String)>) {
    let s = d.sequence(core_model::ids::SeqId(seq)).unwrap();
    let lane = s.lanes.iter().find(|l| l.name == "Keys").unwrap();
    let take = lane.active().unwrap();
    let mut starts: Vec<i64> = take
        .events
        .values()
        .filter(|e| matches!(e.body, EventBody::Note { .. }))
        .map(|e| e.tick.0)
        .collect();
    starts.sort();
    let gain = d.mixer().strip(keys_strip(d)).unwrap().gain_db;
    let markers = s
        .markers
        .iter()
        .map(|m| (m.tick.0, m.name.clone()))
        .collect();
    (starts, gain, markers)
}

/// The document has theirs' three edits ([`three_edits`]).
fn assert_three_edits(d: &Document, seq: u64) {
    let (starts, gain, markers) = edits_of(d, seq);
    assert_eq!(starts, [Q / 2, 2 * Q]);
    assert!((gain - 20.0 * 0.5f64.log10()).abs() < 0.01, "{gain}");
    assert_eq!(markers, [(4 * Q, "Bridge".to_owned())]);
}

// ---------------------------------------------------------------- the steps

/// Ruling 12, spec §7.8: Apply changes the project at once, in one undo
/// step named "Merge DAWproject"; one undo restores it.
#[test]
fn merge_applies_theirs_in_one_step() {
    let _g = serial();
    let p = merge_project();
    let file = returned(&exported(&p), Strip::Nothing, &[], three_edits);
    let before = p.s.debug_document_json();
    let j = merge(&p.s, mreq(&file, MergeStepRow::Apply));
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
    let report = j.report.clone().expect("a report");
    assert_eq!(
        report.summary,
        "Merged: 2 applied, 0 kept, 1 added, 0 removed, 0 in conflict"
    );
    assert_eq!(p.s.interchange_report().unwrap(), Some(report));
    assert_eq!(j.merge.as_ref().map(|m| m.added), Some(1));
    assert_three_edits(&doc_of(&p.s), p.seq);
    assert_eq!(
        p.s.snapshot().unwrap().undo_label.as_deref(),
        Some("Merge DAWproject")
    );
    p.s.undo().unwrap();
    assert_eq!(p.s.debug_document_json(), before);
}

/// Ruling 12: Preview reports the counts and changes nothing.
#[test]
fn preview_changes_nothing() {
    let _g = serial();
    let p = merge_project();
    let file = returned(&exported(&p), Strip::Nothing, &[], three_edits);
    let doc = p.s.debug_document_json();
    let label = p.s.snapshot().unwrap().undo_label;
    let media = media_listing(&p);
    let j = merge(&p.s, mreq(&file, MergeStepRow::Preview));
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
    let m = j.merge.clone().expect("the preview");
    assert_eq!(
        (m.applied, m.kept, m.added, m.removed, m.conflicts),
        (2, 0, 1, 0, 0)
    );
    assert_eq!(
        m.summary,
        "Merged: 2 applied, 0 kept, 1 added, 0 removed, 0 in conflict"
    );
    assert!(m.conflict_lines.is_empty());
    let snaps = p.s.dawproject_snapshots().unwrap();
    assert_eq!(m.export_id, snaps[0].export_id);
    assert_eq!(m.exported_at, snaps[0].exported_at);
    assert_eq!(
        j.report.as_ref().map(|r| r.summary.clone()),
        Some(m.summary)
    );
    assert_eq!(p.s.debug_document_json(), doc);
    assert_eq!(p.s.snapshot().unwrap().undo_label, label);
    assert_eq!(media_listing(&p), media);
    // No hold.
    assert!(matches!(
        p.s.confirm_merge(j.id),
        Err(VizError::Model { .. })
    ));
}

/// Ruling 12: Hold changes nothing until `confirm_merge`, which applies as
/// Apply does.
#[test]
fn hold_then_confirm() {
    let _g = serial();
    let p = merge_project();
    let file = returned(&exported(&p), Strip::Nothing, &[], three_edits);
    let doc = p.s.debug_document_json();
    assert_eq!(p.s.save_as_refusal().unwrap(), None);
    let j = merge(&p.s, mreq(&file, MergeStepRow::Hold));
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
    assert!(j.merge.is_some());
    assert_eq!(p.s.debug_document_json(), doc, "nothing changed yet");
    // Task 10 fix round 1: the app asks before the Save panel.
    assert_eq!(
        p.s.save_as_refusal().unwrap().as_deref(),
        Some("Finish or cancel the merge waiting to be confirmed, then save as.")
    );
    let report = p.s.confirm_merge(j.id).unwrap();
    assert_eq!(p.s.save_as_refusal().unwrap(), None);
    assert_eq!(
        report.summary,
        "Merged: 2 applied, 0 kept, 1 added, 0 removed, 0 in conflict"
    );
    assert_eq!(p.s.interchange_report().unwrap(), Some(report));
    assert_three_edits(&doc_of(&p.s), p.seq);
    assert_eq!(
        p.s.snapshot().unwrap().undo_label.as_deref(),
        Some("Merge DAWproject")
    );
    // The hold is gone.
    assert!(matches!(
        p.s.confirm_merge(j.id),
        Err(VizError::Model { .. })
    ));
}

/// Review Focus 3: the held media goes on Cancel.
#[cfg(target_os = "macos")]
#[test]
fn hold_then_cancel_removes_its_media() {
    let _g = serial();
    let p = merge_project();
    let file = with_new_audio(&exported(&p));
    let before = media_listing(&p);
    let doc = p.s.debug_document_json();
    let j = merge(&p.s, mreq(&file, MergeStepRow::Hold));
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
    let held = media_listing(&p);
    assert!(
        held.iter()
            .any(|f| f.ends_with(".caf") && !before.contains(f)),
        "the new audio is in Media/: {held:?}"
    );
    p.s.cancel_merge(j.id).unwrap();
    assert_eq!(media_listing(&p), before);
    assert_eq!(p.s.debug_document_json(), doc);
    assert!(matches!(
        p.s.cancel_merge(j.id),
        Err(VizError::Model { .. })
    ));
}

/// Review Focus 3: an edit after the preview refuses the confirm, applies
/// nothing and drops the hold.
#[test]
fn confirm_after_an_edit_is_refused() {
    let _g = serial();
    let p = merge_project();
    let file = returned(&exported(&p), Strip::Nothing, &[], three_edits);
    let j = merge(&p.s, mreq(&file, MergeStepRow::Hold));
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
    let lane = p.s.lanes(p.seq).unwrap()[0].id;
    p.s.edit(
        EditCommand::Lane {
            edit: LaneEdit::Rename {
                seq: p.seq,
                lane,
                name: "Renamed".into(),
            },
        },
        None,
    )
    .unwrap();
    let doc = p.s.debug_document_json();
    match p.s.confirm_merge(j.id) {
        Err(VizError::Model { message }) => assert_eq!(message, CHANGED_SINCE),
        other => panic!("{other:?}"),
    }
    assert_eq!(p.s.debug_document_json(), doc, "nothing applied");
    assert!(matches!(
        p.s.confirm_merge(j.id),
        Err(VizError::Model { .. })
    ));
    assert!(matches!(
        p.s.cancel_merge(j.id),
        Err(VizError::Model { .. })
    ));
}

/// Ruling 12: a plug-in capture that changes the document with no undo
/// step (here an export check on the host's thread) also refuses.
#[test]
fn a_step_free_capture_also_refuses() {
    let _g = serial();
    let p = merge_project();
    let slot = p.s.plugin_slots().unwrap()[0].id;
    set_live_gain(&p, slot, 0.25);
    let file = returned(&exported(&p), Strip::Nothing, &[], three_edits);
    let j = merge(&p.s, mreq(&file, MergeStepRow::Hold));
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
    let label = p.s.snapshot().unwrap().undo_label;
    set_live_gain(&p, slot, 0.6);
    p.s.export_dawproject_check(req(p.seq, &p.out.join("c.dawproject")))
        .unwrap();
    assert_eq!(p.s.snapshot().unwrap().undo_label, label, "no undo step");
    match p.s.confirm_merge(j.id) {
        Err(VizError::Model { message }) => assert_eq!(message, CHANGED_SINCE),
        other => panic!("{other:?}"),
    }
}

/// Review Focus 3: Merge while recording or in an open edit is Busy and
/// keeps the hold.
#[test]
fn confirm_while_busy_keeps_the_hold() {
    let _g = serial();
    let p = merge_project_with(true);
    let file = returned(&exported(&p), Strip::Nothing, &[], three_edits);
    let j = merge(&p.s, mreq(&file, MergeStepRow::Hold));
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
    p.s.record(
        p.seq,
        RecordSpecRow {
            replace: true,
            punch: None,
            looped: false,
            count_in_bars: 0,
            pre_roll_bars: 0,
            passes_as_takes: true,
        },
    )
    .unwrap();
    let until = Instant::now() + common::wait_limit();
    while !p.s.record_state().unwrap().recording {
        assert!(Instant::now() < until, "recording did not start");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        matches!(p.s.confirm_merge(j.id), Err(VizError::Busy { .. })),
        "recording"
    );
    p.s.stop().unwrap();
    idle_until(&p.s, |s| !s.record_state().unwrap().recording);
    p.s.begin_edit("Drag".into()).unwrap();
    assert!(
        matches!(p.s.confirm_merge(j.id), Err(VizError::Busy { .. })),
        "an open edit"
    );
    p.s.end_edit().unwrap();
    let report = p.s.confirm_merge(j.id).unwrap();
    assert!(
        report.summary.starts_with("Merged: 2 applied"),
        "{report:?}"
    );
    assert_three_edits(&doc_of(&p.s), p.seq);
}

/// Review Focus 3: closing the session drops the hold and its media.
#[cfg(target_os = "macos")]
#[test]
fn closing_drops_a_held_merge() {
    let _g = serial();
    let p = merge_project();
    let file = with_new_audio(&exported(&p));
    let before = media_listing(&p);
    let j = merge(&p.s, mreq(&file, MergeStepRow::Hold));
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
    let held = media_listing(&p);
    assert!(held.len() > before.len(), "{held:?}");
    p.s.close().unwrap();
    let after = listing(&p.pkg.join("Media"));
    assert!(!p.pkg.join("Media/.incoming").exists(), "{after:?}");
    for f in &after {
        assert!(before.contains(f), "{f} is left: {after:?}");
    }
}

// ---------------------------------------------------------------- refusals

/// Ruling 13: each refusal fails the job with its one sentence.
#[test]
fn refusals_name_their_reason() {
    let _g = serial();
    let p = merge_project();
    let src = exported(&p);
    let export_id = p.s.dawproject_snapshots().unwrap()[0].export_id.clone();

    // Another project's file.
    let other = merge_project();
    let theirs = exported(&other);
    failed_with(
        &merge(&p.s, mreq(&theirs, MergeStepRow::Preview)),
        NOT_FROM_HERE,
    );

    // A hand file from another DAW.
    let project = std::fs::read(format!("{FIXTURES}/hand_other.project.xml")).unwrap();
    let metadata = std::fs::read(format!("{FIXTURES}/hand_other.metadata.xml")).unwrap();
    let hand = p.out.join("Hand.dawproject");
    let mut w = ZipWriter::new(std::fs::File::create(&hand).unwrap());
    w.add("project.xml", EntryMethod::Deflated, &project)
        .unwrap();
    w.add("metadata.xml", EntryMethod::Deflated, &metadata)
        .unwrap();
    w.finish().unwrap();
    failed_with(
        &merge(&p.s, mreq(&hand, MergeStepRow::Preview)),
        NOT_FROM_VIZDAW,
    );

    // The record and the export line stripped, no `snapshot`: the
    // project's one snapshot is named, with its date and track count.
    let lost = returned(&src, Strip::ExportId, &[], |_, _| {});
    let rows = p.s.dawproject_snapshots().unwrap();
    assert_eq!(rows.len(), 1);
    let sentence = needs_snapshot(&rows);
    let date = core_io::dawproject::merge::text::utc_date(rows[0].exported_at);
    assert!(sentence.contains(&export_id), "{sentence}");
    assert!(sentence.contains(&date), "{sentence}");
    assert!(sentence.contains("3 tracks"), "{sentence}");
    failed_with(&merge(&p.s, mreq(&lost, MergeStepRow::Preview)), &sentence);

    // A `snapshot` naming another export than the record's.
    let r = DawprojectMergeRequestRow {
        snapshot: Some("00000000-0000-4000-8000-000000000099".into()),
        ..mreq(&src, MergeStepRow::Preview)
    };
    failed_with(&merge(&p.s, r), NOT_FROM_HERE);

    // The snapshot truncated.
    let snap = p.pkg.join(format!("Interchange/{export_id}.json"));
    let bytes = std::fs::read(&snap).unwrap();
    std::fs::write(&snap, &bytes[..bytes.len() / 2]).unwrap();
    failed_with(
        &merge(&p.s, mreq(&src, MergeStepRow::Preview)),
        SNAPSHOT_DAMAGED,
    );

    // The snapshot deleted.
    std::fs::remove_file(&snap).unwrap();
    failed_with(
        &merge(&p.s, mreq(&src, MergeStepRow::Preview)),
        NOT_FROM_HERE,
    );
    std::fs::write(&snap, &bytes).unwrap();
    assert_eq!(
        merge(&p.s, mreq(&src, MergeStepRow::Preview)).state,
        JobStateRow::Done
    );

    // The exported sequence deleted here.
    p.s.edit(
        EditCommand::Sequence {
            edit: SequenceEdit::New {
                name: "Other".into(),
            },
        },
        None,
    )
    .unwrap();
    p.s.edit(
        EditCommand::Sequence {
            edit: SequenceEdit::Delete {
                seq: p.seq,
                delete_references: true,
            },
        },
        None,
    )
    .unwrap();
    failed_with(
        &merge(&p.s, mreq(&src, MergeStepRow::Preview)),
        SEQUENCE_GONE,
    );
}

/// Ruling 13: with the export ID lost, the chosen snapshot is the base.
#[test]
fn chosen_snapshot_merges_a_stripped_file() {
    let _g = serial();
    let p = merge_project();
    let src = exported(&p);
    let export_id = p.s.dawproject_snapshots().unwrap()[0].export_id.clone();
    let lost = returned(&src, Strip::ExportId, &[], three_edits);
    let j = merge(
        &p.s,
        DawprojectMergeRequestRow {
            snapshot: Some(export_id.clone()),
            ..mreq(&lost, MergeStepRow::Apply)
        },
    );
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
    let report = j.report.unwrap();
    assert_eq!(items(&report, ReportKindRow::Note, SNAPSHOT_CHOSEN), 1);
    assert_three_edits(&doc_of(&p.s), p.seq);

    // Its tags made to name nothing too: matched by name and order.
    p.s.undo().unwrap();
    let untagged = returned(&src, Strip::ExportId, &[], |p, m| {
        for i in &mut p.structure {
            if let DpStructureItem::Track(t) = i {
                if let Some(c) = &mut t.named.comment {
                    *c = c.replace("vz:t", "vz:t9");
                }
            }
        }
        three_edits(p, m);
    });
    let j = merge(
        &p.s,
        DawprojectMergeRequestRow {
            snapshot: Some(export_id),
            ..mreq(&untagged, MergeStepRow::Apply)
        },
    );
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
    let report = j.report.unwrap();
    assert_eq!(items(&report, ReportKindRow::Note, SNAPSHOT_CHOSEN), 1);
    assert_eq!(items(&report, ReportKindRow::Note, BY_NAME), 1);
    assert_three_edits(&doc_of(&p.s), p.seq);
}

/// An unsaved project's snapshot is in its scratch package.
#[test]
fn untitled_project_merges_from_its_temp_root() {
    let _g = serial();
    let mut b = core_engine::fixtures::Builder::new();
    let synth = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, keys) = b.sequence("Song", "Keys", Some(synth)).unwrap();
    b.events(
        seq,
        keys,
        vec![EventSpec::new(
            Tick(0),
            EventBody::Note {
                pitch: 60,
                velocity: 0x8000,
                release_velocity: 0,
                duration: Tick(Q),
                attribute: None,
                per_note: Default::default(),
            },
        )],
    )
    .unwrap();
    let s = Session::from_document(b.finish()).unwrap();
    let dir = tempfile::tempdir().unwrap();
    s.debug_set_trash_dir(Some(text(&dir.path().join("Trash"))));
    s.debug_free_bytes(None).unwrap();
    let dest = dir.path().join("u.dawproject");
    export(&s, req(seq.0, &dest));
    let file = returned(&dest, Strip::Nothing, &[], |p, _| {
        let keys = track_id(p, "Keys");
        let clip = clips(p, &keys).first_mut().unwrap();
        let Some(DpTimeline::Notes(n)) = clip.content.as_deref_mut() else {
            panic!("notes")
        };
        n.notes[0].time = 0.5;
    });
    let j = merge(&s, mreq(&file, MergeStepRow::Apply));
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
    let d = doc_of(&s);
    let lane = keys_lane(&d, seq.0);
    let l = d.sequence(seq).unwrap().lane(lane).unwrap();
    let ticks: Vec<i64> = l
        .active()
        .unwrap()
        .events
        .values()
        .map(|e| e.tick.0)
        .collect();
    assert_eq!(ticks, [Q / 2]);
    s.close().unwrap();
}

// ---------------------------------------------------------------- snapshots

/// Ruling 13: the project's snapshots, newest first, with track counts.
#[test]
fn snapshots_list_newest_first() {
    let _g = serial();
    let p = merge_project();
    assert_eq!(
        p.s.dawproject_snapshots().unwrap(),
        Vec::<SnapshotRow>::new()
    );
    let add_track = || {
        p.s.edit(
            EditCommand::Lane {
                edit: LaneEdit::Add {
                    seq: p.seq,
                    name: "More".into(),
                    index: None,
                },
            },
            None,
        )
        .unwrap();
    };
    let mut ids = Vec::new();
    for k in 0..3 {
        if k > 0 {
            add_track();
        }
        exported_as(&p, &format!("e{k}"));
        let rows = p.s.dawproject_snapshots().unwrap();
        let new: Vec<String> = rows
            .iter()
            .map(|r| r.export_id.clone())
            .filter(|i| !ids.contains(i))
            .collect();
        assert_eq!(new.len(), 1);
        ids.push(new[0].clone());
        // A second apart at most: the newest is first even within one.
        std::thread::sleep(Duration::from_millis(20));
    }
    let rows = p.s.dawproject_snapshots().unwrap();
    let got: Vec<(String, u32)> = rows
        .iter()
        .map(|r| (r.export_id.clone(), r.tracks))
        .collect();
    assert_eq!(
        got,
        [
            (ids[2].clone(), 5),
            (ids[1].clone(), 4),
            (ids[0].clone(), 3)
        ]
    );
    assert!(
        rows.windows(2)
            .all(|w| w[0].exported_at >= w[1].exported_at)
    );
    for id in &ids {
        assert!(p.s.has_snapshot(id.clone()).unwrap());
    }
    assert!(
        !p.s.has_snapshot("00000000-0000-4000-8000-000000000099".into())
            .unwrap()
    );
    assert!(!p.s.has_snapshot("../x".into()).unwrap());
    assert!(!p.s.has_snapshot(String::new()).unwrap());
}

/// The project's ID, and the record head's in `dawproject_info`.
#[test]
fn project_id_is_read() {
    let _g = serial();
    let p = merge_project();
    assert_eq!(p.s.project_id().unwrap(), None);
    let file = exported(&p);
    let id = p.s.project_id().unwrap().expect("stamped by the export");
    let info = vizdaw_ffi::dawproject_info(text(&file)).unwrap();
    assert_eq!(info.project_id, Some(id));
    let lost = returned(&file, Strip::ExportId, &[], |_, _| {});
    assert_eq!(
        vizdaw_ffi::dawproject_info(text(&lost)).unwrap().project_id,
        None
    );
}

// ---------------------------------------------------------------- cancel and busy

/// Lessons: a cancel before the run, after each of stages 5–10 and inside
/// the apply (11) leaves
/// `Media/`, `Media/.incoming/` and the document as they were.
#[test]
fn cancel_at_every_stage_leaves_media_unchanged() {
    let _g = serial();
    let p = merge_project();
    let file = returned(&exported(&p), Strip::Nothing, &[], three_edits);
    let before = media_listing(&p);
    let incoming = listing(&p.pkg.join("Media/.incoming"));
    let doc = p.s.debug_document_json();
    // Before the run.
    p.s.debug_free_bytes(None).unwrap();
    p.s.debug_hold_jobs(true).unwrap();
    let id =
        p.s.merge_dawproject(mreq(&file, MergeStepRow::Apply))
            .unwrap();
    p.s.cancel_job(id).unwrap();
    p.s.debug_hold_jobs(false).unwrap();
    assert_eq!(wait_job(&p.s, id).state, JobStateRow::Cancelled);
    assert_eq!(media_listing(&p), before);
    assert_eq!(p.s.debug_document_json(), doc);
    // 11: inside the apply, at its first check (review M2).
    for stage in 5..=11 {
        p.s.debug_free_bytes(None).unwrap();
        p.s.debug_dawproject_cancel_at(Some(stage)).unwrap();
        let j = merge(&p.s, mreq(&file, MergeStepRow::Apply));
        assert_eq!(j.state, JobStateRow::Cancelled, "stage {stage}: {j:?}");
        assert_eq!(media_listing(&p), before, "stage {stage}");
        assert_eq!(
            listing(&p.pkg.join("Media/.incoming")),
            incoming,
            "stage {stage}"
        );
        assert_eq!(p.s.debug_document_json(), doc, "stage {stage}");
    }
    // The hook is used once.
    assert_eq!(
        merge(&p.s, mreq(&file, MergeStepRow::Apply)).state,
        JobStateRow::Done
    );
}

fn busy<T: std::fmt::Debug>(r: Result<T, VizError>, what: &str) {
    assert!(matches!(r, Err(VizError::Busy { .. })), "{what}: {r:?}");
}

/// L1: one interchange job at a time, a held merge counting; an open edit,
/// recording and a media job refuse; a running merge holds media jobs.
#[test]
fn busy_states_refuse() {
    let _g = serial();
    let p = merge_project();
    let file = returned(&exported(&p), Strip::Nothing, &[], three_edits);
    let r = || mreq(&file, MergeStepRow::Preview);
    // A running export.
    p.s.debug_hold_jobs(true).unwrap();
    let e =
        p.s.export_dawproject(req(p.seq, &p.out.join("y.dawproject")))
            .unwrap();
    busy(p.s.merge_dawproject(r()), "an export");
    p.s.cancel_job(e).unwrap();
    p.s.debug_hold_jobs(false).unwrap();
    wait_job(&p.s, e);
    // A Convert job.
    p.s.debug_hold_jobs(true).unwrap();
    let c = p.s.convert_media().unwrap();
    busy(p.s.merge_dawproject(r()), "a Convert job");
    p.s.cancel_job(c).unwrap();
    p.s.debug_hold_jobs(false).unwrap();
    wait_job(&p.s, c);
    // A running merge holds media jobs and another merge.
    p.s.debug_hold_jobs(true).unwrap();
    let m = p.s.merge_dawproject(r()).unwrap();
    busy(p.s.convert_media(), "a conversion under a merge");
    busy(p.s.merge_dawproject(r()), "another merge");
    p.s.cancel_job(m).unwrap();
    p.s.debug_hold_jobs(false).unwrap();
    assert_eq!(wait_job(&p.s, m).state, JobStateRow::Cancelled);
    // A held merge.
    let h = merge(&p.s, mreq(&file, MergeStepRow::Hold));
    assert_eq!(h.state, JobStateRow::Done, "{h:?}");
    busy(p.s.merge_dawproject(r()), "a held merge");
    busy(p.s.convert_media(), "a conversion under a held merge");
    p.s.cancel_merge(h.id).unwrap();
    // An open edit.
    p.s.begin_edit("Drag".into()).unwrap();
    busy(p.s.merge_dawproject(r()), "an open edit");
    p.s.end_edit().unwrap();
    // Recording (offline, an armed track).
    let rec = merge_project_with(true);
    rec.s
        .record(
            rec.seq,
            RecordSpecRow {
                replace: true,
                punch: None,
                looped: false,
                count_in_bars: 0,
                pre_roll_bars: 0,
                passes_as_takes: true,
            },
        )
        .unwrap();
    let until = Instant::now() + common::wait_limit();
    while !rec.s.record_state().unwrap().recording {
        assert!(Instant::now() < until, "recording did not start");
        std::thread::sleep(Duration::from_millis(5));
    }
    busy(rec.s.merge_dawproject(r()), "recording");
    rec.s.stop().unwrap();
    rec.s.close().unwrap();
    // Free again.
    assert_eq!(merge(&p.s, r()).state, JobStateRow::Done);
}

/// Hostile requests: `Model` or `Io` (never `Internal`); a job that starts
/// fails with a sentence that is not an internal error.
#[test]
fn hostile_requests() {
    let _g = serial();
    let p = merge_project();
    let file = exported(&p);
    let out = &p.out;
    let fifo = out.join("fifo.dawproject");
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    let lp = out.join("loop.dawproject");
    std::os::unix::fs::symlink(&lp, &lp).unwrap();
    let folder = out.join("dir.dawproject");
    std::fs::create_dir(&folder).unwrap();
    let long = format!("/{}.dawproject", "a/".repeat(2100));
    let doc = p.s.debug_document_json();
    let paths: Vec<(String, &str)> = vec![
        (String::new(), "empty"),
        ("relative.dawproject".into(), "relative"),
        (text(&folder), "a folder"),
        (text(&fifo), "a FIFO"),
        ("/dev/zero".into(), "/dev/zero"),
        (text(&lp), "a symlink loop"),
        (long, "over 4 KiB"),
        (format!("{}/a\0b.dawproject", text(out)), "a NUL"),
        (
            format!("{}/\u{fffd}\u{ffff}.dawproject", text(out)),
            "not UTF-8 on disk",
        ),
    ];
    let fine = |e: &VizError| matches!(e, VizError::Model { .. } | VizError::Io { .. });
    for (path, what) in &paths {
        for step in [
            MergeStepRow::Preview,
            MergeStepRow::Hold,
            MergeStepRow::Apply,
        ] {
            let r = DawprojectMergeRequestRow {
                path: path.clone(),
                ..mreq(&file, step)
            };
            match p.s.merge_dawproject(r) {
                Err(e) => assert!(fine(&e), "{what}: {e:?}"),
                Ok(id) => {
                    let j = wait_job(&p.s, id);
                    assert_eq!(j.state, JobStateRow::Failed, "{what}: {j:?}");
                    let m = j.message.unwrap_or_default();
                    assert!(!m.starts_with("internal error"), "{what}: {m}");
                }
            }
        }
    }
    for date in [
        "13/11/2026",
        "",
        "2026-11-1",
        "2026-11-13 ",
        "２０２６-11-13",
    ] {
        let r = DawprojectMergeRequestRow {
            date: date.into(),
            ..mreq(&file, MergeStepRow::Preview)
        };
        match p.s.merge_dawproject(r) {
            Err(VizError::Model { message }) => {
                assert_eq!(message, "the merge date must be YYYY-MM-DD", "{date:?}")
            }
            other => panic!("{date:?}: {other:?}"),
        }
    }
    for snapshot in ["x".to_owned(), "a".repeat(4096), "../x".into()] {
        let r = DawprojectMergeRequestRow {
            snapshot: Some(snapshot.clone()),
            ..mreq(&file, MergeStepRow::Preview)
        };
        match p.s.merge_dawproject(r) {
            Err(VizError::Model { message }) => {
                assert!(message.contains("snapshot"), "{message}")
            }
            other => panic!("{snapshot:.16}: {other:?}"),
        }
    }
    for job in [999, 0, u64::MAX] {
        assert!(matches!(
            p.s.confirm_merge(job),
            Err(VizError::Model { .. })
        ));
        assert!(matches!(p.s.cancel_merge(job), Err(VizError::Model { .. })));
    }
    // Another job's ID is not a held merge.
    let e = wait_job(
        &p.s,
        p.s.export_dawproject(req(p.seq, &out.join("z.dawproject")))
            .unwrap(),
    );
    assert!(matches!(
        p.s.confirm_merge(e.id),
        Err(VizError::Model { .. })
    ));
    assert!(matches!(
        p.s.cancel_merge(e.id),
        Err(VizError::Model { .. })
    ));
    assert_eq!(p.s.debug_document_json(), doc);
}

// ---------------------------------------------------------------- the report

/// Ruling 15: a conflict reaches the app as a Conflict row.
#[test]
fn conflict_rows_reach_swift() {
    let _g = serial();
    let p = merge_project();
    let file = returned(&exported(&p), Strip::Nothing, &[], |p, _| {
        half_level(p, "Keys")
    });
    let strip = keys_strip(&doc_of(&p.s));
    p.s.set_gain(strip.0, -3.0).unwrap();
    let j = merge(&p.s, mreq(&file, MergeStepRow::Preview));
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
    let report = j.report.unwrap();
    assert_eq!(items(&report, ReportKindRow::Conflict, CONFLICT_SETTING), 1);
    let m = j.merge.unwrap();
    assert_eq!(m.conflicts, 1);
    assert_eq!(m.conflict_lines.len(), 1);
}

/// Ruling 10: theirs' plug-in state reaches the running plug-in, and undo
/// takes it back.
#[test]
fn merge_state_reaches_the_plugin() {
    let _g = serial();
    let p = merge_project();
    let slot = p.s.plugin_slots().unwrap()[0].id;
    set_live_gain(&p, slot, 0.25);
    let src = exported(&p);
    let z = ZipReader::open(&src, &PackageLimits::default()).unwrap();
    let e = z.find("plugins/001.clap-preset").unwrap();
    let mut state = z.read(e, u64::MAX, None, &AtomicBool::new(false)).unwrap();
    assert_eq!(&state[..4], b"VZTG");
    state[4..12].copy_from_slice(&0.75f64.to_le_bytes());
    let file = returned(
        &src,
        Strip::Nothing,
        &[("plugins/001.clap-preset", state)],
        |_, _| {},
    );
    let j = merge(&p.s, mreq(&file, MergeStepRow::Apply));
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
    let value = |s: &Session| s.debug_plugin_param(slot, 0).unwrap_or(-1.0);
    idle_until(&p.s, |s| (value(s) - 0.75).abs() < 1e-9);
    p.s.undo().unwrap();
    idle_until(&p.s, |s| (value(s) - 0.25).abs() < 1e-9);
}

/// Review I1: a Save while a merge is held keeps its media, so the merge
/// confirmed after it has no offline audio; Save As waits for the hold.
#[cfg(target_os = "macos")]
#[test]
fn saving_keeps_a_held_merge_whole() {
    let _g = serial();
    let p = merge_project();
    let file = with_new_audio(&exported(&p));
    let before = media_listing(&p);
    let j = merge(&p.s, mreq(&file, MergeStepRow::Hold));
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
    let held = media_listing(&p);
    // The new audio, and the copies of the audio theirs kept (removed at
    // Merge: the matched regions keep ours').
    let new: Vec<String> = held
        .iter()
        .filter(|f| f.ends_with(".caf") && !before.contains(*f))
        .cloned()
        .collect();
    assert!(new.contains(&"new.caf".to_owned()), "{held:?}");
    let new = ["new.caf".to_owned()];
    // Save As is Busy and keeps the hold.
    let elsewhere = p.dir.path().join("Q.vizdaw");
    match p.s.save_as(text(&elsewhere)) {
        Err(VizError::Busy { message }) => {
            assert_eq!(message, vizdaw_ffi::dawproject::SAVE_AS_HELD);
            assert_eq!(p.s.save_as_refusal().unwrap(), Some(message));
        }
        other => panic!("{other:?}"),
    }
    assert!(!elsewhere.exists());
    // Save keeps the held media.
    p.s.save().unwrap();
    for f in held
        .iter()
        .filter(|f| !before.contains(*f) && !f.starts_with('.'))
    {
        assert!(p.pkg.join("Media").join(f).exists(), "{f} kept on Save");
    }
    p.s.confirm_merge(j.id).unwrap();
    p.s.recheck_media().unwrap();
    idle_until(&p.s, |s| s.debug_overviews_idle());
    let media = p.s.media().unwrap();
    assert!(media.iter().all(|m| m.offline.is_none()), "{media:?}");
    assert!(
        media.iter().any(|m| m.path == format!("Media/{}", new[0])),
        "{media:?}"
    );
    // After Merge, Save As works again.
    p.s.save_as(text(&elsewhere)).unwrap();
}

/// Review M1: an Apply that finishes while recording is not swapped in.
#[test]
fn apply_never_swaps_while_recording() {
    let _g = serial();
    let p = merge_project_with(true);
    let file = returned(&exported(&p), Strip::Nothing, &[], three_edits);
    p.s.debug_hold_jobs(true).unwrap();
    let id =
        p.s.merge_dawproject(mreq(&file, MergeStepRow::Apply))
            .unwrap();
    p.s.record(
        p.seq,
        RecordSpecRow {
            replace: true,
            punch: None,
            looped: false,
            count_in_bars: 0,
            pre_roll_bars: 0,
            passes_as_takes: true,
        },
    )
    .unwrap();
    let until = Instant::now() + common::wait_limit();
    while !p.s.record_state().unwrap().recording {
        assert!(Instant::now() < until, "recording did not start");
        std::thread::sleep(Duration::from_millis(5));
    }
    let doc = p.s.debug_document_json();
    p.s.debug_hold_jobs(false).unwrap();
    let j = wait_job(&p.s, id);
    failed_with(&j, RECORDING_APPLY);
    assert_eq!(
        RECORDING_APPLY, "Stop recording, then merge again.",
        "typed from the ruling"
    );
    assert_eq!(p.s.debug_document_json(), doc);
    p.s.stop().unwrap();
}

/// Review M3: Ruling 13's sentence, typed from the plan.
#[test]
fn needs_snapshot_names_each_snapshot() {
    let row = |n: u8, tracks: u32| SnapshotRow {
        export_id: format!("00000000-0000-4000-8000-00000000000{n}"),
        exported_at: 1_700_000_000,
        tracks,
    };
    assert_eq!(
        needs_snapshot(&[row(1, 3), row(2, 1)]),
        "This file lost its export ID; give `snapshot`, one of: \
         00000000-0000-4000-8000-000000000001 (2023-11-14 22:13 UTC, 3 tracks), \
         00000000-0000-4000-8000-000000000002 (2023-11-14 22:13 UTC, 1 track)"
    );
    assert_eq!(needs_snapshot(&[]), NOT_FROM_HERE);
}
