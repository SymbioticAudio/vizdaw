use core_model::command::{Command, RoutingCmd, apply};
use core_model::document::{DestKind, Document};
use core_model::history::History;
use core_model::ids::{AnyId, DestId, OutputPathId, PluginInstanceId, StripId};
use core_model::mixer::{BUILTIN_SYNTH_STRIP, PluginRef, StripKind};
use core_model::selection::ViewFilter;

fn run(d: &Document, c: RoutingCmd) -> core_model::command::Applied {
    apply(d, &Command::Routing(c)).unwrap()
}

fn endpoint(r: &str, ch: u8) -> DestKind {
    DestKind::MidiEndpoint {
        endpoint_ref: r.into(),
        group: 0,
        channel: ch,
    }
}

fn add(d: &Document, name: &str, kind: DestKind) -> core_model::command::Applied {
    run(
        d,
        RoutingCmd::AddDestination {
            name: name.into(),
            kind,
            default_patch: None,
        },
    )
}

fn strips(d: &Document) -> Vec<(StripId, StripKind, String)> {
    d.mixer()
        .strips()
        .values()
        .map(|s| (s.id, s.kind, s.name.clone()))
        .collect()
}

#[test]
fn builtin_destinations_share_one_strip() {
    let a = add(&Document::new(), "Piano", endpoint("builtin:synth", 0));
    assert_eq!(
        a.report.created,
        vec![AnyId::Dest(DestId(0)), AnyId::Strip(StripId(1))]
    );
    let b = add(&a.doc, "Drums", endpoint("builtin:synth", 9));
    assert_eq!(b.report.created, vec![AnyId::Dest(DestId(1))]);
    let c = add(&b.doc, "Ext", endpoint("Some Synth", 0));
    assert_eq!(
        strips(&c.doc),
        vec![
            (StripId(0), StripKind::Master, "Master".into()),
            (
                StripId(1),
                StripKind::BuiltinSynth,
                BUILTIN_SYNTH_STRIP.into()
            ),
        ]
    );
    // Removing every user keeps the strip (Plan Ruling 1).
    let d = run(
        &c.doc,
        RoutingCmd::RemoveDestination {
            dest: DestId(0),
            replacement: None,
        },
    )
    .doc;
    let d = run(
        &d,
        RoutingCmd::RemoveDestination {
            dest: DestId(1),
            replacement: None,
        },
    )
    .doc;
    assert_eq!(
        d.mixer().find_kind(StripKind::BuiltinSynth),
        Some(StripId(1))
    );
}

#[test]
fn plugin_destination_gets_an_instance_and_an_instrument_strip() {
    let a = add(&Document::new(), "Pad", DestKind::plugin("au:pad"));
    assert_eq!(
        a.report.created,
        vec![
            AnyId::Dest(DestId(0)),
            AnyId::Plugin(PluginInstanceId(0)),
            AnyId::Strip(StripId(1)),
        ]
    );
    assert_eq!(
        strips(&a.doc)[1],
        (StripId(1), StripKind::Instrument(DestId(0)), "Pad".into())
    );
    // The caller's slot ID is ignored: a second plugin gets ID 1.
    let b = add(&a.doc, "Lead", DestKind::plugin("au:lead"));
    assert!(b.doc.plugin_slot(PluginInstanceId(1)).is_some());
    assert_eq!(b.doc.counters().plugin, 2);
    // Renaming the destination renames its strip.
    let c = run(
        &b.doc,
        RoutingCmd::RenameDestination {
            dest: DestId(0),
            name: "Warm Pad".into(),
        },
    );
    assert_eq!(c.doc.mixer().strip(StripId(1)).unwrap().name, "Warm Pad");
}

#[test]
fn editing_plugin_kinds_keeps_or_replaces_the_instance() {
    let a = add(&Document::new(), "Pad", DestKind::plugin("au:pad")).doc;
    // Same plugin, new metadata: same instance.
    let mut same = DestKind::plugin("au:pad");
    if let DestKind::PluginInstrument { slot } = &mut same {
        slot.plugin.version = "2.0".into();
        slot.id = PluginInstanceId(77);
    }
    let b = run(
        &a,
        RoutingCmd::EditDestination {
            dest: DestId(0),
            kind: same,
            default_patch: None,
        },
    );
    let slot = b.doc.plugin_slot(PluginInstanceId(0)).unwrap();
    assert_eq!(slot.plugin.version, "2.0");
    assert!(b.report.created.is_empty());
    // Another plugin: a new instance, same strip.
    let c = run(
        &b.doc,
        RoutingCmd::EditDestination {
            dest: DestId(0),
            kind: DestKind::plugin("au:other"),
            default_patch: None,
        },
    );
    assert_eq!(c.report.created, vec![AnyId::Plugin(PluginInstanceId(1))]);
    assert_eq!(
        c.doc.plugin_slot(PluginInstanceId(1)).unwrap().plugin,
        PluginRef::from_legacy("au:other")
    );
    assert_eq!(
        c.doc.mixer().find_kind(StripKind::Instrument(DestId(0))),
        Some(StripId(1))
    );
    // Plugin → built-in endpoint: the instrument strip goes, the built-in strip comes.
    let d = run(
        &c.doc,
        RoutingCmd::EditDestination {
            dest: DestId(0),
            kind: endpoint("builtin:synth", 0),
            default_patch: None,
        },
    );
    assert_eq!(d.report.removed, vec![AnyId::Strip(StripId(1))]);
    assert_eq!(
        strips(&d.doc),
        vec![
            (StripId(0), StripKind::Master, "Master".into()),
            (
                StripId(2),
                StripKind::BuiltinSynth,
                BUILTIN_SYNTH_STRIP.into()
            ),
        ]
    );
    // Endpoint → plugin: a new instrument strip.
    let e = run(
        &d.doc,
        RoutingCmd::EditDestination {
            dest: DestId(0),
            kind: DestKind::plugin("au:pad"),
            default_patch: None,
        },
    );
    assert_eq!(
        e.doc.mixer().find_kind(StripKind::Instrument(DestId(0))),
        Some(StripId(3))
    );
}

#[test]
fn instrument_slots_refuse_a_sidechain() {
    let mut k = DestKind::plugin("au:pad");
    if let DestKind::PluginInstrument { slot } = &mut k {
        slot.sidechain = Some(StripId(0));
    }
    let r = apply(
        &Document::new(),
        &Command::Routing(RoutingCmd::AddDestination {
            name: "Pad".into(),
            kind: k,
            default_patch: None,
        }),
    );
    assert!(r.is_err());
}

#[test]
fn output_paths_get_audio_path_strips() {
    let a = run(
        &Document::new(),
        RoutingCmd::AddOutput {
            name: "Main".into(),
            reference: "bus:0".into(),
        },
    );
    assert_eq!(
        a.report.created,
        vec![AnyId::Output(OutputPathId(0)), AnyId::Strip(StripId(1))]
    );
    let b = run(
        &a.doc,
        RoutingCmd::RenameOutput {
            output: OutputPathId(0),
            name: "Stems".into(),
        },
    );
    assert_eq!(b.doc.mixer().strip(StripId(1)).unwrap().name, "Stems");
    let c = run(
        &b.doc,
        RoutingCmd::RemoveOutput {
            output: OutputPathId(0),
            replacement: None,
        },
    );
    assert_eq!(
        c.report.removed,
        vec![AnyId::Output(OutputPathId(0)), AnyId::Strip(StripId(1))]
    );
    assert_eq!(c.doc.mixer().strips().len(), 1);
}

#[test]
fn removing_a_plugin_destination_is_one_undo_step() {
    let start = Document::new();
    let mut h = History::new(start.clone());
    let f = ViewFilter::default();
    h.execute(
        &Command::Routing(RoutingCmd::AddDestination {
            name: "Pad".into(),
            kind: DestKind::plugin("au:pad"),
            default_patch: None,
        }),
        &f,
    )
    .unwrap();
    let before = h.doc().clone();
    assert_eq!(before.mixer().strips().len(), 2);
    assert!(
        before
            .mixer()
            .find_kind(StripKind::Instrument(DestId(0)))
            .is_some()
    );
    assert!(before.plugin_slot(PluginInstanceId(0)).is_some());
    assert_eq!(h.undo_depth(), 1);
    h.execute(
        &Command::Routing(RoutingCmd::RemoveDestination {
            dest: DestId(0),
            replacement: None,
        }),
        &f,
    )
    .unwrap();
    assert_eq!(h.undo_depth(), 2);
    assert_eq!(h.doc().mixer().strips().len(), 1);
    assert!(
        h.doc()
            .mixer()
            .find_kind(StripKind::Instrument(DestId(0)))
            .is_none()
    );
    assert!(h.doc().plugin_slot(PluginInstanceId(0)).is_none());
    h.undo().unwrap();
    assert_eq!(h.undo_depth(), 1);
    assert_eq!(h.doc(), &before);
    assert!(
        h.doc()
            .mixer()
            .find_kind(StripKind::Instrument(DestId(0)))
            .is_some()
    );
    assert!(h.doc().plugin_slot(PluginInstanceId(0)).is_some());
    // Undoing the add returns strips and counters to the start.
    h.undo().unwrap();
    assert_eq!(h.doc(), &start);
    assert_eq!(h.doc().mixer().strips().len(), 1);
    assert_eq!(h.doc().counters(), start.counters());
}
