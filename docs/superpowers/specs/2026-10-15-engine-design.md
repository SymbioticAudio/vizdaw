# VizDAW Sub-project 2b — Engine Design

- **Date:** 2026-10-15
- **Status:** Draft, written unattended (decisions logged under Rulings), awaiting review
- **Parent spec:** `2026-10-15-vizdaw-design.md` (Sections 3, 4, 6, 10, 12)
- **Depends on:** `2026-10-15-core-model-design.md` (2a): `flatten`, tick ↔ seconds, `Document`, `Destination`

## 1. Goal

Make a core-model sequence play, sample-accurately, through Apple's built-in software synth, with a real-time path that never blocks or allocates.

Deliverables:
- `core-engine`: a platform-neutral crate. It compiles a `Document` + `SeqId` into an immutable schedule, runs the transport, renders blocks with sample-accurate event offsets, and hands schedules to the audio thread lock-free.
- `platform-macos`: CoreAudio HAL output (IOProc), CoreMIDI output, and the Apple DLS synth hosted as the default **built-in synth** destination.
- `vizdaw-play`: a CLI, `vizdaw-play <project.vizdaw> [--seq name]`, that plays a sequence to the built-in synth.

**Success:**
- The demo fixture plays audibly and in time through the built-in synth on the default output device.
- Offline renders are deterministic and do not depend on block size.
- A counting allocator records zero allocations on the render path.
- Tempo-ramp timing matches 2a's tick → seconds conversion to within ±1 sample.

### Deferred (not in 2b)
- Audio region playback and recording; disk streaming (sub-project 5).
- Plugins, AU and CLAP (sub-project 3). `PluginInstrument` destinations do not play yet (see §5).
- PDC, multi-core rendering, auxiliary real-time threads (so no `os_workgroup` join; see Rulings).
- MIDI input, recording, input effects.
- MIDI 2.0 UMP output (type 4). 2b sends MIDI 1.0 only. The output path is shaped so UMP can be added later (§6).
- Separate input/output devices, aggregate devices, hog mode, latency compensation for recording.

## 2. Constraints

- Rust stable, edition 2024.
- `core-engine` has no OS dependencies and builds and tests on Linux. `platform-macos` is `#![cfg(target_os = "macos")]`, and `vizdaw-play` is cfg-gated the same way.
- `core-engine` denies `clippy::unwrap_used`, `expect_used`, `indexing_slicing` and `panic`, the same lints as 2a.
- `platform-macos` keeps all `unsafe` in small modules (`hal.rs`, `dls.rs`, `midi.rs`), each with a `// SAFETY:` comment per block.
- Everything reachable from the render callback is marked `// RT`. Code marked `// RT` must not: allocate, free, lock, do I/O, log, or make a syscall other than CoreAudio/CoreMIDI render-time calls.

### Crates (all verified on crates.io, 2026-10-15)

| Crate | Version | Used by | Purpose |
|---|---|---|---|
| `rtrb` | 0.4 | core-engine, platform-macos | Wait-free SPSC rings: commands in, retired schedules out, MIDI-out events |
| `thiserror` | 2 | all | Error enums |
| `objc2-core-audio` | 0.3.2 | platform-macos | HAL: `AudioDeviceCreateIOProcID`, property listeners, `HostTime` |
| `objc2-core-audio-types` | 0.3.2 | platform-macos | `AudioBufferList`, `AudioTimeStamp`, `AudioStreamBasicDescription` |
| `objc2-audio-toolbox` | 0.3.2 | platform-macos | `AudioComponent`, `AudioUnit`, `MusicDevice` (DLS synth) |
| `objc2-core-midi` | 0.3.2 | platform-macos | Client, virtual source, output port, `MIDIEventList` |
| `objc2-core-foundation` | 0.3.2 | platform-macos | `CFString` for names |
| `clap` | 4 | vizdaw-play | Argument parsing |
| dev: `proptest`, `insta` | — | core-engine | Property and snapshot tests (already used by 2a) |

Not chosen:
- `coreaudio-rs`: it centres on the AUHAL output unit, but the parent spec requires the HAL IOProc.
- `coremidi`: a nice safe wrapper, but it would be a second binding family next to objc2, which sub-project 3 needs anyway for AU hosting.
- `arc-swap`: the last `Arc` could be dropped on the audio thread.
- `assert_no_alloc`: replaced by a 40-line test allocator, so no runtime dependency is needed.

## 3. Architecture

```
            non-RT (control thread: CLI main / FFI caller)                     RT (HAL IOProc thread)
 Document ─▶ Compiler ─▶ Box<Schedule> ─▶ cmd ring (rtrb) ───────────────▶ Renderer.process(block)
   (2a)      flatten +                      Play/Stop/Locate/Loop             │ swap schedule, step transport
             tick→sample                                                      │ emit events at offsets ─▶ Sinks
                                                                              │   ├─ InstrumentNode (DLS synth) ─ render ─▶ mix ─▶ device
 Collector thread ◀── retired Box<Schedule> ◀── retire ring (rtrb) ◀──────────┘   └─ MidiOut ring ─▶ MIDI sender thread ─▶ CoreMIDI
 Shared atomics ◀─────────────────── position (frames), state, fault bits, counters ◀────┘
```

### core-engine modules
- `schedule`: the `Schedule`, `TempoTable` and `TargetTable` types, plus the compiler.
- `midi1`: MIDI 2.0 → MIDI 1.0 translation, done at compile time.
- `transport`: transport state machine; loop and locate.
- `render`: `Renderer`, which is the real-time object, and `RenderGraph`, which is instrument nodes → master sum.
- `backend`: the traits in §7.
- `offline`: `OfflineBackend` and `EventLog` for tests and `--offline`.
- `engine`: the `Engine` control handle (non-RT).
- `fixtures` (cargo feature `fixtures`): `demo_document()`, which builds the demo project through 2a commands. It is shared with tests, the CLI and 2c. The demo's content is defined in the 2c spec (`2026-10-15-app-shell-design.md` §6).

## 4. Schedule compilation (non-RT)

`Engine::load(&Document, SeqId) -> Result<CompileReport, EngineError>` runs synchronously on the caller's thread. The caller calls it again after every committed edit.

1. **Flatten:** call `flatten(seq, full range, no filters)` from 2a. It already applies references, loops, transpose, takes, mute/solo and the nested-timing rule, and gives each `ResolvedEvent` an absolute tick, a destination and a group/channel.
2. **Resolve targets:** map each `DestId` to a `TargetIdx` through `TargetTable` (§5). If an event cannot be routed, do not play it; count it in `CompileReport.unroutable` with its source path.
3. **Time:**
   - Convert each event tick to frames as `round(tick_to_seconds(t) × sample_rate)` with 2a's conversion. 2a is the single source of tempo math; the engine has none of its own.
   - Frame 0 is the sequence's origin: the start of bar 0 when there is a pickup, otherwise tick 0, as 2a defines it.
4. **Translate:** `midi1` turns each event into zero or more `Midi1Msg { len: u8, bytes: [u8; 3] }` (§6). A note becomes a note-on at its start and a note-off at `tick + duration`, each converted to frames separately. SysEx bytes go into the schedule's arena, and their entries hold `(offset, len)`.
5. **Sort:** entries are sorted by `(frame, class, source order)`, where `class` is: note-off < program/bank < CC/pitch bend/pressure < note-on. That order means a note retriggered on the same frame still sounds, and patch changes land before notes.
6. **Tempo table:** a piecewise-linear map from frames to ticks for real-time position use (display, schedule swap, loop). It is built from 2a's `tick_to_seconds`:
   - one knot per tempo point;
   - inside linear ramps, one knot every 960 ticks (1/64 note at PPQ 15360).
   - Lookups are a binary search plus one interpolation, with no allocation.
7. **Output:**
   - `Schedule { sample_rate, entries: Box<[Entry]>, sysex: Box<[u8]>, tempo: TempoTable, length_frames, loop_range: Option<(u64, u64)>, used_channels: [u16; MAX_TARGETS] }`.
   - `Entry { frame: u64, target: TargetIdx (u8), msg: Midi1Msg | SysEx(range) }`, 24 bytes.
   - The schedule is boxed and sent through the command ring.

`CompileReport` holds: the event count, unroutable events, dropped MIDI 2.0-only data (per-note controllers, note attributes, 8-bit SysEx) by kind, a warning when group ≠ 0 events fold onto the built-in synth, and the compile time.

## 5. Targets and the built-in synth

- **`TargetTable`:** up to 16 targets (`MAX_TARGETS`), built on the control thread when a document is loaded. Kinds:
  - `BuiltinSynth`: one DLS synth instance.
  - `MidiVirtualSource`: VizDAW's own CoreMIDI virtual source, "VizDAW Out".
  - `MidiDestination(name)`: an existing CoreMIDI destination, matched by display name, then by unique ID if stored.
- **Mapping from 2a destinations:**
  - A `MidiEndpoint { endpoint_ref, … }` is resolved by its `endpoint_ref` string:
    - `"builtin:synth"` → `BuiltinSynth`;
    - `"virtual:out"` → `MidiVirtualSource`;
    - anything else → `MidiDestination(endpoint_ref)`. If no such destination exists, the events are unroutable and reported.
  - A `PluginInstrument` is unroutable in 2b and is reported by name.
- **Built-in synth:**
  - Apple's `DLSMusicDevice` (`kAudioUnitType_MusicDevice` / `kAudioUnitSubType_DLSSynth` / Apple), which loads the system GM bank (`gs_instruments.dls`).
  - One instance serves all 16 channels. Group is ignored, and events from groups other than 0 fold onto it with a compile warning.
  - Setup happens on the control thread: instantiate the unit, set its output stream format (stereo, non-interleaved f32 at the device rate) and `MaximumFramesPerSlice = 4096`, then call `AudioUnitInitialize`.
  - Render: `MusicDeviceMIDIEvent(status, d1, d2, offset_frames)` for each event in the block, then `AudioUnitRender` into pre-allocated buffers.
  - SysEx goes through `MusicDeviceSysEx`, which has no frame offset, so it lands at the block start; this is documented as a limitation.
- The render graph in 2b is: the built-in synth node(s) → master sum (gain 1.0, no clipping protection beyond a hard clamp at ±1.0 with a fault bit) → device channels 1–2. Other device channels get zeros.

## 6. MIDI 2.0 → 1.0 translation (`midi1`)

This follows the MIDI 2.0 UMP spec's default translation rules (M2-104, "MIDI 2.0 to MIDI 1.0"), implemented as pure functions and table-tested.
- **Note on:** velocity16 → `v >> 9`. If that gives 0 for a non-zero source velocity, send 1, because velocity 0 means note-off in MIDI 1.0.
- **Note off:** note-off status `0x8n`, release velocity → `v >> 9`.
- **CC:** 32-bit value → `v >> 25`. Pitch bend: 32 → 14 bits, `v >> 18`, split into LSB/MSB. Channel and poly pressure: `v >> 25`.
- **Program with bank:** CC 0 (MSB), then CC 32 (LSB), then program change, all on the same frame in that order.
- **Dropped and counted:**
  - per-note controllers and per-note pitch bend;
  - note attributes (microtonal pitch 7.9);
  - 8-bit SysEx.
  - Text events are not output.
- **SysEx:** 7-bit SysEx is sent as `F0 … F7`.
- The UMP path is planned for later. `Midi1Msg` sits behind `enum WireMsg { Midi1(Midi1Msg), SysEx(range) }`, so a `Ump64` variant can be added for MIDI 2.0 destinations without touching the compiler's sort or scheduling.

## 7. Backend traits (`core-engine::backend`)

```rust
pub struct BlockTime { pub host_time_ns: u64, pub sample_rate: f64 } // first frame of block hits the output at host_time_ns
pub trait AudioBackend {
    fn open(&mut self, cfg: &AudioConfig) -> Result<StreamInfo, BackendError>;   // device, rate, block size, channels
    fn start(&mut self, renderer: Renderer) -> Result<(), BackendError>;         // renderer moves to RT thread
    fn stop(&mut self) -> Result<Renderer, BackendError>;                         // returns it (no drop on RT)
    fn take_events(&mut self) -> Vec<BackendEvent>;                               // device lost, rate/buffer changed
}
pub trait InstrumentNode: Send {                       // RT methods
    fn midi(&mut self, offset: u32, msg: &[u8]);
    fn render(&mut self, frames: u32, out: &mut StereoBuf);
    fn reset(&mut self);                               // all notes off
}
pub trait MidiOut: Send {                              // RT method; impl must be wait-free
    fn send(&mut self, host_time_ns: u64, msg: &[u8]) -> bool;  // false = dropped (ring full)
}
pub trait MidiBackend {
    fn destinations(&self) -> Vec<MidiPortInfo>;
    fn open_virtual_source(&mut self, name: &str) -> Result<Box<dyn MidiOut>, BackendError>;
    fn open_destination(&mut self, port: &MidiPortInfo) -> Result<Box<dyn MidiOut>, BackendError>;
}
```
- A Linux backend later implements the same traits with PipeWire/JACK/ALSA plus a software synth (for example FluidSynth).
- `OfflineBackend` implements `AudioBackend`. Its `RecordingNode` and `RecordingMidiOut` write every event into a pre-sized `EventLog` with its absolute frame.

## 8. Real-time path

### Renderer
`Renderer::process(&mut self, out: &mut [&mut [f32]], frames: u32, time: BlockTime)` runs these steps for each block:
1. **Drain the command ring.** For `SetSchedule`, first check that the retire ring has room. If it does not, leave the new schedule queued and try again next block. Then swap the schedule and push the old one to the retire ring. Swapping while playing keeps the musical position: old frame → tick through the old `TempoTable`, then tick → frame through the new one. Notes that were sounding get note-offs at offset 0, and the new schedule fires only events at or after the new position. Notes held across an edit are cut (see Rulings).
2. **Step the transport** over `[pos, pos + frames)`. If a loop end or the sequence end falls inside the block, split the block into sub-ranges at that frame. At a loop wrap, emit note-offs for active notes and jump to the loop start. At the sequence end with no loop, play one second of tail, then go to `Stopped` and set the `ended` bit.
3. **Emit events.** A cursor into `entries` (an index, found by binary search on locate or swap) emits each entry in a sub-range at `offset = frame − block_start`:
   - to an `InstrumentNode`, as `midi(offset, bytes)`;
   - to a `MidiOut`, as `send(host_time_ns + offset × 1e9 / sample_rate + midi_offset_ns, bytes)`.
4. **Active-note tracker.** A fixed `[[u128; 16]; MAX_TARGETS]` bitset tracks which notes are on per target, channel and pitch. It also keeps a per-channel "sustain down" bit. Stop, locate, loop wrap and swap send note-offs for the set bits and CC 64 = 0 where sustain is down.
5. **Render nodes, then sum** into `out` channels 0–1, clamp, and zero the remaining channels.
6. **Publish state:**
   - `position_frames` (AtomicU64) and state (AtomicU8);
   - fault bits (AtomicU32): xrun, clipped, midi-dropped, schedule-swap-deferred, panic;
   - counters for dropped MIDI and deferred swaps.

### Control handle
- `Engine` (non-RT) provides `load`, `play`, `stop`, `locate_tick(Tick)`, `set_loop(bool)`, `position_tick() -> Tick`, `state()` and `faults()`.
- Tick ↔ frame conversions on the control side use the `TempoTable` of the last schedule loaded.
- **Commands** travel through an `rtrb` ring of `EngineCmd`, capacity 64; a full ring returns `EngineError::Busy`. The commands are `SetSchedule(Box<Schedule>)`, `Play`, `Stop`, `Locate(u64)` and `SetLoop(Option<(u64, u64)>)`.
- **Retired schedules** are dropped on a collector thread that drains the retire ring every 10 ms.

### Rules
- No allocation, deallocation, locks or `Arc` drops on the render path.
- Every buffer (node scratch space up to 4096 frames, sub-range list) is allocated in `Renderer::new` or `prepare(max_frames)`.
- If a block is larger than `max_frames`, it is rendered as silence and the xrun bit is set.

## 9. platform-macos

### HAL output (`hal.rs`)
- **Opening the device:** use the default output device, or a device named with `--device`.
  - Read the nominal sample rate and `kAudioDevicePropertyBufferFrameSize`. If `--buffer N` is given (32–2048), set the buffer size.
  - Read the output stream formats. Accept only f32 linear PCM, interleaved or not; refuse anything else with `BackendError::Format`.
- **IOProc:**
  - The IOProc is created with `AudioDeviceCreateIOProcID`. Its client data is a `Box<RtContext>` that owns the `Renderer` and the scratch planar buffers.
  - It is an `extern "C-unwind"` function wrapped in `catch_unwind`. On a panic it outputs silence for that block and every later one, sets the panic fault bit, and never unwinds into CoreAudio.
  - `inOutputTime.mHostTime` is converted to nanoseconds once per block (`AudioConvertHostTimeToNanos`) to form `BlockTime`.
  - The engine renders planar output; the IOProc interleaves or copies it into the device's `AudioBufferList`.
- **Listeners:** `DeviceIsAlive`, `NominalSampleRate`, `BufferFrameSize` and the system default output device. The listener callbacks only set atomics. The control thread polls them through `take_events()` and responds:
  - **Device dies, or the default changes while following the default:** stop and reopen on the new default. The schedule is kept if the rate is the same and recompiled if not.
  - **Rate change:** reformat the DLS synth and recompile at the new rate.
  - **Buffer change:** call `prepare(new_max)` on a stopped renderer, then restart.
- The IOProc thread is already the device's workgroup thread. 2b creates no other real-time threads, so nothing else joins the workgroup.

### DLS synth (`dls.rs`)
- `DlsSynth` implements `InstrumentNode`, as set out in §5.
- `render` builds an `AudioTimeStamp` with an increasing `mSampleTime` and calls `AudioUnitRender` into the node's two pre-allocated channel buffers.
- `reset` sends CC 123 (all notes off) and CC 121 (reset controllers) on all 16 channels.
- A unit-level offline test drives the node with no audio device present (§10).

### CoreMIDI (`midi.rs`)
- One `MIDIClient` per process, created on the control thread.
- The virtual source is created with `MIDISourceCreateWithProtocol(kMIDIProtocol_1_0)`. Named destinations use `MIDIOutputPortCreate` + `MIDISendEventList`.
- Messages are packed as MIDI 1.0 channel-voice UMP (type 2) and SysEx7 (type 3) in a `MIDIEventList`.
- **Sending:** sending is not real-time safe, because it is a Mach IPC call. So `MidiOut::send` pushes `(host_time_ns, len, bytes[3] | sysex range)` into an `rtrb` ring (capacity 4096). A MIDI sender thread drains the ring every 1 ms and calls `MIDIReceivedEventList` / `MIDISendEventList` with the host-time timestamp.
  - Timestamps point at the block's output time, which is in the future by at least the buffer and safety offset, so sender-thread jitter is absorbed.
  - `midi_offset_ns` (default 0, CLI `--midi-offset-ms`) lets external gear be lined up with audio.

## 10. vizdaw-play (CLI)

```
vizdaw-play <project.vizdaw> [--seq NAME] [--loop] [--device NAME] [--buffer N]
            [--to builtin|virtual|both] [--midi-offset-ms N] [--offline OUT.txt] [--list]
vizdaw-play --write-demo <dir/demo.vizdaw>
```
- **Loading:** the CLI loads the project with 2a. `--seq` picks a sequence by name; by default it picks the only top-level sequence, or else lists them and exits with code 2.
- **Routing with `--to`:** it re-maps every `builtin:synth` destination at load time. `virtual` sends those events to "VizDAW Out" instead, so a third-party synth app or MIDI Monitor can receive them.
- **During playback:** it prints the position as bar·beat·unit about 10 times a second, plus any faults and counters. It exits 0 when the `ended` bit is set. Ctrl-C stops the transport, which sends note-offs before exit; a `sigaction` flag is polled by the main loop.
- **`--offline`:** writes the `EventLog` (frame, target, bytes) from `OfflineBackend` at 48 kHz with 512-frame blocks and exits, without touching audio hardware. Used in CI and in bug reports.
- **`--write-demo`:** saves `fixtures::demo_document()` as a package. The checked-in `fixtures/demo.vizdaw` is produced this way, and a test checks that it matches a fresh build.

## 11. Errors and faults

- `EngineError`: `Model(ModelError)`, `UnknownSequence`, `Backend(BackendError)`, `Busy`, `TooManyTargets`.
- `BackendError`: `NoDevice`, `Format { .. }`, `Os { call: &'static str, status: i32 }`, `ComponentMissing`.
- Real-time faults are bits, never errors. The control side reads them and clears them on read (`faults()`).
- Unroutable events and translation drops are reported in `CompileReport` (parent §5: "reported, not silently dropped").

## 12. Testing

### Unit tests (core-engine)
- `midi1` translation tables: velocity 0/1/0xFFFF, the velocity-to-1 rule, pitch-bend centre `0x8000_0000` → `0x2000`, and bank+program ordering.
- Entry sort classes.
- `TempoTable` knots; a sub-range split exactly at a loop end.

### Offline render tests (`tests/offline.rs`, insta snapshots of `EventLog`)
- Fixtures:
  - the demo project;
  - a single 4/4 bar at 120 BPM: quarter notes land at frames 0, 24000, 48000, 72000 at 48 kHz;
  - a nested reference with transpose and loop count;
  - looped playback over 3 wraps (checks note-offs at the wrap);
  - a program + bank change;
  - an unroutable plugin destination (reported, absent from the log).
- **Block-size independence (proptest):** a random document rendered with block sizes {1, 7, 64, 512, 4096} and random per-block jitter must give identical absolute-frame logs.
- **Schedule swap mid-play:** events before the swap come from the old schedule and events after it from the new one. Active notes get note-offs at the swap frame. When the tempo changes in the new schedule, the musical position is kept.

### Tempo-ramp timing (`tests/tempo.rs`)
- **Analytic case:** a linear ramp 120 → 60 BPM over 4 bars. Each event's frame must equal `round(2a::tick_to_seconds × sr)` and must also match the closed-form integral for 2a's documented ramp semantics, within ±1 frame.
- **Proptest:** random tempo maps (steps and ramps, 5–999 BPM). Compiled frames must stay within ±1 frame of 2a's conversion, and `TempoTable` frame → tick must stay within ±1 tick inside ramps.
- **Nested sequences:** a referenced sequence with its own tempo map is timed by the parent's map, following 2a's nested-timing rule.

### Real-time safety (`tests/rt_alloc.rs`)
- The test's `#[global_allocator]` counts `alloc`, `dealloc` and `realloc` on threads where a thread-local `ARMED` flag is set.
- It arms the flag around `Renderer::process` and drives these scenarios through the offline backend:
  - 10k blocks of play, stop, locate and loop wrap;
  - 100 schedule swaps, including one with a full retire ring (deferred);
  - a full MIDI ring (drops);
  - blocks larger than `max_frames`.
- It asserts zero counted allocations.
- `vizdaw-play --features rt-check` installs the same allocator in the IOProc and prints the count at exit. The smoke test expects 0. This counts Rust allocations only; allocations inside Apple's code are not visible to it.

### platform-macos tests
These need no audio or MIDI hardware and run on CI macOS runners.
- **`dls_renders_sound`:** instantiate `DlsSynth` at 48 kHz, send a program change and note-on, render 20 × 512 frames, and require RMS > −40 dBFS. A note-off followed by 2 s of rendering decays below −80 dBFS.
- **`virtual_source_roundtrip`:** create "VizDAW Test Out", connect an input port of the same client to it, send 100 timed events through `MidiOut` + the sender thread, and check the bytes and order. Timestamps must equal the requested host times.
- **`hal_default_device_opens`:** `#[ignore]` by default (it needs an output device). Open, render 1 s of silence, check that `position_frames` advances by about the sample rate, then close.

### Manual smoke test (documented in `crates/vizdaw-play/README.md`)
1. Run `cargo run -p vizdaw-play --release -- fixtures/demo.vizdaw --seq Song`. Drums, bass and chords should be audible. The ritardando at the end should be audible, and the process should exit 0 after the tail.
2. Add `--loop`, let it wrap twice, then press Ctrl-C. There should be no hanging notes.
3. Add `--to virtual` and watch the events in MIDI Monitor (or `receivemidi`, if installed).
4. While it plays, switch the output device in Control Center. Playback should continue on the new device or stop with a reported fault, never crash.
5. Run with `--features rt-check`. The reported count should be 0.

### Performance targets
- Compiling the demo project, or a 64-bar song with 3 reference levels: < 10 ms (2a flatten < 5 ms plus conversion and sort).
- `Renderer::process` for an event-free 512-frame block, excluding nodes: < 5 µs on an M1.
- **CI:**
  - `cargo test -p core-engine` and `cargo clippy -D warnings` on macOS and Linux;
  - the platform-macos tests on macOS;
  - `vizdaw-play --offline` on the fixture, with its output diffed against the snapshot.

## 13. Workspace changes

- Add members `crates/core-engine`, `crates/platform-macos` and `crates/vizdaw-play`. `fixtures/demo.vizdaw` is checked in.
- `core-engine` depends on `core-model` (path), `rtrb` and `thiserror`.
- `platform-macos` depends on `core-engine` and the objc2 crates listed in §2, with only the features it needs (`AudioHardware`, `HostTime`, `AudioComponent`, `AudioUnit`, `AUComponent`, `AudioUnitProperties`, `MusicDevice`, `MIDIServices`, `MIDIMessages`).

## Rulings

- **Ruling:** The engine does no tempo math of its own. Event frames come from 2a's `tick_to_seconds`, and the real-time `TempoTable` is a piecewise-linear table sampled from it (a knot every 1/64 note inside ramps) — why: there is one source of truth for ramp semantics, and the "drawn ramps sound linear" definition lives only in 2a — cost if wrong: if 2a's conversion is too slow or imprecise for compile budgets, we would add a closed-form table in core-engine. That is about a day, and the proptests already compare against 2a.
- **Ruling:** Schedules travel as `Box<Schedule>` through `rtrb` rings: commands in, retired schedules out to a collector thread. `arc-swap` and `triple_buffer` are not used — why: the audio thread can never perform the final free, and the swap is wait-free with fixed capacity — cost if wrong: a deferred swap when the retire ring is full delays an edit by one block. It is visible as a fault counter and cheap to change.
- **Ruling:** Each edit recompiles the whole schedule synchronously in `Engine::load`. There is no incremental compilation — why: 2a's flatten target is < 5 ms for a 64-bar, 3-level song, which is well under interactive latency — cost if wrong: large projects would stutter on edit. We would move compilation to a worker thread with coalescing, about a day, with no API change because `load` already returns a report.
- **Ruling:** A schedule swap during playback cuts sounding notes (note-off at the swap point), and the new schedule fires only events at or after the position — why: this is simple and can never leave a note hanging; the parent's drum-lane rule already avoids mid-note starts — cost if wrong: held notes get cut whenever an edit lands during playback. A later refinement can diff active notes against the new schedule, which is local to `render`.
- **Ruling:** The built-in synth is `DLSMusicDevice` (`kAudioUnitSubType_DLSSynth`), not `AUMIDISynth` — why: it loads the system GM bank with no sound-bank URL or preload step, and the bank file is present on this machine (`CoreAudio.component/.../gs_instruments.dls`) — cost if wrong: if Apple removes the DLS synth we switch the subtype to `kAudioUnitSubType_MIDISynth` and load the bank by URL, which is confined to `dls.rs`.
- **Ruling:** One DLS instance covers all destinations that use `builtin:synth`. Groups other than 0 fold onto it with a warning — why: 16 channels are enough for 2b demos and fixtures, and per-group instances add mixing and setup — cost if wrong: multi-group SMF imports (sub-project 6) collide channels. Per-group instances can be added inside `TargetTable`.
- **Ruling:** Destinations are bound to backends by `endpoint_ref` string conventions (`builtin:synth`, `virtual:out`, otherwise a CoreMIDI destination name) — why: 2a stores `endpoint_ref` opaquely, and this needs no model change — cost if wrong: names are fragile across renamed devices. A later model revision can store CoreMIDI unique IDs, and 2b already tries the unique ID after the name.
- **Ruling:** `PluginInstrument` destinations are unroutable in 2b and are reported, not redirected to the built-in synth — why: silent substitution hides routing errors (parent §5 says reported, not silently dropped) — cost if wrong: projects made for plugins are silent until sub-project 3. The report names each one.
- **Ruling:** CoreMIDI sends go through a ring to a sender thread and are never called on the IOProc — why: `MIDISend*`/`MIDIReceived*` are Mach IPC and not real-time safe, and timestamps in the future absorb the hop — cost if wrong: extra latency is impossible because events are timestamped, but if the sender thread starves, events could arrive late. That is visible in the roundtrip test.
- **Ruling:** CoreMIDI output uses `MIDIEventList` with the MIDI 1.0 protocol (UMP types 2/3), not the deprecated `MIDIPacketList` — why: it is the same container MIDI 2.0 (type 4) will use later, so the UMP upgrade only swaps message packing — cost if wrong: none expected; `MIDIEventList` has been available since macOS 11.
- **Ruling:** The CoreAudio/CoreMIDI bindings are the objc2 family (`objc2-core-audio`, `objc2-audio-toolbox`, `objc2-core-midi` 0.3.2), not `coreaudio-rs`/`coremidi` — why: the parent requires the HAL IOProc, which coreaudio-rs does not target, and sub-project 3's AU hosting needs objc2 anyway. One binding family keeps types consistent — cost if wrong: more hand-written `unsafe` wrappers, roughly 500 lines, isolated in three modules.
- **Ruling:** The real-time allocation check uses our own counting `#[global_allocator]` in tests, not `assert_no_alloc` — why: the task asks for counting, and it needs no runtime dependency — cost if wrong: about 40 lines of test code to maintain.
- **Ruling:** No `os_workgroup` join in 2b — why: the IOProc thread is already in the device's workgroup, and 2b creates no auxiliary real-time threads — cost if wrong: none until multi-core rendering, which must add joins for its worker threads.
- **Ruling:** A panic in the IOProc is caught. Output goes silent and a fault bit is set; the process does not abort — why: unwinding into CoreAudio is UB, aborting loses the user's session, and clippy lints already make panics unlikely — cost if wrong: a silently failing engine. The fault bit is shown in the CLI and UI.
- **Ruling:** The demo document builder lives in `core-engine` behind a `fixtures` feature and is shared with the CLI and 2c — why: one fixture serves engine tests, the smoke test and the app's "Create demo project" — cost if wrong: a model crate dependency inside an engine feature. It can be moved to its own small crate in minutes.
- **Ruling:** SysEx sent to the DLS synth lands at the block start, with no frame offset — why: the `MusicDeviceSysEx` API has no offset parameter — cost if wrong: up to one buffer (≤ 43 ms at 2048/48 k) of SysEx timing error. That is acceptable, since SysEx is rarely timing-critical.
- **Ruling:** The master bus hard-clamps at ±1.0 and sets a fault bit; there is no limiter — why: it protects ears and speakers with no DSP design work — cost if wrong: audible clipping on dense demos. The fix is to lower the demo velocities or add master gain later.
