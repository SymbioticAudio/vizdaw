//! DAWproject round trips (plan 6d Task 8): a document exported, written as
//! a package, opened, mapped and applied to a new project comes back with
//! the same projection, within the tolerances below. Every export in this
//! file also passes the vendored schema.
//!
//! **Tolerances**, on `projection(original)` and `projection(imported)`:
//! exact for names, notes with their expression, MIDI events, region ticks,
//! offsets, lengths, fades, mutes and crossfade flags, the tempo and meter,
//! markers, song info, sends' targets and flags, and devices (format, ID,
//! bypass, state hash); within 1e-9 for levels, pan and automation values
//! (the hold points the import adds are dropped); colours exact; a region's
//! `gain_db` equal after `LeaveOut` and 0 after `Apply`. The import does not
//! restore a pickup (it has no place for one), so the original's ticks are
//! shifted by its origin before they are compared.

mod common;

use common::generators::*;
use common::package::{TestPlist, copy_hash, media_of, write_package};
use common::xmllint::schema_check;
use core_engine::fixtures::{BAR, Builder, Q};
use core_io::dawproject::export::{
    ClipRule, DpExport, ExportEnv, ExportOptions, RegionGain, export,
};
use core_io::dawproject::import::{ImportEnv, apply, map};
use core_io::dawproject::package::open;
use core_io::dawproject::record::{Projection, as_returned, projection, returned};
use core_io::limits::PackageLimits;
use core_io::report::{ItemKind, Report};
use core_model::command::{Command, LaneCmd, SequenceCmd};
use core_model::document::Document;
use core_model::history::History;
use core_model::ids::{AnyId, SeqId};
use core_model::mixer::PluginFormat;
use core_model::selection::ViewFilter;
use core_model::time::Tick;
use std::sync::atomic::AtomicBool;

const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/dawproject");

// ---------------------------------------------------------------- the harness

/// Everything one trip made.
struct Trip {
    /// The imported project.
    doc: Document,
    /// Its sequence.
    seq: SeqId,
    /// The export's files.
    x: DpExport,
    /// What the import's report says.
    import: Report,
}

fn env() -> ExportEnv<'static> {
    ExportEnv {
        plist: &TestPlist,
        app_version: "1.2.3",
        project_id: "00000000-0000-4000-8000-000000000001",
        export_id: "E1",
        exported_at: 1_700_000_000,
    }
}

fn options(seq: SeqId, region_gain: RegionGain) -> ExportOptions {
    ExportOptions {
        seq,
        region_gain,
        clips: ClipRule::SilenceGap(Tick(4 * Q)),
        left_out: Default::default(),
    }
}

/// A document like a new project: the master and one sequence with no
/// tracks.
fn new_project() -> (Document, SeqId) {
    let mut h = History::new(Document::new());
    let r = h
        .execute(
            &Command::Sequence(SequenceCmd::New {
                name: "Sequence 1".into(),
            }),
            &ViewFilter::default(),
        )
        .unwrap();
    let seq = r
        .created
        .iter()
        .find_map(|id| match id {
            AnyId::Seq(s) => Some(*s),
            _ => None,
        })
        .unwrap();
    let lane = h.doc().sequence(seq).unwrap().lanes.front().unwrap().id;
    h.execute(
        &Command::Lane(LaneCmd::Remove { seq, lane }),
        &ViewFilter::default(),
    )
    .unwrap();
    (h.doc().clone(), seq)
}

/// Exports `doc`, checks the export against the schema, writes the package,
/// opens and maps it (each entry's media the original item, or its gain
/// copy) and applies it to a new project at the same rate.
fn trip(doc: &Document, o: &ExportOptions) -> Trip {
    let t = trip_without_schema(doc, o);
    check_schema(&t.x);
    t
}

/// Both files of `x` against the vendored XSDs.
fn check_schema(x: &DpExport) {
    schema_check(&format!("{DIR}/Project.xsd"), &x.project_xml);
    schema_check(&format!("{DIR}/MetaData.xsd"), &x.metadata_xml);
}

/// [`trip`] without the schema check (for the timed case, which checks the
/// export afterwards).
fn trip_without_schema(doc: &Document, o: &ExportOptions) -> Trip {
    let x = export(doc, o, &env(), &AtomicBool::new(false)).expect("exports");
    let dir = tempfile::tempdir().unwrap();
    let path = write_package(dir.path(), "Trip", &x);
    let pkg = open(&path, &PackageLimits::default(), &AtomicBool::new(false)).expect("opens");
    let media = media_of(&pkg, &x, doc);
    let installed = |_: PluginFormat, _: &str| true;
    let imp = map(
        &pkg,
        &media,
        &ImportEnv {
            installed: &installed,
            plist: &TestPlist,
            rate: 48_000,
            ids: None,
        },
        &AtomicBool::new(false),
    )
    .expect("maps");
    let (new, seq) = new_project();
    let mut h = History::new(new);
    let mut report = Report::default();
    h.begin("Import DAWproject").unwrap();
    let states = apply(&mut h, &imp, seq, &mut report).expect("applies");
    h.commit().unwrap();
    let mut doc = h.doc().clone();
    for (slot, bytes) in states {
        doc = core_model::state::capture(&doc, slot, &bytes)
            .expect("captures")
            .0;
    }
    Trip {
        doc,
        seq,
        x,
        import: report,
    }
}

/// [`trip`] as the plan names it: the imported document, the export's report
/// and the import's.
fn round_trip(doc: &Document, o: &ExportOptions) -> (Document, Report, Report) {
    let t = trip(doc, o);
    (t.doc, t.x.report, t.import)
}

// ---------------------------------------------------------------- comparing

const EPS: f64 = 1e-9;

#[track_caller]
fn near(a: f64, b: f64, what: &str) {
    assert!((a - b).abs() <= EPS, "{what}: {a} vs {b}");
}

/// Asserts that `imported` is `original` within the tolerances, after the
/// merge's normalisers put both in the file's frame (plan 6e Ruling 1).
/// `gain` is the export's region-gain option. Under `Apply` each region
/// with gain must come back with its gain copy's hash; that hash is then
/// cleared, as `as_returned` clears the original's.
#[track_caller]
fn same(original: &Projection, imported: &Projection, gain: RegionGain) {
    assert_eq!(imported.origin, 0, "an import has no pickup");
    let origin = original.origin;
    let mut theirs = returned(imported, origin);
    if gain == RegionGain::Apply {
        assert_eq!(original.tracks.len(), theirs.tracks.len(), "tracks");
        for (a, b) in original.tracks.iter().zip(&mut theirs.tracks) {
            assert_eq!(
                a.regions.len(),
                b.regions.len(),
                "track '{}': regions",
                a.name
            );
            for (r, s) in a.regions.iter().zip(&mut b.regions) {
                if r.gain_db != 0.0 {
                    assert_eq!(
                        s.media_hash,
                        r.media_hash.as_ref().map(|h| copy_hash(h, r.gain_db)),
                        "track '{}': a region's gain copy",
                        a.name
                    );
                    s.media_hash = None;
                }
            }
        }
    }
    assert_projection_eq(&as_returned(original, origin, gain), &theirs);
}

/// Asserts that two projections in the same frame are equal within the
/// tolerances.
#[track_caller]
fn assert_projection_eq(original: &Projection, imported: &Projection) {
    assert_eq!(original.origin, imported.origin, "origin");
    assert_eq!(original.info, imported.info, "song info");
    assert_eq!(original.markers, imported.markers, "markers");
    assert_eq!(original.tempo, imported.tempo, "tempo map");
    assert_eq!(original.meter, imported.meter, "meter map");
    assert_eq!(original.strips.len(), imported.strips.len(), "strips");
    for (i, (a, b)) in original.strips.iter().zip(&imported.strips).enumerate() {
        let at = format!("strip {i} '{}'", a.name);
        assert_eq!(a.name, b.name, "{at}");
        assert_eq!(a.role, b.role, "{at}");
        assert_eq!(a.colour, b.colour, "{at}: colour");
        near(a.gain_db, b.gain_db, &format!("{at}: level"));
        near(a.pan, b.pan, &format!("{at}: pan"));
        assert_eq!((a.mute, a.solo), (b.mute, b.solo), "{at}");
        assert_eq!(a.output, b.output, "{at}: output");
        assert_eq!(a.sends.len(), b.sends.len(), "{at}: sends");
        for (s, t) in a.sends.iter().zip(&b.sends) {
            assert_eq!((s.0, s.2, s.3), (t.0, t.2, t.3), "{at}: send");
            near(s.1, t.1, &format!("{at}: send level"));
        }
        assert_eq!(a.devices, b.devices, "{at}: devices");
    }
    assert_eq!(original.tracks.len(), imported.tracks.len(), "tracks");
    for (a, b) in original.tracks.iter().zip(&imported.tracks) {
        let at = format!("track '{}'", a.name);
        assert_eq!(a.name, b.name, "{at}");
        assert_eq!(
            (a.notes, a.audio, a.strip),
            (b.notes, b.audio, b.strip),
            "{at}"
        );
        assert_eq!(a.notes_played, b.notes_played, "{at}: notes");
        assert_eq!(a.midi, b.midi, "{at}: MIDI events");
        assert_eq!(a.regions, b.regions, "{at}: regions");
        assert_eq!(a.automation.len(), b.automation.len(), "{at}: automation");
        for ((na, pa), (nb, pb)) in a.automation.iter().zip(&b.automation) {
            assert_eq!(na, nb, "{at}: automation target");
            assert_eq!(pa.len(), pb.len(), "{at}: {na} points");
            for ((ta, va), (tb, vb)) in pa.iter().zip(pb) {
                assert_eq!(ta, tb, "{at}: {na} tick");
                near(*va, *vb, &format!("{at}: {na}"));
            }
        }
    }
}

/// The trip's projections: the original's and the imported one's.
fn projections(doc: &Document, seq: SeqId, t: &Trip) -> (Projection, Projection) {
    (
        projection(doc, seq).expect("projects"),
        projection(&t.doc, t.seq).expect("projects the import"),
    )
}

// ---------------------------------------------------------------- tests

use core_model::automation::AutomationTarget;
use core_model::command::{AudioCmd, CrossfadeLength, MixerCmd};
use core_model::event::{EventBody, FadeShape, LoopCount, PerNoteCtl, PerNoteKind};
use core_model::meter::{KeySig, Meter, MeterEntry, MeterMap};
use core_model::sequence::Marker;
use proptest::prelude::*;

/// Counts the items of the report named `what`.
fn count(r: &Report, kind: ItemKind, what: &str) -> u64 {
    r.items
        .iter()
        .find(|i| i.kind == kind && i.what == what)
        .map_or(0, |i| i.count)
}

/// Every reference event of lane `name` of the imported sequence, with its
/// target and repeats.
fn uses_on(t: &Trip, name: &str) -> Vec<(SeqId, LoopCount)> {
    let s = t.doc.sequence(t.seq).unwrap();
    let lane = s
        .lanes
        .iter()
        .find(|l| l.name == name)
        .unwrap_or_else(|| panic!("no track {name}"));
    let mut out: Vec<(i64, SeqId, LoopCount)> = lane
        .active()
        .unwrap()
        .events
        .values()
        .filter_map(|e| match &e.body {
            EventBody::Reference {
                seq, loop_count, ..
            } => Some((e.tick.0, *seq, *loop_count)),
            _ => None,
        })
        .collect();
    out.sort_by_key(|u| u.0);
    out.into_iter().map(|(_, s, l)| (s, l)).collect()
}

#[track_caller]
fn trips_equal(doc: &Document, seq: SeqId, gain: RegionGain) -> Trip {
    let t = trip(doc, &options(seq, gain));
    let (a, b) = projections(doc, seq, &t);
    same(&a, &b, gain);
    t
}

// ---------------------------------------------------------------- fixed cases

fn riff_doc(uses: &[(i64, u32, i8)]) -> (Document, SeqId) {
    let mut b = Builder::new();
    let (seq, keys, dest, _) = song(&mut b, "Keys");
    let (riff, rl) = b.sequence("Riff", "Riff", Some(dest)).unwrap();
    b.notes(riff, rl, &[(0, 60, 0x8000, Q), (Q, 64, 0x9000, Q)])
        .unwrap();
    lock(&mut b, riff, BAR);
    for (tick, count, transpose) in uses {
        b.reference(seq, keys, *tick, riff, *count, *transpose)
            .unwrap();
    }
    (b.finish(), seq)
}

#[test]
fn nested_references_reshare() {
    // One sequence used three times, once looped twice.
    let (doc, seq) = riff_doc(&[(0, 1, 0), (2 * BAR, 2, 0), (6 * BAR, 1, 0)]);
    let t = trips_equal(&doc, seq, RegionGain::Apply);
    let uses = uses_on(&t, "Keys");
    assert_eq!(uses.len(), 3, "{uses:?}");
    assert!(
        uses.iter().all(|u| u.0 == uses[0].0),
        "one sequence: {uses:?}"
    );
    assert_eq!(
        uses.iter().map(|u| u.1).collect::<Vec<_>>(),
        vec![
            LoopCount::Count(1),
            LoopCount::Count(2),
            LoopCount::Count(1)
        ]
    );
    assert_eq!(
        t.doc.sequences().len(),
        2,
        "the project's and the shared one"
    );
}

#[test]
fn transposed_uses_stay_separate() {
    let (doc, seq) = riff_doc(&[(0, 1, 0), (2 * BAR, 1, 2), (4 * BAR, 1, 0)]);
    let t = trips_equal(&doc, seq, RegionGain::Apply);
    assert_eq!(
        count(
            &t.x.report,
            ItemKind::Note,
            core_io::dawproject::export::TRANSPOSED
        ),
        1
    );
    // The two plain uses share their sequence; the transposed use, written
    // as played, is a clip of its own and shares nothing with them.
    let uses = uses_on(&t, "Keys");
    assert_eq!(uses.len(), 2, "{uses:?}");
    assert_eq!(uses[0].0, uses[1].0);
    assert_eq!(t.doc.sequences().len(), 2);
}

#[test]
fn takes_and_vizdaw_only_state_are_listed() {
    use core_model::automation::AutomationMode;
    use core_model::command::SettingsCmd;
    use core_model::input_fx::{InputFx, InputFxKind};
    use core_model::lane::LaneInput;
    use core_model::learn::{LearnKind, LearnMap, LearnMode, LearnSource};
    use core_model::mpe::MpeConfig;
    use core_model::sequence::KeySlot;
    let mut b = Builder::new();
    let (seq, keys, _, _) = song(&mut b, "Keys");
    let (pad, _, _) = note_track(&mut b, seq, "Pad");
    let (drums, _, _) = note_track(&mut b, seq, "Drums");
    b.notes(seq, keys, &[(0, 64, 0x8000, Q)]).unwrap();
    // An inactive take, then a new active one.
    b.run(Command::Lane(LaneCmd::AddTake {
        seq,
        lane: keys,
        name: "Take 2".into(),
    }))
    .unwrap();
    b.notes(seq, keys, &[(0, 67, 0x8000, Q)]).unwrap();
    b.run(Command::Lane(LaneCmd::SetInputFx {
        seq,
        lane: keys,
        chain: vec![InputFx {
            kind: InputFxKind::Transpose { semitones: 2 },
            bypass: false,
        }],
    }))
    .unwrap();
    b.run(Command::Lane(LaneCmd::SetInput {
        seq,
        lane: pad,
        input: Some(LaneInput::Midi {
            port: None,
            channel: None,
        }),
    }))
    .unwrap();
    b.run(Command::Lane(LaneCmd::SetMpe {
        seq,
        lane: pad,
        mpe: Some(MpeConfig::default()),
    }))
    .unwrap();
    b.drum_rows(seq, drums, &[("Kick", 36), ("Snare", 38)])
        .unwrap();
    b.notes(seq, drums, &[(0, 36, 0x8000, Q), (Q, 38, 0x8000, Q)])
        .unwrap();
    let master = b.doc().mixer().master();
    b.mixer(MixerCmd::SetAutomationMode {
        strips: vec![master],
        mode: AutomationMode::Touch,
    })
    .unwrap();
    b.run(Command::Settings(SettingsCmd::SetLearn {
        map: LearnMap {
            source: LearnSource {
                port: "Knobs".into(),
                kind: LearnKind::Cc7 { channel: 0, cc: 7 },
            },
            target: AutomationTarget::StripFader(master),
            mode: LearnMode::Absolute,
            pickup: true,
        },
    }))
    .unwrap();
    b.run(Command::Sequence(SequenceCmd::SetMeter {
        seq,
        meter: MeterMap::new(vec![MeterEntry {
            tick: Tick(0),
            meter: Meter { num: 4, den: 4 },
            key: KeySig {
                sharps: 2,
                minor: false,
            },
        }])
        .unwrap(),
    }))
    .unwrap();
    let (other, _) = b.sequence("Other", "Track 1", None).unwrap();
    b.run(Command::Sequence(SequenceCmd::SetKeyboardSlot {
        seq: other,
        slot: Some(KeySlot {
            key: "a".into(),
            modifiers: 0,
        }),
    }))
    .unwrap();
    let doc = b.finish();
    let t = trips_equal(&doc, seq, RegionGain::Apply);
    for what in [
        "Inactive takes",
        "Input effects",
        "Track inputs and monitoring",
        "MPE input settings",
        "Drum rows (the notes are kept)",
        "Automation modes",
        "MIDI learn",
        "Key signatures",
        "Keyboard slots",
        "Other sequences (not exported)",
    ] {
        assert_eq!(count(&t.x.report, ItemKind::LeftOut, what), 1, "{what}");
    }
    // Only the active take comes back; its notes are the take-2 ones.
    let kept = t.doc.sequence(t.seq).unwrap();
    let lane = kept.lanes.iter().find(|l| l.name == "Keys").unwrap();
    assert_eq!(lane.takes.len(), 1);
}

#[test]
fn mpe_expression_survives() {
    // Exact per-note values at the ends and the middle of each range.
    let mut b = Builder::new();
    let (seq, keys, _, _) = song(&mut b, "Keys");
    let bends = [
        0u32,
        0x8000_0000,
        0x8000_0000 + 0x8000_0000 / 48,
        u32::MAX,
        1,
    ];
    let mut at = 0;
    for (i, bend) in bends.iter().enumerate() {
        let per_note = vec![
            PerNoteCtl {
                kind: PerNoteKind::PitchBend,
                offset: Tick(0),
                value: *bend,
            },
            PerNoteCtl {
                kind: PerNoteKind::Registered(74),
                offset: Tick(Q / 4),
                value: [0, 0x8000_0000, u32::MAX, 12_345_678, 7][i],
            },
            PerNoteCtl {
                kind: PerNoteKind::Registered(7),
                offset: Tick(Q / 2),
                value: [u32::MAX, 0, 0x4000_0000, 99, 0xFFFF_0000][i],
            },
            PerNoteCtl {
                kind: PerNoteKind::Registered(10),
                offset: Tick(Q / 2 + 1),
                value: [0x8000_0000, 1, 0, u32::MAX, 0x1234_5678][i],
            },
        ];
        b.events(
            seq,
            keys,
            vec![spec_at(
                at,
                EventBody::Note {
                    pitch: 60 + i as u8,
                    velocity: 0x8000,
                    release_velocity: 0,
                    duration: Tick(Q),
                    attribute: None,
                    per_note: per_note.into_iter().collect(),
                },
            )],
        )
        .unwrap();
        at += BAR;
    }
    let t = trips_equal(&b.finish(), seq, RegionGain::Apply);
    let expressive = projection(&t.doc, t.seq).unwrap().tracks[0]
        .notes_played
        .iter()
        .filter(|n| n.expression.len() == 4)
        .count();
    assert_eq!(expressive, bends.len());
}

#[test]
fn drum_track_keeps_its_notes() {
    let mut b = Builder::new();
    let (seq, drums, _, _) = song(&mut b, "Drums");
    b.drum_rows(seq, drums, &[("Kick", 36), ("Snare", 38), ("Hat", 42)])
        .unwrap();
    let mut notes = Vec::new();
    for bar in 0..2 {
        for (i, pitch) in [36u8, 42, 38, 42].iter().enumerate() {
            notes.push((bar * BAR + i as i64 * Q, *pitch, 0x7000 + i as u16, Q / 4));
        }
    }
    b.notes(seq, drums, &notes).unwrap();
    let t = trips_equal(&b.finish(), seq, RegionGain::Apply);
    assert_eq!(
        count(
            &t.x.report,
            ItemKind::LeftOut,
            "Drum rows (the notes are kept)"
        ),
        1
    );
    let n = projection(&t.doc, t.seq).unwrap().tracks[0]
        .notes_played
        .len();
    assert_eq!(n, 8);
}

#[test]
fn ramps_and_a_pickup() {
    let mut b = Builder::new();
    let (seq, keys, _, _) = song(&mut b, "Keys");
    b.notes(seq, keys, &[(-Q / 2, 60, 0x8000, Q), (Q, 62, 0x8000, Q)])
        .unwrap();
    b.tempo(
        seq,
        &[(0, 120, true), (BAR, 140, false), (3 * BAR, 90, true)],
    )
    .unwrap();
    b.run(Command::Sequence(SequenceCmd::SetStartOffset {
        seq,
        offset: Tick(Q),
    }))
    .unwrap();
    b.run(Command::Sequence(SequenceCmd::SetMarkers {
        seq,
        markers: vec![
            Marker {
                tick: Tick(0),
                name: "Verse".into(),
            },
            Marker {
                tick: Tick(2 * BAR),
                name: "Chorus".into(),
            },
        ]
        .into_iter()
        .collect(),
    }))
    .unwrap();
    let doc = b.finish();
    let t = trips_equal(&doc, seq, RegionGain::Apply);
    let a = projection(&doc, seq).unwrap();
    let i = projection(&t.doc, t.seq).unwrap();
    // The origin is applied: bar 1 of the original is a quarter into the import.
    assert_eq!(a.origin, -Q);
    assert_eq!(
        i.markers,
        vec![(Q, "Verse".to_owned()), (Q + 2 * BAR, "Chorus".to_owned())]
    );
    assert_eq!(i.meter, vec![(0, 1, 4), (Q, 4, 4)]);
    assert_eq!(
        i.tempo
            .iter()
            .filter(|p| p.0 >= Q)
            .map(|p| (p.0 - Q, p.1, p.2))
            .collect::<Vec<_>>(),
        a.tempo
    );
    assert_eq!(
        count(
            &t.x.report,
            ItemKind::Note,
            "The song starts before bar 1; other DAWs show it from the pickup's start."
        ),
        1
    );
}

#[test]
fn shared_instrument_is_copied() {
    let mut b = Builder::new();
    let (dest, slot) = b.plugin_dest("Synth", clap("org.example.synth")).unwrap();
    let (seq, a) = b.sequence("Song", "Lead", Some(dest)).unwrap();
    let c = b.lane(seq, "Pad", Some(dest)).unwrap();
    b.notes(seq, a, &[(0, 60, 0x8000, Q)]).unwrap();
    b.notes(seq, c, &[(Q, 64, 0x8000, Q)]).unwrap();
    let b = capture(b, slot, b"shared settings");
    let doc = b.finish();
    let t = trip(&doc, &options(seq, RegionGain::Apply));
    assert_eq!(
        count(
            &t.x.report,
            ItemKind::Changed,
            "Instrument 'Synth' shared by several tracks was copied to each"
        ),
        1
    );
    // Two tracks, each on an instrument of its own with the same state.
    let want = core_model::state::blob_id(b"shared settings");
    let mut seen = Vec::new();
    for lane in ["Lead", "Pad"] {
        let l = t.doc.sequence(t.seq).unwrap();
        let l = l.lanes.iter().find(|l| l.name == lane).unwrap();
        let dest = l.midi_dest.expect("a destination");
        let inst = t.doc.instruments().get(&dest).expect("its instrument");
        let core_model::document::DestKind::PluginInstrument { slot } = &inst.kind else {
            panic!("a plug-in instrument")
        };
        assert_eq!(slot.state, Some(want), "{lane}");
        seen.push(dest);
    }
    assert_ne!(seen[0], seen[1], "each track has its own copy");
    // Each copy keeps the instrument's own name, not its track's.
    for dest in &seen {
        assert_eq!(t.doc.instruments().get(dest).unwrap().name, "Synth");
    }
    let pa = projection(&doc, seq).unwrap();
    let pb = projection(&t.doc, t.seq).unwrap();
    assert_eq!(
        pa.tracks
            .iter()
            .map(|t| &t.notes_played)
            .collect::<Vec<_>>(),
        pb.tracks
            .iter()
            .map(|t| &t.notes_played)
            .collect::<Vec<_>>()
    );
    let names = |p: &Projection| p.strips.iter().map(|s| s.name.clone()).collect::<Vec<_>>();
    assert_eq!(names(&pb), names(&pa), "strip names exact");
    assert_eq!(names(&pb)[..2], ["Synth", "Synth"]);
    assert_eq!(
        pb.strips
            .iter()
            .map(|s| s.devices.clone())
            .collect::<Vec<_>>()[..2],
        pa.strips
            .iter()
            .map(|s| s.devices.clone())
            .collect::<Vec<_>>()[..2]
    );
}

#[test]
fn stereo_and_mono_media() {
    let mut b = Builder::new();
    let (seq, _, _, _) = song(&mut b, "Keys");
    let (mono_lane, _) = audio_track(&mut b, seq, "Mono");
    let (wide_lane, _) = audio_track(&mut b, seq, "Wide");
    let mono = media(&mut b, "mono", 96_000, 1);
    let wide = media(&mut b, "wide", 144_000, 2);
    b.region(
        seq,
        mono_lane,
        0,
        mono,
        100,
        48_000,
        0.0,
        plain_fade(480),
        plain_fade(0),
    )
    .unwrap();
    b.region(
        seq,
        wide_lane,
        BAR,
        wide,
        0,
        144_000,
        0.0,
        plain_fade(0),
        plain_fade(960),
    )
    .unwrap();
    b.region(
        seq,
        wide_lane,
        6 * BAR,
        mono,
        2_000,
        24_000,
        0.0,
        plain_fade(0),
        plain_fade(0),
    )
    .unwrap();
    let t = trips_equal(&b.finish(), seq, RegionGain::Apply);
    let channels: Vec<(String, u16, u64)> = t
        .doc
        .media()
        .values()
        .map(|m| (m.name.clone(), m.channels, m.length_frames))
        .collect();
    assert!(
        channels.contains(&("mono".to_owned(), 1, 96_000)),
        "{channels:?}"
    );
    assert!(
        channels.contains(&("wide".to_owned(), 2, 144_000)),
        "{channels:?}"
    );
}

#[test]
fn crossfades_survive() {
    let mut b = Builder::new();
    let (seq, _, _, _) = song(&mut b, "Keys");
    let (lane, _) = audio_track(&mut b, seq, "Drums");
    let m = media(&mut b, "loop", 96_000, 2);
    b.region(
        seq,
        lane,
        0,
        m,
        0,
        96_000,
        0.0,
        plain_fade(0),
        plain_fade(0),
    )
    .unwrap();
    let second = b
        .region(
            seq,
            lane,
            Q,
            m,
            0,
            96_000,
            0.0,
            plain_fade(0),
            plain_fade(0),
        )
        .unwrap();
    b.run(Command::Audio(AudioCmd::Crossfade {
        event: second,
        length: CrossfadeLength::Frames(4_800),
        shape: FadeShape::EqualPower,
    }))
    .unwrap();
    let doc = b.finish();
    let t = trips_equal(&doc, seq, RegionGain::Apply);
    let regions = projection(&t.doc, t.seq)
        .unwrap()
        .tracks
        .iter()
        .find(|x| x.name == "Drums")
        .unwrap()
        .regions
        .clone();
    assert_eq!(regions.len(), 2);
    assert!(regions[1].crossfade && !regions[0].crossfade);
    assert_eq!(regions[1].fade_in, 4_800);
    // The shape is not carried: the file keeps lengths only.
    let s = t.doc.sequence(t.seq).unwrap();
    let l = s.lanes.iter().find(|l| l.name == "Drums").unwrap();
    let shapes: Vec<FadeShape> = l
        .active()
        .unwrap()
        .events
        .values()
        .filter_map(|e| match &e.body {
            EventBody::AudioRegion {
                fade_in,
                crossfade: true,
                ..
            } => Some(fade_in.shape),
            _ => None,
        })
        .collect();
    assert_eq!(shapes, vec![FadeShape::Linear]);
    assert_eq!(
        count(
            &t.x.report,
            ItemKind::Changed,
            core_io::dawproject::export::FADE_SHAPES
        ),
        1
    );
}

/// `riff_doc` with a fill use of the riff at tick 0, `length` long.
fn fill_doc(length: i64) -> (Document, SeqId) {
    use core_model::command::ReferenceCmd;
    let (doc, seq) = riff_doc(&[]);
    let mut b = Builder::from_doc(doc);
    let riff = *b
        .doc()
        .sequences()
        .keys()
        .find(|k| **k != seq)
        .expect("the riff");
    let keys = b.doc().sequence(seq).unwrap().lanes.front().unwrap().id;
    b.run(Command::Reference(ReferenceCmd::Insert {
        seq,
        lane: keys,
        tick: Tick(0),
        target: riff,
        length: Some(Tick(length)),
        loop_count: LoopCount::Fill,
        transpose: 0,
    }))
    .unwrap();
    (b.finish(), seq)
}

#[test]
fn fill_uses_are_written_as_notes() {
    // A fill that ends inside a pass has no loop to write: the notes of
    // every pass are, and they come back as notes on the track, not as a use.
    let (doc, seq) = fill_doc(2 * BAR + 2 * Q);
    let t = trips_equal(&doc, seq, RegionGain::Apply);
    assert!(uses_on(&t, "Keys").is_empty());
    let n = projection(&t.doc, t.seq).unwrap().tracks[0]
        .notes_played
        .len();
    assert_eq!(n, 6, "two notes in each of three passes");
    // A fill of whole passes is a loop in the file, and a counted use again.
    let (doc, seq) = fill_doc(3 * BAR);
    let t = trips_equal(&doc, seq, RegionGain::Apply);
    let uses = uses_on(&t, "Keys");
    assert_eq!(
        uses.iter().map(|u| u.1).collect::<Vec<_>>(),
        vec![LoopCount::Count(3)]
    );
}

#[test]
fn group_channels_are_written_as_group_one() {
    let mut b = Builder::new();
    let (seq, keys, _, _) = song(&mut b, "Keys");
    let on = |tick: i64, pitch: u8, group_channel: Option<(u8, u8)>| {
        let mut s = spec_at(tick, EventBody::note(pitch, 0x8000, Tick(Q / 2)));
        s.group_channel = group_channel;
        s
    };
    b.events(
        seq,
        keys,
        vec![
            on(0, 60, None),
            on(Q, 61, None),
            on(2 * Q, 62, Some((0, 5))),
            on(3 * Q, 63, Some((1, 5))),
            on(4 * Q, 64, Some((15, 9))),
            on(5 * Q, 65, None),
        ],
    )
    .unwrap();
    let t = trips_equal(&b.finish(), seq, RegionGain::Apply);
    assert_eq!(
        count(
            &t.x.report,
            ItemKind::Changed,
            core_io::dawproject::export::MIDI_GROUPS
        ),
        2
    );
    let channels: Vec<u8> = projection(&t.doc, t.seq).unwrap().tracks[0]
        .notes_played
        .iter()
        .map(|n| n.channel)
        .collect();
    assert_eq!(channels, vec![0, 0, 5, 5, 9, 0]);

    // When another channel has the most notes it is the track's main one,
    // plays on the instrument's channel, and the import says so.
    let mut b = Builder::new();
    let (seq, keys, _, _) = song(&mut b, "Keys");
    b.events(
        seq,
        keys,
        vec![
            on(0, 60, Some((0, 5))),
            on(Q, 61, Some((0, 5))),
            on(2 * Q, 62, None),
        ],
    )
    .unwrap();
    let (imported, _, import) = round_trip(&b.finish(), &options(seq, RegionGain::Apply));
    assert_eq!(
        count(
            &import,
            ItemKind::Changed,
            core_io::dawproject::import::MAIN_CHANNEL
        ),
        1
    );
    let notes = projection(&imported, new_project().1).unwrap().tracks[0]
        .notes_played
        .len();
    assert_eq!(notes, 3);
}

#[test]
fn a_thousand_tracks() {
    let mut b = Builder::new();
    let (seq, first) = b.sequence("Song", "T0", None).unwrap();
    let mut lanes = vec![first];
    for i in 1..1_000 {
        lanes.push(b.lane(seq, &format!("T{i}"), None).unwrap());
    }
    for (i, lane) in lanes.iter().enumerate() {
        let notes: Vec<(i64, u8, u16, i64)> = (0..10)
            .map(|n| (n * Q, 40 + (i % 40) as u8, 0x8000, Q / 2))
            .collect();
        b.notes(seq, *lane, &notes).unwrap();
    }
    let doc = b.finish();
    let start = std::time::Instant::now();
    let t = trip_without_schema(&doc, &options(seq, RegionGain::Apply));
    let took = start.elapsed();
    println!("1 000 tracks: export and import in {took:?}");
    assert!(took.as_secs() < 10, "{took:?}");
    check_schema(&t.x);
    let p = projection(&t.doc, t.seq).unwrap();
    assert_eq!(p.tracks.len(), 1_000);
    assert!(p.tracks.iter().all(|x| x.notes_played.len() == 10));
    let names: Vec<&str> = p.tracks.iter().map(|x| x.name.as_str()).collect();
    assert_eq!(names[0], "T0");
    assert_eq!(names[999], "T999");
}

// ---------------------------------------------------------------- the properties

fn gain_of(s: &Spec) -> RegionGain {
    if s.gain {
        RegionGain::Apply
    } else {
        RegionGain::LeaveOut
    }
}

/// Asserts that the export's report holds only lines `s` can give, and the
/// import's only the "from VizDAW" note.
#[track_caller]
fn only_expected(s: &Spec, t: &Trip) {
    use core_io::dawproject::export::{
        CROSSFADES, FADE_SHAPES, MIDI_GROUPS, PLAY_QUANTIZED, REGION_GAIN,
    };
    let quantized = s.notes.iter().any(|t| t.quantize.is_some());
    let groups = s
        .notes
        .iter()
        .flat_map(|t| &t.notes)
        .any(|n| n.channel.is_some_and(|c| c.0 > 0));
    let plain = |f: &dyn Fn(&Slot) -> bool| s.audio.iter().flat_map(|a| &a.slots).any(f);
    let pairs = plain(&|x| matches!(x, Slot::Pair { .. }));
    let shapes = plain(&|x| match x {
        Slot::Plain {
            fade_in, fade_out, ..
        } => (fade_in.0 > 0 && fade_in.1 > 0) || (fade_out.0 > 0 && fade_out.1 > 0),
        Slot::Pair { shape, .. } => *shape > 0,
    });
    let gains = !s.gain && plain(&|x| matches!(x, Slot::Plain { gain, .. } if *gain != 0.0));
    let unsaved = s.notes.iter().any(|t| t.strip.state.is_none())
        || s.notes
            .iter()
            .any(|t| t.strip.insert.as_ref().is_some_and(|i| i.0.is_none()));
    for i in &t.x.report.items {
        let ok = match (i.kind, i.what.as_str()) {
            (ItemKind::Changed, w) if w == MIDI_GROUPS => groups,
            (ItemKind::Changed, w) if w == FADE_SHAPES => shapes,
            (ItemKind::Changed, w) if w == CROSSFADES => pairs,
            (ItemKind::Changed, w) if w == PLAY_QUANTIZED => quantized,
            (ItemKind::LeftOut, w) if w == REGION_GAIN => gains,
            (ItemKind::Note, w) if w.starts_with("The song starts before bar 1") => {
                s.pickup_beats > 0
            }
            (ItemKind::Note, w)
                if w.starts_with("Plug-in '") && w.ends_with("has no saved settings") =>
            {
                unsaved
            }
            _ => false,
        };
        assert!(ok, "an unexpected export line {:?} '{}'", i.kind, i.what);
    }
    for i in &t.import.items {
        assert!(
            i.kind == ItemKind::Note && i.what == core_io::dawproject::import::FROM_VIZDAW,
            "an unexpected import line {:?} '{}'",
            i.kind,
            i.what
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(common::cases(64)))]

    #[test]
    fn round_trips_preserve_the_projection(s in spec()) {
        let (doc, seq) = build(&s);
        let gain = gain_of(&s);
        let t = trip(&doc, &options(seq, gain));
        let (a, b) = projections(&doc, seq, &t);
        same(&a, &b, gain);
        only_expected(&s, &t);
    }
}

#[test]
fn every_export_passes_the_schema() {
    use proptest::strategy::ValueTree;
    use proptest::test_runner::TestRunner;
    // 16 specs from a fixed seed: both files of each against the vendored
    // XSDs (every trip above does the same for the ones it makes).
    let mut runner = TestRunner::deterministic();
    let strategy = spec();
    for _ in 0..16 {
        let s = strategy.new_tree(&mut runner).unwrap().current();
        let (doc, seq) = build(&s);
        let x = export(
            &doc,
            &options(seq, gain_of(&s)),
            &env(),
            &AtomicBool::new(false),
        )
        .expect("exports");
        check_schema(&x);
    }
}
