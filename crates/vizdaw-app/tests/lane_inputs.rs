//! The track input controls' model (slice 8c Task 2; Swift
//! `LaneInputsTests`, the model rows of `RecordUiCarryTests` and the lane
//! rows of `RecordModelTests`): the picker's choices by the device's
//! channel names or the MIDI sources, read off the UI thread; a return
//! track keeping its synth; the monitor cycle; the meters and their
//! two-second clip hold, with one peak read feeding the meters and the
//! channel; the opening note; the fast poll while metering; and
//! `lane.set_input` / `lane.set_monitor` on the channel.

mod support;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use core_control::registry::ArgValue;
use core_control::{ControlError, ErrorCode};
use support::{FakeSession, demo_fake, dest_row, ext_dest, lane_row};
use vizdaw_app::commands::{self, Ran};
use vizdaw_app::lane_inputs::{
    CLIP_HOLD, InputChoice, InputGroup, LaneInputs, OPENING_NOTE_MACOS, OPENING_NOTE_OTHER,
    input_holder, opening_note,
};
use vizdaw_app::mixer::MixerMath;
use vizdaw_app::testing::FakeCore;
use vizdaw_app::transport::{PollResult, TransportModel};
use vizdaw_app::{DocId, DocSession, DocumentModel, DocumentStore};
use vizdaw_ffi::edit::{EditCommand, LaneEdit, LaneInputRow, MonitorModeRow};
use vizdaw_ffi::live::InputChannelsRow;
use vizdaw_ffi::snapshot::LaneRow;
use vizdaw_ffi::{InputInfoRow, VizError};

const FAST: Duration = Duration::from_millis(33);
const SLOW: Duration = Duration::from_millis(250);

fn audio_lane(id: u64, input: Option<LaneInputRow>, arm: bool, monitor: MonitorModeRow) -> LaneRow {
    let mut l = lane_row(id, &format!("Audio {id}"), false);
    l.audio_out = Some(7);
    l.input = input;
    l.arm = arm;
    l.monitor = monitor;
    l
}

fn mono(first: u16) -> Option<LaneInputRow> {
    Some(LaneInputRow::Audio {
        first,
        stereo: false,
        return_of: None,
    })
}

fn keys(input: Option<LaneInputRow>) -> LaneRow {
    let mut l = lane_row(31, "Keys", false);
    l.midi_dest = Some(1);
    l.input = input;
    l
}

fn info(channels: u32, names: &[&str], opening: bool) -> InputInfoRow {
    InputInfoRow {
        device: if channels > 0 {
            "Fake".into()
        } else {
            String::new()
        },
        channels,
        names: names.iter().map(|n| (*n).to_owned()).collect(),
        input_latency_frames: 0,
        output_latency_frames: 0,
        buffer_frames: 0,
        round_trip_frames: 0,
        opening,
        sample_rate: 0.0,
        midi_fx_dropped: 0,
        per_note_dropped: 0,
        mpe_stolen: 0,
    }
}

/// A store over a fake core whose new demo is Song (3) with `lanes`,
/// shown.
struct Doc {
    store: DocumentStore,
    core: Arc<FakeCore>,
    fake: Arc<FakeSession>,
    id: DocId,
}

impl Doc {
    fn new(lanes: Vec<LaneRow>) -> Self {
        Doc::with_core(Arc::new(FakeCore::new()), lanes)
    }

    fn with_core(core: Arc<FakeCore>, lanes: Vec<LaneRow>) -> Self {
        let mut store = DocumentStore::with_core(core.clone());
        let fake = Doc::hand_out(&core, lanes);
        let id = store.new_demo().unwrap();
        store.document_mut(id).unwrap().select(Some(3));
        Doc {
            store,
            core,
            fake,
            id,
        }
    }

    fn hand_out(core: &FakeCore, lanes: Vec<LaneRow>) -> Arc<FakeSession> {
        let fake = Arc::new(demo_fake());
        fake.set_lanes(3, lanes);
        core.hand_out("new_demo", fake.clone());
        fake
    }

    fn m(&mut self) -> &mut DocumentModel {
        self.store.document_mut(self.id).unwrap()
    }

    fn lane(&mut self, id: u64) -> LaneRow {
        self.m()
            .lanes()
            .iter()
            .find(|l| l.id == id)
            .unwrap()
            .clone()
    }

    fn titles(&mut self, lane: u64) -> HashMap<&'static str, Vec<String>> {
        let l = self.lane(lane);
        titles(&self.m().inputs().groups(&l))
    }

    /// A picker opens and its reads run (here, then land).
    fn refresh(&mut self) {
        self.store.refresh_inputs(self.id);
        self.store.run_reads();
    }
}

fn titles(g: &[InputGroup]) -> HashMap<&'static str, Vec<String>> {
    g.iter()
        .map(|g| (g.title, g.choices.iter().map(|c| c.title.clone()).collect()))
        .collect()
}

fn strings(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| (*s).to_owned()).collect()
}

fn run(id: &str, pairs: &[(&str, ArgValue)], m: &mut DocumentModel) -> Result<Ran, ControlError> {
    let args: Vec<(String, ArgValue)> = pairs
        .iter()
        .map(|(n, v)| ((*n).to_string(), v.clone()))
        .collect();
    commands::run(id, &args, m)
}

fn int(v: i64) -> ArgValue {
    ArgValue::Int(v)
}

fn text(v: &str) -> ArgValue {
    ArgValue::Text(v.into())
}

fn flag(v: bool) -> ArgValue {
    ArgValue::Flag(v)
}

// ---- The picker ----

#[test]
fn audio_choices_name_the_devices_channels() {
    let mut d = Doc::new(vec![audio_lane(30, None, false, MonitorModeRow::Auto)]);
    d.fake.set_input(info(3, &["Mic", "Guitar", ""], false));
    // Before the device was read: two channels.
    assert_eq!(d.titles(30)["Audio"], strings(&["In 1", "In 2", "In 1+2"]));
    d.refresh();
    let g = d.titles(30);
    assert_eq!(g[""], strings(&["No Input"]));
    assert_eq!(
        g["Audio"],
        strings(&["In 1 (Mic)", "In 2 (Guitar)", "In 3", "In 1+2"])
    );
    assert!(
        !g.contains_key("MIDI Input"),
        "an audio track offers no MIDI"
    );
    let l = d.lane(30);
    let pair = d.m().inputs().groups(&l)[1].choices.last().unwrap().clone();
    d.m().choose_input(&pair, &l);
    assert_eq!(
        d.fake.edits().last(),
        Some(&EditCommand::Lane {
            edit: LaneEdit::SetInput {
                seq: 3,
                lane: 30,
                input: Some(LaneInputRow::Audio {
                    first: 0,
                    stereo: true,
                    return_of: None
                })
            }
        })
    );
    assert_eq!(
        d.m().snapshot().undo_label.as_deref(),
        Some("Set Track Input")
    );
    assert_eq!(LaneInputs::title(pair.input.as_ref()), "In 1+2");
    assert_eq!(LaneInputs::title(None), "No Input");
}

/// Before the document holds the engine, the picker lists the chosen
/// device's channels and names (read without the engine); a mono input
/// offers In 1 alone; with nothing known, In 1, In 2 and In 1+2. The
/// device is read only while the engine's input is closed.
#[test]
fn the_picker_lists_the_devices_channels_before_the_engine() {
    let mut d = Doc::new(vec![audio_lane(30, None, false, MonitorModeRow::Auto)]);
    d.core.set_input_channels(Some(InputChannelsRow {
        channels: 4,
        names: strings(&["Mic", "", "Synth L", "Synth R"]),
    }));
    d.refresh();
    assert_eq!(
        d.titles(30)["Audio"],
        strings(&[
            "In 1 (Mic)",
            "In 2",
            "In 3 (Synth L)",
            "In 4 (Synth R)",
            "In 1+2",
            "In 3+4"
        ])
    );
    assert_eq!(d.m().inputs().known_channels(), Some(4));
    // Once the engine's input is open, it is what counts, and the device
    // is not read.
    let reads = d.core.call_count("audio_input_channels");
    d.fake.set_input(info(1, &["Mono"], false));
    d.refresh();
    assert_eq!(d.titles(30)["Audio"], strings(&["In 1 (Mono)"]));
    assert_eq!(d.m().inputs().known_channels(), Some(1));
    assert_eq!(d.core.call_count("audio_input_channels"), reads);
    // Nothing known: the two-channel guess.
    d.fake.set_input(info(0, &[], false));
    d.core.set_input_channels(None);
    d.refresh();
    assert_eq!(d.titles(30)["Audio"], strings(&["In 1", "In 2", "In 1+2"]));
    assert_eq!(d.m().inputs().known_channels(), None);
}

#[test]
fn a_return_lane_keeps_its_synth() {
    let ret = Some(LaneInputRow::Audio {
        first: 2,
        stereo: false,
        return_of: Some(5),
    });
    let mut d = Doc::new(vec![audio_lane(
        30,
        ret.clone(),
        false,
        MonitorModeRow::Auto,
    )]);
    d.fake.set_destinations(vec![dest_row(5, "Moog")]);
    let l = d.lane(30);
    assert_eq!(d.m().input_badge(&l).as_deref(), Some("\u{21E0} Moog"));
    assert_eq!(d.m().input_return_name(&l).as_deref(), Some("Moog"));
    let plain = audio_lane(31, mono(0), false, MonitorModeRow::Auto);
    assert_eq!(d.m().input_badge(&plain), None);
    let pair = d.m().inputs().groups(&l)[1]
        .choices
        .iter()
        .find(|c| c.title == "In 1+2")
        .unwrap()
        .clone();
    d.m().choose_input(&pair, &l);
    assert_eq!(
        d.fake.edits().last(),
        Some(&EditCommand::Lane {
            edit: LaneEdit::SetInput {
                seq: 3,
                lane: 30,
                input: Some(LaneInputRow::Audio {
                    first: 0,
                    stereo: true,
                    return_of: Some(5)
                })
            }
        })
    );
    // The same input again is no edit.
    let l = d.lane(30);
    let count = d.fake.edits().len();
    let same = InputChoice::new("In 1+2", l.input.clone());
    d.m().choose_input(&same, &l);
    assert_eq!(d.fake.edits().len(), count);
    // An unknown destination is the synth.
    d.fake.set_destinations(vec![]);
    d.fake.set_undo_label(Some("bump"));
    let _ = d.m().edit(EditCommand::Lane {
        edit: LaneEdit::Rename {
            seq: 3,
            lane: 30,
            name: "Ret".into(),
        },
    });
    let l = d.lane(30);
    assert_eq!(d.m().input_badge(&l).as_deref(), Some("\u{21E0} Synth"));
}

#[test]
fn midi_choices_are_sources_and_channels() {
    let pads = Some(LaneInputRow::Midi {
        port: Some("Pads".into()),
        channel: None,
    });
    let mut d = Doc::new(vec![keys(pads)]);
    d.core.set_midi_sources(strings(&["Keystation", "Pads"]));
    d.refresh();
    let g = d.titles(31);
    assert!(!g.contains_key("Audio"));
    assert_eq!(
        g["MIDI Input"],
        strings(&["All MIDI Inputs", "Keystation", "Pads"])
    );
    assert_eq!(
        g["MIDI Channel"].first().map(String::as_str),
        Some("Any Channel")
    );
    assert_eq!(g["MIDI Channel"].len(), 17);
    let l = d.lane(31);
    let ch3 = d.m().inputs().groups(&l)[2]
        .choices
        .iter()
        .find(|c| c.title == "Channel 3")
        .unwrap()
        .clone();
    d.m().choose_input(&ch3, &l);
    assert_eq!(
        d.fake.edits().last(),
        Some(&EditCommand::Lane {
            edit: LaneEdit::SetInput {
                seq: 3,
                lane: 31,
                input: Some(LaneInputRow::Midi {
                    port: Some("Pads".into()),
                    channel: Some(2)
                })
            }
        })
    );
    assert_eq!(
        LaneInputs::title(Some(&LaneInputRow::Midi {
            port: Some("Pads".into()),
            channel: Some(2)
        })),
        "Pads \u{00B7} Ch 3"
    );
    assert_eq!(
        LaneInputs::title(Some(&LaneInputRow::Midi {
            port: None,
            channel: None
        })),
        "All MIDI"
    );
    // MPE drops the channel.
    let mut mpe = keys(Some(LaneInputRow::Midi {
        port: Some("Pads".into()),
        channel: Some(2),
    }));
    mpe.mpe = Some(vizdaw_ffi::mpe::MpeRow {
        zone: vizdaw_ffi::mpe::MpeZoneRow::Lower,
        members: 15,
        bend_range: 48,
    });
    assert_eq!(LaneInputs::title_of(&mpe), "Pads \u{00B7} MPE");
    // A new track with neither destination nor output offers both.
    let both = titles(&d.m().inputs().groups(&lane_row(32, "New", false)));
    assert!(both.contains_key("Audio") && both.contains_key("MIDI Input"));
}

/// Plan 5f-1 Ruling 17: a surface's port hears nothing in a track, and
/// the picker says why.
#[test]
fn a_surfaces_port_is_marked_in_the_picker() {
    let mut d = Doc::new(vec![keys(None)]);
    d.core.set_midi_sources(strings(&["Keystation", "MCU In"]));
    d.refresh();
    d.m().inputs_mut().set_claimed_ports(strings(&["MCU In"]));
    assert_eq!(
        d.titles(31)["MIDI Input"],
        strings(&[
            "All MIDI Inputs",
            "Keystation",
            "MCU In (used by a surface)"
        ])
    );
}

#[test]
fn a_shared_paths_picker_names_its_lane() {
    let mut vox = audio_lane(30, mono(0), false, MonitorModeRow::Auto);
    vox.name = "Vox".into();
    let other = audio_lane(32, None, false, MonitorModeRow::Auto);
    assert_eq!(LaneInputs::picker_title_shared(&vox, true), "Vox: In 1");
    assert_eq!(LaneInputs::picker_title_shared(&vox, false), "In 1");
    let mut d = Doc::new(vec![vox.clone(), other.clone()]);
    assert_eq!(d.m().input_picker_title(&vox), "Vox: In 1");
    let mut apart = other;
    apart.audio_out = Some(8);
    let mut d = Doc::new(vec![vox.clone(), apart]);
    assert_eq!(d.m().input_picker_title(&vox), "In 1");
}

/// The picker never reads the sources on the UI thread: they are read
/// off it, the model answers from what it holds while the read is held,
/// and an older read released after a newer one changes nothing.
#[test]
fn the_source_list_is_read_off_the_main_thread() {
    let mut d = Doc::new(vec![keys(None)]);
    d.core.set_midi_sources(strings(&["Keys", "Pads"]));
    let hold = d.core.hold("midi_source_ports");
    d.store.refresh_inputs(d.id);
    assert_eq!(
        d.core.call_count("midi_source_ports"),
        0,
        "no read in the call"
    );
    let mut reads = d.store.take_reads();
    assert_eq!(reads.len(), 1);
    let older = reads.pop().unwrap();
    let older = std::thread::spawn(move || older.run());
    assert!(hold.wait_started(Duration::from_secs(5)));
    // The model answers before the read is released.
    assert_eq!(d.titles(31)["MIDI Input"], strings(&["All MIDI Inputs"]));
    // A newer read lands first.
    d.core.set_midi_sources(strings(&["New"]));
    d.store.refresh_inputs(d.id);
    let newer = d.store.take_reads().pop().unwrap();
    let landed = std::thread::spawn(move || newer.run()).join().unwrap();
    d.store.land(landed);
    assert_eq!(
        d.titles(31)["MIDI Input"],
        strings(&["All MIDI Inputs", "New"])
    );
    // The older one, released last, changes nothing.
    hold.release();
    d.store.land(older.join().unwrap());
    assert_eq!(
        d.titles(31)["MIDI Input"],
        strings(&["All MIDI Inputs", "New"])
    );
    assert_eq!(d.store.input_sources().midi, strings(&["New"]));
    let me = std::thread::current().id();
    let threads = d.core.read_threads("midi_source_ports");
    assert_eq!(threads.len(), 2);
    assert!(threads.iter().all(|t| *t != me), "read off the UI thread");
    assert!(
        d.core
            .read_threads("audio_input_channels")
            .iter()
            .all(|t| *t != me)
    );
}

/// A new document's first picker lists the sources the launch warm-up
/// read, before its own refresh has run.
#[test]
fn a_new_documents_first_menu_lists_the_warmed_sources() {
    let core = Arc::new(FakeCore::new());
    core.set_midi_sources(strings(&["Keys", "Pads"]));
    let mut store = DocumentStore::with_core(core.clone());
    store.warm_sources();
    assert_eq!(core.call_count("midi_source_ports"), 0, "off the UI thread");
    store.run_reads();
    assert_eq!(core.call_count("midi_source_ports"), 1);
    Doc::hand_out(&core, vec![keys(None)]);
    let id = store.new_demo().unwrap();
    let m = store.document_mut(id).unwrap();
    m.select(Some(3));
    assert_eq!(m.inputs().midi_sources(), strings(&["Keys", "Pads"]));
    let l = m.lanes()[0].clone();
    assert_eq!(
        titles(&m.inputs().groups(&l))["MIDI Input"],
        strings(&["All MIDI Inputs", "Keys", "Pads"])
    );
    assert_eq!(core.call_count("midi_source_ports"), 1, "seeded, not read");
}

// ---- The monitor button ----

#[test]
fn the_monitor_button_cycles_auto_input_off() {
    use MonitorModeRow::{Auto, Input, Off};
    assert_eq!(LaneInputs::next_monitor(Auto), Input);
    assert_eq!(LaneInputs::next_monitor(Input), Off);
    assert_eq!(LaneInputs::next_monitor(Off), Auto);
    assert_eq!(
        [Auto, Input, Off].map(LaneInputs::monitor_label),
        ["A", "I", "\u{2013}"]
    );
    assert_eq!(
        LaneInputs::monitor_help(Auto),
        "Input monitoring: Auto (heard while armed and stopped, or recording in range)"
    );
    assert_eq!(
        LaneInputs::monitor_help(Input),
        "Input monitoring: always the input"
    );
    assert_eq!(LaneInputs::monitor_help(Off), "Input monitoring: off");
    // The mixer's are the same functions.
    assert_eq!(vizdaw_app::mixer::next_monitor(Off), Auto);
    assert_eq!(
        vizdaw_app::mixer::monitor_help(Off),
        LaneInputs::monitor_help(Off)
    );
    let mut d = Doc::new(vec![audio_lane(30, None, false, Auto)]);
    let l = d.lane(30);
    d.m().cycle_lane_monitor(&l);
    assert_eq!(
        d.fake.edits().last(),
        Some(&EditCommand::Lane {
            edit: LaneEdit::SetMonitor {
                seq: 3,
                lane: 30,
                monitor: Input
            }
        })
    );
    assert_eq!(
        d.m().snapshot().undo_label.as_deref(),
        Some("Set Track Monitor")
    );
    assert_eq!(d.lane(30).monitor, Input);
}

// ---- The meters ----

#[test]
fn meters_show_armed_or_monitored_lanes_and_hold_the_clip() {
    let armed = audio_lane(
        30,
        Some(LaneInputRow::Audio {
            first: 1,
            stereo: true,
            return_of: None,
        }),
        true,
        MonitorModeRow::Auto,
    );
    let idle = audio_lane(31, mono(0), false, MonitorModeRow::Auto);
    let mut d = Doc::new(vec![armed, idle]);
    let f = d.fake.clone();
    let m = d.m();
    let mut t = Instant::now();
    assert!(m.metering());
    assert!(m.record_polls_fast(), "meters poll fast");
    f.set_input_levels(vec![0.9, 0.5, 0.7]);
    assert!(m.poll_meters(t));
    assert_eq!(m.inputs().level(30), 0.7);
    assert_eq!(
        m.inputs().level(31),
        0.0,
        "an unarmed Auto track shows no meter"
    );
    f.set_input_levels(vec![0.0, 0.0, 0.0]);
    m.poll_meters(t);
    assert_eq!(m.inputs().level(30), 0.7 * MixerMath::METER_DECAY);
    assert!(!m.inputs().is_clipped(30));
    f.set_input_levels(vec![0.0, 1.2, 0.0]);
    m.poll_meters(t);
    assert!(m.inputs().is_clipped(30));
    f.set_input_levels(vec![0.0, 0.0, 0.0]);
    t += Duration::from_millis(1_900);
    m.poll_meters(t);
    assert!(m.inputs().is_clipped(30), "held for two seconds");
    t += Duration::from_millis(200);
    m.poll_meters(t);
    assert!(!m.inputs().is_clipped(30));
    assert_eq!(CLIP_HOLD, Duration::from_secs(2));
    // A recording's clipped samples light it too.
    m.toggle_record().unwrap();
    f.set_pass_clipped(30, 3);
    m.refresh_record(false);
    m.poll_meters(t);
    assert!(m.inputs().is_clipped(30));
    // No new clipped samples: it goes out after the hold.
    t += CLIP_HOLD + Duration::from_millis(1);
    m.poll_meters(t);
    assert!(!m.inputs().is_clipped(30));
}

#[test]
fn no_meter_no_fast_poll() {
    let mut d = Doc::new(vec![audio_lane(30, mono(0), false, MonitorModeRow::Auto)]);
    let t = TransportModel::new(d.m()).unwrap();
    assert!(!d.m().metering());
    assert_eq!(t.poll_interval_for(d.m()), SLOW);
    d.fake.set_input_levels(vec![1.0]);
    let reads = d.fake.reads("input_levels");
    d.m().poll_meters(Instant::now());
    assert!(d.m().inputs().levels().is_empty() && d.m().inputs().clipped().is_empty());
    assert_eq!(
        d.fake.reads("input_levels"),
        reads,
        "no meter: no peak read"
    );
}

/// The header meters and `state.get input` read the same read-and-reset
/// peaks; neither steals the other's.
#[test]
fn the_meters_and_the_channel_each_see_every_peak() {
    let mut d = Doc::new(vec![audio_lane(30, mono(0), true, MonitorModeRow::Auto)]);
    d.fake.set_input_levels(vec![1.2]);
    assert_eq!(d.m().take_channel_levels().unwrap(), [1.2]);
    d.m().poll_meters(Instant::now());
    assert!(
        d.m().inputs().is_clipped(30),
        "the channel no longer steals the clip"
    );
    d.fake.set_input_levels(vec![0.7]);
    d.m().poll_meters(Instant::now());
    assert_eq!(
        d.m().take_channel_levels().unwrap(),
        [0.7],
        "the meters no longer steal the peak"
    );
    // A failing read keeps the hold for the next.
    d.fake.set_input_levels(vec![0.4]);
    d.m().poll_meters(Instant::now());
    d.fake.fail_next(
        "input_levels",
        VizError::Busy {
            message: "busy".into(),
        },
    );
    assert!(d.m().take_channel_levels().is_err());
    assert_eq!(d.m().take_channel_levels().unwrap(), [0.4]);
}

/// With Software Monitoring off, an Input-monitored track that is not
/// armed shows no meter, and the poll slows.
#[test]
fn software_monitoring_off_hides_an_input_monitored_meter() {
    let core = Arc::new(FakeCore::new());
    core.set_software_monitoring(true);
    let mut d = Doc::with_core(
        core.clone(),
        vec![audio_lane(30, mono(0), false, MonitorModeRow::Input)],
    );
    assert!(d.m().metering());
    d.fake.set_input_levels(vec![0.5]);
    core.set_software_monitoring(false);
    d.m().poll_meters(Instant::now());
    assert!(!d.m().metering());
    assert!(!d.m().inputs().monitoring_on());
    let t = TransportModel::new(d.m()).unwrap();
    assert_eq!(t.poll_interval_for(d.m()), SLOW);
    assert_eq!(d.m().inputs().level(30), 0.0);
}

/// Software Monitoring reaches the meters at once, not at the next poll.
#[test]
fn software_monitoring_reaches_the_meters_at_once() {
    let core = Arc::new(FakeCore::new());
    core.set_software_monitoring(true);
    let mut d = Doc::with_core(core.clone(), vec![]);
    assert!(d.m().inputs().monitoring_on());
    core.set_software_monitoring(false);
    d.m().inputs_mut().refresh_monitoring();
    assert!(!d.m().inputs().monitoring_on());
}

/// A transport read that fails (Busy while a plug-in editor opens, or an
/// error) does not freeze the meters.
#[test]
fn meters_poll_while_the_transport_read_fails() {
    let mut d = Doc::new(vec![audio_lane(30, mono(0), true, MonitorModeRow::Auto)]);
    let mut t = TransportModel::new(d.m()).unwrap();
    for (e, result) in [
        (
            VizError::Busy {
                message: "plugin host is busy".into(),
            },
            PollResult::Busy,
        ),
        (
            VizError::Internal {
                message: "boom".into(),
            },
            PollResult::Failed,
        ),
    ] {
        d.fake.set_transport_error(Some(e));
        d.fake.set_input_levels(vec![0.6]);
        let (r, changed) = t.poll_document(&mut d.store, d.id);
        assert_eq!(r, result);
        assert!(changed);
        assert_eq!(d.m().inputs().level(30), 0.6);
        d.fake.set_input_levels(vec![0.0]);
        t.poll_document(&mut d.store, d.id);
        assert_eq!(d.m().inputs().level(30), 0.6 * MixerMath::METER_DECAY);
    }
    d.fake.set_transport_error(None);
}

/// While the input opens (the microphone prompt may be up) the status
/// line says so, in the platform's words, never over another message.
#[test]
fn an_opening_input_says_so() {
    assert_eq!(
        OPENING_NOTE_MACOS,
        "Opening the audio input\u{2026} If macOS asks for microphone access, answer its prompt."
    );
    assert_eq!(OPENING_NOTE_OTHER, "Opening the audio input\u{2026}");
    assert!(OPENING_NOTE_MACOS.starts_with(OPENING_NOTE_OTHER));
    let expected = if cfg!(target_os = "macos") {
        OPENING_NOTE_MACOS
    } else {
        OPENING_NOTE_OTHER
    };
    assert_eq!(opening_note(), expected);

    let mut d = Doc::new(vec![audio_lane(30, mono(0), true, MonitorModeRow::Auto)]);
    d.fake.set_input(info(2, &["L", "R"], true));
    d.m().poll_meters(Instant::now());
    assert!(d.m().inputs().opening());
    assert_eq!(d.m().status(), Some(opening_note()));
    d.fake.set_input(info(2, &["L", "R"], false));
    d.m().poll_meters(Instant::now());
    assert!(!d.m().inputs().opening());
    assert_eq!(d.m().status(), None);
    // A refusal from the same click is not hidden by the note.
    let refusal = "Track Audio 30 records input 3, but the audio input has 2 channel(s)";
    d.m().set_status(Some(refusal.into()));
    d.fake.set_input(info(2, &["L", "R"], true));
    d.m().poll_meters(Instant::now());
    assert!(d.m().inputs().opening());
    assert_eq!(d.m().status(), Some(refusal));
    // Nor is it cleared when the input has opened.
    d.fake.set_input(info(2, &["L", "R"], false));
    d.m().poll_meters(Instant::now());
    assert_eq!(d.m().status(), Some(refusal));
}

/// Disarming clears "opening", its status line, and the meters.
#[test]
fn opening_is_cleared_on_disarm() {
    let mut d = Doc::new(vec![audio_lane(30, mono(0), true, MonitorModeRow::Auto)]);
    d.fake.set_input(info(2, &["L", "R"], true));
    d.fake.set_input_levels(vec![1.5]);
    d.m().poll_meters(Instant::now());
    assert!(d.m().inputs().opening() && d.m().status() == Some(opening_note()));
    assert!(d.m().inputs().is_clipped(30) && d.m().inputs().level(30) > 0.0);
    assert!(d.m().edit(EditCommand::Lane {
        edit: LaneEdit::SetFlags {
            seq: 3,
            lane: 30,
            mute: None,
            solo: None,
            arm: Some(false),
        },
    }));
    d.m().poll_meters(Instant::now());
    assert!(!d.m().inputs().opening());
    assert_eq!(d.m().status(), None);
    assert!(d.m().inputs().levels().is_empty() && d.m().inputs().clipped().is_empty());
}

/// The device is read from the first session whose input names one.
#[test]
fn the_input_holder_is_the_first_session_with_a_device() {
    let quiet = Arc::new(FakeSession::new(vec![]));
    let holder = Arc::new(FakeSession::new(vec![]));
    holder.set_input(info(2, &["L", "R"], false));
    let q: Arc<dyn DocSession> = quiet;
    let h: Arc<dyn DocSession> = holder;
    let found = input_holder(&[q.clone(), h.clone()]).unwrap();
    assert!(Arc::ptr_eq(&found, &h));
    assert!(input_holder(&[q]).is_none());
    assert!(input_holder(&[]).is_none());
}

/// The poll runs fast while an armed track meters with the transport
/// stopped and nothing recording, and slow once it is disarmed.
#[test]
fn an_armed_meter_polls_fast_while_stopped_and_slow_once_disarmed() {
    let mut d = Doc::new(vec![audio_lane(30, mono(0), true, MonitorModeRow::Auto)]);
    let t = TransportModel::new(d.m()).unwrap();
    assert!(!t.is_playing() && !d.m().record().is_recording() && !d.m().counting_in());
    assert_eq!(t.poll_interval_for(d.m()), FAST);
    assert!(d.m().edit(EditCommand::Lane {
        edit: LaneEdit::SetFlags {
            seq: 3,
            lane: 30,
            mute: None,
            solo: None,
            arm: Some(false),
        },
    }));
    assert_eq!(t.poll_interval_for(d.m()), SLOW);
}

// ---- The channel ----

fn track() -> Vec<(&'static str, ArgValue)> {
    vec![("seq", int(3)), ("lane", int(31))]
}

fn with(extra: &[(&'static str, ArgValue)]) -> Vec<(&'static str, ArgValue)> {
    let mut v = track();
    v.extend(extra.iter().cloned());
    v
}

#[test]
fn lane_inputs_and_monitors_are_lane_edits() {
    let mut d = Doc::new(vec![lane_row(30, "Groove", false), keys(None)]);
    let f = d.fake.clone();
    let m = d.m();
    let ran = run(
        "lane.set_input",
        &with(&[
            ("kind", text("audio")),
            ("channel", int(3)),
            ("stereo", flag(true)),
        ]),
        m,
    )
    .unwrap();
    assert_eq!(ran.undo.as_deref(), Some("Set Track Input"));
    assert_eq!(
        f.edits().last(),
        Some(&EditCommand::Lane {
            edit: LaneEdit::SetInput {
                seq: 3,
                lane: 31,
                input: Some(LaneInputRow::Audio {
                    first: 2,
                    stereo: true,
                    return_of: None
                })
            }
        })
    );
    run(
        "lane.set_input",
        &with(&[
            ("kind", text("midi")),
            ("port", text("Keys")),
            ("midi_channel", int(16)),
        ]),
        m,
    )
    .unwrap();
    assert_eq!(
        f.edits().last(),
        Some(&EditCommand::Lane {
            edit: LaneEdit::SetInput {
                seq: 3,
                lane: 31,
                input: Some(LaneInputRow::Midi {
                    port: Some("Keys".into()),
                    channel: Some(15)
                })
            }
        })
    );
    run("lane.set_input", &with(&[("kind", text("none"))]), m).unwrap();
    assert_eq!(
        f.edits().last(),
        Some(&EditCommand::Lane {
            edit: LaneEdit::SetInput {
                seq: 3,
                lane: 31,
                input: None
            }
        })
    );
    let ran = run("lane.set_monitor", &with(&[("mode", text("off"))]), m).unwrap();
    assert_eq!(ran.undo.as_deref(), Some("Set Track Monitor"));
    assert_eq!(
        f.edits().last(),
        Some(&EditCommand::Lane {
            edit: LaneEdit::SetMonitor {
                seq: 3,
                lane: 31,
                monitor: MonitorModeRow::Off
            }
        })
    );
    // A synth's return must be an external destination.
    f.set_destinations(vec![
        dest_row(5, "Plug"),
        ext_dest(6, "Moog", Default::default()),
    ]);
    let e = run(
        "lane.set_input",
        &with(&[("kind", text("audio")), ("return_of", int(5))]),
        m,
    )
    .unwrap_err();
    assert_eq!(e.code, ErrorCode::InvalidParams);
    assert!(
        e.message.contains("not an external MIDI destination"),
        "{}",
        e.message
    );
    run(
        "lane.set_input",
        &with(&[("kind", text("audio")), ("return_of", int(6))]),
        m,
    )
    .unwrap();
    // The core's refusal is the call's failure.
    f.set_edit_error(Some(VizError::Model {
        message: "input.channels out of range".into(),
    }));
    let e = run("lane.set_input", &with(&[("kind", text("audio"))]), m).unwrap_err();
    assert_eq!(e.code, ErrorCode::Failed);
    assert!(e.message.contains("input.channels"), "{}", e.message);
    // The core's Busy is `busy`, not `failed`.
    for message in [
        "Finish the open edit first.",
        "Stop recording to change which tracks record.",
    ] {
        f.set_edit_error(Some(VizError::Busy {
            message: message.into(),
        }));
        let e = run("lane.set_input", &with(&[("kind", text("none"))]), m).unwrap_err();
        assert_eq!((e.code, e.message.as_str()), (ErrorCode::Busy, message));
    }
    f.set_edit_error(None);
}

/// Busy while a pass records: the channel's call is `busy`, a picker
/// choice says so in the status line, and the input stays.
#[test]
fn an_input_change_while_recording_is_busy() {
    let mut d = Doc::new(vec![audio_lane(30, mono(0), true, MonitorModeRow::Auto)]);
    let f = d.fake.clone();
    let m = d.m();
    m.toggle_record().unwrap();
    let edits = f.edits().len();
    let e = run(
        "lane.set_input",
        &[
            ("seq", int(3)),
            ("lane", int(30)),
            ("kind", text("audio")),
            ("channel", int(2)),
        ],
        m,
    )
    .unwrap_err();
    assert_eq!(e.code, ErrorCode::Busy);
    assert_eq!(e.message, vizdaw_ffi::session::RECORDING_LANES);
    let l = m.lanes()[0].clone();
    let pair = m.inputs().groups(&l)[1].choices.last().unwrap().clone();
    m.choose_input(&pair, &l);
    assert_eq!(m.status(), Some(vizdaw_ffi::session::RECORDING_LANES));
    assert_eq!(f.edits().len(), edits);
    assert_eq!(m.lanes()[0].input, mono(0));
}

#[test]
fn hostile_lane_params_are_invalid() {
    let mut d = Doc::new(vec![keys(None)]);
    let f = d.fake.clone();
    let m = d.m();
    let before = f.edits().len();
    for extra in [
        vec![("kind", text("video"))],
        vec![("kind", text("audio")), ("channel", int(0))],
        vec![("kind", text("audio")), ("channel", int(257))],
        vec![("kind", text("audio")), ("return_of", int(-1))],
        vec![("kind", text("midi")), ("midi_channel", int(17))],
        vec![("kind", text("midi")), ("midi_channel", int(0))],
        vec![("kind", int(1))],
        vec![],
    ] {
        let e = run("lane.set_input", &with(&extra), m).unwrap_err();
        assert_eq!(e.code, ErrorCode::InvalidParams, "{extra:?}");
    }
    let e = run("lane.set_monitor", &with(&[("mode", text("loud"))]), m).unwrap_err();
    assert_eq!(e.code, ErrorCode::InvalidParams);
    let e = run("lane.set_monitor", &with(&[("mode", text("OFF"))]), m).unwrap_err();
    assert_eq!(e.code, ErrorCode::InvalidParams);
    let e = run(
        "lane.set_monitor",
        &[("seq", int(-1)), ("lane", int(31)), ("mode", text("off"))],
        m,
    )
    .unwrap_err();
    assert_eq!(e.code, ErrorCode::InvalidParams);
    // Wrong types are not the ids.
    let e = run(
        "lane.set_monitor",
        &[("seq", text("3")), ("lane", int(31)), ("mode", text("off"))],
        m,
    )
    .unwrap_err();
    assert_eq!(e.code, ErrorCode::InvalidParams);
    let e = run("lane.set_monitor", &with(&[("volume", int(3))]), m).unwrap_err();
    assert_eq!(e.code, ErrorCode::InvalidParams, "an unknown parameter");
    assert_eq!(f.edits().len(), before);
    // An open drag refuses them.
    assert!(m.begin_gesture("Move", vec![]));
    let e = run("lane.set_monitor", &with(&[("mode", text("off"))]), m).unwrap_err();
    assert_eq!(e.code, ErrorCode::Unavailable);
    assert!(!commands::enabled("lane.set_monitor", m));
    assert_eq!(f.edits().len(), before);
}

/// Parameters that do not belong to the kind are refused.
#[test]
fn lane_input_params_must_fit_the_kind() {
    let mut d = Doc::new(vec![keys(None)]);
    let f = d.fake.clone();
    let m = d.m();
    for extra in [
        vec![("kind", text("audio")), ("port", text("Keys"))],
        vec![("kind", text("audio")), ("midi_channel", int(2))],
        vec![("kind", text("midi")), ("channel", int(2))],
        vec![("kind", text("midi")), ("stereo", flag(true))],
        vec![("kind", text("midi")), ("return_of", int(6))],
        vec![("kind", text("none")), ("channel", int(2))],
        vec![("kind", text("none")), ("port", text("Keys"))],
    ] {
        let e = run("lane.set_input", &with(&extra), m).unwrap_err();
        assert_eq!(e.code, ErrorCode::InvalidParams, "{extra:?}");
    }
    assert!(f.edits().is_empty());
    assert!(commands::enabled("lane.set_input", m));
    run(
        "lane.set_input",
        &with(&[("kind", text("audio")), ("channel", int(2))]),
        m,
    )
    .unwrap();
    assert_eq!(f.edits().len(), 1);
}
