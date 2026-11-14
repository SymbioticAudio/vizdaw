//! The chosen default EQ (plan 6g-2 Task 1, Rulings 2–5): new channels get
//! the user's choice when the catalog says it is a usable effect, else
//! Multi-Q with a note, else nothing; new buses get it too, and so does the
//! master of a new project, while opened projects keep their strips. The
//! catalog lookup of Multi-Q is replaced by `bundled::debug_set_multiq`;
//! the choice is looked up in the test-plugins bundle. Offline only.

use core_model::mixer::{PluginFormat, PluginRef};
use std::sync::{Arc, Mutex, MutexGuard};
use vizdaw_ffi::bundled::{self, debug_clear_multiq, debug_set_multiq};
use vizdaw_ffi::host::{force_offline, reset_host};
use vizdaw_ffi::{
    BitDepthRow, DefaultEqRow, DocSnapshot, PluginFormatTag, ProjectFormatRow, Session,
    StripKindRow, StripRow, TrackKindRow, default_eq, default_eq_note, set_default_eq,
    set_new_channel_eq,
};

static SERIAL: Mutex<()> = Mutex::new(());

const GAIN_ID: &str = "vizdaw.test.gain";

/// Turns the test gain on or off in the catalog, as the Plugin Manager's
/// `set_enabled` does.
fn enable_gain(on: bool) {
    vizdaw_ffi::catalog::with(|c| c.set_enabled(PluginFormat::Clap, GAIN_ID, on));
}

/// One test at a time; offline; the test plug-ins in the catalog and
/// enabled; the switch on; the override and the choice cleared.
fn serial() -> MutexGuard<'static, ()> {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    force_offline();
    vizdaw_ffi::host::set_offline_rate(48_000.0);
    reset_host();
    vizdaw_ffi::catalog::add_test_bundle(&test_plugins::bundle()).unwrap();
    enable_gain(true);
    set_new_channel_eq(true);
    debug_clear_multiq();
    set_default_eq(None);
    g
}

fn mq() -> PluginRef {
    PluginRef {
        format: PluginFormat::Clap,
        id: "com.dusk-audio.multi-q".into(),
        name: "Multi-Q".into(),
        vendor: "Dusk Audio".into(),
        version: "0.10.9".into(),
    }
}

fn gain() -> DefaultEqRow {
    DefaultEqRow {
        format: PluginFormatTag::Clap,
        id: GAIN_ID.into(),
        name: "Test Gain".into(),
    }
}

fn choice(id: &str, name: &str) -> DefaultEqRow {
    DefaultEqRow {
        format: PluginFormatTag::Clap,
        id: id.into(),
        name: name.into(),
    }
}

fn fallback(name: &str) -> String {
    format!("{name} is not available, so new channels get Multi-Q.")
}

const NEITHER: &str =
    "Test Gain is not available, and neither is Multi-Q, so new channels get no EQ.";

fn first_seq(s: &Session) -> u64 {
    s.snapshot().unwrap().sequences[0].id
}

fn strips(s: &Session) -> Vec<StripRow> {
    s.mixer().unwrap().strips
}

fn audio_strip(s: &Session) -> StripRow {
    strips(s)
        .into_iter()
        .find(|x| matches!(x.kind, StripKindRow::AudioPath { .. }))
        .unwrap()
}

fn master(s: &Session) -> StripRow {
    strips(s)
        .into_iter()
        .find(|x| x.kind == StripKindRow::Master)
        .unwrap()
}

fn bus(s: &Session, name: &str) -> StripRow {
    strips(s)
        .into_iter()
        .find(|x| x.kind == StripKindRow::Bus && x.name == name)
        .unwrap()
}

fn eq_id(x: &StripRow) -> Option<String> {
    x.eq.as_ref().map(|e| e.plugin.id.clone())
}

/// The step's lines the snapshot carries, and the document's warnings.
fn lines(snap: &DocSnapshot) -> Vec<String> {
    snap.edit_notes
        .iter()
        .chain(snap.warnings.iter())
        .cloned()
        .collect()
}

fn no_fallback_line(snap: &DocSnapshot) {
    for w in lines(snap) {
        assert!(!w.contains("not available"), "{w}");
    }
}

/// A new session with one audio track; answers it and the track's snapshot.
fn with_audio_track() -> (Arc<Session>, DocSnapshot) {
    let s = Session::new_empty().unwrap();
    let snap = s
        .new_track(first_seq(&s), TrackKindRow::Audio, None)
        .unwrap();
    (s, snap)
}

#[test]
fn chosen_eq_goes_on_new_channels() {
    let _g = serial();
    debug_set_multiq(Some(mq()));
    set_default_eq(Some(gain()));
    assert_eq!(default_eq(), Some(gain()));
    assert_eq!(default_eq_note(), None);
    assert_eq!(bundled::fallback_note(), None);

    let (s, snap) = with_audio_track();
    assert_eq!(eq_id(&audio_strip(&s)).as_deref(), Some(GAIN_ID));
    no_fallback_line(&snap);
    let snap = s.add_bus("Bus 1".into()).unwrap();
    assert_eq!(eq_id(&bus(&s, "Bus 1")).as_deref(), Some(GAIN_ID));
    no_fallback_line(&snap);
    s.close().unwrap();

    set_default_eq(None);
    assert_eq!(default_eq(), None);
    debug_clear_multiq();
}

#[test]
fn chosen_eq_falls_back_with_a_note() {
    let _g = serial();
    debug_set_multiq(Some(mq()));
    set_default_eq(Some(gain()));

    // Disabled in the Plugin Manager.
    enable_gain(false);
    assert_eq!(default_eq_note(), Some(fallback("Test Gain")));
    let (s, snap) = with_audio_track();
    assert_eq!(eq_id(&audio_strip(&s)), Some(mq().id));
    let want = fallback("Test Gain");
    assert_eq!(snap.edit_notes.iter().filter(|w| **w == want).count(), 1);
    s.close().unwrap();
    enable_gain(true);

    // An instrument, an unknown ID, a NUL, a very long ID.
    let long = "x".repeat(70_000);
    for id in [
        "vizdaw.test.synth4",
        "vizdaw.test.unknown",
        "vizdaw.test\0gain",
        long.as_str(),
    ] {
        set_default_eq(Some(choice(id, "Test Gain")));
        assert_eq!(
            default_eq_note(),
            Some(fallback("Test Gain")),
            "{}",
            id.len()
        );
        let (s, snap) = with_audio_track();
        assert_eq!(eq_id(&audio_strip(&s)), Some(mq().id));
        assert!(snap.edit_notes.contains(&fallback("Test Gain")));
        s.close().unwrap();
    }

    // A long name is cut.
    set_default_eq(Some(choice("vizdaw.test.unknown", &"n".repeat(300))));
    let cut = format!("{}…", "n".repeat(100));
    assert_eq!(default_eq_note(), Some(fallback(&cut)));
    let (s, snap) = with_audio_track();
    assert!(snap.edit_notes.contains(&fallback(&cut)));
    s.close().unwrap();

    // Multi-Q unusable too: no EQ, no step line, the Settings sentence.
    set_default_eq(Some(choice("vizdaw.test.unknown", "Test Gain")));
    debug_set_multiq(None);
    assert_eq!(bundled::new_strip_eq(), None);
    assert_eq!(bundled::fallback_note(), None);
    let (s, snap) = with_audio_track();
    assert_eq!(eq_id(&audio_strip(&s)), None);
    no_fallback_line(&snap);
    s.close().unwrap();
    assert_eq!(default_eq_note().as_deref(), Some(NEITHER));

    // The switch off: no note.
    set_new_channel_eq(false);
    assert_eq!(default_eq_note(), None);
    debug_set_multiq(Some(mq()));
    assert_eq!(default_eq_note(), None);
    assert_eq!(bundled::fallback_note(), None);
    set_new_channel_eq(true);

    set_default_eq(None);
    assert_eq!(default_eq_note(), None);
    debug_clear_multiq();
}

#[test]
fn new_projects_master_gets_it() {
    let _g = serial();
    debug_set_multiq(Some(mq()));
    let check = |s: &Session, want: Option<String>| {
        assert_eq!(eq_id(&master(s)), want);
        let snap = s.snapshot().unwrap();
        assert!(!snap.dirty);
        assert_eq!(snap.undo_label, None);
    };
    let format = ProjectFormatRow {
        sample_rate: 48_000,
        depth: BitDepthRow::Int24,
    };
    let s = Session::new_empty().unwrap();
    check(&s, Some(mq().id));
    s.close().unwrap();
    let s = Session::new_project(format).unwrap();
    check(&s, Some(mq().id));
    s.close().unwrap();

    // The chosen EQ, too.
    set_default_eq(Some(gain()));
    let s = Session::new_empty().unwrap();
    check(&s, Some(GAIN_ID.into()));
    s.close().unwrap();
    set_default_eq(None);

    set_new_channel_eq(false);
    let s = Session::new_empty().unwrap();
    check(&s, None);
    s.close().unwrap();
    let s = Session::new_project(format).unwrap();
    check(&s, None);
    s.close().unwrap();
    set_new_channel_eq(true);

    let s = Session::new_demo().unwrap();
    assert_eq!(eq_id(&master(&s)), None);
    s.close().unwrap();
    debug_clear_multiq();
}

#[test]
fn existing_projects_keep_their_strips() {
    let _g = serial();
    debug_set_multiq(None);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Old.vizdaw");
    let s = Session::new_empty().unwrap();
    s.debug_set_trash_dir(Some(dir.path().join("Trash").display().to_string()));
    s.new_track(first_seq(&s), TrackKindRow::Audio, None)
        .unwrap();
    s.add_bus("Bus 1".into()).unwrap();
    s.save_as(path.display().to_string()).unwrap();
    s.close().unwrap();

    debug_set_multiq(Some(mq()));
    let s = Session::open(path.display().to_string()).unwrap();
    assert!(!s.snapshot().unwrap().dirty);
    assert_eq!(eq_id(&master(&s)), None);
    assert_eq!(eq_id(&bus(&s, "Bus 1")), None);
    assert_eq!(eq_id(&audio_strip(&s)), None);
    let before = strips(&s);

    s.add_bus("Bus 2".into()).unwrap();
    assert_eq!(eq_id(&bus(&s, "Bus 2")), Some(mq().id));
    assert_eq!(eq_id(&bus(&s, "Bus 1")), None);
    assert_eq!(eq_id(&master(&s)), None);
    assert_eq!(eq_id(&audio_strip(&s)), None);

    s.undo().unwrap();
    assert_eq!(strips(&s), before);
    s.close().unwrap();
    debug_clear_multiq();
}
