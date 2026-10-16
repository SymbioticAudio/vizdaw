# VizDAW Sub-project 3 — Plugin Hosting and Mixer Design

- **Date:** 2026-10-16
- **Status:** Draft, awaiting review
- **Parent spec:** `2026-10-15-vizdaw-design.md` (Sections 4, 6, 7, 10, 11, 12)
- **Depends on:** 2a core model, 2b engine, 2c app shell (branch `overnight`)

## 1. Goal

Play AU and CLAP instruments and effects through a full mixer, with plugin delay compensation (PDC) and multi-core rendering, from a SwiftUI app that can browse, open, manage and recover from plugins.

**Success:**
- An AU instrument from the user's installed set (e.g. Diva) plays a sequence through an insert chain of AU and CLAP effects, a send to a reverb bus, and the master.
- A sidechained compressor ducks a pad on the kick. A multi-output drum instrument feeds separate strips.
- Offline renders are byte-identical for any worker count (1 to 8) and any block size, with PDC sample-exact.
- The render path makes zero allocations on every thread, including workers.
- A plugin that crashes during a scan is blocklisted, and the Plugin Manager can disable it. A project whose plugin crashes VizDAW reopens in safe mode with plugins off.

### Decisions (from brainstorming, 2026-10-16)

| # | Decision |
|---|---|
| Q1 | Full mixer: instrument strips, audio-path strips, aux buses, master; inserts, sends, routing. |
| Q2 | Standard routing **plus** sidechain inputs and multi-output instruments. |
| Q3 | Plugins run in-process. AUv3 runs out of process where the plugin allows. `PluginInstance` is the seam for a later out-of-process host. |
| Q4 | AU v2/v3 and CLAP only. No VST3; the same seam admits it later. |
| Approach | Parallel graph rendering across cores from day one, on worker threads joined to the device's audio workgroup. |
| Note | A Plugin Manager window to enable/disable plugins, plus safe mode. |

### Out of scope
- Automation lanes and their playback (sub-project 4, Strip Chart). Parameters, gestures and stable IDs are ready for them.
- Audio-region playback (sub-project 5). Audio-path strips exist, route and process, but their source is silent.
- Out-of-process hosting for AU v2 / CLAP; VST3; surround (all channel groups here are mono or stereo, and the types carry a channel count).
- Plugin presets browsing, offline bounce, freeze, input monitoring through plugins, MIDI-effect plugins.
- Visual polish of the mixer (sub-project 7).

### Limitations
- A crash inside an in-process plugin ends VizDAW. Autosave and safe mode (§10) limit the damage.
- Changes made inside a plugin's own UI that are not parameters (loading a preset, editing a wavetable) are saved with the project but are not individually undoable.

## 2. Constraints

- All 2a/2b constraints carry over: Rust stable, edition 2024; `core-model`, `core-engine` and `core-host` build and test on Linux; the same clippy denials (`unwrap_used`, `expect_used`, `indexing_slicing`, `panic`) on those crates; `// RT` code never allocates, frees, locks, does I/O, logs or makes syscalls beyond the documented wait/wake primitives and plugin process calls.
- `unsafe` lives in small modules with a `// SAFETY:` comment per block.
- Plugins are untrusted code. The host survives every plugin *error* (bad return codes, NaN output, refusing state, failing to load). It cannot survive a plugin *crash*; it only records enough to recover.
- New crates (versions checked on crates.io 2026-10-16): `clack-host 0.2`, `clack-extensions 0.2` (CLAP host); `clack-plugin 0.2` (test plugins only); `crossbeam-queue` (`ArrayQueue` for the ready queue); existing `objc2-*` 0.3.2 crates with added features (`AVFAudio`, `AudioUnit`, `AUParameters`).

## 3. Architecture

```
app/ (Swift)   Mixer window · Plugin browser · Plugin windows (NSWindow+NSView) · Plugin Manager · safe-mode/recovery prompts
core-ffi       mixer commands · catalog · params · editor handles · host_idle()
core-model     Mixer, Strip, PluginSlot, routing validation, file format v2
core-engine    graph compiler (PDC) · parallel renderer · worker pool · meters
core-host      PluginInstance trait · CLAP host (clack) · catalog/blocklist types · scanner protocol
platform-macos AU v2/v3 host · RtWorkgroup (os_workgroup) · RT thread setup · AU enumeration
vizdaw-scan    out-of-process scanner binary (one plugin per invocation)
test-plugins   in-repo CLAP plugins for tests (gain+latency, ducker, multi-out synth, crasher, hanger, NaN)
```

Platform seams (for a Linux port): `PluginFormat` (scan + instantiate; AU on macOS, CLAP everywhere), `RtWorkgroup` (join/leave; no-op elsewhere), `RtWaker` (wait/wake for workers).

## 4. Model (core-model)

### Entities
- **`Mixer`** on `Document`: `strips: OrdMap<StripId, Strip>`, `order: Vector<StripId>` (display order), `master: StripId`.
- **`Strip`**: `name`, `kind`, `inserts: Vector<PluginSlot>`, `gain_db` (−∞ … +12, default 0), `pan` (−1 … +1), `mute`, `solo`, `sends: Vector<Send>`, `output: Output`.
- **`StripKind`**: `Instrument(InstrumentId)` (main output of a plugin instrument), `InstrumentOutput(InstrumentId, u16)` (an extra output of a multi-output instrument), `AudioPath(OutputPathId)`, `Bus`, `Master`.
- **`Output`**: `Master` | `Bus(StripId)`. Master always outputs to device channels 1–2.
- **`Send`**: `target: StripId` (a bus), `level_db`, `pre_fader: bool`, `enabled: bool`.
- **`PluginSlot`**: `id: PluginInstanceId`, `plugin: PluginRef`, `bypass: bool`, `sidechain: Option<StripId>`, `state: Option<StateBlobId>`, `params: OrdMap<ParamId, f64>` (the parameter overlay, below).
- **`PluginRef`**: `format` (`Au` | `Clap`), `id` (AU: `type/subtype/manufacturer` four-char codes; CLAP: plugin id), `name`, `vendor`, `version`. Matching on load uses format + id only; a version change is reported, not refused.
- **`Destination::PluginInstrument`** changes from `{ plugin_ref: String }` to `{ slot: PluginSlot }`. Creating one also creates its `Instrument` strip.

### Rules (validated on every command and on load)
- Routing is a DAG over outputs, sends and sidechain edges. A command that would create a cycle fails with `ModelError::RoutingCycle` and changes nothing.
- Outputs target a `Bus` or master; sends target a `Bus` only. A strip cannot route or send to itself, and master has no output or sends.
- Every `PluginInstrument` has exactly one `Instrument` strip; every `OutputPath` has exactly one `AudioPath` strip; exactly one `Master`. Deleting an instrument or output path deletes its strips and retargets anything routed to them to master (inside the same undo step).
- `InstrumentOutput` strips exist only for output indices the instrument's last known port layout has. If a plugin later reports fewer outputs, the extra strips stay and are silent (reported, not deleted).
- Solo: a soloed strip sounds, along with every strip downstream of it (buses and master on its path). Others are muted by solo. A strip's explicit `mute` silences its sidechain feed; a solo-implied mute does not.

### Parameter overlay and state
- `state` points at an opaque blob stored in the package as `PluginState/<blob-id>.bin` (not inline in `project.json`). Blobs are immutable; a new capture gets a new id; unreferenced blobs are removed on save.
- `params` holds values changed through VizDAW (mixer controls or plugin-UI gestures) since the last state capture. Loading an instance restores `state`, then applies `params`. The engine host diffs overlays between snapshots and sends changed values to the instance.
- Each gesture (begin → end) is one undoable `SetPluginParams` command. Undo is therefore an ordinary snapshot restore.
- **Capture** (on save, on autosave, before an instance is removed): the host reads the instance state, stores a new blob, and replaces `state` and clears `params` through a non-undoable `CaptureState` command that does not create a history entry (the sound is unchanged).

### File format
- Version 2. The v1 → v2 migration adds a mixer (master, one `Instrument` strip per plugin instrument, one `AudioPath` strip per output path, all routed to master) and turns each `plugin_ref` string into a `PluginSlot` whose `PluginRef` is a placeholder named after the old string.

## 5. Plugin abstraction (core-host)

```rust
pub trait PluginInstance: Send {
    fn info(&self) -> &PluginInfo;              // ports, params, latency, flags
    fn activate(&mut self, sr: f64, max_frames: u32) -> Result<(), HostError>;   // non-RT
    fn deactivate(&mut self);                                                 // non-RT
    fn process(&mut self, ctx: &mut ProcessCtx) -> ProcessStatus;               // RT
    fn reset(&mut self);                                                       // RT
    fn save_state(&mut self) -> Result<Vec<u8>, HostError>;                      // main thread
    fn load_state(&mut self, data: &[u8]) -> Result<(), HostError>;              // main thread
    fn editor(&mut self) -> Option<Box<dyn PluginEditor>>;                      // main thread
}
```

- `ProcessCtx` carries input groups, output groups (each `&mut [&mut [f32]]` with a channel count), the sidechain group, an event list (MIDI 1.0 bytes and MIDI 2.0 UMP, sorted by frame), parameter changes in, parameter changes out (gesture begin/value/end from the plugin), and transport info (tempo, position in beats, playing).
- `PluginInfo`: audio ports (main in, sidechain in, N outputs), note port present, parameters (`ParamId`, name, range, default, stepped, automatable), latency in samples, tail.
- Latency changes are reported through a host callback → the engine recompiles the graph (§6).
- Threads: "main" is the app's main thread (CLI and tests: the calling thread). `core-host` exposes `host_idle()`; the app calls it from a main-thread timer at 60 Hz to service CLAP `request_callback`, deferred state reads and latency/param-rescan requests.
- **CLAP host** (`core-host::clap`, `clack-host`): extensions `audio-ports`, `note-ports`, `params`, `state`, `latency`, `gui` (embedded cocoa view), `tail`, `log`, `thread-check`. MIDI 2.0 notes go out as CLAP note events plus note expressions; other events as `midi`/`midi2` events. Search paths: `~/Library/Audio/Plug-Ins/CLAP`, `/Library/Audio/Plug-Ins/CLAP` (Linux: `~/.clap`, `/usr/lib/clap`).
- **AU host** (`platform-macos::au`): enumeration through `AudioComponentFindNext`; v2 instantiate synchronously off the main thread, v3 through `AVAudioUnit.instantiate` with `loadOutOfProcess` when the component allows. Render via `AURenderBlock` with an input pull block; MIDI through `scheduleMIDIEventListBlock` (UMP) when available, else `MusicDeviceMIDIEvent`. Parameters through `AUParameterTree`; plugin-UI gestures through parameter-tree observers (v3) and `AUEventListener` begin/end gesture events (v2). State through `fullStateForDocument`/`kAudioUnitProperty_ClassInfo`, serialized as a binary plist. Editor: `requestViewController` (v3) or `kAudioUnitProperty_CocoaUI` (v2).
- Sidechain on AU uses input bus 1; on CLAP, the non-main input port. A plugin without a sidechain port shows no sidechain selector.

## 6. Graph compiler (core-engine, non-RT)

Runs after each committed edit, together with the existing schedule compiler, and hands one immutable `Graph` to the audio thread through the existing command ring.

- **Nodes:** one per plugin instrument (`InstrumentNode`: event input, N output groups), one per strip (`StripNode`: sums its inputs in a fixed order, runs its inserts in order, applies fader + pan, writes its main output and its send taps). Audio-path strips have a silent source input until sub-project 5.
- **Edges:** instrument output → strip, strip output → bus/master, send → bus, strip post-fader → sidechain input of a specific insert. Each edge has a preallocated delay line.
- **PDC:** node latency = sum of its inserts' latencies (bypassed inserts keep their latency so bypassing never shifts timing). Compute each node's output arrival time; every edge into a node is delayed to the latest arrival among that node's inputs (sidechain inputs align to the main input of the node they feed, measured at the insert position). The master arrival time `L` is the total latency: MIDI to the graph is sent `L` samples early relative to the device, and external MIDI output plus the displayed position are delayed by `L`, so everything lines up with the timeline.
- **Instance reuse:** instances are keyed by `PluginInstanceId`; a recompiled graph takes over running instances without reactivation. New instances are created and activated on a loader thread before the graph is published; removed ones are deactivated and dropped off the RT thread (returned through the collector).
- **Limits:** max 1024 nodes, max delay per edge 2 s at the current sample rate; beyond either, the command still commits but the engine reports `GraphTooLarge` / `LatencyTooLarge` and keeps playing the previous graph.

## 7. Parallel renderer (core-engine, RT)

- **Pool:** `W` worker threads, default `min(performance cores − 1, 7)`, setting 0–15. Created at engine start with real-time scheduling (`THREAD_TIME_CONSTRAINT_POLICY` on macOS) and joined to the device's `os_workgroup` through `RtWorkgroup`. On device change, workers leave and rejoin the new workgroup.
- **Per block:** the callback thread resets each node's pending-input counter (atomics laid out in the graph), pushes the source nodes onto a bounded `ArrayQueue` sized to the node count, and wakes `min(W, ready)` workers through `RtWaker` (macOS: Mach semaphores). Every thread, the callback thread included, pops a node, renders it, then decrements the counters of its successors; whoever brings a counter to zero pushes that node. The callback returns after the master node completes. Workers spin briefly (≈ 20 µs) before waiting.
- **Determinism:** a node only reads its inputs after all are done and sums them in edge order, so the result is independent of which thread ran what. `W = 0` runs everything on the callback thread.
- **Deadline:** if master is not finished before the block deadline minus a margin, the block outputs silence for that callback, an xrun is counted, and the nodes still running finish into a discarded block (they are never interrupted).
- **Meters and load:** each strip writes peak L/R to atomics; the engine writes per-block DSP load (render time / block time). The UI polls at 30 Hz.
- **Faults:** NaN/Inf or |x| > 64 in a plugin's output → that output block is zeroed and a per-instance fault counter increments; 16 faulty blocks in a row bypass the instance and raise `PluginFault`. A plugin returning an error status is treated the same.

## 8. Scanner (vizdaw-scan)

- One process per plugin: `vizdaw-scan --format au|clap --id … --path …` loads, activates at 48 kHz/512, processes one silent block, reads ports and parameters, deactivates, and prints a JSON `PluginInfo` to stdout. Timeouts: 10 s (AU v2, CLAP), 30 s (AU v3).
- The app runs scans on a background queue, up to 4 at a time. A crash, non-zero exit or timeout blocklists that plugin with the reason (`crashed (signal 11)`, `timed out`, `failed: <message>`).
- Cache: `~/Library/Application Support/VizDAW/plugins.json`, keyed by format + id + bundle path + bundle version + modification time. Only new or changed plugins are rescanned at launch.
- AU v3 instantiation needs the app context; when `vizdaw-scan` cannot instantiate a v3 component it records it as `not validated` and the app validates it on first use.

## 9. Plugin Manager and plugin windows

- **Plugin Manager** (Window menu): a table of every known plugin — name, vendor, format, version, type (instrument/effect), status (`OK`, `Disabled`, `Blocklisted: reason`, `Not validated`, `Missing`). Actions: enable/disable toggle, rescan selected, rescan all, clear blocklist entry (rescans), reveal in Finder. Search field and format filter.
- Enabled/disabled state is an app preference, not part of projects. Disabled and blocklisted plugins are hidden from the plugin browser. A project slot that names one loads as a placeholder labelled "Disabled in Plugin Manager"; enabling it there offers to load the waiting slots.
- **Plugin browser:** a popover from insert slots and from "New Plugin Instrument…", grouped by vendor, filtered to instruments or effects, with search.
- **Plugin windows:** one `NSWindow` per instance hosting the plugin's view, titled "‹strip› — ‹plugin›", resizing when the plugin allows, with a small header holding bypass. Closing the window does not remove the plugin; removing the plugin closes its window. Windows reopen on project open if they were open at save (stored per instance).
- **Placeholders** show in slot and window as the plugin name in a dimmed style with the reason, and keep their saved state untouched.

## 10. Crash recovery

- **Autosave:** while a project has unsaved changes, every 2 minutes and right after any plugin is loaded, the app writes a full copy (with captured plugin states) into the package's `Autosave/` folder. Opening a project whose autosave is newer than `project.json` offers "Restore autosave / Open last saved".
- **Crash marker:** before loading plugins for a project the app writes `~/Library/Application Support/VizDAW/open-projects/<hash>.json` naming the project and, during each instance creation, the plugin being loaded. A clean close removes it.
- **Safe mode:** opening a project with a leftover marker, or holding Shift while opening, shows "VizDAW quit unexpectedly while this project was open[, while loading ‹plugin›]" with "Open with Plugins Off / Open Normally / Cancel" (plus "Disable ‹plugin›" when one is named). Plugins Off loads every slot as a placeholder; each slot then has a "Load" action, and the strip header has "Load All".

## 11. FFI and app

- `core-ffi` adds: mixer read model (strips with slots, sends, routing, meters), mixer commands (all undoable through the existing history), catalog and manager actions, param list/get/set with gesture begin/end, editor open/close returning an `NSView` pointer to Swift, `host_idle()`, safe-mode/marker helpers, autosave trigger.
- **Mixer window** (functional, not polished): horizontally scrolling strips — name, input/kind label, insert slots (add, bypass, open, remove, drag to reorder, sidechain menu when supported), send slots (target, level, pre/post), output menu, pan, fader, mute/solo, peak meter; master at the right; "Add Bus"; a DSP-load readout. Instrument strip menu: "Add Outputs…" for multi-output plugins.

## 12. Errors and faults

New `ModelError::RoutingCycle`, `InvalidRoutingTarget`. New `HostError` (load failed, state rejected, activate failed, editor unavailable). New engine faults: `PluginFault(instance)`, `GraphTooLarge`, `LatencyTooLarge`, `WorkgroupJoinFailed` (the workers then run at RT priority without the workgroup, reported once). All surface in the UI's fault poll.

## 13. Testing

- **core-model:** proptests that any mixer command sequence undoes to the exact original; the cycle check never accepts a cycle and never rejects a DAG; solo-set computation against a reference; v1 → v2 migration of the 2a fixtures.
- **core-engine:** golden graph-compiler cases (latencies, alignment, sidechain alignment, bypass keeps latency); PDC sample-exact with test nodes of known latency (an impulse arrives at master at the same sample on every path); parallel renders byte-identical to `W = 0` across random DAGs, worker counts 1–8 and block sizes; zero allocations on callback and worker threads (rt-check); deadline-miss path; fault bypass after 16 bad blocks.
- **core-host:** CLAP host tests against `test-plugins` (gain with configurable latency, sidechain ducker, 4-output synth, NaN emitter, state round-trip). Latency-change recompiles.
- **platform-macos:** AU tests against Apple built-ins (`AUDelay`, `AUDynamicsProcessor` with sidechain, `AUMatrixReverb`, `DLSMusicDevice`), including state round-trip and gesture capture.
- **vizdaw-scan:** crasher and hanger test plugins end up blocklisted with the right reason; cache invalidates on modification time.
- **App:** Swift tests for the mixer view model; a smoke script; a manual checklist (plugin windows, Plugin Manager, safe mode by killing VizDAW during load, autosave restore, a sidechain and a multi-out session with the user's installed plugins).

## 14. Delivery

Three plans, built in order on branch `sp3-plugins`:
- **3a — Mixer and parallel graph:** model and file v2, graph compiler with PDC, worker pool and workgroup, test nodes, meters. Ends with the demo playing through the built-in synth via the mixer.
- **3b — Plugin hosts and scanner:** `PluginInstance`, CLAP host and `test-plugins`, AU v2/v3 host, `vizdaw-scan`, catalog and blocklist.
- **3c — App:** FFI, mixer window, plugin browser, plugin windows, Plugin Manager, autosave, crash marker and safe mode.

## Rulings (defaults chosen while writing; change any on review)

- Sidechain feeds tap the source strip post-fader, so its fader sets how hard it drives the detector — common DAW behaviour.
- Bypassed inserts keep reporting latency so toggling bypass never shifts timing.
- Plugin state lives in separate package files rather than inline JSON, keeping `project.json` diffable and undo snapshots small.
- Parameter changes are undoable per gesture via the overlay; non-parameter plugin changes are not undoable.
- Disabled/enabled is an app-wide preference, not per project.
- Default worker count `min(perf cores − 1, 7)`; on an M-series Mac mini with 4 or more performance cores this is 3+.
- Pan is constant-power (−3 dB at centre); on a stereo strip it acts as balance with the same law.
- Fader range −∞ to +12 dB.
- Test CLAP plugins are written in-repo (GPL) with `clack-plugin` rather than vendoring third-party examples.
