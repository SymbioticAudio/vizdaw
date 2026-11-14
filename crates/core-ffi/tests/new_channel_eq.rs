//! New channels get the bundled EQ (plan 6g Task 3, Rulings 3–8): the
//! first audio track's output path, the first built-in-synth track's strip
//! and each plug-in instrument's strip get Multi-Q in their EQ slot while
//! the "Put an EQ on new channels" switch is on and Multi-Q is usable. The
//! catalog lookup is replaced by `bundled::debug_set_multiq`, except in the
//! `bundled_multiq_` test, which needs the built plug-in. Offline only.

#![recursion_limit = "256"]

mod common;

use core_io::smf::{self, Division, SmfEvent, SmfFile, SmfTrack};
use core_model::document::Document;
use core_model::mixer::{PluginFormat, PluginRef, StripKind};
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};
use vizdaw_ffi::bundled::{self, debug_clear_multiq, debug_set_multiq};
use vizdaw_ffi::host::{force_offline, reset_host};
use vizdaw_ffi::{
    BitDepthRow, JobStateRow, MidiImportRequestRow, PluginFormatTag, PluginRefRow,
    ProjectFormatRow, Session, StripKindRow, StripRow, TrackKindRow, new_channel_eq,
    set_new_channel_eq,
};

static SERIAL: Mutex<()> = Mutex::new(());

/// One test at a time; offline; the switch on and the override cleared.
fn serial() -> MutexGuard<'static, ()> {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    force_offline();
    vizdaw_ffi::host::set_offline_rate(48_000.0);
    reset_host();
    vizdaw_ffi::catalog::add_test_bundle(&test_plugins::bundle()).unwrap();
    set_new_channel_eq(true);
    debug_clear_multiq();
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

fn synth4() -> PluginRefRow {
    PluginRefRow {
        format: PluginFormatTag::Clap,
        id: "vizdaw.test.synth4".into(),
        name: "Test Synth 4-Out".into(),
        vendor: "VizDAW".into(),
        version: "1.0.0".into(),
    }
}

fn first_seq(s: &Session) -> u64 {
    s.snapshot().unwrap().sequences[0].id
}

fn strips(s: &Session) -> Vec<StripRow> {
    s.mixer().unwrap().strips
}

/// The EQ name of every strip, by strip ID.
fn eqs(s: &Session) -> Vec<(u64, Option<String>)> {
    strips(s)
        .into_iter()
        .map(|x| (x.id, x.eq.map(|e| e.plugin.name)))
        .collect()
}

fn instrument_strips(s: &Session) -> Vec<StripRow> {
    strips(s)
        .into_iter()
        .filter(|x| matches!(x.kind, StripKindRow::Instrument { .. }))
        .collect()
}

/// The session's document.
fn doc_of(s: &Session) -> Document {
    core_model::file::from_json(&s.debug_document_json()).unwrap()
}

/// Every strip of a new project's tracks has Multi-Q, or none has an EQ.
/// Answers how many strips are not bus or master.
fn check_channels(s: &Session, want: Option<&str>) -> usize {
    let mut n = 0;
    for x in strips(s) {
        let name = x.eq.as_ref().map(|e| e.plugin.name.as_str());
        match x.kind {
            StripKindRow::AudioPath { .. }
            | StripKindRow::Instrument { .. }
            | StripKindRow::BuiltinSynth => {
                n += 1;
                assert_eq!(name, want, "{}", x.name);
            }
            _ => assert_eq!(name, None, "{}", x.name),
        }
    }
    n
}

/// The steps `new_tracks_get_multiq` and `no_default_when_multiq_is_unusable`
/// share: answers the session.
fn build_tracks(want: Option<&str>) -> Arc<Session> {
    let s = Session::new_empty().unwrap();
    let seq = first_seq(&s);
    s.new_track(seq, TrackKindRow::Audio, None).unwrap();
    assert_eq!(check_channels(&s, want), 1);
    s.new_track(seq, TrackKindRow::Instrument, None).unwrap();
    assert_eq!(check_channels(&s, want), 2);
    s.add_plugin_instrument("Keys".into(), synth4()).unwrap();
    assert_eq!(check_channels(&s, want), 3);
    let before = eqs(&s);
    // Later tracks share those strips; a MIDI track has none.
    s.new_track(seq, TrackKindRow::Audio, None).unwrap();
    s.new_track(seq, TrackKindRow::Instrument, None).unwrap();
    s.new_track(seq, TrackKindRow::Midi, None).unwrap();
    assert_eq!(eqs(&s), before);
    s
}

#[test]
fn new_tracks_get_multiq() {
    let _g = serial();
    debug_set_multiq(Some(mq()));
    assert_eq!(bundled::multiq(), Some(mq()));
    assert_eq!(bundled::new_strip_eq(), Some(mq()));
    let s = build_tracks(Some("Multi-Q"));
    // The document's slots name Multi-Q, at its flat default.
    let doc = doc_of(&s);
    let mut filled = 0;
    for x in doc.mixer().strips().values() {
        if matches!(
            x.kind,
            StripKind::AudioPath(_) | StripKind::Instrument(_) | StripKind::BuiltinSynth
        ) {
            let eq = x.eq.as_ref().unwrap();
            assert_eq!(eq.plugin, mq());
            assert!(eq.state.is_none() && eq.params.is_empty() && !eq.bypass);
            filled += 1;
        }
    }
    assert_eq!(filled, 3);
    s.close().unwrap();
    debug_clear_multiq();
}

#[test]
fn no_default_when_multiq_is_unusable() {
    let _g = serial();
    debug_set_multiq(None);
    assert_eq!(bundled::multiq(), None);
    assert_eq!(bundled::new_strip_eq(), None);
    let s = build_tracks(None);
    let snap = s.snapshot().unwrap();
    for w in snap.warnings.iter().chain(snap.edit_notes.iter()) {
        assert!(!w.contains("Multi-Q") && !w.contains("EQ"), "{w}");
    }
    s.close().unwrap();
    debug_clear_multiq();
}

#[test]
fn the_switch_is_read_per_command() {
    let _g = serial();
    debug_set_multiq(Some(mq()));
    let s = Session::new_empty().unwrap();
    set_new_channel_eq(false);
    assert!(!new_channel_eq());
    assert_eq!(bundled::new_strip_eq(), None);
    s.add_plugin_instrument("One".into(), synth4()).unwrap();
    let one = instrument_strips(&s);
    assert_eq!(one.len(), 1);
    assert!(one[0].eq.is_none());
    set_new_channel_eq(true);
    assert!(new_channel_eq());
    s.add_plugin_instrument("Two".into(), synth4()).unwrap();
    let both = instrument_strips(&s);
    assert_eq!(both.len(), 2);
    let two = both.iter().find(|x| x.name == "Two").unwrap();
    assert_eq!(two.eq.as_ref().unwrap().plugin.name, "Multi-Q");
    let one = both.iter().find(|x| x.name == "One").unwrap();
    assert!(one.eq.is_none(), "the first strip is left alone");
    s.close().unwrap();
    debug_clear_multiq();
}

#[test]
fn new_track_reads_the_switch() {
    let _g = serial();
    debug_set_multiq(Some(mq()));
    for on in [false, true] {
        set_new_channel_eq(on);
        assert_eq!(new_channel_eq(), on);
        let s = Session::new_empty().unwrap();
        s.new_track(first_seq(&s), TrackKindRow::Audio, None)
            .unwrap();
        let want = on.then_some("Multi-Q");
        assert_eq!(check_channels(&s, want), 1);
        s.close().unwrap();
    }
    set_new_channel_eq(true);
    debug_clear_multiq();
}

// ---- imports ----

fn on(c: u8, pitch: u8, vel: u8) -> SmfEvent {
    SmfEvent::Channel(core_engine::midi1::Midi1Msg::three(0x90 | c, pitch, vel))
}

fn off(c: u8, pitch: u8) -> SmfEvent {
    SmfEvent::Channel(core_engine::midi1::Midi1Msg::three(0x80 | c, pitch, 0x40))
}

/// A type 1 file at 960 per quarter: a conductor, then two tracks with two
/// notes each, on channels 1 and 2.
fn two_tracks() -> Vec<u8> {
    let end = SmfEvent::Meta {
        kind: 0x2F,
        data: vec![],
    };
    let mut tracks = vec![SmfTrack {
        events: vec![(0, end.clone())],
    }];
    for c in [0, 1] {
        tracks.push(SmfTrack {
            events: vec![
                (0, on(c, 60, 100)),
                (480, off(c, 60)),
                (960, on(c, 62, 90)),
                (1440, off(c, 62)),
                (1440, end.clone()),
            ],
        });
    }
    smf::write(&SmfFile {
        format: 1,
        division: Division::Ppq(960),
        tracks,
    })
    .unwrap()
}

fn text(p: &Path) -> String {
    p.display().to_string()
}

#[test]
fn imports_get_it_unless_they_fill_it() {
    use common::dawproject::{GAIN_ID, export, plugin, project_with, req, wait_job};
    use core_model::command::{Command, LaneCmd, MixerCmd, RoutingCmd};
    use core_model::ids::AnyId;
    let _g = serial();
    debug_set_multiq(Some(mq()));

    // A MIDI file as a new project: its one built-in synth strip.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("two.mid");
    std::fs::write(&path, two_tracks()).unwrap();
    let s = Session::new_empty().unwrap();
    let id = s
        .import_midi(MidiImportRequestRow {
            path: text(&path),
            new_project: true,
            seq: first_seq(&s),
            lane: None,
            tick: 0,
            channel_map: Vec::new(),
            split_channels: None,
            mpe: false,
            use_tempo: true,
        })
        .unwrap();
    let j = wait_job(&s, id);
    assert_eq!(j.state, JobStateRow::Done, "{:?}", j.message);
    let synths: Vec<StripRow> = strips(&s)
        .into_iter()
        .filter(|x| x.kind == StripKindRow::BuiltinSynth)
        .collect();
    assert_eq!(synths.len(), 1);
    assert_eq!(synths[0].eq.as_ref().unwrap().plugin.name, "Multi-Q");
    s.close().unwrap();

    // A DAWproject whose channel carries an EQ-tagged CLAP keeps it: the
    // "Stereo" track plays alone on output "Solo", whose strip has the
    // test gain in its EQ slot, so that strip is its channel.
    let p = project_with(|b, seq, _, _| {
        let r = b
            .run(Command::Routing(RoutingCmd::AddOutput {
                name: "Solo".into(),
                reference: "audio:3-4".into(),
            }))
            .unwrap();
        let out = r
            .created
            .iter()
            .find_map(|id| match id {
                AnyId::Output(o) => Some(*o),
                _ => None,
            })
            .unwrap();
        let lane = b
            .doc()
            .sequence(seq)
            .unwrap()
            .lanes
            .iter()
            .find(|l| l.name == "Stereo")
            .unwrap()
            .id;
        b.run(Command::Lane(LaneCmd::SetOutput {
            seq,
            lane,
            output: Some(out),
        }))
        .unwrap();
        let solo = b.strip_of(StripKind::AudioPath(out)).unwrap();
        b.mixer(MixerCmd::SetEq {
            strip: solo,
            plugin: Some(plugin(GAIN_ID, "Test Gain")),
        })
        .unwrap();
    });
    let file = p.out.join("eq.dawproject");
    export(&p.s, req(p.seq, &file));
    p.s.close().unwrap();
    let t = tempfile::tempdir().unwrap();
    let s = Session::new_project(ProjectFormatRow {
        sample_rate: 48_000,
        depth: BitDepthRow::Int24,
    })
    .unwrap();
    s.debug_set_trash_dir(Some(text(&t.path().join("Trash"))));
    s.debug_free_bytes(None).unwrap();
    s.save_as(text(&t.path().join("I.vizdaw"))).unwrap();
    let j = wait_job(
        &s,
        s.import_dawproject(vizdaw_ffi::DawprojectImportRequestRow { path: text(&file) })
            .unwrap(),
    );
    assert_eq!(j.state, JobStateRow::Done, "{:?}", j.message);
    let paths: Vec<StripRow> = strips(&s)
        .into_iter()
        .filter(|x| matches!(x.kind, StripKindRow::AudioPath { .. }))
        .collect();
    assert!(!paths.is_empty());
    let stereo = paths.iter().find(|x| x.name == "Stereo").unwrap();
    let eq = stereo.eq.as_ref().unwrap();
    assert_eq!(eq.plugin.id, GAIN_ID, "{paths:?}");
    // No orphan default is left behind.
    let doc = doc_of(&s);
    assert!(
        doc.find_slot(core_model::ids::PluginInstanceId(eq.id))
            .is_some()
    );
    let multiqs = doc
        .mixer()
        .strips()
        .values()
        .filter(|x| x.eq.as_ref().is_some_and(|e| e.plugin == mq()))
        .count();
    // "Mono" has no EQ in the file: it keeps the flat default (Ruling 10).
    assert_eq!(multiqs, 1, "{paths:?}");
    s.close().unwrap();
    debug_clear_multiq();
}

// ---- the built plug-in ----

/// The folder holding the bundled plug-ins (`VIZDAW_BUNDLED_PLUGINS`).
#[cfg(target_os = "macos")]
fn bundled_dir() -> std::path::PathBuf {
    match std::env::var_os("VIZDAW_BUNDLED_PLUGINS") {
        Some(dir) if !dir.is_empty() => std::path::PathBuf::from(dir),
        _ => panic!("VIZDAW_BUNDLED_PLUGINS is not set: build it with scripts/build-multiq.sh"),
    }
}

#[cfg(target_os = "macos")]
#[test]
#[ignore = "needs scripts/build-multiq.sh and VIZDAW_BUNDLED_PLUGINS"]
fn bundled_multiq_loads_in_a_new_track() {
    use common::dawproject::idle_until;
    use vizdaw_ffi::SlotStatus;
    let _g = serial();
    // Multi-Q may write its settings on load: never into the real home.
    let home = tempfile::tempdir().unwrap();
    // SAFETY: the tests of this file run one at a time (`serial`), and
    // nothing else in this process reads the environment meanwhile.
    unsafe { std::env::set_var("HOME", home.path()) };
    let clap = bundled_dir().join("Multi-Q.clap");
    assert!(
        clap.exists(),
        "{} is missing: build it with scripts/build-multiq.sh",
        clap.display()
    );
    vizdaw_ffi::catalog::add_test_bundle(&clap).unwrap();
    let got = bundled::multiq().expect("the bundled Multi-Q is usable");
    assert_eq!(got.id, "com.dusk-audio.multi-q");

    let s = Session::new_empty().unwrap();
    s.new_track(first_seq(&s), TrackKindRow::Audio, None)
        .unwrap();
    let path = strips(&s)
        .into_iter()
        .find(|x| matches!(x.kind, StripKindRow::AudioPath { .. }))
        .unwrap();
    let slot = path.eq.expect("the EQ slot is filled").id;
    idle_until(&s, |s| {
        s.plugin_slots()
            .unwrap()
            .iter()
            .any(|r| r.id == slot && r.status != SlotStatus::Loading)
    });
    let row = s
        .plugin_slots()
        .unwrap()
        .into_iter()
        .find(|r| r.id == slot)
        .unwrap();
    assert_eq!(row.status, SlotStatus::Live, "{row:?}");
    s.close().unwrap();
}
