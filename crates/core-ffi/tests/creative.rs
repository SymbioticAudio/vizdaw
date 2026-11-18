//! Co-Producer's automation writes, their previews, the yes and the reads
//! (plan 8e Task 3; Rulings 1–4): `automation.write` builds the model's own
//! `WriteAutomation` from values in the target's units, its preview lists
//! exactly what its apply does, the target's strip decides who says yes,
//! odd targets are refused at preview, the reads change nothing, and every
//! input is hostile. No command writes notes (owner, 2026-11-16).
//!
//! Every test that makes a song runs in a child with a temp home: its
//! inserts load the test gain plug-in. Parameter values come from the fake
//! `PluginLookup`. Offline only.

use core_control::registry::NULL_TEXT;
use core_host::PluginKind;
use core_host::bundled::MULTIQ_CLAP_ID;
use core_host::catalog::Catalog;
use core_model::automation::{AutomationTarget, fader_db, fader_norm, pan_norm};
use core_model::command::{AutomationWrite, Command};
use core_model::document::Document;
use core_model::history::History;
use core_model::ids::{PluginInstanceId, SeqId, StripId};
use core_model::mixer::{PluginFormat, PluginRef};
use core_model::time::Tick;
use std::sync::{Arc, Mutex, MutexGuard};
use vizdaw_ffi::bundled::{debug_clear_multiq, debug_set_multiq};
use vizdaw_ffi::co_producer::{CoProducerApproval, PlannedRow, PreviewRow, plan_of, preview_items};
use vizdaw_ffi::control::{ControlArg, ControlResult, ControlValue, result_json};
use vizdaw_ffi::creative::{
    CoProducerCurveRow, CoProducerNotesRow, CurvePointRow, MAX_READ, NoteRow, commands_for,
};
use vizdaw_ffi::edit::{EditCommand, EventEdit, LaneEdit, SequenceEdit};
use vizdaw_ffi::host::{force_offline, reset_host};
use vizdaw_ffi::input::upscale_velocity;
use vizdaw_ffi::mixer_chores::{PluginLookup, resolve_plugin};
use vizdaw_ffi::{
    EventKindRow, EventRow, ParamRow, PluginRefRow, Session, StripKindRow, StripRow, TrackKindRow,
    VizError, set_default_eq, set_new_channel_eq,
};

static SERIAL: Mutex<()> = Mutex::new(());

const GAIN_ID: &str = "vizdaw.test.gain";
const STAMP: &str = "2026-11-17 10-00";
const CHANGED: &str = "The song changed since the preview; preview again.";
const BAR: u64 = 61_440;

/// One test at a time; offline; a catalog holding only the test plug-ins;
/// the fake Multi-Q. Test builds keep the support folder in a per-process
/// temporary folder (`recovery::support_dir`).
fn serial() -> MutexGuard<'static, ()> {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    force_offline();
    vizdaw_ffi::host::set_offline_rate(48_000.0);
    reset_host();
    vizdaw_ffi::catalog::with(|c| *c = Catalog::new());
    vizdaw_ffi::catalog::add_test_bundle(&test_plugins::bundle()).unwrap();
    set_new_channel_eq(false);
    set_default_eq(None);
    debug_clear_multiq();
    debug_set_multiq(Some(PluginRef {
        format: PluginFormat::Clap,
        id: MULTIQ_CLAP_ID.into(),
        name: "Multi-Q".into(),
        vendor: String::new(),
        version: "0.10.9".into(),
    }));
    g
}

/// Runs `test` again alone in a child of this test binary whose `HOME`
/// and `CFFIXED_USER_HOME` are a temp folder; answers whether the caller
/// is that child (and so runs the body).
fn in_temp_home(test: &str) -> bool {
    const MARK: &str = "VIZDAW_TEST_TEMP_HOME";
    if let Some(home) = std::env::var_os(MARK) {
        assert_eq!(std::env::var_os("HOME"), Some(home.clone()), "HOME");
        assert_eq!(
            std::env::var_os("CFFIXED_USER_HOME"),
            Some(home),
            "CFFIXED_USER_HOME"
        );
        return true;
    }
    let home = tempfile::tempdir().unwrap();
    let out = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            test,
            "--exact",
            "--include-ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("HOME", home.path())
        .env("CFFIXED_USER_HOME", home.path())
        .env(MARK, home.path())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    print!("{stdout}");
    eprint!("{}", String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success(), "{test} failed in its temp home");
    assert!(
        stdout.contains("test result: ok. 1 passed"),
        "the child did not run {test} exactly once"
    );
    false
}

fn int(name: &str, value: i64) -> ControlArg {
    ControlArg {
        name: name.into(),
        value: ControlValue::Int { value },
    }
}

fn id(name: &str, value: u64) -> ControlArg {
    int(name, value as i64)
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

/// `automation.write` over bars 1–2 of sequence `seq`.
fn write(seq: u64, target: &str, points: &str) -> Vec<ControlArg> {
    vec![
        id("seq", seq),
        text("target", target),
        int("start", 0),
        id("end", BAR),
        text("points", points),
    ]
}

fn doc(s: &Session) -> Document {
    core_model::file::from_json(&s.debug_document_json()).unwrap()
}

/// The document without plug-in state blobs, which a plug-in that has
/// just started running stores on its own.
fn shape(s: &Session) -> serde_json::Value {
    fn strip(v: &mut serde_json::Value) {
        match v {
            serde_json::Value::Object(m) => {
                m.remove("state");
                m.values_mut().for_each(strip);
            }
            serde_json::Value::Array(a) => a.iter_mut().for_each(strip),
            _ => {}
        }
    }
    let mut v: serde_json::Value = serde_json::from_str(&s.debug_document_json()).unwrap();
    strip(&mut v);
    v
}

fn strips(s: &Session) -> Vec<StripRow> {
    s.mixer().unwrap().strips
}

fn strip_id(s: &Session, name: &str) -> u64 {
    strips(s)
        .into_iter()
        .find(|x| x.name == name)
        .unwrap_or_else(|| panic!("no strip {name}"))
        .id
}

fn message<T: std::fmt::Debug>(r: Result<T, VizError>) -> String {
    match r {
        Err(e) => e.to_string(),
        Ok(v) => panic!("expected a refusal, got {v:?}"),
    }
}

fn items(p: &PreviewRow) -> Vec<(String, String, bool)> {
    p.items
        .iter()
        .map(|i| (i.thing.clone(), i.change.clone(), i.composers_work))
        .collect()
}

fn catalog_ref(id: &str) -> PluginRefRow {
    PluginRefRow::from(&vizdaw_ffi::catalog::with(|c| {
        c.preferred(PluginFormat::Clap, id).unwrap().plugin.clone()
    }))
}

fn lane_ids(s: &Session, seq: u64) -> Vec<u64> {
    s.lanes(seq).unwrap().into_iter().map(|l| l.id).collect()
}

/// A new track of `kind` named `name`; its ID.
fn track(s: &Session, seq: u64, kind: TrackKindRow, name: &str) -> u64 {
    let before = lane_ids(s, seq);
    s.new_track(seq, kind, None).unwrap();
    let lane = lane_ids(s, seq)
        .into_iter()
        .find(|l| !before.contains(l))
        .unwrap();
    s.edit(
        EditCommand::Lane {
            edit: LaneEdit::Rename {
                seq,
                lane,
                name: name.into(),
            },
        },
        None,
    )
    .unwrap();
    lane
}

/// `(tick, pitch, 7-bit velocity)` notes into `lane`, each 480 long.
fn add_notes(s: &Session, seq: u64, lane: u64, notes: &[(i64, u8, u8)]) {
    let events = notes
        .iter()
        .map(|(t, p, v)| {
            let mut row = EventRow::blank(0, *t, EventKindRow::Note);
            row.pitch = *p;
            row.velocity = upscale_velocity(*v);
            row.duration = 480;
            row
        })
        .collect();
    s.edit(
        EditCommand::Event {
            edit: EventEdit::Insert { seq, lane, events },
        },
        None,
    )
    .unwrap();
}

/// Piano's notes, inserted out of order: six starts in bars 1–4, two
/// pitches at each.
fn piano_notes() -> Vec<(i64, u8, u8)> {
    (0..12)
        .rev()
        .map(|i: i64| ((i / 2) * 40_960, 72 - (i % 2) as u8 * 12, 10 + i as u8 * 9))
        .collect()
}

/// Resolves through the Plugin Manager's rules; answers Cutoff (0–1,
/// 0.5) and a stepped Mode (0–3) for `slot` while `running`.
struct Fake {
    slot: u64,
    running: bool,
}

fn param(id: u64, name: &str, max: f64, value: f64, stepped: bool) -> ParamRow {
    ParamRow {
        id,
        name: name.into(),
        module: String::new(),
        min: 0.0,
        max,
        default: value,
        value,
        stepped,
        automatable: true,
        hidden: false,
    }
}

impl PluginLookup for Fake {
    fn resolve(&self, text: &str, want: PluginKind) -> Result<PluginRef, VizError> {
        resolve_plugin(text, want)
    }
    fn params(&self, slot: PluginInstanceId) -> Option<Vec<ParamRow>> {
        (self.running && slot.0 == self.slot).then(|| {
            vec![
                param(0, "Cutoff", 1.0, 0.5, false),
                param(1, "Mode", 3.0, 1.0, true),
            ]
        })
    }
}

/// The composer's tracks Piano (instrument, 12 notes in bars 1–4) and Vox
/// (audio), their strips Piano and Vox with a test gain insert on Vox;
/// then Co-Producer's bus Drums, and Co-Producer's test gain insert on
/// Vox, with a yes. Saved.
struct Song {
    s: Arc<Session>,
    _dir: tempfile::TempDir,
    seq: u64,
    piano_lane: u64,
    vox_lane: u64,
    piano: u64,
    vox: u64,
    drums: u64,
    master: u64,
    /// Co-Producer's insert on Vox.
    ours: u64,
}

fn song() -> Song {
    let dir = tempfile::tempdir().unwrap();
    let s = Session::new_empty().unwrap();
    // A new song's first sequence is 0, which `automation.write` and the
    // reads refuse (Task 1's `seq` is at least 1): the song plays a second.
    let first = s.snapshot().unwrap().sequences[0].id;
    s.edit(
        EditCommand::Sequence {
            edit: SequenceEdit::New {
                name: "Song".into(),
            },
        },
        None,
    )
    .unwrap();
    let seq = s
        .snapshot()
        .unwrap()
        .sequences
        .iter()
        .map(|q| q.id)
        .find(|q| *q != first)
        .unwrap();
    assert!(seq >= 1);
    let old: Vec<u64> = strips(&s).iter().map(|x| x.id).collect();
    let piano_lane = track(&s, seq, TrackKindRow::Instrument, "Piano");
    let vox_lane = track(&s, seq, TrackKindRow::Audio, "Vox");
    let made: Vec<StripRow> = strips(&s)
        .into_iter()
        .filter(|x| !old.contains(&x.id) || x.kind == StripKindRow::BuiltinSynth)
        .collect();
    let piano = made
        .iter()
        .find(|x| x.kind == StripKindRow::BuiltinSynth)
        .unwrap()
        .id;
    let vox = made
        .iter()
        .find(|x| matches!(x.kind, StripKindRow::AudioPath { .. }))
        .unwrap()
        .id;
    s.rename_strip(piano, "Piano".into()).unwrap();
    s.rename_strip(vox, "Vox".into()).unwrap();
    s.add_insert(vox, None, catalog_ref(GAIN_ID)).unwrap();
    add_notes(&s, seq, piano_lane, &piano_notes());
    s.save_as(dir.path().join("P.vizdaw").display().to_string())
        .unwrap();
    s.co_producer_begin("add a drum bus".into()).unwrap();
    s.co_producer_enter(STAMP.into()).unwrap();
    s.add_bus("Drums".into()).unwrap();
    s.co_producer_leave();
    s.co_producer_end().unwrap();
    let drums = strip_id(&s, "Drums (Co-Producer)");
    // Co-Producer's insert on the composer's Vox, with a yes.
    let inserts: Vec<u64> = vox_inserts(&s, vox);
    s.co_producer_begin("add a gain".into()).unwrap();
    let plan = vec![planned(
        "mixer.add_insert",
        vec![id("strip", vox), text("plugin", "clap:vizdaw.test.gain")],
    )];
    let p = s.co_producer_preview(plan.clone()).unwrap();
    assert!(p.needs_yes, "{p:?}");
    s.co_producer_enter(STAMP.into()).unwrap();
    s.co_producer_take(p.token, plan.clone(), CoProducerApproval::Sheet)
        .unwrap();
    s.run_mixer(plan[0].id.clone(), plan[0].args.clone())
        .unwrap();
    s.co_producer_leave();
    s.co_producer_end().unwrap();
    let ours = vox_inserts(&s, vox)
        .into_iter()
        .find(|x| !inserts.contains(x))
        .unwrap();
    let master = strips(&s)
        .into_iter()
        .find(|x| x.kind == StripKindRow::Master)
        .unwrap()
        .id;
    Song {
        s,
        _dir: dir,
        seq,
        piano_lane,
        vox_lane,
        piano,
        vox,
        drums,
        master,
        ours,
    }
}

fn vox_inserts(s: &Session, vox: u64) -> Vec<u64> {
    strips(s)
        .into_iter()
        .find(|x| x.id == vox)
        .unwrap()
        .inserts
        .iter()
        .map(|i| i.id)
        .collect()
}

/// One Co-Producer call, as the app's apply makes it: the take with the
/// sheet's Allow, then each command through `run_creative`.
fn take_and_run(s: &Session, token: String, plan: &[PlannedRow]) -> Result<(), VizError> {
    s.co_producer_enter(STAMP.into()).unwrap();
    let r = s
        .co_producer_take(token, plan.to_vec(), CoProducerApproval::Sheet)
        .and_then(|()| {
            for p in plan {
                s.run_creative(p.id.clone(), p.args.clone())?;
            }
            Ok(())
        });
    s.co_producer_leave();
    r
}

/// `plan` previewed on `s`'s document with `host` as the plug-in host.
fn preview_with(
    s: &Session,
    plan: Vec<PlannedRow>,
    host: &dyn PluginLookup,
) -> Result<Vec<(String, String, bool)>, VizError> {
    let plan = plan_of(&plan)?;
    let h = History::new(doc(s));
    Ok(preview_items(&h, None, &plan, host)?
        .into_iter()
        .map(|i| (i.thing, i.change, i.composers_work))
        .collect())
}

fn built(
    d: &Document,
    id: &str,
    args: Vec<ControlArg>,
    host: &dyn PluginLookup,
) -> Result<Vec<Command>, VizError> {
    let args = vizdaw_ffi::co_producer::validated(id, &args)?;
    commands_for(d, id, &args, host)
}

#[test]
fn creative_commands_build_the_models_own() {
    if !in_temp_home("creative_commands_build_the_models_own") {
        return;
    }
    let _g = serial();
    let t = song();
    let d = doc(&t.s);
    let fake = Fake {
        slot: t.ours,
        running: true,
    };
    let seq = SeqId(t.seq);
    assert_eq!(
        built(
            &d,
            "automation.write",
            write(t.seq, &format!("fader:{}", t.drums), "0:-6 61440:0"),
            &fake
        )
        .unwrap(),
        vec![Command::WriteAutomation(AutomationWrite {
            seq,
            target: AutomationTarget::StripFader(StripId(t.drums)),
            start: Tick(0),
            end: Tick(BAR as i64),
            points: vec![(Tick(0), fader_norm(-6.0)), (Tick(61_440), fader_norm(0.0))],
            lane_name: "Drums (Co-Producer) fader Automation".into(),
        })]
    );
    let Command::WriteAutomation(w) = &built(
        &d,
        "automation.write",
        write(t.seq, &format!("pan:{}", t.piano), "15360:-30"),
        &fake,
    )
    .unwrap()[0] else {
        panic!("not a write")
    };
    assert_eq!(w.points, vec![(Tick(15_360), pan_norm(-0.3))]);
    assert_eq!(w.lane_name, "Piano pan Automation");
    let Command::WriteAutomation(w) = &built(
        &d,
        "automation.write",
        write(t.seq, &format!("param:{}:0", t.ours), "0:0.25 30720:1"),
        &fake,
    )
    .unwrap()[0] else {
        panic!("not a write")
    };
    assert_eq!(w.points, vec![(Tick(0), 0.25), (Tick(30_720), 1.0)]);
    assert_eq!(
        w.target,
        AutomationTarget::PluginParam(PluginInstanceId(t.ours), core_model::mixer::ParamId(0))
    );
    assert_eq!(w.lane_name, "Vox Cutoff Automation");
    // Mode is 0–3: 3 is stored as 1.
    let Command::WriteAutomation(w) = &built(
        &d,
        "automation.write",
        write(t.seq, &format!("param:{}:1", t.ours), "0:3"),
        &fake,
    )
    .unwrap()[0] else {
        panic!("not a write")
    };
    assert_eq!(w.points, vec![(Tick(0), 1.0)]);
    // Sends, mute and solo; an empty list clears.
    let Command::WriteAutomation(w) = &built(
        &d,
        "automation.write",
        write(t.seq, &format!("mute:{}", t.vox), "0:1 100:0"),
        &fake,
    )
    .unwrap()[0] else {
        panic!("not a write")
    };
    assert_eq!(w.points, vec![(Tick(0), 1.0), (Tick(100), 0.0)]);
    let Command::WriteAutomation(w) = &built(
        &d,
        "automation.write",
        write(t.seq, &format!("solo:{}", t.vox), ""),
        &fake,
    )
    .unwrap()[0] else {
        panic!("not a write")
    };
    assert!(w.points.is_empty());
    assert_eq!(w.lane_name, "Vox solo Automation");
    // No command writes notes (owner, 2026-11-16).
    for gone in ["midi.new_part", "midi.add_notes"] {
        assert!(built(&d, gone, vec![], &fake).is_err(), "{gone}");
        let empty: Vec<(String, core_control::registry::ArgValue)> = Vec::new();
        assert!(commands_for(&d, gone, &empty, &fake).is_err(), "{gone}");
    }
    t.s.close().unwrap();
}

#[test]
fn creative_writes_ask_for_the_composers_work() {
    if !in_temp_home("creative_writes_ask_for_the_composers_work") {
        return;
    }
    let _g = serial();
    let t = song();
    let s = &t.s;
    s.co_producer_begin("ride the levels".into()).unwrap();
    let drums = planned(
        "automation.write",
        write(t.seq, &format!("fader:{}", t.drums), "0:-6 61440:0"),
    );
    let piano = planned(
        "automation.write",
        write(t.seq, &format!("fader:{}", t.piano), "0:-6 61440:0"),
    );
    // Free: Co-Producer's own bus.
    let p = s.co_producer_preview(vec![drums.clone()]).unwrap();
    assert!(!p.needs_yes, "{p:?}");
    assert!(
        items(&p)
            .iter()
            .any(|(thing, change, _)| thing == "Strip Drums (Co-Producer)"
                && change == "fader automation 0 points → 2 points, bars 1–2"),
        "{p:?}"
    );
    // Asks: the composer's Piano.
    let p = s.co_producer_preview(vec![piano.clone()]).unwrap();
    assert!(p.needs_yes, "{p:?}");
    assert!(
        items(&p).contains(&(
            "Strip Piano".into(),
            "fader automation 0 points → 2 points, bars 1–2".into(),
            true
        )),
        "{p:?}"
    );
    // Asks: Co-Producer's insert on the composer's Vox.
    let fake = Fake {
        slot: t.ours,
        running: true,
    };
    let got = preview_with(
        s,
        vec![planned(
            "automation.write",
            write(t.seq, &format!("param:{}:0", t.ours), "0:0.25 61440:1"),
        )],
        &fake,
    )
    .unwrap();
    assert!(got.iter().any(|i| i.2), "{got:?}");
    assert!(
        got.iter().any(|(thing, change, yes)| thing == "Strip Vox"
            && change.contains("Cutoff automation 0 points → 2 points, bars 1–2")
            && *yes),
        "{got:?}"
    );
    // A mixed plan asks, for exactly the Piano item.
    let p = s.co_producer_preview(vec![drums.clone(), piano]).unwrap();
    assert!(p.needs_yes, "{p:?}");
    let asking: Vec<String> = p
        .items
        .iter()
        .filter(|i| i.composers_work)
        .map(|i| i.thing.clone())
        .collect();
    assert_eq!(asking, vec!["Strip Piano".to_string()], "{p:?}");
    s.co_producer_end().unwrap();
    // The composer's own points on Co-Producer's bus: a write over them
    // asks (Task 2's fix), though the strip is Co-Producer's.
    s.run_creative(drums.id.clone(), drums.args.clone())
        .unwrap();
    s.co_producer_begin("ride the drums".into()).unwrap();
    let over = planned(
        "automation.write",
        write(t.seq, &format!("fader:{}", t.drums), "0:-3"),
    );
    let p = s.co_producer_preview(vec![over]).unwrap();
    assert!(p.needs_yes, "{p:?}");
    assert!(
        items(&p)
            .iter()
            .any(|(thing, _, yes)| thing == "Strip Drums (Co-Producer)" && *yes),
        "{p:?}"
    );
    s.co_producer_end().unwrap();
    t.s.close().unwrap();
}

#[test]
fn creative_previews_match_what_the_apply_does() {
    if !in_temp_home("creative_previews_match_what_the_apply_does") {
        return;
    }
    let _g = serial();
    let t = song();
    let s = &t.s;
    s.co_producer_begin("shape the mix".into()).unwrap();
    let plans = [
        write(
            t.seq,
            &format!("fader:{}", t.piano),
            "0:-6 30720:-12 61440:0",
        ),
        write(t.seq, &format!("pan:{}", t.vox), "0:-100 61440:100"),
        write(t.seq, &format!("mute:{}", t.drums), "0:0 15360:1"),
        write(t.seq, &format!("fader:{}", t.piano), "15360:-3"),
        write(t.seq, &format!("fader:{}", t.piano), ""),
    ];
    for args in plans {
        let plan = vec![planned("automation.write", args)];
        // A hand edit between the preview and the take.
        let p = s.co_producer_preview(plan.clone()).unwrap();
        s.set_gain(t.master, -1.0).unwrap();
        let before = shape(s);
        let e = take_and_run(s, p.token, &plan).unwrap_err();
        assert_eq!(e.to_string(), CHANGED, "{plan:?}");
        assert_eq!(shape(s), before, "{plan:?}");
        // Previewed again, it does exactly what it said.
        let p = s.co_producer_preview(plan.clone()).unwrap();
        assert!(!p.items.is_empty(), "{plan:?}");
        let start = doc(s);
        take_and_run(s, p.token, &plan).unwrap();
        let said: Vec<(String, String)> =
            core_model::diff::describe_with(&start, &doc(s), &|_, _| None)
                .into_iter()
                .map(|i| (i.thing, i.change))
                .collect();
        let shown: Vec<(String, String)> =
            p.items.into_iter().map(|i| (i.thing, i.change)).collect();
        assert_eq!(said, shown, "{plan:?}");
    }
    let r = s.co_producer_end().unwrap().unwrap();
    assert!(r.starts_with("Co-Producer: "), "{r}");
    t.s.close().unwrap();
}

#[test]
fn odd_creative_targets_are_refused_at_preview() {
    if !in_temp_home("odd_creative_targets_are_refused_at_preview") {
        return;
    }
    let _g = serial();
    let t = song();
    let s = &t.s;
    s.co_producer_begin("try odd things".into()).unwrap();
    let before = shape(s);
    let version = s.snapshot().unwrap().version;
    let refused = |args: Vec<ControlArg>| {
        message(s.co_producer_preview(vec![planned("automation.write", args)]))
    };
    assert_eq!(
        refused(write(t.seq, &format!("fader:{}", t.piano), "0:20")),
        "fader is −144 to 12."
    );
    assert_eq!(
        refused(write(t.seq, &format!("send:{}:1", t.piano), "0:20")),
        format!("This song has no send:{}:1.", t.piano)
    );
    assert_eq!(
        refused(write(t.seq, &format!("pan:{}", t.piano), "0:101")),
        "pan is −100 to 100."
    );
    assert_eq!(
        refused(write(t.seq, &format!("mute:{}", t.piano), "0:0.5")),
        "mute is 0 to 1."
    );
    assert_eq!(
        refused(write(t.seq, &format!("solo:{}", t.piano), "0:2")),
        "solo is 0 to 1."
    );
    let mode = |host: &dyn PluginLookup, value: &str| {
        message(preview_with(
            s,
            vec![planned(
                "automation.write",
                write(t.seq, &format!("param:{}:1", t.ours), &format!("0:{value}")),
            )],
            host,
        ))
    };
    let running = Fake {
        slot: t.ours,
        running: true,
    };
    assert_eq!(mode(&running, "0.5"), "Mode takes whole values.");
    assert_eq!(mode(&running, "4"), "Mode is 0 to 3.");
    assert_eq!(
        mode(
            &Fake {
                slot: t.ours,
                running: false
            },
            "1"
        ),
        format!(
            "The plug-in in slot {} is not running yet; read plugin_params again in a moment.",
            t.ours
        )
    );
    assert_eq!(
        message(preview_with(
            s,
            vec![planned(
                "automation.write",
                write(t.seq, &format!("param:{}:9", t.ours), "0:1"),
            )],
            &running,
        )),
        format!("Slot {} has no parameter 9.", t.ours)
    );
    assert_eq!(
        refused(write(t.seq, "fader:999", "0:0")),
        "This song has no fader:999."
    );
    assert_eq!(
        refused(write(t.seq, "param:999:0", "0:0")),
        "This song has no param:999:0."
    );
    assert_eq!(
        refused(write(99, &format!("fader:{}", t.piano), "0:0")),
        "This song has no sequence 99."
    );
    // A plan with one refused write runs nothing.
    let e = s.co_producer_preview(vec![
        planned(
            "automation.write",
            write(t.seq, &format!("fader:{}", t.drums), "0:0"),
        ),
        planned(
            "automation.write",
            write(t.seq, &format!("fader:{}", t.piano), "0:20"),
        ),
    ]);
    assert_eq!(message(e), "fader is −144 to 12.");
    assert_eq!(shape(s), before);
    assert_eq!(s.snapshot().unwrap().version, version);
    // So does `run_creative`.
    assert_eq!(
        message(s.run_creative(
            "automation.write".into(),
            write(t.seq, &format!("fader:{}", t.piano), "0:20"),
        )),
        "fader is −144 to 12."
    );
    assert_eq!(shape(s), before);
    s.co_producer_end().unwrap();
    // Outside a request a write's undo label is its title; in one it is
    // the request's one step.
    let fader = write(t.seq, &format!("fader:{}", t.piano), "0:-6");
    s.run_creative("automation.write".into(), fader.clone())
        .unwrap();
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Write Automation")
    );
    let r = s.co_producer_begin("fade the piano".into()).unwrap();
    s.co_producer_enter(STAMP.into()).unwrap();
    s.run_creative("automation.write".into(), fader.clone())
        .unwrap();
    s.run_creative(
        "automation.write".into(),
        write(t.seq, &format!("pan:{}", t.piano), "0:10"),
    )
    .unwrap();
    s.co_producer_leave();
    assert_eq!(s.co_producer_end().unwrap(), Some(r.label.clone()));
    assert_eq!(r.label, "Co-Producer: Fade the Piano");
    assert_eq!(s.snapshot().unwrap().undo_label, Some(r.label));
    s.undo().unwrap();
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Write Automation")
    );
    // Busy while an edit is open.
    s.begin_edit("Drag".into()).unwrap();
    assert!(matches!(
        s.run_creative("automation.write".into(), fader),
        Err(VizError::Busy { .. })
    ));
    s.end_edit().unwrap();
    t.s.close().unwrap();
}

#[test]
fn reads_change_nothing() {
    if !in_temp_home("reads_change_nothing") {
        return;
    }
    let _g = serial();
    let t = song();
    let s = &t.s;
    let version = s.snapshot().unwrap().version;
    let row = s
        .co_producer_midi_notes(t.seq, t.piano_lane, None, None)
        .unwrap();
    let mut want: Vec<NoteRow> = piano_notes()
        .into_iter()
        .map(|(tick, pitch, velocity)| NoteRow {
            start: tick as u64,
            pitch,
            length: 480,
            velocity,
        })
        .collect();
    want.sort_by_key(|n| (n.start, n.pitch));
    assert_eq!(
        row,
        CoProducerNotesRow {
            seq: t.seq,
            lane: t.piano_lane,
            name: "Piano".into(),
            notes: want.clone(),
            truncated: false,
        }
    );
    // A range gives the notes starting inside it.
    let row = s
        .co_producer_midi_notes(t.seq, t.piano_lane, Some(40_960), Some(122_880))
        .unwrap();
    let inside: Vec<NoteRow> = want
        .iter()
        .copied()
        .filter(|n| (40_960..122_880).contains(&n.start))
        .collect();
    assert_eq!(inside.len(), 4);
    assert_eq!(row.notes, inside);
    // The automation read, after an applied write.
    s.run_creative(
        "automation.write".into(),
        write(
            t.seq,
            &format!("fader:{}", t.piano),
            "0:-6 30720:-144 61440:12",
        ),
    )
    .unwrap();
    let version = {
        let now = s.snapshot().unwrap().version;
        assert!(now > version);
        now
    };
    let c = s
        .co_producer_automation(t.seq, format!("fader:{}", t.piano), 0, 2 * BAR)
        .unwrap();
    assert_eq!(c.target, format!("fader:{}", t.piano));
    assert_eq!(c.target_name, "Piano fader");
    assert_eq!((c.unit.as_str(), c.min, c.max), ("dB", -144.0, 12.0));
    assert_eq!((c.start_bar, c.end_bar), (1, 3));
    assert!(!c.truncated);
    let ticks: Vec<u64> = c.points.iter().map(|p| p.tick).collect();
    assert_eq!(ticks, vec![0, 30_720, 61_440]);
    let values: Vec<f64> = c.points.iter().map(|p| p.value).collect();
    assert!((values[0] - -6.0).abs() < 1e-9, "{values:?}");
    assert_eq!(values[1], -144.0);
    assert!((values[2] - 12.0).abs() < 1e-9, "{values:?}");
    assert_eq!(values[0], fader_db(fader_norm(-6.0)));
    // A part of the range.
    let c = s
        .co_producer_automation(t.seq, format!("fader:{}", t.piano), 1, BAR)
        .unwrap();
    assert_eq!(c.points.len(), 2);
    // Other units.
    let c = s
        .co_producer_automation(t.seq, format!("pan:{}", t.vox), 0, BAR)
        .unwrap();
    assert_eq!((c.unit.as_str(), c.min, c.max), ("%", -100.0, 100.0));
    assert!(c.points.is_empty());
    let c = s
        .co_producer_automation(t.seq, format!("mute:{}", t.vox), 0, BAR)
        .unwrap();
    assert_eq!((c.unit.as_str(), c.min, c.max), ("on/off", 0.0, 1.0));
    assert_eq!(c.target_name, "Vox mute");
    // Neither read changed anything.
    assert_eq!(s.snapshot().unwrap().version, version);
    // A track of 5000 notes gives the first 4096.
    let big = track(s, t.seq, TrackKindRow::Midi, "Big");
    let many: Vec<(i64, u8, u8)> = (0..5000).map(|i| (i * 10, 60, 100)).collect();
    add_notes(s, t.seq, big, &many);
    let version = s.snapshot().unwrap().version;
    let row = s.co_producer_midi_notes(t.seq, big, None, None).unwrap();
    assert_eq!(row.notes.len(), MAX_READ);
    assert!(row.truncated);
    assert_eq!(row.notes[4095].start, 40_950);
    // Refusals.
    assert_eq!(
        message(s.co_producer_midi_notes(t.seq, t.vox_lane, None, None)),
        "Track Vox is not a MIDI or instrument track."
    );
    let other = s
        .snapshot()
        .unwrap()
        .sequences
        .iter()
        .map(|q| q.id)
        .find(|q| *q != t.seq)
        .unwrap();
    let theirs = track(s, other, TrackKindRow::Midi, "Elsewhere");
    assert_eq!(
        message(s.co_producer_midi_notes(t.seq, theirs, None, None)),
        format!("Track {theirs} is not in sequence {}.", t.seq)
    );
    let version = {
        assert!(s.snapshot().unwrap().version > version);
        s.snapshot().unwrap().version
    };
    assert_eq!(
        message(s.co_producer_midi_notes(t.seq, 999_999, None, None)),
        format!("Track 999999 is not in sequence {}.", t.seq)
    );
    assert_eq!(
        message(s.co_producer_midi_notes(99, t.piano_lane, None, None)),
        "This song has no sequence 99."
    );
    for (a, b) in [
        (Some(10), None),
        (None, Some(10)),
        (Some(10), Some(10)),
        (Some(20), Some(10)),
        (Some(0), Some(u64::MAX)),
        (Some(u64::MAX - 1), Some(u64::MAX)),
    ] {
        assert!(
            s.co_producer_midi_notes(t.seq, t.piano_lane, a, b).is_err(),
            "{a:?} {b:?}"
        );
    }
    for (seq, lane) in [(u64::MAX, t.piano_lane), (t.seq, u64::MAX), (0, 0)] {
        assert!(s.co_producer_midi_notes(seq, lane, None, None).is_err());
    }
    let long = "x".repeat(70_000);
    for target in [
        "fader:999",
        "bogus",
        "",
        "fader:\0",
        long.as_str(),
        "send:1:0",
        "param:1",
    ] {
        let m = message(s.co_producer_automation(t.seq, target.into(), 0, BAR));
        assert!(m.chars().count() < 200, "{}", m.len());
    }
    assert_eq!(
        message(s.co_producer_automation(t.seq, "fader:999".into(), 0, BAR)),
        "This song has no fader:999."
    );
    for (seq, start, end) in [
        (u64::MAX, 0, BAR),
        (t.seq, 0, u64::MAX),
        (t.seq, u64::MAX - 1, u64::MAX),
        (t.seq, BAR, BAR),
        (t.seq, BAR, 0),
    ] {
        assert!(
            s.co_producer_automation(seq, format!("fader:{}", t.piano), start, end)
                .is_err(),
            "{seq} {start} {end}"
        );
    }
    // A parameter needs its plug-in running; a test gain has no param 999.
    assert!(
        s.co_producer_automation(t.seq, format!("param:{}:999", t.ours), 0, BAR)
            .is_err()
    );
    assert_eq!(s.snapshot().unwrap().version, version);
    t.s.close().unwrap();
}

#[test]
fn run_creative_refuses_hostile_input() {
    if !in_temp_home("run_creative_refuses_hostile_input") {
        return;
    }
    let _g = serial();
    let t = song();
    let s = &t.s;
    let before = shape(s);
    let version = s.snapshot().unwrap().version;
    let fader = format!("fader:{}", t.piano);
    let long = "x".repeat(70_000);
    let mut cases: Vec<(String, Vec<ControlArg>)> = vec![
        ("midi.new_part".into(), write(t.seq, &fader, "0:0")),
        ("midi.add_notes".into(), write(t.seq, &fader, "0:0")),
        ("mixer.set_strip".into(), write(t.seq, &fader, "0:0")),
        ("bogus".into(), vec![]),
        (long.clone(), vec![]),
        ("automation.write\0".into(), write(t.seq, &fader, "0:0")),
    ];
    // A missing arg, each in turn.
    for drop in ["seq", "target", "start", "end", "points"] {
        cases.push((
            "automation.write".into(),
            write(t.seq, &fader, "0:0")
                .into_iter()
                .filter(|a| a.name != drop)
                .collect(),
        ));
    }
    let with = |name: &str, value: ControlArg| -> Vec<ControlArg> {
        let mut a: Vec<ControlArg> = write(t.seq, &fader, "0:0")
            .into_iter()
            .filter(|a| a.name != name)
            .collect();
        a.push(value);
        a
    };
    let points = "1:0 ".repeat(17_500);
    assert!(points.len() >= 70_000);
    for bad in [
        with("points", text("points", &points)),
        with("points", text("points", &long)),
        with("points", text("points", "0:0\0")),
        with("points", text("points", "0:NaN")),
        with("points", text("points", "5:0 4:0")),
        with("points", text("points", "70000:0")),
        with("points", int("points", 3)),
        with("seq", int("seq", -1)),
        with(
            "seq",
            ControlArg {
                name: "seq".into(),
                value: ControlValue::Int {
                    value: u64::MAX as i64,
                },
            },
        ),
        with("seq", int("seq", i64::MAX)),
        with("seq", text("seq", "1")),
        with("target", text("target", &long)),
        with("target", text("target", "fader:1\0")),
        with("target", text("target", &format!("fader:{}", u64::MAX))),
        with("target", text("target", NULL_TEXT)),
        with("end", int("end", 0)),
        with("end", int("end", i64::MAX)),
        with("start", int("start", -5)),
        with("seq", text("bogus", "1")),
    ] {
        cases.push(("automation.write".into(), bad));
    }
    let mut extra = write(t.seq, &fader, "0:0");
    extra.push(text("bogus", "1"));
    cases.push(("automation.write".into(), extra));
    let mut twice = write(t.seq, &fader, "0:0");
    twice.push(int("start", 0));
    cases.push(("automation.write".into(), twice));
    for (id, args) in cases {
        let m = message(s.run_creative(id.clone(), args.clone()));
        assert!(m.chars().count() < 400, "{} {}", id.len(), m.len());
        assert_eq!(shape(s), before, "{}", id.len());
    }
    assert_eq!(s.snapshot().unwrap().version, version);
    // A preview refuses them too, and no plan names a note write.
    for gone in ["midi.new_part", "midi.add_notes"] {
        assert!(
            s.co_producer_preview(vec![planned(gone, write(t.seq, &fader, "0:0"))])
                .is_err()
        );
    }
    t.s.close().unwrap();
}

#[test]
fn creative_results_json() {
    let j = result_json(&ControlResult::CoProducerMidiNotes {
        doc: "d1".into(),
        row: CoProducerNotesRow {
            seq: 1,
            lane: 4,
            name: "Piano".into(),
            notes: vec![
                NoteRow {
                    start: 0,
                    pitch: 60,
                    length: 480,
                    velocity: 100,
                },
                NoteRow {
                    start: 15_360,
                    pitch: 64,
                    length: 960,
                    velocity: 1,
                },
            ],
            truncated: true,
        },
    });
    assert_eq!(
        j,
        serde_json::json!({
            "doc": "d1", "seq": 1, "lane": 4, "name": "Piano",
            "notes": [[0, 60, 480, 100], [15360, 64, 960, 1]], "truncated": true,
        })
    );
    let j = result_json(&ControlResult::CoProducerAutomation {
        doc: "d1".into(),
        row: CoProducerCurveRow {
            target: "fader:3".into(),
            target_name: "Piano fader".into(),
            unit: "dB".into(),
            min: -144.0,
            max: 12.0,
            start_bar: 1,
            end_bar: 2,
            points: vec![
                CurvePointRow {
                    tick: 0,
                    value: -6.0,
                },
                CurvePointRow {
                    tick: 61_440,
                    value: 0.0,
                },
            ],
            truncated: false,
        },
    });
    assert_eq!(
        j,
        serde_json::json!({
            "doc": "d1", "target": "fader:3", "target_name": "Piano fader", "unit": "dB",
            "min": -144.0, "max": 12.0, "start_bar": 1, "end_bar": 2,
            "points": [[0, -6.0], [61440, 0.0]], "truncated": false,
        })
    );
}

#[test]
fn solo_automation_always_asks() {
    if !in_temp_home("solo_automation_always_asks") {
        return;
    }
    let _g = serial();
    let t = song();
    let s = &t.s;
    s.co_producer_begin("solo the drums".into()).unwrap();
    // On Co-Producer's own bus too: a solo silences the composer's tracks.
    let p = s
        .co_producer_preview(vec![planned(
            "automation.write",
            write(t.seq, &format!("solo:{}", t.drums), "0:1 61440:0"),
        )])
        .unwrap();
    assert!(p.needs_yes, "{p:?}");
    assert!(
        items(&p).contains(&(
            "Strip Drums (Co-Producer)".into(),
            "soloed: other tracks are silent while it's on".into(),
            true
        )),
        "{p:?}"
    );
    assert!(
        items(&p)
            .iter()
            .any(|(thing, change, yes)| thing == "Strip Drums (Co-Producer)"
                && change.starts_with("solo automation")
                && *yes),
        "{p:?}"
    );
    // A fader write on the same bus stays free.
    let p = s
        .co_producer_preview(vec![planned(
            "automation.write",
            write(t.seq, &format!("fader:{}", t.drums), "0:-6"),
        )])
        .unwrap();
    assert!(!p.needs_yes, "{p:?}");
    s.co_producer_end().unwrap();
    t.s.close().unwrap();
}

#[test]
fn automation_writes_in_a_new_songs_first_sequence() {
    if !in_temp_home("automation_writes_in_a_new_songs_first_sequence") {
        return;
    }
    let _g = serial();
    let s = Session::new_empty().unwrap();
    let seq = s.snapshot().unwrap().sequences[0].id;
    assert_eq!(seq, 0, "a new song's first sequence is 0");
    let lane = track(&s, seq, TrackKindRow::Midi, "Keys");
    let master = strips(&s)
        .into_iter()
        .find(|x| x.kind == StripKindRow::Master)
        .unwrap()
        .id;
    let fader = format!("fader:{master}");
    // Through a preview and the composer's yes, as the app applies it.
    s.co_producer_begin("fade out".into()).unwrap();
    let plan = vec![planned(
        "automation.write",
        write(seq, &fader, "0:0 61440:-144"),
    )];
    let p = s.co_producer_preview(plan.clone()).unwrap();
    assert!(p.needs_yes, "{p:?}");
    take_and_run(&s, p.token, &plan).unwrap();
    s.co_producer_end().unwrap();
    let c = s.co_producer_automation(seq, fader, 0, BAR).unwrap();
    let ticks: Vec<u64> = c.points.iter().map(|p| p.tick).collect();
    assert_eq!(ticks, vec![0, BAR]);
    assert_eq!(c.points[1].value, -144.0);
    // The first track's notes read too.
    let row = s.co_producer_midi_notes(seq, lane, None, None).unwrap();
    assert!(row.notes.is_empty());
    s.close().unwrap();
}
