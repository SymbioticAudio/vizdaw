//! The process-wide engine host: one engine, one output, one playing
//! session at a time. Created lazily on the first Play. The engine plays
//! the inserts and plugin instruments of the session that played last
//! (its plugin host is the engine's insert factory, Plan Ruling 2).

use crate::error::VizError;
use crate::guard::lock;
use crate::plugins;
use core_engine::backend::{AudioBackend, BackendEvent};
use core_engine::engine::{Engine, EngineConfig, PendingPlay};
use core_engine::graph::NoInserts;
use core_engine::input::{InputSpec, MidiIn, MidiInputSpec, MidiSender, MonitorRoute, port_key};
use core_engine::measure::MeasureStatus;
use core_engine::offline::{EventLog, FakeInput, FakeSynth, OfflineBackend, OfflineSinks};
use core_engine::record::{RecordHandles, RecordRequest};
use core_engine::render::Renderer;
use core_engine::schedule::CompileReport;
use core_engine::transport::PlayState;
use core_host::{IdleReport, PluginHost};
use core_model::document::Document;
use core_model::ids::{DestId, SeqId};
use core_model::lane::{LaneInput, MonitorMode};
use core_model::time::Tick;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// A rate offline hosts run at instead of the project's (f64 bits; 0:
/// follow the project, 5a Ruling 20).
static OFFLINE_RATE_BITS: AtomicU64 = AtomicU64::new(0);

/// The offline device's rate for a project at `project` Hz.
fn offline_rate(project: f64) -> f64 {
    let r = f64::from_bits(OFFLINE_RATE_BITS.load(Ordering::Relaxed));
    if r > 0.0 { r } else { project }
}

/// Makes offline hosts created from now on run at `rate` whatever the
/// project's rate (test hook: a device that refused it); 0 follows the
/// project again.
#[cfg(feature = "test-hooks")]
pub fn set_offline_rate(rate: f64) {
    OFFLINE_RATE_BITS.store(rate.to_bits(), Ordering::Relaxed);
}

/// Two rates are the same within half a hertz.
fn same_rate(a: f64, b: f64) -> bool {
    (a - b).abs() <= 0.5
}

/// The rate a host for `doc` asks the device for: the project rate.
fn project_rate(doc: &Document) -> f64 {
    f64::from(doc.settings().format.sample_rate)
}
/// Offline block size.
pub const OFFLINE_BLOCK: u32 = 512;
/// Device label of the offline backend.
pub const OFFLINE_DEVICE: &str = "Offline";

/// Why Play, Record and Measure wait while a measurement runs (plan 5c-2
/// Ruling 9).
pub const MEASURING: &str = "A synth's return latency is being measured; try again in a moment.";

/// Why Measure waits for the transport (plan 5c-2 Ruling 9).
pub const STOP_FIRST: &str = "Stop the transport first: Measure runs with it stopped.";

fn measuring_busy() -> VizError {
    VizError::Busy {
        message: MEASURING.into(),
    }
}

static HOST: Mutex<Option<Host>> = Mutex::new(None);
static FORCE_OFFLINE: AtomicBool = AtomicBool::new(false);
/// Engine loads made by the host (test hook counter).
static LOADS: AtomicU64 = AtomicU64::new(0);
static HOSTS_CREATED: AtomicU64 = AtomicU64::new(0);
/// Hands out [`Host::epoch`] values (process-wide, so a new host never
/// reuses an old one's).
static EPOCHS: AtomicU64 = AtomicU64::new(1);
/// New hosts' disks stall (the `stall_disk` test hook).
static STALL_DISK: AtomicBool = AtomicBool::new(false);
/// The fake input offline hosts get: `Some(choice)` set by the
/// `set_fake_input` test hook, `None` read from the environment.
static FAKE_INPUT: Mutex<Option<Option<FakeInput>>> = Mutex::new(None);

/// The fake input of offline hosts created from now on (spec §12.1):
/// `VIZDAW_AUDIO_LOOPBACK=<frames>` loops the output back, else
/// `VIZDAW_AUDIO_INPUT=fake` gives the test signals; neither is no input.
/// Tests choose with the `set_fake_input` hook instead. Never a device:
/// an offline host opens no audio hardware, so no microphone permission
/// is asked (plan 5c Global Constraints).
pub fn fake_input() -> Option<FakeInput> {
    fake_input_choice().0
}

/// The fake input and whether the test hook chose it, from one read of
/// the hook.
fn fake_input_choice() -> (Option<FakeInput>, bool) {
    let hook = *lock(&FAKE_INPUT);
    match hook {
        Some(v) => (v, true),
        None => (env_fake_input(), false),
    }
}

/// The fake input the environment asks for.
fn env_fake_input() -> Option<FakeInput> {
    if let Some(n) = std::env::var("VIZDAW_AUDIO_LOOPBACK")
        .ok()
        .and_then(|v| v.trim().parse::<u32>().ok())
    {
        return Some(FakeInput::Loopback(n));
    }
    std::env::var("VIZDAW_AUDIO_INPUT")
        .is_ok_and(|v| v == "fake")
        .then_some(FakeInput::Signals)
}

/// Makes offline hosts created from now on use `input` (test hook; it
/// wins over the environment).
#[cfg(feature = "test-hooks")]
pub fn set_fake_input(input: Option<FakeInput>) {
    *lock(&FAKE_INPUT) = Some(input);
}

/// The fake synth offline hosts get: `Some(choice)` set by the
/// `set_fake_synth` test hook, `None` read from the environment.
static FAKE_SYNTH: Mutex<Option<Option<f64>>> = Mutex::new(None);

/// The fake synth of offline hosts created from now on (spec §12.1;
/// plan 5c-2 Ruling 7): `VIZDAW_FAKE_SYNTH=<delay ms>` (0–500; anything
/// else is ignored) adds the MIDI destination "Fake Synth", which answers
/// every note on fake input channel 3 that long after it is heard. Only
/// with the signals input: with the loopback or no input the host has no
/// "Fake Synth" (nothing would play it). Tests choose with
/// `set_fake_synth`. Live hosts never have it.
///
/// The delay the synth really answers with is the backend's: a delay
/// below one block is allowed but answered one block late (a note is sent
/// during the block it plays in), and it grows to cover the largest block
/// rendered, plus the engine's negative MIDI offset when there is one, so
/// a note never sounds early. Read `OfflineBackend::synth_delay` after a
/// run, not this value, for the delay a measurement should find.
pub fn fake_synth() -> Option<f64> {
    fake_synth_choice().0
}

/// The fake synth's delay and whether the test hook chose it, from one
/// read of the hook.
fn fake_synth_choice() -> (Option<f64>, bool) {
    let hook = *lock(&FAKE_SYNTH);
    match hook {
        Some(v) => (v, true),
        None => (env_fake_synth(), false),
    }
}

/// The fake synth delay the environment asks for.
fn env_fake_synth() -> Option<f64> {
    let max = f64::from(core_model::document::MAX_RETURN_LATENCY_MS);
    std::env::var("VIZDAW_FAKE_SYNTH")
        .ok()
        .and_then(|v| v.trim().parse::<f64>().ok())
        .filter(|ms| ms.is_finite() && (0.0..=max).contains(ms))
}

/// Makes offline hosts created from now on have the fake synth answering
/// `delay_ms` late, or none (test hook; it wins over the environment).
#[cfg(feature = "test-hooks")]
pub fn set_fake_synth(delay_ms: Option<f64>) {
    *lock(&FAKE_SYNTH) = Some(delay_ms);
}

/// The fake input of a new offline host. The fake synth answers on the
/// signals input's channel 3, so it brings that input when none was
/// chosen (plan 5c-2 Ruling 7), unless the synth came from the
/// environment and the test hook chose the input: a `VIZDAW_FAKE_SYNTH`
/// left in a shell never overrides a test's explicit choice (pre-flight
/// M14).
fn offline_input(
    input: Option<FakeInput>,
    input_chosen: bool,
    synth: bool,
    synth_chosen: bool,
) -> Option<FakeInput> {
    match input {
        None if synth && (synth_chosen || !input_chosen) => Some(FakeInput::Signals),
        i => i,
    }
}

/// Where the CoreMIDI sources' messages go: the installed host's engine
/// (spec §6.6; set by `install`, Task 12 review M1).
static MIDI_TO: Mutex<Option<MidiSender>> = Mutex::new(None);

/// Source messages dropped because `MIDI_TO` was busy (pre-flight M8:
/// CoreMIDI's thread never waits for it).
static MIDI_SOURCE_DROPS: AtomicU64 = AtomicU64::new(0);

/// The record listener on the CoreMIDI sources (plan 5c Task 12): one
/// client and port per process, reconnected, never recreated (pre-flight
/// M7). Live builds only.
#[cfg(all(target_os = "macos", not(feature = "test-hooks")))]
static SOURCES: Mutex<Option<platform_macos::midi::SourceListener>> = Mutex::new(None);

/// A message from CoreMIDI source `port` at `host_ns` for the engine. It
/// runs on CoreMIDI's high-priority thread, so it never blocks or
/// allocates: a busy `MIDI_TO` drops the message and counts it
/// (pre-flight M8), and the sender itself only tries its lock.
#[cfg_attr(
    all(not(target_os = "macos"), not(feature = "test-hooks")),
    allow(dead_code)
)]
fn midi_from_source(port: u32, host_ns: u64, bytes: &[u8]) {
    let Some(msg) = core_engine::input::channel_message(bytes) else {
        return;
    };
    let to = match MIDI_TO.try_lock() {
        Ok(g) => g,
        Err(std::sync::TryLockError::Poisoned(p)) => p.into_inner(),
        Err(std::sync::TryLockError::WouldBlock) => {
            MIDI_SOURCE_DROPS.fetch_add(1, Ordering::Relaxed);
            return;
        }
    };
    if let Some(tx) = to.as_ref() {
        let _ = tx.send(MidiIn { port, host_ns, msg });
    }
}

/// How many source messages were dropped: the engine's MIDI input was
/// busy (pre-flight M8) or a SysEx was over the listener's bound (Task
/// 12 review M4). The engine's own queue counts its drops in
/// `MidiSender::dropped`.
pub fn midi_source_drops() -> u64 {
    MIDI_SOURCE_DROPS
        .load(Ordering::Relaxed)
        .saturating_add(listener_drops())
}

/// SysEx the record listener dropped for being over its bound (Task 12
/// review M4).
#[cfg(all(target_os = "macos", not(feature = "test-hooks")))]
fn listener_drops() -> u64 {
    lock(&SOURCES)
        .as_ref()
        .map_or(0, platform_macos::midi::SourceListener::dropped)
}

#[cfg(any(not(target_os = "macos"), feature = "test-hooks"))]
fn listener_drops() -> u64 {
    0
}

/// Hands `bytes` to the engine as if CoreMIDI source `name` sent them at
/// `host_ns` (test hook: tests connect no CoreMIDI listener).
#[cfg(feature = "test-hooks")]
pub fn debug_midi_from_source(name: &str, host_ns: u64, bytes: &[u8]) {
    midi_from_source(port_key(name), host_ns, bytes);
}

/// Connect attempts, counted in every build (Task 12 review M3: tests
/// pin that offline hosts never try).
static MIDI_CONNECTS: AtomicU64 = AtomicU64::new(0);

/// Why the sources' MIDI input is unavailable: the last connect failed
/// (Task 12 review M2). Reported as a fault, again after each Play clears
/// the faults, until a connect works.
static MIDI_UNAVAILABLE: Mutex<Option<String>> = Mutex::new(None);

/// The error connects report instead of trying (test hook).
#[cfg(feature = "test-hooks")]
static MIDI_CONNECT_FAILURE: Mutex<Option<String>> = Mutex::new(None);

/// Makes MIDI source connects fail with `why` (`None`: they work again
/// and no failure is remembered; test hook, Task 12 review M2).
#[cfg(feature = "test-hooks")]
pub fn set_midi_connect_failure(why: Option<String>) {
    if why.is_none() {
        *lock(&MIDI_UNAVAILABLE) = None;
    }
    *lock(&MIDI_CONNECT_FAILURE) = why;
}

/// How many times a host tried to connect the MIDI sources (test hook).
#[cfg(feature = "test-hooks")]
pub fn midi_source_connects() -> u64 {
    MIDI_CONNECTS.load(Ordering::Relaxed)
}

/// Connects as a live host would, reporting a failure on the current
/// host (test hook: offline hosts never connect).
#[cfg(feature = "test-hooks")]
pub fn debug_connect_midi_sources() {
    connect_and_report();
}

/// Connects the record listener to the MIDI sources present now: armed
/// MIDI lanes hear them (spec §6.6), stamped with their host time (spec
/// §6.3). The first call makes the process's one listener; later calls
/// connect the sources plugged in since (pre-flight M7). Only for a live
/// host (callers make sure); test builds connect nothing
/// (`debug_midi_from_source` and `midi.inject` stand in). Never called
/// with the host lock held: a cold CoreMIDI server can take seconds to
/// create the client (Task 12 review). Returns the fault to report when
/// the input is unavailable.
fn connect_midi_sources() -> Option<String> {
    MIDI_CONNECTS.fetch_add(1, Ordering::Relaxed);
    let fault = try_connect_midi_sources()
        .err()
        .map(|e| format!("MIDI input is unavailable: {e}"));
    *lock(&MIDI_UNAVAILABLE) = fault.clone();
    fault
}

fn try_connect_midi_sources() -> Result<(), String> {
    #[cfg(feature = "test-hooks")]
    if let Some(why) = lock(&MIDI_CONNECT_FAILURE).clone() {
        return Err(why);
    }
    #[cfg(all(target_os = "macos", not(feature = "test-hooks")))]
    {
        let mut slot = lock(&SOURCES);
        match slot.as_mut() {
            Some(l) => {
                l.reconnect();
            }
            None => {
                let l = platform_macos::midi::SourceListener::connect_all(
                    "VizDAW Record Input",
                    Box::new(midi_from_source),
                )
                .map_err(|e| e.to_string())?;
                *slot = Some(l);
            }
        }
    }
    Ok(())
}

/// Reports `fault` (a MIDI input that could not open) on `h` once.
fn note_midi_fault(h: &mut Host, fault: Option<String>) {
    if let Some(f) = fault
        && !h.faults.contains(&f)
    {
        h.faults.push(f);
    }
}

/// Connects without the host lock, then reports a failure on the host.
fn connect_and_report() {
    let fault = connect_midi_sources();
    if let Some(h) = lock(&HOST).as_mut() {
        note_midi_fault(h, fault);
    }
}

/// Picks up MIDI sources plugged in since the live host started (Record
/// with an armed MIDI lane). Only when the host is live (an
/// `Output::Hal`): offline hosts, the e2e's among them, never connect the
/// machine's real MIDI sources (pre-flight M7).
pub fn listen_to_midi_sources() {
    let live = lock(&HOST).as_ref().is_some_and(Host::is_live);
    #[cfg(feature = "test-hooks")]
    let live = live || PRETEND_LIVE_MIDI.load(Ordering::Relaxed);
    if live {
        connect_and_report();
    }
}

/// [`listen_to_midi_sources`] on a thread of its own: an arm never waits
/// for a cold CoreMIDI server (plan 5d-1 Ruling 17).
pub fn listen_to_midi_sources_soon() {
    let _ = std::thread::Builder::new()
        .name("vizdaw-midi-connect".into())
        .spawn(listen_to_midi_sources);
}

/// Offline hosts connect the MIDI sources as live ones do (test hook:
/// counts connects; test builds connect nothing real).
#[cfg(feature = "test-hooks")]
static PRETEND_LIVE_MIDI: AtomicBool = AtomicBool::new(false);

/// Makes [`listen_to_midi_sources`] connect for offline hosts too (test
/// hook, plan 5d-1 Ruling 17).
#[cfg(feature = "test-hooks")]
pub fn debug_pretend_live_midi(on: bool) {
    PRETEND_LIVE_MIDI.store(on, Ordering::Relaxed);
}

/// Holds the next rebuild between dropping the old host and making its
/// new one (test hook, Task 12 review M1).
#[cfg(feature = "test-hooks")]
static REBUILD_GATE: Mutex<Option<(std::sync::mpsc::Sender<()>, std::sync::mpsc::Receiver<()>)>> =
    Mutex::new(None);

/// The test's side of [`hold_next_rebuild`].
#[cfg(feature = "test-hooks")]
pub struct RebuildGate {
    reached: std::sync::mpsc::Receiver<()>,
    release: std::sync::mpsc::Sender<()>,
}

#[cfg(feature = "test-hooks")]
impl RebuildGate {
    /// Waits until the rebuild is held; false after `limit`.
    pub fn wait_reached(&self, limit: Duration) -> bool {
        self.reached.recv_timeout(limit).is_ok()
    }

    /// Lets the rebuild go on.
    pub fn release(&self) {
        let _ = self.release.send(());
    }
}

/// Makes the next rebuild wait, its old host dropped and its new one not
/// yet made, until the gate is released (test hook: races a Play into
/// the empty slot deterministically).
#[cfg(feature = "test-hooks")]
pub fn hold_next_rebuild() -> RebuildGate {
    let (reached_tx, reached) = std::sync::mpsc::channel();
    let (release, release_rx) = std::sync::mpsc::channel();
    *lock(&REBUILD_GATE) = Some((reached_tx, release_rx));
    RebuildGate { reached, release }
}

/// The input device live hosts open (`None`: the output device's own
/// input; spec §6.1 Settings › Audio "Input device").
static INPUT_DEVICE: Mutex<Option<String>> = Mutex::new(None);

/// Chooses the audio input device by name (`None`: "Same as output").
/// Takes effect at once for an open input (it is reopened on the input
/// thread); while recording or measuring, when the pass or measurement
/// ends (5d-2 final review I1). A device that
/// failed to open is tried again then (plan 5d-1 Ruling 15).
#[uniffi::export]
pub fn set_audio_input_device(name: Option<String>) {
    let name = name.filter(|n| !n.is_empty());
    // Lock order HOST → INPUT_DEVICE: `INPUT_DEVICE` is released before
    // `HOST` is taken below.
    let changed = {
        let mut d = lock(&INPUT_DEVICE);
        let changed = *d != name;
        *d = name;
        changed
    };
    clear_input_failure();
    if changed {
        // An input already open (or opening: [`Host::finish_job`] reopens
        // it) moves to the new device now: closed, then reopened by the
        // input thread with the new config (`pending_input`), offline as
        // two input changes. Under a record
        // hold or a measurement it applies when that ends
        // ([`Host::settle_input`]; 5d-2 final review I1).
        if let Some(h) = lock(&HOST).as_mut() {
            if !h.record_hold && h.measure.is_none() && h.asked_input() {
                h.ensure_input(false);
                h.ensure_input(true);
            }
        }
    }
}

/// Lets the next input open try the device again after it failed (plan
/// 5d-1 Ruling 15: the input picker and the arm button).
pub fn clear_input_failure() {
    if let Some(h) = lock(&HOST).as_mut() {
        h.input_failed = None;
    }
}

/// [`clear_input_failure`] when `owner` holds the engine (an edit of its
/// lanes; Task 7 review m6: another window's edit leaves it).
pub fn clear_input_failure_of(owner: u64) {
    if let Some(h) = lock(&HOST).as_mut().filter(|h| h.owner == owner) {
        h.input_failed = None;
    }
}

/// Whether `owner`'s host is opening or closing its input on the input
/// thread (plan 5d-1 Ruling 13).
pub fn input_opening(owner: u64) -> bool {
    lock(&HOST)
        .as_ref()
        .is_some_and(|h| h.owner == owner && h.job.is_some())
}

/// Whether the input thread is at work, whoever holds the engine (test
/// hook, Task 8 review m1).
#[cfg(feature = "test-hooks")]
pub fn debug_input_job() -> bool {
    lock(&HOST).as_ref().is_some_and(|h| h.job.is_some())
}

/// Waits up to `limit`, with no lock held between looks, until no input
/// thread is at work; false when one still is.
pub fn wait_input(limit: Duration) -> bool {
    let until = Instant::now() + limit;
    loop {
        if !lock(&HOST).as_ref().is_some_and(|h| h.job.is_some()) {
            return true;
        }
        if Instant::now() >= until {
            return false;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// Whether the lanes of `seq` want the engine while it stands still (plan
/// 5d-1 Ruling 16): an input to open, a monitored input or MIDI thru. The
/// same test as [`Host::wants_live`], asked of the document instead of the
/// engine's installed spec.
pub fn wants_live(doc: &Document, seq: SeqId) -> bool {
    wants_audio_input(doc, seq) || spec_wants_live(&input_spec(doc, seq))
}

/// Whether input spec `spec` listens to anything: a monitored audio input,
/// or a MIDI lane (every one [`input_spec`] lists is armed or plays thru).
fn spec_wants_live(spec: &InputSpec) -> bool {
    !spec.monitor.is_empty() || !spec.midi.is_empty()
}

/// Whether the lanes of `seq` need audio input: an armed lane with an
/// audio input, or one monitored Input while Software Monitoring is on
/// (spec §6.1: input opens only while one is; plan 5d-1 Ruling 8).
pub fn wants_audio_input(doc: &Document, seq: SeqId) -> bool {
    let software = crate::audio::software_monitoring();
    doc.sequence(seq).is_ok_and(|s| {
        s.lanes.iter().any(|l| {
            (l.arm || (software && l.monitor == MonitorMode::Input))
                && l.input.as_ref().is_some_and(LaneInput::is_audio)
        })
    })
}

/// Whether a live host may open audio input now. Never in test builds
/// (plan 5c Global Constraints: tests open no device and never ask for
/// the microphone); otherwise unless macOS refused it, which is reported.
#[cfg(all(feature = "test-hooks", target_os = "macos"))]
fn live_input_allowed(_faults: &mut Vec<String>) -> bool {
    false
}

#[cfg(all(not(feature = "test-hooks"), target_os = "macos"))]
fn live_input_allowed(faults: &mut Vec<String>) -> bool {
    admit_input(platform_macos::permission::microphone(), faults)
}

/// What the microphone permission lets input do (spec §6.1): open unless
/// macOS refused it. A refusal is reported once in `faults` as
/// `DENIED_MESSAGE`, which Record then refuses with (pre-flight M6;
/// `record::channel_refusal` looks for its start). Undetermined opens:
/// the first open is what asks.
#[cfg(target_os = "macos")]
pub fn admit_input(
    access: platform_macos::permission::MicAccess,
    faults: &mut Vec<String>,
) -> bool {
    use platform_macos::permission::{DENIED_MESSAGE, MicAccess};
    if access == MicAccess::Denied {
        if !faults.iter().any(|f| f == DENIED_MESSAGE) {
            faults.push(DENIED_MESSAGE.to_string());
        }
        return false;
    }
    true
}

/// The permission offline hosts pretend macOS gave (test hook): when
/// set, arming an audio lane reports the refusal as a live host would,
/// without reading or asking for the real one.
#[cfg(feature = "test-hooks")]
static FAKE_MIC_DENIED: AtomicBool = AtomicBool::new(false);

/// Makes offline hosts behave as if macOS refused the microphone (test
/// hook; pre-flight M6).
#[cfg(feature = "test-hooks")]
pub fn set_fake_microphone_denied(denied: bool) {
    FAKE_MIC_DENIED.store(denied, Ordering::Relaxed);
}

/// How long Record and Measure wait for an input that is opening (plan
/// 5d-1 Ruling 13).
pub const INPUT_WAIT: Duration = Duration::from_secs(3);

/// Why Record or Measure did not wait longer.
pub const INPUT_OPENING: &str = "The audio input is still opening; try again in a moment.";

/// How long the input thread waits for the answer to the first
/// microphone prompt (Ruling 14).
#[cfg(all(target_os = "macos", not(feature = "test-hooks")))]
const PROMPT_WAIT: Duration = Duration::from_secs(120);

/// Hands out [`Host::generation`] values.
static GENERATIONS: AtomicU64 = AtomicU64::new(1);

/// Offline hosts open their fake input on the input thread, this long
/// (ms; 0: at once) — test hook, plan 5d-1 Ruling 13.
#[cfg(feature = "test-hooks")]
static INPUT_OPEN_DELAY_MS: AtomicU64 = AtomicU64::new(0);

/// Makes offline hosts open (or close) their fake input on the input
/// thread after `delay` (`None`: at once, as before; test hook).
#[cfg(feature = "test-hooks")]
pub fn debug_input_open_delay(delay: Option<Duration>) {
    let ms = delay.map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
    INPUT_OPEN_DELAY_MS.store(ms, Ordering::Relaxed);
}

/// The answer the fake user gives the next (first) microphone prompt
/// (`None`: no prompt; test hook, Ruling 14).
#[cfg(feature = "test-hooks")]
static FAKE_PROMPT: Mutex<Option<bool>> = Mutex::new(None);

/// Makes the next offline input open ask the fake user, who answers
/// `answer` (true: allow; false: deny, after which macOS reads denied).
/// `None`: no prompt (test hook, Ruling 14).
#[cfg(feature = "test-hooks")]
pub fn set_fake_microphone_prompt(answer: Option<bool>) {
    *lock(&FAKE_PROMPT) = answer;
}

/// The fake user has not answered the prompt yet (test hook, pre-flight
/// I1).
#[cfg(feature = "test-hooks")]
static PROMPT_HELD: AtomicBool = AtomicBool::new(false);

/// While `held`, the fake microphone prompt waits for its answer (as a
/// user who has not clicked yet); false answers it (test hook,
/// pre-flight I1).
#[cfg(feature = "test-hooks")]
pub fn debug_hold_microphone_prompt(held: bool) {
    PROMPT_HELD.store(held, Ordering::SeqCst);
}

/// How long the fake prompt waits for its answer (ms; test hook, final
/// review m1). Ten minutes unless a test shortens it.
#[cfg(feature = "test-hooks")]
static PROMPT_LIMIT_MS: AtomicU64 = AtomicU64::new(600_000);

/// Sets how long the fake microphone prompt waits for its answer before
/// it counts as unanswered (`None`: ten minutes; test hook, final review
/// m1), as the live prompt's 120 s.
#[cfg(feature = "test-hooks")]
pub fn debug_microphone_prompt_limit(limit: Option<Duration>) {
    let ms = limit.map_or(600_000, |d| {
        u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
    });
    PROMPT_LIMIT_MS.store(ms, Ordering::SeqCst);
}

/// The fault an input change reports when the microphone prompt was left
/// up past its wait (final review m1; Ruling 14: only a grant opens the
/// input). The next arm or input edit asks again (Ruling 15).
pub const PROMPT_UNANSWERED: &str =
    "audio input could not open: waiting for microphone permission; arm the lane again";

/// Offline input opens that fail before one works (test hook, Ruling 15).
#[cfg(feature = "test-hooks")]
static FAIL_OPENS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Makes the next `n` offline input opens fail as a busy device would
/// (test hook, Ruling 15).
#[cfg(feature = "test-hooks")]
pub fn debug_fail_input_opens(n: u32) {
    FAIL_OPENS.store(n, Ordering::Relaxed);
}

/// Whether an offline input open fails now (one fewer to fail).
#[cfg(feature = "test-hooks")]
fn take_fake_failure() -> bool {
    let mut n = FAIL_OPENS.load(Ordering::Relaxed);
    while n > 0 {
        match FAIL_OPENS.compare_exchange(n, n - 1, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => return true,
            Err(v) => n = v,
        }
    }
    false
}

/// What a host's audio input offers (spec §6.1, §12.1 `state.get input`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InputInfo {
    /// Input channels (0: no input).
    pub channels: u32,
    /// Their names.
    pub names: Vec<String>,
    /// Input latency in frames.
    pub input_latency_frames: u32,
    /// Output latency in frames.
    pub output_latency_frames: u32,
    /// The device's I/O buffer, frames (0 before a device opened).
    pub buffer_frames: u32,
}

impl InputInfo {
    /// What an opened stream's input offers.
    pub fn of(info: &core_engine::backend::StreamInfo) -> InputInfo {
        InputInfo {
            channels: info.input_channels,
            names: info.input_channel_names.clone(),
            input_latency_frames: info.input_latency_frames,
            output_latency_frames: info.output_latency_frames,
            buffer_frames: info.buffer_frames,
        }
    }

    /// The round trip a monitored input takes (plan 5d-1 Ruling 9): both
    /// latencies and a buffer each way.
    pub fn round_trip_frames(&self) -> u32 {
        self.input_latency_frames
            .saturating_add(self.output_latency_frames)
            .saturating_add(self.buffer_frames.saturating_mul(2))
    }
}

/// What the lanes of `seq` listen to (spec §6.6): armed or Input-monitored
/// lanes with an input. MIDI lanes play thru unless their monitor is Off;
/// audio lanes are heard through their output path (the document's first
/// when they have none) unless it is Off or Software Monitoring is off
/// (plan 5d-1 Ruling 8).
pub fn input_spec(doc: &Document, seq: SeqId) -> InputSpec {
    let software = crate::audio::software_monitoring();
    let mut spec = InputSpec::default();
    let Ok(s) = doc.sequence(seq) else {
        return spec;
    };
    let first_out = doc.outputs().keys().next().copied();
    for l in &s.lanes {
        if !(l.arm || l.monitor == MonitorMode::Input) {
            continue;
        }
        match &l.input {
            Some(LaneInput::Midi { port, channel }) => spec.midi.push(MidiInputSpec {
                lane: l.id,
                port: port.as_deref().map(port_key),
                channel: *channel,
                dest: l.midi_dest,
                thru: l.monitor != MonitorMode::Off,
                armed: l.arm,
            }),
            Some(LaneInput::Audio { channels, .. })
                if software && l.monitor != MonitorMode::Off =>
            {
                if let Some(path) = l.audio_out.or(first_out) {
                    spec.monitor.push(MonitorRoute {
                        lane: l.id,
                        path,
                        left: channels.first(),
                        right: channels.last().unwrap_or(channels.first()),
                        mode: l.monitor,
                        armed: l.arm,
                    });
                }
            }
            _ => {}
        }
    }
    spec
}

/// The external synths whose return the lanes of `seq` record (spec
/// §6.7), each once: the engine interns them at every load, so Measure
/// can play them (plan 5c-2 Ruling 10). A link to a destination that is
/// missing or not external is left out (the model clears such links;
/// this is defence in depth).
pub fn return_dests(doc: &Document, seq: SeqId) -> Vec<DestId> {
    let mut out: Vec<DestId> = Vec::new();
    if let Ok(s) = doc.sequence(seq) {
        for d in s
            .lanes
            .iter()
            .filter_map(|l| l.input.as_ref().and_then(LaneInput::return_of))
            .filter(|d| {
                doc.instruments()
                    .get(d)
                    .is_some_and(|x| x.kind.is_external())
            })
        {
            if !out.contains(&d) {
                out.push(d);
            }
        }
    }
    out
}

fn next_epoch() -> u64 {
    EPOCHS.fetch_add(1, Ordering::Relaxed)
}

/// Each session's online media, handed to the engine before every load
/// (5a Task 10).
static MEDIA: Mutex<BTreeMap<u64, Vec<core_media::stream::MediaFile>>> =
    Mutex::new(BTreeMap::new());

/// Sets session `owner`'s online media; reloads use it.
pub fn set_media(owner: u64, files: Vec<core_media::stream::MediaFile>) {
    lock(&MEDIA).insert(owner, files);
}

/// Forgets session `owner`'s media (it closed).
pub fn forget_media(owner: u64) {
    lock(&MEDIA).remove(&owner);
}

/// Session `owner`'s media table as `(media, path)` (test hook).
#[cfg(feature = "test-hooks")]
pub fn debug_media(owner: u64) -> Vec<(u64, String)> {
    media_of(owner)
        .into_iter()
        .map(|f| (f.id.0, f.path.to_string_lossy().into_owned()))
        .collect()
}

fn media_of(owner: u64) -> Vec<core_media::stream::MediaFile> {
    lock(&MEDIA).get(&owner).cloned().unwrap_or_default()
}

/// Media whose reads failed while `owner` played (each once).
pub fn take_stream_errors(owner: u64) -> Vec<(core_model::ids::MediaId, String)> {
    lock(&HOST)
        .as_ref()
        .filter(|h| h.owner == owner)
        .map(|h| h.engine.take_stream_errors())
        .unwrap_or_default()
}

/// Stalls the disk of the current host and of hosts created from now on
/// (`true`), as a disk that never answers: a Play waits its full
/// `LOCATE_WAIT` (test hook).
#[cfg(feature = "test-hooks")]
pub fn stall_disk(on: bool) {
    STALL_DISK.store(on, Ordering::Relaxed);
    if let Some(h) = lock(&HOST).as_ref() {
        h.engine.streamer().set_stalled(on);
    }
}

/// How long a still-running device gets to play the note-offs before a
/// rebuild (as in `vizdaw-play`'s live loop).
const DRAIN: Duration = Duration::from_millis(300);

/// True when the offline backend is selected (`VIZDAW_AUDIO=offline`, or
/// the `force_offline` test hook).
pub fn offline_mode() -> bool {
    FORCE_OFFLINE.load(Ordering::Relaxed)
        || std::env::var("VIZDAW_AUDIO").is_ok_and(|v| v == "offline")
}

/// Selects the offline backend for this process (test hook).
#[cfg(feature = "test-hooks")]
pub fn force_offline() {
    FORCE_OFFLINE.store(true, Ordering::Relaxed);
}

/// Buffer sizes the offline device refuses to reopen at (test hook).
#[cfg(feature = "test-hooks")]
static REFUSED_BUFFERS: Mutex<Vec<u32>> = Mutex::new(Vec::new());

/// Makes the offline device refuse a reopen at any of `sizes` (test hook;
/// empty: none).
#[cfg(feature = "test-hooks")]
pub fn debug_refuse_buffer_sizes(sizes: &[u32]) {
    *lock(&REFUSED_BUFFERS) = sizes.to_vec();
}

/// Drops the host (test hook): the next Play creates a new one.
#[cfg(feature = "test-hooks")]
pub fn reset_host() {
    reset();
    lock(&RETIRED_LOG).clear();
}

/// Queues a device event as if the output reported it (test hook); the
/// owner's next `state` call handles it.
#[cfg(feature = "test-hooks")]
pub fn simulate_event(ev: BackendEvent) {
    if let Some(h) = lock(&HOST).as_mut() {
        h.injected.push(ev);
    }
}

/// Number of hosts created so far in this process (test hook).
#[cfg(feature = "test-hooks")]
pub fn hosts_created() -> u64 {
    HOSTS_CREATED.load(Ordering::Relaxed)
}

/// The engine's automation override of `strip`'s fader in dB, if it holds
/// one (test hook).
#[cfg(feature = "test-hooks")]
pub fn fader_override(strip: u64) -> Option<f64> {
    lock(&HOST)
        .as_ref()?
        .engine
        .overrides()?
        .fader_db(core_model::ids::StripId(strip))
}

/// Offline events of hosts retired for another session's document (their
/// logs go with them), until the next `drain_offline_log`.
#[cfg(feature = "test-hooks")]
static RETIRED_LOG: Mutex<Vec<(u64, Vec<u8>)>> = Mutex::new(Vec::new());

/// Drains the offline event log as `(frame, bytes)` (test hook): the
/// events of hosts retired since the last drain first, then the current
/// host's.
#[cfg(feature = "test-hooks")]
pub fn drain_offline_log() -> Vec<(u64, Vec<u8>)> {
    let mut events = std::mem::take(&mut *lock(&RETIRED_LOG));
    let host = lock(&HOST);
    if let Some(log) = host.as_ref().and_then(|h| h.log.as_ref()) {
        events.extend(log.drain().iter().map(|r| (r.frame, r.bytes().to_vec())));
    }
    events
}

/// What the next `reload` should do instead of recompiling (test hook).
#[cfg(feature = "test-hooks")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReloadFault {
    /// Return an engine error.
    Error,
    /// Panic while holding the host lock.
    Panic,
}

#[cfg(feature = "test-hooks")]
static RELOAD_FAULT: Mutex<Option<ReloadFault>> = Mutex::new(None);

/// Makes the next `reload` that reaches the engine fail (test hook).
#[cfg(feature = "test-hooks")]
pub fn inject_reload_fault(f: Option<ReloadFault>) {
    *lock(&RELOAD_FAULT) = f;
}

/// Makes the next `stop_recording` fail as if the engine's stop could not
/// be sent (test hook, Task 11 re-review m1).
#[cfg(feature = "test-hooks")]
static STOP_RECORDING_FAULT: AtomicBool = AtomicBool::new(false);

/// Makes the next `stop_recording` that reaches the engine fail before
/// the engine stops (test hook).
#[cfg(feature = "test-hooks")]
pub fn inject_stop_recording_fault(on: bool) {
    STOP_RECORDING_FAULT.store(on, Ordering::Relaxed);
}

/// Holds a measurement past its end (test hook, pre-flight M5): while
/// held, `Session::measure_return` keeps waiting and does not end it.
#[cfg(feature = "test-hooks")]
static HOLD_MEASURE: AtomicBool = AtomicBool::new(false);

/// Holds (`true`) or releases every measurement's end (test hook,
/// pre-flight M5): Play, Record and Measure calls can then be made while
/// one is certainly installed.
#[cfg(feature = "test-hooks")]
pub fn debug_hold_measure(on: bool) {
    HOLD_MEASURE.store(on, Ordering::Relaxed);
}

/// Whether a test holds measurements past their end.
#[cfg(feature = "test-hooks")]
pub(crate) fn measure_held() -> bool {
    HOLD_MEASURE.load(Ordering::Relaxed)
}

/// Never without the test hooks.
#[cfg(not(feature = "test-hooks"))]
pub(crate) const fn measure_held() -> bool {
    false
}

/// Milliseconds Measure waits after ending its measurement, before it
/// reads the result (test hook, Task 9 re-review: a descheduled thread).
#[cfg(feature = "test-hooks")]
static MEASURE_END_DELAY_MS: AtomicU64 = AtomicU64::new(0);

/// Makes every Measure wait `d` after ending its measurement, before it
/// reads the result (test hook).
#[cfg(feature = "test-hooks")]
pub fn debug_measure_end_delay(d: Duration) {
    let ms = u64::try_from(d.as_millis()).unwrap_or(u64::MAX);
    MEASURE_END_DELAY_MS.store(ms, Ordering::Relaxed);
}

/// The wait [`debug_measure_end_delay`] asked for.
pub(crate) fn after_measure_end() {
    #[cfg(feature = "test-hooks")]
    {
        let ms = MEASURE_END_DELAY_MS.load(Ordering::Relaxed);
        if ms > 0 {
            std::thread::sleep(Duration::from_millis(ms));
        }
    }
}

/// Closed sessions' plugin hosts still waiting for processors (test hook).
#[cfg(feature = "test-hooks")]
pub fn closing_hosts_waiting() -> usize {
    plugins::closing_hosts()
}

/// The names of the MIDI destinations the live engine has interned as
/// targets (test hook; empty without a host).
#[cfg(feature = "test-hooks")]
pub fn midi_destination_targets() -> Vec<String> {
    lock(&HOST).as_ref().map_or_else(Vec::new, |h| {
        h.engine
            .targets()
            .iter()
            .filter_map(|(_, k)| match k {
                core_engine::schedule::targets::TargetKind::MidiDestination(p) => {
                    Some(p.name.clone())
                }
                _ => None,
            })
            .collect()
    })
}

/// The fake input an offline host opens and the delay of the fake synth
/// it plays, if any. Only the signals input plays the synth: with any
/// other, its queue would fill and drop every note (Task 7 review I1).
fn offline_choice() -> (Option<FakeInput>, Option<f64>) {
    let (input, input_chosen) = fake_input_choice();
    let (delay, synth_chosen) = fake_synth_choice();
    let input = offline_input(input, input_chosen, delay.is_some(), synth_chosen);
    let delay = delay.filter(|_| input == Some(FakeInput::Signals));
    (input, delay)
}

/// The MIDI destinations a new destination may play (plan 5d-1 Ruling
/// 25; "New MIDI Destination…"): the running host's (what its loads
/// resolve ports against) when there is one; else the system's CoreMIDI
/// destinations, or offline the fake synth's port when an offline host
/// would have it (Task 11 review m3). Infallible: the whole body runs
/// inside `catch_unwind`, so a panic in either CoreMIDI listing (a live
/// host's ports are one too) answers no ports rather than crossing the
/// FFI (pre-flight M5; Task 12 review I1). [`lock`] recovers a poisoned
/// `HOST`, so later callers are unaffected.
#[uniffi::export]
pub fn midi_destination_ports() -> Vec<String> {
    std::panic::catch_unwind(destination_ports).unwrap_or_default()
}

/// [`midi_destination_ports`]' body, which may panic.
fn destination_ports() -> Vec<String> {
    if let Some(ports) = lock(&HOST).as_ref().map(|h| h.engine.ports()) {
        return ports.into_iter().map(|p| p.name).collect();
    }
    if offline_mode() {
        return match offline_choice() {
            (_, Some(_)) => vec![core_engine::offline::FAKE_SYNTH_PORT.to_string()],
            _ => Vec::new(),
        };
    }
    #[cfg(target_os = "macos")]
    {
        platform_macos::midi::list_destinations()
            .into_iter()
            .map(|p| p.name)
            .collect()
    }
    #[cfg(not(target_os = "macos"))]
    {
        Vec::new()
    }
}

/// The audio input devices Settings › Audio offers (plan 5d-2 Task 1):
/// every device with input channels on a live host; none offline.
/// Infallible: the body runs inside `catch_unwind`, so a panic in the
/// HAL listing answers no devices rather than crossing the FFI (5d-2
/// pre-flight M1).
#[uniffi::export]
pub fn audio_input_devices() -> Vec<String> {
    std::panic::catch_unwind(audio_input_devices_body).unwrap_or_default()
}

/// [`audio_input_devices`]' body, which may panic.
fn audio_input_devices_body() -> Vec<String> {
    if offline_mode() {
        return Vec::new();
    }
    #[cfg(target_os = "macos")]
    {
        platform_macos::hal::input_devices()
    }
    #[cfg(not(target_os = "macos"))]
    {
        Vec::new()
    }
}

/// The chosen input device's channels and their names (5d-2 final review
/// I4), for the input picker before any document holds the engine: read
/// from the device's properties, so no device is opened, no host made and
/// no microphone asked. Offline: the fake input for "Same as output", and
/// nothing for a named device (offline hosts offer none). `None` when
/// unknown: no input device, a chosen device not connected. Infallible: the
/// body runs inside `catch_unwind`, so a panic in the HAL read answers
/// `None` rather than crossing the FFI.
#[uniffi::export]
pub fn audio_input_channels() -> Option<crate::live::InputChannelsRow> {
    std::panic::catch_unwind(audio_input_channels_body)
        .ok()
        .flatten()
}

/// [`audio_input_channels`]' body, which may panic.
fn audio_input_channels_body() -> Option<crate::live::InputChannelsRow> {
    let device = lock(&INPUT_DEVICE).clone();
    let (channels, names) = if offline_mode() {
        // As an offline host's input (`OfflineBackend`): the fake input's
        // channels, named as it names them. The input that host opens,
        // which the fake synth brings when no input was chosen (5e-1
        // Task 1: Measure's channel check reads this before its claim).
        let _ = offline_choice().0.filter(|_| device.is_none())?;
        let n = core_engine::offline::FAKE_INPUT_CHANNELS as u32;
        (n, (1..=n).map(|k| format!("Fake In {k}")).collect())
    } else {
        #[cfg(target_os = "macos")]
        {
            platform_macos::hal::input_channels_of(device.as_deref())?
        }
        #[cfg(not(target_os = "macos"))]
        {
            return None;
        }
    };
    Some(crate::live::InputChannelsRow { channels, names })
}

/// The MIDI sources a MIDI lane's input picker offers (plan 5d-2 Task 1):
/// the system's sources but VizDAW's own on a live host; none offline.
/// Infallible: the body runs inside `catch_unwind`, so a panic in the
/// CoreMIDI listing answers no sources rather than crossing the FFI (5d-2
/// pre-flight M1).
#[uniffi::export]
pub fn midi_source_ports() -> Vec<String> {
    std::panic::catch_unwind(midi_source_ports_body).unwrap_or_default()
}

/// [`midi_source_ports`]' body, which may panic.
fn midi_source_ports_body() -> Vec<String> {
    if offline_mode() {
        return Vec::new();
    }
    #[cfg(target_os = "macos")]
    {
        platform_macos::midi::list_sources()
    }
    #[cfg(not(target_os = "macos"))]
    {
        Vec::new()
    }
}

/// How many engine loads the host made (test hook).
#[cfg(feature = "test-hooks")]
pub fn loads() -> u64 {
    LOADS.load(Ordering::Relaxed)
}

/// The live engine's worker count (test hook; `None` without a host).
#[cfg(feature = "test-hooks")]
pub fn engine_workers() -> Option<usize> {
    lock(&HOST).as_ref().map(|h| h.engine.config().workers)
}

/// True when a host exists and its engine is not stopped (test hook).
#[cfg(feature = "test-hooks")]
pub fn engine_active() -> bool {
    lock(&HOST)
        .as_ref()
        .is_some_and(|h| h.engine.state() != PlayState::Stopped)
}

/// Puts `h` in the host slot and points the CoreMIDI sources at its
/// engine. Only an installed host gets them: a rebuild's host that lost
/// the slot to a Play is dropped without ever taking them (Task 12
/// review M1).
fn install(slot: &mut Option<Host>, h: Host) {
    *lock(&MIDI_TO) = Some(h.engine.midi_sender());
    *slot = Some(h);
}

/// Lets a stopped host's still-running output play its note-offs, then
/// drops it (output first, then the engine). Call without the host lock.
fn retire(old: Option<Host>) {
    let Some(old) = old else {
        return;
    };
    std::thread::sleep(DRAIN);
    #[cfg(feature = "test-hooks")]
    if let Some(log) = &old.log {
        lock(&RETIRED_LOG).extend(log.drain().iter().map(|r| (r.frame, r.bytes().to_vec())));
    }
    drop(old);
}

/// Drops the host: its engine stops first, so played and auditioned
/// notes end (their note-offs reach external MIDI destinations), then the
/// output plays them and the host goes, without the host lock (as in
/// `play`'s handover).
pub fn reset() {
    let mut old = lock(&HOST).take();
    // No engine: nothing hears the sources until the next host.
    *lock(&MIDI_TO) = None;
    if let Some(h) = old.as_mut() {
        let _ = h.engine.stop();
    }
    retire(old);
}

/// While set, the offline driver renders nothing and skips the time held
/// (plan 5e-1 check I-1: code then runs before the roll).
static OFFLINE_HOLD: AtomicBool = AtomicBool::new(false);

/// Holds (`on`) or lets go the offline renderer (test hook): while held
/// it renders nothing, and the time held is skipped, not caught up in one
/// burst at the release.
#[cfg(feature = "test-hooks")]
pub fn debug_hold_offline(on: bool) {
    OFFLINE_HOLD.store(on, Ordering::Relaxed);
}

/// Renders an offline backend in real time on its own thread.
struct OfflineDriver {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl OfflineDriver {
    fn start(
        renderer: Renderer,
        rate: f64,
        input: Option<FakeInput>,
        synth: Option<&FakeSynth>,
    ) -> Result<(OfflineDriver, InputInfo), VizError> {
        let mut backend = OfflineBackend::new(rate, OFFLINE_BLOCK);
        if let Some(k) = input {
            backend = backend.with_input(k);
        }
        if let Some(s) = synth {
            backend = backend.with_synth(s);
        }
        let info = backend.open(&core_engine::backend::AudioConfig::default())?;
        let inputs = InputInfo::of(&info);
        backend.start(renderer)?;
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let thread = std::thread::Builder::new()
            .name("vizdaw-offline".into())
            .spawn(move || {
                let mut t0 = Instant::now();
                let mut held: Option<Instant> = None;
                while !flag.load(Ordering::Relaxed) {
                    if OFFLINE_HOLD.load(Ordering::Relaxed) {
                        held.get_or_insert_with(Instant::now);
                        std::thread::sleep(Duration::from_millis(2));
                        continue;
                    }
                    // The time held is skipped (the clock origin moves on).
                    if let Some(h) = held.take() {
                        t0 += h.elapsed();
                    }
                    let due = (t0.elapsed().as_secs_f64() * rate) as u64;
                    if due > backend.frames() {
                        backend.run_frames(due - backend.frames());
                    }
                    std::thread::sleep(Duration::from_millis(2));
                }
                let _ = backend.stop();
            })
            .map_err(|e| VizError::Engine {
                message: format!("offline thread: {e}"),
            })?;
        Ok((
            OfflineDriver {
                stop,
                thread: Some(thread),
            },
            inputs,
        ))
    }
}

impl Drop for OfflineDriver {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

enum Output {
    /// Held for its `Drop`, which stops the render thread.
    Offline(#[allow(dead_code)] OfflineDriver),
    /// The device, shared with the input thread while it reopens it
    /// without the host lock (plan 5d-1 Ruling 13).
    #[cfg(target_os = "macos")]
    Hal(Arc<Mutex<platform_macos::hal::CoreAudioBackend>>),
}

/// What a host and its input thread share (plan 5d-1 Ruling 13;
/// pre-flight I1).
#[derive(Default)]
struct JobFlags {
    /// Set when the host goes: the thread then touches nothing of it and
    /// hands nothing back.
    cancel: AtomicBool,
    /// The thread is past the microphone prompt: a host that goes waits
    /// for it (it may be reopening the device); before that it does not
    /// (the user may take minutes to answer).
    asked: AtomicBool,
}

impl JobFlags {
    /// Marks the thread past the prompt; whether it may go on. With
    /// [`Drop for Host`](Host)'s store of `cancel` before its load of
    /// `asked` (both `SeqCst`), either the thread sees the cancel or the
    /// host sees `asked` and waits for it.
    fn past_prompt(&self) -> bool {
        self.asked.store(true, Ordering::SeqCst);
        !self.cancel.load(Ordering::SeqCst)
    }

    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }
}

/// The thread changing a host's input (plan 5d-1 Ruling 13).
struct InputJob {
    flags: Arc<JobFlags>,
    thread: Option<JoinHandle<()>>,
    /// Open or closed, as the thread will leave the input.
    want: bool,
    /// The input device chosen when the job started (what an open opens;
    /// 5d-2 Task 1 review I1).
    device: Option<String>,
}

/// What an input thread does without the host lock.
enum Opener {
    /// Reopens a live device with `cfg`, asking for the microphone first
    /// when macOS never asked (Ruling 14). The device is held weakly: a
    /// host that goes while the prompt is open takes its device with it.
    #[cfg(target_os = "macos")]
    Hal {
        backend: std::sync::Weak<Mutex<platform_macos::hal::CoreAudioBackend>>,
        cfg: core_engine::backend::AudioConfig,
    },
    /// The offline device under the test hooks: waits `delay`, the fake
    /// user answering `prompt` to the first microphone prompt.
    #[cfg(feature = "test-hooks")]
    Fake {
        delay: Duration,
        prompt: Option<bool>,
    },
}

/// What an input thread found.
enum Opened {
    /// The device reopened: what it offers, the workers' environment, and
    /// why its input failed to open (it then reopened without input).
    #[cfg(target_os = "macos")]
    Device {
        info: core_engine::backend::StreamInfo,
        env: core_engine::pool::RtEnv,
        failed: Option<String>,
    },
    /// The device could not reopen at all.
    #[cfg(target_os = "macos")]
    Broken(String),
    /// macOS (or the fake user) refused the microphone.
    Denied,
    /// The microphone prompt was left unanswered past its wait: the input
    /// stays closed (final review m1).
    Unanswered,
    /// The fake input changed.
    #[cfg(feature = "test-hooks")]
    Fake,
}

/// Does `opener`'s work for an input that should end `want` (open or
/// closed). Runs on the input thread, with no lock but the device's.
/// `None` when the host went meanwhile (pre-flight I1: checked right
/// after the prompt, before the device is touched).
fn open_input(opener: Opener, want: bool, flags: &JobFlags) -> Option<Opened> {
    match opener {
        #[cfg(target_os = "macos")]
        Opener::Hal { backend, cfg } => {
            use platform_macos::permission::{MicAccess, microphone};
            // The first open asks, here, off the caller's thread; a denial
            // is known before anything records (Ruling 14). Test builds
            // never ask (they never open a live device either).
            // A host that went before the prompt is never asked for
            // (Task 7 review m5); the prompt's answer is the access.
            #[cfg(not(feature = "test-hooks"))]
            let access = if want && microphone() == MicAccess::Undetermined {
                if flags.cancelled() {
                    return None;
                }
                platform_macos::permission::request_microphone(PROMPT_WAIT)
            } else {
                microphone()
            };
            #[cfg(feature = "test-hooks")]
            let access = microphone();
            if !flags.past_prompt() {
                return None;
            }
            if want && access == MicAccess::Denied {
                return Some(Opened::Denied);
            }
            // Only a grant opens the input (Ruling 14; final review m1): a
            // prompt left up past the wait is not one.
            if want && access == MicAccess::Undetermined {
                return Some(Opened::Unanswered);
            }
            let backend = backend.upgrade()?;
            let mut b = lock(&backend);
            Some(match reopen_backend(&mut b, &cfg) {
                Ok((info, env, failed)) => Opened::Device { info, env, failed },
                Err(e) => Opened::Broken(e.to_string()),
            })
        }
        #[cfg(feature = "test-hooks")]
        Opener::Fake { delay, prompt } => {
            if prompt.is_some() {
                // As the live prompt: not asked for a host that went
                // (Task 7 review m5).
                if flags.cancelled() {
                    return None;
                }
                // The fake user has not answered yet (bounded, as the
                // live prompt's wait is).
                let limit = Duration::from_millis(PROMPT_LIMIT_MS.load(Ordering::SeqCst));
                let until = Instant::now() + limit;
                while PROMPT_HELD.load(Ordering::SeqCst) && Instant::now() < until {
                    std::thread::sleep(Duration::from_millis(2));
                }
            }
            if !flags.past_prompt() {
                return None;
            }
            // Still unanswered after the wait: not a grant (final review
            // m1).
            if want && prompt.is_some() && PROMPT_HELD.load(Ordering::SeqCst) {
                return Some(Opened::Unanswered);
            }
            // The device's slowness, cut short when the host goes.
            let until = Instant::now() + delay;
            loop {
                if flags.cancelled() {
                    return None;
                }
                let left = until.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    break;
                }
                std::thread::sleep(left.min(Duration::from_millis(5)));
            }
            Some(if want && prompt == Some(false) {
                Opened::Denied
            } else {
                Opened::Fake
            })
        }
    }
}

/// Stops `b`, reopens it with `cfg` (without input when the input fails
/// to open: its reason is returned; each open falls back to the device's
/// own buffer size when it refuses the one asked for, Task 6 review I2)
/// and restarts it with the same renderer; what it offers and the
/// workers' environment.
#[cfg(target_os = "macos")]
fn reopen_backend(
    b: &mut platform_macos::hal::CoreAudioBackend,
    cfg: &core_engine::backend::AudioConfig,
) -> Result<
    (
        core_engine::backend::StreamInfo,
        core_engine::pool::RtEnv,
        Option<String>,
    ),
    VizError,
> {
    let mut r = b.stop()?;
    let (info, failed) = match open_or_default(cfg.clone(), |c| b.open(c)) {
        Ok(i) => (i, None),
        Err(e) if cfg.input_enabled => {
            let f = format!("{}: {e}", crate::record::INPUT_FAILED_PREFIX);
            let out = core_engine::backend::AudioConfig {
                input_enabled: false,
                ..cfg.clone()
            };
            (open_or_default(out, |c| b.open(c))?, Some(f))
        }
        Err(e) => return Err(e.into()),
    };
    if info.buffer_frames > r.config().max_frames {
        r.prepare(info.buffer_frames);
    }
    // No peak held under the old config outlives it, and none of the new
    // one is wiped (Task 4 parked item): cleared while stopped.
    r.clear_input_peaks();
    b.start(r)?;
    let env = rt_env(b, info.buffer_frames, info.sample_rate);
    Ok((info, env, failed))
}

/// Gives what an input thread found back to host `generation`, under
/// the host lock (tried, never waited for: a host being dropped may hold
/// it while it joins this thread, after setting `cancel`). A host not in
/// the slot yet (a rebuild's, before it is installed) is waited for; one
/// that never gets there is dropped, which cancels.
fn hand_back(generation: u64, flags: &JobFlags, want: bool, done: Opened) {
    loop {
        if flags.cancelled() {
            return;
        }
        let mut slot = match HOST.try_lock() {
            Ok(g) => g,
            Err(std::sync::TryLockError::Poisoned(p)) => p.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) => {
                std::thread::sleep(Duration::from_millis(1));
                continue;
            }
        };
        if let Some(h) = slot.as_mut().filter(|h| h.generation == generation) {
            h.finish_job(want, done);
            return;
        }
        drop(slot);
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// The engine and its output.
struct Host {
    // Field order matters: the output stops (and frees the renderer)
    // before the engine's collector thread is joined.
    output: Output,
    engine: Engine,
    owner: u64,
    /// The session whose plugin host is the engine's insert factory (0:
    /// none).
    factory_owner: u64,
    /// The engine loaded a document with plugin slots since it was made:
    /// its instances and plugin targets are keyed by that document's
    /// instance IDs, so another session must not inherit them (Plan
    /// Ruling 2).
    plugins_in_engine: bool,
    seq: SeqId,
    looped: bool,
    /// Play was asked and not stopped since. The engine applies Play on
    /// its next audio block, so `engine.state()` can still say Stopped
    /// right after `play`; a rebuild must resume from this too.
    want_play: bool,
    /// Where the asked Play stops (Play Selection, plan 4f): a rebuild
    /// resumes with it.
    until: Option<Tick>,
    /// The tick a device rebuild resumed this host from (plan 5e-1
    /// re-review N1: its renderer publishes no position until it runs).
    resumed_at: Option<Tick>,
    /// Changed by every Play, Stop and release: a Play that waited for
    /// the disk without the lock goes on only if nothing came meanwhile
    /// ([`finish_play`]).
    epoch: u64,
    device: String,
    sample_rate: f64,
    /// The project rate this host was opened for (the device was asked
    /// for it; Ruling 20).
    project_rate: f64,
    /// The device runs at another rate than `project_rate`: regions are
    /// muted.
    rate_mismatch: bool,
    report: Vec<String>,
    faults: Vec<String>,
    /// Instances the engine bypassed for faults (sticky for this engine,
    /// 3a Plan Ruling 23).
    faulted: std::collections::BTreeSet<u64>,
    /// Read only by the `drain_offline_log` test hook.
    #[cfg_attr(not(feature = "test-hooks"), allow(dead_code))]
    log: Option<EventLog>,
    /// Events queued by the `simulate_event` test hook.
    #[cfg_attr(not(feature = "test-hooks"), allow(dead_code))]
    injected: Vec<BackendEvent>,
    /// What the output's input offers.
    inputs: InputInfo,
    /// A live output has its input open (an offline one: the input it
    /// would have open).
    input_open: bool,
    /// Times the input was opened or closed (test hook counter).
    #[cfg_attr(not(feature = "test-hooks"), allow(dead_code))]
    input_changes: u64,
    /// A record tap is in (Task 11 review I1): the input stays as it is
    /// until it goes, so an edit never restarts or closes the device under
    /// a pass.
    record_hold: bool,
    /// What the input should become once the hold ends.
    pending_input: Option<bool>,
    /// The input device the open input (or the input thread's open) uses:
    /// a different choice reopens it once nothing holds the input (5d-2
    /// final review I1).
    opened_device: Option<String>,
    /// Why a live output's input could not open: not tried again (each
    /// try restarts the device) until no lane wants input any more.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    input_failed: Option<String>,
    /// The return-latency measurement installed, if any (plan 5c-2
    /// Ruling 9): Play, Record and another Measure are `Busy` until it
    /// ends, and the input stays as it is (pre-flight M1). Its status
    /// tells whose it is (Task 9 review).
    measure: Option<Arc<MeasureStatus>>,
    /// This host among all hosts made: an input thread hands back only to
    /// the host that started it.
    generation: u64,
    /// The input changing on its own thread (plan 5d-1 Ruling 13): input
    /// changes asked meanwhile wait for it in `pending_input`.
    job: Option<InputJob>,
    /// The input thread could not reopen the device (pre-flight M7): the
    /// next claim replaces this host.
    broken: bool,
}

impl Drop for Host {
    fn drop(&mut self) {
        // The input thread hands nothing back to a host that is going. Past
        // the prompt it may be reopening the device: it finishes before the
        // output goes. At the prompt it is not waited for (pre-flight I1).
        if let Some(mut j) = self.job.take() {
            j.flags.cancel.store(true, Ordering::SeqCst);
            if j.flags.asked.load(Ordering::SeqCst) {
                if let Some(t) = j.thread.take() {
                    let _ = t.join();
                }
            }
        }
    }
}

impl Host {
    /// A host for a project at `project` Hz: the device is asked for that
    /// rate; when it runs at another, regions are muted (Ruling 20).
    fn create(project: f64) -> Result<Host, VizError> {
        let mut h = Host::create_any(project)?;
        if STALL_DISK.load(Ordering::Relaxed) {
            h.engine.streamer().set_stalled(true);
        }
        h.project_rate = project;
        h.rate_mismatch = !same_rate(h.sample_rate, project);
        h.engine.set_regions_muted(h.rate_mismatch);
        Ok(h)
    }

    /// Whether this host plays through a device (not offline).
    fn is_live(&self) -> bool {
        match self.output {
            Output::Offline(_) => false,
            #[cfg(target_os = "macos")]
            Output::Hal(_) => true,
        }
    }

    fn create_any(project: f64) -> Result<Host, VizError> {
        HOSTS_CREATED.fetch_add(1, Ordering::Relaxed);
        if offline_mode() {
            let (input, delay) = offline_choice();
            let synth = delay.map(FakeSynth::new);
            let mut sinks = OfflineSinks::new(1 << 16);
            if let Some(s) = &synth {
                sinks = sinks.with_fake_synth(s);
            }
            let log = sinks.log();
            let rate = offline_rate(project);
            let mut cfg = EngineConfig::new(rate);
            cfg.stream = crate::audio::stream_config();
            let (engine, renderer) = Engine::new(cfg, Box::new(sinks))?;
            let (driver, inputs) = OfflineDriver::start(renderer, rate, input, synth.as_ref())?;
            let mut h = Host::new(
                engine,
                Output::Offline(driver),
                OFFLINE_DEVICE,
                rate,
                Some(log),
            );
            h.inputs = inputs;
            return Ok(h);
        }
        Host::create_live(project)
    }

    #[cfg(target_os = "macos")]
    fn create_live(project: f64) -> Result<Host, VizError> {
        use platform_macos::hal::CoreAudioBackend;
        use platform_macos::rt::MacRtPlatform;
        use platform_macos::sinks::MacSinks;
        use std::sync::Arc;
        let mut backend = CoreAudioBackend::new();
        // Output only: `ensure_input` opens input once a lane needs it.
        let info = open_or_default(live_audio_config(project, false), |c| backend.open(c))?;
        let (engine, renderer) = Engine::with_platform(
            live_config(info.sample_rate),
            Box::new(MacSinks::new()),
            Arc::new(MacRtPlatform),
        )?;
        engine.set_rt_env(rt_env(&backend, info.buffer_frames, info.sample_rate));
        backend.start(renderer)?;
        let mut h = Host::new(
            engine,
            Output::Hal(Arc::new(Mutex::new(backend))),
            &info.device_name,
            info.sample_rate,
            None,
        );
        // The output's latency is known before any input opens (Task 8
        // left live hosts with an empty `InputInfo`).
        h.inputs = InputInfo::of(&info);
        Ok(h)
    }

    #[cfg(not(target_os = "macos"))]
    fn create_live(_project: f64) -> Result<Host, VizError> {
        Err(VizError::NoDevice {
            message: "live audio is macOS-only; set VIZDAW_AUDIO=offline".into(),
        })
    }

    fn new(
        engine: Engine,
        output: Output,
        device: &str,
        sample_rate: f64,
        log: Option<EventLog>,
    ) -> Host {
        Host {
            output,
            engine,
            owner: 0,
            factory_owner: 0,
            plugins_in_engine: false,
            seq: SeqId(0),
            looped: false,
            want_play: false,
            until: None,
            resumed_at: None,
            epoch: next_epoch(),
            device: device.to_string(),
            sample_rate,
            project_rate: sample_rate,
            rate_mismatch: false,
            report: Vec::new(),
            faults: Vec::new(),
            faulted: std::collections::BTreeSet::new(),
            log,
            injected: Vec::new(),
            inputs: InputInfo::default(),
            input_open: false,
            input_changes: 0,
            record_hold: false,
            pending_input: None,
            opened_device: None,
            input_failed: None,
            measure: None,
            generation: GENERATIONS.fetch_add(1, Ordering::Relaxed),
            job: None,
            broken: false,
        }
    }

    /// The transport plays (or was asked to), or a recording holds the
    /// engine. A song that ended by itself is seen first (pre-flight M3).
    fn busy(&mut self) -> bool {
        self.collect_faults();
        self.record_hold || self.want_play || self.engine.state() == PlayState::Playing
    }

    /// The engine monitors or plays thru: it has audio input open or
    /// routes for monitored audio or MIDI sources.
    fn wants_live(&self) -> bool {
        self.input_open || spec_wants_live(self.engine.inputs())
    }

    /// Ends the installed measurement (its sounding note ends) and makes
    /// one input change (pre-flight I1, M1; Task 9 review I2): to `want`
    /// when given (what the current document asks), else to what was
    /// asked while it ran.
    fn end_measuring(&mut self, want: Option<bool>) -> Result<(), VizError> {
        self.measure = None;
        let removed = self.engine.stop_measure().map_err(VizError::from);
        // A good measurement stands when the input change after it fails:
        // the failure is the device's, kept as a fault (plan 5d-1 Ruling
        // 22; 5c-2 final review finding 12). Since Task 7 the change runs
        // on the input thread, so its failure is asynchronous and reaches
        // the faults from there: `ensure_input` cannot fail the call.
        let want = want.or(self.pending_input.take());
        self.pending_input = None;
        self.settle_input(want);
        removed
    }

    /// Applies `want` (asked while a record hold or a measurement kept the
    /// input as it was), and moves an input left open to a device chosen
    /// meanwhile: closed, then reopened (5d-2 final review I1). An input
    /// thread at work checks the device itself when it hands back
    /// ([`Host::finish_job`]).
    fn settle_input(&mut self, want: Option<bool>) {
        let stale =
            self.job.is_none() && self.input_open && self.opened_device != *lock(&INPUT_DEVICE);
        match want {
            Some(true) | None if stale => {
                self.ensure_input(false);
                self.ensure_input(true);
            }
            Some(w) => self.ensure_input(w),
            None => {}
        }
    }

    /// Opens or closes a live output's input to match `want` (spec §6.1),
    /// restarting the device with the same renderer on the input thread
    /// (plan 5d-1 Ruling 13). Offline outputs keep their fake input.
    /// While a record tap, a measurement or the input thread is at work,
    /// the change waits for it ([`Host::end_record_hold`],
    /// [`Host::end_measuring`], [`Host::finish_job`]). It never fails:
    /// a device or permission failure is a fault (`input_failed`), from
    /// the input thread when the change runs there (plan 5d-1 Ruling 22).
    fn ensure_input(&mut self, want: bool) {
        if self.record_hold || self.measure.is_some() || self.job.is_some() {
            self.pending_input = Some(want);
            return;
        }
        let was = self.input_open;
        self.apply_input(want);
        if self.input_open != was {
            self.input_changes += 1;
        }
    }

    /// Open or closed, as the input was last asked to end: the change
    /// waiting, else the input thread's, else as it is (Task 7 parked r1).
    fn asked_input(&self) -> bool {
        self.pending_input
            .or(self.job.as_ref().map(|j| j.want))
            .unwrap_or(self.input_open)
    }

    /// Ends the hold a record tap put on the input and applies what was
    /// asked meanwhile.
    fn end_record_hold(&mut self) {
        self.record_hold = false;
        let want = self.pending_input.take();
        self.settle_input(want);
    }

    fn apply_input(&mut self, want: bool) {
        #[cfg(target_os = "macos")]
        if let Output::Hal(b) = &self.output {
            let backend = Arc::downgrade(b);
            let want = want && live_input_allowed(&mut self.faults);
            if self.blocked_by_failure(want) || want == self.input_open {
                return;
            }
            let cfg = live_audio_config(self.project_rate, want);
            // The job opens the device in `cfg`: one read (Task 1 rr-m1).
            let device = cfg.input_device.clone();
            self.start_job(Opener::Hal { backend, cfg }, want, device);
            return;
        }
        #[cfg(all(feature = "test-hooks", target_os = "macos"))]
        if want && FAKE_MIC_DENIED.load(Ordering::Relaxed) {
            admit_input(
                platform_macos::permission::MicAccess::Denied,
                &mut self.faults,
            );
        }
        if self.blocked_by_failure(want) {
            return;
        }
        #[cfg(feature = "test-hooks")]
        if want != self.input_open {
            if want && take_fake_failure() {
                let f = format!(
                    "{}: the fake device is busy",
                    crate::record::INPUT_FAILED_PREFIX
                );
                self.faults.push(f.clone());
                self.input_failed = Some(f);
                return;
            }
            let delay = Duration::from_millis(INPUT_OPEN_DELAY_MS.load(Ordering::Relaxed));
            let prompt = if want {
                lock(&FAKE_PROMPT).take()
            } else {
                None
            };
            if !delay.is_zero() || prompt.is_some() {
                let device = lock(&INPUT_DEVICE).clone();
                self.start_job(Opener::Fake { delay, prompt }, want, device);
                return;
            }
        }
        // The fake input stays; what would be open is tracked.
        if want != self.input_open {
            self.engine.clear_input_peaks();
            if want {
                self.opened_device = lock(&INPUT_DEVICE).clone();
            }
        }
        self.input_open = want;
    }

    /// Whether an input that failed to open stops this change (Ruling 15:
    /// it is not tried again until the input or the arm of a lane changes,
    /// or another input device is chosen): it is reported again after Play
    /// cleared the faults. Closing always clears the failure.
    fn blocked_by_failure(&mut self, want: bool) -> bool {
        if !want {
            self.input_failed = None;
            return false;
        }
        match &self.input_failed {
            Some(f) => {
                if !self.faults.contains(f) {
                    self.faults.push(f.clone());
                }
                true
            }
            None => false,
        }
    }

    /// Starts the input thread on `opener` for an input that should end
    /// `want` (Ruling 13), opening `device`.
    #[cfg_attr(
        not(any(target_os = "macos", feature = "test-hooks")),
        allow(dead_code)
    )]
    fn start_job(&mut self, opener: Opener, want: bool, device: Option<String>) {
        let flags = Arc::new(JobFlags::default());
        let (generation, theirs) = (self.generation, Arc::clone(&flags));
        let spawned = std::thread::Builder::new()
            .name("vizdaw-input".into())
            .spawn(move || {
                if let Some(done) = open_input(opener, want, &theirs) {
                    hand_back(generation, &theirs, want, done);
                }
            });
        match spawned {
            Ok(t) => {
                self.job = Some(InputJob {
                    flags,
                    thread: Some(t),
                    want,
                    device,
                })
            }
            Err(e) => {
                // Said once and not tried again until something changes,
                // as a failed open (Task 7 review m9).
                let f = format!(
                    "{}: no input thread ({e})",
                    crate::record::INPUT_FAILED_PREFIX
                );
                if !self.faults.contains(&f) {
                    self.faults.push(f.clone());
                }
                if want {
                    self.input_failed = Some(f);
                }
            }
        }
    }

    /// Takes what the input thread found (under the host lock), then
    /// applies what was asked meanwhile.
    fn finish_job(&mut self, want: bool, done: Opened) {
        let job = self.job.take();
        let was = self.input_open;
        // A reopened device's peaks were cleared between its stop and
        // start (Task 4 parked item); clearing now would wipe fresh ones.
        #[cfg_attr(not(target_os = "macos"), allow(unused_mut))]
        let mut cleared = false;
        match done {
            #[cfg(target_os = "macos")]
            Opened::Device { info, env, failed } => {
                self.engine.set_rt_env(env);
                self.input_open = info.input_channels > 0;
                self.inputs = InputInfo::of(&info);
                cleared = true;
                if let Some(f) = failed {
                    if !self.faults.contains(&f) {
                        self.faults.push(f.clone());
                    }
                    self.input_failed = Some(f);
                }
            }
            #[cfg(target_os = "macos")]
            Opened::Broken(why) => {
                let f = format!("the audio device could not reopen: {why}");
                if !self.faults.contains(&f) {
                    self.faults.push(f);
                }
                // No device under it: the next claim replaces this host
                // (pre-flight M7).
                self.broken = true;
            }
            Opened::Denied => {
                #[cfg(target_os = "macos")]
                admit_input(
                    platform_macos::permission::MicAccess::Denied,
                    &mut self.faults,
                );
                // The fake user said no: macOS reads denied from now on.
                #[cfg(feature = "test-hooks")]
                FAKE_MIC_DENIED.store(true, Ordering::Relaxed);
                self.input_open = false;
            }
            Opened::Unanswered => {
                // Asked again at the next arm or input edit (Ruling 15).
                let f = PROMPT_UNANSWERED.to_string();
                if !self.faults.contains(&f) {
                    self.faults.push(f.clone());
                }
                self.input_failed = Some(f);
                self.input_open = false;
            }
            #[cfg(feature = "test-hooks")]
            Opened::Fake => self.input_open = want,
        }
        let _ = want;
        if let Some(j) = job.as_ref().filter(|j| j.want && self.input_open) {
            self.opened_device = j.device.clone();
        }
        if self.input_open != was {
            self.input_changes += 1;
            // No peak held under the old config outlives it.
            if !cleared {
                self.engine.clear_input_peaks();
            }
        }
        // A device chosen while this job opened the one before (5d-2 Task 1
        // review I1): closed, then reopened with the new one. Under a record
        // hold or a measurement the pair waits for it to end
        // ([`Host::settle_input`]).
        let stale =
            self.input_open && job.is_some_and(|j| j.want && j.device != *lock(&INPUT_DEVICE));
        // A broken device is not tried again: the next claim replaces the
        // host, which opens what the lanes want (Task 7 review m4).
        if let Some(w) = self.pending_input.take().filter(|_| !self.broken) {
            if w && stale {
                self.ensure_input(false);
                self.ensure_input(true);
            } else {
                self.ensure_input(w);
            }
        } else if stale && !self.broken {
            self.ensure_input(false);
            self.ensure_input(true);
        }
    }

    /// Makes `owner`'s plugin host the engine's insert factory (if it is
    /// not already) and activates its plugins for this engine's rate and
    /// block size.
    fn attach(&mut self, owner: u64, plugins: Option<&mut PluginHost>) -> bool {
        let mut restarted = false;
        let block = self.engine.block_frames();
        if self.factory_owner != owner {
            match plugins.as_deref() {
                Some(p) => {
                    self.engine.set_insert_factory(Box::new(p.factory()));
                    self.factory_owner = owner;
                }
                // No plugin host here (a call off the session's plugin
                // thread, e.g. the app's Measure task; 5d-2 Task 8 review
                // I1): another session's instances must not serve this
                // document, so the engine gets none, and nobody owns the
                // factory: `owner`'s next claim with its host installs it
                // (on a fresh engine when this one loaded plugin slots).
                None => {
                    self.engine.set_insert_factory(Box::new(NoInserts));
                    self.factory_owner = 0;
                }
            }
        }
        if let Some(p) = plugins {
            // Instances unloaded since (a plugin turned off): the engine
            // must not keep playing them.
            for id in p.take_evicted() {
                self.engine.restart_instance(id);
                restarted = true;
            }
            let (rate, max) = p.config();
            if rate != self.sample_rate || max < block {
                // Processors still in the engine are made for the old
                // configuration: the engine drops them (they come back and
                // are activated again; the host then owes a load).
                for id in p.reconfigure(self.sample_rate, block.max(max)) {
                    self.engine.restart_instance(id);
                    restarted = true;
                }
            }
        }
        plugins::set_config(self.sample_rate, block);
        restarted
    }

    fn load(
        &mut self,
        owner: u64,
        doc: &Document,
        seq: SeqId,
        looped: bool,
    ) -> Result<(), VizError> {
        LOADS.fetch_add(1, Ordering::Relaxed);
        self.plugins_in_engine |= has_slots(doc);
        self.engine.set_media(media_of(owner));
        self.engine.set_inputs(input_spec(doc, seq));
        self.engine.set_return_dests(return_dests(doc, seq));
        // Off strips play no automation unless it is suspended (plan 5e-1
        // Ruling 3).
        self.engine
            .set_automation_suspended(crate::automation::suspended());
        let report = self.engine.load(doc, seq)?;
        self.report = report_lines(&report);
        self.engine.set_loop(looped)?;
        self.looped = looped;
        Ok(())
    }

    /// Reads the fault bits into the sticky list.
    fn collect_faults(&mut self) {
        for name in self.engine.faults().names() {
            if !self.faults.iter().any(|f| f == name) {
                self.faults.push(name.to_string());
            }
        }
        if self.engine.take_ended() {
            self.want_play = false;
        }
    }

    /// Device events reported since the last call (plus injected ones).
    fn take_events(&mut self) -> Vec<BackendEvent> {
        #[allow(unused_mut)]
        let mut events = std::mem::take(&mut self.injected);
        match &mut self.output {
            Output::Offline(_) => {}
            // Not while the input thread reopens it: the next poll reads
            // them (plan 5d-1 Ruling 13).
            #[cfg(target_os = "macos")]
            Output::Hal(b) => match b.try_lock() {
                Ok(mut b) => events.extend(b.take_events()),
                Err(std::sync::TryLockError::Poisoned(p)) => {
                    events.extend(p.into_inner().take_events())
                }
                Err(std::sync::TryLockError::WouldBlock) => {}
            },
        }
        events
    }

    /// Reopens a live device as it is (input open or not) with the I/O
    /// buffer size now asked for (plan 5d-1 Ruling 9; review I1). If the
    /// device refuses it, it is reopened at `old` and the refusal comes
    /// back as [`Reopened::Kept`]; if that fails too, [`Reopened::Lost`].
    /// Offline outputs keep their blocks.
    fn reopen(&mut self, old: u32) -> Reopened {
        match &mut self.output {
            Output::Offline(_) => {
                #[cfg(feature = "test-hooks")]
                {
                    let refused = lock(&REFUSED_BUFFERS).clone();
                    let new = crate::audio::io_buffer_frames();
                    if refused.contains(&new) {
                        let e = VizError::Engine {
                            message: format!("the device refused a buffer of {new} frames"),
                        };
                        return if refused.contains(&old) {
                            Reopened::Lost(e)
                        } else {
                            Reopened::Kept(e)
                        };
                    }
                }
                let _ = old;
                Reopened::Done
            }
            #[cfg(target_os = "macos")]
            Output::Hal(b) => {
                // `set_buffer_frames` waited for the input thread: the
                // device is free.
                let mut b = lock(b);
                let b = &mut *b;
                let Ok(mut r) = b.stop() else {
                    return Reopened::Lost(VizError::Engine {
                        message: "the audio device would not stop".into(),
                    });
                };
                let cfg = live_audio_config(self.project_rate, self.input_open);
                if self.input_open {
                    self.opened_device = cfg.input_device.clone();
                }
                // The size asked for is not softened: a refusal is an
                // error, and the old size comes back.
                let (info, refused) = match b.open(&cfg) {
                    Ok(i) => (i, None),
                    Err(e) => {
                        let mut back = cfg;
                        back.buffer_frames = Some(old);
                        match open_or_default(back, |c| b.open(c)) {
                            Ok(i) => (i, Some(VizError::from(e))),
                            Err(e) => return Reopened::Lost(e.into()),
                        }
                    }
                };
                if info.buffer_frames > r.config().max_frames {
                    r.prepare(info.buffer_frames);
                }
                // The channel count may change with the reopen: no peak
                // outlives the config (final review m3), cleared while
                // stopped.
                r.clear_input_peaks();
                if let Err(e) = b.start(r) {
                    return Reopened::Lost(e.into());
                }
                self.engine
                    .set_rt_env(rt_env(b, info.buffer_frames, info.sample_rate));
                self.input_open = info.input_channels > 0;
                self.inputs = InputInfo::of(&info);
                match refused {
                    None => Reopened::Done,
                    Some(e) => Reopened::Kept(e),
                }
            }
        }
    }

    /// Restarts the device, preparing a larger buffer if needed (as the
    /// CLI does).
    fn restart_output(&mut self, frames: u32) -> Result<(), VizError> {
        match &mut self.output {
            Output::Offline(_) => {
                let _ = frames;
                Ok(())
            }
            // The input thread's reopen takes the new size anyway.
            #[cfg(target_os = "macos")]
            Output::Hal(_) if self.job.is_some() => Ok(()),
            #[cfg(target_os = "macos")]
            Output::Hal(b) => {
                let mut b = lock(b);
                let mut r = b.stop()?;
                if frames > r.config().max_frames {
                    r.prepare(frames);
                }
                b.start(r)?;
                // The workers follow the new block period.
                self.engine.set_rt_env(rt_env(&b, frames, self.sample_rate));
                Ok(())
            }
        }
    }
}

/// How a buffer-size reopen ended (review I1).
enum Reopened {
    /// The device runs at the new size.
    Done,
    /// The device refused it and runs at the old size again.
    Kept(VizError),
    /// Neither size could be opened: the device is not running.
    Lost(VizError),
}

/// Opens with `cfg`; if the device refuses the buffer size asked for,
/// once more with the device's own (review I2: a refused size must not
/// stop the device opening at all).
#[cfg(target_os = "macos")]
fn open_or_default<T, E>(
    cfg: core_engine::backend::AudioConfig,
    mut open: impl FnMut(&core_engine::backend::AudioConfig) -> Result<T, E>,
) -> Result<T, E> {
    match open(&cfg) {
        Err(_) if cfg.buffer_frames.is_some() => open(&core_engine::backend::AudioConfig {
            buffer_frames: None,
            ..cfg
        }),
        r => r,
    }
}

/// A live output's configuration: the project rate, input when `input`
/// (from the chosen input device).
#[cfg(target_os = "macos")]
fn live_audio_config(project: f64, input: bool) -> core_engine::backend::AudioConfig {
    core_engine::backend::AudioConfig {
        sample_rate: Some(project),
        buffer_frames: Some(crate::audio::io_buffer_frames()),
        input_enabled: input,
        input_device: lock(&INPUT_DEVICE).clone(),
        ..core_engine::backend::AudioConfig::default()
    }
}

/// Live blocks may use 80 % of their duration (3a Plan Ruling 18).
pub const DEADLINE: f64 = 0.8;

/// The live engine's settings: the spec's default worker count
/// `min(performance cores − 1, 7)` (3a Plan Ruling 20) and the deadline.
/// The offline backend keeps 0 workers and no deadline.
#[cfg(target_os = "macos")]
pub fn live_config(sample_rate: f64) -> EngineConfig {
    let mut cfg = EngineConfig::new(sample_rate);
    cfg.workers = platform_macos::rt::default_workers();
    cfg.deadline = Some(DEADLINE);
    cfg.stream = crate::audio::stream_config();
    cfg
}

/// The device's block period and workgroup, for the workers.
#[cfg(target_os = "macos")]
fn rt_env(
    backend: &platform_macos::hal::CoreAudioBackend,
    frames: u32,
    sample_rate: f64,
) -> core_engine::pool::RtEnv {
    core_engine::pool::RtEnv {
        period_ns: (f64::from(frames) * 1e9 / sample_rate.max(1.0)).round() as u64,
        workgroup: backend.workgroup(),
    }
}

/// A rebuild `handle_events` started: the old host is out of the slot;
/// [`rebuild`] finishes it without the host lock.
struct Rebuild {
    old: Host,
    at: Tick,
    playing: bool,
    /// The device still renders: its note-offs get `DRAIN` to play.
    drain: bool,
}

/// Handles device events for the owner (mirrors `vizdaw-play`'s live
/// loop): a buffer-size change restarts the output at once; a lost device,
/// a new default device or a rate change take the host out of `slot` and
/// return the rebuild, which the caller runs with [`rebuild`] after
/// dropping the host lock (Plan 4d Ruling 1: the drain sleeps 300 ms).
fn handle_events(slot: &mut Option<Host>) -> Result<Option<Rebuild>, VizError> {
    let Some(h) = slot.as_mut() else {
        return Ok(None);
    };
    for ev in h.take_events() {
        match ev {
            BackendEvent::BufferSizeChanged(n) => {
                if let Some(h) = slot.as_mut() {
                    h.restart_output(n)?;
                }
            }
            BackendEvent::DeviceLost
            | BackendEvent::DefaultDeviceChanged
            | BackendEvent::SampleRateChanged(_) => {
                let Some(mut old) = slot.take() else {
                    return Ok(None);
                };
                let at = old.engine.transport_tick();
                // Playback that ended by itself is not resumed.
                if old.engine.take_ended() {
                    old.want_play = false;
                }
                // `want_play` is the user's intent (the engine applies Play
                // and Stop a block late, so its state can lag it). A song in
                // its release tail is past its end: a Play would restart it.
                let playing = old.want_play && old.engine.state() != PlayState::Tail;
                let drain = !matches!(ev, BackendEvent::DeviceLost);
                if drain {
                    let _ = old.engine.stop();
                }
                // The rebuilt host replaces any later queued events.
                return Ok(Some(Rebuild {
                    old,
                    at,
                    playing,
                    drain,
                }));
            }
        }
    }
    Ok(None)
}

/// Finishes a rebuild without the host lock: lets a still-running output
/// play its note-offs, drops it (output first, then the engine, whose
/// processors go back to the plugin host), makes a new host for the
/// owner, recompiles and resumes at the same tick. The owner's plugins
/// follow the new engine's rate (`attach`). A failed rebuild leaves no
/// host; a host another call made meanwhile wins.
fn rebuild(r: Rebuild, doc: &Document, plugins: Option<&mut PluginHost>) -> Result<(), VizError> {
    let Rebuild {
        old,
        at,
        playing,
        drain,
    } = r;
    let (owner, seq, looped, until) = (old.owner, old.seq, old.looped, old.until);
    if drain {
        std::thread::sleep(DRAIN);
    }
    // Read once the old output has stopped (dropped first, below).
    let overrides = old.engine.overrides();
    drop(old);
    #[cfg(feature = "test-hooks")]
    if let Some((reached, release)) = lock(&REBUILD_GATE).take() {
        let _ = reached.send(());
        let _ = release.recv_timeout(Duration::from_secs(30));
    }
    let mut new = Host::create(project_rate(doc))?;
    // Without the host lock (a cold CoreMIDI server is slow; M7: live
    // hosts only).
    if new.is_live() {
        let fault = connect_midi_sources();
        note_midi_fault(&mut new, fault);
    }
    new.attach(owner, plugins);
    new.ensure_input(wants_audio_input(doc, seq));
    // Automated values held while stopped survive the rebuild (Plan 4d
    // Ruling 13, final review Minor 1).
    if let Some(o) = overrides {
        new.engine.seed_overrides(o);
    }
    new.load(owner, doc, seq, looped)?;
    new.engine.locate_tick(at)?;
    new.resumed_at = Some(at);
    if playing {
        match until {
            Some(t) => new.engine.play_until(t)?,
            None => new.engine.play()?,
        }
        new.want_play = true;
        new.until = until;
    }
    new.owner = owner;
    new.seq = seq;
    let mut slot = lock(&HOST);
    let spare = if slot.is_none() {
        install(&mut slot, new);
        None
    } else {
        Some(new)
    };
    drop(slot);
    drop(spare);
    Ok(())
}

/// The compile report as messages: warnings, unroutable events, then the
/// mixer graph's (placeholders as "plugin N: why", other warnings, and why
/// a graph was not used).
fn report_lines(r: &CompileReport) -> Vec<String> {
    let mut lines = r.warnings.clone();
    lines.extend(r.unroutable.iter().map(|u| u.reason.clone()));
    let g = &r.graph;
    lines.extend(
        g.placeholders
            .iter()
            .map(|(id, why)| format!("{id}: {why}")),
    );
    lines.extend(g.warnings.iter().cloned());
    lines.extend(g.error.iter().map(|e| format!("mixer graph not used: {e}")));
    lines
}

/// Where a session's transport stands, as the host sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct HostState {
    /// Playing or in the release tail.
    pub playing: bool,
    /// The loaded sequence.
    pub seq: SeqId,
    /// Position.
    pub tick: Tick,
    /// Host time (ns) stamped right after `tick` was read.
    pub host_time_ns: u64,
    /// Stream rate.
    pub sample_rate: f64,
    /// Device label.
    pub device: String,
    /// Sticky fault names.
    pub faults: Vec<String>,
    /// Last compile report.
    pub report: Vec<String>,
    /// The device runs at another rate than the project: regions are
    /// muted (Ruling 20).
    pub rate_mismatch: bool,
    /// Record's count-in is playing (plan 5d-1 Ruling 7).
    pub counting_in: bool,
}

/// A Play [`play`] prepared; [`finish_play`] waits for the disk and
/// sends it.
#[derive(Debug)]
#[must_use = "the Play is sent by `finish_play`"]
pub struct HostPlay {
    owner: u64,
    epoch: u64,
    pending: PendingPlay,
    /// The host is live: `finish_play` connects the MIDI sources, without
    /// the host lock (M7, Task 12 review).
    connect_midi: bool,
    /// The tick this Play located to, when it located (plan 5e-1 review
    /// I-2: the engine's published position shows the old one until the
    /// renderer applies it).
    located: Option<Tick>,
}

/// What a [`claim_host`] waits for before it takes the engine.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Idle {
    /// Nothing but a measurement (Play).
    Any,
    /// A stopped transport and no recording too (Measure).
    Stopped,
    /// And no other session's input thread at work (an arm's claim, plan
    /// 5d-1 Ruling 16).
    Untouched,
}

/// Takes the engine host for `owner` as Play does (and Measure, plan 5c-2
/// Ruling 9): device events nobody polled are handled; a host holding
/// another session's plugins, or opened for another project rate, is
/// replaced; one is made when there is none; `owner`'s plugin host
/// becomes the insert factory and is synced. `Busy` while a measurement
/// runs, and, as `idle` says, also while the transport plays or a
/// recording holds the engine, or another session's input thread is at
/// work, before anything is replaced. Returns the locked slot (holding the
/// host) and whether instances were restarted.
fn claim_host(
    owner: u64,
    doc: &Document,
    mut plugins: Option<&mut PluginHost>,
    idle: Idle,
) -> Result<(std::sync::MutexGuard<'static, Option<Host>>, bool), VizError> {
    let idle_only = idle != Idle::Any;
    let project = project_rate(doc);
    let mut slot = loop {
        let mut slot = lock(&HOST);
        if let Some(h) = slot.as_mut() {
            if h.measure.is_some() {
                return Err(measuring_busy());
            }
            if idle_only && h.busy() {
                return Err(VizError::Busy {
                    message: STOP_FIRST.into(),
                });
            }
            // Measure waits for its input with no measurement installed
            // yet: an arm in another window must not take that engine
            // (Task 8 review m1).
            if idle == Idle::Untouched && h.owner != owner && h.job.is_some() {
                return Err(VizError::Busy {
                    message: INPUT_OPENING.into(),
                });
            }
        }
        // Device events nobody polled (the host had no owner, or another
        // one): a rebuild-class event drops the host so a fresh one opens
        // below.
        if let Some(h) = slot.as_mut().filter(|h| h.owner != owner) {
            for ev in h.take_events() {
                match ev {
                    BackendEvent::BufferSizeChanged(n) => h.restart_output(n)?,
                    _ => {
                        *slot = None;
                        break;
                    }
                }
            }
        } else if let Some(r) = handle_events(&mut slot)? {
            // The drain and the rebuild run without the host lock; then
            // start over (another call may have changed the host).
            drop(slot);
            rebuild(r, doc, plugins.as_deref_mut())?;
            continue;
        }
        // Another session's plugins played here, or the host was opened for
        // another project rate: a fresh engine (for plugins: so none of its
        // instances is taken for this document's same-numbered slots; for
        // rates: so the device is asked for this project's, Ruling 20).
        // Or its device could not reopen (pre-flight M7).
        // A claim with no plugin host keeps an engine nobody's factory
        // feeds: a fresh one would get no plugins either.
        let hostless = plugins.is_none();
        if slot.as_ref().is_some_and(|h| {
            (h.factory_owner != owner && h.plugins_in_engine && !(hostless && h.factory_owner == 0))
                || !same_rate(h.project_rate, project)
                || h.broken
        }) {
            let mut old = slot.take();
            if let Some(h) = old.as_mut() {
                // Its played and auditioned notes end before the engine
                // goes (4b Plan Ruling 1).
                let _ = h.engine.stop();
            }
            // The note-offs play and the old host is dropped without the
            // lock, so no other host call (the UI's timers) waits on them.
            // Then start over: another call may have changed the host
            // meanwhile.
            drop(slot);
            retire(old);
            continue;
        }
        break slot;
    };
    if slot.is_none() {
        install(&mut slot, Host::create(project)?);
    }
    let host = slot.as_mut().ok_or_else(|| VizError::Internal {
        message: "engine host missing".into(),
    })?;
    let restarted = host.attach(owner, plugins.as_deref_mut());
    if let Some(p) = plugins {
        // Slots still loading play as placeholders; `service` loads again
        // once they are in.
        crate::recovery::before_sync();
        p.sync(doc);
    }
    Ok((slot, restarted))
}

/// Plays `seq` of `doc` for session `owner`, up to the disk wait (spec
/// §5.5): the caller drops every lock and calls [`finish_play`]. A
/// different owner, sequence or an explicit `start` reloads (stopping
/// whoever played); otherwise the engine resumes where it stopped.
/// `until` stops it there (the loop ignored; plan 4f Ruling 9). `Busy`
/// while a measurement runs (plan 5c-2 Ruling 9).
pub fn play(
    owner: u64,
    doc: &Document,
    seq: SeqId,
    start: Option<Tick>,
    until: Option<Tick>,
    looped: bool,
    plugins: Option<&mut PluginHost>,
) -> Result<HostPlay, VizError> {
    let (mut slot, restarted) = claim_host(owner, doc, plugins, Idle::Any)?;
    let host = slot.as_mut().ok_or_else(|| VizError::Internal {
        message: "engine host missing".into(),
    })?;
    host.faults.clear();
    // Still unavailable: said again (Task 12 review M2).
    let midi_fault = lock(&MIDI_UNAVAILABLE).clone();
    note_midi_fault(host, midi_fault);
    if host.owner != owner {
        // Another session's recording ends below: its hold goes with it.
        host.record_hold = false;
        host.pending_input = None;
    }
    host.ensure_input(wants_audio_input(doc, seq));
    let mut located = None;
    if host.owner != owner || host.seq != seq || start.is_some() {
        located = Some(start.unwrap_or(Tick::ZERO));
        host.engine.stop()?;
        if host.owner != owner {
            // Another session's recording ends here: its tap must not
            // capture this one's playback.
            host.engine.stop_recording()?;
        }
        host.load(owner, doc, seq, looped)?;
        host.engine.locate_tick(start.unwrap_or(Tick::ZERO))?;
        host.owner = owner;
        host.seq = seq;
    } else if restarted {
        // Resumed with restarted instances: compile their placeholders.
        host.load(owner, doc, seq, looped)?;
    }
    // The intent is Play from here on: a rebuild during the wait resumes
    // playing, and a Stop during it cancels this Play.
    let pending = host.engine.begin_play(until);
    host.want_play = true;
    host.until = until;
    host.epoch = next_epoch();
    Ok(HostPlay {
        owner,
        epoch: host.epoch,
        pending,
        connect_midi: host.is_live(),
        located,
    })
}

impl HostPlay {
    /// Plays count-in `c` before the transport rolls (Record's, plan 5d-1
    /// Ruling 7).
    pub fn count_in(&mut self, c: Option<core_engine::count_in::CountIn>) {
        self.pending.set_count_in(c);
    }

    /// The tick this Play located to; `None` when it resumes where the
    /// transport stands.
    pub fn located(&self) -> Option<Tick> {
        self.located
    }
}

/// Waits for the disk (up to `LOCATE_WAIT`, without any lock: the UI's
/// polls and every other host call go on meanwhile), then sends the
/// Play, unless a Stop, another Play, a release or a new engine came in
/// the meantime (they win; a rebuilt engine resumes on its own).
pub fn finish_play(p: HostPlay) -> Result<(), VizError> {
    p.pending.wait();
    if p.connect_midi {
        connect_and_report();
    }
    let mut slot = lock(&HOST);
    match slot.as_mut() {
        Some(h) if h.owner == p.owner && h.epoch == p.epoch => {
            Ok(h.engine.finish_play(p.pending)?)
        }
        _ => Ok(()),
    }
}

/// Stops if `owner` is playing.
pub fn stop(owner: u64) -> Result<(), VizError> {
    let mut slot = lock(&HOST);
    match slot.as_mut() {
        Some(h) if h.owner == owner => {
            h.want_play = false;
            h.until = None;
            h.epoch = next_epoch();
            Ok(h.engine.stop()?)
        }
        _ => Ok(()),
    }
}

/// Moves `owner`'s transport. Returns false when `owner` does not hold
/// the engine (the caller keeps the position for its next Play).
pub fn locate(owner: u64, tick: Tick) -> Result<bool, VizError> {
    let mut slot = lock(&HOST);
    match slot.as_mut() {
        Some(h) if h.owner == owner => {
            h.engine.locate_tick(tick)?;
            Ok(true)
        }
        _ => Ok(false),
    }
}

/// Turns looping on or off if `owner` holds the engine.
pub fn set_loop(owner: u64, on: bool) -> Result<(), VizError> {
    let mut slot = lock(&HOST);
    match slot.as_mut() {
        Some(h) if h.owner == owner => {
            h.engine.set_loop(on)?;
            h.looped = on;
            Ok(())
        }
        _ => Ok(()),
    }
}

/// Sends an audition note through `owner`'s engine (Plan Ruling 9); false
/// when `owner` does not hold the engine or it has no target for `dest`.
pub fn audition(
    owner: u64,
    doc: &Document,
    dest: DestId,
    pitch: u8,
    velocity: u16,
    on: bool,
) -> Result<bool, VizError> {
    let mut slot = lock(&HOST);
    match slot.as_mut() {
        Some(h) if h.owner == owner => Ok(h.engine.audition(doc, dest, pitch, velocity, on)?),
        _ => Ok(false),
    }
}

/// After an edit: starts loading the plugins `doc` gained (and drops the
/// ones it lost), then recompiles if `owner` holds the engine and no
/// plugin is loading (otherwise `service` loads once they are in).
pub fn reload(
    owner: u64,
    doc: &Document,
    looped: bool,
    mut plugins: Option<&mut PluginHost>,
) -> Result<(), VizError> {
    let ready = plugins.as_deref_mut().is_none_or(|p| {
        crate::recovery::before_sync();
        p.sync(doc)
    });
    let mut slot = lock(&HOST);
    match slot.as_mut() {
        Some(h) if h.owner == owner => {
            #[cfg(feature = "test-hooks")]
            match lock(&RELOAD_FAULT).take() {
                Some(ReloadFault::Error) => {
                    return Err(VizError::Engine {
                        message: "injected reload fault".into(),
                    });
                }
                #[allow(clippy::panic)]
                Some(ReloadFault::Panic) => panic!("injected reload panic"),
                None => {}
            }
            let seq = h.seq;
            // The loaded sequence may have been deleted by the edit.
            if doc.sequence(seq).is_err() {
                let stopped = h.engine.stop().map_err(VizError::from);
                // A measurement of it ends with it (pre-flight I1), and
                // the input it opened closes: no session holds the engine
                // now, so no lane wants it (Task 9 re-review).
                let ended = if h.measure.is_some() {
                    h.end_measuring(Some(false))
                } else {
                    Ok(())
                };
                h.epoch = next_epoch();
                h.want_play = false;
                h.owner = 0;
                return stopped.and(ended);
            }
            if h.factory_owner == owner {
                // Instances unloaded since (a plugin turned off): dropped
                // by this load (or the one `service` runs once loads are
                // done).
                for id in plugins.map(PluginHost::take_evicted).unwrap_or_default() {
                    h.engine.restart_instance(id);
                }
            }
            if !ready {
                return Ok(());
            }
            // An edit armed or disarmed a lane: input follows (spec §6.1).
            h.ensure_input(wants_audio_input(doc, seq));
            h.load(owner, doc, seq, looped)
        }
        _ => Ok(()),
    }
}

/// Claims the engine for session `owner`'s sequence `seq` of `doc` while
/// it stands idle (plan 5d-1 Ruling 16, 5c carry I2): no session plays,
/// records or measures, and no other session's input thread is at work
/// (Measure's input wait among them, Task 8 review m1). It loads `seq`
/// stopped at `at` and opens the input its lanes want, so armed and
/// monitored lanes are heard and thru plays before the first Play.
/// Nothing when `owner` holds the engine with `seq` loaded already, or
/// when the engine is busy (never taken then). True when it claimed (Task
/// 8 review m2): the caller moves its position to the engine. `Busy`
/// with [`INPUT_OPENING`] when another session's input thread is at work:
/// the caller tries again later (Task 8 parked n2).
pub fn claim_idle(
    owner: u64,
    doc: &Document,
    seq: SeqId,
    at: Tick,
    looped: bool,
    plugins: Option<&mut PluginHost>,
) -> Result<bool, VizError> {
    // Held already, unless a claim with no plugin host (Measure off the
    // plugin thread) left the engine without this session's factory: this
    // one, with the host, installs it (5d-2 Task 8 re-review N1).
    // The heal keeps the engine's playhead (the session's cursor is unset
    // while it holds the engine; re-review-2 R2), across a retire too.
    let hosted = plugins.is_some();
    let held = match lock(&HOST).as_ref() {
        Some(h) if h.owner == owner && h.seq == seq => {
            if h.factory_owner == owner || !hosted {
                return Ok(false);
            }
            Some(h.engine.position_tick())
        }
        _ => None,
    };
    let at = held.unwrap_or(at);
    let (mut slot, _) = match claim_host(owner, doc, plugins, Idle::Untouched) {
        Ok(s) => s,
        // Another window's idle engine opening its input: the caller owes
        // the claim (Task 8 parked n2).
        Err(VizError::Busy { message }) if message == INPUT_OPENING => {
            return Err(VizError::Busy { message });
        }
        Err(VizError::Busy { .. }) => return Ok(false),
        Err(e) => return Err(e),
    };
    let host = slot.as_mut().ok_or_else(|| VizError::Internal {
        message: "engine host missing".into(),
    })?;
    if host.owner != owner {
        // Another session's held position and plugins go with its claim,
        // and so do its faults; a MIDI input still unavailable is said
        // again, as Play does (Task 8 review m3).
        host.record_hold = false;
        host.pending_input = None;
        host.faults.clear();
        // The old holder's failed open is not this arm's: the claim tries
        // the device again (Ruling 15; final review m2).
        host.input_failed = None;
        let midi_fault = lock(&MIDI_UNAVAILABLE).clone();
        note_midi_fault(host, midi_fault);
    }
    host.engine.stop()?;
    host.load(owner, doc, seq, looped)?;
    host.engine.locate_tick(at)?;
    host.owner = owner;
    host.seq = seq;
    host.want_play = false;
    host.until = None;
    host.epoch = next_epoch();
    host.ensure_input(wants_audio_input(doc, seq));
    Ok(true)
}

/// `owner`'s view of the transport, or `None` when it does not hold the
/// engine. For the owner, device events are handled first (`doc` is the
/// owner's document, needed to recompile after a rebuild).
pub fn state(
    owner: u64,
    doc: &Document,
    plugins: Option<&mut PluginHost>,
) -> Result<Option<HostState>, VizError> {
    let mut slot = lock(&HOST);
    match slot.as_ref() {
        Some(h) if h.owner == owner => {}
        _ => return Ok(None),
    }
    if let Some(r) = handle_events(&mut slot)? {
        drop(slot);
        rebuild(r, doc, plugins)?;
        slot = lock(&HOST);
    }
    let Some(h) = slot.as_mut().filter(|h| h.owner == owner) else {
        return Ok(None);
    };
    h.collect_faults();
    let playing = h.engine.state() != PlayState::Stopped;
    // The stamp follows the tick read directly: the playhead extrapolates
    // from this pair (spec §3.4).
    let tick = h.engine.position_tick();
    let host_time_ns = crate::clock::now_ns();
    Ok(Some(HostState {
        playing,
        seq: h.seq,
        tick,
        host_time_ns,
        sample_rate: h.sample_rate,
        device: h.device.clone(),
        faults: h.faults.clone(),
        report: h.report.clone(),
        rate_mismatch: h.rate_mismatch,
        counting_in: h.engine.counting_in(),
    }))
}

/// `(rate_mismatch, device rate)` if `owner` holds the engine.
pub fn device(owner: u64) -> Option<(bool, f64)> {
    lock(&HOST)
        .as_ref()
        .filter(|h| h.owner == owner)
        .map(|h| (h.rate_mismatch, h.sample_rate))
}

/// Drops the host if `owner` holds it and it was opened for another
/// project rate than `project` (a format change, or its undo; Ruling 20):
/// playback stops and the next Play asks the device for the new rate. The
/// note-offs drain without the host lock, as in [`reset`].
pub fn reset_if_owner(owner: u64, project: f64) {
    let mut slot = lock(&HOST);
    if !slot
        .as_ref()
        .is_some_and(|h| h.owner == owner && !same_rate(h.project_rate, project))
    {
        return;
    }
    let mut old = slot.take();
    drop(slot);
    if let Some(h) = old.as_mut() {
        let _ = h.engine.stop();
    }
    retire(old);
}

/// Sets the I/O buffer size and reopens the device with it, all under one
/// lock (pre-flight M4): `Busy` while a recording holds the engine or a
/// measurement runs. If the device refuses the size, it is reopened at the
/// old one (review I1); if that fails too, the host is retired. Either
/// way the setting is the size the device has, and the error comes back.
pub(crate) fn set_buffer_frames(frames: u32) -> Result<(), VizError> {
    // The input thread reopens the device too: its change ends first,
    // waited for without the lock (plan 5d-1 Ruling 13); then the checks
    // and the reopen run under one lock with nothing else at the device.
    let mut slot = loop {
        let slot = lock(&HOST);
        if !slot.as_ref().is_some_and(|h| h.job.is_some()) {
            break slot;
        }
        drop(slot);
        if !wait_input(INPUT_WAIT) {
            return Err(VizError::Busy {
                message: INPUT_OPENING.into(),
            });
        }
    };
    if let Some(h) = slot.as_ref() {
        if h.record_hold {
            return Err(VizError::Busy {
                message: "Stop recording to change the I/O buffer size.".into(),
            });
        }
        if h.measure.is_some() {
            return Err(VizError::Busy {
                message: "Wait for the measurement to finish.".into(),
            });
        }
    }
    let old = crate::audio::swap_io_buffer(frames);
    if old == frames {
        return Ok(());
    }
    let Some(h) = slot.as_mut() else {
        return Ok(());
    };
    match h.reopen(old) {
        Reopened::Done => Ok(()),
        Reopened::Kept(e) => {
            // The device runs at the old size; so does the setting.
            crate::audio::swap_io_buffer(old);
            Err(e)
        }
        Reopened::Lost(e) => {
            crate::audio::swap_io_buffer(old);
            // No device under it: the next claim builds a new host. The
            // error goes back to the caller; a fault pushed here would go
            // with the retired host (Task 6 parked n3).
            let mut gone = slot.take();
            *lock(&MIDI_TO) = None;
            drop(slot);
            if let Some(h) = gone.as_mut() {
                let _ = h.engine.stop();
            }
            retire(gone);
            Err(e)
        }
    }
}

/// Drops the engine when nobody plays, records, measures or monitors, so
/// the next one is made with the stream settings now asked for (plan 5d-1
/// Ruling 10); a busy engine is left alone, and so is one whose sequence
/// wants live input (pre-flight I2: dropping it would silence armed lanes
/// and thru until the next arm or Play).
pub fn drop_if_idle() {
    let mut slot = lock(&HOST);
    if !slot
        .as_mut()
        .is_some_and(|h| h.measure.is_none() && h.job.is_none() && !h.busy() && !h.wants_live())
    {
        return;
    }
    let mut old = slot.take();
    // No engine: nothing hears the sources until the next host.
    *lock(&MIDI_TO) = None;
    drop(slot);
    if let Some(h) = old.as_mut() {
        let _ = h.engine.stop();
    }
    retire(old);
}

/// Releases the engine if `owner` holds it (a session is closing).
pub fn release(owner: u64) {
    let mut slot = lock(&HOST);
    if let Some(h) = slot.as_mut() {
        if h.owner == owner {
            let _ = h.engine.stop();
            // Its measurement ends with it (pre-flight I1), and the input
            // it opened closes: no session holds the engine now (Task 9
            // re-review).
            if h.measure.is_some() {
                let _ = h.end_measuring(Some(false));
            }
            h.want_play = false;
            h.epoch = next_epoch();
            h.owner = 0;
            // Nothing listens for a window that is gone (Task 8 review
            // I1): an engine it left monitoring, playing thru or opening
            // its input goes now, as `drop_if_idle` drops it (a job still
            // at work is cancelled by the drop). Whether it was playing
            // does not matter (final review I1): the Stop above lands only
            // at the renderer's next block, so `busy()` would still see it
            // playing and leave the input open and monitored for nobody.
            // `retire` drains the note-offs the Stop sends.
            // `close` finishes any pass before it releases, so `record_hold`
            // is never set here today. It stays as a defence: a future path
            // that left the hold set would otherwise close the input under a
            // recording.
            if !h.record_hold && (h.wants_live() || h.job.is_some()) {
                let old = slot.take();
                *lock(&MIDI_TO) = None;
                drop(slot);
                retire(old);
            }
        }
    }
}

/// The main-thread timer's plugin step for session `owner` (spec §5,
/// 3b `PluginHost::service`): with the engine when `owner` plays through
/// it, so finished loads, restarts and edits that waited reach it; else
/// the host's own `idle`. An engine error is kept as a fault (the host
/// retries once something changes).
pub fn service(owner: u64, doc: &Document, plugins: &mut PluginHost) -> IdleReport {
    let mut slot = lock(&HOST);
    match slot.as_mut() {
        Some(h) if h.owner == owner && h.factory_owner == owner => {
            h.plugins_in_engine |= has_slots(doc);
            match plugins.service(&mut h.engine, doc, h.seq) {
                Ok(r) => {
                    if let Some(c) = &r.compiled {
                        h.report = report_lines(c);
                    }
                    r
                }
                Err(e) => {
                    let f = format!("plugin load: {e}");
                    if !h.faults.contains(&f) {
                        h.faults.push(f);
                    }
                    IdleReport::default()
                }
            }
        }
        _ => {
            // Not playing: slots the catalog released (a plugin enabled
            // again) still load.
            crate::recovery::before_sync();
            plugins.sync(doc);
            plugins.idle()
        }
    }
}

/// Whether `doc` has a plugin slot (an insert or a plugin instrument).
fn has_slots(doc: &Document) -> bool {
    doc.mixer().inserts().next().is_some()
        || doc.instruments().values().any(|d| {
            matches!(
                d.kind,
                core_model::document::DestKind::PluginInstrument { .. }
            )
        })
}

/// The session whose plugin host feeds the engine, if any.
pub fn factory_owner() -> Option<u64> {
    lock(&HOST)
        .as_ref()
        .map(|h| h.factory_owner)
        .filter(|o| *o != 0)
}

/// `owner`'s meters, DSP load, latency and fault-bypassed instances, or
/// `None` when it does not hold the engine.
pub fn levels(owner: u64) -> Option<crate::mixer::MixerLevels> {
    let mut slot = lock(&HOST);
    let h = slot.as_mut().filter(|h| h.owner == owner)?;
    h.faulted
        .extend(h.engine.plugin_faults().into_iter().map(|i| i.0));
    let load = h.engine.dsp_load();
    Some(crate::mixer::MixerLevels {
        meters: h
            .engine
            .meters()
            .into_iter()
            .map(|m| crate::mixer::MeterRow {
                strip: m.strip.0,
                peak_l: m.peak_l,
                peak_r: m.peak_r,
            })
            .collect(),
        dsp_load: load.last,
        dsp_peak: load.peak,
        latency_frames: h.engine.latency_frames(),
        faulted: h.faulted.iter().copied().collect(),
    })
}

/// Installs a record tap in `owner`'s engine (spec §6.2): the next block
/// that plays starts capturing. Fails only when `owner` does not hold the
/// engine or its command queue is full.
pub fn start_recording(owner: u64, req: &RecordRequest) -> Result<RecordHandles, VizError> {
    let mut slot = lock(&HOST);
    match slot.as_mut() {
        Some(h) if h.owner == owner => {
            if h.measure.is_some() {
                return Err(measuring_busy());
            }
            let handles = h.engine.start_recording(req)?;
            h.record_hold = true;
            Ok(handles)
        }
        _ => Err(VizError::Engine {
            message: "this document does not hold the audio engine".into(),
        }),
    }
}

/// Stops `owner`'s transport and removes its record tap (nothing when
/// another session holds the engine: its tap went with the old engine,
/// or that session's Play removed it).
pub fn stop_recording(owner: u64) -> Result<(), VizError> {
    let mut slot = lock(&HOST);
    match slot.as_mut() {
        Some(h) if h.owner == owner => {
            h.want_play = false;
            h.until = None;
            h.epoch = next_epoch();
            // Every step runs whatever an earlier one says (Task 11
            // re-review m1): the pass is over for the session either way,
            // so the input hold always goes and the deferred change
            // applies; the first error is returned.
            let stopped: Result<(), VizError> = {
                #[cfg(feature = "test-hooks")]
                let injected = STOP_RECORDING_FAULT.swap(false, Ordering::Relaxed);
                #[cfg(not(feature = "test-hooks"))]
                let injected = false;
                if injected {
                    Err(VizError::Engine {
                        message: "injected stop_recording fault".into(),
                    })
                } else {
                    h.engine.stop().map_err(VizError::from)
                }
            };
            let removed = h.engine.stop_recording().map_err(VizError::from);
            // The tap is out (or the session gave up on it): input follows
            // the lanes again.
            h.end_record_hold();
            stopped.and(removed)
        }
        _ => Ok(()),
    }
}

/// Starts measuring `dest`'s return latency for session `owner` (spec
/// §6.7; plan 5c-2 Ruling 9): claims the engine as Play would (`Busy`
/// while any session plays or records, or a measurement runs), loads
/// `seq` (whose lane `lane` records `dest`'s return on `channels`) with
/// the transport stopped, opens the input and installs the measurement.
/// `Model` for a destination that is not an external MIDI synth (the
/// engine leaves that check to its caller, Task 5) and for a channel the
/// input lacks, refused as Record refuses it (with the denied or
/// failed-open reason when there is one). The caller waits without any
/// lock, then calls [`end_measure`].
pub fn start_measure(
    owner: u64,
    doc: &Document,
    seq: SeqId,
    dest: DestId,
    lane: &str,
    channels: core_model::lane::InputChannels,
    plugins: Option<&mut PluginHost>,
) -> Result<Arc<MeasureStatus>, VizError> {
    crate::synth::external_dest(doc, dest)?;
    // A channel the device lacks needs no engine to refuse (5d-2 carry):
    // refused here, before the claim, which may retire another window's
    // plugin engine. Unknown channels (no device read) are left to the
    // check after the input opens.
    if let Some(c) = audio_input_channels().filter(|c| c.channels > 0) {
        if let Some(why) = crate::record::channel_refusal(lane, channels, c.channels, &[]) {
            return Err(crate::events::bad(&why));
        }
    }
    let (generation, holder, asked) = {
        let (mut slot, _) = claim_host(owner, doc, plugins, Idle::Stopped)?;
        let host = slot.as_mut().ok_or_else(|| VizError::Internal {
            message: "engine host missing".into(),
        })?;
        host.faults.clear();
        // What the holder asked of the input before Measure's own ask.
        // Known race (5d-1 re-review, T7 r1): if the holder edits its input
        // during the wait below and Measure is then refused, this restores
        // the earlier ask until the holder's next reload or edit.
        let asked = host.asked_input();
        host.ensure_input(true);
        (host.generation, host.owner, asked)
    };
    // The input opens on its own thread, without the host lock (plan 5d-1
    // Ruling 13); then nothing may have taken the engine meanwhile.
    let opened = wait_input(INPUT_WAIT);
    let mut slot = lock(&HOST);
    let host = slot
        .as_mut()
        .filter(|h| h.generation == generation)
        .ok_or_else(|| VizError::Busy {
            message: "The audio engine changed; measure again.".into(),
        })?;
    let refusal = if host.measure.is_some() {
        Some(measuring_busy())
    } else if host.busy() {
        Some(VizError::Busy {
            message: STOP_FIRST.into(),
        })
    } else if !opened {
        Some(VizError::Busy {
            message: INPUT_OPENING.into(),
        })
    } else {
        None
    };
    if let Some(e) = refusal {
        // The input Measure opened follows the lanes again (Task 7 review
        // m3): this session's when it holds the engine, closed when nobody
        // does; another holder's own change stands. It waits for the
        // thread when one is at work.
        if host.owner == owner {
            host.ensure_input(wants_audio_input(doc, seq));
        } else if host.owner == 0 {
            host.ensure_input(false);
        } else if host.owner == holder {
            // Another window holds the idle engine: its input goes back
            // to what it asked before Measure's ask (Task 7 parked r1),
            // not left open for nobody.
            host.ensure_input(asked);
        }
        return Err(e);
    }
    match install_measure(host, owner, doc, seq, dest, lane, channels) {
        Ok(status) => {
            host.measure = Some(Arc::clone(&status));
            Ok(status)
        }
        Err(e) => {
            // Whatever failed, the input follows the lanes again (Task 9
            // review M6).
            host.ensure_input(wants_audio_input(doc, seq));
            Err(e)
        }
    }
}

/// [`start_measure`] once the input is open: refuses a channel it lacks,
/// loads `seq` stopped and installs the measurement.
fn install_measure(
    host: &mut Host,
    owner: u64,
    doc: &Document,
    seq: SeqId,
    dest: DestId,
    lane: &str,
    channels: core_model::lane::InputChannels,
) -> Result<Arc<MeasureStatus>, VizError> {
    if let Some(why) =
        crate::record::channel_refusal(lane, channels, host.inputs.channels, &host.faults)
    {
        return Err(crate::events::bad(&why));
    }
    host.engine.stop()?;
    let looped = host.looped;
    host.load(owner, doc, seq, looped)?;
    host.owner = owner;
    host.seq = seq;
    host.want_play = false;
    host.until = None;
    host.epoch = next_epoch();
    let first = channels.first();
    let mut inputs = vec![first];
    if let Some(r) = channels.last().filter(|r| *r != first) {
        inputs.push(r);
    }
    host.engine
        .start_measure(doc, dest, inputs, crate::record::latency_trim())?
        .map_err(|why| crate::events::bad(&format!("the synth cannot be played: {why}")))
}

/// Ends the measurement `status` [`start_measure`] returned, whoever
/// holds the engine now (it is removed and its sounding note ends;
/// pre-flight I1). The input then changes once (Task 9 review I2): when
/// `owner` still holds the engine, to what the lanes of `seq` in `doc`
/// (the session's current document) want; else to what was asked while
/// it ran. False when it was no longer installed: a deleted sequence, a
/// closed window or a new device ended it first (nothing is changed then,
/// so a later measurement is never ended by an earlier one's caller).
pub fn end_measure(
    owner: u64,
    doc: &Document,
    seq: SeqId,
    status: &Arc<MeasureStatus>,
) -> Result<bool, VizError> {
    let mut slot = lock(&HOST);
    match slot.as_mut() {
        Some(h) if h.measure.as_ref().is_some_and(|m| Arc::ptr_eq(m, status)) => {
            let want = (h.owner == owner).then(|| wants_audio_input(doc, seq));
            h.end_measuring(want)?;
            Ok(true)
        }
        _ => Ok(false),
    }
}

/// Whether a return-latency measurement runs (in any session).
pub fn measuring() -> bool {
    lock(&HOST).as_ref().is_some_and(|h| h.measure.is_some())
}

/// Whether `owner`'s renderer has its transport stopped (the end of a
/// song without a loop), when it holds the engine. Right after a Play this
/// still reads stopped until the renderer applies it.
pub fn transport_stopped(owner: u64) -> Option<bool> {
    lock(&HOST)
        .as_ref()
        .filter(|h| h.owner == owner)
        .map(|h| h.engine.state() == PlayState::Stopped)
}

/// The tick a device rebuild resumed `owner`'s host from, when it holds
/// the engine and its host came from a rebuild.
pub fn resumed_at(owner: u64) -> Option<Tick> {
    lock(&HOST)
        .as_ref()
        .filter(|h| h.owner == owner)
        .and_then(|h| h.resumed_at)
}

/// Whether `owner`'s host means to play (its Play intent, ahead of the
/// renderer), when it holds the engine. A song that ended by itself is
/// seen first, as `busy` sees it (final review m4).
pub fn wants_play(owner: u64) -> Option<bool> {
    lock(&HOST).as_mut().filter(|h| h.owner == owner).map(|h| {
        h.collect_faults();
        h.want_play
    })
}

/// The session holding the engine, if any.
pub fn holder() -> Option<u64> {
    lock(&HOST).as_ref().map(|h| h.owner).filter(|o| *o != 0)
}

/// The streamer's `(underruns, late starts)` so far in `owner`'s engine,
/// when it holds it (pre-flight M11).
pub fn stream_counters(owner: u64) -> Option<(u64, u64)> {
    lock(&HOST).as_ref().filter(|h| h.owner == owner).map(|h| {
        let c = h.engine.streamer().counters();
        (c.underruns, c.late)
    })
}

/// The sticky faults of `owner`'s host, when it holds the engine.
pub fn faults(owner: u64) -> Vec<String> {
    lock(&HOST)
        .as_ref()
        .filter(|h| h.owner == owner)
        .map(|h| h.faults.clone())
        .unwrap_or_default()
}

/// Where `owner`'s transport is heard, when it holds the engine.
pub fn position(owner: u64) -> Option<(SeqId, Tick)> {
    lock(&HOST)
        .as_ref()
        .filter(|h| h.owner == owner)
        .map(|h| (h.seq, h.engine.position_tick()))
}

/// `owner`'s input, when it holds the engine.
pub fn input_info(owner: u64) -> Option<(String, InputInfo)> {
    lock(&HOST)
        .as_ref()
        .filter(|h| h.owner == owner)
        .map(|h| (h.device.clone(), h.inputs.clone()))
}

/// `owner`'s device input channel peaks since the last read (plan 5d-1
/// Ruling 27): empty unless it holds the engine with its input open.
pub fn input_levels(owner: u64) -> Vec<f32> {
    lock(&HOST)
        .as_ref()
        .filter(|h| h.owner == owner && h.input_open)
        .map(|h| {
            h.engine
                .input_peaks(usize::try_from(h.inputs.channels).unwrap_or(0))
        })
        .unwrap_or_default()
}

/// Whether `owner`'s host has (or, offline, would have) its input open
/// (test hook, Task 11 review I1).
#[cfg(feature = "test-hooks")]
pub fn debug_input_open(owner: u64) -> Option<bool> {
    lock(&HOST)
        .as_ref()
        .filter(|h| h.owner == owner)
        .map(|h| h.input_open)
}

/// Whether the host has (or, offline, would have) its input open,
/// whoever holds it (test hook, Task 9 re-review).
#[cfg(feature = "test-hooks")]
pub fn debug_host_input_open() -> Option<bool> {
    lock(&HOST).as_ref().map(|h| h.input_open)
}

/// How many times `owner`'s host opened or closed its input (test hook).
#[cfg(feature = "test-hooks")]
pub fn debug_input_changes(owner: u64) -> Option<u64> {
    lock(&HOST)
        .as_ref()
        .filter(|h| h.owner == owner)
        .map(|h| h.input_changes)
}

/// The MIDI input of `owner`'s engine, when it holds it.
pub fn midi_sender(owner: u64) -> Option<MidiSender> {
    lock(&HOST)
        .as_ref()
        .filter(|h| h.owner == owner)
        .map(|h| h.engine.midi_sender())
}

/// Sends a gesture to `owner`'s engine (plan 5e-1 Ruling 6): false when
/// another session holds the engine (or none is), or the queue is full.
pub fn gesture(owner: u64, msg: core_engine::gesture::GestureMsg) -> bool {
    let sender = lock(&HOST)
        .as_ref()
        .filter(|h| h.owner == owner)
        .map(|h| h.engine.gesture_sender());
    // Sent without the host lock: the sender has its own.
    sender.is_some_and(|s| s.send(msg))
}

/// Installs a gesture tap in `owner`'s engine (plan 5e-1 Ruling 7);
/// `None` when another session holds the engine or its command queue is
/// full.
pub fn start_gestures(owner: u64) -> Option<core_engine::gesture::GestureHandles> {
    lock(&HOST)
        .as_mut()
        .filter(|h| h.owner == owner)
        .and_then(|h| h.engine.start_gestures().ok())
}

/// Removes `owner`'s gesture tap. A full queue is ignored: the tap goes
/// with the engine anyway.
pub fn stop_gestures(owner: u64) {
    if let Some(h) = lock(&HOST).as_mut().filter(|h| h.owner == owner) {
        let _ = h.engine.stop_gestures();
    }
}

/// The value `owner`'s engine plays for a strip or send target now, as
/// its override snapshot holds it (plan 5e-1 Ruling 26), when `owner`
/// holds the engine and the snapshot has the control.
pub fn heard(owner: u64, t: core_model::automation::AutomationTarget) -> Option<f64> {
    let dest = core_engine::schedule::AutoDest::of(t)?;
    lock(&HOST)
        .as_ref()
        .filter(|h| h.owner == owner)?
        .engine
        .overrides()?
        .value(dest)
}

/// The gestures `owner`'s engine dropped so far (pre-flight M10), when it
/// holds it.
pub fn gestures_dropped(owner: u64) -> Option<u64> {
    lock(&HOST)
        .as_ref()
        .filter(|h| h.owner == owner)
        .map(|h| h.engine.gestures_dropped())
}

/// Whether the engine, whoever holds it, compiles with automation
/// suspended (test hook, plan 5e-1 Task 2 review M5).
#[cfg(feature = "test-hooks")]
pub fn debug_automation_suspended() -> Option<bool> {
    lock(&HOST)
        .as_ref()
        .map(|h| h.engine.automation_suspended())
}

/// The input routes the engine was last given, whoever holds it (test
/// hook, Task 8 review I1).
#[cfg(feature = "test-hooks")]
pub fn debug_host_inputs() -> Option<InputSpec> {
    lock(&HOST).as_ref().map(|h| h.engine.inputs().clone())
}

/// The input routes `owner`'s engine was last given (test hook).
#[cfg(feature = "test-hooks")]
pub fn debug_inputs(owner: u64) -> Option<InputSpec> {
    lock(&HOST)
        .as_ref()
        .filter(|h| h.owner == owner)
        .map(|h| h.engine.inputs().clone())
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    use platform_macos::permission::{DENIED_MESSAGE, MicAccess};

    /// Review I2: a refused buffer size is tried again with the device's
    /// own; a refusal without a size asked for is not retried.
    #[test]
    fn a_refused_buffer_size_falls_back_to_the_devices_own() {
        let cfg = live_audio_config(48_000.0, false);
        assert!(cfg.buffer_frames.is_some());
        let mut asked = Vec::new();
        let r: Result<u32, ()> = open_or_default(cfg.clone(), |c| {
            asked.push(c.buffer_frames);
            if c.buffer_frames.is_some() {
                Err(())
            } else {
                Ok(7)
            }
        });
        assert_eq!(r, Ok(7));
        assert_eq!(asked, vec![cfg.buffer_frames, None]);
        let mut tries = 0;
        let none = core_engine::backend::AudioConfig::default();
        let r: Result<u32, ()> = open_or_default(none, |_| {
            tries += 1;
            Err(())
        });
        assert_eq!((r, tries), (Err(()), 1));
    }

    /// Pre-flight M8: CoreMIDI's thread never waits for the engine's MIDI
    /// input. With its lock held elsewhere a source's message is dropped
    /// and counted, and the call returns at once.
    #[test]
    fn a_held_midi_input_lock_drops_and_counts_instead_of_blocking() {
        let held = lock(&MIDI_TO);
        let before = midi_source_drops();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            midi_from_source(port_key("Keystation"), 0, &[0x90, 60, 100]);
            let _ = tx.send(());
        });
        let returned = rx.recv_timeout(Duration::from_secs(2)).is_ok();
        drop(held);
        assert!(returned, "midi_from_source blocked on a held lock");
        assert!(midi_source_drops() > before);
    }

    /// Pre-flight M6: a refused microphone keeps input closed and is
    /// reported once, with the words Record refuses with; granted or not
    /// yet asked opens (the first open asks). Fake states only: the real
    /// permission is never read here.
    #[test]
    fn a_refused_microphone_is_reported_once_with_the_record_wording() {
        assert!(DENIED_MESSAGE.starts_with(crate::record::DENIED_PREFIX));
        let mut faults = vec!["xrun".to_string()];
        assert!(admit_input(MicAccess::Granted, &mut faults));
        assert!(admit_input(MicAccess::Undetermined, &mut faults));
        assert_eq!(faults, ["xrun"]);
        assert!(!admit_input(MicAccess::Denied, &mut faults));
        assert!(!admit_input(MicAccess::Denied, &mut faults));
        assert_eq!(faults, ["xrun", DENIED_MESSAGE]);
    }

    /// Plan 5c-2 Ruling 7 and pre-flight M14: the synth brings the
    /// signals input when no input was chosen, but an environment synth
    /// never overrides a test's explicit input choice.
    #[test]
    fn the_fake_synth_brings_the_signals_input_unless_a_test_chose_one() {
        let sig = Some(FakeInput::Signals);
        let lb = Some(FakeInput::Loopback(64));
        // Both from the environment, or the synth chosen by a test.
        assert_eq!(offline_input(None, false, true, false), sig);
        assert_eq!(offline_input(None, true, true, true), sig);
        assert_eq!(offline_input(None, false, true, true), sig);
        // An environment synth and a test's explicit "no input".
        assert_eq!(offline_input(None, true, true, false), None);
        // A chosen input always stands, whoever chose it.
        for (ic, sc) in [(false, false), (false, true), (true, false), (true, true)] {
            assert_eq!(offline_input(lb, ic, true, sc), lb);
            assert_eq!(offline_input(lb, ic, false, sc), lb);
            assert_eq!(offline_input(sig, ic, true, sc), sig);
            assert_eq!(offline_input(sig, ic, false, sc), sig);
            // No synth brings nothing.
            assert_eq!(offline_input(None, ic, false, sc), None);
        }
    }

    /// Task 7 review M4 (pre-flight M2): `return_dests` leaves out a link
    /// to a destination that is missing or not external, though
    /// validation never lets one through.
    #[test]
    fn return_dests_keeps_only_external_destinations() {
        use core_engine::fixtures::Builder;
        use core_model::command::{Command, LaneCmd};
        use core_model::lane::InputChannels;
        let mut b = Builder::new();
        let synth = b.dest("Synth", "Synth", 0).unwrap();
        let (seq, _) = b.sequence("S", "Part", None).unwrap();
        let lane = b.lane(seq, "Rec", None).unwrap();
        b.run(Command::Lane(LaneCmd::SetInput {
            seq,
            lane,
            input: Some(LaneInput::Audio {
                channels: InputChannels::Mono(0),
                return_of: Some(synth),
            }),
        }))
        .unwrap();
        let doc = b.finish();
        assert_eq!(return_dests(&doc, seq), vec![synth]);
        // Unvalidated edits of the serialized document.
        let edit = |f: &dyn Fn(&mut serde_json::Value)| -> Document {
            let mut v = serde_json::to_value(&doc).unwrap();
            f(&mut v);
            serde_json::from_value(v).unwrap()
        };
        let builtin = edit(&|v| {
            v["instruments"][0]["kind"]["midi_endpoint"]["endpoint_ref"] =
                core_model::mixer::BUILTIN_SYNTH_REF.into();
        });
        assert!(
            !builtin.instruments()[&synth].kind.is_external(),
            "{:?}",
            builtin.instruments()[&synth].kind
        );
        assert!(return_dests(&builtin, seq).is_empty());
        let missing = edit(&|v| {
            v["instruments"] = serde_json::json!([]);
        });
        assert!(missing.instruments().is_empty());
        assert!(return_dests(&missing, seq).is_empty());
    }

    /// Task 8 note: a stream's input (and its latencies) reach the host's
    /// `InputInfo` whatever the backend.
    #[test]
    fn a_streams_input_is_what_the_host_reports() {
        let info = core_engine::backend::StreamInfo {
            device_name: "Out".into(),
            sample_rate: 48_000.0,
            buffer_frames: 256,
            channels: 2,
            input_channels: 2,
            input_channel_names: vec!["In 1".into(), "In 2".into()],
            input_latency_frames: 40,
            output_latency_frames: 60,
        };
        assert_eq!(
            InputInfo::of(&info),
            InputInfo {
                channels: 2,
                names: vec!["In 1".into(), "In 2".into()],
                input_latency_frames: 40,
                output_latency_frames: 60,
                buffer_frames: 256,
            }
        );
        // Plan 5d-1 Ruling 9: both latencies and a buffer each way.
        assert_eq!(InputInfo::of(&info).round_trip_frames(), 40 + 60 + 2 * 256);
    }
}
