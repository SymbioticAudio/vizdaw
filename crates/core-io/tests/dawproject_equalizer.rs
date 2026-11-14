//! A DAWproject `Equalizer` onto Multi-Q's bands (plan 6g Task 5, Ruling
//! 10): the parameter table, the band assignment, clamping, what is left
//! out, and the import putting the result in the EQ slot or an insert.
//! Pure: nothing is loaded or rendered.

mod common;

use common::merge::{Strip as Stripped, fixture, stripped_package, theirs_of, trip};
use common::package::{TestPlist, package_bytes};
use core_engine::fixtures::Builder;
use core_io::dawproject::import::{
    BUILTIN_EFFECTS, EQ_BANDS, EQ_CHANGED, EQ_GAINS, ImportEnv, MediaSource, apply, map,
};
use core_io::dawproject::merge::apply::{LABEL, apply_merge};
use core_io::dawproject::merge::decide::plan;
use core_io::dawproject::multiq::{
    BANDS, Domain, FREQ, GAIN, MqBand, PARAMS, Q, Range, SLOPES_DB, juce_hash, map_equalizer,
    param_id,
};
use core_io::dawproject::package::{DpPackage, open};
use core_io::dawproject::{
    DpDeviceKind, DpEqBand, DpEqBandType, DpNamed, DpRealParam, DpStructureItem, DpTrack, DpUnit,
};
use core_io::limits::PackageLimits;
use core_io::report::{ItemKind, Report};
use core_io::zip::{EntryMethod, ZipWriter};
use core_model::command::{Command, LaneCmd, MixerCmd, SequenceCmd};
use core_model::document::Document;
use core_model::history::History;
use core_model::ids::{AnyId, PluginInstanceId, SeqId, StripId};
use core_model::mixer::{ParamId, PluginFormat, PluginRef, PluginSlot, Strip, StripKind};
use core_model::selection::ViewFilter;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

// ---------------------------------------------------------------- helpers

fn mq() -> PluginRef {
    PluginRef {
        format: PluginFormat::Clap,
        id: "com.dusk-audio.multi-q".into(),
        name: "Multi-Q".into(),
        vendor: "Dusk Audio".into(),
        version: "0.10.9".into(),
    }
}

fn real(unit: DpUnit, v: f64) -> Option<DpRealParam> {
    Some(DpRealParam {
        named: DpNamed::default(),
        parameter_id: None,
        unit,
        value: Some(v),
        min: None,
        max: None,
    })
}

fn hz(v: f64) -> Option<DpRealParam> {
    real(DpUnit::Hertz, v)
}

fn db(v: f64) -> Option<DpRealParam> {
    real(DpUnit::Decibel, v)
}

fn lin(v: f64) -> Option<DpRealParam> {
    real(DpUnit::Linear, v)
}

fn band(kind: DpEqBandType, freq: Option<DpRealParam>) -> DpEqBand {
    DpEqBand {
        kind: Some(kind),
        order: None,
        freq,
        gain: None,
        q: None,
        enabled: None,
    }
}

fn bell(f: f64, g: f64) -> DpEqBand {
    DpEqBand {
        gain: db(g),
        ..band(DpEqBandType::Bell, hz(f))
    }
}

fn ordered(kind: DpEqBandType, f: f64, order: i32) -> DpEqBand {
    DpEqBand {
        order: Some(order),
        ..band(kind, hz(f))
    }
}

fn mq_band(bands: &[MqBand], n: u8) -> &MqBand {
    bands.iter().find(|b| b.band == n).expect("every band")
}

fn value(params: &[(ParamId, f64)], id: &str) -> Option<f64> {
    let want = ParamId(u64::from(juce_hash(id)));
    params
        .iter()
        .rev()
        .find(|(p, _)| *p == want)
        .map(|(_, v)| *v)
}

#[track_caller]
fn close(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-9, "{a} != {b}");
}

// ---------------------------------------------------------------- the table

#[test]
fn the_table_matches_the_dump() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../core-host/tests/data/multiq-params.json"
    );
    let text = std::fs::read_to_string(path).expect("Task 1's dump");
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    let domain = match json["domain"].as_str().unwrap() {
        "plain" => Domain::Plain,
        "normalized" => Domain::Normalized,
        d => panic!("unknown domain {d}"),
    };
    assert_eq!(core_io::dawproject::multiq::DOMAIN, domain);
    let range = |name: &str, r: &Range| {
        let j = &json["ranges"][name];
        assert_eq!(j["start"].as_f64().unwrap(), r.start, "{name}");
        assert_eq!(j["end"].as_f64().unwrap(), r.end, "{name}");
        assert_eq!(j["log"].as_bool().unwrap_or(false), r.log, "{name}");
        assert_eq!(j["interval"].as_f64().unwrap_or(0.0), r.interval, "{name}");
        assert_eq!(j["skew"].as_f64().unwrap_or(1.0), r.skew, "{name}");
    };
    range("freq", &FREQ);
    range("gain", &GAIN);
    range("q", &Q);
    let rows = json["params"].as_array().unwrap();
    assert_eq!(rows.len(), PARAMS.len());
    for (row, p) in rows.iter().zip(PARAMS.iter()) {
        assert_eq!(row["string_id"].as_str().unwrap(), p.string_id);
        assert_eq!(row["clap_id"].as_u64().unwrap(), u64::from(p.clap_id));
        assert_eq!(row["name"].as_str().unwrap(), p.name);
        assert_eq!(row["min"].as_f64().unwrap(), p.min, "{}", p.string_id);
        assert_eq!(row["max"].as_f64().unwrap(), p.max, "{}", p.string_id);
        assert_eq!(
            row["default"].as_f64().unwrap(),
            p.default,
            "{}",
            p.string_id
        );
        assert_eq!(
            row["stepped"].as_bool().unwrap(),
            p.stepped,
            "{}",
            p.string_id
        );
        assert_eq!(juce_hash(p.string_id), p.clap_id, "{}", p.string_id);
        assert_eq!(param_id(p.string_id), Some(ParamId(u64::from(p.clap_id))));
    }
    // The choice lists are not in the dump: they are typed from MultiQ.cpp,
    // and the render check pins Peaking and one slope.
    assert!(json["params"][0].get("choices").is_none());
    assert_eq!(SLOPES_DB, [6, 12, 18, 24, 36, 48, 72, 96]);
    assert_eq!(BANDS, 8);
    assert_eq!(juce_hash("band1_freq"), 1_472_467_003);
    assert_eq!(juce_hash("band5_gain"), 1_586_997_182);
    assert_eq!(juce_hash("band3_shape"), 188_548_288);
    assert_eq!(param_id("band1_gain"), None);
}

// ---------------------------------------------------------------- mapping

#[test]
fn bands_map_by_type() {
    let bands = [
        ordered(DpEqBandType::HighPass, 80.0, 2),
        DpEqBand {
            gain: db(3.0),
            ..band(DpEqBandType::LowShelf, hz(120.0))
        },
        DpEqBand {
            q: lin(1.4),
            ..bell(400.0, -2.0)
        },
        bell(3000.0, 1.5),
        DpEqBand {
            gain: db(-1.0),
            ..band(DpEqBandType::HighShelf, hz(8000.0))
        },
        band(DpEqBandType::LowPass, hz(18_000.0)),
    ];
    let m = map_equalizer(&bands, "T");
    assert!(m.changed.is_empty(), "{:?}", m.changed);
    assert!(m.left_out.is_empty(), "{:?}", m.left_out);
    assert_eq!(m.bands.len(), 8);

    let b1 = mq_band(&m.bands, 1);
    assert!(b1.enabled);
    close(b1.freq, 80.0);
    assert_eq!((b1.shape, b1.slope, b1.gain_db), (None, Some(1), None));
    let b2 = mq_band(&m.bands, 2);
    assert!(b2.enabled);
    close(b2.freq, 120.0);
    assert_eq!((b2.shape, b2.gain_db), (Some(0), Some(3.0)));
    let b3 = mq_band(&m.bands, 3);
    close(b3.freq, 400.0);
    assert_eq!(
        (b3.shape, b3.gain_db, b3.q),
        (Some(0), Some(-2.0), Some(1.4))
    );
    let b4 = mq_band(&m.bands, 4);
    close(b4.freq, 3000.0);
    assert_eq!((b4.shape, b4.gain_db, b4.q), (Some(0), Some(1.5), None));
    let b7 = mq_band(&m.bands, 7);
    close(b7.freq, 8000.0);
    assert_eq!((b7.shape, b7.gain_db), (Some(0), Some(-1.0)));
    let b8 = mq_band(&m.bands, 8);
    assert!(b8.enabled);
    close(b8.freq, 18_000.0);
    assert_eq!((b8.shape, b8.gain_db), (None, None));
    for n in [5, 6] {
        assert!(!mq_band(&m.bands, n).enabled, "band {n}");
    }

    // The overlay, in the CLAP value domain of Task 1's table.
    let p = &m.params;
    close(value(p, "band1_enabled").unwrap(), 1.0);
    close(
        value(p, "band1_freq").unwrap(),
        (80.0f64 / 20.0).ln() / 1000f64.ln(),
    );
    close(value(p, "band1_slope").unwrap(), 1.0 / 7.0);
    close(value(p, "band2_gain").unwrap(), 27.0 / 48.0);
    close(value(p, "band2_shape").unwrap(), 0.0);
    close(
        value(p, "band3_q").unwrap(),
        (1.4f64 / 0.1).ln() / 1000f64.ln(),
    );
    close(value(p, "band3_gain").unwrap(), 22.0 / 48.0);
    assert_eq!(value(p, "band4_q"), None, "a missing Q keeps the default");
    close(value(p, "band5_enabled").unwrap(), 0.0);
    close(value(p, "band6_enabled").unwrap(), 0.0);
    close(value(p, "band7_shape").unwrap(), 0.0);
    close(value(p, "band8_enabled").unwrap(), 1.0);
    assert_eq!(value(p, "band8_slope"), None, "no order keeps the slope");
    assert!(
        p.iter()
            .all(|(_, v)| v.is_finite() && (0.0..=1.0).contains(v))
    );
}

#[test]
fn crowded_bands_spill_then_leave() {
    let passes = [
        band(DpEqBandType::HighPass, hz(30.0)),
        band(DpEqBandType::HighPass, hz(60.0)),
        band(DpEqBandType::HighPass, hz(90.0)),
    ];
    let m = map_equalizer(&passes, "T");
    close(mq_band(&m.bands, 1).freq, 30.0);
    let b2 = mq_band(&m.bands, 2);
    close(b2.freq, 60.0);
    assert_eq!(b2.shape, Some(2), "High Pass");
    close(value(&m.params, "band2_shape").unwrap(), 1.0);
    assert_eq!(m.left_out.len(), 1, "{:?}", m.left_out);
    assert!(
        m.left_out[0].starts_with("Equalizer band on track 'T': highPass 90 Hz"),
        "{:?}",
        m.left_out
    );

    // Seven bells, the highest first in the file.
    let bells: Vec<DpEqBand> = [9000.0, 100.0, 200.0, 400.0, 800.0, 1600.0, 3200.0]
        .into_iter()
        .map(|f| bell(f, 1.0))
        .collect();
    let m = map_equalizer(&bells, "T");
    for (n, f) in [(3, 100.0), (4, 200.0), (5, 400.0), (6, 800.0)] {
        let b = mq_band(&m.bands, n);
        close(b.freq, f);
        assert_eq!(b.shape, Some(0), "Peaking on band {n}");
    }
    for (n, f) in [(2, 1600.0), (7, 3200.0)] {
        let b = mq_band(&m.bands, n);
        close(b.freq, f);
        assert_eq!(b.shape, Some(1), "Peaking on band {n}");
    }
    close(value(&m.params, "band2_shape").unwrap(), 0.5);
    close(value(&m.params, "band7_shape").unwrap(), 0.5);
    assert!(!mq_band(&m.bands, 1).enabled);
    assert!(!mq_band(&m.bands, 8).enabled);
    assert_eq!(m.left_out.len(), 1, "{:?}", m.left_out);
    assert!(
        m.left_out[0].starts_with("Equalizer band on track 'T': bell 9000 Hz"),
        "{:?}",
        m.left_out
    );
    assert!(m.changed.is_empty(), "{:?}", m.changed);
}

#[test]
fn values_are_clamped_and_listed() {
    let bands = [
        ordered(DpEqBandType::HighPass, 5.0, 3),
        ordered(DpEqBandType::HighPass, 100.0, 3),
        band(DpEqBandType::LowPass, hz(40_000.0)),
        DpEqBand {
            q: lin(0.0),
            ..bell(1000.0, 30.0)
        },
    ];
    let m = map_equalizer(&bands, "T");
    let b1 = mq_band(&m.bands, 1);
    close(b1.freq, 20.0);
    assert_eq!(b1.slope, Some(2), "order 3 is 18 dB/oct");
    close(mq_band(&m.bands, 8).freq, 20_000.0);
    let b3 = mq_band(&m.bands, 3);
    assert_eq!((b3.gain_db, b3.q), (Some(24.0), Some(0.1)));
    close(value(&m.params, "band3_gain").unwrap(), 1.0);
    close(value(&m.params, "band3_q").unwrap(), 0.0);
    close(value(&m.params, "band8_freq").unwrap(), 1.0);
    close(value(&m.params, "band1_freq").unwrap(), 0.0);

    assert_eq!(m.changed.len(), 5, "{:#?}", m.changed);
    let line = |band: u8, what: &str| {
        let start = format!("Equalizer band on track 'T', Multi-Q band {band}: ");
        assert!(
            m.changed
                .iter()
                .any(|l| l.starts_with(&start) && l.contains(what)),
            "no '{start}…{what}' in {:#?}",
            m.changed
        );
    };
    line(1, "frequency 5 Hz set to 20 Hz");
    line(2, "slope kept at 12 dB/oct");
    line(8, "frequency 40000 Hz set to 20000 Hz");
    line(3, "gain 30 dB set to 24 dB");
    line(3, "Q 0 set to 0.1");
    assert!(m.left_out.is_empty(), "{:?}", m.left_out);
}

#[test]
fn unmappable_bands_are_left_out() {
    let normalized = Some(DpRealParam {
        named: DpNamed::default(),
        parameter_id: None,
        unit: DpUnit::Normalized,
        value: Some(0.5),
        min: None,
        max: None,
    });
    let bands = [
        bell(f64::NAN, 1.0),
        bell(f64::INFINITY, 1.0),
        DpEqBand {
            gain: db(1.0),
            ..band(DpEqBandType::Bell, normalized)
        },
        DpEqBand {
            gain: db(1.0),
            ..band(DpEqBandType::Bell, None)
        },
        DpEqBand {
            kind: None,
            ..bell(100.0, 1.0)
        },
        band(DpEqBandType::Bell, real(DpUnit::Semitones, 60.0)),
        DpEqBand {
            gain: db(f64::NAN),
            ..bell(500.0, 0.0)
        },
    ];
    let m = map_equalizer(&bands, "T");
    assert_eq!(m.left_out.len(), bands.len(), "{:#?}", m.left_out);
    assert!(
        m.left_out
            .iter()
            .all(|l| l.starts_with("Equalizer band on track 'T': ")),
        "{:#?}",
        m.left_out
    );
    assert!(m.bands.iter().all(|b| !b.enabled), "{:?}", m.bands);
    assert!(m.changed.is_empty(), "{:?}", m.changed);
    assert!(m.params.iter().all(|(_, v)| v.is_finite()));

    // A normalised frequency with a range is scaled into it.
    let ranged = Some(DpRealParam {
        named: DpNamed::default(),
        parameter_id: None,
        unit: DpUnit::Normalized,
        value: Some(0.5),
        min: Some(100.0),
        max: Some(300.0),
    });
    let m = map_equalizer(&[band(DpEqBandType::Bell, ranged)], "T");
    assert!(m.left_out.is_empty(), "{:?}", m.left_out);
    close(mq_band(&m.bands, 3).freq, 200.0);
}

// ---------------------------------------------------------------- import

/// A project.xml with `structure` after the master track.
fn project_xml(structure: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Project version="1.0">
  <Application name="Fixture" version="1"/>
  <Structure>
    <Track id="m" name="Master" contentType="audio notes"><Channel id="mc" role="master"/></Track>
    {structure}
  </Structure>
  <Arrangement/>
</Project>"#
    )
}

/// An audio track "name" whose channel holds `devices`.
fn audio_track(id: &str, name: &str, devices: &str) -> String {
    format!(
        r#"<Track id="{id}" name="{name}" contentType="audio"><Channel id="{id}c" role="regular"><Devices>{devices}</Devices></Channel></Track>"#
    )
}

const EQ_ONE: &str = r#"<Equalizer id="e1" deviceName="Channel EQ" deviceRole="audioFX"><Enabled value="false"/><Band type="highPass" order="2"><Freq unit="hertz" value="80"/></Band><Band type="bell"><Freq unit="hertz" value="1000"/><Gain unit="decibel" value="6"/><Q unit="linear" value="1"/></Band><InputGain unit="decibel" value="3"/><OutputGain unit="decibel" value="-2"/></Equalizer>"#;
const EQ_TWO: &str = r#"<Equalizer id="e2" deviceName="Second EQ" deviceRole="audioFX"><Band type="lowShelf"><Freq unit="hertz" value="200"/><Gain unit="linear" value="2"/></Band></Equalizer>"#;

fn package(dir: &Path, project: &str) -> DpPackage {
    let path = dir.join("Eq.dawproject");
    let mut z = ZipWriter::new(std::fs::File::create(&path).unwrap());
    z.add("project.xml", EntryMethod::Deflated, project.as_bytes())
        .unwrap();
    z.finish().unwrap();
    open(&path, &PackageLimits::default(), &AtomicBool::new(false)).expect("opens")
}

fn yes(_: PluginFormat, _: &str) -> bool {
    true
}

fn env<'a>(eq: Option<&'a PluginRef>) -> ImportEnv<'a> {
    ImportEnv {
        installed: &yes,
        plist: &TestPlist,
        rate: 48_000,
        ids: None,
        eq_plugin: eq,
    }
}

/// A new document: the master and one sequence with no tracks.
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

/// `pkg` mapped with `eq` and applied to a new document.
fn imported(pkg: &DpPackage, eq: Option<&PluginRef>) -> Applied {
    let imp = map(
        pkg,
        &BTreeMap::<usize, MediaSource>::new(),
        &env(eq),
        &AtomicBool::new(false),
    )
    .expect("maps");
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

/// The output-path strip of track `name`.
fn track_strip(a: &Applied, name: &str) -> Strip {
    let d = a.h.doc();
    let lane = d
        .sequence(a.seq)
        .unwrap()
        .lanes
        .iter()
        .find(|l| l.name == name)
        .unwrap_or_else(|| panic!("no track {name}"));
    let out = lane.audio_out.expect("an output path");
    d.mixer()
        .strips()
        .values()
        .find(|s| s.kind == StripKind::AudioPath(out))
        .unwrap()
        .clone()
}

/// The `Equalizer` devices of the file's track `name`, in order.
fn file_bands(pkg: &DpPackage, name: &str) -> Vec<Vec<DpEqBand>> {
    fn find<'a>(items: &'a [DpStructureItem], name: &str) -> Option<&'a DpTrack> {
        items.iter().find_map(|i| match i {
            DpStructureItem::Track(t) if t.named.name.as_deref() == Some(name) => Some(t),
            _ => None,
        })
    }
    let t = find(&pkg.project.structure, name).expect("the track");
    t.channel
        .as_ref()
        .unwrap()
        .devices
        .iter()
        .filter(|d| d.kind == DpDeviceKind::Equalizer)
        .map(|d| d.bands.clone())
        .collect()
}

fn overlay(s: &PluginSlot) -> BTreeMap<ParamId, f64> {
    s.params.iter().map(|(k, v)| (*k, *v)).collect()
}

fn as_map(p: &[(ParamId, f64)]) -> BTreeMap<ParamId, f64> {
    p.iter().copied().collect()
}

fn item<'a>(r: &'a Report, kind: ItemKind, what: &str) -> Option<&'a core_io::report::Item> {
    r.items.iter().find(|i| i.kind == kind && i.what == what)
}

#[test]
fn import_puts_it_in_the_eq_slot() {
    let dir = tempfile::tempdir().unwrap();
    let xml = project_xml(&audio_track("t", "T", &format!("{EQ_ONE}{EQ_TWO}")));
    let pkg = package(dir.path(), &xml);
    let bands = file_bands(&pkg, "T");
    assert_eq!(bands.len(), 2);
    assert_eq!(bands[0].len(), 2, "the reader keeps each Band");
    assert_eq!(bands[0][0].kind, Some(DpEqBandType::HighPass));
    assert_eq!(bands[0][0].order, Some(2));
    let one = map_equalizer(&bands[0], "T");
    let two = map_equalizer(&bands[1], "T");

    let a = imported(&pkg, Some(&mq()));
    let s = track_strip(&a, "T");
    let eq = s.eq.as_ref().expect("the EQ slot");
    assert_eq!(eq.plugin, mq());
    assert!(eq.bypass, "the device's Enabled is false");
    assert_eq!(overlay(eq), as_map(&one.params));
    close(
        value(&one.params, "band3_freq").unwrap(),
        (1000.0f64 / 20.0).ln() / 1000f64.ln(),
    );
    assert_eq!(s.inserts.len(), 1);
    let ins = s.inserts.front().unwrap();
    assert_eq!(ins.plugin, mq());
    assert!(!ins.bypass);
    assert_eq!(overlay(ins), as_map(&two.params));
    // Linear gain 2 is +6.02 dB.
    let g = value(&two.params, "band2_gain").unwrap();
    close(g, (20.0 * 2f64.log10() + 24.0) / 48.0);

    let r = &a.report;
    assert!(
        item(r, ItemKind::LeftOut, BUILTIN_EFFECTS).is_none(),
        "{r:#?}"
    );
    assert!(item(r, ItemKind::LeftOut, EQ_BANDS).is_none(), "{r:#?}");
    assert!(item(r, ItemKind::Changed, EQ_CHANGED).is_none(), "{r:#?}");
    let gains = item(r, ItemKind::LeftOut, EQ_GAINS).expect("the gains line");
    assert_eq!(gains.details.len(), 2, "{:?}", gains.details);
    assert!(
        gains.details[0].starts_with("Equalizer on track 'T': InputGain")
            && gains.details[0].contains("value=3"),
        "{:?}",
        gains.details
    );
    assert!(
        gains.details[1].starts_with("Equalizer on track 'T': OutputGain")
            && gains.details[1].contains("value=-2"),
        "{:?}",
        gains.details
    );
    assert!(a.states.is_empty());

    // Without Multi-Q, 6d's behaviour stands: the device is listed.
    let a = imported(&pkg, None);
    let s = track_strip(&a, "T");
    assert!(s.eq.is_none());
    assert!(s.inserts.is_empty());
    let eqs = item(&a.report, ItemKind::LeftOut, BUILTIN_EFFECTS).expect("6d's line");
    assert!(
        eqs.details[0].starts_with("Equalizer on track 'T': Band type=highPass"),
        "{:?}",
        eqs.details
    );
    assert!(item(&a.report, ItemKind::LeftOut, EQ_GAINS).is_none());
}

#[test]
fn eq_tagged_plugin_after_an_equalizer_keeps_the_slot() {
    let dir = tempfile::tempdir().unwrap();
    let plugin = r#"<ClapPlugin id="p" deviceID="org.example.eq" deviceName="Their EQ" deviceRole="audioFX" comment="vz:q5"/>"#;
    let xml = project_xml(&audio_track("t", "T", &format!("{EQ_TWO}{plugin}")));
    let pkg = package(dir.path(), &xml);
    let two = map_equalizer(&file_bands(&pkg, "T")[0], "T");
    let a = imported(&pkg, Some(&mq()));
    let s = track_strip(&a, "T");
    assert_eq!(s.eq.as_ref().unwrap().plugin.id, "org.example.eq");
    assert_eq!(s.inserts.len(), 1);
    let ins = s.inserts.front().unwrap();
    assert_eq!(ins.plugin, mq());
    assert_eq!(overlay(ins), as_map(&two.params));
}

#[test]
fn vizdaw_round_trip_is_unchanged() {
    const STATE: &[u8] = b"<multiq state/>";
    let mut b = Builder::new();
    let (seq, _, _, _) = common::generators::song(&mut b, "Lead");
    let (_, strip) = common::generators::audio_track(&mut b, seq, "Gtr");
    let r = b
        .mixer(MixerCmd::SetEq {
            strip,
            plugin: Some(mq()),
        })
        .unwrap();
    let slot = r
        .created
        .iter()
        .find_map(|id| match id {
            AnyId::Plugin(p) => Some(*p),
            _ => None,
        })
        .unwrap();
    let b = common::generators::capture(b, slot, STATE);
    let doc = b.finish();
    let t = trip(&doc, seq);
    let path = t.dir.path().join("Trip.dawproject");
    std::fs::write(&path, package_bytes(&t.export).unwrap()).unwrap();
    let pkg = open(&path, &PackageLimits::default(), &AtomicBool::new(false)).unwrap();

    let a = imported(&pkg, Some(&mq()));
    let s = track_strip(&a, "Gtr");
    let eq = s.eq.as_ref().expect("the EQ slot");
    assert_eq!(eq.plugin, mq());
    assert!(eq.params.is_empty());
    let state = a.states.iter().find(|(id, _)| *id == eq.id).unwrap();
    assert_eq!(&*state.1, STATE);
    for what in [EQ_BANDS, EQ_GAINS, BUILTIN_EFFECTS] {
        assert!(
            item(&a.report, ItemKind::LeftOut, what).is_none(),
            "{:#?}",
            a.report
        );
    }
    assert!(item(&a.report, ItemKind::Changed, EQ_CHANGED).is_none());
}

#[test]
fn merge_fills_only_new_strips() {
    let (doc, seq) = fixture();
    let t = trip(&doc, seq);
    // Theirs: the exported project with one audio track added.
    let mut b = Builder::from_doc(t.base_doc.clone());
    common::generators::audio_track(&mut b, seq, "Gtr");
    let theirs_doc = b.finish();
    let x = trip(&theirs_doc, seq).export;
    let path = t.dir.path().join("Theirs.dawproject");
    std::fs::write(&path, stripped_package(&x, Stripped::Nothing)).unwrap();
    let p = plan(
        t.snapshot.clone(),
        theirs_of(&t, &path),
        &t.base_doc,
        false,
        &AtomicBool::new(false),
    )
    .expect("plans");

    let before: Vec<StripId> = t.base_doc.mixer().strips().keys().copied().collect();
    assert!(
        t.base_doc.mixer().strips().values().all(|s| s.eq.is_none()),
        "the existing strips have no EQ"
    );
    let mut h = History::new(t.base_doc.clone());
    h.set_new_strip_eq(Some(mq()));
    let mut report = Report::default();
    h.begin(LABEL).unwrap();
    apply_merge(&mut h, &p, "2026-11-13", &mut report).expect("applies");
    h.commit().unwrap();

    let d = h.doc();
    let lane = d
        .sequence(seq)
        .unwrap()
        .lanes
        .iter()
        .find(|l| l.name == "Gtr")
        .expect("the new track");
    let out = lane.audio_out.expect("its output path");
    let new = d.mixer().find_kind(StripKind::AudioPath(out)).unwrap();
    assert!(!before.contains(&new));
    assert_eq!(
        d.mixer().strip(new).unwrap().eq.as_ref().map(|e| &e.plugin),
        Some(&mq())
    );
    for id in before {
        assert!(
            d.mixer().strip(id).unwrap().eq.is_none(),
            "existing strip {id:?} stays empty"
        );
    }
}

#[test]
fn reader_keeps_bands_structurally() {
    let dir = tempfile::tempdir().unwrap();
    let xml = project_xml(&audio_track(
        "t",
        "T",
        r#"<Equalizer deviceName="E" deviceRole="audioFX"><Band type="notch" order="4"><Freq unit="hertz" value="50"/><Gain unit="decibel" value="-3"/><Q unit="linear" value="8"/><Enabled value="false"/></Band><Band type="sideways"><Freq unit="hertz" value="1"/></Band></Equalizer>"#,
    ));
    let pkg = package(dir.path(), &xml);
    let b = &file_bands(&pkg, "T")[0];
    assert_eq!(b.len(), 2);
    assert_eq!(b[0].kind, Some(DpEqBandType::Notch));
    assert_eq!(b[0].order, Some(4));
    assert_eq!(b[0].freq.as_ref().unwrap().value, Some(50.0));
    assert_eq!(b[0].gain.as_ref().unwrap().unit, DpUnit::Decibel);
    assert_eq!(b[0].q.as_ref().unwrap().value, Some(8.0));
    assert_eq!(
        b[0].enabled,
        Some(false),
        "Enabled is read as a bool parameter's value"
    );
    assert_eq!(b[1].kind, None, "an unknown type");
}
