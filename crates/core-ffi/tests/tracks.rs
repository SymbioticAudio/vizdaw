//! New projects and tracks (plan tracks Task 2, Rulings 3, 4 and 6): a new
//! project opens on "Sequence 1" with no tracks; New Audio, Instrument and
//! MIDI Track and Duplicate Track are one undo step each; a track's kind
//! follows its routing. Offline only: no audio device or MIDI port opens.

#![recursion_limit = "256"]

use core_engine::fixtures::Builder;
use core_engine::offline::FAKE_SYNTH_PORT;
use core_model::command::{Command, LaneCmd, RoutingCmd};
use core_model::document::DestKind;
use core_model::ids::AnyId;
use core_model::lane::{InputChannels, LaneInput};
use std::sync::{Mutex, MutexGuard};
use vizdaw_ffi::host::{force_offline, reset_host, set_fake_synth};
use vizdaw_ffi::{
    LaneInputRow, LaneRow, MonitorModeRow, ProjectFormatRow, Session, StripKindRow, TrackKindRow,
    VizError,
};

static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    force_offline();
    set_fake_synth(None);
    vizdaw_ffi::audio::reset_audio_settings();
    reset_host();
    g
}

fn first_seq(s: &Session) -> u64 {
    s.snapshot().unwrap().sequences[0].id
}

fn outputs(s: &Session) -> usize {
    s.mixer()
        .unwrap()
        .strips
        .iter()
        .filter(|x| matches!(x.kind, StripKindRow::AudioPath { .. }))
        .count()
}

fn synth_dests(s: &Session) -> usize {
    s.destinations()
        .unwrap()
        .iter()
        .filter(|d| d.name == "Built-in Synth")
        .count()
}

fn counts(s: &Session) -> (usize, usize, usize) {
    (
        s.lanes(first_seq(s)).unwrap().len(),
        s.destinations().unwrap().len(),
        outputs(s),
    )
}

fn names(s: &Session) -> Vec<String> {
    s.lanes(first_seq(s))
        .unwrap()
        .into_iter()
        .map(|l| l.name)
        .collect()
}

#[test]
fn new_projects_start_with_sequence_1() {
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let rate48 = ProjectFormatRow {
        sample_rate: 48_000,
        depth: vizdaw_ffi::BitDepthRow::Int24,
    };
    for s in [
        Session::new_empty().unwrap(),
        Session::new_project(rate48).unwrap(),
    ] {
        let snap = s.snapshot().unwrap();
        assert_eq!(snap.sequences.len(), 1);
        assert_eq!(snap.sequences[0].name, "Sequence 1");
        assert_eq!(snap.sequences[0].lane_count, 0);
        assert!(s.lanes(snap.sequences[0].id).unwrap().is_empty());
        assert_eq!(snap.undo_label, None, "nothing to undo");
        assert!(!snap.dirty, "not dirty");
        let path = dir.path().join("P.vizdaw");
        s.save_as(path.display().to_string()).unwrap();
        s.close().unwrap();
        let back = Session::open(path.display().to_string()).unwrap();
        let snap = back.snapshot().unwrap();
        assert_eq!(snap.sequences.len(), 1);
        assert_eq!(snap.sequences[0].name, "Sequence 1");
        back.close().unwrap();
        std::fs::remove_dir_all(&path).unwrap();
    }
}

#[test]
fn new_track_makes_each_kind_in_one_undo_step() {
    let _g = serial();
    vizdaw_ffi::audio::set_default_monitor_mode(MonitorModeRow::Input);
    let s = Session::new_empty().unwrap();
    let seq = first_seq(&s);
    assert_eq!(counts(&s), (0, 0, 0));

    // Audio
    let snap = s.new_track(seq, TrackKindRow::Audio, None).unwrap();
    assert_eq!(snap.undo_label.as_deref(), Some("New Audio Track"));
    let l = s.lanes(seq).unwrap();
    assert_eq!(l.len(), 1);
    assert_eq!(l[0].name, "Audio 1");
    assert_eq!(l[0].kind, TrackKindRow::Audio);
    assert_eq!(l[0].audio_out_name.as_deref(), Some("Audio 1"));
    assert_eq!(
        l[0].input,
        Some(LaneInputRow::Audio {
            first: 0,
            stereo: false,
            return_of: None
        })
    );
    assert_eq!(l[0].monitor, MonitorModeRow::Input);
    let out = l[0].audio_out.expect("an output path");
    let strips = s.mixer().unwrap().strips;
    let strip = strips
        .iter()
        .find(|x| x.kind == StripKindRow::AudioPath { path: out })
        .unwrap();
    assert_eq!(strip.name, "Audio 1");
    assert_eq!(counts(&s), (1, 0, 1));
    s.undo().unwrap();
    assert_eq!(counts(&s), (0, 0, 0));
    s.redo().unwrap();

    // Instrument
    let snap = s.new_track(seq, TrackKindRow::Instrument, None).unwrap();
    assert_eq!(snap.undo_label.as_deref(), Some("New Instrument Track"));
    let l = s.lanes(seq).unwrap();
    assert_eq!(l[1].name, "Inst 1");
    assert_eq!(l[1].kind, TrackKindRow::Instrument);
    assert_eq!(
        l[1].input,
        Some(LaneInputRow::Midi {
            port: None,
            channel: None
        })
    );
    assert_eq!(l[1].destination.as_deref(), Some("Built-in Synth"));
    // Every kind takes the "Monitoring for new tracks" default.
    assert_eq!(l[1].monitor, MonitorModeRow::Input);
    assert_eq!(counts(&s), (2, 1, 1));
    s.undo().unwrap();
    assert_eq!(counts(&s), (1, 0, 1));
    s.redo().unwrap();

    // A second Instrument reuses the destination.
    let snap = s.new_track(seq, TrackKindRow::Instrument, None).unwrap();
    assert_eq!(snap.undo_label.as_deref(), Some("New Instrument Track"));
    let l = s.lanes(seq).unwrap();
    assert_eq!(l[2].name, "Inst 2");
    assert_eq!(l[2].midi_dest, l[1].midi_dest);
    assert_eq!(l[2].monitor, MonitorModeRow::Input);
    assert_eq!(synth_dests(&s), 1);
    assert_eq!(counts(&s), (3, 1, 1));
    s.undo().unwrap();
    assert_eq!(counts(&s), (2, 1, 1));
    vizdaw_ffi::audio::reset_audio_settings();
    s.close().unwrap();
}

#[test]
fn a_midi_track_with_no_external_destination_has_none() {
    let _g = serial();
    let s = Session::new_empty().unwrap();
    let seq = first_seq(&s);
    let snap = s.new_track(seq, TrackKindRow::Midi, None).unwrap();
    assert_eq!(snap.undo_label.as_deref(), Some("New MIDI Track"));
    let l = s.lanes(seq).unwrap();
    assert_eq!(l[0].kind, TrackKindRow::Midi);
    assert_eq!(l[0].midi_dest, None);
    assert_eq!(
        l[0].input,
        Some(LaneInputRow::Midi {
            port: None,
            channel: None
        })
    );
    // With an external destination, the new track plays it.
    set_fake_synth(Some(10.0));
    s.add_midi_destination("Hw".into(), FAKE_SYNTH_PORT.into(), 0)
        .unwrap();
    let hw = s
        .destinations()
        .unwrap()
        .into_iter()
        .find(|d| d.external)
        .unwrap()
        .id;
    s.new_track(seq, TrackKindRow::Midi, None).unwrap();
    let l = s.lanes(seq).unwrap();
    assert_eq!(l[1].midi_dest, Some(hw));
    assert_eq!(l[1].kind, TrackKindRow::Midi);
    s.close().unwrap();
    set_fake_synth(None);
}

#[test]
fn new_track_goes_below_after() {
    let _g = serial();
    let s = Session::new_empty().unwrap();
    let seq = first_seq(&s);
    s.new_track(seq, TrackKindRow::Midi, None).unwrap();
    s.new_track(seq, TrackKindRow::Midi, None).unwrap();
    let ids: Vec<u64> = s.lanes(seq).unwrap().iter().map(|l| l.id).collect();
    s.new_track(seq, TrackKindRow::Audio, Some(ids[0])).unwrap();
    let l = s.lanes(seq).unwrap();
    assert_eq!(l[0].id, ids[0]);
    assert_eq!(l[1].name, "Audio 1");
    assert_eq!(l[2].id, ids[1]);
    s.new_track(seq, TrackKindRow::Audio, Some(999)).unwrap();
    let l = s.lanes(seq).unwrap();
    assert_eq!(l.len(), 4);
    assert_eq!(l[3].name, "Audio 2");
    // The smallest free number is used.
    s.edit(
        vizdaw_ffi::EditCommand::Lane {
            edit: vizdaw_ffi::LaneEdit::Rename {
                seq,
                lane: l[0].id,
                name: "Bass".into(),
            },
        },
        None,
    )
    .unwrap();
    s.new_track(seq, TrackKindRow::Midi, None).unwrap();
    assert_eq!(
        names(&s),
        ["Bass", "Audio 1", "MIDI 2", "Audio 2", "MIDI 1"]
    );
    s.close().unwrap();
}

fn row(s: &Session, seq: u64, name: &str) -> LaneRow {
    s.lanes(seq)
        .unwrap()
        .into_iter()
        .find(|l| l.name == name)
        .unwrap()
}

#[test]
fn track_kind_follows_routing() {
    let _g = serial();
    let mut b = Builder::new();
    let (seq, plain) = b.sequence("S", "Plain", None).unwrap();
    let synth = b
        .dest_kind(
            "Synth",
            DestKind::MidiEndpoint {
                endpoint_ref: core_model::mixer::BUILTIN_SYNTH_REF.into(),
                group: 0,
                channel: 0,
            },
            None,
        )
        .unwrap();
    let plugin = b
        .plugin_dest("Keys", core_model::mixer::PluginRef::from_legacy("keys"))
        .unwrap()
        .0;
    let ext = b.dest("Hw", "hw", 0).unwrap();
    let on_synth = b.lane(seq, "OnSynth", Some(synth)).unwrap();
    let on_plugin = b.lane(seq, "OnPlugin", Some(plugin)).unwrap();
    let on_ext = b.lane(seq, "OnExt", Some(ext)).unwrap();
    let audio_in = b.lane(seq, "AudioIn", Some(ext)).unwrap();
    let with_out = b.lane(seq, "WithOut", None).unwrap();
    let with_region = b.lane(seq, "WithRegion", None).unwrap();
    let out = {
        let r = b
            .run(Command::Routing(RoutingCmd::AddOutput {
                name: "Out".into(),
                reference: "audio:1-2".into(),
            }))
            .unwrap();
        r.created
            .iter()
            .find_map(|id| match id {
                AnyId::Output(o) => Some(*o),
                _ => None,
            })
            .unwrap()
    };
    b.run(Command::Lane(LaneCmd::SetInput {
        seq,
        lane: audio_in,
        input: Some(LaneInput::Audio {
            channels: InputChannels::Mono(0),
            return_of: None,
        }),
    }))
    .unwrap();
    b.run(Command::Lane(LaneCmd::SetOutput {
        seq,
        lane: with_out,
        output: Some(out),
    }))
    .unwrap();
    let media = b.media("Media/x.wav", 48_000, 1).unwrap();
    b.region(
        seq,
        with_region,
        0,
        media,
        0,
        1000,
        0.0,
        Default::default(),
        Default::default(),
    )
    .unwrap();
    // The region gave the lane an output; take it away so only the region
    // decides.
    b.run(Command::Lane(LaneCmd::SetOutput {
        seq,
        lane: with_region,
        output: None,
    }))
    .unwrap();
    let _ = (plain, on_synth, on_plugin, on_ext);
    let s = Session::from_document(b.finish()).unwrap();
    let seq = seq.0;
    // 1. an audio input wins over an external destination
    assert_eq!(row(&s, seq, "AudioIn").kind, TrackKindRow::Audio);
    // 2. a built-in synth or a plugin instrument
    assert_eq!(row(&s, seq, "OnSynth").kind, TrackKindRow::Instrument);
    assert_eq!(row(&s, seq, "OnPlugin").kind, TrackKindRow::Instrument);
    // 3. an external destination
    assert_eq!(row(&s, seq, "OnExt").kind, TrackKindRow::Midi);
    // 4. an output path, or audio regions in the active take
    assert_eq!(row(&s, seq, "WithOut").kind, TrackKindRow::Audio);
    assert_eq!(row(&s, seq, "WithRegion").kind, TrackKindRow::Audio);
    assert_eq!(row(&s, seq, "WithRegion").audio_out, None);
    // 5. otherwise MIDI: a plain Add Lane track
    assert_eq!(row(&s, seq, "Plain").kind, TrackKindRow::Midi);
    s.close().unwrap();
}

#[test]
fn duplicate_track_is_one_undo_step() {
    let _g = serial();
    let s = Session::new_empty().unwrap();
    let seq = first_seq(&s);
    s.new_track(seq, TrackKindRow::Instrument, None).unwrap();
    let lane = s.lanes(seq).unwrap()[0].id;
    let snap = s.duplicate_track(seq, lane).unwrap();
    assert_eq!(snap.undo_label.as_deref(), Some("Duplicate Track"));
    let l = s.lanes(seq).unwrap();
    assert_eq!(l.len(), 2);
    assert_eq!(l[1].name, "Inst 1 copy");
    assert_eq!(l[1].kind, TrackKindRow::Instrument);
    s.undo().unwrap();
    assert_eq!(s.lanes(seq).unwrap().len(), 1);
    s.close().unwrap();
}

#[test]
fn new_track_refuses_unknown_ids_and_busy() {
    let _g = serial();
    let s = Session::new_empty().unwrap();
    let seq = first_seq(&s);
    s.new_track(seq, TrackKindRow::Midi, None).unwrap();
    let lane = s.lanes(seq).unwrap()[0].id;
    let v = s.snapshot().unwrap().version;
    assert!(matches!(
        s.new_track(9999, TrackKindRow::Audio, None),
        Err(VizError::Model { .. })
    ));
    assert!(matches!(
        s.duplicate_track(seq, 9999),
        Err(VizError::Model { .. })
    ));
    assert!(matches!(
        s.duplicate_track(9999, lane),
        Err(VizError::Model { .. })
    ));
    assert_eq!(
        s.snapshot().unwrap().version,
        v,
        "unknown ids change nothing"
    );
    s.begin_edit("Drag".into()).unwrap();
    let v_open = s.snapshot().unwrap().version;
    assert!(matches!(
        s.new_track(seq, TrackKindRow::Instrument, None),
        Err(VizError::Busy { .. })
    ));
    assert!(matches!(
        s.duplicate_track(seq, lane),
        Err(VizError::Busy { .. })
    ));
    assert_eq!(s.snapshot().unwrap().version, v_open, "nothing changed");
    s.end_edit().unwrap();
    let snap = s.snapshot().unwrap();
    assert_eq!(snap.undo_label.as_deref(), Some("New MIDI Track"));
    assert_eq!(counts(&s), (1, 0, 0));
    s.close().unwrap();
}

/// Same Tracks as…: a source track whose (old-file) name Rename would
/// refuse gets "Track N" in the copy instead of failing the template.
#[test]
fn same_tracks_as_copes_with_blank_source_names() {
    let _g = serial();
    let mut b = Builder::new();
    let (seq, first) = b.sequence("Song", "Keep", None).unwrap();
    let blank = b.lane(seq, "", None).unwrap();
    let spaces = b.lane(seq, "  ", None).unwrap();
    let _ = (first, blank, spaces);
    let s = Session::from_document(b.finish()).unwrap();
    s.new_sequence_from_template(
        "Shape".into(),
        vizdaw_ffi::SequenceTemplate::SameLanesAs { seq: seq.0 },
    )
    .unwrap();
    let shape = s
        .snapshot()
        .unwrap()
        .sequences
        .into_iter()
        .find(|q| q.name == "Shape")
        .unwrap();
    let names: Vec<String> = s
        .lanes(shape.id)
        .unwrap()
        .into_iter()
        .map(|l| l.name)
        .collect();
    assert_eq!(names, ["Keep", "Track 2", "Track 3"]);
    s.close().unwrap();
}

#[test]
fn a_midi_track_takes_the_new_track_monitor_default() {
    let _g = serial();
    vizdaw_ffi::audio::set_default_monitor_mode(MonitorModeRow::Input);
    let s = Session::new_empty().unwrap();
    let seq = first_seq(&s);
    s.new_track(seq, TrackKindRow::Midi, None).unwrap();
    assert_eq!(s.lanes(seq).unwrap()[0].monitor, MonitorModeRow::Input);
    vizdaw_ffi::audio::reset_audio_settings();
    s.close().unwrap();
}

#[test]
fn a_new_empty_sequence_has_no_tracks_and_is_one_undo_step() {
    use vizdaw_ffi::templates::SequenceTemplate;
    let _g = serial();
    let s = Session::new_empty().unwrap();
    let before = s.snapshot().unwrap().sequences.len();
    let snap = s
        .new_sequence_from_template("Plain".into(), SequenceTemplate::Empty)
        .unwrap();
    assert_eq!(snap.sequences.len(), before + 1);
    let plain = snap
        .sequences
        .iter()
        .find(|q| q.name == "Plain")
        .unwrap()
        .id;
    assert!(s.lanes(plain).unwrap().is_empty());
    assert_eq!(
        snap.undo_label.as_deref(),
        Some("New Sequence from Template")
    );
    let snap = s.undo().unwrap();
    assert_eq!(snap.sequences.len(), before);
    // The other templates still make their tracks.
    let snap = s
        .new_sequence_from_template("Kit".into(), SequenceTemplate::DrumKit)
        .unwrap();
    let kit = snap.sequences.iter().find(|q| q.name == "Kit").unwrap().id;
    assert_eq!(s.lanes(kit).unwrap().len(), 1);
    s.close().unwrap();
}
