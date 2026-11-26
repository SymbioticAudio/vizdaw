//! What a change does, in plain items (plan 8a Task 4, Ruling 6): the
//! list the composer says yes to.

use core_model::automation::{AutomationTarget, SendIndex};
use core_model::command::{
    AudioCmd, AutomationWrite, Command, EventCmd, LaneCmd, MediaCmd, MixerCmd, RoutingCmd,
    SequenceCmd, apply,
};
use core_model::diff::{ChangeItem, MAX_PARTS, bar_of, describe, describe_with, instrument_strip};
use core_model::document::{DestKind, Document};
use core_model::event::{EventBody, EventSpec, Fade, FadeShape};
use core_model::file::{from_json, to_json};
use core_model::history::History;
use core_model::ids::{LaneId, MediaId, PluginInstanceId, SeqId, StripId};
use core_model::mixer::{AuxSend, Output, ParamId, PluginFormat, PluginRef, Swatch};
use core_model::selection::ViewFilter;
use core_model::tempo::{Bpm, TempoMap};
use core_model::time::Tick;

fn run(d: &Document, c: Command) -> Document {
    apply(d, &c).unwrap().doc
}

fn item(thing: &str, change: &str) -> ChangeItem {
    ChangeItem {
        thing: thing.into(),
        change: change.into(),
        co_producers: false,
    }
}

/// An item about something Co-Producer made that takes away nothing.
fn ours(thing: &str, change: &str) -> ChangeItem {
    ChangeItem {
        co_producers: true,
        ..item(thing, change)
    }
}

/// `c` applied with Co-Producer acting, so what it makes is marked.
fn acting(d: &Document, c: Command) -> Document {
    let mut h = History::new(d.clone());
    h.begin_request("help").unwrap();
    h.set_acting(true).unwrap();
    h.execute(&c, &ViewFilter::default()).unwrap();
    h.doc().clone()
}

fn add_lane(d: &Document, seq: u64, name: &str) -> Document {
    run(
        d,
        Command::Lane(LaneCmd::Add {
            seq: SeqId(seq),
            name: name.into(),
            index: None,
        }),
    )
}

fn notes(d: &Document, lane: u64, n: usize) -> Document {
    run(
        d,
        Command::Event(EventCmd::Insert {
            seq: SeqId(0),
            lane: LaneId(lane),
            events: (0..n)
                .map(|i| EventSpec::new(Tick(i as i64 * 10), EventBody::note(60, 0x8000, Tick(5))))
                .collect(),
        }),
    )
}

fn regions(d: &Document, lane: u64, n: usize) -> Document {
    let mut d = d.clone();
    if d.media().is_empty() {
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
        );
    }
    let fade = Fade {
        length_frames: 0,
        shape: FadeShape::Linear,
    };
    for i in 0..n {
        d = run(
            &d,
            Command::Audio(AudioCmd::Insert {
                seq: SeqId(0),
                lane: LaneId(lane),
                tick: Tick(i as i64 * 10_000),
                media: MediaId(0),
                source_offset: 0,
                length: 1000,
                gain_db: 0.0,
                fade_in: fade,
                fade_out: fade,
            }),
        );
    }
    d
}

/// Sequence 0 "Song" with lane 0 "Kick" and lane 1 "Lead Vox".
fn song() -> Document {
    let d = run(
        &Document::new(),
        Command::Sequence(SequenceCmd::New {
            name: "Song".into(),
        }),
    );
    let d = run(
        &d,
        Command::Lane(LaneCmd::Rename {
            seq: SeqId(0),
            lane: LaneId(0),
            name: "Kick".into(),
        }),
    );
    add_lane(&d, 0, "Lead Vox")
}

fn lane_id(d: &Document, name: &str) -> LaneId {
    d.sequence(SeqId(0))
        .unwrap()
        .lanes
        .iter()
        .find(|l| l.name == name)
        .unwrap()
        .id
}

#[test]
fn equal_documents_have_no_items() {
    let d = song();
    assert_eq!(describe(&d, &d.clone()), vec![]);
}

#[test]
fn a_removed_track_says_what_it_held() {
    let d = song();
    let vox = lane_id(&d, "Lead Vox");
    let d = notes(&d, vox.0, 4);
    let gone = run(
        &d,
        Command::Lane(LaneCmd::Remove {
            seq: SeqId(0),
            lane: vox,
        }),
    );
    assert_eq!(
        describe(&d, &gone),
        vec![item("Track 2, Lead Vox", "deleted, with 4 notes")]
    );
    // An empty track says only "deleted"; regions and notes both named.
    let empty = run(
        &song(),
        Command::Lane(LaneCmd::Remove {
            seq: SeqId(0),
            lane: LaneId(0),
        }),
    );
    assert_eq!(
        describe(&song(), &empty),
        vec![item("Track 1, Kick", "deleted")]
    );
    let both = notes(&regions(&song(), 0, 2), 0, 1);
    let gone = run(
        &both,
        Command::Lane(LaneCmd::Remove {
            seq: SeqId(0),
            lane: LaneId(0),
        }),
    );
    // Its regions gave it an output path of its own, which goes with it
    // (plan 8e Task 6c, fix round 1).
    assert_eq!(
        describe(&both, &gone),
        vec![
            item("Track 1, Kick", "deleted, with 2 regions and 1 note"),
            item("Strip Audio 1", "deleted"),
        ]
    );
}

#[test]
fn an_added_track_is_added() {
    let d = song();
    let more = add_lane(&d, 0, "Bass");
    assert_eq!(describe(&d, &more), vec![item("Track 3, Bass", "added")]);
}

#[test]
fn a_renamed_track_names_both() {
    let d = song();
    let vox = lane_id(&d, "Lead Vox");
    let renamed = run(
        &d,
        Command::Lane(LaneCmd::Rename {
            seq: SeqId(0),
            lane: vox,
            name: "Vox".into(),
        }),
    );
    assert_eq!(
        describe(&d, &renamed),
        vec![item("Track 2, Lead Vox", "renamed to \"Vox\"")]
    );
}

#[test]
fn changed_event_counts_are_listed() {
    let d = regions(&song(), 0, 12);
    let fewer = run(
        &d,
        Command::Event(EventCmd::Delete {
            events: d.all_events().take(2).map(|(_, e)| e.id).collect(),
        }),
    );
    assert_eq!(
        describe(&d, &fewer),
        vec![item("Track 1, Kick", "12 regions → 10 regions")]
    );
    let vox = lane_id(&d, "Lead Vox");
    let d = notes(&song(), vox.0, 340);
    let ids: Vec<_> = d.all_events().take(4).map(|(_, e)| e.id).collect();
    let fewer = run(&d, Command::Event(EventCmd::Delete { events: ids }));
    assert_eq!(
        describe(&d, &fewer),
        vec![item("Track 2, Lead Vox", "340 notes → 336 notes")]
    );
}

#[test]
fn sequences_added_removed_and_renamed() {
    let d = song();
    let two = run(
        &d,
        Command::Sequence(SequenceCmd::New {
            name: "Verse".into(),
        }),
    );
    assert_eq!(describe(&d, &two), vec![item("Sequence 2, Verse", "added")]);
    assert_eq!(
        describe(&two, &d),
        vec![item("Sequence 2, Verse", "deleted")]
    );
    let renamed = run(
        &two,
        Command::Sequence(SequenceCmd::Rename {
            seq: SeqId(1),
            name: "Chorus".into(),
        }),
    );
    assert_eq!(
        describe(&two, &renamed),
        vec![item("Sequence 2, Verse", "renamed to \"Chorus\"")]
    );
}

fn bus(d: &Document, name: &str) -> StripId {
    *d.mixer()
        .strips()
        .iter()
        .find(|(_, s)| s.name == name)
        .unwrap()
        .0
}

#[test]
fn strips_added_removed_and_renamed() {
    let d = song();
    let with = run(
        &d,
        Command::Mixer(MixerCmd::AddBus {
            name: "Drums".into(),
            index: None,
        }),
    );
    assert_eq!(describe(&d, &with), vec![item("Strip Drums", "added")]);
    assert_eq!(describe(&with, &d), vec![item("Strip Drums", "deleted")]);
    let renamed = run(
        &with,
        Command::Mixer(MixerCmd::RenameStrip {
            strip: bus(&with, "Drums"),
            name: "Kit".into(),
        }),
    );
    assert_eq!(
        describe(&with, &renamed),
        vec![item("Strip Drums", "renamed to \"Kit\"")]
    );
}

#[test]
fn an_unlisted_change_is_other_changes() {
    let d = song();
    let faster = run(
        &d,
        Command::Sequence(SequenceCmd::SetTempo {
            seq: SeqId(0),
            tempo: TempoMap::constant(Bpm(140_000)).unwrap(),
        }),
    );
    assert_ne!(d, faster);
    assert_eq!(
        describe(&d, &faster),
        vec![item("The song", "other changes")]
    );
}

#[test]
fn names_are_shown_safely() {
    let d = song();
    // Names the model's own rename refuses can still arrive in a file.
    let hostile = from_json(
        &to_json(&d)
            .unwrap()
            .replace("\"Lead Vox\"", "\"a\\nb\\u202Ec\""),
    )
    .unwrap();
    let vox = lane_id(&d, "Lead Vox");
    let renamed = run(
        &hostile,
        Command::Lane(LaneCmd::Rename {
            seq: SeqId(0),
            lane: vox,
            name: "Vox".into(),
        }),
    );
    assert_eq!(
        describe(&hostile, &renamed),
        vec![item("Track 2, a b c", "renamed to \"Vox\"")]
    );
    let long = "x".repeat(80);
    let named = run(
        &d,
        Command::Lane(LaneCmd::Rename {
            seq: SeqId(0),
            lane: vox,
            name: long.clone(),
        }),
    );
    let items = describe(&d, &named);
    let shown = items[0].change.trim_start_matches("renamed to \"");
    let shown = shown.trim_end_matches('"');
    assert_eq!(shown.chars().count(), 60);
    assert!(shown.ends_with('…'));
    assert_eq!(shown, format!("{}…", "x".repeat(59)));
    // Line and paragraph separators and C1 controls become spaces too.
    let sep = run(
        &d,
        Command::Mixer(MixerCmd::AddBus {
            name: "a\u{2028}b\u{2029}c\u{85}d\u{200F}e".into(),
            index: None,
        }),
    );
    assert_eq!(describe(&d, &sep), vec![item("Strip a b c d e", "added")]);
}

#[test]
fn items_know_co_producers_things() {
    let d = acting(
        &song(),
        Command::Lane(LaneCmd::Add {
            seq: SeqId(0),
            name: "Bass".into(),
            index: None,
        }),
    );
    let bass = lane_id(&d, "Bass (Co-Producer)");
    let rename = |d: &Document, lane: LaneId, name: &str| {
        run(
            d,
            Command::Lane(LaneCmd::Rename {
                seq: SeqId(0),
                lane,
                name: name.into(),
            }),
        )
    };
    let remove = |d: &Document, lane: LaneId| {
        run(
            d,
            Command::Lane(LaneCmd::Remove {
                seq: SeqId(0),
                lane,
            }),
        )
    };
    assert_eq!(
        describe(&d, &rename(&d, bass, "Sub")),
        vec![ours("Track 3, Bass (Co-Producer)", "renamed to \"Sub\"")]
    );
    let vox = lane_id(&d, "Lead Vox");
    assert_eq!(
        describe(&d, &rename(&d, vox, "Vox")),
        vec![item("Track 2, Lead Vox", "renamed to \"Vox\"")]
    );
    // Deleting an empty Co-Producer track is its own; a filled one is not.
    assert_eq!(
        describe(&d, &remove(&d, bass)),
        vec![ours("Track 3, Bass (Co-Producer)", "deleted")]
    );
    let filled = notes(&d, bass.0, 4);
    assert_eq!(
        describe(&filled, &remove(&filled, bass)),
        vec![item("Track 3, Bass (Co-Producer)", "deleted, with 4 notes")]
    );
    // A falling count takes the composer's work away; a rising one does not.
    let twelve = notes(&d, bass.0, 12);
    let ids: Vec<_> = twelve
        .all_events()
        .filter(|(loc, _)| loc.lane == bass)
        .take(2)
        .map(|(_, e)| e.id)
        .collect();
    let ten = run(&twelve, Command::Event(EventCmd::Delete { events: ids }));
    assert_eq!(
        describe(&twelve, &ten),
        vec![item("Track 3, Bass (Co-Producer)", "12 notes → 10 notes")]
    );
    assert_eq!(
        describe(&ten, &twelve),
        vec![ours("Track 3, Bass (Co-Producer)", "10 notes → 12 notes")]
    );
    // A Co-Producer bus: renamed and deleted are its own.
    let with = acting(
        &d,
        Command::Mixer(MixerCmd::AddBus {
            name: "Drums".into(),
            index: None,
        }),
    );
    let drums = bus(&with, "Drums (Co-Producer)");
    assert_eq!(
        describe(&d, &with),
        vec![item("Strip Drums (Co-Producer)", "added")]
    );
    let kit = run(
        &with,
        Command::Mixer(MixerCmd::RenameStrip {
            strip: drums,
            name: "Kit".into(),
        }),
    );
    assert_eq!(
        describe(&with, &kit),
        vec![ours("Strip Drums (Co-Producer)", "renamed to \"Kit\"")]
    );
    let gone = run(&with, Command::Mixer(MixerCmd::RemoveBus { strip: drums }));
    assert_eq!(
        describe(&with, &gone),
        vec![ours("Strip Drums (Co-Producer)", "deleted")]
    );
}

#[test]
fn strip_colour_is_an_item() {
    let d = run(
        &song(),
        Command::Mixer(MixerCmd::AddBus {
            name: "Drums".into(),
            index: None,
        }),
    );
    let drums = bus(&d, "Drums");
    let colour = |d: &Document, c: Option<Swatch>| {
        run(
            d,
            Command::Mixer(MixerCmd::SetColour {
                strip: drums,
                colour: c,
            }),
        )
    };
    let blue = colour(&d, Some(Swatch::Blue));
    assert_eq!(
        describe(&d, &blue),
        vec![item("Strip Drums", "colour none → blue")]
    );
    assert_eq!(
        describe(&blue, &d),
        vec![item("Strip Drums", "colour blue → none")]
    );
    assert_eq!(
        describe(&blue, &colour(&blue, Some(Swatch::Purple))),
        vec![item("Strip Drums", "colour blue → purple")]
    );
    // A Co-Producer strip's colour is its own.
    let ours_d = acting(
        &song(),
        Command::Mixer(MixerCmd::AddBus {
            name: "Drums".into(),
            index: None,
        }),
    );
    let id = bus(&ours_d, "Drums (Co-Producer)");
    let teal = run(
        &ours_d,
        Command::Mixer(MixerCmd::SetColour {
            strip: id,
            colour: Some(Swatch::Teal),
        }),
    );
    assert_eq!(
        describe(&ours_d, &teal),
        vec![ours("Strip Drums (Co-Producer)", "colour none → teal")]
    );
}

/// Sequence 0 with lane 0 "Kick" and lane 1 "Bass", which holds four
/// notes off a 3840-tick grid, velocities 0x8000.
fn bass_song() -> Document {
    let d = run(
        &Document::new(),
        Command::Sequence(SequenceCmd::New {
            name: "Song".into(),
        }),
    );
    let d = run(
        &d,
        Command::Lane(LaneCmd::Rename {
            seq: SeqId(0),
            lane: LaneId(0),
            name: "Kick".into(),
        }),
    );
    let d = add_lane(&d, 0, "Bass");
    run(
        &d,
        Command::Event(EventCmd::Insert {
            seq: SeqId(0),
            lane: LaneId(1),
            events: [100, 4000, 7600, 11600]
                .iter()
                .map(|t| EventSpec::new(Tick(*t), EventBody::note(60, 0x8000, Tick(480))))
                .collect(),
        }),
    )
}

fn bass_notes(d: &Document) -> Vec<core_model::ids::EventId> {
    d.lane(SeqId(0), LaneId(1))
        .unwrap()
        .active()
        .unwrap()
        .events
        .values()
        .map(|e| e.id)
        .collect()
}

#[test]
fn note_changes_are_items() {
    use core_model::modify::{Modify, Quantize};
    let d = bass_song();
    let quantized = run(
        &d,
        Command::ModifyNotes {
            seq: SeqId(0),
            lanes: vec![LaneId(1)],
            range: None,
            modify: Modify::Quantize(Quantize::Grid {
                grid: Tick(3840),
                strength: 100,
            }),
        },
    );
    assert_eq!(
        describe(&d, &quantized),
        vec![item("Track 2, Bass", "4 notes changed (start)")]
    );
    // One note already has the velocity: three change.
    let ids = bass_notes(&d);
    let one = run(
        &d,
        Command::Event(EventCmd::SetVelocity {
            events: vec![ids[0]],
            velocity: 0x9000,
        }),
    );
    let all = run(
        &one,
        Command::Event(EventCmd::SetVelocity {
            events: ids.clone(),
            velocity: 0x9000,
        }),
    );
    assert_eq!(
        describe(&one, &all),
        vec![item("Track 2, Bass", "3 notes changed (velocity)")]
    );
    let both = run(
        &quantized,
        Command::Event(EventCmd::SetVelocity {
            events: vec![ids[1]],
            velocity: 0x9000,
        }),
    );
    assert_eq!(
        describe(&d, &both),
        vec![item("Track 2, Bass", "4 notes changed (start, velocity)")]
    );
    // A falling count and a change are one item.
    let fewer = run(
        &quantized,
        Command::Event(EventCmd::Delete {
            events: vec![ids[3]],
        }),
    );
    assert_eq!(
        describe(&d, &fewer),
        vec![item(
            "Track 2, Bass",
            "4 notes → 3 notes, 3 notes changed (start)"
        )]
    );
}

fn with_markers(d: &Document, markers: &[(i64, &str)]) -> Document {
    run(
        d,
        Command::Sequence(SequenceCmd::SetMarkers {
            seq: SeqId(0),
            markers: markers
                .iter()
                .map(|(t, n)| core_model::sequence::Marker {
                    tick: Tick(*t),
                    name: (*n).into(),
                })
                .collect(),
        }),
    )
}

#[test]
fn marker_changes_are_items() {
    let d = with_markers(&song(), &[(0, "Verse"), (30720, "Outro")]);
    let three = with_markers(&d, &[(0, "Verse"), (15360, "Chorus"), (30720, "Outro")]);
    assert_eq!(
        describe(&d, &three),
        vec![item("Marker 2, Chorus", "added")]
    );
    let intro = with_markers(&three, &[(0, "Intro"), (15360, "Chorus"), (30720, "Outro")]);
    assert_eq!(
        describe(&three, &intro),
        vec![item("Marker 1, Verse", "renamed to \"Intro\"")]
    );
    let two = with_markers(&three, &[(0, "Verse"), (15360, "Chorus")]);
    assert_eq!(
        describe(&three, &two),
        vec![item("Marker 3, Outro", "deleted")]
    );
    // Two markers at one tick, one renamed: one rename.
    let pair = with_markers(&song(), &[(0, "A"), (0, "B")]);
    let renamed = with_markers(&song(), &[(0, "A"), (0, "C")]);
    assert_eq!(
        describe(&pair, &renamed),
        vec![item("Marker 2, B", "renamed to \"C\"")]
    );
    // Names are shown safely.
    let hostile = with_markers(&d, &[(0, "Verse\nOK: all fine"), (30720, "Outro")]);
    assert_eq!(
        describe(&d, &hostile),
        vec![item("Marker 1, Verse", "renamed to \"Verse OK: all fine\"")]
    );
}

// ---- Plan 8d Ruling 4: the mixer in the preview ----

/// A mixer fixture: buses Kick, Snare and Bass (the composer's), bus
/// "Drums (Co-Producer)" (marked), master, and plug-in instrument
/// destination "Synth" with its strip.
fn mix() -> Document {
    let mut d = song();
    for name in ["Kick", "Snare", "Bass"] {
        d = run(
            &d,
            Command::Mixer(MixerCmd::AddBus {
                name: name.into(),
                index: None,
            }),
        );
    }
    let d = acting(
        &d,
        Command::Mixer(MixerCmd::AddBus {
            name: "Drums".into(),
            index: None,
        }),
    );
    run(
        &d,
        Command::Routing(RoutingCmd::AddDestination {
            name: "Synth".into(),
            kind: DestKind::plugin("Synth"),
            default_patch: None,
        }),
    )
}

/// `cmds` executed in order on a `History`.
fn exec(d: &Document, cmds: Vec<MixerCmd>) -> Document {
    let mut h = History::new(d.clone());
    for c in cmds {
        h.execute(&Command::Mixer(c), &ViewFilter::default())
            .unwrap();
    }
    h.doc().clone()
}

fn plugin(name: &str) -> PluginRef {
    PluginRef {
        format: PluginFormat::Clap,
        id: format!("test.{name}"),
        name: name.into(),
        vendor: "Test".into(),
        version: "1".into(),
    }
}

fn add_insert(strip: StripId, name: &str) -> MixerCmd {
    MixerCmd::AddInsert {
        strip,
        index: None,
        plugin: plugin(name),
    }
}

fn inserts(d: &Document, strip: StripId) -> Vec<PluginInstanceId> {
    d.mixer()
        .strip(strip)
        .unwrap()
        .inserts
        .iter()
        .map(|p| p.id)
        .collect()
}

fn instrument_slot(d: &Document) -> PluginInstanceId {
    d.instruments()
        .values()
        .find_map(|dest| match &dest.kind {
            DestKind::PluginInstrument { slot } => Some(slot.id),
            _ => None,
        })
        .unwrap()
}

#[test]
fn strip_levels_are_items() {
    let d = mix();
    let kick = bus(&d, "Kick");
    let low = exec(
        &d,
        vec![MixerCmd::SetGain {
            strip: kick,
            gain_db: -2.0,
        }],
    );
    let lower = exec(
        &low,
        vec![MixerCmd::SetGain {
            strip: kick,
            gain_db: -6.0,
        }],
    );
    assert_eq!(
        describe(&low, &lower),
        vec![item("Strip Kick", "volume −2.0 dB → −6.0 dB")]
    );
    let silent = exec(
        &low,
        vec![MixerCmd::SetGain {
            strip: kick,
            gain_db: -144.0,
        }],
    );
    assert_eq!(
        describe(&low, &silent),
        vec![item("Strip Kick", "volume −2.0 dB → −∞ dB")]
    );
    let left = exec(
        &d,
        vec![MixerCmd::SetPan {
            strip: kick,
            pan: -0.3,
        }],
    );
    assert_eq!(
        describe(&d, &left),
        vec![item("Strip Kick", "pan centre → 30% left")]
    );
    let both = exec(
        &d,
        vec![
            MixerCmd::SetMute {
                strip: kick,
                mute: true,
            },
            MixerCmd::SetSolo {
                strip: kick,
                solo: true,
            },
        ],
    );
    assert_eq!(
        describe(&d, &both),
        vec![item("Strip Kick", "muted, soloed")]
    );
    assert_eq!(
        describe(&both, &d),
        vec![item("Strip Kick", "unmuted, unsoloed")]
    );
    // Co-Producer's own strip is its own.
    let drums = bus(&d, "Drums (Co-Producer)");
    let up = exec(
        &d,
        vec![MixerCmd::SetGain {
            strip: drums,
            gain_db: 1.5,
        }],
    );
    assert_eq!(
        describe(&d, &up),
        vec![ours("Strip Drums (Co-Producer)", "volume 0.0 dB → 1.5 dB")]
    );
}

#[test]
fn routing_is_an_item() {
    let d = mix();
    let (kick, drums) = (bus(&d, "Kick"), bus(&d, "Drums (Co-Producer)"));
    let routed = exec(
        &d,
        vec![MixerCmd::SetOutput {
            strip: kick,
            output: Output::Bus(drums),
        }],
    );
    let items = describe(&d, &routed);
    assert_eq!(
        items,
        vec![item("Strip Kick", "output Master → Drums (Co-Producer)")]
    );
    assert!(!items[0].co_producers);
}

#[test]
fn sends_are_items() {
    let d = mix();
    let (snare, drums) = (bus(&d, "Snare"), bus(&d, "Drums (Co-Producer)"));
    let sent = exec(
        &d,
        vec![MixerCmd::AddSend {
            strip: snare,
            send: AuxSend::new(drums),
        }],
    );
    assert_eq!(
        describe(&d, &sent),
        vec![item(
            "Strip Snare",
            "send to Drums (Co-Producer) added (0.0 dB, post-fader)"
        )]
    );
    let edit = |d: &Document, index: usize, f: &dyn Fn(&mut AuxSend)| {
        let mut send = d.mixer().strip(snare).unwrap().sends[index];
        f(&mut send);
        exec(
            d,
            vec![MixerCmd::EditSend {
                strip: snare,
                index,
                send,
            }],
        )
    };
    let quieter = edit(&sent, 0, &|s| s.level_db = -3.0);
    assert_eq!(
        describe(&sent, &quieter),
        vec![item(
            "Strip Snare",
            "send to Drums (Co-Producer) 0.0 dB → −3.0 dB"
        )]
    );
    let pre_off = edit(&sent, 0, &|s| {
        s.pre_fader = true;
        s.enabled = false;
    });
    assert_eq!(
        describe(&sent, &pre_off),
        vec![item(
            "Strip Snare",
            "send to Drums (Co-Producer) now pre-fader, off"
        )]
    );
    let removed = exec(
        &sent,
        vec![MixerCmd::RemoveSend {
            strip: snare,
            index: 0,
        }],
    );
    assert_eq!(
        describe(&sent, &removed),
        vec![item("Strip Snare", "send to Drums (Co-Producer) deleted")]
    );
    // Two sends to one bus: only the edited one is a part.
    let two = exec(
        &sent,
        vec![MixerCmd::AddSend {
            strip: snare,
            send: AuxSend::new(drums),
        }],
    );
    let second = edit(&two, 1, &|s| s.level_db = -6.0);
    assert_eq!(
        describe(&two, &second),
        vec![item(
            "Strip Snare",
            "send to Drums (Co-Producer) 0.0 dB → −6.0 dB"
        )]
    );
}

#[test]
fn inserts_are_items() {
    let d = mix();
    let kick = bus(&d, "Kick");
    let one = exec(&d, vec![add_insert(kick, "Test Comp")]);
    let at_two = exec(&one, vec![add_insert(kick, "Test Verb")]);
    let at_two = exec(
        &at_two,
        vec![MixerCmd::AddInsert {
            strip: kick,
            index: Some(1),
            plugin: plugin("Test Gain"),
        }],
    );
    let two = exec(&one, vec![add_insert(kick, "Test Verb")]);
    assert_eq!(
        describe(&two, &at_two),
        vec![item("Strip Kick", "insert Test Gain added at 2")]
    );
    // Three inserts: Test Comp, Test Gain, Test Verb.
    let three = at_two;
    let ids = inserts(&three, kick);
    let removed = exec(&three, vec![MixerCmd::RemoveInsert { slot: ids[0] }]);
    assert_eq!(
        describe(&three, &removed),
        vec![item("Strip Kick", "insert Test Comp deleted")]
    );
    let moved = exec(
        &three,
        vec![MixerCmd::MoveInsert {
            slot: ids[2],
            strip: kick,
            index: 0,
        }],
    );
    assert_eq!(
        describe(&three, &moved),
        vec![item("Strip Kick", "insert Test Verb moved 3 → 1")]
    );
    let bypassed = exec(
        &three,
        vec![MixerCmd::SetBypass {
            slot: ids[1],
            bypass: true,
        }],
    );
    assert_eq!(
        describe(&three, &bypassed),
        vec![item("Strip Kick", "insert Test Gain bypassed")]
    );
    assert_eq!(
        describe(&bypassed, &three),
        vec![item("Strip Kick", "insert Test Gain turned back on")]
    );
    let drums = bus(&d, "Drums (Co-Producer)");
    let keyed = exec(
        &three,
        vec![MixerCmd::SetSidechain {
            slot: ids[0],
            source: Some(drums),
        }],
    );
    let gone = exec(&keyed, vec![MixerCmd::RemoveBus { strip: drums }]);
    assert_eq!(
        describe(&keyed, &gone),
        vec![
            ours("Strip Drums (Co-Producer)", "deleted"),
            item(
                "Strip Kick",
                "insert Test Comp sidechain Drums (Co-Producer) → none"
            ),
        ]
    );
}

#[test]
fn eq_is_an_item() {
    let d = mix();
    let bass = bus(&d, "Bass");
    let eq = |d: &Document, name: &str| {
        exec(
            d,
            vec![MixerCmd::SetEq {
                strip: bass,
                plugin: Some(plugin(name)),
            }],
        )
    };
    let multi = eq(&d, "Multi-Q");
    assert_eq!(
        describe(&d, &multi),
        vec![item("Strip Bass", "EQ Multi-Q added")]
    );
    let a = eq(&d, "A");
    assert_eq!(
        describe(&a, &eq(&a, "B")),
        vec![item("Strip Bass", "EQ A → B")]
    );
    let emptied = exec(
        &a,
        vec![MixerCmd::SetEq {
            strip: bass,
            plugin: None,
        }],
    );
    assert_eq!(
        describe(&a, &emptied),
        vec![item("Strip Bass", "EQ A deleted")]
    );
}

#[test]
fn parameters_are_items() {
    let d = mix();
    let kick = bus(&d, "Kick");
    let d = exec(&d, vec![add_insert(kick, "Test Gain")]);
    let slot = inserts(&d, kick)[0];
    let set = |d: &Document, slot: PluginInstanceId, v: f64| {
        exec(
            d,
            vec![MixerCmd::SetPluginParams {
                slot,
                params: vec![(ParamId(3), v)],
            }],
        )
    };
    let lookup = |_: PluginInstanceId, p: ParamId| (p == ParamId(3)).then(|| ("Gain".into(), 0.5));
    let turned = set(&d, slot, 0.75);
    assert_eq!(
        describe_with(&d, &turned, &lookup),
        vec![item("Strip Kick", "Test Gain Gain 0.5 → 0.75")]
    );
    // An overlay value before is the old value.
    let first = set(&d, slot, 0.6);
    assert_eq!(
        describe_with(&first, &set(&first, slot, 0.75), &lookup),
        vec![item("Strip Kick", "Test Gain Gain 0.6 → 0.75")]
    );
    assert_eq!(
        describe(&first, &set(&first, slot, 0.75)),
        vec![item("Strip Kick", "Test Gain parameter 3 0.6 → 0.75")]
    );
    // Neither a name nor a value known.
    assert_eq!(
        describe(&d, &turned),
        vec![item("Strip Kick", "Test Gain parameter 3 set to 0.75")]
    );
    // An instrument's own slot joins its main strip's item.
    let synth = instrument_slot(&d);
    assert_eq!(instrument_strip(&d, synth), Some(bus(&d, "Synth")));
    assert_eq!(instrument_strip(&d, slot), None);
    let played = set(&d, synth, 0.25);
    assert_eq!(
        describe_with(&d, &played, &lookup),
        vec![item("Strip Synth", "Synth Gain 0.5 → 0.25")]
    );
    let louder = exec(
        &played,
        vec![MixerCmd::SetGain {
            strip: bus(&d, "Synth"),
            gain_db: 2.0,
        }],
    );
    assert_eq!(
        describe_with(&d, &louder, &lookup),
        vec![item(
            "Strip Synth",
            "volume 0.0 dB → 2.0 dB, Synth Gain 0.5 → 0.25"
        )]
    );
}

#[test]
fn removing_a_bus_shows_what_it_fed() {
    let d = mix();
    let (kick, snare, drums) = (
        bus(&d, "Kick"),
        bus(&d, "Snare"),
        bus(&d, "Drums (Co-Producer)"),
    );
    let fed = exec(
        &d,
        vec![
            MixerCmd::SetOutput {
                strip: kick,
                output: Output::Bus(drums),
            },
            MixerCmd::AddSend {
                strip: snare,
                send: AuxSend::new(drums),
            },
        ],
    );
    let gone = exec(&fed, vec![MixerCmd::RemoveBus { strip: drums }]);
    assert_eq!(
        describe(&fed, &gone),
        vec![
            ours("Strip Drums (Co-Producer)", "deleted"),
            item("Strip Kick", "output Drums (Co-Producer) → Master"),
            item("Strip Snare", "send to Drums (Co-Producer) deleted"),
        ]
    );
}

#[test]
fn long_items_are_capped() {
    let d = mix();
    let kick = bus(&d, "Kick");
    let d = exec(&d, vec![add_insert(kick, "Test Gain")]);
    let slot = inserts(&d, kick)[0];
    let many = exec(
        &d,
        vec![MixerCmd::SetPluginParams {
            slot,
            params: (0..20).map(|i| (ParamId(i), 0.5)).collect(),
        }],
    );
    let items = describe(&d, &many);
    assert_eq!(items.len(), 1);
    let parts: Vec<&str> = items[0].change.split(", ").collect();
    assert_eq!(parts.len(), MAX_PARTS + 1);
    assert_eq!(parts[0], "Test Gain parameter 0 set to 0.5");
    assert_eq!(parts[11], "Test Gain parameter 11 set to 0.5");
    assert_eq!(parts[12], "and 8 more changes");
}

#[test]
fn mixer_names_are_shown_safely() {
    let d = mix();
    let kick = bus(&d, "Kick");
    let with = exec(&d, vec![add_insert(kick, "a\u{202E}b")]);
    assert_eq!(
        describe(&d, &with),
        vec![item("Strip Kick", "insert a b added at 1")]
    );
    let slot = inserts(&with, kick)[0];
    let turned = exec(
        &with,
        vec![MixerCmd::SetPluginParams {
            slot,
            params: vec![(ParamId(1), 0.125)],
        }],
    );
    let hostile = |_: PluginInstanceId, _: ParamId| Some(("x\ny".to_string(), 1.0));
    assert_eq!(
        describe_with(&with, &turned, &hostile),
        vec![item("Strip Kick", "a b x y 1 → 0.125")]
    );
}

#[test]
fn an_instruments_own_slot_is_listed() {
    let d = mix();
    let synth = instrument_slot(&d);
    let off = exec(
        &d,
        vec![MixerCmd::SetBypass {
            slot: synth,
            bypass: true,
        }],
    );
    let items = describe(&d, &off);
    assert_eq!(
        items,
        vec![item("Strip Synth", "instrument Synth bypassed")]
    );
    // The composer's strip, so it needs a yes.
    assert!(!items[0].co_producers);
    assert_eq!(
        describe(&off, &d),
        vec![item("Strip Synth", "instrument Synth turned back on")]
    );
}

// ---- Plan 8e Ruling 4: automation in the preview ----

/// One 4/4 bar.
const BAR: i64 = 4 * core_model::time::PPQ;

/// A write of `n` points of `target` spread over bars 9–12 of
/// sequence 0 (none for a clear), over the whole of those bars.
fn automate(target: AutomationTarget, n: i64) -> Command {
    Command::WriteAutomation(AutomationWrite {
        seq: SeqId(0),
        target,
        start: Tick(8 * BAR),
        end: Tick(12 * BAR - 1),
        points: (0..n)
            .map(|i| (Tick(8 * BAR + i * (4 * BAR / n.max(1))), 0.5))
            .collect(),
        lane_name: "Automation".into(),
    })
}

/// The item about strip `name`.
fn strip_item(items: &[ChangeItem], name: &str) -> ChangeItem {
    let thing = format!("Strip {name}");
    items
        .iter()
        .find(|i| i.thing == thing)
        .unwrap_or_else(|| panic!("no {thing} in {items:?}"))
        .clone()
}

#[test]
fn automation_is_a_strip_part() {
    let d = mix();
    let kick = bus(&d, "Kick");
    let written = run(&d, automate(AutomationTarget::StripFader(kick), 12));
    assert_eq!(
        describe(&d, &written),
        vec![
            item("Track 3, Automation", "added"),
            item(
                "Strip Kick",
                "fader automation 0 points → 12 points, bars 9–12"
            ),
        ]
    );
    let cleared = run(&written, automate(AutomationTarget::StripFader(kick), 0));
    assert_eq!(
        describe(&written, &cleared),
        vec![item("Strip Kick", "fader automation cleared, bars 9–12")]
    );
    // Pan, mute and solo have their own labels.
    for (target, label) in [
        (AutomationTarget::StripPan(kick), "pan"),
        (AutomationTarget::StripMute(kick), "mute"),
        (AutomationTarget::StripSolo(kick), "solo"),
    ] {
        let after = run(&d, automate(target, 4));
        assert_eq!(
            strip_item(&describe(&d, &after), "Kick").change,
            format!("{label} automation 0 points → 4 points, bars 9–12")
        );
    }
    // A send names its bus.
    let drums = bus(&d, "Drums (Co-Producer)");
    let sent = exec(
        &d,
        vec![MixerCmd::AddSend {
            strip: kick,
            send: AuxSend::new(drums),
        }],
    );
    let send = run(
        &sent,
        automate(AutomationTarget::SendLevel(kick, SendIndex(0)), 12),
    );
    assert_eq!(
        strip_item(&describe(&sent, &send), "Kick"),
        item(
            "Strip Kick",
            "send to Drums (Co-Producer) automation 0 points → 12 points, bars 9–12"
        )
    );
    // A parameter names its slot and, from the lookup, itself.
    let lookup =
        |_: PluginInstanceId, p: ParamId| (p == ParamId(3)).then(|| ("Cutoff".into(), 0.5));
    let with = exec(&d, vec![add_insert(kick, "Test Gain")]);
    let slot = inserts(&with, kick)[0];
    let swept = run(
        &with,
        automate(AutomationTarget::PluginParam(slot, ParamId(3)), 12),
    );
    assert_eq!(
        strip_item(&describe_with(&with, &swept, &lookup), "Kick").change,
        "Test Gain Cutoff automation 0 points → 12 points, bars 9–12"
    );
    assert_eq!(
        strip_item(&describe(&with, &swept), "Kick").change,
        "Test Gain parameter 3 automation 0 points → 12 points, bars 9–12"
    );
    // An instrument slot's parameter joins its main strip's item.
    let synth = instrument_slot(&d);
    let played = run(
        &d,
        automate(AutomationTarget::PluginParam(synth, ParamId(3)), 12),
    );
    assert_eq!(
        strip_item(&describe_with(&d, &played, &lookup), "Synth"),
        item(
            "Strip Synth",
            "Synth Cutoff automation 0 points → 12 points, bars 9–12"
        )
    );
    // On Co-Producer's own bus the item is Co-Producer's.
    let own = run(&d, automate(AutomationTarget::StripFader(drums), 12));
    assert_eq!(
        strip_item(&describe(&d, &own), "Drums (Co-Producer)"),
        ours(
            "Strip Drums (Co-Producer)",
            "fader automation 0 points → 12 points, bars 9–12"
        )
    );
    // Rewriting part of a curve counts the points of the changed bars.
    let redone = run(
        &written,
        Command::WriteAutomation(AutomationWrite {
            seq: SeqId(0),
            target: AutomationTarget::StripFader(kick),
            start: Tick(10 * BAR),
            end: Tick(11 * BAR - 1),
            points: vec![(Tick(10 * BAR), 0.25)],
            lane_name: "Automation".into(),
        }),
    );
    assert_eq!(
        describe(&written, &redone),
        vec![item(
            "Strip Kick",
            "fader automation 3 points → 1 point, bars 11–11"
        )]
    );
}

#[test]
fn bars_follow_the_meter() {
    use core_model::meter::{KeySig, Meter, MeterEntry, MeterMap};
    let d = song();
    let q = core_model::time::PPQ;
    assert_eq!(bar_of(&d, SeqId(0), Tick(0)), 1);
    assert_eq!(bar_of(&d, SeqId(0), Tick(3 * q)), 1);
    assert_eq!(bar_of(&d, SeqId(0), Tick(4 * q)), 2);
    let waltz = run(
        &d,
        Command::Sequence(SequenceCmd::SetMeter {
            seq: SeqId(0),
            meter: MeterMap::new(vec![MeterEntry {
                tick: Tick(0),
                meter: Meter { num: 3, den: 4 },
                key: KeySig::default(),
            }])
            .unwrap(),
        }),
    );
    assert_eq!(bar_of(&waltz, SeqId(0), Tick(3 * 15_360)), 2);
    assert_eq!(bar_of(&waltz, SeqId(0), Tick(3 * 15_360 - 1)), 1);
    // Before bar 1 and in an unknown sequence it is bar 1.
    assert_eq!(bar_of(&waltz, SeqId(0), Tick(-q)), 1);
    assert_eq!(bar_of(&waltz, SeqId(9), Tick(10 * q)), 1);
}

#[test]
fn automation_parts_count_toward_the_cap() {
    let d = mix();
    let kick = bus(&d, "Kick");
    let d = exec(&d, vec![add_insert(kick, "Test Gain")]);
    let slot = inserts(&d, kick)[0];
    let mut h = History::new(d.clone());
    for i in 0..20 {
        h.execute(
            &automate(AutomationTarget::PluginParam(slot, ParamId(i)), 1),
            &ViewFilter::default(),
        )
        .unwrap();
    }
    let item = strip_item(&describe(&d, h.doc()), "Kick");
    // An automation part holds ", bars", so the parts are told apart by
    // how each starts.
    let parts: Vec<&str> = item.change.split(", Test Gain ").collect();
    assert_eq!(parts.len(), MAX_PARTS);
    assert_eq!(
        parts[0],
        "Test Gain parameter 0 automation 0 points → 1 point, bars 9–9"
    );
    assert_eq!(
        parts[11],
        "parameter 11 automation 0 points → 1 point, bars 9–9, and 8 more changes"
    );
}

/// Fix round 1: a write on Co-Producer's own strip that takes away
/// points from a track that is not Co-Producer's needs a yes; points
/// from Co-Producer's own track do not.
#[test]
fn automation_over_the_composers_points_needs_a_yes() {
    let d = mix();
    let drums = bus(&d, "Drums (Co-Producer)");
    let fader = AutomationTarget::StripFader(drums);
    let point_in = |d: &Document, lane: LaneId| {
        run(
            d,
            Command::Event(EventCmd::Insert {
                seq: SeqId(0),
                lane,
                events: vec![EventSpec::new(
                    Tick(9 * BAR),
                    EventBody::Automation {
                        target: fader,
                        value: 0.25,
                    },
                )],
            }),
        )
    };
    // A point drawn on the composer's Kick track.
    let drawn = point_in(&d, lane_id(&d, "Kick"));
    let over = run(&drawn, automate(fader, 12));
    let it = strip_item(&describe(&drawn, &over), "Drums (Co-Producer)");
    assert_eq!(it.change, "fader automation 1 point → 12 points, bars 9–12");
    assert!(!it.co_producers);
    // The same point on a track Co-Producer made.
    let made = acting(
        &d,
        Command::Lane(LaneCmd::Add {
            seq: SeqId(0),
            name: "Ideas".into(),
            index: None,
        }),
    );
    let ours_lane = made
        .sequence(SeqId(0))
        .unwrap()
        .lanes
        .iter()
        .find(|l| l.co_producer)
        .unwrap()
        .id;
    let drawn = point_in(&made, ours_lane);
    let over = run(&drawn, automate(fader, 12));
    let it = strip_item(&describe(&drawn, &over), "Drums (Co-Producer)");
    assert_eq!(it.change, "fader automation 1 point → 12 points, bars 9–12");
    assert!(it.co_producers);
}

/// The region on `lane` of sequence 0 (the first one).
fn region_on(d: &Document, lane: LaneId) -> core_model::ids::EventId {
    d.sequence(SeqId(0))
        .unwrap()
        .lanes
        .iter()
        .find(|l| l.id == lane)
        .unwrap()
        .active()
        .unwrap()
        .events
        .values()
        .find(|e| matches!(e.body, EventBody::AudioRegion { .. }))
        .unwrap()
        .id
}

fn fades_of(d: &Document, event: core_model::ids::EventId) -> (Fade, Fade) {
    match d.event(event).unwrap().0.body {
        EventBody::AudioRegion {
            fade_in, fade_out, ..
        } => (fade_in, fade_out),
        _ => panic!("not a region"),
    }
}

/// Plan 8e Task 6b: a region's fade shape is its track's item, in the
/// shapes' menu titles; a Co-Producer track's needs no yes.
#[test]
fn fade_shapes_are_track_items() {
    let d = regions(&song(), 1, 1);
    let event = region_on(&d, LaneId(1));
    let (fade_in, fade_out) = fades_of(&d, event);
    let shaped = |d: &Document, fin: FadeShape, fout: FadeShape| {
        run(
            d,
            Command::Audio(AudioCmd::SetFades {
                event,
                fade_in: Fade {
                    shape: fin,
                    ..fade_in
                },
                fade_out: Fade {
                    shape: fout,
                    ..fade_out
                },
            }),
        )
    };
    assert_eq!(
        describe(&d, &shaped(&d, FadeShape::SCurve, FadeShape::Linear)),
        vec![item("Track 2, Lead Vox", "fade in linear → s-curve")]
    );
    assert_eq!(
        describe(&d, &shaped(&d, FadeShape::Linear, FadeShape::EqualPower)),
        vec![item("Track 2, Lead Vox", "fade out linear → equal power")]
    );
    let both = shaped(&d, FadeShape::Exponential, FadeShape::SCurve);
    assert_eq!(
        describe(&d, &both),
        vec![item(
            "Track 2, Lead Vox",
            "fade in linear → exponential, fade out linear → s-curve"
        )]
    );
    // On a track Co-Producer made, the item is Co-Producer's.
    let made = acting(
        &d,
        Command::Lane(LaneCmd::Add {
            seq: SeqId(0),
            name: "Loop".into(),
            index: None,
        }),
    );
    let lane = lane_id(&made, "Loop (Co-Producer)");
    let made = regions(&made, lane.0, 1);
    let event = region_on(&made, lane);
    let (fade_in, fade_out) = fades_of(&made, event);
    let after = run(
        &made,
        Command::Audio(AudioCmd::SetFades {
            event,
            fade_in: Fade {
                shape: FadeShape::SCurve,
                ..fade_in
            },
            fade_out,
        }),
    );
    assert_eq!(
        describe(&made, &after),
        vec![ours(
            "Track 3, Loop (Co-Producer)",
            "fade in linear → s-curve"
        )]
    );
}

/// Plan 8e Task 6b: a track's output is its track's item, by the outputs'
/// names; none is "none".
#[test]
fn track_outputs_are_track_items() {
    let add = |d: &Document, name: &str| {
        run(
            d,
            Command::Routing(RoutingCmd::AddOutput {
                name: name.into(),
                reference: format!("hw:{name}"),
            }),
        )
    };
    let d = add(&add(&song(), "Main Out"), "Cue \u{202E}Out");
    let ids: Vec<_> = d.outputs().keys().copied().collect();
    let (main, cue) = (ids[ids.len() - 2], ids[ids.len() - 1]);
    let set = |d: &Document, lane: LaneId, output| {
        run(
            d,
            Command::Lane(LaneCmd::SetOutput {
                seq: SeqId(0),
                lane,
                output,
            }),
        )
    };
    let vox = lane_id(&d, "Lead Vox");
    let routed = set(&d, vox, Some(main));
    assert_eq!(
        describe(&d, &routed),
        vec![item("Track 2, Lead Vox", "output none → Main Out")]
    );
    assert_eq!(
        describe(&routed, &set(&routed, vox, Some(cue))),
        vec![item("Track 2, Lead Vox", "output Main Out → Cue  Out")]
    );
    assert_eq!(
        describe(&routed, &set(&routed, vox, None)),
        vec![item("Track 2, Lead Vox", "output Main Out → none")]
    );
    let made = acting(
        &d,
        Command::Lane(LaneCmd::Add {
            seq: SeqId(0),
            name: "Loop".into(),
            index: None,
        }),
    );
    let lane = lane_id(&made, "Loop (Co-Producer)");
    assert_eq!(
        describe(&made, &set(&made, lane, Some(main))),
        vec![ours(
            "Track 3, Loop (Co-Producer)",
            "output none → Main Out"
        )]
    );
}

/// Plan 8e Task 6b: the Built-in Synth's sound is one item, by its title.
#[test]
fn the_synth_sound_is_an_item() {
    use core_model::document::Patch;
    use core_model::mixer::BUILTIN_SYNTH_REF;
    let d = run(
        &song(),
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
    let dest = *d
        .instruments()
        .iter()
        .find(|(_, x)| x.kind.is_builtin_synth())
        .unwrap()
        .0;
    let sound = |d: &Document, program: u8, channel: u8| {
        run(
            d,
            Command::Routing(RoutingCmd::EditDestination {
                dest,
                kind: DestKind::MidiEndpoint {
                    endpoint_ref: BUILTIN_SYNTH_REF.into(),
                    group: 0,
                    channel,
                },
                default_patch: Some(Patch {
                    program,
                    bank: None,
                }),
            }),
        )
    };
    let pad = sound(&d, 89, 0);
    assert_eq!(
        describe(&d, &pad),
        vec![item("Built-in Synth", "sound Piano → Pad")]
    );
    for (program, channel, title) in [(33, 0, "Bass"), (80, 0, "Lead"), (0, 9, "Drums")] {
        assert_eq!(
            describe(&pad, &sound(&pad, program, channel)),
            vec![item("Built-in Synth", &format!("sound Pad → {title}"))]
        );
    }
    // Piano again, and a program no sound uses.
    assert_eq!(
        describe(&pad, &sound(&pad, 0, 0)),
        vec![item("Built-in Synth", "sound Pad → Piano")]
    );
    assert_eq!(
        describe(&pad, &sound(&pad, 5, 0)),
        vec![item("Built-in Synth", "sound Pad → another sound")]
    );
}

// ---- groups-vcas-transport Task 7: the group and VCA items. ----

fn group(d: &Document, c: core_model::command::GroupCmd) -> Document {
    run(d, Command::Groups(c))
}

fn group_id(d: &Document, name: &str) -> core_model::ids::GroupId {
    d.mixer()
        .groups()
        .values()
        .find(|g| g.name == name)
        .unwrap()
        .id
}

fn vca_id(d: &Document, name: &str) -> core_model::ids::VcaId {
    d.mixer()
        .vcas()
        .values()
        .find(|v| v.name == name)
        .unwrap()
        .id
}

#[test]
fn group_and_vca_items_read_plainly() {
    use core_model::automation::AutomationMode;
    use core_model::command::GroupCmd as G;
    let d = mix();
    let (kick, snare, bass) = (bus(&d, "Kick"), bus(&d, "Snare"), bus(&d, "Bass"));
    let drums = bus(&d, "Drums (Co-Producer)");

    // A group: each part, in its order.
    let g0 = group(
        &d,
        G::NewGroup {
            name: "Drums".into(),
            colour: Swatch::Red,
            members: vec![kick, snare],
        },
    );
    assert_eq!(
        describe(&d, &g0),
        vec![item("Group Drums", "added, Kick added, Snare added")]
    );
    let g = group_id(&g0, "Drums");
    assert_eq!(
        describe(&g0, &d),
        vec![item("Group Drums", "deleted")],
        "undoing it deletes it"
    );
    let g1 = group(
        &g0,
        G::RenameGroup {
            group: g,
            name: "Kit".into(),
        },
    );
    assert_eq!(
        describe(&g0, &g1),
        vec![item("Group Drums", "renamed Drums → Kit")]
    );
    let g2 = group(
        &g1,
        G::SetGroupColour {
            group: g,
            colour: Swatch::Blue,
        },
    );
    assert_eq!(
        describe(&g1, &g2),
        vec![item("Group Kit", "colour red → blue")]
    );
    let g3 = group(
        &g2,
        G::AddToGroup {
            group: g,
            strips: vec![bass],
        },
    );
    assert_eq!(describe(&g2, &g3), vec![item("Group Kit", "Bass added")]);
    let g4 = group(
        &g3,
        G::RemoveFromGroup {
            group: g,
            strips: vec![snare],
        },
    );
    assert_eq!(describe(&g3, &g4), vec![item("Group Kit", "Snare removed")]);
    let g5 = group(
        &g4,
        G::SetGroupLinks {
            group: g,
            mute: Some(false),
            solo: Some(false),
        },
    );
    assert_eq!(
        describe(&g4, &g5),
        vec![item("Group Kit", "mute link off, solo link off")]
    );
    assert_eq!(
        describe(&g5, &g4),
        vec![item("Group Kit", "mute link on, solo link on")]
    );
    let g6 = group(&g5, G::DeleteGroup { group: g });
    assert_eq!(describe(&g5, &g6), vec![item("Group Kit", "deleted")]);

    // The mark and the members decide (Ruling 10; owner, 2026-11-19): a
    // group Co-Producer made, empty or of its own strips only, is
    // Co-Producer's.
    let e = acting(
        &d,
        Command::Groups(G::NewGroup {
            name: "Empty".into(),
            colour: Swatch::Green,
            members: vec![],
        }),
    );
    assert_eq!(describe(&d, &e), vec![ours("Group Empty", "added")]);
    let own = acting(
        &d,
        Command::Groups(G::NewGroup {
            name: "Own".into(),
            colour: Swatch::Green,
            members: vec![drums],
        }),
    );
    assert_eq!(
        describe(&d, &own),
        vec![ours("Group Own", "added, Drums (Co-Producer) added")]
    );
    let o = group_id(&own, "Own");
    let renamed = group(
        &own,
        G::RenameGroup {
            group: o,
            name: "Mine".into(),
        },
    );
    assert_eq!(
        describe(&own, &renamed),
        vec![ours("Group Own", "renamed Own → Mine")]
    );
    // A composer's strip joining, or leaving, is theirs.
    let joined = group(
        &own,
        G::AddToGroup {
            group: o,
            strips: vec![kick],
        },
    );
    assert_eq!(
        describe(&own, &joined),
        vec![item("Group Own", "Kick added")]
    );
    assert_eq!(
        describe(&joined, &own),
        vec![item("Group Own", "Kick removed")]
    );

    // A VCA: each part, and what it does to its members.
    let v0 = group(
        &d,
        G::NewVca {
            name: "Kit".into(),
            colour: Swatch::Teal,
            members: vec![kick],
        },
    );
    assert_eq!(
        describe(&d, &v0),
        vec![item("VCA Kit", "added, Kick assigned")]
    );
    let v = vca_id(&v0, "Kit");
    let named = group(
        &d,
        G::NewVca {
            name: "VCA 1".into(),
            colour: Swatch::Teal,
            members: vec![kick],
        },
    );
    assert_eq!(
        describe(&d, &named),
        vec![item("VCA 1", "added, Kick assigned")]
    );
    let level = |from: &Document, db: f64| {
        group(
            from,
            G::SetVcaLevel {
                vca: v,
                gain_db: db,
            },
        )
    };
    let v1 = level(&v0, -2.0);
    assert_eq!(
        describe(&v0, &v1),
        vec![
            item("Strip Kick", "heard 0.0 dB → −2.0 dB"),
            item("VCA Kit", "level 0.0 dB → −2.0 dB"),
        ]
    );
    let v2 = level(&v1, -5.0);
    assert_eq!(
        describe(&v1, &v2),
        vec![
            item("Strip Kick", "heard −2.0 dB → −5.0 dB"),
            item("VCA Kit", "level −2.0 dB → −5.0 dB"),
        ]
    );
    // Its own fader moving says only that.
    let own_move = exec(
        &v2,
        vec![MixerCmd::SetGain {
            strip: kick,
            gain_db: -1.0,
        }],
    );
    assert_eq!(
        describe(&v2, &own_move),
        vec![item("Strip Kick", "volume 0.0 dB → −1.0 dB")]
    );
    let muted = group(&v2, G::SetVcaMute { vca: v, mute: true });
    assert_eq!(
        describe(&v2, &muted),
        vec![
            item("Strip Kick", "muted by a VCA"),
            item("VCA Kit", "muted"),
        ]
    );
    assert_eq!(
        describe(&muted, &v2),
        vec![
            item("Strip Kick", "no longer muted by a VCA"),
            item("VCA Kit", "unmuted"),
        ]
    );
    let soloed = group(&v2, G::SetVcaSolo { vca: v, solo: true });
    assert_eq!(describe(&v2, &soloed), vec![item("VCA Kit", "soloed")]);
    assert_eq!(describe(&soloed, &v2), vec![item("VCA Kit", "unsoloed")]);
    let touch = group(
        &v2,
        G::SetVcaMode {
            vca: v,
            mode: AutomationMode::Touch,
        },
    );
    assert_eq!(
        describe(&v2, &touch),
        vec![item("VCA Kit", "mode Read → Touch")]
    );
    let more = group(
        &v2,
        G::AssignVca {
            vca: v,
            strips: vec![snare],
        },
    );
    assert_eq!(
        describe(&v2, &more),
        vec![
            item("Strip Snare", "heard 0.0 dB → −5.0 dB"),
            item("VCA Kit", "Snare assigned"),
        ]
    );
    let fewer = group(
        &more,
        G::UnassignVca {
            vca: v,
            strips: vec![snare],
        },
    );
    assert_eq!(
        describe(&more, &fewer),
        vec![
            item("Strip Snare", "heard −5.0 dB → 0.0 dB"),
            item("VCA Kit", "Snare unassigned"),
        ]
    );
    let named = group(
        &v2,
        G::RenameVca {
            vca: v,
            name: "Drums".into(),
        },
    );
    let named = group(
        &named,
        G::SetVcaColour {
            vca: v,
            colour: Swatch::Pink,
        },
    );
    assert_eq!(
        describe(&v2, &named),
        vec![item("VCA Kit", "renamed Kit → Drums, colour teal → pink")]
    );
    let gone = group(&v2, G::DeleteVca { vca: v });
    assert_eq!(
        describe(&v2, &gone),
        vec![
            item("Strip Kick", "heard −5.0 dB → 0.0 dB"),
            item("VCA Kit", "deleted"),
        ]
    );
    let floor = level(&v2, -144.0);
    assert_eq!(
        describe(&v2, &floor),
        vec![
            item("Strip Kick", "heard −5.0 dB → −∞ dB"),
            item("VCA Kit", "level −5.0 dB → −∞ dB"),
        ]
    );

    // A VCA Co-Producer made, of its own strips only, is Co-Producer's.
    let mine = acting(
        &d,
        Command::Groups(G::NewVca {
            name: "Mine".into(),
            colour: Swatch::Teal,
            members: vec![drums],
        }),
    );
    let m = vca_id(&mine, "Mine");
    let down = group(
        &mine,
        G::SetVcaLevel {
            vca: m,
            gain_db: -3.0,
        },
    );
    assert_eq!(
        describe(&mine, &down),
        vec![
            ours("Strip Drums (Co-Producer)", "heard 0.0 dB → −3.0 dB"),
            ours("VCA Mine", "level 0.0 dB → −3.0 dB"),
        ]
    );
}

/// Carry (b): a VCA's automation is its item's part, and deleting the VCA
/// says its automation no longer plays; whose it is follows the members.
#[test]
fn vca_automation_is_a_vca_part() {
    use core_model::command::GroupCmd as G;
    let d = mix();
    let (kick, drums) = (bus(&d, "Kick"), bus(&d, "Drums (Co-Producer)"));
    let with = group(
        &d,
        G::NewVca {
            name: "Kit".into(),
            colour: Swatch::Teal,
            members: vec![kick],
        },
    );
    let v = vca_id(&with, "Kit");
    let written = run(&with, automate(AutomationTarget::VcaFader(v), 12));
    assert_eq!(
        describe(&with, &written),
        vec![
            item("Track 3, Automation", "added"),
            item(
                "VCA Kit",
                "fader automation 0 points → 12 points, bars 9–12"
            ),
        ]
    );
    let gone = group(&written, G::DeleteVca { vca: v });
    assert_eq!(
        describe(&written, &gone),
        vec![item(
            "VCA Kit",
            "deleted, fader automation no longer plays (12 points, bars 9–12)"
        )]
    );
    // Co-Producer's own VCA with Co-Producer's own automation track is
    // Co-Producer's; on a composer's track, or with a composer's member,
    // it is not.
    let mine = acting(
        &d,
        Command::Groups(G::NewVca {
            name: "Mine".into(),
            colour: Swatch::Teal,
            members: vec![drums],
        }),
    );
    let m = vca_id(&mine, "Mine");
    let ours_written = acting(&mine, automate(AutomationTarget::VcaFader(m), 4));
    let deleted = group(&ours_written, G::DeleteVca { vca: m });
    assert_eq!(
        describe(&ours_written, &deleted),
        vec![ours(
            "VCA Mine",
            "deleted, fader automation no longer plays (4 points, bars 9–12)"
        )]
    );
    let theirs_written = run(&mine, automate(AutomationTarget::VcaFader(m), 4));
    let deleted = group(&theirs_written, G::DeleteVca { vca: m });
    assert_eq!(
        describe(&theirs_written, &deleted),
        vec![item(
            "VCA Mine",
            "deleted, fader automation no longer plays (4 points, bars 9–12)"
        )]
    );
}

/// Owner, 2026-11-19: a group or VCA is Co-Producer's only when it made
/// it (its mark) and every member, before and after, is its own. So the
/// composer's group or VCA asks, even empty.
#[test]
fn the_composers_empty_group_or_vca_asks() {
    use core_model::automation::AutomationMode;
    use core_model::command::GroupCmd as G;
    let d = mix();
    let drums = bus(&d, "Drums (Co-Producer)");
    let new_group = || G::NewGroup {
        name: "Empty".into(),
        colour: Swatch::Green,
        members: vec![],
    };
    let new_vca = || G::NewVca {
        name: "Ride".into(),
        colour: Swatch::Teal,
        members: vec![],
    };

    // The composer's empty group: every change is theirs.
    let e = group(&d, new_group());
    assert_eq!(describe(&d, &e), vec![item("Group Empty", "added")]);
    let g = group_id(&e, "Empty");
    for (c, change) in [
        (
            G::RenameGroup {
                group: g,
                name: "Kit".into(),
            },
            "renamed Empty → Kit",
        ),
        (
            G::SetGroupColour {
                group: g,
                colour: Swatch::Blue,
            },
            "colour green → blue",
        ),
        (
            G::SetGroupLinks {
                group: g,
                mute: Some(false),
                solo: None,
            },
            "mute link off",
        ),
        (G::DeleteGroup { group: g }, "deleted"),
        (
            G::AddToGroup {
                group: g,
                strips: vec![drums],
            },
            "Drums (Co-Producer) added",
        ),
    ] {
        let after = acting(&e, Command::Groups(c));
        assert_eq!(
            describe(&e, &after),
            vec![item("Group Empty", change)],
            "{change}"
        );
    }

    // The composer's empty VCA: the same.
    let e = group(&d, new_vca());
    assert_eq!(describe(&d, &e), vec![item("VCA Ride", "added")]);
    let v = vca_id(&e, "Ride");
    for (c, change) in [
        (
            G::RenameVca {
                vca: v,
                name: "Kit".into(),
            },
            "renamed Ride → Kit",
        ),
        (
            G::SetVcaColour {
                vca: v,
                colour: Swatch::Pink,
            },
            "colour teal → pink",
        ),
        (
            G::SetVcaLevel {
                vca: v,
                gain_db: -3.0,
            },
            "level 0.0 dB → −3.0 dB",
        ),
        (G::SetVcaMute { vca: v, mute: true }, "muted"),
        (
            G::SetVcaMode {
                vca: v,
                mode: AutomationMode::Touch,
            },
            "mode Read → Touch",
        ),
        (G::DeleteVca { vca: v }, "deleted"),
    ] {
        let after = acting(&e, Command::Groups(c));
        assert_eq!(
            describe(&e, &after),
            vec![item("VCA Ride", change)],
            "{change}"
        );
    }
    let filled = acting(
        &e,
        Command::Groups(G::AssignVca {
            vca: v,
            strips: vec![drums],
        }),
    );
    assert!(
        describe(&e, &filled).contains(&item("VCA Ride", "Drums (Co-Producer) assigned")),
        "{:?}",
        describe(&e, &filled)
    );

    // Co-Producer's own: made by it, with only its own strips, it is free.
    let own = acting(
        &d,
        Command::Groups(G::NewGroup {
            name: "Own".into(),
            colour: Swatch::Green,
            members: vec![drums],
        }),
    );
    assert_eq!(
        describe(&d, &own),
        vec![ours("Group Own", "added, Drums (Co-Producer) added")]
    );
    let o = group_id(&own, "Own");
    let renamed = acting(
        &own,
        Command::Groups(G::RenameGroup {
            group: o,
            name: "Mine".into(),
        }),
    );
    assert_eq!(
        describe(&own, &renamed),
        vec![ours("Group Own", "renamed Own → Mine")]
    );
    let gone = acting(&own, Command::Groups(G::DeleteGroup { group: o }));
    assert_eq!(describe(&own, &gone), vec![ours("Group Own", "deleted")]);
    let empty = acting(&d, Command::Groups(new_group()));
    assert_eq!(describe(&d, &empty), vec![ours("Group Empty", "added")]);
    let mine = acting(
        &d,
        Command::Groups(G::NewVca {
            name: "Mine".into(),
            colour: Swatch::Teal,
            members: vec![drums],
        }),
    );
    let m = vca_id(&mine, "Mine");
    let down = acting(
        &mine,
        Command::Groups(G::SetVcaLevel {
            vca: m,
            gain_db: -3.0,
        }),
    );
    assert_eq!(
        describe(&mine, &down),
        vec![
            ours("Strip Drums (Co-Producer)", "heard 0.0 dB → −3.0 dB"),
            ours("VCA Mine", "level 0.0 dB → −3.0 dB"),
        ]
    );

    // A composer's strip joining Co-Producer's group still asks.
    let kick = bus(&d, "Kick");
    let joined = acting(
        &own,
        Command::Groups(G::AddToGroup {
            group: o,
            strips: vec![kick],
        }),
    );
    assert_eq!(
        describe(&own, &joined),
        vec![item("Group Own", "Kick added")]
    );
}

/// Plan 8b Ruling 1, for groups and VCAs too: a hand rename of
/// Co-Producer's group or VCA takes it over, so Co-Producer then asks.
/// Its other group, not renamed, stays its own.
#[test]
fn a_hand_rename_takes_a_group_or_vca_over() {
    use core_model::command::GroupCmd as G;
    let d = mix();
    let drums = bus(&d, "Drums (Co-Producer)");
    let mut h = History::new(d.clone());
    h.begin_request("help").unwrap();
    h.set_acting(true).unwrap();
    for c in [
        G::NewGroup {
            name: "Drums".into(),
            colour: Swatch::Green,
            members: vec![drums],
        },
        G::NewGroup {
            name: "Other".into(),
            colour: Swatch::Green,
            members: vec![],
        },
        G::NewVca {
            name: "Ride".into(),
            colour: Swatch::Teal,
            members: vec![drums],
        },
    ] {
        h.execute(&Command::Groups(c), &ViewFilter::default())
            .unwrap();
    }
    let (g, other) = (group_id(h.doc(), "Drums"), group_id(h.doc(), "Other"));
    let v = vca_id(h.doc(), "Ride");
    h.set_acting(false).unwrap();
    for c in [
        G::RenameGroup {
            group: g,
            name: "Kit".into(),
        },
        G::RenameVca {
            vca: v,
            name: "Fader".into(),
        },
    ] {
        h.execute(&Command::Groups(c), &ViewFilter::default())
            .unwrap();
    }
    let taken = h.doc().clone();
    let m = taken.mixer();
    assert!(!m.groups()[&g].co_producer);
    assert!(!m.vcas()[&v].co_producer);
    assert!(m.groups()[&other].co_producer);

    let renamed = acting(
        &taken,
        Command::Groups(G::RenameGroup {
            group: g,
            name: "Drums".into(),
        }),
    );
    assert_eq!(
        describe(&taken, &renamed),
        vec![item("Group Kit", "renamed Kit → Drums")]
    );
    let renamed = acting(
        &taken,
        Command::Groups(G::RenameVca {
            vca: v,
            name: "Ride".into(),
        }),
    );
    assert_eq!(
        describe(&taken, &renamed),
        vec![item("VCA Fader", "renamed Fader → Ride")]
    );
    let renamed = acting(
        &taken,
        Command::Groups(G::RenameGroup {
            group: other,
            name: "Mine".into(),
        }),
    );
    assert_eq!(
        describe(&taken, &renamed),
        vec![ours("Group Other", "renamed Other → Mine")]
    );
}
