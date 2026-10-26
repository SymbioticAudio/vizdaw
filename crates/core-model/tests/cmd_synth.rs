//! Tracking a hardware synth in the model (plan 5c-2 Tasks 2 and 3;
//! sub-project 5 spec §4.5, §4.5a, §6.7): an audio input can record the
//! return of an external MIDI destination, the destination carries its
//! return latency as an undoable edit, removing the synth relinks or
//! unlinks the return lane, files write both only when set, and Track
//! Synth sets the return lane up in one step.

use core_model::ModelError;
use core_model::command::{Command, LaneCmd, RoutingCmd, SequenceCmd, apply};
use core_model::document::{DestKind, Document};
use core_model::file::{from_json, to_json};
use core_model::history::History;
use core_model::ids::{AnyId, DestId, LaneId, SeqId};
use core_model::lane::{InputChannels, LaneInput, MonitorMode};
use core_model::selection::ViewFilter;
use serde_json::{Value, json};

struct Doc {
    doc: Document,
    /// An external synth ("Juno"), which lane 0 plays.
    juno: DestId,
    /// Another external synth, played by nothing.
    moog: DestId,
    /// A plugin instrument.
    pad: DestId,
    /// The built-in synth.
    synth: DestId,
}

fn run(d: &mut Document, cmd: Command) -> core_model::command::Report {
    let a = apply(d, &cmd).unwrap();
    *d = a.doc;
    a.report
}

fn add_dest(d: &mut Document, name: &str, kind: DestKind) -> DestId {
    let r = run(
        d,
        Command::Routing(RoutingCmd::AddDestination {
            name: name.into(),
            kind,
            default_patch: None,
        }),
    );
    r.created
        .iter()
        .find_map(|id| match id {
            AnyId::Dest(x) => Some(*x),
            _ => None,
        })
        .unwrap()
}

fn endpoint(name: &str) -> DestKind {
    DestKind::MidiEndpoint {
        endpoint_ref: name.into(),
        group: 0,
        channel: 0,
    }
}

/// Sequence 0 with lane 0 ("Part") playing "Juno"; no output path.
fn doc() -> Doc {
    let mut d = apply(
        &Document::new(),
        &Command::Sequence(SequenceCmd::New { name: "S".into() }),
    )
    .unwrap()
    .doc;
    let juno = add_dest(&mut d, "Juno", endpoint("Juno"));
    let moog = add_dest(&mut d, "Moog", endpoint("Moog"));
    let pad = add_dest(&mut d, "Pad", DestKind::plugin("au:pad"));
    let synth = add_dest(&mut d, "Synth", endpoint("builtin:synth"));
    run(
        &mut d,
        Command::Lane(LaneCmd::Rename {
            seq: SeqId(0),
            lane: LaneId(0),
            name: "Part".into(),
        }),
    );
    run(
        &mut d,
        Command::Lane(LaneCmd::SetDest {
            seq: SeqId(0),
            lane: LaneId(0),
            dest: Some(juno),
        }),
    );
    Doc {
        doc: d,
        juno,
        moog,
        pad,
        synth,
    }
}

fn latency(d: &Document, dest: DestId) -> f32 {
    d.instruments().get(&dest).unwrap().return_latency_ms
}

fn set_latency(dest: DestId, ms: f32) -> Command {
    Command::Routing(RoutingCmd::SetReturnLatency { dest, ms })
}

fn set_input(lane: LaneId, input: Option<LaneInput>) -> Command {
    Command::Lane(LaneCmd::SetInput {
        seq: SeqId(0),
        lane,
        input,
    })
}

fn returning(channels: InputChannels, dest: DestId) -> LaneInput {
    LaneInput::Audio {
        channels,
        return_of: Some(dest),
    }
}

/// Adds lane "Rec" (index 1) recording input 3 as `dest`'s return.
fn with_return_lane(d: &mut Document, dest: DestId) -> LaneId {
    let r = run(
        d,
        Command::Lane(LaneCmd::Add {
            seq: SeqId(0),
            name: "Rec".into(),
            index: None,
        }),
    );
    let lane = r
        .created
        .iter()
        .find_map(|id| match id {
            AnyId::Lane(l) => Some(*l),
            _ => None,
        })
        .unwrap();
    run(
        d,
        set_input(lane, Some(returning(InputChannels::Mono(2), dest))),
    );
    lane
}

#[test]
fn return_latency_is_an_undoable_edit_of_external_destinations_only() {
    let d = doc();
    let mut h = History::new(d.doc.clone());
    let f = ViewFilter::default();
    assert_eq!(latency(h.doc(), d.juno), 0.0);
    h.execute(&set_latency(d.juno, 12.5), &f).unwrap();
    assert_eq!(h.undo_label(), Some("Set Return Latency"));
    assert_eq!(latency(h.doc(), d.juno), 12.5);
    h.execute(&set_latency(d.juno, 500.0), &f).unwrap();
    assert_eq!(latency(h.doc(), d.juno), 500.0);
    h.undo().unwrap();
    h.undo().unwrap();
    assert_eq!(latency(h.doc(), d.juno), 0.0);
    // Not external, unknown, or out of range: refused, nothing changes.
    for (dest, ms) in [
        (d.pad, 1.0),
        (d.synth, 1.0),
        (DestId(99), 1.0),
        (d.juno, -0.5),
        (d.juno, 500.5),
        (d.juno, f32::NAN),
        (d.juno, f32::INFINITY),
    ] {
        assert!(
            apply(&d.doc, &set_latency(dest, ms)).is_err(),
            "{dest} {ms}"
        );
    }
    // An edit of the synth's kind keeps its latency.
    let mut doc = apply(&d.doc, &set_latency(d.juno, 7.0)).unwrap().doc;
    run(
        &mut doc,
        Command::Routing(RoutingCmd::EditDestination {
            dest: d.juno,
            kind: endpoint("Juno II"),
            default_patch: None,
        }),
    );
    assert_eq!(latency(&doc, d.juno), 7.0);
}

#[test]
fn return_of_names_an_external_destination() {
    let d = doc();
    let lane = LaneId(0);
    let ok = apply(
        &d.doc,
        &set_input(lane, Some(returning(InputChannels::Stereo(4), d.juno))),
    )
    .unwrap()
    .doc;
    let input = ok.lane(SeqId(0), lane).unwrap().input.clone().unwrap();
    assert_eq!(input.return_of(), Some(d.juno));
    for bad in [d.pad, d.synth, DestId(99)] {
        assert!(
            apply(
                &d.doc,
                &set_input(lane, Some(returning(InputChannels::Mono(0), bad)))
            )
            .is_err(),
            "{bad}"
        );
    }
    // A plain audio input names none.
    let plain = LaneInput::Audio {
        channels: InputChannels::Mono(0),
        return_of: None,
    };
    assert_eq!(plain.return_of(), None);
    assert!(apply(&d.doc, &set_input(lane, Some(plain))).is_ok());
}

fn lane_json(text: &str, index: usize) -> Value {
    let v: Value = serde_json::from_str(text).unwrap();
    v["document"]["sequences"][0]["lanes"][index].clone()
}

fn dest_json(text: &str, name: &str) -> Value {
    let v: Value = serde_json::from_str(text).unwrap();
    v["document"]["instruments"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["name"] == name)
        .unwrap()
        .clone()
}

#[test]
fn files_write_the_link_and_the_latency_only_when_set() {
    let d = doc();
    let text = to_json(&d.doc).unwrap();
    assert!(dest_json(&text, "Juno").get("return_latency_ms").is_none());
    assert_eq!(from_json(&text).unwrap(), d.doc);
    let mut doc = d.doc.clone();
    let rec = with_return_lane(&mut doc, d.juno);
    run(&mut doc, set_latency(d.juno, 12.5));
    let plain = LaneInput::Audio {
        channels: InputChannels::Mono(1),
        return_of: None,
    };
    run(&mut doc, set_input(LaneId(0), Some(plain)));
    let text = to_json(&doc).unwrap();
    assert_eq!(dest_json(&text, "Juno")["return_latency_ms"], json!(12.5));
    assert!(dest_json(&text, "Moog").get("return_latency_ms").is_none());
    assert_eq!(
        lane_json(&text, 1)["input"],
        json!({"audio": {"channels": {"mono": 2}, "return_of": d.juno.0}})
    );
    assert_eq!(
        lane_json(&text, 0)["input"],
        json!({"audio": {"channels": {"mono": 1}}})
    );
    assert_eq!(from_json(&text).unwrap(), doc);
    assert_eq!(
        doc.lane(SeqId(0), rec)
            .unwrap()
            .input
            .as_ref()
            .unwrap()
            .return_of(),
        Some(d.juno)
    );
}

#[test]
fn a_damaged_link_or_latency_in_a_file_is_refused() {
    let d = doc();
    let mut doc = d.doc.clone();
    with_return_lane(&mut doc, d.juno);
    let text = to_json(&doc).unwrap();
    let base: Value = serde_json::from_str(&text).unwrap();
    let corrupt = |v: &Value, needle: &str| match from_json(&serde_json::to_string(v).unwrap()) {
        Err(ModelError::Corrupt { problems }) => assert!(
            problems.iter().any(|p| p.contains(needle)),
            "{needle}: {problems:?}"
        ),
        other => panic!("{needle}: {other:?}"),
    };
    let mut v = base.clone();
    v["document"]["sequences"][0]["lanes"][1]["input"]["audio"]["return_of"] = json!(77);
    corrupt(&v, "77");
    for ms in [json!(900.0), json!(-1.0)] {
        let mut v = base.clone();
        v["document"]["instruments"][0]["return_latency_ms"] = ms;
        corrupt(&v, "return_latency_ms");
    }
}

#[test]
fn removing_the_synth_relinks_or_unlinks_its_return_lane() {
    let d = doc();
    let mut doc = d.doc.clone();
    let rec = with_return_lane(&mut doc, d.juno);
    let link = |doc: &Document| {
        doc.lane(SeqId(0), rec)
            .unwrap()
            .input
            .as_ref()
            .unwrap()
            .return_of()
    };
    let remove = |doc: &Document, replacement| {
        apply(
            doc,
            &Command::Routing(RoutingCmd::RemoveDestination {
                dest: d.juno,
                replacement,
            }),
        )
    };
    // An external replacement takes the link along.
    assert_eq!(link(&remove(&doc, Some(d.moog)).unwrap().doc), Some(d.moog));
    // A plugin replacement unlinks it (lane 0 now plays the plugin).
    assert_eq!(link(&remove(&doc, Some(d.pad)).unwrap().doc), None);
    // A link alone is no use: with nothing playing the synth, no
    // replacement is needed, and the lane keeps its channels.
    run(
        &mut doc,
        Command::Lane(LaneCmd::SetDest {
            seq: SeqId(0),
            lane: LaneId(0),
            dest: None,
        }),
    );
    let gone = remove(&doc, None).unwrap().doc;
    assert_eq!(link(&gone), None);
    assert_eq!(
        gone.lane(SeqId(0), rec).unwrap().input,
        Some(LaneInput::Audio {
            channels: InputChannels::Mono(2),
            return_of: None,
        })
    );
    assert_eq!(gone.lane(SeqId(0), rec).unwrap().monitor, MonitorMode::Auto);
}

fn link_of(doc: &Document, lane: LaneId) -> Option<DestId> {
    doc.lane(SeqId(0), lane)
        .unwrap()
        .input
        .as_ref()
        .and_then(LaneInput::return_of)
}

fn edit_kind(dest: DestId, kind: DestKind) -> Command {
    Command::Routing(RoutingCmd::EditDestination {
        dest,
        kind,
        default_patch: None,
    })
}

#[test]
fn making_a_synth_non_external_clears_its_links_and_latency_in_one_step() {
    let d = doc();
    let mut doc = d.doc.clone();
    let rec = with_return_lane(&mut doc, d.juno);
    run(&mut doc, set_latency(d.juno, 12.5));
    let f = ViewFilter::default();
    for new_kind in [DestKind::plugin("au:pad"), endpoint("builtin:synth")] {
        let mut h = History::new(doc.clone());
        h.execute(&edit_kind(d.juno, new_kind), &f).unwrap();
        assert_eq!(h.undo_label(), Some("Edit Destination"));
        assert_eq!(link_of(h.doc(), rec), None);
        assert_eq!(latency(h.doc(), d.juno), 0.0);
        // The lane keeps its channels.
        assert!(
            h.doc()
                .lane(SeqId(0), rec)
                .unwrap()
                .input
                .as_ref()
                .unwrap()
                .is_audio()
        );
        h.undo().unwrap();
        assert_eq!(link_of(h.doc(), rec), Some(d.juno));
        assert_eq!(latency(h.doc(), d.juno), 12.5);
        h.redo().unwrap();
        assert_eq!(link_of(h.doc(), rec), None);
        assert_eq!(latency(h.doc(), d.juno), 0.0);
    }
    // External to external keeps both.
    let kept = apply(&doc, &edit_kind(d.juno, endpoint("Juno II")))
        .unwrap()
        .doc;
    assert_eq!(link_of(&kept, rec), Some(d.juno));
    assert_eq!(latency(&kept, d.juno), 12.5);
}

#[test]
fn a_file_linking_or_delaying_a_non_external_destination_is_refused() {
    let d = doc();
    let mut doc = d.doc.clone();
    with_return_lane(&mut doc, d.juno);
    let base: Value = serde_json::from_str(&to_json(&doc).unwrap()).unwrap();
    let corrupt = |v: &Value, needle: &str| match from_json(&serde_json::to_string(v).unwrap()) {
        Err(ModelError::Corrupt { problems }) => assert!(
            problems.iter().any(|p| p.contains(needle)),
            "{needle}: {problems:?}"
        ),
        other => panic!("{needle}: {other:?}"),
    };
    let idx = |name: &str| {
        base["document"]["instruments"]
            .as_array()
            .unwrap()
            .iter()
            .position(|x| x["name"] == name)
            .unwrap()
    };
    // The link names a plugin, then the built-in synth.
    for target in [d.pad, d.synth] {
        let mut v = base.clone();
        v["document"]["sequences"][0]["lanes"][1]["input"]["audio"]["return_of"] = json!(target.0);
        corrupt(&v, "return_of");
    }
    // A latency on a plugin, then on the built-in synth.
    for name in ["Pad", "Synth"] {
        let mut v = base.clone();
        v["document"]["instruments"][idx(name)]["return_latency_ms"] = json!(5.0);
        corrupt(&v, "return_latency_ms");
    }
}

#[test]
fn input_and_removal_edits_undo_and_redo_the_link() {
    let d = doc();
    let f = ViewFilter::default();
    let mut h = History::new(d.doc.clone());
    let input = set_input(LaneId(0), Some(returning(InputChannels::Mono(2), d.juno)));
    h.execute(&input, &f).unwrap();
    assert_eq!(link_of(h.doc(), LaneId(0)), Some(d.juno));
    h.undo().unwrap();
    assert_eq!(h.doc().lane(SeqId(0), LaneId(0)).unwrap().input, None);
    h.redo().unwrap();
    assert_eq!(link_of(h.doc(), LaneId(0)), Some(d.juno));
    let remove = Command::Routing(RoutingCmd::RemoveDestination {
        dest: d.juno,
        replacement: Some(d.moog),
    });
    h.execute(&remove, &f).unwrap();
    assert_eq!(link_of(h.doc(), LaneId(0)), Some(d.moog));
    h.undo().unwrap();
    assert_eq!(link_of(h.doc(), LaneId(0)), Some(d.juno));
    h.redo().unwrap();
    assert_eq!(link_of(h.doc(), LaneId(0)), Some(d.moog));
}

#[test]
fn a_relinked_lane_takes_the_new_synths_latency_not_the_old() {
    // The link moves with the lane; the latency belongs to the synth.
    let d = doc();
    let mut doc = d.doc.clone();
    let rec = with_return_lane(&mut doc, d.juno);
    run(&mut doc, set_latency(d.juno, 12.5));
    run(&mut doc, set_latency(d.moog, 3.0));
    let moved = apply(
        &doc,
        &Command::Routing(RoutingCmd::RemoveDestination {
            dest: d.juno,
            replacement: Some(d.moog),
        }),
    )
    .unwrap()
    .doc;
    assert_eq!(link_of(&moved, rec), Some(d.moog));
    assert_eq!(latency(&moved, d.moog), 3.0);
}
