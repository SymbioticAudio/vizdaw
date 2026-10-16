# VizDAW 3a — Mixer and Parallel Graph Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give VizDAW a full mixer model (strips, inserts, sends, sidechains, plugin slots with parameter overlay and state blobs, project file v2) and a real-time render graph with plugin delay compensation, rendered in parallel on a worker pool joined to the device's audio workgroup — so the demo fixture plays through the built-in synth's strip into the master, byte-identical for any worker count and block size, with zero allocations on every render thread.

**Architecture:** `core-model` gains `Mixer`/`Strip`/`PluginSlot` on `Document`, routing rules (DAG, valid targets, solo set), `MixerCmd` edits, content-addressed plugin state blobs and the v1 → v2 file migration. `core-engine` gains a `graph` module: `mixer_spec` turns the document's mixer into a platform-neutral `GraphSpec`; `plan` computes the topological order and PDC delays; `Graph::build` makes the immutable runtime graph (per-node buffers, per-edge delay lines, insert processors adopted by `PluginInstanceId`); the `Exec` executor runs a block by pushing source nodes onto a bounded `ArrayQueue` and letting the callback thread and `W` pool workers pop, render and complete nodes. `Engine::load` compiles the graph after the schedule and ships it through the existing command ring; retired graphs go to the collector. Inserts in 3a are `EffectNode`s from an `InsertFactory` — `TestInserts` (gain, latency, noise, ducker, NaN, error, stall) stand in for plugins; 3b adapts `PluginInstance` to the same seam. `platform-macos` implements the seams: Mach-semaphore `RtWaker`, `THREAD_TIME_CONSTRAINT_POLICY` promotion, and the HAL device's `os_workgroup`.

**Tech Stack:** Rust stable (1.99.1 installed; workspace `rust-version = "1.85"`), edition 2024, Cargo workspace (resolver 3, `members = ["crates/*"]`); existing `im 15`, `serde 1`, `serde_json 1`, `thiserror 2`, `rtrb 0.4`, objc2 `0.3.2` family, `clap 4`; new `crossbeam-queue 0.3` (core-engine); dev `proptest 1`, `insta 1`.

**Spec:** `docs/superpowers/specs/2026-10-16-plugin-hosting-design.md` — this plan implements only slice **3a** of its §14 (§4 model and file v2 including `PluginSlot`/`PluginRef`, parameter overlay and state-blob storage as data; §6 graph compiler with PDC; §7 parallel renderer, worker pool, workgroup, meters, DSP load, faults; §12 the 3a errors and faults; §13 the core-model and core-engine tests). The engine spec `docs/superpowers/specs/2026-10-15-engine-design.md` (its Rulings stay binding: master hard clamp, one DLS instance for all `builtin:synth` destinations, faults are bits) and the parent `docs/superpowers/specs/2026-10-15-vizdaw-design.md` apply. Executors read the 3a slice of the spec, the engine spec's Rulings and this plan's Plan Rulings.

**Out of scope (3b/3c):** `PluginInstance`, CLAP and AU hosting, the scanner, catalog and blocklist, plugin editors, capture from real plugins, all Swift/FFI app work. `core-ffi` and the Swift app must keep compiling and their tests must keep passing; no FFI API changes are planned.

## Global Constraints

- Every shell command runs after `export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools` (the Xcode license is not accepted, so only the Command Line Tools work). Every bash block below starts with that line and `cd /Users/lattawb/workspace/vizdaw`.
- Work on branch `sp3-plugins` (already checked out). Commit at the end of every task, adding only the files the task lists; never push. End every commit message with the line `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- Rust stable, edition 2024; every crate manifest keeps `edition.workspace = true`, `license.workspace = true`, `rust-version.workspace = true`. License GPL-3.0-only.
- `core-model` and `core-engine` have no OS dependencies and build and test on Linux. macOS-only code lives in `platform-macos` (`#![cfg(target_os = "macos")]`, dependencies under `[target.'cfg(target_os = "macos")'.dependencies]`).
- `core-model`, `core-engine` and `platform-macos` keep `#![deny(missing_docs)]` and `#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic))]`. Library code therefore uses `get`/`get_mut`, never `[i]`. Tests may `unwrap` and index.
- `unsafe` lives only in: `core-engine/src/graph/exec.rs` (the executor's cells and job pointers), `core-engine/src/pool.rs` (dereferencing a published job), the existing `core-engine/src/rt_check.rs`, and `platform-macos/src/{hal,dls,midi,rt}.rs`. Every `unsafe` block and `unsafe impl` has a `// SAFETY:` comment.
- Everything reachable from `Renderer::process` and from a worker's job loop is marked `// RT` and must not allocate, free, lock, do I/O, log, or make syscalls other than CoreAudio render calls, `Instant::now`, and the documented `RtWaker` wake/wait (Mach semaphores on macOS; `park`/`unpark` in the portable fallback). Workers may lock the environment mutex only between jobs.
- No allocation on the callback thread or on worker threads while rendering, checked by `tests/rt_alloc.rs` (unchanged) and the new `tests/rt_alloc_graph.rs`.
- Determinism: for a given document and graph, the master output is bit-identical for worker counts 0–8 and any block-size sequence. Nodes sum inputs in a fixed edge order; nothing depends on which thread rendered what.
- Limits (spec §6): at most 1024 graph nodes and 4096 inserts; at most 2 s (`2 × sample_rate` frames) of delay on any edge. Beyond them the command still commits, the engine raises `GRAPH_TOO_LARGE` / `LATENCY_TOO_LARGE` and keeps the previous graph.
- Faults (spec §7): NaN/Inf or |x| > 64 in an insert's output zeroes that block and counts; 16 consecutive faulty blocks (or error statuses) bypass the instance and raise `PLUGIN_FAULT`.
- Fader range −∞ … +12 dB, stored as `f64` dB with −144 dB meaning −∞; pan −1 … +1, constant power (−3 dB at centre).
- Crate versions: only `crossbeam-queue = "0.3"` is new. No other new runtime crates.
- Each task ends green: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`. Run `cargo fmt --all` before checking.
- Automated steps never play audio through a device; listening is the human smoke test in Task 14.

## Review Focus

The five failure modes a user is most likely to hit that the spec implies but no single requirement spells out, each pinned by a named test in its owning task:

1. **Mixer edits while playing** (fader, pan, mute, solo, send level, bypass) — the edit is heard on the next block with no dropout, compensated paths keep their delay memory and running inserts keep their instances; no allocation on the render threads. Pinned: Task 9 `same_shape_adoption_keeps_delay_state`; Task 11 `parameter_edits_reuse_instances_and_state`; Task 12 `graph_render_path_never_allocates` (swaps every 300 blocks).
2. **Routing edits that would close a loop or leave dangling routes** — a send, output or sidechain that closes a cycle (including sidechains from master or from the strip itself) is refused with nothing changed; removing a bus or an instrument retargets outputs to master, drops sends and clears sidechains inside one undo step. Pinned: Task 4 `cycle_check_matches_reference`; Task 5 `output_cycle_is_refused_and_changes_nothing`, `sidechain_cycles_are_refused`, `removing_a_bus_retargets_and_undo_restores`, `mixer_commands_undo_to_the_original`.
3. **Opening older or incomplete projects** — v1 projects (with `plugin_ref` strings) migrate to slots and a mixer; a project whose plugin cannot be created loads with placeholder inserts (audio passes, reported); a missing state blob loads and is listed, not fatal. Pinned: Task 2 `v1_plugin_instruments_migrate_to_slots_and_strips`; Task 6 `missing_blob_loads_and_is_reported`; Task 11 `placeholders_pass_audio_and_are_reported`.
4. **A misbehaving insert** (NaN, Inf, huge values, error status) — its block is silenced, the strip keeps playing, after 16 bad blocks the instance is bypassed (keeping its latency) and reported once by instance ID. Pinned: Task 9 `faulty_insert_is_zeroed_then_bypassed`, `error_status_counts_as_fault`; Task 11 `plugin_fault_is_reported_by_instance`.
5. **Overload and device changes with workers running** — a block that misses its deadline is silent and counted, the next block waits for stragglers without touching their nodes, stopping or resizing the renderer waits for them, and workers rejoin a new workgroup on an environment change. Pinned: Task 9 `missed_deadline_is_silent_and_recovers`, `env_changes_rejoin_the_workgroup`, `dropping_a_runner_waits_for_stragglers`; Task 11 `prepare_resizes_graph_buffers`.

## Plan Rulings

Decisions the spec leaves open or gets wrong for 3a, each implemented and tested as stated (format: Ruling — why — cost if wrong):

1. **Ruling:** The built-in synth gets its own strip kind, `StripKind::BuiltinSynth`: an instrument strip whose instrument is built in (`is_instrument()` is true). A document has at most one; it is created in the same command as the first destination whose `endpoint_ref` is `builtin:synth` (and by migration/derivation), all such destinations on every channel share it, and it is never deleted automatically (it keeps its inserts and fader when the last user goes). The graph feeds it from the `BuiltinSynth` target's sink — why: spec §4's `Instrument(InstrumentId)` names a plugin instrument destination, while the engine ruling has one DLS instance for every `builtin:synth` destination, so one strip per destination would need one DLS per destination — cost if wrong: per-destination built-in strips need per-destination DLS instances in `TargetTable` (engine ruling change), a new strip kind payload and a v2 → v3 migration; about a day.
2. **Ruling:** The spec's `InstrumentId` is the existing `DestId` (`StripKind::Instrument(DestId)`, `InstrumentOutput(DestId, u16)`) — why: plugin instruments are instrument-list destinations in 2a — cost if wrong: none; a newtype alias.
3. **Ruling:** The spec's `Send` struct is named `AuxSend` — why: `Send` shadows `std::marker::Send` in every module that imports it — cost if wrong: a rename.
4. **Ruling:** Fader and send levels are `f64` dB in `[-144, +12]`; −144 dB (and below, only in memory) means −∞/silence. The master strip's pan is not applied (fader only); every other strip uses the constant-power law (`θ = (pan + 1)·π/4`, gains `cos θ`, `sin θ`, −3 dB at centre), as balance on stereo signals — why: JSON cannot hold −∞ and −144 dB matches the region-gain floor; applying a centred −3 dB pan on master too would stack −6 dB on every path — cost if wrong: master pan is one line in `graph/mixer.rs`; a different floor is a validation constant.
5. **Ruling:** Solo makes audible the soloed strips plus every strip downstream **and upstream** of them through outputs and sends (sidechain edges do not count) — why: spec §4 lists only downstream, which would make a soloed bus silent because its sources are solo-muted — cost if wrong: drop the upstream walk in `mixer_rules::audible` and its reference test.
6. **Ruling:** A document without a mixer (a migrated v1 file, or `Document::from_parts` with `mixer: None`) gets a derived one on load: master, one `Instrument` strip per plugin instrument, the built-in synth strip if any destination uses it, one `AudioPath` strip per output path, all to master. Serialized documents always carry their mixer; the v1 → v2 JSON migration converts slots and counters and leaves `mixer` absent so derivation is the only place that builds one — why: one implementation for migration and for programmatic construction, and 13 test fixtures keep working with `mixer: None` — cost if wrong: a hand-edited v2 file without `mixer` loads instead of failing; add a required-field check in `slow_path`.
7. **Ruling:** Migration turns each `plugin_ref` string into `PluginRef::from_legacy`: format `Au` when the string starts with `au:` else `Clap`, `id` and `name` are the whole string, vendor and version empty; instance IDs are allocated 0, 1, … in destination order — why: spec §4 asks for "a placeholder named after the old string" and 2a's fixtures use `au:`/`clap:` prefixes — cost if wrong: 3b's loader may re-resolve legacy ids; local to `PluginRef::from_legacy`.
8. **Ruling:** `StateBlobId` is the 64-bit FNV-1a hash of the blob's bytes; files are `PluginState/<16 hex digits>.bin`. Blob bytes live in memory in `Document::states` (`Arc<[u8]>`, not serialized); saving writes missing referenced blobs before `project.json` and then deletes `.bin` files referenced neither by the document nor by any autosave; autosave writes blobs but never deletes; a referenced blob missing on disk loads as missing (`Document::missing_states`) — why: a counter-allocated ID can be re-issued after undo while another history version still uses it (two different blobs, one file name); content addressing makes blobs immutable and deduplicated, and keeping autosave references keeps "Restore autosave" whole — cost if wrong: a 64-bit collision silently shares a file (probability ~n²/2⁶⁵); switch to a 128-bit hash in `state.rs`.
9. **Ruling:** Capture is `History::capture_state(slot, bytes)` (and the pure `state::capture`): it stores the blob, sets `slot.state`, clears `slot.params`, and changes the present version without an undo step; it is refused inside an open transaction — why: spec §4 says capture is non-undoable and creates no history entry, which an ordinary `Command` cannot express — cost if wrong: none; 3b calls it.
10. **Ruling:** `EditDestination` from one plugin instrument to the same plugin (`format` + `id`) keeps the instance, its state and overlay (only name/vendor/version metadata update); any other plugin gets a new instance ID. Renaming a destination renames its `Instrument` strip; renaming an output path renames its `AudioPath` strip — why: spec §4 matches plugins on format + id; strips are created with the destination's name — cost if wrong: two lines in `command/routing.rs`.
11. **Ruling:** An instrument's own slot cannot have a sidechain and cannot be removed with `RemoveInsert` (it goes with its destination) — why: sidechains feed inserts (spec §6) — cost if wrong: a validation rule.
12. **Ruling:** The 3a insert seam is `graph::EffectNode` (`latency`, `process(frames, io, sidechain) -> ProcessStatus`, `reset`) created by an `InsertFactory` on the control thread. `NoInserts` (the default) makes every slot a placeholder: audio passes through with zero latency and the slot is listed in `CompileReport.graph.placeholders`. `TestInserts` builds test nodes from CLAP ids `vizdaw.test:<key>[=<value>],…` — why: spec §14 puts "test nodes with configurable latency standing in for plugins" in 3a; 3b wraps `PluginInstance` in an `EffectNode` — cost if wrong: 3b changes one trait.
13. **Ruling:** Graph diagnostics (placeholders, strips without an instrument, a missing built-in strip, a graph error) go to `CompileReport.graph: GraphReport`, not to `CompileReport.warnings` — why: 2b's `warnings` is "one line per kind of problem" and existing tests assert it exactly — cost if wrong: callers that print only `warnings` miss them; `vizdaw-play` prints both.
14. **Ruling:** New insert instances are created on the control thread inside `Engine::load` (no loader thread in 3a) — why: test nodes are instant and 3a has no plugins; spec §6's loader thread matters only for real plugins — cost if wrong: 3b moves `InsertFactory::make` calls to a loader thread before publishing, with no renderer change.
15. **Ruling:** A recompiled graph whose structure hash (`GraphSpec::shape`: nodes, edges, taps, insert instances, latencies, sidechains — not gains, mutes or bypass) equals the running graph's takes over the running graph's node state (buffers and edge delay lines) wholesale; inserts always move by `PluginInstanceId`, but only to a slot that still names the same plugin (format + id), so a different project reusing an ID gets a new instance. Structural edits start new edge delay lines empty — why: parameter edits happen constantly and must not glitch; structural edits are rare — cost if wrong: up to the compensation delay of silence on compensated paths after a structural edit; per-edge adoption is local to `Graph::adopt_from`.
16. **Ruling:** No gain smoothing in 3a: fader, pan, mute and send changes apply at the next block boundary — why: automation and smoothing belong to sub-project 4 — cost if wrong: zipper noise on fast fader moves; a per-strip ramp in `exec.rs`.
17. **Ruling:** PDC realisation: instruments still receive MIDI at the transport position; the audio path is `L` frames late, so MIDI-out timestamps get `+L` frames and the published position is `pos − L` (saturating) — why: this is spec §6's "MIDI to the graph is sent L samples early relative to the device, and external MIDI output plus the displayed position are delayed by L" with the transport as reference — cost if wrong: none expected; when `L` changes mid-play, external MIDI and the display jump by the difference.
18. **Ruling:** Deadline = block start + 0.8 × block duration (`EngineConfig.deadline = Some(0.8)` live; `None` offline and in `core-ffi`). A block whose previous run completed but whose last worker is still leaving the job spins until it has left (bounded by the deadline; unbounded offline, which keeps offline renders deterministic). On a miss the callback outputs silence, raises `XRUN` and counts `deadline_misses`; a later block that finds stragglers still running outputs silence, counts `busy_blocks`, does not advance the transport and remembers the frames, which every instrument sink `skip`s before the next real block — why: spec §7 leaves the margin and recovery open; never touching running nodes is the only safe choice — cost if wrong: tune the fraction; the transport rule matches 2b's oversized-block xrun.
19. **Ruling:** `Exec` has a diagnostic switch `callback_helps` (default on; `Engine::set_callback_helps`, `Runner::set_callback_helps`) — why: tests must force a stalled node onto a worker to exercise the deadline path deterministically — cost if wrong: one public method.
20. **Ruling:** `EngineConfig::new` keeps `workers = 0` and `deadline = None`; `vizdaw-play` uses `min(performance cores − 1, 7)` (`--workers N` overrides, 0–15) and deadline 0.8; `core-ffi` stays at 0 workers until 3c — why: tests and the FFI stay deterministic and unchanged; the spec's default applies where a device plays — cost if wrong: one line in each caller.
21. **Ruling:** Every worker has its own waker (`RtPlatform::waker()` is called once per worker): job wakes go round robin, environment changes and shutdown wake every worker. The portable `StdPlatform` waker is `park`/`unpark` with one token; the macOS waker is one Mach semaphore per worker. Spurious wakeups are allowed — why: `std` has no RT-safe semaphore, and with one shared token pool a fast worker swallowed every wake so the others never applied an environment change (seen while validating this plan) — cost if wrong: a woken busy worker delays nothing, because the callback thread renders whatever no worker took.
22. **Ruling:** Real-time environment changes (`RtEnv { period_ns, workgroup }`) reach workers through an epoch counter and a mutex read between jobs; each worker leaves its old workgroup, re-applies `THREAD_TIME_CONSTRAINT_POLICY` for the new period and joins the new workgroup. A failed join raises `WORKGROUP_JOIN_FAILED`; a failed promotion raises the extra bit `THREAD_POLICY_FAILED` — why: `os_workgroup_join`/`leave` must run on the worker itself — cost if wrong: none; the bits are reported, never fatal.
23. **Ruling:** A fault-bypassed insert stays bypassed for the life of its instance (until removed from the mixer or the engine is rebuilt); its ID is published once in a 4096-slot atomic fault board read by `Engine::plugin_faults()` — why: re-enabling is a 3c UI action — cost if wrong: add a re-enable command that recreates the instance.
24. **Ruling:** Meters are per-strip peak L/R since the last read, stored as `f32` bits in `AtomicU32` (`fetch_max`, valid because non-negative floats order like their bits), measured post-fader and post-mute; DSP load is render time ÷ block duration, last block and peak since the last read — why: spec §7 asks for peaks and load polled at 30 Hz — cost if wrong: none.
25. **Ruling:** Graph evaluation and summing order is fixed: instrument nodes first in target order, then strips in `StripId` order; a strip's inputs are its instrument feed, then for each source strip in `StripId` order its main output and then its sends in send order — why: determinism needs one documented order — cost if wrong: none.
26. **Ruling:** All 3a graph buffers are stereo; mono sources are treated as stereo — why: spec §1 allows mono or stereo groups and 3a has no mono sources — cost if wrong: 3b adds channel counts to `StereoBuf` users.
27. **Ruling:** `OfflineSinks::with_tone()` makes the offline built-in synth produce a deterministic, sample-accurate test tone (per-note sawtooth, events applied at their frame offsets) in addition to logging; `OfflineBackend::output(frames)` exposes the last block — why: graph determinism must be tested with sound flowing from the schedule — cost if wrong: none; test-only behaviour behind a builder flag.
28. **Ruling:** The offline golden `fixtures/demo.offline.txt` does **not** change and the demo digest stays `lines 335 hash 1feb33c1bfd60916`: the log records MIDI events only, the demo's graph latency is 0 (no inserts), the built-in synth's sink is rendered exactly once per block by its graph node (same `rendered` frame accounting), and offline runs have no deadline. `fixtures/demo.vizdaw/project.json` **does** change (version 2, mixer with master and the built-in synth strip, new counters) and is regenerated with `cargo run -p vizdaw-play -- --write-demo fixtures/demo.vizdaw` in Tasks 2 and 3 — why: the file format changed; the event stream did not — cost if wrong: if the digest ever changes, diff the regenerated log against the checked-in one before updating either.
29. **Ruling:** The engine keeps graph cycle defence even though the model forbids cycles: `plan` refuses cycles (`GRAPH_INVALID` fault, previous graph kept) and `Graph::build` drops self-edges — why: the engine must never deadlock or alias on a corrupt document — cost if wrong: none.
30. **Ruling:** `RETIRE_CAPACITY` grows from 8 to 16 — why: every edit now retires a graph as well as a schedule; with 8 slots 2b's `swap_waits_while_the_retire_ring_is_full` saw the fifth edit deferred and `rt_alloc`'s swap loop ran out of command-ring room (both found while validating this plan) — cost if wrong: 64 more bytes of ring; none.

## Spec gaps and contradictions found while planning

- §4 `StripKind::Instrument(InstrumentId)` has no place for the built-in synth (Ruling 1) and the model has no `InstrumentId` (Ruling 2).
- §4 solo: "downstream only" silences a soloed bus (Ruling 5).
- §4 fader "−∞" is not representable in JSON (Ruling 4); "Pan −3 dB at centre" on every strip including master stacks (Ruling 4).
- §4 "a new capture gets a new id" with counters collides across undo/redo branches; "unreferenced blobs are removed on save" would delete blobs autosaves still need (Ruling 8).
- §4 capture is "a non-undoable command that does not create a history entry" — not expressible as a 2a `Command` (Ruling 9).
- §6 says new instances are created on a loader thread; 3a has nothing slow to load (Ruling 14).
- §7 deadline: margin and what the next block does while stragglers run are unspecified (Ruling 18).
- §12 lists `WorkgroupJoinFailed` but not a failed RT promotion (Ruling 22).
- §6/§7 do not say what happens to delay-line memory on recompiles (Ruling 15).

## File Structure

```
crates/core-model/
  src/ids.rs                 + StripId, PluginInstanceId, StateBlobId; AnyId::{Strip, Plugin}; Counters.{strip, plugin}
  src/error.rs               + ModelError::{RoutingCycle, InvalidRoutingTarget}
  src/mixer.rs               NEW Mixer, Strip, StripKind, Output, AuxSend, PluginSlot, PluginRef, PluginFormat, ParamId, gain/pan laws, derive_mixer
  src/mixer_rules.rs         NEW routing edges, cycle check, target checks, value checks, mixer_problems, audible (solo set)
  src/state.rs               NEW blob ids, file names, capture
  src/document.rs            Document.mixer, Document.states, DestKind::PluginInstrument { slot }, slot lookup
  src/rules.rs               validate() also runs mixer_problems
  src/file.rs                VERSION 2, migrate_v1_to_v2, PluginState/ blobs on save/load/autosave
  src/history.rs             History::capture_state
  src/command/mod.rs         Command::Mixer
  src/command/strips.rs      NEW keeps strips in step with destinations and output paths
  src/command/routing.rs     destination/output edits create, rename and remove strips; slot instance IDs
  src/command/mixer.rs       NEW MixerCmd
  tests/fixtures/v1/*.json   the 2a goldens, moved (inputs of the migration test)
  tests/{mixer_types,mixer_document,mixer_routing,mixer_rules,mixer_commands,plugin_state}.rs   NEW
crates/core-engine/
  Cargo.toml                 + crossbeam-queue 0.3
  src/graph/mod.rs           NEW module root, GraphReport
  src/graph/dsp.rs           NEW mix/scale/peak/fault helpers (RT)
  src/graph/delay.rs         NEW DelayLine (RT)
  src/graph/node.rs          NEW EffectNode, ProcessStatus, InsertFactory, NoInserts, TestNode, TestMode, TestInserts
  src/graph/spec.rs          NEW GraphSpec, NodeSpec, StripSpec, InputSpec, InsertSpec, Tap, shape hash
  src/graph/pdc.rs           NEW plan(): order, arrival times, edge delays, limits
  src/graph/exec.rs          NEW Graph, SinkTable, InsertState, Exec, Job, Runner (unsafe, RT)
  src/graph/mixer.rs         NEW mixer_spec(): Document mixer → GraphSpec
  src/pool.rs                NEW RtWaker, RtWorkgroup, RtPlatform, RtEnv, StdPlatform, Pool, PoolShared
  src/render.rs              SinkTable, SetGraph, Retired, graph rendering, PDC offsets, meters, load, new faults
  src/engine.rs              EngineConfig.{workers, deadline}, with_platform, graph compile in load, meters/load/faults API
  src/offline.rs             tone sinks, OfflineBackend::output
  src/schedule/targets.rs    BUILTIN_SYNTH_REF from core-model; slot name in the unroutable message
  src/schedule/compile.rs    CompileReport.graph
  src/rt_check.rs            set_arm_workers / arm_workers
  src/fixtures.rs            Builder mixer helpers
  tests/{graph_dsp,graph_plan,graph_exec,graph_parallel,graph_mixer,mixer_engine,rt_alloc_graph}.rs   NEW
crates/platform-macos/
  src/rt.rs                  NEW MachWaker, promote_current_thread, OsWorkgroup, MacRtPlatform, performance_cores
  src/hal.rs                 CoreAudioBackend::workgroup()
  tests/{rt,mixer}.rs        NEW
crates/vizdaw-play/          --workers, platform + workgroup wiring, load display, README smoke steps, CLI test
fixtures/demo.vizdaw/project.json   regenerated (v2); fixtures/demo.offline.txt unchanged
```

Tests are integration tests (`tests/<name>.rs`) using the public API, except where a task says otherwise.

---
### Task 1: core-model IDs, routing errors and mixer types

**Goal:** Add the mixer's IDs and errors and a standalone `mixer` module with every type of spec §4 (no `Document` wiring, counters or file changes yet).

**Files:**
- Modify: `crates/core-model/src/ids.rs`
- Modify: `crates/core-model/src/error.rs`
- Create: `crates/core-model/src/mixer.rs`
- Modify: `crates/core-model/src/lib.rs`
- Test: `crates/core-model/tests/mixer_types.rs`

**Interfaces:**
- Consumes: `core_model::ids::{DestId, OutputPathId, AnyId}`, `ModelError`.
- Produces:
  - `ids::{StripId, PluginInstanceId, StateBlobId}` (`u64` newtypes, Display `strip3`, `plug4`, `state5`); `AnyId::{Strip(StripId), Plugin(PluginInstanceId)}`.
  - `ModelError::RoutingCycle { path: Vec<StripId> }`, `ModelError::InvalidRoutingTarget { strip: StripId, target: StripId, reason: String }`.
  - `mixer::{BUILTIN_SYNTH_REF = "builtin:synth", GAIN_FLOOR_DB = -144.0, GAIN_MAX_DB = 12.0, MASTER_NAME = "Master", BUILTIN_SYNTH_STRIP = "Built-in Synth"}`.
  - `mixer::ParamId(pub u64)`, `PluginFormat::{Au, Clap}`, `PluginRef { format, id, name, vendor, version }` + `PluginRef::from_legacy(&str)`, `PluginSlot { id, plugin, bypass, sidechain: Option<StripId>, state: Option<StateBlobId>, params: OrdMap<ParamId, f64> }` + `PluginSlot::new(id, plugin)`.
  - `StripKind::{Instrument(DestId), InstrumentOutput(DestId, u16), BuiltinSynth, AudioPath(OutputPathId), Bus, Master}` (`Copy + Ord`) + `is_instrument()`; `Output::{Master, Bus(StripId)}`; `AuxSend { target, level_db, pre_fader, enabled }` + `AuxSend::new(target)`.
  - `Strip { id, name, kind, inserts: Vector<PluginSlot>, gain_db, pan, mute, solo, sends: Vector<AuxSend>, output }` + `Strip::new(id, name, kind)`.
  - `Mixer` (serde via `MixerRepr { master, order, strips }`): `new(master)`, `strips()`, `order()`, `master()`, `strip(id) -> Result<&Strip>`, `find_kind(StripKind) -> Option<StripId>`, `inserts() -> impl Iterator<Item = (StripId, usize, &PluginSlot)>`; (Tasks 2, 3 and 5 add the crate-internal `add`/`strip_mut`, `remove` and `move_to` where they are first used, so no dead code is ever committed).
  - `mixer::{db_to_gain(f64) -> f64, pan_gains(f64) -> (f64, f64)}`.

- [ ] **Step 1: Write the failing test**

Create `crates/core-model/tests/mixer_types.rs`:

```rust
use core_model::ModelError;
use core_model::ids::{AnyId, DestId, PluginInstanceId, StateBlobId, StripId};
use core_model::mixer::{
    AuxSend, Mixer, MixerRepr, Output, ParamId, PluginFormat, PluginRef, PluginSlot, Strip,
    StripKind, db_to_gain, pan_gains,
};
use std::f64::consts::FRAC_1_SQRT_2;

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

#[test]
fn new_ids_display() {
    assert_eq!(AnyId::Strip(StripId(3)).to_string(), "strip3");
    assert_eq!(AnyId::Plugin(PluginInstanceId(4)).to_string(), "plug4");
    assert_eq!(StateBlobId(5).to_string(), "state5");
}

#[test]
fn routing_errors_name_the_strips() {
    let e = ModelError::RoutingCycle {
        path: vec![StripId(1), StripId(2), StripId(1)],
    };
    assert!(e.to_string().contains("StripId(2)"), "{e}");
    let e = ModelError::InvalidRoutingTarget {
        strip: StripId(1),
        target: StripId(0),
        reason: "sends target buses only".into(),
    };
    assert_eq!(
        e.to_string(),
        "strip1 cannot route to strip0: sends target buses only"
    );
}

#[test]
fn fader_and_pan_laws() {
    assert!(close(db_to_gain(0.0), 1.0));
    assert!(close(db_to_gain(-144.0), 0.0));
    assert!(close(db_to_gain(-200.0), 0.0));
    assert!((db_to_gain(-6.020_6) - 0.5).abs() < 1e-4);
    assert!((db_to_gain(12.0) - 3.981_071_705_534_973).abs() < 1e-9);
    let (l, r) = pan_gains(0.0);
    assert!(close(l, FRAC_1_SQRT_2) && close(r, FRAC_1_SQRT_2));
    let (l, r) = pan_gains(-1.0);
    assert!(close(l, 1.0) && r.abs() < 1e-12);
    let (l, r) = pan_gains(1.0);
    assert!(l.abs() < 1e-12 && close(r, 1.0));
    assert_eq!(pan_gains(5.0), pan_gains(1.0));
}

#[test]
fn strip_defaults() {
    let s = Strip::new(StripId(2), "Bus A", StripKind::Bus);
    assert_eq!((s.gain_db, s.pan, s.mute, s.solo), (0.0, 0.0, false, false));
    assert_eq!(s.output, Output::Master);
    assert!(s.inserts.is_empty() && s.sends.is_empty());
    assert!(StripKind::BuiltinSynth.is_instrument());
    assert!(StripKind::Instrument(DestId(0)).is_instrument());
    assert!(!StripKind::InstrumentOutput(DestId(0), 1).is_instrument());
    assert!(!StripKind::Bus.is_instrument());
    let send = AuxSend::new(StripId(4));
    assert_eq!(
        (send.target, send.level_db, send.pre_fader, send.enabled),
        (StripId(4), 0.0, false, true)
    );
}

#[test]
fn legacy_plugin_refs() {
    let r = PluginRef::from_legacy("au:piano");
    assert_eq!(
        (r.format, r.id.as_str(), r.name.as_str(), r.vendor.as_str()),
        (PluginFormat::Au, "au:piano", "au:piano", "")
    );
    assert_eq!(PluginRef::from_legacy("clap:perc").format, PluginFormat::Clap);
    assert_eq!(PluginRef::from_legacy("p").format, PluginFormat::Clap);
}

fn sample_mixer() -> Mixer {
    let mut slot = PluginSlot::new(PluginInstanceId(0), PluginRef::from_legacy("clap:comp"));
    slot.sidechain = Some(StripId(1));
    slot.params.insert(ParamId(3), 0.25);
    let mut bus = Strip::new(StripId(2), "Verb", StripKind::Bus);
    bus.inserts.push_back(slot);
    let mut synth = Strip::new(StripId(1), "Built-in Synth", StripKind::BuiltinSynth);
    synth.sends.push_back(AuxSend::new(StripId(2)));
    synth.output = Output::Bus(StripId(2));
    Mixer::try_from(MixerRepr {
        master: StripId(0),
        order: vec![StripId(1), StripId(2)],
        strips: vec![
            Strip::new(StripId(0), "Master", StripKind::Master),
            synth,
            bus,
        ],
    })
    .unwrap()
}

#[test]
fn mixer_json_round_trip() {
    let m = sample_mixer();
    let v = serde_json::to_value(&m).unwrap();
    assert_eq!(v["master"], 0);
    assert_eq!(v["strips"][0]["kind"], "master");
    assert_eq!(v["strips"][1]["kind"], "builtin_synth");
    assert_eq!(v["strips"][1]["output"], serde_json::json!({"bus": 2}));
    assert_eq!(
        v["strips"][2]["inserts"][0]["params"],
        serde_json::json!([[3, 0.25]])
    );
    assert_eq!(v["strips"][2]["inserts"][0]["plugin"]["format"], "clap");
    let back: Mixer = serde_json::from_value(v).unwrap();
    assert_eq!(back, m);
    assert_eq!(back.strip(StripId(2)).unwrap().name, "Verb");
    assert_eq!(
        back.strip(StripId(9)).unwrap_err(),
        ModelError::UnknownId(AnyId::Strip(StripId(9)))
    );
    assert_eq!(back.find_kind(StripKind::BuiltinSynth), Some(StripId(1)));
    assert_eq!(back.find_kind(StripKind::AudioPath(core_model::ids::OutputPathId(0))), None);
    let slots: Vec<_> = back.inserts().map(|(s, i, p)| (s, i, p.id)).collect();
    assert_eq!(slots, vec![(StripId(2), 0, PluginInstanceId(0))]);
}

#[test]
fn unknown_fields_and_duplicate_strips_are_refused() {
    let mut v = serde_json::to_value(sample_mixer()).unwrap();
    v["strips"][1]["bogus"] = serde_json::json!(1);
    assert!(serde_json::from_value::<Mixer>(v).is_err());
    let r = Mixer::try_from(MixerRepr {
        master: StripId(0),
        order: vec![],
        strips: vec![
            Strip::new(StripId(0), "Master", StripKind::Master),
            Strip::new(StripId(0), "X", StripKind::Bus),
        ],
    });
    assert!(r.unwrap_err().contains("duplicate strip id strip0"));
}

#[test]
fn a_new_mixer_holds_only_master() {
    let m = Mixer::new(StripId(7));
    assert_eq!(m.master(), StripId(7));
    assert!(m.order().is_empty());
    let s = m.strip(StripId(7)).unwrap();
    assert_eq!((s.name.as_str(), s.kind), ("Master", StripKind::Master));
}
```

- [ ] **Step 2: Run the test to verify it fails**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test mixer_types
```

Expected: compile errors — `unresolved import core_model::mixer`, no `StripId` in `ids`.

- [ ] **Step 3: Add the IDs**

In `crates/core-model/src/ids.rs`, after the `DrumRowId` `id_type!` block add:

```rust
id_type!(
    /// Identifies a mixer strip.
    StripId,
    "strip"
);
id_type!(
    /// Identifies a plugin instance (an insert or a plugin instrument's
    /// own slot); stable for the instance's life, never reused.
    PluginInstanceId,
    "plug"
);
id_type!(
    /// Identifies an immutable plugin state blob (the FNV-1a hash of its
    /// bytes), stored as `PluginState/<16 hex digits>.bin`.
    StateBlobId,
    "state"
);
```

Add two variants at the end of `enum AnyId`:

```rust
    /// A mixer strip.
    Strip(StripId),
    /// A plugin instance.
    Plugin(PluginInstanceId),
```

Add the two arms at the end of `impl fmt::Display for AnyId`'s match:

```rust
            AnyId::Strip(i) => i.fmt(f),
            AnyId::Plugin(i) => i.fmt(f),
```

After `any_from!(DrumRowId, DrumRow);` add:

```rust
any_from!(StripId, Strip);
any_from!(PluginInstanceId, Plugin);
```

- [ ] **Step 4: Add the routing errors**

In `crates/core-model/src/error.rs` change the import to `use crate::ids::{AnyId, SeqId, StripId};` and add after the `Cycle` variant:

```rust
    /// An output, send or sidechain would make the routing graph cyclic.
    /// `path` starts and ends with the same strip.
    #[error("routing cycle through strips {path:?}")]
    RoutingCycle {
        /// The cycle, first element repeated at the end.
        path: Vec<StripId>,
    },
    /// An output or send targets a strip it may not target.
    #[error("{strip} cannot route to {target}: {reason}")]
    InvalidRoutingTarget {
        /// The routed strip.
        strip: StripId,
        /// The refused target.
        target: StripId,
        /// Why.
        reason: String,
    },
```

- [ ] **Step 5: Write the mixer module**

Create `crates/core-model/src/mixer.rs`:

```rust
//! The mixer model (spec §4): strips, plugin slots, sends and outputs.
//! Routing rules live in [`crate::mixer_rules`]; edits in
//! `crate::command::mixer`.

use crate::error::{ModelError, Result};
use crate::ids::{AnyId, DestId, OutputPathId, PluginInstanceId, StateBlobId, StripId};
use im::{OrdMap, Vector};
use serde::{Deserialize, Serialize};

/// Endpoint reference that selects Apple's built-in synth (engine spec §5).
pub const BUILTIN_SYNTH_REF: &str = "builtin:synth";
/// Fader and send levels at or below this are silence (−∞ dB).
pub const GAIN_FLOOR_DB: f64 = -144.0;
/// Highest fader and send level.
pub const GAIN_MAX_DB: f64 = 12.0;
/// Name of the master strip.
pub const MASTER_NAME: &str = "Master";
/// Name of the built-in synth's strip.
pub const BUILTIN_SYNTH_STRIP: &str = "Built-in Synth";

/// A plugin parameter ID (CLAP `clap_id`, AU parameter address).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ParamId(pub u64);

/// Plugin formats VizDAW hosts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginFormat {
    /// Audio Unit (v2 or v3).
    Au,
    /// CLAP.
    Clap,
}

/// Which plugin a slot holds. Loading matches on `format` + `id` only; a
/// version change is reported, not refused.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginRef {
    /// Format.
    pub format: PluginFormat,
    /// AU: `type/subtype/manufacturer` four-char codes; CLAP: plugin id.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Vendor.
    pub vendor: String,
    /// Version as the plugin reports it.
    pub version: String,
}

impl PluginRef {
    /// The reference a version-1 `plugin_ref` string becomes: format `Au`
    /// for an `au:` prefix, else `Clap`; `id` and `name` are the whole
    /// string; vendor and version are empty (Plan Ruling 7).
    pub fn from_legacy(s: &str) -> PluginRef {
        PluginRef {
            format: if s.starts_with("au:") {
                PluginFormat::Au
            } else {
                PluginFormat::Clap
            },
            id: s.to_string(),
            name: s.to_string(),
            vendor: String::new(),
            version: String::new(),
        }
    }
}

/// A plugin instance: an insert, or a plugin instrument's own slot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginSlot {
    /// Stable instance ID.
    pub id: PluginInstanceId,
    /// The plugin.
    pub plugin: PluginRef,
    /// Bypassed; a bypassed insert keeps its latency.
    pub bypass: bool,
    /// Strip whose post-fader output feeds this insert's sidechain input.
    pub sidechain: Option<StripId>,
    /// The last captured state blob.
    pub state: Option<StateBlobId>,
    /// Values changed through VizDAW since the last capture (the overlay).
    #[serde(with = "param_list")]
    pub params: OrdMap<ParamId, f64>,
}

impl PluginSlot {
    /// An active slot with no sidechain, state or overlay.
    pub fn new(id: PluginInstanceId, plugin: PluginRef) -> PluginSlot {
        PluginSlot {
            id,
            plugin,
            bypass: false,
            sidechain: None,
            state: None,
            params: OrdMap::new(),
        }
    }
}

/// The overlay serializes as `[[param, value], …]` ordered by param, so
/// JSON object keys never have to be numbers.
mod param_list {
    use super::ParamId;
    use im::OrdMap;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(m: &OrdMap<ParamId, f64>, s: S) -> Result<S::Ok, S::Error> {
        s.collect_seq(m.iter().map(|(k, v)| (*k, *v)))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<OrdMap<ParamId, f64>, D::Error> {
        let v: Vec<(ParamId, f64)> = Vec::deserialize(d)?;
        Ok(v.into_iter().collect())
    }
}

/// What feeds a strip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StripKind {
    /// The main output of a plugin instrument destination.
    Instrument(DestId),
    /// Extra output `n` (≥ 1) of a multi-output plugin instrument.
    InstrumentOutput(DestId, u16),
    /// The built-in synth: an instrument strip whose instrument is built in
    /// (one per document, Plan Ruling 1).
    BuiltinSynth,
    /// An audio output path (silent source until sub-project 5).
    AudioPath(OutputPathId),
    /// An aux bus.
    Bus,
    /// The master.
    Master,
}

impl StripKind {
    /// True for strips fed by an instrument's main output.
    pub fn is_instrument(self) -> bool {
        matches!(self, StripKind::Instrument(_) | StripKind::BuiltinSynth)
    }
}

/// Where a strip's main output goes. Master itself always plays on device
/// channels 1–2 and keeps `Output::Master`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Output {
    /// The master strip.
    Master,
    /// An aux bus.
    Bus(StripId),
}

/// An aux send (the spec's `Send`, Plan Ruling 3).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuxSend {
    /// The bus it feeds.
    pub target: StripId,
    /// Level in dB (`GAIN_FLOOR_DB` … `GAIN_MAX_DB`).
    pub level_db: f64,
    /// Tapped before the fader (after the inserts) instead of after it.
    pub pre_fader: bool,
    /// Off sends keep their settings but carry nothing.
    pub enabled: bool,
}

impl AuxSend {
    /// A post-fader send at 0 dB, enabled.
    pub fn new(target: StripId) -> AuxSend {
        AuxSend {
            target,
            level_db: 0.0,
            pre_fader: false,
            enabled: true,
        }
    }
}

/// A mixer channel strip.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Strip {
    /// Strip ID.
    pub id: StripId,
    /// Display name.
    pub name: String,
    /// What feeds it.
    pub kind: StripKind,
    /// Insert chain, processed in order.
    pub inserts: Vector<PluginSlot>,
    /// Fader in dB (`GAIN_FLOOR_DB` = −∞ … `GAIN_MAX_DB`).
    pub gain_db: f64,
    /// Pan −1 … +1 (constant power; ignored on master).
    pub pan: f64,
    /// Explicit mute (also silences its sidechain feed).
    pub mute: bool,
    /// Solo.
    pub solo: bool,
    /// Aux sends.
    pub sends: Vector<AuxSend>,
    /// Main output.
    pub output: Output,
}

impl Strip {
    /// A strip at 0 dB, centred, unmuted, with no inserts or sends, to
    /// master.
    pub fn new(id: StripId, name: impl Into<String>, kind: StripKind) -> Strip {
        Strip {
            id,
            name: name.into(),
            kind,
            inserts: Vector::new(),
            gain_db: 0.0,
            pan: 0.0,
            mute: false,
            solo: false,
            sends: Vector::new(),
            output: Output::Master,
        }
    }
}

/// The mixer: strips by ID, display order and the master. Change it with
/// commands.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(into = "MixerRepr", try_from = "MixerRepr")]
pub struct Mixer {
    strips: OrdMap<StripId, Strip>,
    order: Vector<StripId>,
    master: StripId,
}

/// The serialized shape of a [`Mixer`]: strips ordered by ID.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MixerRepr {
    /// The master strip's ID.
    pub master: StripId,
    /// Display order of every strip except master.
    pub order: Vec<StripId>,
    /// Every strip, master included.
    pub strips: Vec<Strip>,
}

impl From<Mixer> for MixerRepr {
    fn from(m: Mixer) -> MixerRepr {
        MixerRepr {
            master: m.master,
            order: m.order.iter().copied().collect(),
            strips: m.strips.values().cloned().collect(),
        }
    }
}

impl TryFrom<MixerRepr> for Mixer {
    type Error = String;
    fn try_from(r: MixerRepr) -> std::result::Result<Mixer, String> {
        let mut strips = OrdMap::new();
        for s in r.strips {
            let id = s.id;
            if strips.insert(id, s).is_some() {
                return Err(format!("duplicate strip id {id}"));
            }
        }
        Ok(Mixer {
            strips,
            order: r.order.into_iter().collect(),
            master: r.master,
        })
    }
}

impl Mixer {
    /// A mixer holding only the master strip `master`.
    pub fn new(master: StripId) -> Mixer {
        Mixer {
            strips: OrdMap::unit(master, Strip::new(master, MASTER_NAME, StripKind::Master)),
            order: Vector::new(),
            master,
        }
    }

    /// Strips by ID.
    pub fn strips(&self) -> &OrdMap<StripId, Strip> {
        &self.strips
    }

    /// Display order (every strip except master).
    pub fn order(&self) -> &Vector<StripId> {
        &self.order
    }

    /// The master strip's ID.
    pub fn master(&self) -> StripId {
        self.master
    }

    /// A strip by ID.
    pub fn strip(&self, id: StripId) -> Result<&Strip> {
        self.strips
            .get(&id)
            .ok_or(ModelError::UnknownId(AnyId::Strip(id)))
    }

    /// The first strip of `kind`, by ID.
    pub fn find_kind(&self, kind: StripKind) -> Option<StripId> {
        self.strips.values().find(|s| s.kind == kind).map(|s| s.id)
    }

    /// Every insert as `(strip, position, slot)`, by strip ID then position.
    pub fn inserts(&self) -> impl Iterator<Item = (StripId, usize, &PluginSlot)> + '_ {
        self.strips.values().flat_map(|s| {
            s.inserts
                .iter()
                .enumerate()
                .map(move |(i, p)| (s.id, i, p))
        })
    }

}

/// Linear gain of a level in dB; at or below [`GAIN_FLOOR_DB`] it is 0.
pub fn db_to_gain(db: f64) -> f64 {
    if db <= GAIN_FLOOR_DB {
        0.0
    } else {
        10f64.powf(db / 20.0)
    }
}

/// Constant-power pan gains `(left, right)`: `θ = (pan + 1)·π/4`, gains
/// `cos θ`, `sin θ` (−3 dB each at centre). `pan` is clamped to ±1.
pub fn pan_gains(pan: f64) -> (f64, f64) {
    let t = (pan.clamp(-1.0, 1.0) + 1.0) * std::f64::consts::FRAC_PI_4;
    (t.cos(), t.sin())
}

```

In `crates/core-model/src/lib.rs` add `pub mod mixer;` after `pub mod meter;`.

- [ ] **Step 6: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test mixer_types
cargo test --workspace
```

Expected: `mixer_types` 8 passed; the whole workspace still passes (`cargo test --workspace --no-fail-fast`): nothing uses the new types yet and the file format is untouched (counters and derivation arrive with format v2 in Task 2).

- [ ] **Step 7: Lint and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
git add crates/core-model/src/ids.rs crates/core-model/src/error.rs crates/core-model/src/mixer.rs crates/core-model/src/lib.rs crates/core-model/tests/mixer_types.rs
git commit -m "feat(core-model): mixer types, strip and plugin IDs, routing errors

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---
### Task 2: Document carries the mixer; plugin slots; file format v2

**Goal:** Put a `Mixer` on every `Document`, turn `DestKind::PluginInstrument { plugin_ref }` into `{ slot: PluginSlot }`, bump the project format to version 2 with a v1 → v2 migration, move the 2a goldens to `tests/fixtures/v1/` as migration inputs, and regenerate the demo package.

**Files:**
- Modify: `crates/core-model/src/ids.rs` (`Counters.{strip, plugin}`)
- Modify: `crates/core-model/src/document.rs`
- Modify: `crates/core-model/src/mixer.rs` (`Mixer::add`, `derive_mixer`)
- Modify: `crates/core-model/src/file.rs`
- Modify: `crates/core-model/src/command/routing.rs` (only `check_dest`)
- Move: `crates/core-model/tests/fixtures/{minimal,nested_references,mixed_lanes,drum_lane}.json` → `crates/core-model/tests/fixtures/v1/`
- Modify: `crates/core-model/tests/file.rs`
- Modify (constructor updates only): `crates/core-model/tests/{cmd_drum,cmd_lane,cmd_reference,query_usage,query_flatten,cmd_audio_routing,model_types,rules,cmd_sequence}.rs`, `crates/core-model/benches/model.rs`
- Modify: `crates/core-engine/src/schedule/targets.rs`
- Modify (constructor updates only): `crates/core-engine/tests/{tempo_table,offline,schedule}.rs`
- Create: `crates/core-model/tests/mixer_document.rs`
- Regenerate: `fixtures/demo.vizdaw/project.json`

**Interfaces:**
- Consumes: Task 1's `mixer::{Mixer, Strip, StripKind, PluginSlot, PluginRef, BUILTIN_SYNTH_REF}`.
- Produces:
  - `Counters { …, strip: u64, plugin: u64 }` with `alloc_strip() -> StripId`, `alloc_plugin() -> PluginInstanceId`; `mixer::derive_mixer(instruments, outputs, &mut Counters) -> Mixer` (Plan Ruling 6).
  - `DestKind::PluginInstrument { slot: PluginSlot }`; `DestKind::plugin(name: &str) -> DestKind` (slot ID 0, `PluginRef::from_legacy(name)`); `DestKind::is_builtin_synth(&self) -> bool`. `Destination` and `DestKind` are no longer `Eq` (they hold `f64` overlays).
  - `Document::mixer(&self) -> &Mixer`; `Document::find_slot(PluginInstanceId) -> Option<SlotOwner>` with `pub enum SlotOwner { Instrument(DestId), Insert(StripId, usize) }`; `Document::plugin_slot(PluginInstanceId) -> Option<&PluginSlot>`.
  - `DocumentRepr { …, mixer: Option<Mixer>, counters }` (`#[serde(default)]`; `None` derives a mixer, Plan Ruling 6). `Document::new()` has a master strip `StripId(0)` and `counters.strip == 1`.
  - `file::VERSION == 2`, `file::migrate_v1_to_v2(Value) -> Result<Value>`, `MIGRATIONS == &[migrate_v1_to_v2]`.
  - `core_engine::schedule::targets::BUILTIN_SYNTH_REF` is now a re-export of `core_model::mixer::BUILTIN_SYNTH_REF`.

- [ ] **Step 1: Write the failing tests**

Create `crates/core-model/tests/mixer_document.rs`:

```rust
use core_model::document::{DestKind, Destination, Document, DocumentRepr, OutputPath, Settings, SlotOwner};
use core_model::file::{from_json, to_json};
use core_model::ids::{Counters, DestId, OutputPathId, PluginInstanceId, StripId};
use core_model::mixer::{BUILTIN_SYNTH_STRIP, Output, PluginRef, StripKind};

fn ep(id: u64, r: &str, ch: u8) -> Destination {
    Destination {
        id: DestId(id),
        name: format!("D{id}"),
        kind: DestKind::MidiEndpoint {
            endpoint_ref: r.into(),
            group: 0,
            channel: ch,
        },
        default_patch: None,
    }
}

fn kinds(d: &Document) -> Vec<(StripId, StripKind, String)> {
    d.mixer()
        .strips()
        .values()
        .map(|s| (s.id, s.kind, s.name.clone()))
        .collect()
}

#[test]
fn counters_allocate_strips_and_instances() {
    let mut c = Counters::default();
    assert_eq!(c.alloc_strip(), StripId(0));
    assert_eq!(c.alloc_strip(), StripId(1));
    assert_eq!(c.alloc_plugin(), PluginInstanceId(0));
    assert_eq!((c.strip, c.plugin), (2, 1));
}

#[test]
fn a_new_document_has_only_master() {
    let d = Document::new();
    assert_eq!(kinds(&d), vec![(StripId(0), StripKind::Master, "Master".into())]);
    assert_eq!(d.mixer().master(), StripId(0));
    assert_eq!(d.counters().strip, 1);
}

#[test]
fn from_parts_without_a_mixer_derives_one() {
    let d = Document::from_parts(DocumentRepr {
        settings: Settings::default(),
        instruments: vec![
            Destination {
                id: DestId(0),
                name: "Pad".into(),
                kind: DestKind::plugin("au:pad"),
                default_patch: None,
            },
            ep(1, "builtin:synth", 0),
            ep(2, "builtin:synth", 9),
            ep(3, "Ext Synth", 0),
        ],
        outputs: vec![OutputPath {
            id: OutputPathId(0),
            name: "Main".into(),
            reference: "bus:main".into(),
        }],
        media: vec![],
        sequences: vec![],
        mixer: None,
        counters: Counters {
            dest: 4,
            output: 1,
            plugin: 1,
            ..Counters::default()
        },
    })
    .unwrap();
    assert_eq!(
        kinds(&d),
        vec![
            (StripId(0), StripKind::Master, "Master".into()),
            (StripId(1), StripKind::Instrument(DestId(0)), "Pad".into()),
            (StripId(2), StripKind::BuiltinSynth, BUILTIN_SYNTH_STRIP.into()),
            (StripId(3), StripKind::AudioPath(OutputPathId(0)), "Main".into()),
        ]
    );
    assert_eq!(
        d.mixer().order().iter().copied().collect::<Vec<_>>(),
        vec![StripId(1), StripId(2), StripId(3)]
    );
    assert!(d.mixer().strips().values().all(|s| s.output == Output::Master));
    assert_eq!(d.counters().strip, 4);
    assert_eq!(d.find_slot(PluginInstanceId(0)), Some(SlotOwner::Instrument(DestId(0))));
    assert_eq!(
        d.plugin_slot(PluginInstanceId(0)).unwrap().plugin,
        PluginRef::from_legacy("au:pad")
    );
    assert_eq!(d.find_slot(PluginInstanceId(9)), None);
}

#[test]
fn json_round_trip_keeps_the_mixer() {
    let d = Document::new();
    let text = to_json(&d).unwrap();
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["version"], 2);
    assert_eq!(v["document"]["mixer"]["master"], 0);
    assert_eq!(v["document"]["counters"]["strip"], 1);
    assert_eq!(from_json(&text).unwrap(), d);
}

#[test]
fn builtin_and_plugin_helpers() {
    assert!(ep(0, "builtin:synth", 3).kind.is_builtin_synth());
    assert!(!ep(0, "virtual:out", 3).kind.is_builtin_synth());
    let DestKind::PluginInstrument { slot } = DestKind::plugin("clap:x") else {
        panic!("not a plugin");
    };
    assert_eq!(slot.id, PluginInstanceId(0));
    assert_eq!(slot.plugin.name, "clap:x");
    assert!(!DestKind::plugin("clap:x").is_builtin_synth());
}
```

In `crates/core-model/tests/file.rs`:
- change the `fixture` helper's path to `.join("tests/fixtures/v1")`;
- replace the `golden` function with:

```rust
fn golden(name: &str) {
    let text = fixture(name);
    // Version-1 input: loading migrates it.
    let d = from_json(&text).unwrap();
    let v2 = to_json(&d).unwrap();
    assert!(v2.contains("\"version\": 2"), "{name}");
    // Canonical: a version-2 file re-serializes byte for byte.
    assert_eq!(to_json(&from_json(&v2).unwrap()).unwrap(), v2, "{name}");
    insta::assert_json_snapshot!(name, summary(&d));
}
```

- add `"/document/mixer"` and `"/document/mixer/strips/0"` to the pointer list in `unknown_fields_are_rejected_at_every_level`;
- extend the imports with `use core_model::document::DestKind; use core_model::ids::{DestId, OutputPathId, PluginInstanceId, StripId}; use core_model::mixer::{Output, PluginFormat, PluginRef, StripKind};` (keep the existing `LaneId, SeqId` import);
- add:

```rust
#[test]
fn v1_plugin_instruments_migrate_to_slots_and_strips() {
    let d = from_json(&fixture("mixed_lanes")).unwrap();
    let DestKind::PluginInstrument { slot } = &d.instruments().get(&DestId(0)).unwrap().kind else {
        panic!("dest0 is not a plugin instrument");
    };
    assert_eq!(slot.id, PluginInstanceId(0));
    assert_eq!(slot.plugin, PluginRef::from_legacy("au:piano"));
    assert_eq!(slot.plugin.format, PluginFormat::Au);
    assert!(slot.params.is_empty() && slot.state.is_none() && !slot.bypass);
    let strips: Vec<(StripId, StripKind, String)> = d
        .mixer()
        .strips()
        .values()
        .map(|s| (s.id, s.kind, s.name.clone()))
        .collect();
    assert_eq!(
        strips,
        vec![
            (StripId(0), StripKind::Master, "Master".to_string()),
            (StripId(1), StripKind::Instrument(DestId(0)), "Piano".to_string()),
            (StripId(2), StripKind::AudioPath(OutputPathId(0)), "Main".to_string()),
        ]
    );
    assert!(d.mixer().strips().values().all(|s| s.output == Output::Master));
    assert_eq!((d.counters().strip, d.counters().plugin), (3, 1));
    let v: Value = serde_json::from_str(&to_json(&d).unwrap()).unwrap();
    assert_eq!(v["version"], 2);
    assert_eq!(
        v["document"]["instruments"][0]["kind"]["plugin_instrument"]["slot"]["plugin"]["format"],
        "au"
    );

    // drum_lane: dest0 is an external endpoint, dest1 the plugin "clap:perc".
    let d = from_json(&fixture("drum_lane")).unwrap();
    assert_eq!(
        d.mixer().find_kind(StripKind::Instrument(DestId(1))),
        Some(StripId(1))
    );
    assert_eq!(d.mixer().find_kind(StripKind::BuiltinSynth), None);
    assert_eq!(d.plugin_slot(PluginInstanceId(0)).unwrap().plugin.id, "clap:perc");
}
```

- [ ] **Step 2: Move the goldens and run the tests to verify they fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p crates/core-model/tests/fixtures/v1
git mv crates/core-model/tests/fixtures/minimal.json crates/core-model/tests/fixtures/nested_references.json crates/core-model/tests/fixtures/mixed_lanes.json crates/core-model/tests/fixtures/drum_lane.json crates/core-model/tests/fixtures/v1/
cargo test -p core-model --test mixer_document --test file
```

Expected: compile errors — no `SlotOwner`, no `DestKind::plugin`, no field `mixer` in `DocumentRepr`.

- [ ] **Step 3: Put the mixer on the document**

In `crates/core-model/src/ids.rs` add two fields at the end of `struct Counters`:

```rust
    /// Next strip ID.
    pub strip: u64,
    /// Next plugin instance ID.
    pub plugin: u64,
```

and two methods at the end of `impl Counters`:

```rust
    /// Allocates a strip ID.
    pub fn alloc_strip(&mut self) -> StripId {
        StripId(bump(&mut self.strip))
    }
    /// Allocates a plugin instance ID.
    pub fn alloc_plugin(&mut self) -> PluginInstanceId {
        PluginInstanceId(bump(&mut self.plugin))
    }
```

In `crates/core-model/src/mixer.rs` add `use crate::document::{DestKind, Destination, OutputPath};`, add `Counters` to the `crate::ids` import, and append:

```rust
/// The mixer a document without one gets (Plan Ruling 6): master; then,
/// in destination order, an `Instrument` strip per plugin instrument and
/// the built-in synth strip at the first `builtin:synth` destination; then
/// an `AudioPath` strip per output path. Everything routes to master.
pub fn derive_mixer<'a>(
    instruments: impl IntoIterator<Item = &'a Destination>,
    outputs: impl IntoIterator<Item = &'a OutputPath>,
    counters: &mut Counters,
) -> Mixer {
    let mut m = Mixer::new(counters.alloc_strip());
    let mut builtin = false;
    for d in instruments {
        match &d.kind {
            DestKind::PluginInstrument { .. } => m.add(
                Strip::new(counters.alloc_strip(), d.name.clone(), StripKind::Instrument(d.id)),
                None,
            ),
            DestKind::MidiEndpoint { endpoint_ref, .. }
                if endpoint_ref == BUILTIN_SYNTH_REF && !builtin =>
            {
                builtin = true;
                m.add(
                    Strip::new(
                        counters.alloc_strip(),
                        BUILTIN_SYNTH_STRIP,
                        StripKind::BuiltinSynth,
                    ),
                    None,
                );
            }
            DestKind::MidiEndpoint { .. } => {}
        }
    }
    for o in outputs {
        m.add(
            Strip::new(counters.alloc_strip(), o.name.clone(), StripKind::AudioPath(o.id)),
            None,
        );
    }
    m
}
```

In `crates/core-model/src/document.rs`:

Replace the imports at the top with:

```rust
use crate::error::{ModelError, Result};
use crate::event::Event;
use crate::ids::{
    AnyId, Counters, DestId, EventId, LaneId, MediaId, OutputPathId, PluginInstanceId, SeqId,
    StripId, TakeId,
};
use crate::lane::Lane;
use crate::mixer::{BUILTIN_SYNTH_REF, Mixer, PluginRef, PluginSlot, derive_mixer};
use crate::modify::ModifyTemplate;
use crate::sequence::Sequence;
use crate::time::Tick;
use im::OrdMap;
use serde::{Deserialize, Serialize};
```

Change `Destination`'s derive to `#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]` (drop `Eq`) and replace `DestKind` with:

```rust
/// Destination kinds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum DestKind {
    /// A MIDI endpoint with a UMP group and channel (each 0–15).
    MidiEndpoint {
        /// Opaque endpoint reference (defined by 2b).
        endpoint_ref: String,
        /// UMP group.
        group: u8,
        /// Channel.
        channel: u8,
    },
    /// A hosted plugin instrument (group 0, channel 0) and its own slot.
    PluginInstrument {
        /// The instrument's plugin instance.
        slot: PluginSlot,
    },
}

impl DestKind {
    /// A plugin instrument whose slot holds `PluginRef::from_legacy(name)`
    /// with instance ID 0; `AddDestination` replaces the ID with a fresh one.
    pub fn plugin(name: &str) -> DestKind {
        DestKind::PluginInstrument {
            slot: PluginSlot::new(PluginInstanceId(0), PluginRef::from_legacy(name)),
        }
    }

    /// True for MIDI endpoints that play on the built-in synth.
    pub fn is_builtin_synth(&self) -> bool {
        matches!(self, DestKind::MidiEndpoint { endpoint_ref, .. } if endpoint_ref == BUILTIN_SYNTH_REF)
    }
}

/// Where a plugin slot lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotOwner {
    /// A plugin instrument destination's own slot.
    Instrument(DestId),
    /// Insert `index` of a strip.
    Insert(StripId, usize),
}
```

Add `mixer: Mixer,` to `struct Document` (after `sequences`), and to `DocumentRepr` after `sequences`:

```rust
    /// The mixer; `None` (version-1 input, programmatic construction)
    /// derives one (Plan Ruling 6).
    #[serde(default)]
    pub mixer: Option<Mixer>,
```

In `From<Document> for DocumentRepr` add `mixer: Some(d.mixer),` after `sequences`. Replace `TryFrom<DocumentRepr> for Document` with:

```rust
impl TryFrom<DocumentRepr> for Document {
    type Error = String;
    fn try_from(r: DocumentRepr) -> std::result::Result<Self, String> {
        let instruments = collect_unique(r.instruments, |v| v.id, "destination")?;
        let outputs = collect_unique(r.outputs, |v| v.id, "output")?;
        let mut counters = r.counters;
        let mixer = match r.mixer {
            Some(m) => m,
            None => derive_mixer(instruments.values(), outputs.values(), &mut counters),
        };
        let mut d = Document {
            settings: r.settings,
            instruments,
            outputs,
            media: collect_unique(r.media, |v| v.id, "media")?,
            sequences: collect_unique(r.sequences, |v| v.id, "sequence")?,
            mixer,
            counters,
            index: OrdMap::new(),
        };
        d.rebuild_index()?;
        Ok(d)
    }
}
```

Replace `Document::new` with:

```rust
    /// An empty document with default settings and a mixer holding only
    /// the master strip.
    pub fn new() -> Document {
        let mut counters = Counters::default();
        let mixer = Mixer::new(counters.alloc_strip());
        Document {
            settings: Settings::default(),
            instruments: OrdMap::new(),
            outputs: OrdMap::new(),
            media: OrdMap::new(),
            sequences: OrdMap::new(),
            mixer,
            counters,
            index: OrdMap::new(),
        }
    }
```

In `crates/core-model/src/mixer.rs`, add to `impl Mixer` (after `inserts`; `derive_mixer` uses it):

```rust
    /// Adds `strip` at display position `index` (default: the end).
    pub(crate) fn add(&mut self, strip: Strip, index: Option<usize>) {
        let id = strip.id;
        self.strips.insert(id, strip);
        let i = index.unwrap_or(self.order.len()).min(self.order.len());
        self.order.insert(i, id);
    }
```

Add to the public accessors (after `counters()`):

```rust
    /// The mixer.
    pub fn mixer(&self) -> &Mixer {
        &self.mixer
    }

    /// Where plugin instance `id` lives.
    pub fn find_slot(&self, id: PluginInstanceId) -> Option<SlotOwner> {
        for d in self.instruments.values() {
            if let DestKind::PluginInstrument { slot } = &d.kind {
                if slot.id == id {
                    return Some(SlotOwner::Instrument(d.id));
                }
            }
        }
        self.mixer
            .inserts()
            .find(|(_, _, p)| p.id == id)
            .map(|(s, i, _)| SlotOwner::Insert(s, i))
    }

    /// Plugin instance `id`'s slot.
    pub fn plugin_slot(&self, id: PluginInstanceId) -> Option<&PluginSlot> {
        match self.find_slot(id)? {
            SlotOwner::Instrument(d) => match &self.instruments.get(&d)?.kind {
                DestKind::PluginInstrument { slot } => Some(slot),
                DestKind::MidiEndpoint { .. } => None,
            },
            SlotOwner::Insert(s, i) => self.mixer.strip(s).ok()?.inserts.get(i),
        }
    }
```

- [ ] **Step 4: Validate plugin slots on destination edits**

In `crates/core-model/src/command/routing.rs` replace `check_dest` with:

```rust
fn check_dest(kind: &DestKind, patch: &Option<Patch>) -> Result<()> {
    match kind {
        DestKind::MidiEndpoint { group, channel, .. } => {
            check_group_channel(Some((*group, *channel)))?;
        }
        DestKind::PluginInstrument { slot } => {
            if slot.sidechain.is_some() {
                return Err(ModelError::invalid(
                    "a plugin instrument's own slot has no sidechain input",
                ));
            }
            if slot.params.values().any(|v| !v.is_finite()) {
                return Err(ModelError::range("params", "non-finite value"));
            }
        }
    }
    if let Some(p) = patch {
        if p.program > 127 || p.bank.is_some_and(|(m, l)| m > 127 || l > 127) {
            return Err(ModelError::range("default_patch", p.program));
        }
    }
    Ok(())
}
```

- [ ] **Step 5: Format version 2 and the migration**

In `crates/core-model/src/file.rs`:
- in the module doc change `"version": 1` to `"version": 2`;
- change the imports to add `use crate::ids::PluginInstanceId; use crate::mixer::{PluginRef, PluginSlot}; use serde_json::json;`;
- set `pub const VERSION: u64 = 2;`;
- replace the `MIGRATIONS` doc and value with:

```rust
/// Migrations indexed by `from - FIRST_VERSION`.
pub const MIGRATIONS: &[Migration] = &[migrate_v1_to_v2];

/// Version 1 → 2: each plugin instrument's `plugin_ref` string becomes a
/// `slot` holding `PluginRef::from_legacy(string)` with instance IDs 0, 1, …
/// in destination order; `counters` gains `strip` (0) and `plugin`. The
/// mixer is left out; loading derives it (Plan Ruling 6).
pub fn migrate_v1_to_v2(mut v: Value) -> Result<Value> {
    let doc = v
        .get_mut("document")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| corrupt("missing document"))?;
    let mut next = 0u64;
    if let Some(Value::Array(insts)) = doc.get_mut("instruments") {
        for d in insts.iter_mut() {
            let Some(p) = d.pointer_mut("/kind/plugin_instrument") else {
                continue;
            };
            let name = p
                .get("plugin_ref")
                .and_then(Value::as_str)
                .ok_or_else(|| corrupt("plugin_instrument without plugin_ref"))?
                .to_string();
            let slot = PluginSlot::new(PluginInstanceId(next), PluginRef::from_legacy(&name));
            *p = json!({ "slot": serde_json::to_value(slot).map_err(corrupt)? });
            next += 1;
        }
    }
    let counters = doc
        .get_mut("counters")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| corrupt("missing counters"))?;
    counters.insert("strip".into(), Value::from(0u64));
    counters.insert("plugin".into(), Value::from(next));
    Ok(v)
}
```

- [ ] **Step 6: Update constructors in existing tests and the engine**

These are mechanical; make exactly these edits:

1. Add the line `mixer: None,` immediately before the `counters:` field of every `DocumentRepr { … }` literal in: `crates/core-model/tests/cmd_reference.rs`, `cmd_drum.rs`, `rules.rs`, `model_types.rs`, `cmd_sequence.rs`, `cmd_lane.rs`, `query_usage.rs`, `query_flatten.rs` and `crates/core-model/benches/model.rs` (10 literals: one in each test file, two in the bench; `grep -rn "DocumentRepr {" crates/core-model` lists them — skip the three hits in `src/document.rs`, which are the struct and its `From` impl).
2. Single plugin destination — replace `DestKind::PluginInstrument {\n plugin_ref: "p".into(),\n }` with `DestKind::plugin("p")` in `cmd_lane.rs`, `query_usage.rs`, `query_flatten.rs` and `cmd_audio_routing.rs`; in the first three also add `plugin: 1,` to the `Counters { … }` literal of the same `DocumentRepr`.
3. Two plugin destinations sharing a closure `dest(i)` — in `cmd_drum.rs` and `cmd_reference.rs` replace the `kind:` field inside the closure with
   `kind: DestKind::PluginInstrument { slot: PluginSlot::new(PluginInstanceId(i), PluginRef::from_legacy("p")) },`
   add `plugin: 2,` to the `Counters { … }` literal, and add `use core_model::ids::PluginInstanceId; use core_model::mixer::{PluginRef, PluginSlot};` to the imports.
4. `crates/core-model/tests/model_types.rs` lists every `Counters` field: add `strip: 0,` and `plugin: 0,` to that literal.
5. `crates/core-engine/tests/tempo_table.rs`: replace `DestKind::PluginInstrument {\n plugin_ref: "au:x".into(),\n }` with `DestKind::plugin("au:x")`.
6. `crates/core-engine/tests/offline.rs` (`unroutable_plugin_is_reported_and_absent`): replace the `DestKind::PluginInstrument { plugin_ref: "clap:pad".into() }` argument with `DestKind::plugin("clap:pad")`.
7. `crates/core-engine/tests/schedule.rs` (`unroutable_events_are_reported_not_played`): replace the `DestKind::PluginInstrument { plugin_ref: "au:pad".into() }` argument with `DestKind::plugin("au:pad")`.

In `crates/core-engine/src/schedule/targets.rs` replace `pub const BUILTIN_SYNTH_REF: &str = "builtin:synth";` (and its doc line) with:

```rust
/// Endpoint reference that selects the built-in synth.
pub use core_model::mixer::BUILTIN_SYNTH_REF;
```

and the first arm of `kinds_for` with:

```rust
        DestKind::PluginInstrument { slot } => Err(format!(
            "plugin instrument {:?} cannot play until plugin hosting exists",
            slot.plugin.name
        )),
```

(The message text is unchanged for names made by `DestKind::plugin`, so `unroutable_plugin_is_reported_and_absent`'s exact warning still matches.)

- [ ] **Step 7: Run the model and engine tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model
cargo test -p core-engine
```

Expected: all pass, including `mixer_document` (5 tests) and `file` (the four goldens keep their existing `.snap` summaries; `v1_plugin_instruments_migrate_to_slots_and_strips` passes). `core-ffi` and `vizdaw-play` are checked in Step 8.

- [ ] **Step 8: Regenerate the demo package and check the offline golden is unchanged**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo run -q -p vizdaw-play -- --write-demo fixtures/demo.vizdaw
grep -c '"mixer"' fixtures/demo.vizdaw/project.json
cargo run -q -p vizdaw-play -- fixtures/demo.vizdaw --seq Song --offline "$TMPDIR/demo.txt"
diff fixtures/demo.offline.txt "$TMPDIR/demo.txt" && echo "offline golden unchanged"
cargo test --workspace
```

Expected: `wrote fixtures/demo.vizdaw`, `1`, `wrote …/demo.txt (170 event(s))`, `offline golden unchanged`, and the whole workspace passes (including `core-ffi` and `vizdaw-play`'s `checked_in_demo_matches_a_fresh_build`). At this point the demo's mixer holds only master: destinations created by commands get their strips in Task 3, which regenerates the file again.

- [ ] **Step 9: Lint and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
git add crates/core-model crates/core-engine/src/schedule/targets.rs crates/core-engine/tests/tempo_table.rs crates/core-engine/tests/offline.rs crates/core-engine/tests/schedule.rs fixtures/demo.vizdaw/project.json
git commit -m "feat(core-model): mixer on the document, plugin slots, project format v2

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---
### Task 3: Destination and output edits keep the strips in step

**Goal:** Adding, editing, renaming and removing destinations and output paths creates, renames and removes their strips in the same command (one undo step), allocates fresh plugin instance IDs, and creates the built-in synth strip when the first `builtin:synth` destination appears.

**Files:**
- Modify: `crates/core-model/src/mixer.rs` (`Mixer::{strip_mut, remove}`)
- Modify: `crates/core-model/src/document.rs` (`Document::mixer_mut`)
- Create: `crates/core-model/src/command/strips.rs`
- Modify: `crates/core-model/src/command/mod.rs`
- Modify: `crates/core-model/src/command/routing.rs`
- Modify: `crates/core-engine/tests/demo.rs`
- Test: `crates/core-model/tests/mixer_routing.rs`
- Regenerate: `fixtures/demo.vizdaw/project.json`

**Interfaces:**
- Consumes: Task 2's `Document::mixer`, `DestKind::is_builtin_synth`, `Mixer::{add, find_kind}`, `Counters::{alloc_strip, alloc_plugin}`.
- Adds: crate `Document::mixer_mut`, crate `Mixer::{strip_mut, remove}`.
- Produces (behaviour of existing `RoutingCmd`s):
  - `AddDestination` with a plugin instrument: the slot gets a fresh `PluginInstanceId` (the caller's ID is ignored) and an `Instrument(dest)` strip named after the destination is added; `Report.created == [Dest, Plugin, Strip]`. Any destination using `builtin:synth` adds the `BuiltinSynth` strip if none exists (`created` gains its `Strip`).
  - `EditDestination`: same plugin (format + id) keeps the slot (metadata updated), another plugin gets a fresh ID; plugin → endpoint removes the destination's `Instrument`/`InstrumentOutput` strips; endpoint → plugin adds the `Instrument` strip.
  - `RenameDestination` / `RenameOutput` rename the `Instrument` / `AudioPath` strip.
  - `RemoveDestination` removes its strips; `AddOutput` adds an `AudioPath` strip; `RemoveOutput` removes it (strip removal retargets outputs to master, drops sends, clears sidechains — `Mixer::remove`).

- [ ] **Step 1: Write the failing test**

Create `crates/core-model/tests/mixer_routing.rs`:

```rust
use core_model::command::{Command, RoutingCmd, apply};
use core_model::document::{DestKind, Document};
use core_model::history::History;
use core_model::ids::{AnyId, DestId, OutputPathId, PluginInstanceId, StripId};
use core_model::mixer::{BUILTIN_SYNTH_STRIP, PluginRef, StripKind};
use core_model::selection::ViewFilter;

fn run(d: &Document, c: RoutingCmd) -> core_model::command::Applied {
    apply(d, &Command::Routing(c)).unwrap()
}

fn endpoint(r: &str, ch: u8) -> DestKind {
    DestKind::MidiEndpoint {
        endpoint_ref: r.into(),
        group: 0,
        channel: ch,
    }
}

fn add(d: &Document, name: &str, kind: DestKind) -> core_model::command::Applied {
    run(
        d,
        RoutingCmd::AddDestination {
            name: name.into(),
            kind,
            default_patch: None,
        },
    )
}

fn strips(d: &Document) -> Vec<(StripId, StripKind, String)> {
    d.mixer()
        .strips()
        .values()
        .map(|s| (s.id, s.kind, s.name.clone()))
        .collect()
}

#[test]
fn builtin_destinations_share_one_strip() {
    let a = add(&Document::new(), "Piano", endpoint("builtin:synth", 0));
    assert_eq!(
        a.report.created,
        vec![AnyId::Dest(DestId(0)), AnyId::Strip(StripId(1))]
    );
    let b = add(&a.doc, "Drums", endpoint("builtin:synth", 9));
    assert_eq!(b.report.created, vec![AnyId::Dest(DestId(1))]);
    let c = add(&b.doc, "Ext", endpoint("Some Synth", 0));
    assert_eq!(
        strips(&c.doc),
        vec![
            (StripId(0), StripKind::Master, "Master".into()),
            (StripId(1), StripKind::BuiltinSynth, BUILTIN_SYNTH_STRIP.into()),
        ]
    );
    // Removing every user keeps the strip (Plan Ruling 1).
    let d = run(
        &c.doc,
        RoutingCmd::RemoveDestination {
            dest: DestId(0),
            replacement: None,
        },
    )
    .doc;
    let d = run(
        &d,
        RoutingCmd::RemoveDestination {
            dest: DestId(1),
            replacement: None,
        },
    )
    .doc;
    assert_eq!(d.mixer().find_kind(StripKind::BuiltinSynth), Some(StripId(1)));
}

#[test]
fn plugin_destination_gets_an_instance_and_an_instrument_strip() {
    let a = add(&Document::new(), "Pad", DestKind::plugin("au:pad"));
    assert_eq!(
        a.report.created,
        vec![
            AnyId::Dest(DestId(0)),
            AnyId::Plugin(PluginInstanceId(0)),
            AnyId::Strip(StripId(1)),
        ]
    );
    assert_eq!(
        strips(&a.doc)[1],
        (StripId(1), StripKind::Instrument(DestId(0)), "Pad".into())
    );
    // The caller's slot ID is ignored: a second plugin gets ID 1.
    let b = add(&a.doc, "Lead", DestKind::plugin("au:lead"));
    assert!(b.doc.plugin_slot(PluginInstanceId(1)).is_some());
    assert_eq!(b.doc.counters().plugin, 2);
    // Renaming the destination renames its strip.
    let c = run(
        &b.doc,
        RoutingCmd::RenameDestination {
            dest: DestId(0),
            name: "Warm Pad".into(),
        },
    );
    assert_eq!(c.doc.mixer().strip(StripId(1)).unwrap().name, "Warm Pad");
}

#[test]
fn editing_plugin_kinds_keeps_or_replaces_the_instance() {
    let a = add(&Document::new(), "Pad", DestKind::plugin("au:pad")).doc;
    // Same plugin, new metadata: same instance.
    let mut same = DestKind::plugin("au:pad");
    if let DestKind::PluginInstrument { slot } = &mut same {
        slot.plugin.version = "2.0".into();
        slot.id = PluginInstanceId(77);
    }
    let b = run(
        &a,
        RoutingCmd::EditDestination {
            dest: DestId(0),
            kind: same,
            default_patch: None,
        },
    );
    let slot = b.doc.plugin_slot(PluginInstanceId(0)).unwrap();
    assert_eq!(slot.plugin.version, "2.0");
    assert!(b.report.created.is_empty());
    // Another plugin: a new instance, same strip.
    let c = run(
        &b.doc,
        RoutingCmd::EditDestination {
            dest: DestId(0),
            kind: DestKind::plugin("au:other"),
            default_patch: None,
        },
    );
    assert_eq!(c.report.created, vec![AnyId::Plugin(PluginInstanceId(1))]);
    assert_eq!(
        c.doc.plugin_slot(PluginInstanceId(1)).unwrap().plugin,
        PluginRef::from_legacy("au:other")
    );
    assert_eq!(c.doc.mixer().find_kind(StripKind::Instrument(DestId(0))), Some(StripId(1)));
    // Plugin → built-in endpoint: the instrument strip goes, the built-in strip comes.
    let d = run(
        &c.doc,
        RoutingCmd::EditDestination {
            dest: DestId(0),
            kind: endpoint("builtin:synth", 0),
            default_patch: None,
        },
    );
    assert_eq!(d.report.removed, vec![AnyId::Strip(StripId(1))]);
    assert_eq!(
        strips(&d.doc),
        vec![
            (StripId(0), StripKind::Master, "Master".into()),
            (StripId(2), StripKind::BuiltinSynth, BUILTIN_SYNTH_STRIP.into()),
        ]
    );
    // Endpoint → plugin: a new instrument strip.
    let e = run(
        &d.doc,
        RoutingCmd::EditDestination {
            dest: DestId(0),
            kind: DestKind::plugin("au:pad"),
            default_patch: None,
        },
    );
    assert_eq!(
        e.doc.mixer().find_kind(StripKind::Instrument(DestId(0))),
        Some(StripId(3))
    );
}

#[test]
fn instrument_slots_refuse_a_sidechain() {
    let mut k = DestKind::plugin("au:pad");
    if let DestKind::PluginInstrument { slot } = &mut k {
        slot.sidechain = Some(StripId(0));
    }
    let r = apply(
        &Document::new(),
        &Command::Routing(RoutingCmd::AddDestination {
            name: "Pad".into(),
            kind: k,
            default_patch: None,
        }),
    );
    assert!(r.is_err());
}

#[test]
fn output_paths_get_audio_path_strips() {
    let a = run(
        &Document::new(),
        RoutingCmd::AddOutput {
            name: "Main".into(),
            reference: "bus:0".into(),
        },
    );
    assert_eq!(
        a.report.created,
        vec![AnyId::Output(OutputPathId(0)), AnyId::Strip(StripId(1))]
    );
    let b = run(
        &a.doc,
        RoutingCmd::RenameOutput {
            output: OutputPathId(0),
            name: "Stems".into(),
        },
    );
    assert_eq!(b.doc.mixer().strip(StripId(1)).unwrap().name, "Stems");
    let c = run(
        &b.doc,
        RoutingCmd::RemoveOutput {
            output: OutputPathId(0),
            replacement: None,
        },
    );
    assert_eq!(c.report.removed, vec![AnyId::Output(OutputPathId(0)), AnyId::Strip(StripId(1))]);
    assert_eq!(c.doc.mixer().strips().len(), 1);
}

#[test]
fn removing_a_plugin_destination_is_one_undo_step() {
    let mut h = History::new(Document::new());
    let f = ViewFilter::default();
    h.execute(
        &Command::Routing(RoutingCmd::AddDestination {
            name: "Pad".into(),
            kind: DestKind::plugin("au:pad"),
            default_patch: None,
        }),
        &f,
    )
    .unwrap();
    let before = h.doc().clone();
    h.execute(
        &Command::Routing(RoutingCmd::RemoveDestination {
            dest: DestId(0),
            replacement: None,
        }),
        &f,
    )
    .unwrap();
    assert_eq!(h.doc().mixer().strips().len(), 1);
    assert!(h.doc().plugin_slot(PluginInstanceId(0)).is_none());
    h.undo().unwrap();
    assert_eq!(h.doc(), &before);
}
```

- [ ] **Step 2: Run the test to verify it fails**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test mixer_routing
```

Expected: FAIL — e.g. `builtin_destinations_share_one_strip` sees `created == [Dest(dest0)]` (no strip yet).

- [ ] **Step 3: Write the strip helpers**

In `crates/core-model/src/document.rs`, add to the crate-internal `impl Document` block (after `counters_mut`):

```rust
    pub(crate) fn mixer_mut(&mut self) -> &mut Mixer {
        &mut self.mixer
    }
```

In `crates/core-model/src/mixer.rs`, add to `impl Mixer` (after `inserts`):

```rust
    pub(crate) fn strip_mut(&mut self, id: StripId) -> Result<&mut Strip> {
        self.strips
            .get_mut(&id)
            .ok_or(ModelError::UnknownId(AnyId::Strip(id)))
    }
```

In `crates/core-model/src/mixer.rs`, add to `impl Mixer` (after `add`):

```rust
    /// Removes a strip (never master): outputs to it go to master, sends to
    /// it are dropped and sidechains from it are cleared.
    pub(crate) fn remove(&mut self, id: StripId) -> Option<Strip> {
        if id == self.master {
            return None;
        }
        let s = self.strips.remove(&id)?;
        self.order.retain(|x| *x != id);
        let ids: Vec<StripId> = self.strips.keys().copied().collect();
        for k in ids {
            if let Some(st) = self.strips.get_mut(&k) {
                if st.output == Output::Bus(id) {
                    st.output = Output::Master;
                }
                st.sends.retain(|x| x.target != id);
                for slot in st.inserts.iter_mut() {
                    if slot.sidechain == Some(id) {
                        slot.sidechain = None;
                    }
                }
            }
        }
        Some(s)
    }
```

Create `crates/core-model/src/command/strips.rs`:

```rust
//! Keeping the mixer in step with the instrument list and output paths
//! (spec §4: every plugin instrument and output path has exactly one
//! strip; Plan Ruling 1 for the built-in synth).

use super::Report;
use crate::document::Document;
use crate::ids::{AnyId, DestId, OutputPathId, StripId};
use crate::mixer::{BUILTIN_SYNTH_STRIP, Strip, StripKind};

fn add_strip(d: &mut Document, kind: StripKind, name: &str, r: &mut Report) -> StripId {
    let id = d.counters_mut().alloc_strip();
    d.mixer_mut().add(Strip::new(id, name, kind), None);
    r.created.push(AnyId::Strip(id));
    id
}

/// Adds the built-in synth strip when a destination uses the built-in
/// synth and the mixer has none.
pub(crate) fn ensure_builtin(d: &mut Document, r: &mut Report) {
    let used = d.instruments().values().any(|x| x.kind.is_builtin_synth());
    if used && d.mixer().find_kind(StripKind::BuiltinSynth).is_none() {
        add_strip(d, StripKind::BuiltinSynth, BUILTIN_SYNTH_STRIP, r);
    }
}

/// Adds plugin destination `dest`'s `Instrument` strip.
pub(crate) fn add_instrument_strip(d: &mut Document, dest: DestId, name: &str, r: &mut Report) {
    add_strip(d, StripKind::Instrument(dest), name, r);
}

/// Adds output path `out`'s `AudioPath` strip.
pub(crate) fn add_audio_path_strip(d: &mut Document, out: OutputPathId, name: &str, r: &mut Report) {
    add_strip(d, StripKind::AudioPath(out), name, r);
}

/// Removes every strip fed by `dest` (its `Instrument` and
/// `InstrumentOutput` strips).
pub(crate) fn remove_dest_strips(d: &mut Document, dest: DestId, r: &mut Report) {
    let ids: Vec<StripId> = d
        .mixer()
        .strips()
        .values()
        .filter(|s| {
            matches!(s.kind, StripKind::Instrument(x) | StripKind::InstrumentOutput(x, _) if x == dest)
        })
        .map(|s| s.id)
        .collect();
    for id in ids {
        if d.mixer_mut().remove(id).is_some() {
            r.removed.push(AnyId::Strip(id));
        }
    }
}

/// Removes the strip of `kind`, if any.
pub(crate) fn remove_kind(d: &mut Document, kind: StripKind, r: &mut Report) {
    if let Some(id) = d.mixer().find_kind(kind) {
        if d.mixer_mut().remove(id).is_some() {
            r.removed.push(AnyId::Strip(id));
        }
    }
}

/// Renames the strip of `kind`, if any.
pub(crate) fn rename_kind(d: &mut Document, kind: StripKind, name: &str) {
    if let Some(id) = d.mixer().find_kind(kind) {
        if let Ok(s) = d.mixer_mut().strip_mut(id) {
            s.name = name.to_string();
        }
    }
}
```

In `crates/core-model/src/command/mod.rs` add `mod strips;` after `mod select;`.

- [ ] **Step 4: Call the helpers from the routing commands**

In `crates/core-model/src/command/routing.rs` add `use super::strips;`, `use crate::mixer::StripKind;` and replace these match arms of `apply` (the others stay):

```rust
        RoutingCmd::AddDestination {
            name,
            kind,
            default_patch,
        } => {
            check_dest(kind, default_patch)?;
            let id = d.counters_mut().alloc_dest();
            r.created.push(AnyId::Dest(id));
            let mut kind = kind.clone();
            if let DestKind::PluginInstrument { slot } = &mut kind {
                slot.id = d.counters_mut().alloc_plugin();
                r.created.push(AnyId::Plugin(slot.id));
            }
            let plugin = matches!(kind, DestKind::PluginInstrument { .. });
            d.instruments_mut().insert(
                id,
                Destination {
                    id,
                    name: name.clone(),
                    kind,
                    default_patch: *default_patch,
                },
            );
            if plugin {
                strips::add_instrument_strip(d, id, name, r);
            }
            strips::ensure_builtin(d, r);
        }
        RoutingCmd::RenameDestination { dest, name } => {
            d.instruments_mut()
                .get_mut(dest)
                .ok_or(ModelError::UnknownId(AnyId::Dest(*dest)))?
                .name = name.clone();
            strips::rename_kind(d, StripKind::Instrument(*dest), name);
        }
        RoutingCmd::EditDestination {
            dest,
            kind,
            default_patch,
        } => {
            check_dest(kind, default_patch)?;
            let old = d
                .instruments()
                .get(dest)
                .ok_or(ModelError::UnknownId(AnyId::Dest(*dest)))?
                .clone();
            let mut kind = kind.clone();
            if let DestKind::PluginInstrument { slot } = &mut kind {
                match &old.kind {
                    DestKind::PluginInstrument { slot: o }
                        if o.plugin.format == slot.plugin.format && o.plugin.id == slot.plugin.id =>
                    {
                        let plugin = slot.plugin.clone();
                        *slot = o.clone();
                        slot.plugin = plugin;
                    }
                    _ => {
                        slot.id = d.counters_mut().alloc_plugin();
                        r.created.push(AnyId::Plugin(slot.id));
                    }
                }
            }
            let was = matches!(old.kind, DestKind::PluginInstrument { .. });
            let is = matches!(kind, DestKind::PluginInstrument { .. });
            let x = d
                .instruments_mut()
                .get_mut(dest)
                .ok_or(ModelError::UnknownId(AnyId::Dest(*dest)))?;
            x.kind = kind;
            x.default_patch = *default_patch;
            match (was, is) {
                (true, false) => strips::remove_dest_strips(d, *dest, r),
                (false, true) => strips::add_instrument_strip(d, *dest, &old.name, r),
                _ => {}
            }
            strips::ensure_builtin(d, r);
        }
        RoutingCmd::AddOutput { name, reference } => {
            let id = d.counters_mut().alloc_output();
            d.outputs_mut().insert(
                id,
                OutputPath {
                    id,
                    name: name.clone(),
                    reference: reference.clone(),
                },
            );
            r.created.push(AnyId::Output(id));
            strips::add_audio_path_strip(d, id, name, r);
        }
        RoutingCmd::RenameOutput { output, name } => {
            d.outputs_mut()
                .get_mut(output)
                .ok_or(ModelError::UnknownId(AnyId::Output(*output)))?
                .name = name.clone();
            strips::rename_kind(d, StripKind::AudioPath(*output), name);
        }
```

In the `RemoveOutput` arm, after `r.removed.push(AnyId::Output(*output));` add:

```rust
            strips::remove_kind(d, StripKind::AudioPath(*output), r);
```

In `remove_destination`, after `r.removed.push(AnyId::Dest(dest));` add:

```rust
    strips::remove_dest_strips(d, dest, r);
```

- [ ] **Step 5: Assert the demo's strips**

In `crates/core-engine/tests/demo.rs`, at the end of `demo_matches_the_app_shell_spec`, add (and `use core_model::mixer::StripKind;`):

```rust
    let strips: Vec<StripKind> = doc.mixer().strips().values().map(|s| s.kind).collect();
    assert_eq!(strips, vec![StripKind::Master, StripKind::BuiltinSynth]);
```

- [ ] **Step 6: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model
cargo test -p core-engine --test demo
```

Expected: all pass (`mixer_routing` 6 tests). Existing tests that compare `Report.created` for `AddDestination` only use `Dest` lookups, so they are unaffected.

- [ ] **Step 7: Regenerate the demo package, check the golden, run everything**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo run -q -p vizdaw-play -- --write-demo fixtures/demo.vizdaw
grep -c '"builtin_synth"' fixtures/demo.vizdaw/project.json
cargo run -q -p vizdaw-play -- fixtures/demo.vizdaw --seq Song --offline "$TMPDIR/demo.txt"
diff fixtures/demo.offline.txt "$TMPDIR/demo.txt" && echo "offline golden unchanged"
cargo test --workspace
```

Expected: `wrote fixtures/demo.vizdaw`, `1`, `offline golden unchanged`, everything passes.

- [ ] **Step 8: Lint and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
git add crates/core-model/src/mixer.rs crates/core-model/src/document.rs crates/core-model/src/command/strips.rs crates/core-model/src/command/mod.rs crates/core-model/src/command/routing.rs crates/core-model/tests/mixer_routing.rs crates/core-engine/tests/demo.rs fixtures/demo.vizdaw/project.json
git commit -m "feat(core-model): destination and output edits keep mixer strips in step

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---
### Task 4: Routing rules, validation and the solo set

**Goal:** Implement spec §4's rules: routing is a DAG over outputs, sends and sidechains; outputs target a bus or master, sends a bus only, nothing targets itself, master has no output or sends; every plugin instrument and output path has exactly one strip; values are in range. `validate` (run on every load) reports every mixer problem; `audible` computes the solo set.

**Files:**
- Create: `crates/core-model/src/mixer_rules.rs`
- Modify: `crates/core-model/src/lib.rs`
- Modify: `crates/core-model/src/rules.rs`
- Test: `crates/core-model/tests/mixer_rules.rs`

**Interfaces:**
- Consumes: Tasks 1–3 (`Mixer`, `Strip`, `StripKind`, `Output`, `Document::mixer`, `Counters`).
- Produces:
  - `mixer_rules::EdgeKind::{Output, Send(usize), Sidechain(PluginInstanceId)}`.
  - `mixer_rules::routing_edges(&Mixer) -> Vec<(StripId, StripId, EdgeKind)>` — outputs (non-master strips, `Output::Master` resolved to the master's ID), sends, then sidechain feeds (source → strip owning the insert).
  - `mixer_rules::find_routing_cycle(&Mixer) -> Option<Vec<StripId>>` (path's first element repeated at the end).
  - `mixer_rules::check_routing(&Mixer) -> Result<()>` — first problem as `InvalidRoutingTarget`, `UnknownId(Strip)` or `RoutingCycle`.
  - `mixer_rules::check_strip_values(&Strip) -> Result<()>`.
  - `mixer_rules::mixer_problems(&Document) -> Vec<String>` (used by `rules::validate`).
  - `mixer_rules::audible(&Mixer) -> BTreeSet<StripId>` (Plan Ruling 5).

- [ ] **Step 1: Write the failing test**

Create `crates/core-model/tests/mixer_rules.rs`:

```rust
use core_model::ModelError;
use core_model::document::{Document, DocumentRepr, Settings};
use core_model::ids::{Counters, PluginInstanceId, StripId};
use core_model::mixer::{AuxSend, Mixer, MixerRepr, Output, PluginRef, PluginSlot, Strip, StripKind};
use core_model::mixer_rules::{
    EdgeKind, audible, check_routing, find_routing_cycle, routing_edges,
};
use core_model::rules::validate;
use proptest::prelude::*;
use std::collections::BTreeSet;

/// Master 0 plus buses 1..=n, built from (output, sends, sidechain
/// sources) per bus.
fn buses(spec: &[(Option<u64>, Vec<u64>, Vec<u64>)]) -> Mixer {
    let mut strips = vec![Strip::new(StripId(0), "Master", StripKind::Master)];
    let mut plug = 0;
    for (i, (out, sends, scs)) in spec.iter().enumerate() {
        let id = StripId(i as u64 + 1);
        let mut s = Strip::new(id, format!("B{}", id.0), StripKind::Bus);
        s.output = out.map_or(Output::Master, |b| Output::Bus(StripId(b)));
        for t in sends {
            s.sends.push_back(AuxSend::new(StripId(*t)));
        }
        for src in scs {
            let mut slot = PluginSlot::new(PluginInstanceId(plug), PluginRef::from_legacy("clap:duck"));
            plug += 1;
            slot.sidechain = Some(StripId(*src));
            s.inserts.push_back(slot);
        }
        strips.push(s);
    }
    Mixer::try_from(MixerRepr {
        master: StripId(0),
        order: (1..=spec.len() as u64).map(StripId).collect(),
        strips,
    })
    .unwrap()
}

fn doc_with(m: Mixer, strips: u64, plugins: u64) -> Document {
    Document::from_parts(DocumentRepr {
        settings: Settings::default(),
        instruments: vec![],
        outputs: vec![],
        media: vec![],
        sequences: vec![],
        mixer: Some(m),
        counters: Counters {
            strip: strips,
            plugin: plugins,
            ..Counters::default()
        },
    })
    .unwrap()
}

#[test]
fn edges_list_outputs_sends_and_sidechains() {
    let m = buses(&[(Some(2), vec![2], vec![]), (None, vec![], vec![1])]);
    assert_eq!(
        routing_edges(&m),
        vec![
            (StripId(1), StripId(2), EdgeKind::Output),
            (StripId(1), StripId(2), EdgeKind::Send(0)),
            (StripId(2), StripId(0), EdgeKind::Output),
            (StripId(1), StripId(2), EdgeKind::Sidechain(PluginInstanceId(0))),
        ]
    );
    assert_eq!(find_routing_cycle(&m), None);
    assert_eq!(check_routing(&m), Ok(()));
}

#[test]
fn cycles_through_sends_and_sidechains_are_found() {
    // 1 → 2 (output), 2 → 1 (send).
    let m = buses(&[(Some(2), vec![], vec![]), (None, vec![1], vec![])]);
    let path = find_routing_cycle(&m).unwrap();
    assert_eq!(path.first(), path.last());
    assert!(matches!(check_routing(&m), Err(ModelError::RoutingCycle { .. })));
    // A sidechain from itself.
    let m = buses(&[(None, vec![], vec![1])]);
    assert_eq!(find_routing_cycle(&m), Some(vec![StripId(1), StripId(1)]));
    // A sidechain from master: master → 1 → master.
    let m = buses(&[(None, vec![], vec![0])]);
    assert!(find_routing_cycle(&m).is_some());
}

#[test]
fn invalid_targets_are_named() {
    let m = buses(&[(Some(1), vec![], vec![])]);
    assert_eq!(
        check_routing(&m),
        Err(ModelError::InvalidRoutingTarget {
            strip: StripId(1),
            target: StripId(1),
            reason: "a strip cannot output to itself".into(),
        })
    );
    let m = buses(&[(None, vec![0], vec![])]);
    assert_eq!(
        check_routing(&m),
        Err(ModelError::InvalidRoutingTarget {
            strip: StripId(1),
            target: StripId(0),
            reason: "a send must target a bus".into(),
        })
    );
    let m = buses(&[(Some(9), vec![], vec![])]);
    assert!(matches!(check_routing(&m), Err(ModelError::UnknownId(_))));
}

#[test]
fn validate_reports_mixer_problems() {
    let m = buses(&[(Some(2), vec![], vec![]), (Some(1), vec![], vec![])]);
    let problems = validate(&doc_with(m, 3, 0));
    assert!(problems.iter().any(|p| p.contains("routing cycle")), "{problems:?}");
    // Counters too low, a duplicate instance ID, a bad fader.
    let mut m = buses(&[(None, vec![], vec![1, 1])]);
    let mut r = MixerRepr::from(m.clone());
    r.strips[1].gain_db = 13.0;
    m = Mixer::try_from(r).unwrap();
    let problems = validate(&doc_with(m, 1, 0));
    assert!(problems.iter().any(|p| p.contains("counters.strip")), "{problems:?}");
    assert!(problems.iter().any(|p| p.contains("plugin.id")), "{problems:?}");
    assert!(problems.iter().any(|p| p.contains("gain_db")), "{problems:?}");
    // A valid document has none.
    assert!(validate(&Document::new()).is_empty());
}

#[test]
fn solo_keeps_upstream_and_downstream() {
    // 1 → 3 (output), 2 → master, 2 sends to 3; 3 → master.
    let mut r = MixerRepr::from(buses(&[
        (Some(3), vec![], vec![]),
        (None, vec![3], vec![]),
        (None, vec![], vec![]),
    ]));
    assert_eq!(audible(&Mixer::try_from(r.clone()).unwrap()).len(), 4);
    r.strips[1].solo = true;
    let set = audible(&Mixer::try_from(r.clone()).unwrap());
    assert_eq!(set, BTreeSet::from([StripId(0), StripId(1), StripId(3)]));
    r.strips[1].solo = false;
    r.strips[3].solo = true;
    let set = audible(&Mixer::try_from(r).unwrap());
    assert_eq!(set, BTreeSet::from([StripId(0), StripId(1), StripId(2), StripId(3)]));
}

/// Reachability by Floyd–Warshall over `edges` among `n` nodes.
fn reach(n: usize, edges: &[(usize, usize)]) -> Vec<Vec<bool>> {
    let mut r = vec![vec![false; n]; n];
    for &(a, b) in edges {
        r[a][b] = true;
    }
    for k in 0..n {
        for i in 0..n {
            for j in 0..n {
                if r[i][k] && r[k][j] {
                    r[i][j] = true;
                }
            }
        }
    }
    r
}

/// Per bus: output, sends, sidechain sources, soloed.
type BusSpec = (Option<u64>, Vec<u64>, Vec<u64>, bool);

fn arb_buses() -> impl Strategy<Value = Vec<BusSpec>> {
    (1usize..7).prop_flat_map(|n| {
        let id = 0..=(n as u64);
        proptest::collection::vec(
            (
                proptest::option::of(1..=(n as u64)),
                proptest::collection::vec(1..=(n as u64), 0..3),
                proptest::collection::vec(id.clone(), 0..2),
                any::<bool>(),
            ),
            n,
        )
    })
}

proptest! {
    #[test]
    fn cycle_check_matches_reference(spec in arb_buses()) {
        let plain: Vec<_> = spec.iter().map(|(o, s, c, _)| (*o, s.clone(), c.clone())).collect();
        let m = buses(&plain);
        let n = spec.len() + 1;
        let edges: Vec<(usize, usize)> = routing_edges(&m)
            .into_iter()
            .map(|(a, b, _)| (a.0 as usize, b.0 as usize))
            .collect();
        let r = reach(n, &edges);
        let cyclic = (0..n).any(|i| r[i][i]);
        match find_routing_cycle(&m) {
            None => prop_assert!(!cyclic),
            Some(path) => {
                prop_assert!(cyclic);
                prop_assert_eq!(path.first(), path.last());
                for w in path.windows(2) {
                    prop_assert!(edges.contains(&(w[0].0 as usize, w[1].0 as usize)));
                }
            }
        }
    }

    #[test]
    fn solo_set_matches_reference(spec in arb_buses()) {
        let plain: Vec<_> = spec.iter().map(|(o, s, c, _)| (*o, s.clone(), c.clone())).collect();
        let mut r = MixerRepr::from(buses(&plain));
        for (i, (_, _, _, solo)) in spec.iter().enumerate() {
            r.strips[i + 1].solo = *solo;
        }
        let m = Mixer::try_from(r).unwrap();
        let n = spec.len() + 1;
        let edges: Vec<(usize, usize)> = routing_edges(&m)
            .into_iter()
            .filter(|(_, _, k)| !matches!(k, EdgeKind::Sidechain(_)))
            .map(|(a, b, _)| (a.0 as usize, b.0 as usize))
            .collect();
        let reach = reach(n, &edges);
        let soloed: Vec<usize> = (1..n).filter(|i| spec[i - 1].3).collect();
        let want: BTreeSet<StripId> = (0..n)
            .filter(|&s| {
                soloed.is_empty()
                    || soloed.iter().any(|&t| t == s || reach[t][s] || reach[s][t])
            })
            .map(|s| StripId(s as u64))
            .collect();
        prop_assert_eq!(audible(&m), want);
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test mixer_rules
```

Expected: compile error — `unresolved import core_model::mixer_rules`.

- [ ] **Step 3: Write the rules**

Create `crates/core-model/src/mixer_rules.rs`:

```rust
//! Mixer rules (spec §4): routing is a DAG over outputs, sends and
//! sidechain feeds; targets are valid; every plugin instrument and output
//! path has exactly one strip; values are in range. Also the solo set.

use crate::document::{DestKind, Document};
use crate::error::{ModelError, Result};
use crate::ids::{AnyId, PluginInstanceId, StripId};
use crate::mixer::{GAIN_FLOOR_DB, GAIN_MAX_DB, Mixer, Output, Strip, StripKind};
use std::collections::{BTreeMap, BTreeSet};

/// What a routing edge is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EdgeKind {
    /// A strip's main output.
    Output,
    /// Send number `n` of the source strip.
    Send(usize),
    /// The source feeds this insert's sidechain input.
    Sidechain(PluginInstanceId),
}

/// Every routing edge `(from, to, kind)`: each non-master strip's output
/// (to master or a bus) and sends, in strip order, then every sidechain
/// feed (source → strip owning the insert).
pub fn routing_edges(m: &Mixer) -> Vec<(StripId, StripId, EdgeKind)> {
    let mut e = Vec::new();
    for s in m.strips().values() {
        if s.id != m.master() {
            let to = match s.output {
                Output::Master => m.master(),
                Output::Bus(b) => b,
            };
            e.push((s.id, to, EdgeKind::Output));
        }
        for (i, x) in s.sends.iter().enumerate() {
            e.push((s.id, x.target, EdgeKind::Send(i)));
        }
    }
    for s in m.strips().values() {
        for slot in s.inserts.iter() {
            if let Some(src) = slot.sidechain {
                e.push((src, s.id, EdgeKind::Sidechain(slot.id)));
            }
        }
    }
    e
}

/// A routing cycle, if any: the path's first strip is repeated at the end.
pub fn find_routing_cycle(m: &Mixer) -> Option<Vec<StripId>> {
    let mut adj: BTreeMap<StripId, Vec<StripId>> = BTreeMap::new();
    for (a, b, _) in routing_edges(m) {
        adj.entry(a).or_default().push(b);
    }
    #[derive(Clone, Copy, PartialEq)]
    enum Color {
        White,
        Grey,
        Black,
    }
    let mut color: BTreeMap<StripId, Color> =
        m.strips().keys().map(|k| (*k, Color::White)).collect();
    for &start in m.strips().keys() {
        if color.get(&start) != Some(&Color::White) {
            continue;
        }
        color.insert(start, Color::Grey);
        let mut stack: Vec<(StripId, usize)> = vec![(start, 0)];
        while let Some((node, i)) = stack.last().copied() {
            match adj.get(&node).and_then(|v| v.get(i)).copied() {
                None => {
                    color.insert(node, Color::Black);
                    stack.pop();
                }
                Some(next) => {
                    if let Some(top) = stack.last_mut() {
                        top.1 += 1;
                    }
                    match color.get(&next).copied() {
                        Some(Color::Grey) => {
                            let pos = stack.iter().position(|(s, _)| *s == next).unwrap_or(0);
                            let mut path: Vec<StripId> =
                                stack.get(pos..).unwrap_or(&[]).iter().map(|(s, _)| *s).collect();
                            path.push(next);
                            return Some(path);
                        }
                        Some(Color::White) => {
                            color.insert(next, Color::Grey);
                            stack.push((next, 0));
                        }
                        // Finished, or a strip that does not exist
                        // (reported by `check_routing`).
                        _ => {}
                    }
                }
            }
        }
    }
    None
}

fn invalid(strip: StripId, target: StripId, reason: &str) -> ModelError {
    ModelError::InvalidRoutingTarget {
        strip,
        target,
        reason: reason.to_string(),
    }
}

fn check_bus(m: &Mixer, from: StripId, to: StripId, what: &str) -> Result<()> {
    if from == to {
        return Err(invalid(from, to, &format!("a strip cannot {what} to itself")));
    }
    if m.strip(to)?.kind != StripKind::Bus {
        let article = if what == "send" { "a send" } else { "an output" };
        return Err(invalid(from, to, &format!("{article} must target a bus")));
    }
    Ok(())
}

/// The first routing problem: an invalid output or send target, an unknown
/// sidechain source, or a cycle.
pub fn check_routing(m: &Mixer) -> Result<()> {
    for s in m.strips().values() {
        if s.id == m.master() {
            if s.output != Output::Master {
                return Err(invalid(s.id, s.id, "master has no output"));
            }
            if let Some(x) = s.sends.front() {
                return Err(invalid(s.id, x.target, "master has no sends"));
            }
        }
        if let Output::Bus(b) = s.output {
            check_bus(m, s.id, b, "output")?;
        }
        for x in s.sends.iter() {
            check_bus(m, s.id, x.target, "send")?;
        }
        for slot in s.inserts.iter() {
            if let Some(src) = slot.sidechain {
                m.strip(src)?;
            }
        }
    }
    if let Some(path) = find_routing_cycle(m) {
        return Err(ModelError::RoutingCycle { path });
    }
    Ok(())
}

fn level(field: &'static str, db: f64) -> Result<()> {
    if db.is_finite() && (GAIN_FLOOR_DB..=GAIN_MAX_DB).contains(&db) {
        Ok(())
    } else {
        Err(ModelError::range(field, db))
    }
}

/// Fader, pan, send levels and overlay values in range.
pub fn check_strip_values(s: &Strip) -> Result<()> {
    level("gain_db", s.gain_db)?;
    if !(s.pan.is_finite() && (-1.0..=1.0).contains(&s.pan)) {
        return Err(ModelError::range("pan", s.pan));
    }
    for x in s.sends.iter() {
        level("send.level_db", x.level_db)?;
    }
    for slot in s.inserts.iter() {
        if slot.params.values().any(|v| !v.is_finite()) {
            return Err(ModelError::range("params", "non-finite value"));
        }
    }
    Ok(())
}

/// Every mixer problem of `doc`, for [`crate::rules::validate`].
pub fn mixer_problems(doc: &Document) -> Vec<String> {
    let m = doc.mixer();
    let c = doc.counters();
    let mut p: Vec<String> = Vec::new();
    let mut push = |ctx: &str, e: ModelError| p.push(format!("{ctx}: {e}"));
    if m.strips().get(&m.master()).map(|s| s.kind) != Some(StripKind::Master) {
        push("mixer", ModelError::invalid("the master strip is missing"));
    }
    let masters = m.strips().values().filter(|s| s.kind == StripKind::Master).count();
    if masters != 1 {
        push("mixer", ModelError::invalid(format!("{masters} master strips")));
    }
    let mut order: Vec<StripId> = m.order().iter().copied().collect();
    order.sort();
    let others: Vec<StripId> = m.strips().keys().copied().filter(|k| *k != m.master()).collect();
    if order != others {
        push(
            "mixer",
            ModelError::invalid("display order does not list every strip exactly once"),
        );
    }
    let mut kinds: BTreeMap<StripKind, StripId> = BTreeMap::new();
    let mut instances: BTreeSet<PluginInstanceId> = BTreeSet::new();
    for s in m.strips().values() {
        let ctx = s.id.to_string();
        if s.id.0 >= c.strip {
            push(&ctx, ModelError::range("counters.strip", s.id));
        }
        if let Err(e) = check_strip_values(s) {
            push(&ctx, e);
        }
        match s.kind {
            StripKind::Instrument(dest) | StripKind::InstrumentOutput(dest, _) => {
                let plugin = matches!(
                    doc.instruments().get(&dest).map(|d| &d.kind),
                    Some(DestKind::PluginInstrument { .. })
                );
                if !plugin {
                    push(&ctx, ModelError::invalid(format!("{dest} is not a plugin instrument")));
                }
                if s.kind == StripKind::InstrumentOutput(dest, 0) {
                    push(&ctx, ModelError::range("instrument_output", 0));
                }
            }
            StripKind::AudioPath(o) => {
                if !doc.outputs().contains_key(&o) {
                    push(&ctx, ModelError::UnknownId(AnyId::Output(o)));
                }
            }
            StripKind::BuiltinSynth | StripKind::Bus | StripKind::Master => {}
        }
        if !matches!(s.kind, StripKind::Bus | StripKind::Master) {
            if let Some(prev) = kinds.insert(s.kind, s.id) {
                push(&ctx, ModelError::invalid(format!("duplicate {:?} strip (also {prev})", s.kind)));
            }
        }
        for slot in s.inserts.iter() {
            if !instances.insert(slot.id) || slot.id.0 >= c.plugin {
                push(&ctx, ModelError::range("plugin.id", slot.id));
            }
        }
    }
    for d in doc.instruments().values() {
        let ctx = d.id.to_string();
        if let DestKind::PluginInstrument { slot } = &d.kind {
            if !kinds.contains_key(&StripKind::Instrument(d.id)) {
                push(&ctx, ModelError::invalid("plugin instrument has no instrument strip"));
            }
            if !instances.insert(slot.id) || slot.id.0 >= c.plugin {
                push(&ctx, ModelError::range("plugin.id", slot.id));
            }
            if slot.sidechain.is_some() {
                push(&ctx, ModelError::invalid("an instrument's own slot has a sidechain"));
            }
            if slot.params.values().any(|v| !v.is_finite()) {
                push(&ctx, ModelError::range("params", "non-finite value"));
            }
        }
    }
    if doc.instruments().values().any(|d| d.kind.is_builtin_synth())
        && !kinds.contains_key(&StripKind::BuiltinSynth)
    {
        push("mixer", ModelError::invalid("no built-in synth strip"));
    }
    for o in doc.outputs().keys() {
        if !kinds.contains_key(&StripKind::AudioPath(*o)) {
            push(&o.to_string(), ModelError::invalid("output path has no strip"));
        }
    }
    if let Err(e) = check_routing(m) {
        push("mixer", e);
    }
    p
}

/// Strips that sound under the current solo state (Plan Ruling 5):
/// every strip when nothing is soloed; otherwise the soloed strips and
/// every strip downstream or upstream of one through outputs and sends.
pub fn audible(m: &Mixer) -> BTreeSet<StripId> {
    let soloed: Vec<StripId> = m.strips().values().filter(|s| s.solo).map(|s| s.id).collect();
    if soloed.is_empty() {
        return m.strips().keys().copied().collect();
    }
    let mut down: BTreeMap<StripId, Vec<StripId>> = BTreeMap::new();
    let mut up: BTreeMap<StripId, Vec<StripId>> = BTreeMap::new();
    for (a, b, k) in routing_edges(m) {
        if !matches!(k, EdgeKind::Sidechain(_)) {
            down.entry(a).or_default().push(b);
            up.entry(b).or_default().push(a);
        }
    }
    let mut set: BTreeSet<StripId> = soloed.iter().copied().collect();
    for adj in [&down, &up] {
        let mut stack = soloed.clone();
        let mut seen: BTreeSet<StripId> = BTreeSet::new();
        while let Some(n) = stack.pop() {
            for x in adj.get(&n).into_iter().flatten() {
                if seen.insert(*x) {
                    set.insert(*x);
                    stack.push(*x);
                }
            }
        }
    }
    set
}
```

In `crates/core-model/src/lib.rs` add `pub mod mixer_rules;` after `pub mod mixer;`.

In `crates/core-model/src/rules.rs`, at the end of `validate` replace

```rust
    if let Some(path) = find_cycle(doc) {
        p.push(ModelError::Cycle { path }.to_string());
    }
    p
```

with

```rust
    if let Some(path) = find_cycle(doc) {
        p.push(ModelError::Cycle { path }.to_string());
    }
    p.extend(crate::mixer_rules::mixer_problems(doc));
    p
```

- [ ] **Step 4: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test mixer_rules
cargo test --workspace
```

Expected: `mixer_rules` 7 passed (two proptests at 256 cases each); the workspace passes. If an existing test's `validate(..)` now reports a mixer problem, the document under test was built with `from_parts` and plugin destinations whose slot IDs collide or exceed `counters.plugin` — fix that test's constructor as in Task 2 Step 6 item 3 (distinct `PluginInstanceId(i)` and `plugin: n`), never the rule.

- [ ] **Step 5: Lint and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
git add crates/core-model/src/mixer_rules.rs crates/core-model/src/lib.rs crates/core-model/src/rules.rs crates/core-model/tests/mixer_rules.rs
git commit -m "feat(core-model): routing DAG and target rules, mixer validation, solo set

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---
### Task 5: Mixer commands

**Goal:** Every mixer edit is an undoable `Command::Mixer(MixerCmd)`: buses, names, order, fader, pan, mute, solo, outputs, sends, inserts (add, remove, move, bypass, sidechain), plugin parameter gestures and extra instrument outputs. Every command re-checks routing and changes nothing on error.

**Files:**
- Modify: `crates/core-model/src/mixer.rs` (`Mixer::move_to`)
- Modify: `crates/core-model/src/document.rs` (`Document::slot_mut`)
- Create: `crates/core-model/src/command/mixer.rs`
- Modify: `crates/core-model/src/command/mod.rs`
- Test: `crates/core-model/tests/mixer_commands.rs`

**Interfaces:**
- Consumes: Task 2 `Document::find_slot`, `SlotOwner`; Task 3 `Document::mixer_mut`, `Mixer::{add, remove, strip_mut}`; Task 4 `mixer_rules::{check_routing, check_strip_values}`.
- Produces:
  - crate `Document::slot_mut(PluginInstanceId) -> Result<&mut PluginSlot>`; crate `Mixer::move_to(StripId, usize)`.
  - `command::MixerCmd` (`Debug + Clone + PartialEq`) with variants `AddBus { name, index: Option<usize> }`, `RemoveBus { strip }`, `RenameStrip { strip, name }`, `MoveStrip { strip, index }`, `SetGain { strip, gain_db }`, `SetPan { strip, pan }`, `SetMute { strip, mute }`, `SetSolo { strip, solo }`, `SetOutput { strip, output }`, `AddSend { strip, send: AuxSend }`, `EditSend { strip, index, send }`, `RemoveSend { strip, index }`, `AddInsert { strip, index: Option<usize>, plugin: PluginRef }`, `RemoveInsert { slot }`, `MoveInsert { slot, strip, index }`, `SetBypass { slot, bypass }`, `SetSidechain { slot, source: Option<StripId> }`, `SetPluginParams { slot, params: Vec<(ParamId, f64)> }`, `AddInstrumentOutput { dest, index: u16, name }`; `MixerCmd::name() -> &'static str`.
  - `Command::Mixer(MixerCmd)`; reports: `AddBus` → `created [Strip]`, `AddInsert` → `created [Plugin]`, `AddInstrumentOutput` → `created [Strip]`, `RemoveBus` → `removed [Strip]`, `RemoveInsert` → `removed [Plugin]`.

- [ ] **Step 1: Write the failing test**

Create `crates/core-model/tests/mixer_commands.rs`:

```rust
use core_model::ModelError;
use core_model::command::{Command, MixerCmd, RoutingCmd, apply};
use core_model::document::{DestKind, Document};
use core_model::history::History;
use core_model::ids::{AnyId, DestId, PluginInstanceId, StripId};
use core_model::mixer::{AuxSend, Output, ParamId, PluginRef, StripKind};
use core_model::mixer_rules::audible;
use core_model::rules::validate;
use core_model::selection::ViewFilter;
use proptest::prelude::*;

fn mx(d: &Document, c: MixerCmd) -> core_model::Result<Document> {
    apply(d, &Command::Mixer(c)).map(|a| a.doc)
}

/// master 0, builtin synth strip 1 (dest0), Pad instrument strip 2 (dest1,
/// plugin 0), audio path strip 3 (out0), buses 4 "A" and 5 "B".
fn base() -> Document {
    let mut d = Document::new();
    for c in [
        RoutingCmd::AddDestination {
            name: "Synth".into(),
            kind: DestKind::MidiEndpoint {
                endpoint_ref: "builtin:synth".into(),
                group: 0,
                channel: 0,
            },
            default_patch: None,
        },
        RoutingCmd::AddDestination {
            name: "Pad".into(),
            kind: DestKind::plugin("au:pad"),
            default_patch: None,
        },
        RoutingCmd::AddOutput {
            name: "Main".into(),
            reference: "bus:0".into(),
        },
    ] {
        d = apply(&d, &Command::Routing(c)).unwrap().doc;
    }
    for name in ["A", "B"] {
        d = mx(&d, MixerCmd::AddBus { name: name.into(), index: None }).unwrap();
    }
    d
}

fn plug(name: &str) -> PluginRef {
    PluginRef::from_legacy(name)
}

#[test]
fn base_layout() {
    let d = base();
    let kinds: Vec<StripKind> = d.mixer().strips().values().map(|s| s.kind).collect();
    assert_eq!(
        kinds,
        vec![
            StripKind::Master,
            StripKind::BuiltinSynth,
            StripKind::Instrument(DestId(1)),
            StripKind::AudioPath(core_model::ids::OutputPathId(0)),
            StripKind::Bus,
            StripKind::Bus,
        ]
    );
    assert!(validate(&d).is_empty(), "{:?}", validate(&d));
}

#[test]
fn faders_pans_and_names() {
    let d = base();
    let d = mx(&d, MixerCmd::SetGain { strip: StripId(1), gain_db: -6.0 }).unwrap();
    let d = mx(&d, MixerCmd::SetPan { strip: StripId(1), pan: -0.5 }).unwrap();
    let d = mx(&d, MixerCmd::RenameStrip { strip: StripId(4), name: "Verb".into() }).unwrap();
    let d = mx(&d, MixerCmd::MoveStrip { strip: StripId(5), index: 0 }).unwrap();
    let s = d.mixer().strip(StripId(1)).unwrap();
    assert_eq!((s.gain_db, s.pan), (-6.0, -0.5));
    assert_eq!(d.mixer().strip(StripId(4)).unwrap().name, "Verb");
    assert_eq!(d.mixer().order().front(), Some(&StripId(5)));
    assert!(matches!(
        mx(&d, MixerCmd::SetGain { strip: StripId(1), gain_db: 12.5 }),
        Err(ModelError::OutOfRange { .. })
    ));
    assert!(mx(&d, MixerCmd::SetPan { strip: StripId(1), pan: f64::NAN }).is_err());
    assert!(mx(&d, MixerCmd::MoveStrip { strip: StripId(0), index: 0 }).is_err());
}

#[test]
fn output_cycle_is_refused_and_changes_nothing() {
    let d = mx(&base(), MixerCmd::SetOutput { strip: StripId(4), output: Output::Bus(StripId(5)) }).unwrap();
    let r = apply(
        &d,
        &Command::Mixer(MixerCmd::SetOutput { strip: StripId(5), output: Output::Bus(StripId(4)) }),
    );
    assert!(matches!(r, Err(ModelError::RoutingCycle { .. })), "{r:?}");
    // A send that closes the same loop is refused too.
    let r = mx(&d, MixerCmd::AddSend { strip: StripId(5), send: AuxSend::new(StripId(4)) });
    assert!(matches!(r, Err(ModelError::RoutingCycle { .. })));
    // Targets: no sends to non-buses, no output for master.
    let r = mx(&d, MixerCmd::AddSend { strip: StripId(1), send: AuxSend::new(StripId(2)) });
    assert!(matches!(r, Err(ModelError::InvalidRoutingTarget { .. })));
    let r = mx(&d, MixerCmd::SetOutput { strip: StripId(0), output: Output::Bus(StripId(4)) });
    assert!(matches!(r, Err(ModelError::InvalidRoutingTarget { .. })));
}

#[test]
fn sends_add_edit_remove() {
    let d = mx(&base(), MixerCmd::AddSend { strip: StripId(1), send: AuxSend::new(StripId(4)) }).unwrap();
    let mut s = AuxSend::new(StripId(4));
    s.level_db = -12.0;
    s.pre_fader = true;
    let d = mx(&d, MixerCmd::EditSend { strip: StripId(1), index: 0, send: s }).unwrap();
    assert_eq!(d.mixer().strip(StripId(1)).unwrap().sends.front(), Some(&s));
    assert!(mx(&d, MixerCmd::EditSend { strip: StripId(1), index: 3, send: s }).is_err());
    let d = mx(&d, MixerCmd::RemoveSend { strip: StripId(1), index: 0 }).unwrap();
    assert!(d.mixer().strip(StripId(1)).unwrap().sends.is_empty());
}

#[test]
fn inserts_add_move_bypass_and_remove() {
    let a = apply(
        &base(),
        &Command::Mixer(MixerCmd::AddInsert { strip: StripId(1), index: None, plugin: plug("clap:eq") }),
    )
    .unwrap();
    assert_eq!(a.report.created, vec![AnyId::Plugin(PluginInstanceId(1))]);
    let d = mx(&a.doc, MixerCmd::AddInsert { strip: StripId(1), index: Some(0), plugin: plug("clap:comp") }).unwrap();
    let ids: Vec<PluginInstanceId> = d.mixer().strip(StripId(1)).unwrap().inserts.iter().map(|p| p.id).collect();
    assert_eq!(ids, vec![PluginInstanceId(2), PluginInstanceId(1)]);
    let d = mx(&d, MixerCmd::MoveInsert { slot: PluginInstanceId(2), strip: StripId(4), index: 9 }).unwrap();
    assert_eq!(d.mixer().strip(StripId(4)).unwrap().inserts.front().map(|p| p.id), Some(PluginInstanceId(2)));
    let d = mx(&d, MixerCmd::SetBypass { slot: PluginInstanceId(2), bypass: true }).unwrap();
    assert!(d.plugin_slot(PluginInstanceId(2)).unwrap().bypass);
    let r = apply(&d, &Command::Mixer(MixerCmd::RemoveInsert { slot: PluginInstanceId(2) })).unwrap();
    assert_eq!(r.report.removed, vec![AnyId::Plugin(PluginInstanceId(2))]);
    // The instrument's own slot is not an insert.
    assert!(mx(&d, MixerCmd::RemoveInsert { slot: PluginInstanceId(0) }).is_err());
    assert!(mx(&d, MixerCmd::SetSidechain { slot: PluginInstanceId(0), source: Some(StripId(1)) }).is_err());
    assert!(matches!(
        mx(&d, MixerCmd::SetBypass { slot: PluginInstanceId(42), bypass: true }),
        Err(ModelError::UnknownId(AnyId::Plugin(PluginInstanceId(42))))
    ));
}

#[test]
fn sidechain_cycles_are_refused() {
    let d = mx(&base(), MixerCmd::AddInsert { strip: StripId(4), index: None, plugin: plug("clap:duck") }).unwrap();
    let p = PluginInstanceId(1);
    let d2 = mx(&d, MixerCmd::SetSidechain { slot: p, source: Some(StripId(1)) }).unwrap();
    assert_eq!(d2.plugin_slot(p).unwrap().sidechain, Some(StripId(1)));
    for bad in [StripId(4), StripId(0)] {
        let r = mx(&d, MixerCmd::SetSidechain { slot: p, source: Some(bad) });
        assert!(matches!(r, Err(ModelError::RoutingCycle { .. })), "{bad}: {r:?}");
    }
    assert!(matches!(
        mx(&d, MixerCmd::SetSidechain { slot: p, source: Some(StripId(99)) }),
        Err(ModelError::UnknownId(_))
    ));
}

#[test]
fn removing_a_bus_retargets_and_undo_restores() {
    let mut h = History::new(base());
    let f = ViewFilter::default();
    for c in [
        MixerCmd::SetOutput { strip: StripId(1), output: Output::Bus(StripId(4)) },
        MixerCmd::AddSend { strip: StripId(2), send: AuxSend::new(StripId(4)) },
        MixerCmd::AddInsert { strip: StripId(5), index: None, plugin: plug("clap:duck") },
        MixerCmd::SetSidechain { slot: PluginInstanceId(1), source: Some(StripId(4)) },
    ] {
        h.execute(&Command::Mixer(c), &f).unwrap();
    }
    let before = h.doc().clone();
    let r = h.execute(&Command::Mixer(MixerCmd::RemoveBus { strip: StripId(4) }), &f).unwrap();
    assert_eq!(r.removed, vec![AnyId::Strip(StripId(4))]);
    let m = h.doc().mixer();
    assert_eq!(m.strip(StripId(1)).unwrap().output, Output::Master);
    assert!(m.strip(StripId(2)).unwrap().sends.is_empty());
    assert_eq!(h.doc().plugin_slot(PluginInstanceId(1)).unwrap().sidechain, None);
    assert!(validate(h.doc()).is_empty());
    h.undo().unwrap();
    assert_eq!(h.doc(), &before);
    // Only buses are removed directly.
    assert!(mx(&before, MixerCmd::RemoveBus { strip: StripId(1) }).is_err());
}

#[test]
fn plugin_params_gesture_is_one_undo_step() {
    let mut h = History::new(base());
    let f = ViewFilter::default();
    let depth = h.undo_depth();
    h.execute(
        &Command::Mixer(MixerCmd::SetPluginParams {
            slot: PluginInstanceId(0),
            params: vec![(ParamId(1), 0.25), (ParamId(7), 1.0), (ParamId(1), 0.5)],
        }),
        &f,
    )
    .unwrap();
    assert_eq!(h.undo_depth(), depth + 1);
    let slot = h.doc().plugin_slot(PluginInstanceId(0)).unwrap();
    assert_eq!(slot.params.get(&ParamId(1)), Some(&0.5));
    assert_eq!(slot.params.get(&ParamId(7)), Some(&1.0));
    assert!(
        apply(
            h.doc(),
            &Command::Mixer(MixerCmd::SetPluginParams { slot: PluginInstanceId(0), params: vec![(ParamId(2), f64::INFINITY)] })
        )
        .is_err()
    );
    h.undo().unwrap();
    assert!(h.doc().plugin_slot(PluginInstanceId(0)).unwrap().params.is_empty());
}

#[test]
fn instrument_outputs_and_solo() {
    let a = apply(
        &base(),
        &Command::Mixer(MixerCmd::AddInstrumentOutput { dest: DestId(1), index: 1, name: "Pad 3-4".into() }),
    )
    .unwrap();
    assert_eq!(a.report.created, vec![AnyId::Strip(StripId(6))]);
    assert_eq!(a.doc.mixer().strip(StripId(6)).unwrap().kind, StripKind::InstrumentOutput(DestId(1), 1));
    for bad in [(DestId(1), 1), (DestId(1), 0), (DestId(0), 1)] {
        assert!(mx(&a.doc, MixerCmd::AddInstrumentOutput { dest: bad.0, index: bad.1, name: "x".into() }).is_err());
    }
    let d = mx(&a.doc, MixerCmd::SetSolo { strip: StripId(1), solo: true }).unwrap();
    let d = mx(&d, MixerCmd::SetMute { strip: StripId(2), mute: true }).unwrap();
    assert!(d.mixer().strip(StripId(2)).unwrap().mute);
    let set = audible(d.mixer());
    assert!(set.contains(&StripId(1)) && set.contains(&StripId(0)) && !set.contains(&StripId(4)));
}

fn arb_cmd() -> impl Strategy<Value = MixerCmd> {
    let strip = (0u64..8).prop_map(StripId);
    let slot = (0u64..5).prop_map(PluginInstanceId);
    let out = prop_oneof![Just(Output::Master), (0u64..8).prop_map(|b| Output::Bus(StripId(b)))];
    prop_oneof![
        (0usize..4).prop_map(|i| MixerCmd::AddBus { name: format!("Bus{i}"), index: Some(i) }),
        strip.clone().prop_map(|strip| MixerCmd::RemoveBus { strip }),
        (strip.clone(), -150.0f64..14.0).prop_map(|(strip, gain_db)| MixerCmd::SetGain { strip, gain_db }),
        (strip.clone(), -1.2f64..1.2).prop_map(|(strip, pan)| MixerCmd::SetPan { strip, pan }),
        (strip.clone(), any::<bool>()).prop_map(|(strip, mute)| MixerCmd::SetMute { strip, mute }),
        (strip.clone(), any::<bool>()).prop_map(|(strip, solo)| MixerCmd::SetSolo { strip, solo }),
        (strip.clone(), out).prop_map(|(strip, output)| MixerCmd::SetOutput { strip, output }),
        (strip.clone(), 0u64..8, any::<bool>()).prop_map(|(strip, t, pre)| {
            let mut send = AuxSend::new(StripId(t));
            send.pre_fader = pre;
            MixerCmd::AddSend { strip, send }
        }),
        (strip.clone(), 0usize..2).prop_map(|(strip, index)| MixerCmd::RemoveSend { strip, index }),
        strip.clone().prop_map(|strip| MixerCmd::AddInsert { strip, index: None, plugin: PluginRef::from_legacy("clap:fx") }),
        slot.clone().prop_map(|slot| MixerCmd::RemoveInsert { slot }),
        (slot.clone(), strip.clone(), 0usize..3).prop_map(|(slot, strip, index)| MixerCmd::MoveInsert { slot, strip, index }),
        (slot.clone(), any::<bool>()).prop_map(|(slot, bypass)| MixerCmd::SetBypass { slot, bypass }),
        (slot.clone(), proptest::option::of(strip.clone())).prop_map(|(slot, source)| MixerCmd::SetSidechain { slot, source }),
        (slot, 0u64..4, -1.0f64..1.0).prop_map(|(slot, p, v)| MixerCmd::SetPluginParams { slot, params: vec![(ParamId(p), v)] }),
        (strip, 0usize..8).prop_map(|(strip, index)| MixerCmd::MoveStrip { strip, index }),
    ]
}

proptest! {
    #[test]
    fn mixer_commands_undo_to_the_original(cmds in proptest::collection::vec(arb_cmd(), 1..40)) {
        let original = base();
        let mut h = History::new(original.clone());
        let f = ViewFilter::default();
        for c in &cmds {
            let _ = h.execute(&Command::Mixer(c.clone()), &f);
            prop_assert!(validate(h.doc()).is_empty(), "{:?} after {:?}", validate(h.doc()), c);
        }
        let after = h.doc().clone();
        while h.undo().is_ok() {}
        prop_assert_eq!(h.doc(), &original);
        while h.redo().is_ok() {}
        prop_assert_eq!(h.doc(), &after);
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test mixer_commands
```

Expected: compile error — no `MixerCmd` in `core_model::command`.

- [ ] **Step 3: Write the commands**

In `crates/core-model/src/document.rs`, add to the crate-internal `impl Document` block (after `mixer_mut`):

```rust
    /// Mutable slot of plugin instance `id`.
    pub(crate) fn slot_mut(&mut self, id: PluginInstanceId) -> Result<&mut PluginSlot> {
        let unknown = ModelError::UnknownId(AnyId::Plugin(id));
        match self.find_slot(id).ok_or(unknown.clone())? {
            SlotOwner::Instrument(d) => match self.instruments.get_mut(&d).map(|x| &mut x.kind) {
                Some(DestKind::PluginInstrument { slot }) => Ok(slot),
                _ => Err(unknown),
            },
            SlotOwner::Insert(s, i) => self.mixer.strip_mut(s)?.inserts.get_mut(i).ok_or(unknown),
        }
    }
```

In `crates/core-model/src/mixer.rs`, add to `impl Mixer` (after `add`):

```rust
    /// Moves a strip to display position `index` (clamped).
    pub(crate) fn move_to(&mut self, id: StripId, index: usize) {
        self.order.retain(|x| *x != id);
        let i = index.min(self.order.len());
        self.order.insert(i, id);
    }
```

Create `crates/core-model/src/command/mixer.rs`:

```rust
//! Mixer edits (spec §4). Every command re-checks routing; on error the
//! document is unchanged (commands work on a copy).

use super::Report;
use crate::document::{DestKind, Document, SlotOwner};
use crate::error::{ModelError, Result};
use crate::ids::{AnyId, DestId, PluginInstanceId, StripId};
use crate::mixer::{AuxSend, Output, ParamId, PluginRef, PluginSlot, Strip, StripKind};
use crate::mixer_rules::{check_routing, check_strip_values};

/// Mixer edits.
#[derive(Debug, Clone, PartialEq)]
pub enum MixerCmd {
    /// Add an aux bus at display position `index` (default: the end).
    AddBus {
        /// Name.
        name: String,
        /// Display position.
        index: Option<usize>,
    },
    /// Remove a bus: outputs to it go to master, sends to it are dropped,
    /// sidechains from it are cleared.
    RemoveBus {
        /// The bus.
        strip: StripId,
    },
    /// Rename a strip.
    RenameStrip {
        /// Target.
        strip: StripId,
        /// New name.
        name: String,
    },
    /// Move a strip (not master) in the display order.
    MoveStrip {
        /// Target.
        strip: StripId,
        /// New position (clamped).
        index: usize,
    },
    /// Set the fader.
    SetGain {
        /// Target.
        strip: StripId,
        /// dB, −144 (−∞) … +12.
        gain_db: f64,
    },
    /// Set the pan.
    SetPan {
        /// Target.
        strip: StripId,
        /// −1 … +1.
        pan: f64,
    },
    /// Mute or unmute.
    SetMute {
        /// Target.
        strip: StripId,
        /// Muted.
        mute: bool,
    },
    /// Solo or unsolo.
    SetSolo {
        /// Target.
        strip: StripId,
        /// Soloed.
        solo: bool,
    },
    /// Route a strip's main output.
    SetOutput {
        /// Target.
        strip: StripId,
        /// Master or a bus.
        output: Output,
    },
    /// Append a send.
    AddSend {
        /// Source strip.
        strip: StripId,
        /// The send.
        send: AuxSend,
    },
    /// Replace send `index`.
    EditSend {
        /// Source strip.
        strip: StripId,
        /// Send position.
        index: usize,
        /// The new send.
        send: AuxSend,
    },
    /// Remove send `index`.
    RemoveSend {
        /// Source strip.
        strip: StripId,
        /// Send position.
        index: usize,
    },
    /// Insert a plugin at `index` (default: the end) with a new instance ID.
    AddInsert {
        /// Strip.
        strip: StripId,
        /// Position.
        index: Option<usize>,
        /// The plugin.
        plugin: PluginRef,
    },
    /// Remove an insert.
    RemoveInsert {
        /// The instance.
        slot: PluginInstanceId,
    },
    /// Move an insert to `strip` at `index` (clamped); it keeps its ID.
    MoveInsert {
        /// The instance.
        slot: PluginInstanceId,
        /// Destination strip.
        strip: StripId,
        /// Destination position.
        index: usize,
    },
    /// Bypass or activate a plugin.
    SetBypass {
        /// The instance.
        slot: PluginInstanceId,
        /// Bypassed.
        bypass: bool,
    },
    /// Route a strip's post-fader output to an insert's sidechain input.
    SetSidechain {
        /// The insert.
        slot: PluginInstanceId,
        /// Source strip, or none.
        source: Option<StripId>,
    },
    /// One parameter gesture: values go into the slot's overlay.
    SetPluginParams {
        /// The instance.
        slot: PluginInstanceId,
        /// `(parameter, value)` in gesture order (later wins).
        params: Vec<(ParamId, f64)>,
    },
    /// Add a strip for extra output `index` (≥ 1) of a plugin instrument.
    AddInstrumentOutput {
        /// The plugin instrument destination.
        dest: DestId,
        /// Output index (0 is the main output's `Instrument` strip).
        index: u16,
        /// Strip name.
        name: String,
    },
}

impl MixerCmd {
    /// Stable command name (undo label).
    pub fn name(&self) -> &'static str {
        match self {
            MixerCmd::AddBus { .. } => "Add Bus",
            MixerCmd::RemoveBus { .. } => "Remove Bus",
            MixerCmd::RenameStrip { .. } => "Rename Strip",
            MixerCmd::MoveStrip { .. } => "Move Strip",
            MixerCmd::SetGain { .. } => "Set Fader",
            MixerCmd::SetPan { .. } => "Set Pan",
            MixerCmd::SetMute { .. } => "Mute",
            MixerCmd::SetSolo { .. } => "Solo",
            MixerCmd::SetOutput { .. } => "Set Output",
            MixerCmd::AddSend { .. } => "Add Send",
            MixerCmd::EditSend { .. } => "Edit Send",
            MixerCmd::RemoveSend { .. } => "Remove Send",
            MixerCmd::AddInsert { .. } => "Add Plugin",
            MixerCmd::RemoveInsert { .. } => "Remove Plugin",
            MixerCmd::MoveInsert { .. } => "Move Plugin",
            MixerCmd::SetBypass { .. } => "Bypass Plugin",
            MixerCmd::SetSidechain { .. } => "Set Sidechain",
            MixerCmd::SetPluginParams { .. } => "Change Plugin Parameter",
            MixerCmd::AddInstrumentOutput { .. } => "Add Instrument Output",
        }
    }
}

fn insert_owner(d: &Document, slot: PluginInstanceId) -> Result<(StripId, usize)> {
    match d.find_slot(slot) {
        Some(SlotOwner::Insert(s, i)) => Ok((s, i)),
        Some(SlotOwner::Instrument(_)) => Err(ModelError::invalid(
            "an instrument's own slot is not an insert",
        )),
        None => Err(ModelError::UnknownId(AnyId::Plugin(slot))),
    }
}

pub(crate) fn apply(d: &mut Document, cmd: &MixerCmd, r: &mut Report) -> Result<()> {
    match cmd {
        MixerCmd::AddBus { name, index } => {
            let id = d.counters_mut().alloc_strip();
            d.mixer_mut().add(Strip::new(id, name.clone(), StripKind::Bus), *index);
            r.created.push(AnyId::Strip(id));
        }
        MixerCmd::RemoveBus { strip } => {
            if d.mixer().strip(*strip)?.kind != StripKind::Bus {
                return Err(ModelError::invalid("only buses are removed directly"));
            }
            d.mixer_mut().remove(*strip);
            r.removed.push(AnyId::Strip(*strip));
        }
        MixerCmd::RenameStrip { strip, name } => {
            d.mixer_mut().strip_mut(*strip)?.name = name.clone();
        }
        MixerCmd::MoveStrip { strip, index } => {
            d.mixer().strip(*strip)?;
            if *strip == d.mixer().master() {
                return Err(ModelError::invalid("master is not in the display order"));
            }
            d.mixer_mut().move_to(*strip, *index);
        }
        MixerCmd::SetGain { strip, gain_db } => {
            let s = d.mixer_mut().strip_mut(*strip)?;
            s.gain_db = *gain_db;
            check_strip_values(s)?;
        }
        MixerCmd::SetPan { strip, pan } => {
            let s = d.mixer_mut().strip_mut(*strip)?;
            s.pan = *pan;
            check_strip_values(s)?;
        }
        MixerCmd::SetMute { strip, mute } => d.mixer_mut().strip_mut(*strip)?.mute = *mute,
        MixerCmd::SetSolo { strip, solo } => d.mixer_mut().strip_mut(*strip)?.solo = *solo,
        MixerCmd::SetOutput { strip, output } => {
            d.mixer_mut().strip_mut(*strip)?.output = *output;
        }
        MixerCmd::AddSend { strip, send } => {
            let s = d.mixer_mut().strip_mut(*strip)?;
            s.sends.push_back(*send);
            check_strip_values(s)?;
        }
        MixerCmd::EditSend { strip, index, send } => {
            let s = d.mixer_mut().strip_mut(*strip)?;
            *s.sends
                .get_mut(*index)
                .ok_or(ModelError::range("send", index))? = *send;
            check_strip_values(s)?;
        }
        MixerCmd::RemoveSend { strip, index } => {
            let s = d.mixer_mut().strip_mut(*strip)?;
            if *index >= s.sends.len() {
                return Err(ModelError::range("send", index));
            }
            s.sends.remove(*index);
        }
        MixerCmd::AddInsert {
            strip,
            index,
            plugin,
        } => {
            d.mixer().strip(*strip)?;
            let id = d.counters_mut().alloc_plugin();
            let s = d.mixer_mut().strip_mut(*strip)?;
            let i = index.unwrap_or(s.inserts.len()).min(s.inserts.len());
            s.inserts.insert(i, PluginSlot::new(id, plugin.clone()));
            r.created.push(AnyId::Plugin(id));
        }
        MixerCmd::RemoveInsert { slot } => {
            let (s, i) = insert_owner(d, *slot)?;
            d.mixer_mut().strip_mut(s)?.inserts.remove(i);
            r.removed.push(AnyId::Plugin(*slot));
        }
        MixerCmd::MoveInsert { slot, strip, index } => {
            d.mixer().strip(*strip)?;
            let (s, i) = insert_owner(d, *slot)?;
            let p = d.mixer_mut().strip_mut(s)?.inserts.remove(i);
            let t = d.mixer_mut().strip_mut(*strip)?;
            let at = (*index).min(t.inserts.len());
            t.inserts.insert(at, p);
        }
        MixerCmd::SetBypass { slot, bypass } => d.slot_mut(*slot)?.bypass = *bypass,
        MixerCmd::SetSidechain { slot, source } => {
            insert_owner(d, *slot)?;
            d.slot_mut(*slot)?.sidechain = *source;
        }
        MixerCmd::SetPluginParams { slot, params } => {
            if params.iter().any(|(_, v)| !v.is_finite()) {
                return Err(ModelError::range("params", "non-finite value"));
            }
            let s = d.slot_mut(*slot)?;
            for (p, v) in params {
                s.params.insert(*p, *v);
            }
        }
        MixerCmd::AddInstrumentOutput { dest, index, name } => {
            let plugin = matches!(
                d.instruments().get(dest).map(|x| &x.kind),
                Some(DestKind::PluginInstrument { .. })
            );
            if !plugin {
                return Err(ModelError::invalid(format!("{dest} is not a plugin instrument")));
            }
            if *index == 0 {
                return Err(ModelError::range("instrument_output", index));
            }
            let kind = StripKind::InstrumentOutput(*dest, *index);
            if d.mixer().find_kind(kind).is_some() {
                return Err(ModelError::invalid("that output already has a strip"));
            }
            let id = d.counters_mut().alloc_strip();
            d.mixer_mut().add(Strip::new(id, name.clone(), kind), None);
            r.created.push(AnyId::Strip(id));
        }
    }
    check_routing(d.mixer())
}
```

In `crates/core-model/src/command/mod.rs`: add `pub mod mixer;` after `pub mod lane;`, `pub use mixer::MixerCmd;` after `pub use lane::LaneCmd;`, a variant to `Command` after `Routing`:

```rust
    /// Mixer edits.
    Mixer(MixerCmd),
```

the arm `Command::Mixer(c) => c.name(),` in `Command::name`, and in `apply_in`:

```rust
        Command::Mixer(c) => {
            mixer::apply(&mut d, c, &mut report)?;
            None
        }
```

- [ ] **Step 4: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test mixer_commands
cargo test --workspace
```

Expected: `mixer_commands` 10 passed; workspace passes.

- [ ] **Step 5: Lint and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
git add crates/core-model/src/mixer.rs crates/core-model/src/document.rs crates/core-model/src/command/mixer.rs crates/core-model/src/command/mod.rs crates/core-model/tests/mixer_commands.rs
git commit -m "feat(core-model): undoable mixer commands with routing checks

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---
### Task 6: Plugin state blobs and capture

**Goal:** Store plugin state as immutable, content-addressed blobs in the package's `PluginState/` folder (spec §4), keep their bytes in memory with the document, and implement capture (store a blob, point the slot at it, clear its overlay) without an undo step.

**Files:**
- Create: `crates/core-model/src/state.rs`
- Modify: `crates/core-model/src/lib.rs`
- Modify: `crates/core-model/src/document.rs`
- Modify: `crates/core-model/src/history.rs`
- Modify: `crates/core-model/src/file.rs`
- Test: `crates/core-model/tests/plugin_state.rs`

**Interfaces:**
- Consumes: Task 2 `Document::{slot_mut, mixer, instruments}`, `PluginSlot::{state, params}`, `StateBlobId`.
- Produces:
  - `state::{STATE_DIR = "PluginState", blob_id(&[u8]) -> StateBlobId, blob_file_name(StateBlobId) -> String, parse_blob_file_name(&str) -> Option<StateBlobId>, capture(&Document, PluginInstanceId, &[u8]) -> Result<(Document, StateBlobId)>, states_in_json(&str) -> BTreeSet<StateBlobId>}`.
  - `Document::{state_blob(StateBlobId) -> Option<&[u8]>, referenced_states() -> BTreeSet<StateBlobId>, missing_states() -> Vec<StateBlobId>}`; crate `Document::states_mut() -> &mut OrdMap<StateBlobId, Arc<[u8]>>`. Blobs are not serialized; `PartialEq` compares them.
  - `History::capture_state(&mut self, slot: PluginInstanceId, bytes: &[u8]) -> Result<StateBlobId>`.
  - `file::save`/`file::autosave` write referenced blobs; `save` prunes unreferenced ones (keeping autosave references); `file::load` reads them.

- [ ] **Step 1: Write the failing test**

Create `crates/core-model/tests/plugin_state.rs`:

```rust
use core_model::command::{Command, MixerCmd, RoutingCmd, apply};
use core_model::document::{DestKind, Document};
use core_model::file::{autosave, load, save};
use core_model::history::{History, Timestamp};
use core_model::ids::{PluginInstanceId, StateBlobId};
use core_model::mixer::ParamId;
use core_model::selection::ViewFilter;
use core_model::state::{blob_file_name, blob_id, capture, parse_blob_file_name, states_in_json};
use std::path::PathBuf;

fn temp_package(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let p = std::env::temp_dir().join(format!(
        "vizdaw-state-{tag}-{}-{nanos}.vizdaw",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&p);
    p
}

/// One plugin instrument (instance 0).
fn pad() -> Document {
    apply(
        &Document::new(),
        &Command::Routing(RoutingCmd::AddDestination {
            name: "Pad".into(),
            kind: DestKind::plugin("au:pad"),
            default_patch: None,
        }),
    )
    .unwrap()
    .doc
}

#[test]
fn blob_ids_are_content_hashes() {
    assert_eq!(blob_id(b"abc"), blob_id(b"abc"));
    assert_ne!(blob_id(b"abc"), blob_id(b"abd"));
    // FNV-1a 64 of the empty input is the offset basis.
    assert_eq!(blob_id(b""), StateBlobId(0xcbf2_9ce4_8422_2325));
    let name = blob_file_name(StateBlobId(0xab));
    assert_eq!(name, "00000000000000ab.bin");
    assert_eq!(parse_blob_file_name(&name), Some(StateBlobId(0xab)));
    assert_eq!(parse_blob_file_name("notes.txt"), None);
}

#[test]
fn capture_sets_state_and_clears_the_overlay_without_an_undo_step() {
    let mut h = History::new(pad());
    let f = ViewFilter::default();
    h.execute(
        &Command::Mixer(MixerCmd::SetPluginParams {
            slot: PluginInstanceId(0),
            params: vec![(ParamId(1), 0.5)],
        }),
        &f,
    )
    .unwrap();
    let depth = h.undo_depth();
    let id = h.capture_state(PluginInstanceId(0), b"state-1").unwrap();
    assert_eq!(h.undo_depth(), depth);
    let slot = h.doc().plugin_slot(PluginInstanceId(0)).unwrap();
    assert_eq!(slot.state, Some(id));
    assert!(slot.params.is_empty());
    assert_eq!(h.doc().state_blob(id), Some(&b"state-1"[..]));
    assert!(h.doc().missing_states().is_empty());
    // A second capture replaces the blob and forgets the first.
    let id2 = h.capture_state(PluginInstanceId(0), b"state-2").unwrap();
    assert_ne!(id, id2);
    assert_eq!(h.doc().state_blob(id), None);
    // Unknown slots and open transactions are refused.
    assert!(h.capture_state(PluginInstanceId(9), b"x").is_err());
    h.begin("gesture").unwrap();
    assert!(h.capture_state(PluginInstanceId(0), b"x").is_err());
    h.abort().unwrap();
    // The pure form leaves its input alone.
    let d = pad();
    let (d2, id3) = capture(&d, PluginInstanceId(0), b"z").unwrap();
    assert!(d.plugin_slot(PluginInstanceId(0)).unwrap().state.is_none());
    assert_eq!(d2.plugin_slot(PluginInstanceId(0)).unwrap().state, Some(id3));
}

#[test]
fn save_writes_blobs_and_load_reads_them() {
    let pkg = temp_package("save");
    let (d, id) = capture(&pad(), PluginInstanceId(0), b"\x00\x01binary\xff").unwrap();
    save(&d, &pkg).unwrap();
    let file = pkg.join("PluginState").join(blob_file_name(id));
    assert_eq!(std::fs::read(&file).unwrap(), b"\x00\x01binary\xff");
    let text = std::fs::read_to_string(pkg.join("project.json")).unwrap();
    assert!(!text.contains("binary"), "blobs are not inline");
    assert_eq!(states_in_json(&text), [id].into_iter().collect());
    assert_eq!(load(&pkg).unwrap(), d);
    std::fs::remove_dir_all(&pkg).unwrap();
}

#[test]
fn unreferenced_blobs_go_on_save_but_autosave_references_stay() {
    let pkg = temp_package("prune");
    let (d1, id1) = capture(&pad(), PluginInstanceId(0), b"one").unwrap();
    autosave(&d1, &pkg, Timestamp(1000)).unwrap();
    let (d2, id2) = capture(&d1, PluginInstanceId(0), b"two").unwrap();
    let stray = pkg.join("PluginState").join(blob_file_name(StateBlobId(7)));
    std::fs::write(&stray, b"stray").unwrap();
    save(&d2, &pkg).unwrap();
    let dir = pkg.join("PluginState");
    assert!(dir.join(blob_file_name(id2)).exists());
    assert!(dir.join(blob_file_name(id1)).exists(), "an autosave still uses it");
    assert!(!stray.exists());
    std::fs::remove_dir_all(&pkg).unwrap();
}

#[test]
fn missing_blob_loads_and_is_reported() {
    let pkg = temp_package("missing");
    let (d, id) = capture(&pad(), PluginInstanceId(0), b"gone").unwrap();
    save(&d, &pkg).unwrap();
    std::fs::remove_file(pkg.join("PluginState").join(blob_file_name(id))).unwrap();
    let back = load(&pkg).unwrap();
    assert_eq!(back.plugin_slot(PluginInstanceId(0)).unwrap().state, Some(id));
    assert_eq!(back.missing_states(), vec![id]);
    assert_eq!(back.state_blob(id), None);
    std::fs::remove_dir_all(&pkg).unwrap();
}
```

- [ ] **Step 2: Run the test to verify it fails**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test plugin_state
```

Expected: compile error — `unresolved import core_model::state`.

- [ ] **Step 3: Keep blobs on the document**

In `crates/core-model/src/document.rs` add `use crate::ids::StateBlobId;` (merge into the `ids` import), `use std::collections::BTreeSet;` and `use std::sync::Arc;`. Add the field to `struct Document` after `mixer`:

```rust
    /// Plugin state blobs by ID (Plan Ruling 8). Never serialized into
    /// `project.json`; `file::save`/`load` keep them in `PluginState/`.
    states: OrdMap<StateBlobId, Arc<[u8]>>,
```

Initialise it with `states: OrdMap::new(),` in `Document::new` and in `TryFrom<DocumentRepr>` (the `From<Document> for DocumentRepr` impl ignores it). Add public accessors after `plugin_slot`:

```rust
    /// The bytes of state blob `id`, when loaded.
    pub fn state_blob(&self, id: StateBlobId) -> Option<&[u8]> {
        self.states.get(&id).map(|b| &b[..])
    }

    /// Every blob a plugin slot points at.
    pub fn referenced_states(&self) -> BTreeSet<StateBlobId> {
        let insts = self.instruments.values().filter_map(|d| match &d.kind {
            DestKind::PluginInstrument { slot } => slot.state,
            DestKind::MidiEndpoint { .. } => None,
        });
        let inserts = self.mixer.inserts().filter_map(|(_, _, p)| p.state);
        insts.chain(inserts).collect()
    }

    /// Referenced blobs whose bytes are not loaded (a missing file).
    pub fn missing_states(&self) -> Vec<StateBlobId> {
        self.referenced_states()
            .into_iter()
            .filter(|id| !self.states.contains_key(id))
            .collect()
    }
```

and a crate accessor after `slot_mut`:

```rust
    pub(crate) fn states_mut(&mut self) -> &mut OrdMap<StateBlobId, Arc<[u8]>> {
        &mut self.states
    }
```

(`&b[..]` on an `Arc<[u8]>` is a full-range reborrow, which `clippy::indexing_slicing` does not flag.)

- [ ] **Step 4: Write the state module**

Create `crates/core-model/src/state.rs`:

```rust
//! Plugin state blobs (spec §4, Plan Ruling 8) and capture (Plan
//! Ruling 9).

use crate::document::Document;
use crate::error::Result;
use crate::ids::{PluginInstanceId, StateBlobId};
use serde_json::Value;
use std::collections::BTreeSet;
use std::sync::Arc;

/// The package folder holding blobs.
pub const STATE_DIR: &str = "PluginState";

/// The ID of a blob: the 64-bit FNV-1a hash of its bytes.
pub fn blob_id(bytes: &[u8]) -> StateBlobId {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    StateBlobId(h)
}

/// `<16 lowercase hex digits>.bin`.
pub fn blob_file_name(id: StateBlobId) -> String {
    format!("{:016x}.bin", id.0)
}

/// The blob ID a file name stands for, if it is one.
pub fn parse_blob_file_name(name: &str) -> Option<StateBlobId> {
    let hex = name.strip_suffix(".bin")?;
    if hex.len() != 16 {
        return None;
    }
    u64::from_str_radix(hex, 16).ok().map(StateBlobId)
}

/// Stores `bytes` as plugin instance `slot`'s state and clears its
/// overlay; blobs no slot points at any more are dropped from the copy.
/// The sound is unchanged, so this is not an undo step.
pub fn capture(doc: &Document, slot: PluginInstanceId, bytes: &[u8]) -> Result<(Document, StateBlobId)> {
    let mut d = doc.clone();
    let id = blob_id(bytes);
    let s = d.slot_mut(slot)?;
    s.state = Some(id);
    s.params.clear();
    d.states_mut().insert(id, Arc::from(bytes));
    let keep = d.referenced_states();
    let stale: Vec<StateBlobId> = d
        .states_mut()
        .keys()
        .filter(|k| !keep.contains(k))
        .copied()
        .collect();
    for k in stale {
        d.states_mut().remove(&k);
    }
    Ok((d, id))
}

/// Blob IDs a `project.json` text references (every `state` field of an
/// object that also has a `plugin` field, i.e. every plugin slot).
pub fn states_in_json(text: &str) -> BTreeSet<StateBlobId> {
    fn walk(v: &Value, out: &mut BTreeSet<StateBlobId>) {
        match v {
            Value::Object(m) => {
                if m.contains_key("plugin") {
                    if let Some(id) = m.get("state").and_then(Value::as_u64) {
                        out.insert(StateBlobId(id));
                    }
                }
                for c in m.values() {
                    walk(c, out);
                }
            }
            Value::Array(a) => {
                for c in a {
                    walk(c, out);
                }
            }
            _ => {}
        }
    }
    let mut out = BTreeSet::new();
    if let Ok(v) = serde_json::from_str::<Value>(text) {
        walk(&v, &mut out);
    }
    out
}
```

In `crates/core-model/src/lib.rs` add `pub mod state;` after `pub mod sequence;`.

(`im::OrdMap` 15 has no `retain`, hence the explicit removal loop.)

- [ ] **Step 5: Capture through the history**

In `crates/core-model/src/history.rs` add `use crate::ids::{PluginInstanceId, StateBlobId};` and, after `abort`, the method:

```rust
    /// Stores `bytes` as plugin instance `slot`'s state and clears its
    /// overlay in the present version, without an undo step (Plan Ruling
    /// 9). Refused inside an open transaction.
    pub fn capture_state(&mut self, slot: PluginInstanceId, bytes: &[u8]) -> Result<StateBlobId> {
        if self.txn.is_some() {
            return Err(ModelError::invalid("cannot capture plugin state inside a transaction"));
        }
        let (doc, id) = crate::state::capture(&self.present.doc, slot, bytes)?;
        self.present.doc = doc;
        Ok(id)
    }
```

- [ ] **Step 6: Blobs in the package**

In `crates/core-model/src/file.rs` add `use crate::state::{STATE_DIR, blob_file_name, parse_blob_file_name, states_in_json};` and `use std::sync::Arc;`, then add:

```rust
/// Writes every referenced blob that is not on disk yet (blobs are
/// immutable, so an existing file is already right). With `prune`, then
/// deletes blob files referenced neither by `doc` nor by any autosave.
fn write_states(doc: &Document, package: &Path, prune: bool) -> Result<()> {
    let dir = package.join(STATE_DIR);
    fs::create_dir_all(&dir)?;
    let referenced = doc.referenced_states();
    for id in &referenced {
        let path = dir.join(blob_file_name(*id));
        if let Some(bytes) = doc.state_blob(*id) {
            if !path.exists() {
                write_atomic(&path, bytes)?;
            }
        }
    }
    if prune {
        let mut keep = referenced;
        for a in list_autosaves(package)? {
            if let Ok(text) = fs::read_to_string(&a) {
                keep.extend(states_in_json(&text));
            }
        }
        for entry in fs::read_dir(&dir)? {
            let path = entry?.path();
            let id = path
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(parse_blob_file_name);
            if let Some(id) = id {
                if !keep.contains(&id) {
                    fs::remove_file(&path)?;
                }
            }
        }
    }
    Ok(())
}

/// Reads every referenced blob that exists in the package.
fn read_states(doc: &mut Document, package: &Path) {
    let dir = package.join(STATE_DIR);
    for id in doc.referenced_states() {
        if let Ok(bytes) = fs::read(dir.join(blob_file_name(id))) {
            doc.states_mut().insert(id, Arc::from(bytes));
        }
    }
}
```

Replace `save` and `load` with:

```rust
/// Saves `doc` into the package directory (created if missing): blobs
/// first, then `project.json`, then unreferenced blobs are removed.
pub fn save(doc: &Document, package: &Path) -> Result<()> {
    fs::create_dir_all(package.join("Media"))?;
    fs::create_dir_all(package.join("Autosave"))?;
    write_states(doc, package, false)?;
    write_atomic(&package.join(PROJECT_FILE), to_json(doc)?.as_bytes())?;
    write_states(doc, package, true)
}

/// Loads the package's `project.json` and its plugin state blobs. A
/// missing blob is listed by `Document::missing_states`, not an error.
pub fn load(package: &Path) -> Result<Document> {
    let mut doc = from_json(&fs::read_to_string(package.join(PROJECT_FILE))?)?;
    read_states(&mut doc, package);
    Ok(doc)
}
```

In `autosave`, before `write_atomic(&path, …)`, add `write_states(doc, package, false)?;`.

- [ ] **Step 7: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test plugin_state
cargo test --workspace
```

Expected: `plugin_state` 5 passed; the workspace passes (`save` now also creates an empty `PluginState/` folder; git does not track empty folders and the demo package stays as it is).

- [ ] **Step 8: Lint and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
git add crates/core-model/src/state.rs crates/core-model/src/lib.rs crates/core-model/src/document.rs crates/core-model/src/history.rs crates/core-model/src/file.rs crates/core-model/tests/plugin_state.rs
git commit -m "feat(core-model): content-addressed plugin state blobs and capture

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---
### Task 7: Graph building blocks — strip DSP, delay lines, insert processors and test nodes

**Goal:** Create `core-engine`'s `graph` module with the allocation-free strip DSP helpers, the fixed `DelayLine`, the `EffectNode` insert seam with its `InsertFactory`, and the test nodes that stand in for plugins in 3a.

**Files:**
- Create: `crates/core-engine/src/graph/mod.rs`
- Create: `crates/core-engine/src/graph/dsp.rs`
- Create: `crates/core-engine/src/graph/delay.rs`
- Create: `crates/core-engine/src/graph/node.rs`
- Modify: `crates/core-engine/src/lib.rs`
- Test: `crates/core-engine/tests/graph_dsp.rs`

**Interfaces:**
- Consumes: `backend::StereoBuf`; `core_model::mixer::{PluginSlot, PluginRef, PluginFormat, db_to_gain, pan_gains}`.
- Produces:
  - `graph::dsp::{FAULT_ABS = 64.0, copy(&mut StereoBuf, &StereoBuf, u32), mix_into(&mut StereoBuf, &StereoBuf, gain: f32, u32), scale(&mut StereoBuf, gl: f32, gr: f32, u32), peaks(&StereoBuf, u32) -> (f32, f32), peak(&StereoBuf, u32) -> f32, is_faulty(&StereoBuf, u32) -> bool, strip_gains(gain_db: f64, pan: f64, apply_pan: bool) -> (f32, f32)}` — all `// RT` except `strip_gains`.
  - `graph::delay::DelayLine { new(delay: u32), delay() -> u32, process(&mut [f32], &mut [f32]), process_buf(&mut StereoBuf, frames: u32), clear() }`.
  - `graph::node::ProcessStatus::{Ok, Error}`; `trait EffectNode: Send { fn latency(&self) -> u32; fn process(&mut self, frames: u32, io: &mut StereoBuf, sidechain: Option<&StereoBuf>) -> ProcessStatus; fn reset(&mut self); }`.
  - `trait InsertFactory: Send { fn make(&mut self, slot: &PluginSlot, sample_rate: f64, max_frames: u32) -> Result<Box<dyn EffectNode>, String>; }`; `NoInserts` (always `Err`, message names the plugin).
  - `TestMode::{Gain(f32), Noise { seed: u32, level: f32 }, Duck, AddSidechain, Nan, Error, Stall(Arc<AtomicBool>), Impulse}`; `TestNode::new(mode, latency)` implementing `EffectNode`.
  - `TestInserts` (`Clone + Default`, `InsertFactory`): `PREFIX = "vizdaw.test:"`, `new()`, `with_stall(Arc<AtomicBool>)`, `plugin(spec: &str) -> PluginRef`, `parse(&self, id: &str) -> Result<TestNode, String>`, `made() -> usize`. Keys: `gain=F`, `latency=N`, `noise=SEED`, `duck`, `sc`, `nan`, `error`, `impulse`, `stall`.

- [ ] **Step 1: Write the failing test**

Create `crates/core-engine/tests/graph_dsp.rs`:

```rust
use core_engine::backend::StereoBuf;
use core_engine::graph::delay::DelayLine;
use core_engine::graph::dsp::{copy, is_faulty, mix_into, peaks, scale, strip_gains};
use core_engine::graph::{
    EffectNode, InsertFactory, NoInserts, ProcessStatus, TestInserts, TestMode, TestNode,
};
use core_model::ids::PluginInstanceId;
use core_model::mixer::{PluginFormat, PluginRef, PluginSlot};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

fn buf(l: &[f32], r: &[f32]) -> StereoBuf {
    StereoBuf {
        left: l.to_vec().into_boxed_slice(),
        right: r.to_vec().into_boxed_slice(),
    }
}

#[test]
fn mixing_scaling_and_peaks() {
    let mut a = buf(&[1.0, 2.0, 3.0], &[0.0, 0.0, 0.0]);
    let b = buf(&[1.0, 1.0, 1.0], &[2.0, -4.0, 1.0]);
    mix_into(&mut a, &b, 0.5, 2);
    assert_eq!(&a.left[..], &[1.5, 2.5, 3.0]);
    assert_eq!(&a.right[..], &[1.0, -2.0, 0.0]);
    scale(&mut a, 2.0, -1.0, 3);
    assert_eq!(&a.left[..], &[3.0, 5.0, 6.0]);
    assert_eq!(&a.right[..], &[-1.0, 2.0, -0.0]);
    assert_eq!(peaks(&a, 3), (6.0, 2.0));
    assert_eq!(peaks(&a, 2), (5.0, 2.0));
    copy(&mut a, &b, 3);
    assert_eq!(&a.right[..], &[2.0, -4.0, 1.0]);
    // Frames beyond the buffers are clamped, never a panic.
    mix_into(&mut a, &b, 1.0, 99);
    assert_eq!(peaks(&a, 99), (2.0, 8.0));
}

#[test]
fn faults_are_nan_inf_or_beyond_64() {
    assert!(!is_faulty(&buf(&[64.0, -64.0], &[0.0, 1.0]), 2));
    assert!(is_faulty(&buf(&[64.5, 0.0], &[0.0, 0.0]), 2));
    assert!(is_faulty(&buf(&[0.0, 0.0], &[f32::NAN, 0.0]), 2));
    assert!(is_faulty(&buf(&[f32::NEG_INFINITY, 0.0], &[0.0, 0.0]), 2));
    assert!(!is_faulty(&buf(&[0.0, f32::NAN], &[0.0, 0.0]), 1));
}

#[test]
fn strip_gain_laws() {
    let (l, r) = strip_gains(0.0, 0.0, true);
    assert!((l - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6 && (l - r).abs() < 1e-9);
    assert_eq!(strip_gains(0.0, 0.0, false), (1.0, 1.0));
    assert_eq!(strip_gains(-144.0, 0.3, true), (0.0, 0.0));
    let (l, r) = strip_gains(0.0, -1.0, true);
    assert_eq!(l, 1.0);
    assert!(r.abs() < 1e-7);
}

#[test]
fn delay_lines_are_exact_across_blocks() {
    let mut d = DelayLine::new(5);
    assert_eq!(d.delay(), 5);
    let mut out = Vec::new();
    for block in 0..4 {
        let mut l = [0.0f32; 3];
        let mut r = [0.0f32; 3];
        if block == 0 {
            l[0] = 1.0;
            r[0] = -1.0;
        }
        d.process(&mut l, &mut r);
        out.extend(l.iter().zip(r.iter()).map(|(a, b)| (*a, *b)));
    }
    let hits: Vec<usize> = out.iter().enumerate().filter(|(_, x)| x.0 != 0.0).map(|(i, _)| i).collect();
    assert_eq!(hits, vec![5]);
    assert_eq!(out[5], (1.0, -1.0));
    let mut zero = DelayLine::new(0);
    let mut l = [1.0f32, 2.0];
    let mut r = [3.0f32, 4.0];
    zero.process(&mut l, &mut r);
    assert_eq!((l, r), ([1.0, 2.0], [3.0, 4.0]));
}

#[test]
fn test_nodes_apply_gain_and_latency() {
    let mut n = TestNode::new(TestMode::Gain(0.5), 2);
    assert_eq!(n.latency(), 2);
    let mut io = buf(&[1.0, 0.0, 0.0, 0.0], &[2.0, 0.0, 0.0, 0.0]);
    assert_eq!(n.process(4, &mut io, None), ProcessStatus::Ok);
    assert_eq!(&io.left[..], &[0.0, 0.0, 0.5, 0.0]);
    assert_eq!(&io.right[..], &[0.0, 0.0, 1.0, 0.0]);
}

#[test]
fn noise_is_deterministic_and_sidechain_is_added() {
    let mut a = TestNode::new(TestMode::Noise { seed: 9, level: 0.25 }, 0);
    let mut b = TestNode::new(TestMode::Noise { seed: 9, level: 0.25 }, 0);
    let (mut x, mut y) = (StereoBuf::new(64), StereoBuf::new(64));
    a.process(64, &mut x, None);
    b.process(32, &mut y, None);
    let mut tail = StereoBuf::new(32);
    b.process(32, &mut tail, None);
    assert_eq!(&x.left[..32], &y.left[..32]);
    assert_eq!(&x.left[32..], &tail.left[..]);
    assert!(x.left.iter().all(|s| s.abs() <= 0.25) && x.left.iter().any(|s| *s != 0.0));
    let mut sc_node = TestNode::new(TestMode::AddSidechain, 0);
    let mut io = buf(&[1.0, 1.0], &[0.0, 0.0]);
    let sc = buf(&[0.5, 0.0], &[0.25, 0.0]);
    sc_node.process(2, &mut io, Some(&sc));
    assert_eq!((&io.left[..], &io.right[..]), (&[1.5, 1.0][..], &[0.25, 0.0][..]));
}

#[test]
fn bad_nodes_report_themselves() {
    let mut nan = TestNode::new(TestMode::Nan, 0);
    let mut io = StereoBuf::new(4);
    nan.process(4, &mut io, None);
    assert!(is_faulty(&io, 4));
    let mut err = TestNode::new(TestMode::Error, 0);
    assert_eq!(err.process(4, &mut io, None), ProcessStatus::Error);
    let mut imp = TestNode::new(TestMode::Impulse, 0);
    let mut io = StereoBuf::new(4);
    imp.process(4, &mut io, None);
    assert_eq!(&io.left[..], &[1.0, 0.0, 0.0, 0.0]);
    let mut io = StereoBuf::new(4);
    imp.process(4, &mut io, None);
    assert_eq!(&io.left[..], &[0.0; 4]);
}

#[test]
fn stall_waits_for_its_flag() {
    let flag = Arc::new(AtomicBool::new(false));
    let f2 = Arc::clone(&flag);
    let t = std::thread::spawn(move || {
        let mut s = TestNode::new(TestMode::Stall(f2), 0);
        let mut io = StereoBuf::new(1);
        let start = std::time::Instant::now();
        s.process(1, &mut io, None);
        start.elapsed()
    });
    std::thread::sleep(std::time::Duration::from_millis(30));
    flag.store(true, Ordering::Release);
    assert!(t.join().unwrap() >= std::time::Duration::from_millis(25));
}

fn slot(id: &str) -> PluginSlot {
    PluginSlot::new(
        PluginInstanceId(0),
        PluginRef {
            format: PluginFormat::Clap,
            id: id.into(),
            name: "x".into(),
            vendor: String::new(),
            version: String::new(),
        },
    )
}

#[test]
fn test_inserts_parse_their_ids() {
    let mut f = TestInserts::new();
    let p = TestInserts::plugin("gain=0.5,latency=64");
    assert_eq!(p.id, "vizdaw.test:gain=0.5,latency=64");
    assert_eq!(p.format, PluginFormat::Clap);
    let node = f.make(&PluginSlot::new(PluginInstanceId(0), p), 48_000.0, 512).unwrap();
    assert_eq!(node.latency(), 64);
    assert_eq!(f.made(), 1);
    assert!(f.make(&slot("vizdaw.test:bogus"), 48_000.0, 512).is_err());
    assert!(f.make(&slot("com.example.synth"), 48_000.0, 512).is_err());
    assert!(f.make(&slot("vizdaw.test:stall"), 48_000.0, 512).is_err(), "no stall flag");
    let mut g = TestInserts::with_stall(Arc::new(AtomicBool::new(true)));
    assert!(g.make(&slot("vizdaw.test:stall"), 48_000.0, 512).is_ok());
    for ok in ["noise=3", "duck", "sc", "nan", "error", "impulse", "latency=0", ""] {
        assert!(f.parse(&format!("vizdaw.test:{ok}")).is_ok(), "{ok}");
    }
    let e = NoInserts.make(&slot("com.example.synth"), 48_000.0, 512).err().unwrap();
    assert!(e.contains("x"), "{e}");
}
```

- [ ] **Step 2: Run the test to verify it fails**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-engine --test graph_dsp
```

Expected: compile error — `could not find graph in core_engine`.

- [ ] **Step 3: Write the DSP helpers**

Create `crates/core-engine/src/graph/dsp.rs`:

```rust
//! Strip DSP on [`StereoBuf`]s. Every function clamps `frames` to the
//! buffers' lengths and, except `strip_gains`, is `// RT`.

use crate::backend::StereoBuf;

/// |x| above this (or NaN/Inf) in an insert's output is a fault (spec §7).
pub const FAULT_ABS: f32 = 64.0;

fn len(frames: u32, b: &StereoBuf) -> usize {
    (frames as usize).min(b.left.len()).min(b.right.len())
}

/// `dst = src` on the first `frames` frames. // RT
pub fn copy(dst: &mut StereoBuf, src: &StereoBuf, frames: u32) {
    let n = len(frames, dst).min(len(frames, src));
    if let (Some(d), Some(s)) = (dst.left.get_mut(..n), src.left.get(..n)) {
        d.copy_from_slice(s);
    }
    if let (Some(d), Some(s)) = (dst.right.get_mut(..n), src.right.get(..n)) {
        d.copy_from_slice(s);
    }
}

/// `dst += gain × src`; a gain of exactly 0 leaves `dst` untouched. // RT
pub fn mix_into(dst: &mut StereoBuf, src: &StereoBuf, gain: f32, frames: u32) {
    if gain == 0.0 {
        return;
    }
    let n = len(frames, dst).min(len(frames, src));
    for (d, s) in [(&mut dst.left, &src.left), (&mut dst.right, &src.right)] {
        if let (Some(d), Some(s)) = (d.get_mut(..n), s.get(..n)) {
            for (a, b) in d.iter_mut().zip(s) {
                *a += gain * *b;
            }
        }
    }
}

/// Multiplies the left channel by `gl` and the right by `gr`. // RT
pub fn scale(buf: &mut StereoBuf, gl: f32, gr: f32, frames: u32) {
    let n = len(frames, buf);
    for (c, g) in [(&mut buf.left, gl), (&mut buf.right, gr)] {
        if let Some(c) = c.get_mut(..n) {
            for x in c {
                *x *= g;
            }
        }
    }
}

/// Peak |x| of each channel; non-finite samples count as 0. // RT
pub fn peaks(buf: &StereoBuf, frames: u32) -> (f32, f32) {
    let n = len(frames, buf);
    let p = |c: &[f32]| {
        c.get(..n)
            .unwrap_or(&[])
            .iter()
            .filter(|x| x.is_finite())
            .fold(0.0f32, |m, x| m.max(x.abs()))
    };
    (p(&buf.left), p(&buf.right))
}

/// Peak over both channels. // RT
pub fn peak(buf: &StereoBuf, frames: u32) -> f32 {
    let (l, r) = peaks(buf, frames);
    l.max(r)
}

/// True when any sample is NaN, infinite or beyond ±[`FAULT_ABS`]. // RT
pub fn is_faulty(buf: &StereoBuf, frames: u32) -> bool {
    let n = len(frames, buf);
    [&buf.left, &buf.right].into_iter().any(|c| {
        c.get(..n)
            .unwrap_or(&[])
            .iter()
            .any(|x| !x.is_finite() || x.abs() > FAULT_ABS)
    })
}

/// Fader × pan as `(left, right)` gains; without pan (master, Plan
/// Ruling 4) both are the fader gain.
pub fn strip_gains(gain_db: f64, pan: f64, apply_pan: bool) -> (f32, f32) {
    let g = core_model::mixer::db_to_gain(gain_db);
    if apply_pan {
        let (l, r) = core_model::mixer::pan_gains(pan);
        ((g * l) as f32, (g * r) as f32)
    } else {
        (g as f32, g as f32)
    }
}
```

- [ ] **Step 4: Write the delay line**

Create `crates/core-engine/src/graph/delay.rs`:

```rust
//! Fixed stereo delay lines for PDC edges and bypassed inserts: allocated
//! once on the control thread, processed allocation-free.

use crate::backend::StereoBuf;

/// A stereo delay of a fixed number of frames.
#[derive(Debug, Clone)]
pub struct DelayLine {
    left: Box<[f32]>,
    right: Box<[f32]>,
    pos: usize,
}

impl DelayLine {
    /// A silent delay of `delay` frames (0 passes audio through).
    pub fn new(delay: u32) -> DelayLine {
        let n = delay as usize;
        DelayLine {
            left: vec![0.0; n].into_boxed_slice(),
            right: vec![0.0; n].into_boxed_slice(),
            pos: 0,
        }
    }

    /// The delay in frames.
    pub fn delay(&self) -> u32 {
        u32::try_from(self.left.len()).unwrap_or(u32::MAX)
    }

    /// Replaces `left`/`right` (same length) with the input delayed by
    /// `delay()` frames. // RT
    pub fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let d = self.left.len().min(self.right.len());
        if d == 0 {
            return;
        }
        for (a, b) in left.iter_mut().zip(right.iter_mut()) {
            let (Some(x), Some(y)) = (self.left.get_mut(self.pos), self.right.get_mut(self.pos))
            else {
                return;
            };
            std::mem::swap(a, x);
            std::mem::swap(b, y);
            self.pos += 1;
            if self.pos == d {
                self.pos = 0;
            }
        }
    }

    /// [`DelayLine::process`] on the first `frames` frames of `buf`. // RT
    pub fn process_buf(&mut self, buf: &mut StereoBuf, frames: u32) {
        let n = (frames as usize).min(buf.left.len()).min(buf.right.len());
        if let (Some(l), Some(r)) = (buf.left.get_mut(..n), buf.right.get_mut(..n)) {
            self.process(l, r);
        }
    }

    /// Silences the delay memory. // RT
    pub fn clear(&mut self) {
        self.left.fill(0.0);
        self.right.fill(0.0);
        self.pos = 0;
    }
}
```

- [ ] **Step 5: Write the insert seam and the test nodes**

Create `crates/core-engine/src/graph/node.rs`:

```rust
//! The insert seam: `EffectNode` processors made by an `InsertFactory`
//! (Plan Ruling 12). 3a ships only test nodes; 3b wraps plugin instances.

use crate::backend::StereoBuf;
use crate::graph::delay::DelayLine;
use crate::graph::dsp;
use core_model::mixer::{PluginFormat, PluginRef, PluginSlot};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// What a processor reported for one block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessStatus {
    /// Processed normally.
    Ok,
    /// The processor failed; counted like a faulty block (spec §7).
    Error,
}

/// An insert processor. `latency` is read on the control thread when the
/// node is created; the other methods run on render threads.
pub trait EffectNode: Send {
    /// Processing latency in frames.
    fn latency(&self) -> u32;
    /// Processes the first `frames` frames of `io` in place; `sidechain` is
    /// the aligned sidechain input when one is routed. // RT
    fn process(&mut self, frames: u32, io: &mut StereoBuf, sidechain: Option<&StereoBuf>)
    -> ProcessStatus;
    /// Clears internal state (delay memory, envelopes). // RT
    fn reset(&mut self);
}

/// Creates insert processors on the control thread.
pub trait InsertFactory: Send {
    /// The processor for `slot` at the stream's rate and largest block.
    /// `Err` leaves the slot a placeholder (audio passes, no latency) and is
    /// reported in `GraphReport::placeholders`.
    fn make(
        &mut self,
        slot: &PluginSlot,
        sample_rate: f64,
        max_frames: u32,
    ) -> Result<Box<dyn EffectNode>, String>;
}

/// The factory before plugin hosting: every slot is a placeholder.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoInserts;

impl InsertFactory for NoInserts {
    fn make(&mut self, slot: &PluginSlot, _: f64, _: u32) -> Result<Box<dyn EffectNode>, String> {
        Err(format!(
            "{} cannot load until plugin hosting exists",
            slot.plugin.name
        ))
    }
}

/// What a test node does.
#[derive(Debug, Clone)]
pub enum TestMode {
    /// `out = in × gain`.
    Gain(f32),
    /// `out = in + level × noise` (xorshift32 from `seed`, per sample).
    Noise {
        /// Generator seed.
        seed: u32,
        /// Amplitude.
        level: f32,
    },
    /// `out = in × (1 − min(1, peak(sidechain)))` per block (a crude ducker).
    Duck,
    /// `out = in + sidechain` (makes sidechain alignment visible).
    AddSidechain,
    /// Writes NaN into the first left sample.
    Nan,
    /// Passes audio through but reports `ProcessStatus::Error`.
    Error,
    /// Spins until the flag is set (at most 5 s), then passes audio through.
    Stall(Arc<AtomicBool>),
    /// Adds a unit impulse at the very first frame it processes.
    Impulse,
}

/// A test stand-in for a plugin: a [`TestMode`] followed by a fixed
/// latency.
#[derive(Debug, Clone)]
pub struct TestNode {
    mode: TestMode,
    latency: u32,
    delay: DelayLine,
    rng: u32,
    fired: bool,
}

impl TestNode {
    /// A node of `mode` delayed by `latency` frames.
    pub fn new(mode: TestMode, latency: u32) -> TestNode {
        let rng = match &mode {
            TestMode::Noise { seed, .. } => *seed | 1,
            _ => 1,
        };
        TestNode {
            mode,
            latency,
            delay: DelayLine::new(latency),
            rng,
            fired: false,
        }
    }
}

/// xorshift32 → [-1, 1). // RT
fn noise(state: &mut u32) -> f32 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *state = x;
    (x as f32 / u32::MAX as f32) * 2.0 - 1.0
}

impl EffectNode for TestNode {
    fn latency(&self) -> u32 {
        self.latency
    }

    fn process(
        &mut self,
        frames: u32,
        io: &mut StereoBuf,
        sidechain: Option<&StereoBuf>,
    ) -> ProcessStatus {
        let n = (frames as usize).min(io.left.len()).min(io.right.len());
        let mut status = ProcessStatus::Ok;
        let duck = match (&self.mode, sidechain) {
            (TestMode::Duck, Some(sc)) => 1.0 - dsp::peak(sc, frames).min(1.0),
            _ => 1.0,
        };
        let (Some(l), Some(r)) = (io.left.get_mut(..n), io.right.get_mut(..n)) else {
            return status;
        };
        match &self.mode {
            TestMode::Gain(g) => {
                for x in l.iter_mut().chain(r.iter_mut()) {
                    *x *= *g;
                }
            }
            TestMode::Noise { level, .. } => {
                for (a, b) in l.iter_mut().zip(r.iter_mut()) {
                    let v = noise(&mut self.rng) * *level;
                    *a += v;
                    *b += v;
                }
            }
            TestMode::Duck => {
                for x in l.iter_mut().chain(r.iter_mut()) {
                    *x *= duck;
                }
            }
            TestMode::AddSidechain => {
                if let Some(sc) = sidechain {
                    let (sl, sr) = (sc.left.get(..n).unwrap_or(&[]), sc.right.get(..n).unwrap_or(&[]));
                    for (a, s) in l.iter_mut().zip(sl) {
                        *a += *s;
                    }
                    for (b, s) in r.iter_mut().zip(sr) {
                        *b += *s;
                    }
                }
            }
            TestMode::Nan => {
                if let Some(x) = l.first_mut() {
                    *x = f32::NAN;
                }
            }
            TestMode::Error => status = ProcessStatus::Error,
            TestMode::Stall(flag) => {
                let start = Instant::now();
                while !flag.load(Ordering::Acquire) && start.elapsed() < Duration::from_secs(5) {
                    std::hint::spin_loop();
                }
            }
            TestMode::Impulse => {
                if !self.fired {
                    self.fired = true;
                    if let (Some(a), Some(b)) = (l.first_mut(), r.first_mut()) {
                        *a += 1.0;
                        *b += 1.0;
                    }
                }
            }
        }
        self.delay.process(l, r);
        status
    }

    fn reset(&mut self) {
        self.delay.clear();
    }
}

/// Builds [`TestNode`]s from CLAP ids `vizdaw.test:<key>[=<value>],…`
/// (Plan Ruling 12). Clones share the `made` counter.
#[derive(Debug, Clone, Default)]
pub struct TestInserts {
    stall: Option<Arc<AtomicBool>>,
    made: Arc<AtomicUsize>,
}

impl TestInserts {
    /// The id prefix of test plugins.
    pub const PREFIX: &'static str = "vizdaw.test:";

    /// A factory without a stall flag (`stall` ids are refused).
    pub fn new() -> TestInserts {
        TestInserts::default()
    }

    /// A factory whose `stall` nodes wait for `flag`.
    pub fn with_stall(flag: Arc<AtomicBool>) -> TestInserts {
        TestInserts {
            stall: Some(flag),
            made: Arc::default(),
        }
    }

    /// The reference of test plugin `spec`, e.g. `"gain=0.5,latency=64"`.
    pub fn plugin(spec: &str) -> PluginRef {
        PluginRef {
            format: PluginFormat::Clap,
            id: format!("{}{spec}", Self::PREFIX),
            name: format!("Test {spec}"),
            vendor: "VizDAW".into(),
            version: "1".into(),
        }
    }

    /// Nodes created so far by this factory and its clones.
    pub fn made(&self) -> usize {
        self.made.load(Ordering::Relaxed)
    }

    /// Parses a test plugin id (default mode `gain=1`, latency 0).
    pub fn parse(&self, id: &str) -> Result<TestNode, String> {
        let spec = id
            .strip_prefix(Self::PREFIX)
            .ok_or_else(|| format!("{id:?} is not a test plugin"))?;
        let mut mode = TestMode::Gain(1.0);
        let mut latency = 0u32;
        for part in spec.split(',').filter(|p| !p.is_empty()) {
            let (k, v) = part.split_once('=').unwrap_or((part, ""));
            let bad = || format!("bad value {v:?} for {k:?}");
            match k {
                "gain" => mode = TestMode::Gain(v.parse().map_err(|_| bad())?),
                "latency" => latency = v.parse().map_err(|_| bad())?,
                "noise" => {
                    mode = TestMode::Noise {
                        seed: v.parse().map_err(|_| bad())?,
                        level: 0.25,
                    }
                }
                "duck" => mode = TestMode::Duck,
                "sc" => mode = TestMode::AddSidechain,
                "nan" => mode = TestMode::Nan,
                "error" => mode = TestMode::Error,
                "impulse" => mode = TestMode::Impulse,
                "stall" => {
                    let flag = self.stall.clone().ok_or("this factory has no stall flag")?;
                    mode = TestMode::Stall(flag);
                }
                other => return Err(format!("unknown test plugin key {other:?}")),
            }
        }
        Ok(TestNode::new(mode, latency))
    }
}

impl InsertFactory for TestInserts {
    fn make(&mut self, slot: &PluginSlot, _: f64, _: u32) -> Result<Box<dyn EffectNode>, String> {
        let n = self.parse(&slot.plugin.id)?;
        self.made.fetch_add(1, Ordering::Relaxed);
        Ok(Box::new(n))
    }
}
```

Create `crates/core-engine/src/graph/mod.rs`:

```rust
//! The mixer render graph (spec §6, §7). Built up over several modules:
//! strip DSP (`dsp`), delay lines (`delay`) and insert processors
//! (`node`).

pub mod delay;
pub mod dsp;
pub mod node;

pub use node::{
    EffectNode, InsertFactory, NoInserts, ProcessStatus, TestInserts, TestMode, TestNode,
};
```

In `crates/core-engine/src/lib.rs` add `pub mod graph;` after `pub mod fixtures;`'s `#[cfg]` pair (i.e. before `pub mod midi1;`).

- [ ] **Step 6: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-engine --test graph_dsp
```

Expected: 9 passed.

- [ ] **Step 7: Lint and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git add crates/core-engine/src/graph crates/core-engine/src/lib.rs crates/core-engine/tests/graph_dsp.rs
git commit -m "feat(core-engine): strip DSP, delay lines, insert seam and test nodes

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---
### Task 8: Graph description and the PDC planner

**Goal:** A platform-neutral `GraphSpec` (instrument and strip nodes, input edges with tap and gain, inserts with latency, bypass and sidechain) and `plan()`, which orders the nodes topologically, computes every node's arrival and output time, the delay on every input and sidechain edge, the total latency `L`, and enforces the size and delay limits (spec §6).

**Files:**
- Create: `crates/core-engine/src/graph/spec.rs`
- Create: `crates/core-engine/src/graph/pdc.rs`
- Modify: `crates/core-engine/src/graph/mod.rs`
- Test: `crates/core-engine/tests/graph_plan.rs`

**Interfaces:**
- Consumes: `schedule::TargetIdx`; `core_model::ids::{PluginInstanceId, StripId}`.
- Produces:
  - `spec::Tap::{Pre, Post}`; `InputSpec { from: usize, tap: Tap, gain: f32 }`; `InsertSpec { instance: Option<PluginInstanceId>, latency: u32, bypass: bool, sidechain: Option<InputSpec> }`; `StripSpec { strip: Option<StripId>, inputs: Vec<InputSpec>, inserts: Vec<InsertSpec>, gain_l: f32, gain_r: f32, audible: bool }` + `StripSpec::new(Option<StripId>)` (unity gains, audible); `NodeSpec::{Instrument { target: TargetIdx, latency: u32 }, Strip(StripSpec)}`; `GraphSpec { nodes: Vec<NodeSpec>, master: usize }` with `deps(i) -> Vec<usize>` (inputs, then sidechain sources) and `shape() -> u64` (FNV-1a over structure: node kinds, targets, input `from`/tap, insert instances, latencies, sidechain `from`/tap — not gains, `audible` or bypass).
  - `pdc::{MAX_NODES = 1024, MAX_INSERTS = 4096}`, `PlanError::{TooLarge { nodes, inserts }, Cycle, BadIndex { node }, LatencyTooLarge { frames }}` (`thiserror`), `Plan { order, t_in, t_out, input_delays: Vec<Vec<u32>>, sidechain_delays: Vec<Vec<u32>>, latency: u64 }`, `plan(&GraphSpec, max_delay: u64) -> Result<Plan, PlanError>`.

PDC rule (spec §6, implemented exactly): a strip's latency is the sum of its inserts' latencies, bypassed or not; an instrument's is its own. `t_in(n) = max(0, max over inputs t_out(src), max over inserts k with a sidechain (t_out(sc_src) − Σ_{j<k} latency_j))`, `t_out(n) = t_in(n) + latency(n)`; input delay = `t_in(n) − t_out(src)`; sidechain delay at insert `k` = `t_in(n) + Σ_{j<k} latency_j − t_out(sc_src)`; `L = t_out(master)`.

- [ ] **Step 1: Write the failing test**

Create `crates/core-engine/tests/graph_plan.rs`:

```rust
use core_engine::graph::{
    GraphSpec, InputSpec, InsertSpec, MAX_NODES, NodeSpec, PlanError, StripSpec, Tap, plan,
};
use core_engine::schedule::TargetIdx;
use core_model::ids::PluginInstanceId;

fn input(from: usize) -> InputSpec {
    InputSpec { from, tap: Tap::Post, gain: 1.0 }
}

fn ins(latency: u32) -> InsertSpec {
    InsertSpec { instance: None, latency, bypass: false, sidechain: None }
}

fn strip(inputs: &[usize], inserts: Vec<InsertSpec>) -> NodeSpec {
    let mut s = StripSpec::new(None);
    s.inputs = inputs.iter().map(|&f| input(f)).collect();
    s.inserts = inserts;
    NodeSpec::Strip(s)
}

const MAX: u64 = 96_000;

#[test]
fn a_chain_adds_insert_latencies() {
    let g = GraphSpec {
        nodes: vec![
            NodeSpec::Instrument { target: TargetIdx(0), latency: 0 },
            strip(&[0], vec![ins(10), ins(20)]),
            strip(&[1], vec![]),
        ],
        master: 2,
    };
    let p = plan(&g, MAX).unwrap();
    assert_eq!(p.order, vec![0, 1, 2]);
    assert_eq!(p.t_out, vec![0, 30, 30]);
    assert_eq!(p.latency, 30);
    assert_eq!(p.input_delays, vec![vec![], vec![0], vec![0]]);
    assert_eq!(p.sidechain_delays, vec![vec![], vec![0, 0], vec![]]);
}

fn parallel(y_latency: u32, y_bypass: bool) -> GraphSpec {
    let mut y = ins(y_latency);
    y.bypass = y_bypass;
    GraphSpec {
        nodes: vec![
            strip(&[], vec![ins(0)]),
            strip(&[0], vec![ins(0)]),
            strip(&[0], vec![y]),
            strip(&[1, 2], vec![]),
        ],
        master: 3,
    }
}

#[test]
fn parallel_paths_are_aligned_to_the_latest() {
    let p = plan(&parallel(100, false), MAX).unwrap();
    assert_eq!(p.t_in, vec![0, 0, 0, 100]);
    assert_eq!(p.input_delays[3], vec![100, 0]);
    assert_eq!(p.latency, 100);
}

#[test]
fn bypass_keeps_latency() {
    assert_eq!(
        plan(&parallel(100, true), MAX).unwrap(),
        plan(&parallel(100, false), MAX).unwrap()
    );
}

/// 0 K (kick, insert latency `k`), 1 S (source), 2 P (input S; inserts:
/// latency 50, then a ducker fed by K), 3 master (inputs P, K).
fn sidechained(k: u32) -> GraphSpec {
    let mut duck = ins(0);
    duck.sidechain = Some(input(0));
    GraphSpec {
        nodes: vec![
            strip(&[], vec![ins(k)]),
            strip(&[], vec![]),
            strip(&[1], vec![ins(50), duck]),
            strip(&[2, 0], vec![]),
        ],
        master: 3,
    }
}

#[test]
fn sidechains_align_to_the_insert_position() {
    let p = plan(&sidechained(0), MAX).unwrap();
    assert_eq!(p.t_in[2], 0);
    assert_eq!(p.sidechain_delays[2], vec![0, 50]);
    assert_eq!(p.input_delays[3], vec![0, 50]);
    assert_eq!(p.latency, 50);
    // A late sidechain source delays the strip's main input instead.
    let p = plan(&sidechained(80), MAX).unwrap();
    assert_eq!(p.t_in[2], 30);
    assert_eq!(p.input_delays[2], vec![30]);
    assert_eq!(p.sidechain_delays[2], vec![0, 0]);
    assert_eq!(p.input_delays[3], vec![0, 0]);
    assert_eq!(p.latency, 80);
}

#[test]
fn cycles_bad_indices_and_limits_are_refused() {
    let cyc = GraphSpec { nodes: vec![strip(&[1], vec![]), strip(&[0], vec![])], master: 1 };
    assert_eq!(plan(&cyc, MAX), Err(PlanError::Cycle));
    let selfie = GraphSpec { nodes: vec![strip(&[0], vec![])], master: 0 };
    assert_eq!(plan(&selfie, MAX), Err(PlanError::Cycle));
    let bad = GraphSpec { nodes: vec![strip(&[9], vec![])], master: 0 };
    assert_eq!(plan(&bad, MAX), Err(PlanError::BadIndex { node: 9 }));
    let no_master = GraphSpec { nodes: vec![strip(&[], vec![])], master: 3 };
    assert_eq!(plan(&no_master, MAX), Err(PlanError::BadIndex { node: 3 }));
    let big = GraphSpec { nodes: (0..=MAX_NODES).map(|_| strip(&[], vec![])).collect(), master: 0 };
    assert_eq!(plan(&big, MAX), Err(PlanError::TooLarge { nodes: 1025, inserts: 0 }));
    assert_eq!(
        plan(&parallel(96_001, false), MAX),
        Err(PlanError::LatencyTooLarge { frames: 96_001 })
    );
    assert!(plan(&parallel(96_000, false), MAX).is_ok());
}

#[test]
fn shape_ignores_parameters_but_not_structure() {
    let a = parallel(100, false);
    let mut b = parallel(100, true);
    if let NodeSpec::Strip(s) = &mut b.nodes[1] {
        s.gain_l = 0.25;
        s.audible = false;
        s.inputs[0].gain = 0.0;
    }
    assert_eq!(a.shape(), b.shape());
    assert_ne!(a.shape(), parallel(99, false).shape());
    let mut c = parallel(100, false);
    if let NodeSpec::Strip(s) = &mut c.nodes[3] {
        s.inputs[0].tap = Tap::Pre;
    }
    assert_ne!(a.shape(), c.shape());
    let mut d = parallel(100, false);
    if let NodeSpec::Strip(s) = &mut d.nodes[1] {
        s.inserts[0].instance = Some(PluginInstanceId(4));
    }
    assert_ne!(a.shape(), d.shape());
    assert_eq!(a.deps(3), vec![1, 2]);
    assert_eq!(sidechained(0).deps(2), vec![1, 0]);
}
```

- [ ] **Step 2: Run the test to verify it fails**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-engine --test graph_plan
```

Expected: compile error — no `GraphSpec` in `core_engine::graph`.

- [ ] **Step 3: Write the graph description**

Create `crates/core-engine/src/graph/spec.rs`:

```rust
//! A platform-neutral description of a render graph, built from the mixer
//! by `graph::mixer` or directly by tests.

use crate::schedule::TargetIdx;
use core_model::ids::{PluginInstanceId, StripId};

/// Which buffer of a source strip an edge reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tap {
    /// After the inserts, before the fader (pre-fader sends).
    Pre,
    /// After fader and pan (outputs, post-fader sends, sidechains;
    /// instrument nodes write here).
    Post,
}

/// An edge into a strip: `gain × tap(from)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InputSpec {
    /// Source node index.
    pub from: usize,
    /// Which buffer of the source.
    pub tap: Tap,
    /// Linear gain (send level, mute factor).
    pub gain: f32,
}

/// One insert of a strip.
#[derive(Debug, Clone, PartialEq)]
pub struct InsertSpec {
    /// Plugin instance (keys instance reuse); `None` in some tests.
    pub instance: Option<PluginInstanceId>,
    /// Latency in frames (kept while bypassed).
    pub latency: u32,
    /// Bypassed: audio passes through a delay of `latency`.
    pub bypass: bool,
    /// Sidechain input, aligned to this insert's position.
    pub sidechain: Option<InputSpec>,
}

/// A strip node: sums `inputs` in order, runs `inserts`, applies
/// `gain_l`/`gain_r` (fader × pan).
#[derive(Debug, Clone, PartialEq)]
pub struct StripSpec {
    /// The model strip (for meters); `None` in tests.
    pub strip: Option<StripId>,
    /// Inputs in summing order.
    pub inputs: Vec<InputSpec>,
    /// Inserts in processing order.
    pub inserts: Vec<InsertSpec>,
    /// Left gain after the inserts.
    pub gain_l: f32,
    /// Right gain after the inserts.
    pub gain_r: f32,
    /// False when muted (explicitly or by solo): metered silent and, for
    /// master, not written to the device.
    pub audible: bool,
}

impl StripSpec {
    /// A strip with no inputs or inserts, unity gains, audible.
    pub fn new(strip: Option<StripId>) -> StripSpec {
        StripSpec {
            strip,
            inputs: Vec::new(),
            inserts: Vec::new(),
            gain_l: 1.0,
            gain_r: 1.0,
            audible: true,
        }
    }
}

/// A graph node.
#[derive(Debug, Clone, PartialEq)]
pub enum NodeSpec {
    /// An instrument sink (the built-in synth in 3a) rendering into its
    /// node's `Post` buffer.
    Instrument {
        /// The engine target whose sink renders.
        target: TargetIdx,
        /// The instrument's latency in frames.
        latency: u32,
    },
    /// A mixer strip.
    Strip(StripSpec),
}

/// A whole graph; `master`'s output goes to the device.
#[derive(Debug, Clone, PartialEq)]
pub struct GraphSpec {
    /// Nodes, by index.
    pub nodes: Vec<NodeSpec>,
    /// Index of the master strip.
    pub master: usize,
}

struct Fnv(u64);

impl Fnv {
    fn u64(&mut self, v: u64) {
        for b in v.to_le_bytes() {
            self.0 ^= u64::from(b);
            self.0 = self.0.wrapping_mul(0x0100_0000_01b3);
        }
    }
}

fn tap_code(t: Tap) -> u64 {
    match t {
        Tap::Pre => 1,
        Tap::Post => 2,
    }
}

impl GraphSpec {
    /// The nodes node `i` reads: its inputs, then its sidechain sources
    /// (repeats kept; out-of-range indices included for `plan` to refuse).
    pub fn deps(&self, i: usize) -> Vec<usize> {
        match self.nodes.get(i) {
            Some(NodeSpec::Strip(s)) => s
                .inputs
                .iter()
                .map(|x| x.from)
                .chain(s.inserts.iter().filter_map(|k| k.sidechain.map(|x| x.from)))
                .collect(),
            _ => Vec::new(),
        }
    }

    /// A hash of the structure (Plan Ruling 15): equal shapes have equal
    /// plans and node layouts, so a new graph may take over the running
    /// graph's node state.
    pub fn shape(&self) -> u64 {
        let mut h = Fnv(0xcbf2_9ce4_8422_2325);
        h.u64(self.nodes.len() as u64);
        h.u64(self.master as u64);
        for n in &self.nodes {
            match n {
                NodeSpec::Instrument { target, latency } => {
                    h.u64(1);
                    h.u64(u64::from(target.0));
                    h.u64(u64::from(*latency));
                }
                NodeSpec::Strip(s) => {
                    h.u64(2);
                    h.u64(s.inputs.len() as u64);
                    for x in &s.inputs {
                        h.u64(x.from as u64);
                        h.u64(tap_code(x.tap));
                    }
                    h.u64(s.inserts.len() as u64);
                    for k in &s.inserts {
                        h.u64(k.instance.map_or(u64::MAX, |p| p.0));
                        h.u64(u64::from(k.latency));
                        match &k.sidechain {
                            Some(x) => {
                                h.u64(x.from as u64);
                                h.u64(tap_code(x.tap));
                            }
                            None => h.u64(u64::MAX),
                        }
                    }
                }
            }
        }
        h.0
    }
}
```

- [ ] **Step 4: Write the planner**

Create `crates/core-engine/src/graph/pdc.rs`:

```rust
//! Plugin delay compensation and evaluation order (spec §6, non-RT).

use crate::graph::spec::{GraphSpec, NodeSpec};
use std::collections::BTreeSet;

/// Most nodes a graph may have.
pub const MAX_NODES: usize = 1024;
/// Most inserts a graph may have.
pub const MAX_INSERTS: usize = 4096;

/// Why a graph cannot be planned.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PlanError {
    /// Beyond [`MAX_NODES`] or [`MAX_INSERTS`].
    #[error("graph too large: {nodes} node(s), {inserts} insert(s) (limits 1024 and 4096)")]
    TooLarge {
        /// Node count.
        nodes: usize,
        /// Insert count.
        inserts: usize,
    },
    /// The edges are cyclic.
    #[error("graph has a cycle")]
    Cycle,
    /// An edge or the master names a node that does not exist.
    #[error("graph refers to node {node}, which does not exist")]
    BadIndex {
        /// The missing index.
        node: usize,
    },
    /// An edge would need more than the maximum delay.
    #[error("a compensation delay of {frames} frames exceeds the 2 s limit")]
    LatencyTooLarge {
        /// The delay needed.
        frames: u64,
    },
}

/// The result of planning.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    /// Topological order, ties broken by lower index.
    pub order: Vec<usize>,
    /// Per node, when its (aligned) inputs arrive.
    pub t_in: Vec<u64>,
    /// Per node, when its output is ready (`t_in + latency`).
    pub t_out: Vec<u64>,
    /// Per node, the delay of each input edge.
    pub input_delays: Vec<Vec<u32>>,
    /// Per strip node, the delay of each insert's sidechain (0 when none).
    pub sidechain_delays: Vec<Vec<u32>>,
    /// Total latency `L` (master's `t_out`).
    pub latency: u64,
}

fn at(v: &[u64], i: usize) -> u64 {
    v.get(i).copied().unwrap_or(0)
}

fn bounded(frames: u64, max: u64) -> Result<u32, PlanError> {
    if frames > max {
        return Err(PlanError::LatencyTooLarge { frames });
    }
    u32::try_from(frames).map_err(|_| PlanError::LatencyTooLarge { frames })
}

/// Plans `spec` (see the PDC rule in spec §6). `max_delay` bounds any one
/// edge delay (2 s of frames in the engine).
pub fn plan(spec: &GraphSpec, max_delay: u64) -> Result<Plan, PlanError> {
    let n = spec.nodes.len();
    let inserts: usize = spec
        .nodes
        .iter()
        .map(|x| match x {
            NodeSpec::Strip(s) => s.inserts.len(),
            NodeSpec::Instrument { .. } => 0,
        })
        .sum();
    if n > MAX_NODES || inserts > MAX_INSERTS {
        return Err(PlanError::TooLarge { nodes: n, inserts });
    }
    if spec.master >= n {
        return Err(PlanError::BadIndex { node: spec.master });
    }
    let mut indeg = vec![0usize; n];
    let mut succ: Vec<Vec<usize>> = vec![Vec::new(); n];
    for i in 0..n {
        for d in spec.deps(i) {
            if d >= n {
                return Err(PlanError::BadIndex { node: d });
            }
            if let Some(v) = succ.get_mut(d) {
                v.push(i);
            }
            if let Some(x) = indeg.get_mut(i) {
                *x += 1;
            }
        }
    }
    let mut ready: BTreeSet<usize> = (0..n).filter(|i| indeg.get(*i) == Some(&0)).collect();
    let mut order = Vec::with_capacity(n);
    while let Some(i) = ready.pop_first() {
        order.push(i);
        for &s in succ.get(i).map(Vec::as_slice).unwrap_or(&[]) {
            if let Some(x) = indeg.get_mut(s) {
                *x -= 1;
                if *x == 0 {
                    ready.insert(s);
                }
            }
        }
    }
    if order.len() != n {
        return Err(PlanError::Cycle);
    }
    let mut t_in = vec![0u64; n];
    let mut t_out = vec![0u64; n];
    let mut input_delays = vec![Vec::new(); n];
    let mut sidechain_delays = vec![Vec::new(); n];
    for &i in &order {
        let Some(node) = spec.nodes.get(i) else {
            continue;
        };
        match node {
            NodeSpec::Instrument { latency, .. } => {
                if let Some(x) = t_out.get_mut(i) {
                    *x = u64::from(*latency);
                }
            }
            NodeSpec::Strip(s) => {
                let mut tin = s.inputs.iter().map(|x| at(&t_out, x.from)).max().unwrap_or(0);
                let mut prefix = 0u64;
                for k in &s.inserts {
                    if let Some(sc) = &k.sidechain {
                        tin = tin.max(at(&t_out, sc.from).saturating_sub(prefix));
                    }
                    prefix += u64::from(k.latency);
                }
                let mut d = Vec::with_capacity(s.inputs.len());
                for x in &s.inputs {
                    d.push(bounded(tin.saturating_sub(at(&t_out, x.from)), max_delay)?);
                }
                let mut sd = Vec::with_capacity(s.inserts.len());
                let mut before = 0u64;
                for k in &s.inserts {
                    sd.push(match &k.sidechain {
                        Some(sc) => bounded((tin + before).saturating_sub(at(&t_out, sc.from)), max_delay)?,
                        None => 0,
                    });
                    before += u64::from(k.latency);
                }
                if let (Some(a), Some(b), Some(c), Some(e)) = (
                    t_in.get_mut(i),
                    t_out.get_mut(i),
                    input_delays.get_mut(i),
                    sidechain_delays.get_mut(i),
                ) {
                    *a = tin;
                    *b = tin + prefix;
                    *c = d;
                    *e = sd;
                }
            }
        }
    }
    Ok(Plan {
        latency: at(&t_out, spec.master),
        order,
        t_in,
        t_out,
        input_delays,
        sidechain_delays,
    })
}
```

Replace `crates/core-engine/src/graph/mod.rs` with:

```rust
//! The mixer render graph (spec §6, §7): strip DSP (`dsp`), delay lines
//! (`delay`), insert processors (`node`), the graph description (`spec`)
//! and PDC planning (`pdc`).

pub mod delay;
pub mod dsp;
pub mod node;
pub mod pdc;
pub mod spec;

pub use node::{
    EffectNode, InsertFactory, NoInserts, ProcessStatus, TestInserts, TestMode, TestNode,
};
pub use pdc::{MAX_INSERTS, MAX_NODES, Plan, PlanError, plan};
pub use spec::{GraphSpec, InputSpec, InsertSpec, NodeSpec, StripSpec, Tap};
```

- [ ] **Step 5: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-engine --test graph_plan
```

Expected: 6 passed.

- [ ] **Step 6: Lint and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
git add crates/core-engine/src/graph/spec.rs crates/core-engine/src/graph/pdc.rs crates/core-engine/src/graph/mod.rs crates/core-engine/tests/graph_plan.rs
git commit -m "feat(core-engine): graph description and PDC planner

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---
### Task 9: The runtime graph, the executor and the worker pool

**Goal:** Build the immutable runtime `Graph` from a spec and plan (node buffers, per-edge delay lines, insert states adopted by instance), render it with the lock-free ready-queue executor on the calling thread plus `W` workers (spec §7), with fault bypass, meters, deadline handling, the `RtWaker`/`RtWorkgroup`/`RtPlatform` seams with a portable `StdPlatform`, and a `Runner` harness. Output must be bit-identical for every worker count and block size.

**Files:**
- Modify: `crates/core-engine/Cargo.toml` (add `crossbeam-queue = "0.3"`)
- Modify: `crates/core-engine/src/render.rs` (new fault bits, counters, meters, fault board, DSP load in `Shared`)
- Modify: `crates/core-engine/src/rt_check.rs` (`set_arm_workers`, `arm_workers`)
- Create: `crates/core-engine/src/graph/exec.rs`
- Create: `crates/core-engine/src/pool.rs`
- Modify: `crates/core-engine/src/graph/mod.rs`, `crates/core-engine/src/lib.rs`
- Test: `crates/core-engine/tests/graph_exec.rs`, `crates/core-engine/tests/graph_parallel.rs`

**Interfaces:**
- Consumes: Task 7 (`dsp`, `DelayLine`, `EffectNode`, `ProcessStatus`, `TestNode`, `TestMode`), Task 8 (`GraphSpec`, `NodeSpec`, `InsertSpec`, `Tap`, `Plan`, `plan`, `MAX_NODES`, `MAX_INSERTS`), `render::{Sink, Shared, Faults}`, `schedule::MAX_TARGETS`, `EngineError::Thread`.
- Produces:
  - `render::Faults::{GRAPH_TOO_LARGE = 32, LATENCY_TOO_LARGE = 64, PLUGIN_FAULT = 128, WORKGROUP_JOIN_FAILED = 256, THREAD_POLICY_FAILED = 512, GRAPH_INVALID = 1024}` (names `graph-too-large`, `latency-too-large`, `plugin-fault`, `workgroup-join-failed`, `thread-policy-failed`, `graph-invalid`); `Counters { …, deadline_misses, busy_blocks, plugin_faults }`.
  - `render::MeterCell { hold(l, r) /* RT */, take() -> (f32, f32) }`, `render::DspLoad { last: f32, peak: f32 }`; `Shared::{meter(usize) -> Option<&MeterCell>, report_plugin_fault(usize, Option<PluginInstanceId>), take_plugin_faults() -> Vec<PluginInstanceId>, set_load(f32), take_load() -> DspLoad, count_deadline_miss(), count_busy_block()}`; `Shared: Default`.
  - `rt_check::{set_arm_workers(bool), arm_workers() -> bool}`.
  - `graph::exec::{FAULT_LIMIT = 16, SinkTable (Default; get_mut, iter_mut), InsertInit::{Adopt(usize), Fresh(Box<dyn EffectNode>), Placeholder}, Graph, Runner}`; `Graph::build(&GraphSpec, &Plan, max_frames: u32, init: impl FnMut(usize, &InsertSpec) -> InsertInit) -> Graph`, `Graph::{latency() -> u64, shape() -> u64, max_frames() -> u32, node_count() -> usize, instruments() -> u16}`; `Graph::prepare(&mut self, max_frames: u32)`; crate `Graph::{adopt_from(&mut self, &mut Graph), write_master(&self, &mut [&mut [f32]], u32)}`.
  - crate `exec::{Exec, RunResult::{Done, Missed}, Job}` with `Exec::{new(Option<Arc<PoolShared>>) -> Box<Exec>, try_finish(&mut self, until: Option<Instant>) -> bool, wait_idle(&mut self), run(&mut self, &Graph, &SinkTable, &Shared, frames, Option<Instant>) -> RunResult}`.
  - `Runner::{serial(Graph), parallel(Graph, workers, Arc<dyn RtPlatform>) -> Result<Runner, EngineError>, block(frames, &mut [f32], &mut [f32]) -> bool, render(total, &[u32]) -> (Vec<f32>, Vec<f32>), replace_graph(Graph) -> Graph, sinks_mut() -> &mut SinkTable, shared() -> &Shared, set_deadline(Option<Duration>), set_callback_helps(bool), set_env(RtEnv)}`.
  - `pool::{SPIN, RtWaker, RtWorkgroup, WorkgroupToken(pub Box<dyn Any + Send>), RtPlatform, RtEnv { period_ns, workgroup }, StdPlatform, Pool, PoolShared}`; `RtWaker::{wake(n) /* RT */, wait(), set_threads(Vec<Thread>)}`; `RtWorkgroup::{join() -> Result<WorkgroupToken, String>, leave(WorkgroupToken)}`; `RtPlatform::{waker() -> Box<dyn RtWaker> /* one per worker */, promote(period_ns) -> Result<(), String>}`; `Pool::{new(workers, Arc<dyn RtPlatform>, Arc<Shared>) -> Result<Pool, EngineError>, handle() -> Arc<PoolShared>, workers(), set_env(RtEnv), set_callback_helps(bool)}`; `PoolShared::{workers(), callback_helps(), set_callback_helps(bool)}`.

- [ ] **Step 1: Write the failing tests**

Create `crates/core-engine/tests/graph_exec.rs`:

```rust
use core_engine::backend::{InstrumentNode, StereoBuf};
use core_engine::graph::exec::{Graph, InsertInit, Runner};
use core_engine::graph::{
    GraphSpec, InputSpec, InsertSpec, NodeSpec, StripSpec, Tap, TestMode, TestNode, plan,
};
use core_engine::render::{Faults, Sink};
use core_engine::schedule::TargetIdx;
use core_model::ids::PluginInstanceId;

/// A spec under construction plus the test node of every insert, in
/// global insert order. The last strip added is master.
struct Net {
    spec: GraphSpec,
    modes: Vec<(TestMode, u32)>,
}

impl Net {
    fn new() -> Net {
        Net { spec: GraphSpec { nodes: Vec::new(), master: 0 }, modes: Vec::new() }
    }

    fn strip(&mut self, inputs: &[usize], inserts: &[(TestMode, u32)]) -> usize {
        let mut s = StripSpec::new(None);
        s.inputs = inputs.iter().map(|&from| InputSpec { from, tap: Tap::Post, gain: 1.0 }).collect();
        for (mode, latency) in inserts {
            s.inserts.push(InsertSpec {
                instance: Some(PluginInstanceId(self.modes.len() as u64)),
                latency: *latency,
                bypass: false,
                sidechain: None,
            });
            self.modes.push((mode.clone(), *latency));
        }
        self.spec.nodes.push(NodeSpec::Strip(s));
        self.spec.master = self.spec.nodes.len() - 1;
        self.spec.master
    }

    fn strip_mut(&mut self, i: usize) -> &mut StripSpec {
        match &mut self.spec.nodes[i] {
            NodeSpec::Strip(s) => s,
            NodeSpec::Instrument { .. } => panic!("not a strip"),
        }
    }

    fn graph(&self) -> Graph {
        let p = plan(&self.spec, 96_000).unwrap();
        Graph::build(&self.spec, &p, 1024, |idx, _| {
            let (m, l) = &self.modes[idx];
            InsertInit::Fresh(Box::new(TestNode::new(m.clone(), *l)))
        })
    }
}

fn nonzero(v: &[f32]) -> Vec<(usize, f32)> {
    v.iter().enumerate().filter(|(_, x)| **x != 0.0).map(|(i, x)| (i, *x)).collect()
}

/// S (impulse) feeds X (latency 0), Y (latency 100) and Z (37 + 213);
/// master sums them.
fn three_paths() -> Net {
    let mut n = Net::new();
    let s = n.strip(&[], &[(TestMode::Impulse, 0)]);
    let x = n.strip(&[s], &[(TestMode::Gain(1.0), 0)]);
    let y = n.strip(&[s], &[(TestMode::Gain(1.0), 100)]);
    let z = n.strip(&[s], &[(TestMode::Gain(1.0), 37), (TestMode::Gain(1.0), 213)]);
    n.strip(&[x, y, z], &[]);
    n
}

#[test]
fn impulse_arrives_at_master_on_the_same_sample_on_every_path() {
    let net = three_paths();
    assert_eq!(net.graph().latency(), 250);
    for sizes in [&[128u32][..], &[1, 7, 64][..]] {
        let mut r = Runner::serial(net.graph());
        let (l, rr) = r.render(1000, sizes);
        assert_eq!(nonzero(&l), vec![(250, 3.0)], "{sizes:?}");
        assert_eq!(nonzero(&rr), vec![(250, 3.0)]);
    }
}

#[test]
fn bypass_keeps_timing() {
    let mut net = three_paths();
    net.strip_mut(2).inserts[0].bypass = true;
    let mut r = Runner::serial(net.graph());
    let (l, _) = r.render(1000, &[256]);
    assert_eq!(nonzero(&l), vec![(250, 3.0)]);
}

#[test]
fn sidechain_is_aligned_to_its_insert() {
    let mut n = Net::new();
    let k = n.strip(&[], &[(TestMode::Impulse, 0)]);
    let s = n.strip(&[], &[(TestMode::Impulse, 0)]);
    let p = n.strip(&[s], &[(TestMode::Gain(1.0), 50), (TestMode::AddSidechain, 0)]);
    n.strip_mut(p).inserts[1].sidechain = Some(InputSpec { from: k, tap: Tap::Post, gain: 1.0 });
    n.strip(&[p], &[]);
    let mut r = Runner::serial(n.graph());
    let (l, _) = r.render(300, &[64]);
    assert_eq!(nonzero(&l), vec![(50, 2.0)]);
}

fn noise_into_master() -> Net {
    let mut n = Net::new();
    let s = n.strip(&[], &[(TestMode::Noise { seed: 1, level: 0.25 }, 0)]);
    n.strip(&[s], &[]);
    n
}

#[test]
fn muted_edges_are_silent_and_unmetered() {
    let mut r = Runner::serial(noise_into_master().graph());
    let (l, _) = r.render(256, &[256]);
    assert!(!nonzero(&l).is_empty());
    assert!(r.shared().meter(0).unwrap().take().0 > 0.0);
    assert!(r.shared().meter(1).unwrap().take().0 > 0.0);

    let mut net = noise_into_master();
    net.strip_mut(1).inputs[0].gain = 0.0;
    net.strip_mut(0).audible = false;
    let mut r = Runner::serial(net.graph());
    let (l, _) = r.render(256, &[256]);
    assert!(nonzero(&l).is_empty());
    assert_eq!(r.shared().meter(0).unwrap().take(), (0.0, 0.0));

    let mut net = noise_into_master();
    net.strip_mut(1).audible = false;
    let mut r = Runner::serial(net.graph());
    let (l, _) = r.render(256, &[256]);
    assert!(nonzero(&l).is_empty(), "a muted master writes silence");
}

fn faulty(mode: TestMode) {
    let mut n = Net::new();
    let s = n.strip(&[], &[(TestMode::Noise { seed: 7, level: 0.25 }, 0), (mode, 0)]);
    n.strip(&[s], &[]);
    let mut r = Runner::serial(n.graph());
    let mut l = [0.0f32; 64];
    let mut rr = [0.0f32; 64];
    for block in 1..=16 {
        assert!(r.block(64, &mut l, &mut rr));
        assert!(nonzero(&l).is_empty(), "block {block} is silenced");
        let f = r.shared().take_faults();
        assert_eq!(f.contains(Faults::PLUGIN_FAULT), block == 16, "block {block}");
    }
    assert_eq!(r.shared().take_plugin_faults(), vec![PluginInstanceId(1)]);
    assert!(r.shared().take_plugin_faults().is_empty(), "read once");
    assert!(r.block(64, &mut l, &mut rr));
    assert!(!nonzero(&l).is_empty(), "the bypassed insert passes the noise");
}

#[test]
fn faulty_insert_is_zeroed_then_bypassed() {
    faulty(TestMode::Nan);
}

#[test]
fn error_status_counts_as_fault() {
    faulty(TestMode::Error);
}

#[test]
fn placeholder_inserts_pass_audio() {
    let net = noise_into_master();
    let mut with = net.spec.clone();
    if let NodeSpec::Strip(s) = &mut with.nodes[0] {
        s.inserts.push(InsertSpec { instance: Some(PluginInstanceId(9)), latency: 0, bypass: false, sidechain: None });
    }
    let p = plan(&with, 96_000).unwrap();
    let g = Graph::build(&with, &p, 1024, |idx, _| {
        if idx == 0 {
            InsertInit::Fresh(Box::new(TestNode::new(TestMode::Noise { seed: 1, level: 0.25 }, 0)))
        } else {
            InsertInit::Placeholder
        }
    });
    let (a, _) = Runner::serial(g).render(512, &[128]);
    let (b, _) = Runner::serial(net.graph()).render(512, &[128]);
    assert_eq!(a, b);
}

/// S (noise) feeds X (latency 0) and Y (×0.5, latency 40); master's
/// input from X carries a 40-frame compensation delay.
fn compensated() -> Net {
    let mut n = Net::new();
    let s = n.strip(&[], &[(TestMode::Noise { seed: 5, level: 0.25 }, 0)]);
    let x = n.strip(&[s], &[(TestMode::Gain(1.0), 0)]);
    let y = n.strip(&[s], &[(TestMode::Gain(0.5), 40)]);
    n.strip(&[x, y], &[]);
    n
}

#[test]
fn same_shape_adoption_keeps_delay_state() {
    let net = compensated();
    let (reference, _) = Runner::serial(net.graph()).render(200, &[50]);
    let mut r = Runner::serial(net.graph());
    let (first, _) = r.render(100, &[50]);
    // Same structure, new master gain: everything is adopted.
    let mut louder = compensated();
    louder.strip_mut(3).gain_l = 2.0;
    louder.strip_mut(3).gain_r = 2.0;
    assert_eq!(louder.spec.shape(), net.spec.shape());
    let p = plan(&louder.spec, 96_000).unwrap();
    let g = Graph::build(&louder.spec, &p, 1024, |idx, _| InsertInit::Adopt(idx));
    r.replace_graph(g);
    let (second, _) = r.render(100, &[50]);
    assert_eq!(first, reference[..100].to_vec());
    let doubled: Vec<f32> = reference[100..].iter().map(|x| x * 2.0).collect();
    assert_eq!(second, doubled);
}

#[test]
fn structural_change_keeps_instances() {
    let net = noise_into_master();
    let (reference, _) = Runner::serial(net.graph()).render(200, &[50]);
    let mut r = Runner::serial(net.graph());
    let _ = r.render(100, &[50]);
    // Add a silent strip feeding master: a new shape, the noise insert adopted.
    let mut wider = Net::new();
    let s = wider.strip(&[], &[(TestMode::Noise { seed: 1, level: 0.25 }, 0)]);
    let q = wider.strip(&[], &[]);
    wider.strip(&[s, q], &[]);
    assert_ne!(wider.spec.shape(), net.spec.shape());
    let p = plan(&wider.spec, 96_000).unwrap();
    let g = Graph::build(&wider.spec, &p, 1024, |idx, _| InsertInit::Adopt(idx));
    r.replace_graph(g);
    let (second, _) = r.render(100, &[50]);
    assert_eq!(second, reference[100..].to_vec());
}

struct Dc(f32);

impl InstrumentNode for Dc {
    fn midi(&mut self, _: u32, _: &[u8]) {}
    fn render(&mut self, frames: u32, out: &mut StereoBuf) {
        out.left[..frames as usize].fill(self.0);
        out.right[..frames as usize].fill(-self.0);
    }
    fn reset(&mut self) {}
}

#[test]
fn instrument_nodes_render_their_sink() {
    let mut s = StripSpec::new(None);
    s.inputs.push(InputSpec { from: 0, tap: Tap::Post, gain: 1.0 });
    let spec = GraphSpec {
        nodes: vec![NodeSpec::Instrument { target: TargetIdx(2), latency: 0 }, NodeSpec::Strip(s)],
        master: 1,
    };
    let p = plan(&spec, 96_000).unwrap();
    let g = Graph::build(&spec, &p, 256, |_, _| InsertInit::Placeholder);
    assert_eq!(g.instruments(), 1 << 2);
    let mut r = Runner::serial(g);
    *r.sinks_mut().get_mut(2).unwrap() = Some(Sink::Instrument(Box::new(Dc(0.5))));
    let (l, rr) = r.render(64, &[64]);
    assert!(l.iter().all(|x| *x == 0.5) && rr.iter().all(|x| *x == -0.5));
}
```

Create `crates/core-engine/tests/graph_parallel.rs`:

```rust
use core_engine::graph::exec::{Graph, InsertInit, Runner};
use core_engine::graph::{
    GraphSpec, InputSpec, InsertSpec, NodeSpec, StripSpec, Tap, TestMode, TestNode, plan,
};
use core_engine::pool::{RtEnv, RtPlatform, RtWaker, RtWorkgroup, StdPlatform, WorkgroupToken};
use core_engine::render::Faults;
use core_model::ids::PluginInstanceId;
use proptest::prelude::*;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// One random strip: inputs `(raw source, pre-fader, gain index)` and
/// inserts `(kind, latency, bypass, raw sidechain source)`; raw sources
/// are reduced modulo the strip's index so the graph is a DAG.
#[derive(Debug, Clone)]
struct RStrip {
    inputs: Vec<(u16, bool, u8)>,
    inserts: Vec<(u8, u16, bool, Option<u16>)>,
}

const GAINS: [f32; 4] = [1.0, 0.5, 0.25, 0.0];

fn build(strips: &[RStrip]) -> (GraphSpec, Vec<(TestMode, u32)>) {
    let mut spec = GraphSpec { nodes: Vec::new(), master: strips.len() };
    let mut modes: Vec<(TestMode, u32)> = Vec::new();
    for (i, r) in strips.iter().enumerate() {
        let mut s = StripSpec::new(None);
        if i > 0 {
            for &(raw, pre, g) in &r.inputs {
                s.inputs.push(InputSpec {
                    from: raw as usize % i,
                    tap: if pre { Tap::Pre } else { Tap::Post },
                    gain: GAINS[g as usize % 4],
                });
            }
        }
        if s.inputs.is_empty() {
            s.inserts.push(InsertSpec { instance: Some(PluginInstanceId(modes.len() as u64)), latency: 0, bypass: false, sidechain: None });
            modes.push((TestMode::Noise { seed: i as u32 + 1, level: 0.25 }, 0));
        }
        for &(kind, lat, bypass, sc) in &r.inserts {
            let mode = match kind % 4 {
                0 => TestMode::Gain(0.5),
                1 => TestMode::Gain(1.0),
                2 => TestMode::Noise { seed: i as u32 * 31 + u32::from(lat), level: 0.125 },
                _ => TestMode::AddSidechain,
            };
            let sidechain = if i > 0 {
                sc.map(|raw| InputSpec { from: raw as usize % i, tap: Tap::Post, gain: 1.0 })
            } else {
                None
            };
            s.inserts.push(InsertSpec {
                instance: Some(PluginInstanceId(modes.len() as u64)),
                latency: u32::from(lat % 300),
                bypass,
                sidechain,
            });
            modes.push((mode, u32::from(lat % 300)));
        }
        s.gain_l = 0.75;
        s.gain_r = 0.5;
        spec.nodes.push(NodeSpec::Strip(s));
    }
    let mut master = StripSpec::new(None);
    master.inputs = (0..strips.len()).map(|from| InputSpec { from, tap: Tap::Post, gain: 1.0 }).collect();
    spec.nodes.push(NodeSpec::Strip(master));
    (spec, modes)
}

fn graph(spec: &GraphSpec, modes: &[(TestMode, u32)]) -> Graph {
    let p = plan(spec, 96_000).unwrap();
    Graph::build(spec, &p, 512, |idx, _| {
        InsertInit::Fresh(Box::new(TestNode::new(modes[idx].0.clone(), modes[idx].1)))
    })
}

fn bits(v: &[f32]) -> Vec<u32> {
    v.iter().map(|x| x.to_bits()).collect()
}

const SIZES: [u32; 5] = [1, 7, 64, 333, 512];

fn assert_same(strips: &[RStrip], workers: usize) {
    let (spec, modes) = build(strips);
    let (rl, rr) = Runner::serial(graph(&spec, &modes)).render(3000, &[512]);
    let mut p = Runner::parallel(graph(&spec, &modes), workers, Arc::new(StdPlatform)).unwrap();
    let (pl, pr) = p.render(3000, &SIZES);
    assert_eq!(bits(&rl), bits(&pl), "left, {workers} worker(s)");
    assert_eq!(bits(&rr), bits(&pr), "right, {workers} worker(s)");
}

/// A fixed 14-strip graph from a small LCG.
fn fixed() -> Vec<RStrip> {
    let mut x: u32 = 12345;
    let mut next = || {
        x = x.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        (x >> 16) as u16
    };
    (0..14)
        .map(|_| RStrip {
            inputs: (0..(next() % 3)).map(|_| (next(), next() % 2 == 0, (next() % 4) as u8)).collect(),
            inserts: (0..(next() % 3))
                .map(|_| {
                    let (kind, lat, bypass, pick, src) = (next(), next(), next(), next(), next());
                    ((kind % 4) as u8, lat % 300, bypass % 5 == 0, (pick % 2 == 0).then_some(src))
                })
                .collect(),
        })
        .collect()
}

#[test]
fn workers_match_serial_for_every_worker_count() {
    let strips = fixed();
    for w in 1..=8 {
        assert_same(&strips, w);
    }
}

fn arb_strip() -> impl Strategy<Value = RStrip> {
    (
        proptest::collection::vec((any::<u16>(), any::<bool>(), 0u8..4), 0..3),
        proptest::collection::vec((0u8..4, 0u16..300, any::<bool>(), proptest::option::of(any::<u16>())), 0..3),
    )
        .prop_map(|(inputs, inserts)| RStrip { inputs, inserts })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]
    #[test]
    fn random_graphs_render_identically(strips in proptest::collection::vec(arb_strip(), 2..16), w in 1usize..=8) {
        assert_same(&strips, w);
    }
}

/// S (noise) → B (stall) → master, one worker, the callback not helping.
fn stalled(flag: Arc<AtomicBool>) -> Runner {
    let mut spec = GraphSpec { nodes: Vec::new(), master: 2 };
    let mut s = StripSpec::new(None);
    s.inserts.push(InsertSpec { instance: None, latency: 0, bypass: false, sidechain: None });
    spec.nodes.push(NodeSpec::Strip(s));
    let mut b = StripSpec::new(None);
    b.inputs.push(InputSpec { from: 0, tap: Tap::Post, gain: 1.0 });
    b.inserts.push(InsertSpec { instance: None, latency: 0, bypass: false, sidechain: None });
    spec.nodes.push(NodeSpec::Strip(b));
    let mut m = StripSpec::new(None);
    m.inputs.push(InputSpec { from: 1, tap: Tap::Post, gain: 1.0 });
    spec.nodes.push(NodeSpec::Strip(m));
    let p = plan(&spec, 96_000).unwrap();
    let g = Graph::build(&spec, &p, 512, |idx, _| {
        if idx == 0 {
            InsertInit::Fresh(Box::new(TestNode::new(TestMode::Noise { seed: 3, level: 0.25 }, 0)))
        } else {
            InsertInit::Fresh(Box::new(TestNode::new(TestMode::Stall(Arc::clone(&flag)), 0)))
        }
    });
    let mut r = Runner::parallel(g, 1, Arc::new(StdPlatform)).unwrap();
    r.set_callback_helps(false);
    r.set_deadline(Some(Duration::from_millis(5)));
    r
}

#[test]
fn missed_deadline_is_silent_and_recovers() {
    let flag = Arc::new(AtomicBool::new(false));
    let mut r = stalled(Arc::clone(&flag));
    let mut l = [1.0f32; 64];
    let mut rr = [1.0f32; 64];
    let t = Instant::now();
    assert!(!r.block(64, &mut l, &mut rr), "the stalled node misses the deadline");
    assert!(t.elapsed() < Duration::from_millis(500));
    assert!(l.iter().all(|x| *x == 0.0));
    assert!(!r.block(64, &mut l, &mut rr), "the next block finds the straggler busy");
    flag.store(true, Ordering::Release);
    std::thread::sleep(Duration::from_millis(50));
    // No deadline for the check, so a slow machine cannot make it flaky.
    r.set_deadline(None);
    assert!(r.block(64, &mut l, &mut rr), "recovered");
    assert!(l.iter().any(|x| *x != 0.0));
}

#[test]
fn dropping_a_runner_waits_for_stragglers() {
    let flag = Arc::new(AtomicBool::new(false));
    let mut r = stalled(Arc::clone(&flag));
    let (mut l, mut rr) = ([0.0f32; 64], [0.0f32; 64]);
    assert!(!r.block(64, &mut l, &mut rr));
    let f2 = Arc::clone(&flag);
    let releaser = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(60));
        f2.store(true, Ordering::Release);
    });
    let t = Instant::now();
    drop(r);
    assert!(t.elapsed() >= Duration::from_millis(40), "drop waited for the stalled node");
    releaser.join().unwrap();
}

#[derive(Default)]
struct FakePlatform {
    promotes: AtomicUsize,
    fail: bool,
}

impl RtPlatform for FakePlatform {
    fn waker(&self) -> Box<dyn RtWaker> {
        StdPlatform.waker()
    }
    fn promote(&self, _: u64) -> Result<(), String> {
        self.promotes.fetch_add(1, Ordering::SeqCst);
        if self.fail { Err("refused".into()) } else { Ok(()) }
    }
}

#[derive(Default)]
struct FakeWorkgroup {
    joins: AtomicUsize,
    leaves: AtomicUsize,
    fail: bool,
}

impl RtWorkgroup for FakeWorkgroup {
    fn join(&self) -> Result<WorkgroupToken, String> {
        if self.fail {
            return Err("cancelled".into());
        }
        self.joins.fetch_add(1, Ordering::SeqCst);
        Ok(WorkgroupToken(Box::new(7u32)))
    }
    fn leave(&self, token: WorkgroupToken) {
        assert_eq!(token.0.downcast::<u32>().map(|b| *b).ok(), Some(7));
        self.leaves.fetch_add(1, Ordering::SeqCst);
    }
}

fn eventually(f: impl Fn() -> bool) -> bool {
    for _ in 0..400 {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    false
}

fn small() -> Graph {
    let (spec, modes) = build(&fixed()[..3]);
    graph(&spec, &modes)
}

#[test]
fn env_changes_rejoin_the_workgroup() {
    let platform = Arc::new(FakePlatform::default());
    let r = Runner::parallel(small(), 3, platform.clone()).unwrap();
    let a = Arc::new(FakeWorkgroup::default());
    r.set_env(RtEnv { period_ns: 10_666_667, workgroup: Some(a.clone()) });
    assert!(eventually(|| a.joins.load(Ordering::SeqCst) == 3));
    assert!(eventually(|| platform.promotes.load(Ordering::SeqCst) == 3));
    let b = Arc::new(FakeWorkgroup::default());
    r.set_env(RtEnv { period_ns: 5_333_333, workgroup: Some(b.clone()) });
    assert!(eventually(|| a.leaves.load(Ordering::SeqCst) == 3 && b.joins.load(Ordering::SeqCst) == 3));
    drop(r);
    assert_eq!(b.leaves.load(Ordering::SeqCst), 3, "workers leave on exit");
    assert_eq!(platform.promotes.load(Ordering::SeqCst), 6, "promoted again for the new period");
}

#[test]
fn failed_join_and_promotion_are_reported() {
    let platform = Arc::new(FakePlatform { promotes: AtomicUsize::new(0), fail: true });
    let r = Runner::parallel(small(), 2, platform).unwrap();
    let wg = Arc::new(FakeWorkgroup { fail: true, ..FakeWorkgroup::default() });
    r.set_env(RtEnv { period_ns: 10_000_000, workgroup: Some(wg) });
    assert!(eventually(|| {
        let f = r.shared().take_faults();
        f.contains(Faults::WORKGROUP_JOIN_FAILED) || f.contains(Faults::THREAD_POLICY_FAILED)
    }));
}
```

- [ ] **Step 2: Run the tests to verify they fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-engine --test graph_exec --test graph_parallel
```

Expected: compile errors — no module `exec` in `graph`, no `pool`.

- [ ] **Step 3: Shared state for meters, faults and load**

In `crates/core-engine/Cargo.toml` add to `[dependencies]`: `crossbeam-queue = "0.3"`.

In `crates/core-engine/src/rt_check.rs` add `use std::sync::atomic::AtomicBool;` and:

```rust
static ARM_WORKERS: AtomicBool = AtomicBool::new(false);

/// Makes render workers arm themselves around every job (tests set this
/// when they install the counting allocator).
pub fn set_arm_workers(on: bool) {
    ARM_WORKERS.store(on, Ordering::Relaxed);
}

/// Whether render workers arm themselves (always with `rt-check`).
pub fn arm_workers() -> bool {
    ARM_WORKERS.load(Ordering::Relaxed) || cfg!(feature = "rt-check")
}
```

In `crates/core-engine/src/render.rs`:

Add imports `use crate::graph::{MAX_INSERTS, MAX_NODES};` and `use core_model::ids::PluginInstanceId;`.

Add after `pub const PANIC: Faults = Faults(16);`:

```rust
    /// A recompiled graph exceeded 1024 nodes or 4096 inserts; the
    /// previous graph keeps playing.
    pub const GRAPH_TOO_LARGE: Faults = Faults(32);
    /// A recompiled graph needs more than 2 s of delay on an edge; the
    /// previous graph keeps playing.
    pub const LATENCY_TOO_LARGE: Faults = Faults(64);
    /// An insert produced 16 faulty blocks in a row and was bypassed
    /// (`Engine::plugin_faults` names it).
    pub const PLUGIN_FAULT: Faults = Faults(128);
    /// A render worker could not join the device's workgroup.
    pub const WORKGROUP_JOIN_FAILED: Faults = Faults(256);
    /// A render worker could not get real-time scheduling.
    pub const THREAD_POLICY_FAILED: Faults = Faults(512);
    /// A recompiled graph was invalid (cycle or bad index); the previous
    /// graph keeps playing.
    pub const GRAPH_INVALID: Faults = Faults(1024);
```

and extend the array in `names()` with:

```rust
            (Faults::GRAPH_TOO_LARGE, "graph-too-large"),
            (Faults::LATENCY_TOO_LARGE, "latency-too-large"),
            (Faults::PLUGIN_FAULT, "plugin-fault"),
            (Faults::WORKGROUP_JOIN_FAILED, "workgroup-join-failed"),
            (Faults::THREAD_POLICY_FAILED, "thread-policy-failed"),
            (Faults::GRAPH_INVALID, "graph-invalid"),
```

Add three fields at the end of `struct Counters`:

```rust
    /// Blocks rendered as silence because the graph missed its deadline.
    pub deadline_misses: u64,
    /// Blocks rendered as silence because nodes of a missed block were
    /// still running.
    pub busy_blocks: u64,
    /// Inserts bypassed for faults.
    pub plugin_faults: u64,
```

Replace `#[derive(Debug, Default)] pub struct Shared { … }` with:

```rust
/// Peak meter of one graph node: the largest |sample| per channel since
/// the last read, as `f32` bits (non-negative floats order like their
/// bits, so `fetch_max` works).
#[derive(Debug, Default)]
pub struct MeterCell {
    l: AtomicU32,
    r: AtomicU32,
}

impl MeterCell {
    /// Raises the held peaks. // RT
    pub fn hold(&self, l: f32, r: f32) {
        if l.is_finite() && l > 0.0 {
            self.l.fetch_max(l.to_bits(), Ordering::Relaxed);
        }
        if r.is_finite() && r > 0.0 {
            self.r.fetch_max(r.to_bits(), Ordering::Relaxed);
        }
    }

    /// Reads and clears the held peaks.
    pub fn take(&self) -> (f32, f32) {
        (
            f32::from_bits(self.l.swap(0, Ordering::Relaxed)),
            f32::from_bits(self.r.swap(0, Ordering::Relaxed)),
        )
    }
}

/// DSP load: render time ÷ block duration.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct DspLoad {
    /// The last block.
    pub last: f32,
    /// The highest since the last read.
    pub peak: f32,
}

/// State shared between the renderer, its workers and the control side
/// (atomics only).
#[derive(Debug)]
pub struct Shared {
    position: AtomicU64,
    state: AtomicU8,
    faults: AtomicU32,
    ended: AtomicBool,
    midi_dropped: AtomicU64,
    swaps_deferred: AtomicU64,
    xruns: AtomicU64,
    deadline_misses: AtomicU64,
    busy_blocks: AtomicU64,
    plugin_faults: AtomicU64,
    load_last: AtomicU32,
    load_peak: AtomicU32,
    meters: Box<[MeterCell]>,
    board: Box<[AtomicU64]>,
}

impl Default for Shared {
    fn default() -> Shared {
        Shared {
            position: AtomicU64::new(0),
            state: AtomicU8::new(0),
            faults: AtomicU32::new(0),
            ended: AtomicBool::new(false),
            midi_dropped: AtomicU64::new(0),
            swaps_deferred: AtomicU64::new(0),
            xruns: AtomicU64::new(0),
            deadline_misses: AtomicU64::new(0),
            busy_blocks: AtomicU64::new(0),
            plugin_faults: AtomicU64::new(0),
            load_last: AtomicU32::new(0),
            load_peak: AtomicU32::new(0),
            meters: (0..MAX_NODES).map(|_| MeterCell::default()).collect(),
            board: (0..MAX_INSERTS).map(|_| AtomicU64::new(0)).collect(),
        }
    }
}
```

(Check that `PlayState::from_u8(0)` is `Stopped`, as the derived `Default` gave before: `AtomicU8::default()` is 0.)

In `impl Shared`, update `counters()` to fill the three new fields from their atomics and add:

```rust
    /// The meter of graph node `i`. // RT
    pub fn meter(&self, i: usize) -> Option<&MeterCell> {
        self.meters.get(i)
    }

    /// Records that the insert at board index `slot` was bypassed for
    /// faults. // RT
    pub fn report_plugin_fault(&self, slot: usize, instance: Option<PluginInstanceId>) {
        self.raise(Faults::PLUGIN_FAULT);
        self.plugin_faults.fetch_add(1, Ordering::Relaxed);
        if let (Some(cell), Some(id)) = (self.board.get(slot), instance) {
            cell.store(id.0.saturating_add(1), Ordering::Relaxed);
        }
    }

    /// Instances bypassed for faults since the last call (read and
    /// cleared).
    pub fn take_plugin_faults(&self) -> Vec<PluginInstanceId> {
        self.board
            .iter()
            .filter_map(|c| {
                let v = c.swap(0, Ordering::Relaxed);
                (v != 0).then(|| PluginInstanceId(v - 1))
            })
            .collect()
    }

    /// Stores one block's DSP load. // RT
    pub fn set_load(&self, load: f32) {
        let bits = load.max(0.0).to_bits();
        self.load_last.store(bits, Ordering::Relaxed);
        self.load_peak.fetch_max(bits, Ordering::Relaxed);
    }

    /// The last block's load and the peak since the last call (cleared).
    pub fn take_load(&self) -> DspLoad {
        DspLoad {
            last: f32::from_bits(self.load_last.load(Ordering::Relaxed)),
            peak: f32::from_bits(self.load_peak.swap(0, Ordering::Relaxed)),
        }
    }

    /// Counts a block that missed its deadline. // RT
    pub fn count_deadline_miss(&self) {
        self.raise(Faults::XRUN);
        self.deadline_misses.fetch_add(1, Ordering::Relaxed);
    }

    /// Counts a block skipped because stragglers were running. // RT
    pub fn count_busy_block(&self) {
        self.raise(Faults::XRUN);
        self.busy_blocks.fetch_add(1, Ordering::Relaxed);
    }
```

- [ ] **Step 4: Write the executor**

Create `crates/core-engine/src/graph/exec.rs`:

```rust
//! The runtime graph and its executor (spec §7). // RT
//!
//! # Protocol
//!
//! A run (one block) resets every node's pending-input counter to its
//! number of input edges, sets `remaining` to the node count and pushes the
//! source nodes onto the ready queue. Every participating thread (the
//! callback thread and the pool's workers) pops a node, renders it,
//! decrements each successor's counter (`AcqRel`), pushes the successors
//! that reach zero, then decrements `remaining`. Hence:
//! - a node is popped exactly once per run, so while a thread renders node
//!   `i` no other thread touches `i`'s state or its inserts;
//! - every input of `i` completed before `i` was pushed and is not written
//!   again in this run, so reading it through a shared reference is sound;
//!   its writes happen-before the read through the counter's release
//!   sequence and the queue's own synchronisation;
//! - an instrument node is the only node of its target, so the only user of
//!   that target's sink during the run;
//! - between runs (`remaining == 0` and no worker inside the job, which
//!   `Exec::try_finish` establishes) the owner has exclusive access.

use crate::backend::StereoBuf;
use crate::error::EngineError;
use crate::graph::delay::DelayLine;
use crate::graph::dsp;
use crate::graph::node::{EffectNode, ProcessStatus};
use crate::graph::pdc::Plan;
use crate::graph::spec::{GraphSpec, InsertSpec, NodeSpec, Tap};
use crate::pool::{Pool, PoolShared, RtEnv, RtPlatform};
use crate::render::{Shared, Sink};
use crate::schedule::MAX_TARGETS;
use core_model::ids::PluginInstanceId;
use crossbeam_queue::ArrayQueue;
use std::cell::UnsafeCell;
use std::ops::Range;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

/// Consecutive faulty blocks before an insert is bypassed (spec §7).
pub const FAULT_LIMIT: u32 = 16;

/// The instrument sinks by target, shared with instrument nodes.
#[derive(Debug)]
pub struct SinkTable {
    cells: Box<[UnsafeCell<Option<Sink>>]>,
}

// SAFETY: a sink is used by its owner between runs (through
// `&mut SinkTable`) and during a run only by the one instrument node of its
// target (module protocol).
unsafe impl Sync for SinkTable {}

impl Default for SinkTable {
    fn default() -> SinkTable {
        SinkTable {
            cells: (0..MAX_TARGETS).map(|_| UnsafeCell::new(None)).collect(),
        }
    }
}

impl SinkTable {
    /// A target's sink slot (owner side).
    pub fn get_mut(&mut self, i: usize) -> Option<&mut Option<Sink>> {
        self.cells.get_mut(i).map(UnsafeCell::get_mut)
    }

    /// Every sink slot (owner side).
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut Option<Sink>> {
        self.cells.iter_mut().map(UnsafeCell::get_mut)
    }
}

/// An insert's runtime state; it moves to the next graph with its
/// instance (Plan Rulings 15 and 23).
pub struct InsertState {
    node: Option<Box<dyn EffectNode>>,
    instance: Option<PluginInstanceId>,
    bad_run: u32,
    faulted: bool,
    bypass: DelayLine,
}

impl InsertState {
    fn new(node: Option<Box<dyn EffectNode>>, instance: Option<PluginInstanceId>) -> InsertState {
        let latency = node.as_ref().map_or(0, |n| n.latency());
        InsertState {
            node,
            instance,
            bad_run: 0,
            faulted: false,
            bypass: DelayLine::new(latency),
        }
    }
}

/// Where a new graph's insert comes from.
pub enum InsertInit {
    /// The running graph's insert at this index (moved on swap).
    Adopt(usize),
    /// A newly created processor.
    Fresh(Box<dyn EffectNode>),
    /// No processor: audio passes through with no latency.
    Placeholder,
}

struct Input {
    from: usize,
    tap: Tap,
    gain: f32,
}

struct InsertDef {
    bypass: bool,
    sidechain: Option<Input>,
}

enum NodeDef {
    Instrument {
        target: usize,
    },
    Strip {
        inputs: Range<usize>,
        inserts: Range<usize>,
        gain_l: f32,
        gain_r: f32,
        audible: bool,
    },
}

struct NodeState {
    pre: StereoBuf,
    post: StereoBuf,
    scratch: StereoBuf,
    sc: StereoBuf,
    delays: Box<[DelayLine]>,
    sc_delays: Box<[DelayLine]>,
}

impl NodeState {
    fn instrument(max: u32) -> NodeState {
        NodeState {
            pre: StereoBuf::new(0),
            post: StereoBuf::new(max),
            scratch: StereoBuf::new(0),
            sc: StereoBuf::new(0),
            delays: Box::new([]),
            sc_delays: Box::new([]),
        }
    }

    fn strip(max: u32, has_sc: bool, delays: Vec<DelayLine>, sc_delays: Vec<DelayLine>) -> NodeState {
        let delayed = delays.iter().any(|d| d.delay() > 0);
        NodeState {
            pre: StereoBuf::new(max),
            post: StereoBuf::new(max),
            scratch: StereoBuf::new(if delayed { max } else { 0 }),
            sc: StereoBuf::new(if has_sc { max } else { 0 }),
            delays: delays.into_boxed_slice(),
            sc_delays: sc_delays.into_boxed_slice(),
        }
    }

    fn resize(&mut self, max: u32) {
        for b in [&mut self.pre, &mut self.post, &mut self.scratch, &mut self.sc] {
            if b.capacity() > 0 {
                *b = StereoBuf::new(max);
            }
        }
    }
}

/// An immutable render graph with its runtime state.
pub struct Graph {
    defs: Box<[NodeDef]>,
    inputs: Box<[Input]>,
    insert_defs: Box<[InsertDef]>,
    succ: Box<[Box<[usize]>]>,
    indegree: Box<[u32]>,
    sources: Box<[usize]>,
    master: usize,
    latency: u64,
    shape: u64,
    max_frames: u32,
    instruments: u16,
    adopt: Box<[(usize, usize)]>,
    nodes: Box<[UnsafeCell<NodeState>]>,
    inserts: Box<[UnsafeCell<Option<InsertState>>]>,
    pending: Box<[AtomicU32]>,
    remaining: AtomicU32,
    queue: ArrayQueue<usize>,
}

// SAFETY: node states, insert states and the sinks are accessed under the
// module protocol; everything else is immutable or atomic.
unsafe impl Sync for Graph {}

impl std::fmt::Debug for Graph {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Graph")
            .field("nodes", &self.defs.len())
            .field("inserts", &self.insert_defs.len())
            .field("latency", &self.latency)
            .finish_non_exhaustive()
    }
}

fn delay_at(v: &[Vec<u32>], i: usize, k: usize) -> u32 {
    v.get(i).and_then(|d| d.get(k)).copied().unwrap_or(0)
}

impl Graph {
    /// Builds the runtime graph for `spec` as planned by `plan`, with
    /// buffers for blocks of up to `max_frames`. `init(index, insert)` says
    /// where each insert's processor comes from (indices count inserts in
    /// node order). Self-edges are dropped (Plan Ruling 29). Not RT.
    pub fn build(
        spec: &GraphSpec,
        plan: &Plan,
        max_frames: u32,
        mut init: impl FnMut(usize, &InsertSpec) -> InsertInit,
    ) -> Graph {
        let n = spec.nodes.len();
        let mut defs = Vec::with_capacity(n);
        let mut inputs = Vec::new();
        let mut insert_defs = Vec::new();
        let mut nodes = Vec::with_capacity(n);
        let mut inserts = Vec::new();
        let mut adopt = Vec::new();
        let mut instruments = 0u16;
        let mut succ: Vec<Vec<usize>> = vec![Vec::new(); n];
        let mut indegree = vec![0u32; n];
        let mut edge = |from: usize, to: usize, succ: &mut Vec<Vec<usize>>| {
            if let Some(v) = succ.get_mut(from) {
                v.push(to);
            }
            if let Some(d) = indegree.get_mut(to) {
                *d += 1;
            }
        };
        for (i, node) in spec.nodes.iter().enumerate() {
            match node {
                NodeSpec::Instrument { target, .. } => {
                    instruments |= 1u16.checked_shl(u32::from(target.0)).unwrap_or(0);
                    defs.push(NodeDef::Instrument {
                        target: usize::from(target.0),
                    });
                    nodes.push(UnsafeCell::new(NodeState::instrument(max_frames)));
                }
                NodeSpec::Strip(s) => {
                    let in_start = inputs.len();
                    let mut delays = Vec::new();
                    for (k, x) in s.inputs.iter().enumerate() {
                        if x.from == i || x.from >= n {
                            continue;
                        }
                        inputs.push(Input {
                            from: x.from,
                            tap: x.tap,
                            gain: x.gain,
                        });
                        delays.push(DelayLine::new(delay_at(&plan.input_delays, i, k)));
                        edge(x.from, i, &mut succ);
                    }
                    let ins_start = insert_defs.len();
                    let mut sc_delays = Vec::new();
                    let mut has_sc = false;
                    for (k, ins) in s.inserts.iter().enumerate() {
                        let sidechain = ins
                            .sidechain
                            .filter(|x| x.from != i && x.from < n)
                            .map(|x| Input {
                                from: x.from,
                                tap: x.tap,
                                gain: x.gain,
                            });
                        if let Some(x) = &sidechain {
                            edge(x.from, i, &mut succ);
                            has_sc = true;
                        }
                        sc_delays.push(DelayLine::new(delay_at(&plan.sidechain_delays, i, k)));
                        insert_defs.push(InsertDef {
                            bypass: ins.bypass,
                            sidechain,
                        });
                        let idx = inserts.len();
                        let state = match init(idx, ins) {
                            InsertInit::Adopt(old) => {
                                adopt.push((idx, old));
                                None
                            }
                            InsertInit::Fresh(node) => Some(InsertState::new(Some(node), ins.instance)),
                            InsertInit::Placeholder => Some(InsertState::new(None, ins.instance)),
                        };
                        inserts.push(UnsafeCell::new(state));
                    }
                    defs.push(NodeDef::Strip {
                        inputs: in_start..inputs.len(),
                        inserts: ins_start..insert_defs.len(),
                        gain_l: s.gain_l,
                        gain_r: s.gain_r,
                        audible: s.audible,
                    });
                    nodes.push(UnsafeCell::new(NodeState::strip(max_frames, has_sc, delays, sc_delays)));
                }
            }
        }
        let sources: Vec<usize> = (0..n).filter(|i| indegree.get(*i) == Some(&0)).collect();
        Graph {
            defs: defs.into_boxed_slice(),
            inputs: inputs.into_boxed_slice(),
            insert_defs: insert_defs.into_boxed_slice(),
            succ: succ.into_iter().map(Vec::into_boxed_slice).collect(),
            pending: indegree.iter().map(|_| AtomicU32::new(0)).collect(),
            indegree: indegree.into_boxed_slice(),
            sources: sources.into_boxed_slice(),
            master: spec.master,
            latency: plan.latency,
            shape: spec.shape(),
            max_frames,
            instruments,
            adopt: adopt.into_boxed_slice(),
            nodes: nodes.into_boxed_slice(),
            inserts: inserts.into_boxed_slice(),
            remaining: AtomicU32::new(0),
            queue: ArrayQueue::new(n.max(1)),
        }
    }

    /// Total latency `L` in frames.
    pub fn latency(&self) -> u64 {
        self.latency
    }

    /// The structure hash of the spec it was built from.
    pub fn shape(&self) -> u64 {
        self.shape
    }

    /// Largest block the buffers hold.
    pub fn max_frames(&self) -> u32 {
        self.max_frames
    }

    /// Number of nodes.
    pub fn node_count(&self) -> usize {
        self.defs.len()
    }

    /// Bitmask of the targets that have an instrument node.
    pub fn instruments(&self) -> u16 {
        self.instruments
    }

    /// Moves adopted inserts out of `old` (the graph this one replaces) and,
    /// when both have the same shape and block size, swaps in `old`'s node
    /// state (buffers and delay lines). Runs between runs. // RT
    pub(crate) fn adopt_from(&mut self, old: &mut Graph) {
        for &(new, prev) in self.adopt.iter() {
            let taken = old.inserts.get_mut(prev).and_then(|c| c.get_mut().take());
            if let Some(cell) = self.inserts.get_mut(new) {
                *cell.get_mut() = taken;
            }
        }
        if self.shape == old.shape
            && self.max_frames == old.max_frames
            && self.nodes.len() == old.nodes.len()
        {
            std::mem::swap(&mut self.nodes, &mut old.nodes);
        }
    }

    /// Reallocates node buffers for blocks of up to `max_frames` (the
    /// renderer calls this from `Renderer::prepare`). Not RT.
    pub fn prepare(&mut self, max_frames: u32) {
        for c in self.nodes.iter_mut() {
            c.get_mut().resize(max_frames);
        }
        self.max_frames = max_frames;
    }

    /// Writes master's output (silence when it is muted) into channels 0–1
    /// of `out`, which the caller zeroed. Call only after a completed run.
    /// // RT
    pub(crate) fn write_master(&self, out: &mut [&mut [f32]], frames: u32) {
        let audible = matches!(self.defs.get(self.master), Some(NodeDef::Strip { audible: true, .. }));
        let Some(cell) = self.nodes.get(self.master) else {
            return;
        };
        if !audible {
            return;
        }
        // SAFETY: the run is complete (`remaining == 0`), so no thread writes
        // any node state until the owner starts the next run.
        let st = unsafe { &*cell.get() };
        let n = frames as usize;
        for (ch, src) in out.iter_mut().zip([&st.post.left, &st.post.right]) {
            if let (Some(d), Some(s)) = (ch.get_mut(..n), src.get(..n)) {
                d.copy_from_slice(s);
            }
        }
    }
}

/// Runs one insert with the fault rule of spec §7. // RT
fn run_insert(
    ins: &mut InsertState,
    bypass: bool,
    frames: u32,
    io: &mut StereoBuf,
    sc: Option<&StereoBuf>,
    shared: &Shared,
    idx: usize,
) {
    if bypass || ins.faulted {
        ins.bypass.process_buf(io, frames);
        return;
    }
    let Some(node) = ins.node.as_mut() else {
        return;
    };
    let status = node.process(frames, io, sc);
    if status == ProcessStatus::Error || dsp::is_faulty(io, frames) {
        io.clear(frames);
        ins.bad_run = ins.bad_run.saturating_add(1);
        if ins.bad_run >= FAULT_LIMIT {
            ins.faulted = true;
            node.reset();
            shared.report_plugin_fault(idx, ins.instance);
        }
    } else {
        ins.bad_run = 0;
    }
}

/// Renders node `i`. // RT
///
/// # Safety
/// `i` was popped from `g.queue` in the current run and `sinks` are that
/// run's sinks (module protocol).
unsafe fn render_node(g: &Graph, sinks: &SinkTable, shared: &Shared, frames: u32, i: usize) {
    let (Some(def), Some(cell)) = (g.defs.get(i), g.nodes.get(i)) else {
        return;
    };
    // SAFETY: this thread popped `i`, so it alone touches node `i`.
    let st = unsafe { &mut *cell.get() };
    match def {
        NodeDef::Instrument { target } => {
            // SAFETY: the only node of this target; the owner leaves sinks
            // alone during a run.
            let sink = sinks.cells.get(*target).map(|c| unsafe { &mut *c.get() });
            match sink {
                Some(Some(Sink::Instrument(node))) => node.render(frames, &mut st.post),
                _ => st.post.clear(frames),
            }
        }
        NodeDef::Strip {
            inputs,
            inserts,
            gain_l,
            gain_r,
            audible,
        } => {
            st.pre.clear(frames);
            for (k, inp) in g.inputs.get(inputs.clone()).unwrap_or(&[]).iter().enumerate() {
                // SAFETY: inputs completed before `i` was pushed and are only
                // read in this run.
                let Some(src) = g.nodes.get(inp.from).map(|c| unsafe { &*c.get() }) else {
                    continue;
                };
                let buf = match inp.tap {
                    Tap::Pre => &src.pre,
                    Tap::Post => &src.post,
                };
                match st.delays.get_mut(k) {
                    Some(d) if d.delay() > 0 => {
                        dsp::copy(&mut st.scratch, buf, frames);
                        d.process_buf(&mut st.scratch, frames);
                        dsp::mix_into(&mut st.pre, &st.scratch, inp.gain, frames);
                    }
                    _ => dsp::mix_into(&mut st.pre, buf, inp.gain, frames),
                }
            }
            for (k, idx) in inserts.clone().enumerate() {
                let (Some(idef), Some(icell)) = (g.insert_defs.get(idx), g.inserts.get(idx)) else {
                    continue;
                };
                // SAFETY: a strip's inserts are touched only while it renders.
                let Some(ins) = (unsafe { &mut *icell.get() }).as_mut() else {
                    continue;
                };
                let sc = match &idef.sidechain {
                    Some(x) => {
                        st.sc.clear(frames);
                        // SAFETY: as for inputs.
                        if let Some(src) = g.nodes.get(x.from).map(|c| unsafe { &*c.get() }) {
                            let buf = match x.tap {
                                Tap::Pre => &src.pre,
                                Tap::Post => &src.post,
                            };
                            dsp::mix_into(&mut st.sc, buf, x.gain, frames);
                        }
                        if let Some(d) = st.sc_delays.get_mut(k) {
                            d.process_buf(&mut st.sc, frames);
                        }
                        Some(&st.sc)
                    }
                    None => None,
                };
                run_insert(ins, idef.bypass, frames, &mut st.pre, sc, shared, idx);
            }
            dsp::copy(&mut st.post, &st.pre, frames);
            dsp::scale(&mut st.post, *gain_l, *gain_r, frames);
            if *audible {
                let (l, r) = dsp::peaks(&st.post, frames);
                if let Some(m) = shared.meter(i) {
                    m.hold(l, r);
                }
            }
        }
    }
}

/// One block's work, published to workers while it runs.
pub(crate) struct Job {
    graph: *const Graph,
    sinks: *const SinkTable,
    shared: *const Shared,
    pool: Option<*const PoolShared>,
    frames: u32,
}

// SAFETY: the pointers are only dereferenced while the job is published
// (module protocol); the pointees are `Sync`.
unsafe impl Send for Job {}
// SAFETY: as above.
unsafe impl Sync for Job {}

impl Job {
    fn empty() -> Job {
        Job {
            graph: std::ptr::null(),
            sinks: std::ptr::null(),
            shared: std::ptr::null(),
            pool: None,
            frames: 0,
        }
    }

    /// Renders one ready node and completes it; false when the queue was
    /// empty. // RT
    ///
    /// # Safety
    /// The job is published (its pointers are valid).
    pub(crate) unsafe fn step(&self) -> bool {
        // SAFETY: caller.
        let (g, sinks, shared) = unsafe { (&*self.graph, &*self.sinks, &*self.shared) };
        let Some(i) = g.queue.pop() else {
            return false;
        };
        // SAFETY: `i` was popped in this run.
        unsafe { render_node(g, sinks, shared, self.frames, i) };
        let mut ready = 0u32;
        for &s in g.succ.get(i).map(|v| &**v).unwrap_or(&[]) {
            if let Some(p) = g.pending.get(s) {
                if p.fetch_sub(1, Ordering::AcqRel) == 1 {
                    let _ = g.queue.push(s);
                    ready += 1;
                }
            }
        }
        if ready > 1 {
            if let Some(pool) = self.pool {
                // SAFETY: the executor holds an `Arc` to the pool for as long
                // as it can publish jobs.
                unsafe { &*pool }.wake(ready - 1);
            }
        }
        g.remaining.fetch_sub(1, Ordering::AcqRel);
        true
    }

    /// Nodes of this run not yet completed. // RT
    ///
    /// # Safety
    /// The job is published, or is the owner's own finished job.
    pub(crate) unsafe fn remaining(&self) -> u32 {
        if self.graph.is_null() {
            return 0;
        }
        // SAFETY: caller.
        unsafe { &*self.graph }.remaining.load(Ordering::Acquire)
    }
}

/// What a run did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RunResult {
    /// Every node completed; master's output is ready.
    Done,
    /// The deadline passed first; nodes still running finish in the
    /// background.
    Missed,
}

/// The executor: owned (boxed, so the job's address is stable) by a
/// renderer or a [`Runner`].
pub(crate) struct Exec {
    pool: Option<Arc<PoolShared>>,
    job: Job,
    running: bool,
}

// SAFETY: `job`'s pointers are only dereferenced under the module
// protocol; moving an `Exec` between threads (a renderer handed to or back
// from the audio thread) does not move what they point at.
unsafe impl Send for Exec {}

impl Exec {
    pub(crate) fn new(pool: Option<Arc<PoolShared>>) -> Box<Exec> {
        Box::new(Exec {
            pool,
            job: Job::empty(),
            running: false,
        })
    }

    /// True when no run is in flight, finishing a completed one. Nodes
    /// still pending (a missed deadline) give false at once; when every
    /// node is done but a worker is still leaving the job, this spins until
    /// it has left or `until` passes (`None`: no limit). // RT
    pub(crate) fn try_finish(&mut self, until: Option<Instant>) -> bool {
        if !self.running {
            return true;
        }
        // SAFETY: the owner keeps the graph of the run in flight alive until
        // this returns true.
        if unsafe { self.job.remaining() } != 0 {
            return false;
        }
        if let Some(p) = &self.pool {
            if !p.retire_job(until) {
                return false;
            }
        }
        self.running = false;
        true
    }

    /// Blocks until no run is in flight. Not RT.
    pub(crate) fn wait_idle(&mut self) {
        while !self.try_finish(None) {
            std::thread::sleep(Duration::from_micros(200));
        }
    }

    /// Renders `g` for one block. Call only after `try_finish` returned
    /// true. With a deadline, returns `Missed` once it passes with nodes
    /// still pending; `Done` may leave a worker still leaving the job, which
    /// the next `try_finish` waits for. // RT
    pub(crate) fn run(
        &mut self,
        g: &Graph,
        sinks: &SinkTable,
        shared: &Shared,
        frames: u32,
        deadline: Option<Instant>,
    ) -> RunResult {
        for (p, d) in g.pending.iter().zip(g.indegree.iter()) {
            p.store(*d, Ordering::Relaxed);
        }
        g.remaining
            .store(u32::try_from(g.defs.len()).unwrap_or(u32::MAX), Ordering::Relaxed);
        while g.queue.pop().is_some() {}
        for &s in g.sources.iter() {
            let _ = g.queue.push(s);
        }
        let pool = self.pool.as_deref().filter(|p| p.active());
        self.job = Job {
            graph: g,
            sinks,
            shared,
            pool: pool.map(|p| p as *const PoolShared),
            frames,
        };
        self.running = true;
        let helps = pool.is_none_or(PoolShared::callback_helps);
        if let Some(p) = pool {
            p.publish(&self.job, g.sources.len());
        }
        loop {
            // SAFETY: this thread published the job; it stays valid until
            // `try_finish` succeeds.
            if helps && unsafe { self.job.step() } {
                continue;
            }
            // SAFETY: as above.
            if unsafe { self.job.remaining() } == 0 {
                break;
            }
            if deadline.is_some_and(|d| Instant::now() >= d) {
                return RunResult::Missed;
            }
            std::hint::spin_loop();
        }
        let _ = self.try_finish(deadline);
        RunResult::Done
    }
}

/// Runs a graph without a renderer or schedule: the harness for graph
/// tests and benchmarks. `block` drives exactly the renderer's executor.
pub struct Runner {
    exec: Box<Exec>,
    graph: Box<Graph>,
    sinks: Box<SinkTable>,
    shared: Arc<Shared>,
    pool: Option<Pool>,
    deadline: Option<Duration>,
}

impl Runner {
    /// Everything on the calling thread (`W = 0`).
    pub fn serial(graph: Graph) -> Runner {
        Runner {
            exec: Exec::new(None),
            graph: Box::new(graph),
            sinks: Box::default(),
            shared: Arc::new(Shared::default()),
            pool: None,
            deadline: None,
        }
    }

    /// The calling thread plus `workers` pool threads from `platform`.
    pub fn parallel(
        graph: Graph,
        workers: usize,
        platform: Arc<dyn RtPlatform>,
    ) -> Result<Runner, EngineError> {
        let shared = Arc::new(Shared::default());
        let pool = Pool::new(workers, platform, Arc::clone(&shared))?;
        Ok(Runner {
            exec: Exec::new(Some(pool.handle())),
            graph: Box::new(graph),
            sinks: Box::default(),
            shared,
            pool: Some(pool),
            deadline: None,
        })
    }

    /// Faults, meters and counters.
    pub fn shared(&self) -> &Shared {
        &self.shared
    }

    /// The instrument sinks (install test instruments here).
    pub fn sinks_mut(&mut self) -> &mut SinkTable {
        self.exec.wait_idle();
        &mut self.sinks
    }

    /// Each block must finish within `d` of its start.
    pub fn set_deadline(&mut self, d: Option<Duration>) {
        self.deadline = d;
    }

    /// Whether the calling thread renders nodes too (default yes).
    pub fn set_callback_helps(&self, on: bool) {
        if let Some(p) = &self.pool {
            p.set_callback_helps(on);
        }
    }

    /// Hands the workers a new real-time environment.
    pub fn set_env(&self, env: RtEnv) {
        if let Some(p) = &self.pool {
            p.set_env(env);
        }
    }

    /// Renders one block into `left`/`right` (zeroed first); false when it
    /// is silent because the deadline was missed or stragglers still run.
    pub fn block(&mut self, frames: u32, left: &mut [f32], right: &mut [f32]) -> bool {
        left.fill(0.0);
        right.fill(0.0);
        let deadline = self.deadline.map(|d| Instant::now() + d);
        if !self.exec.try_finish(deadline) {
            return false;
        }
        match self.exec.run(&self.graph, &self.sinks, &self.shared, frames, deadline) {
            RunResult::Done => {
                self.graph.write_master(&mut [left, right], frames);
                true
            }
            RunResult::Missed => false,
        }
    }

    /// Renders `total` frames in blocks cycling through `sizes` (clamped to
    /// the graph's block size); returns both channels.
    pub fn render(&mut self, total: usize, sizes: &[u32]) -> (Vec<f32>, Vec<f32>) {
        let max = self.graph.max_frames().max(1) as usize;
        let (mut l, mut r) = (vec![0.0f32; max], vec![0.0f32; max]);
        let (mut out_l, mut out_r) = (Vec::with_capacity(total), Vec::with_capacity(total));
        let mut k = 0usize;
        while out_l.len() < total {
            let size = sizes.get(k % sizes.len().max(1)).copied().unwrap_or(64) as usize;
            k += 1;
            let n = size.clamp(1, max).min(total - out_l.len());
            let (Some(bl), Some(br)) = (l.get_mut(..n), r.get_mut(..n)) else {
                break;
            };
            self.block(u32::try_from(n).unwrap_or(1), bl, br);
            out_l.extend_from_slice(bl);
            out_r.extend_from_slice(br);
        }
        (out_l, out_r)
    }

    /// Swaps in `graph`, which adopts from the current one (as a renderer
    /// does on `SetGraph`); returns the old graph.
    pub fn replace_graph(&mut self, mut graph: Graph) -> Graph {
        self.exec.wait_idle();
        graph.adopt_from(&mut self.graph);
        std::mem::replace(&mut *self.graph, graph)
    }
}

impl Drop for Runner {
    fn drop(&mut self) {
        self.exec.wait_idle();
    }
}
```

(`left`/`right` are moved into the array literal `[left, right]` and re-borrowed by `write_master`; the `&mut [&mut [f32]; 2]` coerces to `&mut [&mut [f32]]`.)

- [ ] **Step 5: Write the pool**

Create `crates/core-engine/src/pool.rs`:

```rust
//! Render workers (spec §7) and the platform seams they use: `RtWaker`
//! (wait/wake), `RtWorkgroup` (join/leave the device's workgroup) and
//! `RtPlatform` (wakers and real-time promotion). `platform-macos`
//! implements them with Mach semaphores, `THREAD_TIME_CONSTRAINT_POLICY`
//! and `os_workgroup`; `StdPlatform` is the portable fallback.

use crate::error::EngineError;
use crate::graph::exec::Job;
use crate::render::{Faults, Shared};
use std::any::Any;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{JoinHandle, Thread};
use std::time::{Duration, Instant};

/// How long an idle worker spins before waiting (spec §7: ≈ 20 µs).
pub const SPIN: Duration = Duration::from_micros(20);

/// Wakes waiting workers.
pub trait RtWaker: Send + Sync {
    /// Lets up to `n` waiting workers run. // RT
    fn wake(&self, n: u32);
    /// Blocks the calling worker until woken; spurious returns are allowed.
    fn wait(&self);
    /// The worker threads, once spawned (for wakers that wake threads
    /// directly).
    fn set_threads(&self, threads: Vec<Thread>) {
        let _ = threads;
    }
}

/// A workgroup membership, returned by `join` and handed back to `leave`
/// on the same thread.
pub struct WorkgroupToken(pub Box<dyn Any + Send>);

impl std::fmt::Debug for WorkgroupToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("WorkgroupToken")
    }
}

/// The audio device's thread workgroup.
pub trait RtWorkgroup: Send + Sync {
    /// Joins the calling thread.
    fn join(&self) -> Result<WorkgroupToken, String>;
    /// Removes the calling thread (the one that joined).
    fn leave(&self, token: WorkgroupToken);
}

/// Platform services for render threads.
pub trait RtPlatform: Send + Sync {
    /// A waker for one worker thread (each worker has its own, so wakes can
    /// be targeted).
    fn waker(&self) -> Box<dyn RtWaker>;
    /// Gives the calling thread real-time scheduling for blocks of
    /// `period_ns`.
    fn promote(&self, period_ns: u64) -> Result<(), String>;
}

/// What workers apply between jobs (Plan Ruling 22).
#[derive(Clone, Default)]
pub struct RtEnv {
    /// Block duration in ns (0: leave scheduling as is).
    pub period_ns: u64,
    /// The device's workgroup, when it has one.
    pub workgroup: Option<Arc<dyn RtWorkgroup>>,
}

impl std::fmt::Debug for RtEnv {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RtEnv")
            .field("period_ns", &self.period_ns)
            .field("workgroup", &self.workgroup.is_some())
            .finish()
    }
}

/// The portable platform: a `park`/`unpark` waker, no promotion.
#[derive(Debug, Clone, Copy, Default)]
pub struct StdPlatform;

impl RtPlatform for StdPlatform {
    fn waker(&self) -> Box<dyn RtWaker> {
        Box::new(StdWaker {
            tokens: AtomicU32::new(0),
            cap: 1,
            threads: OnceLock::new(),
        })
    }

    fn promote(&self, _: u64) -> Result<(), String> {
        Ok(())
    }
}

/// A token count capped at `cap`, plus `unpark` (Plan Ruling 21).
struct StdWaker {
    tokens: AtomicU32,
    cap: u32,
    threads: OnceLock<Vec<Thread>>,
}

impl RtWaker for StdWaker {
    fn wake(&self, n: u32) {
        if n == 0 {
            return;
        }
        let mut t = self.tokens.load(Ordering::Acquire);
        loop {
            let next = t.saturating_add(n).min(self.cap);
            match self
                .tokens
                .compare_exchange_weak(t, next, Ordering::AcqRel, Ordering::Acquire)
            {
                Ok(_) => break,
                Err(now) => t = now,
            }
        }
        if let Some(ts) = self.threads.get() {
            for t in ts {
                t.unpark();
            }
        }
    }

    fn wait(&self) {
        loop {
            let t = self.tokens.load(Ordering::Acquire);
            if t > 0 {
                if self
                    .tokens
                    .compare_exchange(t, t - 1, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok()
                {
                    return;
                }
                continue;
            }
            std::thread::park();
        }
    }

    fn set_threads(&self, threads: Vec<Thread>) {
        let _ = self.threads.set(threads);
    }
}

/// What the executor and the workers share.
pub struct PoolShared {
    job: AtomicPtr<Job>,
    in_job: AtomicU32,
    /// One waker per worker.
    wakers: Box<[Box<dyn RtWaker>]>,
    /// Round-robin cursor for job wakes.
    next: AtomicUsize,
    workers: usize,
    stop: AtomicBool,
    helps: AtomicBool,
    env: Mutex<RtEnv>,
    epoch: AtomicU64,
    /// Environment applications so far (each worker adds one per epoch it
    /// applies).
    acks: AtomicU64,
}

impl std::fmt::Debug for PoolShared {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PoolShared")
            .field("workers", &self.workers)
            .finish_non_exhaustive()
    }
}

impl PoolShared {
    /// Number of worker threads.
    pub fn workers(&self) -> usize {
        self.workers
    }

    /// Whether the callback thread renders nodes too.
    pub fn callback_helps(&self) -> bool {
        self.helps.load(Ordering::Relaxed)
    }

    /// Sets whether the callback thread renders nodes too (Plan Ruling 19).
    pub fn set_callback_helps(&self, on: bool) {
        self.helps.store(on, Ordering::Relaxed);
    }

    pub(crate) fn active(&self) -> bool {
        self.workers > 0 && !self.stop.load(Ordering::Acquire)
    }

    /// Publishes a job and wakes workers for its `ready` source nodes. // RT
    pub(crate) fn publish(&self, job: *const Job, ready: usize) {
        self.job.store(job.cast_mut(), Ordering::SeqCst);
        self.wake(u32::try_from(ready).unwrap_or(u32::MAX));
    }

    /// Wakes up to `n` workers, round robin (a busy one finds the job
    /// done or takes the next node). // RT
    pub(crate) fn wake(&self, n: u32) {
        let w = self.wakers.len();
        if w == 0 {
            return;
        }
        for _ in 0..(n as usize).min(w) {
            let i = self.next.fetch_add(1, Ordering::Relaxed) % w;
            if let Some(k) = self.wakers.get(i) {
                k.wake(1);
            }
        }
    }

    fn wake_all(&self) {
        for k in self.wakers.iter() {
            k.wake(1);
        }
    }

    /// Unpublishes the job and spins until every worker has left it, or
    /// until `until` passes (false: one is still inside). // RT
    pub(crate) fn retire_job(&self, until: Option<Instant>) -> bool {
        self.job.store(std::ptr::null_mut(), Ordering::SeqCst);
        while self.in_job.load(Ordering::SeqCst) != 0 {
            if until.is_some_and(|u| Instant::now() >= u) {
                return false;
            }
            std::hint::spin_loop();
        }
        true
    }

    /// Publishes `env`, wakes every worker and waits (up to 1 s, control
    /// thread only) until each one applied it.
    fn set_env(&self, env: RtEnv) {
        if let Ok(mut g) = self.env.lock() {
            *g = env;
        }
        let workers = u64::try_from(self.workers).unwrap_or(u64::MAX);
        let target = self.acks.load(Ordering::Acquire).saturating_add(workers);
        self.epoch.fetch_add(1, Ordering::AcqRel);
        self.wake_all();
        let start = Instant::now();
        while self.acks.load(Ordering::Acquire) < target && start.elapsed() < Duration::from_secs(1) {
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// One worker's share of the published job, if any. // RT
    fn help(&self) {
        self.in_job.fetch_add(1, Ordering::SeqCst);
        let p = self.job.load(Ordering::SeqCst);
        if !p.is_null() {
            // SAFETY: a non-null job stays valid until `in_job` is back to
            // zero: `Exec::try_finish` clears the pointer and waits for that
            // before the job's graph, sinks or shared state change or go.
            let job = unsafe { &*p };
            let mut idle: Option<Instant> = None;
            loop {
                // SAFETY: as above.
                if unsafe { job.step() } {
                    idle = None;
                    continue;
                }
                // SAFETY: as above.
                if unsafe { job.remaining() } == 0 {
                    break;
                }
                let now = Instant::now();
                match idle {
                    None => idle = Some(now),
                    Some(t) if now.duration_since(t) >= SPIN => break,
                    Some(_) => std::hint::spin_loop(),
                }
            }
        }
        self.in_job.fetch_sub(1, Ordering::SeqCst);
    }
}

fn worker(s: &PoolShared, me: usize, platform: &dyn RtPlatform, faults: &Shared) {
    let mut epoch = 0u64;
    let mut joined: Option<(Arc<dyn RtWorkgroup>, WorkgroupToken)> = None;
    let Some(waker) = s.wakers.get(me) else {
        return;
    };
    loop {
        waker.wait();
        if s.stop.load(Ordering::Acquire) {
            break;
        }
        let e = s.epoch.load(Ordering::Acquire);
        if e != epoch {
            epoch = e;
            let env = s.env.lock().map(|g| (*g).clone()).unwrap_or_default();
            if let Some((wg, token)) = joined.take() {
                wg.leave(token);
            }
            if env.period_ns > 0 && platform.promote(env.period_ns).is_err() {
                faults.raise(Faults::THREAD_POLICY_FAILED);
            }
            if let Some(wg) = env.workgroup {
                match wg.join() {
                    Ok(t) => joined = Some((wg, t)),
                    Err(_) => faults.raise(Faults::WORKGROUP_JOIN_FAILED),
                }
            }
            s.acks.fetch_add(1, Ordering::AcqRel);
        }
        let arm = crate::rt_check::arm_workers();
        if arm {
            crate::rt_check::arm();
        }
        s.help();
        if arm {
            crate::rt_check::disarm();
        }
    }
    if let Some((wg, token)) = joined.take() {
        wg.leave(token);
    }
}

/// The worker threads. Dropping the pool stops and joins them.
pub struct Pool {
    shared: Arc<PoolShared>,
    handles: Vec<JoinHandle<()>>,
}

impl std::fmt::Debug for Pool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pool").field("workers", &self.handles.len()).finish()
    }
}

impl Pool {
    /// Starts `workers` threads named `vizdaw-render-N`; they raise faults
    /// in `faults`.
    pub fn new(
        workers: usize,
        platform: Arc<dyn RtPlatform>,
        faults: Arc<Shared>,
    ) -> Result<Pool, EngineError> {
        let shared = Arc::new(PoolShared {
            job: AtomicPtr::new(std::ptr::null_mut()),
            in_job: AtomicU32::new(0),
            wakers: (0..workers).map(|_| platform.waker()).collect(),
            next: AtomicUsize::new(0),
            workers,
            stop: AtomicBool::new(false),
            helps: AtomicBool::new(true),
            env: Mutex::new(RtEnv::default()),
            epoch: AtomicU64::new(0),
            acks: AtomicU64::new(0),
        });
        let mut pool = Pool {
            shared,
            handles: Vec::with_capacity(workers),
        };
        for i in 0..workers {
            let (s, p, f) = (Arc::clone(&pool.shared), Arc::clone(&platform), Arc::clone(&faults));
            let h = std::thread::Builder::new()
                .name(format!("vizdaw-render-{i}"))
                .spawn(move || worker(&s, i, &*p, &f))
                .map_err(|e| EngineError::Thread {
                    name: "vizdaw-render",
                    reason: e.to_string(),
                })?;
            pool.handles.push(h);
        }
        pool.register_threads();
        Ok(pool)
    }

    fn register_threads(&self) {
        for (k, h) in self.shared.wakers.iter().zip(&self.handles) {
            k.set_threads(vec![h.thread().clone()]);
        }
    }

    /// The handle executors publish jobs through.
    pub fn handle(&self) -> Arc<PoolShared> {
        Arc::clone(&self.shared)
    }

    /// Number of worker threads.
    pub fn workers(&self) -> usize {
        self.handles.len()
    }

    /// Hands the workers a new real-time environment (applied between
    /// jobs); returns once every worker applied it, or after 1 s. Not RT.
    pub fn set_env(&self, env: RtEnv) {
        self.shared.set_env(env);
    }

    /// Whether the callback thread renders nodes too.
    pub fn set_callback_helps(&self, on: bool) {
        self.shared.set_callback_helps(on);
    }
}

impl Drop for Pool {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
        // A failed `new` may not have registered the threads yet.
        self.register_threads();
        self.shared.wake_all();
        for h in self.handles.drain(..) {
            let _ = h.join();
        }
    }
}
```

Replace `crates/core-engine/src/graph/mod.rs` with:

```rust
//! The mixer render graph (spec §6, §7): strip DSP (`dsp`), delay lines
//! (`delay`), insert processors (`node`), the graph description (`spec`),
//! PDC planning (`pdc`) and the runtime graph and executor (`exec`).

pub mod delay;
pub mod dsp;
pub mod exec;
pub mod node;
pub mod pdc;
pub mod spec;

pub use exec::{Graph, InsertInit, Runner, SinkTable};
pub use node::{
    EffectNode, InsertFactory, NoInserts, ProcessStatus, TestInserts, TestMode, TestNode,
};
pub use pdc::{MAX_INSERTS, MAX_NODES, Plan, PlanError, plan};
pub use spec::{GraphSpec, InputSpec, InsertSpec, NodeSpec, StripSpec, Tap};
```

In `crates/core-engine/src/lib.rs` add `pub mod pool;` after `pub mod offline;`, and extend the crate doc's second paragraph with: "Graph workers (`pool`) are `// RT` while they run a job; `unsafe` lives only in `graph/exec.rs`, `pool.rs` and `rt_check.rs`."

- [ ] **Step 6: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-engine --test graph_exec --test graph_parallel
cargo test -p core-engine --release --test graph_parallel
```

Expected: `graph_exec` 10 passed; `graph_parallel` 6 passed in debug and release (determinism must hold with optimisations; the release run catches reordering bugs).

- [ ] **Step 7: Lint and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git add crates/core-engine/Cargo.toml Cargo.lock crates/core-engine/src/render.rs crates/core-engine/src/rt_check.rs crates/core-engine/src/graph crates/core-engine/src/pool.rs crates/core-engine/src/lib.rs crates/core-engine/tests/graph_exec.rs crates/core-engine/tests/graph_parallel.rs
git commit -m "feat(core-engine): runtime graph, parallel executor and worker pool

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---
### Task 10: From the mixer to a graph spec

**Goal:** Translate a document's mixer into a `GraphSpec`: built-in synth targets become instrument nodes feeding the built-in synth strip (Plan Ruling 1), every strip a strip node, outputs and sends input edges, sidechains insert edges; mute, solo, fader, pan and send levels become gains (Plan Rulings 4, 5); summing order is fixed (Plan Ruling 25). Add `Builder` helpers for mixer edits.

**Files:**
- Create: `crates/core-engine/src/graph/mixer.rs`
- Modify: `crates/core-engine/src/graph/mod.rs`
- Modify: `crates/core-engine/src/fixtures.rs`
- Test: `crates/core-engine/tests/graph_mixer.rs`

**Interfaces:**
- Consumes: Task 8 spec types; Task 7 `dsp::strip_gains`; core-model `Document::mixer`, `mixer::{Output, PluginSlot, Strip, StripKind, db_to_gain}`, `mixer_rules::audible`, `command::MixerCmd`.
- Produces:
  - `graph::mixer::MixerGraph { spec: GraphSpec, strips: Vec<(usize, StripId)>, warnings: Vec<String> }`.
  - `graph::mixer::mixer_spec(doc: &Document, instruments: &[(TargetIdx, u32)], insert_latency: &mut dyn FnMut(&PluginSlot) -> u32) -> MixerGraph` — node order: `instruments` in the given order, then every strip by `StripId`; `spec.master` is master's node.
  - Edge rules: instrument → built-in strip (or master, with a warning, when there is none) `{Post, 1}`; each non-master strip's output → its target `{Post, pass}`; each send → its bus `{Pre|Post, pass × (enabled ? db_to_gain(level) : 0)}` where `pass = 0` when the strip is muted explicitly or by solo, else 1; sidechain → insert `{Post, 0 if the source is explicitly muted else 1}`; strip gains = fader × pan (master: fader only); `audible = !(mute || solo-muted)`.
  - `fixtures::Builder::{from_doc(Document) -> Builder, mixer(MixerCmd) -> Result<Report>, strip_of(StripKind) -> Result<StripId>, bus(&str) -> Result<StripId>, insert(StripId, PluginRef) -> Result<PluginInstanceId>}`.

- [ ] **Step 1: Write the failing test**

Create `crates/core-engine/tests/graph_mixer.rs`:

```rust
use core_engine::fixtures::{Builder, demo_document};
use core_engine::graph::mixer::mixer_spec;
use core_engine::graph::{InputSpec, NodeSpec, StripSpec, Tap};
use core_engine::schedule::TargetIdx;
use core_model::command::MixerCmd;
use core_model::document::{DestKind, Document, DocumentRepr, Settings};
use core_model::ids::{Counters, StripId};
use core_model::mixer::{AuxSend, Mixer, PluginRef, PluginSlot, StripKind};
use std::f32::consts::FRAC_1_SQRT_2;

fn strip(spec: &core_engine::graph::GraphSpec, i: usize) -> &StripSpec {
    match &spec.nodes[i] {
        NodeSpec::Strip(s) => s,
        NodeSpec::Instrument { .. } => panic!("node {i} is an instrument"),
    }
}

fn post(from: usize, gain: f32) -> InputSpec {
    InputSpec { from, tap: Tap::Post, gain }
}

fn zero(_: &PluginSlot) -> u32 {
    0
}

#[test]
fn the_demo_plays_through_the_builtin_strip() {
    let doc = demo_document().unwrap();
    let g = mixer_spec(&doc, &[(TargetIdx(0), 0)], &mut zero);
    assert!(g.warnings.is_empty(), "{:?}", g.warnings);
    assert_eq!(g.spec.nodes[0], NodeSpec::Instrument { target: TargetIdx(0), latency: 0 });
    assert_eq!(g.spec.master, 1);
    assert_eq!(g.strips, vec![(1, StripId(0)), (2, StripId(1))]);
    let master = strip(&g.spec, 1);
    let synth = strip(&g.spec, 2);
    assert_eq!(synth.inputs, vec![post(0, 1.0)]);
    assert_eq!(master.inputs, vec![post(2, 1.0)]);
    assert_eq!((master.gain_l, master.gain_r), (1.0, 1.0), "no pan on master");
    assert!((synth.gain_l - FRAC_1_SQRT_2).abs() < 1e-6 && synth.gain_l == synth.gain_r);
    assert!(master.audible && synth.audible);
}

/// Synth (builtin strip 1) sends pre-fader at −6 dB to bus A (2); bus B (3)
/// has a ducker sidechained from the synth. Nodes: 0 instrument, 1 master,
/// 2 synth, 3 A, 4 B.
fn routed() -> Builder {
    let mut b = Builder::new();
    b.dest("Synth", "builtin:synth", 0).unwrap();
    let synth = b.strip_of(StripKind::BuiltinSynth).unwrap();
    let a = b.bus("A").unwrap();
    let bus_b = b.bus("B").unwrap();
    let mut send = AuxSend::new(a);
    send.level_db = -6.020_6;
    send.pre_fader = true;
    b.mixer(MixerCmd::AddSend { strip: synth, send }).unwrap();
    let duck = b.insert(bus_b, PluginRef::from_legacy("clap:duck")).unwrap();
    b.mixer(MixerCmd::SetSidechain { slot: duck, source: Some(synth) }).unwrap();
    b
}

#[test]
fn outputs_sends_and_sidechains_become_edges() {
    let b = routed();
    let g = mixer_spec(b.doc(), &[(TargetIdx(0), 0)], &mut zero);
    assert_eq!(strip(&g.spec, 1).inputs, vec![post(2, 1.0), post(3, 1.0), post(4, 1.0)]);
    let a = &strip(&g.spec, 3).inputs;
    assert_eq!(a.len(), 1);
    assert_eq!((a[0].from, a[0].tap), (2, Tap::Pre));
    assert!((a[0].gain - 0.5).abs() < 1e-4);
    let duck = &strip(&g.spec, 4).inserts[0];
    assert_eq!(duck.sidechain, Some(post(2, 1.0)));
    assert_eq!(duck.instance, Some(core_model::ids::PluginInstanceId(0)));
}

#[test]
fn mute_and_solo_become_gains() {
    // Explicit mute: every edge from the synth is 0, its sidechain feed too.
    let mut b = routed();
    b.mixer(MixerCmd::SetMute { strip: StripId(1), mute: true }).unwrap();
    let g = mixer_spec(b.doc(), &[(TargetIdx(0), 0)], &mut zero);
    assert_eq!(strip(&g.spec, 1).inputs[0], post(2, 0.0));
    assert_eq!(strip(&g.spec, 3).inputs[0].gain, 0.0);
    assert_eq!(strip(&g.spec, 4).inserts[0].sidechain, Some(post(2, 0.0)));
    assert!(!strip(&g.spec, 2).audible);

    // Solo B: the synth is solo-muted, but still drives B's sidechain.
    let mut b = routed();
    b.mixer(MixerCmd::SetSolo { strip: StripId(3), solo: true }).unwrap();
    let g = mixer_spec(b.doc(), &[(TargetIdx(0), 0)], &mut zero);
    assert_eq!(strip(&g.spec, 1).inputs, vec![post(2, 0.0), post(3, 0.0), post(4, 1.0)]);
    assert_eq!(strip(&g.spec, 4).inserts[0].sidechain, Some(post(2, 1.0)));
    assert!(!strip(&g.spec, 3).audible && strip(&g.spec, 4).audible);

    // Solo A keeps its source (the synth, upstream through the send).
    let mut b = routed();
    b.mixer(MixerCmd::SetSolo { strip: StripId(2), solo: true }).unwrap();
    let g = mixer_spec(b.doc(), &[(TargetIdx(0), 0)], &mut zero);
    assert_eq!(strip(&g.spec, 1).inputs, vec![post(2, 1.0), post(3, 1.0), post(4, 0.0)]);
}

#[test]
fn faders_pans_and_disabled_sends() {
    let mut b = routed();
    b.mixer(MixerCmd::SetPan { strip: StripId(1), pan: -1.0 }).unwrap();
    b.mixer(MixerCmd::SetGain { strip: StripId(0), gain_db: -6.020_6 }).unwrap();
    b.mixer(MixerCmd::SetPan { strip: StripId(0), pan: 1.0 }).unwrap();
    let mut off = AuxSend::new(StripId(2));
    off.enabled = false;
    b.mixer(MixerCmd::EditSend { strip: StripId(1), index: 0, send: off }).unwrap();
    let g = mixer_spec(b.doc(), &[(TargetIdx(0), 0)], &mut zero);
    let synth = strip(&g.spec, 2);
    assert_eq!(synth.gain_l, 1.0);
    assert!(synth.gain_r.abs() < 1e-7);
    let master = strip(&g.spec, 1);
    assert!((master.gain_l - 0.5).abs() < 1e-4 && master.gain_l == master.gain_r);
    assert_eq!(strip(&g.spec, 3).inputs[0].gain, 0.0);
}

#[test]
fn insert_latencies_and_bypass_come_from_the_slots() {
    let mut b = routed();
    let eq = b.insert(StripId(1), PluginRef::from_legacy("clap:eq")).unwrap();
    b.mixer(MixerCmd::SetBypass { slot: eq, bypass: true }).unwrap();
    let mut lat = |s: &PluginSlot| if s.id == eq { 64 } else { 0 };
    let g = mixer_spec(b.doc(), &[(TargetIdx(0), 7)], &mut lat);
    assert_eq!(g.spec.nodes[0], NodeSpec::Instrument { target: TargetIdx(0), latency: 7 });
    let ins = &strip(&g.spec, 2).inserts[0];
    assert_eq!((ins.instance, ins.latency, ins.bypass), (Some(eq), 64, true));
}

#[test]
fn missing_builtin_strip_and_silent_instrument_strips_are_reported() {
    let doc = Document::from_parts(DocumentRepr {
        settings: Settings::default(),
        instruments: vec![],
        outputs: vec![],
        media: vec![],
        sequences: vec![],
        mixer: Some(Mixer::new(StripId(0))),
        counters: Counters { strip: 1, ..Counters::default() },
    })
    .unwrap();
    let g = mixer_spec(&doc, &[(TargetIdx(0), 0)], &mut zero);
    assert_eq!(strip(&g.spec, 1).inputs, vec![post(0, 1.0)]);
    assert!(g.warnings[0].contains("no built-in synth strip"));

    let mut b = Builder::new();
    b.dest_kind("Pad", DestKind::plugin("au:pad"), None).unwrap();
    let g = mixer_spec(b.doc(), &[], &mut zero);
    assert_eq!(g.warnings, vec!["strip \"Pad\" has no instrument until plugin hosting exists".to_string()]);
}
```

- [ ] **Step 2: Run the test to verify it fails**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-engine --test graph_mixer
```

Expected: compile errors — no `graph::mixer`, no `Builder::strip_of`.

- [ ] **Step 3: Write the translation**

Create `crates/core-engine/src/graph/mixer.rs`:

```rust
//! The mixer → graph translation (spec §6; Plan Rulings 1, 4, 5, 25).

use crate::graph::dsp::strip_gains;
use crate::graph::spec::{GraphSpec, InputSpec, InsertSpec, NodeSpec, StripSpec, Tap};
use crate::schedule::TargetIdx;
use core_model::document::Document;
use core_model::ids::StripId;
use core_model::mixer::{Output, PluginSlot, Strip, StripKind, db_to_gain};
use core_model::mixer_rules::audible;
use std::collections::BTreeMap;

/// A graph spec made from a document's mixer.
#[derive(Debug, Clone, PartialEq)]
pub struct MixerGraph {
    /// The spec.
    pub spec: GraphSpec,
    /// `(node index, strip)` of every strip node (meters).
    pub strips: Vec<(usize, StripId)>,
    /// Problems worth showing (not errors).
    pub warnings: Vec<String>,
}

/// Builds the graph of `doc`'s mixer. `instruments` are the built-in synth
/// targets that have an instrument sink, with their latency;
/// `insert_latency` gives each insert's latency (0 for placeholders).
pub fn mixer_spec(
    doc: &Document,
    instruments: &[(TargetIdx, u32)],
    insert_latency: &mut dyn FnMut(&PluginSlot) -> u32,
) -> MixerGraph {
    let m = doc.mixer();
    let on = audible(m);
    let base = instruments.len();
    let index: BTreeMap<StripId, usize> = m
        .strips()
        .keys()
        .enumerate()
        .map(|(i, id)| (*id, base + i))
        .collect();
    let node = |id: StripId| index.get(&id).copied();
    let mut warnings = Vec::new();
    let mut strips: Vec<StripSpec> = m
        .strips()
        .values()
        .map(|s| StripSpec::new(Some(s.id)))
        .collect();
    let master = node(m.master()).unwrap_or(base);
    let feed = m.find_kind(StripKind::BuiltinSynth).and_then(node);
    if feed.is_none() && !instruments.is_empty() {
        warnings.push("no built-in synth strip: the built-in synth plays straight to master".to_string());
    }
    let to = feed.unwrap_or(master);
    for (i, _) in instruments.iter().enumerate() {
        if let Some(s) = to.checked_sub(base).and_then(|k| strips.get_mut(k)) {
            s.inputs.push(InputSpec {
                from: i,
                tap: Tap::Post,
                gain: 1.0,
            });
        }
    }
    let muted = |s: &Strip| s.mute || !on.contains(&s.id);
    for s in m.strips().values() {
        let Some(from) = node(s.id) else {
            continue;
        };
        let pass = if muted(s) { 0.0 } else { 1.0 };
        if s.id != m.master() {
            let target = match s.output {
                Output::Master => m.master(),
                Output::Bus(b) => b,
            };
            if let Some(t) = node(target)
                .and_then(|t| t.checked_sub(base))
                .and_then(|k| strips.get_mut(k))
            {
                t.inputs.push(InputSpec {
                    from,
                    tap: Tap::Post,
                    gain: pass,
                });
            }
        }
        for x in s.sends.iter() {
            let level = if x.enabled {
                db_to_gain(x.level_db) as f32
            } else {
                0.0
            };
            if let Some(t) = node(x.target)
                .and_then(|t| t.checked_sub(base))
                .and_then(|k| strips.get_mut(k))
            {
                t.inputs.push(InputSpec {
                    from,
                    tap: if x.pre_fader { Tap::Pre } else { Tap::Post },
                    gain: pass * level,
                });
            }
        }
    }
    for (k, s) in m.strips().values().enumerate() {
        let Some(spec) = strips.get_mut(k) else {
            continue;
        };
        let (gl, gr) = strip_gains(s.gain_db, s.pan, s.id != m.master());
        spec.gain_l = gl;
        spec.gain_r = gr;
        spec.audible = !muted(s);
        for slot in s.inserts.iter() {
            let sidechain = slot.sidechain.and_then(|src| {
                let from = node(src)?;
                let explicit = m.strip(src).map(|x| x.mute).unwrap_or(false);
                Some(InputSpec {
                    from,
                    tap: Tap::Post,
                    gain: if explicit { 0.0 } else { 1.0 },
                })
            });
            spec.inserts.push(InsertSpec {
                instance: Some(slot.id),
                latency: insert_latency(slot),
                bypass: slot.bypass,
                sidechain,
            });
        }
        if matches!(s.kind, StripKind::Instrument(_) | StripKind::InstrumentOutput(..)) {
            warnings.push(format!(
                "strip {:?} has no instrument until plugin hosting exists",
                s.name
            ));
        }
    }
    let strip_nodes = m
        .strips()
        .keys()
        .enumerate()
        .map(|(i, id)| (base + i, *id))
        .collect();
    let mut nodes: Vec<NodeSpec> = instruments
        .iter()
        .map(|(t, l)| NodeSpec::Instrument {
            target: *t,
            latency: *l,
        })
        .collect();
    nodes.extend(strips.into_iter().map(NodeSpec::Strip));
    MixerGraph {
        spec: GraphSpec { nodes, master },
        strips: strip_nodes,
        warnings,
    }
}
```

In `crates/core-engine/src/graph/mod.rs` add `pub mod mixer;` after `pub mod exec;` and update the module doc's list with "and the mixer translation (`mixer`)".

- [ ] **Step 4: Builder helpers**

In `crates/core-engine/src/fixtures.rs` extend the imports to `use core_model::command::{Command, DrumCmd, EventCmd, LaneCmd, MixerCmd, ReferenceCmd, Report, RoutingCmd, SequenceCmd, apply};`, `use core_model::ids::{AnyId, DestId, EventId, LaneId, PluginInstanceId, SeqId, StripId};` and `use core_model::mixer::{PluginRef, StripKind};`, and add to `impl Builder` after `run`:

```rust
    /// Continues building `doc`.
    pub fn from_doc(doc: Document) -> Builder {
        Builder { doc }
    }

    /// Applies a mixer command.
    pub fn mixer(&mut self, cmd: MixerCmd) -> Result<Report> {
        self.run(Command::Mixer(cmd))
    }

    /// The strip of `kind`.
    pub fn strip_of(&self, kind: StripKind) -> Result<StripId> {
        self.doc
            .mixer()
            .find_kind(kind)
            .ok_or_else(|| ModelError::invalid(format!("no {kind:?} strip")))
    }

    /// Adds an aux bus.
    pub fn bus(&mut self, name: &str) -> Result<StripId> {
        let r = self.mixer(MixerCmd::AddBus {
            name: name.into(),
            index: None,
        })?;
        created(&r, |id| match id {
            AnyId::Strip(s) => Some(*s),
            _ => None,
        })
    }

    /// Appends an insert to `strip`.
    pub fn insert(&mut self, strip: StripId, plugin: PluginRef) -> Result<PluginInstanceId> {
        let r = self.mixer(MixerCmd::AddInsert {
            strip,
            index: None,
            plugin,
        })?;
        created(&r, |id| match id {
            AnyId::Plugin(p) => Some(*p),
            _ => None,
        })
    }
```

- [ ] **Step 5: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-engine --test graph_mixer
```

Expected: 6 passed.

- [ ] **Step 6: Lint and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git add crates/core-engine/src/graph/mixer.rs crates/core-engine/src/graph/mod.rs crates/core-engine/src/fixtures.rs crates/core-engine/tests/graph_mixer.rs
git commit -m "feat(core-engine): mixer to graph translation with mute, solo and sends

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---
### Task 11: The renderer and the engine play through the graph

**Goal:** `Engine::load` compiles the mixer graph after the schedule and publishes it with `EngineCmd::SetGraph`; the renderer emits MIDI as before, renders the graph with the executor (workers from `EngineConfig.workers`), applies PDC (MIDI-out `+L`, position `−L`), hard-clamps master, publishes meters, DSP load and faults, handles missed deadlines and stragglers, and retires graphs through the collector. The offline built-in synth can make a test tone so audio determinism can be tested end to end.

**Files:**
- Modify: `crates/core-engine/src/backend.rs` (`InstrumentNode::latency`)
- Replace: `crates/core-engine/src/render.rs`
- Replace: `crates/core-engine/src/engine.rs`
- Modify: `crates/core-engine/src/offline.rs`
- Modify: `crates/core-engine/src/schedule/compile.rs` (`CompileReport.graph`)
- Modify: `crates/core-engine/src/graph/mod.rs` (`GraphReport`)
- Modify: `crates/core-engine/tests/engine.rs` (one expectation: the −3 dB pan law)
- Test: `crates/core-engine/tests/mixer_engine.rs`

**Interfaces:**
- Consumes: Tasks 7–10 (`Graph`, `InsertInit`, `plan`, `PlanError`, `mixer_spec`, `Exec`, `SinkTable`, `Pool`, `RtPlatform`, `RtEnv`, `StdPlatform`, `InsertFactory`, `NoInserts`, `TestInserts`), the Task 9 `Shared` API.
- Produces:
  - `InstrumentNode::latency(&self) -> u32` (default 0).
  - `graph::GraphReport { nodes: usize, latency_frames: u64, placeholders: Vec<(PluginInstanceId, String)>, warnings: Vec<String>, error: Option<String> }`; `CompileReport.graph: GraphReport`.
  - `render::RETIRE_CAPACITY` = 16 (was 8; Plan Ruling 30).
  - `render::EngineCmd::SetGraph(Box<Graph>)`, `render::Retired::{Schedule(Box<Schedule>), Graph(Box<Graph>)}`, `RenderConfig.deadline: Option<f64>`, `Renderer::latency() -> u64`; `Renderer` drops, `prepare`s, `reset_instruments` and `sink_mut`s only after stragglers finish.
  - `EngineConfig { …, workers: usize, deadline: Option<f64> }` (`new` → 0 and `None`); `Engine::with_platform(cfg, Box<dyn SinkFactory>, Arc<dyn RtPlatform>) -> Result<(Engine, Renderer), EngineError>`; `Engine::{set_insert_factory(Box<dyn InsertFactory>), set_rt_env(RtEnv), set_callback_helps(bool), latency_frames() -> u64, meters() -> Vec<StripMeter>, dsp_load() -> DspLoad, plugin_faults() -> Vec<PluginInstanceId>}`; `engine::StripMeter { strip, peak_l, peak_r }`.
  - `offline::OfflineSinks::with_tone(self) -> OfflineSinks`; `offline::OfflineBackend::output(&self, frames: u32) -> (&[f32], &[f32])`.

- [ ] **Step 1: Write the failing test**

Create `crates/core-engine/tests/mixer_engine.rs`:

```rust
use core_engine::backend::AudioBackend;
use core_engine::engine::{Engine, EngineConfig, find_sequence};
use core_engine::fixtures::{Builder, Q, demo_document};
use core_engine::graph::TestInserts;
use core_engine::offline::{OfflineBackend, OfflineSinks};
use core_engine::render::Faults;
use core_engine::schedule::BuiltinRoute;
use core_model::command::MixerCmd;
use core_model::document::Document;
use core_model::ids::{PluginInstanceId, SeqId, StripId};
use core_model::mixer::{AuxSend, PluginRef, StripKind};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

const SR: f64 = 48_000.0;

fn fnv(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

fn song() -> (Document, SeqId) {
    let doc = demo_document().unwrap();
    let song = find_sequence(&doc, "Song").unwrap();
    (doc, song)
}

/// Renders `total` frames in blocks cycling through `sizes`; returns the
/// left channel.
fn render(b: &mut OfflineBackend, total: usize, sizes: &[u32]) -> Vec<f32> {
    let mut out = Vec::with_capacity(total);
    let mut k = 0;
    while out.len() < total {
        let n = (sizes[k % sizes.len()] as usize).min(total - out.len()) as u32;
        k += 1;
        b.reserve(n);
        b.block(n);
        out.extend_from_slice(b.output(n).0);
    }
    out
}

#[test]
fn the_demo_log_is_unchanged_with_and_without_workers() {
    for workers in [0, 4] {
        let (doc, song) = song();
        let mut cfg = EngineConfig::new(SR);
        cfg.workers = workers;
        let sinks = OfflineSinks::new(100_000);
        let log = sinks.log();
        let (mut engine, r) = Engine::new(cfg, Box::new(sinks)).unwrap();
        let report = engine.load(&doc, song).unwrap();
        assert_eq!(report.graph.nodes, 3, "built-in synth instrument, master, built-in strip");
        assert_eq!(report.graph.latency_frames, 0);
        assert!(report.graph.error.is_none() && report.graph.warnings.is_empty());
        let mut b = OfflineBackend::new(SR, 512);
        b.start(r).unwrap();
        engine.play().unwrap();
        for _ in 0..100_000 {
            b.run_frames(512);
            if engine.take_ended() {
                break;
            }
        }
        let text = log.drain_text();
        assert_eq!(text.lines().count(), 335, "{workers} worker(s)");
        assert_eq!(format!("{:016x}", fnv(&text)), "1feb33c1bfd60916", "{workers} worker(s)");
        assert!(engine.faults().is_empty());
    }
}

/// The demo plus a reverb-ish bus: synth → (gain, latency 37) → master and
/// a post send to "Verb" (noise, latency 100; then a sidechain adder fed by
/// the synth).
fn mixed() -> (Document, SeqId) {
    let (doc, song) = song();
    let mut b = Builder::from_doc(doc);
    let synth = b.strip_of(StripKind::BuiltinSynth).unwrap();
    let verb = b.bus("Verb").unwrap();
    b.insert(synth, TestInserts::plugin("gain=0.8,latency=37")).unwrap();
    b.insert(verb, TestInserts::plugin("noise=5,latency=100")).unwrap();
    let sc = b.insert(verb, TestInserts::plugin("sc")).unwrap();
    b.mixer(MixerCmd::SetSidechain { slot: sc, source: Some(synth) }).unwrap();
    let mut send = AuxSend::new(verb);
    send.level_db = -6.0;
    b.mixer(MixerCmd::AddSend { strip: synth, send }).unwrap();
    (b.finish(), song)
}

fn mixed_audio(workers: usize, sizes: &[u32]) -> Vec<f32> {
    let (doc, song) = mixed();
    let mut cfg = EngineConfig::new(SR);
    cfg.workers = workers;
    let (mut engine, r) = Engine::new(cfg, Box::new(OfflineSinks::new(1 << 16).with_tone())).unwrap();
    engine.set_insert_factory(Box::new(TestInserts::new()));
    let report = engine.load(&doc, song).unwrap();
    assert_eq!(report.graph.latency_frames, 137);
    let mut b = OfflineBackend::new(SR, 512);
    b.start(r).unwrap();
    engine.play().unwrap();
    let out = render(&mut b, 3 * 48_000, sizes);
    assert!(engine.faults().is_empty(), "{:?}", engine.faults().names());
    out
}

#[test]
fn audio_is_identical_across_workers_and_block_sizes() {
    let reference = mixed_audio(0, &[512]);
    assert!(reference.iter().any(|x| *x != 0.0));
    let bits = |v: &[f32]| v.iter().map(|x| x.to_bits()).collect::<Vec<_>>();
    for (w, sizes) in [(1, &[64u32][..]), (3, &[333, 512][..]), (8, &[1024, 7, 4096][..])] {
        assert_eq!(bits(&mixed_audio(w, sizes)), bits(&reference), "{w} worker(s), {sizes:?}");
    }
}

/// One note on the built-in synth through an insert of latency `latency`.
fn one_note(latency: u32) -> (Document, SeqId) {
    let mut b = Builder::new();
    let d = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, lane) = b.sequence("Bar", "Notes", Some(d)).unwrap();
    b.notes(seq, lane, &[(0, 60, 0x8000, Q / 2)]).unwrap();
    let synth = b.strip_of(StripKind::BuiltinSynth).unwrap();
    b.insert(synth, TestInserts::plugin(&format!("latency={latency}"))).unwrap();
    (b.finish(), seq)
}

#[test]
fn graph_latency_delays_midi_out_and_the_position() {
    let (doc, seq) = one_note(480);
    let mut cfg = EngineConfig::new(SR);
    cfg.route = BuiltinRoute::Both;
    let sinks = OfflineSinks::new(1000);
    let log = sinks.log();
    let (mut engine, r) = Engine::new(cfg, Box::new(sinks)).unwrap();
    engine.set_insert_factory(Box::new(TestInserts::new()));
    let report = engine.load(&doc, seq).unwrap();
    assert_eq!(report.graph.latency_frames, 480);
    assert_eq!(engine.latency_frames(), 480);
    let mut b = OfflineBackend::new(SR, 512);
    b.start(r).unwrap();
    engine.play().unwrap();
    b.run_frames(24_000);
    let text = log.drain_text();
    for line in ["0 t0 90 3c 40", "480 t1 90 3c 40", "12000 t0 80 3c 00", "12480 t1 80 3c 00"] {
        assert!(text.lines().any(|l| l == line), "missing {line:?} in\n{text}");
    }
    assert_eq!(engine.position_frames(), 24_000 - 480);
}

#[test]
fn limits_keep_the_previous_graph() {
    let build = |latency: u32| {
        let mut b = Builder::new();
        let d = b.dest("Synth", "builtin:synth", 0).unwrap();
        let (seq, _) = b.sequence("S", "L", Some(d)).unwrap();
        let synth = b.strip_of(StripKind::BuiltinSynth).unwrap();
        let long = b.bus("Long").unwrap();
        b.mixer(MixerCmd::AddSend { strip: synth, send: AuxSend::new(long) }).unwrap();
        b.insert(long, TestInserts::plugin(&format!("latency={latency}"))).unwrap();
        (b.finish(), seq)
    };
    let (mut engine, _r) = Engine::new(EngineConfig::new(SR), Box::new(OfflineSinks::new(10))).unwrap();
    engine.set_insert_factory(Box::new(TestInserts::new()));
    let (ok, seq) = build(100);
    assert_eq!(engine.load(&ok, seq).unwrap().graph.latency_frames, 100);
    let (bad, seq) = build(96_001);
    let report = engine.load(&bad, seq).unwrap();
    assert!(report.graph.error.as_deref().unwrap().contains("exceeds the 2 s limit"));
    assert!(engine.faults().contains(Faults::LATENCY_TOO_LARGE));
    assert_eq!(engine.latency_frames(), 100, "the previous graph keeps playing");
}

#[test]
fn meters_and_load_are_published() {
    let (doc, song) = song();
    let (mut engine, r) =
        Engine::new(EngineConfig::new(SR), Box::new(OfflineSinks::new(1 << 16).with_tone())).unwrap();
    engine.load(&doc, song).unwrap();
    let mut b = OfflineBackend::new(SR, 512);
    b.start(r).unwrap();
    engine.play().unwrap();
    b.run_frames(48_000);
    let meters = engine.meters();
    let strips: Vec<StripId> = meters.iter().map(|m| m.strip).collect();
    assert_eq!(strips, vec![StripId(0), StripId(1)]);
    assert!(meters.iter().all(|m| m.peak_l > 0.0 && m.peak_r > 0.0), "{meters:?}");
    assert!(engine.meters().iter().all(|m| m.peak_l == 0.0), "read and cleared");
    let load = engine.dsp_load();
    assert!(load.last.is_finite() && load.last >= 0.0 && load.peak >= 0.0);
}

#[test]
fn plugin_fault_is_reported_by_instance() {
    let mut b = Builder::new();
    let d = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, _) = b.sequence("S", "L", Some(d)).unwrap();
    let synth = b.strip_of(StripKind::BuiltinSynth).unwrap();
    let nan = b.insert(synth, TestInserts::plugin("nan")).unwrap();
    let doc = b.finish();
    let (mut engine, r) = Engine::new(EngineConfig::new(SR), Box::new(OfflineSinks::new(10))).unwrap();
    engine.set_insert_factory(Box::new(TestInserts::new()));
    engine.load(&doc, seq).unwrap();
    let mut b = OfflineBackend::new(SR, 256);
    b.start(r).unwrap();
    b.run_frames(20 * 256);
    assert_eq!(engine.plugin_faults(), vec![nan]);
    assert!(engine.faults().contains(Faults::PLUGIN_FAULT));
    assert_eq!(engine.counters().plugin_faults, 1);
}

/// Noise on the synth strip, sent to "Wet" (×0.5, latency 64), so master's
/// input from the synth carries a 64-frame compensation delay.
fn wet(master_db: f64) -> (Document, SeqId) {
    let mut b = Builder::new();
    let d = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, _) = b.sequence("S", "L", Some(d)).unwrap();
    let synth = b.strip_of(StripKind::BuiltinSynth).unwrap();
    b.insert(synth, TestInserts::plugin("noise=9")).unwrap();
    let wet = b.bus("Wet").unwrap();
    b.insert(wet, TestInserts::plugin("gain=0.5,latency=64")).unwrap();
    b.mixer(MixerCmd::AddSend { strip: synth, send: AuxSend::new(wet) }).unwrap();
    b.mixer(MixerCmd::SetGain { strip: StripId(0), gain_db: master_db }).unwrap();
    (b.finish(), seq)
}

#[test]
fn parameter_edits_reuse_instances_and_state() {
    let factory = TestInserts::new();
    let (doc1, seq) = wet(0.0);
    let (doc2, _) = wet(-6.0);
    let (mut engine, r) = Engine::new(EngineConfig::new(SR), Box::new(OfflineSinks::new(10))).unwrap();
    engine.set_insert_factory(Box::new(factory.clone()));
    engine.load(&doc1, seq).unwrap();
    assert_eq!(factory.made(), 2);
    let mut b = OfflineBackend::new(SR, 512);
    b.start(r).unwrap();
    let _ = render(&mut b, 5120, &[512]);
    engine.load(&doc2, seq).unwrap();
    assert_eq!(factory.made(), 2, "a fader edit creates no instance");
    let edited = render(&mut b, 5120, &[512]);

    let (mut reference, r) = Engine::new(EngineConfig::new(SR), Box::new(OfflineSinks::new(10))).unwrap();
    reference.set_insert_factory(Box::new(TestInserts::new()));
    reference.load(&doc2, seq).unwrap();
    let mut rb = OfflineBackend::new(SR, 512);
    rb.start(r).unwrap();
    let whole = render(&mut rb, 10_240, &[512]);
    assert_eq!(edited, whole[5120..].to_vec(), "no glitch: delay memory and noise state kept");

    // A structural edit adopts the instances too; a new insert is made.
    let mut b2 = Builder::from_doc(doc2);
    b2.bus("Extra").unwrap();
    engine.load(b2.doc(), seq).unwrap();
    assert_eq!(factory.made(), 2);
    b2.insert(StripId(0), TestInserts::plugin("gain=1")).unwrap();
    engine.load(b2.doc(), seq).unwrap();
    assert_eq!(factory.made(), 3);
}

#[test]
fn placeholders_pass_audio_and_are_reported() {
    let (doc, song) = song();
    let mut b = Builder::from_doc(doc);
    let synth = b.strip_of(StripKind::BuiltinSynth).unwrap();
    b.insert(synth, PluginRef::from_legacy("clap:missing")).unwrap();
    let (mut engine, r) =
        Engine::new(EngineConfig::new(SR), Box::new(OfflineSinks::new(1 << 16).with_tone())).unwrap();
    let report = engine.load(b.doc(), song).unwrap();
    assert_eq!(report.graph.placeholders.len(), 1);
    assert_eq!(report.graph.placeholders[0].0, PluginInstanceId(0));
    assert!(report.graph.placeholders[0].1.contains("clap:missing"));
    assert_eq!(report.graph.latency_frames, 0);
    let mut ob = OfflineBackend::new(SR, 512);
    ob.start(r).unwrap();
    engine.play().unwrap();
    let out = render(&mut ob, 48_000, &[512]);
    assert!(out.iter().any(|x| *x != 0.0), "the placeholder passes the synth");
}

#[test]
fn prepare_resizes_graph_buffers() {
    let (doc, song) = song();
    let mut cfg = EngineConfig::new(SR);
    cfg.max_frames = 512;
    let (mut engine, r) = Engine::new(cfg, Box::new(OfflineSinks::new(1 << 16))).unwrap();
    engine.load(&doc, song).unwrap();
    let mut b = OfflineBackend::new(SR, 512);
    b.start(r).unwrap();
    b.run_blocks([512, 2048]);
    assert!(engine.faults().contains(Faults::XRUN), "2048 > 512 before prepare");
    let mut r = b.stop().unwrap();
    r.prepare(2048);
    b.start(r).unwrap();
    b.run_blocks([2048, 2048]);
    assert!(engine.faults().is_empty());
}

#[test]
fn a_stalled_node_misses_the_deadline_without_blocking() {
    let flag = Arc::new(AtomicBool::new(false));
    let mut bld = Builder::new();
    let d = bld.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, _) = bld.sequence("S", "L", Some(d)).unwrap();
    let synth = bld.strip_of(StripKind::BuiltinSynth).unwrap();
    bld.insert(synth, TestInserts::plugin("stall")).unwrap();
    let doc = bld.finish();
    let mut cfg = EngineConfig::new(SR);
    cfg.workers = 1;
    cfg.deadline = Some(0.5);
    let (mut engine, r) = Engine::new(cfg, Box::new(OfflineSinks::new(10))).unwrap();
    engine.set_insert_factory(Box::new(TestInserts::with_stall(Arc::clone(&flag))));
    engine.set_callback_helps(false);
    engine.load(&doc, seq).unwrap();
    let mut b = OfflineBackend::new(SR, 512);
    b.start(r).unwrap();
    b.run_blocks([512, 512]);
    let c = engine.counters();
    assert_eq!((c.deadline_misses, c.busy_blocks), (1, 1));
    assert!(engine.faults().contains(Faults::XRUN));
    flag.store(true, Ordering::Release);
    std::thread::sleep(std::time::Duration::from_millis(50));
    b.run_blocks([512]);
    assert_eq!(engine.counters().busy_blocks, 1, "recovered");
}
```

In `crates/core-engine/tests/engine.rs`, in `events_land_at_their_offsets_and_stop_releases`, replace

```rust
    assert_eq!(out[0], 0.25, "instrument output is mixed into channel 0");
```

with

```rust
    assert_eq!(
        out[0],
        0.25 * std::f32::consts::FRAC_1_SQRT_2,
        "instrument output reaches channel 0 through the built-in strip (−3 dB pan law)"
    );
```

- [ ] **Step 2: Run the test to verify it fails**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-engine --test mixer_engine
```

Expected: compile errors — no field `graph` on `CompileReport`, no `with_tone`, no `set_insert_factory`.

- [ ] **Step 3: Instrument latency and the graph report**

In `crates/core-engine/src/backend.rs`, add to `trait InstrumentNode` after `skip`:

```rust
    /// Output latency in frames, read on the control thread when the sink
    /// is created (the DLS synth: 0).
    fn latency(&self) -> u32 {
        0
    }
```

In `crates/core-engine/src/graph/mod.rs` add `use core_model::ids::PluginInstanceId;` and:

```rust
/// What compiling the mixer graph did (`CompileReport::graph`, Plan
/// Ruling 13).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GraphReport {
    /// Nodes in the graph.
    pub nodes: usize,
    /// Total latency `L` in frames.
    pub latency_frames: u64,
    /// Slots whose processor could not be created, and why (they pass
    /// audio with no latency).
    pub placeholders: Vec<(PluginInstanceId, String)>,
    /// Other problems (no built-in strip, strips without an instrument).
    pub warnings: Vec<String>,
    /// Why this graph was not used; the previous one keeps playing.
    pub error: Option<String>,
}
```

In `crates/core-engine/src/schedule/compile.rs` add to `CompileReport` after `compile_time`:

```rust
    /// The mixer graph compiled with this schedule (`Engine::load`).
    pub graph: crate::graph::GraphReport,
```

- [ ] **Step 4: The renderer**

Replace `crates/core-engine/src/render.rs` with:

```rust
//! The real-time renderer and the state it publishes.

use crate::backend::{BlockTime, MidiOut, StereoBuf};
use crate::graph::exec::{Exec, RunResult, SinkTable};
use crate::graph::{Graph, MAX_INSERTS, MAX_NODES};
use crate::notes::ActiveNotes;
use crate::pool::PoolShared;
use crate::schedule::{MAX_TARGETS, Schedule, TargetIdx};
use crate::transport::{PlayState, Step, Transport};
use core_model::ids::PluginInstanceId;
use rtrb::{Consumer, Producer};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Real-time fault bits, cleared when read by [`crate::engine::Engine::faults`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Faults(pub u32);

impl Faults {
    /// A block was larger than the prepared size, missed its deadline or
    /// found stragglers running (rendered as silence).
    pub const XRUN: Faults = Faults(1);
    /// The master bus was clamped at ±1.0.
    pub const CLIPPED: Faults = Faults(2);
    /// A MIDI output queue was full; a message was dropped.
    pub const MIDI_DROPPED: Faults = Faults(4);
    /// A schedule or graph swap waited a block because the retire ring was
    /// full.
    pub const SWAP_DEFERRED: Faults = Faults(8);
    /// The audio callback panicked; output is silent.
    pub const PANIC: Faults = Faults(16);
    /// A recompiled graph exceeded 1024 nodes or 4096 inserts; the
    /// previous graph keeps playing.
    pub const GRAPH_TOO_LARGE: Faults = Faults(32);
    /// A recompiled graph needs more than 2 s of delay on an edge; the
    /// previous graph keeps playing.
    pub const LATENCY_TOO_LARGE: Faults = Faults(64);
    /// An insert produced 16 faulty blocks in a row and was bypassed
    /// (`Engine::plugin_faults` names it).
    pub const PLUGIN_FAULT: Faults = Faults(128);
    /// A render worker could not join the device's workgroup.
    pub const WORKGROUP_JOIN_FAILED: Faults = Faults(256);
    /// A render worker could not get real-time scheduling.
    pub const THREAD_POLICY_FAILED: Faults = Faults(512);
    /// A recompiled graph was invalid (cycle or bad index); the previous
    /// graph keeps playing.
    pub const GRAPH_INVALID: Faults = Faults(1024);

    /// True when every bit of `other` is set.
    pub fn contains(self, other: Faults) -> bool {
        self.0 & other.0 == other.0
    }

    /// True when no bit is set.
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Names of the set bits.
    pub fn names(self) -> Vec<&'static str> {
        [
            (Faults::XRUN, "xrun"),
            (Faults::CLIPPED, "clipped"),
            (Faults::MIDI_DROPPED, "midi-dropped"),
            (Faults::SWAP_DEFERRED, "swap-deferred"),
            (Faults::PANIC, "panic"),
            (Faults::GRAPH_TOO_LARGE, "graph-too-large"),
            (Faults::LATENCY_TOO_LARGE, "latency-too-large"),
            (Faults::PLUGIN_FAULT, "plugin-fault"),
            (Faults::WORKGROUP_JOIN_FAILED, "workgroup-join-failed"),
            (Faults::THREAD_POLICY_FAILED, "thread-policy-failed"),
            (Faults::GRAPH_INVALID, "graph-invalid"),
        ]
        .into_iter()
        .filter(|(f, _)| self.contains(*f))
        .map(|(_, n)| n)
        .collect()
    }
}

/// Counters published by the renderer.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counters {
    /// MIDI messages dropped because an output queue was full.
    pub midi_dropped: u64,
    /// Blocks in which a schedule or graph swap was deferred.
    pub swaps_deferred: u64,
    /// Blocks rendered as silence because they were too large.
    pub xruns: u64,
    /// Blocks rendered as silence because the graph missed its deadline.
    pub deadline_misses: u64,
    /// Blocks rendered as silence because nodes of a missed block were
    /// still running.
    pub busy_blocks: u64,
    /// Inserts bypassed for faults.
    pub plugin_faults: u64,
}

/// Peak meter of one graph node: the largest |sample| per channel since
/// the last read, as `f32` bits (non-negative floats order like their
/// bits, so `fetch_max` works).
#[derive(Debug, Default)]
pub struct MeterCell {
    l: AtomicU32,
    r: AtomicU32,
}

impl MeterCell {
    /// Raises the held peaks. // RT
    pub fn hold(&self, l: f32, r: f32) {
        if l.is_finite() && l > 0.0 {
            self.l.fetch_max(l.to_bits(), Ordering::Relaxed);
        }
        if r.is_finite() && r > 0.0 {
            self.r.fetch_max(r.to_bits(), Ordering::Relaxed);
        }
    }

    /// Reads and clears the held peaks.
    pub fn take(&self) -> (f32, f32) {
        (
            f32::from_bits(self.l.swap(0, Ordering::Relaxed)),
            f32::from_bits(self.r.swap(0, Ordering::Relaxed)),
        )
    }
}

/// DSP load: render time ÷ block duration.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct DspLoad {
    /// The last block.
    pub last: f32,
    /// The highest since the last read.
    pub peak: f32,
}

/// State shared between the renderer, its workers and the control side
/// (atomics only).
#[derive(Debug)]
pub struct Shared {
    position: AtomicU64,
    state: AtomicU8,
    faults: AtomicU32,
    ended: AtomicBool,
    midi_dropped: AtomicU64,
    swaps_deferred: AtomicU64,
    xruns: AtomicU64,
    deadline_misses: AtomicU64,
    busy_blocks: AtomicU64,
    plugin_faults: AtomicU64,
    load_last: AtomicU32,
    load_peak: AtomicU32,
    meters: Box<[MeterCell]>,
    board: Box<[AtomicU64]>,
}

impl Default for Shared {
    fn default() -> Shared {
        Shared {
            position: AtomicU64::new(0),
            state: AtomicU8::new(0),
            faults: AtomicU32::new(0),
            ended: AtomicBool::new(false),
            midi_dropped: AtomicU64::new(0),
            swaps_deferred: AtomicU64::new(0),
            xruns: AtomicU64::new(0),
            deadline_misses: AtomicU64::new(0),
            busy_blocks: AtomicU64::new(0),
            plugin_faults: AtomicU64::new(0),
            load_last: AtomicU32::new(0),
            load_peak: AtomicU32::new(0),
            meters: (0..MAX_NODES).map(|_| MeterCell::default()).collect(),
            board: (0..MAX_INSERTS).map(|_| AtomicU64::new(0)).collect(),
        }
    }
}

impl Shared {
    /// Sets fault bits. // RT
    pub fn raise(&self, f: Faults) {
        self.faults.fetch_or(f.0, Ordering::Relaxed);
    }

    /// Reads and clears the fault bits.
    pub fn take_faults(&self) -> Faults {
        Faults(self.faults.swap(0, Ordering::Relaxed))
    }

    /// Schedule frame of the transport, minus the graph latency (Plan
    /// Ruling 17).
    pub fn position(&self) -> u64 {
        self.position.load(Ordering::Relaxed)
    }

    /// Transport state.
    pub fn state(&self) -> PlayState {
        PlayState::from_u8(self.state.load(Ordering::Relaxed))
    }

    /// True once after playback reached the end and the tail finished.
    pub fn take_ended(&self) -> bool {
        self.ended.swap(false, Ordering::Relaxed)
    }

    /// Current counters.
    pub fn counters(&self) -> Counters {
        Counters {
            midi_dropped: self.midi_dropped.load(Ordering::Relaxed),
            swaps_deferred: self.swaps_deferred.load(Ordering::Relaxed),
            xruns: self.xruns.load(Ordering::Relaxed),
            deadline_misses: self.deadline_misses.load(Ordering::Relaxed),
            busy_blocks: self.busy_blocks.load(Ordering::Relaxed),
            plugin_faults: self.plugin_faults.load(Ordering::Relaxed),
        }
    }

    /// The meter of graph node `i`. // RT
    pub fn meter(&self, i: usize) -> Option<&MeterCell> {
        self.meters.get(i)
    }

    /// Records that the insert at board index `slot` was bypassed for
    /// faults. // RT
    pub fn report_plugin_fault(&self, slot: usize, instance: Option<PluginInstanceId>) {
        self.raise(Faults::PLUGIN_FAULT);
        self.plugin_faults.fetch_add(1, Ordering::Relaxed);
        if let (Some(cell), Some(id)) = (self.board.get(slot), instance) {
            cell.store(id.0.saturating_add(1), Ordering::Relaxed);
        }
    }

    /// Instances bypassed for faults since the last call (read and
    /// cleared).
    pub fn take_plugin_faults(&self) -> Vec<PluginInstanceId> {
        self.board
            .iter()
            .filter_map(|c| {
                let v = c.swap(0, Ordering::Relaxed);
                (v != 0).then(|| PluginInstanceId(v - 1))
            })
            .collect()
    }

    /// Stores one block's DSP load. // RT
    pub fn set_load(&self, load: f32) {
        let bits = load.max(0.0).to_bits();
        self.load_last.store(bits, Ordering::Relaxed);
        self.load_peak.fetch_max(bits, Ordering::Relaxed);
    }

    /// The last block's load and the peak since the last call (cleared).
    pub fn take_load(&self) -> DspLoad {
        DspLoad {
            last: f32::from_bits(self.load_last.load(Ordering::Relaxed)),
            peak: f32::from_bits(self.load_peak.swap(0, Ordering::Relaxed)),
        }
    }

    /// Counts a block that missed its deadline. // RT
    pub fn count_deadline_miss(&self) {
        self.raise(Faults::XRUN);
        self.deadline_misses.fetch_add(1, Ordering::Relaxed);
    }

    /// Counts a block skipped because stragglers were running. // RT
    pub fn count_busy_block(&self) {
        self.raise(Faults::XRUN);
        self.busy_blocks.fetch_add(1, Ordering::Relaxed);
    }
}

/// Where a target's events go.
pub enum Sink {
    /// An instrument rendered into the graph.
    Instrument(Box<dyn crate::backend::InstrumentNode>),
    /// A timestamped MIDI output.
    Midi(Box<dyn MidiOut>),
}

impl std::fmt::Debug for Sink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Sink::Instrument(_) => "Sink::Instrument",
            Sink::Midi(_) => "Sink::Midi",
        })
    }
}

/// Commands from the control side, applied at the start of a block.
#[derive(Debug)]
pub enum EngineCmd {
    /// Replace the schedule, keeping the musical position.
    SetSchedule(Box<Schedule>),
    /// Replace the render graph; it adopts the running graph's inserts and,
    /// with the same shape, its node state.
    SetGraph(Box<Graph>),
    /// Start playing.
    Play,
    /// Stop (note-offs are sent).
    Stop,
    /// Move to a schedule frame.
    Locate(u64),
    /// Set or clear the loop range.
    SetLoop(Option<(u64, u64)>),
    /// Install the sink of a new target (targets are only appended).
    AddSink(TargetIdx, Sink),
}

/// Something the renderer hands back for freeing off the audio thread.
#[derive(Debug)]
pub enum Retired {
    /// A replaced schedule.
    Schedule(Box<Schedule>),
    /// A replaced graph (with the inserts it did not hand on).
    Graph(Box<Graph>),
}

/// Capacity of the command ring.
pub const CMD_CAPACITY: usize = 64;
/// Capacity of the retire ring: eight edits, each retiring a schedule and
/// a graph (Plan Ruling 30).
pub const RETIRE_CAPACITY: usize = 16;

/// Renderer settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderConfig {
    /// Stream sample rate.
    pub sample_rate: f64,
    /// Largest block (≤ [`crate::backend::MAX_FRAMES`] is typical).
    pub max_frames: u32,
    /// Added to every MIDI-out timestamp.
    pub midi_offset_ns: i64,
    /// Fraction of the block duration the graph may take before the block
    /// is given up (Plan Ruling 18); `None` waits for the graph.
    pub deadline: Option<f64>,
}

/// The real-time object. Built by [`crate::engine::Engine::new`], moved to
/// the audio thread by a backend and handed back on stop.
pub struct Renderer {
    cfg: RenderConfig,
    cmds: Consumer<EngineCmd>,
    retire: Producer<Retired>,
    shared: Arc<Shared>,
    schedule: Option<Box<Schedule>>,
    cursor: usize,
    transport: Transport,
    notes: ActiveNotes,
    sinks: Box<SinkTable>,
    bufs: Vec<StereoBuf>,
    block_ns: u64,
    /// Controllers were reset (stop, locate, wrap or swap): the next played
    /// frames start with the chased state.
    chase_pending: bool,
    graph: Option<Box<Graph>>,
    exec: Box<Exec>,
    /// Graph latency `L` in frames (Plan Ruling 17).
    latency: u64,
    /// Frames of busy blocks the sinks have not been told about.
    pending_skip: u64,
}

impl std::fmt::Debug for Renderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Renderer")
            .field("cfg", &self.cfg)
            .field("transport", &self.transport)
            .field("graph", &self.graph)
            .finish_non_exhaustive()
    }
}

/// Everything `send` needs, borrowed apart from the note tracker.
struct Out<'a> {
    sinks: &'a mut SinkTable,
    shared: &'a Shared,
    block_ns: u64,
    ns_per_frame: f64,
    /// MIDI offset plus the graph latency, in ns.
    late_ns: i64,
}

impl Out<'_> {
    /// Sends `bytes` to `target` at block offset `offset`. // RT
    fn send(&mut self, target: TargetIdx, offset: u32, bytes: &[u8]) {
        match self.sinks.get_mut(usize::from(target.0)) {
            Some(Some(Sink::Instrument(node))) => node.midi(offset, bytes),
            Some(Some(Sink::Midi(out))) => {
                let at = self.block_ns as i64
                    + (f64::from(offset) * self.ns_per_frame).round() as i64
                    + self.late_ns;
                if !out.send(at.max(0) as u64, bytes) {
                    self.shared.raise(Faults::MIDI_DROPPED);
                    self.shared.midi_dropped.fetch_add(1, Ordering::Relaxed);
                }
            }
            _ => {}
        }
    }
}

/// Tells every instrument sink not in `except` (a target bitmask) that
/// `frames` frames passed unrendered. // RT
fn skip_sinks(sinks: &mut SinkTable, frames: u64, except: u16) {
    for (t, s) in sinks.iter_mut().enumerate() {
        let bit = u32::try_from(t).ok().and_then(|t| 1u16.checked_shl(t)).unwrap_or(0);
        if except & bit != 0 {
            continue;
        }
        if let Some(Sink::Instrument(node)) = s {
            let mut left = frames;
            while left > 0 {
                let k = u32::try_from(left).unwrap_or(u32::MAX);
                node.skip(k);
                left -= u64::from(k);
            }
        }
    }
}

/// Hard clamp of channels 0–1 at ±1.0; non-finite samples become 0. Raises
/// `CLIPPED` when anything was changed (engine ruling). // RT
fn clamp_master(out: &mut [&mut [f32]], frames: u32, shared: &Shared) {
    let n = frames as usize;
    let mut clipped = false;
    for ch in out.iter_mut().take(2) {
        if let Some(c) = ch.get_mut(..n) {
            for x in c {
                if !x.is_finite() {
                    clipped = true;
                    *x = 0.0;
                } else if *x > 1.0 || *x < -1.0 {
                    clipped = true;
                    *x = x.clamp(-1.0, 1.0);
                }
            }
        }
    }
    if clipped {
        shared.raise(Faults::CLIPPED);
    }
}

impl Renderer {
    pub(crate) fn new(
        cfg: RenderConfig,
        cmds: Consumer<EngineCmd>,
        retire: Producer<Retired>,
        shared: Arc<Shared>,
        pool: Option<Arc<PoolShared>>,
    ) -> Renderer {
        let mut r = Renderer {
            cfg,
            cmds,
            retire,
            shared,
            schedule: None,
            cursor: 0,
            transport: Transport::new(cfg.sample_rate.max(0.0).round() as u64),
            notes: ActiveNotes::default(),
            sinks: Box::default(),
            bufs: Vec::new(),
            block_ns: 0,
            chase_pending: false,
            graph: None,
            exec: Exec::new(pool),
            latency: 0,
            pending_skip: 0,
        };
        r.prepare(cfg.max_frames);
        r
    }

    /// (Re)allocates scratch and graph buffers for blocks of up to
    /// `max_frames`. Not real-time safe: call only while stopped.
    pub fn prepare(&mut self, max_frames: u32) {
        self.exec.wait_idle();
        self.cfg.max_frames = max_frames;
        self.bufs = (0..MAX_TARGETS)
            .map(|_| StereoBuf::new(max_frames))
            .collect();
        if let Some(g) = self.graph.as_mut() {
            g.prepare(max_frames);
        }
    }

    /// Settings.
    pub fn config(&self) -> RenderConfig {
        self.cfg
    }

    /// Sets fault bits (used by backends, e.g. after a caught panic). // RT
    pub fn raise(&self, f: Faults) {
        self.shared.raise(f);
    }

    /// Renders one block of `frames` frames into planar `out` channels.
    /// Channels 0–1 get master, the rest zeros. // RT
    pub fn process(&mut self, out: &mut [&mut [f32]], frames: u32, time: BlockTime) {
        #[cfg(feature = "rt-check")]
        crate::rt_check::arm();
        self.process_inner(out, frames, time);
        #[cfg(feature = "rt-check")]
        crate::rt_check::disarm();
    }

    fn latency_ns(&self) -> i64 {
        (self.latency as f64 * 1e9 / self.cfg.sample_rate.max(1.0)).round() as i64
    }

    fn process_inner(&mut self, out: &mut [&mut [f32]], frames: u32, time: BlockTime) {
        let started = Instant::now();
        let block_s = f64::from(frames) / self.cfg.sample_rate.max(1.0);
        let deadline = self
            .cfg
            .deadline
            .and_then(|f| Duration::try_from_secs_f64(f * block_s).ok())
            .and_then(|d| started.checked_add(d));
        let n = frames as usize;
        for ch in out.iter_mut() {
            ch.fill(0.0);
        }
        if !self.exec.try_finish(deadline) {
            // Nodes of a missed block are still running: touch nothing they
            // use (Plan Ruling 18).
            self.shared.count_busy_block();
            self.pending_skip = self.pending_skip.saturating_add(u64::from(frames));
            return;
        }
        if self.pending_skip > 0 {
            let skip = std::mem::take(&mut self.pending_skip);
            skip_sinks(&mut self.sinks, skip, 0);
        }
        let graph_small = self.graph.as_ref().is_some_and(|g| g.max_frames() < frames);
        if frames > self.cfg.max_frames || graph_small || out.iter().any(|c| c.len() < n) {
            skip_sinks(&mut self.sinks, u64::from(frames), 0);
            self.shared.raise(Faults::XRUN);
            self.shared.xruns.fetch_add(1, Ordering::Relaxed);
            return;
        }
        self.block_ns = time.host_time_ns;
        self.drain_commands();
        let length = self.schedule.as_ref().map_or(0, |s| s.length_frames);
        let late_ns = self.cfg.midi_offset_ns + self.latency_ns();
        let mut o = Out {
            sinks: &mut self.sinks,
            shared: &self.shared,
            block_ns: time.host_time_ns,
            ns_per_frame: 1e9 / self.cfg.sample_rate.max(1.0),
            late_ns,
        };
        let mut offset = 0u32;
        while offset < frames {
            let step = self.transport.next(length, offset, frames - offset);
            match step {
                Step::Play { from, to, offset } => {
                    if let Some(s) = self.schedule.as_deref() {
                        if std::mem::take(&mut self.chase_pending) {
                            s.chase
                                .state_before(from, |t, m| o.send(t, offset, m.as_slice()));
                        }
                        while let Some(e) = s.entries.get(self.cursor) {
                            if e.frame >= to {
                                break;
                            }
                            let off =
                                offset + u32::try_from(e.frame.saturating_sub(from)).unwrap_or(0);
                            let bytes = s.bytes(&e.msg);
                            o.send(e.target, off, bytes);
                            self.notes.observe(e.target, bytes);
                            self.cursor += 1;
                        }
                    }
                }
                Step::Wrap { offset, to } => {
                    release_all(&mut self.notes, &mut o, offset);
                    if let Some(s) = self.schedule.as_deref() {
                        reset_controllers(s, &mut o, offset);
                    }
                    self.chase_pending = true;
                    self.cursor = self.schedule.as_ref().map_or(0, |s| s.cursor_at(to));
                }
                Step::End { offset } => release_all(&mut self.notes, &mut o, offset),
                Step::Idle { .. } => {}
            }
            offset += step.frames();
        }
        self.render_graph(out, frames, deadline);
        self.publish();
        if block_s > 0.0 {
            self.shared
                .set_load((started.elapsed().as_secs_f64() / block_s) as f32);
        }
    }

    /// Renders the graph (or, before the first graph arrives, sums the
    /// instrument sinks) into channels 0–1 and clamps. // RT
    fn render_graph(&mut self, out: &mut [&mut [f32]], frames: u32, deadline: Option<Instant>) {
        let Some(g) = self.graph.as_deref() else {
            self.mix_direct(out, frames);
            clamp_master(out, frames, &self.shared);
            return;
        };
        match self
            .exec
            .run(g, &self.sinks, &self.shared, frames, deadline)
        {
            RunResult::Done => {
                g.write_master(out, frames);
                let covered = g.instruments();
                skip_sinks(&mut self.sinks, u64::from(frames), covered);
                clamp_master(out, frames, &self.shared);
            }
            RunResult::Missed => self.shared.count_deadline_miss(),
        }
    }

    /// Applies queued commands. A `SetSchedule` or `SetGraph` waits when
    /// the retire ring is full (and everything after it waits too, to keep
    /// order). // RT
    fn drain_commands(&mut self) {
        loop {
            let deferred = match self.cmds.peek() {
                Ok(EngineCmd::SetSchedule(_)) => {
                    self.schedule.is_some() && self.retire.slots() == 0
                }
                Ok(EngineCmd::SetGraph(_)) => self.graph.is_some() && self.retire.slots() == 0,
                Ok(_) => false,
                Err(_) => break,
            };
            if deferred {
                self.shared.raise(Faults::SWAP_DEFERRED);
                self.shared.swaps_deferred.fetch_add(1, Ordering::Relaxed);
                break;
            }
            let Ok(cmd) = self.cmds.pop() else {
                break;
            };
            self.apply(cmd);
        }
    }

    /// Note-offs, then a controller reset on every channel of the current
    /// schedule, at block offset 0; the next played frames chase the
    /// controller state (ruling O23). // RT
    fn release_now(&mut self) {
        let late_ns = self.cfg.midi_offset_ns + self.latency_ns();
        let mut o = Out {
            sinks: &mut self.sinks,
            shared: &self.shared,
            block_ns: self.block_ns,
            ns_per_frame: 0.0,
            late_ns,
        };
        release_all(&mut self.notes, &mut o, 0);
        if let Some(s) = self.schedule.as_deref() {
            reset_controllers(s, &mut o, 0);
        }
        self.chase_pending = true;
    }

    fn apply(&mut self, cmd: EngineCmd) {
        match cmd {
            EngineCmd::SetSchedule(new) => {
                self.release_now();
                if let Some(old) = self.schedule.take() {
                    let tick = old.tempo.frame_to_tick(self.transport.pos);
                    self.transport.pos = new.tempo.tick_to_frame(tick);
                    self.transport.loop_range = self.transport.loop_range.map(|(s, e)| {
                        (
                            new.tempo.tick_to_frame(old.tempo.frame_to_tick(s)),
                            new.tempo.tick_to_frame(old.tempo.frame_to_tick(e)),
                        )
                    });
                    if let Err(rtrb::PushError::Full(old)) = self.retire.push(Retired::Schedule(old)) {
                        // Unreachable: room was checked in `drain_commands`.
                        // Leaking is the only RT-safe choice.
                        std::mem::forget(old);
                    }
                }
                self.cursor = new.cursor_at(self.transport.pos);
                self.schedule = Some(new);
            }
            EngineCmd::SetGraph(mut new) => {
                if let Some(mut old) = self.graph.take() {
                    new.adopt_from(&mut old);
                    if let Err(rtrb::PushError::Full(old)) = self.retire.push(Retired::Graph(old)) {
                        // Unreachable: room was checked in `drain_commands`.
                        std::mem::forget(old);
                    }
                }
                self.latency = new.latency();
                self.graph = Some(new);
            }
            EngineCmd::Play => {
                let length = self.schedule.as_ref().map_or(0, |s| s.length_frames);
                self.transport.play(length);
                self.seek();
            }
            EngineCmd::Stop => {
                self.release_now();
                self.transport.stop();
            }
            EngineCmd::Locate(frame) => {
                self.release_now();
                self.transport.locate(frame);
                self.seek();
            }
            EngineCmd::SetLoop(range) => self.transport.loop_range = range,
            EngineCmd::AddSink(idx, sink) => {
                if let Some(slot) = self.sinks.get_mut(usize::from(idx.0)) {
                    if let Some(old) = slot.replace(sink) {
                        // Unreachable: the engine only fills empty slots.
                        std::mem::forget(old);
                    }
                }
            }
        }
    }

    fn seek(&mut self) {
        self.cursor = self
            .schedule
            .as_ref()
            .map_or(0, |s| s.cursor_at(self.transport.pos));
    }

    /// Before the first graph: renders instrument sinks and sums them into
    /// channels 0–1 (2b behaviour). // RT
    fn mix_direct(&mut self, out: &mut [&mut [f32]], frames: u32) {
        let n = frames as usize;
        for (sink, buf) in self.sinks.iter_mut().zip(self.bufs.iter_mut()) {
            let Some(Sink::Instrument(node)) = sink else {
                continue;
            };
            node.render(frames, buf);
            for (i, src) in [&buf.left, &buf.right].into_iter().enumerate() {
                let (Some(dst), Some(src)) =
                    (out.get_mut(i).and_then(|c| c.get_mut(..n)), src.get(..n))
                else {
                    continue;
                };
                for (d, s) in dst.iter_mut().zip(src) {
                    *d += *s;
                }
            }
        }
    }

    fn publish(&mut self) {
        let s = &self.shared;
        s.position.store(
            self.transport.pos.saturating_sub(self.latency),
            Ordering::Relaxed,
        );
        s.state.store(self.transport.state as u8, Ordering::Relaxed);
        if self.transport.take_ended() {
            s.ended.store(true, Ordering::Relaxed);
        }
    }

    /// Resets every instrument (all notes off). Call while stopped.
    pub fn reset_instruments(&mut self) {
        self.exec.wait_idle();
        for sink in self.sinks.iter_mut().flatten() {
            if let Sink::Instrument(node) = sink {
                node.reset();
            }
        }
    }

    /// Mutable access to a target's sink (for backends and tests, not RT).
    pub fn sink_mut(&mut self, idx: TargetIdx) -> Option<&mut Sink> {
        self.exec.wait_idle();
        self.sinks
            .get_mut(usize::from(idx.0))
            .and_then(Option::as_mut)
    }

    /// The graph latency `L` in frames.
    pub fn latency(&self) -> u64 {
        self.latency
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        // Stragglers of a missed block may still use the graph and sinks.
        self.exec.wait_idle();
    }
}

/// Pitch-bend centre and CC 121 (reset all controllers) on every channel
/// `s` uses, at block offset `offset`. // RT
fn reset_controllers(s: &Schedule, o: &mut Out<'_>, offset: u32) {
    for (t, mask) in s.used_channels.iter().enumerate() {
        let target = TargetIdx(t as u8);
        for ch in 0..16u8 {
            if mask & (1 << ch) != 0 {
                o.send(target, offset, &[0xE0 | ch, 0x00, 0x40]);
                o.send(target, offset, &[0xB0 | ch, 121, 0x00]);
            }
        }
    }
}

/// Note-offs (and pedal-ups) for every target at block offset `offset`. // RT
fn release_all(notes: &mut ActiveNotes, o: &mut Out<'_>, offset: u32) {
    for t in 0..MAX_TARGETS {
        let target = TargetIdx(t as u8);
        notes.release(target, |m| o.send(target, offset, m.as_slice()));
    }
}
```

What changed from 2b: the retire ring holds 16 items (an edit now retires a schedule and a graph, so 2b's tests still see the ninth edit deferred); sinks live in a `SinkTable` shared with instrument nodes; `process` first finishes the previous run (or, with stragglers still running, outputs silence and remembers the frames), renders the graph instead of summing sinks (the old sum remains as `mix_direct` until the first graph arrives), clamps master exactly as before, adds `L` to MIDI-out timestamps, publishes `pos − L`, and measures DSP load; `SetGraph` swaps graphs like `SetSchedule` swaps schedules, through the same retire ring.

- [ ] **Step 5: The engine**

Replace `crates/core-engine/src/engine.rs` with:

```rust
//! The control handle (non-RT): compiles documents and mixer graphs, sends
//! commands to the renderer and reads what it publishes.

use crate::backend::MidiPortInfo;
use crate::error::{BackendError, EngineError};
use crate::graph::mixer::mixer_spec;
use crate::graph::{EffectNode, Graph, GraphReport, InsertFactory, InsertInit, NoInserts, PlanError, plan};
use crate::pool::{Pool, RtEnv, RtPlatform, StdPlatform};
use crate::render::{
    CMD_CAPACITY, Counters, DspLoad, EngineCmd, Faults, RETIRE_CAPACITY, RenderConfig, Renderer,
    Retired, Shared, Sink,
};
use crate::schedule::{
    BuiltinRoute, CompileOptions, CompileReport, MAX_TARGETS, TargetIdx, TargetKind, TargetTable,
    TempoTable, compile,
};
use crate::transport::PlayState;
use core_model::document::Document;
use core_model::ids::{PluginInstanceId, SeqId, StripId};
use core_model::mixer::{PluginFormat, PluginSlot};
use core_model::time::Tick;
use rtrb::{Producer, RingBuffer};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

/// Creates sinks for new targets (control thread).
pub trait SinkFactory: Send {
    /// MIDI destinations that exist now.
    fn ports(&self) -> Vec<MidiPortInfo>;
    /// Creates the sink of target `target` (the index the engine gives
    /// it) at the stream's rate and block size.
    fn make(
        &mut self,
        target: TargetIdx,
        kind: &TargetKind,
        sample_rate: f64,
        max_frames: u32,
    ) -> Result<Sink, BackendError>;
}

/// Engine settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EngineConfig {
    /// Stream sample rate.
    pub sample_rate: f64,
    /// Largest block the renderer accepts.
    pub max_frames: u32,
    /// Added to MIDI-out timestamps (CLI `--midi-offset-ms`).
    pub midi_offset_ns: i64,
    /// Where `builtin:synth` destinations play.
    pub route: BuiltinRoute,
    /// Render worker threads besides the callback thread (0–15; Plan
    /// Ruling 20).
    pub workers: usize,
    /// Fraction of a block the graph may take (Plan Ruling 18); `None`
    /// waits (offline).
    pub deadline: Option<f64>,
}

impl EngineConfig {
    /// Defaults: 4096-frame blocks, no MIDI offset, built-in routing, no
    /// workers, no deadline.
    pub fn new(sample_rate: f64) -> EngineConfig {
        EngineConfig {
            sample_rate,
            max_frames: crate::backend::MAX_FRAMES,
            midi_offset_ns: 0,
            route: BuiltinRoute::Builtin,
            workers: 0,
            deadline: None,
        }
    }
}

/// One strip's peaks since the last read.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StripMeter {
    /// The strip.
    pub strip: StripId,
    /// Left peak (linear).
    pub peak_l: f32,
    /// Right peak (linear).
    pub peak_r: f32,
}

/// An insert instance in the last published graph. An instance is reused
/// only for a slot with the same ID and the same plugin (format + id).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Live {
    index: usize,
    latency: u32,
    format: PluginFormat,
    plugin: String,
}

impl Live {
    fn serves(&self, slot: &PluginSlot) -> bool {
        self.format == slot.plugin.format && self.plugin == slot.plugin.id
    }
}

/// The control handle.
pub struct Engine {
    cfg: EngineConfig,
    cmds: Producer<EngineCmd>,
    shared: Arc<Shared>,
    targets: TargetTable,
    factory: Box<dyn SinkFactory>,
    inserts: Box<dyn InsertFactory>,
    tempo: Option<TempoTable>,
    length_frames: u64,
    loop_range: Option<(u64, u64)>,
    /// The default loop (bar 1 to the end) is on: loads re-send it.
    default_loop: bool,
    /// Latency of each target's instrument sink.
    instrument_latency: [Option<u32>; MAX_TARGETS],
    /// Insert instances in the last published graph, by ID.
    live: BTreeMap<PluginInstanceId, Live>,
    /// `(node, strip)` of the last published graph.
    strip_nodes: Vec<(usize, StripId)>,
    /// Latency of the last published graph.
    latency: u64,
    collector_stop: Arc<AtomicBool>,
    collector_paused: Arc<AtomicBool>,
    collector: Option<JoinHandle<()>>,
    pool: Option<Pool>,
}

impl std::fmt::Debug for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Engine")
            .field("cfg", &self.cfg)
            .field("targets", &self.targets)
            .finish_non_exhaustive()
    }
}

impl Engine {
    /// Creates the control handle and its renderer, starts the collector
    /// thread that frees retired schedules and graphs every 10 ms and, with
    /// `cfg.workers > 0`, the portable worker pool. Fails when a thread
    /// cannot be started (ruling O24).
    pub fn new(
        cfg: EngineConfig,
        factory: Box<dyn SinkFactory>,
    ) -> Result<(Engine, Renderer), EngineError> {
        Engine::with_platform(cfg, factory, Arc::new(StdPlatform))
    }

    /// [`Engine::new`] with platform wakers and real-time promotion for the
    /// workers (`platform-macos` passes `MacRtPlatform`).
    pub fn with_platform(
        cfg: EngineConfig,
        factory: Box<dyn SinkFactory>,
        platform: Arc<dyn RtPlatform>,
    ) -> Result<(Engine, Renderer), EngineError> {
        Engine::with_spawner(cfg, factory, platform, |b, f| b.spawn(f))
    }

    /// [`Engine::with_platform`] with the collector's thread spawn injected
    /// (tests make it fail).
    fn with_spawner(
        cfg: EngineConfig,
        factory: Box<dyn SinkFactory>,
        platform: Arc<dyn RtPlatform>,
        spawn: impl FnOnce(
            std::thread::Builder,
            Box<dyn FnOnce() + Send + 'static>,
        ) -> std::io::Result<JoinHandle<()>>,
    ) -> Result<(Engine, Renderer), EngineError> {
        let (cmd_tx, cmd_rx) = RingBuffer::<EngineCmd>::new(CMD_CAPACITY);
        let (retire_tx, mut retire_rx) = RingBuffer::<Retired>::new(RETIRE_CAPACITY);
        let shared = Arc::new(Shared::default());
        let collector_stop = Arc::new(AtomicBool::new(false));
        let collector_paused = Arc::new(AtomicBool::new(false));
        let stop = Arc::clone(&collector_stop);
        let paused = Arc::clone(&collector_paused);
        const COLLECTOR: &str = "vizdaw-collector";
        let collector = spawn(
            std::thread::Builder::new().name(COLLECTOR.into()),
            Box::new(move || {
                loop {
                    while !paused.load(Ordering::Relaxed) {
                        let Ok(old) = retire_rx.pop() else {
                            break;
                        };
                        drop(old);
                    }
                    if stop.load(Ordering::Relaxed) {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
            }),
        )
        .map_err(|e| EngineError::Thread {
            name: COLLECTOR,
            reason: e.to_string(),
        })?;
        let pool = if cfg.workers > 0 {
            Some(Pool::new(cfg.workers, platform, Arc::clone(&shared))?)
        } else {
            None
        };
        let renderer = Renderer::new(
            RenderConfig {
                sample_rate: cfg.sample_rate,
                max_frames: cfg.max_frames,
                midi_offset_ns: cfg.midi_offset_ns,
                deadline: cfg.deadline,
            },
            cmd_rx,
            retire_tx,
            Arc::clone(&shared),
            pool.as_ref().map(Pool::handle),
        );
        let engine = Engine {
            cfg,
            cmds: cmd_tx,
            shared,
            targets: TargetTable::new(),
            factory,
            inserts: Box::new(NoInserts),
            tempo: None,
            length_frames: 0,
            loop_range: None,
            default_loop: false,
            instrument_latency: [None; MAX_TARGETS],
            live: BTreeMap::new(),
            strip_nodes: Vec::new(),
            latency: 0,
            collector_stop,
            collector_paused,
            collector: Some(collector),
            pool,
        };
        Ok((engine, renderer))
    }

    /// Replaces the insert factory (default [`NoInserts`]; tests pass
    /// `TestInserts`, 3b the plugin hosts). Affects inserts created by later
    /// loads.
    pub fn set_insert_factory(&mut self, f: Box<dyn InsertFactory>) {
        self.inserts = f;
    }

    /// Hands the render workers the device's period and workgroup (Plan
    /// Ruling 22); no-op without workers.
    pub fn set_rt_env(&self, env: RtEnv) {
        if let Some(p) = &self.pool {
            p.set_env(env);
        }
    }

    /// Whether the callback thread renders graph nodes too (diagnostic
    /// hook, Plan Ruling 19).
    pub fn set_callback_helps(&self, on: bool) {
        if let Some(p) = &self.pool {
            p.set_callback_helps(on);
        }
    }

    /// Pauses or resumes freeing retired schedules and graphs (a diagnostic
    /// hook: tests use it to fill the retire ring).
    pub fn pause_collector(&self, paused: bool) {
        self.collector_paused.store(paused, Ordering::Relaxed);
    }

    /// Settings.
    pub fn config(&self) -> EngineConfig {
        self.cfg
    }

    /// Targets created so far.
    pub fn targets(&self) -> &TargetTable {
        &self.targets
    }

    /// Compiles `seq` and the mixer graph and sends them (and sinks for new
    /// targets) to the renderer. Call again after every committed edit.
    pub fn load(&mut self, doc: &Document, seq: SeqId) -> Result<CompileReport, EngineError> {
        // Room for every possible new sink plus the graph, the schedule and
        // the loop, so no push below can fail after targets were interned.
        if self.cmds.slots() < MAX_TARGETS + 3 {
            return Err(EngineError::Busy);
        }
        let opts = CompileOptions {
            sample_rate: self.cfg.sample_rate,
            route: self.cfg.route,
            ports: self.factory.ports(),
        };
        let (sr, max) = (self.cfg.sample_rate, self.cfg.max_frames);
        let before = self.targets.len();
        let mut new_sinks: Vec<(TargetIdx, Sink)> = Vec::new();
        let targets = &mut self.targets;
        let factory = &mut self.factory;
        let mut resolve = |kind: &TargetKind| -> Result<Result<TargetIdx, String>, EngineError> {
            if let Some(i) = targets.find(kind) {
                return Ok(Ok(i));
            }
            if targets.len() >= MAX_TARGETS {
                return Err(EngineError::TooManyTargets);
            }
            // `intern` appends, so the new target gets the next index.
            let next = u8::try_from(targets.len()).map_err(|_| EngineError::TooManyTargets)?;
            match factory.make(TargetIdx(next), kind, sr, max) {
                Ok(sink) => {
                    let i = targets.intern(kind.clone())?;
                    new_sinks.push((i, sink));
                    Ok(Ok(i))
                }
                Err(e) => Ok(Err(e.to_string())),
            }
        };
        let compiled = compile(doc, seq, &opts, &mut resolve);
        let (schedule, mut report) = match compiled {
            Ok(c) => c,
            Err(e) => {
                // Targets interned by this load have no sinks yet; forget
                // them (their sinks drop with `new_sinks`).
                self.targets.truncate(before);
                return Err(e);
            }
        };
        for (i, sink) in new_sinks {
            if let (Sink::Instrument(node), Some(l)) =
                (&sink, self.instrument_latency.get_mut(usize::from(i.0)))
            {
                *l = Some(node.latency());
            }
            self.push(EngineCmd::AddSink(i, sink))?;
        }
        report.graph = self.compile_graph(doc)?;
        self.tempo = Some(schedule.tempo.clone());
        self.length_frames = schedule.length_frames;
        self.loop_range = schedule.loop_range;
        self.push(EngineCmd::SetSchedule(Box::new(schedule)))?;
        if self.default_loop {
            // The length may have changed; the mapped old range would not
            // follow it.
            self.push(EngineCmd::SetLoop(self.loop_range))?;
        }
        Ok(report)
    }

    /// Builds and publishes the mixer graph. A graph that cannot be planned
    /// is reported (and its fault raised) and the previous graph keeps
    /// playing (spec §6).
    fn compile_graph(&mut self, doc: &Document) -> Result<GraphReport, EngineError> {
        let (sr, max) = (self.cfg.sample_rate, self.cfg.max_frames);
        let instruments: Vec<(TargetIdx, u32)> = self
            .targets
            .iter()
            .filter(|(_, k)| **k == TargetKind::BuiltinSynth)
            .filter_map(|(i, _)| {
                let lat = self.instrument_latency.get(usize::from(i.0)).copied().flatten()?;
                Some((i, lat))
            })
            .collect();
        let mut fresh: BTreeMap<PluginInstanceId, (Box<dyn EffectNode>, Live)> = BTreeMap::new();
        let mut adopted: BTreeMap<PluginInstanceId, Live> = BTreeMap::new();
        let mut placeholders: Vec<(PluginInstanceId, String)> = Vec::new();
        let live = &self.live;
        let inserts = &mut self.inserts;
        let mut latency = |slot: &PluginSlot| -> u32 {
            if let Some(l) = live.get(&slot.id).filter(|l| l.serves(slot)) {
                adopted.insert(slot.id, l.clone());
                return l.latency;
            }
            match inserts.make(slot, sr, max) {
                Ok(node) => {
                    let lat = node.latency();
                    let info = Live {
                        index: 0,
                        latency: lat,
                        format: slot.plugin.format,
                        plugin: slot.plugin.id.clone(),
                    };
                    fresh.insert(slot.id, (node, info));
                    lat
                }
                Err(why) => {
                    placeholders.push((slot.id, why));
                    0
                }
            }
        };
        let mg = mixer_spec(doc, &instruments, &mut latency);
        let mut report = GraphReport {
            nodes: mg.spec.nodes.len(),
            latency_frames: 0,
            placeholders,
            warnings: mg.warnings,
            error: None,
        };
        let max_delay = (2.0 * sr).round().max(0.0) as u64;
        let p = match plan(&mg.spec, max_delay) {
            Ok(p) => p,
            Err(e) => {
                self.shared.raise(match e {
                    PlanError::TooLarge { .. } => Faults::GRAPH_TOO_LARGE,
                    PlanError::LatencyTooLarge { .. } => Faults::LATENCY_TOO_LARGE,
                    PlanError::Cycle | PlanError::BadIndex { .. } => Faults::GRAPH_INVALID,
                });
                report.error = Some(e.to_string());
                return Ok(report);
            }
        };
        let mut next_live: BTreeMap<PluginInstanceId, Live> = BTreeMap::new();
        let graph = Graph::build(&mg.spec, &p, max, |idx, ins| {
            let Some(id) = ins.instance else {
                return InsertInit::Placeholder;
            };
            if let Some(l) = adopted.remove(&id) {
                let old = l.index;
                next_live.insert(id, Live { index: idx, ..l });
                return InsertInit::Adopt(old);
            }
            match fresh.remove(&id) {
                Some((node, info)) => {
                    next_live.insert(id, Live { index: idx, ..info });
                    InsertInit::Fresh(node)
                }
                None => InsertInit::Placeholder,
            }
        });
        report.latency_frames = graph.latency();
        self.push(EngineCmd::SetGraph(Box::new(graph)))?;
        self.live = next_live;
        self.latency = report.latency_frames;
        self.strip_nodes = mg.strips;
        Ok(report)
    }

    fn push(&mut self, cmd: EngineCmd) -> Result<(), EngineError> {
        self.cmds.push(cmd).map_err(|_| EngineError::Busy)
    }

    /// Starts playback (from the start when at or past the end).
    pub fn play(&mut self) -> Result<(), EngineError> {
        self.push(EngineCmd::Play)
    }

    /// Stops playback; sounding notes get note-offs.
    pub fn stop(&mut self) -> Result<(), EngineError> {
        self.push(EngineCmd::Stop)
    }

    /// Moves to a schedule frame.
    pub fn locate_frame(&mut self, frame: u64) -> Result<(), EngineError> {
        self.push(EngineCmd::Locate(frame))
    }

    /// Moves to a tick of the loaded sequence.
    pub fn locate_tick(&mut self, tick: Tick) -> Result<(), EngineError> {
        let frame = self.tempo.as_ref().map_or(0, |t| t.tick_to_frame(tick));
        self.locate_frame(frame)
    }

    /// Loops bar 1 to the end of the loaded sequence, or stops looping.
    pub fn set_loop(&mut self, on: bool) -> Result<(), EngineError> {
        let range = if on { self.loop_range } else { None };
        self.push(EngineCmd::SetLoop(range))?;
        self.default_loop = on;
        Ok(())
    }

    /// Loops an explicit frame range, or stops looping.
    pub fn set_loop_range(&mut self, range: Option<(u64, u64)>) -> Result<(), EngineError> {
        self.push(EngineCmd::SetLoop(range))?;
        self.default_loop = false;
        Ok(())
    }

    /// Frame of the transport as heard (schedule frame minus the graph
    /// latency).
    pub fn position_frames(&self) -> u64 {
        self.shared.position()
    }

    /// Tick of the transport as heard, through the last loaded schedule's
    /// tempo table.
    pub fn position_tick(&self) -> Tick {
        let f = self.shared.position();
        self.tempo
            .as_ref()
            .map_or(Tick::ZERO, |t| t.frame_to_tick(f))
    }

    /// Frame of the loaded sequence's end.
    pub fn length_frames(&self) -> u64 {
        self.length_frames
    }

    /// Latency `L` of the last published graph, in frames.
    pub fn latency_frames(&self) -> u64 {
        self.latency
    }

    /// Transport state.
    pub fn state(&self) -> PlayState {
        self.shared.state()
    }

    /// Reads and clears the fault bits.
    pub fn faults(&self) -> Faults {
        self.shared.take_faults()
    }

    /// Current counters.
    pub fn counters(&self) -> Counters {
        self.shared.counters()
    }

    /// True once after playback reached the end and the tail finished.
    pub fn take_ended(&self) -> bool {
        self.shared.take_ended()
    }

    /// Peak meters of every strip of the last published graph since the
    /// last call (read and cleared).
    pub fn meters(&self) -> Vec<StripMeter> {
        self.strip_nodes
            .iter()
            .filter_map(|(node, strip)| {
                let (l, r) = self.shared.meter(*node)?.take();
                Some(StripMeter {
                    strip: *strip,
                    peak_l: l,
                    peak_r: r,
                })
            })
            .collect()
    }

    /// DSP load of the last block and the peak since the last call.
    pub fn dsp_load(&self) -> DspLoad {
        self.shared.take_load()
    }

    /// Instances bypassed for faults since the last call (read and
    /// cleared).
    pub fn plugin_faults(&self) -> Vec<PluginInstanceId> {
        self.shared.take_plugin_faults()
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.collector_stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.collector.take() {
            let _ = h.join();
        }
    }
}

/// Finds a sequence by name.
pub fn find_sequence(doc: &Document, name: &str) -> Result<SeqId, EngineError> {
    doc.sequences()
        .values()
        .find(|s| s.name == name)
        .map(|s| s.id)
        .ok_or_else(|| EngineError::UnknownSequence(name.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::offline::OfflineSinks;

    #[test]
    fn new_fails_when_the_collector_thread_cannot_start() {
        let r = Engine::with_spawner(
            EngineConfig::new(48_000.0),
            Box::new(OfflineSinks::new(4)),
            Arc::new(StdPlatform),
            |_, _| Err(std::io::Error::other("no threads")),
        );
        match r {
            Err(EngineError::Thread { name, .. }) => assert_eq!(name, "vizdaw-collector"),
            other => panic!("expected a thread error, got {other:?}"),
        }
    }

    #[test]
    fn new_succeeds_normally() {
        let (engine, _r) =
            Engine::new(EngineConfig::new(48_000.0), Box::new(OfflineSinks::new(4))).unwrap();
        assert!(engine.targets().is_empty());
    }
}
```

- [ ] **Step 6: Tone sinks and block output for offline runs**

In `crates/core-engine/src/offline.rs`:

Add, before `pub struct RecordingNode`:

```rust
/// A deterministic, sample-accurate test voice (Plan Ruling 27): every
/// held note adds a quiet sawtooth whose period depends on its pitch;
/// events apply at their frame offsets.
struct Tone {
    on: [u128; 16],
    events: [(u32, [u8; 3]); 64],
    len: usize,
    t: u64,
}

impl Tone {
    fn new() -> Tone {
        Tone {
            on: [0; 16],
            events: [(0, [0; 3]); 64],
            len: 0,
            t: 0,
        }
    }

    /// Queues a message for the next render (at most 64 per block). // RT
    fn queue(&mut self, offset: u32, msg: &[u8]) {
        let (Some(&s), Some(slot)) = (msg.first(), self.events.get_mut(self.len)) else {
            return;
        };
        *slot = (
            offset,
            [s, msg.get(1).copied().unwrap_or(0), msg.get(2).copied().unwrap_or(0)],
        );
        self.len += 1;
    }

    fn apply(&mut self, [s, d1, d2]: [u8; 3]) {
        let Some(mask) = self.on.get_mut(usize::from(s & 0x0F)) else {
            return;
        };
        let bit = 1u128 << (d1 & 0x7F);
        match s & 0xF0 {
            0x90 if d2 > 0 => *mask |= bit,
            0x80 | 0x90 => *mask &= !bit,
            0xB0 if d1 == 123 => *mask = 0,
            _ => {}
        }
    }

    /// Renders `frames` frames, applying queued events at their offsets. // RT
    fn render(&mut self, frames: u32, out: &mut StereoBuf) {
        let n = (frames as usize).min(out.left.len()).min(out.right.len());
        let mut next = 0;
        for f in 0..n {
            while next < self.len {
                let Some(&(off, m)) = self.events.get(next) else {
                    break;
                };
                if off as usize > f {
                    break;
                }
                self.apply(m);
                next += 1;
            }
            let mut v = 0.0f32;
            for mask in self.on {
                let mut m = mask;
                while m != 0 {
                    let p = u64::from(m.trailing_zeros());
                    m &= m - 1;
                    v += ((self.t.wrapping_mul(p + 1) % 512) as f32 / 512.0 - 0.5) * 0.02;
                }
            }
            if let (Some(l), Some(r)) = (out.left.get_mut(f), out.right.get_mut(f)) {
                *l = v;
                *r = v;
            }
            self.t += 1;
        }
        self.flush(next);
    }

    /// Applies the queued events from index `from` on and empties the
    /// queue. // RT
    fn flush(&mut self, from: usize) {
        for i in from..self.len {
            if let Some(&(_, m)) = self.events.get(i) {
                self.apply(m);
            }
        }
        self.len = 0;
    }

    /// A block passed unrendered. // RT
    fn skip(&mut self, frames: u32) {
        self.flush(0);
        self.t += u64::from(frames);
    }
}
```

Add a field `tone: Option<Box<Tone>>,` to `RecordingNode` and replace its `impl InstrumentNode` with:

```rust
impl InstrumentNode for RecordingNode {
    fn midi(&mut self, offset: u32, msg: &[u8]) {
        let r = LogRecord::new(self.rendered + u64::from(offset), self.target, msg);
        if self.tx.push(r).is_err() {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
        if let Some(t) = self.tone.as_mut() {
            t.queue(offset, msg);
        }
    }

    fn render(&mut self, frames: u32, out: &mut StereoBuf) {
        match self.tone.as_mut() {
            Some(t) => t.render(frames, out),
            None => out.clear(frames),
        }
        self.rendered += u64::from(frames);
    }

    fn reset(&mut self) {}

    fn skip(&mut self, frames: u32) {
        if let Some(t) = self.tone.as_mut() {
            t.skip(frames);
        }
        self.rendered += u64::from(frames);
    }
}
```

Add a field `tone: bool,` to `OfflineSinks` (initialised `false` in `new`), the builder method

```rust
    /// Built-in synth sinks also render a deterministic test tone (Plan
    /// Ruling 27).
    pub fn with_tone(mut self) -> OfflineSinks {
        self.tone = true;
        self
    }
```

and set `tone: self.tone.then(|| Box::new(Tone::new())),` in the `RecordingNode { … }` literal of `make`. Add to `impl OfflineBackend`:

```rust
    /// The last block's output: `frames` frames of the left and right
    /// channels.
    pub fn output(&self, frames: u32) -> (&[f32], &[f32]) {
        let n = frames as usize;
        (
            self.left.get(..n).unwrap_or(&[]),
            self.right.get(..n).unwrap_or(&[]),
        )
    }
```

- [ ] **Step 7: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-engine --test mixer_engine
cargo test -p core-engine --release --test mixer_engine audio_is_identical
cargo test --workspace
```

Expected: `mixer_engine` 10 passed (debug and the release determinism run); the workspace passes — in particular `offline::demo_plays_to_the_end` keeps `lines 335 hash 1feb33c1bfd60916`, `rt_alloc` stays at zero allocations, `vizdaw-play`'s `offline_render_of_the_demo_matches_the_checked_in_log` still matches `fixtures/demo.offline.txt` (Plan Ruling 28), and `core-ffi`'s tests pass unchanged.

- [ ] **Step 8: Lint and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p vizdaw-play --all-targets --features rt-check -- -D warnings
git add crates/core-engine/src crates/core-engine/tests/engine.rs crates/core-engine/tests/mixer_engine.rs
git commit -m "feat(core-engine): render through the mixer graph with PDC, workers, meters and faults

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---
### Task 12: Zero allocations on the callback and worker threads

**Goal:** Prove spec §7's "zero allocations on every thread": with the counting allocator installed and workers armed, playing a mixer with inserts, sends, a sidechain, a faulting insert, parameter and structural graph swaps (including deferred ones), a missed deadline and busy blocks allocates nothing on the callback thread or on any worker.

**Files:**
- Test: `crates/core-engine/tests/rt_alloc_graph.rs`

**Interfaces:**
- Consumes: `rt_check::{CountingAlloc, arm, disarm, count, set_arm_workers}` (Task 9), `Engine::{set_insert_factory, set_callback_helps, pause_collector, counters, plugin_faults}`, `EngineConfig.{workers, deadline}`, `TestInserts::{new, with_stall, plugin}`, `Builder::{from_doc, strip_of, bus, insert, mixer}`, `OfflineSinks::with_tone`.
- Produces: nothing new (a test binary of its own: the counter is global, so this file holds one test, like `tests/rt_alloc.rs`, engine ruling 26).

- [ ] **Step 1: Write the test**

Create `crates/core-engine/tests/rt_alloc_graph.rs`:

```rust
//! Counts allocations on the render path with the mixer graph and worker
//! threads: the callback thread is armed around `OfflineBackend::block`,
//! and every worker arms itself around each job (`set_arm_workers`).

use core_engine::backend::AudioBackend;
use core_engine::engine::{Engine, EngineConfig, find_sequence};
use core_engine::fixtures::{Builder, demo_document};
use core_engine::graph::TestInserts;
use core_engine::offline::{OfflineBackend, OfflineSinks};
use core_engine::render::Faults;
use core_engine::rt_check::{self, CountingAlloc};
use core_engine::schedule::BuiltinRoute;
use core_model::command::MixerCmd;
use core_model::document::Document;
use core_model::ids::{SeqId, StripId};
use core_model::mixer::{AuxSend, StripKind};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

#[global_allocator]
static ALLOC: CountingAlloc = CountingAlloc;

fn armed_block(b: &mut OfflineBackend, frames: u32) {
    rt_check::arm();
    b.block(frames);
    rt_check::disarm();
}

/// The demo with: a gain/latency insert and a NaN insert (it faults after
/// 16 blocks) on the synth, a post send to "Verb" (noise, latency 200) whose
/// second insert adds the synth's sidechain, and a pre-fader send to "Dry".
fn mixed(master_db: f64, extra_bus: bool) -> (Document, SeqId) {
    let doc = demo_document().unwrap();
    let song = find_sequence(&doc, "Song").unwrap();
    let mut b = Builder::from_doc(doc);
    let synth = b.strip_of(StripKind::BuiltinSynth).unwrap();
    let verb = b.bus("Verb").unwrap();
    let dry = b.bus("Dry").unwrap();
    b.insert(synth, TestInserts::plugin("gain=0.7,latency=33")).unwrap();
    b.insert(synth, TestInserts::plugin("nan")).unwrap();
    b.insert(verb, TestInserts::plugin("noise=3,latency=200")).unwrap();
    let sc = b.insert(verb, TestInserts::plugin("sc")).unwrap();
    b.mixer(MixerCmd::SetSidechain { slot: sc, source: Some(synth) }).unwrap();
    b.mixer(MixerCmd::AddSend { strip: synth, send: AuxSend::new(verb) }).unwrap();
    let mut pre = AuxSend::new(dry);
    pre.pre_fader = true;
    b.mixer(MixerCmd::AddSend { strip: synth, send: pre }).unwrap();
    b.mixer(MixerCmd::SetGain { strip: StripId(0), gain_db: master_db }).unwrap();
    if extra_bus {
        b.bus("Extra").unwrap();
    }
    (b.finish(), song)
}

/// One test: the allocation counter is global.
#[test]
fn graph_render_path_never_allocates() {
    rt_check::set_arm_workers(true);
    // The counter works on this thread.
    let before = rt_check::count();
    rt_check::arm();
    drop(Vec::<u8>::with_capacity(10));
    rt_check::disarm();
    assert_eq!(rt_check::count() - before, 2);

    let (doc, song) = mixed(0.0, false);
    let (quieter, _) = mixed(-3.0, false);
    let (wider, _) = mixed(-3.0, true);

    // Phase 1: three workers, both routes, 3000 blocks of varying size,
    // with a parameter swap every 300 blocks and a structural swap every
    // 700 (its delay lines are new; its instances are adopted).
    let mut cfg = EngineConfig::new(48_000.0);
    cfg.workers = 3;
    cfg.route = BuiltinRoute::Both;
    let sinks = OfflineSinks::new(1 << 20).with_tone();
    let log = sinks.log();
    let (mut engine, renderer) = Engine::new(cfg, Box::new(sinks)).unwrap();
    engine.set_insert_factory(Box::new(TestInserts::new()));
    engine.load(&doc, song).unwrap();
    let mut b = OfflineBackend::new(48_000.0, 512);
    b.start(renderer).unwrap();
    b.reserve(4096);
    engine.set_loop(true).unwrap();
    engine.play().unwrap();
    let before = rt_check::count();
    for i in 0..3000u32 {
        if i % 300 == 150 {
            engine.load(if i % 600 == 150 { &quieter } else { &doc }, song).unwrap();
        }
        if i % 700 == 350 {
            engine.load(&wider, song).unwrap();
        }
        armed_block(&mut b, 1 + (i * 37) % 1024);
        if i % 100 == 0 {
            log.drain();
        }
    }
    assert_eq!(rt_check::count() - before, 0, "allocations while playing the graph");
    assert!(engine.faults().contains(Faults::PLUGIN_FAULT));
    assert_eq!(engine.plugin_faults().len(), 1);

    // Phase 2: graph swaps with the retire ring full (deferred swaps).
    // Each load queues three commands and each applied swap retires two
    // items, so the 16-slot retire ring is full after eight blocks;
    // resuming at block 10 keeps the 64-slot command ring from refusing a
    // load, and stopping at 20 keeps the loop from outrunning the collector
    // (it frees every 10 ms; an optimised loop makes 20 edits in far less).
    engine.pause_collector(true);
    for i in 0..20 {
        if i == 10 {
            engine.pause_collector(false);
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        engine.load(if i % 2 == 0 { &quieter } else { &doc }, song).unwrap();
        armed_block(&mut b, 256);
    }
    assert!(engine.counters().swaps_deferred >= 1);
    assert_eq!(rt_check::count() - before, 0, "allocations during graph swaps");
    log.drain();
    drop(b.stop().unwrap());
    drop(engine);

    // Phase 3: a stalled insert on a worker misses the deadline; the next
    // blocks find it busy; then it recovers.
    let flag = Arc::new(AtomicBool::new(false));
    let (stalled, seq) = {
        let mut b = Builder::from_doc(demo_document().unwrap());
        let synth = b.strip_of(StripKind::BuiltinSynth).unwrap();
        b.insert(synth, TestInserts::plugin("stall")).unwrap();
        let song = find_sequence(b.doc(), "Song").unwrap();
        (b.finish(), song)
    };
    let mut cfg = EngineConfig::new(48_000.0);
    cfg.workers = 1;
    cfg.deadline = Some(0.5);
    let (mut engine, renderer) = Engine::new(cfg, Box::new(OfflineSinks::new(1 << 16))).unwrap();
    engine.set_insert_factory(Box::new(TestInserts::with_stall(Arc::clone(&flag))));
    engine.set_callback_helps(false);
    engine.load(&stalled, seq).unwrap();
    let mut b = OfflineBackend::new(48_000.0, 512);
    b.start(renderer).unwrap();
    engine.play().unwrap();
    let before = rt_check::count();
    for _ in 0..3 {
        armed_block(&mut b, 512);
    }
    flag.store(true, Ordering::Release);
    std::thread::sleep(std::time::Duration::from_millis(50));
    for _ in 0..10 {
        armed_block(&mut b, 512);
    }
    let c = engine.counters();
    assert!(c.deadline_misses >= 1 && c.busy_blocks >= 1, "{c:?}");
    assert_eq!(rt_check::count() - before, 0, "allocations on the deadline path");
    drop(b.stop().unwrap());
    drop(engine);
    rt_check::set_arm_workers(false);
}
```

- [ ] **Step 2: Run it**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-engine --test rt_alloc_graph
cargo test -p core-engine --release --test rt_alloc_graph
cargo test -p core-engine --test rt_alloc
```

Expected: 1 passed in debug and release; the 2b `rt_alloc` test still passes. If the count is not zero, find the allocation by running the test under a debugger with a breakpoint in `rt_check::note` when `ARMED` is set (`rust-lldb target/debug/deps/rt_alloc_graph-* -- graph_render_path_never_allocates`, then `breakpoint set -n core_engine::rt_check::note`), and fix the code path; never loosen the assertion. Likely culprits are a `Vec` built inside `render_node`, an `Arc` clone in `Exec::run`, or a `format!` on a fault path.

- [ ] **Step 3: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
git add crates/core-engine/tests/rt_alloc_graph.rs
git commit -m "test(core-engine): zero allocations on callback and worker threads with the graph

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---
### Task 13: macOS real-time seams — Mach semaphores, time-constraint threads, the device workgroup

**Goal:** Implement the platform seams of spec §7 on macOS: an `RtWaker` on a Mach semaphore, `THREAD_TIME_CONSTRAINT_POLICY` promotion for worker threads, the HAL device's `os_workgroup` as an `RtWorkgroup`, the performance-core count for the default worker count, and prove that the demo plays through the DLS synth's strip into master with workers.

**Files:**
- Create: `crates/platform-macos/src/rt.rs`
- Modify: `crates/platform-macos/src/lib.rs`
- Modify: `crates/platform-macos/src/hal.rs` (`CoreAudioBackend::workgroup`)
- Modify: `crates/platform-macos/Cargo.toml` (dev-dependency `core-model`)
- Test: `crates/platform-macos/tests/rt.rs`, `crates/platform-macos/tests/mixer.rs`

**Interfaces:**
- Consumes: `core_engine::pool::{RtPlatform, RtWaker, RtWorkgroup, WorkgroupToken, StdPlatform, RtEnv}`, `core_engine::graph::exec::{Graph, InsertInit, Runner}`, `Engine::with_platform`, `MacSinks`, `objc2_core_audio::kAudioDevicePropertyIOThreadOSWorkgroup` (already in `objc2-core-audio` 0.3.2 with the `AudioHardware` feature).
- Produces:
  - `platform_macos::rt::MachWaker { new() -> Result<MachWaker, String> }` implementing `RtWaker`.
  - `platform_macos::rt::promote_current_thread(period_ns: u64) -> Result<(), String>`.
  - `platform_macos::rt::OsWorkgroup` implementing `RtWorkgroup`; `unsafe fn from_owned(*mut c_void) -> Option<OsWorkgroup>`.
  - `platform_macos::rt::MacRtPlatform` implementing `RtPlatform` (Mach waker, falling back to `StdPlatform`'s; promotion).
  - `platform_macos::rt::{performance_cores() -> usize, default_workers() -> usize}` (`min(performance cores − 1, 7)`).
  - `CoreAudioBackend::workgroup(&self) -> Option<Arc<dyn RtWorkgroup>>` (the open device's IO-thread workgroup).

ABI facts used below, checked against the Command Line Tools SDK (`$(xcrun --show-sdk-path)/usr/include`): `os/workgroup_base.h` — on LP64 `__OS_WORKGROUP_JOIN_TOKEN_SIZE__` is 36 and `struct os_workgroup_join_token_opaque_s { uint32_t sig; char opaque[36]; }`; `os/workgroup_object.h` — `int os_workgroup_join(os_workgroup_t, os_workgroup_join_token_t)`, `void os_workgroup_leave(os_workgroup_t, os_workgroup_join_token_t)`, both real-time safe, leave on the joining thread; `mach/thread_policy.h` — `THREAD_TIME_CONSTRAINT_POLICY` = 2, the struct is `{ uint32_t period, computation, constraint; boolean_t preemptible }` (count 4); `mach/semaphore.h` — `semaphore_signal`/`semaphore_wait`; `CoreAudio/AudioHardware.h` — `kAudioDevicePropertyIOThreadOSWorkgroup` returns an `os_workgroup_t` "the caller is responsible for releasing". Re-check these with `grep -n "JOIN_TOKEN_SIZE\|THREAD_TIME_CONSTRAINT_POLICY " $(xcrun --show-sdk-path)/usr/include/os/workgroup_base.h $(xcrun --show-sdk-path)/usr/include/mach/thread_policy.h` before writing the module.

- [ ] **Step 1: Write the failing tests**

In `crates/platform-macos/Cargo.toml`, under `[target.'cfg(target_os = "macos")'.dev-dependencies]`, add `core-model = { path = "../core-model" }`.

Create `crates/platform-macos/tests/rt.rs`:

```rust
#![cfg(target_os = "macos")]

use core_engine::backend::{AudioBackend, AudioConfig};
use core_engine::graph::exec::{Graph, InsertInit, Runner};
use core_engine::graph::{GraphSpec, InputSpec, InsertSpec, NodeSpec, StripSpec, Tap, TestMode, TestNode, plan};
use core_engine::pool::{RtEnv, RtPlatform, RtWaker};
use platform_macos::hal::CoreAudioBackend;
use platform_macos::rt::{MacRtPlatform, MachWaker, default_workers, performance_cores, promote_current_thread};
use std::sync::Arc;
use std::sync::mpsc;
use std::time::Duration;

#[test]
fn mach_waker_wakes_a_waiting_thread() {
    let w = Arc::new(MachWaker::new().unwrap());
    let (tx, rx) = mpsc::channel();
    let w2 = Arc::clone(&w);
    let t = std::thread::spawn(move || {
        w2.wait();
        tx.send(()).unwrap();
    });
    std::thread::sleep(Duration::from_millis(20));
    assert!(rx.try_recv().is_err(), "still waiting");
    w.wake(1);
    rx.recv_timeout(Duration::from_secs(1)).unwrap();
    t.join().unwrap();
    // A wake with nobody waiting is kept for the next wait.
    w.wake(1);
    w.wait();
}

#[test]
fn worker_threads_can_be_promoted() {
    let r = std::thread::spawn(|| promote_current_thread(10_666_667)).join().unwrap();
    assert_eq!(r, Ok(()));
    let r = std::thread::spawn(|| MacRtPlatform.promote(2_666_667)).join().unwrap();
    assert_eq!(r, Ok(()));
}

#[test]
fn core_counts() {
    assert!(performance_cores() >= 1);
    assert_eq!(default_workers(), performance_cores().saturating_sub(1).min(7));
}

fn noise_graph() -> Graph {
    let mut spec = GraphSpec { nodes: Vec::new(), master: 4 };
    for i in 0..4 {
        let mut s = StripSpec::new(None);
        s.inserts.push(InsertSpec { instance: None, latency: 10 * i, bypass: false, sidechain: None });
        spec.nodes.push(NodeSpec::Strip(s));
    }
    let mut m = StripSpec::new(None);
    m.inputs = (0..4).map(|from| InputSpec { from, tap: Tap::Post, gain: 0.5 }).collect();
    spec.nodes.push(NodeSpec::Strip(m));
    let p = plan(&spec, 96_000).unwrap();
    Graph::build(&spec, &p, 512, |idx, ins| {
        InsertInit::Fresh(Box::new(TestNode::new(
            TestMode::Noise { seed: idx as u32 + 1, level: 0.25 },
            ins.latency,
        )))
    })
}

#[test]
fn mac_platform_workers_render_like_serial() {
    let (want, _) = Runner::serial(noise_graph()).render(48_000, &[512]);
    let mut r = Runner::parallel(noise_graph(), 4, Arc::new(MacRtPlatform)).unwrap();
    r.set_env(RtEnv { period_ns: 10_666_667, workgroup: None });
    let (got, _) = r.render(48_000, &[256, 512, 64]);
    assert_eq!(
        got.iter().map(|x| x.to_bits()).collect::<Vec<_>>(),
        want.iter().map(|x| x.to_bits()).collect::<Vec<_>>()
    );
    assert!(r.shared().take_faults().is_empty());
}

#[test]
#[ignore = "needs an audio output device"]
fn the_default_device_has_a_workgroup_workers_can_join() {
    let mut backend = CoreAudioBackend::new();
    backend.open(&AudioConfig::default()).unwrap();
    let wg = backend.workgroup().expect("macOS 11+ devices expose an IO workgroup");
    let t = std::thread::spawn(move || {
        let token = wg.join().unwrap();
        wg.leave(token);
    });
    t.join().unwrap();
}
```

Create `crates/platform-macos/tests/mixer.rs`:

```rust
#![cfg(target_os = "macos")]

use core_engine::backend::AudioBackend;
use core_engine::engine::{Engine, EngineConfig, find_sequence};
use core_engine::fixtures::{Builder, demo_document};
use core_engine::offline::OfflineBackend;
use core_model::command::MixerCmd;
use core_model::ids::StripId;
use core_model::mixer::StripKind;
use platform_macos::rt::MacRtPlatform;
use platform_macos::sinks::MacSinks;
use std::sync::Arc;

fn rms_dbfs(x: &[f32]) -> f64 {
    let sum: f64 = x.iter().map(|v| f64::from(*v) * f64::from(*v)).sum();
    20.0 * (sum / x.len().max(1) as f64).sqrt().max(1e-12).log10()
}

#[test]
fn the_demo_plays_through_the_builtin_strip_into_master() {
    let doc = demo_document().unwrap();
    let song = find_sequence(&doc, "Song").unwrap();
    let mut cfg = EngineConfig::new(48_000.0);
    cfg.workers = 2;
    let (mut engine, r) =
        Engine::with_platform(cfg, Box::new(MacSinks::new()), Arc::new(MacRtPlatform)).unwrap();
    let report = engine.load(&doc, song).unwrap();
    assert!(report.graph.error.is_none() && report.graph.warnings.is_empty());
    let mut b = OfflineBackend::new(48_000.0, 512);
    b.start(r).unwrap();
    engine.play().unwrap();
    let mut out = Vec::new();
    for _ in 0..(2 * 48_000 / 512) {
        b.block(512);
        out.extend_from_slice(b.output(512).0);
    }
    let level = rms_dbfs(&out);
    assert!(level > -40.0, "the demo is too quiet through the mixer: {level:.1} dBFS");
    let meters = engine.meters();
    let synth = meters.iter().find(|m| m.strip == StripId(1)).unwrap();
    assert!(synth.peak_l > 0.0, "{meters:?}");

    // Muting the built-in synth's strip silences master from the next block.
    let mut bld = Builder::from_doc(doc);
    let strip = bld.strip_of(StripKind::BuiltinSynth).unwrap();
    bld.mixer(MixerCmd::SetMute { strip, mute: true }).unwrap();
    engine.load(bld.doc(), song).unwrap();
    b.block(512);
    assert!(b.output(512).0.iter().all(|x| *x == 0.0));
    assert!(!engine.faults().contains(core_engine::render::Faults::XRUN));
}
```

- [ ] **Step 2: Run them to verify they fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p platform-macos --test rt --test mixer
```

Expected: compile error — `could not find rt in platform_macos`.

- [ ] **Step 3: Write the module**

Create `crates/platform-macos/src/rt.rs`:

```rust
//! Real-time thread support on macOS (spec §7): a Mach-semaphore waker,
//! `THREAD_TIME_CONSTRAINT_POLICY` promotion, the device's `os_workgroup`,
//! and the performance-core count. All `unsafe` here calls libSystem.

use core_engine::pool::{RtPlatform, RtWaker, RtWorkgroup, StdPlatform, WorkgroupToken};
use std::ffi::{CStr, c_char, c_void};

type KernReturn = i32;
type MachPort = u32;

#[repr(C)]
struct TimebaseInfo {
    numer: u32,
    denom: u32,
}

/// `struct thread_time_constraint_policy` (`mach/thread_policy.h`).
#[repr(C)]
struct TimeConstraintPolicy {
    period: u32,
    computation: u32,
    constraint: u32,
    preemptible: i32,
}

const THREAD_TIME_CONSTRAINT_POLICY: u32 = 2;
const THREAD_TIME_CONSTRAINT_POLICY_COUNT: u32 = 4;
const SYNC_POLICY_FIFO: i32 = 0;
/// `__OS_WORKGROUP_JOIN_TOKEN_SIZE__` on LP64 (`os/workgroup_base.h`).
const JOIN_TOKEN_SIZE: usize = 36;

/// `struct os_workgroup_join_token_opaque_s`.
#[repr(C)]
struct JoinToken {
    sig: u32,
    opaque: [u8; JOIN_TOKEN_SIZE],
}

unsafe extern "C" {
    static mach_task_self_: MachPort;
    fn semaphore_create(task: MachPort, semaphore: *mut MachPort, policy: i32, value: i32) -> KernReturn;
    fn semaphore_destroy(task: MachPort, semaphore: MachPort) -> KernReturn;
    fn semaphore_signal(semaphore: MachPort) -> KernReturn;
    fn semaphore_wait(semaphore: MachPort) -> KernReturn;
    fn mach_timebase_info(info: *mut TimebaseInfo) -> KernReturn;
    fn pthread_self() -> *mut c_void;
    fn pthread_mach_thread_np(thread: *mut c_void) -> MachPort;
    fn thread_policy_set(thread: MachPort, flavor: u32, policy: *mut i32, count: u32) -> KernReturn;
    fn os_workgroup_join(wg: *mut c_void, token: *mut JoinToken) -> i32;
    fn os_workgroup_leave(wg: *mut c_void, token: *mut JoinToken);
    fn os_release(object: *mut c_void);
    fn sysctlbyname(
        name: *const c_char,
        oldp: *mut c_void,
        oldlenp: *mut usize,
        newp: *mut c_void,
        newlen: usize,
    ) -> i32;
}

/// An `RtWaker` on one Mach semaphore (one per worker).
#[derive(Debug)]
pub struct MachWaker {
    sem: MachPort,
}

impl MachWaker {
    /// A semaphore with count 0.
    pub fn new() -> Result<MachWaker, String> {
        let mut sem: MachPort = 0;
        // SAFETY: `sem` is a valid out-pointer; `mach_task_self_` is this
        // task's port, set up by libSystem before `main`.
        let kr = unsafe { semaphore_create(mach_task_self_, &mut sem, SYNC_POLICY_FIFO, 0) };
        if kr == 0 {
            Ok(MachWaker { sem })
        } else {
            Err(format!("semaphore_create failed ({kr})"))
        }
    }
}

impl RtWaker for MachWaker {
    fn wake(&self, n: u32) {
        for _ in 0..n {
            // SAFETY: `sem` is a live semaphore owned by `self`;
            // `semaphore_signal` is a real-time-safe Mach trap.
            unsafe {
                semaphore_signal(self.sem);
            }
        }
    }

    fn wait(&self) {
        // SAFETY: as above; an interrupted wait returns early, which the
        // waker contract allows.
        unsafe {
            semaphore_wait(self.sem);
        }
    }
}

impl Drop for MachWaker {
    fn drop(&mut self) {
        // SAFETY: the semaphore was created by `new` on this task and is
        // destroyed once.
        unsafe {
            semaphore_destroy(mach_task_self_, self.sem);
        }
    }
}

/// Gives the calling thread `THREAD_TIME_CONSTRAINT_POLICY` for blocks of
/// `period_ns`: period and constraint one block, computation half a block
/// (clamped to 50 µs … 50 ms), preemptible.
pub fn promote_current_thread(period_ns: u64) -> Result<(), String> {
    let mut tb = TimebaseInfo { numer: 0, denom: 0 };
    // SAFETY: `tb` is a valid out-pointer.
    unsafe {
        mach_timebase_info(&mut tb);
    }
    if tb.numer == 0 || tb.denom == 0 {
        return Err("mach_timebase_info failed".into());
    }
    let to_abs = |ns: u64| -> u32 {
        u32::try_from(u128::from(ns) * u128::from(tb.denom) / u128::from(tb.numer)).unwrap_or(u32::MAX)
    };
    let period = to_abs(period_ns);
    let mut policy = TimeConstraintPolicy {
        period,
        computation: to_abs((period_ns / 2).clamp(50_000, 50_000_000)).min(period),
        constraint: period,
        preemptible: 1,
    };
    // SAFETY: `policy` is a `thread_time_constraint_policy` of exactly
    // `THREAD_TIME_CONSTRAINT_POLICY_COUNT` integers; the port names the
    // calling thread (`pthread_mach_thread_np` needs no deallocation).
    let kr = unsafe {
        thread_policy_set(
            pthread_mach_thread_np(pthread_self()),
            THREAD_TIME_CONSTRAINT_POLICY,
            (&mut policy as *mut TimeConstraintPolicy).cast::<i32>(),
            THREAD_TIME_CONSTRAINT_POLICY_COUNT,
        )
    };
    if kr == 0 {
        Ok(())
    } else {
        Err(format!("thread_policy_set failed ({kr})"))
    }
}

/// An audio device's IO-thread workgroup (one retained reference).
#[derive(Debug)]
pub struct OsWorkgroup {
    wg: *mut c_void,
}

// SAFETY: `os_workgroup_t` objects are reference counted and thread safe;
// join/leave may be called from any thread (each on its own token).
unsafe impl Send for OsWorkgroup {}
// SAFETY: as above.
unsafe impl Sync for OsWorkgroup {}

impl OsWorkgroup {
    /// Takes ownership of one reference to `wg` (released on drop).
    ///
    /// # Safety
    /// `wg` is null or an `os_workgroup_t` the caller owns one reference to.
    pub unsafe fn from_owned(wg: *mut c_void) -> Option<OsWorkgroup> {
        (!wg.is_null()).then_some(OsWorkgroup { wg })
    }
}

impl RtWorkgroup for OsWorkgroup {
    fn join(&self) -> Result<WorkgroupToken, String> {
        let mut token = Box::new(JoinToken {
            sig: 0,
            opaque: [0; JOIN_TOKEN_SIZE],
        });
        // SAFETY: `wg` is alive (we hold a reference) and `token` is a
        // writable, correctly sized join token.
        let rc = unsafe { os_workgroup_join(self.wg, &mut *token) };
        if rc == 0 {
            Ok(WorkgroupToken(token))
        } else {
            Err(format!("os_workgroup_join failed ({rc})"))
        }
    }

    fn leave(&self, token: WorkgroupToken) {
        if let Ok(mut t) = token.0.downcast::<JoinToken>() {
            // SAFETY: `t` was filled by `os_workgroup_join` on this
            // workgroup; the pool leaves on the thread that joined.
            unsafe { os_workgroup_leave(self.wg, &mut *t) };
        }
    }
}

impl Drop for OsWorkgroup {
    fn drop(&mut self) {
        // SAFETY: releases the one reference taken in `from_owned`.
        unsafe { os_release(self.wg) };
    }
}

/// The macOS platform for render workers.
#[derive(Debug, Clone, Copy, Default)]
pub struct MacRtPlatform;

impl RtPlatform for MacRtPlatform {
    fn waker(&self) -> Box<dyn RtWaker> {
        match MachWaker::new() {
            Ok(w) => Box::new(w),
            Err(_) => StdPlatform.waker(),
        }
    }

    fn promote(&self, period_ns: u64) -> Result<(), String> {
        promote_current_thread(period_ns)
    }
}

fn sysctl_i32(name: &CStr) -> Option<i32> {
    let mut v: i32 = 0;
    let mut len = size_of::<i32>();
    // SAFETY: `v` is a writable i32 of `len` bytes; `name` is
    // NUL-terminated; no new value is set.
    let rc = unsafe {
        sysctlbyname(
            name.as_ptr(),
            (&mut v as *mut i32).cast::<c_void>(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    (rc == 0 && len == size_of::<i32>()).then_some(v)
}

/// Performance cores (`hw.perflevel0.physicalcpu`, else `hw.physicalcpu`;
/// at least 1).
pub fn performance_cores() -> usize {
    [c"hw.perflevel0.physicalcpu", c"hw.physicalcpu"]
        .into_iter()
        .find_map(|n| sysctl_i32(n).filter(|v| *v > 0))
        .and_then(|v| usize::try_from(v).ok())
        .unwrap_or(1)
}

/// The spec's default worker count: `min(performance cores − 1, 7)`.
pub fn default_workers() -> usize {
    performance_cores().saturating_sub(1).min(7)
}
```

In `crates/platform-macos/src/lib.rs` add `pub mod rt;` after `pub mod midi;` and change the doc line about `unsafe` to "All `unsafe` lives in `hal.rs`, `dls.rs`, `midi.rs` and `rt.rs`, with a `// SAFETY:` comment per block."

- [ ] **Step 4: The device workgroup**

In `crates/platform-macos/src/hal.rs` add `kAudioDevicePropertyIOThreadOSWorkgroup` to the `objc2_core_audio` import list, `use core_engine::pool::RtWorkgroup;`, and to `impl CoreAudioBackend` (after `info`):

```rust
    /// The open device's IO-thread workgroup, for render workers to join
    /// (spec §7); `None` when closed or unsupported.
    pub fn workgroup(&self) -> Option<Arc<dyn RtWorkgroup>> {
        let device = self.device?;
        let raw: usize = get(
            device,
            kAudioDevicePropertyIOThreadOSWorkgroup,
            kAudioObjectPropertyScopeGlobal,
        )
        .ok()?;
        // SAFETY: the HAL returns a retained `os_workgroup_t` that "the
        // caller is responsible for releasing" (AudioHardware.h);
        // `OsWorkgroup` releases it on drop.
        let wg = unsafe { crate::rt::OsWorkgroup::from_owned(raw as *mut c_void) }?;
        Some(Arc::new(wg))
    }
```

(`get::<usize>` reads a pointer-sized property; `usize` is `Copy + Default` as `get` requires.)

- [ ] **Step 5: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p platform-macos --test rt --test mixer
cargo test -p platform-macos --test rt -- --ignored
```

Expected: `rt` 4 passed (1 ignored), `mixer` 1 passed; the ignored workgroup test passes on a Mac with an output device (it may fail on a CI runner without one; that run is informational, like `hal_default_device_opens`).

- [ ] **Step 6: Lint and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git add crates/platform-macos
git commit -m "feat(platform-macos): Mach waker, time-constraint workers, device workgroup

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---
### Task 14: vizdaw-play plays through the mixer with workers; final checks

**Goal:** `vizdaw-play` plays the demo through the built-in synth's strip into master on the parallel renderer: `--workers N` (default `min(performance cores − 1, 7)` live, 0 offline), the macOS platform seams, the device's workgroup and period handed to the workers (again after buffer-size changes), deadline 0.8, DSP load in the status line, graph diagnostics on stderr. The offline golden stays byte-identical, also with workers. Then the whole workspace (including `core-ffi` and the Swift app build) is verified.

**Files:**
- Modify: `crates/vizdaw-play/src/args.rs`
- Replace: `crates/vizdaw-play/src/live.rs`
- Modify: `crates/vizdaw-play/src/offline.rs`
- Modify: `crates/vizdaw-play/src/main.rs`
- Modify: `crates/vizdaw-play/README.md`
- Test: `crates/vizdaw-play/tests/cli.rs`

**Interfaces:**
- Consumes: `Engine::{with_platform, set_rt_env, dsp_load, counters}`, `EngineConfig.{workers, deadline}`, `CompileReport.graph`, `platform_macos::rt::{MacRtPlatform, default_workers}`, `CoreAudioBackend::workgroup`, `pool::RtEnv`.
- Produces: CLI option `--workers N` (0–15); `offline::render(doc, seq, route, looped, workers: usize)`.

- [ ] **Step 1: Write the failing tests**

Add to `crates/vizdaw-play/tests/cli.rs`:

```rust
#[test]
fn offline_render_with_workers_matches_the_checked_in_log() {
    let dir = temp("workers");
    let log = dir.join("demo.txt");
    let out = bin()
        .arg(repo().join("fixtures/demo.vizdaw"))
        .args(["--seq", "Song", "--workers", "4", "--offline"])
        .arg(&log)
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let got = std::fs::read_to_string(&log).unwrap();
    let want = std::fs::read_to_string(repo().join("fixtures/demo.offline.txt")).unwrap();
    assert!(got == want, "offline log with workers differs from fixtures/demo.offline.txt");
}

#[test]
fn too_many_workers_are_refused() {
    let out = bin()
        .arg(repo().join("fixtures/demo.vizdaw"))
        .args(["--workers", "16", "--offline", "x.txt"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
}
```

- [ ] **Step 2: Run them to verify they fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p vizdaw-play --test cli workers
```

Expected: FAIL — `unexpected argument '--workers'` (exit 2 in the first test; the second passes for the wrong reason until the option exists, which is fine).

- [ ] **Step 3: The option**

In `crates/vizdaw-play/src/args.rs`, add to `struct Args` after `midi_offset_ms`:

```rust
    /// Render worker threads besides the audio thread (0–15). Default:
    /// performance cores − 1, at most 7, when playing live; 0 offline.
    #[arg(long, value_name = "N", value_parser = clap::value_parser!(u32).range(0..=15))]
    pub workers: Option<u32>,
```

- [ ] **Step 4: Offline rendering takes a worker count**

In `crates/vizdaw-play/src/offline.rs` change the signature and config:

```rust
/// Plays `seq` to its end and returns the compile report and the log text.
/// When `looped`, the loop is switched off during the third pass, so three
/// passes play followed by the tail. `workers` render threads help; the
/// log does not depend on them.
pub fn render(
    doc: &Document,
    seq: SeqId,
    route: BuiltinRoute,
    looped: bool,
    workers: usize,
) -> Result<(CompileReport, String), String> {
    let sinks = OfflineSinks::new(1 << 16);
    let log = sinks.log();
    let mut cfg = EngineConfig::new(RATE);
    cfg.route = route;
    cfg.workers = workers;
```

(the rest of the function is unchanged).

In `crates/vizdaw-play/src/main.rs` replace the offline block's first lines with:

```rust
    if let Some(out) = &args.offline {
        let workers = args.workers.unwrap_or(0) as usize;
        let (report, text) = offline::render(&doc, seq, args.to.into(), args.looped, workers)?;
        for w in report.warnings.iter().chain(&report.graph.warnings) {
            eprintln!("warning: {w}");
        }
        for (id, why) in &report.graph.placeholders {
            eprintln!("warning: {id}: {why}");
        }
        if let Some(e) = &report.graph.error {
            eprintln!("warning: mixer graph not used: {e}");
        }
```

(keep the following `std::fs::write` and `eprintln!("wrote …")` lines).

- [ ] **Step 5: Live playback through the platform seams**

Replace `crates/vizdaw-play/src/live.rs` with:

```rust
//! Live playback through CoreAudio, the DLS synth's mixer strip and
//! CoreMIDI (macOS), rendered in parallel on worker threads joined to the
//! device's workgroup.

use crate::args::Args;
use crate::signal;
use core_engine::backend::{AudioBackend, AudioConfig, BackendEvent, StreamInfo};
use core_engine::engine::{Engine, EngineConfig};
use core_engine::pool::RtEnv;
use core_engine::render::Renderer;
use core_engine::transport::PlayState;
use core_model::document::Document;
use core_model::ids::SeqId;
use core_model::time::Tick;
use platform_macos::hal::CoreAudioBackend;
use platform_macos::rt::{MacRtPlatform, default_workers};
use platform_macos::sinks::MacSinks;
use std::io::Write;
use std::sync::Arc;
use std::time::Duration;

/// Live blocks may use 80 % of their duration (Plan Ruling 18).
const DEADLINE: f64 = 0.8;

struct Session {
    backend: CoreAudioBackend,
    engine: Engine,
    info: StreamInfo,
}

fn engine_config(args: &Args, rate: f64) -> EngineConfig {
    let mut cfg = EngineConfig::new(rate);
    cfg.route = args.to.into();
    cfg.midi_offset_ns = (args.midi_offset_ms * 1e6).round() as i64;
    cfg.workers = args.workers.map_or_else(default_workers, |w| w as usize);
    cfg.deadline = Some(DEADLINE);
    cfg
}

/// The device's block period and workgroup, for the workers.
fn rt_env(backend: &CoreAudioBackend, info: &StreamInfo) -> RtEnv {
    RtEnv {
        period_ns: (f64::from(info.buffer_frames) * 1e9 / info.sample_rate.max(1.0)).round() as u64,
        workgroup: backend.workgroup(),
    }
}

/// Opens the device, builds the engine at its rate, loads and starts.
fn open(
    args: &Args,
    doc: &Document,
    seq: SeqId,
    at: Tick,
    playing: bool,
) -> Result<Session, String> {
    let mut backend = CoreAudioBackend::new();
    let cfg = AudioConfig {
        device: args.device.clone(),
        buffer_frames: args.buffer,
    };
    let info = backend
        .open(&cfg)
        .map_err(|e| format!("audio device: {e}"))?;
    let ecfg = engine_config(args, info.sample_rate);
    let (mut engine, renderer) = Engine::with_platform(
        ecfg,
        Box::new(MacSinks::new()),
        Arc::new(MacRtPlatform),
    )
    .map_err(|e| e.to_string())?;
    let env = rt_env(&backend, &info);
    let joined = env.workgroup.is_some();
    engine.set_rt_env(env);
    let report = engine.load(doc, seq).map_err(|e| e.to_string())?;
    for w in report.warnings.iter().chain(&report.graph.warnings) {
        eprintln!("warning: {w}");
    }
    for (id, why) in &report.graph.placeholders {
        eprintln!("warning: {id}: {why}");
    }
    if let Some(e) = &report.graph.error {
        eprintln!("warning: mixer graph not used: {e}");
    }
    engine.set_loop(args.looped).map_err(|e| e.to_string())?;
    if at != Tick::ZERO {
        engine.locate_tick(at).map_err(|e| e.to_string())?;
    }
    if playing {
        engine.play().map_err(|e| e.to_string())?;
    }
    backend
        .start(renderer)
        .map_err(|e| format!("audio start: {e}"))?;
    eprintln!(
        "playing on {} at {} Hz, {} frames, {} event(s), compiled in {:.1} ms, \
         {} worker(s){}, latency {} frame(s)",
        info.device_name,
        info.sample_rate,
        info.buffer_frames,
        report.events,
        report.compile_time.as_secs_f64() * 1e3,
        ecfg.workers,
        if joined { " in the device workgroup" } else { "" },
        report.graph.latency_frames
    );
    Ok(Session {
        backend,
        engine,
        info,
    })
}

/// Stops the device, frees the renderer on this thread, then drops the
/// engine: its MIDI sender drains and sends All Notes Off to every MIDI
/// output it owned before the next session opens any.
fn close(mut s: Session) {
    let r: Option<Renderer> = s.backend.stop().ok();
    drop(r);
    drop(s.engine);
}

/// Stops the transport and gives a live device a few blocks to play the
/// note-offs (and release tails).
fn stop_and_wait(s: &mut Session) {
    let _ = s.engine.stop();
    std::thread::sleep(Duration::from_millis(300));
}

/// Plays until the end (exit 0) or Ctrl-C.
pub fn run(args: &Args, doc: &Document, seq: SeqId) -> Result<(), String> {
    signal::install();
    let meter = doc.sequence(seq).map_err(|e| e.to_string())?.meter.clone();
    let mut s = open(args, doc, seq, Tick::ZERO, true)?;
    loop {
        std::thread::sleep(Duration::from_millis(100));
        let pos = meter.tick_to_bbu(s.engine.position_tick(), 4);
        let load = s.engine.dsp_load();
        print!(
            "\r{:>4}.{}.{}  load {:>3.0}%  ",
            pos.bar,
            pos.beat,
            pos.unit + 1,
            load.peak * 100.0
        );
        let _ = std::io::stdout().flush();
        let faults = s.engine.faults();
        if !faults.is_empty() {
            let c = s.engine.counters();
            eprintln!(
                "\nfault: {} (midi dropped {}, swaps deferred {}, xruns {}, deadline misses {}, \
                 busy blocks {}, plugin faults {})",
                faults.names().join(", "),
                c.midi_dropped,
                c.swaps_deferred,
                c.xruns,
                c.deadline_misses,
                c.busy_blocks,
                c.plugin_faults
            );
        }
        if s.engine.take_ended() {
            println!();
            break;
        }
        if signal::interrupted() {
            // Stop sends note-offs; give the device a few blocks to play
            // them and the release tails.
            stop_and_wait(&mut s);
            println!();
            break;
        }
        for ev in s.backend.take_events() {
            match ev {
                BackendEvent::BufferSizeChanged(n) => {
                    eprintln!("\nbuffer size changed to {n}");
                    let mut r = s.backend.stop().map_err(|e| e.to_string())?;
                    if n > r.config().max_frames {
                        r.prepare(n);
                    }
                    s.backend.start(r).map_err(|e| e.to_string())?;
                    s.info.buffer_frames = n;
                    s.engine.set_rt_env(rt_env(&s.backend, &s.info));
                }
                BackendEvent::DeviceLost
                | BackendEvent::DefaultDeviceChanged
                | BackendEvent::SampleRateChanged(_) => {
                    eprintln!("\n{ev:?}: reopening the output");
                    let at = s.engine.position_tick();
                    let playing = s.engine.state() == PlayState::Playing;
                    if !matches!(ev, BackendEvent::DeviceLost) {
                        // The device still renders: let the engine send its
                        // note-offs to MIDI outputs before the rebuild.
                        stop_and_wait(&mut s);
                    }
                    let old_rate = s.info.sample_rate;
                    close(s);
                    s = open(args, doc, seq, at, playing)?;
                    if s.info.sample_rate != old_rate {
                        eprintln!(
                            "sample rate {old_rate} -> {}: recompiled",
                            s.info.sample_rate
                        );
                    }
                    break;
                }
            }
        }
    }
    close(s);
    Ok(())
}
```

(The old engine is dropped in `close`, which joins its workers; their workgroup memberships are left on their own threads.)

- [ ] **Step 6: README**

In `crates/vizdaw-play/README.md`, add `[--workers N]` to the usage block's first line after `[--buffer N]`, add this paragraph after the `--offline` paragraph:

```markdown
Playback goes through the project's mixer: the built-in synth plays on its
"Built-in Synth" strip (inserts, fader, pan, sends) into master, rendered
by the audio thread plus `--workers N` worker threads (0–15; default:
performance cores − 1, at most 7; 0 for `--offline`). Workers run with
real-time priority in the output device's workgroup. The status line shows
the DSP load (render time ÷ block time, peak over 0.1 s).
```

and append these smoke steps:

```markdown
6. `cargo run -p vizdaw-play --release -- fixtures/demo.vizdaw --seq Song --workers 0`
   then the same with the default and with `--workers 7` — the music sounds
   the same each time (at −3 dB compared with 2b: the built-in strip's
   centred pan); the startup line names the worker count and "in the device
   workgroup"; the load figure stays well below 100 %.
7. While it plays with workers, switch the output device or its buffer size
   in Audio MIDI Setup — playback continues; no `workgroup-join-failed` or
   `thread-policy-failed` fault is printed.
8. `cargo run -p vizdaw-play --release --features rt-check -- fixtures/demo.vizdaw --workers 4`
   — the last line reads `rt-check: 0 allocation(s) on the render path`
   (workers arm themselves under `rt-check`).
```

- [ ] **Step 7: Run everything**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p vizdaw-play
cargo run -q -p vizdaw-play -- fixtures/demo.vizdaw --seq Song --offline "$TMPDIR/demo.txt"
diff fixtures/demo.offline.txt "$TMPDIR/demo.txt" && echo "golden unchanged"
cargo run -q -p vizdaw-play -- fixtures/demo.vizdaw --seq Song --workers 8 --offline "$TMPDIR/demo8.txt"
diff fixtures/demo.offline.txt "$TMPDIR/demo8.txt" && echo "golden unchanged with 8 workers"
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p vizdaw-play --all-targets --features rt-check -- -D warnings
cargo test --workspace
cargo test -p core-ffi
cargo bench --workspace --no-run
```

Expected: everything passes; both diffs print their "unchanged" line (Plan Ruling 28: the event log has 335 lines and the demo digest is still `lines 335 hash 1feb33c1bfd60916`). `core-ffi` needs no source change: its `Engine::new(EngineConfig::new(..), ..)` calls keep 0 workers and no deadline. If the Swift toolchain is usable on this machine, also run `scripts/build-ffi.sh && scripts/swift.sh build` (CI's `app` job does this); with the Xcode license unaccepted it may not run locally, which is acceptable because the FFI surface did not change.

- [ ] **Step 8: Human smoke test**

Ask the human partner to run README steps 1–8 on a Mac with speakers and report anything audible that differs from the expectations.

- [ ] **Step 9: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/vizdaw-play
git commit -m "feat(vizdaw-play): play through the mixer on worker threads in the device workgroup

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

## Self-Review

**Spec coverage (3a slice of §14):**
- §4 entities `Mixer`, `Strip`, `StripKind`, `Output`, `Send` (`AuxSend`), `PluginSlot`, `PluginRef`, `Destination::PluginInstrument { slot }` — Tasks 1–3; built-in synth strip — Task 3 (Ruling 1).
- §4 rules: DAG over outputs/sends/sidechains, valid targets, one strip per plugin instrument/output path/master, deletion retargeting in one undo step, solo — Tasks 3–5. "InstrumentOutput strips exist only for indices the instrument's last known port layout has" needs a port layout, which arrives with plugins in 3b; 3a lets `AddInstrumentOutput` create any index ≥ 1 and never deletes them (consistent with "extra strips stay and are silent").
- §4 parameter overlay and state (blobs in `PluginState/`, immutable, unreferenced removed on save, overlay per gesture undoable, capture non-undoable) — Tasks 5–6.
- §4 file format v2 and migration — Task 2.
- §6 nodes, edges with delay lines, PDC incl. sidechain alignment and bypass-keeps-latency, total latency `L` and MIDI/display offsets, instance reuse, limits — Tasks 8, 9, 11 (Rulings 14, 15, 17).
- §7 pool, workgroup, RT scheduling, per-block protocol with atomics and `ArrayQueue`, spin then wait, determinism, `W = 0`, deadline, meters, DSP load, faults — Tasks 9, 11, 13 (Rulings 18–24).
- §12 `RoutingCycle`, `InvalidRoutingTarget` (Task 1), `PluginFault`, `GraphTooLarge`, `LatencyTooLarge`, `WorkgroupJoinFailed` (Tasks 9, 11). `HostError` belongs to 3b.
- §13 core-model proptests (undo, cycle check, solo reference, migration of the 2a fixtures) — Tasks 2, 4, 5; core-engine golden graph cases, PDC sample-exact, parallel determinism across W 1–8 and block sizes, zero allocations on callback and workers, deadline path, fault bypass after 16 blocks — Tasks 8, 9, 11, 12.
- §14 "ends with the demo playing through the built-in synth via the mixer" — Task 13 `the_demo_plays_through_the_builtin_strip_into_master` and Task 14 smoke steps.

**Placeholder scan:** every code step shows complete code or exact, enumerated mechanical edits (Task 2 Step 6); generated artefacts (`fixtures/demo.vizdaw/project.json`) come from the stated commands and are pinned by `checked_in_demo_matches_a_fresh_build`; the offline golden is pinned by its digest.

**Type consistency:** `StripId`/`PluginInstanceId`/`StateBlobId` (Task 1) are used unchanged throughout; `RtPlatform::waker()` takes no argument everywhere (Tasks 9, 13); `Exec::try_finish(Option<Instant>)` (Task 9) is what `Renderer` calls (Task 11); `InsertInit`, `Graph::build`, `GraphReport`, `MixerGraph.strips` and `Engine::meters` agree across Tasks 9–11; `Builder::{from_doc, strip_of, bus, insert, mixer}` (Task 10) are what Tasks 11–13 call.

**Review Focus:** each of the five lines names a test in its owning task; all five tests are written out above.

**Dry run:** while writing, every task's code was applied in order to a scratch copy of the repository (outside the repo, nothing committed) and checked with the task's own commands: clippy `-D warnings` clean after each task, 393 workspace tests passing at the end, the offline golden unchanged with 0 and 8 workers, the macOS workgroup join passing on a real output device, and a silent live run (`--to virtual`) reporting "3 worker(s) in the device workgroup" with no faults. The dry run is why several details look the way they do: dead-code-free task boundaries (crate-internal mutators arrive with their first caller), per-worker wakers (Ruling 21), `try_finish` waiting for a leaving worker (Ruling 18), `RETIRE_CAPACITY` 16 (Ruling 30). One pre-existing issue surfaced: 2b's `tests/rt_alloc.rs` fails with `Busy` when run with `--release` (its swap loop outruns the 10 ms collector), before and after this plan; CI runs it in debug, where it passes.
