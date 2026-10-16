//! Apple's units through the plugin host and the engine (offline, never on
//! a device): an AU insert, an AU instrument, overlay values and state
//! capture.

use core_engine::backend::AudioBackend;
use core_engine::engine::{Engine, EngineConfig, find_sequence};
use core_engine::fixtures::{Builder, Q, demo_document};
use core_engine::offline::{OfflineBackend, OfflineSinks};
use core_host::PluginHost;
use core_model::command::{Command, MixerCmd};
use core_model::history::History;
use core_model::mixer::{ParamId, PluginFormat, PluginRef, StripKind};
use core_model::selection::ViewFilter;
use platform_macos::au::{AuFormat, component};
use std::time::Duration;

fn au(id: &str) -> PluginRef {
    component(id).unwrap().plugin
}

fn host() -> PluginHost {
    let mut h = PluginHost::new(48_000.0, 4096).unwrap();
    h.add_format(Box::new(AuFormat::new()));
    h
}

fn engine(h: &PluginHost) -> (Engine, OfflineBackend) {
    let (mut e, r) = Engine::new(
        EngineConfig::new(48_000.0),
        Box::new(OfflineSinks::new(1 << 16).with_tone()),
    )
    .unwrap();
    e.set_insert_factory(Box::new(h.factory()));
    let mut b = OfflineBackend::new(48_000.0, 512);
    b.start(r).unwrap();
    (e, b)
}

fn render(b: &mut OfflineBackend, frames: usize) -> Vec<f32> {
    let mut out = Vec::new();
    while out.len() < frames {
        b.block(512);
        out.extend_from_slice(b.output(512).0);
    }
    out
}

#[test]
fn a_dry_au_delay_insert_passes_the_strip_unchanged() {
    let doc = demo_document().unwrap();
    let song = find_sequence(&doc, "Song").unwrap();
    let (mut plain, mut pb) = engine(&host());
    plain.load(&doc, song).unwrap();
    plain.play().unwrap();
    let dry = render(&mut pb, 48_000);

    let mut b = Builder::from_doc(doc);
    let synth = b.strip_of(StripKind::BuiltinSynth).unwrap();
    let delay = b.insert(synth, au("aufx/dely/appl")).unwrap();
    b.mixer(MixerCmd::SetPluginParams {
        slot: delay,
        params: vec![(ParamId(0), 0.0)],
    })
    .unwrap();
    let doc = b.finish();
    let mut h = host();
    h.sync(&doc);
    let r = h.wait_ready(Duration::from_secs(10));
    assert_eq!(r.loaded, vec![delay], "{:?}", r.failed);
    let (mut e, mut be) = engine(&h);
    let report = e.load(&doc, song).unwrap();
    assert!(report.graph.placeholders.is_empty(), "{:?}", report.graph);
    e.play().unwrap();
    let wet = render(&mut be, 48_000);
    let err = dry
        .iter()
        .zip(&wet)
        .map(|(d, w)| (d - w).abs())
        .fold(0.0f32, f32::max);
    assert!(dry.iter().any(|x| *x != 0.0));
    assert!(err < 1e-3, "max difference {err}");
    drop(e);
}

#[test]
fn the_dls_unit_plays_as_a_plugin_instrument() {
    let mut b = Builder::new();
    let (d, id) = b.plugin_dest("GM", au("aumu/dls /appl")).unwrap();
    let (seq, lane) = b.sequence("S", "L", Some(d)).unwrap();
    b.notes(seq, lane, &[(0, 60, 0xFFFF, 4 * Q)]).unwrap();
    let doc = b.finish();
    let mut h = host();
    h.sync(&doc);
    assert_eq!(h.wait_ready(Duration::from_secs(10)).loaded, vec![id]);
    let (mut e, mut be) = engine(&h);
    let report = e.load(&doc, seq).unwrap();
    assert!(report.unroutable.is_empty(), "{:?}", report.warnings);
    e.play().unwrap();
    let out = render(&mut be, 24_000);
    assert!(out.iter().any(|x| x.abs() > 1e-3));
    drop(e);
}

#[test]
fn au_state_is_captured_and_restored() {
    let doc = demo_document().unwrap();
    let mut b = Builder::from_doc(doc);
    let synth = b.strip_of(StripKind::BuiltinSynth).unwrap();
    let delay = b.insert(synth, au("aufx/dely/appl")).unwrap();
    let mut hist = History::new(b.finish());
    hist.execute(
        &Command::Mixer(MixerCmd::SetPluginParams {
            slot: delay,
            params: vec![(ParamId(1), 0.25)],
        }),
        &ViewFilter::default(),
    )
    .unwrap();
    {
        let mut h = host();
        h.sync(hist.doc());
        h.wait_ready(Duration::from_secs(10));
        h.capture(&mut hist, delay).unwrap();
    }
    assert!(hist.doc().plugin_slot(delay).unwrap().params.is_empty());
    let mut fresh = host();
    fresh.sync(hist.doc());
    let r = fresh.wait_ready(Duration::from_secs(10));
    assert!(r.rejected_state.is_empty(), "{:?}", r.rejected_state);
    let v = fresh
        .instance(delay)
        .unwrap()
        .param_value(ParamId(1))
        .unwrap();
    assert!((v - 0.25).abs() < 1e-6, "{v}");
    assert_eq!(
        fresh.instance(delay).unwrap().info().plugin.format,
        PluginFormat::Au
    );
}
