//! An automation curve's target, read one way by the decision and the
//! apply (Task 7 fix round 6, automation case B).
//!
//! **The key.** Each side names a track's curve by its places on that side
//! ("fader 2", "send 2 0", "param 2 1 7": the strip, a send and a device by
//! index). [`key`] gives the strip as its row of `matches.strips` and a
//! send by its target's row ("send 1 3"), so a curve of theirs and one of
//! ours on the same send have one key, whatever order the two strips hold
//! their sends in. A plug-in parameter names its device by the slot's
//! plug-in (format and ID) and which slot of that plug-in it is on the
//! strip ("param 1 clap:org.example.Comp#0 7"), never by its place in the
//! strip's list (Task 7 fix round 6, case B follow-up).
//!
//! **Where it lands.** [`resolve`] finds a key's target in ours through a
//! [`Holds`]: `decide` asks it of ours as the merge will leave it (ours'
//! strips, sends and devices, the buses it adds and the sends its strip
//! step adds), `apply` of the document it is editing. A curve `decide`
//! cannot resolve is listed under [`super::text::NOT_APPLIED`] and runs no
//! command; one it can is placed by the apply, after the strip step when
//! its send or device is made there.
//!
//! **A device's identity.** Ours' slot for theirs' device is the slot of
//! that plug-in ([`find`]); when theirs' slot came from one of ours (Task
//! 2's [`Origins::slots`], with the file's tags), it must be that very
//! slot ([`pick`]): a slot ours removed, or swapped for another plug-in,
//! is not there. Only when theirs' device list lands (the strip step's
//! device change) is the slot the one the step makes for theirs' device.

use crate::dawproject::import::Origins;
use crate::dawproject::record::{PStrip, StripIds};
use core_model::ids::PluginInstanceId;

/// A projection's device: format, plug-in ID, bypass, state hash.
pub(crate) type Device = (String, String, bool, Option<u64>);

/// A device as a curve's key names it: its plug-in, and which slot of
/// that plug-in it is on its strip (0 for the first).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PluginAt {
    pub(crate) format: String,
    pub(crate) id: String,
    pub(crate) n: usize,
}

impl PluginAt {
    /// Device `d` of `devices`.
    fn of(devices: &[Device], d: usize) -> Option<PluginAt> {
        let (format, id, ..) = devices.get(d)?;
        let n = devices
            .iter()
            .take(d)
            .filter(|x| x.0 == *format && x.1 == *id)
            .count();
        Some(PluginAt {
            format: format.clone(),
            id: id.clone(),
            n,
        })
    }

    /// As one word of a key: "clap:org.example.Comp#0" (a space, `#` or
    /// `%` in the ID escaped).
    fn word(&self) -> String {
        let esc = |x: &str| {
            x.replace('%', "%25")
                .replace(' ', "%20")
                .replace('#', "%23")
                .replace(':', "%3A")
        };
        format!("{}:{}#{}", esc(&self.format), esc(&self.id), self.n)
    }

    fn parse(w: &str) -> Option<PluginAt> {
        let un = |x: &str| {
            x.replace("%3A", ":")
                .replace("%23", "#")
                .replace("%20", " ")
                .replace("%25", "%")
        };
        let (rest, n) = w.rsplit_once('#')?;
        let (format, id) = rest.split_once(':')?;
        Some(PluginAt {
            format: un(format),
            id: un(id),
            n: n.parse().ok()?,
        })
    }
}

/// The slot of `at` among a strip's `devices` (with their slot IDs `ids`,
/// in the same order): the `n`-th slot of that plug-in.
pub(crate) fn find(devices: &[Device], ids: &[u64], at: &PluginAt) -> Option<u64> {
    devices
        .iter()
        .zip(ids)
        .filter(|(x, _)| x.0 == at.format && x.1 == at.id)
        .nth(at.n)
        .map(|(_, id)| *id)
}

/// [`find`], held to `origin` (ours' slot theirs' came from, when known):
/// the slot found must be that one.
pub(crate) fn pick(
    devices: &[Device],
    ids: &[u64],
    at: &PluginAt,
    origin: Option<u64>,
) -> Option<u64> {
    let found = find(devices, ids, at)?;
    origin.is_none_or(|x| x == found).then_some(found)
}

/// Ours' slot theirs' device `at` came from (Task 2's origins), when the
/// file's tags name one.
pub(crate) fn origin_of(
    strip: &PStrip,
    ids: &StripIds,
    origins: &Origins,
    at: &PluginAt,
) -> Option<u64> {
    let slot = find(&strip.devices, &ids.devices, at)?;
    origins.slots.get(&PluginInstanceId(slot)).copied()
}

/// A strip's devices once theirs' device list lands on it: ours' first
/// `keep` (its instrument, which the merge never changes), then theirs'
/// after its first `skip`.
pub(crate) fn landed(ours: &[Device], keep: usize, theirs: &[Device], skip: usize) -> Vec<Device> {
    ours.iter()
        .take(keep)
        .chain(theirs.iter().skip(skip))
        .cloned()
        .collect()
}

/// [`key`]'s mark for a strip on theirs' side only (`decide`'s
/// `Side::Theirs`).
pub(crate) const THEIRS: usize = 1;
/// [`key`]'s mark for a strip on ours' side only (`decide`'s `Side::Ours`).
pub(crate) const OURS: usize = 2;

/// Side `tag`'s curve `name`, as its projection names it, keyed as the
/// merge compares it: the strip as its row (`row`), a send by its target's
/// row (through `strips`, that side's). A strip or send target with no row
/// is on that side only: it keeps its place, marked `@tag`, so no other
/// side's curve has its key.
pub(crate) fn key(
    name: &str,
    tag: usize,
    row: impl Fn(usize) -> Option<usize>,
    strips: &[PStrip],
) -> String {
    let mut words: Vec<String> = name.split(' ').map(str::to_owned).collect();
    let c = words.get(1).and_then(|c| c.parse::<usize>().ok());
    let mark = |w: &mut String, k: Option<usize>| match k {
        Some(k) => *w = k.to_string(),
        None => *w = format!("{w}@{tag}"),
    };
    if words.first().is_some_and(|w| w == "send") {
        let to = c
            .zip(words.get(2).and_then(|i| i.parse::<usize>().ok()))
            .and_then(|(c, i)| strips.get(c)?.sends.get(i).map(|s| s.0))
            .and_then(&row);
        if let Some(w) = words.get_mut(2) {
            mark(w, to);
        }
    }
    if words.first().is_some_and(|w| w == "param") {
        let at = c
            .zip(words.get(2).and_then(|d| d.parse::<usize>().ok()))
            .and_then(|(c, d)| PluginAt::of(&strips.get(c)?.devices, d));
        if let Some(w) = words.get_mut(2) {
            match at {
                Some(at) => *w = at.word(),
                None => *w = format!("{w}@{tag}"),
            }
        }
    }
    if let Some(w) = words.get_mut(1) {
        mark(w, c.and_then(&row));
    }
    words.join(" ")
}

/// What ours holds, as [`resolve`] asks it. Strips are rows of
/// `matches.strips`.
pub(crate) trait Holds {
    /// Ours' strip.
    type Strip: Copy;
    /// One of its sends.
    type Send;
    /// One of its device slots.
    type Slot;
    /// Ours' strip for row `row`.
    fn strip(&self, row: usize) -> Option<Self::Strip>;
    /// Strip `from`'s send to strip `to`, each with its row.
    fn send(&self, from: (usize, Self::Strip), to: (usize, Self::Strip)) -> Option<Self::Send>;
    /// Strip `strip`'s (of row `row`) slot for theirs' device `at`.
    fn slot(&self, strip: (usize, Self::Strip), at: &PluginAt) -> Option<Self::Slot>;
}

/// Where a curve lands in ours.
pub(crate) enum Landing<H: Holds> {
    Fader(H::Strip),
    Pan(H::Strip),
    Mute(H::Strip),
    Send(H::Strip, H::Send),
    /// A device slot's parameter, by its stable ID.
    Param(H::Slot, u64),
}

/// Why a curve does not land in ours.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Missing {
    /// Ours has no strip for it.
    Strip,
    /// Its strip has no send to the strip of that row (`None`: a strip
    /// ours has no row for).
    Send(Option<usize>),
    /// Its strip has no slot of that plug-in, or not the one theirs'
    /// came from (removed or swapped here).
    Plugin,
}

/// Where curve `key` (a [`key`]) lands in `ours`, or why it does not.
pub(crate) fn resolve<H: Holds>(key: &str, ours: &H) -> Result<Landing<H>, Missing> {
    let words: Vec<&str> = key.split(' ').collect();
    let n = |i: usize| words.get(i).and_then(|w| w.parse::<usize>().ok());
    let row = n(1).ok_or(Missing::Strip)?;
    let strip = ours.strip(row).ok_or(Missing::Strip)?;
    match words.first().copied() {
        Some("fader") => Ok(Landing::Fader(strip)),
        Some("pan") => Ok(Landing::Pan(strip)),
        Some("mute") => Ok(Landing::Mute(strip)),
        Some("send") => {
            let to = n(2).ok_or(Missing::Send(None))?;
            let target = ours.strip(to).ok_or(Missing::Send(Some(to)))?;
            let send = ours
                .send((row, strip), (to, target))
                .ok_or(Missing::Send(Some(to)))?;
            Ok(Landing::Send(strip, send))
        }
        Some("param") => {
            let at = words
                .get(2)
                .and_then(|w| PluginAt::parse(w))
                .ok_or(Missing::Plugin)?;
            let q = words
                .get(3)
                .and_then(|w| w.parse::<u64>().ok())
                .ok_or(Missing::Plugin)?;
            let slot = ours.slot((row, strip), &at).ok_or(Missing::Plugin)?;
            Ok(Landing::Param(slot, q))
        }
        _ => Err(Missing::Strip),
    }
}
