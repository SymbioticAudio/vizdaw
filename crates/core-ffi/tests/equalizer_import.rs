//! A DAWproject `Equalizer` imported through the session (plan 6g Task 5,
//! Ruling 10): with the bundled Multi-Q usable (the catalog lookup replaced
//! by `bundled::debug_set_multiq`) and the switch on, the channel with an
//! `Equalizer` gets the mapped Multi-Q in its EQ slot in place of the flat
//! default, and the channel with none keeps the default. Offline only.

#![recursion_limit = "256"]

mod common;

use core_io::dawproject::multiq::map_equalizer;
use core_io::dawproject::{DpEqBand, DpEqBandType, DpNamed, DpRealParam, DpUnit};
use core_io::zip::{EntryMethod, ZipWriter};
use core_model::document::Document;
use core_model::mixer::{ParamId, PluginFormat, PluginRef, StripKind};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Mutex, MutexGuard};
use vizdaw_ffi::bundled::{debug_clear_multiq, debug_set_multiq};
use vizdaw_ffi::host::{force_offline, reset_host};
use vizdaw_ffi::{BitDepthRow, JobStateRow, ProjectFormatRow, Session, set_new_channel_eq};

static SERIAL: Mutex<()> = Mutex::new(());

/// One test at a time; offline; the switch on and the override cleared.
fn serial() -> MutexGuard<'static, ()> {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    force_offline();
    vizdaw_ffi::host::set_offline_rate(48_000.0);
    reset_host();
    set_new_channel_eq(true);
    debug_clear_multiq();
    g
}

fn mq() -> PluginRef {
    PluginRef {
        format: PluginFormat::Clap,
        id: "com.dusk-audio.multi-q".into(),
        name: "Multi-Q".into(),
        vendor: "Dusk Audio".into(),
        version: "0.10.9".into(),
    }
}

fn text(p: &Path) -> String {
    p.display().to_string()
}

/// The session's document.
fn doc_of(s: &Session) -> Document {
    core_model::file::from_json(&s.debug_document_json()).unwrap()
}

const PROJECT: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Project version="1.0">
  <Application name="Fixture" version="1"/>
  <Structure>
    <Track id="m" name="Master" contentType="audio notes"><Channel id="mc" role="master"/></Track>
    <Track id="a" name="Eq" contentType="audio"><Channel id="ac" role="regular"><Devices><Equalizer deviceName="Channel EQ" deviceRole="audioFX"><Band type="bell"><Freq unit="hertz" value="1000"/><Gain unit="decibel" value="6"/><Q unit="linear" value="1"/></Band></Equalizer></Devices></Channel></Track>
    <Track id="b" name="Plain" contentType="audio"><Channel id="bc" role="regular"/></Track>
  </Structure>
  <Arrangement/>
</Project>"#;

/// The bell the file's `Equalizer` holds.
fn file_band() -> DpEqBand {
    let p = |unit, v| {
        Some(DpRealParam {
            named: DpNamed::default(),
            parameter_id: None,
            unit,
            value: Some(v),
            min: None,
            max: None,
        })
    };
    DpEqBand {
        kind: Some(DpEqBandType::Bell),
        order: None,
        freq: p(DpUnit::Hertz, 1000.0),
        gain: p(DpUnit::Decibel, 6.0),
        q: p(DpUnit::Linear, 1.0),
        enabled: None,
    }
}

#[test]
fn equalizer_import_replaces_the_default() {
    use common::dawproject::wait_job;
    let _g = serial();
    debug_set_multiq(Some(mq()));

    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("eq.dawproject");
    let mut z = ZipWriter::new(std::fs::File::create(&file).unwrap());
    z.add("project.xml", EntryMethod::Deflated, PROJECT.as_bytes())
        .unwrap();
    z.finish().unwrap();

    let s = Session::new_project(ProjectFormatRow {
        sample_rate: 48_000,
        depth: BitDepthRow::Int24,
    })
    .unwrap();
    s.debug_set_trash_dir(Some(text(&dir.path().join("Trash"))));
    s.debug_free_bytes(None).unwrap();
    s.save_as(text(&dir.path().join("I.vizdaw"))).unwrap();
    let j = wait_job(
        &s,
        s.import_dawproject(vizdaw_ffi::DawprojectImportRequestRow { path: text(&file) })
            .unwrap(),
    );
    assert_eq!(j.state, JobStateRow::Done, "{:?}", j.message);

    let doc = doc_of(&s);
    let seq = doc.sequences().keys().next().copied().unwrap();
    let strip_of = |name: &str| {
        let lane = doc
            .sequence(seq)
            .unwrap()
            .lanes
            .iter()
            .find(|l| l.name == name)
            .unwrap_or_else(|| panic!("no track {name}"));
        let out = lane.audio_out.expect("an output path");
        let id = doc.mixer().find_kind(StripKind::AudioPath(out)).unwrap();
        doc.mixer().strip(id).unwrap().clone()
    };

    let eq_strip = strip_of("Eq");
    let eq = eq_strip.eq.as_ref().expect("the mapped Multi-Q");
    assert_eq!(eq.plugin, mq());
    assert!(!eq.bypass);
    let want: BTreeMap<ParamId, f64> = map_equalizer(&[file_band()], "Eq")
        .params
        .into_iter()
        .collect();
    let got: BTreeMap<ParamId, f64> = eq.params.iter().map(|(k, v)| (*k, *v)).collect();
    assert_eq!(got, want);
    assert!(eq_strip.inserts.is_empty(), "one slot, no second Multi-Q");

    let plain = strip_of("Plain");
    let def = plain.eq.as_ref().expect("the flat default");
    assert_eq!(def.plugin, mq());
    assert!(def.params.is_empty());

    // No orphan default: the only Multi-Q slots are these two.
    let mut multiqs: Vec<_> = doc
        .mixer()
        .strips()
        .values()
        .flat_map(|x| x.eq.iter().chain(x.inserts.iter()))
        .filter(|slot| slot.plugin == mq())
        .map(|slot| slot.id)
        .collect();
    multiqs.sort();
    let mut want_ids = vec![eq.id, def.id];
    want_ids.sort();
    assert_eq!(multiqs, want_ids);
    for id in multiqs {
        assert!(doc.find_slot(id).is_some());
    }
    s.close().unwrap();
    debug_clear_multiq();
}
