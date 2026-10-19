//! Mixer rules (spec §4): routing is a DAG over outputs, sends and
//! sidechain feeds; targets are valid; every plugin instrument and output
//! path has exactly one strip; values are in range. Also the solo set.

use crate::document::{DestKind, Document};
use crate::error::{ModelError, Result};
use crate::ids::{AnyId, PluginInstanceId, StripId};
use crate::mixer::{GAIN_FLOOR_DB, GAIN_MAX_DB, Mixer, Output, Strip, StripKind};
use std::collections::{BTreeMap, BTreeSet};

/// What a routing edge is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EdgeKind {
    /// A strip's main output.
    Output,
    /// Send number `n` of the source strip.
    Send(usize),
    /// The source feeds this insert's sidechain input.
    Sidechain(PluginInstanceId),
}

/// Every routing edge `(from, to, kind)`: each non-master strip's output
/// (to master or a bus) and sends, in strip order, then every sidechain
/// feed (source → strip owning the insert).
pub fn routing_edges(m: &Mixer) -> Vec<(StripId, StripId, EdgeKind)> {
    let mut e = Vec::new();
    for s in m.strips().values() {
        if s.id != m.master() {
            let to = match s.output {
                Output::Master => m.master(),
                Output::Bus(b) => b,
            };
            e.push((s.id, to, EdgeKind::Output));
        }
        for (i, x) in s.sends.iter().enumerate() {
            e.push((s.id, x.target, EdgeKind::Send(i)));
        }
    }
    for s in m.strips().values() {
        for slot in s.inserts.iter() {
            if let Some(src) = slot.sidechain {
                e.push((src, s.id, EdgeKind::Sidechain(slot.id)));
            }
        }
    }
    e
}

/// A routing cycle, if any: the path's first strip is repeated at the end.
pub fn find_routing_cycle(m: &Mixer) -> Option<Vec<StripId>> {
    let mut adj: BTreeMap<StripId, Vec<StripId>> = BTreeMap::new();
    for (a, b, _) in routing_edges(m) {
        adj.entry(a).or_default().push(b);
    }
    #[derive(Clone, Copy, PartialEq)]
    enum Color {
        White,
        Grey,
        Black,
    }
    let mut color: BTreeMap<StripId, Color> =
        m.strips().keys().map(|k| (*k, Color::White)).collect();
    for &start in m.strips().keys() {
        if color.get(&start) != Some(&Color::White) {
            continue;
        }
        color.insert(start, Color::Grey);
        let mut stack: Vec<(StripId, usize)> = vec![(start, 0)];
        while let Some((node, i)) = stack.last().copied() {
            match adj.get(&node).and_then(|v| v.get(i)).copied() {
                None => {
                    color.insert(node, Color::Black);
                    stack.pop();
                }
                Some(next) => {
                    if let Some(top) = stack.last_mut() {
                        top.1 += 1;
                    }
                    match color.get(&next).copied() {
                        Some(Color::Grey) => {
                            let pos = stack.iter().position(|(s, _)| *s == next).unwrap_or(0);
                            let mut path: Vec<StripId> = stack
                                .get(pos..)
                                .unwrap_or(&[])
                                .iter()
                                .map(|(s, _)| *s)
                                .collect();
                            path.push(next);
                            return Some(path);
                        }
                        Some(Color::White) => {
                            color.insert(next, Color::Grey);
                            stack.push((next, 0));
                        }
                        // Finished, or a strip that does not exist
                        // (reported by `check_routing`).
                        _ => {}
                    }
                }
            }
        }
    }
    None
}

fn invalid(strip: StripId, target: StripId, reason: &str) -> ModelError {
    ModelError::InvalidRoutingTarget {
        strip,
        target,
        reason: reason.to_string(),
    }
}

fn check_bus(m: &Mixer, from: StripId, to: StripId, what: &str) -> Result<()> {
    if from == to {
        return Err(invalid(
            from,
            to,
            &format!("a strip cannot {what} to itself"),
        ));
    }
    if m.strip(to)?.kind != StripKind::Bus {
        let article = if what == "send" {
            "a send"
        } else {
            "an output"
        };
        return Err(invalid(from, to, &format!("{article} must target a bus")));
    }
    Ok(())
}

/// The first routing problem: an invalid output or send target, an unknown
/// sidechain source, or a cycle.
pub fn check_routing(m: &Mixer) -> Result<()> {
    for s in m.strips().values() {
        if s.id == m.master() {
            if s.output != Output::Master {
                return Err(invalid(s.id, s.id, "master has no output"));
            }
            if let Some(x) = s.sends.front() {
                return Err(invalid(s.id, x.target, "master has no sends"));
            }
        }
        if let Output::Bus(b) = s.output {
            check_bus(m, s.id, b, "output")?;
        }
        for x in s.sends.iter() {
            check_bus(m, s.id, x.target, "send")?;
        }
        for slot in s.inserts.iter() {
            if let Some(src) = slot.sidechain {
                m.strip(src)?;
            }
        }
    }
    if let Some(path) = find_routing_cycle(m) {
        return Err(ModelError::RoutingCycle { path });
    }
    Ok(())
}

fn level(field: &'static str, db: f64) -> Result<()> {
    if db.is_finite() && (GAIN_FLOOR_DB..=GAIN_MAX_DB).contains(&db) {
        Ok(())
    } else {
        Err(ModelError::range(field, db))
    }
}

/// Fader, pan, send levels and overlay values in range.
pub fn check_strip_values(s: &Strip) -> Result<()> {
    level("gain_db", s.gain_db)?;
    if !(s.pan.is_finite() && (-1.0..=1.0).contains(&s.pan)) {
        return Err(ModelError::range("pan", s.pan));
    }
    for x in s.sends.iter() {
        level("send.level_db", x.level_db)?;
    }
    for slot in s.inserts.iter() {
        if slot.params.values().any(|v| !v.is_finite()) {
            return Err(ModelError::range("params", "non-finite value"));
        }
    }
    Ok(())
}

/// Every mixer problem of `doc`, for [`crate::rules::validate`].
pub fn mixer_problems(doc: &Document) -> Vec<String> {
    let m = doc.mixer();
    let c = doc.counters();
    let mut p: Vec<String> = Vec::new();
    let mut push = |ctx: &str, e: ModelError| p.push(format!("{ctx}: {e}"));
    if m.strips().get(&m.master()).map(|s| s.kind) != Some(StripKind::Master) {
        push("mixer", ModelError::invalid("the master strip is missing"));
    }
    let masters = m
        .strips()
        .values()
        .filter(|s| s.kind == StripKind::Master)
        .count();
    if masters != 1 {
        push(
            "mixer",
            ModelError::invalid(format!("{masters} master strips")),
        );
    }
    let mut order: Vec<StripId> = m.order().iter().copied().collect();
    order.sort();
    let others: Vec<StripId> = m
        .strips()
        .keys()
        .copied()
        .filter(|k| *k != m.master())
        .collect();
    if order != others {
        push(
            "mixer",
            ModelError::invalid("display order does not list every strip exactly once"),
        );
    }
    let mut kinds: BTreeMap<StripKind, StripId> = BTreeMap::new();
    let mut instances: BTreeSet<PluginInstanceId> = BTreeSet::new();
    for s in m.strips().values() {
        let ctx = s.id.to_string();
        if s.id.0 >= c.strip {
            push(&ctx, ModelError::range("counters.strip", s.id));
        }
        if let Err(e) = check_strip_values(s) {
            push(&ctx, e);
        }
        match s.kind {
            StripKind::Instrument(dest) | StripKind::InstrumentOutput(dest, _) => {
                let plugin = matches!(
                    doc.instruments().get(&dest).map(|d| &d.kind),
                    Some(DestKind::PluginInstrument { .. })
                );
                if !plugin {
                    push(
                        &ctx,
                        ModelError::invalid(format!("{dest} is not a plugin instrument")),
                    );
                }
                if s.kind == StripKind::InstrumentOutput(dest, 0) {
                    push(&ctx, ModelError::range("instrument_output", 0));
                }
            }
            StripKind::AudioPath(o) => {
                if !doc.outputs().contains_key(&o) {
                    push(&ctx, ModelError::UnknownId(AnyId::Output(o)));
                }
            }
            StripKind::BuiltinSynth | StripKind::Bus | StripKind::Master => {}
        }
        if !matches!(s.kind, StripKind::Bus | StripKind::Master) {
            if let Some(prev) = kinds.insert(s.kind, s.id) {
                push(
                    &ctx,
                    ModelError::invalid(format!("duplicate {:?} strip (also {prev})", s.kind)),
                );
            }
        }
        for slot in s.inserts.iter() {
            if !instances.insert(slot.id) || slot.id.0 >= c.plugin {
                push(&ctx, ModelError::range("plugin.id", slot.id));
            }
        }
    }
    for d in doc.instruments().values() {
        let ctx = d.id.to_string();
        if let DestKind::PluginInstrument { slot } = &d.kind {
            if !kinds.contains_key(&StripKind::Instrument(d.id)) {
                push(
                    &ctx,
                    ModelError::invalid("plugin instrument has no instrument strip"),
                );
            }
            if !instances.insert(slot.id) || slot.id.0 >= c.plugin {
                push(&ctx, ModelError::range("plugin.id", slot.id));
            }
            if slot.sidechain.is_some() {
                push(
                    &ctx,
                    ModelError::invalid("an instrument's own slot has a sidechain"),
                );
            }
            if slot.params.values().any(|v| !v.is_finite()) {
                push(&ctx, ModelError::range("params", "non-finite value"));
            }
        }
    }
    if doc
        .instruments()
        .values()
        .any(|d| d.kind.is_builtin_synth())
        && !kinds.contains_key(&StripKind::BuiltinSynth)
    {
        push("mixer", ModelError::invalid("no built-in synth strip"));
    }
    for o in doc.outputs().keys() {
        if !kinds.contains_key(&StripKind::AudioPath(*o)) {
            push(
                &o.to_string(),
                ModelError::invalid("output path has no strip"),
            );
        }
    }
    if let Err(e) = check_routing(m) {
        push("mixer", e);
    }
    p
}

/// Per strip, the strips audible while it is soloed (Plan Ruling 5):
/// itself and every strip downstream or upstream of it through outputs
/// and sends (sidechains excluded). Shared by [`audible`] and the render
/// graph's automatable solos (Plan 4d Ruling 11).
pub fn solo_reach(m: &Mixer) -> BTreeMap<StripId, BTreeSet<StripId>> {
    let mut down: BTreeMap<StripId, Vec<StripId>> = BTreeMap::new();
    let mut up: BTreeMap<StripId, Vec<StripId>> = BTreeMap::new();
    for (a, b, k) in routing_edges(m) {
        if !matches!(k, EdgeKind::Sidechain(_)) {
            down.entry(a).or_default().push(b);
            up.entry(b).or_default().push(a);
        }
    }
    m.strips()
        .keys()
        .map(|&s| {
            let mut set = BTreeSet::from([s]);
            for adj in [&down, &up] {
                let mut stack = vec![s];
                let mut seen: BTreeSet<StripId> = BTreeSet::new();
                while let Some(n) = stack.pop() {
                    for x in adj.get(&n).into_iter().flatten() {
                        if seen.insert(*x) {
                            set.insert(*x);
                            stack.push(*x);
                        }
                    }
                }
            }
            (s, set)
        })
        .collect()
}

/// Strips that sound under the current solo state (Plan Ruling 5):
/// every strip when nothing is soloed; otherwise the soloed strips and
/// every strip downstream or upstream of one through outputs and sends.
pub fn audible(m: &Mixer) -> BTreeSet<StripId> {
    if !m.strips().values().any(|s| s.solo) {
        return m.strips().keys().copied().collect();
    }
    let reach = solo_reach(m);
    m.strips()
        .values()
        .filter(|s| s.solo)
        .filter_map(|s| reach.get(&s.id))
        .flatten()
        .copied()
        .collect()
}
