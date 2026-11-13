//! DAWproject import of the package, the structure, the mixer, plug-ins,
//! the tempo and meter maps, markers and song info (plan 6d Task 6).

mod common;

use common::measure;
use core_io::IoError;
use core_io::dawproject::import::{
    BOTH_STRIPS, BUILTIN_EFFECTS, CANCEL_EVERY, COLOURS_AND_COMMENTS, COLOURS_NEAREST, DpImport,
    FOLDERS, FROM_VIZDAW, INFO_CUT, ImportEnv, METER_MOVED, MediaSource, NAMES_CUT, NOT_INSTALLED,
    OUTPUT_TO_TRACK, PLAYS_BUILTIN, PLUGIN_SETTINGS, TOO_LARGE_TO_CARRY, VCA, VST, apply,
    debug_items_after_pause, debug_pause_at, debug_paused, map,
};
use core_io::dawproject::package::{
    AudioSource, DpPackage, ODD_RATE, RECORD_UNREADABLE, SONG_INFO_UNREADABLE,
    debug_take_external_stats, open,
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

#[global_allocator]
static ALLOC: common::CountingAlloc = common::CountingAlloc;

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
        ids: None,
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

/// The item `what` of `kind` has a detail starting with `start`.
#[track_caller]
fn has_detail(r: &Report, kind: ItemKind, what: &str, start: &str) {
    let i = item(r, kind, what).unwrap_or_else(|| panic!("no {kind:?} '{what}' in {:#?}", r.items));
    assert!(
        i.details.iter().any(|d| d.starts_with(start)),
        "no detail '{start}…' in {:?}",
        i.details
    );
}

/// No item `what` has a detail starting with `start`.
#[track_caller]
fn lacks_detail(r: &Report, what: &str, start: &str) {
    assert!(
        r.items
            .iter()
            .filter(|i| i.what == what)
            .all(|i| i.details.iter().all(|d| !d.starts_with(start))),
        "unexpected '{what}' detail '{start}…' in {:#?}",
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

    has_detail(r, ItemKind::LeftOut, VST, "Bass VST");
    let eq = item(r, ItemKind::LeftOut, BUILTIN_EFFECTS).expect("EQ line");
    assert_eq!(eq.count, 1);
    assert_eq!(eq.details.len(), 2, "{:?}", eq.details);
    assert!(
        eq.details[0].starts_with("Equalizer on track 'Gtr': Band type=bell"),
        "{:?}",
        eq.details
    );
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
    lacks_detail(&a.report, NOT_INSTALLED, "Lead");
    has_detail(&a.report, ItemKind::Note, NOT_INSTALLED, "Delay");
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
    has_detail(&a.report, ItemKind::Note, NOT_INSTALLED, "Lead");
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
    has_detail(&a.report, ItemKind::LeftOut, PLUGIN_SETTINGS, "Delay");
    let gtr = audio_strip(&a, &lane(&a, "Gtr"));
    assert_eq!(gtr.inserts.len(), 1);
    assert_eq!(state_of(&a, gtr.inserts[0].id), None);
    lacks_detail(&a.report, PLUGIN_SETTINGS, "Lead");
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
    has(&a.report, ItemKind::LeftOut, COLOURS_AND_COMMENTS);
    lacks(&a.report, FROM_VIZDAW);

    let pkg = small(dir.path(), &four(["vz:t3", "vz:t4", "", ""]), None);
    assert_eq!(pkg.ret.tagged_tracks, 2);
    assert!(pkg.ret.from_vizdaw());
    let a = imported(&pkg, &yes);
    has(&a.report, ItemKind::Note, FROM_VIZDAW);
    lacks(&a.report, COLOURS_AND_COMMENTS);

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

    // A record packed tighter than 200:1 (its document is one long run):
    // only its first 64 KiB is unpacked, so the ratio does not apply.
    let record = format!(
        r#"{{"format":1,"project_id":"P","export_id":"E8","document":"{}"}}"#,
        "a".repeat(4_000_000)
    );
    let path = package(
        dir.path(),
        "TightRec",
        &[
            ("project.xml", project.as_bytes()),
            ("vizdaw/record.json", record.as_bytes()),
        ],
    );
    let pkg = open_ok(&path);
    assert_eq!(
        pkg.ret.record.as_ref().map(|h| h.export_id.as_str()),
        Some("E8")
    );
    lacks(&pkg.report, RECORD_UNREADABLE);
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
    has_detail(&a.report, ItemKind::Note, PLAYS_BUILTIN, "Plain");
    assert_eq!(
        midi_strip(&a, &lane(&a, "Plain")).kind,
        StripKind::BuiltinSynth
    );
}

#[test]
fn instruments_keep_the_files_names() {
    // The instrument a track makes is named from the file's channel, else
    // its instrument device, and from the track only when the file names
    // neither. Two tracks may share a name the file gives.
    let dir = tempfile::tempdir().unwrap();
    let track = |id: &str, name: &str, chan: &str, device: &str| {
        format!(
            r#"<Track id="{id}" name="{name}" contentType="notes"><Channel id="{id}c" {chan} role="regular" destination="mc"><Devices><ClapPlugin id="{id}d" {device} deviceID="org.example.{id}" deviceName="Plug-in {id}" deviceRole="instrument"/></Devices></Channel></Track>"#
        )
    };
    let structure = [
        track("a", "Lead", r#"name="Big Synth""#, r#"name="Device A""#),
        track("b", "Pad", r#"name="Big Synth""#, ""),
        track("c", "Bass", "", r#"name="Low End""#),
        track("d", "Keys", "", ""),
        track("e", "Arp", r#"name="  ""#, ""),
    ]
    .concat();
    let pkg = small(dir.path(), &project_xml(&structure, ""), None);
    let a = imported(&pkg, &yes);
    let dest_name = |track: &str| {
        let dest = lane(&a, track).midi_dest.expect("a destination");
        a.h.doc().instruments()[&dest].name.clone()
    };
    assert_eq!(dest_name("Lead"), "Big Synth");
    assert_eq!(dest_name("Pad"), "Big Synth");
    assert_eq!(dest_name("Bass"), "Low End");
    assert_eq!(dest_name("Keys"), "Keys");
    assert_eq!(dest_name("Arp"), "Arp", "a blank name is no name");
    assert_eq!(midi_strip(&a, &lane(&a, "Lead")).name, "Big Synth");
    assert_eq!(
        lane(&a, "Lead").name,
        "Lead",
        "the track keeps its own name"
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
    let flag = AtomicBool::new(false);
    assert_eq!(pkg.needed_bytes(4, &flag), Ok(5_000 + 576_000 + 192_000));
    assert_eq!(pkg.needed_bytes(2, &flag), Ok(5_000 + 288_000 + 96_000));
    assert_eq!(
        pkg.needed_bytes(4, &AtomicBool::new(true)),
        Err(IoError::Cancelled)
    );

    // A lying element: the size floor (file size × 4) wins.
    let liar = r#"<Clip time="0" duration="1"><Audio sampleRate="48000" channels="1" duration="0"><File path="audio/1.wav"/></Audio></Clip>"#.to_string();
    let project = with_audio(&[liar]);
    let path = package(
        dir.path(),
        "Liar",
        &[("project.xml", project.as_bytes()), ("audio/1.wav", &entry)],
    );
    assert_eq!(open_ok(&path).needed_bytes(4, &flag), Ok(5_000 + 20_000));

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
        ANALYZERS, BAD_METER, BUILTIN_INSTRUMENTS, EXTRA_INSTRUMENTS, LEVELS_CUT, LOOP_OUTPUTS,
        LOOP_SENDS, MAP_ORDER, MASTER_SENDS, NO_PLUGIN_ID, NO_STRIP, NOTE_EFFECTS, SEND_PAN,
        SEND_TARGETS, SHARED_SYNTH, TEMPO_CLAMPED, TEXT_DROPPED,
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
        (ItemKind::LeftOut, EXTRA_INSTRUMENTS.to_string()),
        (ItemKind::LeftOut, NOTE_EFFECTS.to_string()),
        (ItemKind::LeftOut, ANALYZERS.to_string()),
        (ItemKind::LeftOut, NO_PLUGIN_ID.to_string()),
        (ItemKind::LeftOut, SEND_TARGETS.to_string()),
        (ItemKind::LeftOut, SEND_PAN.to_string()),
        (ItemKind::Changed, LEVELS_CUT.to_string()),
        (ItemKind::LeftOut, SHARED_SYNTH.to_string()),
        (ItemKind::LeftOut, BUILTIN_INSTRUMENTS.to_string()),
        (ItemKind::LeftOut, NO_STRIP.to_string()),
        (ItemKind::LeftOut, COLOURS_AND_COMMENTS.to_string()),
        (ItemKind::Changed, TEMPO_CLAMPED.to_string()),
        (ItemKind::LeftOut, BAD_METER.to_string()),
        (ItemKind::LeftOut, MAP_ORDER.to_string()),
        (ItemKind::LeftOut, TEXT_DROPPED.to_string()),
    ] {
        has(r, kind, &what);
    }
    has_detail(r, ItemKind::LeftOut, EXTRA_INSTRUMENTS, "More on 'Keys'");
    has_detail(r, ItemKind::LeftOut, NOTE_EFFECTS, "Arp");
    has_detail(r, ItemKind::LeftOut, ANALYZERS, "Scope");
    has_detail(r, ItemKind::LeftOut, NO_PLUGIN_ID, "Nameless");
    has_detail(
        r,
        ItemKind::LeftOut,
        BUILTIN_INSTRUMENTS,
        "BuiltinDevice on track 'Organ'",
    );
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

/// A package of `entries`, each written with its own method.
fn package_with(dir: &Path, stem: &str, entries: &[(&str, &[u8], EntryMethod)]) -> PathBuf {
    let path = dir.join(format!("{stem}.dawproject"));
    let mut z = ZipWriter::new(std::fs::File::create(&path).unwrap());
    for (name, bytes, method) in entries {
        z.add(name, *method, bytes).unwrap();
    }
    z.finish().unwrap();
    path
}

/// `n` note tracks, each playing a CLAP instrument whose state is
/// `state(i)`.
fn tracks_with_states(n: usize, state: impl Fn(usize) -> String) -> String {
    let tracks: String = (0..n)
        .map(|i| {
            format!(
                r#"<Track id="t{i}" name="T{i}" contentType="notes"><Channel id="c{i}" role="regular"><Devices><ClapPlugin id="d{i}" deviceID="org.example.s" deviceName="S{i}" deviceRole="instrument"><State path="{}"/></ClapPlugin></Devices></Channel></Track>"#,
                state(i)
            )
        })
        .collect();
    project_xml(&tracks, "")
}

/// Bytes deflate cannot shrink much.
fn noise(n: usize, seed: u64) -> Vec<u8> {
    let mut x = seed | 1;
    (0..n)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x as u8
        })
        .collect()
}

#[test]
fn plugin_states_are_bounded() {
    let dir = tempfile::tempdir().unwrap();
    let flag = AtomicBool::new(false);

    // One deflated 64 MiB state (about 64 KiB packed) named by 1 000
    // devices: the ratio refuses it before it is unpacked, once.
    let zeros = vec![0u8; 64 << 20];
    let project = tracks_with_states(1_000, |_| "plugins/big.clap-preset".into());
    let path = package_with(
        dir.path(),
        "Zeros",
        &[
            ("project.xml", project.as_bytes(), EntryMethod::Deflated),
            ("plugins/big.clap-preset", &zeros, EntryMethod::Deflated),
        ],
    );
    drop(zeros);
    let pkg = open_ok(&path);
    let (imp, seen) = measure(|| mapped(&pkg, &yes));
    assert!(seen.peak < 16 << 20, "peak {} bytes", seen.peak);
    let line = item(&imp.report, ItemKind::LeftOut, PLUGIN_SETTINGS).expect("listed");
    assert_eq!(line.count, 1_000);

    // A 1 MiB state named by 1 000 devices is read once and shared.
    let state = noise(1 << 20, 7);
    let path = package_with(
        dir.path(),
        "Shared",
        &[
            ("project.xml", project.as_bytes(), EntryMethod::Deflated),
            ("plugins/big.clap-preset", &state, EntryMethod::Stored),
        ],
    );
    let pkg = open_ok(&path);
    let (imp, seen) = measure(|| mapped(&pkg, &yes));
    assert!(seen.peak < 8 << 20, "peak {} bytes", seen.peak);
    lacks(&imp.report, PLUGIN_SETTINGS);
    let (doc, seq) = new_doc();
    let mut h = History::new(doc);
    h.begin("Import DAWproject").unwrap();
    let states = apply(&mut h, &imp, seq, &mut Report::default()).unwrap();
    h.commit().unwrap();
    assert_eq!(states.len(), 1_000);
    assert!(states.iter().all(|(_, s)| Arc::ptr_eq(s, &states[0].1)));
    assert_eq!(&*states[0].1, &state[..]);

    // Distinct states past the total cap are left out as too large to carry.
    let entries: Vec<(String, Vec<u8>)> = (0..5)
        .map(|i| (format!("plugins/{i}.clap-preset"), noise(1 << 20, i + 1)))
        .collect();
    let project = tracks_with_states(5, |i| format!("plugins/{i}.clap-preset"));
    let mut files: Vec<(&str, &[u8], EntryMethod)> =
        vec![("project.xml", project.as_bytes(), EntryMethod::Deflated)];
    files.extend(
        entries
            .iter()
            .map(|(n, b)| (n.as_str(), b.as_slice(), EntryMethod::Stored)),
    );
    let path = package_with(dir.path(), "Many", &files);
    let lowered = PackageLimits {
        max_state_total: 3 << 20,
        ..PackageLimits::default()
    };
    let pkg = open(&path, &lowered, &flag).unwrap();
    let imp = mapped(&pkg, &yes);
    let line = item(&imp.report, ItemKind::LeftOut, PLUGIN_SETTINGS).expect("listed");
    assert_eq!(line.count, 2);
    assert!(
        line.details.iter().all(|d| d.ends_with(TOO_LARGE_TO_CARRY)),
        "{:?}",
        line.details
    );
}

#[test]
fn many_tracks_keep_the_report_small() {
    let dir = tempfile::tempdir().unwrap();
    let tracks: String = (0..core_io::limits::MAX_IMPORT_TRACKS)
        .map(|i| {
            format!(
                r#"<Track id="t{i}" name="T{i}" contentType="notes"><Channel id="c{i}" role="regular"/></Track>"#
            )
        })
        .collect();
    let pkg = small(dir.path(), &project_xml(&tracks, ""), None);
    let start = std::time::Instant::now();
    let imp = mapped(&pkg, &yes);
    let took = start.elapsed();
    assert!(took.as_secs() < 30, "{took:?}");
    assert!(imp.report.items.len() < 10, "{:#?}", imp.report.items.len());
    let line = item(&imp.report, ItemKind::Note, PLAYS_BUILTIN).expect("listed");
    assert_eq!(line.count, core_io::limits::MAX_IMPORT_TRACKS as u64);
    assert_eq!(line.details.len(), core_io::limits::MAX_DETAILS);
}

#[test]
fn opening_never_looks_at_external_files() {
    let dir = tempfile::tempdir().unwrap();
    let external = r#"<Clip time="0" duration="1"><Audio sampleRate="48000" channels="1" duration="1"><File path="/net/hostile.example/share/x.wav" external="true"/></Audio></Clip>"#;
    debug_take_external_stats();
    let pkg = small(
        dir.path(),
        &with_audio(&[external.to_string(), audio_clip("audio/1.wav", 48_000)]),
        None,
    );
    let info = pkg.info();
    assert_eq!(info.audio_files, 2);
    let outside = pkg
        .audio()
        .iter()
        .filter(|a| matches!(a.source, AudioSource::External(_)))
        .count();
    assert_eq!(outside, 1);
    assert_eq!(debug_take_external_stats(), 0, "open and info stat nothing");

    // The confirmed import's size check is where they are looked at.
    let outside = dir.path().join("o.wav");
    std::fs::write(&outside, vec![0u8; 10]).unwrap();
    let clip = format!(
        r#"<Clip time="0" duration="1"><Audio sampleRate="48000" channels="1" duration="1"><File path="{}" external="true"/></Audio></Clip>"#,
        outside.display()
    );
    let pkg = small(dir.path(), &with_audio(&[clip]), None);
    assert_eq!(debug_take_external_stats(), 0);
    assert_eq!(pkg.needed_bytes(4, &AtomicBool::new(false)), Ok(192_000));
    assert_eq!(debug_take_external_stats(), 1);
}

#[test]
fn eq_slot_units_and_states_map() {
    let dir = tempfile::tempdir().unwrap();
    use core_io::dawproject::import::{MASTER_PAN_SOLO, SEND_PAN};
    let structure = r##"
    <Track id="q" name="Q" contentType="notes">
      <Channel id="qc" role="regular" destination="mc">
        <Devices>
          <ClapPlugin id="qd" deviceID="org.example.q" deviceName="Inst" deviceRole="instrument"/>
          <ClapPlugin id="qe" comment="vz:q5" deviceID="org.example.eq" deviceName="Eq" deviceRole="audioFX"/>
          <ClapPlugin id="qf" deviceID="org.example.fx" deviceName="Fx" deviceRole="audioFX"/>
          <AuPlugin id="qg" deviceID="aufx/test/xmpl" deviceName="Ext" deviceRole="audioFX"><State path="/elsewhere/x.aupreset" external="true"/></AuPlugin>
        </Devices>
        <Pan unit="percent" value="50" min="-100" max="100"/>
        <Sends><Send id="qs" destination="bc" type="post"><Pan unit="linear" value="0" min="-1" max="1"/><Volume unit="linear" value="1"/></Send></Sends>
        <Volume unit="decibel" value="-6"/>
      </Channel>
    </Track>
    <Track id="n" name="N" contentType="audio">
      <Channel id="nc" role="regular" destination="mc"><Volume unit="normalized" value="0.5"/></Channel>
    </Track>
    <Track id="b" name="B" contentType="audio"><Channel id="bc" role="effect" destination="mc"/></Track>"##;
    let project = project_xml(structure, "").replace(
        r#"<Channel id="mc" role="master"/>"#,
        r#"<Channel id="mc" role="master" solo="true"><Pan unit="normalized" value="0.25"/></Channel>"#,
    );
    let pkg = small(dir.path(), &project, None);
    let a = imported(&pkg, &yes);
    let q = midi_strip(&a, &lane(&a, "Q"));
    assert_eq!(
        q.eq.as_ref().map(|e| e.plugin.id.as_str()),
        Some("org.example.eq")
    );
    let inserts: Vec<&str> = q.inserts.iter().map(|s| s.plugin.id.as_str()).collect();
    assert_eq!(inserts, ["org.example.fx", "aufx/test/xmpl"]);
    assert_eq!(q.gain_db, -6.0);
    assert!((q.pan - 0.5).abs() < 1e-9, "{}", q.pan);
    assert_eq!(q.sends.len(), 1);
    lacks(&a.report, SEND_PAN);
    // The external state is left out; the devices without one are not listed.
    let line = item(&a.report, ItemKind::LeftOut, PLUGIN_SETTINGS).expect("listed");
    assert_eq!(line.count, 1);
    assert!(line.details[0].starts_with("Ext"), "{:?}", line.details);
    let n = audio_strip(&a, &lane(&a, "N"));
    assert!((n.gain_db - -6.0206).abs() < 1e-3, "{}", n.gain_db);
    has(&a.report, ItemKind::LeftOut, MASTER_PAN_SOLO);
    let master =
        a.h.doc()
            .mixer()
            .strip(a.h.doc().mixer().master())
            .unwrap()
            .clone();
    assert_eq!(master.pan, 0.0);
    assert!(!master.solo);
}

#[test]
fn notes_and_audio_on_one_track_use_both_strips() {
    let dir = tempfile::tempdir().unwrap();
    let structure = r#"<Track id="t" name="Both" contentType="notes audio"><Channel id="tc" role="regular" destination="mc"><Devices><ClapPlugin id="td" deviceID="org.example.i" deviceName="I" deviceRole="instrument"/><ClapPlugin id="te" deviceID="org.example.fx" deviceName="Fx" deviceRole="audioFX"/></Devices></Channel></Track>"#;
    let pkg = small(dir.path(), &project_xml(structure, ""), None);
    let a = imported(&pkg, &yes);
    let l = lane(&a, "Both");
    assert_eq!(midi_strip(&a, &l).inserts.len(), 1);
    assert_eq!(audio_strip(&a, &l).inserts.len(), 1);
    has_detail(&a.report, ItemKind::Changed, BOTH_STRIPS, "Both");
}

#[test]
fn a_hostile_count_of_tracks_is_refused_at_once() {
    // Found by the hostile-XML property (plan 6d Task 8): applying n tracks
    // takes time quadratic in n (the model handles each command in time
    // linear in the project), so 100 000 tracks ran for
    // hours and held gigabytes. Past the limits the file is refused when it
    // is opened.
    let dir = tempfile::tempdir().unwrap();
    for (n, effect, why) in [
        (20_000, false, "more than 1000 tracks"),
        (20_000, true, "more than 256 buses"),
    ] {
        let structure: String = (0..n)
            .map(|i| {
                let role = if effect { "effect" } else { "regular" };
                format!(
                    r#"<Track id="t{i}" contentType="{}"><Channel id="c{i}" role="{role}" destination="mc"/></Track>"#,
                    if effect { "audio" } else { "notes" }
                )
            })
            .collect();
        let project = project_xml(&structure, "");
        let path = package(dir.path(), "Many", &[("project.xml", project.as_bytes())]);
        let start = std::time::Instant::now();
        let (r, seen) = measure(|| open(&path, &PackageLimits::default(), &AtomicBool::new(false)));
        let Err(IoError::Refused(s)) = r else {
            panic!("{n} tracks were not refused")
        };
        assert!(s.contains(why), "{s}");
        assert!(start.elapsed().as_secs() < 5, "{:?}", start.elapsed());
        assert!(
            seen.peak < 256 * 1024 * 1024,
            "peak {} MiB",
            seen.peak >> 20
        );
    }
}

// ---------------------------------------------------------------- origins (plan 6e Task 2)

mod origins {
    use super::*;
    use crate::common::merge;
    use crate::common::package::{TestPlist, write_package};
    use core_engine::fixtures::Q;
    use core_io::dawproject::export::{
        ClipRule, DpExport, ExportEnv, ExportOptions, RegionGain, export,
    };
    use core_io::dawproject::import::{Origins, TAGS_IGNORED, apply_tagged};
    use core_io::dawproject::record::{ObjectRef, read_snapshot};
    use core_media::import::Imported;
    use core_model::document::DestKind;
    use core_model::event::EventBody;
    use core_model::ids::{EventId, LaneId};
    use core_model::time::Tick;

    const PROJECT: &str = "00000000-0000-4000-8000-000000000001";
    const EXPORT: &str = "00000000-0000-4000-8000-0000000000e1";

    /// Task 1's fixture exported.
    fn exported() -> (Document, SeqId, DpExport) {
        let (doc, seq) = merge::fixture();
        let o = ExportOptions {
            seq,
            region_gain: RegionGain::Apply,
            clips: ClipRule::SilenceGap(Tick(4 * Q)),
            left_out: Default::default(),
        };
        let env = ExportEnv {
            plist: &TestPlist,
            app_version: "1.2.3",
            project_id: PROJECT,
            export_id: EXPORT,
            exported_at: 1_700_000_000,
        };
        let x = export(&doc, &o, &env, &AtomicBool::new(false)).expect("exports");
        (doc, seq, x)
    }

    /// `doc`'s `seq` exported.
    fn export_of(doc: &Document, seq: SeqId) -> DpExport {
        let o = ExportOptions {
            seq,
            region_gain: RegionGain::Apply,
            clips: ClipRule::SilenceGap(Tick(4 * Q)),
            left_out: Default::default(),
        };
        let env = ExportEnv {
            plist: &TestPlist,
            app_version: "1.2.3",
            project_id: PROJECT,
            export_id: EXPORT,
            exported_at: 1_700_000_000,
        };
        export(doc, &o, &env, &AtomicBool::new(false)).expect("exports")
    }

    /// The export's IDs, as the snapshot holds them.
    fn ids_of(x: &DpExport) -> BTreeMap<String, ObjectRef> {
        read_snapshot(x.snapshot_json.as_bytes())
            .expect("reads")
            .ids
    }

    /// Each audio file of `pkg` as the original media item of `doc`.
    fn media_of(doc: &Document, x: &DpExport, pkg: &DpPackage) -> BTreeMap<usize, MediaSource> {
        let mut media = BTreeMap::new();
        for a in pkg.audio() {
            let AudioSource::Entry(entry) = &a.source else {
                panic!("an exported file is outside the package")
            };
            let file = x.audio.iter().find(|f| &f.entry == entry).unwrap();
            let item = doc.media().get(&file.media).unwrap();
            media.insert(
                a.key,
                MediaSource::Online(vec![Imported {
                    path: format!("Media/{}.caf", a.name),
                    name: file.name.clone(),
                    original_name: item.name.clone(),
                    channels: item.channels,
                    sample_rate: item.sample_rate,
                    length_frames: item.length_frames,
                    content_hash: item.content_hash.clone().unwrap_or_default(),
                    clipped: 0,
                }]),
            );
        }
        media
    }

    struct Tagged {
        a: Applied,
        origins: Origins,
    }

    /// Maps `pkg` with `ids` trusted and applies it to a new project with
    /// [`apply_tagged`].
    fn tagged(
        pkg: &DpPackage,
        media: &BTreeMap<usize, MediaSource>,
        ids: Option<&BTreeMap<String, ObjectRef>>,
    ) -> Tagged {
        let env = ImportEnv { ids, ..env(&yes) };
        let imp = map(pkg, media, &env, &AtomicBool::new(false)).expect("maps");
        let (doc, seq) = new_doc();
        let mut h = History::new(doc);
        let mut report = Report::default();
        h.begin("Import DAWproject").unwrap();
        let (states, origins) = apply_tagged(&mut h, &imp, seq, &mut report).expect("applies");
        h.commit().unwrap();
        Tagged {
            a: Applied {
                h,
                seq,
                report,
                states,
            },
            origins,
        }
    }

    /// The package of `x`, its project.xml changed by `edit`.
    fn package_of(dir: &Path, x: &DpExport, edit: impl Fn(&str) -> String) -> DpPackage {
        let mut x = x.clone();
        x.project_xml = edit(&x.project_xml);
        open_ok(&write_package(dir, "Return", &x))
    }

    fn lane_of(doc: &Document, seq: SeqId, name: &str) -> Lane {
        doc.sequence(seq)
            .unwrap()
            .lanes
            .iter()
            .find(|l| l.name == name)
            .unwrap_or_else(|| panic!("no track {name}"))
            .clone()
    }

    /// The events of `l`'s active take that `keep` picks, by tick.
    fn events(l: &Lane, keep: impl Fn(&EventBody) -> bool) -> Vec<EventId> {
        let mut v: Vec<(i64, EventId)> = l
            .active()
            .unwrap()
            .events
            .values()
            .filter(|e| keep(&e.body))
            .map(|e| (e.tick.0, e.id))
            .collect();
        v.sort();
        v.into_iter().map(|(_, e)| e).collect()
    }

    fn is_use(b: &EventBody) -> bool {
        matches!(b, EventBody::Reference { .. })
    }

    fn is_region(b: &EventBody) -> bool {
        matches!(b, EventBody::AudioRegion { .. })
    }

    fn instrument_slot(doc: &Document, l: &Lane) -> PluginInstanceId {
        match &doc.instruments()[&l.midi_dest.unwrap()].kind {
            DestKind::PluginInstrument { slot } => slot.id,
            k => panic!("not a plug-in instrument: {k:?}"),
        }
    }

    fn bus_strip(doc: &Document, name: &str) -> StripId {
        doc.mixer()
            .strips()
            .values()
            .find(|s| s.kind == StripKind::Bus && s.name == name)
            .map(|s| s.id)
            .unwrap()
    }

    #[test]
    fn origins_follow_the_tags() {
        let (doc, seq, x) = exported();
        let dir = tempfile::tempdir().unwrap();
        let pkg = package_of(dir.path(), &x, str::to_owned);
        let t = tagged(&pkg, &media_of(&doc, &x, &pkg), None);
        lacks(&t.a.report, TAGS_IGNORED);
        let new = t.a.h.doc();
        let o = &t.origins;

        let (bass, keys) = (lane_of(&doc, seq, "Bass"), lane_of(&doc, seq, "Keys"));
        let nb = lane_of(new, t.a.seq, "Bass");
        let nk = lane_of(new, t.a.seq, "Keys");
        let na = lane_of(new, t.a.seq, "Keys audio");
        assert_eq!(
            o.lanes,
            BTreeMap::from([
                (nb.id, (bass.id.0, false)),
                (nk.id, (keys.id.0, false)),
                (na.id, (keys.id.0, true)),
            ])
        );

        let m = doc.mixer();
        let base_strip = |k: StripKind| m.find_kind(k).unwrap().0;
        let nm = new.mixer();
        let new_strip = |k: StripKind| nm.find_kind(k).unwrap();
        assert_eq!(
            o.strips,
            BTreeMap::from([
                (
                    new_strip(StripKind::Instrument(nb.midi_dest.unwrap())),
                    base_strip(StripKind::Instrument(bass.midi_dest.unwrap()))
                ),
                (
                    new_strip(StripKind::Instrument(nk.midi_dest.unwrap())),
                    base_strip(StripKind::Instrument(keys.midi_dest.unwrap()))
                ),
                (
                    new_strip(StripKind::AudioPath(na.audio_out.unwrap())),
                    base_strip(StripKind::AudioPath(keys.audio_out.unwrap()))
                ),
                (bus_strip(new, "Reverb"), bus_strip(&doc, "Reverb").0),
            ])
        );

        // The two uses and the two regions, each to its own.
        let mut want = BTreeMap::new();
        for (n, b) in events(&nb, is_use).into_iter().zip(events(&bass, is_use)) {
            want.insert(n, b.0);
        }
        for (n, b) in events(&na, is_region)
            .into_iter()
            .zip(events(&keys, is_region))
        {
            want.insert(n, b.0);
        }
        assert_eq!(want.len(), 4, "two uses and two regions");
        assert_eq!(o.events, want);

        assert_eq!(o.markers, vec![Some(0), Some(1)]);

        let bass_strip = m
            .strip(
                m.find_kind(StripKind::Instrument(bass.midi_dest.unwrap()))
                    .unwrap(),
            )
            .unwrap();
        let new_bass_strip = nm
            .strip(
                nm.find_kind(StripKind::Instrument(nb.midi_dest.unwrap()))
                    .unwrap(),
            )
            .unwrap();
        assert_eq!(
            o.slots,
            BTreeMap::from([
                (instrument_slot(new, &nb), instrument_slot(&doc, &bass).0),
                (instrument_slot(new, &nk), instrument_slot(&doc, &keys).0),
                (new_bass_strip.inserts[0].id, bass_strip.inserts[0].id.0),
            ])
        );
    }

    #[test]
    fn our_repeated_tags_are_not_ignored() {
        let (doc, seq) = merge::shared_fixture();
        let x = export_of(&doc, seq);
        // The shapes are there: the pad strip's channel twice, the use's
        // tag on more than one clip.
        let pad = lane_of(&doc, seq, "Pad A");
        let pad_strip = doc
            .mixer()
            .find_kind(StripKind::Instrument(pad.midi_dest.unwrap()))
            .unwrap();
        let strip_tag = format!(r#"comment="vz:s{}""#, pad_strip.0);
        assert_eq!(x.project_xml.matches(&strip_tag).count(), 2, "two copies");
        let lp = lane_of(&doc, seq, "Loop");
        let use_id = events(&lp, is_use)[0];
        let use_tag = format!(r#"comment="vz:e{}""#, use_id.0);
        assert!(
            x.project_xml.matches(&use_tag).count() > 1,
            "one clip per pass"
        );

        let dir = tempfile::tempdir().unwrap();
        let pkg = package_of(dir.path(), &x, str::to_owned);
        let ids = ids_of(&x);
        for trusted in [None, Some(&ids)] {
            let t = tagged(&pkg, &BTreeMap::new(), trusted);
            lacks(&t.a.report, TAGS_IGNORED);
            let new = t.a.h.doc();
            let o = &t.origins;
            let nm = new.mixer();
            let base = doc.mixer().strip(pad_strip).unwrap();
            for name in ["Pad A", "Pad B"] {
                let l = lane_of(new, t.a.seq, name);
                let base_lane = lane_of(&doc, seq, name);
                assert_eq!(o.lanes[&l.id], (base_lane.id.0, false), "{name}");
                let s = nm
                    .find_kind(StripKind::Instrument(l.midi_dest.unwrap()))
                    .unwrap();
                assert_eq!(o.strips[&s], pad_strip.0, "{name}'s strip");
                assert_eq!(
                    o.slots[&instrument_slot(new, &l)],
                    instrument_slot(&doc, &pad).0,
                    "{name}'s instrument"
                );
                let insert = nm.strip(s).unwrap().inserts[0].id;
                assert_eq!(o.slots[&insert], base.inserts[0].id.0, "{name}'s insert");
            }
            let nl = lane_of(new, t.a.seq, "Loop");
            let top: Vec<EventId> = nl.active().unwrap().events.values().map(|e| e.id).collect();
            assert!(top.len() > 1, "{top:?}");
            for e in &top {
                assert_eq!(o.events.get(e), Some(&use_id.0), "every pass is the use");
            }
        }
    }

    #[test]
    fn trusted_ids_win_over_tags() {
        let (doc, seq, x) = exported();
        let (bass, keys) = (lane_of(&doc, seq, "Bass"), lane_of(&doc, seq, "Keys"));
        let (tb, tk) = (format!("vz:t{}", bass.id.0), format!("vz:t{}", keys.id.0));
        // The Bass track's comment and the Keys track's swapped. The Keys
        // track comes before its audio part, which keeps its tag.
        let swap = |xml: &str| {
            let (cb, ck) = (format!(r#"comment="{tb}""#), format!(r#"comment="{tk}""#));
            assert_eq!(xml.matches(&cb).count(), 1);
            assert_eq!(
                xml.matches(&ck).count(),
                2,
                "the Keys track and its audio part"
            );
            xml.replacen(&ck, "\u{0}", 1)
                .replace(&cb, &ck)
                .replace('\u{0}', &cb)
        };
        let dir = tempfile::tempdir().unwrap();
        let pkg = package_of(dir.path(), &x, swap);
        let media = media_of(&doc, &x, &pkg);
        let ids = ids_of(&x);
        let lanes = |o: &Origins, new: &Document, s: SeqId| -> Vec<(u64, bool)> {
            ["Bass", "Keys", "Keys audio"]
                .iter()
                .map(|n| o.lanes[&lane_of(new, s, n).id])
                .collect()
        };

        let t = tagged(&pkg, &media, Some(&ids));
        assert_eq!(
            lanes(&t.origins, t.a.h.doc(), t.a.seq),
            vec![(bass.id.0, false), (keys.id.0, false), (keys.id.0, true)],
            "the IDs win"
        );
        lacks(&t.a.report, TAGS_IGNORED);

        let t = tagged(&pkg, &media, None);
        assert_eq!(
            lanes(&t.origins, t.a.h.doc(), t.a.seq),
            vec![(keys.id.0, false), (bass.id.0, false), (keys.id.0, true)],
            "the tags, swapped"
        );
    }

    /// The hostile file: hostile tags and IDs, and `clips` one-note clips
    /// on track A all tagged `vz:e5`, each starting before the one before
    /// it ends (so they are not one use's passes).
    fn hostile(clips: usize) -> String {
        let structure = concat!(
            r#"<Track id="ta" name="A" comment="vz:t1" contentType="notes"/>"#,
            r#"<Track id="tb" name="B" comment="vz:t1" contentType="notes"/>"#,
            r#"<Track id="tc" name="C" comment="vz:t999" contentType="notes"/>"#,
            r#"<Track id="td" name="D" comment="vz:s3" contentType="notes"/>"#,
            r#"<Track id="t1" name="E" contentType="notes"/>"#,
        );
        let mut s = String::with_capacity(clips * 160);
        for i in 0..clips {
            s.push_str(&format!(
                r#"<Clip time="{}" duration="1" comment="vz:e5"><Notes><Note time="0" duration="0.5" channel="0" key="60" vel="0.5"/></Notes></Clip>"#,
                i as f64 / 2.0
            ));
        }
        let arrangement =
            format!(r#"<Lanes timeUnit="beats"><Clips track="ta">{s}</Clips></Lanes>"#);
        project_xml(structure, &arrangement)
    }

    #[test]
    fn hostile_tags_are_ignored_and_counted() {
        let dir = tempfile::tempdir().unwrap();
        let project = hostile(10_000);
        let path = package_as(
            dir.path(),
            "Hostile",
            &[("project.xml", project.as_bytes())],
            false,
        );
        let pkg = open_ok(&path);
        let ids = BTreeMap::from([(
            "t1".to_owned(),
            ObjectRef {
                kind: "strip".into(),
                id: 1,
                slot: None,
                index: None,
            },
        )]);
        let start = std::time::Instant::now();
        let t = tagged(&pkg, &BTreeMap::new(), Some(&ids));
        let took = start.elapsed();
        let o = &t.origins;
        let new = t.a.h.doc();
        let id = |n: &str| -> LaneId { lane_of(new, t.a.seq, n).id };

        assert_eq!(o.lanes.values().filter(|(l, _)| *l == 1).count(), 1);
        assert_eq!(o.lanes.get(&id("A")), Some(&(1, false)));
        assert_eq!(o.lanes.get(&id("C")), Some(&(999, false)), "kept as named");
        for n in ["B", "D", "E"] {
            assert_eq!(o.lanes.get(&id(n)), None, "track {n}");
        }
        assert!(o.lanes.values().all(|(l, _)| *l != 3), "{:?}", o.lanes);
        assert!(
            o.strips.values().all(|s| *s != 3 && *s != 1),
            "{:?}",
            o.strips
        );

        let fives: Vec<EventId> = o
            .events
            .iter()
            .filter(|(_, v)| **v == 5)
            .map(|(e, _)| *e)
            .collect();
        assert_eq!(fives.len(), 1, "{} events map to 5", fives.len());
        assert_eq!(o.events.len(), 1);
        let a = lane_of(new, t.a.seq, "A");
        let firstclip = a
            .active()
            .unwrap()
            .events
            .values()
            .min_by_key(|e| e.tick)
            .unwrap();
        assert_eq!(firstclip.tick, Tick(0));
        assert_eq!(fives[0], firstclip.id, "the first clip's note");

        let i = item(&t.a.report, ItemKind::Note, TAGS_IGNORED)
            .unwrap_or_else(|| panic!("no '{TAGS_IGNORED}' in {:#?}", t.a.report.items));
        assert_eq!(i.count, 10_002);
        assert!(took.as_secs_f64() < 2.0, "{took:?}");
    }

    #[test]
    fn apply_still_matches_6d() {
        let dir = tempfile::tempdir().unwrap();
        let (doc, _, x) = exported();
        let returned = package_of(dir.path(), &x, str::to_owned);
        let media = media_of(&doc, &x, &returned);
        let mut cases: Vec<(String, DpPackage, BTreeMap<usize, MediaSource>)> = vec![
            ("hand_other".into(), hand_other(dir.path()), BTreeMap::new()),
            ("export".into(), returned, media),
        ];
        for (name, project) in [
            ("many_notes", many_notes(100)),
            ("hostile", hostile(50)),
            (
                "clap",
                project_xml(
                    &(clap_track("A", "a", "vz:t3 lead", "mc")
                        + &clap_track("B", "b", "vz:t3", "mc")),
                    "",
                ),
            ),
        ] {
            let pkg = small(dir.path(), &project, None);
            cases.push((name.into(), pkg, BTreeMap::new()));
        }
        for (name, pkg, media) in cases {
            let imp = map(&pkg, &media, &env(&yes), &AtomicBool::new(false)).expect("maps");
            let run = |tagged: bool| {
                let (doc, seq) = new_doc();
                let mut h = History::new(doc);
                let mut report = Report::default();
                h.begin("Import DAWproject").unwrap();
                let states = if tagged {
                    apply_tagged(&mut h, &imp, seq, &mut report).unwrap().0
                } else {
                    apply(&mut h, &imp, seq, &mut report).unwrap()
                };
                h.commit().unwrap();
                (h.doc().clone(), report, states)
            };
            assert_eq!(run(false), run(true), "{name}");
        }
    }
}
