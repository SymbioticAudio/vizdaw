//! Placing a recording (plan 5c Task 7; sub-project 5 spec §6.3–§6.5):
//! input files become regions at the timeline frame that was heard when
//! they were captured (device delay and graph latency taken off), pre-roll
//! stays as region handles, punch and loop ranges cut what is kept, loop
//! passes become takes (discarded ones dropped), and MIDI lands where it
//! was played with replace clearing only the recorded range.

use core_engine::fixtures::{Builder, Q};
use core_engine::midi1::Midi1Msg;
use core_engine::record::{Mark, RecordedMidi};
use core_engine::schedule::TempoTable;
use core_model::command::{RecordPass, RecordedMedia};
use core_model::document::Document;
use core_model::event::EventBody;
use core_model::ids::{LaneId, SeqId};
use core_model::time::Tick;
use vizdaw_ffi::passes::{AudioTake, Captured, PassFate, RecordRange, decide, plan};

/// 120 BPM at 48 kHz: one beat (Q ticks) is 24 000 frames.
const BEAT: u64 = 24_000;

struct Doc {
    doc: Document,
    seq: SeqId,
    keys: LaneId,
    mic: LaneId,
}

fn doc(bpm: Option<u32>) -> Doc {
    let mut b = Builder::new();
    let synth = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, keys) = b.sequence("Song", "Keys", Some(synth)).unwrap();
    // An old note at beat 2 and a CC at beat 6.
    b.notes(seq, keys, &[(2 * Q, 40, 0x8000, Q)]).unwrap();
    let mic = b.lane(seq, "Mic", None).unwrap();
    if let Some(bpm) = bpm {
        b.tempo(seq, &[(0, bpm, false)]).unwrap();
    }
    Doc {
        doc: b.finish(),
        seq,
        keys,
        mic,
    }
}

fn file(frames: u64) -> RecordedMedia {
    RecordedMedia {
        path: "Media/Recorded/Mic 2026-10-20 120000.caf".into(),
        sample_rate: 48_000,
        channels: 1,
        length_frames: frames,
        content_hash: "cd".repeat(32),
        name: "Mic 2026-10-20 120000".into(),
        pass: 1,
    }
}

fn mark(run: u64, frame: u64, wrap: bool) -> Mark {
    Mark { run, frame, wrap }
}

struct Case<'a> {
    marks: Vec<Mark>,
    end_run: u64,
    latency: u64,
    input_delay: i64,
    range: RecordRange,
    looped: bool,
    replace: bool,
    takes: bool,
    fates: Vec<PassFate>,
    audio: Vec<AudioTake>,
    midi: Vec<RecordedMidi>,
    midi_lanes: Vec<LaneId>,
    d: &'a Doc,
}

impl<'a> Case<'a> {
    fn new(d: &'a Doc, start: u64) -> Case<'a> {
        Case {
            marks: vec![mark(0, start, false)],
            end_run: 4 * BEAT,
            latency: 0,
            input_delay: 0,
            range: RecordRange { start, end: None },
            looped: false,
            replace: true,
            takes: true,
            fates: Vec::new(),
            audio: Vec::new(),
            midi: Vec::new(),
            midi_lanes: Vec::new(),
            d,
        }
    }

    fn run(&self) -> Option<RecordPass> {
        plan(&Captured {
            doc: &self.d.doc,
            seq: self.d.seq,
            marks: &self.marks,
            end_run: self.end_run,
            latency: self.latency,
            input_delay: self.input_delay,
            range: self.range,
            looped: self.looped,
            replace: self.replace,
            passes_as_takes: self.takes,
            fates: &self.fates,
            audio: &self.audio,
            midi_lanes: &self.midi_lanes,
            midi: &self.midi,
            with_gaps: false,
        })
        .unwrap()
    }
}

/// `(tick, source_offset, length)` of every region in the pass's active
/// content of lane `lane`.
fn active_regions(p: &RecordPass, lane: LaneId) -> Vec<(i64, u64, u64)> {
    p.lanes
        .iter()
        .filter(|l| l.lane == lane)
        .flat_map(|l| l.active.regions.iter())
        .map(|r| (r.tick.0, r.source_offset, r.length))
        .collect()
}

fn audio_case(d: &Doc, start: u64, frames: u64) -> Case<'_> {
    let mut c = Case::new(d, start);
    c.end_run = frames;
    c.audio = vec![AudioTake {
        lane: d.mic,
        media: file(frames),
    }];
    c
}

#[test]
fn a_take_lands_where_it_was_heard_from_the_record_start() {
    let d = doc(None);
    let c = audio_case(&d, 2 * BEAT, 4 * BEAT);
    let p = c.run().unwrap();
    assert_eq!(p.media.len(), 1);
    assert_eq!(active_regions(&p, d.mic), vec![(2 * Q, 0, 4 * BEAT)]);
    assert!(
        p.lanes.iter().all(|l| l.takes.is_empty()),
        "no takes without a loop"
    );
}

#[test]
fn the_device_delay_and_graph_latency_come_off_the_file() {
    let d = doc(None);
    let mut c = audio_case(&d, 2 * BEAT, 4 * BEAT);
    c.latency = 512;
    c.input_delay = 300;
    let p = c.run().unwrap();
    // File frame 812 was captured while the record start was heard.
    assert_eq!(
        active_regions(&p, d.mic),
        vec![(2 * Q, 812, 4 * BEAT - 812)]
    );
}

#[test]
fn pre_roll_is_kept_as_a_handle_and_punch_cuts_the_rest() {
    let d = doc(None);
    // Playing from beat 1, recording from beat 2: the first beat of the
    // file is a handle before the region.
    let mut c = audio_case(&d, BEAT, 5 * BEAT);
    c.marks = vec![mark(0, 0, false)];
    let p = c.run().unwrap();
    assert_eq!(active_regions(&p, d.mic), vec![(Q, BEAT, 4 * BEAT)]);
    // Punch beats 3–4: only that much, the rest of the file as handles.
    c.range = RecordRange {
        start: 2 * BEAT,
        end: Some(4 * BEAT),
    };
    let p = c.run().unwrap();
    assert_eq!(active_regions(&p, d.mic), vec![(2 * Q, 2 * BEAT, 2 * BEAT)]);
    // A punch the playback never reached records nothing.
    c.range = RecordRange {
        start: 9 * BEAT,
        end: Some(10 * BEAT),
    };
    assert_eq!(c.run(), None);
}

#[test]
fn loop_passes_become_takes_and_discarded_ones_are_dropped() {
    let d = doc(None);
    let len = 2 * BEAT;
    let mut c = audio_case(&d, BEAT, 3 * len);
    c.looped = true;
    c.range = RecordRange {
        start: BEAT,
        end: Some(BEAT + len),
    };
    c.marks = vec![
        mark(0, BEAT, false),
        mark(len, BEAT, true),
        mark(2 * len, BEAT, true),
    ];
    let p = c.run().unwrap();
    let l = p.lanes.iter().find(|l| l.lane == d.mic).unwrap();
    let names: Vec<&str> = l.takes.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, vec!["Take 2", "Take 3", "Take 4"]);
    let offsets: Vec<u64> = l
        .takes
        .iter()
        .map(|(_, t)| t.regions[0].source_offset)
        .collect();
    assert_eq!(offsets, vec![0, len, 2 * len]);
    assert!(l.takes.iter().all(|(_, t)| t.regions[0].tick == Tick(Q)));
    // Keep after pass 1, discard after pass 2: passes 1 and 3 stay.
    let mut fates = Vec::new();
    decide(&mut fates, 1, true);
    decide(&mut fates, 2, false);
    assert_eq!(fates, vec![PassFate::Kept, PassFate::Discarded]);
    c.fates = fates;
    let p = c.run().unwrap();
    let l = p.lanes.iter().find(|l| l.lane == d.mic).unwrap();
    let offsets: Vec<u64> = l
        .takes
        .iter()
        .map(|(_, t)| t.regions[0].source_offset)
        .collect();
    assert_eq!(offsets, vec![0, 2 * len]);
    assert_eq!(l.takes[1].0, "Take 3");
    // Every pass discarded: nothing to apply.
    c.fates = vec![PassFate::Discarded; 3];
    assert_eq!(c.run(), None);
}

#[test]
fn keep_protects_and_discard_drops_since_the_last_keep() {
    let mut f = Vec::new();
    decide(&mut f, 2, false);
    assert_eq!(f, vec![PassFate::Discarded; 2]);
    decide(&mut f, 3, true);
    assert_eq!(f[2], PassFate::Kept);
    decide(&mut f, 5, false);
    assert_eq!(
        f,
        vec![
            PassFate::Discarded,
            PassFate::Discarded,
            PassFate::Kept,
            PassFate::Discarded,
            PassFate::Discarded
        ]
    );
    // A pass playing when nothing completed is untouched.
    let mut g = Vec::new();
    decide(&mut g, 0, false);
    assert!(g.is_empty());
}

fn on(pitch: u8, vel: u8) -> Midi1Msg {
    Midi1Msg::three(0x90, pitch, vel)
}

fn off(pitch: u8) -> Midi1Msg {
    Midi1Msg::three(0x80, pitch, 64)
}

fn midi(lane: LaneId, run: i64, msg: Midi1Msg) -> RecordedMidi {
    RecordedMidi { lane, run, msg }
}

#[test]
fn midi_lands_where_it_was_played_and_replace_clears_the_range() {
    let d = doc(None);
    let mut c = Case::new(&d, 2 * BEAT);
    // Four beats were heard: the output ran on by the latency.
    c.end_run = 4 * BEAT + 256;
    c.latency = 256;
    c.midi_lanes = vec![d.keys];
    let r = |beats: f64| (beats * BEAT as f64) as i64 + 256;
    c.midi = vec![
        // Before the record start (pre-roll): dropped.
        midi(d.keys, r(-0.5), on(50, 100)),
        midi(d.keys, r(0.0), on(60, 127)),
        midi(d.keys, r(0.5), off(60)),
        midi(d.keys, r(1.0), Midi1Msg::three(0xB0, 1, 64)),
        // Held at Stop: it ends at the end of what was recorded.
        midi(d.keys, r(3.0), on(62, 1)),
    ];
    let p = c.run().unwrap();
    let l = p.lanes.iter().find(|l| l.lane == d.keys).unwrap();
    // Replace: the recorded range, from the record start to Stop.
    assert_eq!(l.clear, vec![(Tick(2 * Q), Tick(6 * Q))]);
    let got: Vec<(i64, String)> = l
        .active
        .events
        .iter()
        .map(|e| {
            let what = match &e.body {
                EventBody::Note {
                    pitch,
                    velocity,
                    release_velocity,
                    duration,
                    ..
                } => format!("note {pitch} {velocity} {release_velocity} {}", duration.0),
                EventBody::Cc { index, value } => format!("cc {index} {value:#x}"),
                other => format!("{other:?}"),
            };
            (e.tick.0, what)
        })
        .collect();
    assert_eq!(
        got,
        vec![
            (2 * Q, format!("note 60 65535 32768 {}", Q / 2)),
            (3 * Q, "cc 1 0x80000000".to_string()),
            (5 * Q, format!("note 62 512 0 {Q}")),
        ]
    );
    // Overdub clears nothing.
    c.replace = false;
    let p = c.run().unwrap();
    assert!(p.lanes[0].clear.is_empty());
    // Replace with nothing played still clears the range.
    c.replace = true;
    c.midi.clear();
    let p = c.run().unwrap();
    assert_eq!(p.lanes[0].clear, vec![(Tick(2 * Q), Tick(6 * Q))]);
    assert!(p.lanes[0].active.events.is_empty());
}

#[test]
fn midi_loop_passes_make_takes_or_merge() {
    let d = doc(None);
    let len = 2 * BEAT;
    let mut c = Case::new(&d, 0);
    c.looped = true;
    c.end_run = 2 * len;
    c.range = RecordRange {
        start: 0,
        end: Some(len),
    };
    c.marks = vec![mark(0, 0, false), mark(len, 0, true)];
    c.midi_lanes = vec![d.keys];
    let half = (BEAT / 2) as i64;
    c.midi = vec![
        midi(d.keys, half, on(60, 100)),
        midi(d.keys, 2 * half, off(60)),
        // In pass 2, at the same place in the loop.
        midi(d.keys, len as i64 + half, on(64, 100)),
        midi(d.keys, len as i64 + 2 * half, off(64)),
    ];
    let p = c.run().unwrap();
    let l = &p.lanes[0];
    assert_eq!(l.takes.len(), 2);
    let pitch = |e: &core_model::event::EventSpec| match e.body {
        EventBody::Note { pitch, .. } => pitch,
        _ => 0,
    };
    assert_eq!(pitch(&l.takes[0].1.events[0]), 60);
    assert_eq!(pitch(&l.takes[1].1.events[0]), 64);
    assert_eq!(l.takes[1].1.events[0].tick, Tick(Q / 2));
    // Without passes as takes, MIDI passes merge into the active take.
    c.takes = false;
    let p = c.run().unwrap();
    let l = &p.lanes[0];
    assert!(l.takes.is_empty());
    let pitches: Vec<u8> = l.active.events.iter().map(pitch).collect();
    assert_eq!(pitches, vec![60, 64]);
}

#[test]
fn a_region_on_an_awkward_tempo_still_lands_on_its_frame() {
    // 97 BPM: frames per tick are not whole.
    let d = doc(Some(97));
    let start = 12_345;
    let mut c = audio_case(&d, start, 100_000);
    c.input_delay = 77;
    let p = c.run().unwrap();
    let (tick, offset, length) = active_regions(&p, d.mic)[0];
    let seq = d.doc.sequence(d.seq).unwrap();
    let t = TempoTable::build(&seq.tempo, Tick(0), 48_000.0);
    let at = t.tick_to_frame(Tick(tick));
    // File frame `offset` plays at frame `at`: the same pairing the
    // capture had (frame `start` ↔ file frame 77).
    assert_eq!(at as i64 - offset as i64, start as i64 - 77);
    assert!(at.abs_diff(start) <= 2, "{at} {start}");
    assert_eq!(offset + length, 100_000);
}

#[test]
fn nothing_armed_or_recorded_is_no_pass() {
    let d = doc(None);
    let c = Case::new(&d, 0);
    assert_eq!(c.run(), None);
    let mut c = Case::new(&d, 0);
    c.marks.clear();
    c.audio = vec![AudioTake {
        lane: d.mic,
        media: file(1000),
    }];
    assert_eq!(c.run(), None, "no marks: the transport never rolled");
}

/// The `(frame, source_offset, length)` of lane `lane`'s active regions,
/// in timeline frames at 120 BPM.
fn active_frames(p: &RecordPass, lane: LaneId, d: &Doc) -> Vec<(u64, u64, u64)> {
    let seq = d.doc.sequence(d.seq).unwrap();
    let t = TempoTable::build(&seq.tempo, Tick(0), 48_000.0);
    active_regions(p, lane)
        .into_iter()
        .map(|(tick, off, len)| (t.tick_to_frame(Tick(tick)), off, len))
        .collect()
}

fn assert_no_overlap(regions: &[(u64, u64, u64)]) {
    for w in regions.windows(2) {
        assert!(
            w[0].0 + w[0].2 <= w[1].0,
            "regions overlap on the timeline: {regions:?}"
        );
    }
}

#[test]
fn an_xrun_cuts_the_region_where_the_transport_stood_still() {
    // Pre-flight M1 with Task 5's xrun: 256 frames played from 500, an
    // 8192-frame block the transport did not advance through, then 256
    // more from 756.
    let d = doc(None);
    let frames = 256 + 8192 + 256;
    let mut c = audio_case(&d, 500, frames);
    c.marks = vec![mark(0, 500, false), mark(256 + 8192, 756, false)];
    let p = c.run().unwrap();
    let got = active_frames(&p, d.mic, &d);
    assert_no_overlap(&got);
    assert_eq!(got, vec![(500, 0, 256), (756, 256 + 8192, 256)]);
}

#[test]
fn a_one_frame_resync_keeps_one_region() {
    // Pre-flight M1: a schedule swap moves the frame by one either way.
    let d = doc(None);
    for nudge in [1i64, -1] {
        let mut c = audio_case(&d, 2 * BEAT, 4 * BEAT);
        let at = (3 * BEAT) as i64 + nudge;
        c.marks = vec![mark(0, 2 * BEAT, false), mark(BEAT, at as u64, false)];
        let p = c.run().unwrap();
        let got = active_regions(&p, d.mic);
        assert_eq!(got.len(), 1, "nudge {nudge}: {got:?}");
        assert_eq!(got[0].0, 2 * Q);
        assert_eq!(got[0].1, 0);
        // The file plays on, one frame early or late.
        assert!(got[0].2.abs_diff(4 * BEAT) <= 1, "nudge {nudge}: {got:?}");
    }
}

#[test]
fn a_stop_and_resume_cuts_the_region() {
    // A stop at run 2 beats (at the stop's frame), then a resume from beat
    // 1: the same run, a new frame (Task 5).
    let d = doc(None);
    let mut c = audio_case(&d, BEAT, 4 * BEAT);
    c.marks = vec![
        mark(0, BEAT, false),
        mark(2 * BEAT, 3 * BEAT, false),
        mark(2 * BEAT, BEAT, false),
    ];
    // The resumed pass replays beats 1–3: it replaces what the first one
    // recorded there (review m1: never two regions over each other).
    let p = c.run().unwrap();
    let got = active_frames(&p, d.mic, &d);
    assert_no_overlap(&got);
    assert_eq!(got, vec![(BEAT, 2 * BEAT, 2 * BEAT)]);
    // Resumed where it stopped: one region, the file plays on.
    c.marks[2] = mark(2 * BEAT, 3 * BEAT, false);
    let p = c.run().unwrap();
    assert_eq!(active_frames(&p, d.mic, &d), vec![(BEAT, 0, 4 * BEAT)]);
    // Resumed after a jump forward: two regions, apart.
    c.marks[2] = mark(2 * BEAT, 5 * BEAT, false);
    let p = c.run().unwrap();
    assert_eq!(
        active_frames(&p, d.mic, &d),
        vec![(BEAT, 0, 2 * BEAT), (5 * BEAT, 2 * BEAT, 2 * BEAT)]
    );
}

#[test]
fn a_region_never_starts_before_the_punch_in() {
    // Review m3: a punch-in between two ticks rounds the region's tick up.
    let d = doc(None);
    let seq = d.doc.sequence(d.seq).unwrap();
    let t = TempoTable::build(&seq.tempo, Tick(0), 48_000.0);
    for s in 2 * BEAT..2 * BEAT + 8 {
        let mut c = audio_case(&d, 0, 4 * BEAT);
        c.marks = vec![mark(0, 0, false)];
        c.range = RecordRange {
            start: s,
            end: Some(3 * BEAT),
        };
        let p = c.run().unwrap();
        let (tick, offset, length) = active_regions(&p, d.mic)[0];
        let at = t.tick_to_frame(Tick(tick));
        assert!(at >= s, "punch-in {s}: region at {at}");
        // Still the capture's pairing: file frame f was heard at frame f.
        assert_eq!(at, offset, "punch-in {s}");
        assert_eq!(offset + length, 3 * BEAT, "punch-in {s}");
    }
}

#[test]
fn a_stop_mark_at_the_song_end_ends_the_region_there() {
    // Pre-flight M2: the transport stopped at the song end (beat 4); the
    // run stopped growing there, and nothing plays after the stop mark.
    let d = doc(None);
    let mut c = audio_case(&d, 2 * BEAT, 2 * BEAT);
    c.marks = vec![mark(0, 2 * BEAT, false), mark(2 * BEAT, 4 * BEAT, false)];
    c.end_run = 2 * BEAT;
    c.midi_lanes = vec![d.keys];
    c.midi = vec![midi(d.keys, BEAT as i64, on(60, 100))];
    let p = c.run().unwrap();
    assert_eq!(active_regions(&p, d.mic), vec![(2 * Q, 0, 2 * BEAT)]);
    let l = p.lanes.iter().find(|l| l.lane == d.keys).unwrap();
    assert_eq!(l.clear, vec![(Tick(2 * Q), Tick(4 * Q))]);
    // Held at the stop: it ends at the song end.
    assert_eq!(l.active.events.len(), 1);
    assert_eq!(l.active.events[0].tick, Tick(3 * Q));
    match l.active.events[0].body {
        EventBody::Note { duration, .. } => assert_eq!(duration, Tick(Q)),
        ref other => panic!("{other:?}"),
    }
}

fn take_names(p: &RecordPass, lane: LaneId) -> Vec<String> {
    p.lanes
        .iter()
        .filter(|l| l.lane == lane)
        .flat_map(|l| l.takes.iter().map(|(n, _)| n.clone()))
        .collect()
}

/// A loop of two beats from beat 1, played once and then `tail` frames of
/// a second pass.
fn sliver_case(d: &Doc, tail: u64) -> Case<'_> {
    let len = 2 * BEAT;
    let mut c = audio_case(d, BEAT, len + tail);
    c.looped = true;
    c.range = RecordRange {
        start: BEAT,
        end: Some(BEAT + len),
    };
    c.marks = vec![mark(0, BEAT, false), mark(len, BEAT, true)];
    c.midi_lanes = vec![d.keys];
    c
}

#[test]
fn a_short_pass_after_a_wrap_is_no_take() {
    // Pre-flight M3 (Ruling): Stop 100 frames after a wrap.
    let d = doc(None);
    let mut c = sliver_case(&d, 100);
    let p = c.run().unwrap();
    assert_eq!(take_names(&p, d.mic), vec!["Take 2"]);
    assert_eq!(take_names(&p, d.keys), vec!["Take 2"]);
    // A fate the caller set to Kept keeps it.
    c.fates = vec![PassFate::Undecided, PassFate::Kept];
    let p = c.run().unwrap();
    assert_eq!(take_names(&p, d.mic), vec!["Take 2", "Take 3"]);
    // A whole beat is a pass.
    let mut c = sliver_case(&d, BEAT);
    let p = c.run().unwrap();
    assert_eq!(take_names(&p, d.mic), vec!["Take 2", "Take 3"]);
    // The pass before it discarded: the short one is all there is, kept.
    c = sliver_case(&d, 100);
    c.fates = vec![PassFate::Discarded];
    let p = c.run().unwrap();
    assert_eq!(take_names(&p, d.mic), vec!["Take 2"]);
    assert_eq!(p.lanes[0].takes.len(), 1);
    let take = &p.lanes.iter().find(|l| l.lane == d.mic).unwrap().takes[0].1;
    assert_eq!(take.regions[0].source_offset, 2 * BEAT);
}

#[test]
fn an_only_pass_under_a_beat_is_kept() {
    // Review I1: loop recording stopped within a beat of its start.
    let d = doc(None);
    let mut c = audio_case(&d, BEAT, 100);
    c.looped = true;
    c.range = RecordRange {
        start: BEAT,
        end: Some(3 * BEAT),
    };
    c.midi_lanes = vec![d.keys];
    c.midi = vec![midi(d.keys, 10, on(60, 100))];
    let p = c.run().unwrap();
    assert_eq!(take_names(&p, d.mic), vec!["Take 2"]);
    assert_eq!(take_names(&p, d.keys), vec!["Take 2"]);
    assert_eq!(p.media.len(), 1, "the file is referenced");
    let keys = p.lanes.iter().find(|l| l.lane == d.keys).unwrap();
    assert_eq!(keys.takes[0].1.events.len(), 1);
}

#[test]
fn merged_midi_keeps_what_was_played_after_the_last_wrap() {
    // Review I1: without passes as takes nothing becomes a take, so the
    // short last pass's MIDI stays (audio still makes takes, and drops it).
    let d = doc(None);
    let mut c = sliver_case(&d, 100);
    c.takes = false;
    let len = (2 * BEAT) as i64;
    c.midi = vec![
        midi(d.keys, 10, on(60, 100)),
        midi(d.keys, 20, off(60)),
        midi(d.keys, len + 10, on(64, 100)),
        midi(d.keys, len + 20, off(64)),
    ];
    let p = c.run().unwrap();
    let keys = p.lanes.iter().find(|l| l.lane == d.keys).unwrap();
    assert!(keys.takes.is_empty());
    let pitches: Vec<u8> = keys
        .active
        .events
        .iter()
        .map(|e| match e.body {
            EventBody::Note { pitch, .. } => pitch,
            _ => 0,
        })
        .collect();
    assert_eq!(pitches, vec![60, 64]);
    // Replace clears the short pass's window too.
    assert_eq!(keys.clear.len(), 2, "{:?}", keys.clear);
    assert_eq!(take_names(&p, d.mic), vec!["Take 2"]);
}

#[test]
fn the_short_pass_rule_is_decided_per_lane() {
    // Re-review I1: a MIDI lane silent through pass 0 (a count-in) with one
    // note just after the wrap keeps that note's pass, while a lane that
    // recorded in pass 0 still drops the sliver.
    let mut b = Builder::new();
    let synth = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, keys) = b.sequence("Song", "Keys", Some(synth)).unwrap();
    let pads = b.lane(seq, "Pads", Some(synth)).unwrap();
    let mic = b.lane(seq, "Mic", None).unwrap();
    let d = Doc {
        doc: b.finish(),
        seq,
        keys,
        mic,
    };
    let mut c = sliver_case(&d, 100);
    let len = (2 * BEAT) as i64;
    c.midi_lanes = vec![keys, pads];
    c.midi = vec![
        midi(pads, 10, on(48, 100)),
        midi(pads, 20, off(48)),
        midi(keys, len + 10, on(60, 100)),
    ];
    let p = c.run().unwrap();
    let lane = |id: LaneId| p.lanes.iter().find(|l| l.lane == id).unwrap();
    let k = lane(keys);
    assert_eq!(k.takes.len(), 2, "{:?}", k.takes);
    assert!(k.takes[0].1.events.is_empty());
    assert_eq!(k.takes[1].1.events.len(), 1, "the note is kept");
    assert_eq!(lane(pads).takes.len(), 1);
    assert_eq!(lane(pads).takes[0].1.events.len(), 1);
    assert_eq!(take_names(&p, mic).len(), 1, "audio recorded in pass 0");
}

#[test]
fn a_lone_controller_in_pass_0_does_not_stand_in_for_a_note() {
    // Re-review 2 m5: a sustain pedal resting through the count-in is no
    // take; the note just after the wrap is kept.
    let d = doc(None);
    let mut c = sliver_case(&d, 100);
    c.audio.clear();
    let len = (2 * BEAT) as i64;
    c.midi = vec![
        midi(d.keys, 10, Midi1Msg::three(0xB0, 64, 127)),
        midi(d.keys, len + 10, on(60, 100)),
    ];
    let p = c.run().unwrap();
    let k = p.lanes.iter().find(|l| l.lane == d.keys).unwrap();
    assert_eq!(k.takes.len(), 2, "{:?}", k.takes);
    assert_eq!(k.takes[1].1.events.len(), 1, "the note is kept");
}

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig::with_cases(256))]

    /// Whatever the marks, delays and ranges: every region lies inside
    /// its file and starts inside the record range.
    #[test]
    fn regions_stay_inside_their_file_and_range(
        len in 1_000u64..50_000,
        passes in 1u64..5,
        delay in -500i64..2_000,
        latency in 0u64..1_024,
        start in 0u64..60_000,
        span in proptest::option::of(1u64..60_000),
        looped in proptest::bool::ANY,
    ) {
        let d = doc(None);
        let frames = passes * len;
        let mut c = audio_case(&d, start, frames);
        c.looped = looped;
        c.latency = latency;
        c.input_delay = delay;
        c.end_run = frames;
        c.range = RecordRange { start, end: span.map(|s| start + s) };
        c.marks = (0..passes).map(|k| mark(k * len, start, k > 0)).collect();
        if let Some(p) = c.run() {
            for l in &p.lanes {
                let all = l.active.regions.iter().chain(l.takes.iter().flat_map(|(_, t)| t.regions.iter()));
                for r in all {
                    proptest::prop_assert!(r.length > 0);
                    proptest::prop_assert!(r.source_offset + r.length <= frames);
                    proptest::prop_assert!(r.tick.0 >= 0);
                }
            }
        }
    }
}
