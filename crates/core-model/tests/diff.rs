//! What a change does, in plain items (plan 8a Task 4, Ruling 6): the
//! list the composer says yes to.

use core_model::command::{
    AudioCmd, Command, EventCmd, LaneCmd, MediaCmd, MixerCmd, SequenceCmd, apply,
};
use core_model::diff::{ChangeItem, describe};
use core_model::document::Document;
use core_model::event::{EventBody, EventSpec, Fade, FadeShape};
use core_model::file::{from_json, to_json};
use core_model::ids::{LaneId, MediaId, SeqId, StripId};
use core_model::tempo::{Bpm, TempoMap};
use core_model::time::Tick;

fn run(d: &Document, c: Command) -> Document {
    apply(d, &c).unwrap().doc
}

fn item(thing: &str, change: &str) -> ChangeItem {
    ChangeItem {
        thing: thing.into(),
        change: change.into(),
    }
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
