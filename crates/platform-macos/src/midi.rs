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
    MIDIGetNumberOfSources, MIDIGetSource, MIDIInputPortCreateWithProtocol,
    MIDIObjectGetIntegerProperty, MIDIObjectGetStringProperty, MIDIObjectRef, MIDIOutputPortCreate,
    MIDIPortConnectSource, MIDIPortDispose, MIDIPortRef, MIDIProtocolID, MIDIReceivedEventList,
    MIDISendEventList, MIDISourceCreateWithProtocol, kMIDIPropertyName, kMIDIPropertyUniqueID,
};
use rtrb::{Consumer, Producer, RingBuffer};
use std::ffi::c_void;
use std::ptr::{self, NonNull};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

/// Virtual sources this process created (endpoint, unique ID, name), so
/// note input never listens to its own output (a feedback loop).
static OWN_SOURCES: Mutex<Vec<(MIDIEndpointRef, Option<i32>, String)>> = Mutex::new(Vec::new());

fn register_own_source(e: MIDIEndpointRef, name: &str) {
    // SAFETY: the property key is a static CFString.
    let id = int_property(e, unsafe { kMIDIPropertyUniqueID });
    if let Ok(mut own) = OWN_SOURCES.lock() {
        own.push((e, id, name.to_string()));
    }
}

fn unregister_own_source(e: MIDIEndpointRef) {
    if let Ok(mut own) = OWN_SOURCES.lock() {
        own.retain(|(o, _, _)| *o != e);
    }
}

/// Whether a source (its endpoint ref, unique ID and name) is one of this
/// process's own virtual sources. The ref or unique ID decides when known;
/// the name is the fallback when the source's unique ID can't be read.
fn is_own_source(
    e: MIDIEndpointRef,
    id: Option<i32>,
    name: Option<&str>,
    own: &[(MIDIEndpointRef, Option<i32>, String)],
) -> bool {
    own.iter().any(|(oe, oid, oname)| {
        *oe == e
            || (id.is_some() && id == *oid)
            || (id.is_none() && name.is_some_and(|n| n == oname))
    })
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dest {
    /// Our virtual source: `MIDIReceivedEventList`.
    Source(MIDIEndpointRef),
    /// A destination through our output port: `MIDISendEventList`.
    Port(MIDIPortRef, MIDIEndpointRef),
}

struct Lane {
    rx: Consumer<Packet>,
    dest: Dest,
    /// Latest timestamp handed to CoreMIDI from this lane.
    latest_ns: u64,
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

/// Delivers everything queued on every lane.
fn drain_lanes(lanes: &mut [Lane], send: &mut impl FnMut(Dest, &Packet)) {
    for lane in lanes.iter_mut() {
        while let Ok(p) = lane.rx.pop() {
            lane.latest_ns = lane.latest_ns.max(p.host_ns);
            send(lane.dest, &p);
        }
    }
}

/// Final drain, then All Notes Off (CC123) on all 16 channels of every
/// distinct output, so nothing is left sounding. The CC123s are stamped no
/// earlier than the latest packet delivered to that output (0, "now", when
/// there was none), so a note-on scheduled ahead cannot land after them.
fn shutdown_lanes(lanes: &mut [Lane], mut send: impl FnMut(Dest, &Packet)) {
    drain_lanes(lanes, &mut send);
    let mut done: Vec<Dest> = Vec::with_capacity(lanes.len());
    for lane in lanes.iter() {
        if done.contains(&lane.dest) {
            continue;
        }
        done.push(lane.dest);
        let at = lanes
            .iter()
            .filter(|l| l.dest == lane.dest)
            .map(|l| l.latest_ns)
            .max()
            .unwrap_or(0);
        for ch in 0..16u8 {
            let p = Packet {
                host_ns: at,
                kind: KIND_CHANNEL,
                len: 3,
                data: [0xB0 | ch, 123, 0, 0, 0, 0],
            };
            send(lane.dest, &p);
        }
    }
}

fn sender_loop(state: Arc<SenderState>) {
    while !state.stop.load(Ordering::Relaxed) {
        if let Ok(mut lanes) = state.lanes.lock() {
            drain_lanes(&mut lanes, &mut deliver);
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    if let Ok(mut lanes) = state.lanes.lock() {
        shutdown_lanes(&mut lanes, deliver);
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
            lanes.push(Lane {
                rx,
                dest,
                latest_ns: 0,
            });
        }
        Box::new(RingMidiOut { tx })
    }

    /// The unique ID of a virtual source this system created.
    pub fn virtual_source_unique_id(&self, name: &str) -> Option<i32> {
        // SAFETY: the property key is a static CFString.
        self.source(name)
            .and_then(|e| int_property(e, unsafe { kMIDIPropertyUniqueID }))
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
                register_own_source(e, name);
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
                unregister_own_source(*e);
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
///
/// `list` points at an event list CoreMIDI handed us. Packets are variable
/// length (`wordCount` words, not the declared 64), so no packet or list is
/// ever read as a whole struct.
fn decode(list: NonNull<MIDIEventList>, out: &mut Vec<Received>, sysex: &mut Vec<u8>) {
    let list = list.as_ptr();
    // SAFETY: `list` is a valid event list for the call; the header fields
    // are read through raw field pointers without forming a reference.
    let (num, mut p) = unsafe {
        (
            ptr::read_unaligned(ptr::addr_of!((*list).numPackets)),
            ptr::addr_of!((*list).packet).cast::<MIDIEventPacket>(),
        )
    };
    for _ in 0..num {
        // SAFETY: CoreMIDI guarantees `numPackets` consecutive packets, each
        // with a timestamp, a word count and `wordCount` words, followed by
        // the next packet (MIDIEventPacketNext). Only those fields are read,
        // through raw field pointers; packets are 4-byte aligned, so the
        // u64 timestamp is read unaligned and the u32 words are aligned.
        let (time_stamp, words) = unsafe {
            let ts = ptr::read_unaligned(ptr::addr_of!((*p).timeStamp));
            let wc = ptr::read_unaligned(ptr::addr_of!((*p).wordCount));
            let n = (wc as usize).min(64);
            let w = ptr::addr_of!((*p).words).cast::<u32>();
            (ts, std::slice::from_raw_parts(w, n))
        };
        // SAFETY: pure conversion.
        let host_ns = unsafe { AudioConvertHostTimeToNanos(time_stamp) };
        let n = words.len();
        let mut i = 0;
        while i < n {
            let w0 = words.get(i).copied().unwrap_or(0);
            match w0 >> 28 {
                0x2 => {
                    let b = [(w0 >> 16) as u8, (w0 >> 8) as u8 & 0x7F, w0 as u8 & 0x7F];
                    let len = channel_len(b[0]);
                    out.push(Received {
                        host_time: time_stamp,
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
                            host_time: time_stamp,
                            host_ns,
                            bytes: std::mem::take(sysex),
                        });
                    }
                    i += 2;
                }
                t => i += ump_words(t),
            }
        }
        // The next packet starts right after this one's `n` words
        // (MIDIEventPacketNext); for the last packet the address is
        // computed but never read, so plain wrapping arithmetic suffices.
        p = p
            .cast::<u8>()
            .wrapping_add(std::mem::offset_of!(MIDIEventPacket, words) + n * 4)
            .cast::<MIDIEventPacket>();
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

/// What a [`NoteListener`] hands each received MIDI 1.0 message to (on
/// CoreMIDI's thread). It must not block.
pub type MessageSink = Box<dyn Fn(&[u8]) + Send + Sync>;

/// Listens to every MIDI source present when it connects (plan 4c: notes
/// played into the list editor). It has its own client, so output is never
/// disturbed; sources added later are picked up by connecting again.
pub struct NoteListener {
    client: MIDIClientRef,
    port: MIDIPortRef,
    sources: u32,
    source_ids: Vec<Option<i32>>,
    _block: RcBlock<dyn Fn(NonNull<MIDIEventList>, *mut c_void)>,
}

// SAFETY: the client and port are plain CoreMIDI object references, usable
// from any thread; the block is only called by CoreMIDI (its captures are
// `Send + Sync`) and is dropped with the listener after the port is
// disposed.
unsafe impl Send for NoteListener {}

impl NoteListener {
    /// Creates client `name` and connects it to every source present now,
    /// except this process's own virtual sources (listening to "VizDAW
    /// Out" would feed played notes back into the input).
    pub fn connect_all(name: &str, sink: MessageSink) -> Result<NoteListener, BackendError> {
        NoteListener::connect(name, sink, false)
    }

    /// Like [`NoteListener::connect_all`] but also listening to this
    /// process's own virtual sources (tests only: it is how a test can play
    /// notes into a listener without an external device).
    #[doc(hidden)]
    pub fn connect_including_own(
        name: &str,
        sink: MessageSink,
    ) -> Result<NoteListener, BackendError> {
        NoteListener::connect(name, sink, true)
    }

    fn connect(
        name: &str,
        sink: MessageSink,
        include_own: bool,
    ) -> Result<NoteListener, BackendError> {
        let mut client: MIDIClientRef = 0;
        let cf = CFString::from_str(name);
        // SAFETY: `client` is a valid out-pointer; no notification callback.
        os("MIDIClientCreate", unsafe {
            MIDIClientCreate(&cf, None, ptr::null_mut(), NonNull::from(&mut client))
        })?;
        // Scratch buffers reused across callbacks (CoreMIDI calls the block
        // from one thread at a time, so the lock is never contended).
        let scratch = Mutex::new((Vec::<Received>::new(), Vec::<u8>::new()));
        let block = RcBlock::new(move |list: NonNull<MIDIEventList>, _src: *mut c_void| {
            if let Ok(mut s) = scratch.try_lock() {
                let (out, sx) = &mut *s;
                out.clear();
                decode(list, out, sx);
                for r in out.iter() {
                    sink(&r.bytes);
                }
            }
        });
        let mut port: MIDIPortRef = 0;
        let pcf = CFString::from_str("VizDAW Notes In");
        // SAFETY: valid client, out-pointer and block; CoreMIDI copies the
        // block, and ours lives as long as the port anyway.
        let made = os("MIDIInputPortCreateWithProtocol", unsafe {
            MIDIInputPortCreateWithProtocol(
                client,
                &pcf,
                MIDIProtocolID::Protocol_1_0,
                NonNull::from(&mut port),
                (&*block as *const block2::DynBlock<_>).cast_mut(),
            )
        });
        if let Err(e) = made {
            // SAFETY: the client was created above and is not used again.
            unsafe {
                MIDIClientDispose(client);
            }
            return Err(e);
        }
        // SAFETY: a pure query.
        let n = unsafe { MIDIGetNumberOfSources() };
        let mut sources = 0u32;
        let mut source_ids = Vec::new();
        let own = OWN_SOURCES.lock().map(|o| o.clone()).unwrap_or_default();
        for i in 0..n {
            // SAFETY: `i` is below the count just read.
            let src = unsafe { MIDIGetSource(i) };
            if src == 0 {
                continue;
            }
            // SAFETY: the property keys are static CFStrings.
            let (name_key, id_key) = unsafe { (kMIDIPropertyName, kMIDIPropertyUniqueID) };
            let id = int_property(src, id_key);
            if !include_own {
                let sname = if id.is_none() {
                    cfstring_property(src, name_key)
                } else {
                    None
                };
                if is_own_source(src, id, sname.as_deref(), &own) {
                    continue;
                }
            }
            // SAFETY: both objects are live; the port belongs to our client.
            let ok = os("MIDIPortConnectSource", unsafe {
                MIDIPortConnectSource(port, src, ptr::null_mut())
            });
            if ok.is_ok() {
                sources = sources.saturating_add(1);
                source_ids.push(id);
            }
        }
        Ok(NoteListener {
            client,
            port,
            sources,
            source_ids,
            _block: block,
        })
    }

    /// How many sources it listens to.
    pub fn sources(&self) -> u32 {
        self.sources
    }

    /// The unique IDs of the sources it listens to (`None` when a source's
    /// ID couldn't be read).
    pub fn source_ids(&self) -> &[Option<i32>] {
        &self.source_ids
    }
}

impl Drop for NoteListener {
    fn drop(&mut self) {
        // SAFETY: the port and client belong to this listener and are not
        // used again.
        unsafe {
            MIDIPortDispose(self.port);
            MIDIClientDispose(self.client);
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

#[cfg(test)]
mod tests {
    use super::*;

    unsafe extern "C" {
        fn mmap(
            addr: *mut c_void,
            len: usize,
            prot: i32,
            flags: i32,
            fd: i32,
            off: i64,
        ) -> *mut c_void;
        fn mprotect(addr: *mut c_void, len: usize, prot: i32) -> i32;
        fn munmap(addr: *mut c_void, len: usize) -> i32;
    }
    const PROT_NONE: i32 = 0;
    const PROT_RW: i32 = 1 | 2;
    const MAP_PRIVATE_ANON: i32 = 0x0002 | 0x1000;
    const HALF: usize = 64 * 1024;

    /// Decodes `words` (an event list as CoreMIDI lays it out) placed so
    /// that the list ends exactly where an unreadable guard page begins:
    /// reading one byte past the list crashes the test.
    fn decode_at_guard(words: &[u32]) -> Vec<Received> {
        // SAFETY: plain anonymous mapping, unmapped below.
        let base = unsafe { mmap(ptr::null_mut(), 2 * HALF, PROT_RW, MAP_PRIVATE_ANON, -1, 0) };
        assert!(!base.is_null() && base as isize != -1);
        // SAFETY: the upper half belongs to the mapping.
        let guard = unsafe { base.cast::<u8>().add(HALF) };
        assert_eq!(unsafe { mprotect(guard.cast(), HALF, PROT_NONE) }, 0);
        let bytes = words.len() * 4;
        // SAFETY: `bytes` < HALF, so the list lies in the readable half.
        let start = unsafe { guard.sub(bytes) }.cast::<u32>();
        unsafe { ptr::copy_nonoverlapping(words.as_ptr(), start, words.len()) };
        let mut out = Vec::new();
        let mut sysex = Vec::new();
        let list = NonNull::new(start.cast::<MIDIEventList>()).unwrap();
        decode(list, &mut out, &mut sysex);
        // SAFETY: the mapping is ours and nothing points into it any more.
        unsafe { munmap(base, 2 * HALF) };
        out
    }

    fn lane(dest: Dest) -> (Lane, RingMidiOut) {
        let (tx, rx) = RingBuffer::new(16);
        (
            Lane {
                rx,
                dest,
                latest_ns: 0,
            },
            RingMidiOut { tx },
        )
    }

    #[test]
    fn shutdown_drains_queued_packets_then_silences_every_output() {
        let (a, mut out_a) = lane(Dest::Source(1));
        let (b, _out_b) = lane(Dest::Source(2));
        let (a2, mut out_a2) = lane(Dest::Source(1));
        assert!(out_a.send(5, &[0x90, 60, 100]));
        assert!(out_a.send(6, &[0x80, 60, 0]));
        assert!(out_a2.send(7, &[0x91, 62, 100]));
        let mut lanes = vec![a, b, a2];
        let mut sent: Vec<(Dest, u64, Vec<u8>)> = Vec::new();
        shutdown_lanes(&mut lanes, |d, p| {
            let (w, _) = p.words();
            let bytes = [(w[0] >> 16) as u8, (w[0] >> 8) as u8, w[0] as u8];
            sent.push((d, p.host_ns, bytes.to_vec()));
        });
        let mut want = vec![
            (Dest::Source(1), 5, vec![0x90, 60, 100]),
            (Dest::Source(1), 6, vec![0x80, 60, 0]),
            (Dest::Source(1), 7, vec![0x91, 62, 100]),
        ];
        // All notes off on every channel, once per distinct output, no
        // earlier than the latest packet queued for that output (an output
        // with nothing queued gets "now", 0).
        for (d, at) in [(Dest::Source(1), 7), (Dest::Source(2), 0)] {
            for ch in 0..16u8 {
                want.push((d, at, vec![0xB0 | ch, 123, 0]));
            }
        }
        assert_eq!(sent, want);
    }

    #[test]
    fn shutdown_stamps_after_packets_delivered_by_earlier_drains() {
        let (a, mut out_a) = lane(Dest::Source(1));
        let mut lanes = vec![a];
        // A note scheduled ahead, delivered by a regular drain…
        assert!(out_a.send(1_000, &[0x90, 60, 100]));
        drain_lanes(&mut lanes, &mut |_, _| {});
        // …then an earlier-stamped packet left for the final drain.
        assert!(out_a.send(500, &[0x80, 61, 0]));
        let mut stamps: Vec<(u64, u8)> = Vec::new();
        shutdown_lanes(&mut lanes, |_, p| stamps.push((p.host_ns, p.data[1])));
        assert_eq!(stamps.first(), Some(&(500, 61)));
        assert_eq!(stamps.len(), 17);
        assert!(
            stamps
                .iter()
                .skip(1)
                .all(|&(t, cc)| t == 1_000 && cc == 123)
        );
    }

    #[test]
    fn own_sources_match_by_ref_id_or_name_fallback() {
        let own = vec![(7, Some(42), "VizDAW Out".to_string())];
        assert!(is_own_source(7, None, None, &own));
        assert!(is_own_source(8, Some(42), Some("Other"), &own));
        assert!(is_own_source(9, None, Some("VizDAW Out"), &own));
        assert!(!is_own_source(9, Some(43), Some("VizDAW Out"), &own));
        assert!(!is_own_source(9, None, Some("Keyboard"), &own));
        assert!(!is_own_source(9, Some(42), None, &[]));
    }

    #[test]
    fn short_packets_are_read_only_up_to_their_word_count() {
        // protocol, numPackets = 2,
        // packet 1: timeStamp 0 (2 words), wordCount 1, note on;
        // packet 2: timeStamp 0, wordCount 1, note off.
        let list = [1, 2, 0, 0, 1, 0x2090_3C40, 0, 0, 1, 0x2080_3C00];
        let got: Vec<Vec<u8>> = decode_at_guard(&list)
            .into_iter()
            .map(|r| r.bytes)
            .collect();
        assert_eq!(got, vec![vec![0x90, 0x3C, 0x40], vec![0x80, 0x3C, 0x00]]);
    }
}
