//! Sessions host their plugins (spec §5, §11): a project's plugins load on
//! open, play through the engine once in, survive another session taking
//! the engine and a device rebuild, and are released when the session
//! closes. The engine host is process-wide, so these tests run one at a
//! time; plugin hosts live on each test's own thread.

use core_engine::backend::BackendEvent;
use core_engine::engine::find_sequence;
use core_engine::fixtures::{Builder, demo_document};
use core_model::mixer::{PluginFormat, PluginRef, StripKind};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use vizdaw_ffi::host::{
    closing_hosts_waiting, engine_active, force_offline, hosts_created, reset_host, simulate_event,
};
use vizdaw_ffi::{Session, SlotRow, SlotStatus};

static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    force_offline();
    vizdaw_ffi::host::set_offline_rate(48_000.0);
    reset_host();
    vizdaw_ffi::catalog::add_test_bundle(&test_plugins::bundle()).unwrap();
    g
}

fn gain_ref(id: &str) -> PluginRef {
    PluginRef {
        format: PluginFormat::Clap,
        id: id.into(),
        name: "Test Gain".into(),
        vendor: "VizDAW".into(),
        version: "1.0.0".into(),
    }
}

/// The demo saved with a `plugin` insert on the built-in synth's strip:
/// (folder, package path, Song, slot ID).
fn project(plugin: &str) -> (tempfile::TempDir, String, u64, u64) {
    let doc = demo_document().unwrap();
    let song = find_sequence(&doc, "Song").unwrap();
    let mut b = Builder::from_doc(doc);
    let synth = b.strip_of(StripKind::BuiltinSynth).unwrap();
    let slot = b.insert(synth, gain_ref(plugin)).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("p.vizdaw");
    core_model::file::save(&b.finish(), &path).unwrap();
    (dir, path.to_string_lossy().into_owned(), song.0, slot.0)
}

fn slot(s: &Session, id: u64) -> SlotRow {
    s.plugin_slots()
        .unwrap()
        .into_iter()
        .find(|r| r.id == id)
        .unwrap()
}

/// Runs the plugin timer until `done` (10 s at most).
fn idle_until(s: &Session, mut done: impl FnMut(&Session) -> bool) {
    let start = Instant::now();
    while !done(s) {
        assert!(start.elapsed() < Duration::from_secs(10), "timed out");
        s.idle().unwrap();
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn placeholder_lines(s: &Session) -> Vec<String> {
    s.transport()
        .unwrap()
        .report
        .into_iter()
        .filter(|l| l.contains("loading") || l.contains("Test Gain"))
        .collect()
}

/// The audio thread applies Play on its next block (2c Plan Ruling 30).
fn playing(s: &Session) -> bool {
    s.transport().unwrap().playing
}

fn open(path: &str) -> Arc<Session> {
    Session::open(path.to_string()).unwrap()
}

#[test]
fn a_projects_plugins_load_on_open_and_reach_the_engine() {
    let _g = serial();
    let (_d, path, song, id) = project("vizdaw.test.gain");
    let s = open(&path);
    assert_eq!(slot(&s, id).status, SlotStatus::Loading);
    // Played before the plugin is in: it is a placeholder for now.
    s.play(song).unwrap();
    assert!(!placeholder_lines(&s).is_empty());
    idle_until(&s, |s| slot(s, id).status == SlotStatus::Live);
    // The next timer step loads the engine with the running plugin.
    idle_until(&s, |s| placeholder_lines(s).is_empty() && playing(s));
    let row = slot(&s, id);
    assert_eq!(row.plugin.name, "Test Gain");
    assert!(!row.instrument && !row.has_sidechain_input);
    assert_eq!(row.outputs, 1);
}

#[test]
fn an_unknown_plugin_is_a_placeholder_with_its_reason() {
    let _g = serial();
    let (_d, path, _song, id) = project("vizdaw.test.nope");
    let s = open(&path);
    let mut problems = Vec::new();
    let start = Instant::now();
    while !matches!(slot(&s, id).status, SlotStatus::Placeholder { .. }) {
        assert!(start.elapsed() < Duration::from_secs(10), "timed out");
        problems.extend(s.idle().unwrap().problems);
        std::thread::sleep(Duration::from_millis(2));
    }
    let SlotStatus::Placeholder { reason } = slot(&s, id).status else {
        unreachable!()
    };
    assert!(reason.contains("failed"), "{reason}");
    assert!(
        problems
            .iter()
            .any(|p| p.starts_with("Built-in Synth — Test Gain: failed")),
        "{problems:?}"
    );
}

#[test]
fn another_session_takes_the_engine_and_the_first_keeps_its_plugins() {
    let _g = serial();
    let (_d, path, song, id) = project("vizdaw.test.gain");
    let a = open(&path);
    a.play(song).unwrap();
    idle_until(&a, |s| {
        placeholder_lines(s).is_empty() && slot(s, id).status == SlotStatus::Live
    });
    let b = Session::new_demo().unwrap();
    let b_song = b
        .snapshot()
        .unwrap()
        .sequences
        .iter()
        .find(|r| r.name == "Song")
        .unwrap()
        .id;
    b.play(b_song).unwrap();
    // A's processor comes back from the engine and is activated again.
    for _ in 0..50 {
        a.idle().unwrap();
        b.idle().unwrap();
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(slot(&a, id).status, SlotStatus::Live);
    a.play(song).unwrap();
    idle_until(&a, |s| placeholder_lines(s).is_empty() && playing(s));
}

#[test]
fn another_documents_plugins_never_play_for_same_numbered_slots() {
    let _g = serial();
    // Two projects whose inserts have the same instance ID and plugin.
    let (_d1, p1, song, id) = project("vizdaw.test.gain");
    let (_d2, p2, _, id2) = project("vizdaw.test.gain");
    assert_eq!(id, id2);
    let a = open(&p1);
    let b = open(&p2);
    a.play(song).unwrap();
    idle_until(&a, |s| {
        placeholder_lines(s).is_empty() && slot(s, id).status == SlotStatus::Live
    });
    let created = hosts_created();
    b.play(song).unwrap();
    assert_eq!(hosts_created(), created + 1, "a fresh engine for b");
    idle_until(&b, |s| {
        placeholder_lines(s).is_empty() && slot(s, id).status == SlotStatus::Live && playing(s)
    });
    // A's processor came back to A (the old engine went): closing A
    // leaves nothing waiting.
    a.idle().unwrap();
    drop(a);
    assert_eq!(closing_hosts_waiting(), 0);
    // Sessions without plugins switch without a rebuild.
    let c = Session::new_demo().unwrap();
    let d = Session::new_demo().unwrap();
    let c_song = c.snapshot().unwrap().sequences[2].id;
    c.play(c_song).unwrap();
    let created = hosts_created();
    d.play(c_song).unwrap();
    // b's engine had plugins: c rebuilt it; d inherits c's (none).
    assert_eq!(hosts_created(), created);
}

#[test]
fn a_device_rebuild_hands_the_plugins_to_the_new_engine() {
    let _g = serial();
    let (_d, path, song, id) = project("vizdaw.test.gain");
    let s = open(&path);
    s.play(song).unwrap();
    idle_until(&s, |s| {
        placeholder_lines(s).is_empty() && slot(s, id).status == SlotStatus::Live
    });
    let created = hosts_created();
    simulate_event(BackendEvent::DeviceLost);
    s.transport().unwrap();
    assert_eq!(hosts_created(), created + 1);
    // The old engine gave the processor back; it is activated again and
    // the new engine loads it.
    idle_until(&s, |s| {
        placeholder_lines(s).is_empty() && slot(s, id).status == SlotStatus::Live && playing(s)
    });
}

#[test]
fn a_sample_rate_change_reactivates_the_plugins_at_the_new_rate() {
    let _g = serial();
    let (_d, path, song, id) = project("vizdaw.test.gain");
    let s = open(&path);
    s.play(song).unwrap();
    idle_until(&s, |s| {
        placeholder_lines(s).is_empty() && slot(s, id).status == SlotStatus::Live
    });
    // The device now runs at 44.1 kHz: the engine is rebuilt at that rate
    // and the plugins are activated for it before they play again.
    assert_eq!(s.debug_plugin_rate().unwrap(), 48_000.0);
    vizdaw_ffi::host::set_offline_rate(44_100.0);
    simulate_event(BackendEvent::SampleRateChanged(44_100.0));
    assert_eq!(s.transport().unwrap().sample_rate, 44_100.0);
    assert_eq!(vizdaw_ffi::plugins::config().0, 44_100.0);
    assert_eq!(s.debug_plugin_rate().unwrap(), 44_100.0, "reconfigured");
    idle_until(&s, |s| {
        placeholder_lines(s).is_empty() && slot(s, id).status == SlotStatus::Live && playing(s)
    });
    vizdaw_ffi::host::set_offline_rate(48_000.0);
}

#[test]
fn closing_a_session_releases_its_plugins() {
    let _g = serial();
    let (_d, path, song, id) = project("vizdaw.test.gain");
    let a = open(&path);
    a.play(song).unwrap();
    idle_until(&a, |s| {
        placeholder_lines(s).is_empty() && slot(s, id).status == SlotStatus::Live
    });
    drop(a);
    assert!(!engine_active(), "the engine went first");
    assert_eq!(closing_hosts_waiting(), 0, "every processor came back");
    // A session that never played closes at once too.
    let b = open(&path);
    idle_until(&b, |s| slot(s, id).status == SlotStatus::Live);
    drop(b);
    assert_eq!(closing_hosts_waiting(), 0);
}

#[cfg(target_os = "macos")]
#[test]
fn the_live_engine_uses_the_default_worker_count() {
    let cfg = vizdaw_ffi::host::live_config(48_000.0);
    assert_eq!(cfg.workers, platform_macos::rt::default_workers());
    assert_eq!(cfg.deadline, Some(vizdaw_ffi::host::DEADLINE));
    // Offline keeps rendering on the callback thread.
    let _g = serial();
    let s = Session::new_demo().unwrap();
    let song = s.snapshot().unwrap().sequences[0].id;
    s.play(song).unwrap();
    assert_eq!(vizdaw_ffi::host::engine_workers(), Some(0));
}

#[test]
fn reentering_the_registry_is_busy_not_a_panic() {
    let _g = serial();
    let (_d, path, _song, id) = project("vizdaw.test.gain");
    let a = open(&path);
    let b = open(&path);
    idle_until(&b, |s| slot(s, id).status == SlotStatus::Live);
    let hosts = vizdaw_ffi::plugins::host_count();
    let mut b = Some(b);
    // A nested run loop inside a plugin call: the timer ticks, another
    // call asks for a host, a session is dropped.
    let inner = vizdaw_ffi::plugins::with(0, |_| {
        let u = a.idle().unwrap();
        assert!(u.snapshot.is_none() && !u.slots_changed && u.problems.is_empty());
        assert!(matches!(
            vizdaw_ffi::plugins::with(0, |_| ()),
            Err(vizdaw_ffi::VizError::Busy { .. })
        ));
        assert!(matches!(
            a.plugin_slots(),
            Err(vizdaw_ffi::VizError::Busy { .. })
        ));
        drop(b.take());
        assert_eq!(vizdaw_ffi::plugins::pending_closes(), 1);
    });
    assert!(inner.is_ok());
    // The deferred close runs on the next tick.
    assert_eq!(vizdaw_ffi::plugins::host_count(), hosts);
    a.idle().unwrap();
    assert_eq!(vizdaw_ffi::plugins::pending_closes(), 0);
    assert_eq!(vizdaw_ffi::plugins::host_count(), hosts - 1);
    assert_eq!(closing_hosts_waiting(), 0);
    // The remaining session is unharmed.
    idle_until(&a, |s| slot(s, id).status == SlotStatus::Live);
}
