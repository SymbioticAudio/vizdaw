//! The merge's base (plan 6e Task 1): snapshot format 2 with the identity
//! of every projection row, its bounded reader, the return normalisers and
//! the report's Conflicts group.

mod common;

use common::merge::fixture;
use common::package::TestPlist;
use common::{CountingAlloc, measure};
use core_engine::fixtures::{BAR, Q};
use core_io::IoError;
use core_io::dawproject::export::{ClipRule, ExportEnv, ExportOptions, RegionGain, export};
use core_io::dawproject::record::{
    EventAt, Identity, ObjectRef, PNote, PRegion, PTrack, Projection, SNAPSHOT_DAMAGED,
    SNAPSHOT_FORMAT, Snapshot, SnapshotLimits, UseAt, as_returned, read_snapshot,
    read_snapshot_file, read_snapshot_with, returned,
};
use core_io::limits::{MAX_SNAPSHOT_BYTES, MAX_XML_NODES};
use core_io::report::{ItemKind, Report};
use core_model::document::{DestKind, Document};
use core_model::event::EventBody;
use core_model::ids::{EventId, SeqId};
use core_model::mixer::StripKind;
use core_model::time::Tick;
use std::collections::BTreeMap;
use std::sync::atomic::AtomicBool;

#[global_allocator]
static ALLOC: CountingAlloc = CountingAlloc;

const KIB: usize = 1024;
const PROJECT: &str = "00000000-0000-4000-8000-000000000001";
const EXPORT: &str = "00000000-0000-4000-8000-0000000000e1";

fn env() -> ExportEnv<'static> {
    ExportEnv {
        plist: &TestPlist,
        app_version: "1.2.3",
        project_id: PROJECT,
        export_id: EXPORT,
        exported_at: 1_700_000_000,
    }
}

fn snapshot_json(doc: &Document, seq: SeqId) -> String {
    let o = ExportOptions {
        seq,
        region_gain: RegionGain::Apply,
        clips: ClipRule::SilenceGap(Tick(4 * Q)),
        left_out: Default::default(),
    };
    export(doc, &o, &env(), &AtomicBool::new(false))
        .expect("exports")
        .snapshot_json
}

#[track_caller]
fn damaged<T: std::fmt::Debug>(r: Result<T, IoError>) {
    assert_eq!(
        r.map(|_| ()),
        Err(IoError::Refused(SNAPSHOT_DAMAGED.to_owned()))
    );
}

/// The events of lane `name` of `seq`'s active take, by tick.
fn events_of(doc: &Document, seq: SeqId, name: &str) -> Vec<(i64, EventId, EventBody)> {
    let s = doc.sequence(seq).unwrap();
    let lane = s.lanes.iter().find(|l| l.name == name).unwrap();
    let mut v: Vec<_> = lane
        .active()
        .unwrap()
        .events
        .values()
        .map(|e| (e.tick.0, e.id, e.body.clone()))
        .collect();
    v.sort_by_key(|e| (e.0, e.1));
    v
}

#[test]
fn snapshot_round_trips_with_identity() {
    let (doc, seq) = fixture();
    let snap = read_snapshot(snapshot_json(&doc, seq).as_bytes()).expect("reads");
    assert_eq!(snap.format, 2);
    assert_eq!(SNAPSHOT_FORMAT, 2);
    assert_eq!(snap.region_gain.as_deref(), Some("apply"));
    let id = snap.identity.as_ref().expect("an identity");
    let p = &snap.projection;

    let s = doc.sequence(seq).unwrap();
    let lane = |n: &str| s.lanes.iter().find(|l| l.name == n).unwrap();
    let (bass, keys) = (lane("Bass"), lane("Keys"));
    assert_eq!(
        p.tracks.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(),
        vec!["Bass", "Keys", "Keys audio"]
    );
    assert_eq!(
        id.tracks.iter().map(|t| t.lane).collect::<Vec<_>>(),
        vec![bass.id.0, keys.id.0, keys.id.0]
    );

    // Bass: its own note, then the riff's notes through each use.
    let riff = *doc
        .sequences()
        .iter()
        .find(|(_, q)| q.name == "Riff")
        .unwrap()
        .0;
    let riff_notes: BTreeMap<u8, u64> = events_of(&doc, riff, "Riff")
        .into_iter()
        .filter_map(|(_, e, b)| match b {
            EventBody::Note { pitch, .. } => Some((pitch, e.0)),
            _ => None,
        })
        .collect();
    let bass_events = events_of(&doc, seq, "Bass");
    let own = bass_events
        .iter()
        .find(|e| matches!(e.2, EventBody::Note { .. }))
        .unwrap()
        .1
        .0;
    let uses: Vec<u64> = bass_events
        .iter()
        .filter(|e| matches!(e.2, EventBody::Reference { .. }))
        .map(|e| e.1.0)
        .collect();
    assert_eq!(uses.len(), 2);
    let t = &p.tracks[0];
    let ti = &id.tracks[0];
    assert_eq!(
        t.notes_played.len(),
        7,
        "one own note, 2 + 2 × 2 riff notes"
    );
    assert_eq!(ti.notes.len(), t.notes_played.len());
    for (n, at) in t.notes_played.iter().zip(&ti.notes) {
        if n.tick < BAR {
            assert_eq!((at.event, at.top), (own, own), "{n:?}");
        } else {
            let top = if n.tick < 2 * BAR { uses[0] } else { uses[1] };
            assert_eq!(
                *at,
                EventAt {
                    event: riff_notes[&n.pitch],
                    top
                },
                "{n:?}"
            );
        }
    }
    assert_eq!(
        ti.uses,
        vec![
            UseAt {
                event: uses[0],
                seq: riff.0,
                tick: BAR,
                end: 2 * BAR,
                transpose: 0,
                passes: 1
            },
            UseAt {
                event: uses[1],
                seq: riff.0,
                tick: 3 * BAR,
                end: 5 * BAR,
                transpose: 0,
                passes: 2
            },
        ]
    );

    // Keys: its note; its audio half: the crossfaded pair.
    let keys_events = events_of(&doc, seq, "Keys");
    let id_of = |audio: bool| -> Vec<u64> {
        keys_events
            .iter()
            .filter(|e| matches!(e.2, EventBody::AudioRegion { .. }) == audio)
            .map(|e| e.1.0)
            .collect()
    };
    let note = id_of(false);
    assert_eq!(
        id.tracks[1].notes,
        vec![EventAt {
            event: note[0],
            top: note[0]
        }]
    );
    assert!(id.tracks[1].uses.is_empty() && id.tracks[1].regions.is_empty());
    let regions = id_of(true);
    assert_eq!(p.tracks[2].regions.len(), 2);
    assert!(p.tracks[2].regions[1].crossfade);
    assert_eq!(
        id.tracks[2].regions,
        regions
            .iter()
            .map(|&e| EventAt { event: e, top: e })
            .collect::<Vec<_>>()
    );
    assert!(id.tracks[2].notes.is_empty() && id.tracks[2].uses.is_empty());

    // Bass's strip: its instrument, then the insert.
    let dest = bass.midi_dest.unwrap();
    let strip = doc.mixer().find_kind(StripKind::Instrument(dest)).unwrap();
    let instrument = match &doc.instruments().get(&dest).unwrap().kind {
        DestKind::PluginInstrument { slot } => slot.id.0,
        _ => panic!("not a plug-in instrument"),
    };
    let insert = doc.mixer().strip(strip).unwrap().inserts[0].id.0;
    let k = t.strip.unwrap();
    assert_eq!(p.strips[k].gain_db, -6.0);
    assert_eq!(id.strips.len(), p.strips.len());
    assert_eq!(id.strips[k].strip, Some(strip.0));
    assert_eq!(id.strips[k].devices, vec![instrument, insert]);
    assert_eq!(p.strips[k].devices.len(), 2);
    assert_eq!(p.markers.len(), 2);
    assert!(p.strips.iter().any(|x| x.name == "Reverb"));
}

/// The fixture's snapshot JSON as a value.
fn snapshot_value() -> serde_json::Value {
    let (doc, seq) = fixture();
    serde_json::from_str(&snapshot_json(&doc, seq)).unwrap()
}

fn bytes(v: &serde_json::Value) -> Vec<u8> {
    serde_json::to_vec(v).unwrap()
}

#[test]
fn format_one_still_reads() {
    let mut v = snapshot_value();
    v["format"] = 1.into();
    let o = v.as_object_mut().unwrap();
    o.remove("identity");
    o.remove("region_gain");
    let snap = read_snapshot(&bytes(&v)).expect("a format 1 snapshot reads");
    assert_eq!(snap.format, 1);
    assert_eq!(snap.identity, None);
    assert_eq!(snap.region_gain, None);
}

#[test]
fn read_snapshot_refuses_damage() {
    let whole = bytes(&snapshot_value());
    assert!(read_snapshot(&whole).is_ok());
    damaged(read_snapshot(&whole[..whole.len() / 2]));

    let mut v = snapshot_value();
    v["format"] = 7.into();
    damaged(read_snapshot(&bytes(&v)));

    let mut v = snapshot_value();
    v["project_id"] = "x".into();
    damaged(read_snapshot(&bytes(&v)));

    let mut v = snapshot_value();
    v["identity"]["tracks"].as_array_mut().unwrap().pop();
    damaged(read_snapshot(&bytes(&v)));

    let mut v = snapshot_value();
    v["identity"]["tracks"][0]["notes"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({"event": 1, "top": 1}));
    damaged(read_snapshot(&bytes(&v)));
}

/// A format 2 snapshot of nothing, with `ids`.
fn small(ids: BTreeMap<String, ObjectRef>) -> Vec<u8> {
    serde_json::to_vec(&Snapshot {
        format: SNAPSHOT_FORMAT,
        project_id: PROJECT.to_owned(),
        export_id: EXPORT.to_owned(),
        exported_at: 0,
        seq: 1,
        origin: 0,
        ids,
        projection: Projection {
            tracks: Vec::new(),
            strips: Vec::new(),
            tempo: Vec::new(),
            meter: Vec::new(),
            markers: Vec::new(),
            info: Vec::new(),
            origin: 0,
        },
        region_gain: Some("apply".to_owned()),
        identity: Some(Identity {
            tracks: Vec::new(),
            strips: Vec::new(),
        }),
    })
    .unwrap()
}

fn object() -> ObjectRef {
    ObjectRef {
        kind: "t".to_owned(),
        id: 0,
        slot: None,
        index: None,
    }
}

#[test]
fn snapshot_limits() {
    let limits = SnapshotLimits {
        max_bytes: 4096,
        max_ids: 100,
    };
    // Exactly at the size limit, padded in an ID; then one byte over.
    let base = small(BTreeMap::from([("a".to_owned(), object())])).len();
    let padded = |n: usize| small(BTreeMap::from([("a".repeat(1 + n), object())]));
    let at = padded(4096 - base);
    assert_eq!(at.len(), 4096);
    assert!(read_snapshot_with(&at, &limits).is_ok());
    let over = padded(4097 - base);
    assert_eq!(over.len(), 4097);
    let (r, seen) = measure(|| read_snapshot_with(&over, &limits));
    damaged(r);
    assert!(seen.peak < 64 * KIB, "peak {} bytes", seen.peak);

    // At most 100 IDs, counted while parsing.
    let ids = |n: usize| (0..n).map(|i| (i.to_string(), object())).collect();
    let hundred = small(ids(100));
    assert!(hundred.len() <= 4096);
    assert_eq!(
        read_snapshot_with(&hundred, &limits)
            .expect("100 IDs read")
            .ids
            .len(),
        100
    );
    let more = small(ids(101));
    assert!(more.len() <= 4096, "{} bytes", more.len());
    let (r, seen) = measure(|| read_snapshot_with(&more, &limits));
    damaged(r);
    assert!(seen.peak < 64 * KIB, "peak {} bytes", seen.peak);

    let d = SnapshotLimits::default();
    assert_eq!(
        (d.max_bytes, d.max_ids),
        (MAX_SNAPSHOT_BYTES, MAX_XML_NODES)
    );
    assert_eq!(MAX_SNAPSHOT_BYTES, 256 << 20);

    // The project's file: missing, regular, a link, a folder, a FIFO.
    let root = tempfile::tempdir().unwrap();
    assert_eq!(read_snapshot_file(root.path(), EXPORT), Ok(None));
    let dir = root.path().join("Interchange");
    std::fs::create_dir(&dir).unwrap();
    assert_eq!(read_snapshot_file(root.path(), EXPORT), Ok(None));
    let file = dir.join(format!("{EXPORT}.json"));
    std::fs::write(&file, small(BTreeMap::new())).unwrap();
    assert!(read_snapshot_file(root.path(), EXPORT).unwrap().is_some());
    std::fs::remove_file(&file).unwrap();

    let elsewhere = root.path().join("elsewhere.json");
    std::fs::write(&elsewhere, small(BTreeMap::new())).unwrap();
    std::os::unix::fs::symlink(&elsewhere, &file).unwrap();
    damaged(read_snapshot_file(root.path(), EXPORT));
    std::fs::remove_file(&file).unwrap();

    std::fs::create_dir(&file).unwrap();
    damaged(read_snapshot_file(root.path(), EXPORT));
    std::fs::remove_dir(&file).unwrap();

    let made = std::process::Command::new("mkfifo")
        .arg(&file)
        .status()
        .unwrap();
    assert!(made.success());
    damaged(read_snapshot_file(root.path(), EXPORT));
}

// ---------------------------------------------------------------- normalisers

fn track(
    notes: Vec<PNote>,
    regions: Vec<PRegion>,
    automation: Vec<(String, Vec<(i64, f64)>)>,
) -> PTrack {
    PTrack {
        name: "T".to_owned(),
        notes: true,
        audio: true,
        strip: None,
        notes_played: notes,
        midi: vec![(100, "cc 0 7 100".to_owned())],
        regions,
        automation,
    }
}

fn note(tick: i64, expression: Vec<(&str, i64, u32)>) -> PNote {
    PNote {
        tick,
        pitch: 60,
        channel: 0,
        velocity: 0x8000,
        release: 0,
        duration: 480,
        expression: expression
            .into_iter()
            .map(|(k, o, v)| (k.to_owned(), o, v))
            .collect(),
    }
}

fn region(tick: i64, hash: Option<&str>, gain_db: f32) -> PRegion {
    PRegion {
        tick,
        media_hash: hash.map(str::to_owned),
        offset: 0,
        length: 48_000,
        fade_in: 0,
        fade_out: 0,
        muted: false,
        name: None,
        crossfade: false,
        gain_db,
    }
}

fn projection(
    tempo: Vec<(i64, u32, bool)>,
    meter: Vec<(i64, u8, u8)>,
    markers: Vec<(i64, &str)>,
    tracks: Vec<PTrack>,
    origin: i64,
) -> Projection {
    Projection {
        tracks,
        strips: Vec::new(),
        tempo,
        meter,
        markers: markers
            .into_iter()
            .map(|(t, n)| (t, n.to_owned()))
            .collect(),
        info: vec![Some("Title".to_owned())],
        origin,
    }
}

#[test]
fn as_returned_shifts_the_pickup_and_flattens_the_last_ramp() {
    let ours = projection(
        vec![(-3840, 120_000, false), (0, 90_000, true)],
        vec![(-3840, 3, 4), (0, 4, 4)],
        vec![(0, "Verse")],
        vec![track(
            vec![note(
                0,
                vec![("pitch_bend", 10, 5), ("registered 74", 0, 7)],
            )],
            vec![region(0, Some("h"), -6.0), region(960, Some("g"), 0.0)],
            vec![("fader 0".to_owned(), vec![(0, 0.5), (480, 0.7)])],
        )],
        -3840,
    );
    let shifted = |hash_of_gained: Option<&str>| {
        let mut t = track(
            vec![note(
                3840,
                vec![("registered 74", 0, 7), ("pitch_bend", 10, 5)],
            )],
            vec![
                region(3840, hash_of_gained, 0.0),
                region(4800, Some("g"), 0.0),
            ],
            vec![("fader 0".to_owned(), vec![(3840, 0.5), (4320, 0.7)])],
        );
        t.midi = vec![(3940, "cc 0 7 100".to_owned())];
        projection(
            vec![(0, 120_000, false), (3840, 90_000, false)],
            vec![(0, 3, 4), (3840, 4, 4)],
            vec![(3840, "Verse")],
            vec![t],
            0,
        )
    };
    assert_eq!(
        as_returned(&ours, -3840, RegionGain::Apply),
        shifted(None),
        "a region with gain comes back as a new copy, without a hash to compare"
    );
    assert_eq!(
        as_returned(&ours, -3840, RegionGain::LeaveOut),
        shifted(Some("h"))
    );
}

#[test]
fn returned_drops_hold_points() {
    let theirs = projection(
        vec![(0, 90_000, false), (3840, 120_000, true)],
        vec![(0, 3, 4), (1920, 4, 4)],
        vec![(3840, "Verse")],
        vec![track(
            vec![note(
                3840,
                vec![("pitch_bend", 10, 5), ("registered 74", 0, 7)],
            )],
            vec![region(3840, Some("h@-6"), 0.0)],
            vec![("fader 0".to_owned(), vec![(0, 0.2), (1, 0.2), (2, 0.8)])],
        )],
        0,
    );
    let mut want_track = track(
        vec![note(
            3840,
            vec![("registered 74", 0, 7), ("pitch_bend", 10, 5)],
        )],
        vec![region(3840, Some("h@-6"), 0.0)],
        vec![("fader 0".to_owned(), vec![(0, 0.2), (2, 0.8)])],
    );
    want_track.midi = vec![(100, "cc 0 7 100".to_owned())];
    let want = projection(
        vec![(3840, 120_000, false)],
        vec![(3840, 4, 4)],
        vec![(3840, "Verse")],
        vec![want_track],
        0,
    );
    assert_eq!(returned(&theirs, -3840), want);
}

#[test]
fn conflicts_come_first_in_plain() {
    let mut r = Report::default();
    r.add(ItemKind::Note, "a note", None);
    r.add(ItemKind::LeftOut, "left out", None);
    r.add(ItemKind::Changed, "changed", None);
    r.add(ItemKind::Conflict, "a conflict", Some("Bass".to_owned()));
    assert_eq!(
        r.plain("Merged"),
        "Merged nothing\n\
         Conflicts\n  a conflict\n    Bass\n\
         Changed\n  changed\n\
         Left out\n  left out\n\
         Notes\n  a note"
    );
}
