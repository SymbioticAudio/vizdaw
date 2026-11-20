//! The metronome through a session (groups-vcas-transport Task 10,
//! Ruling 19): its settings are saved per project and left out of the
//! file while default, each change is one undo step that marks the song
//! unsaved, the click never reaches a bounce, a stem or an export for
//! another DAW, and the three commands refuse hostile input on the
//! registry's path and the app's. Offline only; one test at a time (the
//! engine host is process-wide).

mod common;

use core_engine::fixtures::{Builder, Q};
use core_media::caf::{CafWriter, WriteMode};
use core_model::command::{Command, SettingsCmd};
use core_model::event::Fade;
use core_model::format::{AudioFormat, BitDepth};
use core_model::mixer::StripKind;
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;
use vizdaw_ffi::control::{ControlArg, ControlValue};
use vizdaw_ffi::host::{force_offline, reset_host};
use vizdaw_ffi::{
    BitDepthRow, BounceRangeRow, BounceRequestRow, BounceSettingsRow, BounceSourceRow,
    ClickModeRow, ClickRow, ContainerRow, ForDawRequestRow, JobStateRow, Session, VizError,
    set_bounce_settings,
};

static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    force_offline();
    reset_host();
    set_bounce_settings(BounceSettingsRow {
        tail_cap_ms: 10_000,
    });
    g
}

const DEFAULT: ClickRow = ClickRow {
    on: false,
    level_db: -6.0,
    mode: ClickModeRow::PlayAndRecord,
};

fn text(name: &str, value: &str) -> ControlArg {
    ControlArg {
        name: name.into(),
        value: ControlValue::Text {
            value: value.into(),
        },
    }
}

fn flag(name: &str, value: bool) -> ControlArg {
    ControlArg {
        name: name.into(),
        value: ControlValue::Flag { value },
    }
}

fn int(name: &str, value: i64) -> ControlArg {
    ControlArg {
        name: name.into(),
        value: ControlValue::Int { value },
    }
}

fn undo_label(s: &Session) -> Option<String> {
    s.snapshot().unwrap().undo_label
}

/// The saved `project.json`'s settings.
fn saved_settings(pkg: &Path) -> Value {
    let text = std::fs::read_to_string(pkg.join("project.json")).unwrap();
    let v: Value = serde_json::from_str(&text).unwrap();
    v["document"]["settings"].clone()
}

#[test]
fn click_settings_are_saved_per_project() {
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let s = Session::new_empty().unwrap();
    assert_eq!(s.click().unwrap(), DEFAULT, "a new song's click");
    assert_eq!(s.transport().unwrap().click, Some(DEFAULT));
    let plain = dir.path().join("Plain.vizdaw");
    s.save_as(plain.display().to_string()).unwrap();
    let settings = saved_settings(&plain);
    assert!(settings.is_object(), "{settings}");
    assert!(
        settings.get("click").is_none(),
        "default writes no click: {settings}"
    );
    let mine = ClickRow {
        on: true,
        level_db: -12.5,
        mode: ClickModeRow::RecordOnly,
    };
    s.set_click(mine).unwrap();
    assert_eq!(s.transport().unwrap().click, Some(mine));
    let kept = dir.path().join("Kept.vizdaw");
    s.save_as(kept.display().to_string()).unwrap();
    assert_eq!(
        saved_settings(&kept)["click"],
        json!({"on": true, "level_db": -12.5, "mode": "record_only"})
    );
    s.close().unwrap();
    let again = Session::open(kept.display().to_string()).unwrap();
    assert_eq!(again.click().unwrap(), mine, "kept through save and open");
    assert!(!again.snapshot().unwrap().dirty);
    again.close().unwrap();
}

#[test]
fn each_click_change_is_one_step() {
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let s = Session::new_empty().unwrap();
    let pkg = dir.path().join("P.vizdaw");
    let save = |s: &Session| {
        s.save_as(pkg.display().to_string()).unwrap();
        assert!(!s.snapshot().unwrap().dirty);
    };
    save(&s);
    let steps: Vec<(&str, Vec<ControlArg>, &str)> = vec![
        ("transport.click", vec![], "Click On"),
        (
            "transport.set_click_level",
            vec![text("level_db", "-12")],
            "Click Level",
        ),
        (
            "transport.set_click_mode",
            vec![text("mode", "record_only")],
            "Click Mode",
        ),
        ("transport.click", vec![flag("on", false)], "Click Off"),
    ];
    let mut settings = vec![s.click().unwrap()];
    let mut labels = Vec::new();
    for (id, args, label) in steps {
        let step = s.snapshot().unwrap().step;
        s.run_click(id.into(), args).unwrap();
        let snap = s.snapshot().unwrap();
        assert_eq!(snap.undo_label.as_deref(), Some(label), "{id}");
        assert_ne!(snap.step, step, "{id}: a step");
        assert!(snap.dirty, "{id}: unsaved");
        settings.push(s.click().unwrap());
        labels.push(label);
        save(&s);
    }
    assert_eq!(
        s.click().unwrap(),
        ClickRow {
            on: false,
            level_db: -12.0,
            mode: ClickModeRow::RecordOnly,
        }
    );
    // The same settings again change nothing.
    let step = s.snapshot().unwrap().step;
    s.run_click("transport.click".into(), vec![flag("on", false)])
        .unwrap();
    assert_eq!(s.snapshot().unwrap().step, step);
    assert!(!s.snapshot().unwrap().dirty);
    // Each Undo takes back exactly one change.
    while let Some(label) = labels.pop() {
        assert_eq!(undo_label(&s).as_deref(), Some(label));
        s.undo().unwrap();
        settings.pop();
        assert_eq!(Some(&s.click().unwrap()), settings.last(), "undo {label}");
    }
    s.close().unwrap();
}

/// `P.vizdaw` at 48 kHz float: lane "Audio" plays a 2-bar CAF from tick
/// 0 on its AudioPath strip. Opened.
struct Project {
    dir: tempfile::TempDir,
    s: Arc<Session>,
    seq: u64,
    path: u64,
}

fn project() -> Project {
    const FRAMES: u64 = 96_000;
    let dir = tempfile::tempdir().unwrap();
    let pkg = dir.path().join("P.vizdaw");
    let mut b = Builder::new();
    b.run(Command::Settings(SettingsCmd::SetFormat {
        format: AudioFormat {
            sample_rate: 48_000,
            depth: BitDepth::Float32,
        },
        swaps: Vec::new(),
    }))
    .unwrap();
    let (seq, lane) = b.sequence("Song", "Audio", None).unwrap();
    let m = b.media("Media/a.caf", FRAMES, 2).unwrap();
    b.region(
        seq,
        lane,
        0,
        m,
        0,
        FRAMES,
        0.0,
        Fade::default(),
        Fade::default(),
    )
    .unwrap();
    let doc = b.finish();
    let path = doc
        .mixer()
        .order()
        .iter()
        .copied()
        .find(|s| {
            doc.mixer()
                .strip(*s)
                .is_ok_and(|x| matches!(x.kind, StripKind::AudioPath(_)))
        })
        .unwrap();
    core_model::file::save(&doc, &pkg).unwrap();
    let mut w = CafWriter::create(
        &pkg.join("Media/a.caf"),
        48_000,
        2,
        BitDepth::Float32,
        WriteMode::Normal,
    )
    .unwrap();
    let samples: Vec<f32> = (0..FRAMES)
        .flat_map(|f| {
            let v = ((f % 997) as f32 + 1.0) / 2000.0;
            [v, -v]
        })
        .collect();
    w.write(&samples).unwrap();
    w.finish().unwrap();
    let s = Session::open(pkg.display().to_string()).unwrap();
    s.debug_set_trash_dir(Some(dir.path().join("Trash").display().to_string()));
    Project {
        dir,
        s,
        seq: seq.0,
        path: path.0,
    }
}

fn wait_done(s: &Session, id: u64) {
    let until = Instant::now() + common::wait_limit();
    loop {
        s.idle().unwrap();
        let j = s.job(id).unwrap();
        if j.state != JobStateRow::Running {
            s.idle().unwrap();
            assert_eq!(j.state, JobStateRow::Done, "{j:?}");
            return;
        }
        assert!(Instant::now() < until, "job {id} still running");
        std::thread::yield_now();
    }
}

/// Every file under `dir` with its bytes, by relative path.
fn files(dir: &Path) -> Vec<(String, Vec<u8>)> {
    fn walk(root: &Path, at: &Path, out: &mut Vec<(String, Vec<u8>)>) {
        for e in std::fs::read_dir(at).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                walk(root, &p, out);
            } else {
                let rel = p.strip_prefix(root).unwrap().display().to_string();
                out.push((rel, std::fs::read(&p).unwrap()));
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

/// The mix, a stem and an export for another DAW into `out`.
fn exports(p: &Project, out: &Path) {
    std::fs::create_dir_all(out).unwrap();
    for (name, source) in [
        ("mix.wav", BounceSourceRow::Mix),
        (
            "stems",
            BounceSourceRow::Stems {
                strips: vec![p.path],
            },
        ),
    ] {
        let dest: PathBuf = if name == "stems" {
            std::fs::create_dir_all(out.join("stems")).unwrap();
            out.join("stems")
        } else {
            out.join(name)
        };
        let req = BounceRequestRow {
            seq: p.seq,
            source,
            range: BounceRangeRow::Ticks {
                start: 0,
                end: 8 * Q,
            },
            container: ContainerRow::Wav,
            depth: BitDepthRow::Float32,
            rate: 48_000,
            dither: false,
            path: Some(dest.display().to_string()),
            add_to_project: false,
            replace: false,
        };
        wait_done(&p.s, p.s.bounce(req).unwrap());
    }
    let daw = out.join("daw");
    std::fs::create_dir_all(&daw).unwrap();
    let req = ForDawRequestRow {
        seq: p.seq,
        strips: None,
        container: ContainerRow::Wav,
        depth: BitDepthRow::Float32,
        rate: 48_000,
        dither: false,
        path: daw.display().to_string(),
        add_to_project: false,
    };
    wait_done(&p.s, p.s.export_for_daw(req).unwrap());
}

#[test]
fn clicks_never_reach_exports() {
    let _g = serial();
    let p = project();
    let off = p.dir.path().join("off");
    exports(&p, &off);
    p.s.set_click(ClickRow {
        on: true,
        level_db: 0.0,
        mode: ClickModeRow::PlayAndRecord,
    })
    .unwrap();
    let on = p.dir.path().join("on");
    exports(&p, &on);
    let (a, b) = (files(&off), files(&on));
    let wavs = a.iter().filter(|(n, _)| n.ends_with(".wav")).count();
    assert!(wavs >= 3, "the mix, a stem and the export's audio: {a:?}");
    assert_eq!(
        a.iter().map(|f| &f.0).collect::<Vec<_>>(),
        b.iter().map(|f| &f.0).collect::<Vec<_>>()
    );
    for ((name, x), (_, y)) in a.iter().zip(&b) {
        if name.ends_with(".wav") {
            assert!(x == y, "{name}: the click reached it");
        }
    }
    p.s.close().unwrap();
}

fn params(args: &[(&str, Value)]) -> Map<String, Value> {
    args.iter()
        .map(|(k, v)| ((*k).to_string(), v.clone()))
        .collect()
}

fn to_arg(name: &str, v: &Value) -> ControlArg {
    match v {
        Value::Bool(b) => flag(name, *b),
        Value::Number(n) => int(name, n.as_i64().unwrap()),
        Value::String(t) => text(name, t),
        other => panic!("{other}"),
    }
}

#[test]
fn click_commands_refuse_hostile_input() {
    let _g = serial();
    let s = Session::new_empty().unwrap();
    let before = s.debug_document_json();
    let version = s.snapshot().unwrap().version;
    let mut cases: Vec<(&str, Vec<(&str, Value)>)> = Vec::new();
    for level in ["0.5", "-60.5", "NaN", "1e1", "", "-6,0"] {
        cases.push((
            "transport.set_click_level",
            vec![("level_db", json!(level))],
        ));
    }
    for mode in ["always", ""] {
        cases.push(("transport.set_click_mode", vec![("mode", json!(mode))]));
    }
    cases.push(("transport.click", vec![("on", json!("yes"))]));
    cases.push(("transport.click", vec![("on", json!(1))]));
    cases.push(("transport.click", vec![("loud", json!(true))]));
    cases.push((
        "transport.set_click_level",
        vec![("level_db", json!("-6")), ("extra", json!("1"))],
    ));
    cases.push((
        "transport.set_click_mode",
        vec![("mode", json!("record_only")), ("x", json!(1))],
    ));
    for (id, args) in &cases {
        // The registry's path (the control channel's).
        let spec = core_control::registry::find(id).unwrap();
        let e = core_control::registry::validate_args(spec, &params(args)).unwrap_err();
        assert_eq!(
            e.code,
            core_control::ErrorCode::InvalidParams,
            "{id} {args:?}: {e:?}"
        );
        // The app's path.
        let call: Vec<ControlArg> = args.iter().map(|(k, v)| to_arg(k, v)).collect();
        let r = s.run_click((*id).into(), call);
        assert!(
            matches!(r, Err(VizError::Model { .. })),
            "{id} {args:?}: {r:?}"
        );
        assert_eq!(
            s.snapshot().unwrap().version,
            version,
            "{id}: nothing changed"
        );
    }
    assert!(matches!(
        s.run_click("transport.clickety".into(), vec![]),
        Err(VizError::Model { .. })
    ));
    for level_db in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 0.5, -60.5] {
        let r = s.set_click(ClickRow {
            level_db,
            ..DEFAULT
        });
        assert!(r.is_err(), "{level_db}: {r:?}");
    }
    assert_eq!(s.debug_document_json(), before);
    assert_eq!(s.click().unwrap(), DEFAULT);
    // The edges are taken.
    s.run_click(
        "transport.set_click_level".into(),
        vec![text("level_db", "-60")],
    )
    .unwrap();
    s.run_click(
        "transport.set_click_level".into(),
        vec![text("level_db", "0")],
    )
    .unwrap();
    assert_eq!(s.click().unwrap().level_db, 0.0);
    s.close().unwrap();
}
