//! Crossfades through the session (plan 6c Task 4): the two edits, the
//! overlap rows, warnings in the snapshot, pasted rows, the envelope under
//! a crossfade and a sample-rate convert that shortens one.
//!
//! The fixture is Task 1's layout in a new lane of the demo: region A at
//! tick 0 (offset 0, 48 000 frames), region B at tick 15 360 (frame 24 000,
//! offset 48 000, 48 000 frames), so they overlap by 24 000 frames.

#![recursion_limit = "256"]

mod common;

use core_media::caf::{CafWriter, WriteMode};
use core_model::event::EventBody;
use core_model::format::BitDepth;
use proptest::prelude::*;
use std::path::Path;
use std::sync::{Arc, LazyLock, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use vizdaw_ffi::host::{force_offline, reset_host};
use vizdaw_ffi::{
    AudioEdit, BitDepthRow, CrossfadeLengthRow, EditCommand, EventEdit, EventKindRow, EventRow,
    FadeRow, FadeShapeRow, ImportTargetRow, JobStateRow, LaneEdit, ProjectFormatRow, Session,
    VizError,
};

const Q: i64 = 15_360;
const BAR: i64 = 4 * Q;

static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    force_offline();
    reset_host();
    g
}

struct Fx {
    s: Arc<Session>,
    seq: u64,
    lane: u64,
    media: u64,
    a: u64,
    b: u64,
    note: u64,
    _dir: tempfile::TempDir,
}

fn caf(path: &Path, frames: usize) {
    let mut w = CafWriter::create(path, 48_000, 2, BitDepth::Float32, WriteMode::Normal).unwrap();
    w.write(&vec![0.25f32; frames * 2]).unwrap();
    w.finish().unwrap();
}

fn wait(s: &Session, id: u64) -> vizdaw_ffi::JobRow {
    let until = Instant::now() + common::wait_limit();
    loop {
        let j = s.job(id).unwrap();
        if j.state != JobStateRow::Running {
            return j;
        }
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn no_fade() -> FadeRow {
    FadeRow {
        length: 0,
        shape: FadeShapeRow::Linear,
    }
}

fn insert(fx: &Fx, lane: u64, tick: i64, offset: u64, length: u64) -> Result<(), VizError> {
    fx.s.edit(
        EditCommand::Audio {
            edit: AudioEdit::Insert {
                seq: fx.seq,
                lane,
                tick,
                media: fx.media,
                source_offset: offset,
                length,
                gain_db: 0.0,
                fade_in: no_fade(),
                fade_out: no_fade(),
            },
        },
        None,
    )
    .map(|_| ())
}

/// The fixture with A at tick 0 of `a_len` frames and B at `b_tick`.
fn fixture_with(a_len: u64, b_tick: i64) -> Fx {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("tone.caf");
    caf(&src, 192_000);
    let s = Session::new_demo().unwrap();
    let seq = s.snapshot().unwrap().sequences[0].id;
    let note = {
        let l0 = s.lanes(seq).unwrap()[0].id;
        s.lane_events(seq, l0)
            .unwrap()
            .events
            .iter()
            .find(|e| e.kind == EventKindRow::Note)
            .unwrap()
            .id
    };
    let id = s
        .import_audio(
            vec![src.display().to_string()],
            Some(ImportTargetRow {
                seq,
                lane: None,
                tick: 0,
            }),
        )
        .unwrap();
    assert_eq!(wait(&s, id).state, JobStateRow::Done);
    let lane = s.lanes(seq).unwrap().last().unwrap().id;
    let media = s.media().unwrap()[0].id;
    let first = regions(&s, seq, lane)[0].id;
    s.edit(
        EditCommand::Event {
            edit: EventEdit::Delete {
                events: vec![first],
            },
        },
        None,
    )
    .unwrap();
    let mut fx = Fx {
        s,
        seq,
        lane,
        media,
        a: 0,
        b: 0,
        note,
        _dir: dir,
    };
    insert(&fx, lane, 0, 0, a_len).unwrap();
    insert(&fx, lane, b_tick, 48_000, 48_000).unwrap();
    let r = regions(&fx.s, seq, lane);
    fx.a = r[0].id;
    fx.b = r[1].id;
    fx
}

fn fixture() -> Fx {
    fixture_with(48_000, Q)
}

fn regions(s: &Session, seq: u64, lane: u64) -> Vec<EventRow> {
    s.lane_events(seq, lane)
        .unwrap()
        .events
        .into_iter()
        .filter(|e| e.kind == EventKindRow::AudioRegion)
        .collect()
}

fn row(fx: &Fx, id: u64) -> EventRow {
    regions(&fx.s, fx.seq, fx.lane)
        .into_iter()
        .find(|e| e.id == id)
        .unwrap()
}

fn xfade(event: u64, length: CrossfadeLengthRow, shape: FadeShapeRow) -> EditCommand {
    EditCommand::Audio {
        edit: AudioEdit::Crossfade {
            event,
            length,
            shape,
        },
    }
}

fn frames(n: u64) -> CrossfadeLengthRow {
    CrossfadeLengthRow::Frames { frames: n }
}

fn move_b(fx: &Fx, delta: i64) -> EditCommand {
    EditCommand::Event {
        edit: EventEdit::Move {
            events: vec![fx.b],
            delta,
            to_lane: None,
        },
    }
}

#[test]
fn lane_events_show_overlaps_and_crossfades() {
    let _g = serial();
    let fx = fixture();
    let (a, b) = (row(&fx, fx.a), row(&fx, fx.b));
    assert_eq!(b.overlap_with, Some(fx.a));
    assert_eq!(b.overlap_frames, 24_000);
    assert!(!b.crossfade);
    assert_eq!(
        (a.overlap_with, a.overlap_frames, a.crossfade),
        (None, 0, false)
    );
    fx.s.edit(xfade(fx.b, frames(4_800), FadeShapeRow::EqualPower), None)
        .unwrap();
    let b = row(&fx, fx.b);
    assert!(b.crossfade);
    assert_eq!(b.fade_in.length, 4_800);
    assert_eq!(b.fade_in.shape, FadeShapeRow::EqualPower);
    assert_eq!(b.overlap_with, Some(fx.a));
}

#[test]
fn crossfade_edit_is_one_undo_step() {
    let _g = serial();
    let fx = fixture();
    let before = fx.s.debug_document_json();
    let snap =
        fx.s.edit(xfade(fx.b, frames(4_800), FadeShapeRow::Linear), None)
            .unwrap();
    assert_eq!(snap.undo_label.as_deref(), Some("Crossfade"));
    assert_eq!(
        fx.s.snapshot().unwrap().undo_label.as_deref(),
        Some("Crossfade")
    );
    let after = fx.s.debug_document_json();
    fx.s.undo().unwrap();
    assert_eq!(fx.s.debug_document_json(), before);
    fx.s.redo().unwrap();
    assert_eq!(fx.s.debug_document_json(), after);
    let snap =
        fx.s.edit(
            EditCommand::Audio {
                edit: AudioEdit::RemoveCrossfade { event: fx.b },
            },
            None,
        )
        .unwrap();
    assert_eq!(snap.undo_label.as_deref(), Some("Remove Crossfade"));
    assert!(!row(&fx, fx.b).crossfade);
    fx.s.undo().unwrap();
    assert!(row(&fx, fx.b).crossfade);
}

#[test]
fn hostile_crossfade_edits_are_refused() {
    let _g = serial();
    let fx = fixture();
    let refused = |cmd: EditCommand| {
        let before = fx.s.debug_document_json();
        let e = fx.s.edit(cmd, None).unwrap_err();
        assert!(matches!(e, VizError::Model { .. }), "{e:?}");
        assert_eq!(fx.s.debug_document_json(), before);
    };
    let lin = FadeShapeRow::Linear;
    refused(xfade(fx.b, frames(u64::MAX), lin));
    refused(xfade(fx.b, frames(0), lin));
    refused(xfade(fx.b, CrossfadeLengthRow::UpTo { frames: 0 }, lin));
    for event in [u64::MAX, fx.note, fx.a] {
        refused(xfade(event, frames(100), lin));
        refused(EditCommand::Audio {
            edit: AudioEdit::RemoveCrossfade { event },
        });
    }
    // B has no crossfade yet.
    refused(EditCommand::Audio {
        edit: AudioEdit::RemoveCrossfade { event: fx.b },
    });
    // A region that overlaps only on another take.
    fx.s.edit(
        EditCommand::Lane {
            edit: LaneEdit::AddTake {
                seq: fx.seq,
                lane: fx.lane,
                name: "Take 2".into(),
            },
        },
        None,
    )
    .unwrap();
    insert(&fx, fx.lane, 20_000, 0, 10_000).unwrap();
    let d = regions(&fx.s, fx.seq, fx.lane)[0].id;
    assert_eq!(row(&fx, d).overlap_with, None);
    refused(xfade(d, frames(100), lin));
}

fn any_edit() -> impl Strategy<Value = AudioEdit> {
    let event = prop_oneof![0u64..40, any::<u64>()];
    let n = prop_oneof![0u64..60_000, any::<u64>()];
    let shape = prop_oneof![
        Just(FadeShapeRow::Linear),
        Just(FadeShapeRow::EqualPower),
        Just(FadeShapeRow::Exponential),
        Just(FadeShapeRow::SCurve),
    ];
    prop_oneof![
        (event.clone(), any::<bool>(), n, shape).prop_map(|(event, up_to, n, shape)| {
            AudioEdit::Crossfade {
                event,
                length: if up_to {
                    CrossfadeLengthRow::UpTo { frames: n }
                } else {
                    CrossfadeLengthRow::Frames { frames: n }
                },
                shape,
            }
        }),
        event.prop_map(|event| AudioEdit::RemoveCrossfade { event }),
    ]
}

static MODEL_DOC: LazyLock<core_model::document::Document> = LazyLock::new(|| {
    let _g = serial();
    let fx = fixture();
    core_model::file::from_json(&fx.s.debug_document_json()).unwrap()
});

proptest! {
    #![proptest_config(ProptestConfig { cases: 2048, failure_persistence: None, ..ProptestConfig::default() })]

    #[test]
    fn random_crossfade_edits_never_panic(edit in any_edit()) {
        let cmd = vizdaw_ffi::edit::to_command(&EditCommand::Audio { edit }).unwrap();
        if let Ok(applied) = core_model::command::apply(&MODEL_DOC, &cmd) {
            prop_assert_eq!(core_model::rules::validate(&applied.doc), Vec::<String>::new());
        }
    }
}

static SESSION_FX: LazyLock<Mutex<Fx>> = LazyLock::new(|| {
    let _g = serial();
    Mutex::new(fixture())
});

proptest! {
    #![proptest_config(ProptestConfig { cases: 64, failure_persistence: None, ..ProptestConfig::default() })]

    #[test]
    fn random_crossfade_edits_on_a_session_are_never_internal(edit in any_edit()) {
        // The fixture first: building it takes the serial lock itself.
        let fx = SESSION_FX.lock().unwrap_or_else(|e| e.into_inner());
        let _g = serial();
        match fx.s.edit(EditCommand::Audio { edit }, None) {
            Ok(_) => {
                fx.s.undo().unwrap();
            }
            Err(e) => prop_assert!(!matches!(e, VizError::Internal { .. }), "{e:?}"),
        }
    }
}

#[test]
fn edit_warnings_reach_the_snapshot() {
    let _g = serial();
    let fx = fixture();
    let make = || {
        fx.s.edit(xfade(fx.b, frames(4_800), FadeShapeRow::Linear), None)
            .unwrap()
    };
    let check = |w: &[String]| {
        assert_eq!(w.len(), 1, "{w:?}");
        assert!(w[0].starts_with("Crossfade removed"), "{w:?}");
        assert!(w[0].contains("track"), "{w:?}");
    };
    // The edit made the crossfade: nothing to say.
    assert!(make().edit_notes.is_empty());
    // Move: past A's end.
    let snap = fx.s.edit(move_b(&fx, 2 * BAR), None).unwrap();
    check(&snap.edit_notes);
    assert!(fx.s.snapshot().unwrap().edit_notes.is_empty());
    assert!(fx.s.undo().unwrap().edit_notes.is_empty());
    assert!(fx.s.redo().unwrap().edit_notes.is_empty());
    fx.s.undo().unwrap();
    assert!(row(&fx, fx.b).crossfade);
    // edit_all.
    let snap =
        fx.s.edit_all("Move".into(), vec![move_b(&fx, 2 * BAR)], None)
            .unwrap();
    check(&snap.edit_notes);
    assert!(fx.s.snapshot().unwrap().edit_notes.is_empty());
    fx.s.undo().unwrap();
    assert!(row(&fx, fx.b).crossfade);
    // A gesture: the last step's warning arrives with the end.
    fx.s.begin_edit("Move".into()).unwrap();
    let step = fx.s.edit_gesture(move_b(&fx, 2 * BAR), None).unwrap();
    check(&step.edit_notes);
    let end = fx.s.end_edit().unwrap();
    check(&end.edit_notes);
    assert!(fx.s.snapshot().unwrap().edit_notes.is_empty());
    fx.s.undo().unwrap();
    // A gesture that goes back to a harmless place has none.
    fx.s.begin_edit("Move".into()).unwrap();
    fx.s.edit_gesture(move_b(&fx, 2 * BAR), None).unwrap();
    fx.s.edit_gesture(move_b(&fx, 0), None).unwrap();
    assert!(fx.s.end_edit().unwrap().edit_notes.is_empty());
    // An aborted gesture says nothing.
    fx.s.begin_edit("Move".into()).unwrap();
    fx.s.edit_gesture(move_b(&fx, 2 * BAR), None).unwrap();
    assert!(fx.s.abort_edit().unwrap().edit_notes.is_empty());
    assert!(row(&fx, fx.b).crossfade);
}

/// Edit notes are one-shot and apart from the standing warnings: a document
/// with an unresolved reference keeps its warning in every snapshot and the
/// notes only in the edit's.
#[test]
fn edit_notes_stay_apart_from_standing_warnings() {
    let _g = serial();
    let fx = fixture();
    // A note in a lane with no destination is unresolved.
    let l0 = fx.s.lanes(fx.seq).unwrap()[0].id;
    let note =
        fx.s.lane_events(fx.seq, l0)
            .unwrap()
            .events
            .into_iter()
            .find(|e| e.id == fx.note)
            .unwrap();
    fx.s.edit(
        EditCommand::Lane {
            edit: LaneEdit::Add {
                seq: fx.seq,
                name: "Loose".into(),
                index: None,
            },
        },
        None,
    )
    .unwrap();
    let loose = fx.s.lanes(fx.seq).unwrap().last().unwrap().id;
    let snap =
        fx.s.edit(
            EditCommand::Event {
                edit: EventEdit::Insert {
                    seq: fx.seq,
                    lane: loose,
                    events: vec![note],
                },
            },
            None,
        )
        .unwrap();
    assert_eq!(snap.warnings.len(), 1, "{:?}", snap.warnings);
    assert!(snap.warnings[0].contains("reference"));
    assert!(snap.edit_notes.is_empty());
    fx.s.edit(xfade(fx.b, frames(4_800), FadeShapeRow::Linear), None)
        .unwrap();
    let snap = fx.s.edit(move_b(&fx, 2 * BAR), None).unwrap();
    assert_eq!(snap.warnings.len(), 1, "{:?}", snap.warnings);
    assert!(snap.warnings[0].contains("reference"));
    assert_eq!(snap.edit_notes.len(), 1, "{:?}", snap.edit_notes);
    assert!(snap.edit_notes[0].starts_with("Crossfade removed"));
    let after = fx.s.snapshot().unwrap();
    assert_eq!(after.warnings, snap.warnings);
    assert!(after.edit_notes.is_empty());
}

/// Removing media that deletes a crossfade's partner says so.
#[test]
fn removing_the_partners_media_says_so() {
    let _g = serial();
    let fx = fixture();
    let two = fx._dir.path().join("two.caf");
    caf(&two, 96_000);
    let id =
        fx.s.import_audio(vec![two.display().to_string()], None)
            .unwrap();
    assert_eq!(wait(&fx.s, id).state, JobStateRow::Done);
    let other =
        fx.s.media()
            .unwrap()
            .into_iter()
            .map(|m| m.id)
            .find(|m| *m != fx.media)
            .unwrap();
    // A third region on the other media, starting inside A.
    fx.s.edit(
        EditCommand::Audio {
            edit: AudioEdit::Insert {
                seq: fx.seq,
                lane: fx.lane,
                tick: Q / 2,
                media: other,
                source_offset: 0,
                length: 48_000,
                gain_db: 0.0,
                fade_in: no_fade(),
                fade_out: no_fade(),
            },
        },
        None,
    )
    .unwrap();
    let c = regions(&fx.s, fx.seq, fx.lane)
        .into_iter()
        .find(|r| r.id != fx.a && r.id != fx.b)
        .unwrap()
        .id;
    fx.s.edit(xfade(c, frames(2_400), FadeShapeRow::Linear), None)
        .unwrap();
    assert!(row(&fx, c).crossfade);
    let snap = fx.s.remove_media(fx.media).unwrap();
    assert!(
        snap.edit_notes
            .iter()
            .any(|n| n.starts_with("Crossfade removed")),
        "{:?}",
        snap.edit_notes
    );
    assert!(!row(&fx, c).crossfade);
    assert!(fx.s.snapshot().unwrap().edit_notes.is_empty());
}

/// A region nested in an earlier one: the earlier region is silent under it
/// after the fade and plays again once it ends.
#[test]
fn a_nested_region_hands_the_earlier_one_its_gain_back() {
    let _g = serial();
    // A: frames 0..120 000; B: 24 000..72 000.
    let fx = fixture_with(120_000, Q);
    fx.s.edit(xfade(fx.b, frames(4_800), FadeShapeRow::Linear), None)
        .unwrap();
    let under = fx.s.region_envelope(fx.a, 30_000, 60_000, 3).unwrap();
    assert_eq!(under, vec![0.0; 3]);
    let after = fx.s.region_envelope(fx.a, 80_000, 100_000, 2).unwrap();
    assert_eq!(after, vec![1.0, 1.0]);
}

#[test]
fn pasted_rows_carry_the_flag() {
    let _g = serial();
    let fx = fixture();
    fx.s.edit(xfade(fx.b, frames(4_800), FadeShapeRow::Linear), None)
        .unwrap();
    let (a, b) = (row(&fx, fx.a), row(&fx, fx.b));
    let flag = |r: &EventRow| match vizdaw_ffi::events::spec(r).unwrap().body {
        EventBody::AudioRegion { crossfade, .. } => crossfade,
        other => panic!("{other:?}"),
    };
    assert!(flag(&b));
    assert!(!flag(&a));
    fx.s.edit(
        EditCommand::Lane {
            edit: LaneEdit::Add {
                seq: fx.seq,
                name: "Copy".into(),
                index: None,
            },
        },
        None,
    )
    .unwrap();
    let other = fx.s.lanes(fx.seq).unwrap().last().unwrap().id;
    fx.s.edit(
        EditCommand::Event {
            edit: EventEdit::Insert {
                seq: fx.seq,
                lane: other,
                events: vec![a, b],
            },
        },
        None,
    )
    .unwrap();
    let pasted = regions(&fx.s, fx.seq, other);
    assert_eq!(pasted.len(), 2);
    assert_eq!(pasted[1].overlap_with, Some(pasted[0].id));
    assert!(pasted[1].crossfade);
    assert_eq!(pasted[1].fade_in.length, 4_800);
    assert!(!pasted[0].crossfade);
}

/// Carry-over: the earlier region's envelope follows the complementary
/// curve under the span, as the engine plays it.
#[test]
fn the_earlier_regions_envelope_follows_the_complement() {
    let _g = serial();
    let fx = fixture();
    fx.s.edit(xfade(fx.b, frames(4_800), FadeShapeRow::Linear), None)
        .unwrap();
    // Columns of 1 200 frames from media frame 24 000: the span is four
    // columns; the centres sit at 1/8, 3/8, 5/8, 7/8 of it.
    let env = fx.s.region_envelope(fx.a, 24_000, 28_800, 4).unwrap();
    for (got, want) in env.iter().zip([0.875, 0.625, 0.375, 0.125]) {
        assert!((got - want).abs() < 1e-4, "{env:?}");
    }
    // Before the span A plays at full gain; B's own envelope rises.
    let before = fx.s.region_envelope(fx.a, 0, 24_000, 2).unwrap();
    assert_eq!(before, vec![1.0, 1.0]);
    let later = fx.s.region_envelope(fx.b, 48_000, 52_800, 4).unwrap();
    for (got, want) in later.iter().zip([0.125, 0.375, 0.625, 0.875]) {
        assert!((got - want).abs() < 1e-4, "{later:?}");
    }
    // Equal power: cos of the position, not 1 - x.
    fx.s.edit(xfade(fx.b, frames(4_800), FadeShapeRow::EqualPower), None)
        .unwrap();
    let eq = fx.s.region_envelope(fx.a, 24_000, 28_800, 4).unwrap();
    let want = (0.125f32 * std::f32::consts::FRAC_PI_2).cos();
    assert!((eq[0] - want).abs() < 1e-4, "{eq:?}");
    // Without the flag A is at full gain again.
    fx.s.edit(
        EditCommand::Audio {
            edit: AudioEdit::RemoveCrossfade { event: fx.b },
        },
        None,
    )
    .unwrap();
    let plain = fx.s.region_envelope(fx.a, 24_000, 28_800, 4).unwrap();
    assert_eq!(plain, vec![1.0; 4]);
}

/// Carry-over: a sample-rate convert that shortens a crossfade says so.
#[test]
fn a_convert_that_shortens_a_crossfade_says_so() {
    let _g = serial();
    // Found by search: at 44.1 kHz the rounded overlap (5 510) is one
    // frame under the rounded crossfade (5 511).
    let fx = fixture_with(30_001, 15_362);
    fx.s.edit(
        xfade(
            fx.b,
            CrossfadeLengthRow::UpTo { frames: u64::MAX },
            FadeShapeRow::Linear,
        ),
        None,
    )
    .unwrap();
    let b = row(&fx, fx.b);
    assert_eq!(
        (b.crossfade, b.fade_in.length, b.overlap_frames),
        (true, 5_998, 5_998)
    );
    let id =
        fx.s.set_project_format(ProjectFormatRow {
            sample_rate: 44_100,
            depth: BitDepthRow::Int24,
        })
        .unwrap();
    let job = wait(&fx.s, id);
    assert_eq!(job.state, JobStateRow::Done, "{job:?}");
    let b = row(&fx, fx.b);
    assert!(b.crossfade);
    assert_eq!(b.fade_in.length, 5_510);
    assert!(
        job.problems
            .iter()
            .any(|p| p.starts_with("Crossfade on region") && p.contains("shortened")),
        "{:?}",
        job.problems
    );
    // The notes live in the job row alone: no later snapshot repeats them.
    assert!(fx.s.end_edit().unwrap().edit_notes.is_empty());
    assert!(
        fx.s.edit_all("Nothing".into(), vec![], None)
            .unwrap()
            .edit_notes
            .is_empty()
    );
    assert!(fx.s.snapshot().unwrap().edit_notes.is_empty());
}
