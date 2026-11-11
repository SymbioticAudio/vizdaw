//! Format version 8 (plan 6a Task 1): song info, region names and the
//! project ID.

use core_model::ModelError;
use core_model::command::{AudioCmd, Command, MediaCmd, SequenceCmd, SettingsCmd, apply};
use core_model::document::{Document, MAX_INFO_BYTES, SongInfo};
use core_model::event::{EventBody, Fade};
use core_model::file::to_json;
use core_model::history::History;
use core_model::ids::{EventId, LaneId, MediaId, SeqId};
use core_model::selection::ViewFilter;
use core_model::time::Tick;
use serde_json::Value;

fn run(d: &Document, c: Command) -> core_model::Result<Document> {
    apply(d, &c).map(|a| a.doc)
}

fn set_info(info: SongInfo) -> Command {
    Command::Settings(SettingsCmd::SetInfo { info })
}

fn titled(s: &str) -> SongInfo {
    SongInfo {
        title: Some(s.into()),
        ..SongInfo::default()
    }
}

/// Sequence 0 with lane 0 and one audio region (event 0).
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
    run(
        &d,
        Command::Audio(AudioCmd::Insert {
            seq: SeqId(0),
            lane: LaneId(0),
            tick: Tick(0),
            media: MediaId(0),
            source_offset: 0,
            length: 96_000,
            gain_db: 0.0,
            fade_in: Fade::default(),
            fade_out: Fade::default(),
        }),
    )
    .unwrap()
}

fn region_name(d: &Document, id: EventId) -> Option<String> {
    match &d.event(id).unwrap().0.body {
        EventBody::AudioRegion { name, .. } => name.clone(),
        other => panic!("not a region: {other:?}"),
    }
}

fn set_name(event: EventId, name: Option<&str>) -> Command {
    Command::Audio(AudioCmd::SetName {
        event,
        name: name.map(Into::into),
    })
}

fn out_of_range(r: core_model::Result<Document>, want: &str) {
    match r {
        Err(ModelError::OutOfRange { field, .. }) => assert_eq!(field, want),
        other => panic!("expected OutOfRange {want}, got {:?}", other.map(|_| ())),
    }
}

#[test]
fn set_info_round_trips_and_undoes() {
    let info = SongInfo {
        title: Some("Song".into()),
        composer: Some("A. Writer".into()),
        comment: Some("line 1\nline 2".into()),
        ..SongInfo::default()
    };
    let cmd = set_info(info.clone());
    let Command::Settings(s) = &cmd else {
        unreachable!()
    };
    assert_eq!(s.name(), "Set Song Info");
    let mut h = History::new(Document::new());
    h.execute(&cmd, &ViewFilter::default()).unwrap();
    assert_eq!(h.doc().settings().info, info);
    h.undo().unwrap();
    assert_eq!(h.doc().settings().info, SongInfo::default());
}

#[test]
fn set_info_checks_each_field() {
    let d = Document::new();
    out_of_range(
        run(&d, set_info(titled(&"x".repeat(MAX_INFO_BYTES + 1)))),
        "info.title",
    );
    let ok = run(&d, set_info(titled(&"x".repeat(MAX_INFO_BYTES)))).unwrap();
    assert_eq!(
        ok.settings().info.title.as_deref().map(str::len),
        Some(MAX_INFO_BYTES)
    );
    out_of_range(run(&d, set_info(titled("a\u{0007}b"))), "info.title");
    let bell = SongInfo {
        website: Some("\u{0007}".into()),
        ..SongInfo::default()
    };
    out_of_range(run(&d, set_info(bell)), "info.website");
    let ws = SongInfo {
        comment: Some("a\n\tb".into()),
        ..SongInfo::default()
    };
    assert!(run(&d, set_info(ws)).is_ok());
    for empty in ["", "  "] {
        let stored = run(&d, set_info(titled(empty))).unwrap();
        assert_eq!(stored.settings().info.title, None);
    }
    // Text is trimmed.
    let t = run(&d, set_info(titled("  Song  "))).unwrap();
    assert_eq!(t.settings().info.title.as_deref(), Some("Song"));
    // A refusal leaves the document equal (apply is pure).
    let with = run(&d, set_info(titled("Keep"))).unwrap();
    assert!(run(&with, set_info(titled("a\u{0007}"))).is_err());
    assert_eq!(with.settings().info.title.as_deref(), Some("Keep"));
}

#[test]
fn region_name_set_rename_and_split() {
    let d = doc();
    let named = run(&d, set_name(EventId(0), Some(" Lead vox "))).unwrap();
    assert_eq!(region_name(&named, EventId(0)).as_deref(), Some("Lead vox"));
    let cleared = run(&named, set_name(EventId(0), Some(""))).unwrap();
    assert_eq!(region_name(&cleared, EventId(0)), None);
    let name = Command::Audio(AudioCmd::SetName {
        event: EventId(0),
        name: Some("\u{200B}".into()),
    });
    out_of_range(run(&d, name), "region.name");
    out_of_range(
        run(
            &d,
            set_name(EventId(0), Some(&"x".repeat(MAX_INFO_BYTES + 1))),
        ),
        "region.name",
    );
    // A non-region is refused.
    let with_note = run(
        &d,
        Command::Event(core_model::command::EventCmd::Insert {
            seq: SeqId(0),
            lane: LaneId(0),
            events: vec![core_model::event::EventSpec::new(
                Tick(0),
                EventBody::note(60, 1, Tick(10)),
            )],
        }),
    )
    .unwrap();
    let note_id = with_note
        .sequence(SeqId(0))
        .unwrap()
        .lanes
        .iter()
        .flat_map(|l| l.takes.iter())
        .flat_map(|t| t.events.values())
        .find(|e| matches!(e.body, EventBody::Note { .. }))
        .unwrap()
        .id;
    assert!(run(&with_note, set_name(note_id, Some("x"))).is_err());

    // Split gives both parts the name; undo restores.
    let mut h = History::new(named);
    h.execute(
        &Command::Audio(AudioCmd::Split {
            event: EventId(0),
            at: Tick(1920),
        }),
        &ViewFilter::default(),
    )
    .unwrap();
    let regions: Vec<_> = h
        .doc()
        .sequence(SeqId(0))
        .unwrap()
        .lanes
        .iter()
        .flat_map(|l| l.takes.iter())
        .flat_map(|t| t.events.values())
        .filter_map(|e| match &e.body {
            EventBody::AudioRegion { name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(regions, vec![Some("Lead vox".into()); 2]);
    h.undo().unwrap();
    assert_eq!(
        region_name(h.doc(), EventId(0)).as_deref(),
        Some("Lead vox")
    );
}

#[test]
fn project_id_is_stamped_through_history() {
    const ID: &str = "0a1b2c3d-0000-4000-8000-00000000000a";
    let mut h = History::new(Document::new());
    for t in ["One", "Two"] {
        h.execute(&set_info(titled(t)), &ViewFilter::default())
            .unwrap();
    }
    h.stamp_project_id(ID).unwrap();
    assert_eq!(h.doc().settings().project_id.as_deref(), Some(ID));
    h.undo().unwrap();
    h.undo().unwrap();
    assert_eq!(h.doc().settings().project_id.as_deref(), Some(ID));
    h.redo().unwrap();
    assert_eq!(h.doc().settings().project_id.as_deref(), Some(ID));
    h.stamp_project_id("11111111-1111-4111-8111-111111111111")
        .unwrap();
    assert_eq!(h.doc().settings().project_id.as_deref(), Some(ID));
    h.begin("T").unwrap();
    assert!(h.stamp_project_id(ID).is_err());
}

#[test]
fn an_empty_song_info_and_unnamed_region_write_nothing() {
    let text = to_json(&doc()).unwrap();
    let v: Value = serde_json::from_str(&text).unwrap();
    assert!(v["document"]["settings"].get("info").is_none());
    assert!(v["document"]["settings"].get("project_id").is_none());
    assert!(!text.contains("\"info\""));
    assert!(!text.contains("\"project_id\""));
    // No region body carries a name key.
    fn regions<'a>(v: &'a Value, out: &mut Vec<&'a Value>) {
        match v {
            Value::Object(m) => {
                if m.get("type").and_then(Value::as_str) == Some("audio_region") {
                    out.push(v);
                }
                m.values().for_each(|c| regions(c, out));
            }
            Value::Array(a) => a.iter().for_each(|c| regions(c, out)),
            _ => {}
        }
    }
    let mut found = Vec::new();
    regions(&v, &mut found);
    assert_eq!(found.len(), 1, "one region is written");
    assert!(found[0].get("name").is_none());
}
