//! The audio settings model over the preferences (slice 9a Task 1; Swift
//! `AudioPreferencesTests` and `AudioReadoutsTests`): handed to the core
//! at launch, kept only once the core took them, tolerant decode, and the
//! readouts' exact texts.

use serde_json::json;
use vizdaw_app::Prefs;
use vizdaw_app::audio_prefs::{
    AudioPrefs, AudioSettings, DISK_TEXT, DeviceChoice, cache_choices, count_text, device_choices,
    round_trip_text, trim_text,
};
use vizdaw_app::testing::FakeCore;
use vizdaw_ffi::edit::MonitorModeRow;
use vizdaw_ffi::host::InputDeviceRow;
use vizdaw_ffi::{InputInfoRow, VizError};

fn dev(name: &str, uid: &str) -> InputDeviceRow {
    InputDeviceRow {
        name: name.into(),
        uid: uid.into(),
    }
}

fn info(device: &str) -> InputInfoRow {
    InputInfoRow {
        device: device.into(),
        channels: 2,
        names: vec![],
        input_latency_frames: 38,
        output_latency_frames: 34,
        buffer_frames: 128,
        round_trip_frames: 328,
        opening: false,
        sample_rate: 48_000.0,
        midi_fx_dropped: 0,
        per_note_dropped: 0,
        mpe_stolen: 0,
    }
}

fn titles(c: &[DeviceChoice]) -> Vec<&str> {
    c.iter().map(|c| c.title.as_str()).collect()
}

#[test]
fn launch_hands_every_setting_to_the_core() {
    let prefs = Prefs::in_memory();
    let core = FakeCore::new();
    let a = AudioPrefs::load(&prefs);
    assert_eq!(*a.settings(), AudioSettings::default());
    a.apply_all(&core);
    assert_eq!(
        core.calls(),
        [
            "set_software_monitoring(true)",
            "set_audio_input_device(None)",
            "set_record_latency_trim(0)",
            "set_default_monitor_mode(Auto)",
            "set_bounce_settings(10000)",
            "set_io_buffer_frames(128)",
            "set_stream_settings(512,3)",
        ]
    );
    assert_eq!(a.problem(), None);
}

#[test]
fn a_setting_is_kept_only_once_the_core_took_it() {
    let mut prefs = Prefs::in_memory();
    let core = FakeCore::new();
    let mut a = AudioPrefs::load(&prefs);
    a.set_buffer_frames(128, &mut prefs, &core).unwrap();
    a.set_software_monitoring(false, &mut prefs, &core).unwrap();
    a.set_latency_trim(-12, &mut prefs, &core).unwrap();
    a.set_cache_mb(1024, &mut prefs, &core).unwrap();
    a.set_read_ahead_seconds(6, &mut prefs, &core).unwrap();
    a.set_default_monitor("off", &mut prefs, &core).unwrap();
    a.set_count_in_bars(2, &mut prefs, &core).unwrap();
    a.set_pre_roll_bars(1, &mut prefs, &core).unwrap();
    a.set_midi_passes_as_takes(true, &mut prefs, &core).unwrap();
    a.set_bounce_tail_seconds(5, &mut prefs, &core).unwrap();
    let again = AudioPrefs::load(&prefs);
    assert_eq!(again.settings(), a.settings());
    assert_eq!(again.settings().cache_mb, 1024);
    assert_eq!(again.settings().latency_trim, -12);
    assert_eq!(again.settings().bounce_tail_seconds, 5);
    // Refused while recording: the setting stays and says why.
    let before = prefs.raw("audio").cloned();
    core.refuse_next(
        "set_io_buffer_frames",
        VizError::Busy {
            message: "Stop recording to change the I/O buffer size.".into(),
        },
    );
    let e = a.set_buffer_frames(256, &mut prefs, &core).unwrap_err();
    assert_eq!(e, "Stop recording to change the I/O buffer size.");
    assert_eq!(a.settings().buffer_frames, 128);
    assert_eq!(prefs.raw("audio").cloned(), before);
    assert_eq!(
        a.problem(),
        Some("Stop recording to change the I/O buffer size.")
    );
    // A later success clears the problem.
    a.set_buffer_frames(256, &mut prefs, &core).unwrap();
    assert_eq!(a.settings().buffer_frames, 256);
    assert_eq!(a.problem(), None);
    // Out of range: clamped.
    a.set_cache_mb(1, &mut prefs, &core).unwrap();
    a.set_read_ahead_seconds(99, &mut prefs, &core).unwrap();
    assert_eq!(
        (a.settings().cache_mb, a.settings().read_ahead_seconds),
        (64, 8)
    );
    a.set_latency_trim(i32::MAX, &mut prefs, &core).unwrap();
    assert_eq!(a.settings().latency_trim, 192_000);
    a.set_default_monitor("loud", &mut prefs, &core).unwrap();
    assert_eq!(
        a.settings().default_monitor,
        "off",
        "an unknown mode changes nothing"
    );
    assert!(
        core.calls()
            .contains(&"set_stream_settings(64,8)".to_string())
    );
}

#[test]
fn a_refused_stream_setting_stays_as_it_was() {
    let mut prefs = Prefs::in_memory();
    let core = FakeCore::new();
    let mut a = AudioPrefs::load(&prefs);
    core.refuse_next(
        "set_stream_settings",
        VizError::Model {
            message: "no".into(),
        },
    );
    assert!(a.set_cache_mb(1024, &mut prefs, &core).is_err());
    assert_eq!(a.settings().cache_mb, 512);
    assert!(prefs.raw("audio").is_none());
}

#[test]
fn an_old_name_only_device_selects_by_name() {
    let mut prefs = Prefs::in_memory();
    prefs
        .set_raw("audio", json!({"inputDevice": "Desk"}))
        .unwrap();
    let core = FakeCore::new();
    let a = AudioPrefs::load(&prefs);
    assert_eq!(a.settings().input_device.as_deref(), Some("Desk"));
    assert_eq!(a.settings().input_device_uid, None);
    a.apply_all(&core);
    assert!(
        core.calls()
            .contains(&"set_audio_input_device(Some(\"Desk\"))".to_string())
    );
}

#[test]
fn the_uid_is_kept() {
    let mut prefs = Prefs::in_memory();
    let core = FakeCore::new();
    let mut a = AudioPrefs::load(&prefs);
    a.set_input_device(Some(dev("A", "u2")), &mut prefs, &core)
        .unwrap();
    let core2 = FakeCore::new();
    let mut again = AudioPrefs::load(&prefs);
    assert_eq!(again.settings().input_device_uid.as_deref(), Some("u2"));
    assert_eq!(again.settings().input_device.as_deref(), Some("A"));
    again.apply_all(&core2);
    assert!(
        core2
            .calls()
            .contains(&"set_audio_input_device(Some(\"u2\"))".to_string())
    );
    assert!(
        !core2
            .calls()
            .contains(&"set_audio_input_device(Some(\"A\"))".to_string())
    );
    again.set_input_device(None, &mut prefs, &core2).unwrap();
    assert_eq!(again.settings().input_device_uid, None);
    assert_eq!(again.settings().input_device, None);
    assert_eq!(
        core2.calls().last().unwrap(),
        "set_audio_input_device(None)"
    );
}

#[test]
fn a_damaged_field_keeps_its_default_and_the_others() {
    let mut prefs = Prefs::in_memory();
    prefs
        .set_raw(
            "audio",
            json!({"bufferFrames": 100, "cacheMb": 9, "readAheadSeconds": "x",
                   "defaultMonitor": "loud", "latencyTrim": 999999, "countInBars": 9,
                   "softwareMonitoring": false}),
        )
        .unwrap();
    let a = AudioPrefs::load(&prefs);
    let want = AudioSettings {
        software_monitoring: false,
        count_in_bars: 4,
        ..AudioSettings::default()
    };
    assert_eq!(*a.settings(), want);
    prefs.set_raw("audio", json!("not json")).unwrap();
    assert_eq!(
        *AudioPrefs::load(&prefs).settings(),
        AudioSettings::default()
    );
}

#[test]
fn record_defaults_come_from_the_settings() {
    let mut prefs = Prefs::in_memory();
    let core = FakeCore::new();
    let mut a = AudioPrefs::load(&prefs);
    a.set_count_in_bars(2, &mut prefs, &core).unwrap();
    a.set_pre_roll_bars(1, &mut prefs, &core).unwrap();
    a.set_midi_passes_as_takes(true, &mut prefs, &core).unwrap();
    let d = a.record_defaults();
    assert_eq!(
        (d.count_in_bars, d.pre_roll_bars, d.midi_passes_as_takes),
        (2, 1, true)
    );
    a.set_count_in_bars(200, &mut prefs, &core).unwrap();
    a.set_pre_roll_bars(200, &mut prefs, &core).unwrap();
    assert_eq!(
        (a.settings().count_in_bars, a.settings().pre_roll_bars),
        (4, 8)
    );
}

#[test]
fn midi_passes_as_takes_defaults_off_and_the_old_key_is_ignored() {
    assert!(!AudioSettings::default().midi_passes_as_takes);
    let decode = |v: serde_json::Value| {
        let mut p = Prefs::in_memory();
        p.set_raw("audio", v).unwrap();
        AudioPrefs::load(&p).settings().midi_passes_as_takes
    };
    assert!(!decode(json!({"passesAsTakes": true})));
    assert!(decode(json!({"midiPassesAsTakes": true})));
    assert!(!decode(json!({"midiPassesAsTakes": "yes"})));
}

#[test]
fn round_trip_before_and_after_the_device_opens() {
    let open = info("Studio");
    assert_eq!(
        round_trip_text(Some(&open), 128),
        "Round trip 6.8 ms (input 38 + output 34 + 2 × 128 frames)"
    );
    let clamped = InputInfoRow {
        buffer_frames: 256,
        round_trip_frames: 584,
        ..open
    };
    assert_eq!(
        round_trip_text(Some(&clamped), 32),
        "Round trip 12.2 ms (input 38 + output 34 + 2 × 256 frames); Studio runs at 256 frames"
    );
    assert_eq!(
        round_trip_text(None, 128),
        "About 5.3 ms at 48 kHz, plus the device's own latency once it opens"
    );
    let closed = InputInfoRow {
        device: String::new(),
        channels: 0,
        input_latency_frames: 0,
        output_latency_frames: 0,
        buffer_frames: 0,
        round_trip_frames: 0,
        sample_rate: 0.0,
        ..info("")
    };
    assert!(round_trip_text(Some(&closed), 1024).starts_with("About 42.7 ms"));
}

#[test]
fn the_cache_picker_keeps_an_off_list_value() {
    let list = vec![64, 128, 256, 512, 1024, 2048, 4096, 8192];
    assert_eq!(cache_choices(512), list);
    assert_eq!(
        cache_choices(300),
        [64, 128, 256, 300, 512, 1024, 2048, 4096, 8192]
    );
}

#[test]
fn a_disconnected_choice_is_kept() {
    let devices = [dev("Studio In", "s1"), dev("Mic", "m1")];
    let mut s = AudioSettings::default();
    assert_eq!(
        titles(&device_choices(&s, &devices)),
        ["Same as output", "Studio In", "Mic"]
    );
    s.input_device = Some("Gone".into());
    let c = device_choices(&s, &devices);
    assert_eq!(
        titles(&c),
        ["Same as output", "Studio In", "Mic", "Gone (not connected)"]
    );
    assert_eq!(c.last().unwrap().name.as_deref(), Some("Gone"));
    assert_eq!(c[0].name, None);
}

#[test]
fn two_same_named_devices_stay_apart() {
    let devices = [dev("A", "u1"), dev("A", "u2")];
    let mut s = AudioSettings::default();
    let c = device_choices(&s, &devices);
    assert_eq!(titles(&c), ["Same as output", "A", "A (2)"]);
    assert_eq!(c[1].uid.as_deref(), Some("u1"));
    assert_eq!(c[2].uid.as_deref(), Some("u2"));
    s.input_device = Some("Old".into());
    s.input_device_uid = Some("u9".into());
    let gone = device_choices(&s, &devices);
    assert_eq!(
        titles(&gone),
        ["Same as output", "A", "A (2)", "Old (not connected)"]
    );
    assert_eq!(gone.last().unwrap().uid.as_deref(), Some("u9"));
    // A listed UID is not "not connected", whatever name was saved.
    s.input_device = Some("Renamed".into());
    s.input_device_uid = Some("u2".into());
    assert_eq!(
        titles(&device_choices(&s, &devices)),
        ["Same as output", "A", "A (2)"]
    );
    // A name alone picks the only listed device with that name.
    s.input_device = Some("A".into());
    s.input_device_uid = None;
    assert_eq!(
        titles(&device_choices(&s, &devices)).len(),
        4,
        "two As: the name stays"
    );
    let one = [dev("Mic", "m1")];
    s.input_device = Some("Mic".into());
    assert_eq!(titles(&device_choices(&s, &one)), ["Same as output", "Mic"]);
}

#[test]
fn trim_and_count_texts() {
    assert_eq!(trim_text(12, 48_000.0), "+12 frames (0.25 ms)");
    assert_eq!(trim_text(-96, 96_000.0), "-96 frames (-1.00 ms)");
    assert_eq!(trim_text(0, 0.0), "0 frames (0.00 ms)");
    assert_eq!(count_text(0), "off");
    assert_eq!(count_text(1), "1 bar");
    assert_eq!(count_text(3), "3 bars");
    assert_eq!(
        DISK_TEXT,
        "Recording warns below 1 GB of free disk space and stops below 100 MB."
    );
    let _ = MonitorModeRow::Auto;
}

#[test]
fn a_refused_launch_size_is_the_problem_and_the_rest_still_go() {
    let prefs = Prefs::in_memory();
    let core = FakeCore::new();
    core.refuse_next(
        "set_io_buffer_frames",
        VizError::Busy {
            message: "busy".into(),
        },
    );
    let mut a = AudioPrefs::load(&prefs);
    a.apply_at_launch(&core);
    assert_eq!(a.problem(), Some("busy"));
    assert!(
        core.calls()
            .contains(&"set_bounce_settings(10000)".to_string())
    );
}
