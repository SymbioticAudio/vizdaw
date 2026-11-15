//! New Bus from a channel strip (plan 6g-2 Task 3, Ruling 7): a bus and a
//! post-fader 0 dB send to it from the strip, as one undo step "New Bus",
//! the bus filled with the default EQ like any new bus. Refusals leave no
//! bus, no step and no open transaction. Offline only; the catalog lookup
//! of Multi-Q is replaced by `bundled::debug_set_multiq`.

use core_model::mixer::{PluginFormat, PluginRef};
use std::sync::{Arc, Mutex, MutexGuard};
use vizdaw_ffi::bundled::{debug_clear_multiq, debug_set_multiq};
use vizdaw_ffi::host::{force_offline, reset_host};
use vizdaw_ffi::{
    SendRow, Session, StripKindRow, StripRow, TrackKindRow, set_default_eq, set_new_channel_eq,
};

static SERIAL: Mutex<()> = Mutex::new(());

/// One test at a time; offline; the switch on; the override and the
/// choice cleared.
fn serial() -> MutexGuard<'static, ()> {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    force_offline();
    vizdaw_ffi::host::set_offline_rate(48_000.0);
    reset_host();
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

fn strips(s: &Session) -> Vec<StripRow> {
    s.mixer().unwrap().strips
}

fn strip(s: &Session, id: u64) -> StripRow {
    strips(s).into_iter().find(|x| x.id == id).unwrap()
}

fn buses(s: &Session) -> Vec<StripRow> {
    strips(s)
        .into_iter()
        .filter(|x| x.kind == StripKindRow::Bus)
        .collect()
}

fn master_id(s: &Session) -> u64 {
    strips(s)
        .into_iter()
        .find(|x| x.kind == StripKindRow::Master)
        .unwrap()
        .id
}

fn eq_id(x: &StripRow) -> Option<String> {
    x.eq.as_ref().map(|e| e.plugin.id.clone())
}

fn send_to(bus: u64) -> SendRow {
    SendRow {
        target: bus,
        level_db: 0.0,
        pre_fader: false,
        enabled: true,
    }
}

/// A new session with one audio track; answers it and the track's strip.
fn with_audio_track() -> (Arc<Session>, u64) {
    let s = Session::new_empty().unwrap();
    let seq = s.snapshot().unwrap().sequences[0].id;
    s.new_track(seq, TrackKindRow::Audio, None).unwrap();
    let id = strips(&s)
        .into_iter()
        .find(|x| matches!(x.kind, StripKindRow::AudioPath { .. }))
        .unwrap()
        .id;
    (s, id)
}

#[test]
fn new_bus_from_a_strip_is_one_step() {
    let _g = serial();
    let (s, track) = with_audio_track();
    let track_label = s.snapshot().unwrap().undo_label;

    let snap = s.new_bus_from(track, "Bus 1".into()).unwrap();
    let made = buses(&s);
    assert_eq!(made.len(), 1);
    assert_eq!(made[0].name, "Bus 1");
    let bus = made[0].id;
    assert_eq!(strip(&s, track).sends, vec![send_to(bus)]);
    assert_eq!(snap.undo_label.as_deref(), Some("New Bus"));

    let snap = s.undo().unwrap();
    assert!(buses(&s).is_empty());
    assert!(strip(&s, track).sends.is_empty());
    assert_eq!(snap.undo_label, track_label);

    s.redo().unwrap();
    let made = buses(&s);
    assert_eq!(made.len(), 1);
    assert_eq!(made[0].id, bus);
    assert_eq!(strip(&s, track).sends, vec![send_to(bus)]);

    s.new_bus_from(track, "Bus 2".into()).unwrap();
    assert_eq!(strip(&s, track).sends.len(), 2);
    assert_eq!(buses(&s).len(), 2);
    s.close().unwrap();
}

#[test]
fn new_bus_gets_the_default_eq() {
    let _g = serial();
    debug_set_multiq(Some(mq()));
    let (s, track) = with_audio_track();
    let before = eq_id(&strip(&s, track));
    s.new_bus_from(track, "Bus 1".into()).unwrap();
    let bus = buses(&s).remove(0);
    assert_eq!(
        bus.eq.as_ref().map(|e| e.plugin.name.clone()).as_deref(),
        Some("Multi-Q")
    );
    assert_eq!(eq_id(&strip(&s, track)), before);
    s.close().unwrap();

    set_new_channel_eq(false);
    let (s, track) = with_audio_track();
    s.new_bus_from(track, "Bus 1".into()).unwrap();
    assert_eq!(eq_id(&buses(&s).remove(0)), None);
    s.close().unwrap();
    set_new_channel_eq(true);
    debug_clear_multiq();
}

#[test]
fn new_bus_refuses_bad_input() {
    let _g = serial();
    let (s, track) = with_audio_track();
    // A strip removed by an undo.
    s.new_bus_from(track, "Gone".into()).unwrap();
    let gone = buses(&s).remove(0).id;
    s.undo().unwrap();

    let master = master_id(&s);
    let cases: Vec<(u64, &str)> = vec![
        (master, "Bus 1"),
        (u64::MAX, "Bus 1"),
        (gone, "Bus 1"),
        (track, ""),
        (track, "   "),
    ];
    for (id, name) in cases {
        let before = s.snapshot().unwrap();
        let count = buses(&s).len();
        let e = s.new_bus_from(id, name.into());
        assert!(e.is_err(), "{id} {name:?}");
        if id == master {
            assert_eq!(e.unwrap_err().to_string(), "Master has no sends.");
        }
        let after = s.snapshot().unwrap();
        assert_eq!(buses(&s).len(), count, "{id} {name:?}");
        assert_eq!(after.undo_label, before.undo_label, "{id} {name:?}");
        assert_eq!(after.version, before.version, "{id} {name:?}");
    }
    // No transaction was left open.
    let snap = s.add_bus("Bus 1".into()).unwrap();
    assert_eq!(snap.undo_label.as_deref(), Some("Add Bus"));
    assert_eq!(buses(&s).len(), 1);
    s.close().unwrap();
}
