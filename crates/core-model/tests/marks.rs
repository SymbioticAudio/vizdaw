//! Co-Producer's marks (plan 8b Task 2, Ruling 1): what Co-Producer makes
//! is marked and named "… (Co-Producer)", the composer takes a thing over
//! by renaming it or with Make Mine, and a hand step never makes a marked
//! thing.

use core_model::command::{Command, LaneCmd, MixerCmd, Report, RoutingCmd, SequenceCmd, apply};
use core_model::document::{DestKind, Document};
use core_model::history::History;
use core_model::ids::{AnyId, DestId, LaneId, OutputPathId, SeqId, StripId};
use core_model::marks::{SUFFIX, lane_strip};
use core_model::mixer::{AuxSend, BUILTIN_SYNTH_REF, PluginFormat, PluginRef, StripKind};
use core_model::selection::ViewFilter;

const SEQ: SeqId = SeqId(0);

fn f() -> ViewFilter {
    ViewFilter::default()
}

fn run(d: &Document, c: Command) -> (Document, Report) {
    let a = apply(d, &c).unwrap();
    (a.doc, a.report)
}

fn created_output(r: &Report) -> OutputPathId {
    r.created
        .iter()
        .find_map(|i| match i {
            AnyId::Output(o) => Some(*o),
            _ => None,
        })
        .unwrap()
}

fn created_lane(r: &Report) -> LaneId {
    r.created
        .iter()
        .find_map(|i| match i {
            AnyId::Lane(l) => Some(*l),
            _ => None,
        })
        .unwrap()
}

fn created_strip(r: &Report) -> StripId {
    r.created
        .iter()
        .find_map(|i| match i {
            AnyId::Strip(s) => Some(*s),
            _ => None,
        })
        .unwrap()
}

fn created_dest(r: &Report) -> DestId {
    r.created
        .iter()
        .find_map(|i| match i {
            AnyId::Dest(d) => Some(*d),
            _ => None,
        })
        .unwrap()
}

fn eq_plugin() -> PluginRef {
    PluginRef {
        format: PluginFormat::Au,
        id: "aufx:eq:test".into(),
        name: "Test EQ".into(),
        vendor: "Test".into(),
        version: "1".into(),
    }
}

/// A song with one hand track, "Audio 1" (lane 0), playing through its own
/// "Audio 1" strip, and a history with nothing to undo.
fn fixture() -> History {
    let (d, _) = run(
        &Document::new(),
        Command::Sequence(SequenceCmd::New { name: "S".into() }),
    );
    let (d, _) = run(
        &d,
        Command::Lane(LaneCmd::Rename {
            seq: SEQ,
            lane: LaneId(0),
            name: "Audio 1".into(),
        }),
    );
    let (d, r) = run(
        &d,
        Command::Routing(RoutingCmd::AddOutput {
            name: "Audio 1".into(),
            reference: "audio:1-2".into(),
        }),
    );
    let out = created_output(&r);
    let (d, _) = run(
        &d,
        Command::Lane(LaneCmd::SetOutput {
            seq: SEQ,
            lane: LaneId(0),
            output: Some(out),
        }),
    );
    History::new(d)
}

fn exec(h: &mut History, c: Command) -> Report {
    h.execute(&c, &f()).unwrap()
}

/// Runs `work` as Co-Producer inside an open request.
fn acting<T>(h: &mut History, work: impl FnOnce(&mut History) -> T) -> T {
    if h.request().is_none() {
        h.begin_request("help me").unwrap();
    }
    h.set_acting(true).unwrap();
    let out = work(h);
    h.set_acting(false).unwrap();
    out
}

/// A new audio track "Audio 2" with its own output path and strip, as
/// three steps; answers the lane and the strip.
fn new_audio_track(h: &mut History) -> (LaneId, StripId) {
    let lane = created_lane(&exec(
        h,
        Command::Lane(LaneCmd::Add {
            seq: SEQ,
            name: "Audio 2".into(),
            index: None,
        }),
    ));
    let r = exec(
        h,
        Command::Routing(RoutingCmd::AddOutput {
            name: "Audio 2".into(),
            reference: "audio:3-4".into(),
        }),
    );
    let (out, strip) = (created_output(&r), created_strip(&r));
    exec(
        h,
        Command::Lane(LaneCmd::SetOutput {
            seq: SEQ,
            lane,
            output: Some(out),
        }),
    );
    (lane, strip)
}

fn new_bus(h: &mut History, name: &str) -> StripId {
    created_strip(&exec(
        h,
        Command::Mixer(MixerCmd::AddBus {
            name: name.into(),
            index: None,
        }),
    ))
}

fn lane_of(h: &History, lane: LaneId) -> core_model::lane::Lane {
    h.doc().lane(SEQ, lane).unwrap().clone()
}

fn strip_of(h: &History, strip: StripId) -> core_model::mixer::Strip {
    h.doc().mixer().strip(strip).unwrap().clone()
}

fn hand_strip(h: &History) -> StripId {
    lane_strip(h.doc(), LaneId(0)).unwrap()
}

fn rename_lane(lane: LaneId, name: &str) -> Command {
    Command::Lane(LaneCmd::Rename {
        seq: SEQ,
        lane,
        name: name.into(),
    })
}

#[test]
fn acting_steps_mark_what_they_make() {
    let mut h = fixture();
    h.set_new_strip_eq(Some(eq_plugin()));
    let hand = hand_strip(&h);
    let (lane, strip) = acting(&mut h, new_audio_track);
    let l = lane_of(&h, lane);
    assert!(l.co_producer);
    assert_eq!(l.name, "Audio 2 (Co-Producer)");
    let s = strip_of(&h, strip);
    assert!(s.co_producer);
    assert_eq!(s.name, "Audio 2 (Co-Producer)");
    assert_eq!(lane_strip(h.doc(), lane), Some(strip));
    // The new strip's EQ from `fill_new_eqs` is marked.
    assert!(s.eq.as_ref().unwrap().co_producer);
    // The hand track and its strip are untouched.
    assert!(!lane_of(&h, LaneId(0)).co_producer);
    assert_eq!(lane_of(&h, LaneId(0)).name, "Audio 1");
    assert!(!strip_of(&h, hand).co_producer);

    // A new bus with a send from the hand strip.
    let bus = acting(&mut h, |h| {
        let bus = new_bus(h, "Drums");
        exec(
            h,
            Command::Mixer(MixerCmd::AddSend {
                strip: hand,
                send: AuxSend::new(bus),
            }),
        );
        bus
    });
    let b = strip_of(&h, bus);
    assert!(b.co_producer);
    assert_eq!(b.name, "Drums (Co-Producer)");
    let hs = strip_of(&h, hand);
    assert!(!hs.co_producer);
    assert_eq!(hs.name, "Audio 1");
    let send = hs.sends.iter().find(|x| x.target == bus).unwrap();
    assert!(send.co_producer);
    // An insert Co-Producer adds to the hand strip is marked; the strip is not.
    acting(&mut h, |h| {
        exec(
            h,
            Command::Mixer(MixerCmd::AddInsert {
                strip: hand,
                index: None,
                plugin: eq_plugin(),
            }),
        )
    });
    let hs = strip_of(&h, hand);
    assert!(hs.inserts.last().unwrap().co_producer);
    assert!(!hs.co_producer);
}

#[test]
fn acting_renames_keep_the_suffix() {
    let mut h = fixture();
    let (lane, _) = acting(&mut h, new_audio_track);
    acting(&mut h, |h| exec(h, rename_lane(lane, "Kick")));
    let l = lane_of(&h, lane);
    assert_eq!(l.name, "Kick (Co-Producer)");
    assert!(l.co_producer);
    acting(&mut h, |h| exec(h, rename_lane(lane, "Kick (Co-Producer)")));
    let l = lane_of(&h, lane);
    assert_eq!(l.name, "Kick (Co-Producer)");
    assert!(l.co_producer);
    // An acting rename of the composer's track marks nothing.
    acting(&mut h, |h| exec(h, rename_lane(LaneId(0), "Vox")));
    let l = lane_of(&h, LaneId(0));
    assert_eq!(l.name, "Vox");
    assert!(!l.co_producer);
}

#[test]
fn hand_steps_never_make_co_producer_things() {
    let mut h = fixture();
    let (lane, _) = acting(&mut h, new_audio_track);
    h.end_request().unwrap();
    let r = exec(&mut h, Command::Lane(LaneCmd::Duplicate { seq: SEQ, lane }));
    let copy = lane_of(&h, created_lane(&r));
    assert!(!copy.co_producer);
    assert_eq!(copy.name, "Audio 2 (Co-Producer) copy");
    // The copy plays through the same (marked) strip; no new strip.
    assert!(lane_of(&h, lane).co_producer);
    let bus = new_bus(&mut h, "Verb");
    let b = strip_of(&h, bus);
    assert!(!b.co_producer);
    assert_eq!(b.name, "Verb");
    // A hand send from the Co-Producer strip is the composer's.
    let strip = lane_strip(h.doc(), lane).unwrap();
    exec(
        &mut h,
        Command::Mixer(MixerCmd::AddSend {
            strip,
            send: AuxSend::new(bus),
        }),
    );
    assert!(!strip_of(&h, strip).sends.iter().any(|s| s.co_producer));
}

#[test]
fn a_hand_rename_takes_over() {
    let mut h = fixture();
    let (lane, strip) = acting(&mut h, new_audio_track);
    h.end_request().unwrap();
    exec(&mut h, rename_lane(lane, "Bass"));
    let l = lane_of(&h, lane);
    assert!(!l.co_producer);
    assert_eq!(l.name, "Bass");
    let s = strip_of(&h, strip);
    assert!(s.co_producer);
    assert_eq!(s.name, "Audio 2 (Co-Producer)");
    exec(
        &mut h,
        Command::Mixer(MixerCmd::RenameStrip {
            strip,
            name: "Bass".into(),
        }),
    );
    let s = strip_of(&h, strip);
    assert!(!s.co_producer);
    assert_eq!(s.name, "Bass");
}

#[test]
fn make_mine_takes_over_a_track_and_its_strip() {
    let mut h = fixture();
    h.set_new_strip_eq(Some(eq_plugin()));
    let (lane, strip, bus) = acting(&mut h, |h| {
        let (lane, strip) = new_audio_track(h);
        let bus = new_bus(h, "Drums");
        exec(
            h,
            Command::Mixer(MixerCmd::AddSend {
                strip,
                send: AuxSend::new(bus),
            }),
        );
        exec(
            h,
            Command::Mixer(MixerCmd::AddInsert {
                strip,
                index: None,
                plugin: eq_plugin(),
            }),
        );
        exec(
            h,
            Command::Mixer(MixerCmd::AddInsert {
                strip: bus,
                index: None,
                plugin: eq_plugin(),
            }),
        );
        (lane, strip, bus)
    });
    h.end_request().unwrap();
    let before = h.doc().clone();
    let depth = h.undo_depth();

    exec(&mut h, Command::Lane(LaneCmd::MakeMine { seq: SEQ, lane }));
    assert_eq!(h.undo_label(), Some("Make Mine"));
    assert_eq!(h.undo_depth(), depth + 1);
    let l = lane_of(&h, lane);
    assert!(!l.co_producer);
    assert_eq!(l.name, "Audio 2");
    let s = strip_of(&h, strip);
    assert!(!s.co_producer);
    assert_eq!(s.name, "Audio 2");
    assert!(s.sends.iter().all(|x| !x.co_producer));
    assert!(s.chain().all(|p| !p.co_producer));
    // The bus is not the track's strip: it stays Co-Producer's.
    let b = strip_of(&h, bus);
    assert!(b.co_producer);
    assert!(b.chain().all(|p| p.co_producer));

    // One undo restores every mark and name exactly.
    h.undo().unwrap();
    assert_eq!(h.doc(), &before);

    // Make Mine of an unmarked track adds no undo step.
    let depth = h.undo_depth();
    exec(
        &mut h,
        Command::Lane(LaneCmd::MakeMine {
            seq: SEQ,
            lane: LaneId(0),
        }),
    );
    assert_eq!(h.undo_depth(), depth);
    assert_eq!(h.doc(), &before);

    // Make Mine of the bus: only the bus, its sends and its slots.
    exec(&mut h, Command::Mixer(MixerCmd::MakeMine { strip: bus }));
    assert_eq!(h.undo_label(), Some("Make Mine"));
    let b = strip_of(&h, bus);
    assert!(!b.co_producer);
    assert_eq!(b.name, "Drums");
    assert!(b.chain().all(|p| !p.co_producer));
    assert!(b.sends.iter().all(|x| !x.co_producer));
    let s = strip_of(&h, strip);
    assert!(s.co_producer);
    assert!(s.sends.iter().all(|x| x.co_producer));
    assert!(lane_of(&h, lane).co_producer);

    // An unknown target is refused.
    assert!(
        h.execute(
            &Command::Mixer(MixerCmd::MakeMine {
                strip: StripId(9_999)
            }),
            &f()
        )
        .is_err()
    );
    assert!(
        h.execute(
            &Command::Lane(LaneCmd::MakeMine {
                seq: SEQ,
                lane: LaneId(9_999)
            }),
            &f()
        )
        .is_err()
    );
}

#[test]
fn lane_strip_matches_the_app() {
    let h = fixture();
    let d = h.doc().clone();
    // An audio lane: its path's strip.
    let audio = lane_strip(&d, LaneId(0)).unwrap();
    assert!(matches!(
        d.mixer().strip(audio).unwrap().kind,
        StripKind::AudioPath(_)
    ));
    // A plug-in instrument lane: its Instrument strip.
    let (d, r) = run(
        &d,
        Command::Routing(RoutingCmd::AddDestination {
            name: "Keys".into(),
            kind: DestKind::plugin("Keys"),
            default_patch: None,
        }),
    );
    let keys = created_dest(&r);
    let (d, r) = run(
        &d,
        Command::Lane(LaneCmd::Add {
            seq: SEQ,
            name: "Keys".into(),
            index: None,
        }),
    );
    let keys_lane = created_lane(&r);
    let (d, _) = run(
        &d,
        Command::Lane(LaneCmd::SetDest {
            seq: SEQ,
            lane: keys_lane,
            dest: Some(keys),
        }),
    );
    let s = lane_strip(&d, keys_lane).unwrap();
    assert_eq!(
        d.mixer().strip(s).unwrap().kind,
        StripKind::Instrument(keys)
    );
    // An external MIDI lane: none.
    let (d, r) = run(
        &d,
        Command::Routing(RoutingCmd::AddDestination {
            name: "Synth".into(),
            kind: DestKind::MidiEndpoint {
                endpoint_ref: "port:1".into(),
                group: 0,
                channel: 0,
            },
            default_patch: None,
        }),
    );
    let ext = created_dest(&r);
    let (d, _) = run(
        &d,
        Command::Lane(LaneCmd::SetDest {
            seq: SEQ,
            lane: keys_lane,
            dest: Some(ext),
        }),
    );
    assert_eq!(lane_strip(&d, keys_lane), None);
    // A built-in synth lane: the built-in synth's strip.
    let (d, r) = run(
        &d,
        Command::Routing(RoutingCmd::AddDestination {
            name: "Built-in Synth".into(),
            kind: DestKind::MidiEndpoint {
                endpoint_ref: BUILTIN_SYNTH_REF.into(),
                group: 0,
                channel: 0,
            },
            default_patch: None,
        }),
    );
    let synth = created_dest(&r);
    let (d, _) = run(
        &d,
        Command::Lane(LaneCmd::SetDest {
            seq: SEQ,
            lane: keys_lane,
            dest: Some(synth),
        }),
    );
    let s = lane_strip(&d, keys_lane).unwrap();
    assert_eq!(d.mixer().strip(s).unwrap().kind, StripKind::BuiltinSynth);
    // A lane with neither, or no lane: none.
    let (d, r) = run(
        &d,
        Command::Lane(LaneCmd::Add {
            seq: SEQ,
            name: "Empty".into(),
            index: None,
        }),
    );
    assert_eq!(lane_strip(&d, created_lane(&r)), None);
    assert_eq!(lane_strip(&d, LaneId(9_999)), None);
    assert_eq!(SUFFIX, " (Co-Producer)");
}
