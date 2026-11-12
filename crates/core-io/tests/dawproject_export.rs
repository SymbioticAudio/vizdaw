//! DAWproject export of the structure, mixer, plug-ins, maps, metadata, tags,
//! the return record and the projection (plan 6d Task 4).

mod common;

use common::xmllint::schema_check;
use core_engine::fixtures::{BAR, Builder, Q};
use core_io::IoError;
use core_io::dawproject::export::{
    ClipRule, DpExport, ExportEnv, ExportOptions, RegionGain, check, export, file_name,
};
use core_io::dawproject::plugins::{NoPlist, PlistCodec, state_entry};
use core_io::dawproject::record::{ObjectRef, Record, Snapshot, projection};
use core_io::dawproject::schema::*;
use core_io::dawproject::units::{
    EXPORT_LINE, TagKind, beats, db_of_linear, linear_gain, nearest_swatch, pan_normalized,
    pan_of_normalized, strip_tags, swatch_hex, tag, ticks,
};
use core_io::limits::PackageLimits;
use core_io::report::{ItemKind, Report};
use core_io::xml::Tree;
use core_model::automation::AutomationMode;
use core_model::automation::AutomationTarget;
use core_model::command::{Command, LaneCmd, MixerCmd, RoutingCmd, SequenceCmd, SettingsCmd};
use core_model::document::{Document, SongInfo};
use core_model::event::{EventBody, Fade};
use core_model::file;
use core_model::ids::{AnyId, LaneId, OutputPathId, PluginInstanceId, SeqId, StripId};
use core_model::input_fx::{InputFx, InputFxKind};
use core_model::lane::LaneInput;
use core_model::learn::{LearnKind, LearnMap, LearnMode, LearnSource};
use core_model::meter::{KeySig, Meter, MeterEntry, MeterMap};
use core_model::mixer::{AuxSend, ParamId, PluginFormat, PluginRef, StripKind, Swatch};
use core_model::mpe::MpeConfig;
use core_model::sequence::{KeySlot, Marker};
use core_model::time::Tick;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/dawproject");

// ---------------------------------------------------------------- helpers

/// The AU codec the tests use: the XML form is the bytes in hex.
struct FakePlist;

impl PlistCodec for FakePlist {
    fn to_xml(&self, binary: &[u8]) -> Result<Vec<u8>, String> {
        let hex: String = binary.iter().map(|b| format!("{b:02x}")).collect();
        Ok(format!("<plist><data>{hex}</data></plist>").into_bytes())
    }

    fn to_binary(&self, _plist: &[u8]) -> Result<Vec<u8>, String> {
        Err("not used".into())
    }
}

fn env<'a>(plist: &'a dyn PlistCodec) -> ExportEnv<'a> {
    ExportEnv {
        plist,
        app_version: "1.2.3",
        project_id: "00000000-0000-4000-8000-000000000001",
        export_id: "E1",
        exported_at: 1_700_000_000,
    }
}

fn opts(seq: SeqId) -> ExportOptions {
    ExportOptions {
        seq,
        region_gain: RegionGain::Apply,
        clips: ClipRule::WholeTrack,
    }
}

fn run(doc: &Document, seq: SeqId) -> DpExport {
    export(doc, &opts(seq), &env(&FakePlist), &AtomicBool::new(false)).expect("exports")
}

fn run_with(doc: &Document, seq: SeqId, plist: &dyn PlistCodec) -> DpExport {
    export(doc, &opts(seq), &env(plist), &AtomicBool::new(false)).expect("exports")
}

/// The never-blessed golden reader of 6a: a hand-written file, byte-exact.
#[track_caller]
fn hand_golden(name: &str, got: &[u8]) {
    let want = std::fs::read(format!("{DIR}/{name}"))
        .unwrap_or_else(|e| panic!("hand-written golden {name} is missing: {e}"));
    assert_eq!(
        String::from_utf8_lossy(got),
        String::from_utf8_lossy(&want),
        "{name}"
    );
}

fn parse(xml: &str) -> Tree {
    Tree::parse(
        xml.as_bytes(),
        &PackageLimits::default(),
        &AtomicBool::new(false),
    )
    .expect("parses")
}

fn read_back(x: &DpExport) -> DpProject {
    let mut r = Report::default();
    let p = read_project(&parse(&x.project_xml), &mut r).expect("reads back");
    assert!(r.is_empty(), "reading back: {:?}", r.items);
    p
}

fn metadata_back(x: &DpExport) -> DpMetaData {
    let mut r = Report::default();
    let m = read_metadata(&parse(&x.metadata_xml), &mut r);
    assert!(r.is_empty(), "reading back: {:?}", r.items);
    m
}

/// Every track, depth first.
fn tracks(p: &DpProject) -> Vec<&DpTrack> {
    fn walk<'a>(t: &'a DpTrack, out: &mut Vec<&'a DpTrack>) {
        out.push(t);
        for c in &t.children {
            walk(c, out);
        }
    }
    let mut out = Vec::new();
    for item in &p.structure {
        if let DpStructureItem::Track(t) = item {
            walk(t, &mut out);
        }
    }
    out
}

fn track<'a>(p: &'a DpProject, id: &str) -> &'a DpTrack {
    tracks(p)
        .into_iter()
        .find(|t| t.named.id.as_deref() == Some(id))
        .unwrap_or_else(|| panic!("no track {id}"))
}

fn channel(t: &DpTrack) -> &DpChannel {
    t.channel.as_deref().expect("a channel")
}

fn count(r: &Report, kind: ItemKind, what: &str) -> usize {
    r.items
        .iter()
        .filter(|i| i.kind == kind && i.what == what)
        .count()
}

fn item<'a>(r: &'a Report, kind: ItemKind, what: &str) -> &'a core_io::report::Item {
    r.items
        .iter()
        .find(|i| i.kind == kind && i.what == what)
        .unwrap_or_else(|| panic!("no {kind:?} '{what}' in {:#?}", r.items))
}

fn validates(x: &DpExport) {
    schema_check(&format!("{DIR}/Project.xsd"), &x.project_xml);
    schema_check(&format!("{DIR}/MetaData.xsd"), &x.metadata_xml);
    let p = read_back(x);
    let mut r = Report::default();
    check_ids(&p, &mut r).expect("IDs are unique and resolve");
    assert!(r.is_empty(), "ID check: {:?}", r.items);
}

fn synth_ref() -> PluginRef {
    PluginRef {
        format: PluginFormat::Clap,
        id: "org.example.synth".into(),
        name: "Synth".into(),
        vendor: "Example".into(),
        version: "1.0".into(),
    }
}

fn delay_ref() -> PluginRef {
    PluginRef {
        format: PluginFormat::Au,
        id: "aufx/dely/xmpl".into(),
        name: "Delay".into(),
        vendor: "Example".into(),
        version: "2.0".into(),
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

fn capture(b: Builder, slot: PluginInstanceId, bytes: &[u8]) -> Builder {
    let (doc, _) = core_model::state::capture(b.doc(), slot, bytes).unwrap();
    Builder::from_doc(doc)
}

fn info(b: &mut Builder, info: SongInfo) {
    b.run(Command::Settings(SettingsCmd::SetInfo { info }))
        .unwrap();
}

fn flags(b: &mut Builder, seq: SeqId, lane: LaneId, mute: Option<bool>, solo: Option<bool>) {
    b.run(Command::Lane(LaneCmd::SetFlags {
        seq,
        lane,
        mute,
        solo,
        arm: None,
    }))
    .unwrap();
}

/// IDs of the structure fixture.
struct Structure {
    doc: Document,
    seq: SeqId,
    keys: LaneId,
    drums: LaneId,
    synth_strip: StripId,
    drum_strip: StripId,
    verb: StripId,
    synth: PluginInstanceId,
    delay: PluginInstanceId,
}

/// Step 1's fixture: "Song" at 120 BPM ramping to 140 at bar 5, 4/4 → 3/4
/// at bar 9, markers "Verse" (bar 1) and "Chorus" (bar 9); "Keys" on a CLAP
/// synth (state "abc") with an AU delay insert (4 bytes of state); "Drums"
/// on audio path "Drums", Blue, with a pre-fader send at −6 dB to bus
/// "Verb"; the master at −1.5 dB; song info T, A and c. No events.
fn structure_fixture() -> Structure {
    let mut b = Builder::new();
    let (dest, synth) = b.plugin_dest("Synth", synth_ref()).unwrap();
    let (seq, keys) = b.sequence("Song", "Keys", Some(dest)).unwrap();
    let drums = b.lane(seq, "Drums", None).unwrap();
    let out = output(&mut b, "Drums");
    route(&mut b, seq, drums, out);
    let drum_strip = b.strip_of(StripKind::AudioPath(out)).unwrap();
    let synth_strip = b.strip_of(StripKind::Instrument(dest)).unwrap();
    let verb = b.bus("Verb").unwrap();
    let delay = b.insert(synth_strip, delay_ref()).unwrap();
    b.mixer(MixerCmd::SetColour {
        strip: drum_strip,
        colour: Some(Swatch::Blue),
    })
    .unwrap();
    b.mixer(MixerCmd::AddSend {
        strip: drum_strip,
        send: AuxSend {
            target: verb,
            level_db: -6.0,
            pre_fader: true,
            enabled: true,
        },
    })
    .unwrap();
    let master = b.doc().mixer().master();
    b.mixer(MixerCmd::SetGain {
        strip: master,
        gain_db: -1.5,
    })
    .unwrap();
    b.tempo(seq, &[(0, 120, true), (4 * BAR, 140, false)])
        .unwrap();
    let meter = MeterMap::new(vec![
        MeterEntry {
            tick: Tick(0),
            meter: Meter { num: 4, den: 4 },
            key: KeySig::default(),
        },
        MeterEntry {
            tick: Tick(8 * BAR),
            meter: Meter { num: 3, den: 4 },
            key: KeySig::default(),
        },
    ])
    .unwrap();
    b.run(Command::Sequence(SequenceCmd::SetMeter { seq, meter }))
        .unwrap();
    b.run(Command::Sequence(SequenceCmd::SetMarkers {
        seq,
        markers: vec![
            Marker {
                tick: Tick(0),
                name: "Verse".into(),
            },
            Marker {
                tick: Tick(8 * BAR),
                name: "Chorus".into(),
            },
        ]
        .into_iter()
        .collect(),
    }))
    .unwrap();
    info(
        &mut b,
        SongInfo {
            title: Some("T".into()),
            artist: Some("A".into()),
            comment: Some("c".into()),
            ..SongInfo::default()
        },
    );
    let b = capture(b, synth, b"abc");
    let b = capture(b, delay, &[0x62, 0x70, 0x6c, 0x00]);
    Structure {
        doc: b.finish(),
        seq,
        keys,
        drums,
        synth_strip,
        drum_strip,
        verb,
        synth,
        delay,
    }
}

// ---------------------------------------------------------------- tests

#[test]
fn structure_matches_the_hand_golden() {
    let f = structure_fixture();
    let x = run(&f.doc, f.seq);
    hand_golden("export_structure.project.xml", x.project_xml.as_bytes());
    hand_golden("export_structure.metadata.xml", x.metadata_xml.as_bytes());
    assert_eq!(
        x.states,
        vec![
            ("plugins/001.clap-preset".to_string(), b"abc".to_vec()),
            (
                "plugins/002.aupreset".to_string(),
                b"<plist><data>62706c00</data></plist>".to_vec()
            ),
        ]
    );
    assert!(x.audio.is_empty());
    assert!(x.report.is_empty(), "{:#?}", x.report.items);
    assert_eq!(x.report.summary.tracks, 2);
    assert_eq!(x.report.summary.plugins, 2);
    assert_eq!(x.report.summary.markers, 2);
    assert!(x.report.summary.tempo_map);
    // The fixture's IDs are the ones the golden names.
    assert_eq!(
        (
            f.keys.0,
            f.drums.0,
            f.synth_strip.0,
            f.drum_strip.0,
            f.verb.0
        ),
        (0, 1, 1, 2, 3)
    );
    assert_eq!((f.synth.0, f.delay.0), (0, 1));
}

#[test]
fn exports_pass_the_vendored_schema() {
    let f = structure_fixture();
    validates(&run(&f.doc, f.seq));
    validates(&run_with(&f.doc, f.seq, &NoPlist));
    let (doc, seq) = listed_fixture();
    validates(&run_with(&doc, seq, &NoPlist));
    let (doc, seq, _, _) = shared_instrument_fixture();
    validates(&run(&doc, seq));
    let (doc, seq, _, _, _) = shared_audio_fixture();
    validates(&run(&doc, seq));
    let (doc, seq, _) = mixed_fixture();
    validates(&run(&doc, seq));
    let (doc, seq) = pickup_fixture();
    validates(&run(&doc, seq));
}

fn shared_instrument_fixture() -> (Document, SeqId, [LaneId; 2], (StripId, PluginInstanceId)) {
    let mut b = Builder::new();
    let (dest, slot) = b.plugin_dest("Synth", synth_ref()).unwrap();
    let (seq, one) = b.sequence("Song", "One", Some(dest)).unwrap();
    let two = b.lane(seq, "Two", Some(dest)).unwrap();
    let strip = b.strip_of(StripKind::Instrument(dest)).unwrap();
    let b = capture(b, slot, b"xyz");
    (b.finish(), seq, [one, two], (strip, slot))
}

fn shared_audio_fixture() -> (Document, SeqId, LaneId, LaneId, StripId) {
    let mut b = Builder::new();
    let (seq, one) = b.sequence("Song", "One", None).unwrap();
    let two = b.lane(seq, "Two", None).unwrap();
    let out = output(&mut b, "Room");
    route(&mut b, seq, one, out);
    route(&mut b, seq, two, out);
    let strip = b.strip_of(StripKind::AudioPath(out)).unwrap();
    (b.finish(), seq, one, two, strip)
}

#[test]
fn shared_strips() {
    // Two tracks on one plug-in instrument: copied to each track.
    let (doc, seq, [one, two], (s, i)) = shared_instrument_fixture();
    let x = run(&doc, seq);
    let p = read_back(&x);
    for l in [one, two] {
        let t = track(&p, &format!("t{}", l.0));
        let c = channel(t);
        assert_eq!(
            c.named.id.as_deref(),
            Some(format!("c{}t{}", s.0, l.0).as_str())
        );
        assert_eq!(
            c.named.comment.as_deref(),
            Some(format!("vz:s{}", s.0).as_str())
        );
        let d = c.devices.first().expect("the instrument");
        assert_eq!(d.kind, DpDeviceKind::Clap);
        assert_eq!(d.role, DpDeviceRole::Instrument);
        assert_eq!(
            d.named.id.as_deref(),
            Some(format!("c{}t{}d{}", s.0, l.0, i.0).as_str())
        );
    }
    assert_eq!(x.states.len(), 2);
    assert_eq!(x.states[0].1, x.states[1].1);
    assert_eq!(x.states[0].1, b"xyz".to_vec());
    assert_eq!(x.states[0].0, "plugins/001.clap-preset");
    assert_eq!(x.states[1].0, "plugins/002.clap-preset");
    let what = "Instrument 'Synth' shared by several tracks was copied to each";
    let it = item(&x.report, ItemKind::Changed, what);
    assert_eq!(it.count, 1);
    assert_eq!(it.details, vec!["2 tracks".to_string()]);

    // Two tracks on one audio path: a submix channel in a bus track.
    let (doc, seq, one, two, s) = shared_audio_fixture();
    let x = run(&doc, seq);
    let p = read_back(&x);
    let bus = track(&p, &format!("b{}", s.0));
    assert_eq!(bus.content, vec![DpContentType::Audio]);
    let sub = channel(bus);
    assert_eq!(sub.named.id.as_deref(), Some(format!("c{}", s.0).as_str()));
    assert_eq!(sub.role, Some(DpMixerRole::Submix));
    for l in [one, two] {
        let c = channel(track(&p, &format!("t{}", l.0)));
        assert_eq!(c.named.id.as_deref(), Some(format!("k{}", l.0).as_str()));
        assert_eq!(c.destination.as_deref(), Some(format!("c{}", s.0).as_str()));
        assert!(c.devices.is_empty());
        assert_eq!(c.volume.as_ref().and_then(|v| v.value), Some(1.0));
        assert_eq!(c.pan.as_ref().and_then(|v| v.value), Some(0.5));
    }
    let it = item(
        &x.report,
        ItemKind::Changed,
        "Audio paths shared by several tracks became submix channels",
    );
    assert_eq!(it.details, vec!["Room".to_string()]);
}

fn mixed_fixture() -> (Document, SeqId, LaneId) {
    let mut b = Builder::new();
    let (dest, _) = b.plugin_dest("Synth", synth_ref()).unwrap();
    let (seq, keys) = b.sequence("Song", "Keys", Some(dest)).unwrap();
    let out = output(&mut b, "Keys out");
    route(&mut b, seq, keys, out);
    b.notes(seq, keys, &[(0, 60, 0x8000, Q)]).unwrap();
    let media = b.media("Media/a.caf", 48_000, 2).unwrap();
    b.region(
        seq,
        keys,
        2 * BAR,
        media,
        0,
        48_000,
        0.0,
        Fade::default(),
        Fade::default(),
    )
    .unwrap();
    (b.finish(), seq, keys)
}

#[test]
fn mixed_track_splits() {
    let (doc, seq, keys) = mixed_fixture();
    let x = run(&doc, seq);
    let p = read_back(&x);
    let notes = track(&p, &format!("t{}", keys.0));
    assert_eq!(notes.named.name.as_deref(), Some("Keys"));
    assert_eq!(notes.content, vec![DpContentType::Notes]);
    let audio = track(&p, &format!("t{}a", keys.0));
    assert_eq!(audio.named.name.as_deref(), Some("Keys audio"));
    assert_eq!(audio.content, vec![DpContentType::Audio]);
    assert_eq!(
        audio.named.comment.as_deref(),
        Some(format!("vz:t{}", keys.0).as_str())
    );
    let it = item(
        &x.report,
        ItemKind::Changed,
        "Tracks with notes and audio were split in two",
    );
    assert_eq!(it.details, vec!["Keys".to_string()]);
    assert_eq!(x.report.summary.tracks, 2);
}

/// A document with every state the export lists (with `NoPlist`).
fn listed_fixture() -> (Document, SeqId) {
    let mut b = Builder::new();
    let (synth, synth_slot) = b.plugin_dest("Synth", synth_ref()).unwrap();
    let ext = b.dest("Ext", "port:ext", 0).unwrap();
    let gm = b.dest("GM", "builtin:synth", 0).unwrap();
    let (seq, keys) = b.sequence("Song", "Keys", Some(synth)).unwrap();
    let pad = b.lane(seq, "Pad", Some(synth)).unwrap();
    let ext_lane = b.lane(seq, "Ext", Some(ext)).unwrap();
    let gm_lane = b.lane(seq, "GM", Some(gm)).unwrap();
    let room_a = b.lane(seq, "Room A", None).unwrap();
    let room_b = b.lane(seq, "Room B", None).unwrap();
    let both = b.lane(seq, "Both", None).unwrap();
    let muted = b.lane(seq, "Muted", None).unwrap();
    let solo = b.lane(seq, "Solo", None).unwrap();
    let room = output(&mut b, "Room");
    route(&mut b, seq, room_a, room);
    route(&mut b, seq, room_b, room);
    let both_out = output(&mut b, "Both out");
    route(&mut b, seq, both, both_out);
    let room_strip = b.strip_of(StripKind::AudioPath(room)).unwrap();
    let synth_strip = b.strip_of(StripKind::Instrument(synth)).unwrap();

    // Split: a note and a region on one track.
    b.notes(seq, both, &[(0, 60, 0x8000, Q)]).unwrap();
    let media = b.media("Media/a.caf", 48_000, 2).unwrap();
    b.region(
        seq,
        both,
        BAR,
        media,
        0,
        48_000,
        0.0,
        Fade::default(),
        Fade::default(),
    )
    .unwrap();
    b.notes(seq, ext_lane, &[(0, 60, 0x8000, Q)]).unwrap();
    b.notes(seq, gm_lane, &[(0, 36, 0x8000, Q)]).unwrap();

    // Inactive take: a note, then a new (active) take.
    b.notes(seq, keys, &[(0, 64, 0x8000, Q)]).unwrap();
    b.run(Command::Lane(LaneCmd::AddTake {
        seq,
        lane: keys,
        name: "Take 2".into(),
    }))
    .unwrap();
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
    b.drum_rows(seq, gm_lane, &[("Snare", 38)]).unwrap();
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
    let delay = b.insert(synth_strip, delay_ref()).unwrap();
    b.mixer(MixerCmd::SetSidechain {
        slot: delay,
        source: Some(room_strip),
    })
    .unwrap();
    b.mixer(MixerCmd::AddInstrumentOutput {
        dest: synth,
        index: 1,
        name: "Synth 2".into(),
    })
    .unwrap();
    let meter = MeterMap::new(vec![MeterEntry {
        tick: Tick(0),
        meter: Meter { num: 4, den: 4 },
        key: KeySig {
            sharps: 2,
            minor: false,
        },
    }])
    .unwrap();
    b.run(Command::Sequence(SequenceCmd::SetMeter { seq, meter }))
        .unwrap();
    b.event(
        seq,
        pad,
        0,
        EventBody::Automation {
            target: AutomationTarget::PluginParam(synth_slot, ParamId(1 << 33)),
            value: 0.5,
        },
    )
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
    b.run(Command::Sequence(SequenceCmd::SetStartOffset {
        seq,
        offset: Tick(Q),
    }))
    .unwrap();
    flags(&mut b, seq, muted, Some(true), None);
    flags(&mut b, seq, solo, None, Some(true));
    let b = capture(b, delay, &[1, 2, 3, 4]);
    (b.finish(), seq)
}

/// Every Left out, Changed and Note line of the brief, as the listed
/// fixture gives them.
fn listed_lines() -> Vec<(ItemKind, &'static str)> {
    use ItemKind::*;
    vec![
        (LeftOut, "Inactive takes"),
        (LeftOut, "Other sequences (not exported)"),
        (LeftOut, "Keyboard slots"),
        (LeftOut, "Input effects"),
        (LeftOut, "Track inputs and monitoring"),
        (LeftOut, "MPE input settings"),
        (LeftOut, "Drum rows (the notes are kept)"),
        (LeftOut, "Automation modes"),
        (LeftOut, "MIDI learn"),
        (LeftOut, "Sidechains"),
        (LeftOut, "Instrument extra outputs"),
        (LeftOut, "Key signatures"),
        (LeftOut, "Settings of plug-in 'Delay'"),
        (LeftOut, "Plug-in parameters past 32 bits"),
        (
            Changed,
            "Instrument 'Synth' shared by several tracks was copied to each",
        ),
        (
            Changed,
            "Audio paths shared by several tracks became submix channels",
        ),
        (Changed, "Tracks with notes and audio were split in two"),
        (
            Note,
            "Track 'Ext' plays an external MIDI destination; the file has no instrument for it",
        ),
        (Note, "Track 'GM' plays the built-in synth here"),
        (Note, "Plug-in 'Synth' has no saved settings"),
        (
            Note,
            "The song starts before bar 1; other DAWs show it from the pickup's start.",
        ),
        (Note, "Track 'Muted' is muted in the project"),
        (Note, "Track 'Keys' is silenced by a solo in the project"),
    ]
}

#[test]
fn vizdaw_only_state_is_listed() {
    // "Colours and comments on markers and tracks" has no state in the
    // document model (markers and tracks have neither; a track's colour is
    // its strip's and is written), so no fixture can give it.
    let (doc, seq) = listed_fixture();
    let x = run_with(&doc, seq, &NoPlist);
    for (kind, what) in listed_lines() {
        assert_eq!(
            count(&x.report, kind, what),
            1,
            "{kind:?} '{what}' in {:#?}",
            x.report.items
        );
    }
    assert_eq!(
        item(
            &x.report,
            ItemKind::LeftOut,
            "Other sequences (not exported)"
        )
        .count,
        1
    );
    // The same lines from the dry run.
    let dry = check(&doc, &opts(seq), &AtomicBool::new(false)).unwrap();
    assert_eq!(dry.items, x.report.items);

    let f = structure_fixture();
    let x = run(&f.doc, f.seq);
    for (kind, what) in listed_lines() {
        assert_eq!(count(&x.report, kind, what), 0, "{kind:?} '{what}'");
    }
}

#[test]
fn tags_and_record() {
    let f = structure_fixture();
    let x = run(&f.doc, f.seq);
    let p = read_back(&x);
    let mut seen_tracks = 0;
    let mut seen_channels = 0;
    for t in tracks(&p) {
        let id = t.named.id.clone().unwrap();
        if let Some(lane) = id.strip_prefix('t') {
            let lane = lane.trim_end_matches('a');
            assert_eq!(t.named.comment, Some(format!("vz:t{lane}")), "{id}");
            seen_tracks += 1;
        }
        let c = channel(t);
        let cid = c.named.id.clone().unwrap();
        if let Some(rest) = cid.strip_prefix('c') {
            let strip: String = rest.chars().take_while(char::is_ascii_digit).collect();
            assert_eq!(c.named.comment, Some(format!("vz:s{strip}")), "{cid}");
            seen_channels += 1;
        }
    }
    assert_eq!((seen_tracks, seen_channels), (2, 4));

    let m = metadata_back(&x);
    assert_eq!(m.fields[12].as_deref(), Some("c\nVizDAW export E1"));
    assert_eq!(m.fields[0].as_deref(), Some("T"));
    let mut b = Builder::from_doc(f.doc.clone());
    info(
        &mut b,
        SongInfo {
            title: Some("T".into()),
            ..SongInfo::default()
        },
    );
    let plain = run(&b.finish(), f.seq);
    assert_eq!(
        metadata_back(&plain).fields[12].as_deref(),
        Some("VizDAW export E1")
    );
    assert_eq!(EXPORT_LINE, "VizDAW export ");

    let rec: Record = serde_json::from_str(&x.record_json).unwrap();
    assert_eq!(rec.format, 1);
    assert_eq!(rec.project_id, "00000000-0000-4000-8000-000000000001");
    assert_eq!(rec.export_id, "E1");
    assert_eq!(rec.exported_at, 1_700_000_000);
    assert_eq!(rec.app_version, "1.2.3");
    assert_eq!(rec.seq, f.seq.0);
    assert_eq!(rec.origin, 0);
    let obj = |kind: &str, id: u64| ObjectRef {
        kind: kind.into(),
        id,
    };
    assert_eq!(rec.ids.get("t0"), Some(&obj("track", 0)));
    assert_eq!(rec.ids.get("t1"), Some(&obj("track", 1)));
    assert_eq!(rec.ids.get("c1"), Some(&obj("strip", 1)));
    assert_eq!(rec.ids.get("c0"), Some(&obj("strip", 0)));
    assert_eq!(rec.ids.get("c1d0"), Some(&obj("device", 0)));
    assert_eq!(rec.ids.get("c1d1"), Some(&obj("device", 1)));
    let want: serde_json::Value = serde_json::from_str(&file::to_json(&f.doc).unwrap()).unwrap();
    assert_eq!(rec.document, want);

    let snap: Snapshot = serde_json::from_str(&x.snapshot_json).unwrap();
    assert_eq!(snap.format, 1);
    assert_eq!(snap.export_id, "E1");
    assert_eq!(snap.ids, rec.ids);
    assert_eq!(snap.origin, 0);
    assert_eq!(snap.projection, projection(&f.doc, f.seq).unwrap());
}

#[test]
fn names_are_escaped() {
    let mut b = Builder::new();
    let name = "A<&>\"'B";
    let (seq, lane) = b.sequence("Song", name, None).unwrap();
    let bus = b.bus("Bus").unwrap();
    // A strip name may hold a tab; reading drops it (text::sanitize).
    b.mixer(MixerCmd::RenameStrip {
        strip: bus,
        name: "A<&>\"B\tC".into(),
    })
    .unwrap();
    info(
        &mut b,
        SongInfo {
            title: Some("Line 1\nLine 2 <&>".into()),
            ..SongInfo::default()
        },
    );
    let doc = b.finish();
    let x = run(&doc, seq);
    validates(&x);
    let p = read_back(&x);
    assert_eq!(
        track(&p, &format!("t{}", lane.0)).named.name.as_deref(),
        Some(name)
    );
    assert_eq!(
        track(&p, &format!("b{}", bus.0)).named.name.as_deref(),
        Some("A<&>\"BC")
    );
    assert!(
        x.project_xml
            .contains("name=\"A&lt;&amp;&gt;&quot;B&#9;C\"")
    );
    assert_eq!(
        metadata_back(&x).fields[0].as_deref(),
        Some("Line 1\nLine 2 <&>")
    );
    assert_eq!(
        file_name(&doc, seq).unwrap(),
        "Line 1-Line 2 <&>.dawproject"
    );
}

#[test]
fn plugin_states() {
    // No property-list codec: the AU state is left out.
    let f = structure_fixture();
    let x = run_with(&f.doc, f.seq, &NoPlist);
    let p = read_back(&x);
    let c = channel(track(&p, "t0"));
    let au = c
        .devices
        .iter()
        .find(|d| d.kind == DpDeviceKind::Au)
        .expect("the AU insert");
    assert_eq!(au.state, None);
    assert_eq!(au.named.comment.as_deref(), Some("vz:d1"));
    assert_eq!(
        count(&x.report, ItemKind::LeftOut, "Settings of plug-in 'Delay'"),
        1
    );
    assert_eq!(
        x.states,
        vec![("plugins/001.clap-preset".to_string(), b"abc".to_vec())]
    );

    // A slot with no state; parameters automation drives.
    let mut b = Builder::new();
    let (dest, slot) = b.plugin_dest("Synth", synth_ref()).unwrap();
    let (seq, lane) = b.sequence("Song", "Keys", Some(dest)).unwrap();
    for (tick, p) in [(0, 5), (Q, u64::from(u32::MAX)), (2 * Q, 1 << 33)] {
        b.event(
            seq,
            lane,
            tick,
            EventBody::Automation {
                target: AutomationTarget::PluginParam(slot, ParamId(p)),
                value: 0.25,
            },
        )
        .unwrap();
    }
    b.mixer(MixerCmd::SetPluginParams {
        slot,
        params: vec![(ParamId(5), 0.75)],
    })
    .unwrap();
    let strip = b.strip_of(StripKind::Instrument(dest)).unwrap();
    let doc = b.finish();
    let x = run(&doc, seq);
    validates(&x);
    assert_eq!(
        count(
            &x.report,
            ItemKind::Note,
            "Plug-in 'Synth' has no saved settings"
        ),
        1
    );
    let it = item(
        &x.report,
        ItemKind::LeftOut,
        "Plug-in parameters past 32 bits",
    );
    assert_eq!(it.details, vec!["Synth".to_string()]);
    assert!(x.states.is_empty());
    let p = read_back(&x);
    let d = &channel(track(&p, &format!("t{}", lane.0))).devices[0];
    assert_eq!(d.state, None);
    let ch = format!("c{}d{}", strip.0, slot.0);
    let params: Vec<(Option<String>, Option<i32>, Option<f64>)> = d
        .parameters
        .iter()
        .map(|r| (r.named.id.clone(), r.parameter_id, r.value))
        .collect();
    assert_eq!(
        params,
        vec![
            (Some(format!("{ch}q5")), Some(5), Some(0.75)),
            (Some(format!("{ch}q4294967295")), Some(-1), None),
        ]
    );
    for r in &d.parameters {
        assert_eq!(r.unit, DpUnit::Normalized);
        assert_eq!((r.min, r.max), (Some(0.0), Some(1.0)));
    }
    assert_eq!(d.enabled.as_ref().and_then(|e| e.value), Some(true));
    assert_eq!(state_entry(1, PluginFormat::Au), "plugins/001.aupreset");
    assert_eq!(
        state_entry(12, PluginFormat::Clap),
        "plugins/012.clap-preset"
    );
    assert_eq!(
        NoPlist.to_xml(b"x"),
        Err("converting Audio Unit settings needs macOS".to_string())
    );
    assert_eq!(
        NoPlist.to_binary(b"x"),
        Err("converting Audio Unit settings needs macOS".to_string())
    );
}

fn pickup_fixture() -> (Document, SeqId) {
    let mut b = Builder::new();
    let (seq, _) = b.sequence("Song", "Keys", None).unwrap();
    b.run(Command::Sequence(SequenceCmd::SetStartOffset {
        seq,
        offset: Tick(Q),
    }))
    .unwrap();
    b.run(Command::Sequence(SequenceCmd::SetMarkers {
        seq,
        markers: vec![Marker {
            tick: Tick(0),
            name: "Verse".into(),
        }]
        .into_iter()
        .collect(),
    }))
    .unwrap();
    (b.finish(), seq)
}

#[test]
fn pickup_moves_file_time_zero() {
    let (doc, seq) = pickup_fixture();
    let x = run(&doc, seq);
    let p = read_back(&x);
    let arr = p.arrangement.as_ref().unwrap();
    assert_eq!(arr.markers[0].time, 1.0);
    assert_eq!(arr.markers[0].named.name.as_deref(), Some("Verse"));
    let rec: Record = serde_json::from_str(&x.record_json).unwrap();
    assert_eq!(rec.origin, -15_360);
    assert_eq!(
        count(
            &x.report,
            ItemKind::Note,
            "The song starts before bar 1; other DAWs show it from the pickup's start."
        ),
        1
    );
    // The pickup bar is a 1/4 bar, then 4/4 from bar 1.
    let ts = p.time_signature.as_ref().unwrap();
    assert_eq!((ts.numerator, ts.denominator), (1, 4));
    let pts: Vec<_> = arr
        .time_signature_automation
        .as_ref()
        .expect("a pickup writes the meter map")
        .points
        .iter()
        .map(|p| (p.time, p.value.clone()))
        .collect();
    assert_eq!(
        pts,
        vec![
            (0.0, DpPointValue::TimeSig(1, 4)),
            (1.0, DpPointValue::TimeSig(4, 4))
        ]
    );
    assert!(arr.tempo_automation.is_none(), "one tempo point");
}

fn big_song(notes: i64) -> (Document, SeqId) {
    let mut b = Builder::new();
    let (seq, lane) = b.sequence("Song", "Keys", None).unwrap();
    let list: Vec<(i64, u8, u16, i64)> = (0..notes)
        .map(|i| (i * Q / 8, 60 + (i % 12) as u8, 0x8000, Q / 16))
        .collect();
    b.notes(seq, lane, &list).unwrap();
    (b.finish(), seq)
}

#[test]
fn export_stops_when_cancelled() {
    let f = structure_fixture();
    let set = AtomicBool::new(true);
    assert_eq!(
        export(&f.doc, &opts(f.seq), &env(&FakePlist), &set).map(|_| ()),
        Err(IoError::Cancelled)
    );
    assert_eq!(check(&f.doc, &opts(f.seq), &set), Err(IoError::Cancelled));

    let flag = AtomicBool::new(false);
    let started = AtomicBool::new(false);
    let (got, after) = std::thread::scope(|s| {
        // The document is built and exported on the job's thread (it is
        // not shared between threads).
        let job = s.spawn(|| {
            let (doc, seq) = big_song(300_000);
            started.store(true, Ordering::Relaxed);
            export(&doc, &opts(seq), &env(&FakePlist), &flag).map(|_| ())
        });
        while !started.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(1));
        }
        std::thread::sleep(Duration::from_millis(50));
        let at = std::time::Instant::now();
        flag.store(true, Ordering::Relaxed);
        let got = job.join().unwrap();
        (got, at.elapsed())
    });
    assert_eq!(got, Err(IoError::Cancelled));
    assert!(after < Duration::from_secs(10), "took {after:?} to stop");
}

/// The song of `projection_is_id_free`, built in one of two orders.
fn song_built(order: bool) -> (Document, SeqId) {
    let mut b = Builder::new();
    let (dest, out) = if order {
        let (d, _) = b.plugin_dest("Synth", synth_ref()).unwrap();
        (d, output(&mut b, "Drums"))
    } else {
        // An unused output and destination first shift every ID.
        output(&mut b, "Spare");
        let o = output(&mut b, "Drums");
        let (d, _) = b.plugin_dest("Synth", synth_ref()).unwrap();
        (d, o)
    };
    let verb = b.bus("Verb").unwrap();
    let (seq, keys) = b.sequence("Song", "Keys", Some(dest)).unwrap();
    let drums = b.lane(seq, "Drums", None).unwrap();
    route(&mut b, seq, drums, out);
    let drum_strip = b.strip_of(StripKind::AudioPath(out)).unwrap();
    b.mixer(MixerCmd::AddSend {
        strip: drum_strip,
        send: AuxSend::new(verb),
    })
    .unwrap();
    b.notes(seq, keys, &[(0, 60, 0x8000, Q), (Q, 62, 0x9000, Q)])
        .unwrap();
    let media = b.media("Media/a.caf", 48_000, 2).unwrap();
    b.region(
        seq,
        drums,
        BAR,
        media,
        10,
        40_000,
        -3.0,
        Fade::default(),
        Fade::default(),
    )
    .unwrap();
    b.event(
        seq,
        drums,
        0,
        EventBody::Automation {
            target: AutomationTarget::StripFader(drum_strip),
            value: 0.5,
        },
    )
    .unwrap();
    if !order {
        // The unused output's strip is not part of the song.
        let spare = b
            .doc()
            .outputs()
            .values()
            .find(|o| o.name == "Spare")
            .map(|o| o.id)
            .unwrap();
        b.run(Command::Routing(RoutingCmd::RemoveOutput {
            output: spare,
            replacement: None,
        }))
        .unwrap();
    }
    (b.finish(), seq)
}

#[test]
fn projection_is_id_free() {
    let (a, sa) = song_built(true);
    let (b, sb) = song_built(false);
    assert_ne!(
        a.mixer().strips().keys().collect::<Vec<_>>(),
        b.mixer().strips().keys().collect::<Vec<_>>(),
        "the builds differ in IDs"
    );
    let pa = projection(&a, sa).unwrap();
    assert_eq!(pa, projection(&b, sb).unwrap());
    assert_eq!(pa.tracks.len(), 2);
    assert_eq!(pa.tracks[0].notes_played.len(), 2);
    assert_eq!(pa.tracks[1].regions.len(), 1);
    assert_eq!(pa.tracks[1].automation.len(), 1);
    // Keys, Drums, Verb, Master.
    assert_eq!(pa.strips.len(), 4);
    assert_eq!(pa.strips[1].sends.len(), 1);
    assert_eq!(pa.strips[3].role, "master");
}

#[test]
fn units_convert_and_tag() {
    assert_eq!(beats(Tick(15_360)), 1.0);
    assert_eq!(beats(Tick(-7_680)), -0.5);
    assert_eq!(ticks(1.0), Some((Tick(15_360), false)));
    assert_eq!(ticks(0.5 / 15_360.0), Some((Tick(1), true)), "half away");
    assert_eq!(ticks(-0.5 / 15_360.0), Some((Tick(-1), true)));
    assert_eq!(ticks(1e300), None);
    assert_eq!(ticks(f64::NAN), None);
    assert_eq!(ticks(f64::INFINITY), None);
    for t in [0, 1, 15_359, -1, 1 << 40] {
        assert_eq!(ticks(beats(Tick(t))), Some((Tick(t), false)), "{t}");
    }
    assert_eq!(linear_gain(0.0), 1.0);
    assert_eq!(linear_gain(-144.0), 0.0);
    assert_eq!(db_of_linear(0.0), -144.0);
    assert_eq!(db_of_linear(-1.0), -144.0);
    assert!((db_of_linear(linear_gain(-6.0)) + 6.0).abs() < 1e-12);
    assert_eq!(pan_normalized(-1.0), 0.0);
    assert_eq!(pan_normalized(0.0), 0.5);
    assert_eq!(pan_of_normalized(1.0), 1.0);
    assert_eq!(swatch_hex(Swatch::Red), "#e5484d");
    assert_eq!(swatch_hex(Swatch::Pink), "#d6409f");
    assert_eq!(nearest_swatch("#0090ff"), Some((Swatch::Blue, false)));
    assert_eq!(nearest_swatch("#0090FF"), Some((Swatch::Blue, false)));
    assert_eq!(nearest_swatch("#0091ff"), Some((Swatch::Blue, true)));
    assert_eq!(nearest_swatch("#ff0000"), Some((Swatch::Red, true)));
    assert_eq!(nearest_swatch("#zzz"), None);
    assert_eq!(nearest_swatch("0090ff"), None);
    assert_eq!(tag(TagKind::Track, 3), "vz:t3");
    assert_eq!(tag(TagKind::Eq, 9), "vz:q9");
    assert_eq!(
        strip_tags("vz:t3 hello vz:s12"),
        (
            "hello".to_string(),
            vec![(TagKind::Track, 3), (TagKind::Strip, 12)]
        )
    );
    assert_eq!(strip_tags("vz:x1 vz:t no"), ("vz:t no".to_string(), vec![]));
    assert_eq!(
        strip_tags("line one\nvz:m2\nline two"),
        ("line one\nline two".to_string(), vec![(TagKind::Marker, 2)])
    );
}

#[test]
fn file_names_come_from_the_title_or_the_sequence() {
    let f = structure_fixture();
    assert_eq!(file_name(&f.doc, f.seq).unwrap(), "T.dawproject");
    let (doc, seq) = pickup_fixture();
    assert_eq!(file_name(&doc, seq).unwrap(), "Song.dawproject");
    let long = "x".repeat(300);
    let mut b = Builder::from_doc(doc);
    info(
        &mut b,
        SongInfo {
            title: Some(long),
            ..SongInfo::default()
        },
    );
    let name = file_name(&b.finish(), seq).unwrap();
    assert_eq!(name.len(), 255);
    assert!(name.ends_with("x.dawproject"));
    assert!(matches!(
        file_name(&Document::new(), SeqId(9)),
        Err(IoError::Refused(_))
    ));
}
