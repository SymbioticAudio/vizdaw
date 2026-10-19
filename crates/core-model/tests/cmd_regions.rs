//! Region commands for the editors (plan 5b Task 1, Rulings 5, 6, 9):
//! Normalize from a measured peak, fades to the cursor, Remove Fades, and
//! Relink rescaling regions when the new file's rate differs.

use core_model::ModelError;
use core_model::command::{AudioCmd, Command, EventCmd, FadeEdge, MediaCmd, SequenceCmd, apply};
use core_model::document::Document;
use core_model::event::{EventBody, EventSpec, Fade, FadeShape};
use core_model::history::History;
use core_model::ids::{EventId, LaneId, MediaId, SeqId};
use core_model::selection::ViewFilter;
use core_model::time::Tick;

fn run(d: &Document, c: Command) -> core_model::Result<Document> {
    apply(d, &c).map(|a| a.doc)
}

fn fade(n: u64, shape: FadeShape) -> Fade {
    Fade {
        length_frames: n,
        shape,
    }
}

/// Sequence 0 with lane 0; media 0 (`Media/a.caf`, 48 kHz, 192 000
/// frames); region 0 at tick 0 (offset 48 000, 96 000 frames, a 4800-frame
/// linear fade in and a 2400-frame equal-power fade out); a note (event 1)
/// at tick 0.
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
    d = run(
        &d,
        Command::Audio(AudioCmd::Insert {
            seq: SeqId(0),
            lane: LaneId(0),
            tick: Tick(0),
            media: MediaId(0),
            source_offset: 48_000,
            length: 96_000,
            gain_db: 0.0,
            fade_in: fade(4800, FadeShape::Linear),
            fade_out: fade(2400, FadeShape::EqualPower),
        }),
    )
    .unwrap();
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

/// `(gain_db, fade_in, fade_out, source_offset, length)` of region 0.
fn region(d: &Document) -> (f32, Fade, Fade, u64, u64) {
    match d.event(EventId(0)).unwrap().0.body {
        EventBody::AudioRegion {
            gain_db,
            fade_in,
            fade_out,
            source_offset,
            length,
            ..
        } => (gain_db, fade_in, fade_out, source_offset, length),
        _ => panic!("event 0 is not a region"),
    }
}

/// The tick `seconds` into sequence 0, and the frame (48 kHz) it maps to.
fn at(d: &Document, seconds: f64) -> (Tick, u64) {
    let tempo = &d.sequence(SeqId(0)).unwrap().tempo;
    let t = tempo.seconds_to_tick(seconds);
    (t, (tempo.tick_to_seconds(t) * 48_000.0).round() as u64)
}

fn normalize(regions: Vec<(u64, f32)>, peak_db: f32) -> Command {
    Command::Audio(AudioCmd::Normalize {
        regions: regions.into_iter().map(|(e, p)| (EventId(e), p)).collect(),
        peak_db,
    })
}

#[test]
fn normalize_sets_gain_from_the_peak_and_is_one_undo_step() {
    let d = doc();
    let cmd = normalize(vec![(0, 0.25), (1, 0.5)], -1.0);
    assert_eq!(cmd.name(), "Normalize");
    let mut h = History::new(d.clone());
    h.execute(&cmd, &ViewFilter::default()).unwrap();
    // −1 − 20·log10(0.25) = 11.0412 dB; the note (event 1) is ignored.
    let (g, ..) = region(h.doc());
    assert!((g - 11.0412).abs() < 1e-3, "gain {g}");
    assert_eq!(h.undo_label(), Some("Normalize"));
    h.undo().unwrap();
    assert_eq!(h.doc(), &d);
}

#[test]
fn normalize_clamps_skips_silence_and_checks_its_target() {
    let d = doc();
    // A near-silent region would need +179 dB: limited to +24, with a note.
    let a = apply(&d, &normalize(vec![(0, 1e-9)], -1.0)).unwrap();
    assert_eq!(region(&a.doc).0, 24.0);
    assert!(
        a.report.warnings.iter().any(|w| w.contains("limited")),
        "{:?}",
        a.report.warnings
    );
    // Only silent (or non-finite) peaks: nothing to do.
    assert!(matches!(
        run(&d, normalize(vec![(0, 0.0)], -1.0)),
        Err(ModelError::Invalid { .. })
    ));
    assert!(matches!(
        run(&d, normalize(vec![(0, f32::NAN)], -1.0)),
        Err(ModelError::Invalid { .. })
    ));
    // Only a note, or nothing at all.
    assert!(run(&d, normalize(vec![(1, 0.5)], -1.0)).is_err());
    assert!(run(&d, normalize(vec![], -1.0)).is_err());
    // The target must be a finite level from −96 to 0 dBFS.
    for bad in [0.5, -96.5, f32::NAN, f32::INFINITY] {
        assert!(
            matches!(
                run(&d, normalize(vec![(0, 0.25)], bad)),
                Err(ModelError::OutOfRange { .. })
            ),
            "{bad}"
        );
    }
    // An unknown event is refused, not ignored.
    assert!(matches!(
        run(&d, normalize(vec![(99, 0.25)], -1.0)),
        Err(ModelError::UnknownId(_))
    ));
}

#[test]
fn fades_follow_the_cursor() {
    let d = doc();
    let fade_to = |at: Tick, edge: FadeEdge| {
        Command::Audio(AudioCmd::FadeTo {
            events: vec![EventId(0), EventId(1)],
            at,
            edge,
        })
    };
    // Fade In to Cursor at 0.5 s: the fade in ends there; the fade out
    // still fits and keeps its length and shape.
    let (t, f) = at(&d, 0.5);
    let c = fade_to(t, FadeEdge::In);
    assert_eq!(c.name(), "Fade In to Cursor");
    let (_, fi, fo, ..) = region(&run(&d, c).unwrap());
    assert_eq!((fi.length_frames, fi.shape), (f, FadeShape::Linear));
    assert_eq!((fo.length_frames, fo.shape), (2400, FadeShape::EqualPower));
    // Fade Out from Cursor at 1.5 s: the fade out starts there.
    let (t, f) = at(&d, 1.5);
    let c = fade_to(t, FadeEdge::Out);
    assert_eq!(c.name(), "Fade Out from Cursor");
    let (_, fi, fo, ..) = region(&run(&d, c).unwrap());
    assert_eq!((fi.length_frames, fo.length_frames), (4800, 96_000 - f));
    // A fade in to 1.99 s leaves no room for the whole fade out: it shrinks.
    let (t, f) = at(&d, 1.99);
    let (_, fi, fo, ..) = region(&run(&d, fade_to(t, FadeEdge::In)).unwrap());
    assert_eq!((fi.length_frames, fo.length_frames), (f, 96_000 - f));
    // The cursor at the region's start, or past its end: nothing to do.
    assert!(matches!(
        run(&d, fade_to(Tick(0), FadeEdge::In)),
        Err(ModelError::Invalid { .. })
    ));
    let (t, _) = at(&d, 3.0);
    assert!(run(&d, fade_to(t, FadeEdge::Out)).is_err());
}

#[test]
fn remove_fades_clears_both_and_ignores_other_events() {
    let d = doc();
    let c = Command::Audio(AudioCmd::RemoveFades {
        events: vec![EventId(1), EventId(0)],
    });
    assert_eq!(c.name(), "Remove Fades");
    let (_, fi, fo, ..) = region(&run(&d, c).unwrap());
    assert_eq!(fi, Fade::default());
    assert_eq!(fo, Fade::default());
    assert!(
        run(
            &d,
            Command::Audio(AudioCmd::RemoveFades {
                events: vec![EventId(1)]
            })
        )
        .is_err()
    );
}

#[test]
fn relink_rescales_regions_when_the_rate_differs() {
    let d = doc();
    let relink = |frames: u64, rate: u32| {
        Command::Media(MediaCmd::Relink {
            media: MediaId(0),
            path: "Media/a 44k.caf".into(),
            content_hash: Some("h".into()),
            length_frames: frames,
            channels: 2,
            sample_rate: rate,
        })
    };
    // 44.1 / 48 = 0.91875: 48 000 → 44 100, 96 000 → 88 200, fades
    // 4800 → 4410 and 2400 → 2205.
    let mut h = History::new(d.clone());
    h.execute(&relink(176_400, 44_100), &ViewFilter::default())
        .unwrap();
    let (_, fi, fo, off, len) = region(h.doc());
    assert_eq!(
        (off, len, fi.length_frames, fo.length_frames),
        (44_100, 88_200, 4410, 2205)
    );
    let m = &h.doc().media()[&MediaId(0)];
    assert_eq!(
        (m.sample_rate, m.length_frames, m.path.as_str()),
        (44_100, 176_400, "Media/a 44k.caf")
    );
    h.undo().unwrap();
    assert_eq!(h.doc(), &d);
    // The region ends at 144 000 frames = 132 300 at 44.1 kHz: one frame
    // of rounding is allowed (and clamped), two are refused.
    assert!(matches!(
        run(&d, relink(132_298, 44_100)),
        Err(ModelError::OutOfRange { .. })
    ));
    let (_, _, _, off, len) = region(&run(&d, relink(132_299, 44_100)).unwrap());
    assert_eq!((off, len), (44_100, 88_199));
    // At the same rate nothing is rescaled and nothing may overrun.
    assert!(run(&d, relink(143_999, 48_000)).is_err());
    let (_, fi, _, off, len) = region(&run(&d, relink(144_000, 48_000)).unwrap());
    assert_eq!((off, len, fi.length_frames), (48_000, 96_000, 4800));
}
