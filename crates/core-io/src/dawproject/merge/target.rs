//! An automation curve's target, read one way by the decision and the
//! apply (Task 7 fix round 6, automation case B).
//!
//! **The key.** Each side names a track's curve by its places on that side
//! ("fader 2", "send 2 0", "param 2 1 7": the strip, a send and a device by
//! index). [`key`] gives the strip as its row of `matches.strips` and a
//! send by its target's row ("send 1 3"), so a curve of theirs and one of
//! ours on the same send have one key, whatever order the two strips hold
//! their sends in.
//!
//! **Where it lands.** [`resolve`] finds a key's target in ours through a
//! [`Holds`]: `decide` asks it of ours as the merge will leave it (ours'
//! strips, sends and devices, the buses it adds and the sends its strip
//! step adds), `apply` of the document it is editing. A curve `decide`
//! cannot resolve is listed under [`super::text::NOT_APPLIED`] and runs no
//! command; one it can is placed by the apply, after the strip step when
//! its send is made there.

use crate::dawproject::record::PStrip;

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
    /// Row `row`'s strip's device slot `device`, in the projection's order.
    fn slot(&self, row: usize, device: usize) -> Option<Self::Slot>;
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
    /// Its strip has no such device.
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
            let device = n(2).ok_or(Missing::Plugin)?;
            let q = words
                .get(3)
                .and_then(|w| w.parse::<u64>().ok())
                .ok_or(Missing::Plugin)?;
            let slot = ours.slot(row, device).ok_or(Missing::Plugin)?;
            Ok(Landing::Param(slot, q))
        }
        _ => Err(Missing::Strip),
    }
}
