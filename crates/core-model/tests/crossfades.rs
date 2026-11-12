//! Crossfades (plan 6c Task 1): the flag on the later region, partners by
//! the engine's overlap rule, `Crossfade` and `Remove Crossfade`, and the
//! validation of a damaged flag.

use core_model::ModelError;
use core_model::command::{
    AudioCmd, Command, CrossfadeLength, EventCmd, LaneCmd, MediaCmd, SequenceCmd, apply,
};
use core_model::crossfade::{self, DEFAULT_SHAPE, Overlap};
use core_model::document::Document;
use core_model::event::{EventBody, EventSpec, Fade, FadeShape};
use core_model::file::{from_json, to_json};
use core_model::history::History;
use core_model::ids::{EventId, LaneId, MediaId, SeqId};
use core_model::selection::ViewFilter;
use core_model::tempo::{Bpm, TempoMap};
use core_model::time::Tick;
use serde_json::Value;

const A: EventId = EventId(0);
const B: EventId = EventId(1);
const NOTE: EventId = EventId(2);

fn run(d: &Document, c: Command) -> core_model::Result<Document> {
    apply(d, &c).map(|a| a.doc)
}

fn region_at(tick: i64, offset: u64, length: u64) -> Command {
    Command::Audio(AudioCmd::Insert {
        seq: SeqId(0),
        lane: LaneId(0),
        tick: Tick(tick),
        media: MediaId(0),
        source_offset: offset,
        length,
        gain_db: 0.0,
        fade_in: Fade::default(),
        fade_out: Fade::default(),
    })
}

/// Sequence 0 (120 BPM) with lane 0; media 0 at 48 kHz; region A (event 0)
/// at tick 0 for 48 000 frames; region B (event 1) at tick 15 360 (frame
/// 24 000), so the overlap is 24 000 frames; a note (event 2) at tick 0.
fn doc() -> Document {
    let mut d = run(
        &Document::new(),
        Command::Sequence(SequenceCmd::New { name: "S".into() }),
    )
    .unwrap();
    d = run(
        &d,
        Command::Media(MediaCmd::Add {
            path: "Media/a.caf".into(),
            sample_rate: 48_000,
            channels: 2,
            length_frames: 192_000,
            content_hash: None,
            name: None,
            origin: None,
        }),
    )
    .unwrap();
    d = run(&d, region_at(0, 0, 48_000)).unwrap();
    d = run(&d, region_at(15_360, 48_000, 48_000)).unwrap();
    run(
        &d,
        Command::Event(EventCmd::Insert {
            seq: SeqId(0),
            lane: LaneId(0),
            events: vec![EventSpec::new(Tick(0), EventBody::note(60, 100, Tick(480)))],
        }),
    )
    .unwrap()
}

fn cross(event: EventId, length: CrossfadeLength, shape: FadeShape) -> Command {
    Command::Audio(AudioCmd::Crossfade {
        event,
        length,
        shape,
    })
}

fn remove(event: EventId) -> Command {
    Command::Audio(AudioCmd::RemoveCrossfade { event })
}

fn fades(d: &Document, id: EventId) -> (Fade, Fade, bool) {
    match &d.event(id).unwrap().0.body {
        EventBody::AudioRegion {
            fade_in,
            fade_out,
            crossfade,
            ..
        } => (*fade_in, *fade_out, *crossfade),
        other => panic!("not a region: {other:?}"),
    }
}

fn set_fade_out(d: &Document, id: EventId, frames: u64) -> Document {
    let (fade_in, _, _) = fades(d, id);
    run(
        d,
        Command::Audio(AudioCmd::SetFades {
            event: id,
            fade_in,
            fade_out: Fade {
                length_frames: frames,
                shape: FadeShape::Linear,
            },
        }),
    )
    .unwrap()
}

fn invalid(r: core_model::Result<Document>, text: &str) {
    match r {
        Err(ModelError::Invalid { reason }) => assert_eq!(reason, text),
        other => panic!("expected {text:?}, got {:?}", other.map(|_| ())),
    }
}

#[test]
fn crossfade_sets_the_fade_in_and_the_flag() {
    assert_eq!(DEFAULT_SHAPE, FadeShape::EqualPower);
    let d = doc();
    let mut h = History::new(d.clone());
    h.execute(
        &cross(B, CrossfadeLength::Frames(4800), FadeShape::EqualPower),
        &ViewFilter::default(),
    )
    .unwrap();
    let (fade_in, _, flag) = fades(h.doc(), B);
    assert_eq!(
        fade_in,
        Fade {
            length_frames: 4800,
            shape: FadeShape::EqualPower
        }
    );
    assert!(flag);
    assert_eq!(h.undo_label(), Some("Crossfade"));
    h.undo().unwrap();
    assert_eq!(h.doc(), &d);
}

#[test]
fn up_to_takes_the_overlap_or_less() {
    let d = doc();
    let len = |d: &Document| fades(d, B).0.length_frames;
    let at = |d: &Document, n: u64| {
        run(d, cross(B, CrossfadeLength::UpTo(n), FadeShape::Linear)).unwrap()
    };
    assert_eq!(len(&at(&d, 480)), 480);
    assert_eq!(len(&at(&d, 100_000)), 24_000);
    let tight = set_fade_out(&d, B, 30_000);
    assert_eq!(len(&at(&tight, 100_000)), 18_000);
    assert_eq!(crossfade::room(&tight, B).unwrap(), 18_000);
    assert_eq!(crossfade::room(&d, B).unwrap(), 24_000);
    assert_eq!(crossfade::room(&d, A).unwrap(), 0);
}

#[test]
fn crossfade_refusals() {
    let d = doc();
    let before = d.clone();
    let none = "This region does not start inside another region on its track.";
    let short = "A crossfade is at least 1 frame long.";
    for len in [CrossfadeLength::Frames(0), CrossfadeLength::UpTo(0)] {
        invalid(run(&d, cross(B, len, FadeShape::Linear)), short);
    }
    invalid(
        run(
            &d,
            cross(B, CrossfadeLength::Frames(24_001), FadeShape::Linear),
        ),
        "A crossfade of 24001 frames runs past the end of the region it crosses (24000 frames).",
    );
    let tight = set_fade_out(&d, B, 30_000);
    invalid(
        run(
            &tight,
            cross(B, CrossfadeLength::Frames(20_000), FadeShape::Linear),
        ),
        "A crossfade of 20000 frames does not fit the region with its fade out (18000 frames).",
    );
    invalid(
        run(&d, cross(A, CrossfadeLength::Frames(10), FadeShape::Linear)),
        none,
    );
    assert!(
        run(
            &d,
            cross(NOTE, CrossfadeLength::Frames(10), FadeShape::Linear)
        )
        .is_err()
    );
    assert!(
        run(
            &d,
            cross(EventId(999), CrossfadeLength::Frames(10), FadeShape::Linear)
        )
        .is_err()
    );
    // Equal ticks are not "inside".
    let with_c = run(&d, region_at(0, 0, 48_000)).unwrap();
    let c = EventId(3);
    invalid(
        run(
            &with_c,
            cross(c, CrossfadeLength::Frames(10), FadeShape::Linear),
        ),
        none,
    );
    // Another take's regions are not partners.
    let t2 = run(
        &d,
        Command::Lane(LaneCmd::AddTake {
            seq: SeqId(0),
            lane: LaneId(0),
            name: "Take 2".into(),
        }),
    )
    .unwrap();
    let t2 = run(&t2, region_at(15_360, 0, 48_000)).unwrap();
    invalid(
        run(
            &t2,
            cross(EventId(3), CrossfadeLength::Frames(10), FadeShape::Linear),
        ),
        none,
    );
    // Refusals leave the document unchanged (apply is pure).
    assert_eq!(d, before);
}

#[test]
fn remove_crossfade_leaves_a_plain_fade_in() {
    let d = doc();
    let mut h = History::new(d);
    h.execute(
        &cross(B, CrossfadeLength::Frames(4800), FadeShape::SCurve),
        &ViewFilter::default(),
    )
    .unwrap();
    let with = fades(h.doc(), B);
    h.execute(&remove(B), &ViewFilter::default()).unwrap();
    assert_eq!(h.undo_label(), Some("Remove Crossfade"));
    let (fade_in, _, flag) = fades(h.doc(), B);
    assert!(!flag);
    assert_eq!(fade_in, with.0);
    invalid(run(h.doc(), remove(B)), "This region has no crossfade.");
    assert!(run(h.doc(), remove(NOTE)).is_err());
    assert!(run(h.doc(), remove(EventId(999))).is_err());
}

#[test]
fn overlaps_follow_the_engine_rule() {
    let d = doc();
    let take = |d: &Document| d.lane(SeqId(0), LaneId(0)).unwrap().takes[0].clone();
    let b_on_a = Overlap {
        with: A,
        frames: 24_000,
    };
    let m = crossfade::overlaps(&d, SeqId(0), &take(&d));
    assert_eq!(m.len(), 1);
    assert_eq!(m.get(&B), Some(&b_on_a));
    assert_eq!(crossfade::overlap(&d, B).unwrap(), Some(b_on_a));
    assert_eq!(crossfade::overlap(&d, A).unwrap(), None);
    assert!(crossfade::overlap(&d, NOTE).is_err());
    assert!(crossfade::overlap(&d, EventId(999)).is_err());

    // C (event 3) starts inside A and B and ends at frame 41 250 (F6); E
    // (event 4) starts exactly where A ends.
    let d2 = run(&d, region_at(20_000, 0, 10_000)).unwrap();
    let d2 = run(&d2, region_at(30_720, 0, 10_000)).unwrap();
    let (c, e) = (EventId(3), EventId(4));
    let m = crossfade::overlaps(&d2, SeqId(0), &take(&d2));
    assert_eq!(m.get(&A), None);
    assert_eq!(m.get(&B), Some(&b_on_a));
    // C is at frame 31 250; B ends at 72 000.
    assert_eq!(
        m.get(&c),
        Some(&Overlap {
            with: B,
            frames: 40_750
        })
    );
    // E is at frame 48 000: A has ended, so B is its partner.
    assert_eq!(
        m.get(&e),
        Some(&Overlap {
            with: B,
            frames: 24_000
        })
    );
    assert_eq!(m.len(), 3);

    // At 60 BPM, B starts at frame 48 000, A's end: not an overlap.
    let slow = run(
        &d,
        Command::Sequence(SequenceCmd::SetTempo {
            seq: SeqId(0),
            tempo: TempoMap::constant(Bpm(60_000)).unwrap(),
        }),
    )
    .unwrap();
    assert!(crossfade::overlaps(&slow, SeqId(0), &take(&slow)).is_empty());
    assert_eq!(crossfade::overlap(&slow, B).unwrap(), None);
}

/// Calls `f` on every `audio_region` body in the JSON, in file order.
fn each_region(v: &mut Value, f: &mut dyn FnMut(usize, &mut Value)) {
    fn walk(v: &mut Value, n: &mut usize, f: &mut dyn FnMut(usize, &mut Value)) {
        if v.get("type").and_then(Value::as_str) == Some("audio_region") {
            f(*n, v);
            *n += 1;
            return;
        }
        match v {
            Value::Object(m) => m.values_mut().for_each(|c| walk(c, n, f)),
            Value::Array(a) => a.iter_mut().for_each(|c| walk(c, n, f)),
            _ => {}
        }
    }
    walk(v, &mut 0, f);
}

#[test]
fn validate_names_a_damaged_crossfade() {
    let text = to_json(&doc()).unwrap();
    let problems = |v: &Value| match from_json(&v.to_string()) {
        Err(ModelError::Corrupt { problems }) => problems,
        other => panic!("expected Corrupt, got {other:?}"),
    };
    // A flagged region that starts inside nothing.
    let mut v: Value = serde_json::from_str(&text).unwrap();
    each_region(&mut v, &mut |i, r| {
        if i == 0 {
            r["crossfade"] = true.into();
            r["fade_in"]["length_frames"] = 100.into();
        }
    });
    let p = problems(&v);
    assert!(p.iter().any(|p| p.contains("crossfade")), "{p:?}");
    // A flagged region with a fade in of 0.
    let mut v: Value = serde_json::from_str(&text).unwrap();
    each_region(&mut v, &mut |i, r| {
        if i == 1 {
            r["crossfade"] = true.into();
        }
    });
    let p = problems(&v);
    assert!(p.iter().any(|p| p.contains("crossfade")), "{p:?}");
    // Longer than the room.
    let mut v: Value = serde_json::from_str(&text).unwrap();
    each_region(&mut v, &mut |i, r| {
        if i == 1 {
            r["crossfade"] = true.into();
            r["fade_in"]["length_frames"] = 24_001.into();
        }
    });
    let p = problems(&v);
    assert!(p.iter().any(|p| p.contains("crossfade")), "{p:?}");
    // The same flag within the room loads.
    let mut v: Value = serde_json::from_str(&text).unwrap();
    each_region(&mut v, &mut |i, r| {
        if i == 1 {
            r["crossfade"] = true.into();
            r["fade_in"]["length_frames"] = 24_000.into();
        }
    });
    assert!(from_json(&v.to_string()).is_ok());
}

#[test]
fn split_keeps_the_flag_on_the_first_part_only() {
    let d = run(
        &doc(),
        cross(B, CrossfadeLength::Frames(4800), FadeShape::Linear),
    )
    .unwrap();
    // B starts at tick 15 360 for 48 000 frames; split 0.5 s (tick 7 680) in.
    let a = apply(
        &d,
        &Command::Audio(AudioCmd::Split {
            event: B,
            at: Tick(15_360 + 7_680),
        }),
    )
    .unwrap();
    assert!(fades(&a.doc, B).2);
    let second = a
        .report
        .created
        .iter()
        .find_map(|c| match c {
            core_model::ids::AnyId::Event(e) => Some(*e),
            _ => None,
        })
        .unwrap();
    assert!(!fades(&a.doc, second).2);
}

#[test]
fn a_muted_partner_is_still_the_partner() {
    // F9: the partner is found by rule, not by mute.
    let d = run(
        &doc(),
        Command::Event(EventCmd::SetMute {
            events: vec![A],
            mute: true,
        }),
    )
    .unwrap();
    assert_eq!(
        crossfade::overlap(&d, B).unwrap(),
        Some(Overlap {
            with: A,
            frames: 24_000
        })
    );
    let d = run(
        &d,
        cross(B, CrossfadeLength::Frames(100), FadeShape::Linear),
    )
    .unwrap();
    assert!(fades(&d, B).2);
}

#[test]
fn a_region_on_another_track_is_not_a_partner() {
    let d = run(
        &doc(),
        Command::Lane(LaneCmd::Add {
            seq: SeqId(0),
            name: "Two".into(),
            index: None,
        }),
    )
    .unwrap();
    let d = run(
        &d,
        Command::Audio(AudioCmd::Insert {
            seq: SeqId(0),
            lane: LaneId(1),
            tick: Tick(15_360),
            media: MediaId(0),
            source_offset: 0,
            length: 48_000,
            gain_db: 0.0,
            fade_in: Fade::default(),
            fade_out: Fade::default(),
        }),
    )
    .unwrap();
    // Event 3 starts inside A's time span, but A is on track 0.
    assert_eq!(crossfade::overlap(&d, EventId(3)).unwrap(), None);
    assert!(
        run(
            &d,
            cross(EventId(3), CrossfadeLength::Frames(10), FadeShape::Linear)
        )
        .is_err()
    );
}

#[test]
fn huge_lengths_are_refused_without_wrapping() {
    let d = doc();
    let before = d.clone();
    for len in [
        CrossfadeLength::Frames(u64::MAX),
        CrossfadeLength::UpTo(u64::MAX),
    ] {
        match len {
            CrossfadeLength::Frames(_) => {
                assert!(run(&d, cross(B, len, FadeShape::Linear)).is_err())
            }
            // UpTo clamps to the room.
            CrossfadeLength::UpTo(_) => {
                let r = run(&d, cross(B, len, FadeShape::Linear)).unwrap();
                assert_eq!(fades(&r, B).0.length_frames, 24_000);
            }
        }
    }
    assert_eq!(d, before);
}

#[test]
fn a_huge_fade_in_in_the_file_is_corrupt() {
    let mut v: Value = serde_json::from_str(&to_json(&doc()).unwrap()).unwrap();
    each_region(&mut v, &mut |i, r| {
        if i == 1 {
            r["crossfade"] = true.into();
            r["fade_in"]["length_frames"] = u64::MAX.into();
        }
    });
    assert!(matches!(
        from_json(&v.to_string()),
        Err(ModelError::Corrupt { .. })
    ));
}

#[test]
fn a_flagged_region_with_missing_media_gets_only_the_media_problem() {
    let mut v: Value = serde_json::from_str(&to_json(&doc()).unwrap()).unwrap();
    each_region(&mut v, &mut |i, r| {
        if i == 1 {
            r["crossfade"] = true.into();
            r["fade_in"]["length_frames"] = 100.into();
            r["media"] = 77.into();
        }
    });
    match from_json(&v.to_string()) {
        Err(ModelError::Corrupt { problems }) => {
            assert!(!problems.is_empty());
            assert!(
                !problems.iter().any(|p| p.contains("crossfade")),
                "{problems:?}"
            );
        }
        other => panic!("expected Corrupt, got {other:?}"),
    }
}

#[test]
fn a_flagged_region_with_no_room_says_so() {
    let mut v: Value = serde_json::from_str(&to_json(&doc()).unwrap()).unwrap();
    each_region(&mut v, &mut |i, r| {
        if i == 0 {
            r["crossfade"] = true.into();
        }
    });
    match from_json(&v.to_string()) {
        Err(ModelError::Corrupt { problems }) => {
            assert!(
                problems.iter().any(|p| p.contains("crossfade")),
                "{problems:?}"
            );
        }
        other => panic!("expected Corrupt, got {other:?}"),
    }
}

// ---- Task 2: settle, Split's refusal ----

const BAR: i64 = 4 * 15_360;
const SPLIT_REFUSAL: &str = "Split outside the crossfade, or remove the crossfade first.";

/// The fixture with `Crossfade(B, Frames(4800), Linear)` applied.
fn flagged() -> Document {
    run(
        &doc(),
        cross(B, CrossfadeLength::Frames(4800), FadeShape::Linear),
    )
    .unwrap()
}

/// Runs `c` through a History on `d`: the report's warnings, the document,
/// and a check that one undo returns to `d`.
fn step(d: &Document, c: Command) -> (Vec<String>, Document) {
    let mut h = History::new(d.clone());
    let report = h.execute(&c, &ViewFilter::default()).unwrap();
    let after = h.doc().clone();
    assert_eq!(h.undo_depth(), 1);
    h.undo().unwrap();
    assert_eq!(h.doc(), d, "one undo restores the start");
    (report.warnings, after)
}

fn mv(events: Vec<EventId>, delta: i64, to_lane: Option<(SeqId, LaneId)>) -> Command {
    Command::Event(EventCmd::Move {
        events,
        delta: Tick(delta),
        to_lane,
    })
}

fn trim(event: EventId, front_frames: i64, back_frames: i64) -> Command {
    Command::Audio(AudioCmd::Trim {
        event,
        front_frames,
        back_frames,
    })
}

fn split(event: EventId, at: i64) -> Command {
    Command::Audio(AudioCmd::Split {
        event,
        at: Tick(at),
    })
}

fn add_lane(d: &Document) -> Document {
    run(
        d,
        Command::Lane(LaneCmd::Add {
            seq: SeqId(0),
            name: "Two".into(),
            index: None,
        }),
    )
    .unwrap()
}

fn one_warning(w: &[String], text: &str) {
    assert_eq!(w.len(), 1, "{w:?}");
    assert!(w[0].contains(text), "{w:?}");
    assert!(!w[0].contains("lane"), "{w:?}");
}

#[test]
fn moving_keeps_shrinks_or_drops() {
    let d = flagged();
    let (w, a) = step(&d, mv(vec![B], -1_920, None));
    assert!(w.is_empty(), "{w:?}");
    assert_eq!(fades(&a, B).0.length_frames, 4800);
    assert!(fades(&a, B).2);

    let (w, a) = step(&d, mv(vec![B], 12_288, None));
    assert!(w.is_empty(), "{w:?}");
    assert_eq!(fades(&a, B).0.length_frames, 4800);
    assert!(fades(&a, B).2);

    let (w, a) = step(&d, mv(vec![B], 13_056, None));
    one_warning(&w, "shortened");
    assert_eq!(fades(&a, B).0.length_frames, 3600);
    assert!(fades(&a, B).2);

    let (w, a) = step(&d, mv(vec![B], 16_000, None));
    one_warning(&w, "removed");
    assert!(w[0].contains("track"), "{w:?}");
    assert!(!fades(&a, B).2);
    assert_eq!(fades(&a, B).0.length_frames, 4800);
    assert!(core_model::rules::validate(&a).is_empty());
}

#[test]
fn moving_both_together_keeps_it() {
    let d = flagged();
    let (w, a) = step(&d, mv(vec![A, B], 2 * BAR, None));
    assert!(w.is_empty(), "{w:?}");
    assert_eq!(fades(&a, B), fades(&d, B));
    let d1 = add_lane(&d);
    let (w, a) = step(&d1, mv(vec![A, B], 0, Some((SeqId(0), LaneId(1)))));
    assert!(w.is_empty(), "{w:?}");
    assert_eq!(fades(&a, B), fades(&d, B));
    assert_eq!(a.locate(B).unwrap().lane, LaneId(1));
}

#[test]
fn trims_shrink_it() {
    let d = flagged();
    let (w, a) = step(&d, trim(A, 0, 22_000));
    one_warning(&w, "shortened");
    assert_eq!(fades(&a, B).0.length_frames, 2000);
    assert!(fades(&a, B).2);

    let (w, a) = step(&d, trim(B, 23_000, 0));
    one_warning(&w, "shortened");
    assert_eq!(fades(&a, B).0.length_frames, 1000);
    assert!(fades(&a, B).2);

    let (w, a) = step(&d, trim(A, 0, 24_000));
    one_warning(&w, "removed");
    assert!(!fades(&a, B).2);
}

#[test]
fn a_different_partner_drops_it() {
    let d = flagged();
    let (w, a) = step(&d, Command::Event(EventCmd::Delete { events: vec![A] }));
    one_warning(&w, "removed");
    assert!(!fades(&a, B).2);

    // C (event 3): inside A, before B, covering B's start.
    let (w, a) = step(&d, region_at(14_000, 0, 10_000));
    one_warning(&w, "different region");
    assert!(w[0].starts_with("Crossfade removed"), "{w:?}");
    assert!(!fades(&a, B).2);
    assert_eq!(crossfade::overlap(&a, B).unwrap().unwrap().with, EventId(3));

    // B alone to track 1, onto a long region there.
    let d1 = add_lane(&d);
    let d1 = run(
        &d1,
        Command::Audio(AudioCmd::Insert {
            seq: SeqId(0),
            lane: LaneId(1),
            tick: Tick(0),
            media: MediaId(0),
            source_offset: 0,
            length: 96_000,
            gain_db: 0.0,
            fade_in: Fade::default(),
            fade_out: Fade::default(),
        }),
    )
    .unwrap();
    let (w, a) = step(&d1, mv(vec![B], 0, Some((SeqId(0), LaneId(1)))));
    one_warning(&w, "different region");
    assert!(!fades(&a, B).2);
    assert!(core_model::rules::validate(&a).is_empty());
}

fn created_events(r: &core_model::command::Report) -> Vec<EventId> {
    r.created
        .iter()
        .filter_map(|c| match c {
            core_model::ids::AnyId::Event(e) => Some(*e),
            _ => None,
        })
        .collect()
}

#[test]
fn copies_keep_it_only_with_their_partner() {
    let d = flagged();
    let dup = |events: Vec<EventId>| {
        Command::Event(EventCmd::Duplicate {
            events,
            ticks: Tick(4 * BAR),
            semitones: 0,
        })
    };
    // Both together: the copied B keeps it.
    let a = apply(&d, &dup(vec![A, B])).unwrap();
    assert!(a.report.warnings.is_empty(), "{:?}", a.report.warnings);
    let copies = created_events(&a.report);
    assert_eq!(copies.len(), 2);
    let flags: Vec<bool> = copies.iter().map(|c| fades(&a.doc, *c).2).collect();
    assert_eq!(flags.iter().filter(|f| **f).count(), 1, "{flags:?}");

    // A alone, then B alone onto A's copy: the lone copy has no flag.
    let d1 = run(&d, dup(vec![A])).unwrap();
    let a = apply(&d1, &dup(vec![B])).unwrap();
    assert!(a.report.warnings.is_empty(), "{:?}", a.report.warnings);
    let copy = created_events(&a.report)[0];
    assert!(crossfade::overlap(&a.doc, copy).unwrap().is_some());
    assert!(!fades(&a.doc, copy).2);
    assert!(fades(&a.doc, B).2);

    // The two specs inserted together into an empty track keep it.
    let d2 = add_lane(&d);
    let specs = vec![d2.event(A).unwrap().0.spec(), d2.event(B).unwrap().0.spec()];
    let a = apply(
        &d2,
        &Command::Event(EventCmd::Insert {
            seq: SeqId(0),
            lane: LaneId(1),
            events: specs,
        }),
    )
    .unwrap();
    assert!(a.report.warnings.is_empty(), "{:?}", a.report.warnings);
    let copies = created_events(&a.report);
    assert_eq!(copies.len(), 2);
    assert!(copies.iter().any(|c| fades(&a.doc, *c).2));
    assert!(core_model::rules::validate(&a.doc).is_empty());
}

#[test]
fn fade_edits_settle_it() {
    let d = flagged();
    let set_in = |frames: u64| {
        Command::Audio(AudioCmd::SetFades {
            event: B,
            fade_in: Fade {
                length_frames: frames,
                shape: FadeShape::Linear,
            },
            fade_out: Fade::default(),
        })
    };
    let (w, a) = step(&d, set_in(0));
    one_warning(&w, "removed");
    assert!(!fades(&a, B).2);

    let (w, a) = step(
        &d,
        Command::Audio(AudioCmd::RemoveFades { events: vec![B] }),
    );
    one_warning(&w, "removed");
    assert!(!fades(&a, B).2);

    let (w, a) = step(&d, set_in(30_000));
    one_warning(&w, "shortened");
    assert_eq!(fades(&a, B).0.length_frames, 24_000);
    assert!(fades(&a, B).2);
}

fn tempo(bpm: u32) -> Command {
    Command::Sequence(SequenceCmd::SetTempo {
        seq: SeqId(0),
        tempo: TempoMap::constant(Bpm(bpm)).unwrap(),
    })
}

#[test]
fn tempo_changes_shrink_or_drop() {
    let d = flagged();
    let (w, a) = step(&d, tempo(240_000));
    assert!(w.is_empty(), "{w:?}");
    assert_eq!(fades(&a, B).0.length_frames, 4800);
    assert!(fades(&a, B).2);
    assert_eq!(crossfade::overlap(&a, B).unwrap().unwrap().frames, 36_000);

    // At 50 BPM B starts at frame 57 600, after A's end.
    let (w, a) = step(&d, tempo(50_000));
    one_warning(&w, "removed");
    assert!(!fades(&a, B).2);
}

#[test]
fn split_inside_a_crossfade_is_refused() {
    let d = flagged();
    invalid(run(&d, split(B, 16_000)), SPLIT_REFUSAL);
    invalid(run(&d, split(A, 16_640)), SPLIT_REFUSAL);
    // F10: the first tick inside the span (frame 24 001 rounds to it).
    invalid(run(&d, split(B, 15_361)), SPLIT_REFUSAL);
    invalid(run(&d, split(A, 15_361)), SPLIT_REFUSAL);

    let a = apply(&d, &split(B, 25_600)).unwrap();
    let second = created_events(&a.report)[0];
    assert!(!fades(&a.doc, second).2);
    assert!(fades(&a.doc, B).2);
    assert!(a.report.warnings.is_empty(), "{:?}", a.report.warnings);

    // F10: exactly at the span's end (frame 28 800) works, on either.
    for ev in [A, B] {
        let (w, a) = step(&d, split(ev, 18_432));
        assert!(w.is_empty(), "{w:?}");
        assert!(fades(&a, B).2);
        assert_eq!(fades(&a, B).0.length_frames, 4800);
        assert!(core_model::rules::validate(&a).is_empty());
    }
}

#[test]
fn splitting_the_partner_before_the_span_keeps_it() {
    let d = flagged();
    let mut h = History::new(d.clone());
    let report = h.execute(&split(A, 7_680), &ViewFilter::default()).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    let second = created_events(&report)[0];
    assert_eq!(fades(h.doc(), B), fades(&d, B));
    assert_eq!(
        crossfade::overlap(h.doc(), B).unwrap(),
        Some(Overlap {
            with: second,
            frames: 24_000
        })
    );
    assert!(core_model::rules::validate(h.doc()).is_empty());
    h.undo().unwrap();
    assert_eq!(h.doc(), &d);
}

#[test]
fn extreme_edits_beside_a_crossfade_never_panic() {
    let d = flagged();
    let big = 1i64 << 40;
    let half = i64::MAX / 2;
    let huge = Command::Event(EventCmd::Insert {
        seq: SeqId(0),
        lane: LaneId(0),
        events: vec![EventSpec::new(
            Tick(14_000),
            EventBody::AudioRegion {
                media: MediaId(0),
                source_offset: 0,
                length: u64::MAX / 2,
                gain_db: 0.0,
                fade_in: Fade::default(),
                fade_out: Fade::default(),
                name: None,
                crossfade: false,
            },
        )],
    });
    // Refused by its media, not by a panic.
    assert!(apply(&d, &huge).is_err());
    let mut cmds = vec![
        mv(vec![B], big, None),
        mv(vec![B], -big, None),
        mv(vec![A], big, None),
        mv(vec![A], -big, None),
        tempo(Bpm::MIN.0),
        tempo(Bpm::MAX.0),
        huge,
    ];
    for ev in [A, B] {
        for (f, b) in [(half, 0), (-half, 0), (0, half), (0, -half), (half, half)] {
            cmds.push(trim(ev, f, b));
        }
    }
    for c in cmds {
        if let Ok(a) = apply(&d, &c) {
            let v = core_model::rules::validate(&a.doc);
            assert!(v.is_empty(), "{c:?}: {v:?}");
            let kept = fades(&a.doc, B).2;
            assert!(
                kept || a.report.warnings.iter().any(|w| w.contains("Crossfade")),
                "{c:?}: {:?}",
                a.report
            );
        }
    }
}

#[test]
fn settle_leaves_a_valid_document_untouched() {
    let d = flagged();
    let a = apply(
        &d,
        &Command::Event(EventCmd::SetMute {
            events: vec![NOTE],
            mute: true,
        }),
    )
    .unwrap();
    assert!(a.report.warnings.is_empty(), "{:?}", a.report.warnings);
    assert_eq!(a.doc.event(B).unwrap().0, d.event(B).unwrap().0);
    assert_eq!(a.doc.event(A).unwrap().0, d.event(A).unwrap().0);
}
