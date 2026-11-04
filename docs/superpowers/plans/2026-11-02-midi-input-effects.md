# VizDAW 5g-1 — MIDI Input Effects Implementation Plan

> **Deferred (owner, 2026-11-03):** this plan runs right after sub-project 7 (UI polish), not in sub-project 5. It stays in the 1.0 release. Before it runs it gets a fresh pre-flight against the tree as it stands then. For 5g-1 the binding owner ruling also applies: a learned message is kept out of the lanes.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Each MIDI lane gets a chain of up to 8 built-in input effects: Arpeggiator, Transpose, Velocity, Chord and Input Quantize. The chain runs on the real-time thread while the lane is armed or monitoring its input. What it plays thru is exactly what is recorded, with Input Quantize applied to recorded note starts only. The chain is an undoable lane edit, edited from an FX button and popover on the lane header and from `lane.set_input_fx` on the control channel.

**Architecture:**
- **`core-model`** gains `core_model::input_fx` (`InputFx`, `InputFxKind` and the effects' settings, with their checks), `Lane.input_fx` and `LaneCmd::SetInputFx`. The field is saved only when the chain is not empty, so `file::VERSION` stays 3.
- **`core-engine`** gains `core_engine::input_fx`, which holds:
  - `LiveMsg`, the live input event. It is MIDI 2.0-shaped: notes with 16-bit velocity, per-note messages addressed by pitch, and channel messages. Plan 5g-2's MPE decoding and encoding use the same type.
  - `InputChain`: the stages, the arpeggiator, an output stage that merges duplicate pitches, a fan-out cap and counters. It has no allocation, no lock and no I/O.

  The renderer feeds every listening lane's chain from the MIDI input queue and ticks it once per block, so arpeggiator steps land on their sample. A changed chain sends its note-offs first. An unchanged chain keeps its state across route changes. The record tap now carries `LiveMsg` and the chain's Input Quantize.
- **`core-ffi`**:
  - `passes` turns recorded `LiveMsg`s into events and applies Input Quantize to note starts.
  - The FFI gains `InputFxRow` and its kinds, `LaneEdit::SetInputFx`, `LaneRow.input_fx`, and `input_fx_from_json`.
  - The lane's input spec now carries the chain. "All MIDI Inputs" lanes now skip the ports that 5f-1's control surfaces use.
- **`core-control`, Swift, `vizctl`:**
  - The control channel gets `lane.set_input_fx` and `state.get document`'s `input_fx` per lane. `state.get input` reports the count of dropped notes.
  - The app gets `InputFxModel`, the lane header's FX button (lit when the chain is active) and the effects popover.
  - Also in this plan: the e2e "`midi.inject` held chord into a lane with an arpeggiator while recording", README 189–195, and two backlog rows.

**Tech Stack:** Rust 2024 (`core-model`, `core-engine`, `core-ffi` with UniFFI 0.32.2, `core-control`, `vizctl`), Swift 6 / SwiftPM (Swift Testing), macOS 14+.

**Spec:** `docs/superpowers/specs/2026-10-18-recording-audio-design.md`, §13 row "5g-1 — MIDI input effects". This row and "5g-2 — MPE input and output" replace the former 5g row; the split is Ruling 1 of this plan, and the writer's commit amends §13. This plan delivers:
- §4.5's `input_fx`: at most 8 slots, empty by default, an undoable lane edit;
- §6.6's "Thru and record both pass through the lane's MIDI input effect chain";
- §6.8 in full:
  - the model;
  - the five built-in effects with every listed setting;
  - the chain on the real-time thread per armed or Input-monitored lane, sample-accurate and synced;
  - fixed-capacity state (32 held, 128 sounding) with overflow counted;
  - note-offs on a change or a bypass;
  - post-effect recording with Input Quantize on recorded starts only;
  - no chain for lanes that are not listening, and none on playback;
  - the FX button and popover, one undo step per gesture;
- §6.9's FX button on `LaneHeaderView`;
- §12.1's `lane.set_input_fx { lane, chain }` and `state.get`'s per-lane `input_fx`;
- §12.3's MIDI input effects tests, the arpeggiator e2e, `rt-check` over the chain, and the manual checklist's "the arpeggiator against a real keyboard, synced and free-running";
- spec Ruling 27.

Plan 5g-2 (`docs/superpowers/plans/2026-11-02-mpe.md`) runs after this one and builds on `LiveMsg`.

Read the whole spec (its §3 rules bind here) and the 5f plans' Interfaces blocks that this plan touches:
- 5f-1's `crate::surfaces::surface_list()` and its `inject_midi` change (`docs/superpowers/plans/2026-11-02-control-surfaces.md`, Tasks 7–8);
- 5f-2's `pub mod bounce;` in `core-engine/src/lib.rs`.

5g runs after 5f-3. Every Find excerpt below matches `f3291d0` exactly once unless the step says it is 5f's text. If 5f moved an excerpt, make the same change to the current text and say so in the report.

## Global Constraints

- **Shell setup.**
  - Every shell command runs after `export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools`. Every bash block below starts with that line and `cd /Users/lattawb/workspace/vizdaw`.
  - A `git commit` without `DEVELOPER_DIR` fails silently: check every commit with `git log -1 --oneline` and `git show --stat HEAD`.
- **Swift only through the scripts, from the repo root.**
  - The scripts: `scripts/build-ffi.sh` (after any Rust change that touches the FFI, before Swift), `scripts/swift.sh build`, `scripts/swift.sh test` and `scripts/make-app.sh`.
  - Never call `swift`, `xcodebuild` or `xcrun` directly.
- **Branch and commits.**
  - Work on branch `sp5-audio`, at the end of plan 5f-3. Never switch branches, never push.
  - Commit at the end of every task, adding only the files the task lists, by path (never `git add -A`). No manifest changes, so `Cargo.lock` never changes.
  - Never commit `.superpowers/`, `.DS_Store` or generated files.
  - End every commit message with these two attribution lines. Use the `Co-Authored-By` line naming the model you actually are:
    ```
    Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
    Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P
    ```
- **Clean room.**
  - Never read or copy anything from `~/workspace/SVP/research`, the Studio Vision binary or any Studio Vision material.
  - No Opcode, Apple Logic, Ableton, Steinberg, Midas or Soundcraft assets, names, artwork or branding anywhere.
  - Effect behaviour comes from this spec and from public MIDI specifications only.
- **No GUI tools, no real devices.**
  - Never use GUI, computer-use or accessibility-automation tools. Views are checked by the owner by eye from README steps 189–195.
  - Launch VizDAW only through the e2e (Task 10) with `VIZDAW_AUDIO=offline`. At the end of a task that launched it, `pgrep -lf MacOS/VizDAW` prints nothing and `ls -d /tmp/vze*` prints nothing new.
  - **Tests never open real MIDI ports or devices, never open real audio input and never trigger the microphone prompt.**
    - MIDI reaches the engine through `MidiSender` (core-engine tests), `Session::inject_midi` (core-ffi tests) and `midi.inject` (e2e). No CoreMIDI client is created.
    - Every `core-ffi` test that plays or records calls `vizdaw_ffi::host::force_offline()` first, through its file's `serial` helper, and chooses its fake input with `host::set_fake_input`.
  - Never touch the real `~/Library/Application Support/VizDAW`, the real preferences, the real Trash or any device. Tests use temp folders and `Session::debug_set_trash_dir`. Swift tests pass `MemoryDefaults()`, and the e2e runs in its throwaway home.
- **Test waits.**
  - Every `core-ffi` integration test that waits for the transport, a pass or the engine polls with a deadline from `common::wait_limit()`, never with a fixed sleep for the result. (`common::wait_limit()` is in `crates/core-ffi/tests/common/mod.rs` and reads `VIZDAW_TEST_WAIT_SECS`.)
  - The core-engine tests drive time with `OfflineBackend::run_frames` and the chain tests with an explicit `ChainClock`. Neither sleeps.
- **Crates and dependencies.**
  - Rust stable, edition 2024, license GPL-3.0-only.
  - No new crate and no new dependency. The arpeggiator's Random mode uses a 32-bit xorshift written in the module (Ruling 9), not a crate.
- **Lints.** Every crate keeps `#![deny(missing_docs)]` and `#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic))]`. New non-test code uses `.get()`, iterators and `checked_*`/`saturating_*`, never indexing or unwraps.
- **Platform-neutral cores.** `core-model`, `core-engine`, `core-media` and `core-control` stay free of OS APIs.
- **Linux CI.**
  - CI runs `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` on `ubuntu-latest`.
  - Nothing in this plan is macOS-only: the chain, the passes and the FFI rows are platform-neutral, and every test here runs on Linux.
  - Every task that changes Rust checks `cargo clippy --workspace --all-targets --target x86_64-unknown-linux-gnu --target-dir target/linux-check -- -D warnings`.
- **TDD.** Every new test is run and seen to fail (RED) before the code that makes it pass.
- **Real-time rules (spec §3).**
  - The chain runs on the real-time thread. No allocation, free, lock or I/O happens there.
  - Everything new on that thread is marked `// RT`:
    - `InputChain::{process, tick, flush, carry_from, quantize, take_dropped}`, the stages, the arpeggiator and the merge;
    - `LiveMsg`'s conversions;
    - the renderer's `live_midi`, `emit_live` and the `SetInputs` carry;
    - `RecordTap::midi`.
  - `InputChain::new` allocates the chain's one `Box<ChainState>` on the control thread, inside `Engine::load`. Retired routes are freed on the retire thread as now.
  - Recursion through the stages is bounded by `MAX_INPUT_FX`. Fan-out is bounded by `MAX_FANOUT` per input event (Ruling 7).
  - **rt-check gates** run in Tasks 4 and 10:
    - `cargo clippy -p vizdaw-play --all-targets --features rt-check -- -D warnings`;
    - `cargo test -p core-engine --features rt-check --test rt_alloc --test rt_alloc_graph --test rt_alloc_record --test rt_alloc_gestures --test rt_alloc_input_fx` (the last is new in Task 4).
- **Media is write-once.** Nothing here writes media.
- **File compatibility.**
  - `core_model::file::VERSION` stays 3. `Lane.input_fx` is `#[serde(default, skip_serializing_if = "Vector::is_empty")]`, so a project with no chains saves byte for byte as before, and older projects open with empty chains.
  - Builds before 5g-1 name a project with a chain Corrupt (`deny_unknown_fields`). This is the pattern 5f-1 Ruling 6 recorded; Task 10 adds the backlog row.
- **FFI safety.**
  - No panic crosses the FFI. Every new exported function or method returns `Result<_, VizError>` and runs inside `ffi_guard`; `input_fx_from_json` is one of them.
  - The only infallible new export is `input_fx_to_json`. It is a plain `pub fn` used by `control.rs`, not exported to Swift.
- **UniFFI changes** (Task 6):
  - New enums: `InputFxKindRow`, `ArpModeRow`, `FeelRow`.
  - New record: `InputFxRow`.
  - New variant: `LaneEdit::SetInputFx`.
  - New fields: `LaneRow.input_fx` (`#[uniffi(default)]`) and `InputInfoRow.midi_fx_dropped` (`#[uniffi(default)]`).
  - New free function: `input_fx_from_json`.
  - No new `Session` method, so `FakeSession` gains nothing for the FFI.
- **Swift.**
  - Generated bindings compile in Swift 5 mode. `VizDAWAppModel`, `VizDAWEditors`, `VizDAW` and the tests compile in Swift 6 mode with no new warnings.
  - No SwiftUI `@State`/`@FocusState` for model data, only for view-local presentation flags. The popover's draft lives in `InputFxModel`.
- **Control channel** (control spec §6, §7).
  - Programmatic calls never open dialogs.
  - Every command is in `core_control::registry::COMMANDS` and handled by `ControlCommands.run`. The Swift test `everyRegisteredMenuCommandHasAHandler` compares them.
  - The registry count test moves from its value at the end of 5f-3 (written there as 146) to that value + 1 in Task 7.
  - Hostile-input tests are added at every layer:
    - `input_fx_commands_check_their_params` (core-control);
    - `inputFxCommandsCheckTheirArguments` (app);
    - `hostile_chains_are_refused_and_change_nothing` (model);
    - `hostile_chain_json_is_refused` (FFI).
- **Generated files** are never committed: `app/Sources/VizDAWCore/vizdaw_ffi.swift`, `app/Sources/vizdaw_ffiFFI/include/`, `target/ffi/`, `dist/`.
- **Each task ends green.**
  - Always: `cargo fmt --all` then `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, the Linux clippy, and `cargo test` for the crates the task touches (the whole workspace in Tasks 4, 6 and 10).
  - Tasks 6–10 also run `cargo clippy -p core-ffi --lib -- -D warnings`, `scripts/build-ffi.sh`, `scripts/swift.sh build` and `scripts/swift.sh test`.
  - Task 10 also runs `scripts/make-app.sh` and the e2e: `VIZDAW_E2E_APP=dist/VizDAW.app cargo test -p vizctl --test e2e -- --ignored --nocapture --test-threads 1`.
  - Known flaky tests: the `damaged_files` proptest, the MIDI listener tests (`platform-macos/tests/midi.rs`, the reconnect test), `core-ffi` catalog tests under parallel load, and the latency e2e timing. Re-run once before investigating, and say so in the report.
  - Baseline: the counts plan 5f-3's last task measured. Its writer estimated registry 146, e2e 13 and README through 188. The counts before 5f were Rust 1632 / 14 ignored, Swift 899, e2e 10 and registry 138.
  - Each task's last step gives the counts it adds. Report the measured counts; a slip in the writer's arithmetic is not a failure.
- **How code is given.**
  - New test files are written whole (`cat > … <<'VIZDAW_EOF'`). For new implementation files, the step gives the public items with their doc lines, and the algorithm where the signature and the tests leave a choice.
  - Changes to existing files are **Find / Replace** excerpts, applied in order with the Edit tool. Each Find occurs exactly once in its file at `f3291d0`, or in the file an earlier task of this plan left (the step says which).
  - If an excerpt does not match, make the same change to the current text. If a name differs from the tree, use the tree's name and say so in the report.
- **Spec values used verbatim:**
  - "an ordered chain of up to 8 `InputFx { kind: InputFxKind, bypass: bool }`";
  - Arpeggiator:
    - "mode Up, Down, Up-Down, Down-Up, As Played, Random or Chord";
    - "rate as a note value from 1/1 to 1/64, straight, dotted or triplet";
    - "octaves 1–4";
    - "gate 5–200 %";
    - "velocity As Played or Fixed (1–127)";
    - "Latch (held notes keep playing after release until new notes arrive)";
    - "Sync: to transport bars and beats while playing, and free-running at the project's current tempo when stopped";
  - "Transpose: ±48 semitones";
  - "Velocity: scale (0–400 %), offset (±127), clamp minimum and maximum, or Fixed";
  - "Chord: adds up to 6 notes at given semitone intervals (±48) to each note";
  - "Input Quantize: grid (the shared snap values) and strength (0–100 %). It moves note starts *only in what is recorded*. Live thru is never delayed";
  - "up to 32 held notes and 128 sounding notes. Overflow drops the newest note and counts it";
  - "A changed or bypassed effect first sends note-offs for every note it is sounding, so no note hangs";
  - "The recorder receives exactly what thru sent to the destination, with Input Quantize then applied to note starts";
  - "Lanes without an armed or monitored state do not run their chain. Playback of recorded events never passes through input effects";
  - "An "FX" button on the lane header (lit when the chain is non-empty and not all bypassed) opens a popover: the chain list: add, remove, drag to reorder, bypass; the selected effect's parameters. Settings are undoable lane edits (one step per gesture)".

## Review Focus

These are the five failure modes a user is most likely to hit that the spec implies but does not spell out, most likely first. Each is pinned by named tests in its task.

1. **A chain that changes while notes sound.** Examples: the user bypasses the arpeggiator mid-phrase, drags a velocity slider while holding a chord, arms a second lane while a latched arpeggio plays, or disarms the lane with keys down.
   - Expected:
     - every note the changed chain was sounding ends once, both in thru and in the recording;
     - a chain whose lane and settings did not change keeps playing (the latched arpeggio goes on);
     - a note-off for a note the new chain never started is dropped, not sent.
   - Pinned by `flush_ends_every_sounding_note_once` (Task 2), and by `a_changed_chain_ends_its_notes_and_a_kept_one_plays_on` and `bypassing_while_held_leaves_no_note_hanging` (Task 4).
2. **Notes pushed out of range or onto each other.** Examples: Transpose +24 on the top octave, Chord intervals that land on a held key, two chord notes on one pitch.
   - Expected:
     - a pitch outside 0–127 is dropped, never folded onto 127;
     - a pitch that two sources reach sounds once and ends only when the last of them ends;
     - chain order matters exactly where a drop applies.
   - Pinned by `out_of_range_pitches_are_dropped_not_folded`, `a_pitch_two_sources_reach_sounds_once_and_ends_once` and `chain_order_matters_where_a_drop_applies` (Task 2).
3. **Arpeggiator timing at the edges.** Examples: a chord pressed in the middle of a step while playing, a stopped transport at another tempo, a locate, a gate over 100 % on a single held note, a triplet rate.
   - Expected:
     - synced steps sit on the grid counted from tick 0, the first at the next step;
     - free-running starts at once and follows the project tempo at the play position;
     - a long gate ends the note before it is played again;
     - a locate moves the grid with the transport.
   - Pinned by `a_synced_arp_waits_for_the_next_step`, `free_running_starts_at_once_at_the_project_tempo`, `a_long_gate_on_one_note_ends_before_it_retriggers`, `a_locate_moves_the_synced_grid` and `rates_include_dotted_and_triplet` (Task 3), and by `arp_steps_follow_the_beat_while_playing` (Task 4).
4. **Too many notes.** Examples: a forearm on the keyboard (33 keys) into the arpeggiator, or eight 6-note chords chained.
   - Expected: the newest note past a capacity is dropped and counted; it reaches `state.get input`'s `midi_fx_dropped`. The real-time thread stays bounded, and nothing panics.
   - Pinned by `the_33rd_held_note_is_dropped_and_counted` (Task 3), `fan_out_is_capped_and_counted` (Task 2), `overflow_reaches_the_counters` (Task 4) and `midi_fx_dropped_is_reported` (Task 6).
5. **A hostile or damaged chain.** Examples: 9 effects, octaves 5, gate 4 %, a sixth-plus-one chord interval, an interval of 0, two Input Quantize effects, a grid that is not a snap value, malformed JSON on the channel, a damaged project file.
   - Expected: each is refused with a range or invalid error (a file is Corrupt, naming the field), and nothing changes.
   - Pinned by `hostile_chains_are_refused_and_change_nothing` and `a_damaged_chain_in_a_file_is_refused` (Task 1), `hostile_chain_json_is_refused` (Task 6), `input_fx_commands_check_their_params` and `inputFxCommandsCheckTheirArguments` (Task 7).

## Plan Rulings

Each ruling decides something the spec leaves open, in the format "what — why — cost if wrong".

1. **Ruling (the split): the former §13 row "5g — MIDI input effects and MPE" becomes two plans, run in this order:**
   - **5g-1**, this plan: input effects, engine and UI.
   - **5g-2**, MPE input and output (`docs/superpowers/plans/2026-11-02-mpe.md`).

   5g-2 builds on 5g-1's `LiveMsg` and the chain's place in the renderer. The writer's commit amends spec §13 with both rows.
   - Why:
     - as one plan, 5g would be about 19 tasks;
     - the effects and MPE are each testable alone, and each has its own golden tests, e2e and README steps;
     - MPE also changes the schedule compiler, the recorder's per-note data and destinations, which the effects never touch.
   - Cost if wrong: one more plan header and final review.
2. **Ruling (the MIDI input path's order).** A message from a source travels in this order:
   1. Raw MIDI 1.0 bytes from CoreMIDI or `midi.inject` go to the engine's MIDI queue (`host::midi_from_source`, `Session::inject_midi`). At the same time they go to 5f-1's surfaces and MIDI learn (`crate::surfaces::input`), which see them raw: before MPE decoding and before input effects.
   2. In the renderer, per listening lane, the message is first matched (port, channel, and since this plan not one of a surface's ports). Then it is decoded: MPE in plan 5g-2, `LiveMsg::from_midi1` here.
   3. Then the input effect chain runs.
   4. Then the output stage merges duplicate pitches.
   5. Then the message plays thru (encoded as MPE for MPE destinations in 5g-2), and the recorder gets the same messages with the chain's Input Quantize.

   Learn and learned maps do not consume what they hear. A learned knob's CC also reaches a lane listening on that port; a backlog row records this (Task 10).
   - Why:
     - learn maps name what the controller sent (a port, a channel, a CC; spec §4.8), so learn must see raw bytes;
     - MPE settings belong to a lane's input (5g-2), so decoding is per lane;
     - Transpose and Chord must move a note's per-note expression with it, so the chain works on decoded per-note events;
     - consuming a learned message would mean a learn-map lookup on CoreMIDI's input thread before the engine queue. That is a new lock-free structure for a case the spec does not ask for.
   - Cost if wrong: a learned CC may also play into an armed lane on the same port. The workaround is to give that lane a named port or channel; the backlog row tracks consuming.
3. **Ruling (surface ports): a lane whose input is "All MIDI Inputs" does not listen to the input ports of the control surfaces in Settings › Surfaces.** `MidiInputSpec` gains `skip_ports: Vec<u32>`, filled by `host::input_spec` from `crate::surfaces::surface_list()` (5f-1 Task 7). A lane naming a surface's port still hears it.
   - Why: an armed omni lane would otherwise arpeggiate MCU fader pitch bends and HUI pings. A surface is not an instrument.
   - Cost if wrong: a surface added while such a lane listens is skipped only from the next load (an edit, a Play or an arm).
4. **Ruling (`LiveMsg`): the chain works on MIDI 2.0-shaped events, not MIDI 1.0 bytes.**
   - `NoteOn { pitch, velocity: u16, expr: NoteExpr }`, `NoteOff { pitch, velocity: u16 }` and `PerNote { pitch, expr: Expr, value: u32 }`, where `Expr` is `Bend`, `Pressure` or `Timbre`, and `Channel(Midi1Msg)` for everything else.
   - `from_midi1` maps note-on with velocity > 0 to `NoteOn`, with the velocity up-scaled by the MIDI 2.0 min-center-max rule (the same as `vizdaw_ffi::input::upscale_velocity`). Note-off, and note-on with velocity 0, map to `NoteOff`. Poly pressure maps to `PerNote { Pressure }` (up-scaled to 32 bits). Every other channel message maps to `Channel`.
   - `to_midi1(ch)` is the inverse, using `midi1`'s down-scaling. `PerNote { Bend | Timbre }` has no MIDI 1.0 form on a non-MPE destination: it answers `None`. (5g-2 counts these drops.)
   - A note-on with velocity 0 now plays thru as a note-off (`8n`), not `9n … 00`.
   - Why: MIDI 2.0 throughout (parent spec §1, §5). Transpose and Chord map one pitch field. 5g-2 decodes MPE into the same type.
   - Cost if wrong: one enum to widen.
5. **Ruling (a chain change): any change to a lane's chain replaces the whole chain, and the old one first sends note-offs for every note it sounds.** A change is to its order, a setting, a bypass, adding or removing.
   - Every route change publishes new routes. A new route whose lane, port, channel, destination, thru flag, targets, destination channel and chain settings all equal an old route's takes over the old chain's state (`InputChain::carry_from`, a swap; no allocation). The arm flag may differ. The latch, the held keys, the step and the sounding notes go on.
   - Every other old route is flushed (`InputChain::flush`) to its old targets and to the recorder.
   - The renderer's blanket "release every thru note" on `SetInputs` now spares the targets of carried routes.
   - Why:
     - the spec's "A changed or bypassed effect first sends note-offs for every note it is sounding";
     - one rule for every change is simpler and safer than per-effect diffs;
     - carrying keeps a latched arpeggio alive when another lane is armed, which every route change (arming, monitoring, any lane input) would otherwise cut.
   - Cost if wrong: a velocity tweak while a chord is held ends that chord; the player presses again.
6. **Ruling (the output stage): the chain's last stage counts note-ons per output pitch.**
   - A note-on for a pitch already sounding is swallowed. A note-off only goes out when the count reaches 0.
   - A note-off for a pitch the chain is not sounding is dropped.
   - This applies to an empty chain too, so an omni lane receiving one pitch from two channels sounds it once.
   - Why: chords and transposes make duplicate pitches; a destination on one channel cannot tell them apart, and a second note-on followed by one note-off hangs some synths.
   - Cost if wrong: two quick hits on one pitch from two sources sound as one long note.
7. **Ruling (capacities).**
   - Each arpeggiator holds at most `MAX_HELD = 32` notes and sounds at most `MAX_SOUNDING = 128`.
   - One input event, or one arpeggiator step, yields at most `MAX_FANOUT = 128` outputs through the rest of the chain.
   - The newest note past a capacity is dropped and counted (`InputChain::take_dropped`, summed into `Counters.midi_fx_dropped`). A dropped note-on's later note-off is dropped too.
   - The other effects (Transpose, Velocity, Chord, Input Quantize) are stateless: they recompute a note-off from their fixed settings, so they need no note table.
   - Why: the spec's capacities. The fan-out cap bounds eight chained 6-note chords (7^8 notes) on the real-time thread.
   - Cost if wrong: a cap to raise.
8. **Ruling (pitch range): a note moved outside 0–127, together with its note-off and its per-note messages, is dropped, never clamped or folded.**
   - Transpose then Chord therefore differs from Chord then Transpose near the ends of the range. This is spec §12.3's "chain order matters … where clamps apply". The Velocity clamps are the other case.
   - Why: folding stacks unrelated notes on 0 or 127.
   - Cost if wrong: a pitch near the edge disappears instead of sounding an octave off.
9. **Ruling (arpeggiator patterns), with held notes `h` (sorted by pitch, or in press order for As Played) and octaves `o`:**
   - The pattern `P` is `h` played in each of `o` octaves, lowest first (pitches over 127 dropped):
     - **Up** is `P`. **Down** is `P` reversed.
     - **Up-Down** is `P` then `P` reversed without its ends (C E G E, …). **Down-Up** is its mirror.
     - **As Played** is `P` in press order, octave by octave.
     - **Random** draws `P[x % len]` each step from a 32-bit xorshift (`x ^= x << 13; x ^= x >> 17; x ^= x << 5`) seeded `0x2545_F491` when the chain is made.
     - **Chord** plays all of `P` at each step.
   - The step index survives added or released notes. It restarts at 0 when the held set empties.
   - Why: public, conventional arpeggiator behaviour. The ends are not repeated so Up-Down has no doubled top or bottom note. The seed makes Random testable.
   - Cost if wrong: one function per mode.
10. **Ruling (arpeggiator time).**
    - **Rate.** A note value is `4 × PPQ / division` ticks, with division 1, 2, 4, 8, 16, 32 or 64. Dotted multiplies by 3/2, triplet by 2/3. At PPQ 15 360 every rate is a whole number of ticks; the shortest is 1/64 triplet, 640 ticks.
    - **Synced (transport playing).** Steps fall on the multiples of the step counted from tick 0. Their frames come from the schedule's `TempoTable::tick_to_frame`, so tempo changes and ramps are followed. The first step after a press is the next step at or after it: a chord pressed mid-step waits for the next step.
    - **Free-running (stopped, including a count-in).** The first step plays at once. Following steps come every `step × 60 × rate ÷ (bpm × PPQ)` frames, with the bpm the schedule's at the play position (120 without a schedule).
    - **Gate.** The note-off comes `gate %` of the step after its note-on. When the same pitch is due again while it still sounds (gate ≥ 100 %), its note-off is sent first, on the same frame.
    - **Latch.** While latched, releasing keys keeps the held set. The first key pressed when no key is down starts a new set.
    - **Velocity.** As Played uses each held note's velocity; Fixed uses `up7(v)`.
    - **Clock reads.** The arpeggiator reads the clock once per block: a loop wrap inside a block moves the steps after the wrap by up to one block.
    - Why: spec §6.8's Sync; the grid counted from tick 0 matches play quantize (4e Ruling 10). Integer ticks with the tempo table give sample-exact steps, which the golden tests need.
    - Cost if wrong: a whole-note rate in an odd meter drifts from the barline (the grid is counted in quarter notes, not bars), and a step right after a loop wrap is late by at most one block.
11. **Ruling (what the recorder hears).** The recorder receives each output as `RecordTap::midi(lane, offset, msg, quantize)`:
    - **Input-driven output** keeps the input's offset: the run frame heard when it arrived (spec §6.3).
    - **Generated output** (arpeggiator steps and gate ends) gets its block offset plus the graph latency (`ChainClock.record_lead`). It is then placed on the timeline frame it was rendered at, which is where it sounds against the playback.

    `RecordedMidi` gains `msg: LiveMsg` (replacing `Midi1Msg`) and `quantize: Option<RecordQuantize>`, the chain's Input Quantize when the message was produced.
    - Why: "the recorder receives exactly what thru sent". A generated note is not late input, so the latency compensation that places input must not move it.
    - Cost if wrong: arpeggiated recordings sit one graph latency early; the e2e checks they are on the grid.
12. **Ruling (Input Quantize).**
    - A chain holds at most one Input Quantize (refused otherwise).
    - Its place in the chain does not matter: it never changes thru, and it applies to the chain's recorded output.
    - The grid is one of the shared snap values from 1/2 to 1/32, straight, dotted or triplet: 15 values (`core_model::input_fx::input_quantize_grids`). Snap's "Bar" is excluded because its length follows the meter. The grid is counted from tick 0, as `QuantizeSpec::apply` counts it.
    - `passes` moves each recorded note's start by `QuantizeSpec::grid(grid, strength).apply` and keeps its length; its per-note offsets move with it. Controllers never move.
    - Why: "Input Quantize then applied to note starts". Keeping the length keeps a quantized phrase's articulation.
    - Cost if wrong: a note's end moves with its start, up to half a grid.
13. **Ruling (where the chain runs).** The chain runs in `Renderer::live_midi`, once per block, before the graph renders:
    1. queued input through `process`;
    2. then `tick` for every route, so arpeggiators step and end gates with no input;
    3. then thru, at the output's block offset, with `Out.ns_per_frame` set from the rate so external MIDI is timed inside the block (it was 0, since every thru went out at offset 0).

    A lane's route exists only while it is armed or monitored Input (unchanged, `host::input_spec`). Playback never meets the chain.
    - Why: §6.8's "sample-accurate and synced".
    - Cost if wrong: a step to a plugin instrument in a block that automation splits into runs may land one run late. The offline log and MIDI destinations are exact.
14. **Ruling (the model's shapes).**
    - `InputFxKind` is externally tagged, snake_case: `{"arpeggiator": {...}}`, `{"transpose": {"semitones": 12}}`, `{"velocity": {...}}`, `{"chord": {"intervals": [4, 7]}}`, `{"input_quantize": {"grid": 3840, "strength": 100}}`.
    - `InputFx` is `{"kind": …, "bypass": true}`, with `bypass` absent when false.
    - The same JSON is `lane.set_input_fx`'s `chain` parameter and `state.get document`'s per-lane `input_fx`.
    - Defaults:
      - Arpeggiator: Up, 1/16 straight, 1 octave, gate 50 %, velocity As Played, Latch off;
      - Transpose: +12;
      - Velocity: scale 100 %, offset 0, min 1, max 127, not Fixed;
      - Chord: [4, 7];
      - Input Quantize: 1/16 at 100 %.
    - Chord intervals must be distinct and not 0, at most 6, each ±48.
    - Velocity's offset is in 7-bit steps, `min ≤ max`, both 1–127. The result is computed on 16 bits: `clamp(v × scale ÷ 100 + offset × 512, up7(min), up7(max))`, or `up7(fixed)`.
    - Why: one JSON shape everywhere, read and written by serde.
    - Cost if wrong: a JSON shape to migrate before release.
15. **Ruling (undo and recording).**
    - `LaneCmd::SetInputFx` is one undo step named "Set Input Effects".
    - A popover gesture (a slider drag, a stepper press, a drag to reorder, an add) is one call.
    - Chain edits are allowed while recording. They do not change which lanes record, so 5d-1 Ruling 18's guard does not apply. The change is heard at once (Ruling 5).
    - Why: spec §6.8, "one step per gesture"; changing the arpeggiator rate mid-take is a performance move.
    - Cost if wrong: one line in `retries_input_cmd`.
16. **Ruling (the FX button and popover).**
    - `HeaderControl.fx` is an 18 × 18 toggle labelled "FX", at x 92 in the header's top row; the name field narrows from 101 to 80 points. It is lit (system purple) when the chain has an effect that is not bypassed.
    - A click opens an `NSPopover` from the button. Its content is a SwiftUI `InputFxPopover` that the app installs through the hook `LaneHeaderView.fxPopoverContent`, since `VizDAWEditors` has no SwiftUI.
    - The popover shows:
      - the chain list, with drag to reorder, a bypass checkbox and remove per row, and an Add menu of the five kinds;
      - under it, the selected effect's controls.
    - Why: §6.9's FX button. Keeping AppKit in the header and SwiftUI in the app follows the existing layering.
    - Cost if wrong: a layout change.
17. **Ruling (counters).**
    - `Counters` gains `midi_fx_dropped`.
    - `InputInfoRow` gains `midi_fx_dropped`, and `state.get input` reports it as `"midi_fx_dropped"`.
    - Why: §6.8's "counts it" needs a place to be read; `state.get input` is the live-input report.
    - Cost if wrong: a field to move.

## Carried items

- No carry from 5f names input effects.
- The 5c hook `core_engine::input::InputChain` (an empty pass-through) is replaced by `core_engine::input_fx::InputChain` in Task 4.
- 5c's `MidiRoute.chain` stays the field's name.
- `docs/feature-backlog.md` gains two rows in Task 10:
  - "Learned controls are not consumed" (Ruling 2);
  - "A pre-5g build names a project with input effects Corrupt".

## File map

| File | Task | Role |
|---|---|---|
| `crates/core-model/src/input_fx.rs`, `src/lane.rs`, `src/command/lane.rs`, `src/rules.rs`, `src/lib.rs`, `tests/input_fx.rs` | 1 | the model |
| `crates/core-engine/src/input_fx/mod.rs`, `src/lib.rs`, `tests/input_fx.rs` | 2 | `LiveMsg`, the chain, the stateless effects, the merge |
| `crates/core-engine/src/input_fx/arp.rs`, `tests/input_fx_arp.rs` | 3 | the arpeggiator |
| `crates/core-engine/src/input.rs`, `src/engine.rs`, `src/render.rs`, `src/record.rs`, `tests/input_fx_live.rs`, `tests/rt_alloc_input_fx.rs`, `tests/live_input.rs`, `tests/record_tap.rs`, `tests/rt_alloc_record.rs` | 4 | the chain in the renderer and the recorder |
| `crates/core-ffi/src/passes.rs`, `tests/passes.rs` | 5 | recording `LiveMsg`, Input Quantize |
| `crates/core-ffi/src/input_fx.rs`, `src/lib.rs`, `src/edit.rs`, `src/snapshot.rs`, `src/host.rs`, `src/live.rs`, `src/session.rs`, `tests/input_fx.rs`, `tests/record.rs` | 6 | the FFI |
| `crates/core-control/src/registry.rs`, `tests/commands.rs`, `crates/core-ffi/src/control.rs`, `tests/control.rs`, `app/Sources/VizDAWAppModel/InputFxCommands.swift`, `ControlCommands.swift`, `app/Tests/VizDAWAppModelTests/InputFxCommandsTests.swift` | 7 | the channel |
| `app/Sources/VizDAWAppModel/InputFxModel.swift`, `DocumentModel.swift`, `app/Tests/VizDAWAppModelTests/InputFxModelTests.swift` | 8 | the popover's model |
| `app/Sources/VizDAWEditors/ArrangementLayout.swift`, `LaneHeaderView.swift`, `app/Sources/VizDAW/InputFxPopover.swift`, `DocumentWindow.swift`, `app/Tests/VizDAWAppModelTests/LaneHeaderInputTests.swift`, `app/Tests/VizDAWEditorsTests/ArrangementLayoutTests.swift` | 9 | the FX button and popover |
| `crates/vizctl/tests/e2e.rs`, `app/README.md`, `docs/feature-backlog.md` | 10 | e2e, README 189–195, backlog |

---

### Task 1: The input effects model (`core-model`)

**Files:**
- Create: `crates/core-model/src/input_fx.rs`
- Modify: `crates/core-model/src/lib.rs`, `crates/core-model/src/lane.rs`, `crates/core-model/src/command/lane.rs`, `crates/core-model/src/rules.rs`
- Test: create `crates/core-model/tests/input_fx.rs`

**Interfaces:**
- Produces (`core_model::input_fx`):
  ```rust
  /// The most effects a lane's chain holds (spec §4.5, §6.8).
  pub const MAX_INPUT_FX: usize = 8;
  /// The most notes a Chord adds (spec §6.8).
  pub const MAX_CHORD_NOTES: usize = 6;
  /// The largest transpose or chord interval, in semitones.
  pub const MAX_SHIFT: i8 = 48;
  #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)] #[serde(deny_unknown_fields)]
  pub struct InputFx { pub kind: InputFxKind, #[serde(default, skip_serializing_if = "is_false")] pub bypass: bool }
  #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)] #[serde(rename_all = "snake_case", deny_unknown_fields)]
  pub enum InputFxKind {
      Arpeggiator(Arp),
      Transpose { semitones: i8 },
      Velocity(VelocityFx),
      Chord { intervals: Vec<i8> },
      InputQuantize { grid: Tick, strength: u8 },
  }
  #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)] #[serde(deny_unknown_fields)]
  pub struct Arp { pub mode: ArpMode, pub rate: NoteValue, pub octaves: u8, pub gate: u16,
      /// None: As Played; Some: Fixed 1–127.
      #[serde(default, skip_serializing_if = "Option::is_none")] pub velocity: Option<u8>,
      #[serde(default, skip_serializing_if = "is_false")] pub latch: bool }
  #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)] #[serde(rename_all = "snake_case")]
  pub enum ArpMode { Up, Down, UpDown, DownUp, AsPlayed, Random, Chord }
  #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)] #[serde(deny_unknown_fields)]
  pub struct NoteValue { pub division: u8, #[serde(default, skip_serializing_if = "Feel::is_straight")] pub feel: Feel }
  #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)] #[serde(rename_all = "snake_case")]
  pub enum Feel { #[default] Straight, Dotted, Triplet }
  #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)] #[serde(deny_unknown_fields)]
  pub struct VelocityFx { pub scale: u16, pub offset: i16, pub min: u8, pub max: u8,
      #[serde(default, skip_serializing_if = "Option::is_none")] pub fixed: Option<u8> }
  impl Default for Arp { /* Ruling 14 */ }
  impl Default for VelocityFx { /* Ruling 14 */ }
  impl Feel { pub fn is_straight(&self) -> bool; }
  impl NoteValue {
      /// 1/16 straight.
      pub const SIXTEENTH: NoteValue;
      /// `4 × PPQ / division`, × 3/2 dotted, × 2/3 triplet (Ruling 10).
      pub fn ticks(self) -> Tick;
  }
  impl InputFx { pub fn new(kind: InputFxKind) -> InputFx; }
  impl InputFxKind {
      /// "Arpeggiator", "Transpose", "Velocity", "Chord", "Input Quantize".
      pub fn name(&self) -> &'static str;
      /// Range checks (Ruling 14; spec §6.8's ranges).
      pub fn check(&self) -> crate::error::Result<()>;
  }
  /// The 15 Input Quantize grids (Ruling 12): for s in 2q, q, q/2, q/4, q/8 (q = PPQ): s, s × 3/2, s × 2/3, in that order.
  pub fn input_quantize_grids() -> [Tick; 15];
  /// Every effect's check, at most `MAX_INPUT_FX`, at most one Input Quantize.
  pub fn check_chain<'a>(chain: impl IntoIterator<Item = &'a InputFx>) -> crate::error::Result<()>;
  ```
  - Error fields:
    - `ModelError::range` with `"input_fx.len"`;
    - `"arpeggiator.rate"`, `"arpeggiator.octaves"`, `"arpeggiator.gate"`, `"arpeggiator.velocity"`;
    - `"transpose.semitones"`;
    - `"velocity.scale"`, `"velocity.offset"`, `"velocity.min"`, `"velocity.max"`, `"velocity.fixed"`;
    - `"chord.intervals"`;
    - `"input_quantize.grid"`, `"input_quantize.strength"`;
    - `ModelError::invalid("a chain holds one Input Quantize")`.
  - `Lane.input_fx: im::Vector<InputFx>`, with `#[serde(default, skip_serializing_if = "Vector::is_empty")]`, empty in `Lane::new`.
  - `LaneCmd::SetInputFx { seq: SeqId, lane: LaneId, chain: Vec<InputFx> }`, named "Set Input Effects". It runs `check_chain` and replaces the lane's chain.
  - `rules.rs` checks every lane's chain when a file loads, so a damaged chain makes the file Corrupt with the field named.

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-model/tests/input_fx.rs <<'VIZDAW_EOF'
//! MIDI input effects in the model (plan 5g-1 Task 1; sub-project 5 spec
//! §4.5, §6.8; Rulings 12, 14, 15): a lane's chain of up to 8 effects is
//! one undoable lane edit, checked by range, saved only when not empty.

use core_model::ModelError;
use core_model::command::{Command, LaneCmd, SequenceCmd, apply};
use core_model::document::Document;
use core_model::file::{VERSION, from_json, to_json};
use core_model::history::History;
use core_model::ids::{LaneId, SeqId};
use core_model::input_fx::{
    Arp, ArpMode, Feel, InputFx, InputFxKind, MAX_INPUT_FX, NoteValue, VelocityFx, check_chain,
    input_quantize_grids,
};
use core_model::selection::ViewFilter;
use core_model::time::{PPQ, Tick};
use serde_json::{Value, json};

fn doc() -> Document {
    apply(&Document::new(), &Command::Sequence(SequenceCmd::New { name: "S".into() }))
        .unwrap()
        .doc
}

fn set(chain: Vec<InputFx>) -> Command {
    Command::Lane(LaneCmd::SetInputFx { seq: SeqId(0), lane: LaneId(0), chain })
}

fn fx(kind: InputFxKind) -> InputFx {
    InputFx::new(kind)
}

fn transpose(s: i8) -> InputFx {
    fx(InputFxKind::Transpose { semitones: s })
}

#[test]
fn a_chain_is_one_undoable_lane_edit() {
    let mut h = History::new(doc());
    let f = ViewFilter::default();
    assert!(h.doc().lane(SeqId(0), LaneId(0)).unwrap().input_fx.is_empty());
    let chain = vec![transpose(12), fx(InputFxKind::Arpeggiator(Arp::default()))];
    h.execute(&set(chain.clone()), &f).unwrap();
    assert_eq!(h.undo_label(), Some("Set Input Effects"));
    let got: Vec<InputFx> = h.doc().lane(SeqId(0), LaneId(0)).unwrap().input_fx.iter().cloned().collect();
    assert_eq!(got, chain);
    h.undo().unwrap();
    assert!(h.doc().lane(SeqId(0), LaneId(0)).unwrap().input_fx.is_empty());
}

#[test]
fn defaults_follow_the_ruling() {
    let a = Arp::default();
    assert_eq!(
        (a.mode, a.rate, a.octaves, a.gate, a.velocity, a.latch),
        (ArpMode::Up, NoteValue { division: 16, feel: Feel::Straight }, 1, 50, None, false)
    );
    let v = VelocityFx::default();
    assert_eq!((v.scale, v.offset, v.min, v.max, v.fixed), (100, 0, 1, 127, None));
    assert_eq!(NoteValue::SIXTEENTH.ticks(), Tick(PPQ / 4));
}

#[test]
fn note_values_are_whole_ticks_from_1_1_to_1_64_triplet() {
    let nv = |division, feel| NoteValue { division, feel }.ticks().0;
    assert_eq!(nv(1, Feel::Straight), 4 * PPQ);
    assert_eq!(nv(4, Feel::Straight), PPQ);
    assert_eq!(nv(8, Feel::Dotted), PPQ * 3 / 4);
    assert_eq!(nv(8, Feel::Triplet), PPQ / 3);
    assert_eq!(nv(64, Feel::Triplet), 640);
    assert_eq!(nv(64, Feel::Dotted), 1440);
    let g = input_quantize_grids();
    assert_eq!(g[0], Tick(2 * PPQ));
    assert_eq!(g[1], Tick(3 * PPQ));
    assert_eq!(g[2], Tick(4 * PPQ / 3));
    assert_eq!(g[9], Tick(PPQ / 4));
    assert_eq!(g[14], Tick(PPQ / 12));
}

#[test]
fn hostile_chains_are_refused_and_change_nothing() {
    let d = doc();
    let arp = |f: &dyn Fn(&mut Arp)| {
        let mut a = Arp::default();
        f(&mut a);
        fx(InputFxKind::Arpeggiator(a))
    };
    let vel = |f: &dyn Fn(&mut VelocityFx)| {
        let mut v = VelocityFx::default();
        f(&mut v);
        fx(InputFxKind::Velocity(v))
    };
    let q = |grid: i64, strength: u8| fx(InputFxKind::InputQuantize { grid: Tick(grid), strength });
    let bad: Vec<Vec<InputFx>> = vec![
        vec![transpose(1); MAX_INPUT_FX + 1],
        vec![arp(&|a| a.octaves = 0)],
        vec![arp(&|a| a.octaves = 5)],
        vec![arp(&|a| a.gate = 4)],
        vec![arp(&|a| a.gate = 201)],
        vec![arp(&|a| a.velocity = Some(0))],
        vec![arp(&|a| a.velocity = Some(128))],
        vec![arp(&|a| a.rate.division = 3)],
        vec![arp(&|a| a.rate.division = 128)],
        vec![transpose(49)],
        vec![transpose(-49)],
        vec![vel(&|v| v.scale = 401)],
        vec![vel(&|v| v.offset = 128)],
        vec![vel(&|v| v.offset = -128)],
        vec![vel(&|v| v.min = 0)],
        vec![vel(&|v| v.max = 128)],
        vec![vel(&|v| { v.min = 90; v.max = 80 })],
        vec![vel(&|v| v.fixed = Some(0))],
        vec![fx(InputFxKind::Chord { intervals: vec![] })],
        vec![fx(InputFxKind::Chord { intervals: vec![1, 2, 3, 4, 5, 6, 7] })],
        vec![fx(InputFxKind::Chord { intervals: vec![0] })],
        vec![fx(InputFxKind::Chord { intervals: vec![4, 4] })],
        vec![fx(InputFxKind::Chord { intervals: vec![49] })],
        vec![q(1000, 100)],
        vec![q(0, 100)],
        vec![q(-3840, 100)],
        vec![q(3840, 101)],
        vec![q(3840, 100), q(1920, 50)],
    ];
    for chain in bad {
        let r = apply(&d, &set(chain.clone()));
        assert!(
            matches!(r, Err(ModelError::OutOfRange { .. } | ModelError::Invalid { .. })),
            "{chain:?}: {r:?}"
        );
        assert!(check_chain(&chain).is_err());
    }
    // The edges are fine.
    let ok = vec![
        arp(&|a| { a.octaves = 4; a.gate = 200; a.velocity = Some(127); a.rate.division = 64 }),
        arp(&|a| { a.octaves = 1; a.gate = 5; a.velocity = Some(1); a.rate.division = 1 }),
        transpose(48),
        transpose(-48),
        vel(&|v| { v.scale = 400; v.offset = -127; v.min = 5; v.max = 5; v.fixed = Some(127) }),
        fx(InputFxKind::Chord { intervals: vec![-48, -12, 3, 7, 12, 48] }),
        q(PPQ / 12, 0),
    ];
    assert!(apply(&d, &set(ok)).is_ok());
    // Unknown lanes are refused too.
    assert!(apply(
        &d,
        &Command::Lane(LaneCmd::SetInputFx { seq: SeqId(0), lane: LaneId(9), chain: vec![] })
    )
    .is_err());
}

fn lane_json(text: &str) -> Value {
    let v: Value = serde_json::from_str(text).unwrap();
    v["document"]["sequences"][0]["lanes"][0].clone()
}

#[test]
fn files_write_a_chain_only_when_set_and_read_it_back() {
    let d = doc();
    let text = to_json(&d).unwrap();
    assert!(lane_json(&text).get("input_fx").is_none());
    let mut a = Arp::default();
    a.latch = true;
    let chain = vec![
        fx(InputFxKind::Arpeggiator(a)),
        InputFx { kind: InputFxKind::Transpose { semitones: -5 }, bypass: true },
    ];
    let d2 = apply(&d, &set(chain)).unwrap().doc;
    let text = to_json(&d2).unwrap();
    assert_eq!(
        lane_json(&text)["input_fx"],
        json!([
            {"kind": {"arpeggiator": {"mode": "up", "rate": {"division": 16}, "octaves": 1, "gate": 50, "latch": true}}},
            {"kind": {"transpose": {"semitones": -5}}, "bypass": true}
        ])
    );
    assert_eq!(from_json(&text).unwrap(), d2);
    assert_eq!(VERSION, 3);
}

#[test]
fn a_damaged_chain_in_a_file_is_refused() {
    let text = to_json(&doc()).unwrap();
    let mut v: Value = serde_json::from_str(&text).unwrap();
    v["document"]["sequences"][0]["lanes"][0]["input_fx"] =
        json!([{"kind": {"arpeggiator": {"mode": "up", "rate": {"division": 16}, "octaves": 9, "gate": 50}}}]);
    match from_json(&serde_json::to_string(&v).unwrap()) {
        Err(ModelError::Corrupt { problems }) => {
            assert!(problems.iter().any(|p| p.contains("arpeggiator.octaves")), "{problems:?}")
        }
        other => panic!("{other:?}"),
    }
    v["document"]["sequences"][0]["lanes"][0]["input_fx"] = json!([{"kind": {"flanger": {}}}]);
    assert!(from_json(&serde_json::to_string(&v).unwrap()).is_err());
}
VIZDAW_EOF
```

- [ ] **Step 2: Run them to see them fail** with `cargo test -p core-model --test input_fx`. Expected: FAIL to compile, because `core_model::input_fx` is not found.

- [ ] **Step 3: Implement.**
   - Write `crates/core-model/src/input_fx.rs` with the Interfaces items. The module doc names spec §6.8 and Rulings 12 and 14. `is_false` is a private `fn is_false(b: &bool) -> bool`.
   - `InputFxKind::check` applies these ranges:
     - Arpeggiator: division ∈ {1, 2, 4, 8, 16, 32, 64}, octaves 1–4, gate 5–200, velocity 1–127 when set;
     - Transpose: |semitones| ≤ 48;
     - Velocity: scale ≤ 400, |offset| ≤ 127, min and max 1–127 with min ≤ max, fixed 1–127 when set;
     - Chord: 1–6 intervals, each non-zero, distinct, |i| ≤ 48;
     - Input Quantize: grid ∈ `input_quantize_grids()`, strength ≤ 100.

   In `crates/core-model/src/lib.rs`:

   Find:
   ```rust
   pub mod lane;
   ```
   Replace:
   ```rust
   pub mod input_fx;
   pub mod lane;
   ```

   In `crates/core-model/src/lane.rs`:

   Find:
   ```rust
       #[serde(default, skip_serializing_if = "MonitorMode::is_auto")]
       pub monitor: MonitorMode,
   }
   ```
   Replace:
   ```rust
       #[serde(default, skip_serializing_if = "MonitorMode::is_auto")]
       pub monitor: MonitorMode,
       /// The MIDI input effect chain (spec §4.5, §6.8): at most
       /// [`crate::input_fx::MAX_INPUT_FX`] effects, run on live input while
       /// the lane is armed or monitoring. Absent in files when empty (plan
       /// 5g-1).
       #[serde(default, skip_serializing_if = "Vector::is_empty")]
       pub input_fx: Vector<crate::input_fx::InputFx>,
   }
   ```

   Find:
   ```rust
               monitor: MonitorMode::Auto,
   ```
   Replace:
   ```rust
               monitor: MonitorMode::Auto,
               input_fx: Vector::new(),
   ```

   In `crates/core-model/src/command/lane.rs`:

   Find:
   ```rust
       /// Set when the lane's input is heard (spec §6.6).
   ```
   Replace:
   ```rust
       /// Set the lane's MIDI input effect chain (spec §6.8; plan 5g-1),
       /// checked by [`crate::input_fx::check_chain`].
       SetInputFx {
           /// Sequence.
           seq: SeqId,
           /// Lane.
           lane: LaneId,
           /// The chain, first effect first.
           chain: Vec<crate::input_fx::InputFx>,
       },
       /// Set when the lane's input is heard (spec §6.6).
   ```

   Find:
   ```rust
               LaneCmd::SetMonitor { .. } => "Set Lane Monitor",
   ```
   Replace:
   ```rust
               LaneCmd::SetMonitor { .. } => "Set Lane Monitor",
               LaneCmd::SetInputFx { .. } => "Set Input Effects",
   ```

   Find:
   ```rust
           LaneCmd::SetMonitor { seq, lane, monitor } => {
               d.lane_mut(*seq, *lane)?.monitor = *monitor;
           }
   ```
   Replace:
   ```rust
           LaneCmd::SetMonitor { seq, lane, monitor } => {
               d.lane_mut(*seq, *lane)?.monitor = *monitor;
           }
           LaneCmd::SetInputFx { seq, lane, chain } => {
               crate::input_fx::check_chain(chain)?;
               d.lane_mut(*seq, *lane)?.input_fx = chain.iter().cloned().collect();
           }
   ```

   In `crates/core-model/src/rules.rs`:

   Find:
   ```rust
               if let Some(i) = &l.input {
                   push(i.check(), &lctx);
               }
   ```
   Replace:
   ```rust
               if let Some(i) = &l.input {
                   push(i.check(), &lctx);
               }
               push(crate::input_fx::check_chain(&l.input_fx), &lctx);
   ```

   Any `match` over `LaneCmd` elsewhere in the workspace that does not compile gets the new arm. These are `edit.rs`'s `retries_input_cmd`, which uses `matches!` and so needs nothing, and any exhaustive match the compiler names. Chain edits never retry the input (Ruling 15).

- [ ] **Step 4: Run the tests and the gates.** Run `cargo test -p core-model`, both clippies and `cargo fmt --all -- --check`. Expected: core-model +6.

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-model/src/input_fx.rs crates/core-model/src/lib.rs crates/core-model/src/lane.rs crates/core-model/src/command/lane.rs crates/core-model/src/rules.rs crates/core-model/tests/input_fx.rs
git commit -m "core-model: MIDI input effect chains on lanes, one undoable edit (plan 5g-1 Task 1)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD
```

---

### Task 2: `LiveMsg`, the chain, Transpose, Velocity, Chord and the merge (`core-engine`)

**Files:**
- Create: `crates/core-engine/src/input_fx/mod.rs`
- Create (an empty stage for Task 3): `crates/core-engine/src/input_fx/arp.rs`, holding `pub(crate) struct ArpState;` and `impl ArpState { pub(crate) fn new(_: &Arp) -> Box<ArpState> }` so the chain compiles. In this task an Arpeggiator stage passes its input through unchanged.
- Modify: `crates/core-engine/src/lib.rs`
- Test: create `crates/core-engine/tests/input_fx.rs`

**Interfaces:**
- Consumes: `core_model::input_fx::*` (Task 1), `core_engine::midi1::{Midi1Msg, note_on_velocity, release_velocity, value7}`, `core_engine::input::on_channel`, `core_engine::schedule::TempoTable`, `core_model::lane::QuantizeSpec`.
- Produces (`core_engine::input_fx`):
  ```rust
  /// 32-bit centre (pitch bend, timbre).
  pub const CENTER: u32 = 0x8000_0000;
  /// Notes an arpeggiator holds; sounding notes; outputs per input event (Ruling 7).
  pub const MAX_HELD: usize = 32;
  pub const MAX_SOUNDING: usize = 128;
  pub const MAX_FANOUT: usize = 128;
  #[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum Expr { Bend, Pressure, Timbre }
  /// A note's expression at its start (5g-2 fills it from MPE; defaults otherwise).
  #[derive(Debug, Clone, Copy, PartialEq, Eq)] pub struct NoteExpr { pub bend: u32, pub pressure: u32, pub timbre: u32 }
  impl Default for NoteExpr { /* bend CENTER, pressure 0, timbre CENTER */ }
  /// A live input event (Ruling 4).
  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  pub enum LiveMsg {
      NoteOn { pitch: u8, velocity: u16, expr: NoteExpr },
      NoteOff { pitch: u8, velocity: u16 },
      PerNote { pitch: u8, expr: Expr, value: u32 },
      Channel(Midi1Msg),
  }
  impl LiveMsg {
      pub fn from_midi1(m: Midi1Msg) -> LiveMsg;            // RT
      pub fn to_midi1(&self, channel: u8) -> Option<Midi1Msg>; // RT
      /// A note-on at 7-bit velocity `v` (up-scaled) with default expression.
      pub fn on(pitch: u8, v: u8) -> LiveMsg;
      /// A note-off with release velocity 0.
      pub fn off(pitch: u8) -> LiveMsg;
      pub fn pitch(&self) -> Option<u8>;
      pub fn with_pitch(self, pitch: u8) -> LiveMsg;
  }
  /// MIDI 2.0 min-center-max up-scaling: 7 → 16 bits; `bits` → 32 bits.
  pub fn up7(v: u8) -> u16;
  pub fn up32(v: u32, bits: u32) -> u32;
  /// Where an output goes: thru at block offset `block`; the recorder at
  /// `record` (Ruling 11).
  #[derive(Debug, Clone, Copy, PartialEq, Eq)] pub struct At { pub block: u32, pub record: i64 }
  impl At { pub const NOW: At = At { block: 0, record: 0 }; }
  /// The block a tick covers (Ruling 10).
  pub struct ChainClock<'a> {
      pub frames: u32, pub sample_rate: f64,
      /// The tempo while stopped (free-running), BPM.
      pub bpm: f64,
      /// While playing: the schedule's tempo map and the block's first schedule frame.
      pub transport: Option<(&'a TempoTable, u64)>,
      /// Added to a generated output's block offset for the recorder: the graph latency in frames (Ruling 11).
      pub record_lead: i64,
  }
  /// The Input Quantize a recorded message carries (Ruling 12).
  #[derive(Debug, Clone, Copy, PartialEq, Eq)] pub struct RecordQuantize { pub grid: i64, pub strength: u8 }
  impl RecordQuantize { pub fn spec(self) -> QuantizeSpec; }   // QuantizeSpec::grid(Tick(grid), strength)
  /// A lane's input effect chain.
  pub struct InputChain { /* config: Box<[InputFx]>, state: Box<ChainState> */ }
  impl InputChain {
      /// Allocates; control thread only.
      pub fn new(fx: &[InputFx]) -> InputChain;
      pub fn config(&self) -> &[InputFx];
      pub fn process(&mut self, msg: LiveMsg, at: At, out: &mut dyn FnMut(At, LiveMsg)); // RT
      pub fn tick(&mut self, clock: &ChainClock<'_>, out: &mut dyn FnMut(At, LiveMsg));   // RT
      /// Note-offs at `at` for every note the chain sounds (ascending pitch), then forgets held and latched notes. // RT
      pub fn flush(&mut self, at: At, out: &mut dyn FnMut(At, LiveMsg));
      /// Takes `old`'s running state when the configs are equal (Ruling 5); else nothing. // RT
      pub fn carry_from(&mut self, old: &mut InputChain);
      /// The first Input Quantize that is not bypassed. // RT
      pub fn quantize(&self) -> Option<RecordQuantize>;
      /// Notes dropped for capacity since the last call. // RT
      pub fn take_dropped(&mut self) -> u64;
  }
  impl Default for InputChain   // the empty chain
  impl Clone for InputChain     // config cloned, fresh state (control thread)
  impl PartialEq for InputChain // configs only
  impl Eq for InputChain
  impl std::fmt::Debug for InputChain // config only
  ```
- Algorithm (the stages, which no signature determines):
  - `InputChain::new` builds one `Stage` per non-bypassed effect except Input Quantize: `Transpose(i8)`, `Velocity(VelocityFx)`, `Chord([i8; 6], u8)` or `Arp(Box<ArpState>)`. It also builds the merge (`[u8; 128]` counts), a chain-local frame counter and the dropped counter.
  - `process` runs `run(stages, msg, at)`. A stage maps the message and calls `run(rest, …)` once per output:
    - Transpose maps `pitch()` by `with_pitch`; Chord emits the original, then each interval;
    - a pitch outside 0–127 is dropped (Ruling 8);
    - Velocity changes `NoteOn` velocities only (Ruling 14).

    At the end of the stages, the merge applies Ruling 6, then calls `out`. Every call to `out` takes one from a budget of `MAX_FANOUT`; past it, outputs are dropped and counted.
  - `tick(clock)` gives each Arp stage `i` the clock and an emitter that runs `run(stages[i+1..], …)`. Each step gets a fresh fan-out budget. After the stages, the frame counter advances by `clock.frames`.
  - `PerNote` messages pass Transpose and Chord like notes and pass Velocity unchanged. `Channel` messages pass every stage unchanged.

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-engine/tests/input_fx.rs <<'VIZDAW_EOF'
//! The input effect chain's stateless effects and its output stage (plan
//! 5g-1 Task 2; spec §6.8, §12.3; Rulings 4–8, 12).

use core_engine::input_fx::{
    At, CENTER, Expr, InputChain, LiveMsg, MAX_FANOUT, NoteExpr, RecordQuantize, up7, up32,
};
use core_engine::midi1::Midi1Msg;
use core_model::input_fx::{InputFx, InputFxKind, VelocityFx};
use core_model::time::Tick;

fn chain(fx: Vec<InputFxKind>) -> InputChain {
    InputChain::new(&fx.into_iter().map(InputFx::new).collect::<Vec<_>>())
}

fn transpose(s: i8) -> InputFxKind {
    InputFxKind::Transpose { semitones: s }
}

fn chord(i: &[i8]) -> InputFxKind {
    InputFxKind::Chord { intervals: i.to_vec() }
}

/// What `c` outputs for `msgs`, as text: "on 60 v100", "off 60",
/// "pn 60 pressure 0x…", "ch b0 01 40".
fn feed(c: &mut InputChain, msgs: &[LiveMsg]) -> Vec<String> {
    let mut out = Vec::new();
    for m in msgs {
        c.process(*m, At::NOW, &mut |_, m| out.push(text(m)));
    }
    out
}

fn text(m: LiveMsg) -> String {
    match m {
        LiveMsg::NoteOn { pitch, velocity, .. } => format!("on {pitch} v{}", velocity >> 9),
        LiveMsg::NoteOff { pitch, .. } => format!("off {pitch}"),
        LiveMsg::PerNote { pitch, expr, value } => format!("pn {pitch} {expr:?} {value:#x}"),
        LiveMsg::Channel(m) => format!("ch {:02x} {:02x} {:02x}", m.bytes[0], m.bytes[1], m.bytes[2]),
    }
}

fn on_(pitch: u8, v: u8) -> LiveMsg {
    LiveMsg::on(pitch, v)
}

fn off_(pitch: u8) -> LiveMsg {
    LiveMsg::off(pitch)
}

#[test]
fn midi1_maps_to_live_messages_and_back() {
    let on = LiveMsg::from_midi1(Midi1Msg::three(0x93, 60, 100));
    assert_eq!(on, LiveMsg::NoteOn { pitch: 60, velocity: up7(100), expr: NoteExpr::default() });
    assert_eq!(up7(100), 0xC924);
    assert_eq!(up7(127), 0xFFFF);
    assert_eq!(up7(64), 0x8000);
    assert_eq!(on.to_midi1(5), Some(Midi1Msg::three(0x95, 60, 100)));
    assert_eq!(
        LiveMsg::from_midi1(Midi1Msg::three(0x90, 60, 0)),
        LiveMsg::NoteOff { pitch: 60, velocity: 0 }
    );
    let off = LiveMsg::from_midi1(Midi1Msg::three(0x80, 60, 64));
    assert_eq!(off.to_midi1(0), Some(Midi1Msg::three(0x80, 60, 64)));
    let poly = LiveMsg::from_midi1(Midi1Msg::three(0xA1, 61, 64));
    assert_eq!(poly, LiveMsg::PerNote { pitch: 61, expr: Expr::Pressure, value: up32(64, 7) });
    assert_eq!(poly.to_midi1(2), Some(Midi1Msg::three(0xA2, 61, 64)));
    let cc = LiveMsg::from_midi1(Midi1Msg::three(0xB4, 1, 64));
    assert_eq!(cc.to_midi1(9), Some(Midi1Msg::three(0xB9, 1, 64)));
    // Per-note bend and timbre have no MIDI 1.0 form (Ruling 4).
    assert_eq!(LiveMsg::PerNote { pitch: 60, expr: Expr::Bend, value: CENTER }.to_midi1(0), None);
    assert_eq!(LiveMsg::PerNote { pitch: 60, expr: Expr::Timbre, value: CENTER }.to_midi1(0), None);
    for v in 0..=127u8 {
        assert_eq!((up7(v) >> 9) as u8, v);
        assert_eq!((up32(u32::from(v), 7) >> 25) as u8, v);
    }
}

#[test]
fn an_empty_chain_passes_input_unchanged() {
    let mut c = InputChain::default();
    assert!(c.config().is_empty());
    let cc = LiveMsg::Channel(Midi1Msg::three(0xB0, 64, 127));
    assert_eq!(
        feed(&mut c, &[on_(60, 100), cc, off_(60)]),
        vec!["on 60 v100", "ch b0 40 7f", "off 60"]
    );
}

#[test]
fn transpose_moves_notes_and_their_per_note_messages() {
    let mut c = chain(vec![transpose(12)]);
    let pn = LiveMsg::PerNote { pitch: 60, expr: Expr::Pressure, value: 5 };
    assert_eq!(
        feed(&mut c, &[on_(60, 100), pn, off_(60)]),
        vec!["on 72 v100", "pn 72 Pressure 0x5", "off 72"]
    );
}

#[test]
fn out_of_range_pitches_are_dropped_not_folded() {
    let mut c = chain(vec![transpose(12)]);
    assert!(feed(&mut c, &[on_(120, 100), off_(120)]).is_empty());
    let mut c = chain(vec![transpose(-48)]);
    assert!(feed(&mut c, &[on_(40, 100), off_(40)]).is_empty());
    let mut c = chain(vec![chord(&[12, -12])]);
    assert_eq!(feed(&mut c, &[on_(120, 100)]), vec!["on 120 v100", "on 108 v100"]);
}

#[test]
fn chord_adds_its_intervals_and_ends_them() {
    let mut c = chain(vec![chord(&[4, 7])]);
    assert_eq!(
        feed(&mut c, &[on_(60, 90), off_(60)]),
        vec!["on 60 v90", "on 64 v90", "on 67 v90", "off 60", "off 64", "off 67"]
    );
}

#[test]
fn chain_order_matters_where_a_drop_applies() {
    let mut a = chain(vec![transpose(10), chord(&[-12])]);
    let mut b = chain(vec![chord(&[-12]), transpose(10)]);
    assert!(feed(&mut a, &[on_(120, 100)]).is_empty());
    assert_eq!(feed(&mut b, &[on_(120, 100)]), vec!["on 118 v100"]);
    // Velocity clamps are order-dependent too.
    let fixed = |v| InputFxKind::Velocity(VelocityFx { fixed: Some(v), ..VelocityFx::default() });
    let half = InputFxKind::Velocity(VelocityFx { scale: 50, ..VelocityFx::default() });
    assert_eq!(feed(&mut chain(vec![fixed(100), half.clone()]), &[on_(60, 10)]), vec!["on 60 v50"]);
    assert_eq!(feed(&mut chain(vec![half, fixed(100)]), &[on_(60, 10)]), vec!["on 60 v100"]);
}

#[test]
fn velocity_scales_offsets_clamps_or_fixes() {
    let v = |f: VelocityFx, vel: u8| feed(&mut chain(vec![InputFxKind::Velocity(f)]), &[on_(60, vel)]);
    let d = VelocityFx::default();
    assert_eq!(v(VelocityFx { scale: 50, ..d }, 100), vec!["on 60 v50"]);
    assert_eq!(v(VelocityFx { offset: 10, ..d }, 100), vec!["on 60 v110"]);
    assert_eq!(v(VelocityFx { offset: -127, ..d }, 100), vec!["on 60 v1"], "never 0: a note-on stays one");
    assert_eq!(v(VelocityFx { max: 100, ..d }, 120), vec!["on 60 v100"]);
    assert_eq!(v(VelocityFx { min: 20, ..d }, 1), vec!["on 60 v20"]);
    assert_eq!(v(VelocityFx { scale: 400, ..d }, 100), vec!["on 60 v127"]);
    assert_eq!(v(VelocityFx { scale: 0, min: 30, ..d }, 100), vec!["on 60 v30"]);
    assert_eq!(v(VelocityFx { fixed: Some(64), ..d }, 3), vec!["on 60 v64"]);
    // Note-offs are untouched.
    let mut c = chain(vec![InputFxKind::Velocity(VelocityFx { fixed: Some(64), ..d })]);
    assert_eq!(
        feed(&mut c, &[on_(60, 3), LiveMsg::NoteOff { pitch: 60, velocity: up7(99) }]).len(),
        2
    );
}

#[test]
fn a_bypassed_effect_passes_its_input() {
    let mut c = InputChain::new(&[InputFx { kind: transpose(12), bypass: true }]);
    assert_eq!(feed(&mut c, &[on_(60, 100)]), vec!["on 60 v100"]);
}

#[test]
fn a_pitch_two_sources_reach_sounds_once_and_ends_once() {
    let mut c = chain(vec![chord(&[12])]);
    assert_eq!(
        feed(&mut c, &[on_(60, 100), on_(72, 100), off_(60), off_(72)]),
        vec!["on 60 v100", "on 72 v100", "on 84 v100", "off 60", "off 72", "off 84"]
    );
    // An off for a note the chain never started is dropped (Ruling 6).
    assert!(feed(&mut c, &[off_(50)]).is_empty());
}

#[test]
fn flush_ends_every_sounding_note_once() {
    let mut c = chain(vec![chord(&[4, 7])]);
    feed(&mut c, &[on_(60, 100)]);
    let mut out = Vec::new();
    let at = At { block: 7, record: -3 };
    c.flush(at, &mut |a, m| out.push((a, text(m))));
    assert_eq!(
        out,
        vec![(at, "off 60".to_string()), (at, "off 64".to_string()), (at, "off 67".to_string())]
    );
    let mut again = Vec::new();
    c.flush(At::NOW, &mut |_, m| again.push(text(m)));
    assert!(again.is_empty(), "flushed once");
    assert!(feed(&mut c, &[off_(60)]).is_empty(), "the late key-up is dropped");
}

#[test]
fn fan_out_is_capped_and_counted() {
    let six = chord(&[1, 2, 3, 4, 5, 6]);
    let mut c = chain(vec![six.clone(), six.clone(), six.clone(), six]);
    let got = feed(&mut c, &[on_(30, 100)]);
    assert!(got.len() <= MAX_FANOUT, "{}", got.len());
    assert!(c.take_dropped() > 0);
    assert_eq!(c.take_dropped(), 0, "taken once");
}

#[test]
fn carry_takes_state_only_between_equal_chains() {
    let mut old = chain(vec![chord(&[4])]);
    feed(&mut old, &[on_(60, 100)]);
    let mut same = chain(vec![chord(&[4])]);
    assert_eq!(same, old);
    same.carry_from(&mut old);
    assert_eq!(feed(&mut same, &[off_(60)]), vec!["off 60", "off 64"], "it knew the notes");
    let mut other = chain(vec![chord(&[3])]);
    assert_ne!(other, old);
    other.carry_from(&mut old);
    assert!(feed(&mut other, &[off_(60)]).is_empty(), "a different chain starts fresh");
}

#[test]
fn quantize_is_the_first_active_input_quantize() {
    let q = |grid, strength, bypass| InputFx {
        kind: InputFxKind::InputQuantize { grid: Tick(grid), strength },
        bypass,
    };
    assert_eq!(InputChain::new(&[q(3840, 80, false)]).quantize(), Some(RecordQuantize { grid: 3840, strength: 80 }));
    assert_eq!(InputChain::new(&[q(3840, 80, true)]).quantize(), None);
    // Input Quantize never changes what plays (Ruling 12).
    let mut c = InputChain::new(&[q(3840, 100, false)]);
    assert_eq!(feed(&mut c, &[on_(60, 100)]), vec!["on 60 v100"]);
}
VIZDAW_EOF
```

- [ ] **Step 2: Run them to see them fail** with `cargo test -p core-engine --test input_fx`. Expected: FAIL to compile, because `core_engine::input_fx` is not found.

- [ ] **Step 3: Implement** the Interfaces items in `crates/core-engine/src/input_fx/mod.rs` (`mod arp;` inside), with the algorithm above. Then, in `crates/core-engine/src/lib.rs`:

   Find:
   ```rust
   pub mod input;
   ```
   Replace:
   ```rust
   pub mod input;
   pub mod input_fx;
   ```

- [ ] **Step 4: Run the tests and the gates.** Run `cargo test -p core-engine --test input_fx`, both clippies and fmt. Expected: core-engine +13.

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-engine/src/input_fx/mod.rs crates/core-engine/src/input_fx/arp.rs crates/core-engine/src/lib.rs crates/core-engine/tests/input_fx.rs
git commit -m "core-engine: live MIDI events and the input effect chain: transpose, velocity, chord, merge (plan 5g-1 Task 2)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD
```

---

### Task 3: The arpeggiator

**Files:**
- Modify: `crates/core-engine/src/input_fx/arp.rs` (replacing Task 2's empty stage), `crates/core-engine/src/input_fx/mod.rs` (the Arp stage calls it)
- Test: create `crates/core-engine/tests/input_fx_arp.rs`

**Interfaces:**
- Consumes: Task 2's `InputChain`, `ChainClock`, `At`, `LiveMsg`, `up7`, `MAX_HELD`, `MAX_SOUNDING`; `TempoTable::{tick_to_frame, frame_to_tick}`; `NoteValue::ticks`.
- Produces: `pub(crate) struct ArpState` with:
  - `pub(crate) fn new(cfg: &Arp) -> Box<ArpState>` (allocates);
  - `pub(crate) fn input(&mut self, msg: LiveMsg, at: At, now: u64, dropped: &mut u64, emit: &mut dyn FnMut(At, LiveMsg))` — `// RT`;
  - `pub(crate) fn tick(&mut self, clock: &ChainClock<'_>, now: u64, emit: &mut dyn FnMut(At, LiveMsg))` — `// RT`, emitting in non-decreasing block offset;
  - `pub(crate) fn flush(&mut self, at: At, emit: &mut dyn FnMut(At, LiveMsg))` — `// RT`.

  The public surface is unchanged: everything is reached through `InputChain`.
- State:
  - held notes `[(pitch, velocity); 32]` in press order, with a count;
  - the keys down (`u128`);
  - the step index;
  - sounding notes `[(pitch, off_at: u64); 128]` on the chain's frame counter;
  - the next free-running step (`f64` frames);
  - the earliest frame a step may play (the press frame);
  - the xorshift state.
- Behaviour:
  - `input` forwards `Channel` messages through `emit` at once and drops `PerNote` messages for held notes (Ruling 9's arpeggiator drops per-note expression).
  - A `NoteOn` past 32 held is dropped and counted.
  - `tick` merges pending gate ends and steps in time order. At equal offsets, offs go before ons.
  - Steps follow Rulings 9 and 10. Synced step frames come from `tick_to_frame(k × step)` against `transport.1`. Free-running steps come from the step's frames at `clock.bpm`.
  - Generated outputs carry `At { block: k, record: k as i64 + clock.record_lead }`.

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-engine/tests/input_fx_arp.rs <<'VIZDAW_EOF'
//! Arpeggiator golden tests (plan 5g-1 Task 3; spec §6.8, §12.3;
//! Rulings 9–11): every mode, rates with dotted and triplet, octaves,
//! gate, velocity, latch, synced to the transport and free-running, and
//! the held-note capacity. 120 BPM at 48 kHz: a 1/16 step is 6000 frames.

use core_engine::input_fx::{At, ChainClock, InputChain, LiveMsg};
use core_engine::schedule::TempoTable;
use core_model::input_fx::{Arp, ArpMode, Feel, InputFx, InputFxKind, NoteValue};
use core_model::tempo::TempoMap;
use core_model::time::Tick;

const SR: f64 = 48_000.0;
const BLOCK: u64 = 256;

fn arp(f: impl FnOnce(&mut Arp)) -> InputChain {
    let mut a = Arp::default();
    f(&mut a);
    InputChain::new(&[InputFx::new(InputFxKind::Arpeggiator(a))])
}

/// Runs `c` for `frames` frames in 256-frame blocks: `script` events at
/// their frames, then a tick. Playing: the transport runs from schedule
/// frame `from` at 120 BPM; stopped: free-running at 120 BPM. Outputs as
/// `(frame, "on 60 v100" | "off 60")`.
fn run(c: &mut InputChain, script: &[(u64, LiveMsg)], frames: u64, playing: bool, from: u64) -> Vec<(u64, String)> {
    let table = TempoTable::build(&TempoMap::default(), Tick(0), SR);
    let mut out = Vec::new();
    let mut b = 0;
    while b < frames {
        for (f, m) in script.iter().filter(|(f, _)| (b..b + BLOCK).contains(f)) {
            let k = (f - b) as u32;
            c.process(*m, At { block: k, record: i64::from(k) }, &mut |a, m| out.push((b + u64::from(a.block), text(m))));
        }
        let clock = ChainClock {
            frames: BLOCK as u32,
            sample_rate: SR,
            bpm: 120.0,
            transport: playing.then_some((&table, from + b)),
            record_lead: 0,
        };
        c.tick(&clock, &mut |a, m| out.push((b + u64::from(a.block), text(m))));
        b += BLOCK;
    }
    out
}

fn text(m: LiveMsg) -> String {
    match m {
        LiveMsg::NoteOn { pitch, velocity, .. } => format!("on {pitch} v{}", velocity >> 9),
        LiveMsg::NoteOff { pitch, .. } => format!("off {pitch}"),
        other => format!("{other:?}"),
    }
}

fn ons(out: &[(u64, String)]) -> Vec<(u64, u8)> {
    out.iter()
        .filter_map(|(f, t)| t.strip_prefix("on ").map(|r| (*f, r.split(' ').next().unwrap().parse().unwrap())))
        .collect()
}

fn pitches(out: &[(u64, String)]) -> Vec<u8> {
    ons(out).into_iter().map(|(_, p)| p).collect()
}

fn press(at: u64, ps: &[u8]) -> Vec<(u64, LiveMsg)> {
    ps.iter().map(|p| (at, LiveMsg::on(*p, 100))).collect()
}

#[test]
fn up_synced_steps_on_the_sixteenths_with_half_gates() {
    let mut c = arp(|_| {});
    let out = run(&mut c, &press(0, &[60, 64, 67]), 24_000, true, 0);
    assert_eq!(
        out,
        vec![
            (0, "on 60 v100".into()),
            (3000, "off 60".into()),
            (6000, "on 64 v100".into()),
            (9000, "off 64".into()),
            (12_000, "on 67 v100".into()),
            (15_000, "off 67".into()),
            (18_000, "on 60 v100".into()),
            (21_000, "off 60".into()),
        ]
    );
}

#[test]
fn every_mode_has_its_order() {
    let held = press(0, &[67, 60, 64]);
    let mode = |m: ArpMode, octaves: u8, n: u64| {
        let mut c = arp(|a| { a.mode = m; a.octaves = octaves; });
        pitches(&run(&mut c, &held, n * 6000, true, 0))
    };
    assert_eq!(mode(ArpMode::Up, 1, 4), vec![60, 64, 67, 60]);
    assert_eq!(mode(ArpMode::Down, 1, 4), vec![67, 64, 60, 67]);
    assert_eq!(mode(ArpMode::UpDown, 1, 6), vec![60, 64, 67, 64, 60, 64]);
    assert_eq!(mode(ArpMode::DownUp, 1, 6), vec![67, 64, 60, 64, 67, 64]);
    assert_eq!(mode(ArpMode::AsPlayed, 1, 4), vec![67, 60, 64, 67]);
    assert_eq!(mode(ArpMode::Up, 2, 7), vec![60, 64, 67, 72, 76, 79, 60]);
    assert_eq!(mode(ArpMode::Down, 2, 3), vec![79, 76, 72]);
    assert_eq!(mode(ArpMode::AsPlayed, 2, 6), vec![67, 60, 64, 79, 72, 76]);
    // Octaves past 127 are dropped from the pattern.
    let mut c = arp(|a| a.octaves = 4);
    assert_eq!(pitches(&run(&mut c, &press(0, &[100]), 3 * 6000, true, 0)), vec![100, 112, 124]);
    // Chord: every held note at each step.
    let mut c = arp(|a| a.mode = ArpMode::Chord);
    let o = ons(&run(&mut c, &held, 12_000, true, 0));
    assert_eq!(o, vec![(0, 60), (0, 64), (0, 67), (6000, 60), (6000, 64), (6000, 67)]);
}

#[test]
fn random_is_seeded_and_draws_from_the_pattern() {
    let held = press(0, &[60, 64, 67]);
    let draw = || {
        let mut c = arp(|a| a.mode = ArpMode::Random);
        pitches(&run(&mut c, &held, 64 * 6000, true, 0))
    };
    let a = draw();
    assert_eq!(a, draw(), "the same seed draws the same notes");
    assert_eq!(a.len(), 64);
    for p in [60, 64, 67] {
        assert!(a.contains(&p), "{a:?}");
    }
    assert!(a.iter().all(|p| [60, 64, 67].contains(p)));
}

#[test]
fn rates_include_dotted_and_triplet() {
    let step = |division, feel| {
        let mut c = arp(|a| a.rate = NoteValue { division, feel });
        let o = ons(&run(&mut c, &press(0, &[60]), 40_000, true, 0));
        o[1].0 - o[0].0
    };
    assert_eq!(step(16, Feel::Straight), 6000);
    assert_eq!(step(8, Feel::Triplet), 8000);
    assert_eq!(step(8, Feel::Dotted), 18_000);
    assert_eq!(step(32, Feel::Straight), 3000);
    assert_eq!(step(64, Feel::Triplet), 1000);
}

#[test]
fn a_synced_arp_waits_for_the_next_step() {
    let mut c = arp(|_| {});
    let o = ons(&run(&mut c, &press(1000, &[60, 64]), 13_000, true, 0));
    assert_eq!(o, vec![(6000, 60), (12_000, 64)]);
}

#[test]
fn a_locate_moves_the_synced_grid() {
    // The transport starts at schedule frame 4000: the next 1/16 is at
    // schedule frame 6000, 2000 frames into the run.
    let mut c = arp(|_| {});
    let o = ons(&run(&mut c, &press(0, &[60]), 9000, true, 4000));
    assert_eq!(o, vec![(2000, 60), (8000, 60)]);
}

#[test]
fn free_running_starts_at_once_at_the_project_tempo() {
    let mut c = arp(|_| {});
    let o = ons(&run(&mut c, &press(1000, &[60, 64]), 14_000, false, 0));
    assert_eq!(o, vec![(1000, 60), (7000, 64), (13_000, 60)]);
}

#[test]
fn gate_and_fixed_velocity() {
    let mut c = arp(|a| { a.gate = 25; a.velocity = Some(90); });
    let out = run(&mut c, &press(0, &[60]), 6000, true, 0);
    assert_eq!(out, vec![(0, "on 60 v90".into()), (1500, "off 60".into())]);
}

#[test]
fn a_long_gate_on_one_note_ends_before_it_retriggers() {
    let mut c = arp(|a| a.gate = 150);
    let out = run(&mut c, &press(0, &[60]), 12_001, true, 0);
    assert_eq!(
        out,
        vec![
            (0, "on 60 v100".into()),
            (6000, "off 60".into()),
            (6000, "on 60 v100".into()),
            (12_000, "off 60".into()),
            (12_000, "on 60 v100".into()),
        ]
    );
}

#[test]
fn releasing_stops_the_arp_and_latch_keeps_it() {
    let mut script = press(0, &[60, 64]);
    script.push((7000, LiveMsg::off(60)));
    script.push((7000, LiveMsg::off(64)));
    let mut c = arp(|_| {});
    assert_eq!(pitches(&run(&mut c, &script, 30_000, true, 0)), vec![60, 64]);
    let mut c = arp(|a| a.latch = true);
    // Latched: it goes on; a new key with none down starts a new set.
    script.push((24_100, LiveMsg::on(67, 100)));
    let p = pitches(&run(&mut c, &script, 42_000, true, 0));
    assert_eq!(p, vec![60, 64, 60, 64, 60, 67, 67, 67]);
}

#[test]
fn per_note_messages_of_held_notes_are_dropped_and_channel_messages_pass() {
    let mut c = arp(|_| {});
    let script = vec![
        (0, LiveMsg::on(60, 100)),
        (10, LiveMsg::PerNote { pitch: 60, expr: core_engine::input_fx::Expr::Pressure, value: 9 }),
        (20, LiveMsg::Channel(core_engine::midi1::Midi1Msg::three(0xB0, 64, 127))),
    ];
    let out = run(&mut c, &script, 256, true, 0);
    assert!(out.iter().all(|(_, t)| !t.contains("PerNote")), "{out:?}");
    assert!(out.iter().any(|(f, t)| *f == 20 && t.contains("Channel")), "{out:?}");
}

#[test]
fn the_33rd_held_note_is_dropped_and_counted() {
    let mut c = arp(|a| a.mode = ArpMode::Chord);
    let keys: Vec<u8> = (30..63).collect();
    assert_eq!(keys.len(), 33);
    let o = ons(&run(&mut c, &press(0, &keys), 256, true, 0));
    assert_eq!(o.len(), 32);
    assert!(!o.iter().any(|(_, p)| *p == 62), "the newest is the one dropped");
    assert_eq!(c.take_dropped(), 1);
}

#[test]
fn generated_notes_carry_the_record_lead() {
    let table = TempoTable::build(&TempoMap::default(), Tick(0), SR);
    let mut c = arp(|_| {});
    c.process(LiveMsg::on(60, 100), At { block: 0, record: -40 }, &mut |_, _| {});
    let mut got = Vec::new();
    let clock = ChainClock { frames: 256, sample_rate: SR, bpm: 120.0, transport: Some((&table, 0)), record_lead: 64 };
    c.tick(&clock, &mut |a, m| got.push((a, text(m))));
    assert_eq!(got, vec![(At { block: 0, record: 64 }, "on 60 v100".to_string())]);
}
VIZDAW_EOF
```

- [ ] **Step 2: Run them to see them fail** with `cargo test -p core-engine --test input_fx_arp`. Expected: FAIL; the Arp stage still passes its input through.

- [ ] **Step 3: Implement `ArpState`** with the Interfaces above. Then wire the Arp stage in `mod.rs`: `process` → `input`, `tick` → `tick`, `flush` → `flush`, and `carry_from` swaps its box. The latch test's expected pitches: the steps at 0, 6000, 12 000, 18 000 and 24 000 play 60, 64, 60, 64, 60. 67 is pressed at 24 100 with no key down, so it starts a new set, and its first step is the next one. The steps at 30 000, 36 000 and 42 000 (the last block runs to 42 240) play 67 three times. If the tree's block arithmetic gives one step more or fewer at the end, check the script against Ruling 10 and correct the expectation, and say so in the report.

- [ ] **Step 4: Run the tests and the gates.** Run `cargo test -p core-engine --test input_fx --test input_fx_arp`, both clippies and fmt. Expected: core-engine +13.

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-engine/src/input_fx/arp.rs crates/core-engine/src/input_fx/mod.rs crates/core-engine/tests/input_fx_arp.rs
git commit -m "core-engine: the arpeggiator, synced and free-running (plan 5g-1 Task 3)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD
```

---

### Task 4: The chain in the renderer and the recorder

**Files:**
- Modify: `crates/core-engine/src/input.rs`, `crates/core-engine/src/engine.rs`, `crates/core-engine/src/render.rs`, `crates/core-engine/src/record.rs`
- Modify (existing tests): `crates/core-engine/tests/live_input.rs`, `tests/record_tap.rs`, `tests/rt_alloc_record.rs`
- Test: create `crates/core-engine/tests/input_fx_live.rs` and `crates/core-engine/tests/rt_alloc_input_fx.rs`

**Interfaces:**
- Consumes: Tasks 2–3's `InputChain`, `ChainClock`, `At`, `LiveMsg`, `RecordQuantize`.
- Produces:
  - `MidiInputSpec` gains `pub fx: Vec<core_model::input_fx::InputFx>` (the lane's chain) and `pub skip_ports: Vec<u32>` (Ruling 3).
  - `MidiRoute.chain: core_engine::input_fx::InputChain`. The old `core_engine::input::InputChain` is removed.
  - `MidiRoute::matches` skips `skip_ports` when `spec.port` is `None`.
  - `pub fn carries(&self, old: &MidiRoute) -> bool` (Ruling 5).
  - `RecordedMidi { lane, run, msg: LiveMsg, quantize: Option<RecordQuantize> }`.
  - `RecordTap::midi(&mut self, lane: LaneId, offset: i64, msg: LiveMsg, quantize: Option<RecordQuantize>)` — `// RT`.
  - `Counters.midi_fx_dropped: u64`.
  - `Renderer::live_midi(&mut self, frames: u32)` (private) is called as `self.live_midi(frames)`. `fn emit_live(…)` is a private free function in `render.rs` that sends one output thru and to the tap.

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-engine/tests/input_fx_live.rs <<'VIZDAW_EOF'
//! The input effect chain in the renderer (plan 5g-1 Task 4; spec §6.6,
//! §6.8, §12.3; Rulings 2, 3, 5, 10, 11, 13): thru plays the chain's
//! output, arpeggiator steps land on their frames stopped and playing,
//! the recorder hears what thru played, a changed chain ends its notes
//! while an unchanged one plays on, and overflow is counted. Offline
//! backend and sinks only.

use core_engine::backend::AudioBackend;
use core_engine::engine::{Engine, EngineConfig};
use core_engine::fixtures::{Builder, Q};
use core_engine::input::{InputSpec, MidiIn, MidiInputSpec, port_key};
use core_engine::input_fx::LiveMsg;
use core_engine::midi1::Midi1Msg;
use core_engine::offline::{EventLog, FakeInput, OfflineBackend, OfflineSinks};
use core_engine::record::RecordRequest;
use core_model::document::Document;
use core_model::ids::{DestId, LaneId, SeqId};
use core_model::input_fx::{Arp, ArpMode, InputFx, InputFxKind};

const SR: f64 = 48_000.0;

struct Doc {
    doc: Document,
    seq: SeqId,
    keys: LaneId,
    pads: LaneId,
    synth: DestId,
}

/// Lanes "Keys" and "Pads" on the synth (channel 3); a note on Pads at
/// beat 8 gives the sequence its length.
fn doc() -> Doc {
    let mut b = Builder::new();
    let synth = b.dest("Synth", "builtin:synth", 3).unwrap();
    let (seq, keys) = b.sequence("Bar", "Keys", Some(synth)).unwrap();
    let pads = b.lane(seq, "Pads", Some(synth)).unwrap();
    b.notes(seq, pads, &[(8 * Q, 30, 0x8000, Q)]).unwrap();
    Doc { doc: b.finish(), seq, keys, pads, synth }
}

fn spec(d: &Doc, lane: LaneId, port: &str, fx: Vec<InputFx>, armed: bool) -> MidiInputSpec {
    MidiInputSpec {
        lane,
        port: Some(port_key(port)),
        channel: None,
        dest: Some(d.synth),
        thru: true,
        armed,
        fx,
        skip_ports: Vec::new(),
    }
}

fn arp(f: impl FnOnce(&mut Arp)) -> InputFx {
    let mut a = Arp::default();
    f(&mut a);
    InputFx::new(InputFxKind::Arpeggiator(a))
}

fn start(d: &Doc, midi: Vec<MidiInputSpec>) -> (Engine, OfflineBackend, EventLog) {
    let sinks = OfflineSinks::new(100_000);
    let log = sinks.log();
    let (mut engine, renderer) = Engine::new(EngineConfig::new(SR), Box::new(sinks)).unwrap();
    engine.set_inputs(InputSpec { midi, monitor: vec![] });
    engine.load(&d.doc, d.seq).unwrap();
    let mut b = OfflineBackend::new(SR, 256).with_input(FakeInput::Signals);
    b.start(renderer).unwrap();
    (engine, b, log)
}

fn send(engine: &Engine, port: &str, bytes: [u8; 3]) {
    assert!(engine.midi_sender().send(MidiIn {
        port: port_key(port),
        host_ns: 0,
        msg: Midi1Msg::three(bytes[0], bytes[1], bytes[2]),
    }));
}

/// The log's note-ons as `(frame, pitch)`.
fn ons(log: &EventLog) -> Vec<(u64, u8)> {
    log.drain()
        .into_iter()
        .filter(|r| r.bytes().len() == 3 && r.bytes()[0] & 0xF0 == 0x90 && r.bytes()[2] > 0)
        .map(|r| (r.frame, r.bytes()[1]))
        .collect()
}

#[test]
fn thru_plays_the_chains_output() {
    let d = doc();
    let fx = vec![InputFx::new(InputFxKind::Transpose { semitones: 12 })];
    let (engine, mut b, log) = start(&d, vec![spec(&d, d.keys, "Keys", fx, false)]);
    b.run_frames(512);
    log.drain();
    send(&engine, "Keys", [0x90, 64, 100]);
    b.run_frames(256);
    let got: Vec<(u64, Vec<u8>)> = log.drain().into_iter().map(|r| (r.frame, r.bytes().to_vec())).collect();
    assert_eq!(got, vec![(512, vec![0x93, 76, 100])]);
}

#[test]
fn arp_steps_play_at_their_frames_while_stopped() {
    let d = doc();
    let (engine, mut b, log) = start(&d, vec![spec(&d, d.keys, "Keys", vec![arp(|_| {})], false)]);
    b.run_frames(512);
    log.drain();
    send(&engine, "Keys", [0x90, 60, 100]);
    send(&engine, "Keys", [0x90, 64, 100]);
    b.run_frames(12_800);
    assert_eq!(ons(&log), vec![(512, 60), (6512, 64), (12_512, 60)]);
}

#[test]
fn arp_steps_follow_the_beat_while_playing() {
    let d = doc();
    let (mut engine, mut b, log) = start(&d, vec![spec(&d, d.keys, "Keys", vec![arp(|_| {})], false)]);
    engine.play().unwrap();
    b.run_frames(256);
    log.drain();
    send(&engine, "Keys", [0x90, 60, 100]);
    send(&engine, "Keys", [0x90, 64, 100]);
    b.run_frames(12_800);
    // The transport rolled at device frame 0: steps at the sixteenths after
    // the press (Ruling 10).
    assert_eq!(ons(&log), vec![(6000, 60), (12_000, 64)]);
}

#[test]
fn the_recorder_hears_what_thru_played() {
    let d = doc();
    let fx = vec![arp(|a| a.mode = ArpMode::UpDown)];
    let (mut engine, mut b, log) = start(&d, vec![spec(&d, d.keys, "Keys", fx, true)]);
    let mut h = engine
        .start_recording(&RecordRequest { channels: vec![], range: None, ring_frames: 4800 })
        .unwrap();
    engine.play().unwrap();
    b.run_frames(256);
    log.drain();
    for p in [60, 64, 67] {
        send(&engine, "Keys", [0x90, p, 100]);
    }
    b.run_frames(48_000);
    let thru: Vec<u8> = ons(&log).into_iter().map(|(_, p)| p).collect();
    let mut rec = Vec::new();
    while let Ok(m) = h.midi.pop() {
        assert_eq!(m.quantize, None);
        if let LiveMsg::NoteOn { pitch, .. } = m.msg {
            rec.push(pitch);
        }
    }
    assert_eq!(thru.len(), 7, "{thru:?}");
    assert_eq!(rec, thru);
}

#[test]
fn a_changed_chain_ends_its_notes_and_a_kept_one_plays_on() {
    let d = doc();
    let keys = spec(&d, d.keys, "Keys", vec![arp(|a| a.latch = true)], false);
    let pads = |s: i8| spec(&d, d.pads, "Pads", vec![InputFx::new(InputFxKind::Transpose { semitones: s })], false);
    let (mut engine, mut b, log) = start(&d, vec![keys.clone(), pads(12)]);
    b.run_frames(512);
    send(&engine, "Keys", [0x90, 60, 100]);
    send(&engine, "Keys", [0x80, 60, 0]);
    send(&engine, "Pads", [0x90, 50, 100]);
    b.run_frames(256);
    log.drain();
    // Pads' chain changes; Keys' (latched) does not.
    engine.set_inputs(InputSpec { midi: vec![keys, pads(5)], monitor: vec![] });
    engine.load(&d.doc, d.seq).unwrap();
    b.run_frames(12_288);
    let got: Vec<(u64, Vec<u8>)> = log.drain().into_iter().map(|r| (r.frame, r.bytes().to_vec())).collect();
    assert_eq!(
        got.iter().filter(|(_, m)| m == &vec![0x83, 62, 0]).count(),
        1,
        "Pads' transposed note ended once: {got:?}"
    );
    let steps: Vec<u64> = got.iter().filter(|(_, m)| m == &vec![0x93, 60, 100]).map(|(f, _)| *f).collect();
    assert_eq!(steps, vec![6512, 12_512], "the latched arpeggio played on");
}

#[test]
fn bypassing_while_held_leaves_no_note_hanging() {
    let d = doc();
    let chord = |bypass| InputFx { kind: InputFxKind::Chord { intervals: vec![4, 7] }, bypass };
    let (mut engine, mut b, log) = start(&d, vec![spec(&d, d.keys, "Keys", vec![chord(false)], false)]);
    b.run_frames(256);
    send(&engine, "Keys", [0x90, 60, 100]);
    b.run_frames(256);
    log.drain();
    engine.set_inputs(InputSpec { midi: vec![spec(&d, d.keys, "Keys", vec![chord(true)], false)], monitor: vec![] });
    engine.load(&d.doc, d.seq).unwrap();
    b.run_frames(256);
    send(&engine, "Keys", [0x80, 60, 0]);
    b.run_frames(256);
    let offs: Vec<Vec<u8>> = log.drain().iter().map(|r| r.bytes().to_vec()).filter(|m| m[0] & 0xF0 == 0x80).collect();
    assert_eq!(offs, vec![vec![0x83, 60, 0], vec![0x83, 64, 0], vec![0x83, 67, 0]]);
}

#[test]
fn surface_ports_are_skipped_by_every_source_lanes() {
    let d = doc();
    let mut s = spec(&d, d.keys, "Keys", vec![], false);
    s.port = None;
    s.skip_ports = vec![port_key("MCU In")];
    let (engine, mut b, log) = start(&d, vec![s]);
    b.run_frames(256);
    log.drain();
    send(&engine, "MCU In", [0x90, 0x5E, 0x7F]);
    send(&engine, "Keys", [0x90, 61, 100]);
    b.run_frames(256);
    assert_eq!(ons(&log), vec![(256, 61)]);
}

#[test]
fn overflow_reaches_the_counters() {
    let d = doc();
    let (engine, mut b, _log) = start(&d, vec![spec(&d, d.keys, "Keys", vec![arp(|_| {})], false)]);
    for p in 30..63u8 {
        send(&engine, "Keys", [0x90, p, 100]);
    }
    b.run_frames(256);
    assert_eq!(engine.counters().midi_fx_dropped, 1);
}
VIZDAW_EOF
```

Then write `crates/core-engine/tests/rt_alloc_input_fx.rs` whole. It follows `rt_alloc_record.rs`: the same `#[global_allocator] static ALLOC: CountingAlloc`, the same `armed_block` helper, and one test so no other thread allocates while counting:

```rust
//! The input effect chain never allocates on the render path (plan 5g-1
//! Task 4; spec §3, §12.3 "`rt-check` covers the chain"): a lane with an
//! arpeggiator (latched, Up-Down, 2 octaves), a chord, a transpose, a
//! velocity and an Input Quantize plays thru and records while notes
//! arrive, through a chain change (the flush), another lane's arming (the
//! carry), a stop and a new play.

use core_engine::backend::AudioBackend;
use core_engine::engine::{Engine, EngineConfig};
use core_engine::fixtures::{Builder, Q};
use core_engine::input::{InputSpec, MidiIn, MidiInputSpec, port_key};
use core_engine::midi1::Midi1Msg;
use core_engine::offline::{FakeInput, OfflineBackend, OfflineSinks};
use core_engine::record::RecordRequest;
use core_engine::rt_check::{self, CountingAlloc};
use core_model::input_fx::{Arp, ArpMode, InputFx, InputFxKind, VelocityFx};
use core_model::time::Tick;

#[global_allocator]
static ALLOC: CountingAlloc = CountingAlloc;

fn armed_block(b: &mut OfflineBackend, frames: u32) {
    rt_check::arm();
    b.block(frames);
    rt_check::disarm();
}

#[test]
fn the_input_chain_never_allocates() {
    let mut bld = Builder::new();
    let synth = bld.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, keys) = bld.sequence("Bar", "Keys", Some(synth)).unwrap();
    let pads = bld.lane(seq, "Pads", Some(synth)).unwrap();
    bld.notes(seq, pads, &[(0, 40, 0x8000, Q), (16 * Q, 41, 0x8000, Q)]).unwrap();
    let doc = bld.finish();
    let chain = |semis: i8| {
        let mut a = Arp::default();
        a.mode = ArpMode::UpDown;
        a.octaves = 2;
        a.latch = true;
        vec![
            InputFx::new(InputFxKind::Velocity(VelocityFx { scale: 120, ..VelocityFx::default() })),
            InputFx::new(InputFxKind::Chord { intervals: vec![3, 7, 10] }),
            InputFx::new(InputFxKind::Arpeggiator(a)),
            InputFx::new(InputFxKind::Transpose { semitones: semis }),
            InputFx::new(InputFxKind::InputQuantize { grid: Tick(Q / 4), strength: 100 }),
        ]
    };
    let lane = |lane, port: &str, fx| MidiInputSpec {
        lane, port: Some(port_key(port)), channel: None, dest: Some(synth),
        thru: true, armed: true, fx, skip_ports: Vec::new(),
    };
    let sinks = OfflineSinks::new(1 << 20).with_tone();
    let log = sinks.log();
    let (mut engine, renderer) = Engine::new(EngineConfig::new(48_000.0), Box::new(sinks)).unwrap();
    engine.set_inputs(InputSpec { midi: vec![lane(keys, "Keys", chain(0))], monitor: vec![] });
    engine.load(&doc, seq).unwrap();
    let midi = engine.midi_sender();
    let mut b = OfflineBackend::new(48_000.0, 512).with_input(FakeInput::Signals);
    b.start(renderer).unwrap();
    b.reserve(4096);
    let mut h = engine
        .start_recording(&RecordRequest { channels: vec![], range: None, ring_frames: 4800 })
        .unwrap();
    engine.play().unwrap();
    let before = rt_check::count();
    for i in 0..2_000u32 {
        armed_block(&mut b, 1 + (i * 37) % 1024);
        if i % 50 == 0 {
            log.drain();
            while h.midi.pop().is_ok() {}
        }
        if i % 9 == 0 {
            let on = i % 18 == 0;
            let msg = Midi1Msg::three(if on { 0x90 } else { 0x80 }, 48 + (i % 24) as u8, 100);
            midi.send(MidiIn { port: port_key("Keys"), host_ns: 0, msg });
        }
        if i == 500 {
            // The chain changes: it flushes.
            engine.set_inputs(InputSpec { midi: vec![lane(keys, "Keys", chain(5))], monitor: vec![] });
            engine.load(&doc, seq).unwrap();
        }
        if i == 800 {
            // Another lane arms: Keys' chain is carried.
            engine.set_inputs(InputSpec {
                midi: vec![lane(keys, "Keys", chain(5)), lane(pads, "Pads", vec![])],
                monitor: vec![],
            });
            engine.load(&doc, seq).unwrap();
        }
        if i == 1200 {
            engine.stop().unwrap();
        }
        if i == 1300 {
            engine.play().unwrap();
        }
    }
    assert_eq!(rt_check::count() - before, 0, "allocations in the input chain");
    assert!(engine.counters().midi_fx_dropped < u64::MAX);
}
```

If `OfflineBackend::reserve` or `Builder::notes`' tuple differs in the tree, follow `rt_alloc_record.rs`'s current calls and say so in the report.

- [ ] **Step 2: Run them to see them fail** with `cargo test -p core-engine --test input_fx_live` and `cargo test -p core-engine --features rt-check --test rt_alloc_input_fx`. Expected: FAIL to compile, because `MidiInputSpec` has no `fx`.

- [ ] **Step 3: Implement.**

   In `crates/core-engine/src/input.rs`, remove the placeholder chain:

   Find:
   ```rust
   /// A lane's MIDI input effect chain (spec §6.8, D26h). Plan 5g fills it
   /// with the built-in effects; until then it passes every message through
   /// unchanged, so thru and record hear the input as played.
   #[derive(Debug, Clone, Default, PartialEq, Eq)]
   pub struct InputChain;

   impl InputChain {
       /// Runs `msg` through the chain, handing every output to `out`. // RT
       pub fn process(&mut self, msg: Midi1Msg, out: &mut impl FnMut(Midi1Msg)) {
           out(msg);
       }
   }

   ```
   Replace with nothing (an empty string).

   Find:
   ```rust
       /// The lane is armed: it records while a tap rolls.
       pub armed: bool,
   }
   ```
   Replace:
   ```rust
       /// The lane is armed: it records while a tap rolls.
       pub armed: bool,
       /// The lane's input effect chain (spec §6.8; plan 5g-1).
       pub fx: Vec<core_model::input_fx::InputFx>,
       /// Sources an every-source lane does not hear: control surfaces'
       /// input ports (plan 5g-1 Ruling 3).
       pub skip_ports: Vec<u32>,
   }
   ```

   Find:
   ```rust
       /// The lane's input chain.
       pub chain: InputChain,
   }
   ```
   Replace:
   ```rust
       /// The lane's input chain (plan 5g-1).
       pub chain: crate::input_fx::InputChain,
   }
   ```

   Find:
   ```rust
           self.spec.port.is_none_or(|p| p == ev.port) && self.spec.channel.is_none_or(|c| c == ch)
       }
   ```
   Replace:
   ```rust
           let port = match self.spec.port {
               Some(p) => p == ev.port,
               None => !self.spec.skip_ports.contains(&ev.port),
           };
           port && self.spec.channel.is_none_or(|c| c == ch)
       }

       /// Whether this new route takes over `old`'s chain state (plan 5g-1
       /// Ruling 5): the same lane, source, channel, destination, thru,
       /// targets and chain; the arm may differ. // RT
       pub fn carries(&self, old: &MidiRoute) -> bool {
           let (a, b) = (&self.spec, &old.spec);
           a.lane == b.lane
               && a.port == b.port
               && a.channel == b.channel
               && a.dest == b.dest
               && a.thru == b.thru
               && self.targets == old.targets
               && self.dest_channel == old.dest_channel
               && self.chain == old.chain
       }
   ```

   In `crates/core-engine/src/engine.rs`:

   Find:
   ```rust
                       chain: crate::input::InputChain,
   ```
   Replace:
   ```rust
                       chain: crate::input_fx::InputChain::new(&m.fx),
   ```

   In `crates/core-engine/src/record.rs`, `RecordedMidi.msg` becomes `crate::input_fx::LiveMsg`, documented "(after the lane's input chain; plan 5g-1 Ruling 11)". Add `pub quantize: Option<crate::input_fx::RecordQuantize>`, documented "The chain's Input Quantize when it was played (Ruling 12)". `RecordTap::midi` takes `msg: LiveMsg, quantize: Option<RecordQuantize>` and pushes both. Drop the `Midi1Msg` import if it is now unused.

   In `crates/core-engine/src/render.rs`:
   - `Counters` gains `pub midi_fx_dropped: u64` ("Notes the input effect chains dropped for capacity (plan 5g-1 Ruling 7)"), and `Shared` gains `midi_fx_dropped: AtomicU64`. Each goes after its `midi_dropped` twin in the struct, `Default` and `counters()`; the Find excerpts are `pub midi_dropped: u64,`, `    midi_dropped: AtomicU64,`, `midi_dropped: AtomicU64::new(0),` and `midi_dropped: self.midi_dropped.load(Ordering::Relaxed),`. Add one `pub(crate) fn count_fx_dropped(&self, n: u64)` beside the other counters.
   - Find `self.live_midi();` and replace it with `self.live_midi(frames);`.
   - Replace the whole `live_midi` function (from its doc comment `/// Takes the queued MIDI input: each message a lane listens to goes` to its closing brace) with:

   ```rust
       /// Takes the queued MIDI input and runs every listening lane's input
       /// chain (spec §6.6, §6.8; plan 5g-1 Ruling 13): input goes through
       /// the chain, then every chain ticks (arpeggiator steps, gate ends).
       /// Each output plays thru at its block offset and, while a tap rolls
       /// and the lane is armed, is recorded: input at the run frame heard
       /// when it arrived, generated notes where they play (Ruling 11). // RT
       fn live_midi(&mut self, frames: u32) {
           let late_ns = self.cfg.midi_offset_ns + self.latency_ns();
           let sr = self.cfg.sample_rate;
           let block_ns = self.block_ns;
           let info = self.block_info();
           let lead = i64::try_from(self.latency).unwrap_or(0);
           let transport = self
               .schedule
               .as_deref()
               .filter(|_| info.playing)
               .map(|s| (&s.tempo, info.frame));
           let clock = crate::input_fx::ChainClock {
               frames,
               sample_rate: sr,
               bpm: info.tempo_bpm,
               transport,
               record_lead: lead,
           };
           let mut o = Out {
               sinks: &mut self.sinks,
               shared: &self.shared,
               block_ns,
               ns_per_frame: 1e9 / sr.max(1.0),
               late_ns,
               base: 0,
           };
           let thru = &mut self.thru;
           let mut tap = self.tap.as_deref_mut();
           let mut routes = self.inputs.as_deref_mut();
           for _ in 0..MIDI_IN_PER_BLOCK {
               let Ok(ev) = self.midi_in.pop() else {
                   break;
               };
               let Some(routes) = routes.as_deref_mut() else {
                   continue;
               };
               let offset = if ev.host_ns == 0 {
                   0
               } else {
                   let ns = i128::from(ev.host_ns) - i128::from(block_ns);
                   (ns as f64 * sr / 1e9).round() as i64
               };
               for r in routes.midi.iter_mut() {
                   if !r.matches(&ev) {
                       continue;
                   }
                   let msg = crate::input_fx::LiveMsg::from_midi1(ev.msg);
                   let at = crate::input_fx::At { block: 0, record: offset };
                   let crate::input::MidiRoute { spec, targets, dest_channel, chain } = r;
                   let q = chain.quantize();
                   chain.process(msg, at, &mut |at, m| {
                       emit_live(spec, *targets, *dest_channel, at, m, q, &mut o, thru, &mut tap)
                   });
               }
           }
           if let Some(routes) = routes {
               for r in routes.midi.iter_mut() {
                   let crate::input::MidiRoute { spec, targets, dest_channel, chain } = r;
                   let q = chain.quantize();
                   chain.tick(&clock, &mut |at, m| {
                       emit_live(spec, *targets, *dest_channel, at, m, q, &mut o, thru, &mut tap)
                   });
                   o.shared.count_fx_dropped(chain.take_dropped());
               }
           }
       }
   ```

   and add, beside `release_all`:

   ```rust
   /// One input-chain output: thru on the route's targets and channel at
   /// its block offset, and to the tap when the lane is armed (plan 5g-1
   /// Ruling 11). // RT
   #[allow(clippy::too_many_arguments)]
   fn emit_live(
       spec: &crate::input::MidiInputSpec,
       targets: [Option<TargetIdx>; 2],
       ch: u8,
       at: crate::input_fx::At,
       m: crate::input_fx::LiveMsg,
       q: Option<crate::input_fx::RecordQuantize>,
       o: &mut Out<'_>,
       thru: &mut ActiveNotes,
       tap: &mut Option<&mut RecordTap>,
   ) {
       if spec.thru {
           if let Some(msg) = m.to_midi1(ch) {
               for t in targets.iter().flatten() {
                   o.send(*t, at.block, msg.as_slice());
                   thru.observe(*t, msg.as_slice());
               }
           }
       }
       if spec.armed {
           if let Some(t) = tap.as_deref_mut() {
               t.midi(spec.lane, at.record, m, q);
           }
       }
   }
   ```

   - Replace the `EngineCmd::SetInputs(new)` arm with the carry (Ruling 5):

   ```rust
               EngineCmd::SetInputs(mut new) => {
                   // Chains whose lane and settings did not change keep
                   // their state; the others end their notes first (plan
                   // 5g-1 Ruling 5).
                   let late_ns = self.cfg.midi_offset_ns + self.latency_ns();
                   let mut o = Out {
                       sinks: &mut self.sinks,
                       shared: &self.shared,
                       block_ns: self.block_ns,
                       ns_per_frame: 0.0,
                       late_ns,
                       base: 0,
                   };
                   let mut kept: u64 = 0;
                   let mut tap = self.tap.as_deref_mut();
                   if let Some(old) = self.inputs.as_deref_mut() {
                       for r in old.midi.iter_mut() {
                           if let Some(n) = new.midi.iter_mut().find(|n| n.carries(r)) {
                               n.chain.carry_from(&mut r.chain);
                               for t in n.targets.iter().flatten() {
                                   kept |= 1u64.checked_shl(u32::from(t.0)).unwrap_or(0);
                               }
                           } else {
                               let crate::input::MidiRoute { spec, targets, dest_channel, chain } = r;
                               let q = chain.quantize();
                               chain.flush(crate::input_fx::At::NOW, &mut |at, m| {
                                   emit_live(spec, *targets, *dest_channel, at, m, q, &mut o, &mut self.thru, &mut tap)
                               });
                           }
                       }
                   }
                   // Notes thru started end, except on carried routes'
                   // targets: a lane disarmed or re-routed must not leave
                   // one hanging.
                   for t in 0..MAX_TARGETS {
                       let bit = u32::try_from(t).ok().and_then(|t| 1u64.checked_shl(t)).unwrap_or(0);
                       if kept & bit == 0 {
                           let target = TargetIdx(t as u8);
                           self.thru.release(target, |m| o.send(target, 0, m.as_slice()));
                       }
                   }
                   if let Some(old) = self.inputs.replace(new) {
                       if let Err(rtrb::PushError::Full(old)) = self.retire.push(Retired::Inputs(old))
                       {
                           // Unreachable: room was checked in `drain_commands`.
                           std::mem::forget(old);
                       }
                   }
               }
   ```

   If the borrow checker refuses `&mut self.thru` inside the closure while `o` borrows `self.sinks`, split the borrows as `live_midi` does (`let thru = &mut self.thru;` before the loop). Fix `use` lines, removing `on_channel` and `Midi1Msg` if they are unused.

   Update the existing tests:
   - The 7 `MidiInputSpec { … }` literals (6 in `tests/live_input.rs`, 1 in `tests/rt_alloc_record.rs`) gain `fx: Vec::new(), skip_ports: Vec::new(),`.
   - In `tests/live_input.rs`:
     - `m.msg.bytes[1]` (twice) becomes `m.msg.pitch().unwrap_or(0)`;
     - `assert_eq!(got[0].msg, note(true, 60));` becomes `assert_eq!(got[0].msg, LiveMsg::from_midi1(note(true, 60)));`, importing `core_engine::input_fx::LiveMsg`.
   - In `tests/record_tap.rs`, `t.midi(lane, k, msg(n))` becomes `t.midi(lane, k, LiveMsg::from_midi1(msg(n)), None)`, and `m.msg.bytes[1]` becomes `m.msg.pitch().unwrap_or(0)`.
   - A live-input test that expected `9n pp 00` thru for a velocity-0 note-on now expects `8n pp 00` (Ruling 4). Say which, if any, in the report.

- [ ] **Step 4: Run the tests and the gates.**
   - Run `cargo test --workspace`, both clippies and fmt.
   - Run the rt-check gates: `cargo clippy -p vizdaw-play --all-targets --features rt-check -- -D warnings` and `cargo test -p core-engine --features rt-check --test rt_alloc --test rt_alloc_graph --test rt_alloc_record --test rt_alloc_gestures --test rt_alloc_input_fx`.
   - `core-ffi` will not compile yet where it builds `MidiInputSpec` or reads `RecordedMidi.msg`. Add `fx: Vec::new(), skip_ports: Vec::new()` in `host::input_spec` (Task 6 fills them). Tasks 5–6 also change `passes.rs`; until Task 5 it maps `m.msg` with a temporary `match` that keeps the old behaviour. Note in the report that Task 5 replaces that `match`.
   - Expected: core-engine +9.

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-engine/src/input.rs crates/core-engine/src/engine.rs crates/core-engine/src/render.rs crates/core-engine/src/record.rs crates/core-engine/tests/input_fx_live.rs crates/core-engine/tests/rt_alloc_input_fx.rs crates/core-engine/tests/live_input.rs crates/core-engine/tests/record_tap.rs crates/core-engine/tests/rt_alloc_record.rs crates/core-ffi/src/host.rs crates/core-ffi/src/passes.rs
git commit -m "core-engine: input effect chains run in the renderer, feed thru and the recorder, carry across route changes (plan 5g-1 Task 4)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD
```

---

### Task 5: Recording the chain's output; Input Quantize on recorded starts (`core-ffi::passes`)

**Files:**
- Modify: `crates/core-ffi/src/passes.rs`
- Test: modify `crates/core-ffi/tests/passes.rs`

**Interfaces:**
- Consumes: `RecordedMidi { msg: LiveMsg, quantize }` (Task 4), `RecordQuantize::spec()` (Task 2).
- Produces:
  - `Heard { u, msg: LiveMsg, quantize: Option<RecordQuantize> }`.
  - `midi_events` reads `LiveMsg`:
    - `NoteOn` pairs with the next `NoteOff` of its pitch in the same pass. With a quantize, the note's tick is `spec.apply(tick)` and its duration is the unquantized one.
    - `PerNote { Pressure }` becomes `EventBody::PolyPressure { pitch, value }` at its tick.
    - `PerNote { Bend | Timbre }` is skipped here; plan 5g-2 attaches it to its note.
    - `Channel` goes through `body_of` as now.
  - Controllers never move.

- [ ] **Step 1: Write the failing tests.**
   - In `crates/core-ffi/tests/passes.rs`, import `core_engine::input_fx::{LiveMsg, RecordQuantize}`.
   - Change `fn midi(…)` to build `RecordedMidi { lane, run, msg: LiveMsg::from_midi1(msg), quantize: None }`, and the file's other `RecordedMidi { … }` literal the same way.
   - Then append:

```rust
fn midi_q(lane: LaneId, run: i64, msg: Midi1Msg, grid: i64, strength: u8) -> RecordedMidi {
    RecordedMidi { lane, run, msg: LiveMsg::from_midi1(msg), quantize: Some(RecordQuantize { grid, strength }) }
}

/// `(tick, duration)` of the notes of `pitches` in the pass's active
/// content of `lane`.
fn note_spans(p: &RecordPass, lane: LaneId, pitches: &[u8]) -> Vec<(i64, i64)> {
    p.lanes
        .iter()
        .filter(|l| l.lane == lane)
        .flat_map(|l| l.active.events.iter())
        .filter_map(|e| match &e.body {
            EventBody::Note { pitch, duration, .. } if pitches.contains(pitch) => Some((e.tick.0, duration.0)),
            _ => None,
        })
        .collect()
}

/// Plan 5g-1 Ruling 12: Input Quantize moves recorded starts toward the
/// grid by its strength and keeps each note's length; controllers stay.
#[test]
fn input_quantize_moves_recorded_starts_and_keeps_lengths() {
    let d = doc(None);
    let mut c = Case::new(&d, 0);
    c.midi_lanes = vec![d.keys];
    let r = |beats: f64| (beats * BEAT as f64) as i64;
    let g = Q / 4;
    c.midi = vec![
        midi_q(d.keys, r(0.3), on(60, 100), g, 100),
        midi_q(d.keys, r(0.8), off(60), g, 100),
        midi_q(d.keys, r(1.1), on(62, 100), g, 50),
        midi_q(d.keys, r(1.6), off(62), g, 50),
        midi(d.keys, r(2.1), Midi1Msg::three(0xB0, 1, 64)),
    ];
    let p = c.run().unwrap();
    assert_eq!(note_spans(&p, d.keys, &[60, 62]), vec![(Q / 4, Q / 2), (Q + Q / 20, Q / 2)]);
    let cc: Vec<i64> = p.lanes.iter().filter(|l| l.lane == d.keys)
        .flat_map(|l| l.active.events.iter())
        .filter(|e| matches!(e.body, EventBody::Cc { .. }))
        .map(|e| e.tick.0)
        .collect();
    assert_eq!(cc, vec![Q * 21 / 10]);
}

/// Poly pressure records as poly pressure (MIDI 2.0 per-note pressure;
/// plan 5g-1 Ruling 4).
#[test]
fn poly_pressure_records_as_poly_pressure() {
    let d = doc(None);
    let mut c = Case::new(&d, 0);
    c.midi_lanes = vec![d.keys];
    c.midi = vec![midi(d.keys, 12_000, Midi1Msg::three(0xA0, 60, 64))];
    let p = c.run().unwrap();
    let got: Vec<(i64, u8, u32)> = p.lanes.iter().filter(|l| l.lane == d.keys)
        .flat_map(|l| l.active.events.iter())
        .filter_map(|e| match e.body {
            EventBody::PolyPressure { pitch, value } => Some((e.tick.0, pitch, value)),
            _ => None,
        })
        .collect();
    assert_eq!(got, vec![(Q / 2, 60, 0x8000_0000)]);
}
```

   If `Case::new(&d, 0)`'s default `end_run` (4 beats) or the file's `on`/`off` helpers differ, use the file's names and say so in the report.

- [ ] **Step 2: Run them to see them fail** with `cargo test -p vizdaw-ffi --test passes`. Expected: the two new tests FAIL: the quantize is ignored, and poly pressure through Task 4's temporary `match` records as nothing.

- [ ] **Step 3: Implement** the Interfaces in `passes.rs`. The note body's velocity and release velocity are the `LiveMsg`'s 16-bit values; there is no up-scaling, since `from_midi1` already up-scaled them. Remove Task 4's temporary `match`.

- [ ] **Step 4: Run the tests and the gates.** Run `cargo test -p vizdaw-ffi --test passes --test record`, both clippies and fmt. The existing MIDI pass tests still pass, with the same velocities. Expected: core-ffi +2.

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-ffi/src/passes.rs crates/core-ffi/tests/passes.rs
git commit -m "core-ffi: passes record the chain's live events; Input Quantize moves recorded starts only (plan 5g-1 Task 5)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD
```

---

### Task 6: The FFI: rows, the lane edit, the spec the engine gets, the counter

**Files:**
- Create: `crates/core-ffi/src/input_fx.rs`
- Modify: `crates/core-ffi/src/lib.rs` (`pub mod input_fx;` and re-exports as the crate does for its other rows), `crates/core-ffi/src/edit.rs` (`LaneEdit::SetInputFx`, `lane_cmd`), `crates/core-ffi/src/snapshot.rs` (`LaneRow.input_fx`), `crates/core-ffi/src/host.rs` (`input_spec`, `pub fn counters(owner: u64) -> Option<core_engine::render::Counters>`), `crates/core-ffi/src/live.rs` (`InputInfoRow.midi_fx_dropped`), `crates/core-ffi/src/session.rs` (`input_info` fills it)
- Test: create `crates/core-ffi/tests/input_fx.rs`; modify `crates/core-ffi/tests/record.rs`

**Interfaces:**
- Consumes: Task 1's model, Task 4's `MidiInputSpec.fx`/`skip_ports` and `Counters.midi_fx_dropped`, 5f-1's `crate::surfaces::surface_list() -> Vec<SurfaceRow>` (its `input` port names).
- Produces (`vizdaw_ffi::input_fx`):
  ```rust
  #[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)] pub enum ArpModeRow { Up, Down, UpDown, DownUp, AsPlayed, Random, Chord }
  #[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)] pub enum FeelRow { Straight, Dotted, Triplet }
  #[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
  pub enum InputFxKindRow {
      Arpeggiator { mode: ArpModeRow, division: u8, feel: FeelRow, octaves: u8, gate: u16, velocity: Option<u8>, latch: bool },
      Transpose { semitones: i8 },
      Velocity { scale: u16, offset: i16, min: u8, max: u8, fixed: Option<u8> },
      Chord { intervals: Vec<i8> },
      InputQuantize { grid: i64, strength: u8 },
  }
  #[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)] pub struct InputFxRow { pub kind: InputFxKindRow, pub bypass: bool }
  impl From<&InputFx> for InputFxRow; impl From<&InputFxRow> for InputFx;
  /// `chain` as `lane.set_input_fx` and `state.get` spell it (Ruling 14); `Model` for anything serde refuses or past 64 KiB.
  #[uniffi::export] pub fn input_fx_from_json(text: String) -> Result<Vec<InputFxRow>, VizError>;
  /// The JSON value of `chain` (for `control.rs`).
  pub fn input_fx_json(chain: &[InputFxRow]) -> serde_json::Value;
  ```
  - `LaneEdit::SetInputFx { seq: u64, lane: u64, chain: Vec<InputFxRow> }` maps to `LaneCmd::SetInputFx`.
  - `LaneRow.input_fx: Vec<InputFxRow>` is `#[uniffi(default)]`.
  - `InputInfoRow.midi_fx_dropped: u64` is `#[uniffi(default)]`, from `host::counters(id)`.
  - `host::input_spec` fills `fx: l.input_fx.iter().cloned().collect()` and `skip_ports`, the `port_key`s of `crate::surfaces::surface_list()`'s `input` names (Ruling 3).

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-ffi/tests/input_fx.rs <<'VIZDAW_EOF'
//! Input effects through a session (plan 5g-1 Task 6; spec §6.8, §12.1):
//! the chain is one undoable lane edit shown in the lane rows; the
//! model refuses hostile chains; the channel's JSON round-trips and
//! hostile JSON is refused; dropped notes are reported.

#![recursion_limit = "256"]

mod common;

use core_engine::offline::FakeInput;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};
use vizdaw_ffi::host::{force_offline, reset_host, set_fake_input};
use vizdaw_ffi::input_fx::{ArpModeRow, FeelRow, InputFxKindRow, InputFxRow, input_fx_from_json, input_fx_json};
use vizdaw_ffi::{EditCommand, LaneEdit, LaneInputRow, LaneRow, Session, VizError};

static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    force_offline();
    set_fake_input(Some(FakeInput::Signals));
    reset_host();
    g
}

fn song(s: &Session) -> u64 {
    s.snapshot().unwrap().sequences.iter().find(|r| r.name == "Song").unwrap().id
}

fn lane(s: &Session, seq: u64, name: &str) -> LaneRow {
    s.lanes(seq).unwrap().into_iter().find(|l| l.name == name).unwrap()
}

fn set(s: &Session, seq: u64, lane: u64, chain: Vec<InputFxRow>) -> Result<(), VizError> {
    s.edit(EditCommand::Lane { edit: LaneEdit::SetInputFx { seq, lane, chain } }, None).map(|_| ())
}

fn row(kind: InputFxKindRow) -> InputFxRow {
    InputFxRow { kind, bypass: false }
}

fn arp() -> InputFxKindRow {
    InputFxKindRow::Arpeggiator {
        mode: ArpModeRow::Up, division: 16, feel: FeelRow::Straight, octaves: 1, gate: 50, velocity: None, latch: false,
    }
}

#[test]
fn input_effects_are_one_undoable_lane_edit() {
    let _g = serial();
    let s = Session::new_demo().unwrap();
    let seq = song(&s);
    let chords = lane(&s, seq, "Chords").id;
    assert!(lane(&s, seq, "Chords").input_fx.is_empty());
    let chain = vec![row(InputFxKindRow::Transpose { semitones: 12 }), InputFxRow { kind: arp(), bypass: true }];
    set(&s, seq, chords, chain.clone()).unwrap();
    assert_eq!(lane(&s, seq, "Chords").input_fx, chain);
    assert_eq!(s.snapshot().unwrap().undo_label.as_deref(), Some("Set Input Effects"));
    s.undo().unwrap();
    assert!(lane(&s, seq, "Chords").input_fx.is_empty());
    s.close().unwrap();
}

#[test]
fn hostile_chains_are_refused_by_the_model() {
    let _g = serial();
    let s = Session::new_demo().unwrap();
    let seq = song(&s);
    let chords = lane(&s, seq, "Chords").id;
    let with_arp = |f: &dyn Fn(&mut InputFxKindRow)| {
        let mut k = arp();
        f(&mut k);
        vec![row(k)]
    };
    for bad in [
        vec![row(InputFxKindRow::Transpose { semitones: 1 }); 9],
        with_arp(&|k| if let InputFxKindRow::Arpeggiator { octaves, .. } = k { *octaves = 5 }),
        with_arp(&|k| if let InputFxKindRow::Arpeggiator { gate, .. } = k { *gate = 4 }),
        with_arp(&|k| if let InputFxKindRow::Arpeggiator { division, .. } = k { *division = 3 }),
        vec![row(InputFxKindRow::Transpose { semitones: 49 })],
        vec![row(InputFxKindRow::Chord { intervals: vec![0] })],
        vec![row(InputFxKindRow::Chord { intervals: vec![1, 2, 3, 4, 5, 6, 7] })],
        vec![row(InputFxKindRow::Velocity { scale: 100, offset: 0, min: 90, max: 80, fixed: None })],
        vec![row(InputFxKindRow::InputQuantize { grid: 1000, strength: 100 })],
        vec![
            row(InputFxKindRow::InputQuantize { grid: 3840, strength: 100 }),
            row(InputFxKindRow::InputQuantize { grid: 1920, strength: 100 }),
        ],
    ] {
        let r = set(&s, seq, chords, bad.clone());
        assert!(matches!(r, Err(VizError::Model { .. })), "{bad:?}: {r:?}");
    }
    assert!(lane(&s, seq, "Chords").input_fx.is_empty());
    s.close().unwrap();
}

#[test]
fn chain_json_round_trips() {
    let chain = vec![
        row(arp()),
        InputFxRow { kind: InputFxKindRow::Velocity { scale: 120, offset: -3, min: 10, max: 120, fixed: None }, bypass: true },
        row(InputFxKindRow::Chord { intervals: vec![4, 7] }),
        row(InputFxKindRow::InputQuantize { grid: 3840, strength: 75 }),
    ];
    let text = input_fx_json(&chain).to_string();
    assert_eq!(input_fx_from_json(text).unwrap(), chain);
    assert_eq!(
        input_fx_json(&[row(InputFxKindRow::Transpose { semitones: -5 })]),
        serde_json::json!([{"kind": {"transpose": {"semitones": -5}}}])
    );
}

#[test]
fn hostile_chain_json_is_refused() {
    for bad in [
        String::new(),
        "{".into(),
        "{}".into(),
        "[1]".into(),
        r#"[{"kind": {"flanger": {}}}]"#.into(),
        r#"[{"kind": {"transpose": {"semitones": 300}}}]"#.into(),
        r#"[{"kind": {"transpose": {"semitones": 1}}, "extra": 1}]"#.into(),
        format!("[{}]", vec![r#"{"kind": {"transpose": {"semitones": 1}}}"#; 5000].join(",")),
    ] {
        let r = input_fx_from_json(bad.clone());
        assert!(matches!(r, Err(VizError::Model { .. })), "{}: {r:?}", &bad[..bad.len().min(60)]);
    }
}

#[test]
fn midi_fx_dropped_is_reported() {
    let _g = serial();
    let s = Session::new_demo().unwrap();
    let seq = song(&s);
    let chords = lane(&s, seq, "Chords").id;
    s.edit(
        EditCommand::Lane {
            edit: LaneEdit::SetInput { seq, lane: chords, input: Some(LaneInputRow::Midi { port: Some("Keys".into()), channel: None }) },
        },
        None,
    )
    .unwrap();
    set(&s, seq, chords, vec![row(arp())]).unwrap();
    s.edit(
        EditCommand::Lane { edit: LaneEdit::SetFlags { seq, lane: chords, mute: None, solo: None, arm: Some(true) } },
        None,
    )
    .unwrap();
    s.play(seq).unwrap();
    let until = Instant::now() + common::wait_limit();
    while !s.transport().unwrap().playing {
        assert!(Instant::now() < until, "never played");
        std::thread::sleep(Duration::from_millis(2));
    }
    s.stop().unwrap();
    let keys: Vec<u8> = (30..63u8).flat_map(|p| [0x90, p, 100]).collect();
    assert!(s.inject_midi("Keys".into(), keys).unwrap());
    while s.input_info().unwrap().midi_fx_dropped < 1 {
        assert!(Instant::now() < until, "the drop was never counted");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(s.input_info().unwrap().midi_fx_dropped, 1);
    s.close().unwrap();
}
VIZDAW_EOF
```

   Then append to `crates/core-ffi/tests/record.rs` (it uses that file's `serial`, `untitled`, `doc`, `spec`, `wait_until`, `notes` and `Q`):

```rust
/// Plan 5g-1 (spec §6.8, §12.3): a chord held into an armed lane with an
/// arpeggiator while recording is recorded as the arpeggio: what thru
/// played, on the sixteenths, each a half step long.
#[test]
fn an_arpeggiated_chord_is_recorded_as_thru_played_it() {
    use core_model::input_fx::{Arp, InputFx, InputFxKind};
    let g = serial(Some(FakeInput::Signals));
    let d = doc(2, true, false);
    let mut b = Builder::from_doc(d.doc.clone());
    b.run(Command::Lane(LaneCmd::SetInputFx {
        seq: d.seq,
        lane: d.keys,
        chain: vec![InputFx::new(InputFxKind::Arpeggiator(Arp::default()))],
    }))
    .unwrap();
    let s = untitled(&g, b.finish());
    s.record(d.seq.0, spec()).unwrap();
    wait_until("beat 1", || s.transport().unwrap().tick >= Q);
    for p in [60u8, 64, 67] {
        assert!(s.inject_midi("Keys".into(), vec![0x90, p, 100]).unwrap());
    }
    wait_until("beat 3", || s.transport().unwrap().tick >= 3 * Q);
    for p in [60u8, 64, 67] {
        assert!(s.inject_midi("Keys".into(), vec![0x80, p, 0]).unwrap());
    }
    wait_until("beat 4", || s.transport().unwrap().tick >= 4 * Q);
    s.stop().unwrap();
    let got: Vec<(i64, u8)> = notes(&s, d.seq.0, d.keys.0)
        .into_iter()
        .filter(|(t, p)| *t >= Q && [60, 64, 67].contains(p))
        .collect();
    assert!(got.len() >= 6, "{got:?}");
    let first = [60u8, 64, 67].iter().position(|p| *p == got[0].1).unwrap();
    for (k, (tick, pitch)) in got.iter().enumerate() {
        assert_eq!(*pitch, [60, 64, 67][(first + k) % 3], "{got:?}");
        let off = tick % (Q / 4);
        assert!(off.min(Q / 4 - off) <= 2, "{tick} is not on a sixteenth: {got:?}");
    }
    let durations: Vec<i64> = s.lane_events(d.seq.0, d.keys.0).unwrap().events.iter()
        .filter(|e| e.tick >= Q && [60, 64, 67].contains(&e.pitch))
        .map(|e| e.duration)
        .collect();
    assert!(durations.iter().all(|t| (t - Q / 8).abs() <= 2), "{durations:?}");
    s.close().unwrap();
}
```

   If `EventRow`'s duration field has another name in the tree (for example `length`), use it and say so in the report.

- [ ] **Step 2: Run them to see them fail** with `cargo test -p vizdaw-ffi --test input_fx --test record`. Expected: FAIL to compile, because `vizdaw_ffi::input_fx` is not found.

- [ ] **Step 3: Implement.**
   - **`input_fx.rs`:** implement the Interfaces. `input_fx_from_json` refuses text over 65 536 bytes. It parses `Vec<core_model::input_fx::InputFx>` with serde and converts it to rows; a serde error becomes `VizError::Model { message }`. The range checks happen at the edit (the model), so the JSON reader stays a parser.
   - **`edit.rs`:** in `LaneEdit`, after `SetMonitor`, add:
     ```rust
     /// Set the lane's MIDI input effect chain (plan 5g-1).
     SetInputFx {
         /// Sequence.
         seq: u64,
         /// Lane.
         lane: u64,
         /// The chain, first effect first.
         chain: Vec<crate::input_fx::InputFxRow>,
     },
     ```
     In `lane_cmd`:

     Find:
     ```rust
             LaneEdit::SetMonitor { seq, lane, monitor } => LaneCmd::SetMonitor {
     ```
     Replace:
     ```rust
             LaneEdit::SetInputFx { seq, lane, chain } => LaneCmd::SetInputFx {
                 seq: SeqId(*seq),
                 lane: LaneId(*lane),
                 chain: chain.iter().map(Into::into).collect(),
             },
             LaneEdit::SetMonitor { seq, lane, monitor } => LaneCmd::SetMonitor {
     ```
   - **`snapshot.rs`:**

     Find:
     ```rust
         /// When its input is heard (plan 5c).
         pub monitor: crate::edit::MonitorModeRow,
     }
     ```
     Replace:
     ```rust
         /// When its input is heard (plan 5c).
         pub monitor: crate::edit::MonitorModeRow,
         /// Its MIDI input effect chain (plan 5g-1).
         #[uniffi(default)]
         pub input_fx: Vec<crate::input_fx::InputFxRow>,
     }
     ```

     Find:
     ```rust
                 monitor: l.monitor.into(),
             }
     ```
     Replace:
     ```rust
                 monitor: l.monitor.into(),
                 input_fx: l.input_fx.iter().map(Into::into).collect(),
             }
     ```
   - **`host.rs` `input_spec`:** the `MidiInputSpec { … armed: l.arm, }` push gains `fx: l.input_fx.iter().cloned().collect(), skip_ports: skip.clone(),`. Compute `skip` once before the loop as `crate::surfaces::surface_list().iter().map(|s| port_key(&s.input)).collect::<Vec<u32>>()`. `host::counters(owner)` follows `input_levels(owner)`: the owner's engine `counters()`, or `None`.
   - **`live.rs` and `session.rs`:** `InputInfoRow` gains `#[uniffi(default)] pub midi_fx_dropped: u64`. Both branches of `Session::input_info` fill it: `host::counters(self.id).map_or(0, |c| c.midi_fx_dropped)`, or 0. Other `InputInfoRow { … }` literals gain `midi_fx_dropped: 0`.

- [ ] **Step 4: Run the tests and the gates.**
   - Run `cargo test --workspace`, both clippies, `cargo clippy -p core-ffi --lib -- -D warnings` and fmt.
   - Then run `scripts/build-ffi.sh`, `scripts/swift.sh build` and `scripts/swift.sh test`. Swift compiles unchanged: the new fields have defaults, and no Swift `switch` over `LaneEdit` is exhaustive without `default`. If one is, add the case and say so.
   - Expected: core-ffi +6.

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-ffi/src/input_fx.rs crates/core-ffi/src/lib.rs crates/core-ffi/src/edit.rs crates/core-ffi/src/snapshot.rs crates/core-ffi/src/host.rs crates/core-ffi/src/live.rs crates/core-ffi/src/session.rs crates/core-ffi/tests/input_fx.rs crates/core-ffi/tests/record.rs
git commit -m "core-ffi: input effect rows, the lane edit, the chain in the engine's input spec, the dropped-note count (plan 5g-1 Task 6)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD
```

---

### Task 7: The control channel: `lane.set_input_fx`, `state.get` per-lane `input_fx`, `midi_fx_dropped`

**Files:**
- Modify: `crates/core-control/src/registry.rs`, `crates/core-control/tests/commands.rs`, `crates/core-ffi/src/control.rs`, `crates/core-ffi/tests/control.rs`, `app/Sources/VizDAWAppModel/ControlCommands.swift`
- Create: `app/Sources/VizDAWAppModel/InputFxCommands.swift`, `app/Tests/VizDAWAppModelTests/InputFxCommandsTests.swift`

**Interfaces:**
- Consumes: `input_fx_from_json`, `input_fx_json`, `LaneEdit.setInputFx`, `InputInfoRow.midiFxDropped` (Task 6).
- Produces:
  - Registry `lane.set_input_fx` (group "Lane", title "Set Input Effects", no key) with params:
    - `seq` and `lane` (`id_param`);
    - `chain`: `ParamKind::Text`, required, help "The effect chain as JSON, first effect first: [{\"kind\": {\"transpose\": {\"semitones\": 12}}}, …]; [] clears it.".
  - `state.get document`'s lane objects gain `"input_fx": input_fx_json(&l.input_fx)`; `state.get input` gains `"midi_fx_dropped"`.
  - Swift `InputFxCommands`:
    ```swift
    @MainActor public enum InputFxCommands {
        public static let setInputFx = "lane.set_input_fx"
        public static let ids: Set<String> = [setInputFx]
        public static func run(_ id: String, args: [ControlArg], doc: UUID, _ m: DocumentModel) -> ControlOutcome
    }
    ```
    - `seq`/`lane` not ids, or `chain` missing or refused by `inputFxFromJson`: `invalid_params`, before the core is called.
    - The core's `Model` refusal: `failed`, "Set Input Effects Failed". The core's `Busy`: `busy`.
    - An open editor drag: `unavailable`, as `SynthCommands` does.

- [ ] **Step 1: Write the failing tests.**
   - In `crates/core-control/tests/commands.rs`:
     - the registry count assertion moves to its end-of-5f-3 value + 1, with the message gaining "; 5g-1 adds lane.set_input_fx";
     - add:

```rust
#[test]
fn input_fx_commands_check_their_params() {
    let run = |id: &str, params: Value| json!({ "id": id, "params": params });
    for params in [
        json!({ "seq": 1, "lane": 2, "chain": "[]" }),
        json!({ "seq": 1, "lane": 2, "chain": "[{\"kind\": {\"transpose\": {\"semitones\": 12}}}]" }),
    ] {
        assert!(
            matches!(ok("command.run", run("lane.set_input_fx", params.clone())), Command::CommandRun { .. }),
            "{params}"
        );
    }
    for params in [
        json!({}),
        json!({ "seq": 1, "lane": 2 }),
        json!({ "lane": 2, "chain": "[]" }),
        json!({ "seq": 1, "chain": "[]" }),
        json!({ "seq": 1, "lane": 2, "chain": [] }),
        json!({ "seq": 1, "lane": 2, "chain": 12 }),
        json!({ "seq": 1, "lane": 2, "chain": null }),
        json!({ "seq": -1, "lane": 2, "chain": "[]" }),
        json!({ "seq": 1, "lane": "Keys", "chain": "[]" }),
        json!({ "seq": 1, "lane": 2, "chain": "x".repeat(100_000) }),
        json!({ "seq": 1, "lane": 2, "chain": "[]", "junk": 1 }),
    ] {
        let (code, _) = err("command.run", run("lane.set_input_fx", params.clone()));
        assert_eq!(code, ErrorCode::InvalidParams, "{params}");
    }
}
```

   - In `crates/core-ffi/tests/control.rs`, extend the test that builds a `ControlResult::Document` result (line 621 at `f3291d0`): its first lane gets `input_fx: vec![InputFxRow { kind: InputFxKindRow::Transpose { semitones: 12 }, bypass: false }]`, and the test asserts `v["lanes"][0]["input_fx"] == json!([{"kind": {"transpose": {"semitones": 12}}}])`. The `ControlResult::InputState` result test asserts `"midi_fx_dropped"` is present. If the file has no `InputState` test, add `input_state_reports_midi_fx_dropped`, building one with `midi_fx_dropped: 3` and asserting `v["midi_fx_dropped"] == 3`.
   - Create `app/Tests/VizDAWAppModelTests/InputFxCommandsTests.swift`:

```swift
import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

/// `lane.set_input_fx` (plan 5g-1 Task 7): arguments checked before the
/// core is called; the core's refusals are the call's failures; nothing
/// opens a dialog.
@MainActor
@Suite(.serialized) struct InputFxCommandsTests {
    func doc() throws -> (UUID, DocumentModel, FakeSession) {
        let f = demoFake()
        let s = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { f }, open: { _ in demoFake() })
        let id = try #require(s.newDemo())
        return (id, try #require(s.model(id)), f)
    }

    func arg(_ name: String, _ v: String) -> ControlArg { ControlArg(name: name, value: .text(value: v)) }
    func int(_ name: String, _ v: Int64) -> ControlArg { ControlArg(name: name, value: .int(value: v)) }

    func code(_ o: ControlOutcome) -> ControlErrorCode? {
        if case .failed(let f) = o { return f.code }
        return nil
    }

    @Test func inputFxCommandsCheckTheirArguments() throws {
        let (id, m, f) = try doc()
        let run = { (args: [ControlArg]) in InputFxCommands.run(InputFxCommands.setInputFx, args: args, doc: id, m) }
        let chain = #"[{"kind": {"transpose": {"semitones": 12}}}]"#
        #expect(code(run([int("seq", 3), int("lane", 31), arg("chain", chain)])) == nil)
        #expect(
            f.edits.last
                == .lane(edit: .setInputFx(seq: 3, lane: 31, chain: [InputFxRow(kind: .transpose(semitones: 12), bypass: false)])))
        #expect(code(run([int("seq", 3), int("lane", 31), arg("chain", "[]")])) == nil)
        #expect(f.edits.last == .lane(edit: .setInputFx(seq: 3, lane: 31, chain: [])))
        let before = f.edits.count
        for bad in ["", "{", "[1]", #"[{"kind": {"flanger": {}}}]"#, #"[{"kind": {"transpose": {"semitones": 300}}}]"#] {
            #expect(code(run([int("seq", 3), int("lane", 31), arg("chain", bad)])) == .invalidParams, "\(bad)")
        }
        #expect(code(run([int("lane", 31), arg("chain", "[]")])) == .invalidParams)
        #expect(code(run([int("seq", 3), arg("chain", "[]")])) == .invalidParams)
        #expect(code(run([int("seq", 3), int("lane", 31)])) == .invalidParams)
        #expect(code(run([int("seq", -3), int("lane", 31), arg("chain", "[]")])) == .invalidParams)
        #expect(f.edits.count == before, "nothing reached the core")
        f.editError = .Model(message: "out of range: arpeggiator.octaves")
        #expect(code(run([int("seq", 3), int("lane", 31), arg("chain", "[]")])) == .failed)
        f.editError = .Busy(message: "Finish the open edit first.")
        #expect(code(run([int("seq", 3), int("lane", 31), arg("chain", "[]")])) == .busy)
        f.editError = nil
    }

    @Test func aDragKeepsTheChannelOff() throws {
        let (id, m, f) = try doc()
        #expect(m.editor.beginGesture("Move", lanes: [], origin: .other))
        let before = f.edits.count
        let o = InputFxCommands.run(
            InputFxCommands.setInputFx, args: [int("seq", 3), int("lane", 31), arg("chain", "[]")], doc: id, m)
        #expect(code(o) == .unavailable)
        #expect(f.edits.count == before)
    }
}
```

   If `FakeSession` names its edit log or error injection differently (`edits`, `editError`), use its names and say so. If it has no edit-error injection, add `var editError: VizError?`, thrown by `edit(cmd:filter:)` when set.

- [ ] **Step 2: Run them to see them fail.** Run `cargo test -p core-control --test commands` and `cargo test -p vizdaw-ffi --test control`; then `scripts/build-ffi.sh` and `scripts/swift.sh test`. Expected: FAIL. The ID is unknown, there are no JSON keys, and `InputFxCommands` is not found.

- [ ] **Step 3: Implement.**
   - **The registry:** a `SET_INPUT_FX_PARAMS` constant beside `SET_INPUT_PARAMS`, and a `CommandSpec` after `lane.set_monitor`'s with `key_only(cmd("lane.set_input_fx", "Lane", "Set Input Effects", None, true))`.
   - **`control.rs`:** the two JSON keys.
   - **`InputFxCommands.swift`:** the interface above. `ids` joins `ControlCommands.handled` (`.union(InputFxCommands.ids)`, appended to the chain 5f left). Dispatch in `run`, beside SynthCommands:

     Find:
     ```swift
             case _ where SynthCommands.ids.contains(id):
                 return SynthCommands.run(id, args: args, doc: docID, m)
     ```
     Replace:
     ```swift
             case _ where SynthCommands.ids.contains(id):
                 return SynthCommands.run(id, args: args, doc: docID, m)
             case _ where InputFxCommands.ids.contains(id):
                 return InputFxCommands.run(id, args: args, doc: docID, m)
     ```

     The chain is parsed with `try inputFxFromJson(text:)`, and the edit applied with `m.apply(try m.session.edit(cmd: .lane(edit: .setInputFx(…)), filter: nil))`.

- [ ] **Step 4: Run the tests and the gates.** Run the Rust tests for `core-control` and `core-ffi`, both clippies, `cargo clippy -p core-ffi --lib -- -D warnings`, `scripts/build-ffi.sh`, `scripts/swift.sh build` and `scripts/swift.sh test`. `everyRegisteredMenuCommandHasAHandler` passes. Expected: core-control +1, core-ffi +0 or +1, Swift +2, registry +1.

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-control/src/registry.rs crates/core-control/tests/commands.rs crates/core-ffi/src/control.rs crates/core-ffi/tests/control.rs app/Sources/VizDAWAppModel/InputFxCommands.swift app/Sources/VizDAWAppModel/ControlCommands.swift app/Tests/VizDAWAppModelTests/InputFxCommandsTests.swift
git commit -m "control: lane.set_input_fx, input_fx in state.get document, midi_fx_dropped in state.get input (plan 5g-1 Task 7)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD
```

---

### Task 8: `InputFxModel`, the popover's model

**Files:**
- Create: `app/Sources/VizDAWAppModel/InputFxModel.swift`, `app/Tests/VizDAWAppModelTests/InputFxModelTests.swift`
- Modify: `app/Sources/VizDAWAppModel/DocumentModel.swift` (`inputFx`, made once like `inputs`)

**Interfaces:**
- Consumes: `LaneRow.inputFx`, `LaneEdit.setInputFx`, `EditorModel.edit` (one undo step, refused during a drag).
- Produces:
  ```swift
  /// The five kinds, in the Add menu's order.
  public enum InputFxName: String, CaseIterable, Sendable { case arpeggiator, transpose, velocity, chord, inputQuantize }
  @Observable @MainActor public final class InputFxModel {
      /// The lane whose chain the popover shows; nil when closed.
      public private(set) var lane: UInt64?
      /// The selected row.
      public var selected: Int?
      /// The selected effect while a slider drags (committed at its end).
      public private(set) var draft: InputFxKindRow?
      public func open(lane: UInt64)
      public func close()
      /// The open lane's chain, the draft shown in place of the selected row.
      public var chain: [InputFxRow] { get }
      nonisolated public static func isLit(_ lane: LaneRow) -> Bool   // non-empty and not all bypassed
      nonisolated public static func defaultKind(_ name: InputFxName) -> InputFxKindRow  // Ruling 14's defaults
      nonisolated public static func title(_ kind: InputFxKindRow) -> String  // "Arpeggiator", …, "Input Quantize"
      nonisolated public static func summary(_ kind: InputFxKindRow) -> String // "Up 1/16 · 1 oct · 50%", "+12", "×120% +0", "+4 +7", "1/16 · 100%"
      /// The Input Quantize grids by name ("1/16", "1/8T", "1/4.", …), in `input_quantize_grids` order (Ruling 12).
      nonisolated public static let quantizeGrids: [(title: String, ticks: Int64)]
      public func add(_ name: InputFxName)          // refused past 8, and a second Input Quantize: the status line says why
      public func remove(at i: Int)
      public func move(from: IndexSet, to: Int)
      public func setBypass(_ i: Int, _ on: Bool)
      public func update(_ i: Int, _ kind: InputFxKindRow)   // a button or stepper: one edit
      public func beginDraft()                      // a slider grabbed
      public func setDraft(_ kind: InputFxKindRow)  // while it moves: no edit
      public func commitDraft()                     // released: one edit if it changed
  }
  ```
  - Every edit is `document.editor.edit(.lane(edit: .setInputFx(seq:lane:chain:)))` on the selected sequence. A refusal leaves the status line set by `EditorModel`.
  - The status texts are "A chain holds at most 8 effects." and "A chain holds one Input Quantize."

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Tests/VizDAWAppModelTests/InputFxModelTests.swift <<'VIZDAW_EOF'
import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

/// The input effects popover's model (plan 5g-1 Task 8; spec §6.8 UI):
/// add, remove, reorder, bypass and settings, each one undoable lane edit;
/// a slider drag is one edit at its end; the FX button's lit state.
@MainActor
@Suite(.serialized) struct InputFxModelTests {
    /// Song's Chords lane (31) with `chain`, the popover open on it.
    func model(_ chain: [InputFxRow] = []) throws -> (DocumentModel, FakeSession) {
        let f = editorFake()
        f.laneTable[3]?[1].inputFx = chain
        let m = try DocumentModel(session: f)
        m.select(3)
        m.inputFx.open(lane: 31)
        return (m, f)
    }

    func last(_ f: FakeSession) -> [InputFxRow]? {
        if case .lane(edit: .setInputFx(seq: 3, lane: 31, let chain))? = f.edits.last { return chain }
        return nil
    }

    let arp = InputFxRow(kind: InputFxModel.defaultKind(.arpeggiator), bypass: false)
    let up12 = InputFxRow(kind: .transpose(semitones: 12), bypass: false)

    @Test func addRemoveReorderAndBypassAreOneEditEach() throws {
        var (m, f) = try model()
        m.inputFx.add(.arpeggiator)
        #expect(last(f) == [arp])
        (m, f) = try model([arp])
        m.inputFx.add(.transpose)
        #expect(last(f) == [arp, up12])
        (m, f) = try model([arp, up12])
        let before = f.edits.count
        m.inputFx.move(from: IndexSet(integer: 1), to: 0)
        #expect(f.edits.count == before + 1)
        #expect(last(f) == [up12, arp])
        (m, f) = try model([up12, arp])
        m.inputFx.setBypass(0, true)
        #expect(last(f) == [InputFxRow(kind: up12.kind, bypass: true), arp])
        (m, f) = try model([up12, arp])
        m.inputFx.remove(at: 0)
        #expect(last(f) == [arp])
        (m, f) = try model([up12])
        m.inputFx.update(0, .transpose(semitones: -7))
        #expect(last(f) == [InputFxRow(kind: .transpose(semitones: -7), bypass: false)])
    }

    @Test func aSliderDragIsOneEditAtItsEnd() throws {
        let (m, f) = try model([InputFxRow(kind: InputFxModel.defaultKind(.velocity), bypass: false)])
        let fx = m.inputFx
        fx.selected = 0
        let before = f.edits.count
        fx.beginDraft()
        for scale in [110, 120, 130, 140] as [UInt16] {
            fx.setDraft(.velocity(scale: scale, offset: 0, min: 1, max: 127, fixed: nil))
            #expect(fx.chain[0].kind == .velocity(scale: scale, offset: 0, min: 1, max: 127, fixed: nil))
        }
        #expect(f.edits.count == before, "no edit while dragging")
        fx.commitDraft()
        #expect(f.edits.count == before + 1)
        #expect(last(f)?[0].kind == .velocity(scale: 140, offset: 0, min: 1, max: 127, fixed: nil))
        fx.beginDraft()
        fx.commitDraft()
        #expect(f.edits.count == before + 1, "an unchanged drag edits nothing")
    }

    @Test func theChainIsBounded() throws {
        var (m, f) = try model(Array(repeating: up12, count: 8))
        var before = f.edits.count
        m.inputFx.add(.chord)
        #expect(f.edits.count == before)
        #expect(m.editor.status == "A chain holds at most 8 effects.")
        (m, f) = try model([InputFxRow(kind: InputFxModel.defaultKind(.inputQuantize), bypass: false)])
        before = f.edits.count
        m.inputFx.add(.inputQuantize)
        #expect(f.edits.count == before)
        #expect(m.editor.status == "A chain holds one Input Quantize.")
    }

    @Test func theButtonIsLitForAnActiveChain() {
        var l = lane(31, "Chords")
        #expect(!InputFxModel.isLit(l))
        l.inputFx = [InputFxRow(kind: .transpose(semitones: 3), bypass: true)]
        #expect(!InputFxModel.isLit(l), "all bypassed")
        l.inputFx.append(InputFxRow(kind: .chord(intervals: [4, 7]), bypass: false))
        #expect(InputFxModel.isLit(l))
    }

    @Test func titlesSummariesAndGrids() {
        #expect(InputFxModel.title(.inputQuantize(grid: 3840, strength: 100)) == "Input Quantize")
        #expect(InputFxModel.defaultKind(.inputQuantize) == .inputQuantize(grid: 3840, strength: 100))
        #expect(InputFxModel.summary(InputFxModel.defaultKind(.arpeggiator)) == "Up 1/16 · 1 oct · 50%")
        #expect(InputFxModel.summary(.transpose(semitones: -5)) == "\u{2212}5")
        #expect(InputFxModel.summary(.chord(intervals: [4, 7])) == "+4 +7")
        #expect(InputFxModel.quantizeGrids.count == 15)
        #expect(InputFxModel.quantizeGrids.contains { $0.title == "1/16" && $0.ticks == 3840 })
        #expect(InputFxModel.quantizeGrids.contains { $0.title == "1/8T" && $0.ticks == 5120 })
        #expect(InputFxModel.quantizeGrids.contains { $0.title == "1/4." && $0.ticks == 23_040 })
    }
}
VIZDAW_EOF
```

   The tests build a fresh `DocumentModel` per starting chain, as `RecordUiCarryTests` does: `editorFake()`'s Song is sequence 3, and its Chords lane (31) is at index 1.

- [ ] **Step 2: Run them to see them fail** with `scripts/swift.sh test`. Expected: FAIL to compile, because `InputFxModel` is not found.

- [ ] **Step 3: Implement** `InputFxModel` and `DocumentModel.inputFx` (the `inputs` pattern: an `@ObservationIgnored private var inputFxModel: InputFxModel?`). `summary` follows the test's spellings: the minus sign is U+2212, and the octave word is "oct".

- [ ] **Step 4: Run the tests and the gates** with `scripts/swift.sh build` and `scripts/swift.sh test`. Expected: Swift +5.

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add app/Sources/VizDAWAppModel/InputFxModel.swift app/Sources/VizDAWAppModel/DocumentModel.swift app/Tests/VizDAWAppModelTests/InputFxModelTests.swift
git commit -m "app: the input effects model: add, remove, reorder, bypass, settings, one edit per gesture (plan 5g-1 Task 8)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD
```

---

### Task 9: The FX button and the effects popover

**Files:**
- Modify: `app/Sources/VizDAWEditors/ArrangementLayout.swift` (`HeaderControl.fx`, the layout), `app/Sources/VizDAWEditors/LaneHeaderView.swift` (draw, click, the popover hook), `app/Sources/VizDAW/DocumentWindow.swift` (installs the hook once)
- Create: `app/Sources/VizDAW/InputFxPopover.swift`
- Test: modify `app/Tests/VizDAWAppModelTests/LaneHeaderInputTests.swift`, `app/Tests/VizDAWEditorsTests/ArrangementLayoutTests.swift`

**Interfaces:**
- Consumes: `InputFxModel` (Task 8).
- Produces:
  - `HeaderControl.fx`;
  - `LaneHeaderLayout.controls` with `(.name, CGRect(x: 8, y: y + 4, width: 80, height: 18))` and `(.fx, CGRect(x: 92, y: y + 4, width: 18, height: 18))`;
  - `@MainActor public static var fxPopoverContent: ((DocumentModel) -> NSViewController)?` on `LaneHeaderView`;
  - `func showFx(seq: UInt64, lane: LaneRow, from r: CGRect)`, which calls `document.inputFx.open(lane:)`, makes the hook's controller, and shows it in an `NSPopover` (behaviour `.transient`) when the header is in a window. Its delegate's `popoverDidClose` calls `document.inputFx.close()`. Without a hook, or with no window (tests), it opens the model only.
  - `InputFxPopover: View`, a SwiftUI view of an `InputFxModel` (Ruling 16):
    - a `List` of the chain with `.onMove` → `move`, a bypass `Toggle` per row → `setBypass`, and a remove button → `remove`;
    - an "Add" `Menu` of the five kinds;
    - the selected effect's controls:
      - Arpeggiator: mode `Picker`; rate `Picker`s (1/1…1/64; straight, dotted, triplet); octaves `Stepper` 1–4; gate `Slider` 5–200; velocity `Picker` As Played/Fixed with a `Stepper` 1–127; Latch `Toggle`;
      - Transpose: `Stepper` −48…48;
      - Velocity: scale `Slider` 0–400, offset `Slider` −127…127, min/max `Stepper`s, Fixed `Toggle` + `Stepper`;
      - Chord: up to 6 interval `Stepper`s with add and remove;
      - Input Quantize: grid `Picker` from `quantizeGrids`, strength `Slider` 0–100.

    Sliders use `onEditingChanged` → `beginDraft`/`commitDraft`, with `setDraft` while they move. Pickers, steppers and toggles call `update`.

- [ ] **Step 1: Write the failing tests.**
   - In `ArrangementLayoutTests.headerControlsAreFoundByPosition`, add `#expect(LaneHeaderLayout.control(at: CGPoint(x: 100, y: 48 + 10), rowTop: 48) == .fx)` and `#expect(LaneHeaderLayout.control(at: CGPoint(x: 50, y: 48 + 10), rowTop: 48) == .name)`. `theControlsShareTheRowWithoutOverlapping` already covers the new rect.
   - Append to `LaneHeaderInputTests`:

```swift
    @Test func theFxButtonIsLitByAnActiveChainAndOpensTheLanesChain() throws {
        let f = editorFake()
        f.laneTable[3]?[1].inputFx = [InputFxRow(kind: .chord(intervals: [4, 7]), bypass: false)]
        let m = try DocumentModel(session: f)
        m.select(3)
        #expect(InputFxModel.isLit(m.lanes[1]))
        final class Probe: NSViewController {
            override func loadView() { view = NSView(frame: .init(x: 0, y: 0, width: 10, height: 10)) }
        }
        var made = 0
        LaneHeaderView.fxPopoverContent = { _ in
            made += 1
            return Probe()
        }
        defer { LaneHeaderView.fxPopoverContent = nil }
        let h = LaneHeaderView(document: m)
        let r = try #require(LaneHeaderLayout.controls(rowTop: h.rowTop(1)).first { $0.0 == .fx }?.1)
        let at = h.convert(NSPoint(x: r.midX, y: r.midY), to: nil)
        let e = try #require(
            NSEvent.mouseEvent(
                with: .leftMouseDown, location: at, modifierFlags: [], timestamp: 0, windowNumber: 0,
                context: nil, eventNumber: 1, clickCount: 1, pressure: 1))
        h.mouseDown(with: e)
        #expect(m.inputFx.lane == 31)
        #expect(made == 1, "the content is made even with no window to show it in")
    }
```

   The click is synthesized as in `RecordUiCarryTests.theMeterHidesUnder46PointsAndTheMonitorClickCycles`.

- [ ] **Step 2: Run them to see them fail** with `scripts/swift.sh test`. Expected: FAIL to compile, because `.fx` is not found.

- [ ] **Step 3: Implement.**
   - In `LaneHeaderView.draw`, `case .fx: toggle("FX", on: InputFxModel.isLit(lane), color: .systemPurple, in: r)`.
   - In `mouseDown`, `case .fx: showFx(seq: seq, lane: lane, from: r)`, where `r` is the button's rect from `LaneHeaderLayout.controls`.
   - VoiceOver: `ArrangementA11y.header` adds ", input effects on" when lit. If that string is pinned by a test, extend the test with the lit case.
   - In `DocumentWindow.swift`, where the window's editors are set up once, install `LaneHeaderView.fxPopoverContent = { NSHostingController(rootView: InputFxPopover(model: $0.inputFx)) }`.

- [ ] **Step 4: Run the tests and the gates** with `scripts/swift.sh build`, `scripts/swift.sh test` and `scripts/make-app.sh`. Do not launch the app: the owner checks the view from README 189–193. Expected: Swift +1 (and the extended layout test).

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add app/Sources/VizDAWEditors/ArrangementLayout.swift app/Sources/VizDAWEditors/LaneHeaderView.swift app/Sources/VizDAW/InputFxPopover.swift app/Sources/VizDAW/DocumentWindow.swift app/Tests/VizDAWAppModelTests/LaneHeaderInputTests.swift app/Tests/VizDAWEditorsTests/ArrangementLayoutTests.swift
git commit -m "app: the lane header's FX button and the input effects popover (plan 5g-1 Task 9)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD
```

---

### Task 10: The arpeggiator e2e, README 189–195, the backlog rows, the full gates

**Files:**
- Modify: `crates/vizctl/tests/e2e.rs`, `app/README.md`, `docs/feature-backlog.md`

**Interfaces:**
- Consumes: everything above; the e2e helpers `App::start_with`, `wait_for_record`, `wait_for_undo`, `lane_named`, `WAIT_MS`.

- [ ] **Step 1: Write the e2e.** Append to `crates/vizctl/tests/e2e.rs`:

```rust
/// Spec §12.3's "`midi.inject` held chord into a lane with an arpeggiator
/// while recording: check the recorded notes are the arpeggio" (plan 5g-1;
/// README 190): a C major chord held for a bar into "Keys" with an Up
/// arpeggiator at 1/16 records as C, E, G repeating, each on a sixteenth
/// and half a sixteenth long. Offline device, no MIDI port.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn arpeggiates_an_injected_chord_while_recording() {
    use core_engine::fixtures::{Builder, Q};
    use core_model::command::{Command, LaneCmd, SequenceCmd};
    use core_model::event::EventBody;
    use core_model::input_fx::{Arp, InputFx, InputFxKind};
    use core_model::lane::LaneInput;
    use core_model::sequence::SeqLength;
    use core_model::time::Tick;
    let mut app = App::start_with(&[]);
    let mut b = Builder::new();
    let synth = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, keys) = b.sequence("Arp", "Keys", Some(synth)).unwrap();
    for c in [
        LaneCmd::SetInput { seq, lane: keys, input: Some(LaneInput::Midi { port: Some("Keys".into()), channel: None }) },
        LaneCmd::SetInputFx { seq, lane: keys, chain: vec![InputFx::new(InputFxKind::Arpeggiator(Arp::default()))] },
    ] {
        b.run(Command::Lane(c)).unwrap();
    }
    b.run(Command::Sequence(SequenceCmd::SetLength { seq, length: SeqLength::Locked(Tick(8 * Q)) })).unwrap();
    let path = app.launched.home.join("Arp.vizdaw");
    core_model::file::save(&b.finish(), &path).unwrap();
    let w = app.call("app.open", json!({"path": path}))["window"].clone();
    app.wait(&w, json!({"condition": "documents_at_least", "count": 1}));
    let lane = lane_named(&mut app, &w, "Keys");
    assert_eq!(lane["input_fx"][0]["kind"]["arpeggiator"]["rate"]["division"], 16, "{lane}");
    app.call("command.run", json!({"id": "transport.arm_previous", "window": w}));
    assert_eq!(lane_named(&mut app, &w, "Keys")["arm"], true);
    app.call(
        "command.run",
        json!({"id": "record.set", "params": {"replace": true, "loop": false, "pre_roll": 0, "count_in": 0}, "window": w}),
    );
    app.call("command.run", json!({"id": "transport.record", "window": w}));
    wait_for_record(&mut app, &w, |r| r["recording"] == true && r["frames"].as_u64() >= Some(24_000));
    app.call(
        "command.run",
        json!({"id": "midi.inject", "params": {"port": "Keys", "bytes": "90 3c 64 90 40 64 90 43 64"}, "window": w}),
    );
    wait_for_record(&mut app, &w, |r| r["frames"].as_u64() >= Some(24_000 * 5));
    app.call(
        "command.run",
        json!({"id": "midi.inject", "params": {"port": "Keys", "bytes": "80 3c 00 80 40 00 80 43 00"}, "window": w}),
    );
    wait_for_record(&mut app, &w, |r| r["recording"] == false);
    wait_for_undo(&mut app, &w, "Record");
    let input = app.call("state.get", json!({"target": "input", "window": w}));
    assert_eq!(input["midi_fx_dropped"], 0, "{input}");
    app.call("command.run", json!({"id": "file.save", "window": w}));
    let doc = core_model::file::load(&path).unwrap();
    let l = doc.sequence(seq).unwrap().lanes.iter().find(|l| l.id == keys).unwrap();
    let notes: Vec<(i64, u8, i64)> = l.active().unwrap().events.values()
        .filter_map(|e| match &e.body {
            EventBody::Note { pitch, duration, .. } => Some((e.tick.0, *pitch, duration.0)),
            _ => None,
        })
        .collect();
    assert!(notes.len() >= 12, "about a bar of sixteenths: {notes:?}");
    let first = [60u8, 64, 67].iter().position(|p| *p == notes[0].1).expect("a chord note");
    for (k, (tick, pitch, duration)) in notes.iter().enumerate() {
        assert_eq!(*pitch, [60, 64, 67][(first + k) % 3], "{notes:?}");
        let off = tick % (Q / 4);
        assert!(off.min(Q / 4 - off) <= 2, "{tick} is off the sixteenths: {notes:?}");
        assert!((duration - Q / 8).abs() <= 2, "{duration}: {notes:?}");
    }
    app.quit();
}
```

   If `record.set`'s count-in parameter has another name, follow `RECORD_SET_PARAMS`. If the recorded lane keeps old events, filter the notes to those after the press, and say so.

- [ ] **Step 2: Write README 189–195** at the end of `app/README.md`:

```markdown
### MIDI input effects (plan 5g-1)

By ear and by eye; the chain is tested (`core-engine/tests/input_fx*.rs`),
the recording (`core-ffi/tests/passes.rs`, `record.rs`) and the e2e
`arpeggiates_an_injected_chord_while_recording`.

189. The lane header shows an "FX" button left of the monitor button: grey
     with an empty chain. Click it: a popover lists the chain (empty) with
     an Add menu (Arpeggiator, Transpose, Velocity, Chord, Input Quantize).
     Add a Transpose: the button lights purple; bypass it: grey again.
190. A real keyboard on a MIDI lane, armed: add an Arpeggiator (Up, 1/16).
     Stopped, hold a chord: it arpeggiates at once at the project tempo;
     change the tempo and it follows. Play with the click on: the notes
     fall on the sixteenths, and a chord pressed between steps waits for
     the next one. Try each mode, 2–4 octaves, a triplet and a dotted rate.
191. Latch: hold a chord and let go: it keeps playing; press a new chord
     with no key down: the pattern changes to it.
192. Transpose +12 then Chord (+4 +7) near the top of the keyboard, then
     drag Chord above Transpose: the top notes differ (notes past the
     highest key are dropped, never folded).
193. Hold a chord through the arpeggiator and bypass it, change its rate,
     drag a velocity slider, arm another lane, disarm this one: no note
     hangs, and a latched arpeggio survives arming another lane.
194. Add Input Quantize (1/16, 100%) and record a loose phrase: thru is
     never delayed; the recorded notes start on the sixteenths and keep
     their lengths. At 50% they move half way.
195. Each change in the popover is one Undo ("Set Input Effects"); a
     slider drag is one step. Save, close and reopen: the chain is there.
```

- [ ] **Step 3: Add the backlog rows** to `docs/feature-backlog.md`, in its table format:
   - "Learned controls are not consumed: a CC learned to a fader also plays into an armed lane on that port (plan 5g-1 Ruling 2)."
   - "A build before 5g names a project with input effects Corrupt (`deny_unknown_fields`); decide on a reader that skips unknown lane fields before release."

- [ ] **Step 4: Run the full gates:**
   - `cargo fmt --all -- --check`, both clippies and `cargo clippy -p core-ffi --lib -- -D warnings`;
   - `cargo test --workspace`;
   - the rt-check gates (with `--test rt_alloc_input_fx`);
   - `scripts/build-ffi.sh`, `scripts/swift.sh build`, `scripts/swift.sh test` and `scripts/make-app.sh`;
   - the e2e: `VIZDAW_E2E_APP=dist/VizDAW.app cargo test -p vizctl --test e2e -- --ignored --nocapture --test-threads 1`.

   Then `pgrep -lf MacOS/VizDAW` prints nothing, and `ls -d /tmp/vze*` prints nothing new.

   Expected, relative to the end of 5f-3:
   - Rust +6 (model), +13 + 13 + 9 (engine), +2 + 6 (FFI), +1 (control) = **+50**;
   - Swift +8;
   - e2e +1;
   - registry +1;
   - README through 195.

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/vizctl/tests/e2e.rs app/README.md docs/feature-backlog.md
git commit -m "e2e: an injected chord through the arpeggiator records as the arpeggio; README 189-195; backlog rows (plan 5g-1 Task 10)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD
```

---

## Self-Review

1. **Spec coverage.**
   - §4.5 `input_fx` (≤ 8, empty default, `LaneCmd`): Task 1.
   - §6.6 "Thru and record both pass through the lane's MIDI input effect chain": Task 4.
   - §6.8:
     - the model: Task 1;
     - the five effects with every setting: Tasks 1–3;
     - real-time, sample-accurate, synced: Tasks 3–4;
     - the lock-free queue, already timestamped: unchanged since 5c, used in Task 4;
     - 32 held / 128 sounding, counted: Tasks 3–4 and 6;
     - note-offs on a change or bypass: Tasks 2 and 4;
     - recording post-effect with Input Quantize: Tasks 4–5;
     - no chain unless armed or monitored, none on playback: Ruling 13 and Task 4 (routes exist only for listening lanes, as before);
     - UI: Tasks 8–9.
   - §6.9's FX button: Task 9.
   - §12.1 `lane.set_input_fx` and `state.get` per-lane `input_fx`: Task 7.
   - §12.3:
     - golden tests per mode, rate (triplets) and octaves, synced and free-running: Task 3;
     - Latch, transpose, velocity, chord and clamps: Tasks 2–3;
     - chain order: Task 2;
     - bypass and changes send note-offs: Tasks 2 and 4;
     - capacity overflow counted: Tasks 2–4;
     - recording equals thru, quantize on recorded starts only: Tasks 4–6;
     - `rt-check`: Task 4;
     - the e2e: Task 10;
     - the manual "arpeggiator against a real keyboard, synced and free-running": README 190.
   - Spec Ruling 27: all of the above.
   - MPE is plan 5g-2's (Ruling 1).
   - No gaps.
2. **Step scan.**
   - Every code step names its file, its signature and the spec values. Bodies are given only where no signature determines them: `live_midi`, `emit_live` and the carry in `SetInputs`, because of the borrow and ordering rules.
   - The arpeggiator's patterns and timing are Rulings 9–10 and the golden tests.
   - Two expectations may need the tree's arithmetic, and each step says what to do: the latch test's last steps (Task 3), and the frame the transport rolls at (Task 4).
3. **Type consistency.**
   - `InputFx`/`InputFxKind` (Task 1) feed `InputChain::new` (Task 2), `MidiInputSpec.fx` (Task 4) and `InputFxRow` (Task 6).
   - `LiveMsg`, `At`, `ChainClock` and `RecordQuantize` (Task 2) are used by Tasks 3–5.
   - `RecordedMidi { msg: LiveMsg, quantize }` (Task 4) is read by `passes` (Task 5).
   - `InputFxKindRow`'s fields match `InputFxKind`'s, with `rate` flattened into `division` and `feel`.
   - `InputFxModel.defaultKind` mirrors Ruling 14.
4. **Review Focus.** Each of the five lines names its tests, and each test is in its owning task.
5. **Proportion.** Spec §6.8 is about 30 lines, and this plan is long because its tests are the effects' golden behaviour. Implementation bodies are left to the implementer except the three renderer functions.

## Execution handoff

Plan complete and saved to `docs/superpowers/plans/2026-11-02-midi-input-effects.md`. The controller runs it subagent-driven under the standing authorization (spec §13), after 5f-3 and before 5g-2.
- Tasks 2–4 build on each other's `LiveMsg` and `InputChain`, and Task 4 changes the renderer's real-time path: a fresh reviewer per task should replay Review Focus 1 and 3 there.
- The final review should run the rt-check gates itself.
