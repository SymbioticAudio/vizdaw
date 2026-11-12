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

use common::package::{TestPlist, write_package};
use common::xmllint::schema_check;
use core_engine::fixtures::{BAR, Builder, Q};
use core_io::dawproject::export::{
    ClipRule, DpExport, ExportEnv, ExportOptions, RegionGain, export,
};
use core_io::dawproject::import::{ImportEnv, MediaSource, apply, map};
use core_io::dawproject::package::{AudioSource, open};
use core_io::dawproject::record::{Projection, projection};
use core_io::limits::PackageLimits;
use core_io::report::{ItemKind, Report};
use core_media::import::Imported;
use core_model::command::{Command, LaneCmd, SequenceCmd};
use core_model::document::Document;
use core_model::history::History;
use core_model::ids::{AnyId, SeqId};
use core_model::mixer::PluginFormat;
use core_model::selection::ViewFilter;
use core_model::time::Tick;
use std::collections::BTreeMap;
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

/// The content hash of a copy of `hash` with `gain_db` applied.
fn copy_hash(hash: &str, gain_db: f32) -> String {
    if gain_db == 0.0 {
        hash.to_owned()
    } else {
        format!("{hash}@{gain_db}")
    }
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
    let mut media = BTreeMap::new();
    for a in pkg.audio() {
        let AudioSource::Entry(entry) = &a.source else {
            panic!("an exported file is outside the package")
        };
        let file = x
            .audio
            .iter()
            .find(|f| &f.entry == entry)
            .unwrap_or_else(|| panic!("no audio file {entry}"));
        let item = doc.media().get(&file.media).expect("the media item");
        let hash = item.content_hash.clone().unwrap_or_default();
        media.insert(
            a.key,
            MediaSource::Online(vec![Imported {
                path: format!("Media/{}.caf", a.name),
                name: file.name.clone(),
                original_name: item.name.clone(),
                channels: item.channels,
                sample_rate: item.sample_rate,
                length_frames: item.length_frames,
                content_hash: copy_hash(&hash, file.gain_db),
                clipped: 0,
            }]),
        );
    }
    let installed = |_: PluginFormat, _: &str| true;
    let imp = map(
        &pkg,
        &media,
        &ImportEnv {
            installed: &installed,
            plist: &TestPlist,
            rate: 48_000,
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

/// Asserts that `imported` is `original` within the tolerances. `gain` is
/// the export's region-gain option; `copies` the hashes of the gain copies
/// the mapping gave the regions.
#[track_caller]
fn same(original: &Projection, imported: &Projection, gain: RegionGain) {
    let shift = -original.origin;
    assert_eq!(imported.origin, 0, "an import has no pickup");
    assert_eq!(original.info, imported.info, "song info");
    assert_eq!(
        original
            .markers
            .iter()
            .map(|(t, n)| (t + shift, n.clone()))
            .collect::<Vec<_>>(),
        imported.markers,
        "markers"
    );
    // The import has no pickup: what plays before the original's tick 0 is
    // its own first (held) entries. The map from there on is compared; a
    // value held across that tick counts as an entry on it.
    // A ramp on the last point leads nowhere, so it is not compared.
    let ends_flat = |mut v: Vec<(i64, u32, bool)>| {
        if let Some(last) = v.last_mut() {
            last.2 = false;
        }
        v
    };
    assert_eq!(
        ends_flat(
            original
                .tempo
                .iter()
                .map(|(t, b, r)| (t + shift, *b, *r))
                .collect()
        ),
        ends_flat(from_tick(
            &imported.tempo,
            shift,
            |p| p.0,
            |p, at| (at, p.1, p.2)
        )),
        "tempo map"
    );
    assert_eq!(
        original
            .meter
            .iter()
            .map(|(t, n, d)| (t + shift, *n, *d))
            .collect::<Vec<_>>(),
        from_tick(&imported.meter, shift, |p| p.0, |p, at| (at, p.1, p.2)),
        "meter map"
    );
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
        // A note's expression is a set: the order the document holds it in
        // is not part of what is carried.
        let moved = |n: &core_io::dawproject::record::PNote, by: i64| {
            let mut n = n.clone();
            n.tick += by;
            n.expression.sort_by(|x, y| (x.1, &x.0).cmp(&(y.1, &y.0)));
            n
        };
        assert_eq!(
            a.notes_played
                .iter()
                .map(|n| moved(n, shift))
                .collect::<Vec<_>>(),
            b.notes_played
                .iter()
                .map(|n| moved(n, 0))
                .collect::<Vec<_>>(),
            "{at}: notes"
        );
        assert_eq!(
            a.midi
                .iter()
                .map(|(t, m)| (t + shift, m.clone()))
                .collect::<Vec<_>>(),
            b.midi,
            "{at}: MIDI events"
        );
        assert_eq!(a.regions.len(), b.regions.len(), "{at}: regions");
        for (r, s) in a.regions.iter().zip(&b.regions) {
            let mut want = r.clone();
            want.tick += shift;
            match gain {
                RegionGain::Apply => {
                    want.media_hash = want.media_hash.map(|h| copy_hash(&h, r.gain_db));
                    want.gain_db = 0.0;
                }
                RegionGain::LeaveOut => want.gain_db = 0.0,
            }
            assert_eq!(&want, s, "{at}: region");
        }
        assert_eq!(a.automation.len(), b.automation.len(), "{at}: automation");
        for ((na, pa), (nb, pb)) in a.automation.iter().zip(&b.automation) {
            assert_eq!(na, nb, "{at}: automation target");
            let pb = without_holds(pb);
            assert_eq!(pa.len(), pb.len(), "{at}: {na} points");
            for ((ta, va), (tb, vb)) in pa.iter().zip(&pb) {
                assert_eq!(ta + shift, *tb, "{at}: {na} tick");
                near(*va, *vb, &format!("{at}: {na}"));
            }
        }
    }
}

/// The entries of a map from tick `at` on; the entry in force there counts
/// as one on it.
fn from_tick<T: Copy>(
    map: &[T],
    at: i64,
    tick: impl Fn(&T) -> i64,
    on: impl Fn(&T, i64) -> T,
) -> Vec<T> {
    let mut out: Vec<T> = map.iter().filter(|p| tick(p) >= at).copied().collect();
    if out.first().is_none_or(|p| tick(p) > at)
        && let Some(held) = map.iter().rev().find(|p| tick(p) < at)
    {
        out.insert(0, on(held, at));
    }
    out
}

/// `points` without the one-tick hold points the import adds after a step.
fn without_holds(points: &[(i64, f64)]) -> Vec<(i64, f64)> {
    let mut out: Vec<(i64, f64)> = Vec::new();
    for p in points {
        if let Some(last) = out.last() {
            if p.0 == last.0 + 1 && p.1 == last.1 {
                continue;
            }
        }
        out.push(*p);
    }
    out
}

/// The trip's projections: the original's and the imported one's.
fn projections(doc: &Document, seq: SeqId, t: &Trip) -> (Projection, Projection) {
    (
        projection(doc, seq).expect("projects"),
        projection(&t.doc, t.seq).expect("projects the import"),
    )
}

// ---------------------------------------------------------------- tests

// ---------------------------------------------------------------- building

use core_model::automation::{AutomationTarget, SendIndex};
use core_model::command::{AudioCmd, CrossfadeLength, EventCmd, MediaCmd, MixerCmd, RoutingCmd};
use core_model::document::{MediaOrigin, SongInfo};
use core_model::event::{
    EventBody, EventSpec, Fade, FadeShape, LoopCount, PerNoteCtl, PerNoteKind,
};
use core_model::ids::{DestId, LaneId, MediaId, OutputPathId, PluginInstanceId, StripId};
use core_model::meter::{KeySig, Meter, MeterEntry, MeterMap};
use core_model::mixer::{AuxSend, ParamId, PluginRef, StripKind, Swatch};
use core_model::sequence::{Marker, SeqLength};

fn clap(id: &str) -> PluginRef {
    PluginRef {
        format: PluginFormat::Clap,
        id: id.into(),
        name: id.into(),
        vendor: "Example".into(),
        version: "1.0".into(),
    }
}

fn output(b: &mut Builder, name: &str) -> OutputPathId {
    let r = b
        .run(Command::Routing(RoutingCmd::AddOutput {
            name: name.into(),
            reference: format!("hw:{name}"),
        }))
        .unwrap();
    r.created
        .iter()
        .find_map(|i| match i {
            AnyId::Output(o) => Some(*o),
            _ => None,
        })
        .unwrap()
}

fn route(b: &mut Builder, seq: SeqId, lane: LaneId, out: OutputPathId) {
    b.run(Command::Lane(LaneCmd::SetOutput {
        seq,
        lane,
        output: Some(out),
    }))
    .unwrap();
}

fn spec_at(tick: i64, body: EventBody) -> EventSpec {
    EventSpec::new(Tick(tick), body)
}

/// The sequence of a new song with a note track "name" on its own CLAP
/// instrument (named the same: the import names an instrument after its
/// track). Returns the instrument's strip and plug-in instance.
fn song(b: &mut Builder, name: &str) -> (SeqId, LaneId, DestId, PluginInstanceId) {
    let (dest, slot) = b
        .plugin_dest(name, clap(&format!("org.example.{name}")))
        .unwrap();
    let (seq, lane) = b.sequence("Song", name, Some(dest)).unwrap();
    (seq, lane, dest, slot)
}

/// A further note track "name" on its own instrument.
fn note_track(b: &mut Builder, seq: SeqId, name: &str) -> (LaneId, DestId, PluginInstanceId) {
    let (dest, slot) = b
        .plugin_dest(name, clap(&format!("org.example.{name}")))
        .unwrap();
    let lane = b.lane(seq, name, Some(dest)).unwrap();
    (lane, dest, slot)
}

/// An audio track "name" on an output path of the same name; returns its
/// lane and strip.
fn audio_track(b: &mut Builder, seq: SeqId, name: &str) -> (LaneId, StripId) {
    let lane = b.lane(seq, name, None).unwrap();
    let out = output(b, name);
    route(b, seq, lane, out);
    (lane, b.strip_of(StripKind::AudioPath(out)).unwrap())
}

/// A media item with a content hash.
fn media(b: &mut Builder, name: &str, frames: u64, channels: u16) -> MediaId {
    let r = b
        .run(Command::Media(MediaCmd::Add {
            path: format!("Media/{name}.caf"),
            sample_rate: 48_000,
            channels,
            length_frames: frames,
            content_hash: Some(format!("hash-{name}")),
            name: Some(name.into()),
            origin: Some(MediaOrigin::Imported {
                original_name: format!("{name}.wav"),
            }),
        }))
        .unwrap();
    r.created
        .iter()
        .find_map(|i| match i {
            AnyId::Media(m) => Some(*m),
            _ => None,
        })
        .unwrap()
}

fn lock(b: &mut Builder, seq: SeqId, ticks: i64) {
    b.run(Command::Sequence(SequenceCmd::SetLength {
        seq,
        length: SeqLength::Locked(Tick(ticks)),
    }))
    .unwrap();
}

fn capture(b: Builder, slot: PluginInstanceId, bytes: &[u8]) -> Builder {
    let (doc, _) = core_model::state::capture(b.doc(), slot, bytes).unwrap();
    Builder::from_doc(doc)
}

fn plain_fade(frames: u64) -> Fade {
    Fade {
        length_frames: frames,
        shape: FadeShape::Linear,
    }
}

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

// ---------------------------------------------------------------- the generator

use proptest::prelude::*;

/// A fader/pan/mute/send/parameter the generator automates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Auto {
    Fader,
    Pan,
    Mute,
    Send,
    Param,
}

#[derive(Debug, Clone)]
struct StripSpec {
    /// Fader in half dB.
    gain: i32,
    /// Pan in sixteenths.
    pan: i32,
    mute: bool,
    solo: bool,
    colour: Option<usize>,
    /// Send to the bus: half dB, pre-fader, enabled.
    send: Option<(i32, bool, bool)>,
    /// An effect: its state, and whether it is bypassed.
    insert: Option<(Option<Vec<u8>>, bool)>,
    /// The instrument's state.
    state: Option<Vec<u8>>,
}

#[derive(Debug, Clone)]
struct NoteSpec {
    tick: i64,
    pitch: u8,
    velocity: u16,
    release: u16,
    duration: i64,
    channel: Option<(u8, u8)>,
    /// (kind 0..4, offset, value).
    expression: Vec<(u8, i64, u32)>,
}

#[derive(Debug, Clone)]
enum Midi {
    Cc(u8, u32),
    Bend(u32),
    Pressure(u32),
    Poly(u8, u32),
    Program(u8, Option<(u8, u8)>),
}

#[derive(Debug, Clone)]
struct NoteTrack {
    strip: StripSpec,
    notes: Vec<NoteSpec>,
    midi: Vec<(i64, Midi)>,
    automation: Vec<(Auto, Vec<(i64, f64)>)>,
}

#[derive(Debug, Clone)]
enum Slot {
    Plain {
        offset: u64,
        length: u64,
        gain: f32,
        fade_in: (u64, usize),
        fade_out: (u64, usize),
        name: Option<String>,
        muted: bool,
        wide: bool,
    },
    /// Two long regions of the stereo item, the second crossfading in.
    Pair { fade: u64, shape: usize },
}

#[derive(Debug, Clone)]
struct AudioTrack {
    strip: StripSpec,
    slots: Vec<Slot>,
    automation: Vec<(Auto, Vec<(i64, f64)>)>,
}

#[derive(Debug, Clone)]
struct Spec {
    notes: Vec<NoteTrack>,
    audio: Vec<AudioTrack>,
    bus: Option<StripSpec>,
    master_gain: i32,
    meter: (u8, u8),
    change: Option<(i64, u8, u8)>,
    tempo: Vec<(i64, u32, bool)>,
    pickup_beats: u8,
    markers: Vec<(i64, String)>,
    info: Vec<(usize, String)>,
    gain: bool,
}

const SWATCHES: [Swatch; 8] = [
    Swatch::Red,
    Swatch::Orange,
    Swatch::Yellow,
    Swatch::Green,
    Swatch::Teal,
    Swatch::Blue,
    Swatch::Purple,
    Swatch::Pink,
];
const SHAPES: [FadeShape; 4] = [
    FadeShape::Linear,
    FadeShape::EqualPower,
    FadeShape::Exponential,
    FadeShape::SCurve,
];

fn word() -> impl Strategy<Value = String> {
    "[A-Z][a-z]{1,6}( [a-z]{1,6}){0,2}"
}

fn strip_spec(with_state: bool, with_insert: bool) -> impl Strategy<Value = StripSpec> {
    (
        -120i32..=24,
        -16i32..=16,
        any::<bool>(),
        any::<bool>(),
        proptest::option::of(0usize..8),
        proptest::option::of((-120i32..=12, any::<bool>(), any::<bool>())),
        proptest::option::of((
            proptest::option::of(proptest::collection::vec(any::<u8>(), 0..32)),
            any::<bool>(),
        )),
        proptest::option::of(proptest::collection::vec(any::<u8>(), 0..32)),
    )
        .prop_map(
            move |(gain, pan, mute, solo, colour, send, insert, state)| StripSpec {
                gain,
                pan,
                mute,
                solo,
                colour,
                send,
                insert: if with_insert { insert } else { None },
                state: if with_state { state } else { None },
            },
        )
}

fn midi() -> impl Strategy<Value = Midi> {
    // No CC 0 or 32 (they are a program's bank) and no data entry.
    let index =
        (1u8..120).prop_filter("plain controller", |i| !matches!(i, 6 | 32 | 38 | 98..=101));
    prop_oneof![
        (index, any::<u32>()).prop_map(|(i, v)| Midi::Cc(i, v)),
        any::<u32>().prop_map(Midi::Bend),
        any::<u32>().prop_map(Midi::Pressure),
        (0u8..128, any::<u32>()).prop_map(|(p, v)| Midi::Poly(p, v)),
        (0u8..128, proptest::option::of((0u8..128, 0u8..128)))
            .prop_map(|(p, b)| Midi::Program(p, b)),
    ]
}

/// Up to `n` automation lanes of the kinds in `kinds`, points at distinct
/// whole beats.
fn automation(kinds: Vec<Auto>) -> impl Strategy<Value = Vec<(Auto, Vec<(i64, f64)>)>> {
    proptest::collection::vec(
        (
            proptest::sample::select(kinds),
            proptest::collection::btree_map(0i64..20, 0u32..=64, 1..5),
        ),
        0..4,
    )
    .prop_map(|lanes| {
        let mut seen = std::collections::BTreeSet::new();
        lanes
            .into_iter()
            .filter(|(k, _)| seen.insert(*k as u8))
            .map(|(k, pts)| {
                let pts = pts
                    .into_iter()
                    .map(|(beat, v)| {
                        let v = if k == Auto::Mute {
                            f64::from(v % 2)
                        } else {
                            f64::from(v) / 64.0
                        };
                        (beat * Q, v)
                    })
                    .collect();
                (k, pts)
            })
            .collect()
    })
}

fn note_track_spec(pickup: i64) -> impl Strategy<Value = NoteTrack> {
    let note = (
        -pickup..24 * Q,
        0u8..128,
        1u16..=u16::MAX,
        any::<u16>(),
        1i64..2 * Q,
        proptest::option::weighted(0.15, (0u8..3, 1u8..16)),
        proptest::collection::vec((0u8..4, 0i64..1_000, any::<u32>()), 0..3),
    )
        .prop_map(
            |(tick, pitch, velocity, release, duration, channel, expression)| NoteSpec {
                tick,
                pitch,
                velocity,
                release,
                duration,
                channel,
                expression,
            },
        );
    (
        strip_spec(true, true),
        proptest::collection::vec(note, 1..16),
        proptest::collection::vec((-pickup..24 * Q, midi()), 0..8),
        automation(vec![
            Auto::Fader,
            Auto::Pan,
            Auto::Mute,
            Auto::Send,
            Auto::Param,
        ]),
    )
        .prop_map(|(strip, notes, midi, automation)| {
            let mut notes = notes;
            notes.sort_by_key(|n| n.tick);
            // No two notes of a pitch overlap; offsets stay inside notes;
            // each expression kind once per offset; the track's main
            // channel (0) has more notes than any other.
            let mut free: BTreeMap<u8, i64> = BTreeMap::new();
            notes.retain(|n| {
                if free.get(&n.pitch).is_some_and(|f| *f > n.tick) {
                    return false;
                }
                free.insert(n.pitch, n.tick + n.duration);
                true
            });
            for n in &mut notes {
                n.expression.iter_mut().for_each(|e| e.1 %= n.duration);
                n.expression.sort_by_key(|e| (e.0, e.1));
                n.expression.dedup_by_key(|e| (e.0, e.1));
            }
            let total = notes.len();
            let mut kept = 0;
            for n in &mut notes {
                if n.channel.is_some() {
                    kept += 1;
                    if kept * 2 >= total {
                        n.channel = None;
                    }
                }
            }
            let mut midi = midi;
            midi.sort_by_key(|m| m.0);
            midi.dedup_by_key(|m| {
                (
                    m.0,
                    match &m.1 {
                        Midi::Cc(i, _) => 1000 + i32::from(*i),
                        Midi::Bend(_) => 1,
                        Midi::Pressure(_) => 2,
                        Midi::Poly(p, _) => 2000 + i32::from(*p),
                        Midi::Program(..) => 3,
                    },
                )
            });
            NoteTrack {
                strip,
                notes,
                midi,
                automation,
            }
        })
}

fn audio_track_spec() -> impl Strategy<Value = AudioTrack> {
    let plain = (
        0u64..1_000,
        480u64..=24_000,
        prop_oneof![Just(0.0f32), Just(-6.0), Just(3.0), Just(-12.5)],
        (0u64..240, 0usize..4),
        (0u64..240, 0usize..4),
        proptest::option::of(word()),
        any::<bool>(),
        any::<bool>(),
    )
        .prop_map(
            |(offset, length, gain, fade_in, fade_out, name, muted, wide)| Slot::Plain {
                offset,
                length,
                gain,
                fade_in,
                fade_out,
                name,
                muted,
                wide,
            },
        );
    let pair = (480u64..4_800, 0usize..4).prop_map(|(fade, shape)| Slot::Pair { fade, shape });
    (
        strip_spec(false, false),
        proptest::collection::vec(prop_oneof![4 => plain, 1 => pair], 1..4),
        automation(vec![Auto::Fader, Auto::Pan, Auto::Mute, Auto::Send]),
    )
        .prop_map(|(strip, slots, automation)| AudioTrack {
            strip,
            slots,
            automation,
        })
}

fn spec() -> impl Strategy<Value = Spec> {
    (
        (2u8..8, prop_oneof![Just(4u8), Just(8u8)]),
        proptest::option::of((1i64..5, 2u8..8, prop_oneof![Just(4u8), Just(8u8)])),
        proptest::collection::vec((1i64..32 * Q, 30u32..=240, any::<bool>()), 0..4),
        (30u32..=240, any::<bool>()),
        0u8..8,
    )
        .prop_flat_map(|(meter, change, points, (bpm0, ramp0), pickup)| {
            let pickup_beats = pickup % meter.0;
            let beat = 4 * Q / i64::from(meter.1);
            let pickup_ticks = i64::from(pickup_beats) * beat;
            let bar = beat * i64::from(meter.0);
            let change = change
                .filter(|(_, n, d)| (*n, *d) != meter)
                .map(|(k, n, d)| (k * bar, n, d));
            let mut tempo = vec![(0, bpm0, ramp0)];
            let mut points = points;
            points.sort_by_key(|p| p.0);
            points.dedup_by_key(|p| p.0);
            tempo.extend(points);
            (
                proptest::collection::vec(note_track_spec(pickup_ticks), 1..4),
                proptest::collection::vec(audio_track_spec(), 0..3),
                proptest::option::of(strip_spec(false, false)),
                -120i32..=24,
                proptest::collection::vec((-pickup_ticks..24 * Q, word()), 0..4),
                proptest::collection::vec((0usize..13, word()), 0..4),
                any::<bool>(),
            )
                .prop_map(
                    move |(notes, audio, bus, master_gain, markers, info, gain)| {
                        let mut markers = markers;
                        markers.sort_by_key(|m| m.0);
                        markers.dedup_by_key(|m| m.0);
                        let mut info = info;
                        info.sort_by_key(|i| i.0);
                        info.dedup_by_key(|i| i.0);
                        Spec {
                            notes,
                            audio,
                            bus,
                            master_gain,
                            meter,
                            change,
                            tempo: tempo.clone(),
                            pickup_beats,
                            markers,
                            info,
                            gain,
                        }
                    },
                )
        })
}

// ---------------------------------------------------------------- building a spec

fn body_of(n: &NoteSpec) -> EventBody {
    EventBody::Note {
        pitch: n.pitch,
        velocity: n.velocity,
        release_velocity: n.release,
        duration: Tick(n.duration),
        attribute: None,
        per_note: n
            .expression
            .iter()
            .map(|(k, offset, value)| PerNoteCtl {
                kind: match k {
                    0 => PerNoteKind::PitchBend,
                    1 => PerNoteKind::Registered(74),
                    2 => PerNoteKind::Registered(7),
                    _ => PerNoteKind::Registered(10),
                },
                offset: Tick(*offset),
                value: *value,
            })
            .collect(),
    }
}

fn midi_body(m: &Midi) -> EventBody {
    match m {
        Midi::Cc(index, value) => EventBody::Cc {
            index: *index,
            value: *value,
        },
        Midi::Bend(value) => EventBody::PitchBend { value: *value },
        Midi::Pressure(value) => EventBody::ChannelPressure { value: *value },
        Midi::Poly(pitch, value) => EventBody::PolyPressure {
            pitch: *pitch,
            value: *value,
        },
        Midi::Program(program, bank) => EventBody::Program {
            program: *program,
            bank: *bank,
        },
    }
}

/// Sets `strip`'s mix as `s` says (send and effect aside).
fn mix(b: &mut Builder, strip: StripId, s: &StripSpec) {
    b.mixer(MixerCmd::SetGain {
        strip,
        gain_db: f64::from(s.gain) / 2.0,
    })
    .unwrap();
    b.mixer(MixerCmd::SetPan {
        strip,
        pan: f64::from(s.pan) / 16.0,
    })
    .unwrap();
    b.mixer(MixerCmd::SetMute {
        strip,
        mute: s.mute,
    })
    .unwrap();
    b.mixer(MixerCmd::SetSolo {
        strip,
        solo: s.solo,
    })
    .unwrap();
    if let Some(c) = s.colour {
        b.mixer(MixerCmd::SetColour {
            strip,
            colour: Some(SWATCHES[c]),
        })
        .unwrap();
    }
}

fn auto_events(
    kinds: &[(Auto, Vec<(i64, f64)>)],
    strip: StripId,
    param: Option<PluginInstanceId>,
    has_send: bool,
) -> Vec<EventSpec> {
    let mut out = Vec::new();
    for (kind, points) in kinds {
        let target = match kind {
            Auto::Fader => AutomationTarget::StripFader(strip),
            Auto::Pan => AutomationTarget::StripPan(strip),
            Auto::Mute => AutomationTarget::StripMute(strip),
            Auto::Send if has_send => AutomationTarget::SendLevel(strip, SendIndex(0)),
            Auto::Param if param.is_some() => {
                AutomationTarget::PluginParam(param.unwrap(), ParamId(7))
            }
            _ => continue,
        };
        for (tick, value) in points {
            out.push(spec_at(
                *tick,
                EventBody::Automation {
                    target,
                    value: *value,
                },
            ));
        }
    }
    out
}

/// The document of `s` and its sequence.
fn build(s: &Spec) -> (Document, SeqId) {
    let mut b = Builder::new();
    let mut states: Vec<(PluginInstanceId, Vec<u8>)> = Vec::new();
    let mut seq = None;
    let bus = s.bus.as_ref().map(|spec| {
        let strip = b.bus("Bus").unwrap();
        mix(&mut b, strip, spec);
        strip
    });
    let mut lanes = Vec::new();
    for (i, t) in s.notes.iter().enumerate() {
        let name = format!("Track {i}");
        let (dest, slot) = b
            .plugin_dest(&name, clap(&format!("org.example.{name}")))
            .unwrap();
        let lane = match seq {
            None => {
                let (q, lane) = b.sequence("Song", &name, Some(dest)).unwrap();
                seq = Some(q);
                lane
            }
            Some(q) => b.lane(q, &name, Some(dest)).unwrap(),
        };
        let strip = b.strip_of(StripKind::Instrument(dest)).unwrap();
        mix(&mut b, strip, &t.strip);
        if let (Some(bus), Some((db, pre, enabled))) = (bus, t.strip.send) {
            b.mixer(MixerCmd::AddSend {
                strip,
                send: AuxSend {
                    target: bus,
                    level_db: f64::from(db) / 2.0,
                    pre_fader: pre,
                    enabled,
                },
            })
            .unwrap();
        }
        if let Some(state) = &t.strip.state {
            states.push((slot, state.clone()));
        }
        if let Some((state, bypass)) = &t.strip.insert {
            let fx = b
                .insert(strip, clap(&format!("org.example.fx{i}")))
                .unwrap();
            b.mixer(MixerCmd::SetBypass {
                slot: fx,
                bypass: *bypass,
            })
            .unwrap();
            if let Some(state) = state {
                states.push((fx, state.clone()));
            }
        }
        lanes.push((
            lane,
            strip,
            Some(slot),
            t.strip.send.is_some() && bus.is_some(),
        ));
    }
    let seq = seq.expect("a note track");
    // Audio tracks, with their media.
    let mono = media(&mut b, "mono", 96_000, 1);
    let wide = media(&mut b, "wide", 192_000, 2);
    let mut audio_lanes = Vec::new();
    for (i, t) in s.audio.iter().enumerate() {
        let (lane, strip) = audio_track(&mut b, seq, &format!("Audio {i}"));
        mix(&mut b, strip, &t.strip);
        if let (Some(bus), Some((db, pre, enabled))) = (bus, t.strip.send) {
            b.mixer(MixerCmd::AddSend {
                strip,
                send: AuxSend {
                    target: bus,
                    level_db: f64::from(db) / 2.0,
                    pre_fader: pre,
                    enabled,
                },
            })
            .unwrap();
        }
        for (k, slot) in t.slots.iter().enumerate() {
            let at = k as i64 * 16 * Q;
            match slot {
                Slot::Plain {
                    offset,
                    length,
                    gain,
                    fade_in,
                    fade_out,
                    name,
                    muted,
                    wide: is_wide,
                } => {
                    let fin = fade_in.0.min(length / 2);
                    let fout = fade_out.0.min(length / 2);
                    let m = if *is_wide { wide } else { mono };
                    let e = b
                        .region(
                            seq,
                            lane,
                            at,
                            m,
                            *offset,
                            *length,
                            *gain,
                            Fade {
                                length_frames: fin,
                                shape: SHAPES[fade_in.1],
                            },
                            Fade {
                                length_frames: fout,
                                shape: SHAPES[fade_out.1],
                            },
                        )
                        .unwrap();
                    if let Some(name) = name {
                        b.run(Command::Audio(AudioCmd::SetName {
                            event: e,
                            name: Some(name.clone()),
                        }))
                        .unwrap();
                    }
                    if *muted {
                        b.run(Command::Event(EventCmd::SetMute {
                            events: vec![e],
                            mute: true,
                        }))
                        .unwrap();
                    }
                }
                Slot::Pair { fade, shape } => {
                    for (j, tick) in [at, at + Q].into_iter().enumerate() {
                        let e = b
                            .region(
                                seq,
                                lane,
                                tick,
                                wide,
                                0,
                                144_000,
                                0.0,
                                Fade::default(),
                                Fade::default(),
                            )
                            .unwrap();
                        if j == 1 {
                            b.run(Command::Audio(AudioCmd::Crossfade {
                                event: e,
                                length: CrossfadeLength::UpTo(*fade),
                                shape: SHAPES[*shape],
                            }))
                            .unwrap();
                        }
                    }
                }
            }
        }
        audio_lanes.push((lane, strip, t, bus.is_some() && t.strip.send.is_some()));
    }
    // Events.
    for ((lane, strip, slot, has_send), t) in lanes.iter().zip(&s.notes) {
        let mut events: Vec<EventSpec> = t
            .notes
            .iter()
            .map(|n| {
                let mut e = spec_at(n.tick, body_of(n));
                e.group_channel = n.channel;
                e
            })
            .collect();
        events.extend(t.midi.iter().map(|(tick, m)| spec_at(*tick, midi_body(m))));
        events.extend(auto_events(&t.automation, *strip, *slot, *has_send));
        b.events(seq, *lane, events).unwrap();
    }
    for (lane, strip, t, has_send) in &audio_lanes {
        let events = auto_events(&t.automation, *strip, None, *has_send);
        if !events.is_empty() {
            b.events(seq, *lane, events).unwrap();
        }
    }
    // The maps, markers and song info.
    let master = b.doc().mixer().master();
    b.mixer(MixerCmd::SetGain {
        strip: master,
        gain_db: f64::from(s.master_gain) / 2.0,
    })
    .unwrap();
    b.tempo(seq, &s.tempo).unwrap();
    let mut entries = vec![MeterEntry {
        tick: Tick(0),
        meter: Meter {
            num: s.meter.0,
            den: s.meter.1,
        },
        key: KeySig::default(),
    }];
    if let Some((tick, num, den)) = s.change {
        entries.push(MeterEntry {
            tick: Tick(tick),
            meter: Meter { num, den },
            key: KeySig::default(),
        });
    }
    b.run(Command::Sequence(SequenceCmd::SetMeter {
        seq,
        meter: MeterMap::new(entries).unwrap(),
    }))
    .unwrap();
    let beat = 4 * Q / i64::from(s.meter.1);
    b.run(Command::Sequence(SequenceCmd::SetStartOffset {
        seq,
        offset: Tick(i64::from(s.pickup_beats) * beat),
    }))
    .unwrap();
    b.run(Command::Sequence(SequenceCmd::SetMarkers {
        seq,
        markers: s
            .markers
            .iter()
            .map(|(tick, name)| Marker {
                tick: Tick(*tick),
                name: name.clone(),
            })
            .collect(),
    }))
    .unwrap();
    let mut info = SongInfo::default();
    for (field, text) in &s.info {
        match field {
            0 => info.title = Some(text.clone()),
            1 => info.artist = Some(text.clone()),
            2 => info.album = Some(text.clone()),
            3 => info.original_artist = Some(text.clone()),
            4 => info.composer = Some(text.clone()),
            5 => info.songwriter = Some(text.clone()),
            6 => info.producer = Some(text.clone()),
            7 => info.arranger = Some(text.clone()),
            8 => info.year = Some(text.clone()),
            9 => info.genre = Some(text.clone()),
            10 => info.copyright = Some(text.clone()),
            11 => info.website = Some(text.clone()),
            _ => info.comment = Some(text.clone()),
        }
    }
    b.run(Command::Settings(
        core_model::command::SettingsCmd::SetInfo { info },
    ))
    .unwrap();
    let mut b = b;
    for (slot, bytes) in states {
        b = capture(b, slot, &bytes);
    }
    (b.finish(), seq)
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
    use core_io::dawproject::export::{CROSSFADES, FADE_SHAPES, MIDI_GROUPS, REGION_GAIN};
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
