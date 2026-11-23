//! VCAs in the model (plan groups-vcas Task 3; spec §2): a VCA's level is
//! an offset on its members' heard level, and its mute and solo are its
//! own flags. No VCA command ever rewrites a member's strip.

use core_model::ModelError;
use core_model::automation::{AutomationMode, AutomationTarget};
use core_model::command::{
    AutomationWrite, Command, GroupCmd, LaneCmd, MixerCmd, RoutingCmd, SequenceCmd, apply,
};
use core_model::document::{DestKind, Document};
use core_model::file::{VERSION, from_json, to_json};
use core_model::groups::{
    MAX_MEMBERS, VCA_MODES, heard_db, vca_muted, vca_offset_db, vca_soloed, vcas_of,
};
use core_model::history::History;
use core_model::ids::{AnyId, SeqId, StripId, VcaId};
use core_model::mixer::{GAIN_FLOOR_DB, Strip, Swatch};
use core_model::mixer_rules::audible;
use core_model::rules::validate;
use core_model::selection::ViewFilter;
use core_model::time::{PPQ, Tick};
use serde_json::Value;

fn run(d: &Document, c: Command) -> core_model::Result<Document> {
    apply(d, &c).map(|a| a.doc)
}

fn vca(d: &Document, c: GroupCmd) -> core_model::Result<Document> {
    run(d, Command::Groups(c))
}

fn id_of(d: &Document, name: &str) -> StripId {
    d.mixer()
        .strips()
        .values()
        .find(|s| s.name == name)
        .map(|s| s.id)
        .unwrap()
}

/// Strips A, B and C (plug-in instruments), a bus "Bus" and the master.
/// A and B go to the bus; C goes to the master.
fn base() -> Document {
    let mut d = Document::new();
    for n in ["A", "B", "C"] {
        d = run(
            &d,
            Command::Routing(RoutingCmd::AddDestination {
                name: n.into(),
                kind: DestKind::plugin(&format!("au:{n}")),
                default_patch: None,
            }),
        )
        .unwrap();
    }
    d = run(
        &d,
        Command::Mixer(MixerCmd::AddBus {
            name: "Bus".into(),
            index: None,
        }),
    )
    .unwrap();
    let bus = id_of(&d, "Bus");
    for n in ["A", "B"] {
        d = run(
            &d,
            Command::Mixer(MixerCmd::SetOutput {
                strip: id_of(&d, n),
                output: core_model::mixer::Output::Bus(bus),
            }),
        )
        .unwrap();
    }
    assert!(validate(&d).is_empty(), "{:?}", validate(&d));
    d
}

/// `d` with a new VCA `name` of `members`, and its ID.
fn new_vca(d: &Document, name: &str, members: Vec<StripId>) -> (Document, VcaId) {
    let a = apply(
        d,
        &Command::Groups(GroupCmd::NewVca {
            name: name.into(),
            colour: Swatch::Purple,
            members,
        }),
    )
    .unwrap();
    let Some(AnyId::Vca(id)) = a.report.created.first().copied() else {
        panic!("no VCA created: {:?}", a.report.created);
    };
    (a.doc, id)
}

fn level(d: &Document, v: VcaId, gain_db: f64) -> Document {
    vca(d, GroupCmd::SetVcaLevel { vca: v, gain_db }).unwrap()
}

fn strips(d: &Document) -> Vec<Strip> {
    d.mixer().strips().values().cloned().collect()
}

#[test]
fn heard_level_is_fader_plus_offsets() {
    let d = base();
    let a = id_of(&d, "A");
    let d = run(
        &d,
        Command::Mixer(MixerCmd::SetGain {
            strip: a,
            gain_db: -6.0,
        }),
    )
    .unwrap();
    let (d, v1) = new_vca(&d, "One", vec![a]);
    let (d, v2) = new_vca(&d, "Two", vec![a]);
    let d = level(&level(&d, v1, -3.0), v2, 2.0);
    assert_eq!(vca_offset_db(d.mixer(), a), -1.0);
    assert_eq!(heard_db(-6.0, -1.0), -7.0);
    assert_eq!(
        heard_db(
            d.mixer().strip(a).unwrap().gain_db,
            vca_offset_db(d.mixer(), a)
        ),
        -7.0
    );
    assert_eq!(vcas_of(d.mixer(), a).count(), 2);
    assert_eq!(vca_offset_db(d.mixer(), id_of(&d, "B")), 0.0);
    assert_eq!(vcas_of(d.mixer(), id_of(&d, "B")).count(), 0);
}

#[test]
fn a_vca_at_minus_infinity_silences() {
    let d = base();
    let a = id_of(&d, "A");
    let (d, v1) = new_vca(&d, "One", vec![a]);
    let (d, v2) = new_vca(&d, "Two", vec![a]);
    let d = level(&level(&d, v1, -144.0), v2, 6.0);
    assert_eq!(vca_offset_db(d.mixer(), a), GAIN_FLOOR_DB);
    assert_eq!(heard_db(0.0, vca_offset_db(d.mixer(), a)), GAIN_FLOOR_DB);
    assert_eq!(heard_db(-144.0, 6.0), GAIN_FLOOR_DB);
    assert_eq!(heard_db(-100.0, -50.0), GAIN_FLOOR_DB);
    assert_eq!(heard_db(f64::NAN, 0.0), GAIN_FLOOR_DB);
}

#[test]
fn heard_total_clamps_at_the_top() {
    assert_eq!(heard_db(10.0, 6.0), 12.0);
    assert_eq!(heard_db(12.0, 0.0), 12.0);
    assert_eq!(heard_db(0.0, 0.0), 0.0);
}

#[test]
fn vca_mute_and_solo_are_flags() {
    let d = base();
    let [a, b, c, bus] = ["A", "B", "C", "Bus"].map(|n| id_of(&d, n));
    let (d, v) = new_vca(&d, "Kit", vec![a]);
    let before = strips(&d);
    let d = vca(&d, GroupCmd::SetVcaMute { vca: v, mute: true }).unwrap();
    let d = vca(&d, GroupCmd::SetVcaSolo { vca: v, solo: true }).unwrap();
    assert_eq!(strips(&d), before);
    for s in d.mixer().strips().values() {
        assert!(!s.mute && !s.solo, "{} changed", s.name);
    }
    assert!(vca_muted(d.mixer(), a));
    assert!(vca_soloed(d.mixer(), a));
    assert!(!vca_muted(d.mixer(), b));
    assert!(!vca_soloed(d.mixer(), b));
    let heard = audible(d.mixer());
    assert!(heard.contains(&a));
    assert!(heard.contains(&bus), "what A reaches");
    assert!(heard.contains(&d.mixer().master()));
    assert!(!heard.contains(&c), "an unrelated strip");
    assert!(!heard.contains(&b), "a sibling into the same bus");
    let d = vca(
        &d,
        GroupCmd::SetVcaSolo {
            vca: v,
            solo: false,
        },
    )
    .unwrap();
    assert_eq!(audible(d.mixer()).len(), d.mixer().strips().len());
}

/// `base()` with a sequence whose lane holds automation for A's fader,
/// A in Touch, A at −4 and muted, and a VCA "Kit" of A and B.
fn automated() -> (Document, VcaId) {
    let mut d = base();
    let a = id_of(&d, "A");
    d = run(&d, Command::Sequence(SequenceCmd::New { name: "S".into() })).unwrap();
    d = run(
        &d,
        Command::Lane(LaneCmd::Add {
            seq: SeqId(0),
            name: "L".into(),
            index: None,
        }),
    )
    .unwrap();
    d = run(
        &d,
        Command::WriteAutomation(AutomationWrite {
            seq: SeqId(0),
            target: AutomationTarget::StripFader(a),
            start: Tick(0),
            end: Tick(4 * PPQ),
            points: vec![(Tick(0), 0.5), (Tick(2 * PPQ), 0.25)],
            lane_name: "Automation".into(),
        }),
    )
    .unwrap();
    d = run(
        &d,
        Command::Mixer(MixerCmd::SetAutomationMode {
            strips: vec![a],
            mode: AutomationMode::Touch,
        }),
    )
    .unwrap();
    d = run(
        &d,
        Command::Mixer(MixerCmd::SetGain {
            strip: a,
            gain_db: -4.0,
        }),
    )
    .unwrap();
    d = run(
        &d,
        Command::Mixer(MixerCmd::SetMute {
            strip: a,
            mute: true,
        }),
    )
    .unwrap();
    assert!(d.all_events().count() >= 2);
    let b = id_of(&d, "B");
    new_vca(&d, "Kit", vec![a, b])
}

#[test]
fn members_faders_are_never_rewritten() {
    let (start, v) = automated();
    let [a, b, c] = ["A", "B", "C"].map(|n| id_of(&start, n));
    let events: Vec<_> = start.all_events().map(|(_, e)| e.clone()).collect();
    let cmds = vec![
        GroupCmd::RenameVca {
            vca: v,
            name: "Drums".into(),
        },
        GroupCmd::SetVcaColour {
            vca: v,
            colour: Swatch::Red,
        },
        GroupCmd::AssignVca {
            vca: v,
            strips: vec![c],
        },
        GroupCmd::SetVcaLevel {
            vca: v,
            gain_db: -12.0,
        },
        GroupCmd::SetVcaLevel {
            vca: v,
            gain_db: -144.0,
        },
        GroupCmd::SetVcaMute { vca: v, mute: true },
        GroupCmd::SetVcaSolo { vca: v, solo: true },
        GroupCmd::SetVcaMode {
            vca: v,
            mode: AutomationMode::Latch,
        },
        GroupCmd::SetVcaMute {
            vca: v,
            mute: false,
        },
        GroupCmd::UnassignVca {
            vca: v,
            strips: vec![a, c],
        },
        GroupCmd::NewVca {
            name: "Other".into(),
            colour: Swatch::Green,
            members: vec![a, b],
        },
        GroupCmd::DeleteVca { vca: v },
    ];
    let mut d = start.clone();
    for cmd in cmds {
        d = vca(&d, cmd.clone()).unwrap_or_else(|e| panic!("{cmd:?}: {e}"));
        assert_eq!(strips(&d), strips(&start), "after {cmd:?}");
        let now: Vec<_> = d.all_events().map(|(_, e)| e.clone()).collect();
        assert_eq!(now, events, "after {cmd:?}");
        assert!(validate(&d).is_empty(), "{:?}", validate(&d));
    }
    let s = d.mixer().strip(a).unwrap();
    assert_eq!(
        (s.gain_db, s.mute, s.solo, s.automation),
        (-4.0, true, false, AutomationMode::Touch)
    );
}

#[test]
fn deleting_a_vca_leaves_members_as_their_faders_say() {
    let d = base();
    let [a, b] = ["A", "B"].map(|n| id_of(&d, n));
    let (d, v) = new_vca(&d, "Kit", vec![a, b]);
    let d = level(&d, v, -6.0);
    let d = vca(&d, GroupCmd::SetVcaMute { vca: v, mute: true }).unwrap();
    let before = strips(&d);
    let a2 = apply(&d, &Command::Groups(GroupCmd::DeleteVca { vca: v })).unwrap();
    assert_eq!(a2.report.removed, vec![AnyId::Vca(v)]);
    let d = a2.doc;
    assert!(d.mixer().vcas().is_empty());
    for s in [a, b] {
        assert_eq!(vca_offset_db(d.mixer(), s), 0.0);
        assert!(!vca_muted(d.mixer(), s));
    }
    assert_eq!(strips(&d), before);
}

#[test]
fn a_strip_may_be_in_a_group_and_many_vcas() {
    let d = base();
    let [a, b] = ["A", "B"].map(|n| id_of(&d, n));
    let d = vca(
        &d,
        GroupCmd::NewGroup {
            name: "Drums".into(),
            colour: Swatch::Blue,
            members: vec![a, b],
        },
    )
    .unwrap();
    let (d, _) = new_vca(&d, "One", vec![a]);
    let (d, _) = new_vca(&d, "Two", vec![a, b]);
    let (d, three) = new_vca(&d, "Three", vec![]);
    let d = vca(
        &d,
        GroupCmd::AssignVca {
            vca: three,
            strips: vec![a],
        },
    )
    .unwrap();
    assert_eq!(vcas_of(d.mixer(), a).count(), 3);
    assert!(validate(&d).is_empty(), "{:?}", validate(&d));
}

#[test]
fn new_vca_defaults_and_first_id() {
    let d = base();
    let (d, v) = new_vca(&d, "  Kit ", vec![]);
    assert_eq!(v, VcaId(1), "the first VCA is ID 1 (preflight F1)");
    let x = d.mixer().vca(v).unwrap();
    assert_eq!(x.name, "Kit");
    assert_eq!(x.colour, Swatch::Purple);
    assert!(x.members.is_empty());
    assert_eq!((x.gain_db, x.mute, x.solo), (0.0, false, false));
    assert_eq!(x.automation, AutomationMode::Read);
    assert_eq!(
        d.mixer().vca(VcaId(77)),
        Err(ModelError::UnknownId(AnyId::Vca(VcaId(77))))
    );
}

#[test]
fn removing_a_strip_removes_its_vca_membership() {
    let d = base();
    let [a, bus] = ["A", "Bus"].map(|n| id_of(&d, n));
    let (d, v) = new_vca(&d, "Kit", vec![a, bus]);
    let mut h = History::new(d.clone());
    h.execute(
        &Command::Mixer(MixerCmd::RemoveBus { strip: bus }),
        &ViewFilter::default(),
    )
    .unwrap();
    let members: Vec<StripId> = h
        .doc()
        .mixer()
        .vca(v)
        .unwrap()
        .members
        .iter()
        .copied()
        .collect();
    assert_eq!(members, vec![a]);
    assert!(validate(h.doc()).is_empty(), "{:?}", validate(h.doc()));
    h.undo().unwrap();
    assert_eq!(h.doc(), &d);
}

#[test]
fn vca_modes_are_off_read_touch_latch() {
    assert_eq!(
        VCA_MODES,
        [
            AutomationMode::Off,
            AutomationMode::Read,
            AutomationMode::Touch,
            AutomationMode::Latch
        ]
    );
    let d = base();
    let (d, v) = new_vca(&d, "Kit", vec![]);
    for mode in VCA_MODES {
        let d = vca(&d, GroupCmd::SetVcaMode { vca: v, mode }).unwrap();
        assert_eq!(d.mixer().vca(v).unwrap().automation, mode);
    }
    for mode in [AutomationMode::Write, AutomationMode::Trim] {
        assert_eq!(
            vca(&d, GroupCmd::SetVcaMode { vca: v, mode }),
            Err(ModelError::invalid(
                "A VCA's automation mode is Off, Read, Touch or Latch."
            ))
        );
    }
}

#[test]
fn vca_command_names() {
    let v = VcaId(1);
    let names: Vec<&str> = [
        GroupCmd::NewVca {
            name: String::new(),
            colour: Swatch::Red,
            members: vec![],
        },
        GroupCmd::RenameVca {
            vca: v,
            name: String::new(),
        },
        GroupCmd::SetVcaColour {
            vca: v,
            colour: Swatch::Red,
        },
        GroupCmd::AssignVca {
            vca: v,
            strips: vec![],
        },
        GroupCmd::UnassignVca {
            vca: v,
            strips: vec![],
        },
        GroupCmd::SetVcaLevel {
            vca: v,
            gain_db: 0.0,
        },
        GroupCmd::SetVcaMute { vca: v, mute: true },
        GroupCmd::SetVcaSolo { vca: v, solo: true },
        GroupCmd::SetVcaMode {
            vca: v,
            mode: AutomationMode::Off,
        },
        GroupCmd::DeleteVca { vca: v },
    ]
    .iter()
    .map(GroupCmd::name)
    .collect();
    assert_eq!(
        names,
        [
            "New VCA",
            "Rename VCA",
            "VCA Colour",
            "Assign to VCA",
            "Remove from VCA",
            "Set VCA",
            "Mute VCA",
            "Solo VCA",
            "Set VCA Automation Mode",
            "Delete VCA"
        ]
    );
}

#[test]
fn hostile_vca_commands_change_nothing() {
    let d = base();
    let [a, b] = ["A", "B"].map(|n| id_of(&d, n));
    let (d, v) = new_vca(&d, "Kit", vec![a]);
    let nowhere = VcaId(u64::MAX);
    let long = "x".repeat(70_000);
    let many: Vec<StripId> = (0..=MAX_MEMBERS as u64).map(StripId).collect();
    let master = d.mixer().master();
    let unknown = StripId(9_999);
    let mut bad = vec![];
    for gain_db in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 12.5, -145.0] {
        bad.push(GroupCmd::SetVcaLevel { vca: v, gain_db });
    }
    bad.extend([
        GroupCmd::RenameVca {
            vca: nowhere,
            name: "N".into(),
        },
        GroupCmd::SetVcaColour {
            vca: nowhere,
            colour: Swatch::Red,
        },
        GroupCmd::AssignVca {
            vca: nowhere,
            strips: vec![b],
        },
        GroupCmd::UnassignVca {
            vca: nowhere,
            strips: vec![a],
        },
        GroupCmd::SetVcaLevel {
            vca: nowhere,
            gain_db: 0.0,
        },
        GroupCmd::SetVcaMute {
            vca: nowhere,
            mute: true,
        },
        GroupCmd::SetVcaSolo {
            vca: nowhere,
            solo: true,
        },
        GroupCmd::SetVcaMode {
            vca: nowhere,
            mode: AutomationMode::Off,
        },
        GroupCmd::DeleteVca { vca: nowhere },
        GroupCmd::NewVca {
            name: "M".into(),
            colour: Swatch::Red,
            members: vec![master],
        },
        GroupCmd::AssignVca {
            vca: v,
            strips: vec![master],
        },
        GroupCmd::NewVca {
            name: "M".into(),
            colour: Swatch::Red,
            members: many.clone(),
        },
        GroupCmd::AssignVca {
            vca: v,
            strips: many,
        },
        GroupCmd::NewVca {
            name: "M".into(),
            colour: Swatch::Red,
            members: vec![b, b],
        },
        GroupCmd::AssignVca {
            vca: v,
            strips: vec![b, b],
        },
        GroupCmd::UnassignVca {
            vca: v,
            strips: vec![a, a],
        },
        GroupCmd::NewVca {
            name: "M".into(),
            colour: Swatch::Red,
            members: vec![unknown],
        },
        GroupCmd::AssignVca {
            vca: v,
            strips: vec![unknown],
        },
        GroupCmd::UnassignVca {
            vca: v,
            strips: vec![unknown],
        },
        // A is in it already; B is not.
        GroupCmd::AssignVca {
            vca: v,
            strips: vec![a],
        },
        GroupCmd::UnassignVca {
            vca: v,
            strips: vec![b],
        },
        GroupCmd::AssignVca {
            vca: v,
            strips: vec![],
        },
        GroupCmd::UnassignVca {
            vca: v,
            strips: vec![],
        },
    ]);
    for name in [long.as_str(), "a\0b", "", "   \t "] {
        bad.push(GroupCmd::NewVca {
            name: name.into(),
            colour: Swatch::Red,
            members: vec![],
        });
        bad.push(GroupCmd::RenameVca {
            vca: v,
            name: name.into(),
        });
    }
    for c in bad {
        let before = d.clone();
        let r = vca(&d, c.clone());
        assert!(r.is_err(), "{c:?} was accepted");
        assert_eq!(d, before);
    }
}

#[test]
fn a_vca_holds_at_most_128() {
    let d = base();
    assert_eq!(
        vca(
            &d,
            GroupCmd::NewVca {
                name: "Big".into(),
                colour: Swatch::Red,
                members: (0..=MAX_MEMBERS as u64).map(StripId).collect(),
            }
        ),
        Err(ModelError::invalid("A VCA holds at most 128 strips."))
    );
}

/// `doc` saved, with `edit` applied to its JSON, loaded again.
fn load_edited(doc: &Document, edit: impl FnOnce(&mut Value)) -> core_model::Result<Document> {
    let mut v: Value = serde_json::from_str(&to_json(doc).unwrap()).unwrap();
    edit(&mut v);
    from_json(&v.to_string())
}

fn corrupt_naming(r: core_model::Result<Document>, what: &str) {
    match r {
        Err(ModelError::Corrupt { problems }) => {
            assert!(
                problems.iter().any(|p| p.contains(what)),
                "{problems:?} does not name {what}"
            );
        }
        other => panic!("not corrupt: {other:?}"),
    }
}

#[test]
fn vcas_save_and_load() {
    assert_eq!(VERSION, 12);
    let d = base();
    let a = id_of(&d, "A");
    let (d, v) = new_vca(&d, "Kit", vec![a]);
    let d = level(&d, v, -3.5);
    let (d, latch) = new_vca(&d, "Pads", vec![]);
    let d = vca(
        &d,
        GroupCmd::SetVcaMode {
            vca: latch,
            mode: AutomationMode::Latch,
        },
    )
    .unwrap();
    let text = to_json(&d).unwrap();
    assert!(text.contains("\"version\": 12"));
    let json: Value = serde_json::from_str(&text).unwrap();
    let vcas = json["document"]["mixer"]["vcas"].as_array().unwrap();
    assert_eq!(vcas.len(), 2);
    assert_eq!(vcas[0]["id"], v.0);
    assert_eq!(vcas[0]["name"], "Kit");
    assert_eq!(vcas[0]["gain_db"], -3.5);
    assert_eq!(vcas[0].get("automation"), None, "Read is not written");
    assert!(vcas[1].get("automation").is_some());
    assert_eq!(from_json(&text).unwrap(), d);

    let plain: Value = serde_json::from_str(&to_json(&base()).unwrap()).unwrap();
    assert_eq!(plain["document"]["mixer"].get("vcas"), None);

    assert!(validate(&d).is_empty());
    corrupt_naming(
        load_edited(&d, |j| {
            j["document"]["mixer"]["vcas"][0]["members"] = Value::from(vec![9_999u64]);
        }),
        &v.to_string(),
    );
    corrupt_naming(
        load_edited(&d, |j| {
            j["document"]["mixer"]["vcas"][0]["gain_db"] = Value::from(99.0);
        }),
        &v.to_string(),
    );
    corrupt_naming(
        load_edited(&d, |j| {
            j["document"]["mixer"]["vcas"][1]["automation"] = Value::from("write");
        }),
        &latch.to_string(),
    );
    corrupt_naming(
        load_edited(&d, |j| {
            j["document"]["counters"]["vca"] = Value::from(1u64);
        }),
        &v.to_string(),
    );
    let master = d.mixer().master();
    corrupt_naming(
        load_edited(&d, |j| {
            j["document"]["mixer"]["vcas"][0]["members"] = Value::from(vec![master.0]);
        }),
        &v.to_string(),
    );
}
