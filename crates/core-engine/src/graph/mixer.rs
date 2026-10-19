//! The mixer → graph translation (spec §6; Plan Rulings 1, 4, 5, 25).

use crate::graph::controls::{FeedSpec, SendSpec, StripControlSpec};
use crate::graph::dsp::strip_gains;
use crate::graph::spec::{GraphSpec, InputSpec, InsertSpec, NodeSpec, StripSpec, Tap};
use crate::schedule::TargetIdx;
use core_model::document::Document;
use core_model::ids::{DestId, OutputPathId, StripId};
use core_model::mixer::{Output, PluginSlot, Strip, StripKind, db_to_gain};
use core_model::mixer_rules::{audible, solo_reach};
use std::collections::BTreeMap;

/// A graph spec made from a document's mixer.
#[derive(Debug, Clone, PartialEq)]
pub struct MixerGraph {
    /// The spec.
    pub spec: GraphSpec,
    /// `(node index, strip)` of every strip node (meters).
    pub strips: Vec<(usize, StripId)>,
    /// Problems worth showing (not errors).
    pub warnings: Vec<String>,
    /// Every strip's automatable controls (4d, `Graph::set_controls`).
    pub controls: Vec<StripControlSpec>,
}

/// Which strip an instrument node feeds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedSource {
    /// The built-in synth: its `BuiltinSynth` strip.
    Builtin,
    /// A plugin instrument destination: its `Instrument` strip.
    Plugin(DestId),
}

/// An instrument node of the graph: a target with an instrument sink.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstrumentFeed {
    /// The engine target whose sink renders.
    pub target: TargetIdx,
    /// The sink's latency in frames.
    pub latency: u32,
    /// The sink's stereo outputs (≥ 1).
    pub outputs: u16,
    /// The strip it feeds.
    pub source: FeedSource,
}

impl InstrumentFeed {
    /// A built-in synth feed.
    pub fn builtin(target: TargetIdx, latency: u32) -> InstrumentFeed {
        InstrumentFeed {
            target,
            latency,
            outputs: 1,
            source: FeedSource::Builtin,
        }
    }

    /// A plugin instrument feed for destination `dest` with `outputs`
    /// stereo outputs.
    pub fn plugin(target: TargetIdx, latency: u32, outputs: u16, dest: DestId) -> InstrumentFeed {
        InstrumentFeed {
            target,
            latency,
            outputs: outputs.max(1),
            source: FeedSource::Plugin(dest),
        }
    }
}

/// Builds the graph of `doc`'s mixer. `instruments` are the targets with an
/// instrument sink (instrument nodes `0..instruments.len()`, in this
/// order); `insert_latency` gives each insert's latency (0 for
/// placeholders).
pub fn mixer_spec(
    doc: &Document,
    instruments: &[InstrumentFeed],
    insert_latency: &mut dyn FnMut(&PluginSlot) -> u32,
) -> MixerGraph {
    let m = doc.mixer();
    let on = audible(m);
    let base = instruments.len();
    let index: BTreeMap<StripId, usize> = m
        .strips()
        .keys()
        .enumerate()
        .map(|(i, id)| (*id, base + i))
        .collect();
    let node = |id: StripId| index.get(&id).copied();
    let mut warnings = Vec::new();
    let mut strips: Vec<StripSpec> = m
        .strips()
        .values()
        .map(|s| StripSpec::new(Some(s.id)))
        .collect();
    let master = node(m.master()).unwrap_or(base);
    // One Regions node per AudioPath strip, after the strips (5a Ruling
    // 11), as that strip's first input (before any feed adds inputs).
    let mut region_paths: Vec<OutputPathId> = Vec::new();
    for (k, s) in m.strips().values().enumerate() {
        if let StripKind::AudioPath(path) = s.kind {
            let from = base + m.strips().len() + region_paths.len();
            region_paths.push(path);
            if let Some(spec) = strips.get_mut(k) {
                spec.inputs.push(InputSpec {
                    from,
                    tap: Tap::Post,
                    gain: 1.0,
                });
            }
        }
    }
    let builtin_strip = m.find_kind(StripKind::BuiltinSynth).and_then(node);
    let mut fed: Vec<usize> = Vec::new();
    for (i, f) in instruments.iter().enumerate() {
        let to = match f.source {
            FeedSource::Builtin => {
                if builtin_strip.is_none() {
                    warnings.push(
                        "no built-in synth strip: the built-in synth plays straight to master"
                            .to_string(),
                    );
                }
                builtin_strip.unwrap_or(master)
            }
            FeedSource::Plugin(d) => match m.find_kind(StripKind::Instrument(d)).and_then(node) {
                Some(n) => n,
                None => {
                    warnings.push(format!(
                        "instrument {d} has no strip: it plays straight to master"
                    ));
                    master
                }
            },
        };
        fed.push(to);
        if let Some(s) = to.checked_sub(base).and_then(|k| strips.get_mut(k)) {
            s.inputs.push(InputSpec {
                from: i,
                tap: Tap::Post,
                gain: 1.0,
            });
        }
        // Extra outputs feed their `InstrumentOutput` strips; a strip for an
        // output the instrument does not have stays silent (spec §4).
        if let FeedSource::Plugin(d) = f.source {
            for s in m.strips().values() {
                let StripKind::InstrumentOutput(x, k) = s.kind else {
                    continue;
                };
                if x != d {
                    continue;
                }
                if k >= f.outputs {
                    warnings.push(format!(
                        "strip {:?} is silent: its instrument has {} output(s)",
                        s.name, f.outputs
                    ));
                    continue;
                }
                if let Some(t) = node(s.id)
                    .and_then(|t| t.checked_sub(base))
                    .and_then(|k| strips.get_mut(k))
                {
                    t.inputs.push(InputSpec {
                        from: i,
                        tap: Tap::Out(k),
                        gain: 1.0,
                    });
                }
            }
        }
    }
    let muted = |s: &Strip| s.mute || !on.contains(&s.id);
    // What each strip feeds, for automation (Plan 4d Ruling 11).
    let mut feeds: BTreeMap<StripId, Vec<FeedSpec>> = BTreeMap::new();
    for s in m.strips().values() {
        let Some(from) = node(s.id) else {
            continue;
        };
        let pass = if muted(s) { 0.0 } else { 1.0 };
        if s.id != m.master() {
            let target = match s.output {
                Output::Master => m.master(),
                Output::Bus(b) => b,
            };
            if let Some(to) = node(target) {
                if let Some(t) = to.checked_sub(base).and_then(|k| strips.get_mut(k)) {
                    feeds.entry(s.id).or_default().push(FeedSpec {
                        to,
                        input: t.inputs.len(),
                        send: None,
                    });
                    t.inputs.push(InputSpec {
                        from,
                        tap: Tap::Post,
                        gain: pass,
                    });
                }
            }
        }
        for (i, x) in s.sends.iter().enumerate() {
            let level = if x.enabled {
                db_to_gain(x.level_db) as f32
            } else {
                0.0
            };
            if let Some(to) = node(x.target) {
                if let Some(t) = to.checked_sub(base).and_then(|k| strips.get_mut(k)) {
                    feeds.entry(s.id).or_default().push(FeedSpec {
                        to,
                        input: t.inputs.len(),
                        send: Some(SendSpec {
                            index: u16::try_from(i).unwrap_or(u16::MAX),
                            level_db: x.level_db,
                            enabled: x.enabled,
                        }),
                    });
                    t.inputs.push(InputSpec {
                        from,
                        tap: if x.pre_fader { Tap::Pre } else { Tap::Post },
                        gain: pass * level,
                    });
                }
            }
        }
    }
    // A soloed strip keeps what it reaches and what reaches it, as
    // `mixer_rules::audible` does for the static solos.
    let mut reach = solo_reach(m);
    let mut controls: Vec<StripControlSpec> = Vec::new();
    for (k, s) in m.strips().values().enumerate() {
        let Some(spec) = strips.get_mut(k) else {
            continue;
        };
        let (gl, gr) = strip_gains(s.gain_db, s.pan, s.id != m.master());
        spec.gain_l = gl;
        spec.gain_r = gr;
        spec.audible = !muted(s);
        controls.push(StripControlSpec {
            strip: s.id,
            node: base + k,
            gain_db: s.gain_db,
            pan: s.pan,
            apply_pan: s.id != m.master(),
            mute: s.mute,
            solo: s.solo,
            feeds: feeds.remove(&s.id).unwrap_or_default(),
            solo_keeps: reach
                .remove(&s.id)
                .map(|x| x.into_iter().collect())
                .unwrap_or_else(|| vec![s.id]),
        });
        for slot in s.inserts.iter() {
            let sidechain = slot.sidechain.and_then(|src| {
                let from = node(src)?;
                let explicit = m.strip(src).map(|x| x.mute).unwrap_or(false);
                Some(InputSpec {
                    from,
                    tap: Tap::Post,
                    gain: if explicit { 0.0 } else { 1.0 },
                })
            });
            spec.inserts.push(InsertSpec {
                instance: Some(slot.id),
                latency: insert_latency(slot),
                bypass: slot.bypass,
                sidechain,
            });
        }
        let has_feed = node(s.id).is_some_and(|n| fed.contains(&n));
        if matches!(s.kind, StripKind::Instrument(_)) && !has_feed {
            warnings.push(format!("strip {:?} has no instrument", s.name));
        }
    }
    let strip_nodes = m
        .strips()
        .keys()
        .enumerate()
        .map(|(i, id)| (base + i, *id))
        .collect();
    let mut nodes: Vec<NodeSpec> = instruments
        .iter()
        .map(|f| NodeSpec::Instrument {
            target: f.target,
            latency: f.latency,
            outputs: f.outputs.max(1),
        })
        .collect();
    nodes.extend(strips.into_iter().map(NodeSpec::Strip));
    nodes.extend(
        region_paths
            .into_iter()
            .map(|path| NodeSpec::Regions { path }),
    );
    MixerGraph {
        spec: GraphSpec { nodes, master },
        strips: strip_nodes,
        warnings,
        controls,
    }
}
