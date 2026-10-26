//! Lane inputs and monitor modes (plan 5c Task 2; sub-project 5 spec §4.5,
//! §6.6): what a lane records and when its input is heard, as undoable
//! lane edits that old files read without and new files write only when
//! set.

use core_model::ModelError;
use core_model::command::{Command, LaneCmd, SequenceCmd, apply};
use core_model::document::Document;
use core_model::file::{from_json, to_json};
use core_model::history::History;
use core_model::ids::{LaneId, SeqId};
use core_model::lane::{InputChannels, LaneInput, MAX_INPUT_CHANNEL, MonitorMode};
use core_model::selection::ViewFilter;
use serde_json::{Value, json};

fn doc() -> Document {
    apply(
        &Document::new(),
        &Command::Sequence(SequenceCmd::New { name: "S".into() }),
    )
    .unwrap()
    .doc
}

fn set_input(input: Option<LaneInput>) -> Command {
    Command::Lane(LaneCmd::SetInput {
        seq: SeqId(0),
        lane: LaneId(0),
        input,
    })
}

fn lane(d: &Document) -> &core_model::lane::Lane {
    d.lane(SeqId(0), LaneId(0)).unwrap()
}

#[test]
fn input_and_monitor_are_undoable_lane_edits() {
    let mut h = History::new(doc());
    let f = ViewFilter::default();
    assert_eq!(lane(h.doc()).input, None);
    assert_eq!(lane(h.doc()).monitor, MonitorMode::Auto);
    let audio = LaneInput::Audio {
        channels: InputChannels::Stereo(2),
        return_of: None,
    };
    h.execute(&set_input(Some(audio.clone())), &f).unwrap();
    assert_eq!(h.undo_label(), Some("Set Lane Input"));
    h.execute(
        &Command::Lane(LaneCmd::SetMonitor {
            seq: SeqId(0),
            lane: LaneId(0),
            monitor: MonitorMode::Input,
        }),
        &f,
    )
    .unwrap();
    assert_eq!(h.undo_label(), Some("Set Lane Monitor"));
    assert_eq!(lane(h.doc()).input, Some(audio));
    assert_eq!(lane(h.doc()).monitor, MonitorMode::Input);
    h.undo().unwrap();
    assert_eq!(lane(h.doc()).monitor, MonitorMode::Auto);
    h.undo().unwrap();
    assert_eq!(lane(h.doc()).input, None);
    let midi = LaneInput::Midi {
        port: Some("Keys".into()),
        channel: Some(15),
    };
    h.execute(&set_input(Some(midi.clone())), &f).unwrap();
    assert_eq!(lane(h.doc()).input, Some(midi));
    h.execute(&set_input(None), &f).unwrap();
    assert_eq!(lane(h.doc()).input, None);
}

#[test]
fn hostile_inputs_are_refused_and_change_nothing() {
    let d = doc();
    let long = "x".repeat(256);
    for bad in [
        LaneInput::Audio {
            channels: InputChannels::Mono(MAX_INPUT_CHANNEL + 1),
            return_of: None,
        },
        LaneInput::Audio {
            channels: InputChannels::Stereo(MAX_INPUT_CHANNEL),
            return_of: None,
        },
        LaneInput::Audio {
            channels: InputChannels::Stereo(u16::MAX),
            return_of: None,
        },
        LaneInput::Midi {
            port: Some(String::new()),
            channel: None,
        },
        LaneInput::Midi {
            port: Some(long),
            channel: None,
        },
        LaneInput::Midi {
            port: Some("a\nb".into()),
            channel: None,
        },
        LaneInput::Midi {
            port: None,
            channel: Some(16),
        },
    ] {
        match apply(&d, &set_input(Some(bad.clone()))) {
            Err(ModelError::OutOfRange { .. }) => {}
            other => panic!("{bad:?}: {other:?}"),
        }
    }
    assert!(
        apply(
            &d,
            &Command::Lane(LaneCmd::SetMonitor {
                seq: SeqId(0),
                lane: LaneId(9),
                monitor: MonitorMode::Off,
            })
        )
        .is_err()
    );
    // The highest pair and channel are fine.
    let ok = LaneInput::Audio {
        channels: InputChannels::Stereo(MAX_INPUT_CHANNEL - 1),
        return_of: None,
    };
    assert!(apply(&d, &set_input(Some(ok))).is_ok());
}

fn lane_json(text: &str) -> Value {
    let v: Value = serde_json::from_str(text).unwrap();
    v["document"]["sequences"][0]["lanes"][0].clone()
}

#[test]
fn files_write_inputs_only_when_set_and_old_lanes_read_without_them() {
    let d = doc();
    let text = to_json(&d).unwrap();
    let l = lane_json(&text);
    assert!(l.get("input").is_none(), "{l}");
    assert!(l.get("monitor").is_none(), "{l}");
    // A lane written before 5c (no keys) reads as no input, Auto.
    assert_eq!(from_json(&text).unwrap(), d);
    let d2 = apply(
        &d,
        &set_input(Some(LaneInput::Midi {
            port: None,
            channel: Some(3),
        })),
    )
    .unwrap()
    .doc;
    let d2 = apply(
        &d2,
        &Command::Lane(LaneCmd::SetMonitor {
            seq: SeqId(0),
            lane: LaneId(0),
            monitor: MonitorMode::Off,
        }),
    )
    .unwrap()
    .doc;
    let text = to_json(&d2).unwrap();
    let l = lane_json(&text);
    assert_eq!(l["input"], json!({"midi": {"port": null, "channel": 3}}));
    assert_eq!(l["monitor"], json!("off"));
    assert_eq!(from_json(&text).unwrap(), d2);
}

#[test]
fn a_damaged_input_in_a_file_is_refused() {
    let text = to_json(&doc()).unwrap();
    let mut v: Value = serde_json::from_str(&text).unwrap();
    v["document"]["sequences"][0]["lanes"][0]["input"] =
        json!({"audio": {"channels": {"mono": 4000}}});
    let bad = serde_json::to_string(&v).unwrap();
    match from_json(&bad) {
        Err(ModelError::Corrupt { problems }) => {
            assert!(
                problems.iter().any(|p| p.contains("input.channels")),
                "{problems:?}"
            );
        }
        other => panic!("{other:?}"),
    }
    v["document"]["sequences"][0]["lanes"][0]["input"] =
        json!({"audio": {"channels": {"quad": 0}}});
    assert!(from_json(&serde_json::to_string(&v).unwrap()).is_err());
}

/// Final review T2: the range error names the channel out of range — the
/// right one of a pair past the last channel, not the pair's first.
#[test]
fn the_range_error_names_the_channel_out_of_range() {
    let d = doc();
    for (channels, named) in [
        (InputChannels::Mono(MAX_INPUT_CHANNEL + 1), "256"),
        (InputChannels::Stereo(MAX_INPUT_CHANNEL), "256"),
        (InputChannels::Stereo(u16::MAX), "65536"),
    ] {
        match apply(
            &d,
            &set_input(Some(LaneInput::Audio {
                channels,
                return_of: None,
            })),
        ) {
            Err(ModelError::OutOfRange { field, value }) => {
                assert_eq!(field, "input.channels");
                assert_eq!(value, named, "{channels:?}");
            }
            other => panic!("{channels:?}: {other:?}"),
        }
    }
}
