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

fn track(lane: LaneId, channels: InputChannels, arm_midi: bool) -> Command {
    Command::Lane(LaneCmd::TrackSynth {
        seq: SeqId(0),
        lane,
        channels,
        arm_midi,
    })
}

/// Spec §6.7, §12.3: Track Synth sets up and arms in one undo step: a lane
/// "<synth> audio" just below records the chosen channels as the synth's
/// return, on the first output path (made when there is none), armed and
/// monitoring Auto; the MIDI lane is disarmed so its part plays (plan
/// 5c-2 Ruling 5).
#[test]
fn track_synth_sets_up_and_arms_in_one_undo_step() {
    let d = doc();
    let mut doc = d.doc.clone();
    run(
        &mut doc,
        Command::Lane(LaneCmd::Add {
            seq: SeqId(0),
            name: "Below".into(),
            index: None,
        }),
    );
    run(
        &mut doc,
        Command::Lane(LaneCmd::SetFlags {
            seq: SeqId(0),
            lane: LaneId(0),
            mute: None,
            solo: None,
            arm: Some(true),
        }),
    );
    let mut h = History::new(doc.clone());
    let f = ViewFilter::default();
    let r = h
        .execute(&track(LaneId(0), InputChannels::Mono(2), false), &f)
        .unwrap();
    assert_eq!(h.undo_label(), Some("Track Synth"));
    let s = h.doc().sequence(SeqId(0)).unwrap();
    let names: Vec<&str> = s.lanes.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, vec!["Part", "Juno audio", "Below"]);
    let rec = &s.lanes[1];
    assert!(r.created.contains(&AnyId::Lane(rec.id)));
    assert_eq!(rec.input, Some(returning(InputChannels::Mono(2), d.juno)));
    assert!(rec.arm);
    assert_eq!(rec.monitor, MonitorMode::Auto);
    let out = rec.audio_out.unwrap();
    assert_eq!(h.doc().outputs().get(&out).unwrap().name, "Audio 1");
    assert!(r.created.contains(&AnyId::Output(out)));
    assert!(!s.lanes[0].arm, "the MIDI lane plays, unarmed");
    let done = h.doc().clone();
    h.undo().unwrap();
    assert_eq!(h.doc(), &doc, "one step undoes all of it");
    h.redo().unwrap();
    assert_eq!(h.doc(), &done, "and one step redoes all of it");
    assert_eq!(h.undo_label(), Some("Track Synth"));
}

/// Undo and redo of the reuse path: no lane is added; the input and the
/// arm states come back.
#[test]
fn track_synth_on_a_reused_lane_undoes_and_redoes() {
    let d = doc();
    let mut doc = d.doc.clone();
    let rec = with_return_lane(&mut doc, d.juno);
    run(
        &mut doc,
        Command::Lane(LaneCmd::SetFlags {
            seq: SeqId(0),
            lane: LaneId(0),
            mute: None,
            solo: None,
            arm: Some(true),
        }),
    );
    let mut h = History::new(doc.clone());
    let f = ViewFilter::default();
    let r = h
        .execute(&track(LaneId(0), InputChannels::Stereo(4), false), &f)
        .unwrap();
    assert!(!r.created.iter().any(|id| matches!(id, AnyId::Lane(_))));
    let done = h.doc().clone();
    let l = done.lane(SeqId(0), rec).unwrap();
    assert!(l.arm);
    assert_eq!(l.input, Some(returning(InputChannels::Stereo(4), d.juno)));
    h.undo().unwrap();
    assert_eq!(h.doc(), &doc);
    let l = h.doc().lane(SeqId(0), rec).unwrap();
    assert!(!l.arm);
    assert_eq!(l.input, Some(returning(InputChannels::Mono(2), d.juno)));
    assert!(h.doc().lane(SeqId(0), LaneId(0)).unwrap().arm);
    h.redo().unwrap();
    assert_eq!(h.doc(), &done);
}

/// A lane returning another synth is never reused; of several lanes
/// returning the same synth the first by order is, and the rest are left
/// alone.
#[test]
fn track_synth_reuse_skips_other_synths_and_takes_the_first_candidate() {
    let d = doc();
    let mut doc = d.doc.clone();
    let moog_rec = with_return_lane(&mut doc, d.moog);
    let juno_a = with_return_lane(&mut doc, d.juno);
    let juno_b = with_return_lane(&mut doc, d.juno);
    let s = doc.sequence(SeqId(0)).unwrap();
    assert_eq!(s.return_lane(d.juno, None), Some(juno_a), "first in order");
    assert_eq!(s.return_lane(d.juno, Some(juno_a)), Some(juno_b));
    assert_eq!(s.return_lane(d.moog, None), Some(moog_rec));
    run(&mut doc, track(LaneId(0), InputChannels::Mono(7), false));
    let s = doc.sequence(SeqId(0)).unwrap();
    assert_eq!(s.lanes.len(), 4, "no lane added");
    let mono7 = Some(returning(InputChannels::Mono(7), d.juno));
    assert_eq!(doc.lane(SeqId(0), juno_a).unwrap().input, mono7);
    assert!(doc.lane(SeqId(0), juno_a).unwrap().arm);
    for other in [juno_b, moog_rec] {
        let l = doc.lane(SeqId(0), other).unwrap();
        assert_eq!(
            l.input.as_ref().map(|i| i.return_of().is_some()),
            Some(true)
        );
        assert_ne!(l.input, mono7);
        assert!(!l.arm, "left alone");
    }
    // With only another synth's return lane, a new lane is added.
    let mut doc = d.doc.clone();
    let moog_rec = with_return_lane(&mut doc, d.moog);
    run(&mut doc, track(LaneId(0), InputChannels::Mono(7), false));
    let s = doc.sequence(SeqId(0)).unwrap();
    assert_eq!(s.lanes.len(), 3);
    assert_eq!(
        doc.lane(SeqId(0), moog_rec).unwrap().input,
        Some(returning(InputChannels::Mono(2), d.moog))
    );
}

/// A refused Track Synth leaves the document and the undo history as they
/// were.
#[test]
fn a_refused_track_synth_changes_nothing() {
    let d = doc();
    let mut doc = d.doc.clone();
    let r = run(
        &mut doc,
        Command::Lane(LaneCmd::Add {
            seq: SeqId(0),
            name: "Pad lane".into(),
            index: None,
        }),
    );
    let pad_lane = r
        .created
        .iter()
        .find_map(|id| match id {
            AnyId::Lane(l) => Some(*l),
            _ => None,
        })
        .unwrap();
    run(
        &mut doc,
        Command::Lane(LaneCmd::SetDest {
            seq: SeqId(0),
            lane: pad_lane,
            dest: Some(d.pad),
        }),
    );
    let mut h = History::new(doc.clone());
    let f = ViewFilter::default();
    let bad = [
        track(pad_lane, InputChannels::Mono(0), false),
        track(LaneId(99), InputChannels::Mono(0), false),
        track(LaneId(0), InputChannels::Mono(256), true),
        track(LaneId(0), InputChannels::Stereo(255), true),
        Command::Lane(LaneCmd::TrackSynth {
            seq: SeqId(9),
            lane: LaneId(0),
            channels: InputChannels::Mono(0),
            arm_midi: false,
        }),
    ];
    for c in &bad {
        assert!(h.execute(c, &f).is_err());
        assert_eq!(h.doc(), &doc);
        assert_eq!(h.undo_depth(), 0);
    }
}

#[test]
fn track_synth_reuses_the_return_lane_and_can_arm_the_midi_lane() {
    let d = doc();
    let mut doc = d.doc.clone();
    run(&mut doc, track(LaneId(0), InputChannels::Mono(2), false));
    let lanes = doc.sequence(SeqId(0)).unwrap().lanes.len();
    let out = doc.sequence(SeqId(0)).unwrap().lanes[1].audio_out;
    // Again, stereo, with the MIDI lane armed: the same lane is set up.
    run(&mut doc, track(LaneId(0), InputChannels::Stereo(4), true));
    let s = doc.sequence(SeqId(0)).unwrap();
    assert_eq!(s.lanes.len(), lanes);
    assert_eq!(
        s.lanes[1].input,
        Some(returning(InputChannels::Stereo(4), d.juno))
    );
    assert_eq!(s.lanes[1].audio_out, out, "its output is kept");
    assert_eq!(doc.outputs().len(), 1);
    assert!(s.lanes[0].arm && s.lanes[1].arm);
    // A lane that records the synth's return elsewhere in the sequence is
    // the one reused, wherever it is.
    let mut doc = d.doc.clone();
    let rec = with_return_lane(&mut doc, d.juno);
    run(
        &mut doc,
        Command::Lane(LaneCmd::Reorder {
            seq: SeqId(0),
            lane: rec,
            to: 0,
        }),
    );
    run(&mut doc, track(LaneId(0), InputChannels::Mono(5), false));
    let s = doc.sequence(SeqId(0)).unwrap();
    assert_eq!(s.lanes.len(), 2);
    assert_eq!(
        doc.lane(SeqId(0), rec).unwrap().input,
        Some(returning(InputChannels::Mono(5), d.juno))
    );
}

/// `Sequence::return_lane` is Track Synth's reuse rule: the lane recording
/// the destination's return, other than `except`.
#[test]
fn return_lane_finds_the_lane_recording_a_destinations_return() {
    let d = doc();
    let mut doc = d.doc.clone();
    let rec = with_return_lane(&mut doc, d.juno);
    let s = doc.sequence(SeqId(0)).unwrap();
    assert_eq!(s.return_lane(d.juno, None), Some(rec));
    assert_eq!(s.return_lane(d.juno, Some(rec)), None);
    assert_eq!(s.return_lane(d.juno, Some(LaneId(0))), Some(rec));
    assert_eq!(s.return_lane(d.moog, None), None);
}

/// Plan 5c-2 Ruling 5: Track Synth refuses a lane that plays no external
/// synth (none, a plugin, the built-in synth), an unknown lane or
/// sequence, and channels past the last; nothing changes.
#[test]
fn track_synth_refuses_what_it_cannot_set_up() {
    let d = doc();
    let mut doc = d.doc.clone();
    for (name, dest) in [
        ("Pad lane", Some(d.pad)),
        ("Synth lane", Some(d.synth)),
        ("Bare", None),
    ] {
        let r = run(
            &mut doc,
            Command::Lane(LaneCmd::Add {
                seq: SeqId(0),
                name: name.into(),
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
            &mut doc,
            Command::Lane(LaneCmd::SetDest {
                seq: SeqId(0),
                lane,
                dest,
            }),
        );
        assert!(
            apply(&doc, &track(lane, InputChannels::Mono(0), false)).is_err(),
            "{name}"
        );
    }
    assert!(apply(&doc, &track(LaneId(99), InputChannels::Mono(0), false)).is_err());
    assert!(
        apply(
            &doc,
            &Command::Lane(LaneCmd::TrackSynth {
                seq: SeqId(9),
                lane: LaneId(0),
                channels: InputChannels::Mono(0),
                arm_midi: false,
            })
        )
        .is_err()
    );
    assert!(apply(&doc, &track(LaneId(0), InputChannels::Mono(256), false)).is_err());
    assert!(apply(&doc, &track(LaneId(0), InputChannels::Stereo(255), false)).is_err());
}
