use core_model::ModelError;
use core_model::ids::{AnyId, DestId, PluginInstanceId, StateBlobId, StripId};
use core_model::mixer::{
    AuxSend, Mixer, MixerRepr, Output, ParamId, PluginFormat, PluginRef, PluginSlot, Strip,
    StripKind, db_to_gain, pan_gains,
};
use std::f64::consts::FRAC_1_SQRT_2;

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

#[test]
fn new_ids_display() {
    assert_eq!(AnyId::Strip(StripId(3)).to_string(), "strip3");
    assert_eq!(AnyId::Plugin(PluginInstanceId(4)).to_string(), "plug4");
    assert_eq!(StateBlobId(5).to_string(), "state5");
}

#[test]
fn routing_errors_name_the_strips() {
    let e = ModelError::RoutingCycle {
        path: vec![StripId(1), StripId(2), StripId(1)],
    };
    assert!(e.to_string().contains("StripId(2)"), "{e}");
    let e = ModelError::InvalidRoutingTarget {
        strip: StripId(1),
        target: StripId(0),
        reason: "sends target buses only".into(),
    };
    assert_eq!(
        e.to_string(),
        "strip1 cannot route to strip0: sends target buses only"
    );
}

#[test]
fn fader_and_pan_laws() {
    assert!(close(db_to_gain(0.0), 1.0));
    assert!(close(db_to_gain(-144.0), 0.0));
    assert!(close(db_to_gain(-200.0), 0.0));
    assert!((db_to_gain(-6.020_6) - 0.5).abs() < 1e-4);
    assert!((db_to_gain(12.0) - 3.981_071_705_534_973).abs() < 1e-9);
    let (l, r) = pan_gains(0.0);
    assert!(close(l, FRAC_1_SQRT_2) && close(r, FRAC_1_SQRT_2));
    let (l, r) = pan_gains(-1.0);
    assert!(close(l, 1.0) && r.abs() < 1e-12);
    let (l, r) = pan_gains(1.0);
    assert!(l.abs() < 1e-12 && close(r, 1.0));
    assert_eq!(pan_gains(5.0), pan_gains(1.0));
}

#[test]
fn strip_defaults() {
    let s = Strip::new(StripId(2), "Bus A", StripKind::Bus);
    assert_eq!((s.gain_db, s.pan, s.mute, s.solo), (0.0, 0.0, false, false));
    assert_eq!(s.output, Output::Master);
    assert!(s.inserts.is_empty() && s.sends.is_empty());
    assert!(StripKind::BuiltinSynth.is_instrument());
    assert!(StripKind::Instrument(DestId(0)).is_instrument());
    assert!(!StripKind::InstrumentOutput(DestId(0), 1).is_instrument());
    assert!(!StripKind::Bus.is_instrument());
    let send = AuxSend::new(StripId(4));
    assert_eq!(
        (send.target, send.level_db, send.pre_fader, send.enabled),
        (StripId(4), 0.0, false, true)
    );
}

#[test]
fn legacy_plugin_refs() {
    let r = PluginRef::from_legacy("au:piano");
    assert_eq!(
        (r.format, r.id.as_str(), r.name.as_str(), r.vendor.as_str()),
        (PluginFormat::Au, "au:piano", "au:piano", "")
    );
    assert_eq!(
        PluginRef::from_legacy("clap:perc").format,
        PluginFormat::Clap
    );
    assert_eq!(PluginRef::from_legacy("p").format, PluginFormat::Clap);
}

fn sample_mixer() -> Mixer {
    let mut slot = PluginSlot::new(PluginInstanceId(0), PluginRef::from_legacy("clap:comp"));
    slot.sidechain = Some(StripId(1));
    slot.params.insert(ParamId(3), 0.25);
    let mut bus = Strip::new(StripId(2), "Verb", StripKind::Bus);
    bus.inserts.push_back(slot);
    let mut synth = Strip::new(StripId(1), "Built-in Synth", StripKind::BuiltinSynth);
    synth.sends.push_back(AuxSend::new(StripId(2)));
    synth.output = Output::Bus(StripId(2));
    Mixer::try_from(MixerRepr {
        master: StripId(0),
        order: vec![StripId(1), StripId(2)],
        strips: vec![
            Strip::new(StripId(0), "Master", StripKind::Master),
            synth,
            bus,
        ],
    })
    .unwrap()
}

#[test]
fn mixer_json_round_trip() {
    let m = sample_mixer();
    let v = serde_json::to_value(&m).unwrap();
    assert_eq!(v["master"], 0);
    assert_eq!(v["strips"][0]["kind"], "master");
    assert_eq!(v["strips"][1]["kind"], "builtin_synth");
    assert_eq!(v["strips"][1]["output"], serde_json::json!({"bus": 2}));
    assert_eq!(
        v["strips"][2]["inserts"][0]["params"],
        serde_json::json!([[3, 0.25]])
    );
    assert_eq!(v["strips"][2]["inserts"][0]["plugin"]["format"], "clap");
    let back: Mixer = serde_json::from_value(v).unwrap();
    assert_eq!(back, m);
    assert_eq!(back.strip(StripId(2)).unwrap().name, "Verb");
    assert_eq!(
        back.strip(StripId(9)).unwrap_err(),
        ModelError::UnknownId(AnyId::Strip(StripId(9)))
    );
    assert_eq!(back.find_kind(StripKind::BuiltinSynth), Some(StripId(1)));
    assert_eq!(
        back.find_kind(StripKind::AudioPath(core_model::ids::OutputPathId(0))),
        None
    );
    let slots: Vec<_> = back.inserts().map(|(s, i, p)| (s, i, p.id)).collect();
    assert_eq!(slots, vec![(StripId(2), 0, PluginInstanceId(0))]);
}

#[test]
fn unknown_fields_and_duplicate_strips_are_refused() {
    let mut v = serde_json::to_value(sample_mixer()).unwrap();
    v["strips"][1]["bogus"] = serde_json::json!(1);
    assert!(serde_json::from_value::<Mixer>(v).is_err());
    let r = Mixer::try_from(MixerRepr {
        master: StripId(0),
        order: vec![],
        strips: vec![
            Strip::new(StripId(0), "Master", StripKind::Master),
            Strip::new(StripId(0), "X", StripKind::Bus),
        ],
    });
    assert!(r.unwrap_err().contains("duplicate strip id strip0"));
}

#[test]
fn a_new_mixer_holds_only_master() {
    let m = Mixer::new(StripId(7));
    assert_eq!(m.master(), StripId(7));
    assert!(m.order().is_empty());
    let s = m.strip(StripId(7)).unwrap();
    assert_eq!((s.name.as_str(), s.kind), ("Master", StripKind::Master));
}
