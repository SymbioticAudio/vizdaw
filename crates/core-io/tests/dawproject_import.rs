//! DAWproject import of the package, the structure, the mixer, plug-ins,
//! the tempo and meter maps, markers and song info (plan 6d Task 6).

use core_io::IoError;
use core_io::dawproject::import::{
    CANCEL_EVERY, COLOURS_NEAREST, COMMENTS, DpImport, FOLDERS, FROM_VIZDAW, INFO_CUT, ImportEnv,
    METER_MOVED, MediaSource, NAMES_CUT, OUTPUT_TO_TRACK, VCA, apply, builtin_effect,
    debug_items_after_pause, debug_pause_at, debug_paused, map, not_installed, plays_builtin,
    plugin_settings, vst,
};
use core_io::dawproject::package::{
    AudioSource, DpPackage, ODD_RATE, RECORD_UNREADABLE, SONG_INFO_UNREADABLE, open,
};
use core_io::dawproject::plugins::PlistCodec;
use core_io::limits::PackageLimits;
use core_io::report::{ItemKind, Report};
use core_io::zip::{EntryMethod, NOT_DAWPROJECT, TOO_LARGE, ZipWriter};
use core_model::command::{Command, LaneCmd, SequenceCmd};
use core_model::document::Document;
use core_model::history::History;
use core_model::ids::{AnyId, PluginInstanceId, SeqId, StripId};
use core_model::lane::Lane;
use core_model::meter::Meter;
use core_model::mixer::{Output, PluginFormat, Strip, StripKind, Swatch};
use core_model::rules;
use core_model::selection::ViewFilter;
use core_model::tempo::Bpm;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/dawproject");
const BEAT: i64 = 15_360;
const CLAP_STATE: &[u8] = b"lead clap state";
const AU_STATE: &[u8] = b"<plist>delay</plist>";

// ---------------------------------------------------------------- helpers

/// The AU codec the tests use: the binary form is the XML with "bin:" before it.
struct FakePlist;

impl PlistCodec for FakePlist {
    fn to_xml(&self, _binary: &[u8]) -> Result<Vec<u8>, String> {
        Err("not used".into())
    }

    fn to_binary(&self, plist: &[u8]) -> Result<Vec<u8>, String> {
        let mut out = b"bin:".to_vec();
        out.extend_from_slice(plist);
        Ok(out)
    }
}

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!("{DIR}/{name}"))
        .unwrap_or_else(|e| panic!("hand-written fixture {name} is missing: {e}"))
}

/// A package at `dir/<stem>.dawproject` holding `entries` (XML deflated,
/// the rest stored).
fn package(dir: &Path, stem: &str, entries: &[(&str, &[u8])]) -> PathBuf {
    package_as(dir, stem, entries, true)
}

/// [`package`], with every entry stored when `deflate` is false (text so
/// repetitive that deflating it would pass the 200:1 ratio).
fn package_as(dir: &Path, stem: &str, entries: &[(&str, &[u8])], deflate: bool) -> PathBuf {
    let path = dir.join(format!("{stem}.dawproject"));
    let file = std::fs::File::create(&path).unwrap();
    let mut z = ZipWriter::new(file);
    for (name, bytes) in entries {
        let method = if deflate && (name.ends_with(".xml") || name.ends_with(".json")) {
            EntryMethod::Deflated
        } else {
            EntryMethod::Stored
        };
        z.add(name, method, bytes).unwrap();
    }
    z.finish().unwrap();
    path
}

fn open_ok(path: &Path) -> DpPackage {
    open(path, &PackageLimits::default(), &AtomicBool::new(false)).expect("opens")
}

/// `hand_other` with both state files, opened.
fn hand_other(dir: &Path) -> DpPackage {
    let project = fixture("hand_other.project.xml");
    let metadata = fixture("hand_other.metadata.xml");
    let path = package(
        dir,
        "Other",
        &[
            ("project.xml", &project),
            ("metadata.xml", &metadata),
            ("plugins/d.aupreset", AU_STATE),
            ("plugins/l.clap-preset", CLAP_STATE),
        ],
    );
    open_ok(&path)
}

/// A project.xml with `structure` inside `Structure` and `arrangement`
/// inside `Arrangement`.
fn project_xml(structure: &str, arrangement: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Project version="1.0">
  <Application name="Fixture" version="1"/>
  <Structure>
    <Track id="m" name="Master" contentType="audio notes"><Channel id="mc" role="master"/></Track>
    {structure}
  </Structure>
  <Arrangement>{arrangement}</Arrangement>
</Project>"#
    )
}

/// A package of just `project` (and `metadata`, when given).
fn small(dir: &Path, project: &str, metadata: Option<&str>) -> DpPackage {
    let mut entries: Vec<(&str, &[u8])> = vec![("project.xml", project.as_bytes())];
    if let Some(m) = metadata {
        entries.push(("metadata.xml", m.as_bytes()));
    }
    open_ok(&package(dir, "Small", &entries))
}

fn yes(_: PluginFormat, _: &str) -> bool {
    true
}

fn env<'a>(installed: &'a dyn Fn(PluginFormat, &str) -> bool) -> ImportEnv<'a> {
    ImportEnv {
        installed,
        plist: &FakePlist,
        rate: 48_000,
    }
}

fn mapped(pkg: &DpPackage, installed: &dyn Fn(PluginFormat, &str) -> bool) -> DpImport {
    map(
        pkg,
        &BTreeMap::<usize, MediaSource>::new(),
        &env(installed),
        &AtomicBool::new(false),
    )
    .expect("maps")
}

/// A document like a new project: the master and one sequence with no
/// tracks.
fn new_doc() -> (Document, SeqId) {
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

struct Applied {
    h: History,
    seq: SeqId,
    report: Report,
    states: Vec<(PluginInstanceId, Arc<[u8]>)>,
}

/// Maps `pkg` and applies it to a new document in one step.
fn imported(pkg: &DpPackage, installed: &dyn Fn(PluginFormat, &str) -> bool) -> Applied {
    let imp = mapped(pkg, installed);
    let (doc, seq) = new_doc();
    let mut h = History::new(doc);
    let mut report = Report::default();
    h.begin("Import DAWproject").unwrap();
    let states = apply(&mut h, &imp, seq, &mut report).expect("applies");
    h.commit().unwrap();
    Applied {
        h,
        seq,
        report,
        states,
    }
}

fn lanes(a: &Applied) -> Vec<Lane> {
    a.h.doc()
        .sequence(a.seq)
        .unwrap()
        .lanes
        .iter()
        .cloned()
        .collect()
}

fn lane(a: &Applied, name: &str) -> Lane {
    lanes(a)
        .into_iter()
        .find(|l| l.name == name)
        .unwrap_or_else(|| panic!("no track {name}"))
}

fn strip_of_kind(a: &Applied, kind: StripKind) -> Strip {
    let d = a.h.doc();
    let id = d.mixer().find_kind(kind).expect("a strip of that kind");
    d.mixer().strip(id).unwrap().clone()
}

/// The strip a track's notes play through.
fn midi_strip(a: &Applied, l: &Lane) -> Strip {
    let dest = l.midi_dest.expect("a destination");
    let d = a.h.doc();
    let kind = if d.instruments()[&dest].kind.is_builtin_synth() {
        StripKind::BuiltinSynth
    } else {
        StripKind::Instrument(dest)
    };
    strip_of_kind(a, kind)
}

fn audio_strip(a: &Applied, l: &Lane) -> Strip {
    strip_of_kind(
        a,
        StripKind::AudioPath(l.audio_out.expect("an output path")),
    )
}

fn bus(a: &Applied, name: &str) -> StripId {
    let d = a.h.doc();
    d.mixer()
        .strips()
        .values()
        .find(|s| s.kind == StripKind::Bus && s.name == name)
        .map(|s| s.id)
        .unwrap_or_else(|| panic!("no bus {name}"))
}

fn buses(a: &Applied) -> Vec<String> {
    let d = a.h.doc();
    d.mixer()
        .order()
        .iter()
        .filter_map(|id| d.mixer().strip(*id).ok())
        .filter(|s| s.kind == StripKind::Bus)
        .map(|s| s.name.clone())
        .collect()
}

fn item<'a>(r: &'a Report, kind: ItemKind, what: &str) -> Option<&'a core_io::report::Item> {
    r.items.iter().find(|i| i.kind == kind && i.what == what)
}

#[track_caller]
fn has(r: &Report, kind: ItemKind, what: &str) {
    assert!(
        item(r, kind, what).is_some(),
        "no {kind:?} '{what}' in {:#?}",
        r.items
    );
}

#[track_caller]
fn lacks(r: &Report, what: &str) {
    assert!(
        r.items.iter().all(|i| i.what != what),
        "unexpected '{what}' in {:#?}",
        r.items
    );
}

fn state_of(a: &Applied, slot: PluginInstanceId) -> Option<Vec<u8>> {
    a.states
        .iter()
        .find(|(s, _)| *s == slot)
        .map(|(_, b)| b.to_vec())
}

/// A note track playing a CLAP instrument, with its channel `<id>c`.
fn clap_track(name: &str, id: &str, comment: &str, dest: &str) -> String {
    format!(
        r#"<Track id="{id}" name="{name}" comment="{comment}" contentType="notes"><Channel id="{id}c" role="regular" destination="{dest}"><Devices><ClapPlugin id="{id}d" deviceID="org.example.{id}" deviceName="Synth {id}" deviceRole="instrument"/></Devices></Channel></Track>"#
    )
}

// ---------------------------------------------------------------- tests

#[test]
fn reads_the_hand_other_file() {
    let dir = tempfile::tempdir().unwrap();
    let pkg = hand_other(dir.path());
    assert!(!pkg.ret.from_vizdaw());
    let a = imported(&pkg, &yes);
    let r = &a.report;

    let names: Vec<String> = lanes(&a).into_iter().map(|l| l.name).collect();
    assert_eq!(names, ["Bass", "Gtr", "Lead"]);
    assert_eq!(buses(&a), ["Band", "Room"]);
    has(r, ItemKind::Changed, FOLDERS);

    // "Bass" plays the built-in synth (its instrument is VST3), "Gtr" is an
    // audio track; both output to the folder's bus.
    let band = bus(&a, "Band");
    let bass = lane(&a, "Bass");
    let synth = midi_strip(&a, &bass);
    assert_eq!(synth.kind, StripKind::BuiltinSynth);
    assert_eq!(synth.output, Output::Bus(band));
    assert_eq!(
        a.h.doc().instruments()[&bass.midi_dest.unwrap()].name,
        "Built-in Synth (ch 1)"
    );
    let gtr = lane(&a, "Gtr");
    assert!(gtr.midi_dest.is_none());
    let gtr_strip = audio_strip(&a, &gtr);
    assert_eq!(gtr_strip.output, Output::Bus(band));
    assert_eq!(
        a.h.doc().mixer().strip(band).unwrap().output,
        Output::Master
    );

    has(r, ItemKind::LeftOut, &vst("Bass VST"));
    let eq = item(r, ItemKind::LeftOut, &builtin_effect("Equalizer", "Gtr")).expect("EQ line");
    assert_eq!(eq.details.len(), 2, "{:?}", eq.details);
    assert!(eq.details[0].contains("bell"), "{:?}", eq.details);
    assert!(eq.details[1].contains("highPass"), "{:?}", eq.details);
    has(r, ItemKind::LeftOut, VCA);

    // The AU delay is the insert, with its state through the codec.
    assert_eq!(gtr_strip.inserts.len(), 1);
    let delay = &gtr_strip.inserts[0];
    assert_eq!(delay.plugin.format, PluginFormat::Au);
    assert_eq!(delay.plugin.id, "aufx/dely/xmpl");
    assert!(!delay.bypass);
    let mut want = b"bin:".to_vec();
    want.extend_from_slice(AU_STATE);
    assert_eq!(state_of(&a, delay.id), Some(want));

    // "Lead" plays its CLAP plug-in; its colour is the nearest swatch.
    let lead = lane(&a, "Lead");
    let lead_strip = midi_strip(&a, &lead);
    assert_eq!(lead_strip.colour, Some(Swatch::Green));
    has(r, ItemKind::Changed, COLOURS_NEAREST);
    let core_model::document::DestKind::PluginInstrument { slot } =
        &a.h.doc().instruments()[&lead.midi_dest.unwrap()].kind
    else {
        panic!("Lead plays a plug-in")
    };
    assert_eq!(slot.plugin.format, PluginFormat::Clap);
    assert_eq!(slot.plugin.id, "org.example.lead");
    assert_eq!(slot.plugin.version, "3.0");
    assert_eq!(state_of(&a, slot.id), Some(CLAP_STATE.to_vec()));

    // The post send to "Room", off.
    assert_eq!(lead_strip.sends.len(), 1);
    let send = lead_strip.sends[0];
    assert_eq!(send.target, bus(&a, "Room"));
    assert!((send.level_db - -6.02).abs() < 0.01, "{}", send.level_db);
    assert!(!send.pre_fader);
    assert!(!send.enabled);

    // 149 ramping to 160 at beat 32; 7/8 moved from beat 30 to the barline
    // at 32.
    let s = a.h.doc().sequence(a.seq).unwrap();
    let tempo: Vec<(i64, Bpm, bool)> = s
        .tempo
        .points()
        .iter()
        .map(|p| (p.tick.0, p.bpm, p.ramp))
        .collect();
    assert_eq!(
        tempo,
        [(0, Bpm(149_000), true), (32 * BEAT, Bpm(160_000), false)]
    );
    let meter: Vec<(i64, Meter)> = s
        .meter
        .entries()
        .iter()
        .map(|e| (e.tick.0, e.meter))
        .collect();
    assert_eq!(
        meter,
        [
            (0, Meter { num: 4, den: 4 }),
            (32 * BEAT, Meter { num: 7, den: 8 })
        ]
    );
    has(r, ItemKind::Changed, METER_MOVED);

    let markers: Vec<(i64, &str)> = s
        .markers
        .iter()
        .map(|m| (m.tick.0, m.name.as_str()))
        .collect();
    assert_eq!(markers, [(0, "Intro"), (64 * BEAT, "Out")]);

    assert_eq!(s.name, "Other Song");
    let info = &a.h.doc().settings().info;
    let fields: Vec<Option<&str>> = info.fields().iter().map(|f| f.as_deref()).collect();
    assert_eq!(
        fields,
        [
            Some("Other Song"),
            Some("An Artist"),
            Some("An Album"),
            Some("First Artist"),
            Some("A Composer"),
            Some("A Songwriter"),
            Some("A Producer"),
            Some("An Arranger"),
            Some("2026"),
            Some("Rock"),
            Some("(c) 2026 Someone"),
            Some("https://example.com"),
            Some("made here"),
        ]
    );
    assert_eq!(r.summary.tracks, 3);
    assert_eq!(r.summary.markers, 2);
    assert!(r.summary.tempo_map);
    assert!(
        r.summary_line("Imported")
            .starts_with("Imported 3 tracks, the tempo map, 2 markers"),
        "{}",
        r.summary_line("Imported")
    );
    lacks(r, FROM_VIZDAW);
    assert!(rules::validate(a.h.doc()).is_empty());
}

#[test]
fn plugins_map_or_wait() {
    let dir = tempfile::tempdir().unwrap();
    let pkg = hand_other(dir.path());

    let lead_only = |_: PluginFormat, id: &str| id == "org.example.lead";
    let a = imported(&pkg, &lead_only);
    lacks(&a.report, &not_installed("Lead"));
    has(&a.report, ItemKind::Note, &not_installed("Delay"));
    let lead = lane(&a, "Lead");
    let core_model::document::DestKind::PluginInstrument { slot } =
        &a.h.doc().instruments()[&lead.midi_dest.unwrap()].kind
    else {
        panic!("Lead plays a plug-in")
    };
    assert_eq!(state_of(&a, slot.id), Some(CLAP_STATE.to_vec()));

    // Not installed: still mapped, with its state, and the Note.
    let none = |_: PluginFormat, _: &str| false;
    let a = imported(&pkg, &none);
    has(&a.report, ItemKind::Note, &not_installed("Lead"));
    let lead = lane(&a, "Lead");
    let core_model::document::DestKind::PluginInstrument { slot } =
        &a.h.doc().instruments()[&lead.midi_dest.unwrap()].kind
    else {
        panic!("Lead still plays its plug-in")
    };
    assert_eq!(state_of(&a, slot.id), Some(CLAP_STATE.to_vec()));
    let gtr = audio_strip(&a, &lane(&a, "Gtr"));
    let mut want = b"bin:".to_vec();
    want.extend_from_slice(AU_STATE);
    assert_eq!(state_of(&a, gtr.inserts[0].id), Some(want));

    // A state entry missing from the ZIP: the plug-in without its settings.
    let project = fixture("hand_other.project.xml");
    let path = package(
        dir.path(),
        "NoDelayState",
        &[
            ("project.xml", &project),
            ("plugins/l.clap-preset", CLAP_STATE),
        ],
    );
    let a = imported(&open_ok(&path), &yes);
    has(&a.report, ItemKind::LeftOut, &plugin_settings("Delay"));
    let gtr = audio_strip(&a, &lane(&a, "Gtr"));
    assert_eq!(gtr.inserts.len(), 1);
    assert_eq!(state_of(&a, gtr.inserts[0].id), None);
    lacks(&a.report, &plugin_settings("Lead"));
    // No metadata.xml: the title is the file's stem.
    assert_eq!(a.h.doc().sequence(a.seq).unwrap().name, "NoDelayState");
}

#[test]
fn tags_are_stripped_and_counted() {
    let dir = tempfile::tempdir().unwrap();
    let four = |comments: [&str; 4]| {
        let tracks: String = comments
            .iter()
            .enumerate()
            .map(|(i, c)| clap_track(&format!("T{i}"), &format!("t{i}"), c, "mc"))
            .collect();
        project_xml(&tracks, "")
    };

    let pkg = small(dir.path(), &four(["vz:t3 hello", "", "", ""]), None);
    assert_eq!(pkg.ret.tagged_tracks, 1);
    assert_eq!(pkg.ret.tracks, 4);
    assert!(!pkg.ret.from_vizdaw());
    let a = imported(&pkg, &yes);
    has(&a.report, ItemKind::LeftOut, COMMENTS);
    lacks(&a.report, FROM_VIZDAW);

    let pkg = small(dir.path(), &four(["vz:t3", "vz:t4", "", ""]), None);
    assert_eq!(pkg.ret.tagged_tracks, 2);
    assert!(pkg.ret.from_vizdaw());
    let a = imported(&pkg, &yes);
    has(&a.report, ItemKind::Note, FROM_VIZDAW);
    lacks(&a.report, COMMENTS);

    // The export line in the metadata comment.
    let meta = "<MetaData><Title>T</Title><Comment>Notes\nVizDAW export E9</Comment></MetaData>";
    let pkg = small(dir.path(), &four(["", "", "", ""]), Some(meta));
    assert_eq!(pkg.ret.export_id.as_deref(), Some("E9"));
    assert!(pkg.ret.from_vizdaw());
    let a = imported(&pkg, &yes);
    assert_eq!(
        a.h.doc().settings().info.comment.as_deref(),
        Some("Notes"),
        "the export line is stripped"
    );
    has(&a.report, ItemKind::Note, FROM_VIZDAW);

    // The record.
    let project = four(["", "", "", ""]);
    let record = br#"{"format":1,"project_id":"00000000-0000-4000-8000-000000000001","export_id":"E7","exported_at":1,"app_version":"1","seq":1,"origin":0,"ids":{},"document":{"big":[1,2,3]}}"#;
    let path = package(
        dir.path(),
        "Rec",
        &[
            ("project.xml", project.as_bytes()),
            ("vizdaw/record.json", record),
        ],
    );
    let pkg = open_ok(&path);
    let head = pkg.ret.record.as_ref().expect("the record");
    assert_eq!(head.format, 1);
    assert_eq!(head.project_id, "00000000-0000-4000-8000-000000000001");
    assert_eq!(head.export_id, "E7");
    assert!(pkg.ret.from_vizdaw());
    lacks(&pkg.report, RECORD_UNREADABLE);

    let path = package(
        dir.path(),
        "BadRec",
        &[
            ("project.xml", project.as_bytes()),
            ("vizdaw/record.json", b"\xff{"),
        ],
    );
    let pkg = open_ok(&path);
    assert!(pkg.ret.record.is_none());
    has(&pkg.report, ItemKind::Note, RECORD_UNREADABLE);
}

#[test]
fn channel_destinations() {
    let dir = tempfile::tempdir().unwrap();
    let structure = format!(
        "{}{}{}{}{}",
        clap_track("ToMaster", "a", "", "mc"),
        clap_track("ToBus", "b", "", "busc"),
        clap_track("ToTrack", "c", "", "dc"),
        clap_track("Target", "d", "", "mc"),
        r#"<Track id="bus" name="Bus" contentType="audio"><Channel id="busc" role="effect" destination="mc"/></Track>"#,
    );
    let pkg = small(dir.path(), &project_xml(&structure, ""), None);
    let a = imported(&pkg, &yes);
    let out = |name: &str| midi_strip(&a, &lane(&a, name)).output;
    assert_eq!(out("ToMaster"), Output::Master);
    assert_eq!(out("ToBus"), Output::Bus(bus(&a, "Bus")));
    assert_eq!(out("ToTrack"), Output::Master);
    assert_eq!(out("Target"), Output::Master);
    has(&a.report, ItemKind::Changed, OUTPUT_TO_TRACK);
    assert_eq!(
        item(&a.report, ItemKind::Changed, OUTPUT_TO_TRACK)
            .unwrap()
            .count,
        1
    );
    // A track with no instrument plays the built-in synth.
    let plain = r#"<Track id="p" name="Plain" contentType="notes"><Channel id="pc" role="regular"/></Track>"#.to_string();
    let pkg = small(dir.path(), &project_xml(&plain, ""), None);
    let a = imported(&pkg, &yes);
    has(&a.report, ItemKind::Note, &plays_builtin("Plain"));
    assert_eq!(
        midi_strip(&a, &lane(&a, "Plain")).kind,
        StripKind::BuiltinSynth
    );
}

#[test]
fn refused_packages() {
    let dir = tempfile::tempdir().unwrap();
    let flag = AtomicBool::new(false);
    let limits = PackageLimits::default();

    let not_zip = dir.path().join("plain.dawproject");
    std::fs::write(&not_zip, b"this is no zip at all").unwrap();
    assert!(matches!(
        open(&not_zip, &limits, &flag),
        Err(IoError::Refused(_))
    ));

    let path = package(dir.path(), "NoProject", &[("metadata.xml", b"<MetaData/>")]);
    assert_eq!(
        open(&path, &limits, &flag).err(),
        Some(IoError::Refused(NOT_DAWPROJECT.into()))
    );

    let project = project_xml("", "");
    let path = package(dir.path(), "Big", &[("project.xml", project.as_bytes())]);
    let lowered = PackageLimits {
        max_project_xml: 64,
        ..limits
    };
    assert_eq!(
        open(&path, &lowered, &flag).err(),
        Some(IoError::Refused(TOO_LARGE.into()))
    );

    let v2 = project.replace(r#"version="1.0""#, r#"version="2.0""#);
    let path = package(dir.path(), "V2", &[("project.xml", v2.as_bytes())]);
    let Err(IoError::Refused(why)) = open(&path, &limits, &flag) else {
        panic!("a version-2 file is refused")
    };
    assert!(why.contains("version 2.0"), "{why}");

    // A metadata.xml past its own limit leaves the song info out.
    let path = package(
        dir.path(),
        "BigMeta",
        &[
            ("project.xml", project.as_bytes()),
            ("metadata.xml", b"<MetaData><Title>T</Title></MetaData>"),
        ],
    );
    let lowered = PackageLimits {
        max_metadata_xml: 8,
        ..limits
    };
    let pkg = open(&path, &lowered, &flag).expect("opens without its song info");
    has(&pkg.report, ItemKind::LeftOut, SONG_INFO_UNREADABLE);
    assert!(pkg.metadata.fields.iter().all(Option::is_none));
}

fn audio_clip(path: &str, rate: u32) -> String {
    format!(
        r#"<Clip time="0" duration="1"><Audio sampleRate="{rate}" channels="2" duration="1.5"><File path="{path}"/></Audio></Clip>"#
    )
}

fn with_audio(clips: &[String]) -> String {
    let structure = r#"<Track id="a" name="A" contentType="audio"><Channel id="ac" role="regular" destination="mc"/></Track>"#;
    let arrangement = format!(
        r#"<Lanes timeUnit="beats"><Clips track="a">{}</Clips></Lanes>"#,
        clips.concat()
    );
    project_xml(structure, &arrangement)
}

#[test]
fn info_majority_rate() {
    let dir = tempfile::tempdir().unwrap();
    let clips = [
        audio_clip("audio/1.wav", 44_100),
        audio_clip("audio/2.wav", 44_100),
        audio_clip("audio/1.wav", 44_100),
        audio_clip("audio/3.wav", 48_000),
    ];
    let pkg = small(dir.path(), &with_audio(&clips), None);
    let info = pkg.info();
    assert_eq!(info.sample_rate, 44_100);
    assert_eq!(info.audio_files, 3, "distinct paths");
    assert_eq!(info.tracks, 1);
    lacks(&pkg.report, ODD_RATE);
    let refs = pkg.audio();
    assert_eq!(refs.len(), 3);
    assert_eq!(refs[0].source, AudioSource::Entry("audio/1.wav".into()));
    assert_eq!(refs[0].frames, 66_150);
    assert_eq!(refs[0].channels, 2);
    assert_eq!(refs[0].name, "1");
    assert_eq!(refs[2].sample_rate, 48_000);

    let pkg = small(
        dir.path(),
        &with_audio(&[audio_clip("audio/1.wav", 22_050)]),
        None,
    );
    assert_eq!(pkg.info().sample_rate, 48_000);
    has(&pkg.report, ItemKind::Note, ODD_RATE);

    let pkg = small(dir.path(), &with_audio(&[]), None);
    assert_eq!(pkg.info().sample_rate, 48_000);
    assert_eq!(pkg.info().audio_files, 0);
    lacks(&pkg.report, ODD_RATE);

    // hand_other: the folder, the bus and the master are not counted.
    let pkg = hand_other(dir.path());
    assert_eq!(pkg.info().tracks, 3);
}

#[test]
fn needed_bytes_counts_entries_external_files_and_converted_copies() {
    let dir = tempfile::tempdir().unwrap();
    let outside = dir.path().join("outside.wav");
    std::fs::write(&outside, vec![0u8; 1000]).unwrap();
    let clips = [
        audio_clip("audio/1.wav", 48_000),
        format!(
            r#"<Clip time="0" duration="1"><Audio sampleRate="48000" channels="1" duration="1"><File path="{}" external="true"/></Audio></Clip>"#,
            outside.display()
        ),
        r#"<Clip time="0" duration="1"><Audio sampleRate="48000" channels="1" duration="1"><File path="https://example.com/x.wav" external="true"/></Audio></Clip>"#.to_string(),
    ];
    let project = with_audio(&clips);
    let entry = vec![7u8; 5000];
    let path = package(
        dir.path(),
        "Ext",
        &[("project.xml", project.as_bytes()), ("audio/1.wav", &entry)],
    );
    let pkg = open_ok(&path);
    let refs = pkg.audio();
    assert_eq!(refs.len(), 3);
    assert_eq!(refs[1].source, AudioSource::External(outside.clone()));
    // Entry: 5 000 extracted + its CAF (1.5 s × 48 000 × 2 × 4 = 576 000);
    // the external file: its CAF (48 000 × 1 × 4 = 192 000, over 1 000 × 4);
    // the URL is never read, so it makes nothing.
    assert_eq!(pkg.needed_bytes(4), 5_000 + 576_000 + 192_000);
    assert_eq!(pkg.needed_bytes(2), 5_000 + 288_000 + 96_000);

    // A lying element: the size floor (file size × 4) wins.
    let liar = r#"<Clip time="0" duration="1"><Audio sampleRate="48000" channels="1" duration="0"><File path="audio/1.wav"/></Audio></Clip>"#.to_string();
    let project = with_audio(&[liar]);
    let path = package(
        dir.path(),
        "Liar",
        &[("project.xml", project.as_bytes()), ("audio/1.wav", &entry)],
    );
    assert_eq!(open_ok(&path).needed_bytes(4), 5_000 + 20_000);

    // Extracting an entry writes its bytes; an external file is not
    // extracted.
    let pkg = open_ok(&package(
        dir.path(),
        "Ext2",
        &[
            ("project.xml", with_audio(&clips).as_bytes()),
            ("audio/1.wav", &entry),
        ],
    ));
    let to = dir.path().join("x.wav");
    let flag = AtomicBool::new(false);
    assert_eq!(pkg.extract(&pkg.audio()[0], &to, &flag), Ok(5_000));
    assert_eq!(std::fs::read(&to).unwrap(), entry);
    assert!(matches!(
        pkg.extract(&pkg.audio()[1], &dir.path().join("y.wav"), &flag),
        Err(IoError::Refused(_))
    ));
}

#[test]
fn hostile_text_is_sanitised() {
    let dir = tempfile::tempdir().unwrap();
    // An attribute over 64 KiB refuses the file (the XML reader's rule);
    // a name under it is cut to 4 096 bytes.
    let name = format!("A\u{1}B\u{202E}C{}", "n".repeat(70_000));
    let project = project_xml(&clap_track(&name, "h", "", "mc"), "");
    let path = package_as(
        dir.path(),
        "Refused",
        &[("project.xml", project.as_bytes())],
        false,
    );
    assert!(matches!(
        open(&path, &PackageLimits::default(), &AtomicBool::new(false)),
        Err(IoError::Refused(_))
    ));
    let name = format!("A\u{1}B\u{202E}C{}", "n".repeat(60_000));
    let structure = clap_track(&name, "h", "", "mc");
    let title = "t".repeat(70_000);
    let meta = format!("<MetaData><Title>{title}</Title></MetaData>");
    let project = project_xml(&structure, "");
    let path = package_as(
        dir.path(),
        "Hostile",
        &[
            ("project.xml", project.as_bytes()),
            ("metadata.xml", meta.as_bytes()),
        ],
        false,
    );
    let pkg = open_ok(&path);
    let a = imported(&pkg, &yes);
    let l = &lanes(&a)[0];
    assert!(l.name.starts_with("ABCnnn"), "{}", &l.name[..10]);
    assert_eq!(l.name.len(), 4096);
    assert!(core_model::lane::valid_name(&l.name).is_some());
    has(&a.report, ItemKind::Changed, NAMES_CUT);
    let info = &a.h.doc().settings().info;
    assert_eq!(info.title.as_deref().map(str::len), Some(4096));
    has(&a.report, ItemKind::Changed, INFO_CUT);
    assert_eq!(a.h.doc().sequence(a.seq).unwrap().name.len(), 4096);
    assert!(rules::validate(a.h.doc()).is_empty());

    // An empty name becomes "Track <n>"; a zero-width-only one too.
    let structure = format!(
        "{}{}",
        clap_track("", "e1", "", "mc"),
        clap_track("\u{200B}", "e2", "", "mc")
    );
    let pkg = small(dir.path(), &project_xml(&structure, ""), None);
    let a = imported(&pkg, &yes);
    let names: Vec<String> = lanes(&a).into_iter().map(|l| l.name).collect();
    assert_eq!(names, ["Track 1", "Track 2"]);
}

/// A project with one track holding `notes` notes in one clip.
fn many_notes(notes: usize) -> String {
    let mut s = String::with_capacity(notes * 60);
    for i in 0..notes {
        s.push_str(&format!(
            r#"<Note time="{i}" duration="0.5" channel="0" key="60" vel="0.5"/>"#
        ));
    }
    let arrangement = format!(
        r#"<Lanes timeUnit="beats"><Clips track="a"><Clip time="0" duration="{notes}"><Notes>{s}</Notes></Clip></Clips></Lanes>"#
    );
    project_xml(&clap_track("A", "a", "", "mc"), &arrangement)
}

#[test]
fn map_stops_when_cancelled() {
    let dir = tempfile::tempdir().unwrap();
    let path = package_as(
        dir.path(),
        "Big",
        &[("project.xml", many_notes(300_000).as_bytes())],
        false,
    );
    let set = AtomicBool::new(true);
    assert_eq!(
        open(&path, &PackageLimits::default(), &set).err(),
        Some(IoError::Cancelled)
    );
    let pkg = open_ok(&path);
    assert_eq!(
        map(&pkg, &BTreeMap::new(), &env(&yes), &set).err(),
        Some(IoError::Cancelled)
    );

    // The map pauses at its 100 000th item until a second thread sets the
    // flag; it then stops within one interval.
    let flag = AtomicBool::new(false);
    debug_pause_at(&flag, Some(100_000));
    let got = std::thread::scope(|s| {
        s.spawn(|| {
            while !debug_paused() {
                std::thread::yield_now();
            }
            flag.store(true, Ordering::Relaxed);
        });
        map(&pkg, &BTreeMap::new(), &env(&yes), &flag).map(|_| ())
    });
    let after = debug_items_after_pause();
    debug_pause_at(&flag, None);
    assert_eq!(got, Err(IoError::Cancelled));
    assert!(after <= CANCEL_EVERY, "{after} items after the flag");

    // Without a cancel it runs to the end.
    assert!(map(&pkg, &BTreeMap::new(), &env(&yes), &AtomicBool::new(false)).is_ok());
}

#[test]
fn apply_validates_and_is_one_step() {
    let dir = tempfile::tempdir().unwrap();
    let pkg = hand_other(dir.path());
    let imp = mapped(&pkg, &yes);
    let (doc, seq) = new_doc();
    let mut h = History::new(doc.clone());
    let depth = h.undo_depth();
    let mut report = Report::default();
    h.begin("Import DAWproject").unwrap();
    apply(&mut h, &imp, seq, &mut report).unwrap();
    h.commit().unwrap();
    assert!(
        rules::validate(h.doc()).is_empty(),
        "{:?}",
        rules::validate(h.doc())
    );
    assert_eq!(h.undo_depth(), depth + 1);
    h.undo().unwrap();
    assert_eq!(h.doc(), &doc);

    // A document with a track is refused.
    let mut h = History::new(doc);
    h.execute(
        &Command::Lane(LaneCmd::Add {
            seq,
            name: "Here".into(),
            index: None,
        }),
        &ViewFilter::default(),
    )
    .unwrap();
    h.begin("Import DAWproject").unwrap();
    assert!(apply(&mut h, &imp, seq, &mut Report::default()).is_err());
    h.abort().unwrap();
}

#[test]
fn every_dropped_item_is_listed() {
    use core_io::dawproject::import::{
        BAD_METER, COLOURS_AND_COMMENTS, LEVELS_CUT, LOOP_OUTPUTS, LOOP_SENDS, MAP_ORDER,
        MASTER_SENDS, NO_STRIP, SEND_PAN, SEND_TARGETS, SHARED_SYNTH, TEMPO_CLAMPED, TEXT_DROPPED,
        analyzer, builtin_instrument, extra_instrument, no_plugin_id, note_effect,
    };
    let dir = tempfile::tempdir().unwrap();
    let project = r##"<?xml version="1.0" encoding="UTF-8"?>
<Project version="1.0">
  <Application name="Fixture" version="1"/>
  <Transport>
    <Tempo id="tempo" unit="bpm" value="2000"/>
    <TimeSignature id="ts" numerator="3" denominator="4"/>
  </Transport>
  <Structure>stray text
    <Track id="m" name="Master" contentType="audio notes">
      <Channel id="mc" role="master">
        <Sends><Send id="ms" destination="ac" type="post"><Volume unit="linear" value="1"/></Send></Sends>
      </Channel>
    </Track>
    <Track id="a" name="BusA" contentType="audio"><Channel id="ac" role="effect" destination="bc"/></Track>
    <Track id="b" name="BusB" contentType="audio">
      <Channel id="bc" role="effect" destination="ac">
        <Sends><Send id="bs" destination="ac" type="post"><Volume unit="linear" value="1"/></Send></Sends>
      </Channel>
    </Track>
    <Track id="k" name="Keys" contentType="notes">
      <Channel id="kc" role="regular" destination="mc">
        <Devices>
          <ClapPlugin id="kd1" deviceID="org.example.keys" deviceName="Keys" deviceRole="instrument"/>
          <ClapPlugin id="kd2" deviceID="org.example.more" deviceName="More" deviceRole="instrument"/>
          <ClapPlugin id="kd3" deviceID="org.example.arp" deviceName="Arp" deviceRole="noteFX"/>
          <ClapPlugin id="kd4" deviceID="org.example.scope" deviceName="Scope" deviceRole="analyzer"/>
          <ClapPlugin id="kd5" deviceName="Nameless" deviceRole="audioFX"/>
        </Devices>
        <Sends>
          <Send id="ks1" destination="xc" type="post"><Volume unit="linear" value="1"/></Send>
          <Send id="ks2" destination="ac" type="pre"><Pan unit="normalized" value="0.25"/><Volume unit="linear" value="1"/></Send>
        </Sends>
        <Volume id="kv" unit="linear" value="8"/>
      </Channel>
    </Track>
    <Track id="s1" name="Synth1" contentType="notes"><Channel id="s1c" role="regular" destination="mc"/></Track>
    <Track id="s2" name="Synth2" contentType="notes">
      <Channel id="s2c" role="regular" destination="mc"><Volume unit="linear" value="0.5"/></Channel>
    </Track>
    <Track id="o" name="Organ" contentType="notes">
      <Channel id="oc" role="regular" destination="mc">
        <Devices><BuiltinDevice id="od" deviceName="Organ" deviceRole="instrument"/></Devices>
      </Channel>
    </Track>
    <Track id="x" name="Auto" contentType="automation">
      <Channel id="xc" role="regular" destination="mc">
        <Devices><ClapPlugin id="xd" deviceID="org.example.fx" deviceName="Fx" deviceRole="audioFX"/></Devices>
      </Channel>
    </Track>
  </Structure>
  <Arrangement>
    <Markers><Marker name="M" color="#ff0000" comment="a note" time="0"/></Markers>
    <TempoAutomation unit="bpm" timeUnit="seconds">
      <Target parameter="tempo"/>
      <RealPoint time="0" value="120" interpolation="linear"/>
      <RealPoint time="2" value="180" interpolation="hold"/>
    </TempoAutomation>
    <TimeSignatureAutomation>
      <Target parameter="ts"/>
      <TimeSignaturePoint time="0" numerator="4" denominator="4"/>
      <TimeSignaturePoint time="8" numerator="3" denominator="5"/>
      <TimeSignaturePoint time="16" numerator="3" denominator="4"/>
      <TimeSignaturePoint time="8" numerator="5" denominator="4"/>
    </TimeSignatureAutomation>
  </Arrangement>
</Project>"##;
    let pkg = small(dir.path(), project, None);
    let a = imported(&pkg, &yes);
    let r = &a.report;
    for (kind, what) in [
        (ItemKind::LeftOut, MASTER_SENDS.to_string()),
        (ItemKind::Changed, LOOP_OUTPUTS.to_string()),
        (ItemKind::LeftOut, LOOP_SENDS.to_string()),
        (ItemKind::LeftOut, extra_instrument("More")),
        (ItemKind::LeftOut, note_effect("Arp")),
        (ItemKind::LeftOut, analyzer("Scope")),
        (ItemKind::LeftOut, no_plugin_id("Nameless")),
        (ItemKind::LeftOut, SEND_TARGETS.to_string()),
        (ItemKind::LeftOut, SEND_PAN.to_string()),
        (ItemKind::Changed, LEVELS_CUT.to_string()),
        (ItemKind::LeftOut, SHARED_SYNTH.to_string()),
        (
            ItemKind::LeftOut,
            builtin_instrument("BuiltinDevice", "Organ"),
        ),
        (ItemKind::LeftOut, NO_STRIP.to_string()),
        (ItemKind::LeftOut, COLOURS_AND_COMMENTS.to_string()),
        (ItemKind::LeftOut, COMMENTS.to_string()),
        (ItemKind::Changed, TEMPO_CLAMPED.to_string()),
        (ItemKind::LeftOut, BAD_METER.to_string()),
        (ItemKind::LeftOut, MAP_ORDER.to_string()),
        (ItemKind::LeftOut, TEXT_DROPPED.to_string()),
    ] {
        has(r, kind, &what);
    }
    assert!(
        rules::validate(a.h.doc()).is_empty(),
        "{:?}",
        rules::validate(a.h.doc())
    );

    // BusA → BusB stays; BusB's output and send back to BusA are cut.
    let d = a.h.doc();
    let (bus_a, bus_b) = (bus(&a, "BusA"), bus(&a, "BusB"));
    assert_eq!(d.mixer().strip(bus_a).unwrap().output, Output::Bus(bus_b));
    assert_eq!(d.mixer().strip(bus_b).unwrap().output, Output::Master);
    assert!(d.mixer().strip(bus_b).unwrap().sends.is_empty());
    // The master's fader stays; +18 dB on Keys is cut to +12.
    let keys = midi_strip(&a, &lane(&a, "Keys"));
    assert_eq!(keys.gain_db, 12.0);
    assert_eq!(keys.sends.len(), 1, "only the send to a bus");
    assert!(keys.sends[0].pre_fader);
    // The first built-in-synth track sets the synth's strip.
    assert_eq!(midi_strip(&a, &lane(&a, "Synth1")).gain_db, 0.0);
    // Auto has no strip.
    let auto = lane(&a, "Auto");
    assert!(auto.midi_dest.is_none() && auto.audio_out.is_none());

    // Tempo in seconds: 120 ramping to 180, reached 2 s in; the transport's
    // 2000 BPM is clamped then replaced by the point at 0.
    let s = d.sequence(a.seq).unwrap();
    let pts = s.tempo.points();
    assert_eq!(pts.len(), 2);
    assert_eq!((pts[0].bpm, pts[0].ramp), (Bpm(120_000), true));
    assert_eq!(pts[1].bpm, Bpm(180_000));
    let at = s.tempo.tick_to_seconds(pts[1].tick);
    assert!((at - 2.0).abs() < 1e-3, "{at} s");
    // 3/4 from the transport replaced by 4/4 at 0; 3/5 and the late 5/4 out.
    let meter: Vec<(i64, Meter)> = s
        .meter
        .entries()
        .iter()
        .map(|e| (e.tick.0, e.meter))
        .collect();
    assert_eq!(
        meter,
        [
            (0, Meter { num: 4, den: 4 }),
            (16 * BEAT, Meter { num: 3, den: 4 })
        ]
    );
}
