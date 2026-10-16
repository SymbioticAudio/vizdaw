use core_model::ModelError;
use core_model::document::{Document, DocumentRepr, Settings};
use core_model::ids::{Counters, PluginInstanceId, StripId};
use core_model::mixer::{
    AuxSend, Mixer, MixerRepr, Output, PluginRef, PluginSlot, Strip, StripKind,
};
use core_model::mixer_rules::{
    EdgeKind, audible, check_routing, find_routing_cycle, routing_edges,
};
use core_model::rules::validate;
use proptest::prelude::*;
use std::collections::BTreeSet;

/// Master 0 plus buses 1..=n, built from (output, sends, sidechain
/// sources) per bus.
fn buses(spec: &[(Option<u64>, Vec<u64>, Vec<u64>)]) -> Mixer {
    let mut strips = vec![Strip::new(StripId(0), "Master", StripKind::Master)];
    let mut plug = 0;
    for (i, (out, sends, scs)) in spec.iter().enumerate() {
        let id = StripId(i as u64 + 1);
        let mut s = Strip::new(id, format!("B{}", id.0), StripKind::Bus);
        s.output = out.map_or(Output::Master, |b| Output::Bus(StripId(b)));
        for t in sends {
            s.sends.push_back(AuxSend::new(StripId(*t)));
        }
        for src in scs {
            let mut slot =
                PluginSlot::new(PluginInstanceId(plug), PluginRef::from_legacy("clap:duck"));
            plug += 1;
            slot.sidechain = Some(StripId(*src));
            s.inserts.push_back(slot);
        }
        strips.push(s);
    }
    Mixer::try_from(MixerRepr {
        master: StripId(0),
        order: (1..=spec.len() as u64).map(StripId).collect(),
        strips,
    })
    .unwrap()
}

fn doc_with(m: Mixer, strips: u64, plugins: u64) -> Document {
    Document::from_parts(DocumentRepr {
        settings: Settings::default(),
        instruments: vec![],
        outputs: vec![],
        media: vec![],
        sequences: vec![],
        mixer: Some(m),
        counters: Counters {
            strip: strips,
            plugin: plugins,
            ..Counters::default()
        },
    })
    .unwrap()
}

#[test]
fn edges_list_outputs_sends_and_sidechains() {
    let m = buses(&[(Some(2), vec![2], vec![]), (None, vec![], vec![1])]);
    assert_eq!(
        routing_edges(&m),
        vec![
            (StripId(1), StripId(2), EdgeKind::Output),
            (StripId(1), StripId(2), EdgeKind::Send(0)),
            (StripId(2), StripId(0), EdgeKind::Output),
            (
                StripId(1),
                StripId(2),
                EdgeKind::Sidechain(PluginInstanceId(0))
            ),
        ]
    );
    assert_eq!(find_routing_cycle(&m), None);
    assert_eq!(check_routing(&m), Ok(()));
}

#[test]
fn cycles_through_sends_and_sidechains_are_found() {
    // 1 → 2 (output), 2 → 1 (send).
    let m = buses(&[(Some(2), vec![], vec![]), (None, vec![1], vec![])]);
    let path = find_routing_cycle(&m).unwrap();
    assert_eq!(path.first(), path.last());
    assert!(matches!(
        check_routing(&m),
        Err(ModelError::RoutingCycle { .. })
    ));
    // A sidechain from itself.
    let m = buses(&[(None, vec![], vec![1])]);
    assert_eq!(find_routing_cycle(&m), Some(vec![StripId(1), StripId(1)]));
    // A sidechain from master: master → 1 → master.
    let m = buses(&[(None, vec![], vec![0])]);
    assert!(find_routing_cycle(&m).is_some());
}

#[test]
fn invalid_targets_are_named() {
    let m = buses(&[(Some(1), vec![], vec![])]);
    assert_eq!(
        check_routing(&m),
        Err(ModelError::InvalidRoutingTarget {
            strip: StripId(1),
            target: StripId(1),
            reason: "a strip cannot output to itself".into(),
        })
    );
    let m = buses(&[(None, vec![0], vec![])]);
    assert_eq!(
        check_routing(&m),
        Err(ModelError::InvalidRoutingTarget {
            strip: StripId(1),
            target: StripId(0),
            reason: "a send must target a bus".into(),
        })
    );
    let m = buses(&[(Some(9), vec![], vec![])]);
    assert!(matches!(check_routing(&m), Err(ModelError::UnknownId(_))));
}

#[test]
fn validate_reports_mixer_problems() {
    let m = buses(&[(Some(2), vec![], vec![]), (Some(1), vec![], vec![])]);
    let problems = validate(&doc_with(m, 3, 0));
    assert!(
        problems.iter().any(|p| p.contains("routing cycle")),
        "{problems:?}"
    );
    // Counters too low, a duplicate instance ID, a bad fader.
    let mut m = buses(&[(None, vec![], vec![1, 1])]);
    let mut r = MixerRepr::from(m.clone());
    r.strips[1].gain_db = 13.0;
    m = Mixer::try_from(r).unwrap();
    let problems = validate(&doc_with(m, 1, 0));
    assert!(
        problems.iter().any(|p| p.contains("counters.strip")),
        "{problems:?}"
    );
    assert!(
        problems.iter().any(|p| p.contains("plugin.id")),
        "{problems:?}"
    );
    assert!(
        problems.iter().any(|p| p.contains("gain_db")),
        "{problems:?}"
    );
    // A valid document has none.
    assert!(validate(&Document::new()).is_empty());
}

#[test]
fn validate_catches_bad_master_and_order() {
    let base = MixerRepr::from(buses(&[(None, vec![], vec![]), (None, vec![], vec![])]));
    // Master ID names no strip.
    let mut r = base.clone();
    r.master = StripId(9);
    let problems = validate(&doc_with(Mixer::try_from(r).unwrap(), 3, 0));
    assert!(
        problems
            .iter()
            .any(|p| p.contains("master strip is missing")),
        "{problems:?}"
    );
    // Master ID names a bus.
    let mut r = base.clone();
    r.master = StripId(1);
    let problems = validate(&doc_with(Mixer::try_from(r).unwrap(), 3, 0));
    assert!(
        problems
            .iter()
            .any(|p| p.contains("master strip is missing")),
        "{problems:?}"
    );
    // Two master strips.
    let mut r = base.clone();
    r.strips[2].kind = StripKind::Master;
    let problems = validate(&doc_with(Mixer::try_from(r).unwrap(), 3, 0));
    assert!(
        problems.iter().any(|p| p.contains("2 master strips")),
        "{problems:?}"
    );
    // Order omits a strip, repeats one, or lists an unknown one.
    for order in [
        vec![StripId(1)],
        vec![StripId(1), StripId(1)],
        vec![StripId(1), StripId(7)],
    ] {
        let mut r = base.clone();
        r.order = order.into_iter().collect();
        let problems = validate(&doc_with(Mixer::try_from(r).unwrap(), 3, 0));
        assert!(
            problems.iter().any(|p| p.contains("display order")),
            "{problems:?}"
        );
    }
}

#[test]
fn solo_keeps_upstream_and_downstream() {
    // 1 → 3 (output), 2 → master, 2 sends to 3; 3 → master.
    let mut r = MixerRepr::from(buses(&[
        (Some(3), vec![], vec![]),
        (None, vec![3], vec![]),
        (None, vec![], vec![]),
    ]));
    assert_eq!(audible(&Mixer::try_from(r.clone()).unwrap()).len(), 4);
    r.strips[1].solo = true;
    let set = audible(&Mixer::try_from(r.clone()).unwrap());
    assert_eq!(set, BTreeSet::from([StripId(0), StripId(1), StripId(3)]));
    r.strips[1].solo = false;
    r.strips[3].solo = true;
    let set = audible(&Mixer::try_from(r).unwrap());
    assert_eq!(
        set,
        BTreeSet::from([StripId(0), StripId(1), StripId(2), StripId(3)])
    );
}

/// Reachability by Floyd–Warshall over `edges` among `n` nodes.
fn reach(n: usize, edges: &[(usize, usize)]) -> Vec<Vec<bool>> {
    let mut r = vec![vec![false; n]; n];
    for &(a, b) in edges {
        r[a][b] = true;
    }
    for k in 0..n {
        for i in 0..n {
            for j in 0..n {
                if r[i][k] && r[k][j] {
                    r[i][j] = true;
                }
            }
        }
    }
    r
}

/// Per bus: output, sends, sidechain sources, soloed.
type BusSpec = (Option<u64>, Vec<u64>, Vec<u64>, bool);

fn arb_buses() -> impl Strategy<Value = Vec<BusSpec>> {
    (1usize..7).prop_flat_map(|n| {
        let id = 0..=(n as u64);
        proptest::collection::vec(
            (
                proptest::option::of(1..=(n as u64)),
                proptest::collection::vec(1..=(n as u64), 0..3),
                proptest::collection::vec(id.clone(), 0..2),
                any::<bool>(),
            ),
            n,
        )
    })
}

proptest! {
    #[test]
    fn cycle_check_matches_reference(spec in arb_buses()) {
        let plain: Vec<_> = spec.iter().map(|(o, s, c, _)| (*o, s.clone(), c.clone())).collect();
        let m = buses(&plain);
        let n = spec.len() + 1;
        let edges: Vec<(usize, usize)> = routing_edges(&m)
            .into_iter()
            .map(|(a, b, _)| (a.0 as usize, b.0 as usize))
            .collect();
        let r = reach(n, &edges);
        let cyclic = (0..n).any(|i| r[i][i]);
        match find_routing_cycle(&m) {
            None => prop_assert!(!cyclic),
            Some(path) => {
                prop_assert!(cyclic);
                prop_assert_eq!(path.first(), path.last());
                for w in path.windows(2) {
                    prop_assert!(edges.contains(&(w[0].0 as usize, w[1].0 as usize)));
                }
            }
        }
    }

    #[test]
    fn solo_set_matches_reference(spec in arb_buses()) {
        let plain: Vec<_> = spec.iter().map(|(o, s, c, _)| (*o, s.clone(), c.clone())).collect();
        let mut r = MixerRepr::from(buses(&plain));
        for (i, (_, _, _, solo)) in spec.iter().enumerate() {
            r.strips[i + 1].solo = *solo;
        }
        let m = Mixer::try_from(r).unwrap();
        let n = spec.len() + 1;
        let edges: Vec<(usize, usize)> = routing_edges(&m)
            .into_iter()
            .filter(|(_, _, k)| !matches!(k, EdgeKind::Sidechain(_)))
            .map(|(a, b, _)| (a.0 as usize, b.0 as usize))
            .collect();
        let reach = reach(n, &edges);
        let soloed: Vec<usize> = (1..n).filter(|i| spec[i - 1].3).collect();
        let want: BTreeSet<StripId> = (0..n)
            .filter(|&s| {
                soloed.is_empty()
                    || soloed.iter().any(|&t| t == s || reach[t][s] || reach[s][t])
            })
            .map(|s| StripId(s as u64))
            .collect();
        prop_assert_eq!(audible(&m), want);
    }
}
