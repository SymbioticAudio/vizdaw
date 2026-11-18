//! Starter project specs (plan 8e Ruling 5). [`parse`] checks the whole
//! spec of `co_producer.starter` before anything happens, so a bad spec
//! opens no window; it is the only parser, and `core-ffi` calls it again.
//!
//! A starter sets up sounds and routing only: tempo, meter, markers,
//! buses, tracks, effects, outputs and sends. The spec has no place for
//! notes, regions or automation. Every function here is total: hostile
//! input gives a sentence, never a panic.

use crate::protocol::truncate;
use crate::registry::{decimal, plugin_ref};
use crate::request::{marker_name, refused_in_name};
use serde_json::{Map, Value};

/// The most tracks in a starter.
pub const MAX_TRACKS: usize = 32;
/// The most buses in a starter.
pub const MAX_BUSES: usize = 16;
/// The most markers in a starter.
pub const MAX_MARKERS: usize = 32;
/// The most effects on one strip.
pub const MAX_EFFECTS: usize = 8;
/// The most sends from one track.
pub const MAX_SENDS: usize = 8;
/// The longest title or strip name, in characters after trimming.
pub const MAX_NAME: usize = 64;
/// The tempo's range in bpm.
pub const TEMPO_BPM: (f64, f64) = (20.0, 300.0);
/// The highest marker bar.
pub const MAX_BAR: u64 = 999;
/// The most beats in a bar.
pub const MAX_METER_NUM: u8 = 32;
/// The beat values a meter may have.
pub const METER_DENS: [u8; 6] = [1, 2, 4, 8, 16, 32];
/// A send level's range in dB (−144 is −∞).
const LEVEL_DB: (f64, f64) = (-144.0, 12.0);

/// What a starter track is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StarterKind {
    /// Plays an instrument (its `plugin`, else the Built-in Synth).
    Instrument,
    /// Records audio.
    Audio,
}

/// A send from a starter track.
#[derive(Debug, Clone, PartialEq)]
pub struct StarterSend {
    /// The bus it feeds, as the spec's `buses` names it.
    pub bus: String,
    /// Its level in dB, −144 (silent) to 12; default 0.
    pub level_db: f64,
}

/// A starter bus.
#[derive(Debug, Clone, PartialEq)]
pub struct StarterBus {
    /// Its name, trimmed.
    pub name: String,
    /// Its effects' plug-in references, in insert order.
    pub effects: Vec<String>,
    /// The bus it goes to, as `buses` names it; None: master.
    pub output: Option<String>,
}

/// A starter track.
#[derive(Debug, Clone, PartialEq)]
pub struct StarterTrack {
    /// Its name, trimmed.
    pub name: String,
    /// Instrument or audio.
    pub kind: StarterKind,
    /// Its instrument's plug-in reference (instrument tracks only); None:
    /// the Built-in Synth.
    pub plugin: Option<String>,
    /// Its effects' plug-in references, in insert order.
    pub effects: Vec<String>,
    /// The bus it goes to, as `buses` names it; None: master.
    pub output: Option<String>,
    /// Its sends, in order.
    pub sends: Vec<StarterSend>,
}

/// A checked starter spec.
#[derive(Debug, Clone, PartialEq)]
pub struct StarterSpec {
    /// The song's title, trimmed.
    pub title: String,
    /// The tempo in thousandths of a bpm.
    pub tempo_milli: u32,
    /// The meter: beats in a bar, and the beat value.
    pub meter: (u8, u8),
    /// The markers: their bar (from 1) and name, in the spec's order.
    pub markers: Vec<(u32, String)>,
    /// The buses, in order.
    pub buses: Vec<StarterBus>,
    /// The tracks, in order.
    pub tracks: Vec<StarterTrack>,
}

/// A key or name fit to show in a sentence: cut short, with any control,
/// bidi or separator character replaced.
fn shown(text: &str) -> String {
    truncate(text, 64)
        .chars()
        .map(|c| if refused_in_name(c) { '\u{FFFD}' } else { c })
        .collect()
}

/// The object `v`, holding only `allowed` keys.
fn object<'a>(
    v: &'a Value,
    what: &str,
    allowed: &[&str],
) -> Result<&'a Map<String, Value>, String> {
    let Value::Object(m) = v else {
        return Err(format!("{what} must be an object."));
    };
    if let Some(k) = m.keys().find(|k| !allowed.contains(&k.as_str())) {
        return Err(format!("Unknown key {} in the starter project.", shown(k)));
    }
    Ok(m)
}

/// The array at `key` of `m` (absent: empty), of at most `max` items.
fn list<'a>(
    m: &'a Map<String, Value>,
    key: &str,
    what: &str,
    max: usize,
) -> Result<&'a [Value], String> {
    match m.get(key) {
        None => Ok(&[]),
        Some(Value::Array(a)) if a.len() <= max => Ok(a),
        Some(Value::Array(_)) => Err(format!("{what} has more than {max} {key}.")),
        Some(_) => Err(format!("{what}'s {key} must be a list.")),
    }
}

/// A title or strip name (P11's rules): text of 1 to [`MAX_NAME`]
/// characters after trimming, with no control, bidi or separator
/// character.
fn name(v: Option<&Value>, what: &str) -> Result<String, String> {
    let bad = || {
        format!(
            "{what} must be text of 1 to {MAX_NAME} characters, with no control, direction or line-separator character."
        )
    };
    let Some(Value::String(raw)) = v else {
        return Err(bad());
    };
    let t = raw.trim();
    if t.is_empty() || t.chars().count() > MAX_NAME || t.chars().any(refused_in_name) {
        return Err(bad());
    }
    Ok(t.to_string())
}

/// Optional text at `key` of `m`.
fn opt_text<'a>(
    m: &'a Map<String, Value>,
    key: &str,
    what: &str,
) -> Result<Option<&'a str>, String> {
    match m.get(key) {
        None => Ok(None),
        Some(Value::String(s)) => Ok(Some(s)),
        Some(_) => Err(format!("{what}'s {key} must be text.")),
    }
}

/// A strip's `effects`: 0 to [`MAX_EFFECTS`] effect references.
fn effects(m: &Map<String, Value>, what: &str) -> Result<Vec<String>, String> {
    list(m, "effects", what, MAX_EFFECTS)?
        .iter()
        .map(|e| match e {
            Value::String(r) => plugin_ref(r, false)
                .map(|()| r.clone())
                .map_err(|why| format!("{what}: an effect {why}.")),
            _ => Err(format!("{what}: an effect must be text.")),
        })
        .collect()
}

fn tempo(m: &Map<String, Value>) -> Result<u32, String> {
    let bad = || {
        format!(
            "tempo must be text, a plain decimal from {} to {} bpm, such as \"118\".",
            TEMPO_BPM.0, TEMPO_BPM.1
        )
    };
    let Some(Value::String(t)) = m.get("tempo") else {
        return Err(bad());
    };
    let bpm = decimal(t, TEMPO_BPM.0, TEMPO_BPM.1).map_err(|_| bad())?;
    // 20 000 to 300 000: always a u32.
    Ok((bpm * 1000.0).round() as u32)
}

fn meter(m: &Map<String, Value>) -> Result<(u8, u8), String> {
    let bad = || {
        format!(
            "meter must be text \"<beats>/<value>\", with 1 to {MAX_METER_NUM} beats and a value of 1, 2, 4, 8, 16 or 32, such as \"4/4\"."
        )
    };
    let Some(Value::String(t)) = m.get("meter") else {
        return Err(bad());
    };
    let number = |s: &str| -> Option<u8> {
        if s.is_empty() || s.len() > 2 || !s.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        s.parse().ok()
    };
    let (num, den) = t.split_once('/').ok_or_else(bad)?;
    match (number(num), number(den)) {
        (Some(n), Some(d)) if (1..=MAX_METER_NUM).contains(&n) && METER_DENS.contains(&d) => {
            Ok((n, d))
        }
        _ => Err(bad()),
    }
}

fn markers(m: &Map<String, Value>) -> Result<Vec<(u32, String)>, String> {
    list(m, "markers", "The starter project", MAX_MARKERS)?
        .iter()
        .map(|v| {
            let mk = object(v, "A marker", &["bar", "name"])?;
            let bar = match mk.get("bar") {
                Some(Value::Number(n)) => n.as_u64().filter(|b| (1..=MAX_BAR).contains(b)),
                _ => None,
            };
            let Some(bar) = bar else {
                return Err(format!(
                    "A marker's bar must be a whole number from 1 to {MAX_BAR}."
                ));
            };
            let name = match mk.get("name") {
                Some(Value::String(s)) => {
                    marker_name(s).map_err(|why| format!("A marker's {why}."))?
                }
                _ => return Err("A marker's name must be text.".into()),
            };
            // 1 to 999: always a u32.
            Ok((bar as u32, name))
        })
        .collect()
}

/// The bus `given` names, ignoring case, as `buses` spells it.
fn bus_named(buses: &[StarterBus], given: &str) -> Result<String, String> {
    let g = given.trim().to_lowercase();
    buses
        .iter()
        .find(|b| b.name.to_lowercase() == g)
        .map(|b| b.name.clone())
        .ok_or_else(|| format!("No bus {} in the starter project.", shown(given.trim())))
}

fn buses(m: &Map<String, Value>) -> Result<Vec<StarterBus>, String> {
    let raw = list(m, "buses", "The starter project", MAX_BUSES)?;
    let mut buses = Vec::with_capacity(raw.len());
    let mut outputs = Vec::with_capacity(raw.len());
    for v in raw {
        let b = object(v, "A bus", &["name", "effects", "output"])?;
        let name = name(b.get("name"), "A bus's name")?;
        let what = format!("Bus {name}");
        buses.push(StarterBus {
            effects: effects(b, &what)?,
            name,
            output: None,
        });
        outputs.push(opt_text(b, "output", &what)?);
    }
    // Outputs name buses, which are all known only now.
    let named: Vec<Option<String>> = outputs
        .iter()
        .map(|o| o.map(|o| bus_named(&buses, o)).transpose())
        .collect::<Result<_, _>>()?;
    for (b, o) in buses.iter_mut().zip(named) {
        b.output = o;
    }
    no_cycle(&buses)?;
    Ok(buses)
}

/// Bus outputs make no cycle: the first cycle found names its first two
/// buses.
fn no_cycle(buses: &[StarterBus]) -> Result<(), String> {
    let index = |n: &str| buses.iter().position(|b| b.name == n);
    for start in 0..buses.len() {
        let mut path = vec![start];
        let mut at = start;
        while let Some(next) = buses
            .get(at)
            .and_then(|b| b.output.as_deref())
            .and_then(index)
        {
            if let Some(i) = path.iter().position(|p| *p == next) {
                let a = path.get(i).and_then(|i| buses.get(*i));
                let b = path.get(i + 1).or(path.get(i)).and_then(|i| buses.get(*i));
                return Err(match (a, b) {
                    (Some(a), Some(b)) if a.name == b.name => {
                        format!("Bus {} feeds itself.", a.name)
                    }
                    (Some(a), Some(b)) => {
                        format!("Buses {} and {} feed each other.", a.name, b.name)
                    }
                    _ => "Buses feed each other.".into(),
                });
            }
            if path.len() > buses.len() {
                break;
            }
            path.push(next);
            at = next;
        }
    }
    Ok(())
}

fn tracks(m: &Map<String, Value>, buses: &[StarterBus]) -> Result<Vec<StarterTrack>, String> {
    let raw = list(m, "tracks", "The starter project", MAX_TRACKS)?;
    if raw.is_empty() {
        return Err(format!(
            "The starter project needs tracks: a list of 1 to {MAX_TRACKS}."
        ));
    }
    raw.iter()
        .map(|v| {
            let t = object(
                v,
                "A track",
                &["name", "kind", "plugin", "effects", "output", "sends"],
            )?;
            let name = name(t.get("name"), "A track's name")?;
            let what = format!("Track {name}");
            let kind = match t.get("kind").and_then(Value::as_str) {
                Some("instrument") => StarterKind::Instrument,
                Some("audio") => StarterKind::Audio,
                _ => return Err(format!("{what}'s kind must be instrument or audio.")),
            };
            let plugin = match opt_text(t, "plugin", &what)? {
                None => None,
                Some(_) if kind == StarterKind::Audio => {
                    return Err("plugin is only for instrument tracks.".into());
                }
                Some(r) => {
                    plugin_ref(r, true).map_err(|why| format!("{what}: plugin {why}."))?;
                    Some(r.to_string())
                }
            };
            let output = opt_text(t, "output", &what)?
                .map(|o| bus_named(buses, o))
                .transpose()?;
            let sends = list(t, "sends", &what, MAX_SENDS)?
                .iter()
                .map(|s| {
                    let s = object(s, "A send", &["bus", "level_db"])?;
                    let bus = match s.get("bus") {
                        Some(Value::String(b)) => bus_named(buses, b)?,
                        _ => return Err(format!("{what}: a send's bus must be text.")),
                    };
                    let level_db = match opt_text(s, "level_db", &what)? {
                        None => 0.0,
                        Some(l) => decimal(l, LEVEL_DB.0, LEVEL_DB.1)
                            .map_err(|why| format!("{what}: a send's level_db {why}."))?,
                    };
                    Ok(StarterSend { bus, level_db })
                })
                .collect::<Result<_, String>>()?;
            Ok(StarterTrack {
                effects: effects(t, &what)?,
                name,
                kind,
                plugin,
                output,
                sends,
            })
        })
        .collect()
}

/// Ruling 5's checks, in full: the spec's shape and limits, P11's names,
/// unique strip names ignoring case, outputs and sends that name a bus of
/// the spec, bus outputs with no cycle, plug-in references of 8d
/// Ruling 3's shape and no unknown key anywhere. Each refusal is one
/// sentence.
pub fn parse(v: &Value) -> Result<StarterSpec, String> {
    let m = object(
        v,
        "The starter project",
        &["title", "tempo", "meter", "markers", "buses", "tracks"],
    )?;
    let title = name(m.get("title"), "The title")?;
    let tempo_milli = tempo(m)?;
    let meter = meter(m)?;
    let markers = markers(m)?;
    let buses = buses(m)?;
    let tracks = tracks(m, &buses)?;
    let mut seen: Vec<String> = Vec::new();
    for n in buses
        .iter()
        .map(|b| &b.name)
        .chain(tracks.iter().map(|t| &t.name))
    {
        let lower = n.to_lowercase();
        if seen.contains(&lower) {
            return Err(format!("Two strips are named {n}."));
        }
        seen.push(lower);
    }
    Ok(StarterSpec {
        title,
        tempo_milli,
        meter,
        markers,
        buses,
        tracks,
    })
}
