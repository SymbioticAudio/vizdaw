//! Live input (sub-project 5 spec §6.6; §6.8's hook): MIDI from sources to
//! armed lanes' destinations (thru) and to the recorder, and audio input
//! monitoring through a lane's output path. The control side describes
//! what each lane listens to ([`InputSpec`]); the engine resolves it into
//! [`InputRoutes`] at its next load, and the renderer applies them each
//! block. // RT where marked

use crate::midi1::Midi1Msg;
use crate::schedule::TargetIdx;
use core_model::ids::{DestId, LaneId, OutputPathId};
use core_model::lane::MonitorMode;
use rtrb::{Consumer, Producer, RingBuffer};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// MIDI messages the input queue holds until the renderer takes them.
pub const MIDI_IN_CAPACITY: usize = 4096;
/// Messages the renderer takes from the queue per block at most.
pub const MIDI_IN_PER_BLOCK: usize = 1024;
/// The most audio lanes monitored at once.
pub const MAX_MONITOR_ROUTES: usize = 64;

/// How lanes name a MIDI source to the real-time side: FNV-1a of its
/// name.
pub fn port_key(name: &str) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for b in name.as_bytes() {
        h ^= u32::from(*b);
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// A MIDI channel message from a source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MidiIn {
    /// The source ([`port_key`] of its name).
    pub port: u32,
    /// When it arrived, in host nanoseconds; 0: now (an injected message,
    /// placed at the block that takes it; plan 5c Ruling 8).
    pub host_ns: u64,
    /// The message.
    pub msg: Midi1Msg,
}

/// The channel message `bytes` holds (`80`–`EF` status, with its data
/// bytes), if it is one.
pub fn channel_message(bytes: &[u8]) -> Option<Midi1Msg> {
    let (&status, rest) = bytes.split_first()?;
    let len = match status & 0xF0 {
        0x80 | 0x90 | 0xA0 | 0xB0 | 0xE0 => 2,
        0xC0 | 0xD0 => 1,
        _ => return None,
    };
    if rest.len() != len || rest.iter().any(|b| *b > 0x7F) {
        return None;
    }
    Some(match rest {
        [a] => Midi1Msg::two(status, *a),
        [a, b] => Midi1Msg::three(status, *a, *b),
        _ => return None,
    })
}

/// Feeds the renderer's MIDI input queue from any thread (CoreMIDI's
/// callback, the control channel). Cheap to clone. Sending never blocks
/// (pre-flight M8: CoreMIDI calls it on a high-priority thread): when
/// another sender holds the queue, the message is dropped and counted like
/// one that found the queue full. The renderer's end takes no lock.
#[derive(Clone)]
pub struct MidiSender {
    tx: Arc<Mutex<Producer<MidiIn>>>,
    dropped: Arc<AtomicU64>,
}

impl std::fmt::Debug for MidiSender {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MidiSender")
    }
}

impl MidiSender {
    /// Queues `ev` without blocking; false when the queue is full or
    /// another sender holds it (it is dropped and counted).
    pub fn send(&self, ev: MidiIn) -> bool {
        let ok = match self.tx.try_lock() {
            Ok(mut tx) => tx.push(ev).is_ok(),
            // A sender panicked while pushing: the producer itself is
            // still sound (`push` either wrote the slot or did not).
            Err(std::sync::TryLockError::Poisoned(p)) => p.into_inner().push(ev).is_ok(),
            Err(std::sync::TryLockError::WouldBlock) => false,
        };
        if !ok {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
        ok
    }

    /// Messages dropped so far because the queue was full or busy (for
    /// diagnostics).
    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
}

/// The MIDI input queue: the sender and the renderer's end.
pub(crate) fn midi_queue() -> (MidiSender, Consumer<MidiIn>) {
    let (tx, rx) = RingBuffer::new(MIDI_IN_CAPACITY);
    (
        MidiSender {
            tx: Arc::new(Mutex::new(tx)),
            dropped: Arc::new(AtomicU64::new(0)),
        },
        rx,
    )
}

/// What a MIDI lane listens to, before targets are resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MidiInputSpec {
    /// The lane.
    pub lane: LaneId,
    /// The source ([`port_key`]); `None`: every source.
    pub port: Option<u32>,
    /// The channel 0–15; `None`: every channel.
    pub channel: Option<u8>,
    /// Where thru goes (the lane's destination).
    pub dest: Option<DestId>,
    /// Thru is on. The control side decides: spec §6.6 plays thru on an
    /// armed lane unless its monitor is Off (`armed && monitor != Off`);
    /// plan 5c Ruling 9 also lets a lane monitored Input listen unarmed.
    /// The engine plays thru whenever this is set.
    pub thru: bool,
    /// The lane is armed: it records while a tap rolls.
    pub armed: bool,
    /// The lane's input effect chain (spec §6.8; plan 5g-1).
    pub fx: Vec<core_model::input_fx::InputFx>,
    /// The lane's MPE input (plan 5g-2): its zone's channels are decoded
    /// into per-note messages before the chain, and `channel` is ignored.
    pub mpe: Option<core_model::mpe::MpeConfig>,
}

/// One audio lane's monitoring (spec §6.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonitorRoute {
    /// The lane: while it records with its input heard, its own regions
    /// are silent (plan 5d-1 Ruling 6).
    pub lane: LaneId,
    /// The output path whose strip hears it.
    pub path: OutputPathId,
    /// The device channel heard left.
    pub left: u16,
    /// The device channel heard right (the same one for a mono input).
    pub right: u16,
    /// When it is heard.
    pub mode: MonitorMode,
    /// The lane is armed.
    pub armed: bool,
}

impl MonitorRoute {
    /// Whether the input is heard now: Input always, Off never, Auto
    /// while armed and either stopped or recording inside the record range
    /// (spec §6.6). // RT
    pub fn heard(&self, playing: bool, recording_in_range: bool) -> bool {
        match self.mode {
            MonitorMode::Input => true,
            MonitorMode::Off => false,
            MonitorMode::Auto => self.armed && (!playing || recording_in_range),
        }
    }
}

/// What every lane listens to (the control side's request).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InputSpec {
    /// MIDI lanes.
    pub midi: Vec<MidiInputSpec>,
    /// Monitored audio lanes (at most [`MAX_MONITOR_ROUTES`]).
    pub monitor: Vec<MonitorRoute>,
}

/// One MIDI lane's resolved route.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MidiRoute {
    /// The request.
    pub spec: MidiInputSpec,
    /// The targets thru plays on (two for a built-in synth routed both
    /// ways).
    pub targets: [Option<TargetIdx>; 2],
    /// The channel thru plays on: the destination's, or its MPE zone's
    /// manager channel (plan 5g-2 P1).
    pub dest_channel: u8,
    /// The destination is a muted external one: thru drops its note-ons
    /// and note-offs (plan 5g-1b); recording is unaffected.
    pub mute_notes: bool,
    /// The lane's input chain (plan 5g-1).
    pub chain: crate::input_fx::InputChain,
    /// The lane's MPE decoder (plan 5g-2), from `spec.mpe`.
    pub decoder: Option<crate::mpe::MpeDecoder>,
    /// The destination's MPE output (plan 5g-2): thru goes through the
    /// route's encoder in [`InputRoutes::encoders`].
    pub mpe_out: Option<core_model::mpe::MpeConfig>,
}

impl MidiRoute {
    /// Whether `ev` is for this lane: with MPE input, a channel of the
    /// decoder's zone (the lane's channel is ignored; plan 5g-2 Ruling 2).
    /// // RT
    pub fn matches(&self, ev: &MidiIn) -> bool {
        let ch = ev.msg.bytes[0] & 0x0F;
        self.spec.port.is_none_or(|p| p == ev.port)
            && match &self.decoder {
                Some(d) => d.accepts(ch),
                None => self.spec.channel.is_none_or(|c| c == ch),
            }
    }

    /// Whether this new route takes over `old`'s chain state (plan 5g-1
    /// Ruling 5): the same lane, source, channel, destination, thru,
    /// mute, targets, chain and MPE output; the arm may differ. With MPE
    /// input on both, the new setting must be the old decoder's live one
    /// (an MCM's write-back carries; plan 5g-2 Ruling 4); a new route
    /// without MPE input carries an old one an MCM turned off. // RT
    pub fn carries(&self, old: &MidiRoute) -> bool {
        let decoders = match (&self.decoder, &old.decoder) {
            (Some(n), Some(o)) => o.config() == Some(n.initial()),
            (None, Some(o)) => o.config().is_none(),
            (Some(_), None) => false,
            (None, None) => true,
        };
        let (a, b) = (&self.spec, &old.spec);
        a.lane == b.lane
            && a.port == b.port
            && a.channel == b.channel
            && a.dest == b.dest
            && a.thru == b.thru
            && self.targets == old.targets
            && self.dest_channel == old.dest_channel
            && self.mute_notes == old.mute_notes
            && self.chain == old.chain
            && self.mpe_out == old.mpe_out
            && decoders
    }
}

/// The renderer's live input routes, published at every load.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InputRoutes {
    /// MIDI lanes.
    pub midi: Box<[MidiRoute]>,
    /// Per `midi` route, its MPE output's encoder (plan 5g-2 P6): built
    /// on the control thread at each load, moved into the next routes when
    /// the route is carried.
    pub encoders: Box<[Option<crate::mpe::MpeEncoder>]>,
    /// Monitored audio lanes.
    pub monitor: Box<[MonitorRoute]>,
}

/// `msg` on channel `ch`. // RT
pub fn on_channel(msg: Midi1Msg, ch: u8) -> Midi1Msg {
    let mut m = msg;
    m.bytes[0] = (m.bytes[0] & 0xF0) | (ch & 0x0F);
    m
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    fn ev(pitch: u8) -> MidiIn {
        MidiIn {
            port: port_key("Keys"),
            host_ns: 0,
            msg: Midi1Msg::three(0x90, pitch, 100),
        }
    }

    /// Pre-flight M8: a sender that finds the queue held by another drops
    /// the message and counts it instead of waiting.
    #[test]
    fn a_held_queue_drops_and_counts_instead_of_blocking() {
        let (tx, mut rx) = midi_queue();
        let holder = tx.clone();
        let (locked_tx, locked_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel::<()>();
        let t = std::thread::spawn(move || {
            let _guard = holder.tx.lock().unwrap();
            locked_tx.send(()).unwrap();
            let _ = release_rx.recv();
        });
        locked_rx.recv().unwrap();
        let started = Instant::now();
        assert!(!tx.send(ev(60)));
        assert!(started.elapsed() < Duration::from_secs(1));
        assert_eq!(tx.dropped(), 1);
        release_tx.send(()).unwrap();
        t.join().unwrap();
        // Free again: queued, nothing more counted.
        assert!(tx.send(ev(61)));
        assert_eq!(tx.dropped(), 1);
        assert_eq!(rx.pop().map(|e| e.msg.bytes[1]), Ok(61));
    }

    #[test]
    fn a_full_queue_drops_and_counts() {
        let (tx, _rx) = midi_queue();
        for _ in 0..MIDI_IN_CAPACITY {
            assert!(tx.send(ev(60)));
        }
        assert!(!tx.send(ev(60)));
        assert!(!tx.clone().send(ev(60)));
        assert_eq!(tx.dropped(), 2);
    }
}
