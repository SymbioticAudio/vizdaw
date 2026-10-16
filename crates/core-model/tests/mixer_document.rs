use core_model::document::{
    DestKind, Destination, Document, DocumentRepr, OutputPath, Settings, SlotOwner,
};
use core_model::file::{from_json, to_json};
use core_model::ids::{Counters, DestId, OutputPathId, PluginInstanceId, StripId};
use core_model::mixer::{BUILTIN_SYNTH_STRIP, Output, PluginRef, StripKind};

fn ep(id: u64, r: &str, ch: u8) -> Destination {
    Destination {
        id: DestId(id),
        name: format!("D{id}"),
        kind: DestKind::MidiEndpoint {
            endpoint_ref: r.into(),
            group: 0,
            channel: ch,
        },
        default_patch: None,
    }
}

fn kinds(d: &Document) -> Vec<(StripId, StripKind, String)> {
    d.mixer()
        .strips()
        .values()
        .map(|s| (s.id, s.kind, s.name.clone()))
        .collect()
}

#[test]
fn counters_allocate_strips_and_instances() {
    let mut c = Counters::default();
    assert_eq!(c.alloc_strip(), StripId(0));
    assert_eq!(c.alloc_strip(), StripId(1));
    assert_eq!(c.alloc_plugin(), PluginInstanceId(0));
    assert_eq!((c.strip, c.plugin), (2, 1));
}

#[test]
fn a_new_document_has_only_master() {
    let d = Document::new();
    assert_eq!(
        kinds(&d),
        vec![(StripId(0), StripKind::Master, "Master".into())]
    );
    assert_eq!(d.mixer().master(), StripId(0));
    assert_eq!(d.counters().strip, 1);
}

#[test]
fn from_parts_without_a_mixer_derives_one() {
    let d = Document::from_parts(DocumentRepr {
        settings: Settings::default(),
        instruments: vec![
            Destination {
                id: DestId(0),
                name: "Pad".into(),
                kind: DestKind::plugin("au:pad"),
                default_patch: None,
            },
            ep(1, "builtin:synth", 0),
            ep(2, "builtin:synth", 9),
            ep(3, "Ext Synth", 0),
        ],
        outputs: vec![OutputPath {
            id: OutputPathId(0),
            name: "Main".into(),
            reference: "bus:main".into(),
        }],
        media: vec![],
        sequences: vec![],
        mixer: None,
        counters: Counters {
            dest: 4,
            output: 1,
            plugin: 1,
            ..Counters::default()
        },
    })
    .unwrap();
    assert_eq!(
        kinds(&d),
        vec![
            (StripId(0), StripKind::Master, "Master".into()),
            (StripId(1), StripKind::Instrument(DestId(0)), "Pad".into()),
            (
                StripId(2),
                StripKind::BuiltinSynth,
                BUILTIN_SYNTH_STRIP.into()
            ),
            (
                StripId(3),
                StripKind::AudioPath(OutputPathId(0)),
                "Main".into()
            ),
        ]
    );
    assert_eq!(
        d.mixer().order().iter().copied().collect::<Vec<_>>(),
        vec![StripId(1), StripId(2), StripId(3)]
    );
    assert!(
        d.mixer()
            .strips()
            .values()
            .all(|s| s.output == Output::Master)
    );
    assert_eq!(d.counters().strip, 4);
    assert_eq!(
        d.find_slot(PluginInstanceId(0)),
        Some(SlotOwner::Instrument(DestId(0)))
    );
    assert_eq!(
        d.plugin_slot(PluginInstanceId(0)).unwrap().plugin,
        PluginRef::from_legacy("au:pad")
    );
    assert_eq!(d.find_slot(PluginInstanceId(9)), None);
}

#[test]
fn json_round_trip_keeps_the_mixer() {
    let d = Document::new();
    let text = to_json(&d).unwrap();
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["version"], 2);
    assert_eq!(v["document"]["mixer"]["master"], 0);
    assert_eq!(v["document"]["counters"]["strip"], 1);
    assert_eq!(from_json(&text).unwrap(), d);
}

#[test]
fn builtin_and_plugin_helpers() {
    assert!(ep(0, "builtin:synth", 3).kind.is_builtin_synth());
    assert!(!ep(0, "virtual:out", 3).kind.is_builtin_synth());
    let DestKind::PluginInstrument { slot } = DestKind::plugin("clap:x") else {
        panic!("not a plugin");
    };
    assert_eq!(slot.id, PluginInstanceId(0));
    assert_eq!(slot.plugin.name, "clap:x");
    assert!(!DestKind::plugin("clap:x").is_builtin_synth());
}
