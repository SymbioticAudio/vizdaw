//! Crash recovery through the FFI (spec §10): the crash marker names the
//! plugins loading and goes on a clean close; a crash leaves it, and the
//! project then opens with plugins off (each slot loads on request);
//! autosaves capture plugin state, wait for drags, and restore as an
//! edited document; saving captures plugin state too.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};
use vizdaw_ffi::host::{force_offline, reset_host};
use vizdaw_ffi::recovery::{recovery_check, set_autosave_interval_ms, set_support_dir};
use vizdaw_ffi::session::simulate_crash;
use vizdaw_ffi::{OpenOptions, ParamValue, PluginFormatTag, PluginRefRow, Session, SlotStatus};

static SERIAL: Mutex<()> = Mutex::new(());

/// One test at a time, each with its own support folder.
fn serial() -> (MutexGuard<'static, ()>, tempfile::TempDir) {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    force_offline();
    reset_host();
    set_autosave_interval_ms(120_000);
    vizdaw_ffi::catalog::add_test_bundle(&test_plugins::bundle()).unwrap();
    let support = tempfile::tempdir().unwrap();
    set_support_dir(support.path().to_string_lossy().into_owned()).unwrap();
    (g, support)
}

fn gain() -> PluginRefRow {
    PluginRefRow {
        format: PluginFormatTag::Clap,
        id: "vizdaw.test.gain".into(),
        name: "Test Gain".into(),
        vendor: "VizDAW".into(),
        version: "1.0.0".into(),
    }
}

/// The demo with a Test Gain insert, saved: (folder, path, slot).
fn project() -> (tempfile::TempDir, String, u64) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("r.vizdaw").to_string_lossy().into_owned();
    let s = Session::new_demo().unwrap();
    let synth = s.mixer().unwrap().strips[0].id;
    s.add_insert(synth, None, gain()).unwrap();
    s.save_as(path.clone()).unwrap();
    let id = s.plugin_slots().unwrap()[0].id;
    (dir, path, id)
}

fn markers(support: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(support.join("open-projects"))
        .map(|rd| rd.flatten().map(|e| e.path()).collect())
        .unwrap_or_default()
}

fn idle_until(s: &Session, mut done: impl FnMut(&Session) -> bool) {
    let t = Instant::now();
    while !done(s) {
        assert!(t.elapsed() < Duration::from_secs(10), "timed out");
        s.idle().unwrap();
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn status(s: &Session) -> SlotStatus {
    s.plugin_slots().unwrap()[0].status.clone()
}

#[test]
fn the_marker_names_loading_plugins_and_a_clean_close_removes_it() {
    let (_g, support) = serial();
    let (_d, path, _id) = project();
    assert!(
        markers(support.path()).is_empty(),
        "the untitled save left none"
    );
    let s = Session::open(path.clone()).unwrap();
    let m = markers(support.path());
    assert_eq!(m.len(), 1);
    let text = std::fs::read_to_string(&m[0]).unwrap();
    assert!(text.contains("vizdaw.test.gain"), "names the load: {text}");
    let check = recovery_check(path.clone()).unwrap();
    assert!(!check.crashed, "open here is not a crash");
    idle_until(&s, |s| status(s) == SlotStatus::Live);
    let text = std::fs::read_to_string(&m[0]).unwrap();
    assert!(!text.contains("vizdaw.test.gain"), "loaded: {text}");
    s.close().unwrap();
    assert!(markers(support.path()).is_empty());
    s.close().unwrap();
    drop(s);
    assert!(markers(support.path()).is_empty());
}

#[test]
fn after_a_crash_the_project_opens_with_plugins_off() {
    let (_g, _support) = serial();
    let (_d, path, id) = project();
    let s = Session::open(path.clone()).unwrap();
    // It crashes while the plugin is still loading.
    simulate_crash(s);
    let check = recovery_check(path.clone()).unwrap();
    assert!(check.crashed);
    assert_eq!(check.loading.len(), 1);
    assert_eq!(check.loading[0].id, "vizdaw.test.gain");
    assert_eq!(check.autosave, None);
    let s = Session::open_with(
        path.clone(),
        OpenOptions {
            plugins_off: true,
            autosave: None,
        },
    )
    .unwrap();
    for _ in 0..20 {
        s.idle().unwrap();
    }
    assert_eq!(
        status(&s),
        SlotStatus::Placeholder {
            reason: "Plugins off (safe mode)".into()
        }
    );
    assert_eq!(s.held_slots().unwrap(), vec![id]);
    assert!(!s.snapshot().unwrap().dirty, "holding is not an edit");
    s.load_plugins(vec![id]).unwrap();
    assert!(s.held_slots().unwrap().is_empty());
    idle_until(&s, |s| status(s) == SlotStatus::Live);
    // Open here now: no longer reported as a crash.
    assert!(!recovery_check(path).unwrap().crashed);
}

#[test]
fn saving_captures_plugin_state() {
    let (_g, _support) = serial();
    let (_d, path, id) = project();
    let s = Session::open(path.clone()).unwrap();
    idle_until(&s, |s| status(s) == SlotStatus::Live);
    s.set_param(id, 0, 0.5).unwrap();
    s.commit_params(id, vec![ParamValue { id: 0, value: 0.5 }])
        .unwrap();
    s.save().unwrap();
    let doc = core_model::file::load(Path::new(&path)).unwrap();
    let slot = doc
        .plugin_slot(core_model::ids::PluginInstanceId(id))
        .unwrap();
    assert!(slot.state.is_some(), "captured on save");
    assert!(slot.params.is_empty(), "the overlay went into the state");
    assert!(!s.snapshot().unwrap().dirty);
}

#[test]
fn autosave_runs_after_edits_and_restores_as_edited() {
    let (_g, _support) = serial();
    let (_d, path, _id) = project();
    let s = Session::open(path.clone()).unwrap();
    idle_until(&s, |s| status(s) == SlotStatus::Live);
    set_autosave_interval_ms(30);
    let synth = s.mixer().unwrap().strips[0].id;
    // Not during a drag.
    s.begin_edit("Fader".into()).unwrap();
    s.set_gain(synth, -9.0).unwrap();
    std::thread::sleep(Duration::from_millis(60));
    s.idle().unwrap();
    let autosaves = |p: &str| core_model::file::list_autosaves(Path::new(p)).unwrap();
    assert!(autosaves(&path).is_empty(), "no autosave inside a drag");
    s.end_edit().unwrap();
    idle_until(&s, |_| !autosaves(&path).is_empty());
    let saved = autosaves(&path)[0].clone();
    let auto = core_model::file::load_autosave(Path::new(&path), &saved).unwrap();
    let slot = auto.mixer().inserts().next().unwrap().2.clone();
    assert!(
        slot.state.is_some(),
        "plugin state captured into the autosave"
    );
    // The project closes without saving; reopening offers the autosave.
    drop(s);
    let check = recovery_check(path.clone()).unwrap();
    assert!(!check.crashed);
    let auto_path = check.autosave.expect("a newer autosave");
    let r = Session::open_with(
        path.clone(),
        OpenOptions {
            plugins_off: false,
            autosave: Some(auto_path),
        },
    )
    .unwrap();
    assert_eq!(r.mixer().unwrap().strips[0].gain_db, -9.0);
    assert!(r.snapshot().unwrap().dirty, "Save keeps the restored copy");
    r.save().unwrap();
    assert!(recovery_check(path).unwrap().autosave.is_none());
}

#[test]
fn a_plugin_loading_into_an_edited_project_autosaves_at_once() {
    let (_g, _support) = serial();
    let (_d, path, _id) = project();
    let s = Session::open(path.clone()).unwrap();
    let synth = s.mixer().unwrap().strips[0].id;
    s.set_gain(synth, -3.0).unwrap();
    // The interval is long; the load itself triggers the autosave.
    idle_until(&s, |s| status(s) == SlotStatus::Live);
    s.idle().unwrap();
    assert_eq!(
        core_model::file::list_autosaves(Path::new(&path))
            .unwrap()
            .len(),
        1
    );
}

/// What the marker on disk said each time a plugin host sync was about to
/// run (the hook below).
static SEEN: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn look() {
    let support = vizdaw_ffi::recovery::support_dir();
    let text = markers(&support)
        .first()
        .and_then(|m| std::fs::read_to_string(m).ok())
        .unwrap_or_default();
    SEEN.lock().unwrap_or_else(|e| e.into_inner()).push(text);
}

fn seen() -> Vec<String> {
    std::mem::take(&mut *SEEN.lock().unwrap_or_else(|e| e.into_inner()))
}

#[test]
fn the_marker_names_a_plugin_before_the_sync_that_loads_it() {
    let (_g, _support) = serial();
    let (_d, path, _id) = project();
    seen();
    vizdaw_ffi::recovery::set_before_sync_hook(Some(look));
    let s = Session::open(path.clone()).unwrap();
    let opening = seen();
    assert!(
        opening.iter().any(|t| t.contains("vizdaw.test.gain")),
        "named before the open's sync: {opening:?}"
    );
    vizdaw_ffi::recovery::set_before_sync_hook(None);
    idle_until(&s, |s| status(s) == SlotStatus::Live);
    // An edit that adds a plugin: named before the reload's sync.
    let synth = s.mixer().unwrap().strips[0].id;
    seen();
    vizdaw_ffi::recovery::set_before_sync_hook(Some(look));
    s.add_insert(synth, None, gain()).unwrap();
    vizdaw_ffi::recovery::set_before_sync_hook(None);
    let editing = seen();
    assert!(
        editing.iter().any(|t| t.contains("vizdaw.test.gain")),
        "named before the edit's sync: {editing:?}"
    );
}

/// One session per package (Task 9 review I-2, which replaced "two
/// sessions on one project share the marker"): a second open is refused
/// and leaves the first one's marker alone.
#[test]
fn a_second_session_on_one_project_is_refused_and_keeps_the_marker() {
    let (_g, support) = serial();
    let (_d, path, _id) = project();
    let a = Session::open(path.clone()).unwrap();
    assert!(matches!(
        Session::open(path.clone()),
        Err(vizdaw_ffi::VizError::Busy { .. })
    ));
    let m = markers(support.path());
    assert_eq!(m.len(), 1, "the first window still has it open");
    let text = std::fs::read_to_string(&m[0]).unwrap();
    assert!(text.contains("vizdaw.test.gain"), "still named: {text}");
    assert!(!recovery_check(path.clone()).unwrap().crashed);
    a.close().unwrap();
    assert!(markers(support.path()).is_empty());
}

#[test]
fn a_closed_session_never_brings_its_marker_back() {
    let (_g, support) = serial();
    let (d, path, _id) = project();
    let s = Session::open(path.clone()).unwrap();
    s.close().unwrap();
    assert!(markers(support.path()).is_empty());
    for _ in 0..10 {
        s.idle().unwrap();
        std::thread::sleep(Duration::from_millis(2));
    }
    let synth = s.mixer().unwrap().strips[0].id;
    s.add_insert(synth, None, gain()).unwrap();
    // A closed window cannot Save As (Task 9 re-review m-3: it would
    // claim a package for a closed window); no marker comes back.
    let other = d.path().join("other.vizdaw");
    assert!(s.save_as(other.to_string_lossy().into_owned()).is_err());
    assert!(!other.exists());
    s.save().unwrap();
    assert!(markers(support.path()).is_empty());
    assert!(!recovery_check(path).unwrap().crashed);
}
