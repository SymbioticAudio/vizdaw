//! CoreMIDI output: VizDAW's virtual source and existing destinations.
//!
//! `MidiOut::send` is wait-free: it pushes MIDI 1.0 UMP-sized packets into
//! an `rtrb` ring. A sender thread drains every ring each millisecond and
//! hands the packets to CoreMIDI (`MIDIReceivedEventList` for the virtual
//! source, `MIDISendEventList` for destinations) with their host-time
//! timestamps, so the IPC never happens on the audio thread.

use block2::RcBlock;
use core_engine::BackendError;
use core_engine::backend::{MidiBackend, MidiOut, MidiPortInfo};
use core_engine::schedule::targets::find_port;
use objc2_core_audio::{AudioConvertHostTimeToNanos, AudioConvertNanosToHostTime};
use objc2_core_foundation::{CFRetained, CFString};
use objc2_core_midi::{
    MIDIClientCreate, MIDIClientDispose, MIDIClientRef, MIDIEndpointDispose, MIDIEndpointRef,
    MIDIEventList, MIDIEventPacket, MIDIGetDestination, MIDIGetNumberOfDestinations,
    MIDIInputPortCreateWithProtocol, MIDIObjectGetIntegerProperty, MIDIObjectGetStringProperty,
    MIDIObjectRef, MIDIOutputPortCreate, MIDIPortConnectSource, MIDIPortDispose, MIDIPortRef,
    MIDIProtocolID, MIDIReceivedEventList, MIDISendEventList, MIDISourceCreateWithProtocol,
    kMIDIPropertyName, kMIDIPropertyUniqueID,
};
use rtrb::{Consumer, Producer, RingBuffer};
use std::ffi::c_void;
use std::ptr::{self, NonNull};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

/// Packets per output ring.
pub const MIDI_RING_CAPACITY: usize = 4096;

/// Packet kinds: a MIDI 1.0 channel-voice message (UMP type 2) or one
/// 6-byte SysEx7 chunk (UMP type 3) with its status.
const KIND_CHANNEL: u8 = 0x20;
const KIND_SYSEX: u8 = 0x30;
const SYSEX_COMPLETE: u8 = 0;
const SYSEX_START: u8 = 1;
const SYSEX_CONTINUE: u8 = 2;
const SYSEX_END: u8 = 3;

/// One ring entry: at most one UMP's worth of MIDI 1.0 data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Packet {
    host_ns: u64,
    kind: u8,
    len: u8,
    data: [u8; 6],
}

impl Packet {
    /// The UMP words of this packet on group 0.
    fn words(&self) -> ([u32; 2], u32) {
        let d = self.data;
        if self.kind == KIND_CHANNEL {
            let w =
                (0x2 << 28) | (u32::from(d[0]) << 16) | (u32::from(d[1]) << 8) | u32::from(d[2]);
            return ([w, 0], 1);
        }
        let status = u32::from(self.kind & 0x0F);
        let w0 = (0x3 << 28)
            | (status << 20)
            | (u32::from(self.len) << 16)
            | (u32::from(d[0]) << 8)
            | u32::from(d[1]);
        let w1 = (u32::from(d[2]) << 24)
            | (u32::from(d[3]) << 16)
            | (u32::from(d[4]) << 8)
            | u32::from(d[5]);
        ([w0, w1], 2)
    }
}

/// The wait-free side of an output.
pub struct RingMidiOut {
    tx: Producer<Packet>,
}

impl MidiOut for RingMidiOut {
    fn send(&mut self, host_time_ns: u64, msg: &[u8]) -> bool {
        match msg {
            [0xF0, rest @ ..] => {
                let payload = match rest {
                    [body @ .., 0xF7] => body,
                    body => body,
                };
                let n = payload.len().div_ceil(6).max(1);
                if self.tx.slots() < n {
                    return false;
                }
                let mut chunks = payload.chunks(6);
                for i in 0..n {
                    let chunk = chunks.next().unwrap_or(&[]);
                    let status = match (n, i) {
                        (1, _) => SYSEX_COMPLETE,
                        (_, 0) => SYSEX_START,
                        (_, i) if i + 1 == n => SYSEX_END,
                        _ => SYSEX_CONTINUE,
                    };
                    let mut data = [0u8; 6];
                    for (d, s) in data.iter_mut().zip(chunk) {
                        *d = *s & 0x7F;
                    }
                    let p = Packet {
                        host_ns: host_time_ns,
                        kind: KIND_SYSEX | status,
                        len: chunk.len() as u8,
                        data,
                    };
                    if self.tx.push(p).is_err() {
                        return false;
                    }
                }
                true
            }
            [status, rest @ ..] if *status >= 0x80 => {
                let mut data = [0u8; 6];
                data[0] = *status;
                for (d, s) in data.iter_mut().skip(1).zip(rest.iter().take(2)) {
                    *d = *s;
                }
                let p = Packet {
                    host_ns: host_time_ns,
                    kind: KIND_CHANNEL,
                    len: msg.len().min(3) as u8,
                    data,
                };
                self.tx.push(p).is_ok()
            }
            _ => true,
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum Dest {
    /// Our virtual source: `MIDIReceivedEventList`.
    Source(MIDIEndpointRef),
    /// A destination through our output port: `MIDISendEventList`.
    Port(MIDIPortRef, MIDIEndpointRef),
}

struct Lane {
    rx: Consumer<Packet>,
    dest: Dest,
}

#[derive(Default)]
struct SenderState {
    lanes: Mutex<Vec<Lane>>,
    stop: AtomicBool,
}

fn os(call: &'static str, status: i32) -> Result<(), BackendError> {
    if status == 0 {
        Ok(())
    } else {
        Err(BackendError::Os { call, status })
    }
}

/// Sends one packet as a one-packet `MIDIEventList`.
fn deliver(dest: Dest, p: &Packet) {
    let (w, count) = p.words();
    let mut words = [0u32; 64];
    words[0] = w[0];
    words[1] = w[1];
    let mut list = MIDIEventList {
        protocol: MIDIProtocolID::Protocol_1_0,
        numPackets: 1,
        packet: [MIDIEventPacket {
            timeStamp: ns_to_host(p.host_ns),
            wordCount: count,
            words,
        }],
    };
    let list_ptr = NonNull::from(&mut list);
    // SAFETY: `list` is a valid one-packet event list for the duration of
    // the call; the endpoint and port belong to our live client.
    unsafe {
        match dest {
            Dest::Source(src) => {
                MIDIReceivedEventList(src, list_ptr);
            }
            Dest::Port(port, endpoint) => {
                MIDISendEventList(port, endpoint, list_ptr);
            }
        }
    }
}

fn sender_loop(state: Arc<SenderState>) {
    while !state.stop.load(Ordering::Relaxed) {
        if let Ok(mut lanes) = state.lanes.lock() {
            for lane in lanes.iter_mut() {
                while let Ok(p) = lane.rx.pop() {
                    deliver(lane.dest, &p);
                }
            }
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn cfstring_property(obj: MIDIObjectRef, key: &CFString) -> Option<String> {
    let mut out: *const CFString = ptr::null();
    // SAFETY: `out` is a valid out-pointer; on success CoreMIDI returns a
    // +1 retained string, which `CFRetained::from_raw` takes over.
    let status = unsafe { MIDIObjectGetStringProperty(obj, key, NonNull::from(&mut out)) };
    let p = NonNull::new(out.cast_mut())?;
    if status != 0 {
        return None;
    }
    // SAFETY: see above.
    let s: CFRetained<CFString> = unsafe { CFRetained::from_raw(p) };
    Some(s.to_string())
}

fn int_property(obj: MIDIObjectRef, key: &CFString) -> Option<i32> {
    let mut v = 0i32;
    // SAFETY: `v` is a valid out-pointer.
    let status = unsafe { MIDIObjectGetIntegerProperty(obj, key, NonNull::from(&mut v)) };
    (status == 0).then_some(v)
}

fn destination_endpoints() -> Vec<(MIDIEndpointRef, MidiPortInfo)> {
    // SAFETY: plain queries of the MIDI server.
    let n = unsafe { MIDIGetNumberOfDestinations() };
    (0..n)
        .filter_map(|i| {
            // SAFETY: `i` is below the count just read.
            let e = unsafe { MIDIGetDestination(i) };
            if e == 0 {
                return None;
            }
            // SAFETY: the property keys are static CFStrings.
            let (name_key, id_key) = unsafe { (kMIDIPropertyName, kMIDIPropertyUniqueID) };
            let name = cfstring_property(e, name_key)?;
            Some((
                e,
                MidiPortInfo {
                    name,
                    unique_id: int_property(e, id_key),
                },
            ))
        })
        .collect()
}

/// Destinations visible to the system (no client needed).
pub fn list_destinations() -> Vec<MidiPortInfo> {
    destination_endpoints()
        .into_iter()
        .map(|(_, p)| p)
        .collect()
}

/// One CoreMIDI client plus the sender thread for every output it opened.
pub struct MidiSystem {
    client: MIDIClientRef,
    out_port: Option<MIDIPortRef>,
    sources: Vec<(String, MIDIEndpointRef)>,
    state: Arc<SenderState>,
    thread: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for MidiSystem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MidiSystem")
            .field("client", &self.client)
            .finish_non_exhaustive()
    }
}

impl MidiSystem {
    /// Creates the process's MIDI client (control thread) and starts the
    /// sender thread.
    pub fn new(name: &str) -> Result<MidiSystem, BackendError> {
        let mut client: MIDIClientRef = 0;
        let cf = CFString::from_str(name);
        // SAFETY: `client` is a valid out-pointer; no notification callback.
        os("MIDIClientCreate", unsafe {
            MIDIClientCreate(&cf, None, ptr::null_mut(), NonNull::from(&mut client))
        })?;
        let state = Arc::new(SenderState::default());
        let s = Arc::clone(&state);
        let thread = std::thread::Builder::new()
            .name("vizdaw-midi-out".into())
            .spawn(move || sender_loop(s))
            .map_err(|_| BackendError::Os {
                call: "spawn MIDI sender",
                status: -1,
            })?;
        Ok(MidiSystem {
            client,
            out_port: None,
            sources: Vec::new(),
            state,
            thread: Some(thread),
        })
    }

    fn add_lane(&mut self, dest: Dest) -> Box<dyn MidiOut> {
        let (tx, rx) = RingBuffer::new(MIDI_RING_CAPACITY);
        if let Ok(mut lanes) = self.state.lanes.lock() {
            lanes.push(Lane { rx, dest });
        }
        Box::new(RingMidiOut { tx })
    }

    /// The endpoint of a virtual source this system created.
    fn source(&self, name: &str) -> Option<MIDIEndpointRef> {
        self.sources
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, e)| *e)
    }
}

impl MidiBackend for MidiSystem {
    fn destinations(&self) -> Vec<MidiPortInfo> {
        list_destinations()
    }

    fn open_virtual_source(&mut self, name: &str) -> Result<Box<dyn MidiOut>, BackendError> {
        let endpoint = match self.source(name) {
            Some(e) => e,
            None => {
                let mut e: MIDIEndpointRef = 0;
                let cf = CFString::from_str(name);
                // SAFETY: valid client and out-pointer.
                os("MIDISourceCreateWithProtocol", unsafe {
                    MIDISourceCreateWithProtocol(
                        self.client,
                        &cf,
                        MIDIProtocolID::Protocol_1_0,
                        NonNull::from(&mut e),
                    )
                })?;
                self.sources.push((name.to_string(), e));
                e
            }
        };
        Ok(self.add_lane(Dest::Source(endpoint)))
    }

    fn open_destination(&mut self, port: &MidiPortInfo) -> Result<Box<dyn MidiOut>, BackendError> {
        let all = destination_endpoints();
        let infos: Vec<MidiPortInfo> = all.iter().map(|(_, p)| p.clone()).collect();
        let found = find_port(&port.name, &infos)
            .or_else(|| {
                infos
                    .iter()
                    .find(|p| p.unique_id.is_some() && p.unique_id == port.unique_id)
            })
            .and_then(|p| all.iter().find(|(_, q)| q == p))
            .map(|(e, _)| *e)
            .ok_or(BackendError::NoDevice)?;
        let out_port = match self.out_port {
            Some(p) => p,
            None => {
                let mut p: MIDIPortRef = 0;
                let cf = CFString::from_str("VizDAW Output");
                // SAFETY: valid client and out-pointer.
                os("MIDIOutputPortCreate", unsafe {
                    MIDIOutputPortCreate(self.client, &cf, NonNull::from(&mut p))
                })?;
                self.out_port = Some(p);
                p
            }
        };
        Ok(self.add_lane(Dest::Port(out_port, found)))
    }
}

impl Drop for MidiSystem {
    fn drop(&mut self) {
        self.state.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        // SAFETY: the objects belong to this client and are not used after
        // this point (the sender thread has stopped).
        unsafe {
            for (_, e) in &self.sources {
                MIDIEndpointDispose(*e);
            }
            if let Some(p) = self.out_port {
                MIDIPortDispose(p);
            }
            MIDIClientDispose(self.client);
        }
    }
}

/// A received MIDI 1.0 message with its timestamp in host nanoseconds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Received {
    /// Timestamp in host-clock ticks, as delivered.
    pub host_time: u64,
    /// Timestamp, converted to nanoseconds.
    pub host_ns: u64,
    /// The message bytes (`F0 … F7` for SysEx).
    pub bytes: Vec<u8>,
}

/// Listens to one of the system's own virtual sources (tests and
/// diagnostics; MIDI input proper is a later sub-project).
pub struct MidiMonitor {
    port: MIDIPortRef,
    received: Arc<Mutex<Vec<Received>>>,
    _block: RcBlock<dyn Fn(NonNull<MIDIEventList>, *mut c_void)>,
}

fn channel_len(status: u8) -> usize {
    match status & 0xF0 {
        0xC0 | 0xD0 => 2,
        _ => 3,
    }
}

/// Size in words of a UMP of message type `t`.
fn ump_words(t: u32) -> usize {
    match t {
        0x0 | 0x1 | 0x2 | 0x6 | 0x7 => 1,
        0x3 | 0x4 | 0x8 | 0x9 | 0xA => 2,
        0xB | 0xC => 3,
        _ => 4,
    }
}

/// Decodes the type-2 and type-3 UMPs of an event list.
fn decode(list: &MIDIEventList, out: &mut Vec<Received>, sysex: &mut Vec<u8>) {
    let mut p: *const MIDIEventPacket = list.packet.as_ptr();
    for _ in 0..list.numPackets {
        // SAFETY: CoreMIDI guarantees `numPackets` consecutive packets, each
        // followed by the next at `words[wordCount]` (MIDIEventPacketNext);
        // packets are 4-byte aligned, so they are read unaligned-safely.
        let pkt = unsafe { ptr::read_unaligned(p) };
        // SAFETY: pure conversion.
        let host_ns = unsafe { AudioConvertHostTimeToNanos(pkt.timeStamp) };
        let words = pkt.words;
        let n = (pkt.wordCount as usize).min(64);
        let mut i = 0;
        while i < n {
            let w0 = words.get(i).copied().unwrap_or(0);
            match w0 >> 28 {
                0x2 => {
                    let b = [(w0 >> 16) as u8, (w0 >> 8) as u8 & 0x7F, w0 as u8 & 0x7F];
                    let len = channel_len(b[0]);
                    out.push(Received {
                        host_time: pkt.timeStamp,
                        host_ns,
                        bytes: b.get(..len).unwrap_or(&b).to_vec(),
                    });
                    i += 1;
                }
                0x3 => {
                    let w1 = words.get(i + 1).copied().unwrap_or(0);
                    let status = (w0 >> 20) & 0xF;
                    let count = ((w0 >> 16) & 0xF) as usize;
                    let bytes = [
                        (w0 >> 8) as u8,
                        w0 as u8,
                        (w1 >> 24) as u8,
                        (w1 >> 16) as u8,
                        (w1 >> 8) as u8,
                        w1 as u8,
                    ];
                    if u32::from(SYSEX_COMPLETE) == status || u32::from(SYSEX_START) == status {
                        sysex.clear();
                        sysex.push(0xF0);
                    }
                    sysex.extend_from_slice(bytes.get(..count.min(6)).unwrap_or(&[]));
                    if u32::from(SYSEX_COMPLETE) == status || u32::from(SYSEX_END) == status {
                        sysex.push(0xF7);
                        out.push(Received {
                            host_time: pkt.timeStamp,
                            host_ns,
                            bytes: std::mem::take(sysex),
                        });
                    }
                    i += 2;
                }
                t => i += ump_words(t),
            }
        }
        // SAFETY: see above; stays within the list CoreMIDI handed us.
        p = unsafe {
            (p.cast::<u8>())
                .add(std::mem::offset_of!(MIDIEventPacket, words) + n * 4)
                .cast::<MIDIEventPacket>()
        };
    }
}

impl MidiMonitor {
    /// Connects an input port of `system`'s client to its virtual source
    /// `name` (opened earlier with `open_virtual_source`).
    pub fn watch(system: &MidiSystem, name: &str) -> Result<MidiMonitor, BackendError> {
        let source = system.source(name).ok_or(BackendError::NoDevice)?;
        let received = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&received);
        let pending = Mutex::new(Vec::new());
        let block = RcBlock::new(move |list: NonNull<MIDIEventList>, _src: *mut c_void| {
            // SAFETY: CoreMIDI passes a valid event list for the call.
            let list = unsafe { list.as_ref() };
            if let (Ok(mut out), Ok(mut sx)) = (sink.lock(), pending.lock()) {
                decode(list, &mut out, &mut sx);
            }
        });
        let mut port: MIDIPortRef = 0;
        let cf = CFString::from_str("VizDAW Monitor");
        // SAFETY: valid client, out-pointer and block; CoreMIDI copies the
        // block, and we keep ours alive for the port's lifetime anyway.
        os("MIDIInputPortCreateWithProtocol", unsafe {
            MIDIInputPortCreateWithProtocol(
                system.client,
                &cf,
                MIDIProtocolID::Protocol_1_0,
                NonNull::from(&mut port),
                (&*block as *const block2::DynBlock<_>).cast_mut(),
            )
        })?;
        // SAFETY: both objects are live and belong to our client.
        os("MIDIPortConnectSource", unsafe {
            MIDIPortConnectSource(port, source, ptr::null_mut())
        })?;
        Ok(MidiMonitor {
            port,
            received,
            _block: block,
        })
    }

    /// Everything received so far.
    pub fn take(&self) -> Vec<Received> {
        self.received
            .lock()
            .map(|mut v| std::mem::take(&mut *v))
            .unwrap_or_default()
    }
}

impl Drop for MidiMonitor {
    fn drop(&mut self) {
        // SAFETY: the port belongs to this monitor and is not used again.
        unsafe {
            MIDIPortDispose(self.port);
        }
    }
}

/// Nanoseconds to host-clock ticks (the timestamps CoreMIDI carries).
pub fn ns_to_host(ns: u64) -> u64 {
    // SAFETY: pure conversion.
    unsafe { AudioConvertNanosToHostTime(ns) }
}

/// Current host time in nanoseconds.
pub fn now_ns() -> u64 {
    // SAFETY: pure queries.
    unsafe { AudioConvertHostTimeToNanos(objc2_core_audio::AudioGetCurrentHostTime()) }
}
