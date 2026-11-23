//! Fader groups in the model (plan groups-vcas Task 1).

use core_model::ModelError;
use core_model::command::{Command, GroupCmd, MixerCmd, RoutingCmd, apply};
use core_model::document::{DestKind, Document};
use core_model::file::{VERSION, from_json, migrate_v10_to_v11, to_json};
use core_model::groups::{
    FADER_FLOOR_DB, Link, MAX_MEMBERS, check_name, group_delta, group_gains, group_of, linked,
    vca_label,
};
use core_model::history::History;
use core_model::ids::{AnyId, GroupId, StripId};
use core_model::mixer::{GAIN_FLOOR_DB, GAIN_MAX_DB, Swatch};
use core_model::rules::validate;
use core_model::selection::ViewFilter;
use proptest::prelude::*;
use serde_json::Value;

fn run(d: &Document, c: Command) -> core_model::Result<Document> {
    apply(d, &c).map(|a| a.doc)
}

fn grp(d: &Document, c: GroupCmd) -> core_model::Result<Document> {
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
    assert!(validate(&d).is_empty(), "{:?}", validate(&d));
    d
}

fn set_gain(d: &Document, strip: StripId, gain_db: f64) -> Document {
    run(d, Command::Mixer(MixerCmd::SetGain { strip, gain_db })).unwrap()
}

/// `base()` with a group "Drums" of `members` (by name) at `gains`.
fn grouped(members: &[&str], gains: &[f64]) -> (Document, GroupId) {
    let mut d = base();
    let ids: Vec<StripId> = members.iter().map(|n| id_of(&d, n)).collect();
    for (s, g) in ids.iter().zip(gains) {
        d = set_gain(&d, *s, *g);
    }
    let a = apply(
        &d,
        &Command::Groups(GroupCmd::NewGroup {
            name: "Drums".into(),
            colour: Swatch::Blue,
            members: ids,
        }),
    )
    .unwrap();
    let [AnyId::Group(g)] = a.report.created[..] else {
        panic!("{:?}", a.report.created);
    };
    (a.doc, g)
}

#[test]
fn a_group_move_keeps_the_balance() {
    let (d, _) = grouped(&["A", "B", "C"], &[-6.0, -3.0, 0.0]);
    let [a, b, c] = ["A", "B", "C"].map(|n| id_of(&d, n));
    assert_eq!(
        group_gains(d.mixer(), b, 0.0),
        vec![(b, 0.0), (a, -3.0), (c, 3.0)]
    );
}

#[test]
fn a_group_move_stops_at_the_top() {
    let (d, _) = grouped(&["A", "B"], &[0.0, 10.0]);
    let [a, b] = ["A", "B"].map(|n| id_of(&d, n));
    assert_eq!(
        group_gains(d.mixer(), a, 6.0),
        vec![(a, 2.0), (b, GAIN_MAX_DB)]
    );
}

#[test]
fn a_group_move_stops_at_the_lowest_finite_step() {
    let (d, _) = grouped(&["A", "B"], &[-50.0, -20.0]);
    let [a, b] = ["A", "B"].map(|n| id_of(&d, n));
    assert_eq!(
        group_gains(d.mixer(), b, -40.0),
        vec![(b, -30.0), (a, FADER_FLOOR_DB)]
    );
}

#[test]
fn a_member_at_minus_infinity_stays_there() {
    let (d, _) = grouped(&["A", "B"], &[GAIN_FLOOR_DB, -6.0]);
    let b = id_of(&d, "B");
    assert_eq!(group_gains(d.mixer(), b, -3.0), vec![(b, -3.0)]);
    // Preflight F2: a member at −∞ does not block a downward move.
    assert_eq!(group_gains(d.mixer(), b, -9.0), vec![(b, -9.0)]);
    assert_eq!(group_delta(&[GAIN_FLOOR_DB, -6.0], -3.0), -3.0);
}

#[test]
fn a_leader_at_minus_infinity_moves_alone() {
    let (d, _) = grouped(&["A", "B"], &[GAIN_FLOOR_DB, -6.0]);
    let a = id_of(&d, "A");
    assert_eq!(group_gains(d.mixer(), a, -10.0), vec![(a, -10.0)]);
}

#[test]
fn ungrouped_and_lone_members_move_alone() {
    let d = base();
    let a = id_of(&d, "A");
    assert_eq!(group_gains(d.mixer(), a, -100.0), vec![(a, -100.0)]);
    let (d, _) = grouped(&["A"], &[-6.0]);
    assert_eq!(group_gains(d.mixer(), a, -100.0), vec![(a, -100.0)]);
}

#[test]
fn a_member_below_the_floor_blocks_downward_moves() {
    assert_eq!(group_delta(&[-80.0, -6.0], -5.0), 0.0);
    assert_eq!(group_delta(&[-80.0, -6.0], 3.0), 3.0);
}

#[test]
fn group_delta_is_zero_when_the_bounds_cross() {
    // Preflight F3: upper 12 − 1e9 is below lower 0.
    assert_eq!(group_delta(&[-80.0, 1e9], 5.0), 0.0);
    assert_eq!(group_delta(&[-80.0, 1e9], -5.0), 0.0);
    // A base above the top: no move fits, in either direction.
    assert_eq!(group_delta(&[20.0], 3.0), 0.0);
    assert_eq!(group_delta(&[20.0], -3.0), 0.0);
    assert_eq!(group_delta(&[], 3.0), 0.0);
    assert_eq!(group_delta(&[GAIN_FLOOR_DB, f64::NAN], 3.0), 0.0);
}

fn base_db() -> impl Strategy<Value = f64> {
    prop_oneof![
        4 => -1e9f64..=1e9,
        1 => Just(f64::NAN),
        1 => Just(f64::INFINITY),
        1 => Just(f64::NEG_INFINITY),
        1 => Just(GAIN_FLOOR_DB),
    ]
}

proptest! {
    #[test]
    fn group_delta_is_total(
        bases in proptest::collection::vec(base_db(), 0..8),
        wanted in prop_oneof![
            4 => any::<f64>(),
            1 => Just(f64::NAN),
            1 => Just(f64::INFINITY),
            1 => Just(f64::NEG_INFINITY),
        ],
    ) {
        let r = group_delta(&bases, wanted);
        prop_assert!(r.is_finite());
        if !wanted.is_finite() {
            prop_assert_eq!(r, 0.0);
        }
        let finite: Vec<f64> = bases
            .iter()
            .copied()
            .filter(|b| b.is_finite() && *b > GAIN_FLOOR_DB)
            .collect();
        if finite.is_empty() {
            prop_assert_eq!(r, 0.0);
        } else {
            let hi = GAIN_MAX_DB - finite.iter().copied().fold(f64::MIN, f64::max);
            let lo = (FADER_FLOOR_DB - finite.iter().copied().fold(f64::MAX, f64::min)).min(0.0);
            if lo > hi || hi < 0.0 {
                prop_assert_eq!(r, 0.0);
            } else {
                prop_assert!(lo <= r && r <= hi, "{} not in {}..={}", r, lo, hi);
            }
        }
        // Never opposite to, or past, what was wanted.
        prop_assert!(r == 0.0 || (r.signum() == wanted.signum() && r.abs() <= wanted.abs()));
    }
}

#[test]
fn linked_follows_the_switches() {
    let (d, g) = grouped(&["A", "B", "C"], &[0.0, 0.0, 0.0]);
    let [a, b, c] = ["A", "B", "C"].map(|n| id_of(&d, n));
    assert_eq!(linked(d.mixer(), a, Link::Mute), vec![a, b, c]);
    assert_eq!(linked(d.mixer(), b, Link::Solo), vec![b, a, c]);
    let d = grp(
        &d,
        GroupCmd::SetGroupLinks {
            group: g,
            mute: Some(false),
            solo: None,
        },
    )
    .unwrap();
    assert_eq!(linked(d.mixer(), a, Link::Mute), vec![a]);
    assert_eq!(linked(d.mixer(), a, Link::Solo), vec![a, b, c]);
    let bus = id_of(&d, "Bus");
    assert_eq!(linked(d.mixer(), bus, Link::Solo), vec![bus]);
}

#[test]
fn new_group_defaults_and_names() {
    let (d, g) = grouped(&["A", "B"], &[0.0, 0.0]);
    let grp = d.mixer().group(g).unwrap();
    assert_eq!(grp.name, "Drums");
    assert!(grp.link_mute && grp.link_solo);
    assert_eq!(group_of(d.mixer(), id_of(&d, "A")).map(|x| x.id), Some(g));
    assert_eq!(group_of(d.mixer(), id_of(&d, "C")), None);
    let names = [
        GroupCmd::NewGroup {
            name: String::new(),
            colour: Swatch::Red,
            members: vec![],
        },
        GroupCmd::RenameGroup {
            group: g,
            name: String::new(),
        },
        GroupCmd::SetGroupColour {
            group: g,
            colour: Swatch::Red,
        },
        GroupCmd::AddToGroup {
            group: g,
            strips: vec![],
        },
        GroupCmd::RemoveFromGroup {
            group: g,
            strips: vec![],
        },
        GroupCmd::SetGroupLinks {
            group: g,
            mute: None,
            solo: None,
        },
        GroupCmd::DeleteGroup { group: g },
    ]
    .map(|c| Command::Groups(c).name());
    assert_eq!(
        names,
        [
            "New Group",
            "Rename Group",
            "Group Colour",
            "Add to Group",
            "Remove from Group",
            "Group Links",
            "Delete Group"
        ]
    );
}

#[test]
fn the_first_group_is_id_1() {
    let d = base();
    let a = apply(
        &d,
        &Command::Groups(GroupCmd::NewGroup {
            name: "G".into(),
            colour: Swatch::Red,
            members: vec![],
        }),
    )
    .unwrap();
    assert_eq!(a.report.created, vec![AnyId::Group(GroupId(1))]);
    // A version-10 file has no group counter: its first group is 1 too.
    let mut v: Value = serde_json::from_str(&to_json(&d).unwrap()).unwrap();
    v["version"] = Value::from(10);
    let counters = v["document"]["counters"].as_object_mut().unwrap();
    assert!(counters.remove("group").is_some());
    assert!(counters.remove("vca").is_some());
    let old = from_json(&v.to_string()).unwrap();
    let a = apply(
        &old,
        &Command::Groups(GroupCmd::NewGroup {
            name: "G".into(),
            colour: Swatch::Red,
            members: vec![],
        }),
    )
    .unwrap();
    assert_eq!(a.report.created, vec![AnyId::Group(GroupId(1))]);
    let mut c = core_model::ids::Counters::default();
    assert_eq!(c.alloc_vca().0, 1);
    c.group = 0;
    assert_eq!(c.alloc_group(), GroupId(1));
    assert_eq!(c.alloc_group(), GroupId(2));
}

#[test]
fn a_strip_joins_one_group_only() {
    let (d, _) = grouped(&["A", "B"], &[0.0, 0.0]);
    let d = grp(
        &d,
        GroupCmd::NewGroup {
            name: "Keys".into(),
            colour: Swatch::Green,
            members: vec![id_of(&d, "C")],
        },
    )
    .unwrap();
    let keys = group_of(d.mixer(), id_of(&d, "C")).unwrap().id;
    let e = grp(
        &d,
        GroupCmd::AddToGroup {
            group: keys,
            strips: vec![id_of(&d, "A")],
        },
    );
    assert_eq!(
        e,
        Err(ModelError::invalid(
            "Strip A is in group Drums; remove it from that group first."
        ))
    );
    let e = grp(
        &d,
        GroupCmd::NewGroup {
            name: "More".into(),
            colour: Swatch::Green,
            members: vec![id_of(&d, "B")],
        },
    );
    assert!(e.is_err());
    // Adding a strip that is free works.
    let bus = id_of(&d, "Bus");
    let d2 = grp(
        &d,
        GroupCmd::AddToGroup {
            group: keys,
            strips: vec![bus],
        },
    )
    .unwrap();
    assert_eq!(
        d2.mixer()
            .group(keys)
            .unwrap()
            .members
            .iter()
            .copied()
            .collect::<Vec<_>>(),
        vec![id_of(&d, "C"), bus]
    );
}

#[test]
fn master_cannot_join() {
    let (d, g) = grouped(&["A"], &[0.0]);
    let master = d.mixer().master();
    let want = Err(ModelError::invalid("Master cannot join a group."));
    assert_eq!(
        grp(
            &d,
            GroupCmd::AddToGroup {
                group: g,
                strips: vec![master],
            },
        ),
        want
    );
    assert_eq!(
        grp(
            &d,
            GroupCmd::NewGroup {
                name: "M".into(),
                colour: Swatch::Red,
                members: vec![master],
            },
        ),
        want
    );
}

#[test]
fn removing_a_strip_removes_its_membership() {
    let (d, g) = grouped(&["A", "Bus"], &[0.0, 0.0]);
    let bus = id_of(&d, "Bus");
    let mut h = History::new(d.clone());
    h.execute(
        &Command::Mixer(MixerCmd::RemoveBus { strip: bus }),
        &ViewFilter::default(),
    )
    .unwrap();
    let members: Vec<StripId> = h
        .doc()
        .mixer()
        .group(g)
        .unwrap()
        .members
        .iter()
        .copied()
        .collect();
    assert_eq!(members, vec![id_of(&d, "A")]);
    assert!(validate(h.doc()).is_empty(), "{:?}", validate(h.doc()));
    h.undo().unwrap();
    assert_eq!(h.doc(), &d);
    assert!(h.doc().mixer().strip(bus).is_ok());
    assert!(h.doc().mixer().group(g).unwrap().members.contains(&bus));
}

#[test]
fn empty_groups_stay() {
    let (d, g) = grouped(&["A", "B"], &[0.0, 0.0]);
    let [a, b] = ["A", "B"].map(|n| id_of(&d, n));
    let d = grp(
        &d,
        GroupCmd::RemoveFromGroup {
            group: g,
            strips: vec![a, b],
        },
    )
    .unwrap();
    assert!(d.mixer().group(g).unwrap().members.is_empty());
    assert!(validate(&d).is_empty());
    let d = grp(&d, GroupCmd::DeleteGroup { group: g }).unwrap();
    assert!(d.mixer().groups().is_empty());
    assert_eq!(d.mixer().strip(a).unwrap().gain_db, 0.0);
}

#[test]
fn hostile_group_commands_change_nothing() {
    let (d, g) = grouped(&["A"], &[0.0]);
    let [b, c] = ["B", "C"].map(|n| id_of(&d, n));
    let nowhere = GroupId(u64::MAX);
    let long = "x".repeat(70_000);
    let many: Vec<StripId> = (0..=MAX_MEMBERS as u64).map(StripId).collect();
    let unknown = StripId(9_999);
    let mut bad = vec![
        GroupCmd::RenameGroup {
            group: nowhere,
            name: "N".into(),
        },
        GroupCmd::SetGroupColour {
            group: nowhere,
            colour: Swatch::Red,
        },
        GroupCmd::AddToGroup {
            group: nowhere,
            strips: vec![b],
        },
        GroupCmd::RemoveFromGroup {
            group: nowhere,
            strips: vec![b],
        },
        GroupCmd::SetGroupLinks {
            group: nowhere,
            mute: Some(false),
            solo: None,
        },
        GroupCmd::DeleteGroup { group: nowhere },
        GroupCmd::NewGroup {
            name: "G".into(),
            colour: Swatch::Red,
            members: many.clone(),
        },
        GroupCmd::AddToGroup {
            group: g,
            strips: many,
        },
        GroupCmd::NewGroup {
            name: "G".into(),
            colour: Swatch::Red,
            members: vec![b, c, b],
        },
        GroupCmd::AddToGroup {
            group: g,
            strips: vec![b, b],
        },
        GroupCmd::RemoveFromGroup {
            group: g,
            strips: vec![id_of(&d, "A"), id_of(&d, "A")],
        },
        GroupCmd::NewGroup {
            name: "G".into(),
            colour: Swatch::Red,
            members: vec![unknown],
        },
        GroupCmd::AddToGroup {
            group: g,
            strips: vec![unknown],
        },
        GroupCmd::RemoveFromGroup {
            group: g,
            strips: vec![unknown],
        },
        // B is not in the group.
        GroupCmd::RemoveFromGroup {
            group: g,
            strips: vec![b],
        },
        GroupCmd::AddToGroup {
            group: g,
            strips: vec![],
        },
        GroupCmd::SetGroupLinks {
            group: g,
            mute: None,
            solo: None,
        },
    ];
    for name in [long.as_str(), "a\0b", "", "   \t "] {
        bad.push(GroupCmd::NewGroup {
            name: name.into(),
            colour: Swatch::Red,
            members: vec![],
        });
        bad.push(GroupCmd::RenameGroup {
            group: g,
            name: name.into(),
        });
    }
    for c in bad {
        let before = d.clone();
        let r = grp(&d, c.clone());
        assert!(r.is_err(), "{c:?} was accepted");
        assert_eq!(d, before);
    }
}

#[test]
fn names_are_checked_and_trimmed() {
    assert_eq!(check_name("A group's name", "  Drums ").unwrap(), "Drums");
    assert_eq!(check_name("x", &"é".repeat(64)).unwrap(), "é".repeat(64));
    assert_eq!(
        check_name("A group's name", &"é".repeat(65)),
        Err(ModelError::invalid("A group's name is 1 to 64 characters."))
    );
    assert_eq!(
        check_name("A group's name", "a\u{7}b"),
        Err(ModelError::invalid(
            "A group's name cannot hold control characters."
        ))
    );
    let (d, g) = grouped(&["A"], &[0.0]);
    let d = grp(
        &d,
        GroupCmd::RenameGroup {
            group: g,
            name: " Kit ".into(),
        },
    )
    .unwrap();
    assert_eq!(d.mixer().group(g).unwrap().name, "Kit");
}

#[test]
fn groups_save_and_load() {
    assert_eq!(VERSION, 12);
    let (d, g) = grouped(&["A", "B"], &[0.0, 0.0]);
    let text = to_json(&d).unwrap();
    assert!(text.contains("\"version\": 12"));
    let v: Value = serde_json::from_str(&text).unwrap();
    let groups = v["document"]["mixer"]["groups"].as_array().unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0]["id"], g.0);
    assert_eq!(groups[0]["name"], "Drums");
    assert_eq!(from_json(&text).unwrap(), d);

    let plain: Value = serde_json::from_str(&to_json(&base()).unwrap()).unwrap();
    assert_eq!(plain["document"]["mixer"].get("groups"), None);

    assert_eq!(migrate_v10_to_v11(v.clone()).unwrap(), v);

    let mut newer = v;
    newer["version"] = Value::from(13);
    assert_eq!(
        from_json(&newer.to_string()),
        Err(ModelError::NewerFormat { found: 13 })
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
fn validate_finds_broken_groups() {
    let (d, g) = grouped(&["A"], &[0.0]);
    let d = grp(
        &d,
        GroupCmd::NewGroup {
            name: "Keys".into(),
            colour: Swatch::Green,
            members: vec![id_of(&d, "B")],
        },
    )
    .unwrap();
    let keys = group_of(d.mixer(), id_of(&d, "B")).unwrap().id;
    assert!(validate(&d).is_empty());
    let a = id_of(&d, "A");
    let master = d.mixer().master();

    corrupt_naming(
        load_edited(&d, |v| {
            v["document"]["mixer"]["groups"][0]["members"] = Value::from(vec![9_999u64]);
        }),
        &g.to_string(),
    );
    corrupt_naming(
        load_edited(&d, |v| {
            v["document"]["mixer"]["groups"][1]["members"] = Value::from(vec![a.0]);
        }),
        &keys.to_string(),
    );
    corrupt_naming(
        load_edited(&d, |v| {
            v["document"]["mixer"]["groups"][0]["members"] = Value::from(vec![master.0]);
        }),
        &g.to_string(),
    );
    // A group ID the counter never handed out.
    corrupt_naming(
        load_edited(&d, |v| {
            v["document"]["counters"]["group"] = Value::from(1u64);
        }),
        &g.to_string(),
    );
}

/// Owner, 2026-11-19: what Co-Producer makes carries its mark, saved and
/// loaded; a file without the mark is the composer's.
#[test]
fn the_co_producer_mark_saves_and_loads() {
    let d = base();
    let mut h = History::new(d.clone());
    h.begin_request("group them").unwrap();
    h.set_acting(true).unwrap();
    for c in [
        GroupCmd::NewGroup {
            name: "Ours".into(),
            colour: Swatch::Red,
            members: vec![],
        },
        GroupCmd::NewVca {
            name: "Ride".into(),
            colour: Swatch::Teal,
            members: vec![],
        },
    ] {
        h.execute(&Command::Groups(c), &ViewFilter::default())
            .unwrap();
    }
    let ours = h.doc().clone();
    let g = ours.mixer().groups().values().next().unwrap();
    let v = ours.mixer().vcas().values().next().unwrap();
    assert!(g.co_producer && v.co_producer);
    // Not renamed: a group or VCA name has no " (Co-Producer)".
    assert_eq!((g.name.as_str(), v.name.as_str()), ("Ours", "Ride"));
    let text = to_json(&ours).unwrap();
    let back = from_json(&text).unwrap();
    assert_eq!(back, ours);
    assert!(back.mixer().groups().values().all(|g| g.co_producer));
    assert!(back.mixer().vcas().values().all(|v| v.co_producer));
    assert!(back.may_have_marks());

    // By hand: no mark, and none written.
    let hand = grp(
        &d,
        GroupCmd::NewGroup {
            name: "Mine".into(),
            colour: Swatch::Red,
            members: vec![],
        },
    )
    .unwrap();
    let hand = run(
        &hand,
        Command::Groups(GroupCmd::NewVca {
            name: "Kit".into(),
            colour: Swatch::Teal,
            members: vec![],
        }),
    )
    .unwrap();
    assert!(!hand.mixer().groups().values().next().unwrap().co_producer);
    assert!(!hand.mixer().vcas().values().next().unwrap().co_producer);
    let v: Value = serde_json::from_str(&to_json(&hand).unwrap()).unwrap();
    assert_eq!(v["document"]["mixer"]["groups"][0].get("co_producer"), None);
    assert_eq!(v["document"]["mixer"]["vcas"][0].get("co_producer"), None);

    // A file without the field loads as the composer's.
    let mut v: Value = serde_json::from_str(&text).unwrap();
    for list in ["groups", "vcas"] {
        for x in v["document"]["mixer"][list].as_array_mut().unwrap() {
            x.as_object_mut().unwrap().remove("co_producer");
        }
    }
    let plain = from_json(&v.to_string()).unwrap();
    assert!(!plain.mixer().groups().values().next().unwrap().co_producer);
    assert!(!plain.mixer().vcas().values().next().unwrap().co_producer);
}

#[test]
fn a_vca_label_never_says_vca_twice() {
    assert_eq!(vca_label("VCA 1"), "VCA 1");
    assert_eq!(vca_label("VCA"), "VCA");
    assert_eq!(vca_label("Strings"), "VCA Strings");
    assert_eq!(vca_label("VCAs and more"), "VCA VCAs and more");
}
