//! What a change does, in plain items (plan 8a Task 4, Ruling 6): the
//! list the composer says yes to.

use core_model::command::{
    AudioCmd, Command, EventCmd, LaneCmd, MediaCmd, MixerCmd, RoutingCmd, SequenceCmd, apply,
};
use core_model::diff::{ChangeItem, MAX_PARTS, describe, describe_with, instrument_strip};
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
    assert_eq!(
        describe(&both, &gone),
        vec![item("Track 1, Kick", "deleted, with 2 regions and 1 note")]
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
