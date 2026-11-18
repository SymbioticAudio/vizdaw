//! The release-fixes changes through Co-Producer's preview (plan 8e Task
//! 6b): `region.set_fade_shape`, `destination.set_sound` and
//! `lane.set_output` preview exactly what their apply does, the track
//! decides who says yes for a fade shape and an output, the Built-in
//! Synth's shared sound always asks, and what the apply would refuse the
//! preview refuses, changing nothing.
//!
//! Offline only; the support folder is the test build's temp folder.

#![recursion_limit = "256"]

mod common;

use core_media::caf::{CafWriter, WriteMode};
use core_model::format::BitDepth;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use vizdaw_ffi::builtin_sound::BuiltinSound;
use vizdaw_ffi::co_producer::{CoProducerApproval, PlannedRow, PreviewRow};
use vizdaw_ffi::control::{ControlArg, ControlValue};
use vizdaw_ffi::host::{force_offline, reset_host};
use vizdaw_ffi::record::set_free_bytes;
use vizdaw_ffi::tracks::TrackKindRow;
use vizdaw_ffi::{
    AudioEdit, EditCommand, EventKindRow, EventRow, FadeRow, FadeShapeRow, ImportTargetRow,
    JobStateRow, LaneEdit, Session, VizError,
};

const STAMP: &str = "2026-11-17 10-00";
const CHANGED: &str = "The song changed since the preview; preview again.";

static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    force_offline();
    reset_host();
    set_free_bytes(None);
    g
}

fn int(name: &str, value: u64) -> ControlArg {
    ControlArg {
        name: name.into(),
        value: ControlValue::Int {
            value: value as i64,
        },
    }
}

fn text(name: &str, value: &str) -> ControlArg {
    ControlArg {
        name: name.into(),
        value: ControlValue::Text {
            value: value.into(),
        },
    }
}

fn planned(id: &str, args: Vec<ControlArg>) -> PlannedRow {
    PlannedRow {
        id: id.into(),
        args,
    }
}

fn fade(seq: u64, lane: u64, event: u64, edge: &str, shape: &str) -> PlannedRow {
    planned(
        "region.set_fade_shape",
        vec![
            int("seq", seq),
            int("lane", lane),
            int("event", event),
            text("fade", edge),
            text("shape", shape),
        ],
    )
}

fn sound(dest: u64, s: &str) -> PlannedRow {
    planned(
        "destination.set_sound",
        vec![int("dest", dest), text("sound", s)],
    )
}

/// `lane.set_output`; `None` is the registry's null.
fn output(seq: u64, lane: u64, to: Option<u64>) -> PlannedRow {
    let to = match to {
        Some(o) => int("output", o),
        None => text("output", "none"),
    };
    planned(
        "lane.set_output",
        vec![int("seq", seq), int("lane", lane), to],
    )
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

fn region(s: &Session, seq: u64, lane: u64) -> EventRow {
    s.lane_events(seq, lane)
        .unwrap()
        .events
        .into_iter()
        .find(|e| e.kind == EventKindRow::AudioRegion)
        .unwrap()
}

fn lane_row(s: &Session, seq: u64, lane: u64) -> vizdaw_ffi::LaneRow {
    s.lanes(seq)
        .unwrap()
        .into_iter()
        .find(|l| l.id == lane)
        .unwrap()
}

/// The track's position and name, as the list says it.
fn track(s: &Session, seq: u64, lane: u64) -> String {
    let lanes = s.lanes(seq).unwrap();
    let i = lanes.iter().position(|l| l.id == lane).unwrap();
    format!("Track {}, {}", i + 1, lanes[i].name)
}

fn synth(s: &Session) -> u64 {
    s.destinations()
        .unwrap()
        .into_iter()
        .find(|d| d.builtin_sound.is_some())
        .expect("a Built-in Synth")
        .id
}

/// The demo, saved, with the composer's audio track "tone" holding one
/// region, and Co-Producer's audio track holding one region of its own.
struct Song {
    s: Arc<Session>,
    dir: tempfile::TempDir,
    seq: u64,
    /// The composer's track.
    tone: u64,
    tone_region: u64,
    /// Co-Producer's track.
    ours: u64,
    ours_region: u64,
    /// The instrument track the demo opens with.
    instrument: u64,
}

fn song() -> Song {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("tone.caf");
    caf(&src, 96_000);
    let s = Session::new_demo().unwrap();
    let seq = s.snapshot().unwrap().sequences[0].id;
    let instrument = s.lanes(seq).unwrap()[0].id;
    let job = s
        .import_audio(
            vec![src.display().to_string()],
            Some(ImportTargetRow {
                seq,
                lane: None,
                tick: 0,
            }),
        )
        .unwrap();
    assert_eq!(wait(&s, job).state, JobStateRow::Done);
    let tone = s.lanes(seq).unwrap().last().unwrap().id;
    let tone_region = region(&s, seq, tone).id;
    let media = s.media().unwrap()[0].id;
    s.save_as(dir.path().join("P.vizdaw").display().to_string())
        .unwrap();
    // Co-Producer's own track, with a region of the same media.
    s.co_producer_begin("add a loop".into()).unwrap();
    s.co_producer_enter(STAMP.into()).unwrap();
    let before: Vec<u64> = s.lanes(seq).unwrap().iter().map(|l| l.id).collect();
    s.new_track(seq, TrackKindRow::Audio, None).unwrap();
    let ours = s
        .lanes(seq)
        .unwrap()
        .iter()
        .map(|l| l.id)
        .find(|l| !before.contains(l))
        .unwrap();
    let flat = FadeRow {
        length: 0,
        shape: FadeShapeRow::Linear,
    };
    s.edit(
        EditCommand::Audio {
            edit: AudioEdit::Insert {
                seq,
                lane: ours,
                tick: 0,
                media,
                source_offset: 0,
                length: 48_000,
                gain_db: 0.0,
                fade_in: flat,
                fade_out: flat,
            },
        },
        None,
    )
    .unwrap();
    s.co_producer_leave();
    s.co_producer_end().unwrap();
    assert!(lane_row(&s, seq, ours).co_producer);
    assert!(!lane_row(&s, seq, tone).co_producer);
    let ours_region = region(&s, seq, ours).id;
    // The synth starts on Piano: the composer's own choice.
    s.set_builtin_sound(synth(&s), BuiltinSound::Piano).unwrap();
    Song {
        s,
        dir,
        seq,
        tone,
        tone_region,
        ours,
        ours_region,
        instrument,
    }
}

fn shape_of(name: &str) -> FadeShapeRow {
    match name {
        "linear" => FadeShapeRow::Linear,
        "equal_power" => FadeShapeRow::EqualPower,
        "exponential" => FadeShapeRow::Exponential,
        _ => FadeShapeRow::SCurve,
    }
}

fn arg_text<'a>(p: &'a PlannedRow, name: &str) -> Option<&'a str> {
    p.args.iter().find_map(|a| match &a.value {
        ControlValue::Text { value } if a.name == name => Some(value.as_str()),
        _ => None,
    })
}

fn arg_int(p: &PlannedRow, name: &str) -> Option<u64> {
    p.args.iter().find_map(|a| match &a.value {
        ControlValue::Int { value } if a.name == name => Some(*value as u64),
        _ => None,
    })
}

/// One planned command as the app's handler runs it.
fn run_as_the_app(s: &Session, p: &PlannedRow) -> Result<(), VizError> {
    let seq = arg_int(p, "seq");
    let lane = arg_int(p, "lane");
    match p.id.as_str() {
        "region.set_fade_shape" => {
            let (seq, lane) = (seq.unwrap(), lane.unwrap());
            let event = arg_int(p, "event").unwrap();
            let row = s
                .lane_events(seq, lane)?
                .events
                .into_iter()
                .find(|e| e.id == event)
                .unwrap();
            let (mut fade_in, mut fade_out) = (row.fade_in, row.fade_out);
            let shape = shape_of(arg_text(p, "shape").unwrap());
            if arg_text(p, "fade") == Some("in") {
                fade_in.shape = shape;
            } else {
                fade_out.shape = shape;
            }
            s.edit(
                EditCommand::Audio {
                    edit: AudioEdit::SetFades {
                        event,
                        fade_in,
                        fade_out,
                    },
                },
                None,
            )
            .map(|_| ())
        }
        "destination.set_sound" => {
            let sound = match arg_text(p, "sound").unwrap() {
                "piano" => BuiltinSound::Piano,
                "bass" => BuiltinSound::Bass,
                "pad" => BuiltinSound::Pad,
                "lead" => BuiltinSound::Lead,
                _ => BuiltinSound::Drums,
            };
            s.set_builtin_sound(arg_int(p, "dest").unwrap(), sound)
        }
        "lane.set_output" => s
            .edit(
                EditCommand::Lane {
                    edit: LaneEdit::SetOutput {
                        seq: seq.unwrap(),
                        lane: lane.unwrap(),
                        output: arg_int(p, "output"),
                    },
                },
                None,
            )
            .map(|_| ()),
        other => panic!("not a fixes command: {other}"),
    }
}

/// One Co-Producer call, as the app's apply makes it.
fn take_and_run(s: &Session, token: String, plan: &[PlannedRow]) -> Result<(), VizError> {
    s.co_producer_enter(STAMP.into()).unwrap();
    let r = s
        .co_producer_take(token, plan.to_vec(), CoProducerApproval::Sheet)
        .and_then(|()| plan.iter().try_for_each(|p| run_as_the_app(s, p)));
    s.co_producer_leave();
    r
}

fn doc(s: &Session) -> core_model::document::Document {
    core_model::file::from_json(&s.debug_document_json()).unwrap()
}

fn items(p: &PreviewRow) -> Vec<(String, String, bool)> {
    p.items
        .iter()
        .map(|i| (i.thing.clone(), i.change.clone(), i.composers_work))
        .collect()
}

/// Everything a preview must leave as it was.
#[derive(Debug, PartialEq)]
struct Untouched {
    doc: String,
    undo: Option<String>,
    version: u64,
}

fn untouched(s: &Session) -> Untouched {
    let snap = s.snapshot().unwrap();
    Untouched {
        doc: s.debug_document_json(),
        undo: snap.undo_label,
        version: snap.version,
    }
}

fn output_name(s: &Session, id: Option<u64>) -> String {
    id.map_or_else(
        || "none".to_string(),
        |id| {
            s.output_paths()
                .unwrap()
                .into_iter()
                .find(|o| o.id == id)
                .unwrap()
                .name
        },
    )
}

/// Each fixes command: its items and who says yes, and an apply that does
/// exactly what its preview said (8a's preview-matches-apply).
#[test]
fn fixes_previews_match_what_the_apply_does() {
    let _g = serial();
    let t = song();
    let s = &t.s;
    let outputs: Vec<u64> = s.output_paths().unwrap().iter().map(|o| o.id).collect();
    let tone_out = lane_row(s, t.seq, t.tone).audio_out;
    assert!(tone_out.is_some(), "an imported track plays an output");
    let ours_out = lane_row(s, t.seq, t.ours).audio_out;
    let master = s
        .mixer()
        .unwrap()
        .strips
        .into_iter()
        .find(|x| x.kind == vizdaw_ffi::StripKindRow::Master)
        .unwrap()
        .id;
    s.co_producer_begin("shape the fades".into()).unwrap();
    let cases = [
        (
            fade(t.seq, t.tone, t.tone_region, "in", "s_curve"),
            track(s, t.seq, t.tone),
            "fade in linear → s-curve".to_string(),
            true,
        ),
        (
            fade(t.seq, t.ours, t.ours_region, "out", "equal_power"),
            track(s, t.seq, t.ours),
            "fade out linear → equal power".to_string(),
            false,
        ),
        (
            sound(synth(s), "pad"),
            "Built-in Synth".to_string(),
            "sound Piano → Pad".to_string(),
            true,
        ),
        (
            output(t.seq, t.tone, None),
            track(s, t.seq, t.tone),
            format!("output {} → none", output_name(s, tone_out)),
            true,
        ),
        (
            output(t.seq, t.tone, Some(outputs[0])),
            track(s, t.seq, t.tone),
            format!("output none → {}", output_name(s, Some(outputs[0]))),
            true,
        ),
        (
            output(t.seq, t.ours, None),
            track(s, t.seq, t.ours),
            format!("output {} → none", output_name(s, ours_out)),
            false,
        ),
    ];
    for (p, thing, change, asks) in cases {
        let plan = vec![p];
        let before = untouched(s);
        let row = s.co_producer_preview(plan.clone()).unwrap();
        assert_eq!(untouched(s), before, "the preview changed something");
        assert_eq!(items(&row), vec![(thing, change, asks)], "{plan:?}");
        assert_eq!(row.needs_yes, asks, "{plan:?}");
        // A hand edit between the preview and the take.
        s.set_gain(master, -1.0).unwrap();
        let before = untouched(s);
        let e = take_and_run(s, row.token, &plan).unwrap_err();
        assert_eq!(e.to_string(), CHANGED, "{plan:?}");
        assert_eq!(untouched(s).doc, before.doc, "{plan:?}");
        // Previewed again, it does exactly what it said.
        let row = s.co_producer_preview(plan.clone()).unwrap();
        let start = doc(s);
        take_and_run(s, row.token, &plan).unwrap();
        let said: Vec<(String, String)> = core_model::diff::describe(&start, &doc(s))
            .into_iter()
            .map(|i| (i.thing, i.change))
            .collect();
        let shown: Vec<(String, String)> =
            row.items.into_iter().map(|i| (i.thing, i.change)).collect();
        assert_eq!(said, shown, "{plan:?}");
    }
    let label = s.co_producer_end().unwrap().unwrap();
    assert!(label.starts_with("Co-Producer: "), "{label}");
    // A shape the region already has changes nothing.
    s.co_producer_begin("again".into()).unwrap();
    let row = s
        .co_producer_preview(vec![fade(t.seq, t.tone, t.tone_region, "in", "s_curve")])
        .unwrap();
    assert!(row.items.is_empty(), "{row:?}");
    assert!(!row.needs_yes);
    s.co_producer_end().unwrap();
    s.close().unwrap();
    drop(t.dir);
}

/// No plug-in is asked anything here.
struct NoPlugins;

impl vizdaw_ffi::mixer_chores::PluginLookup for NoPlugins {
    fn resolve(
        &self,
        text: &str,
        want: core_host::PluginKind,
    ) -> Result<core_model::mixer::PluginRef, VizError> {
        vizdaw_ffi::mixer_chores::resolve_plugin(text, want)
    }
    fn params(
        &self,
        _slot: core_model::ids::PluginInstanceId,
    ) -> Option<Vec<vizdaw_ffi::ParamRow>> {
        None
    }
}

/// An output to another output names both, by `doc.outputs()`'s names.
#[test]
fn an_output_change_names_both_outputs() {
    use core_model::command::{Command, RoutingCmd, apply};
    let _g = serial();
    let t = song();
    let s = &t.s;
    let d = doc(s);
    let d = apply(
        &d,
        &Command::Routing(RoutingCmd::AddOutput {
            name: "Cue \u{202E}Mix".into(),
            reference: "audio:3-4".into(),
        }),
    )
    .unwrap()
    .doc;
    let cue = *d.outputs().keys().next_back().unwrap();
    let was = lane_row(s, t.seq, t.tone).audio_out.unwrap();
    let was = d
        .outputs()
        .get(&core_model::ids::OutputPathId(was))
        .unwrap()
        .name
        .clone();
    let plan = vizdaw_ffi::co_producer::plan_of(&[output(t.seq, t.tone, Some(cue.0))]).unwrap();
    let h = core_model::history::History::new(d);
    let got = vizdaw_ffi::co_producer::preview_items(&h, None, &plan, &NoPlugins).unwrap();
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0].thing, track(s, t.seq, t.tone));
    assert_eq!(got[0].change, format!("output {was} → Cue  Mix"));
    assert!(got[0].composers_work);
    s.close().unwrap();
}

/// The preview refuses what the app's handler refuses, with its words,
/// and changes nothing.
#[test]
fn fixes_previews_refuse_what_the_apply_refuses() {
    let _g = serial();
    let t = song();
    let s = &t.s;
    let other_seq = s.snapshot().unwrap().sequences.len() as u64 + 999;
    s.co_producer_begin("try odd things".into()).unwrap();
    let before = untouched(s);
    let refused = |plan: PlannedRow| -> String {
        match s.co_producer_preview(vec![plan.clone()]) {
            Err(e) => e.to_string(),
            Ok(p) => panic!("{plan:?} previewed: {p:?}"),
        }
    };
    // A fade: the region must be an audio region on that track's take.
    assert_eq!(
        refused(fade(t.seq, t.tone, t.ours_region, "in", "linear")),
        "region.set_fade_shape: `event` names no event on that track's active take"
    );
    assert_eq!(
        refused(fade(t.seq, t.tone, 999_999, "in", "linear")),
        "region.set_fade_shape: `event` names no event on that track's active take"
    );
    let note = s
        .lane_events(t.seq, t.instrument)
        .unwrap()
        .events
        .into_iter()
        .find(|e| e.kind != EventKindRow::AudioRegion)
        .expect("the demo's first track holds notes")
        .id;
    assert_eq!(
        refused(fade(t.seq, t.instrument, note, "in", "linear")),
        "region.set_fade_shape: `event` must be an audio region"
    );
    assert_eq!(
        refused(fade(t.seq, 999_999, t.tone_region, "in", "linear")),
        "region.set_fade_shape: `lane` names no track of that sequence"
    );
    assert!(
        s.co_producer_preview(vec![fade(other_seq, t.tone, t.tone_region, "in", "linear")])
            .is_err()
    );
    // A sound: only a Built-in Synth's.
    let mixer = s.mixer().unwrap();
    let strip = mixer.strips[0].id;
    for bad in [999_999, strip.max(1) + 10_000] {
        assert!(
            s.co_producer_preview(vec![sound(bad, "pad")]).is_err(),
            "dest {bad}"
        );
    }
    // An output: only an audio track's, and only an output the song has.
    assert_eq!(
        refused(output(t.seq, t.instrument, None)),
        "Only an audio track has an output."
    );
    assert_eq!(
        refused(output(t.seq, t.tone, Some(999_999))),
        "There is no output 999999."
    );
    assert!(
        s.co_producer_preview(vec![output(t.seq, 999_999, None)])
            .is_err()
    );
    assert!(
        s.co_producer_preview(vec![output(other_seq, t.tone, None)])
            .is_err()
    );
    // Selection forms and wrong types never reach the clone.
    for plan in [
        planned(
            "region.set_fade_shape",
            vec![text("fade", "in"), text("shape", "linear")],
        ),
        planned("lane.set_output", vec![int("output", 0)]),
        planned(
            "destination.set_sound",
            vec![text("dest", "1"), text("sound", "pad")],
        ),
        planned(
            "destination.set_sound",
            vec![int("dest", 1), text("sound", &"x".repeat(70_000))],
        ),
        planned(
            "lane.set_output",
            vec![
                int("seq", t.seq),
                int("lane", t.tone),
                text("output", "a\0b"),
            ],
        ),
        planned("automation.release_all", vec![]),
    ] {
        assert!(
            s.co_producer_preview(vec![plan.clone()]).is_err(),
            "{plan:?}"
        );
    }
    assert_eq!(untouched(s), before, "a refused preview changed something");
    s.co_producer_end().unwrap();
    s.close().unwrap();
}

/// The preview's synth sounds are the window's: every sound, by title.
#[test]
fn every_synth_sound_previews_by_its_title() {
    let _g = serial();
    let t = song();
    let s = &t.s;
    let dest = synth(s);
    s.co_producer_begin("try sounds".into()).unwrap();
    for (name, title) in [
        ("bass", "Bass"),
        ("pad", "Pad"),
        ("lead", "Lead"),
        ("drums", "Drums"),
    ] {
        let p = s.co_producer_preview(vec![sound(dest, name)]).unwrap();
        assert_eq!(
            items(&p),
            vec![(
                "Built-in Synth".to_string(),
                format!("sound Piano → {title}"),
                true
            )]
        );
    }
    assert!(
        s.co_producer_preview(vec![sound(dest, "piano")])
            .unwrap()
            .items
            .is_empty()
    );
    s.co_producer_end().unwrap();
    s.close().unwrap();
    let _: PathBuf = t.dir.path().to_path_buf();
}
