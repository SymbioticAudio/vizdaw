# VizDAW 3b — Plugin Hosts and Scanner Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Real AU and CLAP plugins play in the 3a mixer graph — as inserts (with sidechains and plugin delay compensation) and as plugin instruments (including multi-output instruments feeding `InstrumentOutput` strips) — with parameters, UI gestures and state flowing through core-model's history, and an out-of-process scanner that fills a catalog with a blocklist, a cache and an enabled/disabled list the 3c Plugin Manager can drive.

**Architecture:** A new Linux-buildable crate `core-host` defines the plugin abstraction — a main-thread `PluginInstance` that activates into a real-time `AudioProcessor` (Plan Ruling 1) — and implements CLAP on `clack-host 0.2`. `core-host::adapt` wraps processors as the engine's `EffectNode` inserts and `InstrumentNode` instruments; `core-host::shelf` hands activated processors to the engine through its `InsertFactory` (Ruling 3); `core-host::PluginHost` keeps one instance per document slot, loads new ones (format work on a loader thread, the rest on the main thread in `idle`, Ruling 2), feeds the parameter overlay, turns plugin-UI gestures into `SetPluginParams` commands, captures state through `History::capture_state`, and restarts plugins whose latency changes. `core-engine` learns plugin instrument targets (`TargetKind::PluginInstrument`, up to 64 targets, released targets reused one load later, Ruling 4), multi-output instrument nodes (`Tap::Out(k)`), per-block transport facts, and recreates sinks and inserts made for a smaller block (Ruling 5). `platform-macos::au` hosts AU v2 and v3 through the AudioUnit C API (Ruling 6). `vizdaw-scan` checks one plugin per process; `core-host::scan` runs it with timeouts and records crashes and hangs in `core-host::catalog`. `test-plugins` is an in-repo `clack-plugin` bundle (gain with latency, ducker, 4-output synth, NaN, crasher, hanger). The task list starts with the five carry-overs from the 3a final review.

**Tech Stack:** Rust stable (1.99.1 installed; workspace `rust-version = "1.85"`), edition 2024, Cargo workspace (resolver 3, `members = ["crates/*"]`); new crates `clack-host 0.2.0` and `clack-extensions 0.2.0` (`core-host`), `clack-plugin 0.2.0` (`test-plugins` only) — all MIT OR Apache-2.0, GPL-compatible; existing `rtrb 0.4`, `serde 1`, `serde_json 1`, `thiserror 2`, `block2 0.6`, objc2 `0.3.2` family with added features (`AudioUnitUtilities`, `objc2-core-foundation`, `block2` on `objc2-audio-toolbox`; `CFData`, `CFError`, `CFPropertyList` on `objc2-core-foundation`).

**Spec:** `docs/superpowers/specs/2026-10-16-plugin-hosting-design.md` — this plan implements only slice **3b** of its §14: §5 (`PluginInstance`, `core-host`, the CLAP host with `audio-ports`, `note-ports`, `params`, `state`, `latency`, `gui`, `tail`, `log`, `thread-check`, the AU host, `host_idle()`), §6's loader thread and latency-change recompile, §8 (scanner, catalog, blocklist, cache), §12's `HostError`, §13's core-host, platform-macos and vizdaw-scan tests, and the parameter gesture and state-capture paths of §4. The 3a plan `docs/superpowers/plans/2026-10-16-mixer-graph.md` (its Plan Rulings stay binding, notably 12 and 14, which this plan completes) and the engine spec's Rulings apply. Executors read the 3b slice of the spec, the 3a Plan Rulings and this plan's Plan Rulings.

**Out of scope (3c):** all Swift and FFI work (`core-ffi` keeps `NoInserts` and compiles unchanged apart from Task 1's one-line relocation fix), the Plugin Manager window, plugin windows and AU editors (Ruling 7), autosave, crash markers and safe mode. The catalog and host APIs below are what 3c drives.

## Global Constraints

- Every shell command runs after `export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools` (the Xcode license is not accepted, so only the Command Line Tools work). Every bash block below starts with that line and `cd /Users/lattawb/workspace/vizdaw`.
- Work on branch `sp3-plugins` (already checked out). Commit at the end of every task, adding only the files the task lists plus `Cargo.lock`; never push. End every commit message with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>` (or the attribution line your harness requires).
- Rust stable, edition 2024; every crate manifest keeps `edition.workspace = true`, `license.workspace = true`, `rust-version.workspace = true`. License GPL-3.0-only.
- `core-model`, `core-engine` and `core-host` have no OS-specific dependencies and must build and test on Linux (`clack-host` uses `libloading` there). AU code lives only in `platform-macos` (`#![cfg(target_os = "macos")]`, dependencies under `[target.'cfg(target_os = "macos")'.dependencies]`).
- `core-model`, `core-engine`, `core-host` and `platform-macos` keep `#![deny(missing_docs)]` and `#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic))]`: library code uses `get`/`get_mut`, never `[i]`. Tests and `test-plugins` may index.
- `unsafe` lives only in: the 3a modules (`core-engine/src/graph/exec.rs`, `core-engine/src/pool.rs`, `core-engine/src/rt_check.rs`, `platform-macos/src/{hal,dls,midi,rt}.rs`), plus `core-host/src/clap/ffi.rs`, `platform-macos/src/au/ffi.rs` and the crasher in `test-plugins/src/plugin.rs`. Every `unsafe` block and `unsafe impl` has a `// SAFETY:` comment.
- Everything reachable from `Renderer::process`, a worker's job, `AudioProcessor::process`/`reset` and the adapters' `process_block`/`render_outputs` is `// RT`: no allocation, free, lock, I/O, logging or syscalls beyond CoreAudio render calls and plugin process calls. Plugin buffers, event lists and queues are allocated at activation. Processors are dropped and deactivated off the audio thread (the engine retires them through its collector; the adapter sends them back to the host).
- No allocation on the callback or worker threads while plugins render, checked by `core-engine`'s `tests/rt_alloc.rs`, `tests/rt_alloc_graph.rs` and the new `core-host/tests/rt_alloc_plugins.rs`.
- Plugins are untrusted (spec §2): every plugin *error* (failing load, refused state, bad status, NaN, a hang or crash during a scan) leaves VizDAW running and is reported; a crash of an in-process plugin during playback cannot be survived.
- Automated tests use only Apple's built-in AUs (`AUDelay` `aufx/dely/appl`, `AUDynamicsProcessor` `aufx/dcmp/appl`, `AUMatrixReverb` `aufx/mrev/appl`, `DLSMusicDevice` `aumu/dls /appl`) and the in-repo CLAP test plugins; third-party plugins appear only in the human checklist (Task 14). Automated steps never play audio through a device.
- Each task ends green: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`. Run `cargo fmt --all` first.
- Code in the steps is complete: new files are written whole with `cat > … <<'VIZDAW_EOF'`; changes to existing files are `git apply` patches against the tree the previous task left. If a patch does not apply (a small fix landed on the branch meanwhile), make the same change by hand and keep going.

## Review Focus

The five failure modes a user is most likely to hit that the spec implies but no single requirement spells out, each pinned by a named test in its owning task:

1. **Editing while plugins play** — a parameter set from the mixer, its undo, a gesture in the plugin's own UI and its undo all reach the running plugin with no dropout and no echo; the gesture is one undo step. Pinned: Task 8 `overlay_edits_and_their_undo_reach_the_running_plugin`, `ui_gestures_become_undoable_commands`; Task 7 `ui_gestures_reach_the_main_thread_and_values_reach_the_plugin`; Task 9 `plugins_render_without_allocating` (values in, gestures out, every 50 blocks).
2. **A plugin that misbehaves** — one that is missing, fails to load, refuses its saved state, is disabled or blocklisted plays as a placeholder with the reason; one that emits NaN is silenced, then bypassed and reported by instance. Pinned: Task 8 `unknown_and_unavailable_plugins_are_placeholders_with_a_reason`; Task 6 `state_round_trips_and_foreign_state_is_rejected`, `unknown_ids_and_missing_bundles_are_errors`; Task 7 `a_nan_plugin_is_bypassed_and_reported_by_instance`.
3. **A plugin that changes its latency while playing** — it is restarted, the graph is recompiled with the new compensation, the end of playback waits for the compensated tail and the position survives a device rebuild. Pinned: Task 8 `a_latency_change_restarts_the_plugin_and_recompiles`; Task 6 `a_latency_change_asks_for_a_restart`; Task 1 `ended_waits_for_the_compensated_tail`, `the_transport_position_is_not_reduced_by_latency`.
4. **The device's block size grows or the project changes structure under running plugins** — instruments and inserts are re-activated for the larger block, removed instances come back and are deactivated on the main thread, meters and fault reports stay with their strip and instance. Pinned: Task 8 `a_larger_block_size_reactivates_plugins`, `removed_instances_are_deactivated_and_dropped`; Task 2 `a_larger_prepare_recreates_instrument_sinks_and_inserts`, `released_targets_are_reused_only_from_the_next_load`; Task 1 `meters_follow_strips_across_structural_edits`, `plugin_faults_survive_structural_edits`.
5. **Scanning hostile plugins** — a plugin that crashes or hangs while being checked is blocklisted with the right reason, VizDAW never dies, and a changed bundle is scanned again while an unchanged one is not. Pinned: Task 11 `crashes_hangs_and_errors_have_reasons`, `a_full_scan_fills_the_catalog_and_the_cache_follows_the_mtime`; Task 14 `scan_fills_the_catalog_and_plugins_lists_it`.

## Plan Rulings

Decisions the spec leaves open or gets wrong for 3b, each implemented and tested as stated (format: Ruling — why — cost if wrong):

1. **Ruling:** Spec §5's single `PluginInstance` object is split: `PluginInstance` (main thread, not `Send`: info, `activate(sr, max) -> Box<dyn AudioProcessor>`, `deactivate`, state, `set_params`, `param_value`, `editor`, `idle`, `gestures`) and `AudioProcessor: Send` (`process(&mut ProcessCtx) -> ProcessStatus`, `reset`) — why: CLAP (and `clack-host`'s `PluginInstance`, which is `!Send`) separates main-thread and audio-thread calls, and capture (`save_state`) must run on the main thread while the processor renders in the graph — cost if wrong: none expected; an out-of-process host implements the same two traits.
2. **Ruling:** The loader thread runs what each format may do off the main thread: AU v2 instantiation, configuration and `AudioUnitInitialize`; AU v3 asynchronous instantiation (waited for up to 30 s). CLAP bundles are loaded, created, initialized and activated on the main thread inside `PluginHost::idle`, one instance per call; the engine loads a document only when `PluginHost::sync` reports no load in progress, and the previous graph keeps playing meanwhile — why: CLAP's `create`, `init` and `activate` are `[main-thread]`, and plugins with GUIs (JUCE) assert the real main thread; spec §6 only requires instances to exist before the graph is published — cost if wrong: a slow CLAP `init` blocks the UI for its duration; moving it means lying to thread-check.
3. **Ruling:** Plugins reach the engine through a shelf: the host activates an instance, wraps the processor in `PluginEffect`/`PluginSynth` and puts it on `Shelf`; the engine's `InsertFactory` (`ShelfFactory`, with the new `make_instrument`) takes it during `Engine::load`. An adapter that is dropped (by the collector thread, or a load that did not use it) sends its processor back to the host through a channel; the host deactivates it on the main thread. A plugin asking for a restart is dropped with `Engine::restart_instance` (one graph plays it as a placeholder "restarting"), re-activated when its processor comes back, and loaded again — why: 3a Ruling 12 fixed the insert seam on the control thread, and only the main thread may deactivate — cost if wrong: a restart costs one placeholder graph (a few blocks of dry signal); a hot-swap would need processor-level handover.
4. **Ruling:** A plugin instrument destination is an engine target `TargetKind::PluginInstrument(PluginInstanceId)`; `MAX_TARGETS` grows from 16 to 64 (instrument and skip masks become `u64`), `CMD_CAPACITY` to 256 and `RETIRE_CAPACITY` to 64. Targets whose instance left the document, is restarting or was made for a smaller block are released (`EngineCmd::RemoveSink`, the sink retires through the collector); a released index is reused only from the next load, after every schedule that named it was replaced — why: targets were append-only, so every replaced plugin would have leaked a target and its instance — cost if wrong: 2 MB more renderer scratch; a project with more than 64 simultaneous targets fails to load with `TooManyTargets`.
5. **Ruling:** Instrument sinks (including the built-in synth) and inserts made for blocks smaller than `Engine::block_frames()` are recreated at the next load; `PluginHost::service` follows a grown `block_frames()` by re-activating every instance (carry-over 2 of the 3a review) — why: a plugin must never get a block larger than it was activated for — cost if wrong: notes sounding at the moment of a buffer-size increase are cut.
6. **Ruling:** The AU host uses the AudioUnit C API for AU v2 and v3 alike (v3 through `AudioComponentInstantiate`, out of process first, then in process): MIDI through `MusicDeviceMIDIEvent`, parameters through `AudioUnitGet/SetParameter` and `kAudioUnitProperty_ParameterList/Info`, plugin-UI gestures through an `AUEventListener` on a serial dispatch queue, state through `kAudioUnitProperty_ClassInfo` as a binary property list, latency changes through a property listener — instead of spec §5's `AVAudioUnit`, `AUParameterTree` and `scheduleMIDIEventListBlock` — why: one code path for both versions, no `AVFAudio` dependency, and every call has a C signature the generated bindings cover — cost if wrong: v3-only features (MIDI 2.0 event lists, parameter-tree observers) need a second path later.
7. **Ruling:** `PluginEditor` and `PluginInstance::editor()` exist in 3b, with the CLAP `gui` extension (embedded Cocoa view) implemented; AU editors (`requestViewController`, `kAudioUnitProperty_CocoaUI`) come with 3c's plugin windows; nothing in 3b opens a window (the test plugins have no GUI, so `editor()` is tested only for `None`) — why: editors need AppKit and a window, which 3c builds — cost if wrong: 3c writes the AU editor and the first real test of the CLAP one.
8. **Ruling:** An AU's sidechain is input bus 1 when the unit has two input elements (the host asks for two); Apple's built-in effects on this macOS have none, so the automated AU tests check that they report no sidechain and that the main path works, and the AU sidechain is in the human checklist; the CLAP sidechain is fully tested with the test ducker — why: spec §13 assumed `AUDynamicsProcessor` has a sidechain bus; the probe in Task 12's rehearsal found `ElementCount` fixed at 1 on every Apple effect — cost if wrong: an AU sidechain bug ships until a third-party unit is tried.
9. **Ruling:** The graph stays stereo (3a Ruling 26): a mono plugin input gets `(L + R) / 2`, a mono output feeds both sides, ports with more channels are offered only their first two — why: spec §1 allows only mono and stereo groups — cost if wrong: surround needs channel counts in `StereoBuf` users.
10. **Ruling:** Plugins receive MIDI 1.0 only in 3b: CLAP gets note on/off as CLAP note events when its note port speaks the CLAP dialect and everything else as `midi` events; AU gets `MusicDeviceMIDIEvent`; SysEx is not passed — why: the engine's sinks receive MIDI 1.0 bytes (`InstrumentNode::midi`); spec §5's MIDI 2.0 path (note expressions, UMP) needs MIDI 2.0 in the schedule first — cost if wrong: per-note expression and SysEx librarians wait for that.
11. **Ruling:** Per-block transport facts (`BlockInfo`: playing, schedule frame, beats, tempo) reach plugins through new default methods `EffectNode::process_block` and `InstrumentNode::render_outputs`, computed once per block at its first frame; CLAP plugins get a transport event assuming 4/4 — why: additive, so 3a's test nodes and the DLS sink are unchanged; the model has no time-signature lookup in the RT path yet — cost if wrong: tempo-synced plugins see 4/4 bars in other meters.
12. **Ruling:** `ended` fires only after the tail plus the graph latency `L`, and hosts relocate a rebuilt engine with `Engine::transport_tick()` (the schedule position) instead of `position_tick()` (the heard position) (carry-overs 1 and the reopen rewind of the 3a review) — why: with real plugin latency the end of the compensated audio would be cut, and every device reopen would rewind by `L` — cost if wrong: none.
13. **Ruling:** Meter cells are assigned per `StripId` (kept while the strip exists, cleared when reused) instead of per node index, and the plugin fault board is an open-addressed set keyed by instance ID instead of by insert index (carry-over 5) — why: a structural edit between two polls moved peaks to the wrong strip and could overwrite a fault report — cost if wrong: none.
14. **Ruling:** `EditDestination` from one plugin to another removes the destination's `InstrumentOutput` strips (the main strip and its inserts stay) and reports the old instance as removed (carry-over 3) — why: the old port layout no longer applies; spec §4 keeps extra strips only when the *same* plugin reports fewer outputs — cost if wrong: one edit loses the user's extra-output strips' settings (undo restores them).
15. **Ruling:** The host keeps, per instance, the parameter values as loaded from its state (`base`) and the overlay values it has sent; an overlay entry that disappears (undo of a gesture) sends the base value again, a different state blob (undo past a capture) is loaded into the running instance, and a capture folds the overlay into the base. A value that does not fit the parameter queue is resent at the next `sync` — why: spec §4 makes undo an ordinary snapshot restore, which the host must translate into parameter changes — cost if wrong: an undo could leave a stale value until the next edit.
16. **Ruling:** Gestures are grouped per instance from the first `Begin` to the matching `End` (values outside a gesture form one-value gestures); `PluginHost::idle` reports them and the caller commits them with `PluginHost::commit_gestures(&mut History, …)` as `SetPluginParams`; the gesture's values count as already sent, so the overlay sync never echoes them back — why: the host has no `History` of its own (the app owns it) — cost if wrong: none.
17. **Ruling:** The catalog (`plugins.json`, version 1) stores sources — a CLAP bundle (path, `CFBundleShortVersionString`, newest modification time of its files) or an AU component (`au:<id>`, its version, mtime 0) — and one entry per plugin; a source whose listing failed is one blocklisted entry with an empty id. Status precedence: Missing, then Blocklisted, then Disabled, then Not validated or OK. Enabled/disabled is a set of `(format, id)` in the same file (an app preference) — why: spec §8's cache key (format + id + path + version + mtime) needs the bundle as the scanning unit, because a bundle's plugin ids are known only after loading it — cost if wrong: a change to one plugin of a bundle rescans the whole bundle.
18. **Ruling:** Scanner protocol: `vizdaw-scan --format clap|au --path P --list` prints the source's plugins, `--id X` checks one plugin (load, activate at 48 kHz / 512, one silent block, deactivate) and prints its `PluginInfo`; the reply is one JSON line on stdout, errors exit 2 with a message on stderr; the caller drains both pipes, kills on timeout, and maps a signal to `crashed (signal N)`, a timeout to `timed out`, an error to `failed: <message>`; up to 4 scans run at once; an AU v3 that cannot be loaded in the scanner is "not validated" — why: spec §8 fixes one process per plugin but not the protocol — cost if wrong: none.
19. **Ruling:** The CLAP test plugins are one `clack-plugin` cdylib (`VizDAW Test.clap`) exposing six plugins; tests build it with a nested `cargo build` into `target/test-plugins` (never waiting on the outer build's lock); the gain plugin's "Wiggle" parameter makes it perform a gesture on Gain, standing in for a user — why: spec §13 needs gestures, latency changes, crashes and hangs on demand — cost if wrong: the first test of a run takes a few seconds longer.
20. **Ruling:** CLAP thread-check answers "main thread" for the thread that created the instance outside audio calls, and "audio thread" inside a scope VizDAW marks around every processor call (including `stop_processing` when a processor is dropped on the collector thread); `log` messages are dropped inside audio calls — why: spec §2 forbids I/O on the render path, and the collector and main threads both make audio-thread calls in a processor's lifetime — cost if wrong: a plugin that checks the thread name instead would complain.
21. **Ruling:** `vizdaw-play` gains `--scan`, `--plugins`, `--insert STRIP=FORMAT:ID` and `--instrument DEST=FORMAT:ID` for live playback; `--offline` keeps rendering the MIDI log without a plugin host (plugin slots are placeholders) — why: the offline golden is a MIDI log, and a command-line way to hear real plugins is needed until 3c — cost if wrong: none.

## Spec gaps and contradictions found while planning

- §5's one-object `PluginInstance` cannot be used from the main thread while it renders (Ruling 1); "v2 instantiate off the main thread" sits beside CLAP's main-thread creation rule (Ruling 2).
- §13 assumes `AUDynamicsProcessor` has a sidechain bus; no Apple effect on this macOS exposes a second input element (Ruling 8).
- §5 names `AVAudioUnit`, `AUParameterTree` and `scheduleMIDIEventListBlock`; the C API covers v2 and v3 without AVFAudio (Ruling 6). §5's MIDI 2.0 note path has no MIDI 2.0 source in the engine yet (Ruling 10).
- §6's instance reuse is silent on block-size growth (Ruling 5) and on a plugin restart (Ruling 3); §4's undo-as-snapshot-restore is silent on how removed overlay entries reach a running plugin (Ruling 15).
- 2b's 16-target limit and append-only target table cannot hold plugin instruments that come and go (Ruling 4).
- §8's cache key includes "bundle path" for every plugin, but AU components are found in the registry without a bundle (Ruling 17); §8 does not say how a bundle's plugin ids are learned without loading it in the app (the scanner's `--list`, Ruling 18).
- §9 asks for an AU editor; nothing in 3b can show one (Ruling 7).
- Carry-overs from the 3a final review (all handled in Task 1, 2 and 8): the end of playback ignored `L`, reopen rewound by `L`, sinks kept an old block size, plugin→plugin edits left stale output strips, meters and fault reports were keyed by node/insert index, and 3a lacked tests of post-fader sends, strip→bus outputs, several instruments and `InstrumentOutput` strips.

## File Structure

```
crates/core-model/
  src/command/routing.rs      plugin→plugin EditDestination drops InstrumentOutput strips (Task 1)
  src/command/strips.rs       remove_output_strips (Task 1)
  src/mixer.rs                PluginFormat: Ord (Task 8)
crates/core-engine/
  src/transport.rs            tail waits for L (Task 1)
  src/render.rs               transport position, fault board by ID, RemoveSink/Retired::Sink, capacities, BlockInfo (Tasks 1–3)
  src/engine.rs               meter slots, transport_frames/tick, plugin instrument targets, stale-sink release,
                              restart_instance, block_frames() public, feeds with outputs (Tasks 1–3)
  src/schedule/targets.rs     TargetKind::PluginInstrument, MAX_TARGETS 64, release/recycle (Task 2)
  src/schedule/tempo.rs       bpm_at (Task 3)
  src/backend.rs              BlockInfo, InstrumentNode::{outputs, render_outputs} (Task 3)
  src/graph/{spec,exec,mixer,node,mod}.rs   meter field, Tap::Out, outputs, InstrumentFeed, make_instrument,
                              TestInstrument, process_block (Tasks 1–3)
  src/fixtures.rs             Builder::plugin_dest (Task 2)
  tests/{mixer_followups,plugin_instruments}.rs   NEW
crates/core-host/             NEW (Tasks 4, 6–11)
  src/{lib,error,info,process,instance,queue}.rs   types and traits
  src/clap/{mod,ffi,handlers,instance,processor}.rs   CLAP host
  src/{adapt,shelf,host,catalog,scan}.rs   engine adapters, shelf, PluginHost, catalog, scanner protocol
  tests/{types,clap_host,engine_plugins,host,rt_alloc_plugins,catalog}.rs, tests/common/mod.rs
crates/test-plugins/          NEW (Task 5): src/{lib,plugin,bundle}.rs, tests/bundle.rs
crates/platform-macos/
  src/au/{mod,ffi}.rs         NEW AU host (Task 12)
  src/sinks.rs                PluginInstrument kind refused (Task 2)
  tests/{au,au_host}.rs       NEW (Tasks 12, 13)
crates/vizdaw-scan/           NEW scanner binary (Tasks 11, 13)
crates/vizdaw-play/           --scan/--plugins/--insert/--instrument, host_idle loop, README checklist (Task 14)
crates/core-ffi/src/host.rs   reopen relocates by transport_tick (Task 1)
```

Tests are integration tests (`tests/<name>.rs`) using the public API.

---

### Task 1: 3a carry-overs — end of playback, relocation, meters, faults, output strips

**Files:**
- Create: `crates/core-engine/tests/mixer_followups.rs`
- Modify: `crates/core-engine/src/engine.rs`
- Modify: `crates/core-engine/src/graph/exec.rs`
- Modify: `crates/core-engine/src/graph/spec.rs`
- Modify: `crates/core-engine/src/render.rs`
- Modify: `crates/core-engine/src/transport.rs`
- Modify: `crates/core-engine/tests/graph_mixer.rs`
- Modify: `crates/core-ffi/src/host.rs`
- Modify: `crates/core-model/src/command/routing.rs`
- Modify: `crates/core-model/src/command/strips.rs`
- Modify: `crates/core-model/tests/mixer_routing.rs`
- Modify: `crates/vizdaw-play/src/live.rs`
- Lockfile: `Cargo.lock` (cargo updates it when dependencies change)

**Interfaces:**
- Consumes: 3a's `Transport`, `Renderer::publish`, `Shared`, `Engine::meters`, `GraphSpec`/`StripSpec`, `RoutingCmd::EditDestination`.
- Produces: `Engine::transport_frames() -> u64`, `Engine::transport_tick() -> Tick`, `Shared::transport_position()`, `Transport.latency: u64`, `StripSpec.meter: Option<usize>`, `Shared::report_plugin_fault(Option<PluginInstanceId>)` (no index argument), `strips::remove_output_strips`.

Fixes the five carry-overs from the 3a final review that real plugin latency makes visible (Rulings 12–14), and adds the 3a test gaps for post-fader sends and strip→bus outputs. The tail now waits `L` frames more; the published transport position (schedule frame) lets a rebuilt engine continue where the old one was; meters live in cells keyed by strip; the fault board is a set of instance IDs; a plugin→plugin destination edit drops the old extra-output strips. `vizdaw-play` and `core-ffi` switch their device-reopen relocation to `transport_tick()`.

- [ ] **Step 1: Write the failing tests**


Create `crates/core-engine/tests/mixer_followups.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p crates/core-engine/tests
cat > crates/core-engine/tests/mixer_followups.rs <<'VIZDAW_EOF'
//! Engine fixes carried over from the 3a review: the end of playback waits
//! for the compensated tail, the transport position survives a rebuild,
//! meters follow strips and plugin faults follow instances across
//! structural edits.

use core_engine::backend::AudioBackend;
use core_engine::engine::{Engine, EngineConfig};
use core_engine::fixtures::{Builder, Q};
use core_engine::graph::TestInserts;
use core_engine::offline::{OfflineBackend, OfflineSinks};
use core_model::command::MixerCmd;
use core_model::document::Document;
use core_model::ids::{SeqId, StripId};
use core_model::mixer::{AuxSend, StripKind};

const SR: f64 = 48_000.0;

/// One quarter note on the built-in synth through an insert of `latency`.
fn one_note(latency: u32) -> (Document, SeqId) {
    let mut b = Builder::new();
    let d = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, lane) = b.sequence("Bar", "Notes", Some(d)).unwrap();
    b.notes(seq, lane, &[(0, 60, 0x8000, Q)]).unwrap();
    let synth = b.strip_of(StripKind::BuiltinSynth).unwrap();
    b.insert(synth, TestInserts::plugin(&format!("latency={latency}")))
        .unwrap();
    (b.finish(), seq)
}

/// Frames played until `take_ended`, in blocks of 480.
fn frames_to_end(latency: u32) -> u64 {
    let (doc, seq) = one_note(latency);
    let (mut engine, r) =
        Engine::new(EngineConfig::new(SR), Box::new(OfflineSinks::new(1000))).unwrap();
    engine.set_insert_factory(Box::new(TestInserts::new()));
    engine.load(&doc, seq).unwrap();
    let mut b = OfflineBackend::new(SR, 480);
    b.start(r).unwrap();
    engine.play().unwrap();
    for k in 1..1000u64 {
        b.run_frames(480);
        if engine.take_ended() {
            return k * 480;
        }
    }
    panic!("never ended");
}

#[test]
fn ended_waits_for_the_compensated_tail() {
    let plain = frames_to_end(0);
    assert_eq!(frames_to_end(4800), plain + 4800);
}

#[test]
fn the_transport_position_is_not_reduced_by_latency() {
    let (doc, seq) = one_note(480);
    let (mut engine, r) =
        Engine::new(EngineConfig::new(SR), Box::new(OfflineSinks::new(1000))).unwrap();
    engine.set_insert_factory(Box::new(TestInserts::new()));
    engine.load(&doc, seq).unwrap();
    let mut b = OfflineBackend::new(SR, 480);
    b.start(r).unwrap();
    engine.play().unwrap();
    b.run_frames(9600);
    assert_eq!(engine.position_frames(), 9600 - 480, "as heard");
    assert_eq!(engine.transport_frames(), 9600, "where to resume");
    assert!(engine.transport_tick() > engine.position_tick());
}

/// Synth → master, with a silent bus A (2) and a bus B (3) fed by a send.
fn buses() -> (Builder, SeqId, StripId, StripId) {
    let mut b = Builder::new();
    let d = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, lane) = b.sequence("S", "L", Some(d)).unwrap();
    b.notes(seq, lane, &[(0, 60, 0xFFFF, 8 * Q)]).unwrap();
    let synth = b.strip_of(StripKind::BuiltinSynth).unwrap();
    let a = b.bus("A").unwrap();
    let bus_b = b.bus("B").unwrap();
    b.mixer(MixerCmd::AddSend {
        strip: synth,
        send: AuxSend::new(bus_b),
    })
    .unwrap();
    (b, seq, a, bus_b)
}

#[test]
fn meters_follow_strips_across_structural_edits() {
    let (mut b, seq, a, bus_b) = buses();
    let (mut engine, r) = Engine::new(
        EngineConfig::new(SR),
        Box::new(OfflineSinks::new(1 << 16).with_tone()),
    )
    .unwrap();
    engine.load(b.doc(), seq).unwrap();
    let mut be = OfflineBackend::new(SR, 512);
    be.start(r).unwrap();
    engine.play().unwrap();
    be.run_frames(4096);
    // Removing A moves B to A's node index; B's peaks must stay B's.
    b.mixer(MixerCmd::RemoveBus { strip: a }).unwrap();
    engine.load(b.doc(), seq).unwrap();
    let m = engine.meters();
    let peak_b = m.iter().find(|x| x.strip == bus_b).unwrap().peak_l;
    assert!(peak_b > 0.0, "{m:?}");
    assert!(m.iter().all(|x| x.strip != a));
}

#[test]
fn plugin_faults_survive_structural_edits() {
    let mut b = Builder::new();
    let d = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, _) = b.sequence("S", "L", Some(d)).unwrap();
    let synth = b.strip_of(StripKind::BuiltinSynth).unwrap();
    let first = b.insert(synth, TestInserts::plugin("nan")).unwrap();
    let (mut engine, r) =
        Engine::new(EngineConfig::new(SR), Box::new(OfflineSinks::new(10))).unwrap();
    engine.set_insert_factory(Box::new(TestInserts::new()));
    engine.load(b.doc(), seq).unwrap();
    let mut be = OfflineBackend::new(SR, 256);
    be.start(r).unwrap();
    be.run_frames(20 * 256);
    // Not read yet: replace it with another faulty instance at the same
    // insert position, which faults too.
    b.mixer(MixerCmd::RemoveInsert { slot: first }).unwrap();
    let second = b.insert(synth, TestInserts::plugin("nan")).unwrap();
    engine.load(b.doc(), seq).unwrap();
    be.run_frames(20 * 256);
    assert_eq!(engine.plugin_faults(), vec![first, second]);
}
VIZDAW_EOF
```

Update `crates/core-engine/tests/graph_mixer.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-engine/tests/graph_mixer.rs b/crates/core-engine/tests/graph_mixer.rs
--- a/crates/core-engine/tests/graph_mixer.rs
+++ b/crates/core-engine/tests/graph_mixer.rs
@@ -229,3 +229,39 @@ fn missing_builtin_strip_and_silent_instrument_strips_are_reported() {
         vec!["strip \"Pad\" has no instrument until plugin hosting exists".to_string()]
     );
 }
+
+#[test]
+fn post_fader_sends_and_bus_outputs_become_post_edges() {
+    // Synth (1) → bus A (2) by its output, post-fader send at −6 dB to bus
+    // B (3); A → master. Nodes: 0 instrument, 1 master, 2 synth, 3 A, 4 B.
+    let mut b = Builder::new();
+    b.dest("Synth", "builtin:synth", 0).unwrap();
+    let synth = b.strip_of(StripKind::BuiltinSynth).unwrap();
+    let a = b.bus("A").unwrap();
+    let bus_b = b.bus("B").unwrap();
+    b.mixer(MixerCmd::SetOutput {
+        strip: synth,
+        output: core_model::mixer::Output::Bus(a),
+    })
+    .unwrap();
+    let mut send = AuxSend::new(bus_b);
+    send.level_db = -6.020_6;
+    b.mixer(MixerCmd::AddSend { strip: synth, send }).unwrap();
+    let g = mixer_spec(b.doc(), &[(TargetIdx(0), 0)], &mut zero);
+    // Master hears A and B, not the synth directly.
+    assert_eq!(strip(&g.spec, 1).inputs, vec![post(3, 1.0), post(4, 1.0)]);
+    assert_eq!(strip(&g.spec, 3).inputs, vec![post(2, 1.0)]);
+    let to_b = &strip(&g.spec, 4).inputs;
+    assert_eq!(to_b.len(), 1);
+    assert_eq!((to_b[0].from, to_b[0].tap), (2, Tap::Post));
+    assert!((to_b[0].gain - 0.5).abs() < 1e-4);
+    // Muting the synth silences its output and its post send alike.
+    b.mixer(MixerCmd::SetMute {
+        strip: synth,
+        mute: true,
+    })
+    .unwrap();
+    let g = mixer_spec(b.doc(), &[(TargetIdx(0), 0)], &mut zero);
+    assert_eq!(strip(&g.spec, 3).inputs, vec![post(2, 0.0)]);
+    assert_eq!(strip(&g.spec, 4).inputs[0].gain, 0.0);
+}
VIZDAW_EOF
```

Update `crates/core-model/tests/mixer_routing.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-model/tests/mixer_routing.rs b/crates/core-model/tests/mixer_routing.rs
--- a/crates/core-model/tests/mixer_routing.rs
+++ b/crates/core-model/tests/mixer_routing.rs
@@ -293,3 +293,71 @@ fn removing_a_plugin_destination_is_one_undo_step() {
     assert_eq!(h.doc().mixer().strips().len(), 1);
     assert_eq!(h.doc().counters(), start.counters());
 }
+
+#[test]
+fn changing_the_plugin_removes_its_extra_output_strips() {
+    use core_model::command::MixerCmd;
+    let a = add(&Document::new(), "Drums", DestKind::plugin("clap:kit")).doc;
+    let b = apply(
+        &a,
+        &Command::Mixer(MixerCmd::AddInstrumentOutput {
+            dest: DestId(0),
+            index: 1,
+            name: "Kick".into(),
+        }),
+    )
+    .unwrap()
+    .doc;
+    let out = b
+        .mixer()
+        .find_kind(StripKind::InstrumentOutput(DestId(0), 1))
+        .unwrap();
+    // Same plugin, new version: the output strip stays.
+    let mut same = DestKind::plugin("clap:kit");
+    if let DestKind::PluginInstrument { slot } = &mut same {
+        slot.plugin.version = "2".into();
+    }
+    let c = run(
+        &b,
+        RoutingCmd::EditDestination {
+            dest: DestId(0),
+            kind: same,
+            default_patch: None,
+        },
+    );
+    assert!(c.doc.mixer().strip(out).is_ok());
+    // Another plugin: its port layout is unknown, so the output strip goes
+    // (the main strip and its inserts stay) and the old instance is reported.
+    let d = run(
+        &c.doc,
+        RoutingCmd::EditDestination {
+            dest: DestId(0),
+            kind: DestKind::plugin("clap:other"),
+            default_patch: None,
+        },
+    );
+    assert!(d.doc.mixer().strip(out).is_err());
+    assert!(
+        d.doc
+            .mixer()
+            .find_kind(StripKind::Instrument(DestId(0)))
+            .is_some()
+    );
+    assert_eq!(
+        d.report.removed,
+        vec![AnyId::Plugin(PluginInstanceId(0)), AnyId::Strip(out)]
+    );
+    // One undo step restores the strip.
+    let mut h = History::new(c.doc.clone());
+    h.execute(
+        &Command::Routing(RoutingCmd::EditDestination {
+            dest: DestId(0),
+            kind: DestKind::plugin("clap:other"),
+            default_patch: None,
+        }),
+        &ViewFilter::default(),
+    )
+    .unwrap();
+    h.undo().unwrap();
+    assert!(h.doc().mixer().strip(out).is_ok());
+}
VIZDAW_EOF
```

- [ ] **Step 2: Run them to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test mixer_routing
cargo test -p core-engine --test mixer_followups --test graph_mixer
```

Expected: `changing_the_plugin_removes_its_extra_output_strips` fails (the output strip survives the edit); the core-engine run does not compile: `no method named `transport_frames` found for struct `Engine``.

- [ ] **Step 3: Implement**


Change `crates/core-engine/src/engine.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-engine/src/engine.rs b/crates/core-engine/src/engine.rs
--- a/crates/core-engine/src/engine.rs
+++ b/crates/core-engine/src/engine.rs
@@ -5,7 +5,7 @@ use crate::backend::MidiPortInfo;
 use crate::error::{BackendError, EngineError};
 use crate::graph::mixer::mixer_spec;
 use crate::graph::{
-    EffectNode, Graph, GraphReport, InsertFactory, InsertInit, NoInserts, PlanError, plan,
+    EffectNode, Graph, GraphReport, InsertFactory, InsertInit, NoInserts, NodeSpec, PlanError, plan,
 };
 use crate::pool::{Pool, RtEnv, RtPlatform, StdPlatform};
 use crate::render::{
@@ -121,8 +121,10 @@ pub struct Engine {
     instrument_latency: [Option<u32>; MAX_TARGETS],
     /// Insert instances in the last published graph, by ID.
     live: BTreeMap<PluginInstanceId, Live>,
-    /// `(node, strip)` of the last published graph.
-    strip_nodes: Vec<(usize, StripId)>,
+    /// Meter cell of every strip of the last published graph, by strip
+    /// (cells are keyed by strip, not node, so a structural edit never
+    /// moves a strip's peaks to another strip).
+    meter_slots: BTreeMap<StripId, usize>,
     /// Latency of the last published graph.
     latency: u64,
     collector_stop: Arc<AtomicBool>,
@@ -233,7 +235,7 @@ impl Engine {
             default_loop: false,
             instrument_latency: [None; MAX_TARGETS],
             live: BTreeMap::new(),
-            strip_nodes: Vec::new(),
+            meter_slots: BTreeMap::new(),
             latency: 0,
             collector_stop,
             collector_paused,
@@ -360,6 +362,33 @@ impl Engine {
         Ok(report)
     }
 
+    /// Meter cells for `strips`: a strip keeps its cell; a new strip gets
+    /// the lowest free one, cleared first (Plan Ruling 13).
+    fn assign_meters(&self, strips: &[(usize, StripId)]) -> BTreeMap<StripId, usize> {
+        let mut out = BTreeMap::new();
+        for (_, id) in strips {
+            if let Some(c) = self.meter_slots.get(id) {
+                out.insert(*id, *c);
+            }
+        }
+        let mut used: std::collections::BTreeSet<usize> = out.values().copied().collect();
+        let mut next = 0usize;
+        for (_, id) in strips {
+            if out.contains_key(id) {
+                continue;
+            }
+            while used.contains(&next) {
+                next += 1;
+            }
+            if let Some(m) = self.shared.meter(next) {
+                m.take();
+                out.insert(*id, next);
+                used.insert(next);
+            }
+        }
+        out
+    }
+
     /// Builds and publishes the mixer graph. A graph that cannot be planned
     /// is reported (and its fault raised) and the previous graph keeps
     /// playing (spec §6).
@@ -406,7 +435,15 @@ impl Engine {
                 }
             }
         };
-        let mg = mixer_spec(doc, &instruments, &mut latency);
+        let mut mg = mixer_spec(doc, &instruments, &mut latency);
+        let slots = self.assign_meters(&mg.strips);
+        for (node, strip) in &mg.strips {
+            if let (Some(NodeSpec::Strip(s)), Some(slot)) =
+                (mg.spec.nodes.get_mut(*node), slots.get(strip))
+            {
+                s.meter = Some(*slot);
+            }
+        }
         let mut report = GraphReport {
             nodes: mg.spec.nodes.len(),
             latency_frames: 0,
@@ -456,7 +493,7 @@ impl Engine {
         self.push(EngineCmd::SetGraph(Box::new(graph)))?;
         self.live = next_live;
         self.latency = report.latency_frames;
-        self.strip_nodes = mg.strips;
+        self.meter_slots = slots;
         Ok(report)
     }
 
@@ -522,6 +559,20 @@ impl Engine {
             .map_or(Tick::ZERO, |t| t.frame_to_tick(f))
     }
 
+    /// Schedule frame of the transport, not reduced by the graph latency:
+    /// where a rebuilt engine locates to continue (a device reopen).
+    pub fn transport_frames(&self) -> u64 {
+        self.shared.transport_position()
+    }
+
+    /// [`Engine::transport_frames`] as a tick of the loaded sequence.
+    pub fn transport_tick(&self) -> Tick {
+        let f = self.shared.transport_position();
+        self.tempo
+            .as_ref()
+            .map_or(Tick::ZERO, |t| t.frame_to_tick(f))
+    }
+
     /// Frame of the loaded sequence's end.
     pub fn length_frames(&self) -> u64 {
         self.length_frames
@@ -555,10 +606,10 @@ impl Engine {
     /// Peak meters of every strip of the last published graph since the
     /// last call (read and cleared).
     pub fn meters(&self) -> Vec<StripMeter> {
-        self.strip_nodes
+        self.meter_slots
             .iter()
-            .filter_map(|(node, strip)| {
-                let (l, r) = self.shared.meter(*node)?.take();
+            .filter_map(|(strip, slot)| {
+                let (l, r) = self.shared.meter(*slot)?.take();
                 Some(StripMeter {
                     strip: *strip,
                     peak_l: l,
VIZDAW_EOF
```

Change `crates/core-engine/src/graph/exec.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-engine/src/graph/exec.rs b/crates/core-engine/src/graph/exec.rs
--- a/crates/core-engine/src/graph/exec.rs
+++ b/crates/core-engine/src/graph/exec.rs
@@ -137,6 +137,7 @@ enum NodeDef {
         gain_l: f32,
         gain_r: f32,
         audible: bool,
+        meter: usize,
     },
 }
 
@@ -373,6 +374,7 @@ impl Graph {
                         gain_l: s.gain_l,
                         gain_r: s.gain_r,
                         audible: s.audible,
+                        meter: s.meter.unwrap_or(i),
                     });
                     nodes.push(UnsafeCell::new(NodeState::strip(
                         max_frames, has_sc, delays, sc_delays,
@@ -513,7 +515,6 @@ fn run_insert(
     io: &mut StereoBuf,
     sc: Option<&StereoBuf>,
     shared: &Shared,
-    idx: usize,
 ) {
     if bypass || ins.faulted {
         if !ins.in_bypass {
@@ -534,7 +535,7 @@ fn run_insert(
         if ins.bad_run >= FAULT_LIMIT {
             ins.faulted = true;
             node.reset();
-            shared.report_plugin_fault(idx, ins.instance);
+            shared.report_plugin_fault(ins.instance);
         }
     } else {
         ins.bad_run = 0;
@@ -575,6 +576,7 @@ unsafe fn render_node(g: &Graph, sinks: &SinkTable, shared: &Shared, frames: u32
             gain_l,
             gain_r,
             audible,
+            meter,
         } => {
             st.pre.clear(frames);
             for (k, inp) in g
@@ -630,13 +632,13 @@ unsafe fn render_node(g: &Graph, sinks: &SinkTable, shared: &Shared, frames: u32
                     }
                     None => None,
                 };
-                run_insert(ins, idef.bypass, frames, &mut st.pre, sc, shared, idx);
+                run_insert(ins, idef.bypass, frames, &mut st.pre, sc, shared);
             }
             dsp::copy(&mut st.post, &st.pre, frames);
             dsp::scale(&mut st.post, *gain_l, *gain_r, frames);
             if *audible {
                 let (l, r) = dsp::peaks(&st.post, frames);
-                if let Some(m) = shared.meter(i) {
+                if let Some(m) = shared.meter(*meter) {
                     m.hold(l, r);
                 }
             }
VIZDAW_EOF
```

Change `crates/core-engine/src/graph/spec.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-engine/src/graph/spec.rs b/crates/core-engine/src/graph/spec.rs
--- a/crates/core-engine/src/graph/spec.rs
+++ b/crates/core-engine/src/graph/spec.rs
@@ -55,6 +55,10 @@ pub struct StripSpec {
     /// False when muted (explicitly or by solo): metered silent and, for
     /// master, not written to the device.
     pub audible: bool,
+    /// Meter cell this strip writes (`Shared::meter`); `None` uses the
+    /// node index (graph tests). The engine gives every strip a cell keyed
+    /// by its `StripId`, so meters survive structural edits.
+    pub meter: Option<usize>,
 }
 
 impl StripSpec {
@@ -67,6 +71,7 @@ impl StripSpec {
             gain_l: 1.0,
             gain_r: 1.0,
             audible: true,
+            meter: None,
         }
     }
 }
VIZDAW_EOF
```

Change `crates/core-engine/src/render.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-engine/src/render.rs b/crates/core-engine/src/render.rs
--- a/crates/core-engine/src/render.rs
+++ b/crates/core-engine/src/render.rs
@@ -140,6 +140,7 @@ pub struct DspLoad {
 #[derive(Debug)]
 pub struct Shared {
     position: AtomicU64,
+    transport_pos: AtomicU64,
     state: AtomicU8,
     faults: AtomicU32,
     ended: AtomicBool,
@@ -162,6 +163,7 @@ impl Default for Shared {
     fn default() -> Shared {
         Shared {
             position: AtomicU64::new(0),
+            transport_pos: AtomicU64::new(0),
             state: AtomicU8::new(0),
             faults: AtomicU32::new(0),
             ended: AtomicBool::new(false),
@@ -198,6 +200,12 @@ impl Shared {
         self.position.load(Ordering::Relaxed)
     }
 
+    /// Schedule frame of the transport (not reduced by the graph latency):
+    /// where a rebuilt engine must locate to continue seamlessly.
+    pub fn transport_position(&self) -> u64 {
+        self.transport_pos.load(Ordering::Relaxed)
+    }
+
     /// Transport state.
     pub fn state(&self) -> PlayState {
         PlayState::from_u8(self.state.load(Ordering::Relaxed))
@@ -230,26 +238,43 @@ impl Shared {
         self.meters.get(i)
     }
 
-    /// Records that the insert at board index `slot` was bypassed for
-    /// faults. // RT
-    pub fn report_plugin_fault(&self, slot: usize, instance: Option<PluginInstanceId>) {
+    /// Records that `instance` was bypassed for faults. The board is an
+    /// open-addressed set keyed by instance ID, so an ID survives structural
+    /// edits until it is read. // RT
+    pub fn report_plugin_fault(&self, instance: Option<PluginInstanceId>) {
         self.raise(Faults::PLUGIN_FAULT);
         self.plugin_faults.fetch_add(1, Ordering::Relaxed);
-        if let (Some(cell), Some(id)) = (self.board.get(slot), instance) {
-            cell.store(id.0.saturating_add(1), Ordering::Relaxed);
+        let Some(id) = instance else {
+            return;
+        };
+        let v = id.0.saturating_add(1);
+        let n = self.board.len().max(1);
+        let start = (id.0 % n as u64) as usize;
+        for k in 0..n {
+            let Some(cell) = self.board.get((start + k) % n) else {
+                return;
+            };
+            match cell.compare_exchange(0, v, Ordering::Relaxed, Ordering::Relaxed) {
+                Ok(_) => return,
+                Err(x) if x == v => return,
+                Err(_) => {}
+            }
         }
     }
 
     /// Instances bypassed for faults since the last call (read and
-    /// cleared).
+    /// cleared), in ascending ID order.
     pub fn take_plugin_faults(&self) -> Vec<PluginInstanceId> {
-        self.board
+        let mut v: Vec<PluginInstanceId> = self
+            .board
             .iter()
             .filter_map(|c| {
                 let v = c.swap(0, Ordering::Relaxed);
                 (v != 0).then(|| PluginInstanceId(v - 1))
             })
-            .collect()
+            .collect();
+        v.sort();
+        v
     }
 
     /// Stores one block's DSP load. // RT
@@ -743,6 +768,9 @@ impl Renderer {
                     }
                 }
                 self.latency = new.latency();
+                // The compensated audio of the last notes arrives L frames
+                // late: the tail waits for it before `ended`.
+                self.transport.latency = self.latency;
                 self.graph = Some(new);
                 self.shared.graphs_applied.fetch_add(1, Ordering::Relaxed);
             }
@@ -807,6 +835,7 @@ impl Renderer {
             self.transport.pos.saturating_sub(self.latency),
             Ordering::Relaxed,
         );
+        s.transport_pos.store(self.transport.pos, Ordering::Relaxed);
         s.state.store(self.transport.state as u8, Ordering::Relaxed);
         if self.transport.take_ended() {
             s.ended.store(true, Ordering::Relaxed);
VIZDAW_EOF
```

Change `crates/core-engine/src/transport.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-engine/src/transport.rs b/crates/core-engine/src/transport.rs
--- a/crates/core-engine/src/transport.rs
+++ b/crates/core-engine/src/transport.rs
@@ -80,6 +80,9 @@ pub struct Transport {
     pub loop_range: Option<(u64, u64)>,
     /// Tail length in frames.
     pub tail_frames: u64,
+    /// Graph latency `L`: added to the tail, so `ended` fires only after
+    /// the delay-compensated audio of the end has been heard.
+    pub latency: u64,
     tail_left: u64,
     ended: bool,
 }
@@ -92,6 +95,7 @@ impl Transport {
             pos: 0,
             loop_range: None,
             tail_frames,
+            latency: 0,
             tail_left: 0,
             ended: false,
         }
@@ -160,7 +164,7 @@ impl Transport {
                 };
                 if self.pos >= end {
                     self.state = PlayState::Tail;
-                    self.tail_left = self.tail_frames;
+                    self.tail_left = self.tail_frames.saturating_add(self.latency);
                     if self.tail_left == 0 {
                         self.state = PlayState::Stopped;
                         self.ended = true;
VIZDAW_EOF
```

Change `crates/core-ffi/src/host.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-ffi/src/host.rs b/crates/core-ffi/src/host.rs
--- a/crates/core-ffi/src/host.rs
+++ b/crates/core-ffi/src/host.rs
@@ -326,7 +326,7 @@ fn handle_events(slot: &mut Option<Host>, doc: &Document) -> Result<(), VizError
                 let Some(mut old) = slot.take() else {
                     return Ok(());
                 };
-                let at = old.engine.position_tick();
+                let at = old.engine.transport_tick();
                 let playing = old.engine.state() == PlayState::Playing;
                 if !matches!(ev, BackendEvent::DeviceLost) {
                     // The device still renders: let the note-offs play.
VIZDAW_EOF
```

Change `crates/core-model/src/command/routing.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-model/src/command/routing.rs b/crates/core-model/src/command/routing.rs
--- a/crates/core-model/src/command/routing.rs
+++ b/crates/core-model/src/command/routing.rs
@@ -272,9 +272,16 @@ pub(crate) fn apply(d: &mut Document, cmd: &RoutingCmd, r: &mut Report) -> Resul
                         *slot = o.clone();
                         slot.plugin = plugin;
                     }
-                    _ => {
+                    other => {
                         slot.id = d.counters_mut().alloc_plugin();
                         r.created.push(AnyId::Plugin(slot.id));
+                        if let DestKind::PluginInstrument { slot: o } = other {
+                            // A different plugin: the old instance and its
+                            // extra-output strips go (their port layout is
+                            // the old plugin's).
+                            r.removed.push(AnyId::Plugin(o.id));
+                            strips::remove_output_strips(d, *dest, r);
+                        }
                     }
                 }
             }
VIZDAW_EOF
```

Change `crates/core-model/src/command/strips.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-model/src/command/strips.rs b/crates/core-model/src/command/strips.rs
--- a/crates/core-model/src/command/strips.rs
+++ b/crates/core-model/src/command/strips.rs
@@ -57,6 +57,22 @@ pub(crate) fn remove_dest_strips(d: &mut Document, dest: DestId, r: &mut Report)
     }
 }
 
+/// Removes `dest`'s `InstrumentOutput` strips (not its main strip).
+pub(crate) fn remove_output_strips(d: &mut Document, dest: DestId, r: &mut Report) {
+    let ids: Vec<StripId> = d
+        .mixer()
+        .strips()
+        .values()
+        .filter(|s| matches!(s.kind, StripKind::InstrumentOutput(x, _) if x == dest))
+        .map(|s| s.id)
+        .collect();
+    for id in ids {
+        if d.mixer_mut().remove(id).is_some() {
+            r.removed.push(AnyId::Strip(id));
+        }
+    }
+}
+
 /// Removes the strip of `kind`, if any.
 pub(crate) fn remove_kind(d: &mut Document, kind: StripKind, r: &mut Report) {
     if let Some(id) = d.mixer().find_kind(kind) {
VIZDAW_EOF
```

Change `crates/vizdaw-play/src/live.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/vizdaw-play/src/live.rs b/crates/vizdaw-play/src/live.rs
--- a/crates/vizdaw-play/src/live.rs
+++ b/crates/vizdaw-play/src/live.rs
@@ -188,7 +188,7 @@ pub fn run(args: &Args, doc: &Document, seq: SeqId) -> Result<(), String> {
                 | BackendEvent::DefaultDeviceChanged
                 | BackendEvent::SampleRateChanged(_) => {
                     eprintln!("\n{ev:?}: reopening the output");
-                    let at = s.engine.position_tick();
+                    let at = s.engine.transport_tick();
                     let playing = s.engine.state() == PlayState::Playing;
                     if !matches!(ev, BackendEvent::DeviceLost) {
                         // The device still renders: let the engine send its
VIZDAW_EOF
```

- [ ] **Step 4: Run the tests to see them pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test mixer_routing
cargo test -p core-engine --test mixer_followups --test graph_mixer
```

Expected: all pass: `mixer_routing` 7 tests, `mixer_followups` 4 tests (`ended_waits_for_the_compensated_tail`, `the_transport_position_is_not_reduced_by_latency`, `meters_follow_strips_across_structural_edits`, `plugin_faults_survive_structural_edits`), `graph_mixer` including `post_fader_sends_and_bus_outputs_become_post_edges`.

- [ ] **Step 5: Check the whole workspace**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Expected: no formatting changes left, no clippy warnings, every test passes.

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-engine/tests/mixer_followups.rs \
  crates/core-engine/src/engine.rs \
  crates/core-engine/src/graph/exec.rs \
  crates/core-engine/src/graph/spec.rs \
  crates/core-engine/src/render.rs \
  crates/core-engine/src/transport.rs \
  crates/core-engine/tests/graph_mixer.rs \
  crates/core-ffi/src/host.rs \
  crates/core-model/src/command/routing.rs \
  crates/core-model/src/command/strips.rs \
  crates/core-model/tests/mixer_routing.rs \
  crates/vizdaw-play/src/live.rs \
  Cargo.lock
git commit -m "$(cat <<'MSG'
fix(engine): wait for the compensated tail, relocate by transport, meters by strip, faults by instance

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
MSG
)"
```


---

### Task 2: Plugin instrument targets with release and reuse

**Files:**
- Create: `crates/core-engine/tests/plugin_instruments.rs`
- Modify: `crates/core-engine/src/engine.rs`
- Modify: `crates/core-engine/src/error.rs`
- Modify: `crates/core-engine/src/fixtures.rs`
- Modify: `crates/core-engine/src/graph/exec.rs`
- Modify: `crates/core-engine/src/graph/mixer.rs`
- Modify: `crates/core-engine/src/graph/mod.rs`
- Modify: `crates/core-engine/src/graph/node.rs`
- Modify: `crates/core-engine/src/offline.rs`
- Modify: `crates/core-engine/src/render.rs`
- Modify: `crates/core-engine/src/schedule/targets.rs`
- Modify: `crates/core-engine/tests/engine.rs`
- Modify: `crates/core-engine/tests/graph_mixer.rs`
- Modify: `crates/core-engine/tests/offline.rs`
- Modify: `crates/core-engine/tests/rt_alloc.rs`
- Modify: `crates/core-engine/tests/rt_alloc_graph.rs`
- Modify: `crates/core-engine/tests/schedule.rs`
- Modify: `crates/core-engine/tests/tempo_table.rs`
- Modify: `crates/platform-macos/src/sinks.rs`
- Lockfile: `Cargo.lock` (cargo updates it when dependencies change)

**Interfaces:**
- Consumes: Task 1's engine; 3a's `TargetTable`, `InsertFactory`, `SinkFactory`, `mixer_spec`, `TestInserts`.
- Produces: `TargetKind::PluginInstrument(PluginInstanceId)`; `MAX_TARGETS = 64`; `TargetTable::{next_index, release, recycle}`; `EngineCmd::RemoveSink(TargetIdx)`, `Retired::Sink(Sink)`; `CMD_CAPACITY = 256`, `RETIRE_CAPACITY = 64`; `InsertFactory::make_instrument(&mut self, &PluginSlot, f64, u32) -> Result<Box<dyn InstrumentNode>, String>` (default `Err`); `Engine::restart_instance(PluginInstanceId)`, `pub Engine::block_frames() -> u32`; `graph::{InstrumentFeed, FeedSource, TestInstrument}`, `InstrumentFeed::{builtin(TargetIdx, u32), plugin(TargetIdx, u32, DestId)}`, `mixer_spec(&Document, &[InstrumentFeed], …)`; `BackendError::NotSupported(String)`; `Builder::plugin_dest(&str, PluginRef) -> Result<(DestId, PluginInstanceId)>`.

Plugin instrument destinations become engine targets whose sinks come from the plugin factory (Ruling 4). `Engine::load` first makes indices released by the previous load reusable, then releases targets whose instance left the document, is restarting or was made for a smaller block (Ruling 5 — this also recreates the built-in synth's sink after a larger `prepare`), then interns and makes sinks. `mixer_spec` takes instrument feeds: the built-in synth feeds its strip as before; a plugin instrument feeds its destination's `Instrument` strip. `TestInserts` makes `TestInstrument`s (`vizdaw.test:synth[,outputs=N][,latency=L]`) for engine tests. Existing tests that hard-coded 16 targets, 8 retire slots or the old "cannot play until plugin hosting exists" text are updated.

- [ ] **Step 1: Write the failing tests**


Create `crates/core-engine/tests/plugin_instruments.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p crates/core-engine/tests
cat > crates/core-engine/tests/plugin_instruments.rs <<'VIZDAW_EOF'
//! Plugin instrument destinations as engine targets: their sink comes from
//! the plugin factory, plays through their `Instrument` strip and is
//! released when the destination goes, restarts or is too small for the
//! renderer.

use core_engine::backend::AudioBackend;
use core_engine::engine::{Engine, EngineConfig};
use core_engine::fixtures::{Builder, Q};
use core_engine::graph::{TestInserts, TestInstrument};
use core_engine::offline::{OfflineBackend, OfflineSinks};
use core_engine::schedule::{TargetIdx, TargetKind};
use core_model::command::{Command, RoutingCmd};
use core_model::document::Document;
use core_model::ids::{DestId, PluginInstanceId, SeqId};
use core_model::mixer::StripKind;

const SR: f64 = 48_000.0;

/// A test synth holding one long note on channel 0.
fn synth_doc() -> (Builder, SeqId, DestId, PluginInstanceId) {
    let mut b = Builder::new();
    let (d, id) = b
        .plugin_dest("Synth", TestInstrument::plugin("synth"))
        .unwrap();
    let (seq, lane) = b.sequence("S", "L", Some(d)).unwrap();
    b.notes(seq, lane, &[(0, 60, 0xFFFF, 16 * Q)]).unwrap();
    (b, seq, d, id)
}

fn engine(max_frames: u32) -> (Engine, OfflineBackend, TestInserts) {
    let mut cfg = EngineConfig::new(SR);
    cfg.max_frames = max_frames;
    let (mut engine, r) = Engine::new(cfg, Box::new(OfflineSinks::new(1 << 16))).unwrap();
    let f = TestInserts::new();
    engine.set_insert_factory(Box::new(f.clone()));
    let mut b = OfflineBackend::new(SR, 256);
    b.start(r).unwrap();
    (engine, b, f)
}

fn plugin_targets(e: &Engine) -> Vec<(TargetIdx, PluginInstanceId)> {
    e.targets()
        .iter()
        .filter_map(|(i, k)| match k {
            TargetKind::PluginInstrument(id) => Some((i, *id)),
            _ => None,
        })
        .collect()
}

#[test]
fn a_plugin_instrument_plays_through_its_strip() {
    let (b, seq, _, id) = synth_doc();
    let (mut engine, mut be, made) = engine(4096);
    let report = engine.load(b.doc(), seq).unwrap();
    assert!(report.unroutable.is_empty(), "{:?}", report.warnings);
    assert!(
        report.graph.warnings.is_empty(),
        "{:?}",
        report.graph.warnings
    );
    assert_eq!(report.graph.nodes, 3, "instrument, master, its strip");
    assert_eq!(plugin_targets(&engine), vec![(TargetIdx(0), id)]);
    assert_eq!(made.made(), 1);
    engine.play().unwrap();
    be.run_frames(1024);
    // 0.1 through the strip's centred pan (−3 dB), master unpanned.
    let (l, r) = be.output(256);
    let want = 0.1 * std::f32::consts::FRAC_1_SQRT_2;
    assert!((l[100] - want).abs() < 1e-6 && (r[100] - want).abs() < 1e-6);
}

#[test]
fn without_a_plugin_factory_the_destination_is_unroutable() {
    let (b, seq, _, _) = synth_doc();
    let (mut engine, _r) =
        Engine::new(EngineConfig::new(SR), Box::new(OfflineSinks::new(10))).unwrap();
    let report = engine.load(b.doc(), seq).unwrap();
    assert_eq!(report.unroutable.len(), 1);
    assert!(
        report.warnings[0].contains("cannot load until plugin hosting exists"),
        "{:?}",
        report.warnings
    );
    assert_eq!(
        report.graph.warnings,
        vec!["strip \"Synth\" has no instrument".to_string()]
    );
    assert!(engine.targets().is_empty());
}

fn replace(doc: &Document, dest: DestId, with: DestId) -> Document {
    core_model::command::apply(
        doc,
        &Command::Routing(RoutingCmd::RemoveDestination {
            dest,
            replacement: Some(with),
        }),
    )
    .unwrap()
    .doc
}

#[test]
fn released_targets_are_reused_only_from_the_next_load() {
    let (mut b, seq, d, first) = synth_doc();
    let (mut engine, mut be, _) = engine(4096);
    engine.load(b.doc(), seq).unwrap();
    be.run_frames(256);
    // Replace the synth with another one in a single edit.
    let (d2, second) = b
        .plugin_dest("Synth 2", TestInstrument::plugin("synth"))
        .unwrap();
    let mut b = Builder::from_doc(replace(b.doc(), d, d2));
    engine.load(b.doc(), seq).unwrap();
    assert_eq!(
        plugin_targets(&engine),
        vec![(TargetIdx(1), second)],
        "index 0 is released, not reused in the same load"
    );
    assert_ne!(first, second);
    be.run_frames(256);
    let (d3, third) = b
        .plugin_dest("Synth 3", TestInstrument::plugin("synth"))
        .unwrap();
    let lane = b.lane(seq, "L3", Some(d3)).unwrap();
    b.notes(seq, lane, &[(0, 64, 0xFFFF, Q)]).unwrap();
    engine.load(b.doc(), seq).unwrap();
    assert_eq!(
        plugin_targets(&engine),
        vec![(TargetIdx(0), third), (TargetIdx(1), second)]
    );
}

#[test]
fn restart_instance_recreates_inserts_and_instruments() {
    let (mut b, seq, _, id) = synth_doc();
    let strip = b.strip_of(StripKind::Instrument(DestId(0))).unwrap();
    let gain = b
        .insert(strip, TestInserts::plugin("gain=0.5,latency=16"))
        .unwrap();
    let (mut engine, mut be, made) = engine(4096);
    engine.load(b.doc(), seq).unwrap();
    be.run_frames(256);
    assert_eq!(made.made(), 2);
    engine.load(b.doc(), seq).unwrap();
    assert_eq!(made.made(), 2, "an ordinary load keeps both");
    engine.restart_instance(gain);
    engine.restart_instance(id);
    engine.load(b.doc(), seq).unwrap();
    assert_eq!(made.made(), 4, "both recreated");
    engine.load(b.doc(), seq).unwrap();
    assert_eq!(made.made(), 4, "a restart applies to one load");
}

#[test]
fn a_larger_prepare_recreates_instrument_sinks_and_inserts() {
    let (mut b, seq, _, _) = synth_doc();
    let strip = b.strip_of(StripKind::Instrument(DestId(0))).unwrap();
    b.insert(strip, TestInserts::plugin("gain=0.5")).unwrap();
    let (mut engine, mut be, made) = engine(512);
    engine.load(b.doc(), seq).unwrap();
    engine.play().unwrap();
    be.run_blocks([256]);
    assert_eq!(made.made(), 2);
    let mut r = be.stop().unwrap();
    r.prepare(2048);
    be.start(r).unwrap();
    engine.load(b.doc(), seq).unwrap();
    assert_eq!(made.made(), 4, "made again for 2048-frame blocks");
    // The new synth missed the held note: play it again from the start.
    engine.locate_frame(0).unwrap();
    be.run_blocks([2048, 2048]);
    let (l, _) = be.output(2048);
    assert!(l.iter().all(|x| *x > 0.0), "plays at the new size");
    assert!(engine.faults().is_empty(), "{:?}", engine.faults().names());
}
VIZDAW_EOF
```

Update `crates/core-engine/tests/engine.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-engine/tests/engine.rs b/crates/core-engine/tests/engine.rs
--- a/crates/core-engine/tests/engine.rs
+++ b/crates/core-engine/tests/engine.rs
@@ -7,7 +7,7 @@ use core_engine::backend::{BlockTime, InstrumentNode, MidiOut, MidiPortInfo, Ste
 use core_engine::engine::{Engine, EngineConfig, SinkFactory, find_sequence};
 use core_engine::fixtures::{Builder, Q};
 use core_engine::render::{Faults, Renderer, Sink};
-use core_engine::schedule::{BuiltinRoute, TargetIdx, TargetKind};
+use core_engine::schedule::{BuiltinRoute, MAX_TARGETS, TargetIdx, TargetKind};
 use core_engine::transport::PlayState;
 use core_model::document::Document;
 use core_model::ids::SeqId;
@@ -242,7 +242,7 @@ impl MidiOut for PortOut {
 
 impl SinkFactory for PortFactory {
     fn ports(&self) -> Vec<MidiPortInfo> {
-        (0..17)
+        (0..=MAX_TARGETS)
             .map(|i| MidiPortInfo {
                 name: format!("Port {i}"),
                 unique_id: None,
@@ -285,13 +285,13 @@ fn failed_load_leaves_the_target_table_unchanged() {
     let f = PortFactory::default();
     let (mut engine, mut r) =
         Engine::new(EngineConfig::new(48_000.0), Box::new(f.clone())).unwrap();
-    let (doc, seq) = ports_doc(17);
+    let (doc, seq) = ports_doc(MAX_TARGETS + 1);
     assert_eq!(engine.load(&doc, seq), Err(EngineError::TooManyTargets));
     assert!(engine.targets().is_empty(), "{:?}", engine.targets());
 
-    let (doc, seq) = ports_doc(16);
+    let (doc, seq) = ports_doc(MAX_TARGETS);
     engine.load(&doc, seq).unwrap();
-    assert_eq!(engine.targets().len(), 16);
+    assert_eq!(engine.targets().len(), MAX_TARGETS);
     engine.play().unwrap();
     let mut ns = 0;
     blocks(&mut r, 1, 64, &mut ns);
@@ -304,7 +304,7 @@ fn failed_load_leaves_the_target_table_unchanged() {
         .map(|(p, _)| *p)
         .collect();
     sounded.sort();
-    assert_eq!(sounded, (0..16).collect::<Vec<_>>());
+    assert_eq!(sounded, (0..MAX_TARGETS).collect::<Vec<_>>());
 }
 
 #[test]
@@ -345,9 +345,9 @@ fn engine_errors() {
         engine.load(&doc, SeqId(99)),
         Err(EngineError::Model(_))
     ));
-    // The command ring holds 64 commands; nothing drains it here.
+    // The command ring holds 256 commands; nothing drains it here.
     let mut busy = false;
-    for _ in 0..100 {
+    for _ in 0..300 {
         if engine.play() == Err(EngineError::Busy) {
             busy = true;
             break;
VIZDAW_EOF
```

Update `crates/core-engine/tests/graph_mixer.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-engine/tests/graph_mixer.rs b/crates/core-engine/tests/graph_mixer.rs
--- a/crates/core-engine/tests/graph_mixer.rs
+++ b/crates/core-engine/tests/graph_mixer.rs
@@ -1,6 +1,6 @@
 use core_engine::fixtures::{Builder, demo_document};
 use core_engine::graph::mixer::mixer_spec;
-use core_engine::graph::{InputSpec, NodeSpec, StripSpec, Tap};
+use core_engine::graph::{InputSpec, InstrumentFeed, NodeSpec, StripSpec, Tap};
 use core_engine::schedule::TargetIdx;
 use core_model::command::MixerCmd;
 use core_model::document::{DestKind, Document, DocumentRepr, Settings};
@@ -30,7 +30,7 @@ fn zero(_: &PluginSlot) -> u32 {
 #[test]
 fn the_demo_plays_through_the_builtin_strip() {
     let doc = demo_document().unwrap();
-    let g = mixer_spec(&doc, &[(TargetIdx(0), 0)], &mut zero);
+    let g = mixer_spec(&doc, &[InstrumentFeed::builtin(TargetIdx(0), 0)], &mut zero);
     assert!(g.warnings.is_empty(), "{:?}", g.warnings);
     assert_eq!(
         g.spec.nodes[0],
@@ -81,7 +81,11 @@ fn routed() -> Builder {
 #[test]
 fn outputs_sends_and_sidechains_become_edges() {
     let b = routed();
-    let g = mixer_spec(b.doc(), &[(TargetIdx(0), 0)], &mut zero);
+    let g = mixer_spec(
+        b.doc(),
+        &[InstrumentFeed::builtin(TargetIdx(0), 0)],
+        &mut zero,
+    );
     assert_eq!(
         strip(&g.spec, 1).inputs,
         vec![post(2, 1.0), post(3, 1.0), post(4, 1.0)]
@@ -104,7 +108,11 @@ fn mute_and_solo_become_gains() {
         mute: true,
     })
     .unwrap();
-    let g = mixer_spec(b.doc(), &[(TargetIdx(0), 0)], &mut zero);
+    let g = mixer_spec(
+        b.doc(),
+        &[InstrumentFeed::builtin(TargetIdx(0), 0)],
+        &mut zero,
+    );
     assert_eq!(strip(&g.spec, 1).inputs[0], post(2, 0.0));
     assert_eq!(strip(&g.spec, 3).inputs[0].gain, 0.0);
     assert_eq!(strip(&g.spec, 4).inserts[0].sidechain, Some(post(2, 0.0)));
@@ -117,7 +125,11 @@ fn mute_and_solo_become_gains() {
         solo: true,
     })
     .unwrap();
-    let g = mixer_spec(b.doc(), &[(TargetIdx(0), 0)], &mut zero);
+    let g = mixer_spec(
+        b.doc(),
+        &[InstrumentFeed::builtin(TargetIdx(0), 0)],
+        &mut zero,
+    );
     assert_eq!(
         strip(&g.spec, 1).inputs,
         vec![post(2, 0.0), post(3, 0.0), post(4, 1.0)]
@@ -132,7 +144,11 @@ fn mute_and_solo_become_gains() {
         solo: true,
     })
     .unwrap();
-    let g = mixer_spec(b.doc(), &[(TargetIdx(0), 0)], &mut zero);
+    let g = mixer_spec(
+        b.doc(),
+        &[InstrumentFeed::builtin(TargetIdx(0), 0)],
+        &mut zero,
+    );
     assert_eq!(
         strip(&g.spec, 1).inputs,
         vec![post(2, 1.0), post(3, 1.0), post(4, 0.0)]
@@ -165,7 +181,11 @@ fn faders_pans_and_disabled_sends() {
         send: off,
     })
     .unwrap();
-    let g = mixer_spec(b.doc(), &[(TargetIdx(0), 0)], &mut zero);
+    let g = mixer_spec(
+        b.doc(),
+        &[InstrumentFeed::builtin(TargetIdx(0), 0)],
+        &mut zero,
+    );
     let synth = strip(&g.spec, 2);
     assert_eq!(synth.gain_l, 1.0);
     assert!(synth.gain_r.abs() < 1e-7);
@@ -186,7 +206,11 @@ fn insert_latencies_and_bypass_come_from_the_slots() {
     })
     .unwrap();
     let mut lat = |s: &PluginSlot| if s.id == eq { 64 } else { 0 };
-    let g = mixer_spec(b.doc(), &[(TargetIdx(0), 7)], &mut lat);
+    let g = mixer_spec(
+        b.doc(),
+        &[InstrumentFeed::builtin(TargetIdx(0), 7)],
+        &mut lat,
+    );
     assert_eq!(
         g.spec.nodes[0],
         NodeSpec::Instrument {
@@ -216,7 +240,7 @@ fn missing_builtin_strip_and_silent_instrument_strips_are_reported() {
         },
     })
     .unwrap();
-    let g = mixer_spec(&doc, &[(TargetIdx(0), 0)], &mut zero);
+    let g = mixer_spec(&doc, &[InstrumentFeed::builtin(TargetIdx(0), 0)], &mut zero);
     assert_eq!(strip(&g.spec, 1).inputs, vec![post(0, 1.0)]);
     assert!(g.warnings[0].contains("no built-in synth strip"));
 
@@ -226,7 +250,7 @@ fn missing_builtin_strip_and_silent_instrument_strips_are_reported() {
     let g = mixer_spec(b.doc(), &[], &mut zero);
     assert_eq!(
         g.warnings,
-        vec!["strip \"Pad\" has no instrument until plugin hosting exists".to_string()]
+        vec!["strip \"Pad\" has no instrument".to_string()]
     );
 }
 
@@ -247,7 +271,11 @@ fn post_fader_sends_and_bus_outputs_become_post_edges() {
     let mut send = AuxSend::new(bus_b);
     send.level_db = -6.020_6;
     b.mixer(MixerCmd::AddSend { strip: synth, send }).unwrap();
-    let g = mixer_spec(b.doc(), &[(TargetIdx(0), 0)], &mut zero);
+    let g = mixer_spec(
+        b.doc(),
+        &[InstrumentFeed::builtin(TargetIdx(0), 0)],
+        &mut zero,
+    );
     // Master hears A and B, not the synth directly.
     assert_eq!(strip(&g.spec, 1).inputs, vec![post(3, 1.0), post(4, 1.0)]);
     assert_eq!(strip(&g.spec, 3).inputs, vec![post(2, 1.0)]);
@@ -261,7 +289,11 @@ fn post_fader_sends_and_bus_outputs_become_post_edges() {
         mute: true,
     })
     .unwrap();
-    let g = mixer_spec(b.doc(), &[(TargetIdx(0), 0)], &mut zero);
+    let g = mixer_spec(
+        b.doc(),
+        &[InstrumentFeed::builtin(TargetIdx(0), 0)],
+        &mut zero,
+    );
     assert_eq!(strip(&g.spec, 3).inputs, vec![post(2, 0.0)]);
     assert_eq!(strip(&g.spec, 4).inputs[0].gain, 0.0);
 }
VIZDAW_EOF
```

Update `crates/core-engine/tests/offline.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-engine/tests/offline.rs b/crates/core-engine/tests/offline.rs
--- a/crates/core-engine/tests/offline.rs
+++ b/crates/core-engine/tests/offline.rs
@@ -2,8 +2,8 @@ use core_engine::backend::AudioBackend;
 use core_engine::engine::{Engine, EngineConfig, find_sequence};
 use core_engine::fixtures::{BAR, Builder, Q, demo_document};
 use core_engine::offline::{EventLog, LogRecord, OfflineBackend, OfflineSinks};
-use core_engine::render::Faults;
-use core_engine::schedule::{BuiltinRoute, CompileReport};
+use core_engine::render::{Faults, RETIRE_CAPACITY};
+use core_engine::schedule::{BuiltinRoute, CompileReport, MAX_TARGETS};
 use core_engine::transport::PlayState;
 use core_model::document::{DestKind, Document};
 use core_model::event::EventBody;
@@ -295,9 +295,13 @@ fn unroutable_plugin_is_reported_and_absent() {
     let doc = b.finish();
     let mut run = start(&doc, seq, 512);
     assert_eq!(run.report.unroutable.len(), 2);
-    assert_eq!(run.report.warnings, vec![
-        "2 event(s) not played: Pad: plugin instrument \"clap:pad\" cannot play until plugin hosting exists".to_string()
-    ]);
+    assert_eq!(
+        run.report.warnings,
+        vec![
+            "2 event(s) not played: Pad: clap:pad cannot load until plugin hosting exists"
+                .to_string()
+        ]
+    );
     insta::assert_snapshot!(run.play_to_end(), @r"
 0 t0 90 3c 40
 24000 t0 80 3c 00
@@ -436,7 +440,8 @@ fn swap_waits_while_the_retire_ring_is_full() {
     let mut run = start(&doc, seq, 64);
     run.engine.pause_collector(true);
     run.backend.run_blocks([64]);
-    for _ in 0..8 {
+    // Each load retires a schedule and a graph.
+    for _ in 0..RETIRE_CAPACITY / 2 {
         run.engine.load(&doc, seq).unwrap();
         run.backend.run_blocks([64]);
     }
@@ -653,7 +658,7 @@ fn load_while_default_loop_is_on_follows_the_new_length() {
 fn sinks_get_the_engine_target_index_after_a_failed_load() {
     use core_engine::EngineError;
     use core_engine::backend::MidiPortInfo;
-    let ports: Vec<MidiPortInfo> = (0..17)
+    let ports: Vec<MidiPortInfo> = (0..=MAX_TARGETS)
         .map(|i| MidiPortInfo {
             name: format!("Port {i}"),
             unique_id: None,
@@ -661,7 +666,7 @@ fn sinks_get_the_engine_target_index_after_a_failed_load() {
         .collect();
     let mut b = Builder::new();
     let (many, _) = b.sequence("Many", "L", None).unwrap();
-    for i in 0..17 {
+    for i in 0..=MAX_TARGETS {
         let name = format!("Port {i}");
         let d = b.dest(&name, &name, 0).unwrap();
         let l = b.lane(many, &name, Some(d)).unwrap();
@@ -675,7 +680,7 @@ fn sinks_get_the_engine_target_index_after_a_failed_load() {
     let sinks = OfflineSinks::new(100).with_ports(ports);
     let log = sinks.log();
     let (mut engine, renderer) = Engine::new(EngineConfig::new(SR), Box::new(sinks)).unwrap();
-    // Fails after 16 sinks were made; the engine forgets those targets.
+    // Fails after 64 sinks were made; the engine forgets those targets.
     assert_eq!(engine.load(&doc, many), Err(EngineError::TooManyTargets));
     engine.load(&doc, one).unwrap();
     let mut backend = OfflineBackend::new(SR, 512);
VIZDAW_EOF
```

Update `crates/core-engine/tests/rt_alloc.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-engine/tests/rt_alloc.rs b/crates/core-engine/tests/rt_alloc.rs
--- a/crates/core-engine/tests/rt_alloc.rs
+++ b/crates/core-engine/tests/rt_alloc.rs
@@ -7,7 +7,7 @@ use core_engine::engine::{Engine, EngineConfig, find_sequence};
 use core_engine::error::EngineError;
 use core_engine::fixtures::demo_document;
 use core_engine::offline::{OfflineBackend, OfflineSinks};
-use core_engine::render::Faults;
+use core_engine::render::{Faults, RETIRE_CAPACITY};
 use core_engine::rt_check::{self, CountingAlloc};
 use core_engine::schedule::BuiltinRoute;
 use core_engine::transport::PlayState;
@@ -110,10 +110,11 @@ fn render_path_never_allocates() {
     assert_eq!(ended, 1, "the end was never reported");
 
     // 100 schedule swaps; with the collector paused the retire ring
-    // (capacity 8) fills and the ninth swap is deferred.
+    // (`RETIRE_CAPACITY`, two entries per load) fills and a swap is
+    // deferred.
     engine.pause_collector(true);
     for i in 0..100 {
-        if i == 12 {
+        if i == RETIRE_CAPACITY / 2 + 4 {
             engine.pause_collector(false);
         }
         // Once the collector resumes it frees the retire ring on its own
VIZDAW_EOF
```

Update `crates/core-engine/tests/rt_alloc_graph.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-engine/tests/rt_alloc_graph.rs b/crates/core-engine/tests/rt_alloc_graph.rs
--- a/crates/core-engine/tests/rt_alloc_graph.rs
+++ b/crates/core-engine/tests/rt_alloc_graph.rs
@@ -7,7 +7,7 @@ use core_engine::engine::{Engine, EngineConfig, find_sequence};
 use core_engine::fixtures::{Builder, demo_document};
 use core_engine::graph::TestInserts;
 use core_engine::offline::{OfflineBackend, OfflineSinks};
-use core_engine::render::Faults;
+use core_engine::render::{Faults, RETIRE_CAPACITY};
 use core_engine::rt_check::{self, CountingAlloc};
 use core_engine::schedule::BuiltinRoute;
 use core_model::command::MixerCmd;
@@ -166,11 +166,10 @@ fn graph_render_path_never_allocates() {
 
     // Phase 2: graph swaps with the retire ring full (deferred swaps),
     // value-only and structural. Each load queues three commands and each
-    // applied swap retires two items, so the 16-slot retire ring is full
-    // after eight blocks; resuming at block 10 keeps the 64-slot command
-    // ring from refusing a load, and stopping at 20 keeps the loop from
-    // outrunning the collector (it frees every 10 ms; an optimised loop
-    // makes 20 edits in far less).
+    // applied swap retires two items, so the retire ring is full after
+    // `RETIRE_CAPACITY / 2` blocks; resuming two blocks later keeps the
+    // command ring from refusing a load, and stopping ten blocks after that
+    // keeps the loop from outrunning the collector (it frees every 10 ms).
     let mut cfg = EngineConfig::new(48_000.0);
     cfg.workers = 3;
     cfg.route = BuiltinRoute::Both;
@@ -190,8 +189,11 @@ fn graph_render_path_never_allocates() {
     let before = rt_check::count();
     let applied = engine.graphs_applied();
     engine.pause_collector(true);
-    for i in 0..20 {
-        if i == 10 {
+    // Two retire entries per load: the ring fills after
+    // `RETIRE_CAPACITY / 2` loads.
+    let fill = RETIRE_CAPACITY / 2;
+    for i in 0..fill + 10 {
+        if i == fill + 2 {
             engine.pause_collector(false);
             std::thread::sleep(std::time::Duration::from_millis(50));
         }
@@ -206,7 +208,7 @@ fn graph_render_path_never_allocates() {
     );
     // Let the deferred swaps land, then check they all did.
     let drain = std::time::Instant::now();
-    while engine.graphs_applied() - applied < 20 && drain.elapsed().as_secs() < 5 {
+    while engine.graphs_applied() - applied < (fill + 10) as u64 && drain.elapsed().as_secs() < 5 {
         armed_block(&mut b, 256);
         std::thread::sleep(std::time::Duration::from_millis(1));
     }
@@ -215,7 +217,11 @@ fn graph_render_path_never_allocates() {
         0,
         "allocations draining deferred swaps"
     );
-    assert_eq!(engine.graphs_applied() - applied, 20, "swaps applied");
+    assert_eq!(
+        engine.graphs_applied() - applied,
+        (fill + 10) as u64,
+        "swaps applied"
+    );
     log.drain();
     drop(b.stop().unwrap());
     drop(engine);
VIZDAW_EOF
```

Update `crates/core-engine/tests/schedule.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-engine/tests/schedule.rs b/crates/core-engine/tests/schedule.rs
--- a/crates/core-engine/tests/schedule.rs
+++ b/crates/core-engine/tests/schedule.rs
@@ -3,8 +3,8 @@ use core_engine::backend::MidiPortInfo;
 use core_engine::fixtures::{BAR, Builder, Q};
 use core_engine::midi1::Midi1Msg;
 use core_engine::schedule::{
-    BuiltinRoute, CompileOptions, CompileReport, Entry, Schedule, TargetIdx, TargetKind,
-    TargetTable, WireMsg, compile,
+    BuiltinRoute, CompileOptions, CompileReport, Entry, MAX_TARGETS, Schedule, TargetIdx,
+    TargetKind, TargetTable, WireMsg, compile,
 };
 use core_model::command::{Command, SequenceCmd};
 use core_model::document::{DestKind, Document, Patch};
@@ -276,24 +276,25 @@ fn unroutable_events_are_reported_not_played() {
     let ok_lane = b.lane(seq, "Ok", Some(synth)).unwrap();
     b.notes(seq, ok_lane, &[(0, 67, 0x8000, Q)]).unwrap();
     let doc = b.finish();
-    let (s, r, _) = compile_all(&doc, seq, &CompileOptions::new(48_000.0));
-    assert_eq!(r.events, 1);
-    assert_eq!(s.entries.len(), 2);
-    assert_eq!(r.unroutable.len(), 3);
-    let reasons: Vec<&str> = r.unroutable.iter().map(|u| u.reason.as_str()).collect();
+    let (s, r, table) = compile_all(&doc, seq, &CompileOptions::new(48_000.0));
+    // The plugin instrument is a target of its own (its sink comes from
+    // the plugin factory, which this resolver stands in for).
+    assert_eq!(r.events, 2);
+    assert_eq!(s.entries.len(), 4);
     assert!(
-        reasons
+        table
             .iter()
-            .any(|w| w.contains("Pad") && w.contains("au:pad")),
-        "{reasons:?}"
+            .any(|(_, k)| matches!(k, TargetKind::PluginInstrument(_)))
     );
+    assert_eq!(r.unroutable.len(), 2);
+    let reasons: Vec<&str> = r.unroutable.iter().map(|u| u.reason.as_str()).collect();
     assert!(
         reasons.iter().any(|w| w.contains("Missing Synth")),
         "{reasons:?}"
     );
     assert!(reasons.contains(&"no destination"), "{reasons:?}");
     assert!(r.unroutable.iter().all(|u| u.path.len() == 1));
-    assert_eq!(r.warnings.len(), 3, "{:?}", r.warnings);
+    assert_eq!(r.warnings.len(), 2, "{:?}", r.warnings);
 }
 
 #[test]
@@ -430,7 +431,7 @@ fn too_many_targets_is_an_error() {
     let mut b = Builder::new();
     let (seq, _) = b.sequence("S", "L", None).unwrap();
     let mut ports = Vec::new();
-    for i in 0..17 {
+    for i in 0..=MAX_TARGETS {
         let name = format!("Port {i}");
         let d = b.dest(&name, &name, 0).unwrap();
         let l = b.lane(seq, &name, Some(d)).unwrap();
VIZDAW_EOF
```

Update `crates/core-engine/tests/tempo_table.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-engine/tests/tempo_table.rs b/crates/core-engine/tests/tempo_table.rs
--- a/crates/core-engine/tests/tempo_table.rs
+++ b/crates/core-engine/tests/tempo_table.rs
@@ -2,9 +2,10 @@ use core_engine::EngineError;
 use core_engine::backend::MidiPortInfo;
 use core_engine::schedule::TempoTable;
 use core_engine::schedule::targets::{
-    BuiltinRoute, TargetIdx, TargetKind, TargetTable, find_port, kinds_for,
+    BuiltinRoute, MAX_TARGETS, TargetIdx, TargetKind, TargetTable, find_port, kinds_for,
 };
 use core_model::document::DestKind;
+use core_model::ids::PluginInstanceId;
 use core_model::tempo::{Bpm, TempoMap, TempoPoint};
 use core_model::time::Tick;
 use proptest::prelude::*;
@@ -48,7 +49,7 @@ fn ramp_knots_are_at_most_a_sixteenth_apart() {
 }
 
 #[test]
-fn target_table_interns_and_caps_at_16() {
+fn target_table_interns_and_caps_at_max_targets() {
     let mut t = TargetTable::new();
     assert_eq!(t.intern(TargetKind::BuiltinSynth).unwrap(), TargetIdx(0));
     assert_eq!(
@@ -56,14 +57,14 @@ fn target_table_interns_and_caps_at_16() {
         TargetIdx(1)
     );
     assert_eq!(t.intern(TargetKind::BuiltinSynth).unwrap(), TargetIdx(0));
-    for i in 0..14 {
+    for i in 0..MAX_TARGETS - 2 {
         let p = MidiPortInfo {
             name: format!("P{i}"),
             unique_id: None,
         };
         t.intern(TargetKind::MidiDestination(p)).unwrap();
     }
-    assert_eq!(t.len(), 16);
+    assert_eq!(t.len(), MAX_TARGETS);
     let p = MidiPortInfo {
         name: "one too many".into(),
         unique_id: None,
@@ -73,7 +74,17 @@ fn target_table_interns_and_caps_at_16() {
         Err(EngineError::TooManyTargets)
     );
     assert_eq!(t.get(TargetIdx(1)), Some(&TargetKind::MidiVirtualSource));
-    assert_eq!(t.iter().count(), 16);
+    assert_eq!(t.iter().count(), MAX_TARGETS);
+    // A released index is reused only after `recycle` (the next load).
+    t.release(TargetIdx(1));
+    assert_eq!(t.get(TargetIdx(1)), None);
+    assert_eq!(t.next_index(), None, "released, not free");
+    t.recycle();
+    assert_eq!(t.next_index(), Some(TargetIdx(1)));
+    assert_eq!(
+        t.intern(TargetKind::PluginInstrument(PluginInstanceId(7))),
+        Ok(TargetIdx(1))
+    );
 }
 
 #[test]
@@ -122,10 +133,9 @@ fn endpoint_refs_map_to_target_kinds() {
             .contains("Nope")
     );
     let plugin = DestKind::plugin("au:x");
-    assert!(
-        kinds_for(&plugin, BuiltinRoute::Builtin, &ports)
-            .unwrap_err()
-            .contains("au:x")
+    assert_eq!(
+        kinds_for(&plugin, BuiltinRoute::Builtin, &ports),
+        Ok(vec![TargetKind::PluginInstrument(PluginInstanceId(0))])
     );
     assert_eq!(find_port("11", &ports), Some(&ports[0]));
     assert_eq!(find_port("-5", &ports), Some(&ports[1]));
VIZDAW_EOF
```

- [ ] **Step 2: Run them to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-engine --test plugin_instruments --test tempo_table --test schedule --test engine --test offline --test graph_mixer --test rt_alloc --test rt_alloc_graph
```

Expected: compile errors, e.g. `unresolved import `core_engine::graph::TestInstrument``, `no method named `plugin_dest``, `cannot find `InstrumentFeed``.

- [ ] **Step 3: Implement**


Change `crates/core-engine/src/engine.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-engine/src/engine.rs b/crates/core-engine/src/engine.rs
--- a/crates/core-engine/src/engine.rs
+++ b/crates/core-engine/src/engine.rs
@@ -5,7 +5,8 @@ use crate::backend::MidiPortInfo;
 use crate::error::{BackendError, EngineError};
 use crate::graph::mixer::mixer_spec;
 use crate::graph::{
-    EffectNode, Graph, GraphReport, InsertFactory, InsertInit, NoInserts, NodeSpec, PlanError, plan,
+    EffectNode, Graph, GraphReport, InsertFactory, InsertInit, InstrumentFeed, NoInserts, NodeSpec,
+    PlanError, plan,
 };
 use crate::pool::{Pool, RtEnv, RtPlatform, StdPlatform};
 use crate::render::{
@@ -17,12 +18,12 @@ use crate::schedule::{
     TempoTable, compile,
 };
 use crate::transport::PlayState;
-use core_model::document::Document;
+use core_model::document::{DestKind, Document};
 use core_model::ids::{PluginInstanceId, SeqId, StripId};
 use core_model::mixer::{PluginFormat, PluginSlot};
 use core_model::time::Tick;
 use rtrb::{Producer, RingBuffer};
-use std::collections::BTreeMap;
+use std::collections::{BTreeMap, BTreeSet};
 use std::sync::Arc;
 use std::sync::atomic::{AtomicBool, Ordering};
 use std::thread::JoinHandle;
@@ -96,14 +97,29 @@ struct Live {
     latency: u32,
     format: PluginFormat,
     plugin: String,
+    /// The block size it was made for.
+    max_frames: u32,
 }
 
 impl Live {
-    fn serves(&self, slot: &PluginSlot) -> bool {
-        self.format == slot.plugin.format && self.plugin == slot.plugin.id
+    /// True when this instance may keep serving `slot` with blocks of up
+    /// to `max_frames`.
+    fn serves(&self, slot: &PluginSlot, max_frames: u32) -> bool {
+        self.format == slot.plugin.format
+            && self.plugin == slot.plugin.id
+            && self.max_frames >= max_frames
     }
 }
 
+/// What the engine knows about a target's sink.
+#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
+struct SinkInfo {
+    /// Instrument latency (`None`: not an instrument sink).
+    latency: Option<u32>,
+    /// The block size the sink was made for.
+    max_frames: u32,
+}
+
 /// The control handle.
 pub struct Engine {
     cfg: EngineConfig,
@@ -117,8 +133,10 @@ pub struct Engine {
     loop_range: Option<(u64, u64)>,
     /// The default loop (bar 1 to the end) is on: loads re-send it.
     default_loop: bool,
-    /// Latency of each target's instrument sink.
-    instrument_latency: [Option<u32>; MAX_TARGETS],
+    /// Each target's sink.
+    sinks: [SinkInfo; MAX_TARGETS],
+    /// Instances to recreate at the next load (`Engine::restart_instance`).
+    restart: BTreeSet<PluginInstanceId>,
     /// Insert instances in the last published graph, by ID.
     live: BTreeMap<PluginInstanceId, Live>,
     /// Meter cell of every strip of the last published graph, by strip
@@ -233,7 +251,8 @@ impl Engine {
             length_frames: 0,
             loop_range: None,
             default_loop: false,
-            instrument_latency: [None; MAX_TARGETS],
+            sinks: [SinkInfo::default(); MAX_TARGETS],
+            restart: BTreeSet::new(),
             live: BTreeMap::new(),
             meter_slots: BTreeMap::new(),
             latency: 0,
@@ -298,9 +317,10 @@ impl Engine {
     /// Compiles `seq` and the mixer graph and sends them (and sinks for new
     /// targets) to the renderer. Call again after every committed edit.
     pub fn load(&mut self, doc: &Document, seq: SeqId) -> Result<CompileReport, EngineError> {
-        // Room for every possible new sink plus the graph, the schedule and
-        // the loop, so no push below can fail after targets were interned.
-        if self.cmds.slots() < MAX_TARGETS + 3 {
+        // Room for removing and adding every possible sink plus the graph,
+        // the schedule and the loop, so no push below can fail after
+        // targets were interned.
+        if self.cmds.slots() < 2 * MAX_TARGETS + 3 {
             return Err(EngineError::Busy);
         }
         let opts = CompileOptions {
@@ -309,26 +329,33 @@ impl Engine {
             ports: self.factory.ports(),
         };
         let (sr, max) = (self.cfg.sample_rate, self.block_frames());
-        let before = self.targets.len();
+        self.targets.recycle();
+        self.release_stale_targets(doc, max)?;
         let mut new_sinks: Vec<(TargetIdx, Sink)> = Vec::new();
         let targets = &mut self.targets;
         let factory = &mut self.factory;
+        let inserts = &mut self.inserts;
         let mut resolve = |kind: &TargetKind| -> Result<Result<TargetIdx, String>, EngineError> {
             if let Some(i) = targets.find(kind) {
                 return Ok(Ok(i));
             }
-            if targets.len() >= MAX_TARGETS {
-                return Err(EngineError::TooManyTargets);
-            }
-            // `intern` appends, so the new target gets the next index.
-            let next = u8::try_from(targets.len()).map_err(|_| EngineError::TooManyTargets)?;
-            match factory.make(TargetIdx(next), kind, sr, max) {
+            let next = targets.next_index().ok_or(EngineError::TooManyTargets)?;
+            let made = match kind {
+                TargetKind::PluginInstrument(id) => match doc.plugin_slot(*id) {
+                    Some(slot) => inserts.make_instrument(slot, sr, max).map(Sink::Instrument),
+                    None => Err(format!("plugin instance {id} has no slot")),
+                },
+                other => factory
+                    .make(next, other, sr, max)
+                    .map_err(|e| e.to_string()),
+            };
+            match made {
                 Ok(sink) => {
                     let i = targets.intern(kind.clone())?;
                     new_sinks.push((i, sink));
                     Ok(Ok(i))
                 }
-                Err(e) => Ok(Err(e.to_string())),
+                Err(e) => Ok(Err(e)),
             }
         };
         let compiled = compile(doc, seq, &opts, &mut resolve);
@@ -337,19 +364,26 @@ impl Engine {
             Err(e) => {
                 // Targets interned by this load have no sinks yet; forget
                 // them (their sinks drop with `new_sinks`).
-                self.targets.truncate(before);
+                for (i, _) in &new_sinks {
+                    self.targets.forget(*i);
+                }
                 return Err(e);
             }
         };
         for (i, sink) in new_sinks {
-            if let (Sink::Instrument(node), Some(l)) =
-                (&sink, self.instrument_latency.get_mut(usize::from(i.0)))
-            {
-                *l = Some(node.latency());
+            if let Some(info) = self.sinks.get_mut(usize::from(i.0)) {
+                *info = SinkInfo {
+                    latency: match &sink {
+                        Sink::Instrument(node) => Some(node.latency()),
+                        Sink::Midi(_) => None,
+                    },
+                    max_frames: max,
+                };
             }
             self.push(EngineCmd::AddSink(i, sink))?;
         }
         report.graph = self.compile_graph(doc)?;
+        self.restart.clear();
         self.tempo = Some(schedule.tempo.clone());
         self.length_frames = schedule.length_frames;
         self.loop_range = schedule.loop_range;
@@ -362,6 +396,49 @@ impl Engine {
         Ok(report)
     }
 
+    /// Releases targets whose sink must go before this load: plugin
+    /// instruments no longer in `doc` or marked for restart, and instrument
+    /// sinks made for blocks smaller than `max` (a later
+    /// `Renderer::prepare` grew the block size; Plan Ruling 5).
+    fn release_stale_targets(&mut self, doc: &Document, max: u32) -> Result<(), EngineError> {
+        let stale: Vec<TargetIdx> = self
+            .targets
+            .iter()
+            .filter(|(i, kind)| {
+                let info = self.sinks.get(usize::from(i.0)).copied().unwrap_or_default();
+                let small = info.latency.is_some() && info.max_frames < max;
+                match kind {
+                    TargetKind::PluginInstrument(id) => {
+                        small
+                            || self.restart.contains(id)
+                            || !doc.instruments().values().any(|d| {
+                                matches!(&d.kind, DestKind::PluginInstrument { slot } if slot.id == *id)
+                            })
+                    }
+                    TargetKind::BuiltinSynth => small,
+                    TargetKind::MidiVirtualSource | TargetKind::MidiDestination(_) => false,
+                }
+            })
+            .map(|(i, _)| i)
+            .collect();
+        for i in stale {
+            self.push(EngineCmd::RemoveSink(i))?;
+            self.targets.release(i);
+            if let Some(info) = self.sinks.get_mut(usize::from(i.0)) {
+                *info = SinkInfo::default();
+            }
+        }
+        Ok(())
+    }
+
+    /// Makes the next load recreate instance `id` (an insert or a plugin
+    /// instrument) instead of keeping the running one: its old processor
+    /// is retired with the graph or sink it lived in. The plugin host calls
+    /// this when a plugin asks to be restarted (a latency change).
+    pub fn restart_instance(&mut self, id: PluginInstanceId) {
+        self.restart.insert(id);
+    }
+
     /// Meter cells for `strips`: a strip keeps its cell; a new strip gets
     /// the lowest free one, cleared first (Plan Ruling 13).
     fn assign_meters(&self, strips: &[(usize, StripId)]) -> BTreeMap<StripId, usize> {
@@ -394,26 +471,34 @@ impl Engine {
     /// playing (spec §6).
     fn compile_graph(&mut self, doc: &Document) -> Result<GraphReport, EngineError> {
         let (sr, max) = (self.cfg.sample_rate, self.block_frames());
-        let instruments: Vec<(TargetIdx, u32)> = self
+        let instruments: Vec<InstrumentFeed> = self
             .targets
             .iter()
-            .filter(|(_, k)| **k == TargetKind::BuiltinSynth)
-            .filter_map(|(i, _)| {
-                let lat = self
-                    .instrument_latency
-                    .get(usize::from(i.0))
-                    .copied()
-                    .flatten()?;
-                Some((i, lat))
+            .filter_map(|(i, k)| {
+                let lat = self.sinks.get(usize::from(i.0))?.latency?;
+                match k {
+                    TargetKind::BuiltinSynth => Some(InstrumentFeed::builtin(i, lat)),
+                    TargetKind::PluginInstrument(id) => {
+                        let dest = doc.instruments().values().find(|d| {
+                            matches!(&d.kind, DestKind::PluginInstrument { slot } if slot.id == *id)
+                        })?;
+                        Some(InstrumentFeed::plugin(i, lat, dest.id))
+                    }
+                    _ => None,
+                }
             })
             .collect();
         let mut fresh: BTreeMap<PluginInstanceId, (Box<dyn EffectNode>, Live)> = BTreeMap::new();
         let mut adopted: BTreeMap<PluginInstanceId, Live> = BTreeMap::new();
         let mut placeholders: Vec<(PluginInstanceId, String)> = Vec::new();
         let live = &self.live;
+        let restart = &self.restart;
         let inserts = &mut self.inserts;
         let mut latency = |slot: &PluginSlot| -> u32 {
-            if let Some(l) = live.get(&slot.id).filter(|l| l.serves(slot)) {
+            if let Some(l) = live
+                .get(&slot.id)
+                .filter(|l| l.serves(slot, max) && !restart.contains(&slot.id))
+            {
                 adopted.insert(slot.id, l.clone());
                 return l.latency;
             }
@@ -425,6 +510,7 @@ impl Engine {
                         latency: lat,
                         format: slot.plugin.format,
                         plugin: slot.plugin.id.clone(),
+                        max_frames: max,
                     };
                     fresh.insert(slot.id, (node, info));
                     lat
@@ -499,8 +585,9 @@ impl Engine {
 
     /// The block size graphs, inserts and sinks are built for: the
     /// configured size, or the renderer's prepared size when a host grew it
-    /// (`Renderer::prepare`).
-    fn block_frames(&self) -> u32 {
+    /// (`Renderer::prepare`). Plugin hosts activate instances for this
+    /// size.
+    pub fn block_frames(&self) -> u32 {
         self.cfg.max_frames.max(self.shared.prepared_frames())
     }
 
VIZDAW_EOF
```

Change `crates/core-engine/src/error.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-engine/src/error.rs b/crates/core-engine/src/error.rs
--- a/crates/core-engine/src/error.rs
+++ b/crates/core-engine/src/error.rs
@@ -26,6 +26,10 @@ pub enum BackendError {
     /// The built-in synth's audio component is not installed.
     #[error("audio component missing")]
     ComponentMissing,
+    /// This factory does not make sinks of this kind (plugin instruments
+    /// come from the plugin factory).
+    #[error("{0} is not made by this sink factory")]
+    NotSupported(String),
 }
 
 /// Failures of the non-real-time engine API.
@@ -43,8 +47,8 @@ pub enum EngineError {
     /// The command ring to the audio thread is full; retry later.
     #[error("engine command queue is full")]
     Busy,
-    /// The document needs more than 16 playback targets.
-    #[error("more than 16 playback targets")]
+    /// The document needs more than `MAX_TARGETS` (64) playback targets.
+    #[error("more than 64 playback targets")]
     TooManyTargets,
     /// A helper thread could not be started.
     #[error("cannot start thread {name:?}: {reason}")]
VIZDAW_EOF
```

Change `crates/core-engine/src/fixtures.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-engine/src/fixtures.rs b/crates/core-engine/src/fixtures.rs
--- a/crates/core-engine/src/fixtures.rs
+++ b/crates/core-engine/src/fixtures.rs
@@ -8,7 +8,7 @@ use core_model::command::{
 use core_model::document::{DestKind, Document, Patch};
 use core_model::event::{EventBody, EventSpec, LoopCount};
 use core_model::ids::{AnyId, DestId, EventId, LaneId, PluginInstanceId, SeqId, StripId};
-use core_model::mixer::{PluginRef, StripKind};
+use core_model::mixer::{PluginRef, PluginSlot, StripKind};
 use core_model::tempo::{Bpm, TempoMap, TempoPoint};
 use core_model::time::{PPQ, Tick};
 use core_model::{ModelError, Result};
@@ -115,6 +115,27 @@ impl Builder {
         })
     }
 
+    /// Adds a plugin instrument destination playing `plugin`; returns the
+    /// destination and its instance.
+    pub fn plugin_dest(
+        &mut self,
+        name: &str,
+        plugin: PluginRef,
+    ) -> Result<(DestId, PluginInstanceId)> {
+        let d = self.dest_kind(
+            name,
+            DestKind::PluginInstrument {
+                slot: PluginSlot::new(PluginInstanceId(0), plugin),
+            },
+            None,
+        )?;
+        let slot = match self.doc.instruments().get(&d).map(|x| &x.kind) {
+            Some(DestKind::PluginInstrument { slot }) => slot.id,
+            _ => return Err(ModelError::invalid("not a plugin instrument")),
+        };
+        Ok((d, slot))
+    }
+
     /// Adds a MIDI-endpoint destination on group 0.
     pub fn dest(&mut self, name: &str, endpoint_ref: &str, channel: u8) -> Result<DestId> {
         self.dest_kind(
VIZDAW_EOF
```

Change `crates/core-engine/src/graph/exec.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-engine/src/graph/exec.rs b/crates/core-engine/src/graph/exec.rs
--- a/crates/core-engine/src/graph/exec.rs
+++ b/crates/core-engine/src/graph/exec.rs
@@ -205,7 +205,7 @@ pub struct Graph {
     latency: u64,
     shape: u64,
     max_frames: u32,
-    instruments: u16,
+    instruments: u64,
     /// `(new index, old index, bypass line for the new spec latency)`.
     adopt: Box<[(usize, usize, DelayLine)]>,
     nodes: Box<[UnsafeCell<NodeState>]>,
@@ -288,7 +288,7 @@ impl Graph {
         let mut nodes = Vec::with_capacity(n);
         let mut inserts = Vec::new();
         let mut adopt = Vec::new();
-        let mut instruments = 0u16;
+        let mut instruments = 0u64;
         let mut succ: Vec<Vec<usize>> = vec![Vec::new(); n];
         let mut indegree = vec![0u32; n];
         let mut edge = |from: usize, to: usize, succ: &mut Vec<Vec<usize>>| {
@@ -305,7 +305,7 @@ impl Graph {
                     // Only the first node of a target renders its sink (the
                     // module protocol needs one user per sink); a repeat, or
                     // a target beyond the table, renders silence.
-                    let bit = 1u16.checked_shl(u32::from(target.0)).unwrap_or(0);
+                    let bit = 1u64.checked_shl(u32::from(target.0)).unwrap_or(0);
                     let target = if bit != 0 && instruments & bit == 0 {
                         instruments |= bit;
                         usize::from(target.0)
@@ -438,7 +438,7 @@ impl Graph {
     }
 
     /// Bitmask of the targets that have an instrument node.
-    pub fn instruments(&self) -> u16 {
+    pub fn instruments(&self) -> u64 {
         self.instruments
     }
 
VIZDAW_EOF
```

Change `crates/core-engine/src/graph/mixer.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-engine/src/graph/mixer.rs b/crates/core-engine/src/graph/mixer.rs
--- a/crates/core-engine/src/graph/mixer.rs
+++ b/crates/core-engine/src/graph/mixer.rs
@@ -4,7 +4,7 @@ use crate::graph::dsp::strip_gains;
 use crate::graph::spec::{GraphSpec, InputSpec, InsertSpec, NodeSpec, StripSpec, Tap};
 use crate::schedule::TargetIdx;
 use core_model::document::Document;
-use core_model::ids::StripId;
+use core_model::ids::{DestId, StripId};
 use core_model::mixer::{Output, PluginSlot, Strip, StripKind, db_to_gain};
 use core_model::mixer_rules::audible;
 use std::collections::BTreeMap;
@@ -20,12 +20,53 @@ pub struct MixerGraph {
     pub warnings: Vec<String>,
 }
 
-/// Builds the graph of `doc`'s mixer. `instruments` are the built-in synth
-/// targets that have an instrument sink, with their latency;
-/// `insert_latency` gives each insert's latency (0 for placeholders).
+/// Which strip an instrument node feeds.
+#[derive(Debug, Clone, Copy, PartialEq, Eq)]
+pub enum FeedSource {
+    /// The built-in synth: its `BuiltinSynth` strip.
+    Builtin,
+    /// A plugin instrument destination: its `Instrument` strip.
+    Plugin(DestId),
+}
+
+/// An instrument node of the graph: a target with an instrument sink.
+#[derive(Debug, Clone, Copy, PartialEq, Eq)]
+pub struct InstrumentFeed {
+    /// The engine target whose sink renders.
+    pub target: TargetIdx,
+    /// The sink's latency in frames.
+    pub latency: u32,
+    /// The strip it feeds.
+    pub source: FeedSource,
+}
+
+impl InstrumentFeed {
+    /// A built-in synth feed.
+    pub fn builtin(target: TargetIdx, latency: u32) -> InstrumentFeed {
+        InstrumentFeed {
+            target,
+            latency,
+            source: FeedSource::Builtin,
+        }
+    }
+
+    /// A plugin instrument feed for destination `dest`.
+    pub fn plugin(target: TargetIdx, latency: u32, dest: DestId) -> InstrumentFeed {
+        InstrumentFeed {
+            target,
+            latency,
+            source: FeedSource::Plugin(dest),
+        }
+    }
+}
+
+/// Builds the graph of `doc`'s mixer. `instruments` are the targets with an
+/// instrument sink (instrument nodes `0..instruments.len()`, in this
+/// order); `insert_latency` gives each insert's latency (0 for
+/// placeholders).
 pub fn mixer_spec(
     doc: &Document,
-    instruments: &[(TargetIdx, u32)],
+    instruments: &[InstrumentFeed],
     insert_latency: &mut dyn FnMut(&PluginSlot) -> u32,
 ) -> MixerGraph {
     let m = doc.mixer();
@@ -45,14 +86,30 @@ pub fn mixer_spec(
         .map(|s| StripSpec::new(Some(s.id)))
         .collect();
     let master = node(m.master()).unwrap_or(base);
-    let feed = m.find_kind(StripKind::BuiltinSynth).and_then(node);
-    if feed.is_none() && !instruments.is_empty() {
-        warnings.push(
-            "no built-in synth strip: the built-in synth plays straight to master".to_string(),
-        );
-    }
-    let to = feed.unwrap_or(master);
-    for (i, _) in instruments.iter().enumerate() {
+    let builtin_strip = m.find_kind(StripKind::BuiltinSynth).and_then(node);
+    let mut fed: Vec<usize> = Vec::new();
+    for (i, f) in instruments.iter().enumerate() {
+        let to = match f.source {
+            FeedSource::Builtin => {
+                if builtin_strip.is_none() {
+                    warnings.push(
+                        "no built-in synth strip: the built-in synth plays straight to master"
+                            .to_string(),
+                    );
+                }
+                builtin_strip.unwrap_or(master)
+            }
+            FeedSource::Plugin(d) => match m.find_kind(StripKind::Instrument(d)).and_then(node) {
+                Some(n) => n,
+                None => {
+                    warnings.push(format!(
+                        "instrument {d} has no strip: it plays straight to master"
+                    ));
+                    master
+                }
+            },
+        };
+        fed.push(to);
         if let Some(s) = to.checked_sub(base).and_then(|k| strips.get_mut(k)) {
             s.inputs.push(InputSpec {
                 from: i,
@@ -126,14 +183,9 @@ pub fn mixer_spec(
                 sidechain,
             });
         }
-        if matches!(
-            s.kind,
-            StripKind::Instrument(_) | StripKind::InstrumentOutput(..)
-        ) {
-            warnings.push(format!(
-                "strip {:?} has no instrument until plugin hosting exists",
-                s.name
-            ));
+        let has_feed = node(s.id).is_some_and(|n| fed.contains(&n));
+        if matches!(s.kind, StripKind::Instrument(_)) && !has_feed {
+            warnings.push(format!("strip {:?} has no instrument", s.name));
         }
     }
     let strip_nodes = m
@@ -144,9 +196,9 @@ pub fn mixer_spec(
         .collect();
     let mut nodes: Vec<NodeSpec> = instruments
         .iter()
-        .map(|(t, l)| NodeSpec::Instrument {
-            target: *t,
-            latency: *l,
+        .map(|f| NodeSpec::Instrument {
+            target: f.target,
+            latency: f.latency,
         })
         .collect();
     nodes.extend(strips.into_iter().map(NodeSpec::Strip));
VIZDAW_EOF
```

Change `crates/core-engine/src/graph/mod.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-engine/src/graph/mod.rs b/crates/core-engine/src/graph/mod.rs
--- a/crates/core-engine/src/graph/mod.rs
+++ b/crates/core-engine/src/graph/mod.rs
@@ -14,8 +14,10 @@ pub mod spec;
 use core_model::ids::PluginInstanceId;
 
 pub use exec::{Graph, InsertInit, Runner, SinkTable};
+pub use mixer::{FeedSource, InstrumentFeed};
 pub use node::{
-    EffectNode, InsertFactory, NoInserts, ProcessStatus, TestInserts, TestMode, TestNode,
+    EffectNode, InsertFactory, NoInserts, ProcessStatus, TestInserts, TestInstrument, TestMode,
+    TestNode,
 };
 pub use pdc::{MAX_INSERTS, MAX_NODES, Plan, PlanError, plan};
 pub use spec::{GraphSpec, InputSpec, InsertSpec, NodeSpec, StripSpec, Tap};
VIZDAW_EOF
```

Change `crates/core-engine/src/graph/node.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-engine/src/graph/node.rs b/crates/core-engine/src/graph/node.rs
--- a/crates/core-engine/src/graph/node.rs
+++ b/crates/core-engine/src/graph/node.rs
@@ -1,7 +1,7 @@
 //! The insert seam: `EffectNode` processors made by an `InsertFactory`
 //! (Plan Ruling 12). 3a ships only test nodes; 3b wraps plugin instances.
 
-use crate::backend::StereoBuf;
+use crate::backend::{InstrumentNode, StereoBuf};
 use crate::graph::delay::DelayLine;
 use crate::graph::dsp;
 use core_model::mixer::{PluginFormat, PluginRef, PluginSlot};
@@ -35,7 +35,8 @@ pub trait EffectNode: Send {
     fn reset(&mut self);
 }
 
-/// Creates insert processors on the control thread.
+/// Creates plugin processors on the control thread: inserts, and the
+/// instruments of plugin instrument destinations.
 pub trait InsertFactory: Send {
     /// The processor for `slot` at the stream's rate and largest block.
     /// `Err` leaves the slot a placeholder (audio passes, no latency) and is
@@ -46,6 +47,22 @@ pub trait InsertFactory: Send {
         sample_rate: f64,
         max_frames: u32,
     ) -> Result<Box<dyn EffectNode>, String>;
+
+    /// The instrument for a plugin instrument destination's `slot`. `Err`
+    /// leaves the destination unroutable (reported in the compile
+    /// warnings) and its strip silent.
+    fn make_instrument(
+        &mut self,
+        slot: &PluginSlot,
+        sample_rate: f64,
+        max_frames: u32,
+    ) -> Result<Box<dyn InstrumentNode>, String> {
+        let _ = (sample_rate, max_frames);
+        Err(format!(
+            "{} cannot load until plugin hosting exists",
+            slot.plugin.name
+        ))
+    }
 }
 
 /// The factory before plugin hosting: every slot is a placeholder.
@@ -290,4 +307,158 @@ impl InsertFactory for TestInserts {
         self.made.fetch_add(1, Ordering::Relaxed);
         Ok(Box::new(n))
     }
+
+    fn make_instrument(
+        &mut self,
+        slot: &PluginSlot,
+        _: f64,
+        max_frames: u32,
+    ) -> Result<Box<dyn InstrumentNode>, String> {
+        let n = TestInstrument::parse(&slot.plugin.id, max_frames)?;
+        self.made.fetch_add(1, Ordering::Relaxed);
+        Ok(Box::new(n))
+    }
+}
+
+/// A test stand-in for a plugin instrument, from CLAP ids
+/// `vizdaw.test:synth[,outputs=N][,latency=L]`: while a note on MIDI
+/// channel `c` is held, output `c % N` carries the constant
+/// `0.1 × (c % N + 1)` (sample-exact from the note's offset), delayed by
+/// `latency` frames.
+#[derive(Debug, Clone)]
+pub struct TestInstrument {
+    outputs: u16,
+    latency: u32,
+    /// Held notes per output.
+    held: [u32; 16],
+    /// `(offset, output, on)` of this block's notes, in arrival order.
+    queue: Vec<(u32, u16, bool)>,
+    delay: Vec<DelayLine>,
+    max_frames: u32,
+}
+
+impl TestInstrument {
+    /// Parses a `vizdaw.test:synth…` id.
+    pub fn parse(id: &str, max_frames: u32) -> Result<TestInstrument, String> {
+        let spec = id
+            .strip_prefix(TestInserts::PREFIX)
+            .ok_or_else(|| format!("{id:?} is not a test plugin"))?;
+        let mut outputs = 1u16;
+        let mut latency = 0u32;
+        let mut synth = false;
+        for part in spec.split(',').filter(|p| !p.is_empty()) {
+            let (k, v) = part.split_once('=').unwrap_or((part, ""));
+            let bad = || format!("bad value {v:?} for {k:?}");
+            match k {
+                "synth" => synth = true,
+                "outputs" => outputs = v.parse().map_err(|_| bad())?,
+                "latency" => latency = v.parse().map_err(|_| bad())?,
+                other => return Err(format!("unknown test instrument key {other:?}")),
+            }
+        }
+        if !synth {
+            return Err(format!("{id:?} is not a test instrument"));
+        }
+        let outputs = outputs.clamp(1, 16);
+        Ok(TestInstrument {
+            outputs,
+            latency,
+            held: [0; 16],
+            queue: Vec::with_capacity(256),
+            delay: (0..outputs).map(|_| DelayLine::new(latency)).collect(),
+            max_frames,
+        })
+    }
+
+    /// The reference of test instrument `spec`, e.g. `"synth,outputs=4"`.
+    pub fn plugin(spec: &str) -> PluginRef {
+        TestInserts::plugin(spec)
+    }
+
+    /// Output count.
+    pub fn output_count(&self) -> u16 {
+        self.outputs
+    }
+
+    /// Renders output `k` into `out` from the held state, applying this
+    /// block's queued notes for that output at their offsets. // RT
+    fn render_one(&mut self, k: u16, frames: u32, out: &mut StereoBuf) {
+        let n = (frames as usize).min(out.left.len()).min(out.right.len());
+        let level = 0.1 * f32::from(k + 1);
+        let mut held = self.held.get(usize::from(k)).copied().unwrap_or(0);
+        let mut at = 0usize;
+        let queue = std::mem::take(&mut self.queue);
+        for &(off, o, on) in queue.iter().filter(|(_, o, _)| *o == k) {
+            let _ = o;
+            let end = (off as usize).min(n);
+            let v = if held > 0 { level } else { 0.0 };
+            if let (Some(l), Some(r)) = (out.left.get_mut(at..end), out.right.get_mut(at..end)) {
+                l.fill(v);
+                r.fill(v);
+            }
+            at = at.max(end);
+            held = if on { held + 1 } else { held.saturating_sub(1) };
+        }
+        self.queue = queue;
+        let v = if held > 0 { level } else { 0.0 };
+        if let (Some(l), Some(r)) = (out.left.get_mut(at..n), out.right.get_mut(at..n)) {
+            l.fill(v);
+            r.fill(v);
+        }
+        if let Some(h) = self.held.get_mut(usize::from(k)) {
+            *h = held;
+        }
+        if let (Some(d), Some(l), Some(r)) = (
+            self.delay.get_mut(usize::from(k)),
+            out.left.get_mut(..n),
+            out.right.get_mut(..n),
+        ) {
+            d.process(l, r);
+        }
+    }
+}
+
+impl InstrumentNode for TestInstrument {
+    fn midi(&mut self, offset: u32, msg: &[u8]) {
+        let (Some(&status), Some(&vel)) = (msg.first(), msg.get(2)) else {
+            return;
+        };
+        let out = u16::from(status & 0x0F) % self.outputs;
+        let on = match status & 0xF0 {
+            0x90 if vel > 0 => true,
+            0x90 | 0x80 => false,
+            _ => return,
+        };
+        if self.queue.len() < self.queue.capacity() {
+            self.queue.push((offset.min(self.max_frames), out, on));
+        }
+    }
+
+    fn render(&mut self, frames: u32, out: &mut StereoBuf) {
+        self.render_one(0, frames, out);
+        // Outputs without a buffer still consume their notes.
+        for k in 1..self.outputs {
+            let mut held = self.held.get(usize::from(k)).copied().unwrap_or(0);
+            for &(_, o, on) in self.queue.iter().filter(|(_, o, _)| *o == k) {
+                let _ = o;
+                held = if on { held + 1 } else { held.saturating_sub(1) };
+            }
+            if let Some(h) = self.held.get_mut(usize::from(k)) {
+                *h = held;
+            }
+        }
+        self.queue.clear();
+    }
+
+    fn reset(&mut self) {
+        self.held = [0; 16];
+        self.queue.clear();
+        for d in &mut self.delay {
+            d.clear();
+        }
+    }
+
+    fn latency(&self) -> u32 {
+        self.latency
+    }
 }
VIZDAW_EOF
```

Change `crates/core-engine/src/offline.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-engine/src/offline.rs b/crates/core-engine/src/offline.rs
--- a/crates/core-engine/src/offline.rs
+++ b/crates/core-engine/src/offline.rs
@@ -336,6 +336,9 @@ impl SinkFactory for OfflineSinks {
         sample_rate: f64,
         _max_frames: u32,
     ) -> Result<Sink, BackendError> {
+        if let TargetKind::PluginInstrument(id) = kind {
+            return Err(BackendError::NotSupported(format!("plugin instance {id}")));
+        }
         let tx = self.log.add(target, self.capacity);
         Ok(match kind {
             TargetKind::BuiltinSynth => Sink::Instrument(Box::new(RecordingNode {
@@ -345,14 +348,14 @@ impl SinkFactory for OfflineSinks {
                 dropped: Arc::clone(&self.log.dropped),
                 tone: self.tone.then(|| Box::new(Tone::new())),
             })),
-            TargetKind::MidiVirtualSource | TargetKind::MidiDestination(_) => {
-                Sink::Midi(Box::new(RecordingMidiOut {
-                    tx,
-                    target,
-                    sample_rate,
-                    accept: self.midi_accepts,
-                }))
-            }
+            TargetKind::MidiVirtualSource
+            | TargetKind::MidiDestination(_)
+            | TargetKind::PluginInstrument(_) => Sink::Midi(Box::new(RecordingMidiOut {
+                tx,
+                target,
+                sample_rate,
+                accept: self.midi_accepts,
+            })),
         })
     }
 }
VIZDAW_EOF
```

Change `crates/core-engine/src/render.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-engine/src/render.rs b/crates/core-engine/src/render.rs
--- a/crates/core-engine/src/render.rs
+++ b/crates/core-engine/src/render.rs
@@ -354,6 +354,9 @@ pub enum EngineCmd {
     SetLoop(Option<(u64, u64)>),
     /// Install the sink of a new target (targets are only appended).
     AddSink(TargetIdx, Sink),
+    /// Remove a released target's sink; it goes back through the retire
+    /// ring (a plugin instrument is deactivated off the audio thread).
+    RemoveSink(TargetIdx),
 }
 
 /// Something the renderer hands back for freeing off the audio thread.
@@ -363,13 +366,15 @@ pub enum Retired {
     Schedule(Box<Schedule>),
     /// A replaced graph (with the inserts it did not hand on).
     Graph(Box<Graph>),
+    /// A removed sink.
+    Sink(Sink),
 }
 
 /// Capacity of the command ring.
-pub const CMD_CAPACITY: usize = 64;
-/// Capacity of the retire ring: eight edits, each retiring a schedule and
-/// a graph (Plan Ruling 30).
-pub const RETIRE_CAPACITY: usize = 16;
+pub const CMD_CAPACITY: usize = 256;
+/// Capacity of the retire ring: room for a schedule, a graph and a
+/// removed sink per target (3a Plan Ruling 30; 3b Plan Ruling 4).
+pub const RETIRE_CAPACITY: usize = 64;
 
 /// Renderer settings.
 #[derive(Debug, Clone, Copy, PartialEq)]
@@ -451,11 +456,11 @@ impl Out<'_> {
 
 /// Tells every instrument sink not in `except` (a target bitmask) that
 /// `frames` frames passed unrendered. // RT
-fn skip_sinks(sinks: &mut SinkTable, frames: u64, except: u16) {
+fn skip_sinks(sinks: &mut SinkTable, frames: u64, except: u64) {
     for (t, s) in sinks.iter_mut().enumerate() {
         let bit = u32::try_from(t)
             .ok()
-            .and_then(|t| 1u16.checked_shl(t))
+            .and_then(|t| 1u64.checked_shl(t))
             .unwrap_or(0);
         if except & bit != 0 {
             continue;
@@ -701,6 +706,7 @@ impl Renderer {
                     self.schedule.is_some() && self.retire.slots() == 0
                 }
                 Ok(EngineCmd::SetGraph(_)) => self.graph.is_some() && self.retire.slots() == 0,
+                Ok(EngineCmd::RemoveSink(_)) => self.retire.slots() == 0,
                 Ok(_) => false,
                 Err(_) => break,
             };
@@ -789,6 +795,20 @@ impl Renderer {
                 self.seek();
             }
             EngineCmd::SetLoop(range) => self.transport.loop_range = range,
+            EngineCmd::RemoveSink(idx) => {
+                // Forget its sounding notes: nothing will play them now.
+                self.notes.release(idx, |_| {});
+                let taken = self
+                    .sinks
+                    .get_mut(usize::from(idx.0))
+                    .and_then(Option::take);
+                if let Some(old) = taken {
+                    if let Err(rtrb::PushError::Full(old)) = self.retire.push(Retired::Sink(old)) {
+                        // Unreachable: room was checked in `drain_commands`.
+                        std::mem::forget(old);
+                    }
+                }
+            }
             EngineCmd::AddSink(idx, sink) => {
                 if let Some(slot) = self.sinks.get_mut(usize::from(idx.0)) {
                     if let Some(old) = slot.replace(sink) {
VIZDAW_EOF
```

Change `crates/core-engine/src/schedule/targets.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-engine/src/schedule/targets.rs b/crates/core-engine/src/schedule/targets.rs
--- a/crates/core-engine/src/schedule/targets.rs
+++ b/crates/core-engine/src/schedule/targets.rs
@@ -4,9 +4,11 @@
 use crate::backend::MidiPortInfo;
 use crate::error::EngineError;
 use core_model::document::DestKind;
+use core_model::ids::PluginInstanceId;
 
-/// Most targets one engine can drive.
-pub const MAX_TARGETS: usize = 16;
+/// Most targets one engine can drive at once (Plan Ruling 4: plugin
+/// instruments are targets too).
+pub const MAX_TARGETS: usize = 64;
 
 /// Index into a [`TargetTable`].
 #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
@@ -21,6 +23,9 @@ pub enum TargetKind {
     MidiVirtualSource,
     /// An existing MIDI destination.
     MidiDestination(MidiPortInfo),
+    /// A plugin instrument destination's instance (its sink comes from the
+    /// engine's plugin factory, `InsertFactory::make_instrument`).
+    PluginInstrument(PluginInstanceId),
 }
 
 /// Endpoint reference that selects the built-in synth.
@@ -40,12 +45,25 @@ pub enum BuiltinRoute {
     Both,
 }
 
-/// The targets of an engine, in creation order. Targets are only ever
-/// appended (a failed load rolls back the ones it added before any sink
-/// reached the renderer), so an index stays valid for the engine's lifetime.
+/// One entry of a [`TargetTable`].
+#[derive(Debug, Clone, PartialEq, Eq)]
+enum Entry {
+    /// In use.
+    Live(TargetKind),
+    /// Released by this load: its sink is being removed, and a schedule
+    /// still queued may name it, so it is not reused before the next load.
+    Released,
+    /// Reusable.
+    Free,
+}
+
+/// The targets of an engine. An index names one target from `intern`
+/// until `release`; a released index is reused only from the next load on
+/// (`recycle`), after every schedule that named it was replaced (Plan
+/// Ruling 4).
 #[derive(Debug, Clone, Default, PartialEq, Eq)]
 pub struct TargetTable {
-    kinds: Vec<TargetKind>,
+    kinds: Vec<Entry>,
 }
 
 impl TargetTable {
@@ -54,55 +72,96 @@ impl TargetTable {
         TargetTable::default()
     }
 
-    /// The index of `kind`, if present.
+    /// The index of live `kind`, if present.
     pub fn find(&self, kind: &TargetKind) -> Option<TargetIdx> {
         self.kinds
             .iter()
-            .position(|k| k == kind)
+            .position(|k| matches!(k, Entry::Live(x) if x == kind))
             .and_then(|i| u8::try_from(i).ok())
             .map(TargetIdx)
     }
 
-    /// Adds `kind` (or returns its existing index).
+    /// The index the next new target gets, or `None` when the table is
+    /// full.
+    pub fn next_index(&self) -> Option<TargetIdx> {
+        let i = self
+            .kinds
+            .iter()
+            .position(|k| *k == Entry::Free)
+            .unwrap_or(self.kinds.len());
+        if i >= MAX_TARGETS {
+            return None;
+        }
+        u8::try_from(i).ok().map(TargetIdx)
+    }
+
+    /// Adds `kind` at [`TargetTable::next_index`] (or returns its existing
+    /// index).
     pub fn intern(&mut self, kind: TargetKind) -> Result<TargetIdx, EngineError> {
         if let Some(i) = self.find(&kind) {
             return Ok(i);
         }
-        if self.kinds.len() >= MAX_TARGETS {
-            return Err(EngineError::TooManyTargets);
+        let i = self.next_index().ok_or(EngineError::TooManyTargets)?;
+        match self.kinds.get_mut(usize::from(i.0)) {
+            Some(e) => *e = Entry::Live(kind),
+            None => self.kinds.push(Entry::Live(kind)),
+        }
+        Ok(i)
+    }
+
+    /// Stops using `idx` (its sink is being removed).
+    pub fn release(&mut self, idx: TargetIdx) {
+        if let Some(e) = self.kinds.get_mut(usize::from(idx.0)) {
+            if matches!(e, Entry::Live(_)) {
+                *e = Entry::Released;
+            }
+        }
+    }
+
+    /// Forgets `idx` at once (a failed load's target that never got a
+    /// sink).
+    pub(crate) fn forget(&mut self, idx: TargetIdx) {
+        if let Some(e) = self.kinds.get_mut(usize::from(idx.0)) {
+            *e = Entry::Free;
         }
-        let i = u8::try_from(self.kinds.len()).map_err(|_| EngineError::TooManyTargets)?;
-        self.kinds.push(kind);
-        Ok(TargetIdx(i))
     }
 
-    /// Drops targets added after the table had `len` entries (rollback of
-    /// a failed load whose new targets never got sinks).
-    pub(crate) fn truncate(&mut self, len: usize) {
-        self.kinds.truncate(len);
+    /// Makes indices released by earlier loads reusable (start of a load).
+    pub fn recycle(&mut self) {
+        for e in &mut self.kinds {
+            if *e == Entry::Released {
+                *e = Entry::Free;
+            }
+        }
     }
 
-    /// The kind at `idx`.
+    /// The live kind at `idx`.
     pub fn get(&self, idx: TargetIdx) -> Option<&TargetKind> {
-        self.kinds.get(usize::from(idx.0))
+        match self.kinds.get(usize::from(idx.0)) {
+            Some(Entry::Live(k)) => Some(k),
+            _ => None,
+        }
     }
 
-    /// Number of targets.
+    /// Number of live targets.
     pub fn len(&self) -> usize {
-        self.kinds.len()
+        self.kinds
+            .iter()
+            .filter(|k| matches!(k, Entry::Live(_)))
+            .count()
     }
 
-    /// True when there are no targets.
+    /// True when there are no live targets.
     pub fn is_empty(&self) -> bool {
-        self.kinds.is_empty()
+        self.len() == 0
     }
 
-    /// `(index, kind)` pairs.
+    /// `(index, kind)` of every live target, by index.
     pub fn iter(&self) -> impl Iterator<Item = (TargetIdx, &TargetKind)> + '_ {
-        self.kinds
-            .iter()
-            .enumerate()
-            .filter_map(|(i, k)| u8::try_from(i).ok().map(|i| (TargetIdx(i), k)))
+        self.kinds.iter().enumerate().filter_map(|(i, k)| match k {
+            Entry::Live(k) => u8::try_from(i).ok().map(|i| (TargetIdx(i), k)),
+            _ => None,
+        })
     }
 }
 
@@ -114,10 +173,7 @@ pub fn kinds_for(
     ports: &[MidiPortInfo],
 ) -> Result<Vec<TargetKind>, String> {
     match kind {
-        DestKind::PluginInstrument { slot } => Err(format!(
-            "plugin instrument {:?} cannot play until plugin hosting exists",
-            slot.plugin.name
-        )),
+        DestKind::PluginInstrument { slot } => Ok(vec![TargetKind::PluginInstrument(slot.id)]),
         DestKind::MidiEndpoint { endpoint_ref, .. } => match endpoint_ref.as_str() {
             BUILTIN_SYNTH_REF => Ok(match route {
                 BuiltinRoute::Builtin => vec![TargetKind::BuiltinSynth],
VIZDAW_EOF
```

Change `crates/platform-macos/src/sinks.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/platform-macos/src/sinks.rs b/crates/platform-macos/src/sinks.rs
--- a/crates/platform-macos/src/sinks.rs
+++ b/crates/platform-macos/src/sinks.rs
@@ -52,6 +52,9 @@ impl SinkFactory for MacSinks {
                 Sink::Midi(self.midi()?.open_virtual_source(VIRTUAL_SOURCE_NAME)?)
             }
             TargetKind::MidiDestination(port) => Sink::Midi(self.midi()?.open_destination(port)?),
+            TargetKind::PluginInstrument(id) => {
+                return Err(BackendError::NotSupported(format!("plugin instance {id}")));
+            }
         })
     }
 }
VIZDAW_EOF
```

- [ ] **Step 4: Run the tests to see them pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-engine --test plugin_instruments --test tempo_table --test schedule --test engine --test offline --test graph_mixer --test rt_alloc --test rt_alloc_graph
```

Expected: all pass, including `plugin_instruments`' `a_plugin_instrument_plays_through_its_strip`, `without_a_plugin_factory_the_destination_is_unroutable`, `released_targets_are_reused_only_from_the_next_load`, `restart_instance_recreates_inserts_and_instruments`, `a_larger_prepare_recreates_instrument_sinks_and_inserts`.

- [ ] **Step 5: Check the whole workspace**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Expected: no formatting changes left, no clippy warnings, every test passes.

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-engine/tests/plugin_instruments.rs \
  crates/core-engine/src/engine.rs \
  crates/core-engine/src/error.rs \
  crates/core-engine/src/fixtures.rs \
  crates/core-engine/src/graph/exec.rs \
  crates/core-engine/src/graph/mixer.rs \
  crates/core-engine/src/graph/mod.rs \
  crates/core-engine/src/graph/node.rs \
  crates/core-engine/src/offline.rs \
  crates/core-engine/src/render.rs \
  crates/core-engine/src/schedule/targets.rs \
  crates/core-engine/tests/engine.rs \
  crates/core-engine/tests/graph_mixer.rs \
  crates/core-engine/tests/offline.rs \
  crates/core-engine/tests/rt_alloc.rs \
  crates/core-engine/tests/rt_alloc_graph.rs \
  crates/core-engine/tests/schedule.rs \
  crates/core-engine/tests/tempo_table.rs \
  crates/platform-macos/src/sinks.rs \
  Cargo.lock
git commit -m "$(cat <<'MSG'
feat(engine): plugin instrument targets, released and reused a load later; recreate sinks for larger blocks

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
MSG
)"
```


---

### Task 3: Multi-output instruments and per-block transport facts

**Files:**
- Modify: `crates/core-engine/src/backend.rs`
- Modify: `crates/core-engine/src/engine.rs`
- Modify: `crates/core-engine/src/graph/exec.rs`
- Modify: `crates/core-engine/src/graph/mixer.rs`
- Modify: `crates/core-engine/src/graph/node.rs`
- Modify: `crates/core-engine/src/graph/spec.rs`
- Modify: `crates/core-engine/src/render.rs`
- Modify: `crates/core-engine/src/schedule/tempo.rs`
- Modify: `crates/core-engine/tests/graph_exec.rs`
- Modify: `crates/core-engine/tests/graph_mixer.rs`
- Modify: `crates/core-engine/tests/graph_plan.rs`
- Modify: `crates/core-engine/tests/plugin_instruments.rs`
- Lockfile: `Cargo.lock` (cargo updates it when dependencies change)

**Interfaces:**
- Consumes: Task 2's `InstrumentFeed`, `TestInstrument`, plugin instrument targets.
- Produces: `backend::BlockInfo { playing, frame, beats, tempo_bpm }`; `InstrumentNode::{outputs() -> u16, render_outputs(&mut self, u32, &mut [StereoBuf], &BlockInfo)}` (defaults); `EffectNode::process_block(&mut self, u32, &mut StereoBuf, Option<&StereoBuf>, &BlockInfo) -> ProcessStatus` (default calls `process`); `Tap::Out(u16)`; `NodeSpec::Instrument { target, latency, outputs }`; `InstrumentFeed.outputs` and `InstrumentFeed::plugin(TargetIdx, u32, u16, DestId)`; `TempoTable::bpm_at(u64, f64) -> f64`.

Instrument nodes hold one stereo buffer per output and call `render_outputs`; strips of kind `InstrumentOutput(dest, k)` read `Tap::Out(k)`, and a strip for an output the instrument does not have stays silent with a warning (spec §4). The renderer computes `BlockInfo` once per block (Ruling 11) and the executor hands it to `process_block` and `render_outputs`. Covers the 3a test gaps for several instruments and `InstrumentOutput` strips.

- [ ] **Step 1: Write the failing tests**


Update `crates/core-engine/tests/graph_exec.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-engine/tests/graph_exec.rs b/crates/core-engine/tests/graph_exec.rs
--- a/crates/core-engine/tests/graph_exec.rs
+++ b/crates/core-engine/tests/graph_exec.rs
@@ -339,6 +339,7 @@ fn instrument_nodes_render_their_sink() {
             NodeSpec::Instrument {
                 target: TargetIdx(2),
                 latency: 0,
+                outputs: 1,
             },
             NodeSpec::Strip(s),
         ],
@@ -371,10 +372,12 @@ fn a_repeated_instrument_target_renders_its_sink_once() {
             NodeSpec::Instrument {
                 target: TargetIdx(2),
                 latency: 0,
+                outputs: 1,
             },
             NodeSpec::Instrument {
                 target: TargetIdx(2),
                 latency: 0,
+                outputs: 1,
             },
             NodeSpec::Strip(s),
         ],
@@ -505,6 +508,7 @@ fn pre_tap_of_an_instrument_reads_its_output() {
             NodeSpec::Instrument {
                 target: TargetIdx(2),
                 latency: 0,
+                outputs: 1,
             },
             NodeSpec::Strip(y),
             NodeSpec::Strip(m),
VIZDAW_EOF
```

Update `crates/core-engine/tests/graph_mixer.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-engine/tests/graph_mixer.rs b/crates/core-engine/tests/graph_mixer.rs
--- a/crates/core-engine/tests/graph_mixer.rs
+++ b/crates/core-engine/tests/graph_mixer.rs
@@ -36,7 +36,8 @@ fn the_demo_plays_through_the_builtin_strip() {
         g.spec.nodes[0],
         NodeSpec::Instrument {
             target: TargetIdx(0),
-            latency: 0
+            latency: 0,
+            outputs: 1,
         }
     );
     assert_eq!(g.spec.master, 1);
@@ -215,7 +216,8 @@ fn insert_latencies_and_bypass_come_from_the_slots() {
         g.spec.nodes[0],
         NodeSpec::Instrument {
             target: TargetIdx(0),
-            latency: 7
+            latency: 7,
+            outputs: 1,
         }
     );
     let ins = &strip(&g.spec, 2).inserts[0];
@@ -297,3 +299,47 @@ fn post_fader_sends_and_bus_outputs_become_post_edges() {
     assert_eq!(strip(&g.spec, 3).inputs, vec![post(2, 0.0)]);
     assert_eq!(strip(&g.spec, 4).inputs[0].gain, 0.0);
 }
+
+#[test]
+fn instrument_outputs_become_out_taps() {
+    let mut b = Builder::new();
+    let (d, _) = b
+        .plugin_dest("Kit", PluginRef::from_legacy("clap:kit"))
+        .unwrap();
+    for (index, name) in [(1u16, "Snare"), (3, "Ghost")] {
+        b.mixer(MixerCmd::AddInstrumentOutput {
+            dest: d,
+            index,
+            name: name.into(),
+        })
+        .unwrap();
+    }
+    // Nodes: 0 the kit (2 outputs), 1 master, 2 kit strip, 3 Snare, 4 Ghost.
+    let g = mixer_spec(
+        b.doc(),
+        &[InstrumentFeed::plugin(TargetIdx(0), 0, 2, d)],
+        &mut zero,
+    );
+    assert_eq!(
+        g.spec.nodes[0],
+        NodeSpec::Instrument {
+            target: TargetIdx(0),
+            latency: 0,
+            outputs: 2,
+        }
+    );
+    assert_eq!(strip(&g.spec, 2).inputs, vec![post(0, 1.0)]);
+    assert_eq!(
+        strip(&g.spec, 3).inputs,
+        vec![InputSpec {
+            from: 0,
+            tap: Tap::Out(1),
+            gain: 1.0
+        }]
+    );
+    assert!(strip(&g.spec, 4).inputs.is_empty());
+    assert_eq!(
+        g.warnings,
+        vec!["strip \"Ghost\" is silent: its instrument has 2 output(s)".to_string()]
+    );
+}
VIZDAW_EOF
```

Update `crates/core-engine/tests/graph_plan.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-engine/tests/graph_plan.rs b/crates/core-engine/tests/graph_plan.rs
--- a/crates/core-engine/tests/graph_plan.rs
+++ b/crates/core-engine/tests/graph_plan.rs
@@ -37,6 +37,7 @@ fn a_chain_adds_insert_latencies() {
             NodeSpec::Instrument {
                 target: TargetIdx(0),
                 latency: 0,
+                outputs: 1,
             },
             strip(&[0], vec![ins(10), ins(20)]),
             strip(&[1], vec![]),
@@ -204,6 +205,7 @@ fn a_single_huge_latency_is_refused_even_off_any_edge() {
             NodeSpec::Instrument {
                 target: TargetIdx(0),
                 latency: l,
+                outputs: 1,
             },
             strip(&[0], vec![]),
         ],
VIZDAW_EOF
```

Update `crates/core-engine/tests/plugin_instruments.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-engine/tests/plugin_instruments.rs b/crates/core-engine/tests/plugin_instruments.rs
--- a/crates/core-engine/tests/plugin_instruments.rs
+++ b/crates/core-engine/tests/plugin_instruments.rs
@@ -6,13 +6,20 @@
 use core_engine::backend::AudioBackend;
 use core_engine::engine::{Engine, EngineConfig};
 use core_engine::fixtures::{Builder, Q};
+use core_engine::graph::{EffectNode, InsertFactory, ProcessStatus};
 use core_engine::graph::{TestInserts, TestInstrument};
 use core_engine::offline::{OfflineBackend, OfflineSinks};
 use core_engine::schedule::{TargetIdx, TargetKind};
+use core_model::command::MixerCmd;
 use core_model::command::{Command, RoutingCmd};
 use core_model::document::Document;
+use core_model::event::{EventBody, EventSpec};
 use core_model::ids::{DestId, PluginInstanceId, SeqId};
+use core_model::mixer::PluginSlot;
 use core_model::mixer::StripKind;
+use core_model::time::Tick;
+use std::sync::Arc;
+use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
 
 const SR: f64 = 48_000.0;
 
@@ -175,3 +182,144 @@ fn a_larger_prepare_recreates_instrument_sinks_and_inserts() {
     assert!(l.iter().all(|x| *x > 0.0), "plays at the new size");
     assert!(engine.faults().is_empty(), "{:?}", engine.faults().names());
 }
+
+/// A note on MIDI channel `ch` (an event-level channel override).
+fn note_on_channel(t: i64, pitch: u8, ch: u8) -> EventSpec {
+    let mut e = EventSpec::new(Tick(t), EventBody::note(pitch, 0xFFFF, Tick(16 * Q)));
+    e.group_channel = Some((0, ch));
+    e
+}
+
+#[test]
+fn multi_output_instruments_feed_their_output_strips() {
+    let mut b = Builder::new();
+    let (d, _) = b
+        .plugin_dest("Drums", TestInstrument::plugin("synth,outputs=4"))
+        .unwrap();
+    let (seq, lane) = b.sequence("S", "L", Some(d)).unwrap();
+    b.events(
+        seq,
+        lane,
+        vec![note_on_channel(0, 36, 0), note_on_channel(0, 38, 2)],
+    )
+    .unwrap();
+    for (index, name) in [(2u16, "Snare"), (5, "Ghost")] {
+        b.mixer(MixerCmd::AddInstrumentOutput {
+            dest: d,
+            index,
+            name: name.into(),
+        })
+        .unwrap();
+    }
+    let main = b.strip_of(StripKind::Instrument(d)).unwrap();
+    let snare = b.strip_of(StripKind::InstrumentOutput(d, 2)).unwrap();
+    let ghost = b.strip_of(StripKind::InstrumentOutput(d, 5)).unwrap();
+    let (mut engine, mut be, _) = engine(4096);
+    let report = engine.load(b.doc(), seq).unwrap();
+    assert_eq!(
+        report.graph.warnings,
+        vec!["strip \"Ghost\" is silent: its instrument has 4 output(s)".to_string()]
+    );
+    engine.play().unwrap();
+    be.run_frames(2048);
+    let m = engine.meters();
+    let peak = |s| m.iter().find(|x| x.strip == s).unwrap().peak_l;
+    let pan = std::f32::consts::FRAC_1_SQRT_2;
+    assert!((peak(main) - 0.1 * pan).abs() < 1e-6, "{m:?}");
+    assert!((peak(snare) - 0.3 * pan).abs() < 1e-6, "{m:?}");
+    assert_eq!(peak(ghost), 0.0);
+    let (l, _) = be.output(256);
+    assert!((l[200] - 0.4 * pan).abs() < 1e-6, "master sums both strips");
+}
+
+#[test]
+fn several_instruments_play_side_by_side() {
+    let mut b = Builder::new();
+    let (a, _) = b.plugin_dest("A", TestInstrument::plugin("synth")).unwrap();
+    let (c, _) = b
+        .plugin_dest("B", TestInstrument::plugin("synth,latency=100"))
+        .unwrap();
+    let (seq, la) = b.sequence("S", "A", Some(a)).unwrap();
+    b.notes(seq, la, &[(0, 60, 0xFFFF, 16 * Q)]).unwrap();
+    let lc = b.lane(seq, "B", Some(c)).unwrap();
+    b.notes(seq, lc, &[(0, 60, 0xFFFF, 16 * Q)]).unwrap();
+    let (mut engine, mut be, _) = engine(4096);
+    let report = engine.load(b.doc(), seq).unwrap();
+    assert_eq!(report.graph.nodes, 5, "two instruments, master, two strips");
+    assert_eq!(report.graph.latency_frames, 100, "A is delayed to match B");
+    engine.play().unwrap();
+    be.run_frames(256);
+    let (l, _) = be.output(256);
+    let pan = std::f32::consts::FRAC_1_SQRT_2;
+    assert_eq!(l[99], 0.0, "both arrive after the compensation");
+    assert!(
+        (l[100] - 0.2 * pan).abs() < 1e-6,
+        "and together: {}",
+        l[100]
+    );
+}
+
+/// An insert that records the transport facts it is given.
+#[derive(Clone, Default)]
+struct Probe {
+    frame: Arc<AtomicU64>,
+    bpm: Arc<AtomicU64>,
+    playing: Arc<AtomicBool>,
+}
+
+impl EffectNode for Probe {
+    fn latency(&self) -> u32 {
+        0
+    }
+    fn process(
+        &mut self,
+        _: u32,
+        _: &mut core_engine::backend::StereoBuf,
+        _: Option<&core_engine::backend::StereoBuf>,
+    ) -> ProcessStatus {
+        ProcessStatus::Ok
+    }
+    fn reset(&mut self) {}
+    fn process_block(
+        &mut self,
+        _: u32,
+        _: &mut core_engine::backend::StereoBuf,
+        _: Option<&core_engine::backend::StereoBuf>,
+        info: &core_engine::backend::BlockInfo,
+    ) -> ProcessStatus {
+        self.frame.store(info.frame, Ordering::Relaxed);
+        self.bpm.store(info.tempo_bpm.to_bits(), Ordering::Relaxed);
+        self.playing.store(info.playing, Ordering::Relaxed);
+        ProcessStatus::Ok
+    }
+}
+
+impl InsertFactory for Probe {
+    fn make(&mut self, _: &PluginSlot, _: f64, _: u32) -> Result<Box<dyn EffectNode>, String> {
+        Ok(Box::new(self.clone()))
+    }
+}
+
+#[test]
+fn inserts_see_tempo_position_and_play_state() {
+    let (mut b, seq, d, _) = synth_doc();
+    b.tempo(seq, &[(0, 90, false)]).unwrap();
+    let strip = b.strip_of(StripKind::Instrument(d)).unwrap();
+    b.insert(strip, TestInserts::plugin("probe")).unwrap();
+    let probe = Probe::default();
+    let (mut engine, mut be, _) = engine(4096);
+    engine.set_insert_factory(Box::new(probe.clone()));
+    engine.load(b.doc(), seq).unwrap();
+    be.run_frames(512);
+    assert!(!probe.playing.load(Ordering::Relaxed));
+    engine.play().unwrap();
+    be.run_frames(1024);
+    assert!(probe.playing.load(Ordering::Relaxed));
+    assert_eq!(
+        probe.frame.load(Ordering::Relaxed),
+        768,
+        "start of the last block"
+    );
+    let bpm = f64::from_bits(probe.bpm.load(Ordering::Relaxed));
+    assert!((bpm - 90.0).abs() < 1e-6, "{bpm}");
+}
VIZDAW_EOF
```

- [ ] **Step 2: Run them to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-engine --test plugin_instruments --test graph_mixer --test graph_plan --test graph_exec
```

Expected: compile errors: `missing field `outputs` in initializer of `NodeSpec``, `this function takes 3 arguments but 4 arguments were supplied` (`InstrumentFeed::plugin`), `no method named `process_block``.

- [ ] **Step 3: Implement**


Change `crates/core-engine/src/backend.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-engine/src/backend.rs b/crates/core-engine/src/backend.rs
--- a/crates/core-engine/src/backend.rs
+++ b/crates/core-engine/src/backend.rs
@@ -100,8 +100,34 @@ impl StereoBuf {
     }
 }
 
-/// An instrument hosted inside the engine (the built-in synth, later
-/// plugins). Every method is called on the real-time thread.
+/// Transport facts for one block, as of its first frame (plugins read
+/// tempo and position from it).
+#[derive(Debug, Clone, Copy, PartialEq)]
+pub struct BlockInfo {
+    /// The transport is playing.
+    pub playing: bool,
+    /// Schedule frame of the block's first frame.
+    pub frame: u64,
+    /// Position in quarter notes (ticks ÷ PPQ; negative in a pickup).
+    pub beats: f64,
+    /// Tempo in beats per minute.
+    pub tempo_bpm: f64,
+}
+
+impl Default for BlockInfo {
+    fn default() -> BlockInfo {
+        BlockInfo {
+            playing: false,
+            frame: 0,
+            beats: 0.0,
+            tempo_bpm: 120.0,
+        }
+    }
+}
+
+/// An instrument hosted inside the engine (the built-in synth, plugin
+/// instruments). Every method but `latency` and `outputs` is called on the
+/// real-time thread.
 pub trait InstrumentNode: Send {
     /// Queues one MIDI 1.0 message (or a whole `F0 … F7` SysEx) at a frame
     /// offset within the next `render` call. // RT
@@ -121,6 +147,26 @@ pub trait InstrumentNode: Send {
     fn latency(&self) -> u32 {
         0
     }
+
+    /// Number of stereo outputs (1 … 16), read on the control thread when
+    /// the sink is created. Output 0 feeds the destination's `Instrument`
+    /// strip, output `k` its `InstrumentOutput(_, k)` strip.
+    fn outputs(&self) -> u16 {
+        1
+    }
+
+    /// Renders every output: `outs[k]` is output `k` (`outs.len()` is
+    /// [`InstrumentNode::outputs`]). The default renders output 0 with
+    /// `render` and silences the rest. // RT
+    fn render_outputs(&mut self, frames: u32, outs: &mut [StereoBuf], info: &BlockInfo) {
+        let _ = info;
+        if let Some((first, rest)) = outs.split_first_mut() {
+            self.render(frames, first);
+            for o in rest {
+                o.clear(frames);
+            }
+        }
+    }
 }
 
 /// A timestamped MIDI output. `send` must be wait-free.
VIZDAW_EOF
```

Change `crates/core-engine/src/engine.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-engine/src/engine.rs b/crates/core-engine/src/engine.rs
--- a/crates/core-engine/src/engine.rs
+++ b/crates/core-engine/src/engine.rs
@@ -116,6 +116,8 @@ impl Live {
 struct SinkInfo {
     /// Instrument latency (`None`: not an instrument sink).
     latency: Option<u32>,
+    /// Instrument outputs.
+    outputs: u16,
     /// The block size the sink was made for.
     max_frames: u32,
 }
@@ -377,6 +379,10 @@ impl Engine {
                         Sink::Instrument(node) => Some(node.latency()),
                         Sink::Midi(_) => None,
                     },
+                    outputs: match &sink {
+                        Sink::Instrument(node) => node.outputs().clamp(1, 16),
+                        Sink::Midi(_) => 0,
+                    },
                     max_frames: max,
                 };
             }
@@ -475,14 +481,15 @@ impl Engine {
             .targets
             .iter()
             .filter_map(|(i, k)| {
-                let lat = self.sinks.get(usize::from(i.0))?.latency?;
+                let info = self.sinks.get(usize::from(i.0))?;
+                let lat = info.latency?;
                 match k {
                     TargetKind::BuiltinSynth => Some(InstrumentFeed::builtin(i, lat)),
                     TargetKind::PluginInstrument(id) => {
                         let dest = doc.instruments().values().find(|d| {
                             matches!(&d.kind, DestKind::PluginInstrument { slot } if slot.id == *id)
                         })?;
-                        Some(InstrumentFeed::plugin(i, lat, dest.id))
+                        Some(InstrumentFeed::plugin(i, lat, info.outputs, dest.id))
                     }
                     _ => None,
                 }
VIZDAW_EOF
```

Change `crates/core-engine/src/graph/exec.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-engine/src/graph/exec.rs b/crates/core-engine/src/graph/exec.rs
--- a/crates/core-engine/src/graph/exec.rs
+++ b/crates/core-engine/src/graph/exec.rs
@@ -19,7 +19,7 @@
 //! - between runs (`remaining == 0` and no worker inside the job, which
 //!   `Exec::try_finish` establishes) the owner has exclusive access.
 
-use crate::backend::StereoBuf;
+use crate::backend::{BlockInfo, StereoBuf};
 use crate::error::EngineError;
 use crate::graph::delay::DelayLine;
 use crate::graph::dsp;
@@ -144,6 +144,8 @@ enum NodeDef {
 struct NodeState {
     pre: StereoBuf,
     post: StereoBuf,
+    /// An instrument node's outputs (empty for strips).
+    outs: Box<[StereoBuf]>,
     scratch: StereoBuf,
     sc: StereoBuf,
     delays: Box<[DelayLine]>,
@@ -151,10 +153,11 @@ struct NodeState {
 }
 
 impl NodeState {
-    fn instrument(max: u32) -> NodeState {
+    fn instrument(max: u32, outputs: u16) -> NodeState {
         NodeState {
             pre: StereoBuf::new(0),
-            post: StereoBuf::new(max),
+            post: StereoBuf::new(0),
+            outs: (0..outputs.max(1)).map(|_| StereoBuf::new(max)).collect(),
             scratch: StereoBuf::new(0),
             sc: StereoBuf::new(0),
             delays: Box::new([]),
@@ -172,6 +175,7 @@ impl NodeState {
         NodeState {
             pre: StereoBuf::new(max),
             post: StereoBuf::new(max),
+            outs: Box::new([]),
             scratch: StereoBuf::new(if delayed { max } else { 0 }),
             sc: StereoBuf::new(if has_sc { max } else { 0 }),
             delays: delays.into_boxed_slice(),
@@ -190,6 +194,9 @@ impl NodeState {
                 *b = StereoBuf::new(max);
             }
         }
+        for b in self.outs.iter_mut() {
+            *b = StereoBuf::new(max);
+        }
     }
 }
 
@@ -254,11 +261,19 @@ fn acyclic(succ: &[Vec<usize>], indegree: &[u32], sources: &[usize]) -> bool {
     seen == indegree.len()
 }
 
-/// The buffer an edge reads from node `from`: an instrument has only its
-/// output, so `Pre` of an instrument reads `post`. // RT
+/// The buffer an edge reads from node `from`: of an instrument, its output
+/// `k` (`Pre` and `Post` read output 0; a missing output reads an empty
+/// buffer, i.e. silence). // RT
 fn tap_buf<'a>(g: &Graph, from: usize, src: &'a NodeState, tap: Tap) -> &'a StereoBuf {
     match (tap, g.defs.get(from)) {
         (Tap::Pre, Some(NodeDef::Strip { .. })) => &src.pre,
+        (_, Some(NodeDef::Instrument { .. })) => {
+            let k = match tap {
+                Tap::Out(k) => usize::from(k),
+                Tap::Pre | Tap::Post => 0,
+            };
+            src.outs.get(k).unwrap_or(&src.post)
+        }
         _ => &src.post,
     }
 }
@@ -301,7 +316,9 @@ impl Graph {
         };
         for (i, node) in spec.nodes.iter().enumerate() {
             match node {
-                NodeSpec::Instrument { target, .. } => {
+                NodeSpec::Instrument {
+                    target, outputs, ..
+                } => {
                     // Only the first node of a target renders its sink (the
                     // module protocol needs one user per sink); a repeat, or
                     // a target beyond the table, renders silence.
@@ -313,7 +330,7 @@ impl Graph {
                         usize::MAX
                     };
                     defs.push(NodeDef::Instrument { target });
-                    nodes.push(UnsafeCell::new(NodeState::instrument(max_frames)));
+                    nodes.push(UnsafeCell::new(NodeState::instrument(max_frames, *outputs)));
                 }
                 NodeSpec::Strip(s) => {
                     let in_start = inputs.len();
@@ -515,6 +532,7 @@ fn run_insert(
     io: &mut StereoBuf,
     sc: Option<&StereoBuf>,
     shared: &Shared,
+    info: &BlockInfo,
 ) {
     if bypass || ins.faulted {
         if !ins.in_bypass {
@@ -528,7 +546,7 @@ fn run_insert(
     let Some(node) = ins.node.as_mut() else {
         return;
     };
-    let status = node.process(frames, io, sc);
+    let status = node.process_block(frames, io, sc, info);
     if status == ProcessStatus::Error || dsp::is_faulty(io, frames) {
         io.clear(frames);
         ins.bad_run = ins.bad_run.saturating_add(1);
@@ -548,7 +566,14 @@ fn run_insert(
 /// `i` was popped from `g.queue` in the current run by the calling thread,
 /// and `sinks` are that run's sinks, which the owner does not touch until
 /// the run is finished (module protocol).
-unsafe fn render_node(g: &Graph, sinks: &SinkTable, shared: &Shared, frames: u32, i: usize) {
+unsafe fn render_node(
+    g: &Graph,
+    sinks: &SinkTable,
+    shared: &Shared,
+    frames: u32,
+    info: &BlockInfo,
+    i: usize,
+) {
     let (Some(def), Some(cell)) = (g.defs.get(i), g.nodes.get(i)) else {
         return;
     };
@@ -566,8 +591,14 @@ unsafe fn render_node(g: &Graph, sinks: &SinkTable, shared: &Shared, frames: u32
             // `&mut SinkTable` between runs.
             let sink = sinks.cells.get(*target).map(|c| unsafe { &mut *c.get() });
             match sink {
-                Some(Some(Sink::Instrument(node))) => node.render(frames, &mut st.post),
-                _ => st.post.clear(frames),
+                Some(Some(Sink::Instrument(node))) => {
+                    node.render_outputs(frames, &mut st.outs, info)
+                }
+                _ => {
+                    for o in st.outs.iter_mut() {
+                        o.clear(frames);
+                    }
+                }
             }
         }
         NodeDef::Strip {
@@ -632,7 +663,7 @@ unsafe fn render_node(g: &Graph, sinks: &SinkTable, shared: &Shared, frames: u32
                     }
                     None => None,
                 };
-                run_insert(ins, idef.bypass, frames, &mut st.pre, sc, shared);
+                run_insert(ins, idef.bypass, frames, &mut st.pre, sc, shared, info);
             }
             dsp::copy(&mut st.post, &st.pre, frames);
             dsp::scale(&mut st.post, *gain_l, *gain_r, frames);
@@ -653,6 +684,7 @@ pub(crate) struct Job {
     shared: *const Shared,
     pool: Option<*const PoolShared>,
     frames: u32,
+    info: BlockInfo,
 }
 
 // SAFETY: the pointees (`Graph`, `SinkTable`, `Shared`, `PoolShared`) are
@@ -672,6 +704,7 @@ impl Job {
             shared: std::ptr::null(),
             pool: None,
             frames: 0,
+            info: BlockInfo::default(),
         }
     }
 
@@ -693,7 +726,7 @@ impl Job {
         };
         // SAFETY: this thread popped `i` from this run's queue just above,
         // and `sinks` are this run's sinks (caller's contract).
-        unsafe { render_node(g, sinks, shared, self.frames, i) };
+        unsafe { render_node(g, sinks, shared, self.frames, &self.info, i) };
         let mut ready = 0u32;
         for &s in g.succ.get(i).map(|v| &**v).unwrap_or(&[]) {
             if let Some(p) = g.pending.get(s) {
@@ -816,6 +849,7 @@ impl Exec {
         sinks: &SinkTable,
         shared: &Shared,
         frames: u32,
+        info: BlockInfo,
         deadline: Option<Instant>,
     ) -> RunResult {
         // Never reset counters or the queue under a run still in flight.
@@ -844,6 +878,7 @@ impl Exec {
                 shared,
                 pool: pool.map(|p| p as *const PoolShared),
                 frames,
+                info,
             };
         }
         self.running = true;
@@ -966,10 +1001,14 @@ impl Runner {
         if !self.exec.try_finish(deadline) {
             return false;
         }
-        match self
-            .exec
-            .run(&self.graph, &self.sinks, &self.shared, frames, deadline)
-        {
+        match self.exec.run(
+            &self.graph,
+            &self.sinks,
+            &self.shared,
+            frames,
+            BlockInfo::default(),
+            deadline,
+        ) {
             RunResult::Done => {
                 self.graph.write_master(&mut [left, right], frames);
                 true
VIZDAW_EOF
```

Change `crates/core-engine/src/graph/mixer.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-engine/src/graph/mixer.rs b/crates/core-engine/src/graph/mixer.rs
--- a/crates/core-engine/src/graph/mixer.rs
+++ b/crates/core-engine/src/graph/mixer.rs
@@ -36,6 +36,8 @@ pub struct InstrumentFeed {
     pub target: TargetIdx,
     /// The sink's latency in frames.
     pub latency: u32,
+    /// The sink's stereo outputs (≥ 1).
+    pub outputs: u16,
     /// The strip it feeds.
     pub source: FeedSource,
 }
@@ -46,15 +48,18 @@ impl InstrumentFeed {
         InstrumentFeed {
             target,
             latency,
+            outputs: 1,
             source: FeedSource::Builtin,
         }
     }
 
-    /// A plugin instrument feed for destination `dest`.
-    pub fn plugin(target: TargetIdx, latency: u32, dest: DestId) -> InstrumentFeed {
+    /// A plugin instrument feed for destination `dest` with `outputs`
+    /// stereo outputs.
+    pub fn plugin(target: TargetIdx, latency: u32, outputs: u16, dest: DestId) -> InstrumentFeed {
         InstrumentFeed {
             target,
             latency,
+            outputs: outputs.max(1),
             source: FeedSource::Plugin(dest),
         }
     }
@@ -117,6 +122,35 @@ pub fn mixer_spec(
                 gain: 1.0,
             });
         }
+        // Extra outputs feed their `InstrumentOutput` strips; a strip for an
+        // output the instrument does not have stays silent (spec §4).
+        if let FeedSource::Plugin(d) = f.source {
+            for s in m.strips().values() {
+                let StripKind::InstrumentOutput(x, k) = s.kind else {
+                    continue;
+                };
+                if x != d {
+                    continue;
+                }
+                if k >= f.outputs {
+                    warnings.push(format!(
+                        "strip {:?} is silent: its instrument has {} output(s)",
+                        s.name, f.outputs
+                    ));
+                    continue;
+                }
+                if let Some(t) = node(s.id)
+                    .and_then(|t| t.checked_sub(base))
+                    .and_then(|k| strips.get_mut(k))
+                {
+                    t.inputs.push(InputSpec {
+                        from: i,
+                        tap: Tap::Out(k),
+                        gain: 1.0,
+                    });
+                }
+            }
+        }
     }
     let muted = |s: &Strip| s.mute || !on.contains(&s.id);
     for s in m.strips().values() {
@@ -199,6 +233,7 @@ pub fn mixer_spec(
         .map(|f| NodeSpec::Instrument {
             target: f.target,
             latency: f.latency,
+            outputs: f.outputs.max(1),
         })
         .collect();
     nodes.extend(strips.into_iter().map(NodeSpec::Strip));
VIZDAW_EOF
```

Change `crates/core-engine/src/graph/node.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-engine/src/graph/node.rs b/crates/core-engine/src/graph/node.rs
--- a/crates/core-engine/src/graph/node.rs
+++ b/crates/core-engine/src/graph/node.rs
@@ -1,7 +1,7 @@
 //! The insert seam: `EffectNode` processors made by an `InsertFactory`
 //! (Plan Ruling 12). 3a ships only test nodes; 3b wraps plugin instances.
 
-use crate::backend::{InstrumentNode, StereoBuf};
+use crate::backend::{BlockInfo, InstrumentNode, StereoBuf};
 use crate::graph::delay::DelayLine;
 use crate::graph::dsp;
 use core_model::mixer::{PluginFormat, PluginRef, PluginSlot};
@@ -33,6 +33,19 @@ pub trait EffectNode: Send {
     ) -> ProcessStatus;
     /// Clears internal state (delay memory, envelopes). // RT
     fn reset(&mut self);
+
+    /// [`EffectNode::process`] with the block's transport facts; plugin
+    /// processors override this. // RT
+    fn process_block(
+        &mut self,
+        frames: u32,
+        io: &mut StereoBuf,
+        sidechain: Option<&StereoBuf>,
+        info: &BlockInfo,
+    ) -> ProcessStatus {
+        let _ = info;
+        self.process(frames, io, sidechain)
+    }
 }
 
 /// Creates plugin processors on the control thread: inserts, and the
@@ -461,4 +474,21 @@ impl InstrumentNode for TestInstrument {
     fn latency(&self) -> u32 {
         self.latency
     }
+
+    fn outputs(&self) -> u16 {
+        self.outputs
+    }
+
+    fn render_outputs(&mut self, frames: u32, outs: &mut [StereoBuf], _: &BlockInfo) {
+        for k in 0..self.outputs {
+            match outs.get_mut(usize::from(k)) {
+                Some(o) => self.render_one(k, frames, o),
+                None => {
+                    let mut dummy = StereoBuf::new(0);
+                    self.render_one(k, 0, &mut dummy);
+                }
+            }
+        }
+        self.queue.clear();
+    }
 }
VIZDAW_EOF
```

Change `crates/core-engine/src/graph/spec.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-engine/src/graph/spec.rs b/crates/core-engine/src/graph/spec.rs
--- a/crates/core-engine/src/graph/spec.rs
+++ b/crates/core-engine/src/graph/spec.rs
@@ -9,9 +9,12 @@ use core_model::ids::{PluginInstanceId, StripId};
 pub enum Tap {
     /// After the inserts, before the fader (pre-fader sends).
     Pre,
-    /// After fader and pan (outputs, post-fader sends, sidechains;
-    /// instrument nodes write here).
+    /// After fader and pan (outputs, post-fader sends, sidechains); of an
+    /// instrument node, its output 0.
     Post,
+    /// Output `k` (≥ 1) of an instrument node (a multi-output plugin
+    /// instrument's extra output).
+    Out(u16),
 }
 
 /// An edge into a strip: `gain × tap(from)`.
@@ -86,6 +89,8 @@ pub enum NodeSpec {
         target: TargetIdx,
         /// The instrument's latency in frames.
         latency: u32,
+        /// Number of stereo outputs (≥ 1).
+        outputs: u16,
     },
     /// A mixer strip.
     Strip(StripSpec),
@@ -115,6 +120,7 @@ fn tap_code(t: Tap) -> u64 {
     match t {
         Tap::Pre => 1,
         Tap::Post => 2,
+        Tap::Out(k) => 3 + u64::from(k),
     }
 }
 
@@ -142,10 +148,15 @@ impl GraphSpec {
         h.u64(self.master as u64);
         for n in &self.nodes {
             match n {
-                NodeSpec::Instrument { target, latency } => {
+                NodeSpec::Instrument {
+                    target,
+                    latency,
+                    outputs,
+                } => {
                     h.u64(1);
                     h.u64(u64::from(target.0));
                     h.u64(u64::from(*latency));
+                    h.u64(u64::from(*outputs));
                 }
                 NodeSpec::Strip(s) => {
                     h.u64(2);
VIZDAW_EOF
```

Change `crates/core-engine/src/render.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-engine/src/render.rs b/crates/core-engine/src/render.rs
--- a/crates/core-engine/src/render.rs
+++ b/crates/core-engine/src/render.rs
@@ -1,6 +1,6 @@
 //! The real-time renderer and the state it publishes.
 
-use crate::backend::{BlockTime, MidiOut, StereoBuf};
+use crate::backend::{BlockInfo, BlockTime, MidiOut, StereoBuf};
 use crate::graph::exec::{Exec, RunResult, SinkTable};
 use crate::graph::{Graph, MAX_INSERTS, MAX_NODES};
 use crate::notes::ActiveNotes;
@@ -602,6 +602,7 @@ impl Renderer {
         }
         self.block_ns = time.host_time_ns;
         self.drain_commands();
+        let info = self.block_info();
         let length = self.schedule.as_ref().map_or(0, |s| s.length_frames);
         let late_ns = self.cfg.midi_offset_ns + self.latency_ns();
         let mut o = Out {
@@ -647,7 +648,7 @@ impl Renderer {
             }
             offset += step.frames();
         }
-        let missed = self.render_graph(out, frames, deadline);
+        let missed = self.render_graph(out, frames, info, deadline);
         self.publish();
         if block_s > 0.0 {
             let load = (started.elapsed().as_secs_f64() / block_s) as f32;
@@ -664,6 +665,7 @@ impl Renderer {
         &mut self,
         out: &mut [&mut [f32]],
         frames: u32,
+        info: BlockInfo,
         deadline: Option<Instant>,
     ) -> bool {
         let Some(g) = self.graph.as_deref() else {
@@ -680,7 +682,7 @@ impl Renderer {
         }
         match self
             .exec
-            .run(g, &self.sinks, &self.shared, frames, deadline)
+            .run(g, &self.sinks, &self.shared, frames, info, deadline)
         {
             RunResult::Done => {
                 g.write_master(out, frames);
@@ -820,6 +822,24 @@ impl Renderer {
         }
     }
 
+    /// The transport facts of the block about to render. // RT
+    fn block_info(&self) -> BlockInfo {
+        let frame = self.transport.pos;
+        let sr = self.cfg.sample_rate;
+        let (beats, tempo_bpm) = self.schedule.as_ref().map_or((0.0, 120.0), |s| {
+            (
+                s.tempo.frame_to_tick(frame).0 as f64 / core_model::time::PPQ as f64,
+                s.tempo.bpm_at(frame, sr),
+            )
+        });
+        BlockInfo {
+            playing: self.transport.state == PlayState::Playing,
+            frame,
+            beats,
+            tempo_bpm,
+        }
+    }
+
     fn seek(&mut self) {
         self.cursor = self
             .schedule
VIZDAW_EOF
```

Change `crates/core-engine/src/schedule/tempo.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-engine/src/schedule/tempo.rs b/crates/core-engine/src/schedule/tempo.rs
--- a/crates/core-engine/src/schedule/tempo.rs
+++ b/crates/core-engine/src/schedule/tempo.rs
@@ -104,6 +104,26 @@ impl TempoTable {
         Tick(t.round() as i64)
     }
 
+    /// The tempo at `frame` in beats per minute (the local slope of the
+    /// map). // RT
+    pub fn bpm_at(&self, frame: u64, sample_rate: f64) -> f64 {
+        let f = frame as f64;
+        let i = self.knots.partition_point(|k| k.frame <= f);
+        let slope = match (
+            i.checked_sub(1).and_then(|j| self.knots.get(j)),
+            self.knots.get(i),
+        ) {
+            (Some(a), Some(b)) if b.tick > a.tick => (b.frame - a.frame) / (b.tick - a.tick) as f64,
+            (None, Some(_)) => self.head_slope,
+            _ => self.tail_slope,
+        };
+        if slope > 0.0 {
+            60.0 * sample_rate / (slope * PPQ as f64)
+        } else {
+            120.0
+        }
+    }
+
     /// The frame at `tick`, rounded to the nearest frame (0 before the
     /// origin). // RT
     pub fn tick_to_frame(&self, tick: Tick) -> u64 {
VIZDAW_EOF
```

- [ ] **Step 4: Run the tests to see them pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-engine --test plugin_instruments --test graph_mixer --test graph_plan --test graph_exec
```

Expected: all pass, including `multi_output_instruments_feed_their_output_strips`, `several_instruments_play_side_by_side`, `inserts_see_tempo_position_and_play_state`, `instrument_outputs_become_out_taps`.

- [ ] **Step 5: Check the whole workspace**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Expected: no formatting changes left, no clippy warnings, every test passes.

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-engine/src/backend.rs \
  crates/core-engine/src/engine.rs \
  crates/core-engine/src/graph/exec.rs \
  crates/core-engine/src/graph/mixer.rs \
  crates/core-engine/src/graph/node.rs \
  crates/core-engine/src/graph/spec.rs \
  crates/core-engine/src/render.rs \
  crates/core-engine/src/schedule/tempo.rs \
  crates/core-engine/tests/graph_exec.rs \
  crates/core-engine/tests/graph_mixer.rs \
  crates/core-engine/tests/graph_plan.rs \
  crates/core-engine/tests/plugin_instruments.rs \
  Cargo.lock
git commit -m "$(cat <<'MSG'
feat(engine): multi-output instrument nodes and per-block transport facts

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
MSG
)"
```


---

### Task 4: core-host: plugin types, traits and parameter queues

**Files:**
- Create: `crates/core-host/Cargo.toml`
- Create: `crates/core-host/src/error.rs`
- Create: `crates/core-host/src/info.rs`
- Create: `crates/core-host/src/instance.rs`
- Create: `crates/core-host/src/lib.rs`
- Create: `crates/core-host/src/process.rs`
- Create: `crates/core-host/src/queue.rs`
- Create: `crates/core-host/tests/types.rs`
- Lockfile: `Cargo.lock` (cargo updates it when dependencies change)

**Interfaces:**
- Consumes: `core_engine::{backend::BlockInfo, graph::ProcessStatus}`, `core_model::mixer::{ParamId, PluginRef}`.
- Produces: crate `core-host` with `HostError`; `PluginInfo { plugin, kind, inputs, outputs, note_input, params, latency, tail }` (+ `sidechain_input`, `main_input`, `output_channels`, `param`), `AudioPortInfo`, `PortRole`, `ParamInfo`, `PluginKind`, `Tail`; `MidiEvent`, `ParamChange`, `ParamEvent::{Begin, Value, End}`, `ParamOut`, `ProcessCtx<'a> { frames, input, sidechain, outputs, events, params_in, params_out, transport }`, `MAX_EVENTS = 512`, `MAX_PARAM_EVENTS = 256`; traits `AudioProcessor`, `PluginInstance`, `PluginEditor`, `PluginFormat`, plus `InstanceEvents`, `ParentView`, `LoadRequest`, `Prepared`, `PrepareJob`; `queue::{param_queues, ParamSender, ParamReceiver, GestureSender, GestureReceiver, PARAM_QUEUE}`.

The plugin abstraction of spec §5 as split by Ruling 1, in a new Linux-buildable crate. `ProcessCtx` carries pre-allocated channel buffers (`Box<[f32]>` each, at least `frames` long), MIDI events, parameter changes in and a fixed-capacity `ParamOut` for gestures out. The queues are `rtrb` rings between the main thread and a processor.

- [ ] **Step 1: Write the failing tests**


Create `crates/core-host/tests/types.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p crates/core-host/tests
cat > crates/core-host/tests/types.rs <<'VIZDAW_EOF'
use core_engine::rt_check::{self, CountingAlloc};
use core_host::queue::param_queues;
use core_host::{
    AudioPortInfo, MAX_PARAM_EVENTS, MidiEvent, ParamChange, ParamEvent, ParamInfo, ParamOut,
    PluginInfo, PluginKind, PortRole, Tail,
};
use core_model::mixer::{ParamId, PluginFormat, PluginRef};

#[global_allocator]
static ALLOC: CountingAlloc = CountingAlloc;

fn ducker() -> PluginInfo {
    let port = |name: &str, role| AudioPortInfo {
        name: name.into(),
        channels: 2,
        role,
    };
    PluginInfo {
        plugin: PluginRef {
            format: PluginFormat::Clap,
            id: "vizdaw.test.ducker".into(),
            name: "Test Ducker".into(),
            vendor: "VizDAW".into(),
            version: "1.0.0".into(),
        },
        kind: PluginKind::Effect,
        inputs: vec![
            port("In", PortRole::Main),
            port("Sidechain", PortRole::Sidechain),
        ],
        outputs: vec![port("Out", PortRole::Main)],
        note_input: false,
        params: vec![ParamInfo {
            id: ParamId(0),
            name: "Depth".into(),
            module: String::new(),
            min: 0.0,
            max: 1.0,
            default: 1.0,
            stepped: false,
            automatable: true,
            hidden: false,
        }],
        latency: 0,
        tail: Tail::Frames(480),
    }
}

#[test]
fn plugin_info_round_trips_as_json_and_finds_ports() {
    let info = ducker();
    let json = serde_json::to_string(&info).unwrap();
    assert!(json.contains("\"role\":\"sidechain\""), "{json}");
    let back: PluginInfo = serde_json::from_str(&json).unwrap();
    assert_eq!(back, info);
    assert_eq!(info.main_input(), Some(0));
    assert_eq!(info.sidechain_input(), Some(1));
    assert_eq!(info.output_channels(), 2);
    assert_eq!(info.param(ParamId(0)).unwrap().name, "Depth");
    assert!(info.param(ParamId(9)).is_none());
}

#[test]
fn midi_events_keep_up_to_three_bytes() {
    let e = MidiEvent::new(7, &[0x90, 60, 100]);
    assert_eq!((e.frame, e.as_slice()), (7, &[0x90u8, 60, 100][..]));
    assert_eq!(MidiEvent::new(0, &[0xC0, 5]).as_slice(), &[0xC0, 5]);
    assert_eq!(MidiEvent::new(0, &[0xF0, 1, 2, 3, 0xF7]).len, 3);
}

#[test]
fn param_out_is_bounded_and_never_allocates() {
    let mut out = ParamOut::default();
    rt_check::arm();
    let before = rt_check::count();
    for i in 0..MAX_PARAM_EVENTS as u64 {
        assert!(out.push(ParamEvent::Value(ParamId(i), 0.5)));
    }
    assert!(!out.push(ParamEvent::End(ParamId(0))), "full");
    let n = out.as_slice().len();
    out.clear();
    let allocs = rt_check::count() - before;
    rt_check::disarm();
    assert_eq!(n, MAX_PARAM_EVENTS);
    assert!(out.as_slice().is_empty());
    assert_eq!(allocs, 0);
}

#[test]
fn queues_carry_values_in_and_gestures_out() {
    let ((mut tx, mut gestures), (mut rx, mut gtx)) = param_queues();
    assert!(tx.send(ParamId(1), 0.25));
    assert!(tx.send(ParamId(2), 0.75));
    let mut buf = [ParamChange::default(); 8];
    rt_check::arm();
    let before = rt_check::count();
    let n = rx.drain_into(&mut buf);
    let lost = gtx.send_all(&[
        ParamEvent::Begin(ParamId(1)),
        ParamEvent::Value(ParamId(1), 0.3),
        ParamEvent::End(ParamId(1)),
    ]);
    let allocs = rt_check::count() - before;
    rt_check::disarm();
    assert_eq!(allocs, 0, "the audio-side ends never allocate");
    assert_eq!((n, lost), (2, 0));
    assert_eq!(
        (buf[0].id, buf[0].value, buf[1].id, buf[1].value),
        (ParamId(1), 0.25, ParamId(2), 0.75)
    );
    assert_eq!(
        gestures.drain(),
        vec![
            ParamEvent::Begin(ParamId(1)),
            ParamEvent::Value(ParamId(1), 0.3),
            ParamEvent::End(ParamId(1)),
        ]
    );
}
VIZDAW_EOF
```

- [ ] **Step 2: Run them to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-host --test types
```

Expected: `error: package ID specification `core-host` did not match any packages`.

- [ ] **Step 3: Implement**


Create `crates/core-host/Cargo.toml`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p crates/core-host
cat > crates/core-host/Cargo.toml <<'VIZDAW_EOF'
[package]
name = "core-host"
version = "0.1.0"
edition.workspace = true
license.workspace = true
rust-version.workspace = true

[dependencies]
core-model = { path = "../core-model" }
core-engine = { path = "../core-engine" }
rtrb = "0.4"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"

[dev-dependencies]
core-engine = { path = "../core-engine", features = ["fixtures"] }
VIZDAW_EOF
```

Create `crates/core-host/src/error.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p crates/core-host/src
cat > crates/core-host/src/error.rs <<'VIZDAW_EOF'
//! Errors plugins can cause (spec §12). The host survives every one of
//! them; only a crash ends the process.

/// A plugin operation failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HostError {
    /// No plugin with this format and id is known (not scanned, or its
    /// bundle is gone).
    #[error("plugin not found: {0}")]
    NotFound(String),
    /// Loading or instantiating failed.
    #[error("load failed: {0}")]
    LoadFailed(String),
    /// The plugin refused a saved state.
    #[error("state rejected: {0}")]
    StateRejected(String),
    /// Activation (allocating for a sample rate and block size) failed.
    #[error("activate failed: {0}")]
    ActivateFailed(String),
    /// The plugin has no editor, or it could not be shown.
    #[error("editor unavailable: {0}")]
    EditorUnavailable(String),
    /// The plugin is disabled or blocklisted in the Plugin Manager.
    #[error("disabled: {0}")]
    Disabled(String),
}
VIZDAW_EOF
```

Create `crates/core-host/src/info.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p crates/core-host/src
cat > crates/core-host/src/info.rs <<'VIZDAW_EOF'
//! What a plugin is: ports, parameters, latency and tail (spec §5). The
//! scanner prints it as JSON and the catalog stores it, so it is serde.

use core_model::mixer::{ParamId, PluginRef};
use serde::{Deserialize, Serialize};

/// Instrument or effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginKind {
    /// Takes notes, makes audio (a plugin instrument destination).
    Instrument,
    /// Processes audio (an insert).
    Effect,
}

/// What an audio port is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PortRole {
    /// The main input or output.
    Main,
    /// A sidechain input (CLAP: the first non-main input; AU: input bus 1).
    Sidechain,
    /// Any other port (an instrument's extra outputs).
    Aux,
}

/// One audio port (a channel group).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AudioPortInfo {
    /// Display name.
    pub name: String,
    /// Channels (1 or 2 in VizDAW; the graph is stereo, Plan Ruling 9).
    pub channels: u16,
    /// Role.
    pub role: PortRole,
}

/// One parameter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParamInfo {
    /// Stable ID (CLAP `clap_id`, AU parameter address).
    pub id: ParamId,
    /// Display name.
    pub name: String,
    /// Group path, `/`-separated (may be empty).
    pub module: String,
    /// Lowest plain value.
    pub min: f64,
    /// Highest plain value.
    pub max: f64,
    /// Default plain value.
    pub default: f64,
    /// Takes whole values only.
    pub stepped: bool,
    /// Automatable (sub-project 4 lists only these).
    pub automatable: bool,
    /// Hidden from the user.
    pub hidden: bool,
}

/// How long a plugin keeps sounding after its input stops.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tail {
    /// No tail (or not reported).
    #[default]
    None,
    /// This many frames.
    Frames(u32),
    /// Forever (a reverb that never decays to silence).
    Infinite,
}

/// Everything VizDAW knows about a plugin.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginInfo {
    /// Format, id, name, vendor, version.
    pub plugin: PluginRef,
    /// Instrument or effect.
    pub kind: PluginKind,
    /// Audio inputs, main first.
    pub inputs: Vec<AudioPortInfo>,
    /// Audio outputs, main first.
    pub outputs: Vec<AudioPortInfo>,
    /// Takes notes.
    pub note_input: bool,
    /// Parameters, in the plugin's order.
    pub params: Vec<ParamInfo>,
    /// Latency in frames (at the last activation).
    pub latency: u32,
    /// Tail.
    pub tail: Tail,
}

impl PluginInfo {
    /// Index of the sidechain input port, if any (a plugin without one
    /// shows no sidechain selector, spec §5).
    pub fn sidechain_input(&self) -> Option<usize> {
        self.inputs
            .iter()
            .position(|p| p.role == PortRole::Sidechain)
    }

    /// Index of the main input port, if any.
    pub fn main_input(&self) -> Option<usize> {
        self.inputs.iter().position(|p| p.role == PortRole::Main)
    }

    /// Total output channels over every port.
    pub fn output_channels(&self) -> usize {
        self.outputs.iter().map(|p| usize::from(p.channels)).sum()
    }

    /// A parameter by ID.
    pub fn param(&self, id: ParamId) -> Option<&ParamInfo> {
        self.params.iter().find(|p| p.id == id)
    }
}
VIZDAW_EOF
```

Create `crates/core-host/src/instance.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p crates/core-host/src
cat > crates/core-host/src/instance.rs <<'VIZDAW_EOF'
//! The plugin abstraction (spec §5, Plan Ruling 1): a main-thread
//! [`PluginInstance`] that activates into a real-time [`AudioProcessor`],
//! and the [`PluginFormat`] that makes instances (AU on macOS, CLAP
//! everywhere).

use crate::error::HostError;
use crate::info::PluginInfo;
use crate::process::ProcessCtx;
use core_engine::graph::ProcessStatus;
use core_model::mixer::{ParamId, PluginRef};
use std::any::Any;
use std::ffi::c_void;
use std::path::PathBuf;

/// The real-time half of an active plugin. Moved to the render threads;
/// used by one thread at a time.
pub trait AudioProcessor: Send {
    /// Processes one block. `Error` counts as a faulty block (spec §7).
    /// // RT
    fn process(&mut self, ctx: &mut ProcessCtx<'_>) -> ProcessStatus;
    /// Clears voices, delay memory and envelopes. // RT
    fn reset(&mut self);
}

/// What an instance asked for since the last [`PluginInstance::idle`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InstanceEvents {
    /// The plugin wants to be deactivated and activated again (CLAP
    /// `request_restart`, e.g. after a latency change).
    pub restart: bool,
    /// Its latency changed (read again after the next activation).
    pub latency_changed: bool,
    /// Its parameter list or values changed outside a gesture.
    pub params_changed: bool,
    /// Its state changed in a way the host should save (CLAP
    /// `state.mark_dirty`).
    pub state_dirty: bool,
}

/// The native view an editor attaches to (an `NSView *` on macOS).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParentView(pub *mut c_void);

/// A plugin's own editor. Main thread only.
pub trait PluginEditor {
    /// Attaches the editor to `parent` and shows it; returns its size in
    /// points.
    fn open(&mut self, parent: ParentView) -> Result<(u32, u32), HostError>;
    /// Asks for a new size; returns the size the plugin accepted.
    fn resize(&mut self, width: u32, height: u32) -> Result<(u32, u32), HostError>;
    /// Whether the user may resize it.
    fn can_resize(&self) -> bool;
    /// Hides and detaches it (also on drop).
    fn close(&mut self);
}

/// The main-thread half of a plugin. Not `Send`: CLAP and AppKit tie it to
/// the thread that made it (Plan Ruling 1).
pub trait PluginInstance {
    /// Ports, parameters, latency and tail as of the last activation.
    fn info(&self) -> &PluginInfo;
    /// Allocates for `sample_rate` and blocks of up to `max_frames` and
    /// hands out the processor. Fails while a processor is out.
    fn activate(
        &mut self,
        sample_rate: f64,
        max_frames: u32,
    ) -> Result<Box<dyn AudioProcessor>, HostError>;
    /// Takes the processor back and frees what `activate` allocated.
    fn deactivate(&mut self, processor: Box<dyn AudioProcessor>);
    /// Whether a processor is out.
    fn is_active(&self) -> bool;
    /// The plugin's full state.
    fn save_state(&mut self) -> Result<Vec<u8>, HostError>;
    /// Restores a state from [`PluginInstance::save_state`].
    fn load_state(&mut self, data: &[u8]) -> Result<(), HostError>;
    /// Sets parameters while inactive (load: state first, then the
    /// overlay, spec §4). Active instances get values through the
    /// processor instead.
    fn set_params(&mut self, values: &[(ParamId, f64)]) -> Result<(), HostError>;
    /// A parameter's current value, when the plugin reports one.
    fn param_value(&mut self, id: ParamId) -> Option<f64>;
    /// The plugin's editor, if it has one.
    fn editor(&mut self) -> Option<Box<dyn PluginEditor + '_>>;
    /// Main-thread servicing (`host_idle`, 60 Hz): runs callbacks the
    /// plugin asked for and reports what it requested.
    fn idle(&mut self) -> InstanceEvents;
}

/// What to load.
#[derive(Debug, Clone, PartialEq)]
pub struct LoadRequest {
    /// The plugin.
    pub plugin: PluginRef,
    /// Where it lives (a CLAP bundle), from the catalog.
    pub path: Option<PathBuf>,
    /// Rate it will be activated at.
    pub sample_rate: f64,
    /// Largest block it will be activated for.
    pub max_frames: u32,
}

/// What a format's loader step made, handed to [`PluginFormat::finish`] on
/// the main thread.
pub struct Prepared(pub Box<dyn Any + Send>);

/// A format's loader step: runs on the loader thread.
pub type PrepareJob = Box<dyn FnOnce() -> Result<Prepared, HostError> + Send>;

/// A plugin format: AU (macOS) or CLAP.
pub trait PluginFormat {
    /// Which format.
    fn format(&self) -> core_model::mixer::PluginFormat;
    /// The part of loading that may run off the main thread (AU v2:
    /// instantiate and allocate; AU v3: the asynchronous instantiation;
    /// CLAP: nothing, Plan Ruling 2).
    fn prepare(&self, req: &LoadRequest) -> PrepareJob;
    /// Finishes on the main thread.
    fn finish(
        &mut self,
        req: &LoadRequest,
        prepared: Prepared,
    ) -> Result<Box<dyn PluginInstance>, HostError>;
}
VIZDAW_EOF
```

Create `crates/core-host/src/lib.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p crates/core-host/src
cat > crates/core-host/src/lib.rs <<'VIZDAW_EOF'
//! `core-host`: the plugin abstraction (spec §5) and its platform-neutral
//! hosts. A plugin is a main-thread [`PluginInstance`] plus the real-time
//! [`AudioProcessor`] it hands out when activated (Plan Ruling 1);
//! [`adapt`] wraps processors as the engine's inserts and instruments.
//!
//! `unsafe` lives only in `clap/ffi.rs`, with a `// SAFETY:` comment per
//! block. Everything marked `// RT` must not allocate, lock or block.
#![deny(missing_docs)]
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic
    )
)]

pub mod error;
pub mod info;
pub mod instance;
pub mod process;
pub mod queue;

pub use error::HostError;
pub use info::{AudioPortInfo, ParamInfo, PluginInfo, PluginKind, PortRole, Tail};
pub use instance::{
    AudioProcessor, InstanceEvents, LoadRequest, ParentView, PluginEditor, PluginFormat,
    PluginInstance, PrepareJob, Prepared,
};
pub use process::{
    MAX_EVENTS, MAX_PARAM_EVENTS, MidiEvent, ParamChange, ParamEvent, ParamOut, ProcessCtx,
};
VIZDAW_EOF
```

Create `crates/core-host/src/process.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p crates/core-host/src
cat > crates/core-host/src/process.rs <<'VIZDAW_EOF'
//! What one `AudioProcessor::process` call gets (spec §5). Every buffer is
//! allocated before processing starts; nothing here allocates. // RT

use core_engine::backend::BlockInfo;
use core_model::mixer::ParamId;

/// Most MIDI events passed in one block (more are dropped and counted).
pub const MAX_EVENTS: usize = 512;
/// Most parameter events in or out of one block.
pub const MAX_PARAM_EVENTS: usize = 256;

/// A MIDI 1.0 channel message at a frame of the block. SysEx is not passed
/// to plugins in 3b (Plan Ruling 10).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MidiEvent {
    /// Frame offset in the block.
    pub frame: u32,
    /// Status and data bytes; `len` of them are used.
    pub bytes: [u8; 3],
    /// 1 … 3.
    pub len: u8,
}

impl MidiEvent {
    /// A message from its bytes (at most 3 are kept).
    pub fn new(frame: u32, msg: &[u8]) -> MidiEvent {
        let mut bytes = [0u8; 3];
        for (d, s) in bytes.iter_mut().zip(msg) {
            *d = *s;
        }
        MidiEvent {
            frame,
            bytes,
            len: msg.len().min(3) as u8,
        }
    }

    /// The used bytes.
    pub fn as_slice(&self) -> &[u8] {
        self.bytes.get(..usize::from(self.len)).unwrap_or(&[])
    }
}

/// A parameter value set by the host for this block.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParamChange {
    /// Frame offset in the block.
    pub frame: u32,
    /// Parameter.
    pub id: ParamId,
    /// Plain value.
    pub value: f64,
}

impl Default for ParamChange {
    fn default() -> ParamChange {
        ParamChange {
            frame: 0,
            id: ParamId(0),
            value: 0.0,
        }
    }
}

/// A parameter event from the plugin (its own UI moved a control).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ParamEvent {
    /// The user grabbed the control.
    Begin(ParamId),
    /// The value changed.
    Value(ParamId, f64),
    /// The user let go.
    End(ParamId),
}

/// A fixed-capacity list of parameter events the plugin produced. // RT
#[derive(Debug, Clone)]
pub struct ParamOut {
    events: [ParamEvent; MAX_PARAM_EVENTS],
    len: usize,
}

impl Default for ParamOut {
    fn default() -> ParamOut {
        ParamOut {
            events: [ParamEvent::End(ParamId(0)); MAX_PARAM_EVENTS],
            len: 0,
        }
    }
}

impl ParamOut {
    /// Appends an event; false when full (the event is dropped). // RT
    pub fn push(&mut self, e: ParamEvent) -> bool {
        match self.events.get_mut(self.len) {
            Some(slot) => {
                *slot = e;
                self.len += 1;
                true
            }
            None => false,
        }
    }

    /// The events so far.
    pub fn as_slice(&self) -> &[ParamEvent] {
        self.events.get(..self.len).unwrap_or(&[])
    }

    /// Forgets every event. // RT
    pub fn clear(&mut self) {
        self.len = 0;
    }
}

/// One block for a processor. Channel buffers hold at least `frames`
/// samples; `outputs` holds every output port's channels, port after port
/// in [`crate::PluginInfo::outputs`] order.
pub struct ProcessCtx<'a> {
    /// Frames to process.
    pub frames: u32,
    /// Main input channels (empty for instruments).
    pub input: &'a [Box<[f32]>],
    /// Sidechain input channels, when the plugin has a sidechain port
    /// (silent when nothing is routed to it).
    pub sidechain: &'a [Box<[f32]>],
    /// Output channels of every port.
    pub outputs: &'a mut [Box<[f32]>],
    /// MIDI events sorted by frame.
    pub events: &'a [MidiEvent],
    /// Parameter changes from the host, sorted by frame.
    pub params_in: &'a [ParamChange],
    /// Parameter events from the plugin.
    pub params_out: &'a mut ParamOut,
    /// Transport facts at the block's first frame.
    pub transport: &'a BlockInfo,
}
VIZDAW_EOF
```

Create `crates/core-host/src/queue.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p crates/core-host/src
cat > crates/core-host/src/queue.rs <<'VIZDAW_EOF'
//! Wait-free parameter queues between the main thread and a processor: the
//! host's values go in, the plugin's gesture events come out. // RT (the
//! audio-side ends)

use crate::process::{ParamChange, ParamEvent};
use core_model::mixer::ParamId;
use rtrb::{Consumer, Producer, RingBuffer};

/// Capacity of each queue.
pub const PARAM_QUEUE: usize = 1024;

/// Main-thread end of the values going to a processor.
pub struct ParamSender(Producer<(ParamId, f64)>);
/// Audio-side end of the values going to a processor.
pub struct ParamReceiver(Consumer<(ParamId, f64)>);
/// Audio-side end of the plugin's parameter events.
pub struct GestureSender(Producer<ParamEvent>);
/// Main-thread end of the plugin's parameter events.
pub struct GestureReceiver(Consumer<ParamEvent>);

impl std::fmt::Debug for ParamSender {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ParamSender")
    }
}

/// Both queues of one processor: `(main-thread ends, audio-side ends)`.
pub fn param_queues() -> (
    (ParamSender, GestureReceiver),
    (ParamReceiver, GestureSender),
) {
    let (ptx, prx) = RingBuffer::new(PARAM_QUEUE);
    let (gtx, grx) = RingBuffer::new(PARAM_QUEUE);
    (
        (ParamSender(ptx), GestureReceiver(grx)),
        (ParamReceiver(prx), GestureSender(gtx)),
    )
}

impl ParamSender {
    /// Queues a value; false when the queue is full (try again later).
    pub fn send(&mut self, id: ParamId, value: f64) -> bool {
        self.0.push((id, value)).is_ok()
    }
}

impl ParamReceiver {
    /// Moves queued values into `out` (at frame 0) until it is full;
    /// returns how many. // RT
    pub fn drain_into(&mut self, out: &mut [ParamChange]) -> usize {
        let mut n = 0;
        for slot in out.iter_mut() {
            let Ok((id, value)) = self.0.pop() else {
                break;
            };
            *slot = ParamChange {
                frame: 0,
                id,
                value,
            };
            n += 1;
        }
        n
    }
}

impl GestureSender {
    /// Queues events; returns how many did not fit. // RT
    pub fn send_all(&mut self, events: &[ParamEvent]) -> usize {
        events.iter().filter(|e| self.0.push(**e).is_err()).count()
    }
}

impl GestureReceiver {
    /// Every queued event.
    pub fn drain(&mut self) -> Vec<ParamEvent> {
        let mut v = Vec::new();
        while let Ok(e) = self.0.pop() {
            v.push(e);
        }
        v
    }
}
VIZDAW_EOF
```

- [ ] **Step 4: Run the tests to see them pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-host --test types
```

Expected: 4 tests pass (`plugin_info_round_trips_as_json_and_finds_ports`, `midi_events_keep_up_to_three_bytes`, `param_out_is_bounded_and_never_allocates`, `queues_carry_values_in_and_gestures_out`).

- [ ] **Step 5: Check the whole workspace**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Expected: no formatting changes left, no clippy warnings, every test passes.

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-host/Cargo.toml \
  crates/core-host/src/error.rs \
  crates/core-host/src/info.rs \
  crates/core-host/src/instance.rs \
  crates/core-host/src/lib.rs \
  crates/core-host/src/process.rs \
  crates/core-host/src/queue.rs \
  crates/core-host/tests/types.rs \
  Cargo.lock
git commit -m "$(cat <<'MSG'
feat(core-host): plugin instance and processor traits, info types, parameter queues

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
MSG
)"
```


---

### Task 5: test-plugins: the in-repo CLAP test bundle

**Files:**
- Create: `crates/test-plugins/Cargo.toml`
- Create: `crates/test-plugins/src/bundle.rs`
- Create: `crates/test-plugins/src/lib.rs`
- Create: `crates/test-plugins/src/plugin.rs`
- Create: `crates/test-plugins/tests/bundle.rs`
- Lockfile: `Cargo.lock` (cargo updates it when dependencies change)

**Interfaces:**
- Consumes: `clack-plugin 0.2`, `clack-extensions 0.2` (plugin side).
- Produces: crate `test-plugins` (cdylib + rlib): `Kind::{Gain, Ducker, Synth4, Nan, Crasher, Hanger}` with `Kind::ALL` and `Kind::id()`, `PARAM_GAIN = 0`, `PARAM_LATENCY = 1`, `PARAM_WIGGLE = 2`, `bundle() -> PathBuf` (builds `target/test-plugins/VizDAW Test.clap`).

Ruling 19. One `clack-plugin` type behind six plugin ids: gain (stereo, Gain 0–2, Latency 0–48000 stepped — a change while active requests a restart and applies at the next activation — and Wiggle, which makes the plugin emit a Begin/Value/End gesture on Gain; its state is `VZTG` + gain + latency), ducker (main × (1 − min(1, peak(sidechain))) per block, sidechain on input port 1), synth4 (4 stereo outputs; a note on channel k plays a saw on output k, sample-exact, CLAP and MIDI note dialects), NaN (writes NaN into every block), crasher (null write in activate → SIGSEGV), hanger (never returns from activate). `bundle()` runs a nested `cargo build -p test-plugins --lib --target-dir target/test-plugins` once per process and assembles the macOS bundle (`Contents/MacOS/VizDAW Test`, `Info.plist` with `CFBundleShortVersionString` 1.0.0) or copies the `.so` elsewhere, swapping it in atomically. The crate's own test loads the bundle with `clack-host` and lists its descriptors.

- [ ] **Step 1: Write the failing tests**


Create `crates/test-plugins/tests/bundle.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p crates/test-plugins/tests
cat > crates/test-plugins/tests/bundle.rs <<'VIZDAW_EOF'
//! The bundle builds and exposes every test plugin through its factory.

use clack_host::prelude::*;
use test_plugins::{Kind, bundle};

#[test]
fn the_bundle_lists_every_test_plugin() {
    let path = bundle();
    assert!(path.exists(), "{path:?}");
    // SAFETY: the bundle is this crate's own clack-plugin binary.
    let entry = unsafe { PluginEntry::load(&path) }.unwrap();
    let factory = entry.get_plugin_factory().unwrap();
    let ids: Vec<String> = factory
        .plugin_descriptors()
        .filter_map(|d| d.id().map(|i| i.to_string_lossy().into_owned()))
        .collect();
    let want: Vec<String> = Kind::ALL.iter().map(|k| k.id().to_string()).collect();
    assert_eq!(ids, want);
}
VIZDAW_EOF
```

- [ ] **Step 2: Run them to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p test-plugins
```

Expected: `error: package ID specification `test-plugins` did not match any packages`.

- [ ] **Step 3: Implement**


Create `crates/test-plugins/Cargo.toml`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p crates/test-plugins
cat > crates/test-plugins/Cargo.toml <<'VIZDAW_EOF'
[package]
name = "test-plugins"
version = "0.1.0"
edition.workspace = true
license.workspace = true
rust-version.workspace = true
publish = false

[lib]
# `cdylib` is the CLAP binary; `rlib` lets tests call `bundle()`.
crate-type = ["cdylib", "rlib"]

[dependencies]
clack-plugin = "0.2"
clack-extensions = { version = "0.2", features = ["clack-plugin", "audio-ports", "note-ports", "params", "state", "latency"] }

[dev-dependencies]
clack-host = "0.2"
VIZDAW_EOF
```

Create `crates/test-plugins/src/bundle.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p crates/test-plugins/src
cat > crates/test-plugins/src/bundle.rs <<'VIZDAW_EOF'
//! Builds `VizDAW Test.clap` for tests: runs `cargo build -p test-plugins`
//! into its own target directory (so it never waits on the outer build's
//! lock) and wraps the library in a bundle (macOS) or copies it (elsewhere).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

/// The bundle's file name.
pub const BUNDLE_NAME: &str = "VizDAW Test.clap";

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."))
}

fn build() -> Result<PathBuf, String> {
    let root = workspace_root();
    let target = root.join("target").join("test-plugins");
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let status = Command::new(cargo)
        .current_dir(&root)
        .args([
            "build",
            "--quiet",
            "-p",
            "test-plugins",
            "--lib",
            "--target-dir",
        ])
        .arg(&target)
        .status()
        .map_err(|e| format!("cannot run cargo: {e}"))?;
    if !status.success() {
        return Err(format!("building test-plugins failed: {status}"));
    }
    let lib = target.join("debug").join(if cfg!(target_os = "macos") {
        "libtest_plugins.dylib"
    } else if cfg!(windows) {
        "test_plugins.dll"
    } else {
        "libtest_plugins.so"
    });
    let bundle = target.join(BUNDLE_NAME);
    // Build the new bundle beside the old one, then swap it in, so a
    // concurrent test process never sees a half-written bundle.
    let tmp = target.join(format!("{BUNDLE_NAME}.{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let _ = std::fs::remove_file(&tmp);
    if cfg!(target_os = "macos") {
        let macos = tmp.join("Contents").join("MacOS");
        std::fs::create_dir_all(&macos).map_err(|e| e.to_string())?;
        std::fs::copy(&lib, macos.join("VizDAW Test")).map_err(|e| e.to_string())?;
        std::fs::write(tmp.join("Contents").join("Info.plist"), INFO_PLIST)
            .map_err(|e| e.to_string())?;
    } else {
        std::fs::copy(&lib, &tmp).map_err(|e| e.to_string())?;
    }
    let _ = std::fs::remove_dir_all(&bundle);
    let _ = std::fs::remove_file(&bundle);
    std::fs::rename(&tmp, &bundle).map_err(|e| e.to_string())?;
    Ok(bundle)
}

/// The path of `VizDAW Test.clap`, built once per test process. Panics
/// (test helper) when the build fails.
pub fn bundle() -> PathBuf {
    static PATH: OnceLock<Result<PathBuf, String>> = OnceLock::new();
    match PATH.get_or_init(build) {
        Ok(p) => p.clone(),
        Err(e) => panic!("{e}"),
    }
}

const INFO_PLIST: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key><string>VizDAW Test</string>
  <key>CFBundleIdentifier</key><string>org.vizdaw.test-plugins</string>
  <key>CFBundleName</key><string>VizDAW Test</string>
  <key>CFBundlePackageType</key><string>BNDL</string>
  <key>CFBundleShortVersionString</key><string>1.0.0</string>
  <key>CFBundleVersion</key><string>1.0.0</string>
</dict>
</plist>
"#;
VIZDAW_EOF
```

Create `crates/test-plugins/src/lib.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p crates/test-plugins/src
cat > crates/test-plugins/src/lib.rs <<'VIZDAW_EOF'
//! In-repo CLAP plugins for host and scanner tests (spec §13), built with
//! `clack-plugin`. One binary, `VizDAW Test.clap`, exposes every kind:
//!
//! | id | what it does |
//! |---|---|
//! | `vizdaw.test.gain` | stereo gain (param 0) with a configurable latency (param 1, restarts); param 2 "Wiggle" makes it perform a UI-style gesture setting Gain; state round-trips both values |
//! | `vizdaw.test.ducker` | main × (1 − min(1, peak(sidechain))) per block; sidechain on input port 1 |
//! | `vizdaw.test.synth4` | instrument with 4 stereo outputs: notes on MIDI channel k sound on output k |
//! | `vizdaw.test.nan` | writes NaN into every block |
//! | `vizdaw.test.crasher` | crashes (SIGSEGV) when activated |
//! | `vizdaw.test.hanger` | never returns from activate |
//!
//! [`bundle`] builds the `.clap` bundle for tests.

mod bundle;
mod plugin;

pub use bundle::bundle;

use clack_plugin::entry::prelude::*;
use clack_plugin::prelude::*;
use plugin::TestPlugin;
use std::ffi::CStr;

/// What a test plugin does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Gain + latency + state + gestures.
    Gain,
    /// Sidechain ducker.
    Ducker,
    /// Four-output synth.
    Synth4,
    /// NaN emitter.
    Nan,
    /// Crashes on activate.
    Crasher,
    /// Hangs on activate.
    Hanger,
}

impl Kind {
    /// Every kind, in factory order.
    pub const ALL: [Kind; 6] = [
        Kind::Gain,
        Kind::Ducker,
        Kind::Synth4,
        Kind::Nan,
        Kind::Crasher,
        Kind::Hanger,
    ];

    /// The CLAP plugin id.
    pub fn id(self) -> &'static str {
        match self {
            Kind::Gain => "vizdaw.test.gain",
            Kind::Ducker => "vizdaw.test.ducker",
            Kind::Synth4 => "vizdaw.test.synth4",
            Kind::Nan => "vizdaw.test.nan",
            Kind::Crasher => "vizdaw.test.crasher",
            Kind::Hanger => "vizdaw.test.hanger",
        }
    }

    fn name(self) -> &'static str {
        match self {
            Kind::Gain => "Test Gain",
            Kind::Ducker => "Test Ducker",
            Kind::Synth4 => "Test Synth 4-Out",
            Kind::Nan => "Test NaN",
            Kind::Crasher => "Test Crasher",
            Kind::Hanger => "Test Hanger",
        }
    }

    fn descriptor(self) -> PluginDescriptor {
        use clack_plugin::plugin::features::{AUDIO_EFFECT, INSTRUMENT, STEREO, SYNTHESIZER};
        let d = PluginDescriptor::new(self.id(), self.name())
            .with_vendor("VizDAW")
            .with_version("1.0.0");
        match self {
            Kind::Synth4 => d.with_features([INSTRUMENT, SYNTHESIZER, STEREO]),
            _ => d.with_features([AUDIO_EFFECT, STEREO]),
        }
    }
}

/// Gain parameter id (0 … 2, default 1).
pub const PARAM_GAIN: u32 = 0;
/// Latency parameter id (0 … 48 000 frames, stepped, default 0).
pub const PARAM_LATENCY: u32 = 1;
/// "Wiggle": setting it to `v` makes the plugin perform a gesture that sets
/// Gain to `v`, as if a user dragged its knob.
pub const PARAM_WIGGLE: u32 = 2;

/// The entry: one factory exposing every [`Kind`].
pub struct TestEntry {
    factory: PluginFactoryWrapper<TestFactory>,
}

impl Entry for TestEntry {
    fn new(_bundle_path: Option<&CStr>) -> Result<Self, EntryLoadError> {
        Ok(TestEntry {
            factory: PluginFactoryWrapper::new(TestFactory {
                descriptors: Kind::ALL.map(Kind::descriptor),
            }),
        })
    }

    fn declare_factories<'a>(&'a self, builder: &mut EntryFactories<'a>) {
        builder.register_factory(&self.factory);
    }
}

struct TestFactory {
    descriptors: [PluginDescriptor; 6],
}

impl PluginFactoryImpl for TestFactory {
    fn plugin_count(&self) -> u32 {
        Kind::ALL.len() as u32
    }

    fn plugin_descriptor(&self, index: u32) -> Option<&PluginDescriptor> {
        self.descriptors.get(index as usize)
    }

    fn create_plugin<'a>(
        &'a self,
        host_info: HostInfo<'a>,
        plugin_id: &CStr,
    ) -> Option<PluginInstance<'a>> {
        let i = Kind::ALL
            .iter()
            .position(|k| k.id().as_bytes() == plugin_id.to_bytes())?;
        let kind = *Kind::ALL.get(i)?;
        let desc = self.descriptors.get(i)?;
        Some(PluginInstance::new::<TestPlugin>(
            host_info,
            desc,
            move |host| Ok(plugin::Shared::new(kind, host)),
            |host, shared| Ok(plugin::MainThread::new(host, shared)),
        ))
    }
}

clack_export_entry!(TestEntry);
VIZDAW_EOF
```

Create `crates/test-plugins/src/plugin.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p crates/test-plugins/src
cat > crates/test-plugins/src/plugin.rs <<'VIZDAW_EOF'
//! The one plugin type behind every [`Kind`]: its behaviour is chosen at
//! creation from the plugin id.

use crate::{Kind, PARAM_GAIN, PARAM_LATENCY, PARAM_WIGGLE};
use clack_extensions::audio_ports::{
    AudioPortFlags, AudioPortInfo, AudioPortInfoWriter, AudioPortType, PluginAudioPorts,
    PluginAudioPortsImpl,
};
use clack_extensions::latency::{PluginLatency, PluginLatencyImpl};
use clack_extensions::note_ports::{
    NoteDialect, NoteDialects, NotePortInfo, NotePortInfoWriter, PluginNotePorts,
    PluginNotePortsImpl,
};
use clack_extensions::params::{
    ParamDisplayWriter, ParamInfo, ParamInfoFlags, ParamInfoWriter, PluginAudioProcessorParams,
    PluginMainThreadParams, PluginParams,
};
use clack_extensions::state::{PluginState, PluginStateImpl};
use clack_plugin::events::event_types::{
    ParamGestureBeginEvent, ParamGestureEndEvent, ParamValueEvent,
};
use clack_plugin::events::spaces::CoreEventSpace;
use clack_plugin::prelude::*;
use clack_plugin::stream::{InputStream, OutputStream};
use std::ffi::CStr;
use std::fmt::Write as _;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

/// Largest latency the gain plugin accepts (frames).
const MAX_LATENCY: u32 = 48_000;

pub struct TestPlugin;

impl Plugin for TestPlugin {
    type AudioProcessor<'a> = Processor<'a>;
    type Shared<'a> = Shared<'a>;
    type MainThread<'a> = MainThread<'a>;

    fn declare_extensions(builder: &mut PluginExtensions<Self>, shared: Option<&Shared<'_>>) {
        let kind = shared.map(|s| s.kind);
        builder.register::<PluginAudioPorts>();
        if kind.is_none_or(|k| k == Kind::Gain) {
            builder
                .register::<PluginParams>()
                .register::<PluginState>()
                .register::<PluginLatency>();
        }
        if kind.is_none_or(|k| k == Kind::Synth4) {
            builder.register::<PluginNotePorts>();
        }
    }
}

/// State every thread may touch.
pub struct Shared<'a> {
    kind: Kind,
    host: HostSharedHandle<'a>,
    gain: AtomicU64,
    /// Latency asked for by the latency parameter.
    want_latency: AtomicU32,
    /// Latency in effect since the last activation.
    latency: AtomicU32,
    /// A Wiggle value waiting to become a gesture (f64 bits).
    wiggle: AtomicU64,
    wiggle_pending: AtomicBool,
}

impl<'a> Shared<'a> {
    pub fn new(kind: Kind, host: HostSharedHandle<'a>) -> Shared<'a> {
        Shared {
            kind,
            host,
            gain: AtomicU64::new(1.0f64.to_bits()),
            want_latency: AtomicU32::new(0),
            latency: AtomicU32::new(0),
            wiggle: AtomicU64::new(0),
            wiggle_pending: AtomicBool::new(false),
        }
    }

    fn gain(&self) -> f64 {
        f64::from_bits(self.gain.load(Ordering::Relaxed))
    }

    fn set_gain(&self, v: f64) {
        self.gain
            .store(v.clamp(0.0, 2.0).to_bits(), Ordering::Relaxed);
    }

    /// Applies one parameter change; `active` says whether a latency change
    /// needs a restart.
    fn set_param(&self, id: u32, value: f64, active: bool) {
        match id {
            PARAM_GAIN => self.set_gain(value),
            PARAM_LATENCY => {
                let l = value.round().clamp(0.0, f64::from(MAX_LATENCY)) as u32;
                self.want_latency.store(l, Ordering::Relaxed);
                if !active {
                    self.latency.store(l, Ordering::Relaxed);
                } else if l != self.latency.load(Ordering::Relaxed) {
                    self.host.request_restart();
                }
            }
            PARAM_WIGGLE => {
                self.wiggle.store(value.to_bits(), Ordering::Relaxed);
                self.wiggle_pending.store(true, Ordering::Release);
            }
            _ => {}
        }
    }

    fn param_value(&self, id: u32) -> Option<f64> {
        match id {
            PARAM_GAIN => Some(self.gain()),
            PARAM_LATENCY => Some(f64::from(self.want_latency.load(Ordering::Relaxed))),
            PARAM_WIGGLE => Some(f64::from_bits(self.wiggle.load(Ordering::Relaxed))),
            _ => None,
        }
    }

    /// Reads parameter events; returns true when Wiggle asks for a gesture.
    fn read_params(&self, events: &InputEvents, active: bool) {
        for e in events {
            if let Some(CoreEventSpace::ParamValue(p)) = e.as_core_event() {
                if let Some(id) = p.param_id() {
                    self.set_param(id.get(), p.value(), active);
                }
            }
        }
    }

    /// Emits the gesture a pending Wiggle asked for.
    fn write_gesture(&self, out: &mut OutputEvents) {
        if !self.wiggle_pending.swap(false, Ordering::Acquire) {
            return;
        }
        let v = f64::from_bits(self.wiggle.load(Ordering::Relaxed));
        self.set_gain(v);
        let id = ClapId::new(PARAM_GAIN);
        let _ = out.try_push(ParamGestureBeginEvent::new(0, id));
        let _ = out.try_push(ParamValueEvent::new(0, id, Pckn::match_all(), self.gain()));
        let _ = out.try_push(ParamGestureEndEvent::new(0, id));
    }
}

impl<'a> PluginShared<'a> for Shared<'a> {}

pub struct MainThread<'a> {
    shared: &'a Shared<'a>,
}

impl<'a> MainThread<'a> {
    pub fn new(_host: HostMainThreadHandle<'a>, shared: &'a Shared<'a>) -> MainThread<'a> {
        MainThread { shared }
    }
}

impl<'a> PluginMainThread<'a, Shared<'a>> for MainThread<'a> {}

fn port(id: u32, name: &[u8], main: bool) -> AudioPortInfo<'_> {
    AudioPortInfo {
        id: ClapId::new(id),
        name,
        channel_count: 2,
        flags: if main {
            AudioPortFlags::IS_MAIN
        } else {
            AudioPortFlags::empty()
        },
        port_type: Some(AudioPortType::STEREO),
        in_place_pair: None,
    }
}

impl PluginAudioPortsImpl for MainThread<'_> {
    fn count(&self, is_input: bool) -> u32 {
        match (self.shared.kind, is_input) {
            (Kind::Synth4, true) => 0,
            (Kind::Synth4, false) => 4,
            (Kind::Ducker, true) => 2,
            _ => 1,
        }
    }

    fn get(&self, index: u32, is_input: bool, writer: &mut AudioPortInfoWriter) {
        let info = match (self.shared.kind, is_input, index) {
            (Kind::Synth4, false, 0) => port(0, b"Out 1", true),
            (Kind::Synth4, false, 1) => port(1, b"Out 2", false),
            (Kind::Synth4, false, 2) => port(2, b"Out 3", false),
            (Kind::Synth4, false, 3) => port(3, b"Out 4", false),
            (Kind::Synth4, _, _) => return,
            (Kind::Ducker, true, 1) => port(1, b"Sidechain", false),
            (_, true, 0) => port(0, b"In", true),
            (_, false, 0) => port(0, b"Out", true),
            _ => return,
        };
        writer.set(&info);
    }
}

impl PluginNotePortsImpl for MainThread<'_> {
    fn count(&self, is_input: bool) -> u32 {
        u32::from(is_input && self.shared.kind == Kind::Synth4)
    }

    fn get(&self, index: u32, is_input: bool, writer: &mut NotePortInfoWriter) {
        if is_input && index == 0 {
            writer.set(&NotePortInfo {
                id: ClapId::new(0),
                name: b"Notes",
                supported_dialects: NoteDialects::CLAP | NoteDialects::MIDI,
                preferred_dialect: Some(NoteDialect::Clap),
            });
        }
    }
}

impl PluginMainThreadParams for MainThread<'_> {
    fn count(&self) -> u32 {
        3
    }

    fn get_info(&self, index: u32, info: &mut ParamInfoWriter) {
        let (id, name, max, def, flags): (u32, &[u8], f64, f64, ParamInfoFlags) = match index {
            0 => (
                PARAM_GAIN,
                b"Gain",
                2.0,
                1.0,
                ParamInfoFlags::IS_AUTOMATABLE,
            ),
            1 => (
                PARAM_LATENCY,
                b"Latency",
                f64::from(MAX_LATENCY),
                0.0,
                ParamInfoFlags::IS_STEPPED,
            ),
            2 => (PARAM_WIGGLE, b"Wiggle", 2.0, 0.0, ParamInfoFlags::empty()),
            _ => return,
        };
        info.set(&ParamInfo {
            id: ClapId::new(id),
            flags,
            cookie: Default::default(),
            name,
            module: b"",
            min_value: 0.0,
            max_value: max,
            default_value: def,
        });
    }

    fn get_value(&self, id: ClapId) -> Option<f64> {
        self.shared.param_value(id.get())
    }

    fn value_to_text(
        &self,
        _id: ClapId,
        value: f64,
        writer: &mut ParamDisplayWriter,
    ) -> std::fmt::Result {
        write!(writer, "{value:.3}")
    }

    fn text_to_value(&self, _id: ClapId, text: &CStr) -> Option<f64> {
        text.to_str().ok()?.trim().parse().ok()
    }

    fn flush(&self, input: &InputEvents, output: &mut OutputEvents) {
        self.shared.read_params(input, false);
        self.shared.write_gesture(output);
    }
}

impl PluginStateImpl for MainThread<'_> {
    fn save(&self, output: &mut OutputStream) -> Result<(), PluginError> {
        let mut bytes = Vec::with_capacity(16);
        bytes.extend_from_slice(b"VZTG");
        bytes.extend_from_slice(&self.shared.gain().to_le_bytes());
        bytes.extend_from_slice(
            &self
                .shared
                .want_latency
                .load(Ordering::Relaxed)
                .to_le_bytes(),
        );
        output.write_all(&bytes)?;
        Ok(())
    }

    fn load(&self, input: &mut InputStream) -> Result<(), PluginError> {
        let mut bytes = Vec::new();
        input.read_to_end(&mut bytes)?;
        if bytes.len() != 16 || !bytes.starts_with(b"VZTG") {
            return Err(PluginError::Message("not a Test Gain state"));
        }
        let mut g = [0u8; 8];
        g.copy_from_slice(&bytes[4..12]);
        let mut l = [0u8; 4];
        l.copy_from_slice(&bytes[12..16]);
        self.shared.set_gain(f64::from_le_bytes(g));
        self.shared
            .set_param(PARAM_LATENCY, f64::from(u32::from_le_bytes(l)), false);
        Ok(())
    }
}

impl PluginLatencyImpl for MainThread<'_> {
    fn get(&self) -> u32 {
        self.shared.latency.load(Ordering::Relaxed)
    }
}

/// One voice of the synth.
#[derive(Clone, Copy, Default)]
struct Voice {
    key: Option<u8>,
    phase: f32,
}

pub struct Processor<'a> {
    shared: &'a Shared<'a>,
    rate: f32,
    /// Gain's delay lines (one per channel) and write position.
    delay: [Vec<f32>; 2],
    pos: usize,
    /// Copies of the inputs (main L/R, sidechain L/R).
    scratch: [Vec<f32>; 4],
    voices: [Voice; 4],
}

impl<'a> PluginAudioProcessor<'a, Shared<'a>, MainThread<'a>> for Processor<'a> {
    fn activate(
        _host: HostAudioProcessorHandle<'a>,
        _main: &MainThread<'a>,
        shared: &'a Shared<'a>,
        cfg: PluginAudioConfiguration,
    ) -> Result<Self, PluginError> {
        match shared.kind {
            Kind::Crasher => {
                // SAFETY: none — this plugin exists to crash its host (a
                // null write raises SIGSEGV).
                unsafe { std::ptr::null_mut::<u8>().write_volatile(1) };
            }
            Kind::Hanger => loop {
                std::thread::sleep(std::time::Duration::from_secs(1));
            },
            _ => {}
        }
        let lat = shared.want_latency.load(Ordering::Relaxed);
        shared.latency.store(lat, Ordering::Relaxed);
        let max = cfg.max_frames_count as usize;
        Ok(Processor {
            shared,
            rate: cfg.sample_rate as f32,
            delay: [vec![0.0; lat as usize], vec![0.0; lat as usize]],
            pos: 0,
            scratch: [
                vec![0.0; max],
                vec![0.0; max],
                vec![0.0; max],
                vec![0.0; max],
            ],
            voices: [Voice::default(); 4],
        })
    }

    fn process(
        &mut self,
        _process: Process,
        mut audio: Audio,
        events: Events,
    ) -> Result<ProcessStatus, PluginError> {
        let n = audio.frames_count() as usize;
        if self.shared.kind == Kind::Synth4 {
            return self.synth(&mut audio, events, n);
        }
        self.shared.read_params(events.input, true);
        self.shared.write_gesture(events.output);
        // Copy the inputs first (the host may process in place).
        for (port, base) in [(0usize, 0usize), (1, 2)] {
            for ch in 0..2 {
                let dst = &mut self.scratch[base + ch][..n];
                match audio
                    .input_port(port)
                    .and_then(|p| p.channels().ok())
                    .and_then(|c| c.into_f32())
                    .and_then(|c| c.channel(ch as u32))
                {
                    Some(src) => dst.copy_from_slice(&src[..n]),
                    None => dst.fill(0.0),
                }
            }
        }
        let factor = match self.shared.kind {
            Kind::Gain => self.shared.gain() as f32,
            Kind::Ducker => {
                let peak = self.scratch[2][..n]
                    .iter()
                    .chain(&self.scratch[3][..n])
                    .fold(0.0f32, |m, x| m.max(x.abs()));
                1.0 - peak.min(1.0)
            }
            _ => 1.0,
        };
        let Some(mut out) = audio.output_port(0) else {
            return Ok(ProcessStatus::Continue);
        };
        let mut out = out
            .channels()?
            .into_f32()
            .ok_or(PluginError::Message("f32 only"))?;
        let lat = self.delay[0].len();
        for ch in 0..2usize {
            let Some(o) = out.channel_mut(ch as u32) else {
                continue;
            };
            let mut pos = self.pos;
            for (i, x) in o[..n].iter_mut().enumerate() {
                let v = self.scratch[ch][i] * factor;
                if lat == 0 {
                    *x = v;
                } else {
                    *x = self.delay[ch][pos];
                    self.delay[ch][pos] = v;
                    pos = (pos + 1) % lat;
                }
            }
            if self.shared.kind == Kind::Nan {
                o[0] = f32::NAN;
            }
        }
        if lat > 0 {
            self.pos = (self.pos + n) % lat;
        }
        Ok(ProcessStatus::Continue)
    }

    fn reset(&mut self) {
        for d in &mut self.delay {
            d.fill(0.0);
        }
        self.voices = [Voice::default(); 4];
    }
}

impl<'a> PluginAudioProcessorParams for Processor<'a> {
    fn flush(&mut self, input: &InputEvents, output: &mut OutputEvents) {
        self.shared.read_params(input, true);
        self.shared.write_gesture(output);
    }
}

impl Processor<'_> {
    /// Saw voices: channel k plays on output k; events are sample-exact.
    fn synth(
        &mut self,
        audio: &mut Audio,
        events: Events,
        n: usize,
    ) -> Result<ProcessStatus, PluginError> {
        let mut voices = self.voices;
        let rate = self.rate.max(1.0);
        // Render each output in turn, replaying the events per output.
        for (k, slot) in voices.iter_mut().enumerate() {
            let mut v = *slot;
            let mut buf = std::mem::take(&mut self.scratch[0]);
            let mut start = 0usize;
            let mut evs = events.input.iter().peekable();
            loop {
                let end = evs
                    .peek()
                    .map(|e| (e.header().time() as usize).min(n))
                    .unwrap_or(n);
                for x in &mut buf[start..end] {
                    *x = match v.key {
                        Some(key) => {
                            let f = 440.0 * 2f32.powf((f32::from(key) - 69.0) / 12.0);
                            v.phase = (v.phase + f / rate).fract();
                            0.25 * (2.0 * v.phase - 1.0)
                        }
                        None => 0.0,
                    };
                }
                start = end;
                let Some(e) = evs.next() else {
                    break;
                };
                let (chan, key, on) = match e.as_core_event() {
                    Some(CoreEventSpace::NoteOn(x)) => (
                        x.pckn().channel.into_specific(),
                        x.pckn().key.into_specific(),
                        true,
                    ),
                    Some(CoreEventSpace::NoteOff(x)) => (
                        x.pckn().channel.into_specific(),
                        x.pckn().key.into_specific(),
                        false,
                    ),
                    Some(CoreEventSpace::Midi(m)) => {
                        let [s, d1, d2] = m.data();
                        let on = s & 0xF0 == 0x90 && d2 > 0;
                        let off = s & 0xF0 == 0x80 || (s & 0xF0 == 0x90 && d2 == 0);
                        if !on && !off {
                            continue;
                        }
                        (Some(u16::from(s & 0x0F)), Some(u16::from(d1)), on)
                    }
                    _ => continue,
                };
                let (Some(chan), Some(key)) = (chan, key) else {
                    continue;
                };
                if usize::from(chan) % 4 != k {
                    continue;
                }
                let key = key.min(127) as u8;
                if on {
                    v = Voice {
                        key: Some(key),
                        phase: 0.0,
                    };
                } else if v.key == Some(key) {
                    v.key = None;
                }
            }
            *slot = v;
            if let Some(mut port) = audio.output_port(k) {
                if let Some(mut out) = port.channels()?.into_f32() {
                    for ch in 0..2 {
                        if let Some(o) = out.channel_mut(ch) {
                            o[..n].copy_from_slice(&buf[..n]);
                        }
                    }
                }
            }
            self.scratch[0] = buf;
        }
        self.voices = voices;
        Ok(ProcessStatus::Continue)
    }
}
VIZDAW_EOF
```

- [ ] **Step 4: Run the tests to see them pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p test-plugins
```

Expected: `the_bundle_lists_every_test_plugin` passes (the first run builds the bundle, a few seconds).

- [ ] **Step 5: Check the whole workspace**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Expected: no formatting changes left, no clippy warnings, every test passes.

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/test-plugins/Cargo.toml \
  crates/test-plugins/src/bundle.rs \
  crates/test-plugins/src/lib.rs \
  crates/test-plugins/src/plugin.rs \
  crates/test-plugins/tests/bundle.rs \
  Cargo.lock
git commit -m "$(cat <<'MSG'
test: in-repo CLAP test plugins (gain, ducker, 4-out synth, NaN, crasher, hanger)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
MSG
)"
```


---

### Task 6: CLAP host

**Files:**
- Create: `crates/core-host/src/clap/ffi.rs`
- Create: `crates/core-host/src/clap/handlers.rs`
- Create: `crates/core-host/src/clap/instance.rs`
- Create: `crates/core-host/src/clap/mod.rs`
- Create: `crates/core-host/src/clap/processor.rs`
- Create: `crates/core-host/tests/clap_host.rs`
- Modify: `crates/core-host/Cargo.toml`
- Modify: `crates/core-host/src/instance.rs`
- Modify: `crates/core-host/src/lib.rs`
- Lockfile: `Cargo.lock` (cargo updates it when dependencies change)

**Interfaces:**
- Consumes: Task 4's traits and types; Task 5's `test_plugins::bundle()` (dev-dependency); `clack-host 0.2`, `clack-extensions 0.2` host side.
- Produces: `core_host::clap::{ClapFormat, ClapInstance, ClapProcessor, list_bundle(&Path) -> Result<Vec<(PluginRef, PluginKind)>, HostError>, search_paths() -> Vec<PathBuf>, find_bundles(&[PathBuf]) -> Vec<PathBuf>}`; `ClapInstance::load(&Path, &str)`; `ClapFormat` implements `PluginFormat` (prepare checks the path, finish loads on the main thread).

`handlers.rs` implements clack's `HostHandlers` with the extensions spec §5 lists: plugin requests (`request_restart`, `request_callback`, latency/params/ports changes, `mark_dirty`, GUI closed) set atomics that `ClapInstance::idle` reads and clears on the main thread; thread-check follows Ruling 20. `instance.rs` reads ports (main, first non-main input as sidechain, aux), note ports (CLAP dialect or not), parameters, latency after each activation and tail; implements state through the `state` extension, inactive parameter flushes, `param_value`, the CLAP `gui` editor (Ruling 7) and `idle`. `processor.rs` copies the context's inputs into the plugin's input ports, merges parameter changes and MIDI into one sorted CLAP event list (notes as CLAP note events when the dialect allows, Ruling 10), passes a transport event, splits the flat output channels into ports without allocating, collects gesture events into `ParamOut` through an allocation-free `OutputEventBuffer`, starts processing on the first block and stops it when dropped. All `unsafe` (loading a bundle, handing a GUI its parent view) is in `ffi.rs`.

- [ ] **Step 1: Write the failing tests**


Create `crates/core-host/tests/clap_host.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p crates/core-host/tests
cat > crates/core-host/tests/clap_host.rs <<'VIZDAW_EOF'
//! The CLAP host against the in-repo test plugins: ports, parameters,
//! notes, multi-out, sidechain and errors.

use core_engine::backend::BlockInfo;
use core_engine::graph::ProcessStatus;
use core_host::clap::{ClapFormat, list_bundle};
use core_host::{
    AudioProcessor, LoadRequest, MidiEvent, ParamChange, ParamOut, PluginFormat, PluginInstance,
    PluginKind, PortRole, ProcessCtx,
};
use core_model::mixer::{ParamId, PluginFormat as Format, PluginRef};

const N: usize = 64;

pub fn load(id: &str) -> Box<dyn PluginInstance> {
    let mut f = ClapFormat::new();
    let req = LoadRequest {
        plugin: PluginRef {
            format: Format::Clap,
            id: id.into(),
            name: id.into(),
            vendor: String::new(),
            version: String::new(),
        },
        path: Some(test_plugins::bundle()),
        sample_rate: 48_000.0,
        max_frames: 512,
    };
    let prepared = (f.prepare(&req))().unwrap();
    f.finish(&req, prepared).unwrap()
}

fn bufs(channels: usize, v: f32) -> Vec<Box<[f32]>> {
    (0..channels)
        .map(|_| vec![v; 512].into_boxed_slice())
        .collect()
}

/// One block of `N` frames.
struct Block {
    input: Vec<Box<[f32]>>,
    sidechain: Vec<Box<[f32]>>,
    outputs: Vec<Box<[f32]>>,
    events: Vec<MidiEvent>,
    params: Vec<ParamChange>,
    out: ParamOut,
}

impl Block {
    fn new(inputs: usize, outputs: usize, level: f32) -> Block {
        Block {
            input: bufs(inputs, level),
            sidechain: Vec::new(),
            outputs: bufs(outputs, 9.0),
            events: Vec::new(),
            params: Vec::new(),
            out: ParamOut::default(),
        }
    }

    fn run(&mut self, p: &mut dyn AudioProcessor) -> ProcessStatus {
        let info = BlockInfo::default();
        let mut ctx = ProcessCtx {
            frames: N as u32,
            input: &mut self.input,
            sidechain: &mut self.sidechain,
            outputs: &mut self.outputs,
            events: &self.events,
            params_in: &self.params,
            params_out: &mut self.out,
            transport: &info,
        };
        p.process(&mut ctx)
    }
}

#[test]
fn the_bundle_lists_kinds() {
    let listed = list_bundle(&test_plugins::bundle()).unwrap();
    let synth = listed
        .iter()
        .find(|(p, _)| p.id == "vizdaw.test.synth4")
        .unwrap();
    assert_eq!(synth.1, PluginKind::Instrument);
    assert_eq!(synth.0.vendor, "VizDAW");
    assert_eq!(synth.0.version, "1.0.0");
    let gain = listed
        .iter()
        .find(|(p, _)| p.id == "vizdaw.test.gain")
        .unwrap();
    assert_eq!(gain.1, PluginKind::Effect);
}

#[test]
fn gain_reports_its_ports_and_parameters_and_processes() {
    let mut inst = load("vizdaw.test.gain");
    let info = inst.info().clone();
    assert_eq!(info.kind, PluginKind::Effect);
    assert_eq!(info.inputs.len(), 1);
    assert_eq!(info.inputs[0].role, PortRole::Main);
    assert_eq!(info.outputs[0].channels, 2);
    let names: Vec<&str> = info.params.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["Gain", "Latency", "Wiggle"]);
    assert!(info.params[1].stepped);
    assert_eq!(inst.param_value(ParamId(0)), Some(1.0));
    let mut p = inst.activate(48_000.0, 512).unwrap();
    assert!(inst.is_active());
    let mut b = Block::new(2, 2, 0.5);
    assert_eq!(b.run(p.as_mut()), ProcessStatus::Ok);
    assert!(b.outputs.iter().all(|c| c[..N].iter().all(|x| *x == 0.5)));
    // A parameter change in the block applies to it.
    b.params.push(ParamChange {
        frame: 0,
        id: ParamId(0),
        value: 0.25,
    });
    b.run(p.as_mut());
    assert!(b.outputs[1][..N].iter().all(|x| *x == 0.125));
    inst.deactivate(p);
    assert!(!inst.is_active());
    assert_eq!(inst.param_value(ParamId(0)), Some(0.25));
}

#[test]
fn the_synth_renders_each_channel_on_its_own_output() {
    let mut inst = load("vizdaw.test.synth4");
    let info = inst.info().clone();
    assert_eq!(info.kind, PluginKind::Instrument);
    assert!(info.note_input && info.inputs.is_empty());
    assert_eq!(info.outputs.len(), 4);
    assert_eq!(info.output_channels(), 8);
    let mut p = inst.activate(48_000.0, 512).unwrap();
    let mut b = Block::new(0, 8, 0.0);
    b.events.push(MidiEvent::new(10, &[0x92, 69, 100]));
    b.run(p.as_mut());
    // Output 2 (channels 4, 5) sounds from frame 10; the others are silent.
    for (ch, buf) in b.outputs.iter().enumerate() {
        let loud = buf[..N].iter().any(|x| *x != 0.0);
        assert_eq!(loud, ch == 4 || ch == 5, "channel {ch}");
    }
    assert!(b.outputs[4][..10].iter().all(|x| *x == 0.0));
    assert!(b.outputs[4][11] != 0.0);
    inst.deactivate(p);
}

#[test]
fn the_ducker_follows_its_sidechain() {
    let mut inst = load("vizdaw.test.ducker");
    assert_eq!(inst.info().sidechain_input(), Some(1));
    let mut p = inst.activate(48_000.0, 512).unwrap();
    let mut b = Block::new(2, 2, 0.8);
    b.sidechain = bufs(2, 0.5);
    b.run(p.as_mut());
    assert!(b.outputs[0][..N].iter().all(|x| (*x - 0.4).abs() < 1e-6));
    // Nothing routed: the sidechain port gets silence.
    b.sidechain.clear();
    b.run(p.as_mut());
    assert!(b.outputs[0][..N].iter().all(|x| (*x - 0.8).abs() < 1e-6));
    inst.deactivate(p);
}

#[test]
fn the_nan_plugin_writes_nan() {
    let mut inst = load("vizdaw.test.nan");
    let mut p = inst.activate(48_000.0, 512).unwrap();
    let mut b = Block::new(2, 2, 0.1);
    assert_eq!(b.run(p.as_mut()), ProcessStatus::Ok);
    assert!(b.outputs[0][0].is_nan());
    inst.deactivate(p);
}

#[test]
fn unknown_ids_and_missing_bundles_are_errors() {
    let mut f = ClapFormat::new();
    let mut req = LoadRequest {
        plugin: PluginRef::from_legacy("vizdaw.test.nope"),
        path: Some(test_plugins::bundle()),
        sample_rate: 48_000.0,
        max_frames: 512,
    };
    let prepared = (f.prepare(&req))().unwrap();
    let Err(err) = f.finish(&req, prepared) else {
        panic!("an unknown id loaded");
    };
    assert!(err.to_string().contains("not found"), "{err}");
    req.path = Some("/nonexistent/X.clap".into());
    assert!((f.prepare(&req))().is_err());
    req.path = None;
    assert!((f.prepare(&req))().is_err());
}

#[test]
fn activating_twice_is_refused() {
    let mut inst = load("vizdaw.test.gain");
    let p = inst.activate(48_000.0, 512).unwrap();
    assert!(inst.activate(48_000.0, 512).is_err());
    inst.deactivate(p);
    let p = inst.activate(44_100.0, 256).unwrap();
    inst.deactivate(p);
}

#[test]
fn state_round_trips_and_foreign_state_is_rejected() {
    let mut a = load("vizdaw.test.gain");
    a.set_params(&[(ParamId(0), 0.3), (ParamId(1), 64.0)])
        .unwrap();
    let state = a.save_state().unwrap();
    let mut b = load("vizdaw.test.gain");
    b.load_state(&state).unwrap();
    assert_eq!(b.param_value(ParamId(0)), Some(0.3));
    let p = b.activate(48_000.0, 512).unwrap();
    assert_eq!(b.info().latency, 64, "latency is read at activation");
    b.deactivate(p);
    let err = b.load_state(b"junk").unwrap_err();
    assert!(
        matches!(err, core_host::HostError::StateRejected(_)),
        "{err}"
    );
    assert!(
        load("vizdaw.test.nan").save_state().is_err(),
        "no state extension"
    );
}

#[test]
fn ui_gestures_come_out_of_process() {
    let mut inst = load("vizdaw.test.gain");
    let mut p = inst.activate(48_000.0, 512).unwrap();
    let mut b = Block::new(2, 2, 1.0);
    // "Wiggle" makes the plugin act as if its Gain knob were dragged.
    b.params.push(ParamChange {
        frame: 0,
        id: ParamId(2),
        value: 0.7,
    });
    b.run(p.as_mut());
    use core_host::ParamEvent::{Begin, End, Value};
    assert_eq!(
        b.out.as_slice(),
        &[Begin(ParamId(0)), Value(ParamId(0), 0.7), End(ParamId(0))]
    );
    inst.deactivate(p);
}

#[test]
fn a_latency_change_asks_for_a_restart() {
    let mut inst = load("vizdaw.test.gain");
    let mut p = inst.activate(48_000.0, 512).unwrap();
    assert_eq!(inst.info().latency, 0);
    assert!(!inst.idle().restart);
    let mut b = Block::new(2, 2, 1.0);
    b.params.push(ParamChange {
        frame: 0,
        id: ParamId(1),
        value: 128.0,
    });
    b.run(p.as_mut());
    assert!(inst.idle().restart);
    assert!(!inst.idle().restart, "read once");
    inst.deactivate(p);
    let p = inst.activate(48_000.0, 512).unwrap();
    assert_eq!(inst.info().latency, 128);
    inst.deactivate(p);
}

#[test]
fn plugins_without_a_gui_have_no_editor() {
    let mut inst = load("vizdaw.test.gain");
    assert!(inst.editor().is_none());
}
VIZDAW_EOF
```

- [ ] **Step 2: Run them to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-host --test clap_host
```

Expected: `unresolved import `core_host::clap``.

- [ ] **Step 3: Implement**


Create `crates/core-host/src/clap/ffi.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p crates/core-host/src/clap
cat > crates/core-host/src/clap/ffi.rs <<'VIZDAW_EOF'
//! The CLAP host's `unsafe` calls: loading a bundle and handing a plugin
//! its parent view.

use crate::error::HostError;
use clack_extensions::gui::{PluginGui, Window};
use clack_host::prelude::*;
use std::ffi::c_void;
use std::path::Path;

/// Loads (or reuses) the entry of the bundle at `path`.
pub(crate) fn load_entry(path: &Path) -> Result<PluginEntry, HostError> {
    // SAFETY: loading a plugin binary runs its initializers; plugins are
    // untrusted code by design (spec §2), which is why the scanner loads
    // unknown bundles in a separate process first. clack keeps the library
    // loaded while the entry or any instance lives.
    unsafe { PluginEntry::load(path) }
        .map_err(|e| HostError::LoadFailed(format!("{}: {e}", path.display())))
}

/// Embeds the plugin's GUI in the `NSView` at `parent`.
pub(crate) fn set_cocoa_parent(
    gui: &PluginGui,
    plugin: &PluginMainThreadHandle,
    parent: *mut c_void,
) -> Result<(), HostError> {
    if parent.is_null() {
        return Err(HostError::EditorUnavailable("no parent view".into()));
    }
    // SAFETY: the caller (the app, through `PluginEditor::open`) passes a
    // live `NSView *` that outlives the editor; the plugin only adds a
    // subview to it.
    let window = unsafe { Window::from_cocoa_nsview(parent) };
    // SAFETY: as above; the window handle is valid for this call.
    unsafe { gui.set_parent(plugin, window) }
        .map_err(|e| HostError::EditorUnavailable(format!("{e:?}")))
}
VIZDAW_EOF
```

Create `crates/core-host/src/clap/handlers.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p crates/core-host/src/clap
cat > crates/core-host/src/clap/handlers.rs <<'VIZDAW_EOF'
//! VizDAW's CLAP host callbacks (clack `HostHandlers`): the plugin's
//! requests land in atomics that `ClapInstance::idle` reads on the main
//! thread; thread-check answers from the main thread's ID and a
//! thread-local "inside an audio call" flag.

use clack_extensions::audio_ports::{
    AudioPortRescanFlags, HostAudioPorts, HostAudioPortsImpl, PluginAudioPorts,
};
use clack_extensions::gui::{GuiSize, HostGui, HostGuiImpl, PluginGui};
use clack_extensions::latency::{HostLatency, HostLatencyImpl, PluginLatency};
use clack_extensions::log::{HostLog, HostLogImpl, LogSeverity};
use clack_extensions::note_ports::{
    HostNotePorts, HostNotePortsImpl, NoteDialects, NotePortRescanFlags, PluginNotePorts,
};
use clack_extensions::params::{
    HostParams, HostParamsImplMainThread, HostParamsImplShared, ParamClearFlags, ParamRescanFlags,
    PluginParams,
};
use clack_extensions::state::{HostState, HostStateImpl, PluginState};
use clack_extensions::tail::{HostTail, HostTailImpl, PluginTail};
use clack_extensions::thread_check::{HostThreadCheck, HostThreadCheckImpl};
use clack_host::prelude::*;
use std::cell::Cell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::thread::ThreadId;

thread_local! {
    static IN_AUDIO: Cell<bool> = const { Cell::new(false) };
}

/// Runs `f` as an `[audio-thread]` call: thread-check reports this thread
/// as the audio thread (and not the main thread) meanwhile. // RT
pub(crate) fn audio_call<T>(f: impl FnOnce() -> T) -> T {
    let was = IN_AUDIO.try_with(|c| c.replace(true)).unwrap_or(false);
    let r = f();
    let _ = IN_AUDIO.try_with(|c| c.set(was));
    r
}

fn in_audio() -> bool {
    IN_AUDIO.try_with(Cell::get).unwrap_or(false)
}

/// What the plugin asked for, read and cleared on the main thread.
#[derive(Debug, Default)]
pub(crate) struct Flags {
    pub restart: AtomicBool,
    pub callback: AtomicBool,
    pub latency: AtomicBool,
    pub params: AtomicBool,
    pub state_dirty: AtomicBool,
    pub ports: AtomicBool,
    pub gui_closed: AtomicBool,
}

impl Flags {
    pub(crate) fn take(flag: &AtomicBool) -> bool {
        flag.swap(false, Ordering::AcqRel)
    }
}

/// The plugin extensions VizDAW uses, queried once while it initializes.
#[derive(Clone, Copy, Default)]
pub(crate) struct PluginExts {
    pub audio_ports: Option<PluginAudioPorts>,
    pub note_ports: Option<PluginNotePorts>,
    pub params: Option<PluginParams>,
    pub state: Option<PluginState>,
    pub latency: Option<PluginLatency>,
    pub tail: Option<PluginTail>,
    pub gui: Option<PluginGui>,
}

/// `[thread-safe]` host data.
pub(crate) struct Shared {
    pub flags: Arc<Flags>,
    pub exts: OnceLock<PluginExts>,
    main: ThreadId,
    name: String,
}

impl Shared {
    pub(crate) fn new(flags: Arc<Flags>, name: String) -> Shared {
        Shared {
            flags,
            exts: OnceLock::new(),
            main: std::thread::current().id(),
            name,
        }
    }
}

impl<'a> SharedHandler<'a> for Shared {
    fn initializing(&self, instance: InitializingPluginHandle<'a>) {
        let _ = self.exts.set(PluginExts {
            audio_ports: instance.get_extension(),
            note_ports: instance.get_extension(),
            params: instance.get_extension(),
            state: instance.get_extension(),
            latency: instance.get_extension(),
            tail: instance.get_extension(),
            gui: instance.get_extension(),
        });
    }

    fn request_restart(&self) {
        self.flags.restart.store(true, Ordering::Release);
    }

    fn request_process(&self) {
        // VizDAW processes every active instance every block.
    }

    fn request_callback(&self) {
        self.flags.callback.store(true, Ordering::Release);
    }
}

impl HostLogImpl for Shared {
    fn log(&self, severity: LogSeverity, message: &str) {
        // Never do I/O inside an audio call (spec §2): drop those.
        if !in_audio() {
            eprintln!("[{}] {severity}: {message}", self.name);
        }
    }
}

impl HostThreadCheckImpl for Shared {
    fn is_main_thread(&self) -> bool {
        !in_audio() && std::thread::current().id() == self.main
    }

    fn is_audio_thread(&self) -> bool {
        in_audio()
    }
}

impl HostParamsImplShared for Shared {
    fn request_flush(&self) {
        // Parameters reach active plugins through `process`, which runs
        // every block.
    }
}

impl HostGuiImpl for Shared {
    fn resize_hints_changed(&self) {}

    fn request_resize(&self, _size: GuiSize) -> Result<(), HostError> {
        Err(HostError::Message("the host window decides its size"))
    }

    fn request_show(&self) -> Result<(), HostError> {
        Ok(())
    }

    fn request_hide(&self) -> Result<(), HostError> {
        Ok(())
    }

    fn closed(&self, _was_destroyed: bool) {
        self.flags.gui_closed.store(true, Ordering::Release);
    }
}

/// `[main-thread]` host data.
pub(crate) struct MainThread<'a> {
    shared: &'a Shared,
}

impl<'a> MainThread<'a> {
    pub(crate) fn new(shared: &'a Shared) -> MainThread<'a> {
        MainThread { shared }
    }
}

impl<'a> MainThreadHandler<'a> for MainThread<'a> {}

impl HostLatencyImpl for MainThread<'_> {
    fn changed(&self) {
        self.shared.flags.latency.store(true, Ordering::Release);
    }
}

impl HostParamsImplMainThread for MainThread<'_> {
    fn rescan(&self, _flags: ParamRescanFlags) {
        self.shared.flags.params.store(true, Ordering::Release);
    }

    fn clear(&self, _id: ClapId, _flags: ParamClearFlags) {}
}

impl HostStateImpl for MainThread<'_> {
    fn mark_dirty(&self) {
        self.shared.flags.state_dirty.store(true, Ordering::Release);
    }
}

impl HostAudioPortsImpl for MainThread<'_> {
    fn is_rescan_flag_supported(&self, _flag: AudioPortRescanFlags) -> bool {
        true
    }

    fn rescan(&self, _flags: AudioPortRescanFlags) {
        self.shared.flags.ports.store(true, Ordering::Release);
    }
}

impl HostNotePortsImpl for MainThread<'_> {
    fn supported_dialects(&self) -> NoteDialects {
        NoteDialects::CLAP | NoteDialects::MIDI
    }

    fn rescan(&self, _flags: NotePortRescanFlags) {
        self.shared.flags.ports.store(true, Ordering::Release);
    }
}

/// `[audio-thread]` host data.
pub(crate) struct AudioThread;

impl AudioProcessorHandler<'_> for AudioThread {}

impl HostTailImpl for AudioThread {
    fn changed(&mut self) {
        // Read again at the next activation; nothing to do on the audio
        // thread.
    }
}

/// The handler set.
pub(crate) struct VizHost;

impl HostHandlers for VizHost {
    type Shared<'a> = Shared;
    type MainThread<'a> = MainThread<'a>;
    type AudioProcessor<'a> = AudioThread;

    fn declare_extensions(builder: &mut HostExtensions<Self>, _shared: &Shared) {
        builder
            .register::<HostLog>()
            .register::<HostThreadCheck>()
            .register::<HostLatency>()
            .register::<HostParams>()
            .register::<HostState>()
            .register::<HostAudioPorts>()
            .register::<HostNotePorts>()
            .register::<HostTail>()
            .register::<HostGui>();
    }
}
VIZDAW_EOF
```

Create `crates/core-host/src/clap/instance.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p crates/core-host/src/clap
cat > crates/core-host/src/clap/instance.rs <<'VIZDAW_EOF'
//! A CLAP plugin instance on the main thread.

use super::ffi;
use super::handlers::{AudioThread, Flags, MainThread, PluginExts, Shared, VizHost};
use super::processor::ClapProcessor;
use crate::error::HostError;
use crate::info::{AudioPortInfo, ParamInfo, PluginInfo, PluginKind, PortRole, Tail};
use crate::instance::{AudioProcessor, InstanceEvents, ParentView, PluginEditor, PluginInstance};
use clack_extensions::audio_ports::{AudioPortFlags, AudioPortInfoBuffer};
use clack_extensions::gui::{GuiApiType, GuiConfiguration, GuiSize};
use clack_extensions::note_ports::{NoteDialects, NotePortInfoBuffer};
use clack_extensions::params::{ParamInfoBuffer, ParamInfoFlags};
use clack_extensions::tail::TailLength;
use clack_host::events::event_types::ParamValueEvent;
use clack_host::prelude::*;
use core_model::mixer::ParamId;
use std::ffi::CString;
use std::path::Path;
use std::sync::Arc;

/// A loaded CLAP plugin (main-thread half).
pub struct ClapInstance {
    inst: clack_host::plugin::PluginInstance<VizHost>,
    flags: Arc<Flags>,
    info: PluginInfo,
    clap_notes: bool,
    gui_open: bool,
}

impl std::fmt::Debug for ClapInstance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClapInstance")
            .field("plugin", &self.info.plugin.id)
            .finish_non_exhaustive()
    }
}

fn text(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(bytes.get(..end).unwrap_or(&[])).into_owned()
}

impl ClapInstance {
    /// Creates plugin `id` from the bundle at `path` (main thread).
    pub fn load(path: &Path, id: &str) -> Result<ClapInstance, HostError> {
        let entry = ffi::load_entry(path)?;
        let listed = super::list_bundle(path)?;
        let (plugin, kind) = listed
            .into_iter()
            .find(|(p, _)| p.id == id)
            .ok_or_else(|| HostError::NotFound(format!("{id} in {}", path.display())))?;
        let cid = CString::new(id).map_err(|e| HostError::LoadFailed(e.to_string()))?;
        let host = HostInfo::new(
            "VizDAW",
            "VizDAW",
            "https://github.com/vizdaw",
            env!("CARGO_PKG_VERSION"),
        )
        .map_err(|e| HostError::LoadFailed(e.to_string()))?;
        let flags = Arc::new(Flags::default());
        let shared_flags = Arc::clone(&flags);
        let name = plugin.name.clone();
        let inst = clack_host::plugin::PluginInstance::<VizHost>::new(
            move |_| Shared::new(shared_flags, name),
            |shared| MainThread::new(shared),
            &entry,
            &cid,
            &host,
        )
        .map_err(|e| HostError::LoadFailed(format!("{id}: {e}")))?;
        let mut me = ClapInstance {
            inst,
            flags,
            info: PluginInfo {
                plugin,
                kind,
                inputs: Vec::new(),
                outputs: Vec::new(),
                note_input: false,
                params: Vec::new(),
                latency: 0,
                tail: Tail::None,
            },
            clap_notes: false,
            gui_open: false,
        };
        me.read_info();
        Ok(me)
    }

    fn exts(&self) -> PluginExts {
        self.inst
            .access_shared_handler(|s| s.exts.get().copied().unwrap_or_default())
    }

    /// Reads ports, note ports and parameters (main thread).
    fn read_info(&mut self) {
        let exts = self.exts();
        let h = self.inst.plugin_handle();
        let mut inputs = Vec::new();
        let mut outputs = Vec::new();
        if let Some(ap) = exts.audio_ports {
            let mut buf = AudioPortInfoBuffer::new();
            for is_input in [true, false] {
                let mut sidechain_taken = false;
                for i in 0..ap.count(&h, is_input) {
                    let Some(p) = ap.get(&h, i, is_input, &mut buf) else {
                        continue;
                    };
                    let role = if p.flags.contains(AudioPortFlags::IS_MAIN) {
                        PortRole::Main
                    } else if is_input && !sidechain_taken {
                        sidechain_taken = true;
                        PortRole::Sidechain
                    } else {
                        PortRole::Aux
                    };
                    let port = AudioPortInfo {
                        name: text(p.name),
                        channels: u16::try_from(p.channel_count).unwrap_or(u16::MAX),
                        role,
                    };
                    if is_input {
                        inputs.push(port);
                    } else {
                        outputs.push(port);
                    }
                }
            }
        }
        let mut note_input = false;
        let mut clap_notes = false;
        if let Some(np) = exts.note_ports {
            if np.count(&h, true) > 0 {
                note_input = true;
                let mut buf = NotePortInfoBuffer::new();
                clap_notes = np
                    .get(&h, 0, true, &mut buf)
                    .is_some_and(|p| p.supported_dialects.contains(NoteDialects::CLAP));
            }
        }
        let mut params = Vec::new();
        if let Some(pe) = exts.params {
            let mut buf = ParamInfoBuffer::new();
            for i in 0..pe.count(&h) {
                if let Some(p) = pe.get_info(&h, i, &mut buf) {
                    params.push(ParamInfo {
                        id: ParamId(u64::from(p.id.get())),
                        name: text(p.name),
                        module: text(p.module),
                        min: p.min_value,
                        max: p.max_value,
                        default: p.default_value,
                        stepped: p.flags.contains(ParamInfoFlags::IS_STEPPED),
                        automatable: p.flags.contains(ParamInfoFlags::IS_AUTOMATABLE),
                        hidden: p.flags.contains(ParamInfoFlags::IS_HIDDEN),
                    });
                }
            }
        }
        if note_input && inputs.is_empty() {
            self.info.kind = PluginKind::Instrument;
        }
        self.info.inputs = inputs;
        self.info.outputs = outputs;
        self.info.note_input = note_input;
        self.info.params = params;
        self.clap_notes = clap_notes;
    }
}

fn clap_id(id: ParamId) -> Option<ClapId> {
    u32::try_from(id.0).ok().and_then(ClapId::from_raw)
}

impl PluginInstance for ClapInstance {
    fn info(&self) -> &PluginInfo {
        &self.info
    }

    fn activate(
        &mut self,
        sample_rate: f64,
        max_frames: u32,
    ) -> Result<Box<dyn AudioProcessor>, HostError> {
        if self.inst.is_active() {
            return Err(HostError::ActivateFailed("already active".into()));
        }
        // Ports may have changed while inactive.
        self.read_info();
        let stopped = self
            .inst
            .activate(
                |_, _| AudioThread,
                PluginAudioConfiguration {
                    sample_rate,
                    min_frames_count: 1,
                    max_frames_count: max_frames.max(1),
                },
            )
            .map_err(|e| HostError::ActivateFailed(e.to_string()))?;
        let exts = self.exts();
        self.info.latency = exts
            .latency
            .map_or(0, |l| l.get(&self.inst.plugin_handle()));
        let mut proc = PluginAudioProcessor::from(stopped);
        self.info.tail = match exts.tail.map(|t| t.get(&proc.plugin_handle())) {
            Some(TailLength::Finite(n)) if n > 0 => Tail::Frames(n),
            Some(TailLength::Infinite) => Tail::Infinite,
            _ => Tail::None,
        };
        Ok(Box::new(ClapProcessor::new(
            proc,
            &self.info,
            max_frames.max(1),
            self.clap_notes,
        )))
    }

    fn deactivate(&mut self, processor: Box<dyn AudioProcessor>) {
        // Dropping the processor stops processing (as an audio-thread
        // call); then the plugin is deactivated here.
        drop(processor);
        let _ = self.inst.try_deactivate();
    }

    fn is_active(&self) -> bool {
        self.inst.is_active()
    }

    fn save_state(&mut self) -> Result<Vec<u8>, HostError> {
        let state = self
            .exts()
            .state
            .ok_or_else(|| HostError::StateRejected("the plugin has no state".into()))?;
        let mut out = Vec::new();
        state
            .save(&self.inst.plugin_handle(), &mut out)
            .map_err(|e| HostError::StateRejected(e.to_string()))?;
        Ok(out)
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), HostError> {
        let state = self
            .exts()
            .state
            .ok_or_else(|| HostError::StateRejected("the plugin has no state".into()))?;
        let mut reader = data;
        state
            .load(&self.inst.plugin_handle(), &mut reader)
            .map_err(|e| HostError::StateRejected(e.to_string()))
    }

    fn set_params(&mut self, values: &[(ParamId, f64)]) -> Result<(), HostError> {
        let Some(params) = self.exts().params else {
            return Ok(());
        };
        let mut events = EventBuffer::with_capacity(values.len());
        for (id, v) in values {
            if let Some(cid) = clap_id(*id) {
                events.push(&ParamValueEvent::new(0, cid, Pckn::match_all(), *v));
            }
        }
        let mut out = EventBuffer::new();
        let mut h = self.inst.inactive_plugin_handle().ok_or_else(|| {
            HostError::StateRejected(
                "parameters of an active plugin go through its processor".into(),
            )
        })?;
        params.flush(&mut h, &events.as_input(), &mut out.as_output());
        Ok(())
    }

    fn param_value(&mut self, id: ParamId) -> Option<f64> {
        let params = self.exts().params?;
        params.get_value(&self.inst.plugin_handle(), clap_id(id)?)
    }

    fn editor(&mut self) -> Option<&mut dyn PluginEditor> {
        let gui = self.exts().gui?;
        let cfg = GuiConfiguration {
            api_type: GuiApiType::default_for_current_platform()?,
            is_floating: false,
        };
        if !gui.is_api_supported(&self.inst.plugin_handle(), cfg) {
            return None;
        }
        Some(self)
    }

    fn idle(&mut self) -> InstanceEvents {
        if Flags::take(&self.flags.callback) {
            self.inst.call_on_main_thread_callback();
        }
        let ports = Flags::take(&self.flags.ports);
        if ports && !self.inst.is_active() {
            self.read_info();
        }
        if Flags::take(&self.flags.gui_closed) {
            self.gui_open = false;
        }
        InstanceEvents {
            restart: Flags::take(&self.flags.restart),
            latency_changed: Flags::take(&self.flags.latency),
            params_changed: Flags::take(&self.flags.params) || ports,
            state_dirty: Flags::take(&self.flags.state_dirty),
        }
    }
}

impl PluginEditor for ClapInstance {
    fn open(&mut self, parent: ParentView) -> Result<(u32, u32), HostError> {
        let gui = self
            .exts()
            .gui
            .ok_or_else(|| HostError::EditorUnavailable("no GUI".into()))?;
        let api = GuiApiType::default_for_current_platform()
            .filter(|a| *a == GuiApiType::COCOA)
            .ok_or_else(|| HostError::EditorUnavailable("embedding needs Cocoa".into()))?;
        let h = self.inst.plugin_handle();
        if self.gui_open {
            gui.hide(&h).ok();
            gui.destroy(&h);
        }
        gui.create(
            &h,
            GuiConfiguration {
                api_type: api,
                is_floating: false,
            },
        )
        .map_err(|e| HostError::EditorUnavailable(format!("{e:?}")))?;
        self.gui_open = true;
        ffi::set_cocoa_parent(&gui, &h, parent.0)?;
        gui.show(&h)
            .map_err(|e| HostError::EditorUnavailable(format!("{e:?}")))?;
        let size = gui.get_size(&h).unwrap_or(GuiSize {
            width: 400,
            height: 300,
        });
        Ok((size.width, size.height))
    }

    fn resize(&mut self, width: u32, height: u32) -> Result<(u32, u32), HostError> {
        let gui = self
            .exts()
            .gui
            .ok_or_else(|| HostError::EditorUnavailable("no GUI".into()))?;
        let h = self.inst.plugin_handle();
        let want = gui
            .adjust_size(&h, GuiSize { width, height })
            .unwrap_or(GuiSize { width, height });
        gui.set_size(&h, want)
            .map_err(|e| HostError::EditorUnavailable(format!("{e:?}")))?;
        Ok((want.width, want.height))
    }

    fn can_resize(&mut self) -> bool {
        match (self.exts().gui, self.gui_open) {
            (Some(gui), true) => gui.can_resize(&self.inst.plugin_handle()),
            _ => false,
        }
    }

    fn close(&mut self) {
        if !self.gui_open {
            return;
        }
        if let Some(gui) = self.exts().gui {
            let h = self.inst.plugin_handle();
            gui.hide(&h).ok();
            gui.destroy(&h);
        }
        self.gui_open = false;
    }
}

impl Drop for ClapInstance {
    fn drop(&mut self) {
        PluginEditor::close(self);
        let _ = self.inst.try_deactivate();
    }
}
VIZDAW_EOF
```

Create `crates/core-host/src/clap/mod.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p crates/core-host/src/clap
cat > crates/core-host/src/clap/mod.rs <<'VIZDAW_EOF'
//! The CLAP host (spec §5) on `clack-host`: bundles are found by the
//! catalog, loaded on the main thread (Plan Ruling 2) and wrapped as
//! [`ClapInstance`] / [`ClapProcessor`].

mod ffi;
mod handlers;
mod instance;
mod processor;

pub use instance::ClapInstance;
pub use processor::ClapProcessor;

use crate::error::HostError;
use crate::info::PluginKind;
use crate::instance::{LoadRequest, PluginFormat, PluginInstance, PrepareJob, Prepared};
use core_model::mixer::{PluginFormat as Format, PluginRef};
use std::path::{Path, PathBuf};

/// The standard CLAP search folders (spec §5): the user's, then the
/// system's.
pub fn search_paths() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    if cfg!(target_os = "macos") {
        let mut v: Vec<PathBuf> = home
            .map(|h| h.join("Library/Audio/Plug-Ins/CLAP"))
            .into_iter()
            .collect();
        v.push(PathBuf::from("/Library/Audio/Plug-Ins/CLAP"));
        v
    } else {
        let mut v: Vec<PathBuf> = home.map(|h| h.join(".clap")).into_iter().collect();
        v.push(PathBuf::from("/usr/lib/clap"));
        v
    }
}

/// Every `.clap` bundle (or file) under `dirs`, recursively, sorted.
pub fn find_bundles(dirs: &[PathBuf]) -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>, depth: u32) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "clap") {
                out.push(p);
            } else if p.is_dir() && depth < 8 {
                walk(&p, out, depth + 1);
            }
        }
    }
    let mut out = Vec::new();
    for d in dirs {
        walk(d, &mut out, 0);
    }
    out.sort();
    out
}

/// The plugins a bundle's factory lists (loads the bundle into this
/// process: the scanner does this, the app does not, spec §8).
pub fn list_bundle(path: &Path) -> Result<Vec<(PluginRef, PluginKind)>, HostError> {
    let entry = ffi::load_entry(path)?;
    let factory = entry
        .get_plugin_factory()
        .ok_or_else(|| HostError::LoadFailed(format!("{}: no plugin factory", path.display())))?;
    let text = |c: Option<&std::ffi::CStr>| c.map(|s| s.to_string_lossy().into_owned());
    Ok(factory
        .plugin_descriptors()
        .filter_map(|d| {
            let id = text(d.id())?;
            let instrument = d.features().any(|f| f.to_bytes() == b"instrument");
            Some((
                PluginRef {
                    format: Format::Clap,
                    name: text(d.name()).unwrap_or_else(|| id.clone()),
                    vendor: text(d.vendor()).unwrap_or_default(),
                    version: text(d.version()).unwrap_or_default(),
                    id,
                },
                if instrument {
                    PluginKind::Instrument
                } else {
                    PluginKind::Effect
                },
            ))
        })
        .collect())
}

/// The CLAP [`PluginFormat`].
#[derive(Debug, Default)]
pub struct ClapFormat;

impl ClapFormat {
    /// The format.
    pub fn new() -> ClapFormat {
        ClapFormat
    }
}

impl PluginFormat for ClapFormat {
    fn format(&self) -> Format {
        Format::Clap
    }

    fn prepare(&self, req: &LoadRequest) -> PrepareJob {
        let path = req.path.clone();
        let id = req.plugin.id.clone();
        Box::new(move || {
            let path = path.ok_or_else(|| HostError::NotFound(id.clone()))?;
            if !path.exists() {
                return Err(HostError::NotFound(format!("{id} ({})", path.display())));
            }
            Ok(Prepared(Box::new(path)))
        })
    }

    fn finish(
        &mut self,
        req: &LoadRequest,
        prepared: Prepared,
    ) -> Result<Box<dyn PluginInstance>, HostError> {
        let path = prepared
            .0
            .downcast::<PathBuf>()
            .map_err(|_| HostError::LoadFailed("not a CLAP load".into()))?;
        Ok(Box::new(ClapInstance::load(&path, &req.plugin.id)?))
    }
}
VIZDAW_EOF
```

Create `crates/core-host/src/clap/processor.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p crates/core-host/src/clap
cat > crates/core-host/src/clap/processor.rs <<'VIZDAW_EOF'
//! A CLAP plugin's real-time half: translates a [`ProcessCtx`] into a CLAP
//! `process` call. Every buffer is allocated in `new`. // RT

use super::handlers::{VizHost, audio_call};
use crate::info::{PluginInfo, PortRole};
use crate::instance::AudioProcessor;
use crate::process::{MAX_EVENTS, MAX_PARAM_EVENTS, ParamEvent, ProcessCtx};
use clack_host::events::event_types::{
    MidiEvent as ClapMidi, NoteOffEvent, NoteOnEvent, ParamValueEvent, TransportEvent,
    TransportFlags,
};
use clack_host::events::io::{OutputEventBuffer, TryPushError};
use clack_host::events::spaces::CoreEventSpace;
use clack_host::events::{EventFlags, EventHeader, Match, UnknownEvent};
use clack_host::prelude::*;
use clack_host::utils::{BeatTime, SecondsTime};
use core_engine::graph::ProcessStatus;
use core_model::mixer::ParamId;

/// Collects the plugin's parameter events into the block's `ParamOut`
/// without allocating; other output events are dropped.
struct GestureOut<'a>(&'a mut crate::process::ParamOut);

impl OutputEventBuffer for GestureOut<'_> {
    fn try_push(&mut self, event: &UnknownEvent) -> Result<(), TryPushError> {
        let id = |c: Option<ClapId>| c.map(|c| ParamId(u64::from(c.get())));
        let e = match event.as_core_event() {
            Some(CoreEventSpace::ParamGestureBegin(g)) => id(g.param_id()).map(ParamEvent::Begin),
            Some(CoreEventSpace::ParamGestureEnd(g)) => id(g.param_id()).map(ParamEvent::End),
            Some(CoreEventSpace::ParamValue(v)) => {
                id(v.param_id()).map(|p| ParamEvent::Value(p, v.value()))
            }
            _ => None,
        };
        match e {
            Some(e) if !self.0.push(e) => Err(TryPushError::new()),
            _ => Ok(()),
        }
    }
}

/// A port's input channels, each cut to the block length.
struct InChans<'a> {
    it: std::slice::IterMut<'a, Box<[f32]>>,
    n: usize,
}

impl<'a> Iterator for InChans<'a> {
    type Item = InputChannel<'a, f32>;
    fn next(&mut self) -> Option<Self::Item> {
        let b = self.it.next()?;
        let len = self.n.min(b.len());
        Some(InputChannel::variable(b.get_mut(..len).unwrap_or(&mut [])))
    }
}

/// A port's output channels, each cut to the block length.
struct OutChans<'a> {
    it: std::slice::IterMut<'a, Box<[f32]>>,
    n: usize,
}

impl<'a> Iterator for OutChans<'a> {
    type Item = &'a mut [f32];
    fn next(&mut self) -> Option<Self::Item> {
        let b = self.it.next()?;
        let len = self.n.min(b.len());
        Some(b.get_mut(..len).unwrap_or(&mut []))
    }
}

/// Splits a flat channel list into consecutive ports. // RT
struct Split<'a, 'c> {
    rest: &'a mut [Box<[f32]>],
    counts: std::slice::Iter<'c, u16>,
}

impl<'a> Iterator for Split<'a, '_> {
    type Item = &'a mut [Box<[f32]>];
    fn next(&mut self) -> Option<Self::Item> {
        let c = usize::from(*self.counts.next()?).min(self.rest.len());
        let (head, tail) = std::mem::take(&mut self.rest).split_at_mut(c);
        self.rest = tail;
        Some(head)
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.counts.size_hint()
    }
}

impl ExactSizeIterator for Split<'_, '_> {}

type NoF64In = std::array::IntoIter<InputChannel<'static, f64>, 0>;
type NoF64Out = std::array::IntoIter<&'static mut [f64], 0>;

/// The real-time half of a [`super::ClapInstance`].
pub struct ClapProcessor {
    proc: Option<PluginAudioProcessor<VizHost>>,
    in_ports: AudioPorts,
    out_ports: AudioPorts,
    /// Channels per input port, and which port is main / sidechain.
    in_counts: Box<[u16]>,
    main_in: Option<usize>,
    sidechain_in: Option<usize>,
    /// Every input port's channels, port after port.
    inputs: Box<[Box<[f32]>]>,
    out_counts: Box<[u16]>,
    events: EventBuffer,
    clap_notes: bool,
    steady: u64,
    max_frames: u32,
}

impl std::fmt::Debug for ClapProcessor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClapProcessor").finish_non_exhaustive()
    }
}

impl ClapProcessor {
    pub(crate) fn new(
        proc: PluginAudioProcessor<VizHost>,
        info: &PluginInfo,
        max_frames: u32,
        clap_notes: bool,
    ) -> ClapProcessor {
        let in_counts: Box<[u16]> = info.inputs.iter().map(|p| p.channels).collect();
        let out_counts: Box<[u16]> = info.outputs.iter().map(|p| p.channels).collect();
        let in_total: usize = in_counts.iter().map(|c| usize::from(*c)).sum();
        let out_total: usize = out_counts.iter().map(|c| usize::from(*c)).sum();
        ClapProcessor {
            proc: Some(proc),
            in_ports: AudioPorts::with_capacity(in_total, in_counts.len()),
            out_ports: AudioPorts::with_capacity(out_total, out_counts.len()),
            main_in: info.inputs.iter().position(|p| p.role == PortRole::Main),
            sidechain_in: info
                .inputs
                .iter()
                .position(|p| p.role == PortRole::Sidechain),
            inputs: (0..in_total)
                .map(|_| vec![0.0f32; max_frames as usize].into_boxed_slice())
                .collect(),
            in_counts,
            out_counts,
            events: EventBuffer::with_capacity(MAX_EVENTS + MAX_PARAM_EVENTS),
            clap_notes,
            steady: 0,
            max_frames,
        }
    }

    /// First channel index of input port `port`.
    fn in_start(&self, port: usize) -> usize {
        self.in_counts
            .iter()
            .take(port)
            .map(|c| usize::from(*c))
            .sum()
    }

    /// Copies the context's main and sidechain channels into the plugin's
    /// input ports; other ports get silence. // RT
    fn fill_inputs(&mut self, ctx: &ProcessCtx<'_>, n: usize) {
        for ch in self.inputs.iter_mut() {
            if let Some(c) = ch.get_mut(..n) {
                c.fill(0.0);
            }
        }
        for (port, src) in [
            (self.main_in, ctx.input),
            (self.sidechain_in, ctx.sidechain),
        ] {
            let Some(port) = port else {
                continue;
            };
            let start = self.in_start(port);
            let count = usize::from(self.in_counts.get(port).copied().unwrap_or(0));
            for k in 0..count {
                // A mono source fills every channel of a stereo port.
                let s = src.get(k).or_else(|| src.first());
                if let (Some(d), Some(s)) = (self.inputs.get_mut(start + k), s) {
                    if let (Some(d), Some(s)) = (d.get_mut(..n), s.get(..n)) {
                        d.copy_from_slice(s);
                    }
                }
            }
        }
    }

    /// Fills the input event list from the context, merged by frame. // RT
    fn fill_events(&mut self, ctx: &ProcessCtx<'_>) {
        self.events.clear();
        let mut params = ctx.params_in.iter().take(MAX_PARAM_EVENTS).peekable();
        let mut midi = ctx.events.iter().take(MAX_EVENTS).peekable();
        loop {
            let take_param = match (params.peek(), midi.peek()) {
                (Some(p), Some(m)) => p.frame <= m.frame,
                (Some(_), None) => true,
                (None, Some(_)) => false,
                (None, None) => break,
            };
            if take_param {
                let Some(p) = params.next() else {
                    break;
                };
                let Some(id) = u32::try_from(p.id.0).ok().and_then(ClapId::from_raw) else {
                    continue;
                };
                self.events.push(&ParamValueEvent::new(
                    p.frame,
                    id,
                    Pckn::match_all(),
                    p.value,
                ));
                continue;
            }
            let Some(m) = midi.next() else {
                break;
            };
            let [status, d1, d2] = m.bytes;
            let ch = u16::from(status & 0x0F);
            let pckn = Pckn::new(0u16, ch, u16::from(d1 & 0x7F), Match::<u32>::All);
            match status & 0xF0 {
                0x90 if self.clap_notes && d2 > 0 => {
                    self.events
                        .push(&NoteOnEvent::new(m.frame, pckn, f64::from(d2) / 127.0))
                }
                0x80 | 0x90 if self.clap_notes => {
                    self.events
                        .push(&NoteOffEvent::new(m.frame, pckn, f64::from(d2) / 127.0))
                }
                _ => self.events.push(&ClapMidi::new(m.frame, 0, m.bytes)),
            }
        }
    }
}

fn transport(ctx: &ProcessCtx<'_>) -> TransportEvent {
    let t = ctx.transport;
    let mut flags = TransportFlags::HAS_TEMPO | TransportFlags::HAS_BEATS_TIMELINE;
    if t.playing {
        flags |= TransportFlags::IS_PLAYING;
    }
    let seconds = t.beats * 60.0 / t.tempo_bpm.max(1.0);
    TransportEvent {
        header: EventHeader::new_core(0, EventFlags::empty()),
        flags,
        song_pos_beats: BeatTime::from_float(t.beats),
        song_pos_seconds: SecondsTime::from_float(seconds),
        tempo: t.tempo_bpm,
        tempo_inc: 0.0,
        loop_start_beats: BeatTime::from_float(0.0),
        loop_end_beats: BeatTime::from_float(0.0),
        loop_start_seconds: SecondsTime::from_float(0.0),
        loop_end_seconds: SecondsTime::from_float(0.0),
        bar_start: BeatTime::from_float((t.beats / 4.0).floor() * 4.0),
        bar_number: (t.beats / 4.0).floor() as i32,
        time_signature_numerator: 4,
        time_signature_denominator: 4,
    }
}

impl AudioProcessor for ClapProcessor {
    fn process(&mut self, ctx: &mut ProcessCtx<'_>) -> ProcessStatus {
        let n = ctx.frames.min(self.max_frames) as usize;
        self.fill_inputs(ctx, n);
        self.fill_events(ctx);
        let t = transport(ctx);
        let Some(proc) = self.proc.as_mut() else {
            return ProcessStatus::Error;
        };
        let steady = self.steady;
        let ins = Split {
            rest: &mut self.inputs,
            counts: self.in_counts.iter(),
        }
        .map(|chans| AudioPortBuffer {
            latency: 0,
            channels: AudioPortBufferType::<_, NoF64In>::F32(InChans {
                it: chans.iter_mut(),
                n,
            }),
        });
        let input = self.in_ports.with_input_buffers(ins);
        let outs = Split {
            rest: ctx.outputs,
            counts: self.out_counts.iter(),
        }
        .map(|chans| AudioPortBuffer {
            latency: 0,
            channels: AudioPortBufferType::<_, NoF64Out>::F32(OutChans {
                it: chans.iter_mut(),
                n,
            }),
        });
        let mut output = self.out_ports.with_output_buffers(outs);
        let events_in = self.events.as_input();
        let mut gestures = GestureOut(ctx.params_out);
        let mut events_out = OutputEvents::from_buffer(&mut gestures);
        let result = audio_call(|| {
            let started = proc.ensure_processing_started().ok()?;
            started
                .process(
                    &input,
                    &mut output,
                    &events_in,
                    &mut events_out,
                    Some(steady),
                    Some(&t),
                )
                .ok()
        });
        self.steady = self.steady.wrapping_add(n as u64);
        match result {
            Some(_) => ProcessStatus::Ok,
            None => ProcessStatus::Error,
        }
    }

    fn reset(&mut self) {
        if let Some(p) = self.proc.as_mut() {
            audio_call(|| p.reset());
        }
        self.steady = 0;
    }
}

impl Drop for ClapProcessor {
    fn drop(&mut self) {
        if let Some(p) = self.proc.take() {
            // `stop_processing` is an audio-thread call.
            audio_call(move || drop(p.into_stopped()));
        }
    }
}
VIZDAW_EOF
```

Change `crates/core-host/Cargo.toml`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-host/Cargo.toml b/crates/core-host/Cargo.toml
--- a/crates/core-host/Cargo.toml
+++ b/crates/core-host/Cargo.toml
@@ -9,9 +9,12 @@ rust-version.workspace = true
 core-model = { path = "../core-model" }
 core-engine = { path = "../core-engine" }
 rtrb = "0.4"
+clack-host = "0.2"
+clack-extensions = { version = "0.2", features = ["clack-host", "audio-ports", "note-ports", "params", "state", "latency", "gui", "tail", "log", "thread-check"] }
 serde = { version = "1", features = ["derive"] }
 serde_json = "1"
 thiserror = "2"
 
 [dev-dependencies]
 core-engine = { path = "../core-engine", features = ["fixtures"] }
+test-plugins = { path = "../test-plugins" }
VIZDAW_EOF
```

Change `crates/core-host/src/instance.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-host/src/instance.rs b/crates/core-host/src/instance.rs
--- a/crates/core-host/src/instance.rs
+++ b/crates/core-host/src/instance.rs
@@ -49,7 +49,7 @@ pub trait PluginEditor {
     /// Asks for a new size; returns the size the plugin accepted.
     fn resize(&mut self, width: u32, height: u32) -> Result<(u32, u32), HostError>;
     /// Whether the user may resize it.
-    fn can_resize(&self) -> bool;
+    fn can_resize(&mut self) -> bool;
     /// Hides and detaches it (also on drop).
     fn close(&mut self);
 }
@@ -81,7 +81,7 @@ pub trait PluginInstance {
     /// A parameter's current value, when the plugin reports one.
     fn param_value(&mut self, id: ParamId) -> Option<f64>;
     /// The plugin's editor, if it has one.
-    fn editor(&mut self) -> Option<Box<dyn PluginEditor + '_>>;
+    fn editor(&mut self) -> Option<&mut dyn PluginEditor>;
     /// Main-thread servicing (`host_idle`, 60 Hz): runs callbacks the
     /// plugin asked for and reports what it requested.
     fn idle(&mut self) -> InstanceEvents;
VIZDAW_EOF
```

Change `crates/core-host/src/lib.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-host/src/lib.rs b/crates/core-host/src/lib.rs
--- a/crates/core-host/src/lib.rs
+++ b/crates/core-host/src/lib.rs
@@ -16,6 +16,7 @@
     )
 )]
 
+pub mod clap;
 pub mod error;
 pub mod info;
 pub mod instance;
VIZDAW_EOF
```

- [ ] **Step 4: Run the tests to see them pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-host --test clap_host
```

Expected: 11 tests pass, including `gain_reports_its_ports_and_parameters_and_processes`, `the_synth_renders_each_channel_on_its_own_output`, `the_ducker_follows_its_sidechain`, `state_round_trips_and_foreign_state_is_rejected`, `ui_gestures_come_out_of_process`, `a_latency_change_asks_for_a_restart`.

- [ ] **Step 5: Check the whole workspace**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Expected: no formatting changes left, no clippy warnings, every test passes.

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-host/src/clap/ffi.rs \
  crates/core-host/src/clap/handlers.rs \
  crates/core-host/src/clap/instance.rs \
  crates/core-host/src/clap/mod.rs \
  crates/core-host/src/clap/processor.rs \
  crates/core-host/tests/clap_host.rs \
  crates/core-host/Cargo.toml \
  crates/core-host/src/instance.rs \
  crates/core-host/src/lib.rs \
  Cargo.lock
git commit -m "$(cat <<'MSG'
feat(core-host): CLAP host on clack-host

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
MSG
)"
```


---

### Task 7: Engine adapters and the shelf

**Files:**
- Create: `crates/core-host/src/adapt.rs`
- Create: `crates/core-host/src/shelf.rs`
- Create: `crates/core-host/tests/common/mod.rs`
- Create: `crates/core-host/tests/engine_plugins.rs`
- Modify: `crates/core-host/src/lib.rs`
- Modify: `crates/core-host/tests/clap_host.rs`
- Lockfile: `Cargo.lock` (cargo updates it when dependencies change)

**Interfaces:**
- Consumes: Task 6's CLAP host; Tasks 2–3's `InsertFactory::make_instrument`, `EffectNode::process_block`, `InstrumentNode::{outputs, render_outputs}`.
- Produces: `core_host::adapt::{PluginEffect, PluginSynth, Returned { id, processor }}` with `new(id, Box<dyn AudioProcessor>, &PluginInfo, max_frames, (ParamReceiver, GestureSender), Sender<Returned>)`; `core_host::shelf::{Shelf, ShelfFactory, Ready::{Effect, Instrument}}` with `Shelf::{new, put, set_reason, clear_reason, has, take_unwanted, factory}`; test helpers in `tests/common/mod.rs` (`plugin`, `load`, `shelve`, `Live`).

Ruling 3. `PluginEffect` maps the engine's stereo insert buffer to the plugin's main input (mono gets the mix, Ruling 9), the sidechain to its sidechain port (silence when unrouted), drains queued parameter values into `params_in`, forwards `ParamOut` to the gesture queue, and writes output port 0 back. `PluginSynth` queues MIDI 1.0 messages (SysEx dropped), insertion-sorts them by frame (no allocation), and writes each output port to the matching graph output. Dropping either sends its processor back. `ShelfFactory` gives a processor only to the slot it was made for (same instance, plugin, kind, and a block size at least the engine's), else the reason the host recorded.

- [ ] **Step 1: Write the failing tests**


Create `crates/core-host/tests/common/mod.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p crates/core-host/tests/common
cat > crates/core-host/tests/common/mod.rs <<'VIZDAW_EOF'
//! Shared test helpers: load a test plugin and shelve its processor.
#![allow(dead_code)]

use core_host::adapt::{PluginEffect, PluginSynth, Returned};
use core_host::clap::ClapFormat;
use core_host::queue::{GestureReceiver, ParamSender, param_queues};
use core_host::shelf::{Ready, Shelf};
use core_host::{LoadRequest, PluginFormat, PluginInstance};
use core_model::ids::PluginInstanceId;
use core_model::mixer::{PluginFormat as Format, PluginRef};
use std::sync::mpsc::Sender;

/// The reference of test plugin `id` (e.g. `vizdaw.test.gain`).
pub fn plugin(id: &str) -> PluginRef {
    PluginRef {
        format: Format::Clap,
        id: id.into(),
        name: id.trim_start_matches("vizdaw.test.").into(),
        vendor: "VizDAW".into(),
        version: "1.0.0".into(),
    }
}

/// Loads test plugin `id` from the test bundle.
pub fn load(id: &str) -> Box<dyn PluginInstance> {
    let mut f = ClapFormat::new();
    let req = LoadRequest {
        plugin: plugin(id),
        path: Some(test_plugins::bundle()),
        sample_rate: 48_000.0,
        max_frames: 4096,
    };
    let prepared = (f.prepare(&req))().unwrap();
    f.finish(&req, prepared).unwrap()
}

/// A loaded, activated, shelved instance and its main-thread ends.
pub struct Live {
    pub inst: Box<dyn PluginInstance>,
    pub params: ParamSender,
    pub gestures: GestureReceiver,
}

/// Activates `inst` for 48 kHz / 4096 and puts it on `shelf` as instance
/// `id` (an insert, or an instrument when `instrument`).
pub fn shelve(
    shelf: &Shelf,
    id: PluginInstanceId,
    mut inst: Box<dyn PluginInstance>,
    instrument: bool,
    back: &Sender<Returned>,
) -> Live {
    let proc = inst.activate(48_000.0, 4096).unwrap();
    let ((params, gestures), audio) = param_queues();
    let info = inst.info().clone();
    let ready = if instrument {
        Ready::Instrument(PluginSynth::new(id, proc, &info, 4096, audio, back.clone()))
    } else {
        Ready::Effect(PluginEffect::new(
            id,
            proc,
            &info,
            4096,
            audio,
            back.clone(),
        ))
    };
    shelf.put(id, &info.plugin, 4096, ready);
    Live {
        inst,
        params,
        gestures,
    }
}
VIZDAW_EOF
```

Create `crates/core-host/tests/engine_plugins.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p crates/core-host/tests
cat > crates/core-host/tests/engine_plugins.rs <<'VIZDAW_EOF'
//! CLAP test plugins playing in the engine's graph through the adapters
//! and the shelf: inserts, a multi-output instrument, a sidechain, faults,
//! latency, gestures and processors coming back.

mod common;

use common::{Live, load, plugin, shelve};
use core_engine::backend::AudioBackend;
use core_engine::engine::{Engine, EngineConfig, find_sequence};
use core_engine::fixtures::{Builder, Q, demo_document};
use core_engine::offline::{OfflineBackend, OfflineSinks};
use core_engine::render::Faults;
use core_host::ParamEvent;
use core_host::adapt::Returned;
use core_host::shelf::Shelf;
use core_model::command::MixerCmd;
use core_model::document::Document;
use core_model::event::{EventBody, EventSpec};
use core_model::ids::{PluginInstanceId, SeqId, StripId};
use core_model::mixer::{ParamId, StripKind};
use core_model::time::Tick;
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

const SR: f64 = 48_000.0;

struct Rig {
    engine: Engine,
    be: OfflineBackend,
    shelf: Shelf,
    back: std::sync::mpsc::Sender<Returned>,
    returned: Receiver<Returned>,
    live: Vec<Live>,
}

fn rig() -> Rig {
    let (mut engine, r) = Engine::new(
        EngineConfig::new(SR),
        Box::new(OfflineSinks::new(1 << 16).with_tone()),
    )
    .unwrap();
    let shelf = Shelf::new();
    engine.set_insert_factory(Box::new(shelf.factory()));
    let mut be = OfflineBackend::new(SR, 256);
    be.start(r).unwrap();
    let (back, returned) = channel();
    Rig {
        engine,
        be,
        shelf,
        back,
        returned,
        live: Vec::new(),
    }
}

impl Rig {
    fn add(&mut self, id: PluginInstanceId, plugin_id: &str, instrument: bool) {
        let live = shelve(&self.shelf, id, load(plugin_id), instrument, &self.back);
        self.live.push(live);
    }

    /// Plays `frames` from the start and returns the left channel.
    fn play(&mut self, frames: usize) -> Vec<f32> {
        self.engine.play().unwrap();
        let mut out = Vec::new();
        while out.len() < frames {
            self.be.block(256);
            out.extend_from_slice(self.be.output(256).0);
        }
        out
    }
}

fn demo() -> (Builder, SeqId) {
    let doc = demo_document().unwrap();
    let song = find_sequence(&doc, "Song").unwrap();
    (Builder::from_doc(doc), song)
}

#[test]
fn a_clap_gain_insert_scales_the_strip() {
    let (b, song) = demo();
    let mut plain = rig();
    plain.engine.load(b.doc(), song).unwrap();
    let dry = plain.play(48_000);

    let (mut b, song) = demo();
    let synth = b.strip_of(StripKind::BuiltinSynth).unwrap();
    let id = b.insert(synth, plugin("vizdaw.test.gain")).unwrap();
    let mut r = rig();
    let mut inst = load("vizdaw.test.gain");
    inst.set_params(&[(ParamId(0), 0.5)]).unwrap();
    let live = shelve(&r.shelf, id, inst, false, &r.back);
    r.live.push(live);
    let report = r.engine.load(b.doc(), song).unwrap();
    assert!(report.graph.placeholders.is_empty(), "{:?}", report.graph);
    let wet = r.play(48_000);
    assert!(dry.iter().any(|x| *x != 0.0));
    for (d, w) in dry.iter().zip(&wet) {
        assert_eq!(*w, *d * 0.5);
    }
    assert!(r.engine.faults().is_empty());
}

fn note_on(ch: u8) -> EventSpec {
    let mut e = EventSpec::new(Tick(0), EventBody::note(69, 0xFFFF, Tick(8 * Q)));
    e.group_channel = Some((0, ch));
    e
}

#[test]
fn a_four_output_synth_feeds_its_output_strips() {
    let mut b = Builder::new();
    let (d, id) = b
        .plugin_dest("Synth", plugin("vizdaw.test.synth4"))
        .unwrap();
    let (seq, lane) = b.sequence("S", "L", Some(d)).unwrap();
    b.events(seq, lane, vec![note_on(0), note_on(2)]).unwrap();
    for (index, name) in [(1u16, "Two"), (2, "Three")] {
        b.mixer(MixerCmd::AddInstrumentOutput {
            dest: d,
            index,
            name: name.into(),
        })
        .unwrap();
    }
    let main = b.strip_of(StripKind::Instrument(d)).unwrap();
    let two = b.strip_of(StripKind::InstrumentOutput(d, 1)).unwrap();
    let three = b.strip_of(StripKind::InstrumentOutput(d, 2)).unwrap();
    let mut r = rig();
    r.add(id, "vizdaw.test.synth4", true);
    let report = r.engine.load(b.doc(), seq).unwrap();
    assert!(report.unroutable.is_empty(), "{:?}", report.warnings);
    assert!(
        report.graph.warnings.is_empty(),
        "{:?}",
        report.graph.warnings
    );
    r.play(4096);
    let m = r.engine.meters();
    let peak = |s: StripId| m.iter().find(|x| x.strip == s).unwrap().peak_l;
    assert!(peak(main) > 0.1, "channel 0 sounds on output 0: {m:?}");
    assert!(peak(three) > 0.1, "channel 2 sounds on output 2: {m:?}");
    assert_eq!(peak(two), 0.0, "nothing on channel 1");
}

/// The built-in synth's strip with a ducker sidechained from a test synth
/// that plays (or not) a long note.
fn ducked(note: bool) -> (Document, SeqId, PluginInstanceId, PluginInstanceId) {
    let (mut b, song) = demo();
    let (d, synth) = b.plugin_dest("Key", plugin("vizdaw.test.synth4")).unwrap();
    let lane = b.lane(song, "Key", Some(d)).unwrap();
    if note {
        b.events(song, lane, vec![note_on(0)]).unwrap();
    }
    let key = b.strip_of(StripKind::Instrument(d)).unwrap();
    let target = b.strip_of(StripKind::BuiltinSynth).unwrap();
    let duck = b.insert(target, plugin("vizdaw.test.ducker")).unwrap();
    b.mixer(MixerCmd::SetSidechain {
        slot: duck,
        source: Some(key),
    })
    .unwrap();
    // The key drives the sidechain post-fader but is not heard: it plays
    // into a bus whose fader is at −∞.
    let sink = b.bus("Sink").unwrap();
    b.mixer(MixerCmd::SetOutput {
        strip: key,
        output: core_model::mixer::Output::Bus(sink),
    })
    .unwrap();
    b.mixer(MixerCmd::SetGain {
        strip: sink,
        gain_db: core_model::mixer::GAIN_FLOOR_DB,
    })
    .unwrap();
    (b.finish(), song, synth, duck)
}

#[test]
fn the_ducker_follows_its_sidechain_source() {
    let level = |note: bool| {
        let (doc, song, synth, duck) = ducked(note);
        let mut r = rig();
        r.add(synth, "vizdaw.test.synth4", true);
        r.add(duck, "vizdaw.test.ducker", false);
        let report = r.engine.load(&doc, song).unwrap();
        assert!(report.graph.placeholders.is_empty(), "{:?}", report.graph);
        r.play(24_000).iter().map(|x| x.abs()).sum::<f32>()
    };
    let open = level(false);
    let ducked = level(true);
    assert!(open > 0.0);
    assert!(ducked < open * 0.9, "ducked {ducked} vs open {open}");
}

#[test]
fn a_nan_plugin_is_bypassed_and_reported_by_instance() {
    let (mut b, song) = demo();
    let synth = b.strip_of(StripKind::BuiltinSynth).unwrap();
    let id = b.insert(synth, plugin("vizdaw.test.nan")).unwrap();
    let mut r = rig();
    r.add(id, "vizdaw.test.nan", false);
    r.engine.load(b.doc(), song).unwrap();
    let out = r.play(20 * 256);
    assert!(out.iter().all(|x| x.is_finite()));
    assert_eq!(r.engine.plugin_faults(), vec![id]);
    assert!(r.engine.faults().contains(Faults::PLUGIN_FAULT));
}

#[test]
fn plugin_latency_is_compensated() {
    let (mut b, song) = demo();
    let synth = b.strip_of(StripKind::BuiltinSynth).unwrap();
    let id = b.insert(synth, plugin("vizdaw.test.gain")).unwrap();
    let mut r = rig();
    let mut inst = load("vizdaw.test.gain");
    inst.set_params(&[(ParamId(1), 300.0)]).unwrap();
    let live = shelve(&r.shelf, id, inst, false, &r.back);
    r.live.push(live);
    let report = r.engine.load(b.doc(), song).unwrap();
    assert_eq!(report.graph.latency_frames, 300);
    assert_eq!(r.engine.latency_frames(), 300);
}

#[test]
fn ui_gestures_reach_the_main_thread_and_values_reach_the_plugin() {
    let (mut b, song) = demo();
    let synth = b.strip_of(StripKind::BuiltinSynth).unwrap();
    let id = b.insert(synth, plugin("vizdaw.test.gain")).unwrap();
    let mut r = rig();
    r.add(id, "vizdaw.test.gain", false);
    r.engine.load(b.doc(), song).unwrap();
    r.play(256);
    assert!(r.live[0].params.send(ParamId(2), 0.25));
    r.play(512);
    assert_eq!(
        r.live[0].gestures.drain(),
        vec![
            ParamEvent::Begin(ParamId(0)),
            ParamEvent::Value(ParamId(0), 0.25),
            ParamEvent::End(ParamId(0)),
        ]
    );
}

#[test]
fn removed_instances_come_back_off_the_audio_thread() {
    let (mut b, song) = demo();
    let synth = b.strip_of(StripKind::BuiltinSynth).unwrap();
    let id = b.insert(synth, plugin("vizdaw.test.gain")).unwrap();
    let mut r = rig();
    r.add(id, "vizdaw.test.gain", false);
    r.engine.load(b.doc(), song).unwrap();
    r.play(512);
    b.mixer(MixerCmd::RemoveInsert { slot: id }).unwrap();
    r.engine.load(b.doc(), song).unwrap();
    let start = Instant::now();
    let back = loop {
        r.be.block(256);
        if let Ok(x) = r.returned.recv_timeout(Duration::from_millis(5)) {
            break x;
        }
        assert!(start.elapsed() < Duration::from_secs(5), "never returned");
    };
    assert_eq!(back.id, id);
    // The main thread deactivates it.
    let live = r.live.pop().unwrap();
    let mut inst = live.inst;
    inst.deactivate(back.processor);
    assert!(!inst.is_active());
}
VIZDAW_EOF
```

Update `crates/core-host/tests/clap_host.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-host/tests/clap_host.rs b/crates/core-host/tests/clap_host.rs
--- a/crates/core-host/tests/clap_host.rs
+++ b/crates/core-host/tests/clap_host.rs
@@ -5,30 +5,15 @@ use core_engine::backend::BlockInfo;
 use core_engine::graph::ProcessStatus;
 use core_host::clap::{ClapFormat, list_bundle};
 use core_host::{
-    AudioProcessor, LoadRequest, MidiEvent, ParamChange, ParamOut, PluginFormat, PluginInstance,
-    PluginKind, PortRole, ProcessCtx,
+    AudioProcessor, LoadRequest, MidiEvent, ParamChange, ParamOut, PluginFormat, PluginKind,
+    PortRole, ProcessCtx,
 };
-use core_model::mixer::{ParamId, PluginFormat as Format, PluginRef};
+use core_model::mixer::{ParamId, PluginRef};
 
-const N: usize = 64;
+mod common;
+use common::load;
 
-pub fn load(id: &str) -> Box<dyn PluginInstance> {
-    let mut f = ClapFormat::new();
-    let req = LoadRequest {
-        plugin: PluginRef {
-            format: Format::Clap,
-            id: id.into(),
-            name: id.into(),
-            vendor: String::new(),
-            version: String::new(),
-        },
-        path: Some(test_plugins::bundle()),
-        sample_rate: 48_000.0,
-        max_frames: 512,
-    };
-    let prepared = (f.prepare(&req))().unwrap();
-    f.finish(&req, prepared).unwrap()
-}
+const N: usize = 64;
 
 fn bufs(channels: usize, v: f32) -> Vec<Box<[f32]>> {
     (0..channels)
VIZDAW_EOF
```

- [ ] **Step 2: Run them to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-host --test engine_plugins --test clap_host
```

Expected: `unresolved import `core_host::adapt``.

- [ ] **Step 3: Implement**


Create `crates/core-host/src/adapt.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p crates/core-host/src
cat > crates/core-host/src/adapt.rs <<'VIZDAW_EOF'
//! Plugin processors as the engine's inserts ([`PluginEffect`]) and
//! instruments ([`PluginSynth`]) (3a Plan Ruling 12). Buffers and queues
//! are allocated here, before the processor reaches a render thread. When
//! an adapter is dropped (always off the audio thread: the engine retires
//! graphs and sinks through its collector) it sends its processor back to
//! the host, which deactivates it on the main thread. // RT (processing)

use crate::info::{PluginInfo, PortRole};
use crate::instance::AudioProcessor;
use crate::process::{MAX_EVENTS, MAX_PARAM_EVENTS, MidiEvent, ParamChange, ParamOut, ProcessCtx};
use crate::queue::{GestureSender, ParamReceiver};
use core_engine::backend::{BlockInfo, InstrumentNode, StereoBuf};
use core_engine::graph::{EffectNode, ProcessStatus};
use core_model::ids::PluginInstanceId;
use std::sync::mpsc::Sender;

/// A processor coming back from the engine.
pub struct Returned {
    /// Its instance.
    pub id: PluginInstanceId,
    /// The processor.
    pub processor: Box<dyn AudioProcessor>,
}

impl std::fmt::Debug for Returned {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Returned").field("id", &self.id).finish()
    }
}

fn channels(n: usize, frames: u32) -> Vec<Box<[f32]>> {
    (0..n)
        .map(|_| vec![0.0f32; frames as usize].into_boxed_slice())
        .collect()
}

/// What every adapter holds.
struct Core {
    id: PluginInstanceId,
    proc: Option<Box<dyn AudioProcessor>>,
    latency: u32,
    max_frames: u32,
    outputs: Vec<Box<[f32]>>,
    /// `(first channel, channels)` of every output port.
    out_ports: Vec<(usize, usize)>,
    params: ParamReceiver,
    gestures: GestureSender,
    params_buf: Box<[ParamChange; MAX_PARAM_EVENTS]>,
    out: ParamOut,
    back: Option<Sender<Returned>>,
}

impl Core {
    fn new(
        id: PluginInstanceId,
        proc: Box<dyn AudioProcessor>,
        info: &PluginInfo,
        max_frames: u32,
        queues: (ParamReceiver, GestureSender),
        back: Sender<Returned>,
    ) -> Core {
        let mut out_ports = Vec::with_capacity(info.outputs.len());
        let mut at = 0;
        for p in &info.outputs {
            let c = usize::from(p.channels);
            out_ports.push((at, c));
            at += c;
        }
        Core {
            id,
            proc: Some(proc),
            latency: info.latency,
            max_frames,
            outputs: channels(at, max_frames),
            out_ports,
            params: queues.0,
            gestures: queues.1,
            params_buf: Box::new([ParamChange::default(); MAX_PARAM_EVENTS]),
            out: ParamOut::default(),
            back: Some(back),
        }
    }

    /// Copies output port `port` into `dst` (a mono port feeds both
    /// sides; a missing port gives silence). // RT
    fn write_port(&self, port: usize, dst: &mut StereoBuf, n: usize) {
        let Some(&(start, count)) = self.out_ports.get(port) else {
            dst.clear(n as u32);
            return;
        };
        let left = self.outputs.get(start);
        let right = if count >= 2 {
            self.outputs.get(start + 1)
        } else {
            left
        };
        for (d, s) in [(&mut dst.left, left), (&mut dst.right, right)] {
            match (d.get_mut(..n), s.and_then(|s| s.get(..n))) {
                (Some(d), Some(s)) => d.copy_from_slice(s),
                (Some(d), None) => d.fill(0.0),
                _ => {}
            }
        }
    }

    /// Runs the processor with `input`, `sidechain` and `events`. // RT
    fn run(
        &mut self,
        frames: u32,
        input: &mut [Box<[f32]>],
        sidechain: &mut [Box<[f32]>],
        events: &[MidiEvent],
        info: &BlockInfo,
    ) -> ProcessStatus {
        let k = self.params.drain_into(&mut self.params_buf[..]);
        self.out.clear();
        let Some(proc) = self.proc.as_mut() else {
            return ProcessStatus::Error;
        };
        let mut ctx = ProcessCtx {
            frames: frames.min(self.max_frames),
            input,
            sidechain,
            outputs: &mut self.outputs,
            events,
            params_in: self.params_buf.get(..k).unwrap_or(&[]),
            params_out: &mut self.out,
            transport: info,
        };
        let status = proc.process(&mut ctx);
        self.gestures.send_all(self.out.as_slice());
        status
    }
}

impl Drop for Core {
    fn drop(&mut self) {
        if let (Some(processor), Some(back)) = (self.proc.take(), self.back.take()) {
            // If the host is gone, the processor is dropped here instead.
            let _ = back.send(Returned {
                id: self.id,
                processor,
            });
        }
    }
}

/// A plugin as an insert.
pub struct PluginEffect {
    core: Core,
    input: Vec<Box<[f32]>>,
    sidechain: Vec<Box<[f32]>>,
}

impl PluginEffect {
    /// Wraps an activated processor (made for blocks of `max_frames`).
    pub fn new(
        id: PluginInstanceId,
        proc: Box<dyn AudioProcessor>,
        info: &PluginInfo,
        max_frames: u32,
        queues: (ParamReceiver, GestureSender),
        back: Sender<Returned>,
    ) -> PluginEffect {
        let main = info
            .main_input()
            .and_then(|i| info.inputs.get(i))
            .map_or(0, |p| usize::from(p.channels));
        let sc = info
            .inputs
            .iter()
            .find(|p| p.role == PortRole::Sidechain)
            .map_or(0, |p| usize::from(p.channels));
        PluginEffect {
            core: Core::new(id, proc, info, max_frames, queues, back),
            input: channels(main, max_frames),
            sidechain: channels(sc, max_frames),
        }
    }
}

/// Copies a stereo buffer into `dst` channels (one channel: the mix). //
/// RT
fn copy_in(dst: &mut [Box<[f32]>], src: &StereoBuf, n: usize) {
    match dst {
        [mono] => {
            if let (Some(d), Some(l), Some(r)) =
                (mono.get_mut(..n), src.left.get(..n), src.right.get(..n))
            {
                for ((d, l), r) in d.iter_mut().zip(l).zip(r) {
                    *d = 0.5 * (l + r);
                }
            }
        }
        [left, right, ..] => {
            for (d, s) in [(left, &src.left), (right, &src.right)] {
                if let (Some(d), Some(s)) = (d.get_mut(..n), s.get(..n)) {
                    d.copy_from_slice(s);
                }
            }
        }
        [] => {}
    }
}

impl EffectNode for PluginEffect {
    fn latency(&self) -> u32 {
        self.core.latency
    }

    fn process(
        &mut self,
        frames: u32,
        io: &mut StereoBuf,
        sidechain: Option<&StereoBuf>,
    ) -> ProcessStatus {
        self.process_block(frames, io, sidechain, &BlockInfo::default())
    }

    fn process_block(
        &mut self,
        frames: u32,
        io: &mut StereoBuf,
        sidechain: Option<&StereoBuf>,
        info: &BlockInfo,
    ) -> ProcessStatus {
        let n = (frames.min(self.core.max_frames) as usize)
            .min(io.left.len())
            .min(io.right.len());
        copy_in(&mut self.input, io, n);
        match sidechain {
            Some(sc) => copy_in(&mut self.sidechain, sc, n),
            None => {
                for c in self.sidechain.iter_mut() {
                    if let Some(c) = c.get_mut(..n) {
                        c.fill(0.0);
                    }
                }
            }
        }
        let status = self
            .core
            .run(n as u32, &mut self.input, &mut self.sidechain, &[], info);
        self.core.write_port(0, io, n);
        status
    }

    fn reset(&mut self) {
        if let Some(p) = self.core.proc.as_mut() {
            p.reset();
        }
    }
}

/// Stable insertion sort by frame: arrival order is frame order except for
/// releases sent at offset 0 after earlier events, and same-frame order
/// must be kept. No allocation (`slice::sort_by_key` may allocate). // RT
fn sort_by_frame(events: &mut [MidiEvent]) {
    for i in 1..events.len() {
        let mut j = i;
        while j > 0 {
            let (Some(a), Some(b)) = (events.get(j - 1), events.get(j)) else {
                break;
            };
            if a.frame <= b.frame {
                break;
            }
            events.swap(j - 1, j);
            j -= 1;
        }
    }
}

/// A plugin as a plugin instrument destination's sink.
pub struct PluginSynth {
    core: Core,
    events: Box<[MidiEvent; MAX_EVENTS]>,
    count: usize,
    outputs: u16,
}

impl PluginSynth {
    /// Wraps an activated processor (made for blocks of `max_frames`).
    pub fn new(
        id: PluginInstanceId,
        proc: Box<dyn AudioProcessor>,
        info: &PluginInfo,
        max_frames: u32,
        queues: (ParamReceiver, GestureSender),
        back: Sender<Returned>,
    ) -> PluginSynth {
        let outputs = u16::try_from(info.outputs.len().clamp(1, 16)).unwrap_or(1);
        PluginSynth {
            core: Core::new(id, proc, info, max_frames, queues, back),
            events: Box::new([MidiEvent::default(); MAX_EVENTS]),
            count: 0,
            outputs,
        }
    }
}

impl InstrumentNode for PluginSynth {
    fn midi(&mut self, offset: u32, msg: &[u8]) {
        // SysEx is not passed to plugins (Plan Ruling 10).
        if msg.first() == Some(&0xF0) {
            return;
        }
        if let Some(slot) = self.events.get_mut(self.count) {
            *slot = MidiEvent::new(offset, msg);
            self.count += 1;
        }
    }

    fn render(&mut self, frames: u32, out: &mut StereoBuf) {
        self.render_outputs(frames, std::slice::from_mut(out), &BlockInfo::default());
    }

    fn render_outputs(&mut self, frames: u32, outs: &mut [StereoBuf], info: &BlockInfo) {
        let n = frames.min(self.core.max_frames);
        let events = self.events.get_mut(..self.count).unwrap_or(&mut []);
        sort_by_frame(events);
        let status = self.core.run(n, &mut [], &mut [], events, info);
        self.count = 0;
        for (k, o) in outs.iter_mut().enumerate() {
            if status == ProcessStatus::Error {
                o.clear(frames);
            } else {
                self.core.write_port(k, o, n as usize);
            }
        }
    }

    fn reset(&mut self) {
        self.count = 0;
        if let Some(p) = self.core.proc.as_mut() {
            p.reset();
        }
    }

    fn latency(&self) -> u32 {
        self.core.latency
    }

    fn outputs(&self) -> u16 {
        self.outputs
    }
}
VIZDAW_EOF
```

Create `crates/core-host/src/shelf.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p crates/core-host/src
cat > crates/core-host/src/shelf.rs <<'VIZDAW_EOF'
//! Activated processors waiting for the engine: the host puts adapters
//! here, and [`ShelfFactory`] (the engine's `InsertFactory`) takes them when
//! a load needs a new instance (Plan Ruling 3). Control thread only.

use crate::adapt::{PluginEffect, PluginSynth};
use core_engine::backend::InstrumentNode;
use core_engine::graph::{EffectNode, InsertFactory};
use core_model::ids::PluginInstanceId;
use core_model::mixer::{PluginFormat, PluginRef, PluginSlot};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};

/// A ready processor.
pub enum Ready {
    /// For an insert slot.
    Effect(PluginEffect),
    /// For a plugin instrument destination.
    Instrument(PluginSynth),
}

struct Entry {
    format: PluginFormat,
    plugin: String,
    max_frames: u32,
    ready: Ready,
}

#[derive(Default)]
struct Inner {
    ready: BTreeMap<PluginInstanceId, Entry>,
    /// Why an instance is not on the shelf (loading, failed, disabled).
    why: BTreeMap<PluginInstanceId, String>,
}

/// The shelf, shared between the host and the engine's factory.
#[derive(Clone, Default)]
pub struct Shelf(Arc<Mutex<Inner>>);

impl std::fmt::Debug for Shelf {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Shelf")
    }
}

impl Shelf {
    /// An empty shelf.
    pub fn new() -> Shelf {
        Shelf::default()
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Puts instance `id`'s processor up for `plugin`, made for blocks of
    /// up to `max_frames`; returns the one it replaces.
    pub fn put(
        &self,
        id: PluginInstanceId,
        plugin: &PluginRef,
        max_frames: u32,
        ready: Ready,
    ) -> Option<Ready> {
        let mut g = self.lock();
        g.why.remove(&id);
        g.ready
            .insert(
                id,
                Entry {
                    format: plugin.format,
                    plugin: plugin.id.clone(),
                    max_frames,
                    ready,
                },
            )
            .map(|e| e.ready)
    }

    /// Records why `id` has no processor (shown as the placeholder reason).
    pub fn set_reason(&self, id: PluginInstanceId, why: impl Into<String>) {
        self.lock().why.insert(id, why.into());
    }

    /// Forgets the reason for `id`.
    pub fn clear_reason(&self, id: PluginInstanceId) {
        self.lock().why.remove(&id);
    }

    /// Whether `id` has a processor waiting.
    pub fn has(&self, id: PluginInstanceId) -> bool {
        self.lock().ready.contains_key(&id)
    }

    /// Takes every processor whose instance `keep` rejects (the document no
    /// longer has it).
    pub fn take_unwanted(&self, keep: impl Fn(PluginInstanceId) -> bool) -> Vec<Ready> {
        let mut g = self.lock();
        let gone: Vec<PluginInstanceId> = g.ready.keys().copied().filter(|k| !keep(*k)).collect();
        gone.into_iter()
            .filter_map(|k| g.ready.remove(&k).map(|e| e.ready))
            .collect()
    }

    /// The engine's factory for this shelf.
    pub fn factory(&self) -> ShelfFactory {
        ShelfFactory(self.clone())
    }

    fn take(&self, slot: &PluginSlot, max_frames: u32, instrument: bool) -> Result<Ready, String> {
        let mut g = self.lock();
        let fits = g.ready.get(&slot.id).is_some_and(|e| {
            e.format == slot.plugin.format
                && e.plugin == slot.plugin.id
                && e.max_frames >= max_frames
                && matches!(e.ready, Ready::Instrument(_)) == instrument
        });
        if fits {
            if let Some(e) = g.ready.remove(&slot.id) {
                return Ok(e.ready);
            }
        }
        Err(g
            .why
            .get(&slot.id)
            .cloned()
            .unwrap_or_else(|| format!("{} is not loaded", slot.plugin.name)))
    }
}

/// The engine's `InsertFactory`: hands out shelved processors.
#[derive(Clone, Debug)]
pub struct ShelfFactory(Shelf);

impl InsertFactory for ShelfFactory {
    fn make(
        &mut self,
        slot: &PluginSlot,
        _sample_rate: f64,
        max_frames: u32,
    ) -> Result<Box<dyn EffectNode>, String> {
        match self.0.take(slot, max_frames, false)? {
            Ready::Effect(e) => Ok(Box::new(e)),
            Ready::Instrument(_) => Err("not an effect".into()),
        }
    }

    fn make_instrument(
        &mut self,
        slot: &PluginSlot,
        _sample_rate: f64,
        max_frames: u32,
    ) -> Result<Box<dyn InstrumentNode>, String> {
        match self.0.take(slot, max_frames, true)? {
            Ready::Instrument(s) => Ok(Box::new(s)),
            Ready::Effect(_) => Err("not an instrument".into()),
        }
    }
}
VIZDAW_EOF
```

Change `crates/core-host/src/lib.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-host/src/lib.rs b/crates/core-host/src/lib.rs
--- a/crates/core-host/src/lib.rs
+++ b/crates/core-host/src/lib.rs
@@ -16,12 +16,14 @@
     )
 )]
 
+pub mod adapt;
 pub mod clap;
 pub mod error;
 pub mod info;
 pub mod instance;
 pub mod process;
 pub mod queue;
+pub mod shelf;
 
 pub use error::HostError;
 pub use info::{AudioPortInfo, ParamInfo, PluginInfo, PluginKind, PortRole, Tail};
VIZDAW_EOF
```

- [ ] **Step 4: Run the tests to see them pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-host --test engine_plugins --test clap_host
```

Expected: all pass: `engine_plugins` 7 tests (`a_clap_gain_insert_scales_the_strip`, `a_four_output_synth_feeds_its_output_strips`, `the_ducker_follows_its_sidechain_source`, `a_nan_plugin_is_bypassed_and_reported_by_instance`, `plugin_latency_is_compensated`, `ui_gestures_reach_the_main_thread_and_values_reach_the_plugin`, `removed_instances_come_back_off_the_audio_thread`) and `clap_host` unchanged.

- [ ] **Step 5: Check the whole workspace**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Expected: no formatting changes left, no clippy warnings, every test passes.

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-host/src/adapt.rs \
  crates/core-host/src/shelf.rs \
  crates/core-host/tests/common/mod.rs \
  crates/core-host/tests/engine_plugins.rs \
  crates/core-host/src/lib.rs \
  crates/core-host/tests/clap_host.rs \
  Cargo.lock
git commit -m "$(cat <<'MSG'
feat(core-host): plugin processors as engine inserts and instruments through a shelf

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
MSG
)"
```


---

### Task 8: PluginHost: loading, overlay, gestures, capture, restarts

**Files:**
- Create: `crates/core-host/src/host.rs`
- Create: `crates/core-host/tests/host.rs`
- Modify: `crates/core-host/src/adapt.rs`
- Modify: `crates/core-host/src/lib.rs`
- Modify: `crates/core-host/src/shelf.rs`
- Modify: `crates/core-model/src/mixer.rs`
- Lockfile: `Cargo.lock` (cargo updates it when dependencies change)

**Interfaces:**
- Consumes: Tasks 6–7; `core_model::history::History::{execute, capture_state}`, `MixerCmd::SetPluginParams`; `Engine::{load, restart_instance, block_frames}`.
- Produces: `core_host::PluginHost` with `new(f64, u32) -> Result<Self, HostError>`, `add_format`, `register_path(Format, &str, PathBuf)`, `set_unavailable(Format, &str, Option<String>)`, `factory() -> ShelfFactory`, `config() -> (f64, u32)`, `pending()`, `live_count()`, `retiring()`, `instance(id) -> Option<&mut dyn PluginInstance>`, `failure(id) -> Option<&str>`, `set_param(id, ParamId, f64)`, `sync(&Document) -> bool`, `idle() -> IdleReport`, `reconfigure(f64, u32) -> Vec<PluginInstanceId>`, `service(&mut Engine, &Document, SeqId) -> Result<IdleReport, EngineError>`, `wait_ready(Duration) -> IdleReport`, `capture(&mut History, id) -> Result<StateBlobId, HostError>`, `capture_all`, `commit_gestures(&mut History, &[Gesture])`; `Gesture { slot, values }`, `IdleReport { loaded, failed, restart, gestures, rejected_state, reload, compiled }`; `Ready::{id, into_processor}`, `PluginEffect/PluginSynth::{id, into_processor}`; `core_model::mixer::PluginFormat: Ord`.

The orchestration of spec §5/§6 (Rulings 2, 3, 5, 15, 16). `sync` diffs the document's slots (instrument slots and inserts) against the live instances: new or changed slots start loading (the format's `prepare` job runs on the `vizdaw-plugin-loader` thread; unavailable plugins fail at once with the catalog's reason), removed slots are taken back from the shelf or wait in `retiring` for the engine to return their processor, and running instances get state and overlay changes. `idle` finishes one main-thread load per call (state, then overlay, then activate and shelve), deactivates returned processors (re-activating those of live slots), runs plugin callbacks, collects restarts and groups gestures. `service` is the app's timer step with an engine. Dropping the host after the engine deactivates everything the engine gave back.

- [ ] **Step 1: Write the failing tests**


Create `crates/core-host/tests/host.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p crates/core-host/tests
cat > crates/core-host/tests/host.rs <<'VIZDAW_EOF'
//! The plugin host: loads a document's plugins (loader thread, then the
//! main thread), feeds the overlay, turns gestures into undoable commands,
//! captures state, restarts on latency changes and follows the engine's
//! block size.

mod common;

use common::plugin;
use core_engine::backend::AudioBackend;
use core_engine::engine::{Engine, EngineConfig, find_sequence};
use core_engine::fixtures::{Builder, demo_document};
use core_engine::offline::{OfflineBackend, OfflineSinks};
use core_host::{PluginHost, PluginKind};
use core_model::command::{Command, MixerCmd};
use core_model::document::Document;
use core_model::history::History;
use core_model::ids::{PluginInstanceId, SeqId};
use core_model::mixer::{ParamId, PluginFormat, StripKind};
use core_model::selection::ViewFilter;
use std::time::Duration;

const SR: f64 = 48_000.0;
const WAIT: Duration = Duration::from_secs(10);

fn host(max: u32) -> PluginHost {
    let mut h = PluginHost::new(SR, max).unwrap();
    for (p, _) in core_host::clap::list_bundle(&test_plugins::bundle()).unwrap() {
        h.register_path(PluginFormat::Clap, &p.id, test_plugins::bundle());
    }
    h
}

fn engine(h: &PluginHost, max: u32) -> (Engine, OfflineBackend) {
    let mut cfg = EngineConfig::new(SR);
    cfg.max_frames = max;
    let (mut e, r) = Engine::new(cfg, Box::new(OfflineSinks::new(1 << 16).with_tone())).unwrap();
    e.set_insert_factory(Box::new(h.factory()));
    let mut b = OfflineBackend::new(SR, 256);
    b.start(r).unwrap();
    (e, b)
}

/// The demo with a Test Gain insert on the built-in synth's strip.
fn with_gain() -> (History, SeqId, PluginInstanceId) {
    let doc = demo_document().unwrap();
    let song = find_sequence(&doc, "Song").unwrap();
    let mut b = Builder::from_doc(doc);
    let synth = b.strip_of(StripKind::BuiltinSynth).unwrap();
    let id = b.insert(synth, plugin("vizdaw.test.gain")).unwrap();
    (History::new(b.finish()), song, id)
}

/// Peak of the next `frames` frames.
fn peak(b: &mut OfflineBackend, frames: usize) -> f32 {
    let mut p = 0.0f32;
    let mut done = 0;
    while done < frames {
        b.block(256);
        p = b.output(256).0.iter().fold(p, |m, x| m.max(x.abs()));
        done += 256;
    }
    p
}

fn run(h: &mut History, cmd: MixerCmd) {
    h.execute(&Command::Mixer(cmd), &ViewFilter::default())
        .unwrap();
}

#[test]
fn loads_the_documents_plugins_and_plays_them() {
    let (hist, song, gain) = with_gain();
    let mut b = Builder::from_doc(hist.doc().clone());
    let (d, synth) = b
        .plugin_dest("Synth4", plugin("vizdaw.test.synth4"))
        .unwrap();
    let lane = b.lane(song, "Keys", Some(d)).unwrap();
    b.notes(song, lane, &[(0, 69, 0xFFFF, 15_360)]).unwrap();
    let doc = b.finish();
    let mut host = host(4096);
    assert!(!host.sync(&doc), "two loads in progress");
    assert_eq!(host.pending(), 2);
    let r = host.wait_ready(WAIT);
    assert_eq!(r.loaded.len(), 2, "{:?}", r.failed);
    assert!(r.reload);
    assert_eq!(
        host.instance(synth).unwrap().info().kind,
        PluginKind::Instrument
    );
    let (mut engine, mut be) = engine(&host, 4096);
    let report = engine.load(&doc, song).unwrap();
    assert!(report.graph.placeholders.is_empty(), "{:?}", report.graph);
    assert!(report.unroutable.is_empty(), "{:?}", report.warnings);
    engine.play().unwrap();
    assert!(peak(&mut be, 9600) > 0.0);
    assert!(host.instance(gain).is_some());
    drop(engine);
    drop(be);
}

#[test]
fn overlay_edits_and_their_undo_reach_the_running_plugin() {
    let (mut hist, song, gain) = with_gain();
    let mut host = host(4096);
    host.sync(hist.doc());
    host.wait_ready(WAIT);
    let (mut engine, mut be) = engine(&host, 4096);
    engine.load(hist.doc(), song).unwrap();
    engine.play().unwrap();
    peak(&mut be, 24_000);
    let full = peak(&mut be, 24_000);
    run(
        &mut hist,
        MixerCmd::SetPluginParams {
            slot: gain,
            params: vec![(ParamId(0), 0.0)],
        },
    );
    host.sync(hist.doc());
    assert_eq!(peak(&mut be, 24_000), 0.0, "gain 0 silences it");
    hist.undo().unwrap();
    host.sync(hist.doc());
    let back = peak(&mut be, 24_000);
    assert!(
        back > 0.5 * full,
        "undo restores the base value: {back} vs {full}"
    );
    drop(engine);
}

#[test]
fn ui_gestures_become_undoable_commands() {
    let (mut hist, song, gain) = with_gain();
    let mut host = host(4096);
    host.sync(hist.doc());
    host.wait_ready(WAIT);
    let (mut engine, mut be) = engine(&host, 4096);
    engine.load(hist.doc(), song).unwrap();
    // "Wiggle" makes the plugin perform a gesture on Gain.
    host.set_param(gain, ParamId(2), 0.4);
    peak(&mut be, 512);
    let r = host.service(&mut engine, hist.doc(), song).unwrap();
    assert_eq!(r.gestures.len(), 1);
    assert_eq!(r.gestures[0].values, vec![(ParamId(0), 0.4)]);
    PluginHost::commit_gestures(&mut hist, &r.gestures).unwrap();
    let slot = hist.doc().plugin_slot(gain).unwrap();
    assert_eq!(slot.params.get(&ParamId(0)), Some(&0.4));
    assert_eq!(hist.undo_label(), Some("Change Plugin Parameter"));
    hist.undo().unwrap();
    assert!(hist.doc().plugin_slot(gain).unwrap().params.is_empty());
    drop(engine);
}

#[test]
fn capture_stores_the_state_and_a_new_host_restores_it() {
    let (mut hist, _song, gain) = with_gain();
    run(
        &mut hist,
        MixerCmd::SetPluginParams {
            slot: gain,
            params: vec![(ParamId(0), 0.3)],
        },
    );
    let depth = hist.undo_depth();
    let doc_before: Document;
    {
        let mut host = host(4096);
        host.sync(hist.doc());
        host.wait_ready(WAIT);
        let blob = host.capture(&mut hist, gain).unwrap();
        let slot = hist.doc().plugin_slot(gain).unwrap();
        assert_eq!(slot.state, Some(blob));
        assert!(
            slot.params.is_empty(),
            "the overlay is folded into the state"
        );
        assert_eq!(hist.undo_depth(), depth, "no undo step");
        doc_before = hist.doc().clone();
    }
    let mut fresh = host(4096);
    fresh.sync(&doc_before);
    let r = fresh.wait_ready(WAIT);
    assert!(r.rejected_state.is_empty());
    assert_eq!(
        fresh.instance(gain).unwrap().param_value(ParamId(0)),
        Some(0.3)
    );
}

#[test]
fn a_latency_change_restarts_the_plugin_and_recompiles() {
    let (hist, song, gain) = with_gain();
    let mut host = host(4096);
    host.sync(hist.doc());
    host.wait_ready(WAIT);
    let (mut engine, mut be) = engine(&host, 4096);
    engine.load(hist.doc(), song).unwrap();
    assert_eq!(engine.latency_frames(), 0);
    host.set_param(gain, ParamId(1), 256.0);
    let mut latency = 0;
    for _ in 0..500 {
        peak(&mut be, 256);
        host.service(&mut engine, hist.doc(), song).unwrap();
        latency = engine.latency_frames();
        if latency == 256 {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(latency, 256);
    assert_eq!(host.instance(gain).unwrap().info().latency, 256);
    drop(engine);
}

#[test]
fn unknown_and_unavailable_plugins_are_placeholders_with_a_reason() {
    let doc = demo_document().unwrap();
    let song = find_sequence(&doc, "Song").unwrap();
    let mut b = Builder::from_doc(doc);
    let synth = b.strip_of(StripKind::BuiltinSynth).unwrap();
    let nope = b.insert(synth, plugin("vizdaw.test.nope")).unwrap();
    let off = b.insert(synth, plugin("vizdaw.test.gain")).unwrap();
    let doc = b.finish();
    let mut host = host(4096);
    host.set_unavailable(
        PluginFormat::Clap,
        "vizdaw.test.gain",
        Some("Disabled in Plugin Manager".into()),
    );
    host.sync(&doc);
    let r = host.wait_ready(WAIT);
    assert_eq!(r.failed.len(), 1);
    assert!(r.failed[0].1.contains("not found"), "{:?}", r.failed);
    let (mut engine, _be) = engine(&host, 4096);
    let report = engine.load(&doc, song).unwrap();
    let why: Vec<(PluginInstanceId, String)> = report.graph.placeholders.clone();
    assert!(
        why.iter()
            .any(|(id, w)| *id == nope && w.contains("not found"))
    );
    assert!(
        why.iter()
            .any(|(id, w)| *id == off && w == "Disabled in Plugin Manager")
    );
    // Enabling it loads the waiting slot.
    host.set_unavailable(PluginFormat::Clap, "vizdaw.test.gain", None);
    host.sync(&doc);
    assert_eq!(host.wait_ready(WAIT).loaded, vec![off]);
    drop(engine);
}

#[test]
fn removed_instances_are_deactivated_and_dropped() {
    let (mut hist, song, gain) = with_gain();
    let mut host = host(4096);
    host.sync(hist.doc());
    host.wait_ready(WAIT);
    let (mut engine, mut be) = engine(&host, 4096);
    engine.load(hist.doc(), song).unwrap();
    peak(&mut be, 512);
    run(&mut hist, MixerCmd::RemoveInsert { slot: gain });
    host.sync(hist.doc());
    engine.load(hist.doc(), song).unwrap();
    assert_eq!(host.live_count(), 0);
    assert_eq!(host.retiring(), 1, "its processor is still in the engine");
    for _ in 0..500 {
        peak(&mut be, 256);
        host.idle();
        if host.retiring() == 0 {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(host.retiring(), 0, "given back and deactivated");
    drop(engine);
}

#[test]
fn a_larger_block_size_reactivates_plugins() {
    let (hist, song, _gain) = with_gain();
    let mut host = host(512);
    host.sync(hist.doc());
    host.wait_ready(WAIT);
    let (mut engine, mut be) = engine(&host, 512);
    engine.load(hist.doc(), song).unwrap();
    engine.play().unwrap();
    peak(&mut be, 512);
    let mut r = be.stop().unwrap();
    r.prepare(2048);
    be.start(r).unwrap();
    let mut done = false;
    for _ in 0..500 {
        let rep = host.service(&mut engine, hist.doc(), song).unwrap();
        if let Some(c) = rep.compiled {
            done = host.config().1 == 2048 && c.graph.placeholders.is_empty();
        }
        be.block(2048);
        if done {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(done, "plugins re-activated for 2048-frame blocks");
    assert_eq!(engine.counters().xruns, 0);
    drop(engine);
}
VIZDAW_EOF
```

- [ ] **Step 2: Run them to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-host --test host
```

Expected: `unresolved import `core_host::PluginHost`` (or `no `PluginHost` in the root`).

- [ ] **Step 3: Implement**


Create `crates/core-host/src/host.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p crates/core-host/src
cat > crates/core-host/src/host.rs <<'VIZDAW_EOF'
//! The plugin host (spec §5, §6): keeps one instance per plugin slot of the
//! document, loads new ones (format work off the main thread on the loader
//! thread, the rest on the main thread, Plan Ruling 2), activates them and
//! shelves their processors for the engine, feeds the parameter overlay to
//! them, turns UI gestures into undoable commands, captures state, and
//! restarts plugins that ask for it. Main thread only (it is not `Send`).

use crate::adapt::{PluginEffect, PluginSynth, Returned};
use crate::clap::ClapFormat;
use crate::error::HostError;
use crate::instance::{LoadRequest, PluginFormat, PluginInstance, Prepared};
use crate::process::ParamEvent;
use crate::queue::{GestureReceiver, ParamSender, param_queues};
use crate::shelf::{Ready, Shelf, ShelfFactory};
use core_engine::EngineError;
use core_engine::engine::Engine;
use core_engine::schedule::CompileReport;
use core_model::command::{Command, MixerCmd};
use core_model::document::{DestKind, Document};
use core_model::history::History;
use core_model::ids::{PluginInstanceId, SeqId, StateBlobId};
use core_model::mixer::{ParamId, PluginFormat as Format, PluginRef, PluginSlot};
use core_model::selection::ViewFilter;
use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// One parameter gesture from a plugin's own UI: an undoable
/// `SetPluginParams` (spec §4).
#[derive(Debug, Clone, PartialEq)]
pub struct Gesture {
    /// The instance.
    pub slot: PluginInstanceId,
    /// Final value per parameter, in first-touched order.
    pub values: Vec<(ParamId, f64)>,
}

/// What [`PluginHost::idle`] did.
#[derive(Debug, Default)]
pub struct IdleReport {
    /// Instances that finished loading and are shelved.
    pub loaded: Vec<PluginInstanceId>,
    /// Instances that could not load, and why (they play as placeholders).
    pub failed: Vec<(PluginInstanceId, String)>,
    /// Instances that asked to be restarted (the engine must drop them).
    pub restart: Vec<PluginInstanceId>,
    /// Finished UI gestures.
    pub gestures: Vec<Gesture>,
    /// Saved states a plugin rejected on load (it plays with its defaults).
    pub rejected_state: Vec<(PluginInstanceId, String)>,
    /// Processors were shelved: the engine should load again.
    pub reload: bool,
    /// The engine load `PluginHost::service` ran, if any.
    pub compiled: Option<CompileReport>,
}

impl IdleReport {
    fn merge(&mut self, o: IdleReport) {
        self.loaded.extend(o.loaded);
        self.failed.extend(o.failed);
        self.restart.extend(o.restart);
        self.gestures.extend(o.gestures);
        self.rejected_state.extend(o.rejected_state);
        self.reload |= o.reload;
        if o.compiled.is_some() {
            self.compiled = o.compiled;
        }
    }
}

/// What the document wants of a slot.
#[derive(Clone)]
struct Want {
    plugin: PluginRef,
    instrument: bool,
    state: Option<(StateBlobId, Arc<[u8]>)>,
    params: BTreeMap<ParamId, f64>,
}

impl Want {
    fn of(doc: &Document, slot: &PluginSlot, instrument: bool) -> Want {
        Want {
            plugin: slot.plugin.clone(),
            instrument,
            state: slot
                .state
                .and_then(|id| doc.state_blob(id).map(|b| (id, Arc::from(b)))),
            params: slot.params.iter().map(|(k, v)| (*k, *v)).collect(),
        }
    }
}

/// A live instance and its main-thread ends.
struct Live {
    inst: Box<dyn PluginInstance>,
    params: ParamSender,
    gestures: GestureReceiver,
    /// The processor is out (on the shelf or in the engine).
    active: bool,
    /// The state blob last loaded into it.
    applied_state: Option<StateBlobId>,
    /// Parameter values before the overlay (to restore when an overlay
    /// entry goes away, e.g. on undo).
    base: BTreeMap<ParamId, f64>,
    /// Overlay values already sent.
    sent: BTreeMap<ParamId, f64>,
    /// Values of the gesture in progress.
    gesture: Vec<(ParamId, f64)>,
    open: usize,
    /// Must be activated again when its processor comes back.
    reactivate: bool,
}

enum State {
    /// Waiting for the loader or for the main-thread step.
    Loading(u64),
    /// Running.
    Live(Box<Live>),
    /// Could not load (or is disabled), with why.
    Failed(String),
}

struct Slot {
    want: Want,
    state: State,
}

/// A job for the loader thread.
type Job = (PluginInstanceId, u64, crate::instance::PrepareJob);
type Done = (PluginInstanceId, u64, Result<Prepared, HostError>);

/// The plugin host. Create it on the main thread; drop the engine before
/// it (processors in the engine come back through it).
pub struct PluginHost {
    formats: Vec<Box<dyn PluginFormat>>,
    paths: BTreeMap<(Format, String), PathBuf>,
    unavailable: BTreeMap<(Format, String), String>,
    sample_rate: f64,
    max_frames: u32,
    slots: BTreeMap<PluginInstanceId, Slot>,
    /// Instances removed from the document whose processor is still out.
    retiring: BTreeMap<PluginInstanceId, Box<dyn PluginInstance>>,
    /// Prepared loads waiting for their main-thread step.
    finishing: VecDeque<(PluginInstanceId, Prepared)>,
    generation: u64,
    shelf: Shelf,
    back_tx: Sender<Returned>,
    back_rx: Receiver<Returned>,
    jobs: Option<Sender<Job>>,
    done: Receiver<Done>,
    loader: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for PluginHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginHost")
            .field("slots", &self.slots.len())
            .finish_non_exhaustive()
    }
}

impl PluginHost {
    /// A host with the CLAP format, activating plugins at `sample_rate`
    /// for blocks of up to `max_frames`; starts the loader thread.
    pub fn new(sample_rate: f64, max_frames: u32) -> Result<PluginHost, HostError> {
        let (jobs, job_rx) = channel::<Job>();
        let (done_tx, done) = channel::<Done>();
        let loader = std::thread::Builder::new()
            .name("vizdaw-plugin-loader".into())
            .spawn(move || {
                while let Ok((id, generation, job)) = job_rx.recv() {
                    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job))
                        .unwrap_or_else(|_| {
                            Err(HostError::LoadFailed("the loader panicked".into()))
                        });
                    if done_tx.send((id, generation, r)).is_err() {
                        break;
                    }
                }
            })
            .map_err(|e| HostError::LoadFailed(format!("cannot start the loader: {e}")))?;
        let (back_tx, back_rx) = channel();
        Ok(PluginHost {
            formats: vec![Box::new(ClapFormat::new())],
            paths: BTreeMap::new(),
            unavailable: BTreeMap::new(),
            sample_rate,
            max_frames: max_frames.max(1),
            slots: BTreeMap::new(),
            retiring: BTreeMap::new(),
            finishing: VecDeque::new(),
            generation: 0,
            shelf: Shelf::new(),
            back_tx,
            back_rx,
            jobs: Some(jobs),
            done,
            loader: Some(loader),
        })
    }

    /// Adds a format (AU on macOS).
    pub fn add_format(&mut self, f: Box<dyn PluginFormat>) {
        self.formats.push(f);
    }

    /// Says where plugin `id` of `format` lives (the catalog does this).
    pub fn register_path(&mut self, format: Format, id: &str, path: PathBuf) {
        self.paths.insert((format, id.to_string()), path);
    }

    /// Marks a plugin as unavailable (disabled or blocklisted); its slots
    /// become placeholders showing `why` (spec §9). `None` makes it
    /// available again (its slots load at the next `sync`).
    pub fn set_unavailable(&mut self, format: Format, id: &str, why: Option<String>) {
        let key = (format, id.to_string());
        match why {
            Some(w) => {
                self.unavailable.insert(key, w);
            }
            None => {
                self.unavailable.remove(&key);
                for s in self.slots.values_mut() {
                    if s.want.plugin.format == format
                        && s.want.plugin.id == id
                        && matches!(s.state, State::Failed(_))
                    {
                        // `sync` loads it again.
                        s.state = State::Failed(String::new());
                    }
                }
            }
        }
    }

    /// The engine's insert factory (install it with
    /// `Engine::set_insert_factory`).
    pub fn factory(&self) -> ShelfFactory {
        self.shelf.factory()
    }

    /// Rate and block size instances are activated for.
    pub fn config(&self) -> (f64, u32) {
        (self.sample_rate, self.max_frames)
    }

    /// Loads still in progress.
    pub fn pending(&self) -> usize {
        self.slots
            .values()
            .filter(|s| matches!(s.state, State::Loading(_)))
            .count()
    }

    /// Live instances (loaded, not failed).
    pub fn live_count(&self) -> usize {
        self.slots
            .values()
            .filter(|s| matches!(s.state, State::Live(_)))
            .count()
    }

    /// Removed instances whose processor the engine has not given back.
    pub fn retiring(&self) -> usize {
        self.retiring.len()
    }

    /// Instance `id`, if loaded (for its editor and parameter list).
    pub fn instance(&mut self, id: PluginInstanceId) -> Option<&mut dyn PluginInstance> {
        match self.slots.get_mut(&id).map(|s| &mut s.state) {
            Some(State::Live(l)) => Some(l.inst.as_mut()),
            _ => None,
        }
    }

    /// Why slot `id` has no instance, if it has none.
    pub fn failure(&self, id: PluginInstanceId) -> Option<&str> {
        match self.slots.get(&id).map(|s| &s.state) {
            Some(State::Failed(w)) => Some(w),
            _ => None,
        }
    }

    /// Sets a parameter now (a mixer control): sent to the running plugin;
    /// the app also records it in the overlay with `SetPluginParams`.
    pub fn set_param(&mut self, id: PluginInstanceId, param: ParamId, value: f64) {
        if let Some(State::Live(l)) = self.slots.get_mut(&id).map(|s| &mut s.state) {
            send(l, param, value);
        }
    }

    /// Call after every committed edit (and on open): starts loading the
    /// slots `doc` gained, drops the ones it lost, and sends overlay and
    /// state changes to running instances. Returns true when no load is in
    /// progress, i.e. the engine may load `doc` now.
    pub fn sync(&mut self, doc: &Document) -> bool {
        let mut wanted: BTreeMap<PluginInstanceId, Want> = BTreeMap::new();
        for d in doc.instruments().values() {
            if let DestKind::PluginInstrument { slot } = &d.kind {
                wanted.insert(slot.id, Want::of(doc, slot, true));
            }
        }
        for (_, _, slot) in doc.mixer().inserts() {
            wanted.insert(slot.id, Want::of(doc, slot, false));
        }
        // Slots that went, or now hold another plugin.
        let gone: Vec<PluginInstanceId> = self
            .slots
            .iter()
            .filter(|(id, s)| {
                wanted.get(id).is_none_or(|w| {
                    w.plugin.format != s.want.plugin.format
                        || w.plugin.id != s.want.plugin.id
                        || w.instrument != s.want.instrument
                })
            })
            .map(|(id, _)| *id)
            .collect();
        for id in gone {
            self.remove(id);
        }
        for (id, want) in wanted {
            let key = (want.plugin.format, want.plugin.id.clone());
            let unavailable = self.unavailable.get(&key).cloned();
            match self.slots.get_mut(&id) {
                Some(s) => {
                    s.want = want;
                    match &mut s.state {
                        State::Live(l) => apply_want(l, &s.want),
                        State::Failed(w) if w.is_empty() => {
                            self.start_load(id);
                        }
                        _ => {}
                    }
                }
                None => {
                    self.slots.insert(
                        id,
                        Slot {
                            want,
                            state: State::Failed(String::new()),
                        },
                    );
                    match unavailable {
                        Some(why) => self.fail(id, why),
                        None => self.start_load(id),
                    }
                }
            }
        }
        self.pending() == 0
    }

    fn fail(&mut self, id: PluginInstanceId, why: String) {
        self.shelf.set_reason(id, why.clone());
        if let Some(s) = self.slots.get_mut(&id) {
            s.state = State::Failed(why);
        }
    }

    fn start_load(&mut self, id: PluginInstanceId) {
        let Some(want) = self.slots.get(&id).map(|s| s.want.clone()) else {
            return;
        };
        if let Some(why) = self
            .unavailable
            .get(&(want.plugin.format, want.plugin.id.clone()))
            .cloned()
        {
            self.fail(id, why);
            return;
        }
        let req = self.request(&want.plugin);
        let Some(format) = self
            .formats
            .iter()
            .find(|f| f.format() == want.plugin.format)
        else {
            self.fail(
                id,
                format!("{:?} plugins are not supported here", want.plugin.format),
            );
            return;
        };
        let job = format.prepare(&req);
        self.generation += 1;
        let generation = self.generation;
        self.shelf
            .set_reason(id, format!("{} is loading", want.plugin.name));
        if let Some(s) = self.slots.get_mut(&id) {
            s.state = State::Loading(generation);
        }
        let sent = self
            .jobs
            .as_ref()
            .is_some_and(|j| j.send((id, generation, job)).is_ok());
        if !sent {
            self.fail(id, "the plugin loader stopped".into());
        }
    }

    fn request(&self, plugin: &PluginRef) -> LoadRequest {
        LoadRequest {
            plugin: plugin.clone(),
            path: self.paths.get(&(plugin.format, plugin.id.clone())).cloned(),
            sample_rate: self.sample_rate,
            max_frames: self.max_frames,
        }
    }

    /// Drops slot `id`: an instance whose processor is out waits in
    /// `retiring` for it.
    fn remove(&mut self, id: PluginInstanceId) {
        self.shelf.clear_reason(id);
        let Some(s) = self.slots.remove(&id) else {
            return;
        };
        if let State::Live(mut l) = s.state {
            let shelved = self.shelf.take_unwanted(|k| k != id);
            let mut back = false;
            for p in shelved.into_iter().filter_map(Ready::into_processor) {
                back = true;
                l.inst.deactivate(p);
            }
            if l.active && !back {
                self.retiring.insert(id, l.inst);
            }
        }
    }

    /// Main-thread servicing (`host_idle`, 60 Hz; spec §5): finishes one
    /// load, deactivates processors the engine gave back, runs plugin
    /// callbacks, and collects restarts and gestures.
    pub fn idle(&mut self) -> IdleReport {
        let mut r = IdleReport::default();
        while let Ok((id, generation, result)) = self.done.try_recv() {
            let current = matches!(
                self.slots.get(&id).map(|s| &s.state),
                Some(State::Loading(g)) if *g == generation
            );
            if !current {
                continue;
            }
            match result {
                Ok(p) => self.finishing.push_back((id, p)),
                Err(e) => {
                    let why = format!("failed: {e}");
                    r.failed.push((id, why.clone()));
                    self.fail(id, why);
                }
            }
        }
        if let Some((id, prepared)) = self.finishing.pop_front() {
            self.finish(id, prepared, &mut r);
        }
        while let Ok(back) = self.back_rx.try_recv() {
            self.returned(back, &mut r);
        }
        let ids: Vec<PluginInstanceId> = self.slots.keys().copied().collect();
        for id in ids {
            self.service_instance(id, &mut r);
        }
        r
    }

    fn finish(&mut self, id: PluginInstanceId, prepared: Prepared, r: &mut IdleReport) {
        let Some(want) = self.slots.get(&id).map(|s| s.want.clone()) else {
            return;
        };
        let req = self.request(&want.plugin);
        let made = match self
            .formats
            .iter_mut()
            .find(|f| f.format() == want.plugin.format)
        {
            Some(f) => f.finish(&req, prepared),
            None => Err(HostError::NotFound(want.plugin.id.clone())),
        };
        let inst = match made {
            Ok(i) => i,
            Err(e) => {
                let why = format!("failed: {e}");
                r.failed.push((id, why.clone()));
                self.fail(id, why);
                return;
            }
        };
        let ((params, gestures), _) = param_queues();
        let mut live = Box::new(Live {
            inst,
            params,
            gestures,
            active: false,
            applied_state: None,
            base: BTreeMap::new(),
            sent: BTreeMap::new(),
            gesture: Vec::new(),
            open: 0,
            reactivate: false,
        });
        // State first, then the overlay (spec §4).
        if let Some((sid, bytes)) = &want.state {
            match live.inst.load_state(bytes) {
                Ok(()) => live.applied_state = Some(*sid),
                Err(e) => r.rejected_state.push((id, e.to_string())),
            }
        }
        live.base = current_values(live.inst.as_mut());
        let overlay: Vec<(ParamId, f64)> = want.params.iter().map(|(k, v)| (*k, *v)).collect();
        if live.inst.set_params(&overlay).is_ok() {
            live.sent = want.params.clone();
        }
        match self.activate(id, &mut live, want.instrument) {
            Ok(()) => {
                r.loaded.push(id);
                r.reload = true;
                if let Some(s) = self.slots.get_mut(&id) {
                    s.state = State::Live(live);
                }
            }
            Err(e) => {
                let why = format!("failed: {e}");
                r.failed.push((id, why.clone()));
                self.fail(id, why);
            }
        }
    }

    /// Activates `live` and shelves its processor with fresh queues.
    fn activate(
        &mut self,
        id: PluginInstanceId,
        live: &mut Live,
        instrument: bool,
    ) -> Result<(), HostError> {
        let proc = live.inst.activate(self.sample_rate, self.max_frames)?;
        let ((params, gestures), audio) = param_queues();
        live.params = params;
        live.gestures = gestures;
        live.active = true;
        live.reactivate = false;
        let info = live.inst.info().clone();
        let back = self.back_tx.clone();
        let ready = if instrument {
            Ready::Instrument(PluginSynth::new(
                id,
                proc,
                &info,
                self.max_frames,
                audio,
                back,
            ))
        } else {
            Ready::Effect(PluginEffect::new(
                id,
                proc,
                &info,
                self.max_frames,
                audio,
                back,
            ))
        };
        if let Some(old) = self.shelf.put(id, &info.plugin, self.max_frames, ready) {
            // Unreachable: a slot shelves one processor at a time.
            drop(old);
        }
        // Values the engine never saw must reach the new processor.
        let sent: Vec<(ParamId, f64)> = live.sent.iter().map(|(k, v)| (*k, *v)).collect();
        for (k, v) in sent {
            live.params.send(k, v);
        }
        Ok(())
    }

    fn returned(&mut self, back: Returned, r: &mut IdleReport) {
        let Returned { id, processor } = back;
        if let Some(mut inst) = self.retiring.remove(&id) {
            inst.deactivate(processor);
            return;
        }
        let instrument = self.slots.get(&id).is_some_and(|s| s.want.instrument);
        let Some(State::Live(mut live)) = self
            .slots
            .get_mut(&id)
            .map(|s| std::mem::replace(&mut s.state, State::Failed(String::new())))
        else {
            drop(processor);
            return;
        };
        live.inst.deactivate(processor);
        live.active = false;
        // A processor comes back only when the engine dropped it: a
        // restart or a new configuration. Activate it again.
        match self.activate(id, &mut live, instrument) {
            Ok(()) => {
                r.reload = true;
                if let Some(s) = self.slots.get_mut(&id) {
                    s.state = State::Live(live);
                }
            }
            Err(e) => {
                let why = format!("failed: {e}");
                r.failed.push((id, why.clone()));
                self.fail(id, why);
            }
        }
    }

    fn service_instance(&mut self, id: PluginInstanceId, r: &mut IdleReport) {
        let Some(State::Live(live)) = self.slots.get_mut(&id).map(|s| &mut s.state) else {
            return;
        };
        let ev = live.inst.idle();
        for e in live.gestures.drain() {
            match e {
                ParamEvent::Begin(_) => live.open += 1,
                ParamEvent::Value(p, v) => {
                    live.sent.insert(p, v);
                    match live.gesture.iter_mut().find(|(k, _)| *k == p) {
                        Some(x) => x.1 = v,
                        None => live.gesture.push((p, v)),
                    }
                    if live.open == 0 {
                        r.gestures.push(Gesture {
                            slot: id,
                            values: std::mem::take(&mut live.gesture),
                        });
                    }
                }
                ParamEvent::End(_) => {
                    live.open = live.open.saturating_sub(1);
                    if live.open == 0 && !live.gesture.is_empty() {
                        r.gestures.push(Gesture {
                            slot: id,
                            values: std::mem::take(&mut live.gesture),
                        });
                    }
                }
            }
        }
        if ev.restart || ev.latency_changed {
            live.reactivate = true;
            self.shelf.set_reason(id, "restarting");
            // Still on the shelf: restart it here; otherwise the engine
            // must give it back first.
            let shelved = self.shelf.take_unwanted(|k| k != id);
            if shelved.is_empty() {
                r.restart.push(id);
            } else {
                for processor in shelved.into_iter().filter_map(Ready::into_processor) {
                    let _ = self.back_tx.send(Returned { id, processor });
                }
            }
        }
    }

    /// Changes the rate or block size: every instance is activated again
    /// (those in the engine once it gives them back; `service` asks it to).
    pub fn reconfigure(&mut self, sample_rate: f64, max_frames: u32) -> Vec<PluginInstanceId> {
        self.sample_rate = sample_rate;
        self.max_frames = max_frames.max(1);
        let mut out_in_engine = Vec::new();
        let ids: Vec<PluginInstanceId> = self.slots.keys().copied().collect();
        for id in ids {
            let Some(State::Live(live)) = self.slots.get_mut(&id).map(|s| &mut s.state) else {
                continue;
            };
            if !live.active {
                continue;
            }
            live.reactivate = true;
            self.shelf.set_reason(id, "restarting");
            let shelved = self.shelf.take_unwanted(|k| k != id);
            if shelved.is_empty() {
                out_in_engine.push(id);
            }
            for processor in shelved.into_iter().filter_map(Ready::into_processor) {
                let _ = self.back_tx.send(Returned { id, processor });
            }
        }
        out_in_engine
    }

    /// The main-thread timer's step with an engine: [`PluginHost::idle`],
    /// then asks the engine to drop restarting instances, follows a grown
    /// block size, syncs `doc` and loads it again when processors were
    /// shelved or restarts are due and no load is in progress.
    pub fn service(
        &mut self,
        engine: &mut Engine,
        doc: &Document,
        seq: SeqId,
    ) -> Result<IdleReport, EngineError> {
        let mut r = self.idle();
        if engine.block_frames() > self.max_frames {
            r.restart
                .extend(self.reconfigure(self.sample_rate, engine.block_frames()));
        }
        for id in &r.restart {
            engine.restart_instance(*id);
        }
        let ready = self.sync(doc);
        if ready && (r.reload || !r.restart.is_empty()) {
            r.compiled = Some(engine.load(doc, seq)?);
        }
        Ok(r)
    }

    /// Calls [`PluginHost::idle`] until no load is in progress (or
    /// `timeout` passes); merges the reports. For tests and command-line
    /// hosts.
    pub fn wait_ready(&mut self, timeout: Duration) -> IdleReport {
        let start = Instant::now();
        let mut all = IdleReport::default();
        loop {
            all.merge(self.idle());
            if self.pending() == 0 && self.finishing.is_empty() {
                return all;
            }
            if start.elapsed() >= timeout {
                return all;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// Stores instance `id`'s state through `History::capture_state` (spec
    /// §4: on save, autosave and before removal; no undo step).
    pub fn capture(
        &mut self,
        history: &mut History,
        id: PluginInstanceId,
    ) -> Result<StateBlobId, HostError> {
        let Some(State::Live(live)) = self.slots.get_mut(&id).map(|s| &mut s.state) else {
            return Err(HostError::NotFound(format!("instance {id} is not loaded")));
        };
        let bytes = live.inst.save_state()?;
        let blob = history
            .capture_state(id, &bytes)
            .map_err(|e| HostError::StateRejected(e.to_string()))?;
        live.applied_state = Some(blob);
        // The overlay is now part of the state.
        let sent = std::mem::take(&mut live.sent);
        live.base.extend(sent);
        if let Some(s) = self.slots.get_mut(&id) {
            s.want.state = history.doc().state_blob(blob).map(|b| (blob, Arc::from(b)));
            s.want.params.clear();
        }
        Ok(blob)
    }

    /// Captures every live instance with state (before saving).
    pub fn capture_all(&mut self, history: &mut History) -> Vec<(PluginInstanceId, HostError)> {
        let ids: Vec<PluginInstanceId> = self.slots.keys().copied().collect();
        ids.into_iter()
            .filter_map(|id| match self.capture(history, id) {
                Ok(_) => None,
                Err(HostError::NotFound(_)) => None,
                Err(e) => Some((id, e)),
            })
            .collect()
    }

    /// Commits gestures as undoable `SetPluginParams` commands.
    pub fn commit_gestures(history: &mut History, gestures: &[Gesture]) -> core_model::Result<()> {
        for g in gestures {
            history.execute(
                &Command::Mixer(MixerCmd::SetPluginParams {
                    slot: g.slot,
                    params: g.values.clone(),
                }),
                &ViewFilter::default(),
            )?;
        }
        Ok(())
    }
}

/// Sends `value` to a running instance (or sets it while inactive) and
/// remembers it; a value that did not fit the queue is not remembered, so
/// the next `sync` sends it again.
fn send(l: &mut Live, param: ParamId, value: f64) {
    let ok = if l.active {
        l.params.send(param, value)
    } else {
        l.inst.set_params(&[(param, value)]).is_ok()
    };
    if ok {
        l.sent.insert(param, value);
    }
}

/// Brings a running instance in line with what the document wants: a new
/// state blob is loaded (an undo past a capture), then overlay values that
/// changed are sent and removed ones go back to their base values.
fn apply_want(l: &mut Live, want: &Want) {
    let want_state = want.state.as_ref().map(|(id, _)| *id);
    if want_state != l.applied_state {
        if let Some((sid, bytes)) = &want.state {
            if l.inst.load_state(bytes).is_ok() {
                l.applied_state = Some(*sid);
                l.base = current_values(l.inst.as_mut());
                l.sent.clear();
            }
        }
    }
    let removed: Vec<ParamId> = l
        .sent
        .keys()
        .filter(|k| !want.params.contains_key(k))
        .copied()
        .collect();
    for p in removed {
        if let Some(v) = l.base.get(&p).copied() {
            send(l, p, v);
        }
        l.sent.remove(&p);
    }
    for (p, v) in &want.params {
        if l.sent.get(p) != Some(v) {
            send(l, *p, *v);
        }
    }
}

fn current_values(inst: &mut dyn PluginInstance) -> BTreeMap<ParamId, f64> {
    let ids: Vec<ParamId> = inst.info().params.iter().map(|p| p.id).collect();
    ids.into_iter()
        .filter_map(|id| inst.param_value(id).map(|v| (id, v)))
        .collect()
}

impl Drop for PluginHost {
    fn drop(&mut self) {
        // Shelved processors and those the engine gave back are deactivated
        // with their instances; the engine must already be gone.
        let mut back: Vec<Returned> = self
            .shelf
            .take_unwanted(|_| false)
            .into_iter()
            .filter_map(|r| {
                let id = r.id();
                r.into_processor()
                    .map(|processor| Returned { id, processor })
            })
            .collect();
        back.extend(self.back_rx.try_iter());
        for Returned { id, processor } in back {
            if let Some(mut inst) = self.retiring.remove(&id) {
                inst.deactivate(processor);
            } else if let Some(State::Live(l)) = self.slots.get_mut(&id).map(|s| &mut s.state) {
                l.inst.deactivate(processor);
                l.active = false;
            }
        }
        self.slots.clear();
        self.jobs = None;
        if let Some(h) = self.loader.take() {
            let _ = h.join();
        }
    }
}
VIZDAW_EOF
```

Change `crates/core-host/src/adapt.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-host/src/adapt.rs b/crates/core-host/src/adapt.rs
--- a/crates/core-host/src/adapt.rs
+++ b/crates/core-host/src/adapt.rs
@@ -203,6 +203,18 @@ fn copy_in(dst: &mut [Box<[f32]>], src: &StereoBuf, n: usize) {
     }
 }
 
+impl PluginEffect {
+    /// The instance it serves.
+    pub fn id(&self) -> PluginInstanceId {
+        self.core.id
+    }
+
+    /// Unwraps the processor (it is not sent back).
+    pub fn into_processor(mut self) -> Option<Box<dyn AudioProcessor>> {
+        self.core.proc.take()
+    }
+}
+
 impl EffectNode for PluginEffect {
     fn latency(&self) -> u32 {
         self.core.latency
@@ -299,6 +311,18 @@ impl PluginSynth {
     }
 }
 
+impl PluginSynth {
+    /// The instance it serves.
+    pub fn id(&self) -> PluginInstanceId {
+        self.core.id
+    }
+
+    /// Unwraps the processor (it is not sent back).
+    pub fn into_processor(mut self) -> Option<Box<dyn AudioProcessor>> {
+        self.core.proc.take()
+    }
+}
+
 impl InstrumentNode for PluginSynth {
     fn midi(&mut self, offset: u32, msg: &[u8]) {
         // SysEx is not passed to plugins (Plan Ruling 10).
VIZDAW_EOF
```

Change `crates/core-host/src/lib.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-host/src/lib.rs b/crates/core-host/src/lib.rs
--- a/crates/core-host/src/lib.rs
+++ b/crates/core-host/src/lib.rs
@@ -19,6 +19,7 @@
 pub mod adapt;
 pub mod clap;
 pub mod error;
+pub mod host;
 pub mod info;
 pub mod instance;
 pub mod process;
@@ -26,6 +27,7 @@ pub mod queue;
 pub mod shelf;
 
 pub use error::HostError;
+pub use host::{Gesture, IdleReport, PluginHost};
 pub use info::{AudioPortInfo, ParamInfo, PluginInfo, PluginKind, PortRole, Tail};
 pub use instance::{
     AudioProcessor, InstanceEvents, LoadRequest, ParentView, PluginEditor, PluginFormat,
VIZDAW_EOF
```

Change `crates/core-host/src/shelf.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-host/src/shelf.rs b/crates/core-host/src/shelf.rs
--- a/crates/core-host/src/shelf.rs
+++ b/crates/core-host/src/shelf.rs
@@ -18,6 +18,24 @@ pub enum Ready {
     Instrument(PluginSynth),
 }
 
+impl Ready {
+    /// The instance it serves.
+    pub fn id(&self) -> PluginInstanceId {
+        match self {
+            Ready::Effect(e) => e.id(),
+            Ready::Instrument(s) => s.id(),
+        }
+    }
+
+    /// Unwraps the processor.
+    pub fn into_processor(self) -> Option<Box<dyn crate::instance::AudioProcessor>> {
+        match self {
+            Ready::Effect(e) => e.into_processor(),
+            Ready::Instrument(s) => s.into_processor(),
+        }
+    }
+}
+
 struct Entry {
     format: PluginFormat,
     plugin: String,
VIZDAW_EOF
```

Change `crates/core-model/src/mixer.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-model/src/mixer.rs b/crates/core-model/src/mixer.rs
--- a/crates/core-model/src/mixer.rs
+++ b/crates/core-model/src/mixer.rs
@@ -25,7 +25,7 @@ pub const BUILTIN_SYNTH_STRIP: &str = "Built-in Synth";
 pub struct ParamId(pub u64);
 
 /// Plugin formats VizDAW hosts.
-#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
+#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
 #[serde(rename_all = "snake_case")]
 pub enum PluginFormat {
     /// Audio Unit (v2 or v3).
VIZDAW_EOF
```

- [ ] **Step 4: Run the tests to see them pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-host --test host
```

Expected: 8 tests pass (`loads_the_documents_plugins_and_plays_them`, `overlay_edits_and_their_undo_reach_the_running_plugin`, `ui_gestures_become_undoable_commands`, `capture_stores_the_state_and_a_new_host_restores_it`, `a_latency_change_restarts_the_plugin_and_recompiles`, `unknown_and_unavailable_plugins_are_placeholders_with_a_reason`, `removed_instances_are_deactivated_and_dropped`, `a_larger_block_size_reactivates_plugins`).

- [ ] **Step 5: Check the whole workspace**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Expected: no formatting changes left, no clippy warnings, every test passes.

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-host/src/host.rs \
  crates/core-host/tests/host.rs \
  crates/core-host/src/adapt.rs \
  crates/core-host/src/lib.rs \
  crates/core-host/src/shelf.rs \
  crates/core-model/src/mixer.rs \
  Cargo.lock
git commit -m "$(cat <<'MSG'
feat(core-host): plugin host with loader thread, overlay sync, gestures, capture and restarts

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
MSG
)"
```


---

### Task 9: Zero allocations with real plugins on every render thread

**Files:**
- Create: `crates/core-host/tests/rt_alloc_plugins.rs`
- Lockfile: `Cargo.lock` (cargo updates it when dependencies change)

**Interfaces:**
- Consumes: Task 8's `PluginHost`; `core_engine::rt_check::{CountingAlloc, arm, disarm, count, set_arm_workers}`.
- Produces: `core-host/tests/rt_alloc_plugins.rs`.

Extends 3a's allocation checks to real CLAP plugins: a gain insert receiving parameter values every 50 blocks and performing a UI gesture every 400, a 4-output synth with notes on four channels and an extra output strip, a ducker sidechained from it, a NaN plugin on a send bus (bypassed after 16 blocks), three workers, varying block sizes. Nothing is implemented here; the test passes against Tasks 6–8, and Step 3 proves it can fail.

- [ ] **Step 1: Write the failing tests**


Create `crates/core-host/tests/rt_alloc_plugins.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p crates/core-host/tests
cat > crates/core-host/tests/rt_alloc_plugins.rs <<'VIZDAW_EOF'
//! Zero allocations on the callback and worker threads with real CLAP
//! plugins in the graph: inserts, a multi-output instrument, a sidechain,
//! parameter changes in, gestures out and a fault bypass.

mod common;

use common::plugin;
use core_engine::backend::AudioBackend;
use core_engine::engine::{Engine, EngineConfig, find_sequence};
use core_engine::fixtures::{Builder, Q, demo_document};
use core_engine::offline::{OfflineBackend, OfflineSinks};
use core_engine::rt_check::{self, CountingAlloc};
use core_host::PluginHost;
use core_model::command::MixerCmd;
use core_model::event::{EventBody, EventSpec};
use core_model::mixer::{AuxSend, ParamId, PluginFormat, StripKind};
use core_model::time::Tick;
use std::time::Duration;

#[global_allocator]
static ALLOC: CountingAlloc = CountingAlloc;

#[test]
fn plugins_render_without_allocating() {
    let doc = demo_document().unwrap();
    let song = find_sequence(&doc, "Song").unwrap();
    let mut b = Builder::from_doc(doc);
    let builtin = b.strip_of(StripKind::BuiltinSynth).unwrap();
    let gain = b.insert(builtin, plugin("vizdaw.test.gain")).unwrap();
    let (d, _) = b
        .plugin_dest("Synth4", plugin("vizdaw.test.synth4"))
        .unwrap();
    let lane = b.lane(song, "Keys", Some(d)).unwrap();
    let mut notes = Vec::new();
    for ch in 0..4u8 {
        let mut e = EventSpec::new(
            Tick(i64::from(ch) * Q),
            EventBody::note(60 + ch, 0xFFFF, Tick(2 * Q)),
        );
        e.group_channel = Some((0, ch));
        notes.push(e);
    }
    b.events(song, lane, notes).unwrap();
    b.mixer(MixerCmd::AddInstrumentOutput {
        dest: d,
        index: 3,
        name: "Four".into(),
    })
    .unwrap();
    let keys = b.strip_of(StripKind::Instrument(d)).unwrap();
    let duck = b.insert(builtin, plugin("vizdaw.test.ducker")).unwrap();
    b.mixer(MixerCmd::SetSidechain {
        slot: duck,
        source: Some(keys),
    })
    .unwrap();
    let verb = b.bus("Verb").unwrap();
    b.insert(verb, plugin("vizdaw.test.nan")).unwrap();
    b.mixer(MixerCmd::AddSend {
        strip: builtin,
        send: AuxSend::new(verb),
    })
    .unwrap();
    let doc = b.finish();

    let mut host = PluginHost::new(48_000.0, 4096).unwrap();
    host.register_path(
        PluginFormat::Clap,
        "vizdaw.test.gain",
        test_plugins::bundle(),
    );
    for id in ["synth4", "ducker", "nan"] {
        host.register_path(
            PluginFormat::Clap,
            &format!("vizdaw.test.{id}"),
            test_plugins::bundle(),
        );
    }
    host.sync(&doc);
    let r = host.wait_ready(Duration::from_secs(10));
    assert_eq!(r.loaded.len(), 4, "{:?}", r.failed);
    let mut cfg = EngineConfig::new(48_000.0);
    cfg.workers = 3;
    let (mut engine, renderer) =
        Engine::new(cfg, Box::new(OfflineSinks::new(1 << 20).with_tone())).unwrap();
    engine.set_insert_factory(Box::new(host.factory()));
    let report = engine.load(&doc, song).unwrap();
    assert!(report.graph.placeholders.is_empty(), "{:?}", report.graph);
    let mut be = OfflineBackend::new(48_000.0, 512);
    be.start(renderer).unwrap();
    be.reserve(4096);
    engine.set_loop(true).unwrap();
    engine.play().unwrap();
    // Warm up: first process calls start processing.
    for _ in 0..8 {
        be.block(256);
    }
    rt_check::set_arm_workers(true);
    let before = rt_check::count();
    for i in 0..2000u32 {
        if i % 50 == 0 {
            host.set_param(gain, ParamId(0), 0.5 + f64::from(i % 100) / 400.0);
        }
        if i % 400 == 0 {
            // A UI-style gesture comes out of the plugin.
            host.set_param(gain, ParamId(2), 0.75);
        }
        rt_check::arm();
        be.block(1 + (i * 37) % 1024);
        rt_check::disarm();
        if i % 100 == 0 {
            host.idle();
        }
    }
    let allocs = rt_check::count() - before;
    rt_check::set_arm_workers(false);
    assert_eq!(allocs, 0, "allocations while plugins rendered");
    assert!(engine.worker_steps() > 0, "workers rendered nodes");
    assert_eq!(
        engine.plugin_faults().len(),
        1,
        "the NaN plugin was bypassed"
    );
    drop(be.stop().unwrap());
    drop(engine);
}
VIZDAW_EOF
```

- [ ] **Step 2: Run it (it passes: Tasks 6–8 already render without allocating)**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-host --test rt_alloc_plugins
```

Expected: `plugins_render_without_allocating` passes.

- [ ] **Step 3: Prove the test catches an allocation**

Add an allocation to the CLAP processor's `process`, run the test, and restore the file:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
sed -i '' 's|        let n = ctx.frames.min(self.max_frames) as usize;|        let n = ctx.frames.min(self.max_frames) as usize; std::hint::black_box(vec![1u8; 3]);|' crates/core-host/src/clap/processor.rs
cargo test -p core-host --test rt_alloc_plugins 2>&1 | grep -E "allocations while plugins rendered|left:"
git checkout -- crates/core-host/src/clap/processor.rs
```

Expected: the test fails with `allocations while plugins rendered` and a large `left:` count; after `git checkout` the file is back as it was.

- [ ] **Step 4: Run the tests to see them pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-host --test rt_alloc_plugins
```

Expected: `plugins_render_without_allocating` passes.

- [ ] **Step 5: Check the whole workspace**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Expected: no formatting changes left, no clippy warnings, every test passes.

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-host/tests/rt_alloc_plugins.rs \
  Cargo.lock
git commit -m "$(cat <<'MSG'
test(core-host): no allocation on render threads with CLAP plugins in the graph

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
MSG
)"
```


---

### Task 10: Plugin catalog: cache, blocklist, enabled state

**Files:**
- Create: `crates/core-host/src/catalog.rs`
- Create: `crates/core-host/tests/catalog.rs`
- Modify: `crates/core-host/src/lib.rs`
- Lockfile: `Cargo.lock` (cargo updates it when dependencies change)

**Interfaces:**
- Consumes: Task 8's `PluginHost::{register_path, set_unavailable}`; Task 4's `PluginInfo`, `PluginKind`.
- Produces: `core_host::catalog::{Catalog, CatalogEntry, CatalogRow, Source, ScanStatus::{Ok, Blocklisted { reason }, NotValidated}, PluginStatus::{Ok, Disabled, Blocklisted(String), NotValidated, Missing}, CATALOG_VERSION}`; `Source::{clap_bundle(&Path), same_place}`; `Catalog::{new, default_path, load, save, entries, get, mark_found(&[Source]) -> Vec<Source>, record(&Source, Vec<CatalogEntry>), set_enabled, clear_blocklist -> Option<Source>, status, rows, browser(PluginKind), apply(&mut PluginHost)}`.

Ruling 17; the data API 3c's Plugin Manager drives (spec §9). `mark_found` takes every source discovery saw and returns those to scan (new, or another version or modification time — their old entries are dropped), marking sources that were not found as missing. `apply` registers CLAP paths with the host and marks disabled, blocklisted and missing plugins unavailable with the placeholder text spec §9 asks for ("Disabled in Plugin Manager").

- [ ] **Step 1: Write the failing tests**


Create `crates/core-host/tests/catalog.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p crates/core-host/tests
cat > crates/core-host/tests/catalog.rs <<'VIZDAW_EOF'
//! The catalog: cache keys, blocklist, enabled state, missing plugins and
//! what the host and the plugin browser get from it.

mod common;

use common::plugin;
use core_host::catalog::{Catalog, CatalogEntry, PluginStatus, ScanStatus, Source};
use core_host::{PluginHost, PluginKind};
use core_model::mixer::PluginFormat;
use std::path::PathBuf;

fn temp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("vizdaw-catalog-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn source(path: &str, mtime: u64) -> Source {
    Source {
        format: PluginFormat::Clap,
        path: PathBuf::from(path),
        version: "1.0".into(),
        mtime,
    }
}

fn entry(src: &Source, id: &str, kind: PluginKind, status: ScanStatus) -> CatalogEntry {
    let mut p = plugin(id);
    p.name = id.into();
    CatalogEntry {
        source: src.clone(),
        plugin: p,
        kind: Some(kind),
        info: None,
        status,
    }
}

fn sample() -> (Catalog, Source, Source) {
    let a = source("/p/A.clap", 10);
    let b = source("/p/B.clap", 20);
    let mut c = Catalog::new();
    c.record(
        &a,
        vec![
            entry(&a, "a.synth", PluginKind::Instrument, ScanStatus::Ok),
            entry(&a, "a.fx", PluginKind::Effect, ScanStatus::Ok),
        ],
    );
    c.record(
        &b,
        vec![entry(
            &b,
            "b.crash",
            PluginKind::Effect,
            ScanStatus::Blocklisted {
                reason: "crashed (signal 11)".into(),
            },
        )],
    );
    (c, a, b)
}

#[test]
fn saves_and_loads_and_reports_corrupt_files() {
    let dir = temp("io");
    let path = dir.join("sub/plugins.json");
    assert_eq!(
        Catalog::load(&path).unwrap(),
        Catalog::new(),
        "missing is empty"
    );
    let (mut c, _, _) = sample();
    c.set_enabled(PluginFormat::Clap, "a.fx", false);
    c.save(&path).unwrap();
    let back = Catalog::load(&path).unwrap();
    assert_eq!(back.entries(), c.entries());
    assert_eq!(
        back.status(PluginFormat::Clap, "a.fx"),
        Some(PluginStatus::Disabled)
    );
    std::fs::write(&path, "{ not json").unwrap();
    assert!(Catalog::load(&path).is_err());
}

#[test]
fn only_new_or_changed_sources_are_scanned_and_gone_ones_are_missing() {
    let (mut c, a, b) = sample();
    let new = source("/p/C.clap", 5);
    let touched = source("/p/A.clap", 11);
    let todo = c.mark_found(&[touched.clone(), new.clone()]);
    assert_eq!(todo, vec![touched, new], "A changed, C is new");
    assert!(
        c.get(PluginFormat::Clap, "a.synth").is_none(),
        "A's old scan is dropped"
    );
    assert_eq!(
        c.status(PluginFormat::Clap, "b.crash"),
        Some(PluginStatus::Missing),
        "B was not found"
    );
    // Found again unchanged: nothing to scan.
    let (mut c, _, _) = sample();
    assert!(c.mark_found(&[a, b]).is_empty());
    assert_eq!(
        c.status(PluginFormat::Clap, "a.synth"),
        Some(PluginStatus::Ok)
    );
}

#[test]
fn statuses_browser_and_clearing_the_blocklist() {
    let (mut c, _, b) = sample();
    assert_eq!(
        c.status(PluginFormat::Clap, "b.crash"),
        Some(PluginStatus::Blocklisted("crashed (signal 11)".into()))
    );
    c.set_enabled(PluginFormat::Clap, "a.fx", false);
    let names = |rows: Vec<core_host::catalog::CatalogRow>| -> Vec<String> {
        rows.into_iter().map(|r| r.plugin.id).collect()
    };
    assert_eq!(names(c.rows()), ["a.fx", "a.synth", "b.crash"]);
    assert_eq!(names(c.browser(PluginKind::Effect)), Vec::<String>::new());
    assert_eq!(names(c.browser(PluginKind::Instrument)), ["a.synth"]);
    c.set_enabled(PluginFormat::Clap, "a.fx", true);
    assert_eq!(names(c.browser(PluginKind::Effect)), ["a.fx"]);
    assert_eq!(c.clear_blocklist(PluginFormat::Clap, "b.crash"), Some(b));
    assert!(
        c.get(PluginFormat::Clap, "b.crash").is_none(),
        "rescan pending"
    );
    assert_eq!(c.clear_blocklist(PluginFormat::Clap, "a.fx"), None);
}

#[test]
fn the_host_learns_paths_and_unavailable_plugins() {
    let bundle = test_plugins::bundle();
    let src = Source::clap_bundle(&bundle);
    assert_eq!(src.version, "1.0.0");
    assert!(src.mtime > 0);
    let mut c = Catalog::new();
    c.record(
        &src,
        vec![
            entry(&src, "vizdaw.test.gain", PluginKind::Effect, ScanStatus::Ok),
            entry(&src, "vizdaw.test.nan", PluginKind::Effect, ScanStatus::Ok),
        ],
    );
    c.set_enabled(PluginFormat::Clap, "vizdaw.test.nan", false);
    let mut host = PluginHost::new(48_000.0, 512).unwrap();
    c.apply(&mut host);
    let mut b = core_engine::fixtures::Builder::new();
    let (_, gain) = b.plugin_dest("G", plugin("vizdaw.test.gain")).unwrap();
    let (_, nan) = b.plugin_dest("N", plugin("vizdaw.test.nan")).unwrap();
    host.sync(b.doc());
    host.wait_ready(std::time::Duration::from_secs(10));
    assert!(host.instance(gain).is_some(), "{:?}", host.failure(gain));
    assert_eq!(host.failure(nan), Some("Disabled in Plugin Manager"));
}
VIZDAW_EOF
```

- [ ] **Step 2: Run them to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-host --test catalog
```

Expected: `unresolved import `core_host::catalog``.

- [ ] **Step 3: Implement**


Create `crates/core-host/src/catalog.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p crates/core-host/src
cat > crates/core-host/src/catalog.rs <<'VIZDAW_EOF'
//! The plugin catalog (spec §8, §9): what scans found, the blocklist, the
//! scan cache and the enabled/disabled preference, stored in
//! `plugins.json`. It is plain data: the scanner fills it, the Plugin
//! Manager (3c) reads [`Catalog::rows`] and changes it with
//! [`Catalog::set_enabled`] and [`Catalog::clear_blocklist`], and
//! [`Catalog::apply`] tells a [`crate::PluginHost`] where plugins live and
//! which are unavailable.

use crate::host::PluginHost;
use crate::info::{PluginInfo, PluginKind};
use core_model::mixer::{PluginFormat, PluginRef};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// File format version of `plugins.json`.
pub const CATALOG_VERSION: u32 = 1;

/// Where plugins come from: a CLAP bundle or an AU component, with what
/// identifies the build that was scanned (the cache key, spec §8).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    /// Format.
    pub format: PluginFormat,
    /// Bundle path (CLAP), or `au:<type/subtype/manufacturer>` (AU).
    pub path: PathBuf,
    /// Bundle (or component) version.
    pub version: String,
    /// Modification time of the bundle's newest file, in seconds since the
    /// epoch (0 for AU).
    pub mtime: u64,
}

impl Source {
    /// The source of a CLAP bundle at `path`: its `CFBundleShortVersionString`
    /// (macOS) and the newest modification time of its files.
    pub fn clap_bundle(path: &Path) -> Source {
        Source {
            format: PluginFormat::Clap,
            path: path.to_path_buf(),
            version: bundle_version(path),
            mtime: newest_mtime(path, 4),
        }
    }

    /// True when this is the same build of the same place as `other`.
    pub fn same_place(&self, other: &Source) -> bool {
        self.format == other.format && self.path == other.path
    }
}

/// Reads `CFBundleShortVersionString` from `Contents/Info.plist`, or "".
fn bundle_version(path: &Path) -> String {
    let Ok(text) = std::fs::read_to_string(path.join("Contents").join("Info.plist")) else {
        return String::new();
    };
    let key = "<key>CFBundleShortVersionString</key>";
    text.split_once(key)
        .and_then(|(_, rest)| rest.split_once("<string>"))
        .and_then(|(_, rest)| rest.split_once("</string>"))
        .map(|(v, _)| v.trim().to_string())
        .unwrap_or_default()
}

fn mtime_of(path: &Path) -> u64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs())
}

/// The newest modification time of `path` and anything under it, `depth`
/// levels down.
fn newest_mtime(path: &Path, depth: u32) -> u64 {
    let mut m = mtime_of(path);
    if depth > 0 {
        if let Ok(rd) = std::fs::read_dir(path) {
            for e in rd.flatten() {
                m = m.max(newest_mtime(&e.path(), depth - 1));
            }
        }
    }
    m
}

/// What scanning a plugin gave.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanStatus {
    /// Loaded, activated and processed a block.
    Ok,
    /// Crashed, hung or failed; never loaded again until cleared.
    Blocklisted {
        /// `crashed (signal 11)`, `timed out`, `failed: <message>`.
        reason: String,
    },
    /// Could not be checked outside the app (AU v3); checked on first use.
    NotValidated,
}

/// One plugin (or, with an empty id, a whole source that failed to list).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogEntry {
    /// Where it came from.
    pub source: Source,
    /// Format, id, name, vendor, version (id empty for a failed source).
    pub plugin: PluginRef,
    /// Instrument or effect, when known.
    pub kind: Option<PluginKind>,
    /// Ports and parameters, when it scanned OK.
    pub info: Option<PluginInfo>,
    /// Scan result.
    pub status: ScanStatus,
}

/// A plugin's status as the Plugin Manager shows it (spec §9).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginStatus {
    /// Usable.
    Ok,
    /// Turned off by the user.
    Disabled,
    /// Crashed, hung or failed during a scan.
    Blocklisted(String),
    /// Not checked yet (AU v3).
    NotValidated,
    /// Its bundle or component is gone.
    Missing,
}

/// One row of the Plugin Manager.
#[derive(Debug, Clone, PartialEq)]
pub struct CatalogRow {
    /// The plugin (id empty for a source that failed as a whole).
    pub plugin: PluginRef,
    /// Instrument or effect, when known.
    pub kind: Option<PluginKind>,
    /// Status.
    pub status: PluginStatus,
    /// Where it lives.
    pub source: Source,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Repr {
    version: u32,
    entries: Vec<CatalogEntry>,
    disabled: Vec<(PluginFormat, String)>,
}

/// The catalog.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Catalog {
    entries: Vec<CatalogEntry>,
    disabled: BTreeSet<(PluginFormat, String)>,
    /// Sources found missing by the last [`Catalog::mark_found`].
    missing: BTreeSet<Source>,
}

impl Catalog {
    /// An empty catalog.
    pub fn new() -> Catalog {
        Catalog::default()
    }

    /// `~/Library/Application Support/VizDAW/plugins.json` on macOS;
    /// `$XDG_DATA_HOME/vizdaw/plugins.json` (or `~/.local/share/…`)
    /// elsewhere.
    pub fn default_path() -> PathBuf {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default();
        if cfg!(target_os = "macos") {
            home.join("Library/Application Support/VizDAW/plugins.json")
        } else {
            std::env::var_os("XDG_DATA_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".local/share"))
                .join("vizdaw/plugins.json")
        }
    }

    /// Reads `path`; a missing file is an empty catalog, an unreadable or
    /// corrupt one is an error (the caller decides whether to start over).
    pub fn load(path: &Path) -> Result<Catalog, String> {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Catalog::new()),
            Err(e) => return Err(format!("{}: {e}", path.display())),
        };
        let r: Repr =
            serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        if r.version != CATALOG_VERSION {
            return Err(format!(
                "{}: catalog version {} (expected {CATALOG_VERSION})",
                path.display(),
                r.version
            ));
        }
        Ok(Catalog {
            entries: r.entries,
            disabled: r.disabled.into_iter().collect(),
            missing: BTreeSet::new(),
        })
    }

    /// Writes `path` (creating its folder) through a temporary file, so a
    /// crash never leaves half a catalog.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let r = Repr {
            version: CATALOG_VERSION,
            entries: self.entries.clone(),
            disabled: self.disabled.iter().cloned().collect(),
        };
        let text = serde_json::to_string_pretty(&r).map_err(|e| e.to_string())?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, path).map_err(|e| e.to_string())
    }

    /// Every entry.
    pub fn entries(&self) -> &[CatalogEntry] {
        &self.entries
    }

    /// The entry of plugin `id` of `format`.
    pub fn get(&self, format: PluginFormat, id: &str) -> Option<&CatalogEntry> {
        self.entries
            .iter()
            .find(|e| e.plugin.format == format && e.plugin.id == id)
    }

    /// Of `found` (every source discovery saw now), the ones that must be
    /// scanned: new, or changed since their scan (another version or
    /// modification time). Entries of changed sources are dropped;
    /// entries whose source was not found are marked missing.
    pub fn mark_found(&mut self, found: &[Source]) -> Vec<Source> {
        let mut todo = Vec::new();
        for s in found {
            let known = self.entries.iter().any(|e| &e.source == s);
            if !known {
                self.entries.retain(|e| !e.source.same_place(s));
                todo.push(s.clone());
            }
        }
        self.missing = self
            .entries
            .iter()
            .map(|e| e.source.clone())
            .filter(|src| !found.iter().any(|f| f.same_place(src)))
            .collect();
        todo
    }

    /// Records what scanning `source` gave (replacing earlier entries of
    /// that place).
    pub fn record(&mut self, source: &Source, entries: Vec<CatalogEntry>) {
        self.entries.retain(|e| !e.source.same_place(source));
        self.missing.remove(source);
        self.entries.extend(entries);
    }

    /// Turns a plugin on or off (an app preference, not part of projects).
    pub fn set_enabled(&mut self, format: PluginFormat, id: &str, enabled: bool) {
        let key = (format, id.to_string());
        if enabled {
            self.disabled.remove(&key);
        } else {
            self.disabled.insert(key);
        }
    }

    /// Forgets a blocklist entry; returns its source, to be scanned again.
    pub fn clear_blocklist(&mut self, format: PluginFormat, id: &str) -> Option<Source> {
        let source = self
            .entries
            .iter()
            .find(|e| {
                e.plugin.format == format
                    && e.plugin.id == id
                    && matches!(e.status, ScanStatus::Blocklisted { .. })
            })?
            .source
            .clone();
        self.entries.retain(|e| !e.source.same_place(&source));
        Some(source)
    }

    /// A plugin's status.
    pub fn status(&self, format: PluginFormat, id: &str) -> Option<PluginStatus> {
        let e = self.get(format, id)?;
        Some(self.status_of(e))
    }

    fn status_of(&self, e: &CatalogEntry) -> PluginStatus {
        if self.missing.contains(&e.source) {
            return PluginStatus::Missing;
        }
        match &e.status {
            ScanStatus::Blocklisted { reason } => PluginStatus::Blocklisted(reason.clone()),
            _ if self
                .disabled
                .contains(&(e.plugin.format, e.plugin.id.clone())) =>
            {
                PluginStatus::Disabled
            }
            ScanStatus::NotValidated => PluginStatus::NotValidated,
            ScanStatus::Ok => PluginStatus::Ok,
        }
    }

    /// The Plugin Manager's rows, by name.
    pub fn rows(&self) -> Vec<CatalogRow> {
        let mut rows: Vec<CatalogRow> = self
            .entries
            .iter()
            .map(|e| CatalogRow {
                plugin: e.plugin.clone(),
                kind: e.kind,
                status: self.status_of(e),
                source: e.source.clone(),
            })
            .collect();
        rows.sort_by(|a, b| {
            a.plugin
                .name
                .to_lowercase()
                .cmp(&b.plugin.name.to_lowercase())
                .then_with(|| a.plugin.id.cmp(&b.plugin.id))
        });
        rows
    }

    /// What the plugin browser lists for `kind`: usable plugins (OK or not
    /// validated), hidden when disabled, blocklisted or missing (spec §9).
    pub fn browser(&self, kind: PluginKind) -> Vec<CatalogRow> {
        self.rows()
            .into_iter()
            .filter(|r| r.kind == Some(kind) && !r.plugin.id.is_empty())
            .filter(|r| matches!(r.status, PluginStatus::Ok | PluginStatus::NotValidated))
            .collect()
    }

    /// Tells `host` where every plugin lives and which ones are
    /// unavailable (their slots load as placeholders with the reason).
    pub fn apply(&self, host: &mut PluginHost) {
        for e in &self.entries {
            if e.plugin.id.is_empty() {
                continue;
            }
            if e.plugin.format == PluginFormat::Clap {
                host.register_path(e.plugin.format, &e.plugin.id, e.source.path.clone());
            }
            let why = match self.status_of(e) {
                PluginStatus::Ok | PluginStatus::NotValidated => None,
                PluginStatus::Disabled => Some("Disabled in Plugin Manager".to_string()),
                PluginStatus::Blocklisted(r) => Some(format!("Blocklisted: {r}")),
                PluginStatus::Missing => Some("Missing".to_string()),
            };
            host.set_unavailable(e.plugin.format, &e.plugin.id, why);
        }
    }
}
VIZDAW_EOF
```

Change `crates/core-host/src/lib.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-host/src/lib.rs b/crates/core-host/src/lib.rs
--- a/crates/core-host/src/lib.rs
+++ b/crates/core-host/src/lib.rs
@@ -17,6 +17,7 @@
 )]
 
 pub mod adapt;
+pub mod catalog;
 pub mod clap;
 pub mod error;
 pub mod host;
VIZDAW_EOF
```

- [ ] **Step 4: Run the tests to see them pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-host --test catalog
```

Expected: 4 tests pass (`saves_and_loads_and_reports_corrupt_files`, `only_new_or_changed_sources_are_scanned_and_gone_ones_are_missing`, `statuses_browser_and_clearing_the_blocklist`, `the_host_learns_paths_and_unavailable_plugins`).

- [ ] **Step 5: Check the whole workspace**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Expected: no formatting changes left, no clippy warnings, every test passes.

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-host/src/catalog.rs \
  crates/core-host/tests/catalog.rs \
  crates/core-host/src/lib.rs \
  Cargo.lock
git commit -m "$(cat <<'MSG'
feat(core-host): plugin catalog with scan cache, blocklist and enabled state

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
MSG
)"
```


---

### Task 11: Scanner protocol and the vizdaw-scan binary

**Files:**
- Create: `crates/core-host/src/scan.rs`
- Create: `crates/vizdaw-scan/Cargo.toml`
- Create: `crates/vizdaw-scan/src/main.rs`
- Create: `crates/vizdaw-scan/tests/scan.rs`
- Modify: `crates/core-host/src/lib.rs`
- Lockfile: `Cargo.lock` (cargo updates it when dependencies change)

**Interfaces:**
- Consumes: Task 10's catalog; Task 6's `clap::{ClapFormat, list_bundle, find_bundles, search_paths}`.
- Produces: `core_host::scan::{ScanReply::{Listed, Scanned, NotValidated}, ScanOutcome::{Done, Crashed, TimedOut, Failed}, ScanOutcome::reason, ScanRequest { format, path, id, timeout }, ScanRequest::args, run(&Path, &ScanRequest), scan_source(&Path, &Source, Duration), scan_all(&mut Catalog, &Path, &[Source], impl Fn(&Source) -> Duration), discover_clap(), TIMEOUT, TIMEOUT_AU_V3, PARALLEL}`; binary crate `vizdaw-scan` (`--format clap|au --path P --list | --id X`).

Ruling 18; spec §8. `run` spawns the scanner, drains stdout and stderr on their own threads (a large reply must never look like a hang), kills it on timeout and maps the exit. `scan_source` lists a source, then checks each plugin in its own process. The binary's AU mode is a stub until Task 13.

- [ ] **Step 1: Write the failing tests**


Create `crates/vizdaw-scan/tests/scan.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p crates/vizdaw-scan/tests
cat > crates/vizdaw-scan/tests/scan.rs <<'VIZDAW_EOF'
//! The scanner in its own process: listing, scanning, crashes, hangs and
//! the catalog cache.

use core_host::catalog::{Catalog, PluginStatus, Source};
use core_host::scan::{ScanOutcome, ScanReply, ScanRequest, run, scan_all};
use core_model::mixer::PluginFormat;
use std::path::Path;
use std::time::Duration;

fn exe() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_vizdaw-scan"))
}

fn req(id: Option<&str>, timeout: Duration) -> ScanRequest {
    ScanRequest {
        format: PluginFormat::Clap,
        path: test_plugins::bundle(),
        id: id.map(String::from),
        timeout,
    }
}

#[test]
fn lists_and_scans_a_plugin() {
    let ScanOutcome::Done(ScanReply::Listed(list)) =
        run(exe(), &req(None, Duration::from_secs(10)))
    else {
        panic!("no list");
    };
    assert_eq!(list.len(), 6);
    let ScanOutcome::Done(ScanReply::Scanned(info)) = run(
        exe(),
        &req(Some("vizdaw.test.ducker"), Duration::from_secs(10)),
    ) else {
        panic!("no scan");
    };
    assert_eq!(info.plugin.name, "Test Ducker");
    assert_eq!(info.sidechain_input(), Some(1));
}

#[test]
fn crashes_hangs_and_errors_have_reasons() {
    let crash = run(
        exe(),
        &req(Some("vizdaw.test.crasher"), Duration::from_secs(10)),
    );
    assert_eq!(
        crash.reason().as_deref(),
        Some("crashed (signal 11)"),
        "{crash:?}"
    );
    let hang = run(
        exe(),
        &req(Some("vizdaw.test.hanger"), Duration::from_millis(1500)),
    );
    assert_eq!(hang, ScanOutcome::TimedOut);
    assert_eq!(hang.reason().as_deref(), Some("timed out"));
    let nope = run(
        exe(),
        &req(Some("vizdaw.test.nope"), Duration::from_secs(10)),
    );
    let why = nope.reason().unwrap();
    assert!(
        why.starts_with("failed: ") && why.contains("not found"),
        "{why}"
    );
}

#[test]
fn a_full_scan_fills_the_catalog_and_the_cache_follows_the_mtime() {
    let bundle = test_plugins::bundle();
    // Work on a copy so touching it never disturbs other tests.
    let dir = std::env::temp_dir().join(format!("vizdaw-scan-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let copy = dir.join("Copy.clap");
    copy_dir(&bundle, &copy);
    let src = Source::clap_bundle(&copy);
    let mut cat = Catalog::new();
    let todo = cat.mark_found(std::slice::from_ref(&src));
    assert_eq!(todo, vec![src.clone()]);
    let n = scan_all(&mut cat, exe(), &todo, |_| Duration::from_millis(3000));
    assert_eq!(n, 1);
    let status = |id: &str| cat.status(PluginFormat::Clap, id).unwrap();
    for id in ["gain", "ducker", "synth4", "nan"] {
        assert_eq!(
            status(&format!("vizdaw.test.{id}")),
            PluginStatus::Ok,
            "{id}"
        );
    }
    assert_eq!(
        status("vizdaw.test.crasher"),
        PluginStatus::Blocklisted("crashed (signal 11)".into())
    );
    assert_eq!(
        status("vizdaw.test.hanger"),
        PluginStatus::Blocklisted("timed out".into())
    );
    assert!(
        cat.get(PluginFormat::Clap, "vizdaw.test.gain")
            .unwrap()
            .info
            .is_some()
    );
    // Unchanged: nothing to scan. Modified: scan again.
    assert!(cat.mark_found(&[Source::clap_bundle(&copy)]).is_empty());
    let exe_file = copy.join("Contents/MacOS/VizDAW Test");
    let target = if exe_file.exists() {
        exe_file
    } else {
        copy.clone()
    };
    let f = std::fs::File::options().write(true).open(&target).unwrap();
    f.set_modified(std::time::SystemTime::now() + Duration::from_secs(120))
        .unwrap();
    let again = Source::clap_bundle(&copy);
    assert_ne!(again.mtime, src.mtime);
    assert_eq!(cat.mark_found(std::slice::from_ref(&again)), vec![again]);
    assert!(
        cat.get(PluginFormat::Clap, "vizdaw.test.gain").is_none(),
        "the old scan is dropped"
    );
}

fn copy_dir(from: &Path, to: &Path) {
    if from.is_file() {
        std::fs::copy(from, to).unwrap();
        return;
    }
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        copy_dir(&e.path(), &to.join(e.file_name()));
    }
}
VIZDAW_EOF
```

- [ ] **Step 2: Run them to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p vizdaw-scan
```

Expected: `error: package ID specification `vizdaw-scan` did not match any packages`.

- [ ] **Step 3: Implement**


Create `crates/core-host/src/scan.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p crates/core-host/src
cat > crates/core-host/src/scan.rs <<'VIZDAW_EOF'
//! The scanner protocol (spec §8): one `vizdaw-scan` process per plugin,
//! so a crash or a hang costs only that process; its outcome becomes a
//! catalog entry (a crash, non-zero exit or timeout blocklists the plugin).

use crate::catalog::{Catalog, CatalogEntry, ScanStatus, Source};
use crate::info::{PluginInfo, PluginKind};
use core_model::mixer::{PluginFormat, PluginRef};
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Time a CLAP or AU v2 scan may take (spec §8).
pub const TIMEOUT: Duration = Duration::from_secs(10);
/// Time an AU v3 scan may take.
pub const TIMEOUT_AU_V3: Duration = Duration::from_secs(30);
/// Scans run at once.
pub const PARALLEL: usize = 4;

/// What `vizdaw-scan` prints on success (one JSON line on stdout).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanReply {
    /// `--list`: the plugins of a source.
    Listed(Vec<(PluginRef, PluginKind)>),
    /// `--id`: the plugin loaded, activated at 48 kHz / 512, processed one
    /// silent block and deactivated.
    Scanned(PluginInfo),
    /// It cannot be checked outside the app (AU v3), and why.
    NotValidated(String),
}

/// How one scan ended.
#[derive(Debug, Clone, PartialEq)]
pub enum ScanOutcome {
    /// The scanner answered.
    Done(ScanReply),
    /// The scanner died from a signal (`None`: unknown).
    Crashed(Option<i32>),
    /// It took longer than allowed and was killed.
    TimedOut,
    /// It exited with an error message.
    Failed(String),
}

impl ScanOutcome {
    /// The blocklist reason (spec §8), when this blocklists the plugin.
    pub fn reason(&self) -> Option<String> {
        match self {
            ScanOutcome::Done(_) => None,
            ScanOutcome::Crashed(Some(sig)) => Some(format!("crashed (signal {sig})")),
            ScanOutcome::Crashed(None) => Some("crashed".to_string()),
            ScanOutcome::TimedOut => Some("timed out".to_string()),
            ScanOutcome::Failed(m) => Some(format!("failed: {m}")),
        }
    }
}

/// One scan to run.
#[derive(Debug, Clone, PartialEq)]
pub struct ScanRequest {
    /// Format.
    pub format: PluginFormat,
    /// Source path (CLAP bundle; ignored for AU).
    pub path: PathBuf,
    /// The plugin to scan, or `None` to list the source.
    pub id: Option<String>,
    /// Time allowed.
    pub timeout: Duration,
}

impl ScanRequest {
    /// The command-line arguments of `vizdaw-scan` for this request.
    pub fn args(&self) -> Vec<String> {
        let mut a = vec![
            "--format".to_string(),
            match self.format {
                PluginFormat::Clap => "clap".to_string(),
                PluginFormat::Au => "au".to_string(),
            },
            "--path".to_string(),
            self.path.to_string_lossy().into_owned(),
        ];
        match &self.id {
            Some(id) => {
                a.push("--id".into());
                a.push(id.clone());
            }
            None => a.push("--list".into()),
        }
        a
    }
}

fn signal_of(status: &std::process::ExitStatus) -> Option<i32> {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        status.signal()
    }
    #[cfg(not(unix))]
    {
        let _ = status;
        None
    }
}

/// Runs `exe` (the `vizdaw-scan` binary) for `req` and waits for it, at
/// most `req.timeout`.
pub fn run(exe: &Path, req: &ScanRequest) -> ScanOutcome {
    let child = Command::new(exe)
        .args(req.args())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(e) => return ScanOutcome::Failed(format!("cannot start the scanner: {e}")),
    };
    // Drain both pipes on their own threads: a large reply must never
    // block the child (which would look like a hang).
    let drain = |p: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut s = String::new();
            if let Some(mut p) = p {
                let _ = p.read_to_string(&mut s);
            }
            s
        })
    };
    let out = drain(
        child
            .stdout
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    );
    let err = drain(
        child
            .stderr
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    );
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) if start.elapsed() >= req.timeout => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(e) => return ScanOutcome::Failed(e.to_string()),
        }
    };
    let stdout = out.join().unwrap_or_default();
    let stderr = err.join().unwrap_or_default();
    let Some(status) = status else {
        return ScanOutcome::TimedOut;
    };
    if let Some(sig) = signal_of(&status) {
        return ScanOutcome::Crashed(Some(sig));
    }
    if !status.success() {
        let msg = stderr.trim();
        return ScanOutcome::Failed(if msg.is_empty() {
            format!("exit status {status}")
        } else {
            msg.lines().last().unwrap_or(msg).to_string()
        });
    }
    match stdout
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .map(serde_json::from_str::<ScanReply>)
    {
        Some(Ok(r)) => ScanOutcome::Done(r),
        Some(Err(e)) => ScanOutcome::Failed(format!("unreadable reply: {e}")),
        None => ScanOutcome::Failed("no reply".to_string()),
    }
}

/// Scans every plugin of `source`: lists it, then scans each plugin on its
/// own. A source that cannot be listed gives one blocklisted entry with an
/// empty id.
pub fn scan_source(exe: &Path, source: &Source, timeout: Duration) -> Vec<CatalogEntry> {
    let list = run(
        exe,
        &ScanRequest {
            format: source.format,
            path: source.path.clone(),
            id: None,
            timeout,
        },
    );
    let plugins = match list {
        ScanOutcome::Done(ScanReply::Listed(p)) => p,
        other => {
            let reason = other
                .reason()
                .unwrap_or_else(|| "failed: unexpected reply".to_string());
            return vec![CatalogEntry {
                source: source.clone(),
                plugin: PluginRef {
                    format: source.format,
                    id: String::new(),
                    name: source
                        .path
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    vendor: String::new(),
                    version: source.version.clone(),
                },
                kind: None,
                info: None,
                status: ScanStatus::Blocklisted { reason },
            }];
        }
    };
    plugins
        .into_iter()
        .map(|(plugin, kind)| {
            let outcome = run(
                exe,
                &ScanRequest {
                    format: source.format,
                    path: source.path.clone(),
                    id: Some(plugin.id.clone()),
                    timeout,
                },
            );
            let (info, status) = match outcome {
                ScanOutcome::Done(ScanReply::Scanned(info)) => (Some(info), ScanStatus::Ok),
                ScanOutcome::Done(ScanReply::NotValidated(_)) => (None, ScanStatus::NotValidated),
                ScanOutcome::Done(ScanReply::Listed(_)) => (
                    None,
                    ScanStatus::Blocklisted {
                        reason: "failed: unexpected reply".into(),
                    },
                ),
                other => (
                    None,
                    ScanStatus::Blocklisted {
                        reason: other.reason().unwrap_or_default(),
                    },
                ),
            };
            CatalogEntry {
                source: source.clone(),
                kind: info.as_ref().map(|i| i.kind).or(Some(kind)),
                plugin: info.as_ref().map_or(plugin, |i| i.plugin.clone()),
                info,
                status,
            }
        })
        .collect()
}

/// Scans `sources` with up to [`PARALLEL`] scanner processes at a time and
/// records the results in `catalog`. Returns the sources scanned.
pub fn scan_all(
    catalog: &mut Catalog,
    exe: &Path,
    sources: &[Source],
    timeout: impl Fn(&Source) -> Duration + Sync,
) -> usize {
    let results: Vec<(Source, Vec<CatalogEntry>)> = std::thread::scope(|s| {
        let mut out = Vec::new();
        for chunk in sources.chunks(PARALLEL.max(1)) {
            let handles: Vec<_> = chunk
                .iter()
                .map(|src| {
                    let t = &timeout;
                    s.spawn(move || (src.clone(), scan_source(exe, src, t(src))))
                })
                .collect();
            for h in handles {
                if let Ok(r) = h.join() {
                    out.push(r);
                }
            }
        }
        out
    });
    let n = results.len();
    for (src, entries) in results {
        catalog.record(&src, entries);
    }
    n
}

/// Every CLAP bundle in the standard folders, as sources.
pub fn discover_clap() -> Vec<Source> {
    crate::clap::find_bundles(&crate::clap::search_paths())
        .iter()
        .map(|p| Source::clap_bundle(p))
        .collect()
}
VIZDAW_EOF
```

Create `crates/vizdaw-scan/Cargo.toml`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p crates/vizdaw-scan
cat > crates/vizdaw-scan/Cargo.toml <<'VIZDAW_EOF'
[package]
name = "vizdaw-scan"
version = "0.1.0"
edition.workspace = true
license.workspace = true
rust-version.workspace = true

[dependencies]
core-host = { path = "../core-host" }
core-engine = { path = "../core-engine" }
core-model = { path = "../core-model" }
serde_json = "1"

[dev-dependencies]
test-plugins = { path = "../test-plugins" }
VIZDAW_EOF
```

Create `crates/vizdaw-scan/src/main.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p crates/vizdaw-scan/src
cat > crates/vizdaw-scan/src/main.rs <<'VIZDAW_EOF'
//! `vizdaw-scan`: checks one plugin in its own process (spec §8).
//!
//! ```text
//! vizdaw-scan --format clap --path <bundle> --list
//! vizdaw-scan --format clap --path <bundle> --id <plugin id>
//! ```
//!
//! `--list` prints the plugins of the bundle; `--id` loads the plugin,
//! activates it at 48 kHz / 512 frames, processes one silent block,
//! deactivates it and prints its `PluginInfo`. The reply is one JSON line
//! on stdout ([`core_host::scan::ScanReply`]); failures print a message on
//! stderr and exit with status 2. A crash or a hang is the caller's to
//! detect.

use core_engine::backend::BlockInfo;
use core_host::clap::{ClapFormat, list_bundle};
use core_host::scan::ScanReply;
use core_host::{LoadRequest, ParamOut, PluginFormat, PluginInstance, ProcessCtx};
use core_model::mixer::{PluginFormat as Format, PluginRef};
use std::path::PathBuf;

/// Sample rate and block size of the check.
const RATE: f64 = 48_000.0;
const BLOCK: u32 = 512;

struct Args {
    format: Format,
    path: PathBuf,
    id: Option<String>,
}

fn parse(mut it: impl Iterator<Item = String>) -> Result<Args, String> {
    let (mut format, mut path, mut id, mut list) = (None, None, None, false);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--format" => {
                format = Some(match it.next().as_deref() {
                    Some("clap") => Format::Clap,
                    Some("au") => Format::Au,
                    other => return Err(format!("unknown format {other:?}")),
                })
            }
            "--path" => path = it.next().map(PathBuf::from),
            "--id" => id = it.next(),
            "--list" => list = true,
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    let format = format.ok_or("--format is required")?;
    if list == id.is_some() {
        return Err("give exactly one of --list and --id".into());
    }
    Ok(Args {
        format,
        path: path.unwrap_or_default(),
        id,
    })
}

/// Activates `inst`, processes one silent block and deactivates it.
fn exercise(inst: &mut dyn PluginInstance) -> Result<(), String> {
    let mut proc = inst.activate(RATE, BLOCK).map_err(|e| e.to_string())?;
    let info = inst.info().clone();
    let bufs = |n: usize| -> Vec<Box<[f32]>> {
        (0..n)
            .map(|_| vec![0.0f32; BLOCK as usize].into_boxed_slice())
            .collect()
    };
    let main = info
        .main_input()
        .and_then(|i| info.inputs.get(i))
        .map_or(0, |p| usize::from(p.channels));
    let sc = info
        .sidechain_input()
        .and_then(|i| info.inputs.get(i))
        .map_or(0, |p| usize::from(p.channels));
    let (mut input, mut sidechain, mut outputs) =
        (bufs(main), bufs(sc), bufs(info.output_channels()));
    let mut out = ParamOut::default();
    let transport = BlockInfo::default();
    let mut ctx = ProcessCtx {
        frames: BLOCK,
        input: &mut input,
        sidechain: &mut sidechain,
        outputs: &mut outputs,
        events: &[],
        params_in: &[],
        params_out: &mut out,
        transport: &transport,
    };
    let status = proc.process(&mut ctx);
    inst.deactivate(proc);
    match status {
        core_engine::graph::ProcessStatus::Ok => Ok(()),
        core_engine::graph::ProcessStatus::Error => Err("processing failed".into()),
    }
}

fn scan(args: &Args) -> Result<ScanReply, String> {
    match (args.format, &args.id) {
        (Format::Clap, None) => list_bundle(&args.path)
            .map(ScanReply::Listed)
            .map_err(|e| e.to_string()),
        (Format::Clap, Some(id)) => {
            let mut f = ClapFormat::new();
            let req = LoadRequest {
                plugin: PluginRef {
                    format: Format::Clap,
                    id: id.clone(),
                    name: id.clone(),
                    vendor: String::new(),
                    version: String::new(),
                },
                path: Some(args.path.clone()),
                sample_rate: RATE,
                max_frames: BLOCK,
            };
            let prepared = (f.prepare(&req))().map_err(|e| e.to_string())?;
            let mut inst = f.finish(&req, prepared).map_err(|e| e.to_string())?;
            exercise(inst.as_mut())?;
            Ok(ScanReply::Scanned(inst.info().clone()))
        }
        (Format::Au, _) => au::scan(args),
    }
}

#[cfg(target_os = "macos")]
mod au {
    use super::*;
    pub fn scan(args: &Args) -> Result<ScanReply, String> {
        let _ = args;
        Err("AU scanning arrives with the AU host".into())
    }
}

#[cfg(not(target_os = "macos"))]
mod au {
    use super::*;
    pub fn scan(_: &Args) -> Result<ScanReply, String> {
        Err("Audio Units exist only on macOS".into())
    }
}

fn main() {
    let result = parse(std::env::args().skip(1)).and_then(|a| scan(&a));
    match result.and_then(|r| serde_json::to_string(&r).map_err(|e| e.to_string())) {
        Ok(line) => println!("{line}"),
        Err(e) => {
            eprintln!("vizdaw-scan: {e}");
            std::process::exit(2);
        }
    }
}
VIZDAW_EOF
```

Change `crates/core-host/src/lib.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-host/src/lib.rs b/crates/core-host/src/lib.rs
--- a/crates/core-host/src/lib.rs
+++ b/crates/core-host/src/lib.rs
@@ -25,6 +25,7 @@ pub mod info;
 pub mod instance;
 pub mod process;
 pub mod queue;
+pub mod scan;
 pub mod shelf;
 
 pub use error::HostError;
VIZDAW_EOF
```

- [ ] **Step 4: Run the tests to see them pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p vizdaw-scan
```

Expected: 3 tests pass (`lists_and_scans_a_plugin`, `crashes_hangs_and_errors_have_reasons` — the crasher gives `crashed (signal 11)`, the hanger `timed out` after 1.5 s —, `a_full_scan_fills_the_catalog_and_the_cache_follows_the_mtime`).

- [ ] **Step 5: Check the whole workspace**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Expected: no formatting changes left, no clippy warnings, every test passes.

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-host/src/scan.rs \
  crates/vizdaw-scan/Cargo.toml \
  crates/vizdaw-scan/src/main.rs \
  crates/vizdaw-scan/tests/scan.rs \
  crates/core-host/src/lib.rs \
  Cargo.lock
git commit -m "$(cat <<'MSG'
feat: vizdaw-scan checks one plugin per process; scan runner and blocklist

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
MSG
)"
```


---

### Task 12: AU host

**Files:**
- Create: `crates/platform-macos/src/au/ffi.rs`
- Create: `crates/platform-macos/src/au/mod.rs`
- Create: `crates/platform-macos/tests/au.rs`
- Modify: `crates/core-host/src/clap/processor.rs`
- Modify: `crates/core-host/src/host.rs`
- Modify: `crates/core-host/src/instance.rs`
- Modify: `crates/core-host/src/process.rs`
- Modify: `crates/platform-macos/Cargo.toml`
- Modify: `crates/platform-macos/src/lib.rs`
- Lockfile: `Cargo.lock` (cargo updates it when dependencies change)

**Interfaces:**
- Consumes: Task 4's traits (with two additions below), Task 8's host (gestures merge); objc2-audio-toolbox (`AudioUnitUtilities`, `objc2-core-foundation`, `block2` features), objc2-core-foundation (`CFData`, `CFError`, `CFPropertyList`).
- Produces: `platform_macos::au::{AuFormat, AuInstance, AuProcessor, AuComponent { plugin, kind, v3 }, components(), component(&str), sources() -> Vec<Source>, au_id(u32, u32, u32), parse_au_id(&str), load(&str, f64, u32) -> Result<AuInstance, HostError>, scan_timeout(bool)}`; `AuInstance::simulate_ui_gesture(ParamId, f64)` (tests); `PluginInstance::gestures(&mut self) -> Vec<ParamEvent>` (default empty; `PluginHost::idle` merges it); `ProcessCtx::{input, sidechain}` become `&'a mut [Box<[f32]>]`.

Ruling 6 (and 2, 7, 8). `ffi.rs` holds every `unsafe` call: component enumeration and lookup, v2 instantiation and v3 asynchronous instantiation (a block completion and a channel), stream formats (non-interleaved f32, 1–2 channels, the instance's sample rate), `MaximumFramesPerSlice`, input callbacks that lend the host's channel buffers when the unit passes null ones (or copy into the unit's), initialize/uninitialize, latency and tail, parameter list/info/get/set, `MusicDeviceMIDIEvent`, state as a binary property list, the latency property listener, the `AUEventListener` for gestures on a serial dispatch queue, and a helper that acts like a plugin UI (begin gesture, `AUParameterSet`, end gesture). `mod.rs` is safe: `AuFormat::prepare` does the instantiation and initialization on the loader thread; `AuInstance` re-configures on activation (callbacks are set while uninitialized), reads ports (effects: bus 0 main, bus 1 sidechain when present; instruments: no inputs), parameters, latency and tail; `AuProcessor` sets parameter changes and MIDI at their frames, points the input feed at the context's buffers, and renders every output bus into its slice of the output channels. `ProcessCtx`'s inputs become mutable so the AU can borrow them (the CLAP processor reads them through a reborrow).

- [ ] **Step 1: Write the failing tests**


Create `crates/platform-macos/tests/au.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p crates/platform-macos/tests
cat > crates/platform-macos/tests/au.rs <<'VIZDAW_EOF'
//! The AU host against Apple's built-in units (never played on a device):
//! enumeration, ports, parameters, processing, sidechain, instruments,
//! state and UI gestures.

use core_engine::backend::BlockInfo;
use core_engine::graph::ProcessStatus;
use core_host::{
    AudioProcessor, MidiEvent, ParamChange, ParamEvent, ParamOut, PluginInstance, PluginKind,
    PortRole, ProcessCtx,
};
use core_model::mixer::ParamId;
use platform_macos::au::{self, AuInstance};
use std::time::{Duration, Instant};

const DELAY: &str = "aufx/dely/appl";
const DYNAMICS: &str = "aufx/dcmp/appl";
const REVERB: &str = "aufx/mrev/appl";
const DLS: &str = "aumu/dls /appl";
const N: usize = 512;

fn load(id: &str) -> AuInstance {
    au::load(id, 48_000.0, N as u32).unwrap()
}

fn bufs(n: usize, v: f32) -> Vec<Box<[f32]>> {
    (0..n).map(|_| vec![v; N].into_boxed_slice()).collect()
}

struct Block {
    input: Vec<Box<[f32]>>,
    sidechain: Vec<Box<[f32]>>,
    outputs: Vec<Box<[f32]>>,
    events: Vec<MidiEvent>,
    params: Vec<ParamChange>,
}

impl Block {
    fn new(inputs: usize, outputs: usize, level: f32) -> Block {
        Block {
            input: bufs(inputs, level),
            sidechain: Vec::new(),
            outputs: bufs(outputs, 9.0),
            events: Vec::new(),
            params: Vec::new(),
        }
    }

    fn run(&mut self, p: &mut dyn AudioProcessor) -> ProcessStatus {
        let info = BlockInfo::default();
        let mut out = ParamOut::default();
        let mut ctx = ProcessCtx {
            frames: N as u32,
            input: &mut self.input,
            sidechain: &mut self.sidechain,
            outputs: &mut self.outputs,
            events: &self.events,
            params_in: &self.params,
            params_out: &mut out,
            transport: &info,
        };
        p.process(&mut ctx)
    }
}

#[test]
fn enumeration_finds_apples_units() {
    let all = au::components();
    let delay = all.iter().find(|c| c.plugin.id == DELAY).unwrap();
    assert_eq!(delay.kind, PluginKind::Effect);
    assert_eq!(delay.plugin.vendor, "Apple");
    assert_eq!(delay.plugin.name, "AUDelay");
    let dls = all.iter().find(|c| c.plugin.id == DLS).unwrap();
    assert_eq!(dls.kind, PluginKind::Instrument);
    assert_eq!(
        au::component(DYNAMICS).unwrap().plugin.name,
        "AUDynamicsProcessor"
    );
    assert!(au::component("aufx/none/appl").is_none());
    assert_eq!(
        au::parse_au_id(DELAY),
        Some((0x61756678, 0x64656c79, 0x6170706c))
    );
    assert!(
        au::sources()
            .iter()
            .any(|s| s.path.to_string_lossy() == format!("au:{DELAY}"))
    );
}

#[test]
fn the_delay_reports_parameters_and_processes() {
    let mut inst = load(DELAY);
    let info = inst.info().clone();
    assert_eq!(info.inputs[0].role, PortRole::Main);
    assert_eq!(info.outputs[0].channels, 2);
    let names: Vec<&str> = info.params.iter().map(|p| p.name.as_str()).collect();
    assert!(names.contains(&"Dry/Wet Mix"), "{names:?}");
    // All dry: the input passes unchanged.
    inst.set_params(&[(ParamId(0), 0.0)]).unwrap();
    let mut p = inst.activate(48_000.0, N as u32).unwrap();
    let mut b = Block::new(2, 2, 0.5);
    assert_eq!(b.run(p.as_mut()), ProcessStatus::Ok);
    assert!(
        b.outputs[0].iter().all(|x| (*x - 0.5).abs() < 1e-4),
        "{:?}",
        &b.outputs[0][..4]
    );
    // All wet with a 5 ms delay: silence, then the input 240 frames later.
    b.params = vec![
        ParamChange {
            frame: 0,
            id: ParamId(0),
            value: 100.0,
        },
        ParamChange {
            frame: 0,
            id: ParamId(1),
            value: 0.005,
        },
    ];
    p.reset();
    b.run(p.as_mut());
    b.params.clear();
    b.run(p.as_mut());
    assert!(b.outputs[1].iter().any(|x| x.abs() > 0.1));
    inst.deactivate(p);
}

#[test]
fn the_dynamics_processor_processes_and_apples_units_offer_no_sidechain() {
    // None of Apple's built-in effects takes a second input bus on current
    // macOS (Plan Ruling 8), so the AU sidechain path is checked by hand
    // with a third-party unit; here the main path and the port report are.
    let mut inst = load(DYNAMICS);
    assert_eq!(inst.info().sidechain_input(), None);
    let mut p = inst.activate(48_000.0, N as u32).unwrap();
    let mut b = Block::new(2, 2, 0.25);
    for _ in 0..20 {
        assert_eq!(b.run(p.as_mut()), ProcessStatus::Ok);
    }
    assert!(b.outputs[0].iter().all(|x| x.is_finite() && x.abs() > 0.0));
    inst.deactivate(p);
}

#[test]
fn the_matrix_reverb_rings() {
    let mut inst = load(REVERB);
    let mut p = inst.activate(48_000.0, N as u32).unwrap();
    let mut b = Block::new(2, 2, 0.3);
    for _ in 0..10 {
        assert_eq!(b.run(p.as_mut()), ProcessStatus::Ok);
    }
    b.input = bufs(2, 0.0);
    let mut tail = false;
    for _ in 0..10 {
        b.run(p.as_mut());
        tail |= b.outputs[0].iter().any(|x| x.abs() > 1e-6);
    }
    assert!(tail, "it rings after the input stops");
    inst.deactivate(p);
}

#[test]
fn the_dls_synth_plays_a_note() {
    let mut inst = load(DLS);
    let info = inst.info().clone();
    assert_eq!(info.kind, PluginKind::Instrument);
    assert!(info.note_input && info.inputs.is_empty());
    let mut p = inst.activate(48_000.0, N as u32).unwrap();
    let mut b = Block::new(0, info.output_channels(), 0.0);
    b.events.push(MidiEvent::new(0, &[0x90, 60, 110]));
    let mut loud = false;
    for _ in 0..20 {
        b.run(p.as_mut());
        b.events.clear();
        loud |= b.outputs[0].iter().any(|x| x.abs() > 1e-4);
    }
    assert!(loud);
    inst.deactivate(p);
}

#[test]
fn state_round_trips() {
    let mut a = load(DELAY);
    a.set_params(&[(ParamId(1), 0.123)]).unwrap();
    let state = a.save_state().unwrap();
    assert!(state.starts_with(b"bplist00"), "a binary property list");
    let mut b = load(DELAY);
    assert!(
        (b.param_value(ParamId(1)).unwrap() - 1.0).abs() < 1e-6,
        "the default"
    );
    b.load_state(&state).unwrap();
    assert!((b.param_value(ParamId(1)).unwrap() - 0.123).abs() < 1e-6);
    assert!(b.load_state(b"junk").is_err());
}

#[test]
fn ui_gestures_are_captured() {
    let mut inst = load(DELAY);
    inst.simulate_ui_gesture(ParamId(0), 25.0);
    let start = Instant::now();
    let mut got = Vec::new();
    while got.len() < 3 && start.elapsed() < Duration::from_secs(2) {
        got.extend(inst.gestures());
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        got,
        vec![
            ParamEvent::Begin(ParamId(0)),
            ParamEvent::Value(ParamId(0), 25.0),
            ParamEvent::End(ParamId(0)),
        ]
    );
    assert_eq!(inst.param_value(ParamId(0)), Some(25.0));
}
VIZDAW_EOF
```

- [ ] **Step 2: Run them to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p platform-macos --test au
```

Expected: `unresolved import `platform_macos::au``.

- [ ] **Step 3: Implement**


Create `crates/platform-macos/src/au/ffi.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p crates/platform-macos/src/au
cat > crates/platform-macos/src/au/ffi.rs <<'VIZDAW_EOF'
//! Every `unsafe` call of the AU host: the AudioUnit C API (which also
//! drives AU v3 components through the system bridge, Plan Ruling 6),
//! CoreFoundation property lists for state, and the AU event listener for
//! UI gestures.

use block2::RcBlock;
use objc2_audio_toolbox::{
    AUListenerDispose, AUParameterListenerRef, AUParameterSet, AURenderCallbackStruct,
    AudioComponent, AudioComponentCopyName, AudioComponentDescription, AudioComponentFindNext,
    AudioComponentFlags, AudioComponentGetDescription, AudioComponentGetVersion,
    AudioComponentInstanceDispose, AudioComponentInstanceNew, AudioComponentInstantiate,
    AudioComponentInstantiationOptions, AudioUnit, AudioUnitAddPropertyListener,
    AudioUnitEventType, AudioUnitGetParameter, AudioUnitGetProperty, AudioUnitGetPropertyInfo,
    AudioUnitInitialize, AudioUnitParameter, AudioUnitParameterInfo, AudioUnitParameterOptions,
    AudioUnitPropertyID, AudioUnitRender, AudioUnitRenderActionFlags, AudioUnitReset,
    AudioUnitScope, AudioUnitSetParameter, AudioUnitSetProperty, AudioUnitUninitialize,
    MusicDeviceMIDIEvent, kAudioUnitProperty_ClassInfo, kAudioUnitProperty_ElementCount,
    kAudioUnitProperty_Latency, kAudioUnitProperty_MaximumFramesPerSlice,
    kAudioUnitProperty_ParameterInfo, kAudioUnitProperty_ParameterList,
    kAudioUnitProperty_SetRenderCallback, kAudioUnitProperty_StreamFormat,
    kAudioUnitProperty_TailTime, kAudioUnitScope_Global,
};
use objc2_core_audio_types::{
    AudioBuffer, AudioBufferList, AudioStreamBasicDescription, AudioTimeStamp, AudioTimeStampFlags,
    SMPTETime, SMPTETimeFlags, SMPTETimeType, kAudioFormatFlagIsFloat,
    kAudioFormatFlagIsNonInterleaved, kAudioFormatFlagIsPacked, kAudioFormatLinearPCM,
};
use objc2_core_foundation::{
    CFData, CFPropertyList, CFPropertyListCreateData, CFPropertyListCreateWithData,
    CFPropertyListFormat, CFRetained, CFString,
};
use std::ffi::c_void;
use std::ptr::{self, NonNull};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Most channels of one bus VizDAW renders (the graph is stereo).
pub(crate) const MAX_BUS_CHANNELS: usize = 2;

fn os(call: &str, status: i32) -> Result<(), String> {
    if status == 0 {
        Ok(())
    } else {
        Err(format!("{call} failed with status {status}"))
    }
}

/// A component (a plugin "class"), as found by enumeration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Component(AudioComponent);

// SAFETY: an `AudioComponent` is an immutable, process-wide registry handle;
// every function taking one is thread-safe.
unsafe impl Send for Component {}

/// What enumeration tells about a component.
#[derive(Debug, Clone)]
pub(crate) struct ComponentInfo {
    pub component: Component,
    pub kind: u32,
    pub subtype: u32,
    pub manufacturer: u32,
    pub name: String,
    pub version: u32,
    pub v3: bool,
}

fn cfstring(s: *const CFString) -> String {
    match NonNull::new(s.cast_mut()) {
        // SAFETY: the caller passes a CFString it owns (+1), released here.
        Some(p) => unsafe { CFRetained::from_raw(p) }.to_string(),
        None => String::new(),
    }
}

/// Every component whose type is one of `types`.
pub(crate) fn components(types: &[u32]) -> Vec<ComponentInfo> {
    let mut out = Vec::new();
    for &kind in types {
        let mut desc = AudioComponentDescription {
            componentType: kind,
            componentSubType: 0,
            componentManufacturer: 0,
            componentFlags: 0,
            componentFlagsMask: 0,
        };
        let mut comp: AudioComponent = ptr::null_mut();
        loop {
            // SAFETY: `desc` is a valid description; `comp` is null or a
            // component this loop got from the previous call.
            comp = unsafe { AudioComponentFindNext(comp, NonNull::from(&mut desc)) };
            if comp.is_null() {
                break;
            }
            let mut d = desc;
            let mut name: *const CFString = ptr::null();
            let mut version = 0u32;
            // SAFETY: `comp` is valid; the out-pointers live for the calls.
            let ok = unsafe {
                AudioComponentGetDescription(comp, NonNull::from(&mut d)) == 0
                    && AudioComponentCopyName(comp, NonNull::from(&mut name)) == 0
            };
            // SAFETY: as above.
            unsafe { AudioComponentGetVersion(comp, NonNull::from(&mut version)) };
            if !ok {
                continue;
            }
            out.push(ComponentInfo {
                component: Component(comp),
                kind: d.componentType,
                subtype: d.componentSubType,
                manufacturer: d.componentManufacturer,
                name: cfstring(name),
                version,
                v3: AudioComponentFlags(d.componentFlags)
                    .contains(AudioComponentFlags::IsV3AudioUnit),
            });
        }
    }
    out
}

/// The component with exactly these codes.
pub(crate) fn find(kind: u32, subtype: u32, manufacturer: u32) -> Option<ComponentInfo> {
    components(&[kind])
        .into_iter()
        .find(|c| c.subtype == subtype && c.manufacturer == manufacturer)
}

/// An open AudioUnit instance; disposed on drop.
pub(crate) struct Unit {
    unit: AudioUnit,
    /// Latency-change flag the property listener sets (its refcon).
    latency_flag: Option<Arc<AtomicBool>>,
}

// SAFETY: an AudioUnit may be used from several threads: the render thread
// renders while the main thread reads properties and parameters (the
// AudioUnit contract); `Unit` adds no unsynchronised state of its own.
unsafe impl Send for Unit {}
// SAFETY: as above.
unsafe impl Sync for Unit {}

impl Drop for Unit {
    fn drop(&mut self) {
        // SAFETY: the instance is owned here and nothing renders it any more
        // (the processor holding the other `Arc` is gone).
        unsafe {
            AudioUnitUninitialize(self.unit);
            AudioComponentInstanceDispose(self.unit);
        }
    }
}

/// Instantiates a v2 component synchronously (any thread, Plan Ruling 2).
pub(crate) fn instantiate_v2(c: Component) -> Result<Unit, String> {
    let mut unit: AudioUnit = ptr::null_mut();
    // SAFETY: `c` is a valid component and `unit` a valid out-pointer.
    os("AudioComponentInstanceNew", unsafe {
        AudioComponentInstanceNew(c.0, NonNull::from(&mut unit))
    })?;
    Ok(Unit {
        unit,
        latency_flag: None,
    })
}

/// Instantiates a component asynchronously (AU v3; out of process when
/// `out_of_process`) and waits up to `timeout` for it.
pub(crate) fn instantiate_async(
    c: Component,
    out_of_process: bool,
    timeout: Duration,
) -> Result<Unit, String> {
    let (tx, rx) = channel::<(usize, i32)>();
    let tx = Mutex::new(Some(tx));
    let done = RcBlock::new(move |unit: AudioUnit, status: i32| {
        if let Some(tx) = tx.lock().ok().and_then(|mut g| g.take()) {
            let _ = tx.send((unit as usize, status));
        }
    });
    let opts = if out_of_process {
        AudioComponentInstantiationOptions::LoadOutOfProcess
    } else {
        AudioComponentInstantiationOptions::LoadInProcess
    };
    // SAFETY: `c` is valid; the block is retained by the system until it is
    // called.
    unsafe { AudioComponentInstantiate(c.0, opts, &done) };
    let (unit, status) = rx
        .recv_timeout(timeout)
        .map_err(|_| "instantiation timed out".to_string())?;
    os("AudioComponentInstantiate", status)?;
    if unit == 0 {
        return Err("AudioComponentInstantiate gave no instance".into());
    }
    Ok(Unit {
        unit: unit as AudioUnit,
        latency_flag: None,
    })
}

fn stream(sample_rate: f64, channels: u32) -> AudioStreamBasicDescription {
    AudioStreamBasicDescription {
        mSampleRate: sample_rate,
        mFormatID: kAudioFormatLinearPCM,
        mFormatFlags: kAudioFormatFlagIsFloat
            | kAudioFormatFlagIsPacked
            | kAudioFormatFlagIsNonInterleaved,
        mBytesPerPacket: 4,
        mFramesPerPacket: 1,
        mBytesPerFrame: 4,
        mChannelsPerFrame: channels,
        mBitsPerChannel: 32,
        mReserved: 0,
    }
}

impl Unit {
    fn get<T: Copy>(
        &self,
        id: AudioUnitPropertyID,
        scope: AudioUnitScope,
        element: u32,
        zero: T,
    ) -> Option<T> {
        let mut v = zero;
        let mut size = size_of::<T>() as u32;
        // SAFETY: `v` has room for `size` bytes of the property's type, which
        // the caller names with `T`.
        let st = unsafe {
            AudioUnitGetProperty(
                self.unit,
                id,
                scope,
                element,
                NonNull::from(&mut v).cast::<c_void>(),
                NonNull::from(&mut size),
            )
        };
        (st == 0).then_some(v)
    }

    fn set<T>(
        &self,
        call: &str,
        id: AudioUnitPropertyID,
        scope: AudioUnitScope,
        element: u32,
        v: &T,
    ) -> Result<(), String> {
        // SAFETY: `v` outlives the call and its size matches the property's
        // type (named by the caller).
        os(call, unsafe {
            AudioUnitSetProperty(
                self.unit,
                id,
                scope,
                element,
                (v as *const T).cast::<c_void>(),
                size_of::<T>() as u32,
            )
        })
    }

    /// Buses in `scope` (input or output).
    pub(crate) fn element_count(&self, scope: AudioUnitScope) -> u32 {
        self.get(kAudioUnitProperty_ElementCount, scope, 0, 0u32)
            .unwrap_or(0)
    }

    /// Asks for `count` buses in `scope`; false when the unit refuses.
    pub(crate) fn set_element_count(&self, scope: AudioUnitScope, count: u32) -> bool {
        self.set(
            "AudioUnitSetProperty(ElementCount)",
            kAudioUnitProperty_ElementCount,
            scope,
            0,
            &count,
        )
        .is_ok()
    }

    /// Channels of bus `element` in `scope`, as the unit has it now.
    pub(crate) fn channels(&self, scope: AudioUnitScope, element: u32) -> u32 {
        self.get(
            kAudioUnitProperty_StreamFormat,
            scope,
            element,
            stream(0.0, 0),
        )
        .map_or(0, |f| f.mChannelsPerFrame)
    }

    /// Sets bus `element` of `scope` to non-interleaved f32.
    pub(crate) fn set_format(
        &self,
        scope: AudioUnitScope,
        element: u32,
        sample_rate: f64,
        channels: u32,
    ) -> Result<(), String> {
        self.set(
            "AudioUnitSetProperty(StreamFormat)",
            kAudioUnitProperty_StreamFormat,
            scope,
            element,
            &stream(sample_rate, channels),
        )
    }

    /// Largest block it will be asked for.
    pub(crate) fn set_max_frames(&self, frames: u32) -> Result<(), String> {
        self.set(
            "AudioUnitSetProperty(MaximumFramesPerSlice)",
            kAudioUnitProperty_MaximumFramesPerSlice,
            kAudioUnitScope_Global,
            0,
            &frames,
        )
    }

    /// Feeds input bus `element` from `callback` with `refcon`.
    pub(crate) fn set_input(&self, element: u32, refcon: *mut c_void) -> Result<(), String> {
        let cb = AURenderCallbackStruct {
            inputProc: Some(render_input),
            inputProcRefCon: refcon,
        };
        self.set(
            "AudioUnitSetProperty(SetRenderCallback)",
            kAudioUnitProperty_SetRenderCallback,
            objc2_audio_toolbox::kAudioUnitScope_Input,
            element,
            &cb,
        )
    }

    /// Allocates for rendering (activation).
    pub(crate) fn initialize(&self) -> Result<(), String> {
        // SAFETY: the unit is open and configured.
        os("AudioUnitInitialize", unsafe {
            AudioUnitInitialize(self.unit)
        })
    }

    /// Frees what `initialize` allocated (deactivation).
    pub(crate) fn uninitialize(&self) {
        // SAFETY: the unit is open; uninitializing twice is harmless.
        unsafe { AudioUnitUninitialize(self.unit) };
    }

    /// Clears its DSP state. // RT
    pub(crate) fn reset(&self) {
        // SAFETY: the unit is open. // RT
        unsafe { AudioUnitReset(self.unit, kAudioUnitScope_Global, 0) };
    }

    /// Latency in seconds.
    pub(crate) fn latency(&self) -> f64 {
        self.get(
            kAudioUnitProperty_Latency,
            kAudioUnitScope_Global,
            0,
            0.0f64,
        )
        .unwrap_or(0.0)
    }

    /// Tail in seconds.
    pub(crate) fn tail(&self) -> f64 {
        self.get(
            kAudioUnitProperty_TailTime,
            kAudioUnitScope_Global,
            0,
            0.0f64,
        )
        .unwrap_or(0.0)
    }

    /// Sets `flag` whenever the unit reports a latency change.
    pub(crate) fn watch_latency(&mut self, flag: Arc<AtomicBool>) -> Result<(), String> {
        let refcon = Arc::as_ptr(&flag).cast_mut().cast::<c_void>();
        // SAFETY: `refcon` points at the flag, which `self` keeps alive
        // (`latency_flag`) until the unit is disposed, which removes the
        // listener.
        os("AudioUnitAddPropertyListener", unsafe {
            AudioUnitAddPropertyListener(
                self.unit,
                kAudioUnitProperty_Latency,
                Some(latency_changed),
                refcon,
            )
        })?;
        self.latency_flag = Some(flag);
        Ok(())
    }

    /// Global parameter addresses.
    pub(crate) fn parameter_ids(&self) -> Vec<u32> {
        let mut size = 0u32;
        // SAFETY: `size` is a valid out-pointer; no data pointer is passed.
        let st = unsafe {
            AudioUnitGetPropertyInfo(
                self.unit,
                kAudioUnitProperty_ParameterList,
                kAudioUnitScope_Global,
                0,
                &mut size,
                ptr::null_mut(),
            )
        };
        if st != 0 || size == 0 {
            return Vec::new();
        }
        let mut ids = vec![0u32; size as usize / size_of::<u32>()];
        let mut size = (ids.len() * size_of::<u32>()) as u32;
        // SAFETY: `ids` has room for `size` bytes.
        let st = unsafe {
            AudioUnitGetProperty(
                self.unit,
                kAudioUnitProperty_ParameterList,
                kAudioUnitScope_Global,
                0,
                NonNull::new_unchecked(ids.as_mut_ptr().cast::<c_void>()),
                NonNull::from(&mut size),
            )
        };
        if st != 0 {
            return Vec::new();
        }
        ids.truncate(size as usize / size_of::<u32>());
        ids
    }

    /// `(name, min, max, default, flags, unit)` of parameter `id`.
    pub(crate) fn parameter_info(
        &self,
        id: u32,
    ) -> Option<(String, f32, f32, f32, AudioUnitParameterOptions, u32)> {
        // SAFETY: the info struct is plain data; all-zero is a valid value.
        let zero: AudioUnitParameterInfo = unsafe { std::mem::zeroed() };
        let info = self.get(
            kAudioUnitProperty_ParameterInfo,
            kAudioUnitScope_Global,
            id,
            zero,
        )?;
        let flags = info.flags;
        let name = if flags.contains(AudioUnitParameterOptions::Flag_HasCFNameString)
            && !info.cfNameString.is_null()
        {
            let s = info.cfNameString;
            if flags.contains(AudioUnitParameterOptions::Flag_CFNameRelease) {
                cfstring(s)
            } else {
                // SAFETY: a non-owned CFString valid while the unit lives.
                unsafe { &*s }.to_string()
            }
        } else {
            let bytes: Vec<u8> = info
                .name
                .iter()
                .take_while(|c| **c != 0)
                .map(|c| *c as u8)
                .collect();
            String::from_utf8_lossy(&bytes).into_owned()
        };
        Some((
            name,
            info.minValue,
            info.maxValue,
            info.defaultValue,
            flags,
            info.unit.0,
        ))
    }

    /// Parameter `id`'s value.
    pub(crate) fn get_param(&self, id: u32) -> Option<f32> {
        let mut v = 0.0f32;
        // SAFETY: `v` is a valid out-pointer.
        let st = unsafe {
            AudioUnitGetParameter(
                self.unit,
                id,
                kAudioUnitScope_Global,
                0,
                NonNull::from(&mut v),
            )
        };
        (st == 0).then_some(v)
    }

    /// Sets parameter `id` at `offset` frames into the next render. // RT
    pub(crate) fn set_param(&self, id: u32, value: f32, offset: u32) {
        // SAFETY: plain values; the unit is open. // RT
        unsafe { AudioUnitSetParameter(self.unit, id, kAudioUnitScope_Global, 0, value, offset) };
    }

    /// Queues a MIDI 1.0 message at `offset` frames into the next render.
    /// // RT
    pub(crate) fn midi(&self, status: u8, d1: u8, d2: u8, offset: u32) {
        // SAFETY: plain values; the unit is open. // RT
        unsafe {
            MusicDeviceMIDIEvent(
                self.unit,
                u32::from(status),
                u32::from(d1),
                u32::from(d2),
                offset,
            )
        };
    }

    /// The unit's state (`ClassInfo`) as a binary property list.
    pub(crate) fn save_state(&self) -> Result<Vec<u8>, String> {
        let mut plist: *const c_void = ptr::null();
        let mut size = size_of::<*const c_void>() as u32;
        // SAFETY: `plist` receives a +1 CFPropertyListRef.
        os("AudioUnitGetProperty(ClassInfo)", unsafe {
            AudioUnitGetProperty(
                self.unit,
                kAudioUnitProperty_ClassInfo,
                kAudioUnitScope_Global,
                0,
                NonNull::from(&mut plist).cast::<c_void>(),
                NonNull::from(&mut size),
            )
        })?;
        let plist = NonNull::new(plist.cast_mut().cast::<CFPropertyList>())
            .ok_or("the unit returned no state")?;
        // SAFETY: we own the +1 reference returned above.
        let plist = unsafe { CFRetained::from_raw(plist) };
        // SAFETY: `plist` is a valid property list; null error pointer.
        let data = unsafe {
            CFPropertyListCreateData(
                None,
                Some(&plist),
                CFPropertyListFormat::BinaryFormat_v1_0,
                0,
                ptr::null_mut(),
            )
        }
        .ok_or("the state is not a property list")?;
        Ok(data.to_vec())
    }

    /// Restores a state from [`Unit::save_state`].
    pub(crate) fn load_state(&self, bytes: &[u8]) -> Result<(), String> {
        let data = CFData::from_bytes(bytes);
        // SAFETY: `data` is valid; null format and error pointers.
        let plist = unsafe {
            CFPropertyListCreateWithData(None, Some(&data), 0, ptr::null_mut(), ptr::null_mut())
        }
        .ok_or("not a property list")?;
        let raw: *const CFPropertyList = CFRetained::as_ptr(&plist).as_ptr();
        // SAFETY: the unit copies what it needs from the property list,
        // which outlives the call.
        os("AudioUnitSetProperty(ClassInfo)", unsafe {
            AudioUnitSetProperty(
                self.unit,
                kAudioUnitProperty_ClassInfo,
                kAudioUnitScope_Global,
                0,
                (&raw const raw).cast::<c_void>(),
                size_of::<*const c_void>() as u32,
            )
        })
    }

    /// Renders `frames` frames of output bus `bus` into `channels` (each at
    /// least `frames` long), at `sample_time`. False when the unit failed.
    /// // RT
    pub(crate) fn render(
        &self,
        bus: u32,
        sample_time: f64,
        frames: u32,
        channels: &mut [Box<[f32]>],
    ) -> bool {
        let n = channels.len().min(MAX_BUS_CHANNELS);
        let bytes = frames * 4;
        let empty = AudioBuffer {
            mNumberChannels: 1,
            mDataByteSize: 0,
            mData: ptr::null_mut(),
        };
        let mut list = BufferList {
            count: n as u32,
            buffers: [empty; MAX_BUS_CHANNELS],
        };
        for (b, ch) in list.buffers.iter_mut().zip(channels.iter_mut()) {
            b.mDataByteSize = bytes;
            b.mData = ch.as_mut_ptr().cast::<c_void>();
        }
        let mut stamp = timestamp(sample_time);
        let mut flags = AudioUnitRenderActionFlags(0);
        // SAFETY: `list` describes `n` buffers of `frames` f32 owned by
        // `channels`, valid for the call; `stamp` and `flags` are valid. //
        // RT
        let st = unsafe {
            AudioUnitRender(
                self.unit,
                &mut flags,
                NonNull::from(&mut stamp),
                bus,
                frames,
                NonNull::from(&mut list).cast::<AudioBufferList>(),
            )
        };
        if st != 0 {
            return false;
        }
        // If the unit substituted its own buffers, copy from them.
        for (b, ch) in list.buffers.iter().zip(channels.iter_mut()) {
            let ours = ch.as_mut_ptr().cast::<c_void>();
            if b.mData != ours && !b.mData.is_null() {
                // SAFETY: the unit guarantees `mData` holds `frames` samples
                // until its next render. // RT
                let src =
                    unsafe { std::slice::from_raw_parts(b.mData.cast::<f32>(), frames as usize) };
                if let Some(d) = ch.get_mut(..frames as usize) {
                    d.copy_from_slice(src);
                }
            }
        }
        true
    }
}

/// An `AudioBufferList` with room for [`MAX_BUS_CHANNELS`] buffers.
#[repr(C)]
struct BufferList {
    count: u32,
    buffers: [AudioBuffer; MAX_BUS_CHANNELS],
}

fn timestamp(sample_time: f64) -> AudioTimeStamp {
    AudioTimeStamp {
        mSampleTime: sample_time,
        mHostTime: 0,
        mRateScalar: 0.0,
        mWordClockTime: 0,
        mSMPTETime: SMPTETime {
            mSubframes: 0,
            mSubframeDivisor: 0,
            mCounter: 0,
            mType: SMPTETimeType(0),
            mFlags: SMPTETimeFlags(0),
            mHours: 0,
            mMinutes: 0,
            mSeconds: 0,
            mFrames: 0,
        },
        mFlags: AudioTimeStampFlags::SampleTimeValid,
        mReserved: 0,
    }
}

/// Where the input callback reads from: set by the processor before each
/// render. Bus 0 is the main input, bus 1 the sidechain.
#[derive(Debug)]
pub(crate) struct InputFeed {
    /// `(pointer, channels)` per bus; null means silence.
    pub buses: [(*mut Box<[f32]>, usize); 2],
    /// Frames available in each channel.
    pub frames: usize,
    /// Silence, for buses with nothing routed (at least `frames` long).
    pub zeros: Box<[f32]>,
}

impl InputFeed {
    /// A feed with silence for blocks of up to `max_frames`.
    pub(crate) fn new(max_frames: u32) -> InputFeed {
        InputFeed {
            buses: [(ptr::null_mut(), 0); 2],
            frames: 0,
            zeros: vec![0.0; max_frames as usize].into_boxed_slice(),
        }
    }
}

// SAFETY: the raw pointers are only dereferenced by the render callback
// while the processor that owns the feed renders (one thread at a time).
unsafe impl Send for InputFeed {}

/// The AudioUnit input callback: hands the unit our input channels (when
/// its buffers are null) or copies them into its buffers. // RT
unsafe extern "C-unwind" fn render_input(
    refcon: NonNull<c_void>,
    _flags: NonNull<AudioUnitRenderActionFlags>,
    _stamp: NonNull<AudioTimeStamp>,
    bus: u32,
    frames: u32,
    io_data: *mut AudioBufferList,
) -> i32 {
    // SAFETY: `refcon` is the processor's boxed `InputFeed`, alive and not
    // otherwise borrowed while the processor is inside `AudioUnitRender`.
    let feed = unsafe { refcon.cast::<InputFeed>().as_mut() };
    let Some(list) = NonNull::new(io_data) else {
        return 0;
    };
    // SAFETY: the unit passes a valid list with `mNumberBuffers` buffers.
    let count = unsafe { (*list.as_ptr()).mNumberBuffers } as usize;
    // SAFETY: the buffers trail the header, `count` of them.
    let buffers = unsafe {
        std::slice::from_raw_parts_mut(
            (&raw mut (*list.as_ptr()).mBuffers).cast::<AudioBuffer>(),
            count,
        )
    };
    let (src, channels) = feed
        .buses
        .get(bus as usize)
        .copied()
        .unwrap_or((ptr::null_mut(), 0));
    let n = (frames as usize).min(feed.frames);
    for (k, b) in buffers.iter_mut().enumerate() {
        // A mono source feeds every channel; nothing routed is silence.
        let chan: Option<&mut Box<[f32]>> = if src.is_null() || channels == 0 {
            None
        } else {
            // SAFETY: `src` points at `channels` boxed channels owned by the
            // processor, valid and unaliased during the render.
            Some(unsafe { &mut *src.add(k.min(channels - 1)) })
        };
        if b.mData.is_null() {
            // The unit wants our buffer: lend it the channel (or silence).
            let data: &mut [f32] = match chan {
                Some(c) => c,
                None => &mut feed.zeros,
            };
            b.mData = data.as_mut_ptr().cast::<c_void>();
            b.mDataByteSize = (n.min(data.len()) * 4) as u32;
            continue;
        }
        let want = (frames as usize).min(b.mDataByteSize as usize / 4);
        // SAFETY: the unit's buffer holds `mDataByteSize` bytes. // RT
        let dst = unsafe { std::slice::from_raw_parts_mut(b.mData.cast::<f32>(), want) };
        let src_samples = chan.and_then(|c| c.get(..n.min(want)));
        let copied = match src_samples {
            Some(s) => {
                if let Some(d) = dst.get_mut(..s.len()) {
                    d.copy_from_slice(s);
                }
                s.len()
            }
            None => 0,
        };
        if let Some(rest) = dst.get_mut(copied..) {
            rest.fill(0.0);
        }
    }
    0
}

/// The latency property listener. // any thread
unsafe extern "C-unwind" fn latency_changed(
    refcon: NonNull<c_void>,
    _unit: AudioUnit,
    _id: AudioUnitPropertyID,
    _scope: AudioUnitScope,
    _element: u32,
) {
    // SAFETY: `refcon` is the `AtomicBool` the unit's owner keeps alive.
    let flag = unsafe { refcon.cast::<AtomicBool>().as_ref() };
    flag.store(true, Ordering::Release);
}

/// `AudioUnitEvent` (not in the generated bindings: it holds a union).
#[repr(C)]
#[derive(Clone, Copy)]
struct AudioUnitEvent {
    event_type: AudioUnitEventType,
    argument: AudioUnitParameter,
}

type ListenerBlock = block2::DynBlock<dyn Fn(*mut c_void, *const c_void, u64, f32)>;

unsafe extern "C-unwind" {
    fn AUEventListenerCreateWithDispatchQueue(
        out_listener: *mut AUParameterListenerRef,
        notification_interval: f32,
        value_change_granularity: f32,
        queue: *mut c_void,
        block: &ListenerBlock,
    ) -> i32;
    fn AUEventListenerAddEventType(
        listener: AUParameterListenerRef,
        object: *mut c_void,
        event: *const AudioUnitEvent,
    ) -> i32;
    fn AUEventListenerNotify(
        listener: AUParameterListenerRef,
        object: *mut c_void,
        event: *const AudioUnitEvent,
    ) -> i32;
    fn dispatch_queue_create(label: *const std::ffi::c_char, attr: *mut c_void) -> *mut c_void;
    fn dispatch_release(object: *mut c_void);
}

/// A UI parameter event: `(kind, parameter, value)` with kind 0 = value,
/// 1 = begin gesture, 2 = end gesture.
pub(crate) type UiEvent = (u32, u32, f32);

/// Listens for parameter changes and gestures made in the plugin's own UI.
pub(crate) struct GestureListener {
    listener: AUParameterListenerRef,
    /// A serial queue, so events arrive in order.
    queue: *mut c_void,
    events: Arc<Mutex<Vec<UiEvent>>>,
}

// SAFETY: the listener reference is used only by its owner; the callback
// side touches only the `Mutex`.
unsafe impl Send for GestureListener {}

impl GestureListener {
    /// Listens to every parameter in `ids` of `unit`.
    pub(crate) fn new(unit: &Unit, ids: &[u32]) -> Result<GestureListener, String> {
        let events: Arc<Mutex<Vec<UiEvent>>> = Arc::default();
        let sink = Arc::clone(&events);
        let block = RcBlock::new(
            move |_obj: *mut c_void, ev: *const c_void, _t: u64, value: f32| {
                // SAFETY: the system passes a valid `AudioUnitEvent` for the
                // call (or null, ignored).
                let Some(ev) = (unsafe { ev.cast::<AudioUnitEvent>().as_ref() }) else {
                    return;
                };
                if let Ok(mut v) = sink.lock() {
                    v.push((ev.event_type.0, ev.argument.mParameterID, value));
                }
            },
        );
        let mut listener: AUParameterListenerRef = ptr::null_mut();
        // SAFETY: a static label; null attributes make a serial queue.
        let queue =
            unsafe { dispatch_queue_create(c"org.vizdaw.au-gestures".as_ptr(), ptr::null_mut()) };
        // SAFETY: valid out-pointer and queue; the block is retained by the
        // listener.
        let st = unsafe {
            AUEventListenerCreateWithDispatchQueue(&mut listener, 0.0, 0.0, queue, &block)
        };
        if st != 0 {
            // SAFETY: the queue was created above and is not used.
            unsafe { dispatch_release(queue) };
            return Err(format!(
                "AUEventListenerCreateWithDispatchQueue failed with status {st}"
            ));
        }
        let me = GestureListener {
            listener,
            queue,
            events,
        };
        for &id in ids {
            for kind in [
                AudioUnitEventType::ParameterValueChange,
                AudioUnitEventType::BeginParameterChangeGesture,
                AudioUnitEventType::EndParameterChangeGesture,
            ] {
                let ev = event(unit, kind, id);
                // SAFETY: `ev` is valid for the call; the listener is open.
                unsafe { AUEventListenerAddEventType(me.listener, ptr::null_mut(), &ev) };
            }
        }
        Ok(me)
    }

    /// Events since the last call, in arrival order.
    pub(crate) fn take(&self) -> Vec<UiEvent> {
        self.events
            .lock()
            .map(|mut v| std::mem::take(&mut *v))
            .unwrap_or_default()
    }
}

impl Drop for GestureListener {
    fn drop(&mut self) {
        // SAFETY: the listener and queue were created by `new` and are
        // released once, the listener first.
        unsafe {
            AUListenerDispose(self.listener);
            dispatch_release(self.queue);
        }
    }
}

fn event(unit: &Unit, kind: AudioUnitEventType, id: u32) -> AudioUnitEvent {
    AudioUnitEvent {
        event_type: kind,
        argument: AudioUnitParameter {
            mAudioUnit: unit.unit,
            mParameterID: id,
            mScope: kAudioUnitScope_Global,
            mElement: 0,
        },
    }
}

/// Does what a plugin's own UI does when the user drags a control: begin
/// gesture, set the value (notifying listeners), end gesture. Tests use it
/// to stand in for a user.
pub(crate) fn ui_gesture(unit: &Unit, id: u32, value: f32) {
    let begin = event(unit, AudioUnitEventType::BeginParameterChangeGesture, id);
    let end = event(unit, AudioUnitEventType::EndParameterChangeGesture, id);
    let mut param = begin.argument;
    // SAFETY: the events and parameter are valid for each call; a null
    // sending listener notifies every listener.
    unsafe {
        AUEventListenerNotify(ptr::null_mut(), ptr::null_mut(), &begin);
        AUParameterSet(
            ptr::null_mut(),
            ptr::null_mut(),
            NonNull::from(&mut param),
            value,
            0,
        );
        AUEventListenerNotify(ptr::null_mut(), ptr::null_mut(), &end);
    }
}
VIZDAW_EOF
```

Create `crates/platform-macos/src/au/mod.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p crates/platform-macos/src/au
cat > crates/platform-macos/src/au/mod.rs <<'VIZDAW_EOF'
//! The Audio Unit host (spec §5): AU v2 and v3 components through the
//! AudioUnit C API (Plan Ruling 6). v2 instances are created on the loader
//! thread; v3 instances asynchronously, out of process when the component
//! allows. Parameters use the AU parameter API, plugin-UI gestures the AU
//! event listener, state the `ClassInfo` property as a binary property
//! list. All `unsafe` is in `ffi.rs`.

mod ffi;

use core_engine::graph::ProcessStatus;
use core_host::catalog::Source;
use core_host::{
    AudioPortInfo, AudioProcessor, HostError, InstanceEvents, LoadRequest, ParamEvent, ParamInfo,
    PluginEditor, PluginFormat, PluginInfo, PluginInstance, PluginKind, PortRole, PrepareJob,
    Prepared, ProcessCtx, Tail,
};
use core_model::mixer::{ParamId, PluginFormat as Format, PluginRef};
use ffi::{ComponentInfo, GestureListener, InputFeed, Unit};
use objc2_audio_toolbox::{
    AudioUnitParameterOptions, kAudioUnitScope_Input, kAudioUnitScope_Output,
    kAudioUnitType_Effect, kAudioUnitType_MusicDevice, kAudioUnitType_MusicEffect,
};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// Component types VizDAW hosts: effects, instruments and MIDI-driven
/// effects.
const TYPES: [u32; 3] = [
    kAudioUnitType_Effect,
    kAudioUnitType_MusicDevice,
    kAudioUnitType_MusicEffect,
];
/// `kAudioUnitParameterUnit_Indexed` and `_Boolean`.
const STEPPED_UNITS: [u32; 2] = [1, 2];

fn fourcc(v: u32) -> String {
    v.to_be_bytes().iter().map(|b| char::from(*b)).collect()
}

fn parse_fourcc(s: &str) -> Option<u32> {
    let b: [u8; 4] = s.as_bytes().try_into().ok()?;
    Some(u32::from_be_bytes(b))
}

/// `type/subtype/manufacturer` four-char codes (spec §4), e.g.
/// `aufx/dely/appl`.
pub fn au_id(kind: u32, subtype: u32, manufacturer: u32) -> String {
    format!(
        "{}/{}/{}",
        fourcc(kind),
        fourcc(subtype),
        fourcc(manufacturer)
    )
}

/// The codes of an AU id.
pub fn parse_au_id(id: &str) -> Option<(u32, u32, u32)> {
    let mut it = id.split('/');
    let (a, b, c) = (it.next()?, it.next()?, it.next()?);
    if it.next().is_some() {
        return None;
    }
    Some((parse_fourcc(a)?, parse_fourcc(b)?, parse_fourcc(c)?))
}

fn version_string(v: u32) -> String {
    format!("{}.{}.{}", v >> 16, (v >> 8) & 0xFF, v & 0xFF)
}

/// An installed component.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuComponent {
    /// Format, id, name, vendor, version.
    pub plugin: PluginRef,
    /// Instrument or effect.
    pub kind: PluginKind,
    /// Implemented with the v3 API.
    pub v3: bool,
}

fn describe(c: &ComponentInfo) -> AuComponent {
    let (vendor, name) = match c.name.split_once(": ") {
        Some((v, n)) => (v.to_string(), n.to_string()),
        None => (String::new(), c.name.clone()),
    };
    AuComponent {
        plugin: PluginRef {
            format: Format::Au,
            id: au_id(c.kind, c.subtype, c.manufacturer),
            name,
            vendor,
            version: version_string(c.version),
        },
        kind: if c.kind == kAudioUnitType_MusicDevice {
            PluginKind::Instrument
        } else {
            PluginKind::Effect
        },
        v3: c.v3,
    }
}

/// Every installed effect and instrument component (no plugin code runs:
/// this only reads the component registry).
pub fn components() -> Vec<AuComponent> {
    ffi::components(&TYPES).iter().map(describe).collect()
}

/// The component of `id`, if installed.
pub fn component(id: &str) -> Option<AuComponent> {
    let (k, s, m) = parse_au_id(id)?;
    ffi::find(k, s, m).map(|c| describe(&c))
}

/// One catalog source per component (path `au:<id>`, its version, no
/// modification time).
pub fn sources() -> Vec<Source> {
    components()
        .into_iter()
        .map(|c| Source {
            format: Format::Au,
            path: PathBuf::from(format!("au:{}", c.plugin.id)),
            version: c.plugin.version,
            mtime: 0,
        })
        .collect()
}

fn locate(id: &str) -> Result<ComponentInfo, HostError> {
    let (k, s, m) =
        parse_au_id(id).ok_or_else(|| HostError::NotFound(format!("bad AU id {id:?}")))?;
    ffi::find(k, s, m).ok_or_else(|| HostError::NotFound(id.to_string()))
}

/// Opens a component: v2 synchronously, v3 asynchronously (out of process
/// first, then in process).
fn open(c: &ComponentInfo) -> Result<Unit, HostError> {
    if !c.v3 {
        return ffi::instantiate_v2(c.component).map_err(HostError::LoadFailed);
    }
    let wait = core_host::scan::TIMEOUT_AU_V3;
    ffi::instantiate_async(c.component, true, wait)
        .or_else(|_| ffi::instantiate_async(c.component, false, wait))
        .map_err(HostError::LoadFailed)
}

/// What the loader made: an open, configured, initialized unit.
struct Opened {
    unit: Unit,
    desc: AuComponent,
    kind: u32,
    config: (f64, u32),
}

/// The AU [`PluginFormat`].
#[derive(Debug, Default)]
pub struct AuFormat;

impl AuFormat {
    /// The format.
    pub fn new() -> AuFormat {
        AuFormat
    }
}

impl PluginFormat for AuFormat {
    fn format(&self) -> Format {
        Format::Au
    }

    fn prepare(&self, req: &LoadRequest) -> PrepareJob {
        let id = req.plugin.id.clone();
        let (sr, max) = (req.sample_rate, req.max_frames);
        Box::new(move || {
            let c = locate(&id)?;
            let unit = open(&c)?;
            configure(&unit, c.kind, sr, max)?;
            unit.initialize().map_err(HostError::ActivateFailed)?;
            Ok(Prepared(Box::new(Opened {
                desc: describe(&c),
                kind: c.kind,
                unit,
                config: (sr, max),
            })))
        })
    }

    fn finish(
        &mut self,
        _req: &LoadRequest,
        prepared: Prepared,
    ) -> Result<Box<dyn PluginInstance>, HostError> {
        let o = prepared
            .0
            .downcast::<Opened>()
            .map_err(|_| HostError::LoadFailed("not an AU load".into()))?;
        Ok(Box::new(AuInstance::new(*o)?))
    }
}

/// Sets every bus to non-interleaved f32 (1 or 2 channels) and the block
/// size.
fn configure(unit: &Unit, kind: u32, sr: f64, max: u32) -> Result<(), HostError> {
    let err = HostError::ActivateFailed;
    unit.set_max_frames(max).map_err(err)?;
    for bus in 0..unit.element_count(kAudioUnitScope_Output) {
        let ch = unit.channels(kAudioUnitScope_Output, bus).clamp(1, 2);
        unit.set_format(kAudioUnitScope_Output, bus, sr, ch)
            .map_err(HostError::ActivateFailed)?;
    }
    // Effects that can take a second input bus have a sidechain (AU bus 1,
    // spec §5); most Apple effects refuse.
    if kind != kAudioUnitType_MusicDevice && unit.element_count(kAudioUnitScope_Input) == 1 {
        let _ = unit.set_element_count(kAudioUnitScope_Input, 2);
    }
    let inputs = unit.element_count(kAudioUnitScope_Input);
    for bus in 0..inputs.min(2) {
        let ch = unit.channels(kAudioUnitScope_Input, bus).clamp(1, 2);
        // A sidechain bus that refuses our format is simply not offered.
        let r = unit.set_format(kAudioUnitScope_Input, bus, sr, ch);
        if bus == 0 && kind != kAudioUnitType_MusicDevice {
            r.map_err(HostError::ActivateFailed)?;
        }
    }
    Ok(())
}

/// A loaded AU (main-thread half).
pub struct AuInstance {
    unit: Arc<Unit>,
    kind: u32,
    info: PluginInfo,
    config: Option<(f64, u32)>,
    active: bool,
    latency: Arc<AtomicBool>,
    listener: Option<GestureListener>,
}

impl std::fmt::Debug for AuInstance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuInstance")
            .field("plugin", &self.info.plugin.id)
            .finish_non_exhaustive()
    }
}

impl AuInstance {
    fn new(mut o: Opened) -> Result<AuInstance, HostError> {
        let latency = Arc::new(AtomicBool::new(false));
        // A unit that cannot report latency changes still works.
        let _ = o.unit.watch_latency(Arc::clone(&latency));
        let mut me = AuInstance {
            unit: Arc::new(o.unit),
            kind: o.kind,
            info: PluginInfo {
                plugin: o.desc.plugin,
                kind: o.desc.kind,
                inputs: Vec::new(),
                outputs: Vec::new(),
                note_input: false,
                params: Vec::new(),
                latency: 0,
                tail: Tail::None,
            },
            config: Some(o.config),
            active: false,
            latency,
            listener: None,
        };
        me.read_info();
        let ids: Vec<u32> = me.unit.parameter_ids();
        me.listener = GestureListener::new(&me.unit, &ids).ok();
        Ok(me)
    }

    fn read_info(&mut self) {
        let u = &self.unit;
        let instrument = self.kind == kAudioUnitType_MusicDevice;
        let port = |name: String, ch: u32, role| AudioPortInfo {
            name,
            channels: u16::try_from(ch.clamp(1, 2)).unwrap_or(2),
            role,
        };
        self.info.inputs = if instrument {
            Vec::new()
        } else {
            (0..u.element_count(kAudioUnitScope_Input).min(2))
                .map(|b| {
                    let role = if b == 0 {
                        PortRole::Main
                    } else {
                        PortRole::Sidechain
                    };
                    port(
                        if b == 0 {
                            "In".into()
                        } else {
                            "Sidechain".into()
                        },
                        u.channels(kAudioUnitScope_Input, b),
                        role,
                    )
                })
                .collect()
        };
        self.info.outputs = (0..u.element_count(kAudioUnitScope_Output))
            .map(|b| {
                port(
                    format!("Out {}", b + 1),
                    u.channels(kAudioUnitScope_Output, b),
                    if b == 0 {
                        PortRole::Main
                    } else {
                        PortRole::Aux
                    },
                )
            })
            .collect();
        self.info.note_input = instrument || self.kind == kAudioUnitType_MusicEffect;
        self.info.params = u
            .parameter_ids()
            .into_iter()
            .filter_map(|id| {
                let (name, min, max, def, flags, unit) = u.parameter_info(id)?;
                Some(ParamInfo {
                    id: ParamId(u64::from(id)),
                    name,
                    module: String::new(),
                    min: f64::from(min),
                    max: f64::from(max),
                    default: f64::from(def),
                    stepped: STEPPED_UNITS.contains(&unit),
                    automatable: flags.contains(AudioUnitParameterOptions::Flag_IsWritable),
                    hidden: !flags.contains(AudioUnitParameterOptions::Flag_IsReadable),
                })
            })
            .collect();
        let sr = self.config.map_or(48_000.0, |c| c.0);
        self.info.latency = (u.latency() * sr).round().max(0.0) as u32;
        let tail = u.tail();
        self.info.tail = if tail > 1000.0 {
            Tail::Infinite
        } else if tail > 0.0 {
            Tail::Frames((tail * sr).round() as u32)
        } else {
            Tail::None
        };
    }

    /// Acts like the plugin's own UI changing parameter `id` (begin
    /// gesture, value, end gesture). For tests: Apple's built-in units have
    /// no scriptable UI.
    pub fn simulate_ui_gesture(&self, id: ParamId, value: f64) {
        if let Ok(id) = u32::try_from(id.0) {
            ffi::ui_gesture(&self.unit, id, value as f32);
        }
    }
}

impl PluginInstance for AuInstance {
    fn info(&self) -> &PluginInfo {
        &self.info
    }

    fn activate(
        &mut self,
        sample_rate: f64,
        max_frames: u32,
    ) -> Result<Box<dyn AudioProcessor>, HostError> {
        if self.active {
            return Err(HostError::ActivateFailed("already active".into()));
        }
        let mut feed = Box::new(InputFeed::new(max_frames));
        let refcon = std::ptr::from_mut::<InputFeed>(&mut feed).cast::<std::ffi::c_void>();
        // The input callbacks are set while uninitialized: some units take
        // their connections only at initialization.
        self.unit.uninitialize();
        configure(&self.unit, self.kind, sample_rate, max_frames)?;
        for bus in 0..self.unit.element_count(kAudioUnitScope_Input).min(2) {
            self.unit
                .set_input(bus, refcon)
                .map_err(HostError::ActivateFailed)?;
        }
        self.unit.initialize().map_err(HostError::ActivateFailed)?;
        self.config = Some((sample_rate, max_frames));
        self.read_info();
        self.active = true;
        Ok(Box::new(AuProcessor {
            unit: Arc::clone(&self.unit),
            feed,
            sample_time: 0.0,
            out_counts: self
                .info
                .outputs
                .iter()
                .map(|p| usize::from(p.channels))
                .collect(),
            max_frames,
        }))
    }

    fn deactivate(&mut self, processor: Box<dyn AudioProcessor>) {
        drop(processor);
        self.active = false;
    }

    fn is_active(&self) -> bool {
        self.active
    }

    fn save_state(&mut self) -> Result<Vec<u8>, HostError> {
        self.unit.save_state().map_err(HostError::StateRejected)
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), HostError> {
        self.unit.load_state(data).map_err(HostError::StateRejected)
    }

    fn set_params(&mut self, values: &[(ParamId, f64)]) -> Result<(), HostError> {
        for (id, v) in values {
            if let Ok(id) = u32::try_from(id.0) {
                self.unit.set_param(id, *v as f32, 0);
            }
        }
        Ok(())
    }

    fn param_value(&mut self, id: ParamId) -> Option<f64> {
        self.unit
            .get_param(u32::try_from(id.0).ok()?)
            .map(f64::from)
    }

    fn editor(&mut self) -> Option<&mut dyn PluginEditor> {
        // AU editors (view controllers, Cocoa UI) arrive with the app's
        // plugin windows (3c, Plan Ruling 7).
        None
    }

    fn idle(&mut self) -> InstanceEvents {
        InstanceEvents {
            latency_changed: self.latency.swap(false, Ordering::AcqRel),
            ..InstanceEvents::default()
        }
    }

    fn gestures(&mut self) -> Vec<ParamEvent> {
        let Some(l) = &self.listener else {
            return Vec::new();
        };
        l.take()
            .into_iter()
            .filter_map(|(kind, id, value)| {
                let id = ParamId(u64::from(id));
                match kind {
                    0 => Some(ParamEvent::Value(id, f64::from(value))),
                    1 => Some(ParamEvent::Begin(id)),
                    2 => Some(ParamEvent::End(id)),
                    _ => None,
                }
            })
            .collect()
    }
}

/// The real-time half of an [`AuInstance`].
pub struct AuProcessor {
    unit: Arc<Unit>,
    feed: Box<InputFeed>,
    sample_time: f64,
    out_counts: Vec<usize>,
    max_frames: u32,
}

impl std::fmt::Debug for AuProcessor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuProcessor").finish_non_exhaustive()
    }
}

impl AudioProcessor for AuProcessor {
    fn process(&mut self, ctx: &mut ProcessCtx<'_>) -> ProcessStatus {
        let n = ctx.frames.min(self.max_frames);
        for p in ctx.params_in {
            if let Ok(id) = u32::try_from(p.id.0) {
                self.unit.set_param(id, p.value as f32, p.frame.min(n));
            }
        }
        for e in ctx.events {
            let [s, d1, d2] = e.bytes;
            self.unit.midi(s, d1, d2, e.frame.min(n.saturating_sub(1)));
        }
        self.feed.buses = [
            (ctx.input.as_mut_ptr(), ctx.input.len()),
            (ctx.sidechain.as_mut_ptr(), ctx.sidechain.len()),
        ];
        self.feed.frames = n as usize;
        let mut ok = true;
        let mut rest: &mut [Box<[f32]>] = ctx.outputs;
        for (bus, count) in self.out_counts.iter().enumerate() {
            let take = (*count).min(rest.len());
            let (port, tail) = std::mem::take(&mut rest).split_at_mut(take);
            rest = tail;
            ok &= self.unit.render(bus as u32, self.sample_time, n, port);
        }
        self.feed.buses = [(std::ptr::null_mut(), 0); 2];
        self.sample_time += f64::from(n);
        if ok {
            ProcessStatus::Ok
        } else {
            ProcessStatus::Error
        }
    }

    fn reset(&mut self) {
        self.unit.reset();
    }
}

/// Loads a component for the scanner and the tests: instantiate, configure
/// and initialize at `sample_rate` / `max_frames`.
pub fn load(id: &str, sample_rate: f64, max_frames: u32) -> Result<AuInstance, HostError> {
    let f = AuFormat::new();
    let req = LoadRequest {
        plugin: PluginRef {
            format: Format::Au,
            id: id.to_string(),
            name: id.to_string(),
            vendor: String::new(),
            version: String::new(),
        },
        path: None,
        sample_rate,
        max_frames,
    };
    let prepared = (f.prepare(&req))()?;
    let o = prepared
        .0
        .downcast::<Opened>()
        .map_err(|_| HostError::LoadFailed("not an AU load".into()))?;
    AuInstance::new(*o)
}

/// How long the scanner waits for an AU.
pub fn scan_timeout(v3: bool) -> Duration {
    if v3 {
        core_host::scan::TIMEOUT_AU_V3
    } else {
        core_host::scan::TIMEOUT
    }
}
VIZDAW_EOF
```

Change `crates/core-host/src/clap/processor.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-host/src/clap/processor.rs b/crates/core-host/src/clap/processor.rs
--- a/crates/core-host/src/clap/processor.rs
+++ b/crates/core-host/src/clap/processor.rs
@@ -167,8 +167,8 @@ impl ClapProcessor {
             }
         }
         for (port, src) in [
-            (self.main_in, ctx.input),
-            (self.sidechain_in, ctx.sidechain),
+            (self.main_in, &*ctx.input),
+            (self.sidechain_in, &*ctx.sidechain),
         ] {
             let Some(port) = port else {
                 continue;
VIZDAW_EOF
```

Change `crates/core-host/src/host.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-host/src/host.rs b/crates/core-host/src/host.rs
--- a/crates/core-host/src/host.rs
+++ b/crates/core-host/src/host.rs
@@ -614,7 +614,9 @@ impl PluginHost {
             return;
         };
         let ev = live.inst.idle();
-        for e in live.gestures.drain() {
+        let mut events = live.gestures.drain();
+        events.extend(live.inst.gestures());
+        for e in events {
             match e {
                 ParamEvent::Begin(_) => live.open += 1,
                 ParamEvent::Value(p, v) => {
VIZDAW_EOF
```

Change `crates/core-host/src/instance.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-host/src/instance.rs b/crates/core-host/src/instance.rs
--- a/crates/core-host/src/instance.rs
+++ b/crates/core-host/src/instance.rs
@@ -85,6 +85,12 @@ pub trait PluginInstance {
     /// Main-thread servicing (`host_idle`, 60 Hz): runs callbacks the
     /// plugin asked for and reports what it requested.
     fn idle(&mut self) -> InstanceEvents;
+
+    /// Parameter events from the plugin's UI that reach the host on the
+    /// main thread (AU listeners); CLAP reports them from `process`.
+    fn gestures(&mut self) -> Vec<crate::process::ParamEvent> {
+        Vec::new()
+    }
 }
 
 /// What to load.
VIZDAW_EOF
```

Change `crates/core-host/src/process.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-host/src/process.rs b/crates/core-host/src/process.rs
--- a/crates/core-host/src/process.rs
+++ b/crates/core-host/src/process.rs
@@ -119,11 +119,13 @@ impl ParamOut {
 pub struct ProcessCtx<'a> {
     /// Frames to process.
     pub frames: u32,
-    /// Main input channels (empty for instruments).
-    pub input: &'a [Box<[f32]>],
+    /// Main input channels (empty for instruments). Mutable so formats
+    /// that take the host's buffers (AU) can lend them; a plugin must not
+    /// rely on what it finds there afterwards.
+    pub input: &'a mut [Box<[f32]>],
     /// Sidechain input channels, when the plugin has a sidechain port
     /// (silent when nothing is routed to it).
-    pub sidechain: &'a [Box<[f32]>],
+    pub sidechain: &'a mut [Box<[f32]>],
     /// Output channels of every port.
     pub outputs: &'a mut [Box<[f32]>],
     /// MIDI events sorted by frame.
VIZDAW_EOF
```

Change `crates/platform-macos/Cargo.toml`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/platform-macos/Cargo.toml b/crates/platform-macos/Cargo.toml
--- a/crates/platform-macos/Cargo.toml
+++ b/crates/platform-macos/Cargo.toml
@@ -7,14 +7,15 @@ rust-version.workspace = true
 
 [target.'cfg(target_os = "macos")'.dependencies]
 core-engine = { path = "../core-engine" }
+core-host = { path = "../core-host" }
+core-model = { path = "../core-model" }
 rtrb = "0.4"
 block2 = "0.6"
 objc2-core-audio = { version = "0.3.2", default-features = false, features = ["std", "AudioHardware", "HostTime", "objc2-core-audio-types"] }
 objc2-core-audio-types = { version = "0.3.2", default-features = false, features = ["std", "CoreAudioBaseTypes"] }
-objc2-audio-toolbox = { version = "0.3.2", default-features = false, features = ["std", "AudioComponent", "AUComponent", "AudioUnitProperties", "MusicDevice", "objc2-core-audio-types"] }
+objc2-audio-toolbox = { version = "0.3.2", default-features = false, features = ["std", "AudioComponent", "AUComponent", "AudioUnitProperties", "AudioUnitUtilities", "MusicDevice", "objc2-core-audio-types", "objc2-core-foundation", "block2"] }
 objc2-core-midi = { version = "0.3.2", default-features = false, features = ["std", "MIDIServices", "MIDIMessages", "objc2", "objc2-core-foundation", "block2"] }
-objc2-core-foundation = { version = "0.3.2", default-features = false, features = ["std", "CFString"] }
+objc2-core-foundation = { version = "0.3.2", default-features = false, features = ["std", "CFString", "CFData", "CFError", "CFPropertyList"] }
 
 [target.'cfg(target_os = "macos")'.dev-dependencies]
 core-engine = { path = "../core-engine", features = ["fixtures"] }
-core-model = { path = "../core-model" }
VIZDAW_EOF
```

Change `crates/platform-macos/src/lib.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/platform-macos/src/lib.rs b/crates/platform-macos/src/lib.rs
--- a/crates/platform-macos/src/lib.rs
+++ b/crates/platform-macos/src/lib.rs
@@ -1,8 +1,9 @@
 //! `platform-macos`: CoreAudio HAL output, CoreMIDI output and Apple's DLS
-//! synth as the built-in synth, implementing `core-engine`'s backend traits.
+//! synth as the built-in synth, implementing `core-engine`'s backend traits,
+//! and the Audio Unit plugin host (`au`).
 //!
-//! All `unsafe` lives in `hal.rs`, `dls.rs`, `midi.rs` and `rt.rs`, with a
-//! `// SAFETY:` comment per block.
+//! All `unsafe` lives in `hal.rs`, `dls.rs`, `midi.rs`, `rt.rs` and
+//! `au/ffi.rs`, with a `// SAFETY:` comment per block.
 #![cfg(target_os = "macos")]
 #![deny(missing_docs)]
 #![cfg_attr(
@@ -15,6 +16,7 @@
     )
 )]
 
+pub mod au;
 pub mod dls;
 pub mod hal;
 pub mod midi;
VIZDAW_EOF
```

- [ ] **Step 4: Run the tests to see them pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p platform-macos --test au
```

Expected: 7 tests pass (`enumeration_finds_apples_units`, `the_delay_reports_parameters_and_processes`, `the_dynamics_processor_processes_and_apples_units_offer_no_sidechain`, `the_matrix_reverb_rings`, `the_dls_synth_plays_a_note`, `state_round_trips`, `ui_gestures_are_captured`). No audio device is opened.

- [ ] **Step 5: Check the whole workspace**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Expected: no formatting changes left, no clippy warnings, every test passes.

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/platform-macos/src/au/ffi.rs \
  crates/platform-macos/src/au/mod.rs \
  crates/platform-macos/tests/au.rs \
  crates/core-host/src/clap/processor.rs \
  crates/core-host/src/host.rs \
  crates/core-host/src/instance.rs \
  crates/core-host/src/process.rs \
  crates/platform-macos/Cargo.toml \
  crates/platform-macos/src/lib.rs \
  Cargo.lock
git commit -m "$(cat <<'MSG'
feat(platform-macos): AU v2/v3 host on the AudioUnit API

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
MSG
)"
```


---

### Task 13: AU plugins through the host, the engine and the scanner

**Files:**
- Create: `crates/platform-macos/tests/au_host.rs`
- Modify: `crates/vizdaw-scan/Cargo.toml`
- Modify: `crates/vizdaw-scan/src/main.rs`
- Modify: `crates/vizdaw-scan/tests/scan.rs`
- Lockfile: `Cargo.lock` (cargo updates it when dependencies change)

**Interfaces:**
- Consumes: Task 12's `AuFormat`, `component`; Task 8's `PluginHost`; Task 11's scanner.
- Produces: `vizdaw-scan --format au --path au:<id> --list` and `--id <id>` (AU v3 load failure → `NotValidated`); `platform-macos/tests/au_host.rs`.

Proves Apple's units play through the whole chain: an `AUDelay` insert at 0 % wet leaves the built-in synth's strip unchanged sample for sample (within 1e-3), `DLSMusicDevice` plays as a plugin instrument destination, and an AU's state survives capture and a fresh host. The scanner gains its AU mode (spec §8: a v3 unit it cannot load is "not validated").

- [ ] **Step 1: Write the failing tests**


Create `crates/platform-macos/tests/au_host.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p crates/platform-macos/tests
cat > crates/platform-macos/tests/au_host.rs <<'VIZDAW_EOF'
//! Apple's units through the plugin host and the engine (offline, never on
//! a device): an AU insert, an AU instrument, overlay values and state
//! capture.

use core_engine::backend::AudioBackend;
use core_engine::engine::{Engine, EngineConfig, find_sequence};
use core_engine::fixtures::{Builder, Q, demo_document};
use core_engine::offline::{OfflineBackend, OfflineSinks};
use core_host::PluginHost;
use core_model::command::{Command, MixerCmd};
use core_model::history::History;
use core_model::mixer::{ParamId, PluginFormat, PluginRef, StripKind};
use core_model::selection::ViewFilter;
use platform_macos::au::{AuFormat, component};
use std::time::Duration;

fn au(id: &str) -> PluginRef {
    component(id).unwrap().plugin
}

fn host() -> PluginHost {
    let mut h = PluginHost::new(48_000.0, 4096).unwrap();
    h.add_format(Box::new(AuFormat::new()));
    h
}

fn engine(h: &PluginHost) -> (Engine, OfflineBackend) {
    let (mut e, r) = Engine::new(
        EngineConfig::new(48_000.0),
        Box::new(OfflineSinks::new(1 << 16).with_tone()),
    )
    .unwrap();
    e.set_insert_factory(Box::new(h.factory()));
    let mut b = OfflineBackend::new(48_000.0, 512);
    b.start(r).unwrap();
    (e, b)
}

fn render(b: &mut OfflineBackend, frames: usize) -> Vec<f32> {
    let mut out = Vec::new();
    while out.len() < frames {
        b.block(512);
        out.extend_from_slice(b.output(512).0);
    }
    out
}

#[test]
fn a_dry_au_delay_insert_passes_the_strip_unchanged() {
    let doc = demo_document().unwrap();
    let song = find_sequence(&doc, "Song").unwrap();
    let (mut plain, mut pb) = engine(&host());
    plain.load(&doc, song).unwrap();
    plain.play().unwrap();
    let dry = render(&mut pb, 48_000);

    let mut b = Builder::from_doc(doc);
    let synth = b.strip_of(StripKind::BuiltinSynth).unwrap();
    let delay = b.insert(synth, au("aufx/dely/appl")).unwrap();
    b.mixer(MixerCmd::SetPluginParams {
        slot: delay,
        params: vec![(ParamId(0), 0.0)],
    })
    .unwrap();
    let doc = b.finish();
    let mut h = host();
    h.sync(&doc);
    let r = h.wait_ready(Duration::from_secs(10));
    assert_eq!(r.loaded, vec![delay], "{:?}", r.failed);
    let (mut e, mut be) = engine(&h);
    let report = e.load(&doc, song).unwrap();
    assert!(report.graph.placeholders.is_empty(), "{:?}", report.graph);
    e.play().unwrap();
    let wet = render(&mut be, 48_000);
    let err = dry
        .iter()
        .zip(&wet)
        .map(|(d, w)| (d - w).abs())
        .fold(0.0f32, f32::max);
    assert!(dry.iter().any(|x| *x != 0.0));
    assert!(err < 1e-3, "max difference {err}");
    drop(e);
}

#[test]
fn the_dls_unit_plays_as_a_plugin_instrument() {
    let mut b = Builder::new();
    let (d, id) = b.plugin_dest("GM", au("aumu/dls /appl")).unwrap();
    let (seq, lane) = b.sequence("S", "L", Some(d)).unwrap();
    b.notes(seq, lane, &[(0, 60, 0xFFFF, 4 * Q)]).unwrap();
    let doc = b.finish();
    let mut h = host();
    h.sync(&doc);
    assert_eq!(h.wait_ready(Duration::from_secs(10)).loaded, vec![id]);
    let (mut e, mut be) = engine(&h);
    let report = e.load(&doc, seq).unwrap();
    assert!(report.unroutable.is_empty(), "{:?}", report.warnings);
    e.play().unwrap();
    let out = render(&mut be, 24_000);
    assert!(out.iter().any(|x| x.abs() > 1e-3));
    drop(e);
}

#[test]
fn au_state_is_captured_and_restored() {
    let doc = demo_document().unwrap();
    let mut b = Builder::from_doc(doc);
    let synth = b.strip_of(StripKind::BuiltinSynth).unwrap();
    let delay = b.insert(synth, au("aufx/dely/appl")).unwrap();
    let mut hist = History::new(b.finish());
    hist.execute(
        &Command::Mixer(MixerCmd::SetPluginParams {
            slot: delay,
            params: vec![(ParamId(1), 0.25)],
        }),
        &ViewFilter::default(),
    )
    .unwrap();
    {
        let mut h = host();
        h.sync(hist.doc());
        h.wait_ready(Duration::from_secs(10));
        h.capture(&mut hist, delay).unwrap();
    }
    assert!(hist.doc().plugin_slot(delay).unwrap().params.is_empty());
    let mut fresh = host();
    fresh.sync(hist.doc());
    let r = fresh.wait_ready(Duration::from_secs(10));
    assert!(r.rejected_state.is_empty(), "{:?}", r.rejected_state);
    let v = fresh
        .instance(delay)
        .unwrap()
        .param_value(ParamId(1))
        .unwrap();
    assert!((v - 0.25).abs() < 1e-6, "{v}");
    assert_eq!(
        fresh.instance(delay).unwrap().info().plugin.format,
        PluginFormat::Au
    );
}
VIZDAW_EOF
```

Update `crates/vizdaw-scan/tests/scan.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/vizdaw-scan/tests/scan.rs b/crates/vizdaw-scan/tests/scan.rs
--- a/crates/vizdaw-scan/tests/scan.rs
+++ b/crates/vizdaw-scan/tests/scan.rs
@@ -134,3 +134,31 @@ fn copy_dir(from: &Path, to: &Path) {
         copy_dir(&e.path(), &to.join(e.file_name()));
     }
 }
+
+#[cfg(target_os = "macos")]
+#[test]
+fn audio_units_are_listed_and_scanned() {
+    use core_host::scan::scan_source;
+    let src = Source {
+        format: PluginFormat::Au,
+        path: "au:aufx/dely/appl".into(),
+        version: String::new(),
+        mtime: 0,
+    };
+    let entries = scan_source(exe(), &src, Duration::from_secs(10));
+    assert_eq!(entries.len(), 1);
+    assert_eq!(entries[0].plugin.name, "AUDelay");
+    assert_eq!(entries[0].status, core_host::catalog::ScanStatus::Ok);
+    let info = entries[0].info.as_ref().unwrap();
+    assert!(info.params.iter().any(|p| p.name == "Delay Time"));
+    let missing = run(
+        exe(),
+        &ScanRequest {
+            format: PluginFormat::Au,
+            path: "au:aufx/none/appl".into(),
+            id: None,
+            timeout: Duration::from_secs(10),
+        },
+    );
+    assert!(missing.reason().unwrap().contains("not found"));
+}
VIZDAW_EOF
```

- [ ] **Step 2: Run them to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p platform-macos --test au_host
cargo test -p vizdaw-scan
```

Expected: `au_host`'s 3 tests pass (they need nothing new — this is the integration check); `vizdaw-scan`'s `audio_units_are_listed_and_scanned` fails: the listing is blocklisted with `failed: AU scanning arrives with the AU host`.

- [ ] **Step 3: Implement**


Change `crates/vizdaw-scan/Cargo.toml`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/vizdaw-scan/Cargo.toml b/crates/vizdaw-scan/Cargo.toml
--- a/crates/vizdaw-scan/Cargo.toml
+++ b/crates/vizdaw-scan/Cargo.toml
@@ -11,5 +11,8 @@ core-engine = { path = "../core-engine" }
 core-model = { path = "../core-model" }
 serde_json = "1"
 
+[target.'cfg(target_os = "macos")'.dependencies]
+platform-macos = { path = "../platform-macos" }
+
 [dev-dependencies]
 test-plugins = { path = "../test-plugins" }
VIZDAW_EOF
```

Change `crates/vizdaw-scan/src/main.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/vizdaw-scan/src/main.rs b/crates/vizdaw-scan/src/main.rs
--- a/crates/vizdaw-scan/src/main.rs
+++ b/crates/vizdaw-scan/src/main.rs
@@ -3,6 +3,8 @@
 //! ```text
 //! vizdaw-scan --format clap --path <bundle> --list
 //! vizdaw-scan --format clap --path <bundle> --id <plugin id>
+//! vizdaw-scan --format au --path au:<type/subtype/manufacturer> --list
+//! vizdaw-scan --format au --id <type/subtype/manufacturer>
 //! ```
 //!
 //! `--list` prints the plugins of the bundle; `--id` loads the plugin,
@@ -127,9 +129,33 @@ fn scan(args: &Args) -> Result<ScanReply, String> {
 #[cfg(target_os = "macos")]
 mod au {
     use super::*;
+    use platform_macos::au;
+
+    /// AU sources are single components: `--list` reads the registry only;
+    /// `--id` loads the unit. A v3 unit that cannot be loaded outside the
+    /// app is "not validated" (spec §8).
     pub fn scan(args: &Args) -> Result<ScanReply, String> {
-        let _ = args;
-        Err("AU scanning arrives with the AU host".into())
+        let id = match &args.id {
+            Some(id) => id.clone(),
+            None => args
+                .path
+                .to_string_lossy()
+                .strip_prefix("au:")
+                .map(String::from)
+                .ok_or("an AU path is au:<type/subtype/manufacturer>")?,
+        };
+        let c = au::component(&id).ok_or_else(|| format!("plugin not found: {id}"))?;
+        if args.id.is_none() {
+            return Ok(ScanReply::Listed(vec![(c.plugin, c.kind)]));
+        }
+        match au::load(&id, RATE, BLOCK) {
+            Ok(mut inst) => {
+                exercise(&mut inst)?;
+                Ok(ScanReply::Scanned(inst.info().clone()))
+            }
+            Err(e) if c.v3 => Ok(ScanReply::NotValidated(e.to_string())),
+            Err(e) => Err(e.to_string()),
+        }
     }
 }
 
VIZDAW_EOF
```

- [ ] **Step 4: Run the tests to see them pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p platform-macos --test au_host
cargo test -p vizdaw-scan
```

Expected: all pass: `au_host` 3 tests, `vizdaw-scan` 4 tests.

- [ ] **Step 5: Check the whole workspace**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Expected: no formatting changes left, no clippy warnings, every test passes.

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/platform-macos/tests/au_host.rs \
  crates/vizdaw-scan/Cargo.toml \
  crates/vizdaw-scan/src/main.rs \
  crates/vizdaw-scan/tests/scan.rs \
  Cargo.lock
git commit -m "$(cat <<'MSG'
feat: Audio Units through the plugin host and the scanner

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
MSG
)"
```


---

### Task 14: vizdaw-play plays real plugins; scan from the command line

**Files:**
- Create: `crates/vizdaw-play/src/plugins.rs`
- Modify: `crates/test-plugins/src/bundle.rs`
- Modify: `crates/test-plugins/src/lib.rs`
- Modify: `crates/vizdaw-play/Cargo.toml`
- Modify: `crates/vizdaw-play/README.md`
- Modify: `crates/vizdaw-play/src/args.rs`
- Modify: `crates/vizdaw-play/src/live.rs`
- Modify: `crates/vizdaw-play/src/main.rs`
- Modify: `crates/vizdaw-play/tests/cli.rs`
- Lockfile: `Cargo.lock` (cargo updates it when dependencies change)

**Interfaces:**
- Consumes: Tasks 8, 10, 11, 12: `PluginHost::{new, add_format, sync, wait_ready, factory, service}`, `Catalog::{load, save, mark_found, apply, rows}`, `scan::scan_all`, `au::sources`, `clap::{search_paths, find_bundles}`.
- Produces: `vizdaw-play --scan [--catalog FILE] [--clap-dir DIR]… [--no-au] [--scan-timeout-ms MS] [--scanner FILE]`, `--plugins`, `--insert STRIP=FORMAT:ID`, `--instrument DEST=FORMAT:ID`; live playback loads the project's plugins before the first graph and calls `PluginHost::service` every 100 ms; `test_plugins::scanner() -> PathBuf`; README plugin checklist.

Ruling 21. The live session owns a `PluginHost` (with the AU format and the catalog applied), made at the device's rate for the engine's block size, and drops it after the engine. The README gains the human checklist with the user's installed plugins (Diva, an AU and a CLAP insert, a sidechain compressor, a multi-output drum instrument, a crashing bundle); nothing in it is automated. `test_plugins::scanner()` builds `vizdaw-scan` into `target/test-plugins` for the CLI test.

- [ ] **Step 1: Write the failing tests**


Update `crates/vizdaw-play/tests/cli.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/vizdaw-play/tests/cli.rs b/crates/vizdaw-play/tests/cli.rs
--- a/crates/vizdaw-play/tests/cli.rs
+++ b/crates/vizdaw-play/tests/cli.rs
@@ -190,3 +190,95 @@ fn too_many_workers_are_refused() {
         .unwrap();
     assert_eq!(out.status.code(), Some(2));
 }
+
+#[test]
+fn scan_fills_the_catalog_and_plugins_lists_it() {
+    let dir = temp("scan");
+    let plugins = dir.join("clap");
+    std::fs::create_dir_all(&plugins).unwrap();
+    let bundle = test_plugins::bundle();
+    copy_dir(&bundle, &plugins.join("VizDAW Test.clap"));
+    let catalog = dir.join("plugins.json");
+    let out = bin()
+        .args([
+            "--scan",
+            "--no-au",
+            "--scan-timeout-ms",
+            "2000",
+            "--catalog",
+        ])
+        .arg(&catalog)
+        .arg("--clap-dir")
+        .arg(&plugins)
+        .arg("--scanner")
+        .arg(test_plugins::scanner())
+        .output()
+        .unwrap();
+    assert!(
+        out.status.success(),
+        "{}",
+        String::from_utf8_lossy(&out.stderr)
+    );
+    let text = String::from_utf8_lossy(&out.stdout);
+    for line in [
+        "clap:vizdaw.test.gain  Test Gain (VizDAW)  OK",
+        "clap:vizdaw.test.crasher  Test Crasher (VizDAW)  Blocklisted: crashed (signal 11)",
+        "clap:vizdaw.test.hanger  Test Hanger (VizDAW)  Blocklisted: timed out",
+    ] {
+        assert!(
+            text.lines().any(|l| l == line),
+            "missing {line:?} in\n{text}"
+        );
+    }
+    // The catalog persists; listing does not scan again.
+    let out = bin()
+        .arg("--plugins")
+        .arg("--catalog")
+        .arg(&catalog)
+        .output()
+        .unwrap();
+    assert!(out.status.success());
+    assert_eq!(String::from_utf8_lossy(&out.stdout), text);
+}
+
+#[test]
+fn bad_plugin_arguments_are_errors() {
+    let dir = temp("badplugin");
+    let catalog = dir.join("plugins.json");
+    for (arg, want) in [
+        (
+            "--insert=Nowhere=au:aufx/dely/appl",
+            "no strip named \"Nowhere\"",
+        ),
+        ("--insert=Master=vst3:x", "expected au:<id> or clap:<id>"),
+        (
+            "--instrument=Nobody=clap:x",
+            "no destination named \"Nobody\"",
+        ),
+    ] {
+        let out = bin()
+            .arg(repo().join("fixtures/demo.vizdaw"))
+            .arg(arg)
+            .arg("--catalog")
+            .arg(&catalog)
+            .args(["--offline"])
+            .arg(dir.join("x.txt"))
+            .output()
+            .unwrap();
+        assert_eq!(out.status.code(), Some(1), "{arg}");
+        let err = String::from_utf8_lossy(&out.stderr);
+        assert!(err.contains(want), "{arg}: {err}");
+    }
+}
+
+fn copy_dir(from: &Path, to: &Path) {
+    if from.is_file() {
+        std::fs::copy(from, to).unwrap();
+        return;
+    }
+    std::fs::create_dir_all(to).unwrap();
+    for e in std::fs::read_dir(from).unwrap() {
+        let e = e.unwrap();
+        copy_dir(&e.path(), &to.join(e.file_name()));
+    }
+}
VIZDAW_EOF
```

- [ ] **Step 2: Run them to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p vizdaw-play --test cli
```

Expected: `scan_fills_the_catalog_and_plugins_lists_it` and `bad_plugin_arguments_are_errors` fail: `error: unexpected argument '--scan' found` (and `'--insert'`).

- [ ] **Step 3: Implement**


Create `crates/vizdaw-play/src/plugins.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p crates/vizdaw-play/src
cat > crates/vizdaw-play/src/plugins.rs <<'VIZDAW_EOF'
//! Plugins for `vizdaw-play`: the catalog (`--scan`, `--plugins`), command
//! line edits that add plugins (`--insert`, `--instrument`) and the plugin
//! host for live playback.

use crate::args::Args;
use core_host::PluginHost;
use core_host::catalog::{Catalog, PluginStatus, Source};
use core_host::scan;
use core_model::command::{Command, MixerCmd, RoutingCmd, apply};
use core_model::document::{DestKind, Document};
use core_model::ids::PluginInstanceId;
use core_model::mixer::{PluginFormat, PluginRef, PluginSlot};
use std::path::PathBuf;
use std::time::Duration;

fn catalog_path(args: &Args) -> PathBuf {
    args.catalog.clone().unwrap_or_else(Catalog::default_path)
}

/// Loads the catalog (a corrupt one starts over, with a warning).
pub fn load_catalog(args: &Args) -> Catalog {
    let path = catalog_path(args);
    Catalog::load(&path).unwrap_or_else(|e| {
        eprintln!("warning: {e}; starting a new catalog");
        Catalog::new()
    })
}

/// The scanner binary: `--scanner`, else next to this one.
fn scanner(args: &Args) -> Result<PathBuf, String> {
    if let Some(p) = &args.scanner {
        return Ok(p.clone());
    }
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    let exe = me
        .parent()
        .map(|d| d.join("vizdaw-scan"))
        .ok_or("cannot find vizdaw-scan")?;
    if exe.exists() {
        Ok(exe)
    } else {
        Err(format!(
            "{} not found (build it with `cargo build -p vizdaw-scan`)",
            exe.display()
        ))
    }
}

fn discover(args: &Args) -> Vec<Source> {
    let dirs = if args.clap_dirs.is_empty() {
        core_host::clap::search_paths()
    } else {
        args.clap_dirs.clone()
    };
    let mut found: Vec<Source> = core_host::clap::find_bundles(&dirs)
        .iter()
        .map(|p| Source::clap_bundle(p))
        .collect();
    #[cfg(target_os = "macos")]
    if !args.no_au {
        found.extend(platform_macos::au::sources());
    }
    found
}

/// `--scan` and `--plugins`.
pub fn run(args: &Args) -> Result<(), String> {
    let mut cat = load_catalog(args);
    if args.scan {
        let exe = scanner(args)?;
        let todo = cat.mark_found(&discover(args));
        eprintln!("scanning {} source(s)…", todo.len());
        let fixed = args.scan_timeout_ms.map(Duration::from_millis);
        scan::scan_all(&mut cat, &exe, &todo, |s: &Source| {
            fixed.unwrap_or(match s.format {
                PluginFormat::Au => scan::TIMEOUT_AU_V3,
                PluginFormat::Clap => scan::TIMEOUT,
            })
        });
        cat.save(&catalog_path(args))?;
    }
    for r in cat.rows() {
        let status = match &r.status {
            PluginStatus::Ok => "OK".to_string(),
            PluginStatus::Disabled => "Disabled".to_string(),
            PluginStatus::Blocklisted(why) => format!("Blocklisted: {why}"),
            PluginStatus::NotValidated => "Not validated".to_string(),
            PluginStatus::Missing => "Missing".to_string(),
        };
        let format = match r.plugin.format {
            PluginFormat::Au => "au",
            PluginFormat::Clap => "clap",
        };
        println!(
            "{format}:{}  {} ({})  {status}",
            r.plugin.id, r.plugin.name, r.plugin.vendor
        );
    }
    Ok(())
}

/// A `FORMAT:ID` plugin, named from the catalog when it knows it.
fn plugin_ref(spec: &str, cat: &Catalog) -> Result<PluginRef, String> {
    let (format, id) = match spec.split_once(':') {
        Some(("au", id)) => (PluginFormat::Au, id),
        Some(("clap", id)) => (PluginFormat::Clap, id),
        _ => return Err(format!("{spec:?}: expected au:<id> or clap:<id>")),
    };
    Ok(cat
        .get(format, id)
        .map(|e| e.plugin.clone())
        .unwrap_or_else(|| PluginRef {
            format,
            id: id.to_string(),
            name: id.to_string(),
            vendor: String::new(),
            version: String::new(),
        }))
}

/// Applies `--insert` and `--instrument` to `doc`.
pub fn edit(doc: &Document, args: &Args, cat: &Catalog) -> Result<Document, String> {
    let mut doc = doc.clone();
    let run = |doc: &Document, cmd: Command| -> Result<Document, String> {
        apply(doc, &cmd).map(|a| a.doc).map_err(|e| e.to_string())
    };
    for spec in &args.instruments {
        let (name, p) = spec
            .split_once('=')
            .ok_or_else(|| format!("{spec:?}: expected DEST=FORMAT:ID"))?;
        let dest = doc
            .instruments()
            .values()
            .find(|d| d.name == name)
            .ok_or_else(|| format!("no destination named {name:?}"))?;
        let kind = DestKind::PluginInstrument {
            slot: PluginSlot::new(PluginInstanceId(0), plugin_ref(p, cat)?),
        };
        let cmd = Command::Routing(RoutingCmd::EditDestination {
            dest: dest.id,
            kind,
            default_patch: dest.default_patch,
        });
        doc = run(&doc, cmd)?;
    }
    for spec in &args.inserts {
        let (name, p) = spec
            .split_once('=')
            .ok_or_else(|| format!("{spec:?}: expected STRIP=FORMAT:ID"))?;
        let strip = doc
            .mixer()
            .strips()
            .values()
            .find(|s| s.name == name)
            .ok_or_else(|| format!("no strip named {name:?}"))?
            .id;
        let cmd = Command::Mixer(MixerCmd::AddInsert {
            strip,
            index: None,
            plugin: plugin_ref(p, cat)?,
        });
        doc = run(&doc, cmd)?;
    }
    Ok(doc)
}

/// A host for `doc`'s plugins, loaded (or failed) before playback starts.
pub fn host(
    doc: &Document,
    cat: &Catalog,
    rate: f64,
    max_frames: u32,
) -> Result<PluginHost, String> {
    let mut h = PluginHost::new(rate, max_frames).map_err(|e| e.to_string())?;
    #[cfg(target_os = "macos")]
    h.add_format(Box::new(platform_macos::au::AuFormat::new()));
    cat.apply(&mut h);
    h.sync(doc);
    let r = h.wait_ready(Duration::from_secs(60));
    for (id, why) in &r.failed {
        eprintln!("warning: plugin {id}: {why}");
    }
    for (id, why) in &r.rejected_state {
        eprintln!("warning: plugin {id}: saved state rejected ({why})");
    }
    Ok(h)
}
VIZDAW_EOF
```

Change `crates/test-plugins/src/bundle.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/test-plugins/src/bundle.rs b/crates/test-plugins/src/bundle.rs
--- a/crates/test-plugins/src/bundle.rs
+++ b/crates/test-plugins/src/bundle.rs
@@ -74,6 +74,31 @@ pub fn bundle() -> PathBuf {
     }
 }
 
+/// The `vizdaw-scan` binary, built once per test process into the same
+/// separate target directory (for tests of programs that run it).
+pub fn scanner() -> PathBuf {
+    static PATH: OnceLock<Result<PathBuf, String>> = OnceLock::new();
+    let r = PATH.get_or_init(|| {
+        let root = workspace_root();
+        let target = root.join("target").join("test-plugins");
+        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
+        let status = Command::new(cargo)
+            .current_dir(&root)
+            .args(["build", "--quiet", "-p", "vizdaw-scan", "--target-dir"])
+            .arg(&target)
+            .status()
+            .map_err(|e| format!("cannot run cargo: {e}"))?;
+        if !status.success() {
+            return Err(format!("building vizdaw-scan failed: {status}"));
+        }
+        Ok(target.join("debug").join("vizdaw-scan"))
+    });
+    match r {
+        Ok(p) => p.clone(),
+        Err(e) => panic!("{e}"),
+    }
+}
+
 const INFO_PLIST: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
 <!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
 <plist version="1.0">
VIZDAW_EOF
```

Change `crates/test-plugins/src/lib.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/test-plugins/src/lib.rs b/crates/test-plugins/src/lib.rs
--- a/crates/test-plugins/src/lib.rs
+++ b/crates/test-plugins/src/lib.rs
@@ -15,7 +15,7 @@
 mod bundle;
 mod plugin;
 
-pub use bundle::bundle;
+pub use bundle::{bundle, scanner};
 
 use clack_plugin::entry::prelude::*;
 use clack_plugin::prelude::*;
VIZDAW_EOF
```

Change `crates/vizdaw-play/Cargo.toml`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/vizdaw-play/Cargo.toml b/crates/vizdaw-play/Cargo.toml
--- a/crates/vizdaw-play/Cargo.toml
+++ b/crates/vizdaw-play/Cargo.toml
@@ -12,7 +12,11 @@ rt-check = ["core-engine/rt-check"]
 [dependencies]
 core-model = { path = "../core-model" }
 core-engine = { path = "../core-engine", features = ["fixtures"] }
+core-host = { path = "../core-host" }
 clap = { version = "4", features = ["derive"] }
 
 [target.'cfg(target_os = "macos")'.dependencies]
 platform-macos = { path = "../platform-macos" }
+
+[dev-dependencies]
+test-plugins = { path = "../test-plugins" }
VIZDAW_EOF
```

Change `crates/vizdaw-play/README.md`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/vizdaw-play/README.md b/crates/vizdaw-play/README.md
--- a/crates/vizdaw-play/README.md
+++ b/crates/vizdaw-play/README.md
@@ -7,6 +7,9 @@ Plays a sequence of a VizDAW project through Apple's built-in DLS synth
 vizdaw-play <project.vizdaw> [--seq NAME] [--loop] [--device NAME] [--buffer N] [--workers N]
             [--to builtin|virtual|both] [--midi-offset-ms N] [--offline OUT.txt] [--list]
 vizdaw-play --write-demo <dir/demo.vizdaw>
+vizdaw-play --scan [--catalog FILE] [--clap-dir DIR]… [--no-au] [--scan-timeout-ms MS] [--scanner FILE]
+vizdaw-play --plugins [--catalog FILE]
+vizdaw-play <project.vizdaw> [--insert "STRIP=au:ID|clap:ID"]… [--instrument "DEST=au:ID|clap:ID"]…
 ```
 
 Exit codes: 0 played to the end (or Ctrl-C), 1 error, 2 bad arguments or
@@ -24,6 +27,15 @@ performance cores − 1, at most 7; 0 for `--offline`). Workers run with
 real-time priority in the output device's workgroup. The status line shows
 the DSP load (render time ÷ block time, peak over 0.1 s).
 
+Plugins: `--scan` finds CLAP bundles (`~/Library/Audio/Plug-Ins/CLAP`,
+`/Library/Audio/Plug-Ins/CLAP`) and Audio Units, checks each new or changed
+one in its own `vizdaw-scan` process (a crash or a hang blocklists it) and
+writes the catalog (`~/Library/Application Support/VizDAW/plugins.json`).
+`--insert` adds a plugin to a strip by name and `--instrument` plays a
+destination on a plugin instrument, for live playback; the project's own
+plugin slots load too. AU ids are `type/subtype/manufacturer` four-char
+codes (`--plugins` lists them).
+
 ## Manual smoke test
 
 Run from the repository root with the toolchain environment exported
@@ -52,3 +64,30 @@ Run from the repository root with the toolchain environment exported
 8. `cargo run -p vizdaw-play --release --features rt-check -- fixtures/demo.vizdaw --workers 4`
    — the last line reads `rt-check: 0 allocation(s) on the render path`
    (workers arm themselves under `rt-check`).
+
+## Manual plugin checklist (3b)
+
+Uses plugins installed on this Mac; nothing here is automated. Build first:
+`cargo build -p vizdaw-scan -p vizdaw-play --release`.
+
+9. `cargo run -p vizdaw-play --release -- --scan` — lists every installed
+   CLAP and AU with its status; a second `--scan` scans nothing new
+   ("scanning 0 source(s)").
+10. Pick an AU instrument from the list (e.g. Diva:
+    `--instrument "Piano=au:aumu/…/…"` with the id `--plugins` prints) and play
+    the demo's Song — the instrument plays the piano part; the status line
+    load stays sane; Ctrl-C leaves no hanging notes.
+11. Add an AU and a CLAP effect to the instrument's strip
+    (`--insert "Piano=au:aufx/dely/appl" --insert "Piano=clap:<id>"`) — both
+    audible; a plugin with latency (a lookahead limiter) keeps the drums in
+    time with the piano.
+12. Sidechain: with a third-party compressor that has a sidechain input,
+    set one up in a project (3c's mixer, or a hand-edited project.json:
+    `"sidechain": <strip id>` on the insert) — the kick ducks the pad.
+13. A multi-output drum instrument with `AddInstrumentOutput` strips — each
+    output plays on its strip.
+14. Copy a known-crashing or hanging test plugin into
+    `~/Library/Audio/Plug-Ins/CLAP` (the in-repo `target/test-plugins/VizDAW
+    Test.clap`) and `--scan` — the crasher shows `Blocklisted: crashed
+    (signal 11)`, the hanger `Blocklisted: timed out`; VizDAW never crashes.
+    Remove it afterwards.
VIZDAW_EOF
```

Change `crates/vizdaw-play/src/args.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/vizdaw-play/src/args.rs b/crates/vizdaw-play/src/args.rs
--- a/crates/vizdaw-play/src/args.rs
+++ b/crates/vizdaw-play/src/args.rs
@@ -30,7 +30,7 @@ impl From<To> for BuiltinRoute {
 #[command(name = "vizdaw-play", version)]
 pub struct Args {
     /// The `.vizdaw` package to play.
-    #[arg(required_unless_present = "write_demo")]
+    #[arg(required_unless_present_any = ["write_demo", "scan", "plugins"])]
     pub project: Option<PathBuf>,
     /// Sequence to play (default: the only top-level sequence).
     #[arg(long = "seq", value_name = "NAME")]
@@ -65,6 +65,39 @@ pub struct Args {
     /// List sequences, output devices and MIDI destinations, then exit.
     #[arg(long)]
     pub list: bool,
+    /// Add a plugin insert before playing: `STRIP=FORMAT:ID`, e.g.
+    /// `"Built-in Synth=au:aufx/dely/appl"` or `"Master=clap:com.u-he.diva"`
+    /// (repeatable; live playback only).
+    #[arg(long = "insert", value_name = "STRIP=FORMAT:ID")]
+    pub inserts: Vec<String>,
+    /// Play a destination on a plugin instrument instead: `DEST=FORMAT:ID`,
+    /// e.g. `"Piano=au:aumu/dls /appl"` (repeatable; live playback only).
+    #[arg(long = "instrument", value_name = "DEST=FORMAT:ID")]
+    pub instruments: Vec<String>,
+    /// Scan for new or changed plugins (CLAP folders and Audio Units), update
+    /// the catalog and list it, then exit.
+    #[arg(long, conflicts_with_all = ["offline", "list", "write_demo"])]
+    pub scan: bool,
+    /// List the plugin catalog, then exit.
+    #[arg(long, conflicts_with_all = ["offline", "list", "write_demo"])]
+    pub plugins: bool,
+    /// The plugin catalog (default: the app's `plugins.json`).
+    #[arg(long, value_name = "FILE")]
+    pub catalog: Option<PathBuf>,
+    /// Look for CLAP bundles here instead of the standard folders
+    /// (repeatable).
+    #[arg(long = "clap-dir", value_name = "DIR")]
+    pub clap_dirs: Vec<PathBuf>,
+    /// The `vizdaw-scan` binary (default: next to this program).
+    #[arg(long, value_name = "FILE")]
+    pub scanner: Option<PathBuf>,
+    /// Do not scan Audio Units.
+    #[arg(long = "no-au")]
+    pub no_au: bool,
+    /// Time each plugin scan may take, in milliseconds (default: 10 s, 30 s
+    /// for AU v3).
+    #[arg(long = "scan-timeout-ms", value_name = "MS")]
+    pub scan_timeout_ms: Option<u64>,
     /// Write the demo project to this package path, then exit.
     #[arg(long = "write-demo", value_name = "DIR/demo.vizdaw", conflicts_with_all = ["project", "offline", "list"])]
     pub write_demo: Option<PathBuf>,
VIZDAW_EOF
```

Change `crates/vizdaw-play/src/live.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/vizdaw-play/src/live.rs b/crates/vizdaw-play/src/live.rs
--- a/crates/vizdaw-play/src/live.rs
+++ b/crates/vizdaw-play/src/live.rs
@@ -1,14 +1,16 @@
-//! Live playback through CoreAudio, the DLS synth's mixer strip and
-//! CoreMIDI (macOS), rendered in parallel on worker threads joined to the
-//! device's workgroup.
+//! Live playback through CoreAudio, the mixer (with the project's AU and
+//! CLAP plugins) and CoreMIDI (macOS), rendered in parallel on worker
+//! threads joined to the device's workgroup.
 
 use crate::args::Args;
-use crate::signal;
+use crate::{plugins, signal};
 use core_engine::backend::{AudioBackend, AudioConfig, BackendEvent, StreamInfo};
 use core_engine::engine::{Engine, EngineConfig};
 use core_engine::pool::RtEnv;
 use core_engine::render::Renderer;
 use core_engine::transport::PlayState;
+use core_host::PluginHost;
+use core_host::catalog::Catalog;
 use core_model::document::Document;
 use core_model::ids::SeqId;
 use core_model::time::Tick;
@@ -25,6 +27,8 @@ const DEADLINE: f64 = 0.8;
 struct Session {
     backend: CoreAudioBackend,
     engine: Engine,
+    /// Dropped after the engine: processors come back through it.
+    host: PluginHost,
     info: StreamInfo,
 }
 
@@ -52,6 +56,7 @@ fn open(
     args: &Args,
     doc: &Document,
     seq: SeqId,
+    catalog: &Catalog,
     at: Tick,
     playing: bool,
 ) -> Result<Session, String> {
@@ -70,6 +75,9 @@ fn open(
     let env = rt_env(&backend, &info);
     let joined = env.workgroup.is_some();
     engine.set_rt_env(env);
+    // Plugins load (and fail, reported) before the first graph.
+    let host = plugins::host(doc, catalog, info.sample_rate, engine.block_frames())?;
+    engine.set_insert_factory(Box::new(host.factory()));
     let report = engine.load(doc, seq).map_err(|e| e.to_string())?;
     for w in report.warnings.iter().chain(&report.graph.warnings) {
         eprintln!("warning: {w}");
@@ -109,17 +117,20 @@ fn open(
     Ok(Session {
         backend,
         engine,
+        host,
         info,
     })
 }
 
 /// Stops the device, frees the renderer on this thread, then drops the
-/// engine: its MIDI sender drains and sends All Notes Off to every MIDI
-/// output it owned before the next session opens any.
+/// engine (its MIDI sender drains and sends All Notes Off to every MIDI
+/// output it owned before the next session opens any), then the plugin
+/// host, which deactivates the processors the engine gave back.
 fn close(mut s: Session) {
     let r: Option<Renderer> = s.backend.stop().ok();
     drop(r);
     drop(s.engine);
+    drop(s.host);
 }
 
 /// Stops the transport and gives a live device a few blocks to play the
@@ -130,12 +141,21 @@ fn stop_and_wait(s: &mut Session) {
 }
 
 /// Plays until the end (exit 0) or Ctrl-C.
-pub fn run(args: &Args, doc: &Document, seq: SeqId) -> Result<(), String> {
+pub fn run(args: &Args, doc: &Document, seq: SeqId, catalog: &Catalog) -> Result<(), String> {
     signal::install();
     let meter = doc.sequence(seq).map_err(|e| e.to_string())?.meter.clone();
-    let mut s = open(args, doc, seq, Tick::ZERO, true)?;
+    let mut s = open(args, doc, seq, catalog, Tick::ZERO, true)?;
     loop {
         std::thread::sleep(Duration::from_millis(100));
+        // `host_idle`: plugin callbacks, restarts (a latency change
+        // recompiles), processors coming back.
+        let r = s
+            .host
+            .service(&mut s.engine, doc, seq)
+            .map_err(|e| e.to_string())?;
+        if let Some(c) = r.compiled {
+            eprintln!("\nrecompiled: latency {} frame(s)", c.graph.latency_frames);
+        }
         let pos = meter.tick_to_bbu(s.engine.position_tick(), 4);
         let load = s.engine.dsp_load();
         print!(
@@ -197,7 +217,7 @@ pub fn run(args: &Args, doc: &Document, seq: SeqId) -> Result<(), String> {
                     }
                     let old_rate = s.info.sample_rate;
                     close(s);
-                    s = open(args, doc, seq, at, playing)?;
+                    s = open(args, doc, seq, catalog, at, playing)?;
                     if s.info.sample_rate != old_rate {
                         eprintln!(
                             "sample rate {old_rate} -> {}: recompiled",
VIZDAW_EOF
```

Change `crates/vizdaw-play/src/main.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/vizdaw-play/src/main.rs b/crates/vizdaw-play/src/main.rs
--- a/crates/vizdaw-play/src/main.rs
+++ b/crates/vizdaw-play/src/main.rs
@@ -4,6 +4,7 @@ mod args;
 #[cfg(target_os = "macos")]
 mod live;
 mod offline;
+mod plugins;
 mod project;
 #[cfg(target_os = "macos")]
 mod signal;
@@ -35,10 +36,16 @@ fn run(args: &Args) -> Result<ExitCode, String> {
         println!("wrote {}", path.display());
         return Ok(ExitCode::SUCCESS);
     }
+    if args.scan || args.plugins {
+        plugins::run(args)?;
+        return Ok(ExitCode::SUCCESS);
+    }
     let Some(path) = &args.project else {
         return Err("no project given".into());
     };
     let doc = project::load(path)?;
+    let catalog = plugins::load_catalog(args);
+    let doc = plugins::edit(&doc, args, &catalog)?;
     if args.list {
         list(&doc);
         return Ok(ExitCode::SUCCESS);
@@ -70,7 +77,7 @@ fn run(args: &Args) -> Result<ExitCode, String> {
         eprintln!("wrote {} ({} event(s))", out.display(), report.events);
         return Ok(ExitCode::SUCCESS);
     }
-    play(args, &doc, seq)?;
+    play(args, &doc, seq, &catalog)?;
     Ok(ExitCode::SUCCESS)
 }
 
@@ -79,8 +86,9 @@ fn play(
     args: &Args,
     doc: &core_model::document::Document,
     seq: core_model::ids::SeqId,
+    catalog: &core_host::catalog::Catalog,
 ) -> Result<(), String> {
-    live::run(args, doc, seq)?;
+    live::run(args, doc, seq, catalog)?;
     #[cfg(feature = "rt-check")]
     eprintln!(
         "rt-check: {} allocation(s) on the render path",
@@ -94,6 +102,7 @@ fn play(
     _: &Args,
     _: &core_model::document::Document,
     _: core_model::ids::SeqId,
+    _: &core_host::catalog::Catalog,
 ) -> Result<(), String> {
     Err("live playback needs macOS; use --offline".into())
 }
VIZDAW_EOF
```

- [ ] **Step 4: Run the tests to see them pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p vizdaw-play --test cli
```

Expected: all 11 `cli` tests pass (the scan test takes about 5 s: the hanger times out after 2 s).

- [ ] **Step 5: Smoke-test by hand (no automated step plays audio)**

Run the README's steps 9–14 with the user's plugins. Expected: as written there; record anything that fails in the ledger for 3c.

- [ ] **Step 6: Check the whole workspace**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Expected: no formatting changes left, no clippy warnings, every test passes.

- [ ] **Step 7: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/vizdaw-play/src/plugins.rs \
  crates/test-plugins/src/bundle.rs \
  crates/test-plugins/src/lib.rs \
  crates/vizdaw-play/Cargo.toml \
  crates/vizdaw-play/README.md \
  crates/vizdaw-play/src/args.rs \
  crates/vizdaw-play/src/live.rs \
  crates/vizdaw-play/src/main.rs \
  crates/vizdaw-play/tests/cli.rs \
  Cargo.lock
git commit -m "$(cat <<'MSG'
feat(vizdaw-play): play AU and CLAP plugins live; --scan and --plugins

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
MSG
)"
```


---

## Self-Review

**Spec coverage (3b slice of §14):**
- §5 `PluginInstance` (split, Ruling 1), `PluginInfo`, `ProcessCtx` (input, sidechain, output groups, MIDI events, parameter changes in and out, transport), `HostError`, `PluginEditor` — Task 4 (and Task 12's two additions); `host_idle()` — `PluginHost::idle`/`service`, Task 8.
- §5 CLAP host with `audio-ports`, `note-ports`, `params`, `state`, `latency`, `gui`, `tail`, `log`, `thread-check`; search paths — Task 6. MIDI to CLAP notes/midi events — Task 6 (Ruling 10).
- §5 AU host: enumeration, v2 off the main thread, v3 asynchronous out of process, render with an input pull, sidechain on bus 1, MIDI, parameters, gestures, state, latency notification — Task 12 (Rulings 6, 8); editors — Ruling 7.
- §5 latency change → recompile — Task 6 (restart request), Task 8 (restart, recompile), Task 2 (`restart_instance`).
- §6 loader thread and instance creation before publishing — Task 8 (Ruling 2); multi-output instruments into `InstrumentOutput` strips — Task 3; plugin instruments as graph nodes — Task 2.
- §4 parameter gestures as `SetPluginParams`, state capture through `History::capture_state`, overlay diffs — Task 8 (Rulings 15, 16).
- §8 scanner, timeouts (10 s, 30 s for AU v3), up to 4 at a time, blocklist reasons, cache by source key, not validated — Tasks 10, 11, 13 (Rulings 17, 18).
- §9 data API for the Plugin Manager (rows, statuses, enable/disable, clear blocklist, browser filter, placeholders "Disabled in Plugin Manager") — Task 10; the windows are 3c.
- §12 `HostError` — Task 4. §13 core-host tests against the test plugins (gain with latency, ducker, 4-output synth, NaN, state round trip), latency-change recompile, platform-macos tests against Apple's units (state round trip, gesture capture), scanner crasher/hanger and cache invalidation — Tasks 5–13; the sidechain-on-AUDynamicsProcessor item is replaced per Ruling 8.
- Carry-overs from the 3a final review — Task 1 (ended + reopen, stale output strips, meters, fault board, post-fader send and bus-output tests), Task 2 (block size growth for sinks and inserts, several instruments), Task 3 (`InstrumentOutput` strips), Task 8 (plugins follow the block size).

**Placeholder scan:** every code step is a complete file or an exact `git apply` patch; every run step names its command and expected result.

**Type consistency:** `PluginInstance`/`AudioProcessor`/`ProcessCtx`/`ParamEvent` (Task 4) are what the CLAP host (Task 6), the adapters (Task 7), the host (Task 8), the scanner (Task 11) and the AU host (Task 12) use; `ProcessCtx::input` and `sidechain` become `&mut` in Task 12, which updates the only reader (`clap/processor.rs`) in the same patch; `InstrumentFeed::plugin(target, latency, outputs, dest)` gains `outputs` in Task 3 together with its only caller; `Shelf`/`Ready::into_processor` (Tasks 7–8), `PluginHost::{sync, idle, service, wait_ready, capture, commit_gestures, register_path, set_unavailable, factory}` (Task 8) are what Tasks 9–14 call; `Catalog::{mark_found, record, apply, rows, browser, status}` and `Source::clap_bundle` (Task 10) are what Tasks 11 and 14 call.

**Review Focus:** each of the five lines names tests in their owning tasks; all of them are written out above.

**Rehearsal:** every task was implemented in order on a scratch copy of the repository (outside the repo, nothing committed), and the plan itself was then replayed from its bash blocks on a fresh clone of `sp3-plugins` at `5972332`: after each task `cargo fmt --check`, `cargo clippy --workspace --all-targets -D warnings` and `cargo test --workspace` were clean (473 tests at the end on macOS; the AU tests ran against Apple's units without opening an audio device). Linux was not built (no Linux target installed); `core-host` uses only portable crates (`clack-host` with `libloading`). The rehearsal is why several details look the way they do: the AU input callback lends the host's buffers when the unit passes null ones and the output-port split takes the length before `mem::take` (both found as `-50` render errors); Apple's effects refuse a second input element (Ruling 8); a sidechain source at −∞ dB drives nothing because sidechains tap post-fader (the ducker test routes its key into a −∞ bus instead); `slice::sort_by_key` may allocate, so MIDI events are insertion-sorted; `release_stale_targets` releases before interning so an index is never reused in the same load.
