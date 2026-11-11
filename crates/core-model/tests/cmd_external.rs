//! `AudioCmd::SetSource` (plan 5f-3 Task 1, Ruling 1; spec §11): one
//! region points at new media, its fades clamped, as one undoable command.

use core_model::ModelError;
use core_model::command::{AudioCmd, Command, EventCmd, MediaCmd, SequenceCmd, apply};
use core_model::document::Document;
use core_model::event::{EventBody, EventSpec, Fade, FadeShape};
use core_model::history::History;
use core_model::ids::{EventId, LaneId, MediaId, SeqId};
use core_model::selection::ViewFilter;
use core_model::time::Tick;

fn run(d: &Document, c: Command) -> core_model::Result<Document> {
    apply(d, &c).map(|a| a.doc)
}

fn fade(n: u64) -> Fade {
    Fade {
        length_frames: n,
        shape: FadeShape::Linear,
    }
}

fn add_media(d: &Document, path: &str, frames: u64) -> Document {
    run(
        d,
        Command::Media(MediaCmd::Add {
            path: path.into(),
            sample_rate: 48_000,
            channels: 2,
            length_frames: frames,
            content_hash: None,
            name: None,
            origin: None,
        }),
    )
    .unwrap()
}

fn region_at(d: &Document, tick: i64) -> Document {
    run(
        d,
        Command::Audio(AudioCmd::Insert {
            seq: SeqId(0),
            lane: LaneId(0),
            tick: Tick(tick),
            media: MediaId(0),
            source_offset: 1000,
            length: 4000,
            gain_db: -6.0,
            fade_in: fade(800),
            fade_out: fade(600),
        }),
    )
    .unwrap()
}

/// Sequence 0 with lane 0; media 0 (`Media/a.caf`, 48 000 frames) and 1
/// (`Media/b.caf`, 2000 frames); regions 0 (tick 0) and 1 (tick 96 000)
/// on media 0 (offset 1000, 4000 frames, −6 dB, fades 800 in and 600
/// out); a note (event 2).
fn doc() -> Document {
    let d = run(
        &Document::new(),
        Command::Sequence(SequenceCmd::New { name: "S".into() }),
    )
    .unwrap();
    let d = add_media(&d, "Media/a.caf", 48_000);
    let d = add_media(&d, "Media/b.caf", 2_000);
    let d = region_at(&d, 0);
    let d = region_at(&d, 96_000);
    run(
        &d,
        Command::Event(EventCmd::Insert {
            seq: SeqId(0),
            lane: LaneId(0),
            events: vec![EventSpec::new(
                Tick(0),
                EventBody::note(60, 0x8000, Tick(100)),
            )],
        }),
    )
    .unwrap()
}

fn set_source(event: u64, media: u64, source_offset: u64, length: u64) -> Command {
    Command::Audio(AudioCmd::SetSource {
        event: EventId(event),
        media: MediaId(media),
        source_offset,
        length,
    })
}

/// `(media, source_offset, length, fade in, fade out, tick, gain)` of
/// region `e`.
fn body(d: &Document, e: u64) -> (MediaId, u64, u64, u64, u64, Tick, f32) {
    let (ev, _) = d.event(EventId(e)).unwrap();
    match &ev.body {
        EventBody::AudioRegion {
            media,
            source_offset,
            length,
            gain_db,
            fade_in,
            fade_out,
            ..
        } => (
            *media,
            *source_offset,
            *length,
            fade_in.length_frames,
            fade_out.length_frames,
            ev.tick,
            *gain_db,
        ),
        _ => panic!("event {e} is not a region"),
    }
}

#[test]
fn set_source_relinks_one_region_only() {
    let d = doc();
    let c = set_source(0, 1, 0, 1000);
    assert_eq!(c.name(), "Set Region Source");
    let after = run(&d, c).unwrap();
    let (media, offset, length, _, _, tick, gain) = body(&after, 0);
    assert_eq!(
        (media, offset, length, tick, gain),
        (MediaId(1), 0, 1000, Tick(0), -6.0)
    );
    assert_eq!(
        body(&after, 1),
        body(&d, 1),
        "the other region still plays media 0"
    );
    assert_eq!(after.media().len(), 2, "no media is added or removed");
}

#[test]
fn fades_are_clamped_to_the_new_length() {
    let d = doc();
    // 800 in and 600 out: 1000 frames keep the fade in and leave 200.
    let a = run(&d, set_source(0, 1, 0, 1000)).unwrap();
    assert_eq!((body(&a, 0).3, body(&a, 0).4), (800, 200));
    // 500 frames: the fade in takes them all.
    let b = run(&d, set_source(0, 1, 500, 500)).unwrap();
    assert_eq!((body(&b, 0).3, body(&b, 0).4), (500, 0));
    // Long enough for both: they stay.
    let c = run(&d, set_source(0, 1, 0, 2000)).unwrap();
    assert_eq!((body(&c, 0).3, body(&c, 0).4), (800, 600));
}

#[test]
fn set_source_refuses_bad_spans() {
    let d = doc();
    for (e, m, offset, length) in [
        (0, 1, 1500, 501),   // past media 1's end
        (0, 1, 0, 0),        // no frames
        (0, 1, u64::MAX, 2), // an offset that overflows
        (0, 7, 0, 10),       // an unknown media item
        (2, 1, 0, 10),       // a note
        (99, 1, 0, 10),      // an unknown event
    ] {
        let c = set_source(e, m, offset, length);
        assert!(run(&d, c.clone()).is_err(), "{e} {m} {offset} {length}");
        let mut h = History::new(d.clone());
        assert!(h.execute(&c, &ViewFilter::default()).is_err());
        assert_eq!(h.doc(), &d, "the document is unchanged");
    }
    assert!(matches!(
        run(&d, set_source(0, 1, 0, 0)),
        Err(ModelError::OutOfRange { .. })
    ));
    assert!(matches!(
        run(&d, set_source(0, 7, 0, 10)),
        Err(ModelError::UnknownId(_))
    ));
    assert!(matches!(
        run(&d, set_source(99, 1, 0, 10)),
        Err(ModelError::UnknownId(_))
    ));
}

#[test]
fn undo_of_set_source_restores_the_span() {
    let d = doc();
    let mut h = History::new(d.clone());
    h.execute(&set_source(0, 1, 0, 1000), &ViewFilter::default())
        .unwrap();
    assert_eq!(h.undo_label(), Some("Set Region Source"));
    let edited = h.doc().clone();
    h.undo().unwrap();
    assert_eq!(h.doc(), &d, "the exact old body");
    h.redo().unwrap();
    assert_eq!(h.doc(), &edited);
}
