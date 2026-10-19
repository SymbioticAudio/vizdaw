//! Media-pool commands, project format changes and output paths for new
//! regions (plan 5a Task 2, Rulings 5 and 6).

use core_model::ModelError;
use core_model::command::{
    AudioCmd, Command, LaneCmd, MediaCmd, MediaSwap, RoutingCmd, SequenceCmd, SettingsCmd, apply,
};
use core_model::document::{Document, MediaOrigin};
use core_model::event::{EventBody, Fade, FadeShape};
use core_model::format::{AudioFormat, BitDepth};
use core_model::history::History;
use core_model::ids::{AnyId, EventId, LaneId, MediaId, OutputPathId, SeqId};
use core_model::mixer::StripKind;
use core_model::selection::ViewFilter;
use core_model::time::Tick;

fn run(d: &Document, c: Command) -> core_model::Result<Document> {
    apply(d, &c).map(|a| a.doc)
}

fn add(path: &str, rate: u32, frames: u64) -> Command {
    Command::Media(MediaCmd::Add {
        path: path.into(),
        sample_rate: rate,
        channels: 2,
        length_frames: frames,
        content_hash: None,
        name: None,
        origin: None,
    })
}

fn fade(n: u64) -> Fade {
    Fade {
        length_frames: n,
        shape: FadeShape::Linear,
    }
}

/// Sequence 0 with lane 0; media 0 (`Media/a.caf`, 48 kHz, 192 000
/// frames) and media 1 (`Media/b.caf`, unused).
fn base() -> Document {
    let mut d = run(
        &Document::new(),
        Command::Sequence(SequenceCmd::New { name: "S".into() }),
    )
    .unwrap();
    d = run(&d, add("Media/a.caf", 48_000, 192_000)).unwrap();
    run(&d, add("Media/b.caf", 48_000, 1000)).unwrap()
}

/// `base` plus a region of media 0: offset 48 000, 96 000 frames, fades
/// 4800 in and 2400 out.
fn with_region() -> Document {
    run(
        &base(),
        Command::Audio(AudioCmd::Insert {
            seq: SeqId(0),
            lane: LaneId(0),
            tick: Tick(0),
            media: MediaId(0),
            source_offset: 48_000,
            length: 96_000,
            gain_db: 0.0,
            fade_in: fade(4800),
            fade_out: fade(2400),
        }),
    )
    .unwrap()
}

/// `(source_offset, length, fade in, fade out)` of region `id`.
fn region(d: &Document, id: u64) -> (u64, u64, u64, u64) {
    match d.event(EventId(id)).unwrap().0.body {
        EventBody::AudioRegion {
            source_offset,
            length,
            fade_in,
            fade_out,
            ..
        } => (
            source_offset,
            length,
            fade_in.length_frames,
            fade_out.length_frames,
        ),
        _ => panic!("not a region"),
    }
}

fn swap(media: u64, path: &str, frames: u64) -> MediaSwap {
    MediaSwap {
        media: MediaId(media),
        path: path.into(),
        length_frames: frames,
        content_hash: Some(format!("hash-{media}")),
    }
}

#[test]
fn add_names_items_after_their_file_unless_told() {
    let d = base();
    let a = &d.media()[&MediaId(0)];
    assert_eq!(a.name, "a");
    assert_eq!(
        a.origin,
        MediaOrigin::Imported {
            original_name: "a.caf".into()
        }
    );
    let d = run(
        &d,
        Command::Media(MediaCmd::Add {
            path: "Media/c.caf".into(),
            sample_rate: 48_000,
            channels: 1,
            length_frames: 10,
            content_hash: Some("h".into()),
            name: Some("Vocal".into()),
            origin: Some(MediaOrigin::Imported {
                original_name: "vocal take.wav".into(),
            }),
        }),
    )
    .unwrap();
    let c = &d.media()[&MediaId(2)];
    assert_eq!(
        (c.name.as_str(), c.content_hash.as_deref()),
        ("Vocal", Some("h"))
    );
}

#[test]
fn rename_relink_and_compact() {
    let d = with_region();
    let d = run(
        &d,
        Command::Media(MediaCmd::Rename {
            media: MediaId(0),
            name: "Drums".into(),
        }),
    )
    .unwrap();
    assert_eq!(d.media()[&MediaId(0)].name, "Drums");
    // The region uses frames up to 144 000: a shorter file is refused.
    let relink = |frames: u64| {
        Command::Media(MediaCmd::Relink {
            media: MediaId(0),
            path: "Media/a2.caf".into(),
            content_hash: Some("x".into()),
            length_frames: frames,
            channels: 2,
            sample_rate: 48_000,
        })
    };
    assert!(run(&d, relink(143_999)).is_err());
    let r = run(&d, relink(144_000)).unwrap();
    let m = &r.media()[&MediaId(0)];
    assert_eq!(
        (m.path.as_str(), m.length_frames, m.name.as_str()),
        ("Media/a2.caf", 144_000, "Drums")
    );
    let a = apply(&d, &Command::Media(MediaCmd::Compact)).unwrap();
    assert_eq!(a.report.removed, vec![AnyId::Media(MediaId(1))]);
    assert_eq!(
        a.doc.media().keys().copied().collect::<Vec<_>>(),
        vec![MediaId(0)]
    );
    assert_eq!(Command::Media(MediaCmd::Compact).name(), "Compact Media");
}

#[test]
fn remove_keeps_its_old_semantics() {
    let d = with_region();
    assert_eq!(
        run(
            &d,
            Command::Media(MediaCmd::Remove {
                media: MediaId(0),
                replacement: None
            })
        ),
        Err(ModelError::InUse {
            users: vec![AnyId::Event(EventId(0))]
        })
    );
    // Media 1 has 1000 frames: too short for the region.
    assert!(
        run(
            &d,
            Command::Media(MediaCmd::Remove {
                media: MediaId(0),
                replacement: Some(MediaId(1))
            })
        )
        .is_err()
    );
}

#[test]
fn set_format_rescales_regions_and_undo_restores() {
    let d = with_region();
    let target = AudioFormat {
        sample_rate: 44_100,
        depth: BitDepth::Int16,
    };
    let cmd = Command::Settings(SettingsCmd::SetFormat {
        format: target,
        swaps: vec![
            swap(0, "Media/a 2.caf", 176_400),
            swap(1, "Media/b 2.caf", 919),
        ],
    });
    assert_eq!(cmd.name(), "Change Project Format");
    let mut h = History::new(d.clone());
    h.execute(&cmd, &ViewFilter::default()).unwrap();
    let after = h.doc().clone();
    assert_eq!(after.settings().format, target);
    // 44.1 / 48: 48 000 → 44 100, 96 000 → 88 200, 4800 → 4410, 2400 → 2205.
    assert_eq!(region(&after, 0), (44_100, 88_200, 4410, 2205));
    let a = &after.media()[&MediaId(0)];
    assert_eq!(
        (
            a.path.as_str(),
            a.sample_rate,
            a.length_frames,
            a.content_hash.as_deref()
        ),
        ("Media/a 2.caf", 44_100, 176_400, Some("hash-0"))
    );
    h.undo().unwrap();
    assert_eq!(h.doc(), &d);
}

#[test]
fn set_format_must_convert_every_item_once() {
    let d = with_region();
    let to = |swaps: Vec<MediaSwap>, rate: u32| {
        run(
            &d,
            Command::Settings(SettingsCmd::SetFormat {
                format: AudioFormat {
                    sample_rate: rate,
                    depth: BitDepth::Int24,
                },
                swaps,
            }),
        )
    };
    // Media 1 is missing.
    assert!(to(vec![swap(0, "Media/x.caf", 10)], 44_100).is_err());
    // Media 0 twice.
    assert!(
        to(
            vec![
                swap(0, "Media/x.caf", 10),
                swap(0, "Media/y.caf", 10),
                swap(1, "Media/z.caf", 10)
            ],
            44_100
        )
        .is_err()
    );
    // Not an allowed rate.
    assert!(
        to(
            vec![swap(0, "Media/x.caf", 10), swap(1, "Media/z.caf", 10)],
            22_050
        )
        .is_err()
    );
    // The same format is refused: that is Convert's job.
    assert!(
        to(
            vec![
                swap(0, "Media/x.caf", 192_000),
                swap(1, "Media/z.caf", 1000)
            ],
            48_000
        )
        .is_err()
    );
    // An empty project changes format without swaps.
    let e = run(
        &Document::new(),
        Command::Settings(SettingsCmd::SetFormat {
            format: AudioFormat {
                sample_rate: 96_000,
                depth: BitDepth::Float32,
            },
            swaps: vec![],
        }),
    )
    .unwrap();
    assert_eq!(e.settings().format.sample_rate, 96_000);
}

#[test]
fn convert_swaps_some_items_at_the_project_rate() {
    // An old 44.1 kHz item in a 48 kHz project.
    let mut d = run(&base(), add("Media/old.wav", 44_100, 88_200)).unwrap();
    d = run(
        &d,
        Command::Audio(AudioCmd::Insert {
            seq: SeqId(0),
            lane: LaneId(0),
            tick: Tick(0),
            media: MediaId(2),
            source_offset: 0,
            length: 88_200,
            gain_db: 0.0,
            fade_in: fade(0),
            fade_out: fade(0),
        }),
    )
    .unwrap();
    let cmd = Command::Media(MediaCmd::Convert {
        swaps: vec![swap(2, "Media/old.caf", 96_000)],
    });
    assert_eq!(cmd.name(), "Convert Media");
    let after = run(&d, cmd).unwrap();
    assert_eq!(after.media()[&MediaId(2)].sample_rate, 48_000);
    assert_eq!(region(&after, 0), (0, 96_000, 0, 0));
    assert!(run(&d, Command::Media(MediaCmd::Convert { swaps: vec![] })).is_err());
}

#[test]
fn a_shorter_converted_file_clamps_its_regions() {
    let d = with_region();
    let after = run(
        &d,
        Command::Media(MediaCmd::Convert {
            swaps: vec![swap(0, "Media/a 2.caf", 100_000)],
        }),
    )
    .unwrap();
    // Offset 48 000 stays; the length shrinks to fit 100 000 frames, and
    // the fade out gets what is left after the fade in.
    assert_eq!(region(&after, 0), (48_000, 52_000, 4800, 2400));
}

#[test]
fn inserting_a_region_gives_the_lane_an_output_path() {
    let d = with_region();
    let lane = d.lane(SeqId(0), LaneId(0)).unwrap();
    assert_eq!(lane.audio_out, Some(OutputPathId(0)));
    let out = &d.outputs()[&OutputPathId(0)];
    assert_eq!(
        (out.name.as_str(), out.reference.as_str()),
        ("Audio 1", "audio:1-2")
    );
    assert!(
        d.mixer()
            .find_kind(StripKind::AudioPath(OutputPathId(0)))
            .is_some()
    );
    // A second lane gets the first existing output path.
    let d = run(
        &d,
        Command::Lane(LaneCmd::Add {
            seq: SeqId(0),
            name: "B".into(),
            index: None,
        }),
    )
    .unwrap();
    let d = run(
        &d,
        Command::Routing(RoutingCmd::AddOutput {
            name: "Later".into(),
            reference: "bus:9".into(),
        }),
    )
    .unwrap();
    let lane_b = d.sequence(SeqId(0)).unwrap().lanes[1].id;
    let d = run(
        &d,
        Command::Audio(AudioCmd::Insert {
            seq: SeqId(0),
            lane: lane_b,
            tick: Tick(0),
            media: MediaId(1),
            source_offset: 0,
            length: 10,
            gain_db: 0.0,
            fade_in: fade(0),
            fade_out: fade(0),
        }),
    )
    .unwrap();
    assert_eq!(
        d.lane(SeqId(0), lane_b).unwrap().audio_out,
        Some(OutputPathId(0))
    );
    assert_eq!(d.outputs().len(), 2);
}

/// Final review I3: the media referenced anywhere in the history (undo,
/// present, redo, and an open transaction's start) with their hashes.
#[test]
fn referenced_media_anywhere_spans_undo_and_redo() {
    let mut h = History::new(Document::new());
    let f = ViewFilter::default();
    let hashed = |path: &str, hash: &str| {
        Command::Media(MediaCmd::Add {
            path: path.into(),
            sample_rate: 48_000,
            channels: 2,
            length_frames: 10,
            content_hash: Some(hash.into()),
            name: None,
            origin: None,
        })
    };
    h.execute(&hashed("Media/a.caf", "aa"), &f).unwrap();
    h.execute(&add("Media/b.caf", 48_000, 10), &f).unwrap();
    h.execute(&hashed("Media/c.caf", "cc"), &f).unwrap();
    // c only in the redo history; a and b in the present.
    h.undo().unwrap();
    let all = h.referenced_media_anywhere();
    let want: Vec<(&str, Option<&str>)> = vec![
        ("Media/a.caf", Some("aa")),
        ("Media/b.caf", None),
        ("Media/c.caf", Some("cc")),
    ];
    let got: Vec<(&str, Option<&str>)> = all
        .iter()
        .map(|(p, x)| (p.as_str(), x.as_deref()))
        .collect();
    assert_eq!(got, want);
    // Only the undo history: back to an empty pool, everything still
    // counts.
    h.undo().unwrap();
    h.undo().unwrap();
    assert!(h.doc().media().is_empty());
    assert_eq!(h.referenced_media_anywhere().len(), 3);
    // A transaction's start counts while it is open.
    h.redo().unwrap();
    h.begin("Drag").unwrap();
    assert_eq!(h.referenced_media_anywhere().len(), 3);
    h.abort().unwrap();
}
