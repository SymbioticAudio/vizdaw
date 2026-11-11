# VizDAW 5g-1 — MIDI Input Effects Implementation Plan

> **Runs now (owner, 2026-11-03 and 2026-11-05):** after sub-project 7 and the Tracks plan, on branch `sp5g-input-fx` from main `a292d2c`. It stays in the 1.0 release. The 2026-11-05 pre-flight (`.superpowers/sdd/2026-11-02-midi-input-effects/preflight-2026-11-05.md`) is folded in (2026-11-09), and every Find excerpt below was re-anchored to `a292d2c`. The owner's 2026-11-09 ruling moves the chain's editor from the track header to the channel strip's MIDI FX section (Ruling 16, Task 9; `.superpowers/sdd/2026-11-02-midi-input-effects/task9-rewrite-2026-11-09.md`). The binding owner ruling holds throughout: a MIDI message a learn map takes stays out of the lanes, and only the learned target's automation is written (Ruling 2).

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Each MIDI or instrument track gets a chain of up to 8 built-in input effects: Arpeggiator, Transpose, Velocity, Chord and Input Quantize. The chain runs on the real-time thread while the track is armed or monitoring its input. What it plays thru is exactly what is recorded, with Input Quantize applied to recorded note starts only. The chain is an undoable lane edit, edited from a MIDI FX section beneath Inserts on the channel strip and from `lane.set_input_fx` on the control channel.

**Architecture:**
- **`core-model`** gains `core_model::input_fx` (`InputFx`, `InputFxKind` and the effects' settings, with their checks), `Lane.input_fx` and `LaneCmd::SetInputFx`. The field is saved only when the chain is not empty; `file::VERSION` goes 4 → 5 with an identity migration (Ruling 18).
- **`core-engine`** gains `core_engine::input_fx`, which holds:
  - `LiveMsg`, the live input event. It is MIDI 2.0-shaped: notes with 16-bit velocity, per-note messages addressed by pitch, and channel messages. Plan 5g-2's MPE decoding and encoding use the same type.
  - `InputChain`: the stages, the arpeggiator, an output stage that merges duplicate pitches, a fan-out cap that also bounds the work done, and counters. It has no allocation, no lock and no I/O.

  The renderer feeds every listening lane's chain from the MIDI input queue and ticks it once per block, so arpeggiator steps land on their sample. A changed chain sends its note-offs first. An unchanged chain keeps its state across route changes. The record tap now carries `LiveMsg` and the chain's Input Quantize.
- **`core-ffi`**:
  - `passes` turns recorded `LiveMsg`s into events and applies Input Quantize to note starts.
  - The FFI gains `InputFxRow` and its kinds, `LaneEdit::SetInputFx`, `LaneRow.input_fx`, `InputInfoRow.midi_fx_dropped` and `input_fx_from_json`.
  - The lane's input spec now carries the chain.
- **`core-control`, Swift, `vizctl`:**
  - The control channel gets `lane.set_input_fx` and `state.get document`'s `input_fx` per lane. `state.get input` reports the count of dropped notes.
  - The app gets `InputFxModel` and the instrument strips' MIDI FX section beneath Inserts: up to 8 slots, each with a settings popover and a bypass. The section is lit in the theme's accent colour when the chain is active.
  - Also in this plan: the e2e "`midi.inject` held chord into a lane with an arpeggiator while recording", README 238–244, and the spec's §13 row brought up to date.

**Tech Stack:** Rust 2024 (`core-model`, `core-engine`, `core-ffi` with UniFFI 0.32.2, `core-control`, `vizctl`), Swift 6 / SwiftPM (Swift Testing; CI builds with Swift 6.1 on `macos-15`), macOS 14+.

**Spec:** `docs/superpowers/specs/2026-10-18-recording-audio-design.md`, §13 row "5g-1 — MIDI input effects". This row and "5g-2 — MPE input and output" replace the former 5g row (Ruling 1). This plan delivers:
- §4.5's `input_fx`: at most 8 slots, empty by default, an undoable lane edit;
- §6.6's "Thru and record both pass through the lane's MIDI input effect chain";
- §6.8 in full (read it there; its values are used verbatim in Rulings 7–14 and Task 1's ranges):
  - the model and the five built-in effects with every listed setting;
  - the chain on the real-time thread per armed or Input-monitored lane, sample-accurate and synced;
  - fixed-capacity state (32 held, 128 sounding) with overflow counted;
  - note-offs on a change or a bypass;
  - post-effect recording with Input Quantize on recorded starts only;
  - no chain for lanes that are not listening, and none on playback;
  - the chain's editor (§6.8's FX button and popover), which the owner moved on 2026-11-09 into the channel strip's MIDI FX section; one undo step per gesture;
- §6.9's FX button: the track header (`LaneHeaderView`) is left unchanged, and the section is in the strip instead (Ruling 16). Task 10 amends §6.8 and §6.9 to match;
- §12.1's `lane.set_input_fx { lane, chain }` and `state.get`'s per-lane `input_fx`;
- §12.3's MIDI input effects tests, the arpeggiator e2e, `rt-check` over the chain, and the manual checklist's "the arpeggiator against a real keyboard, synced and free-running";
- spec Ruling 27.

The §13 row's "surface ports skipped by every-source lanes" is already delivered by 5f-1 Ruling 17 (a surface's input port is taken whole before the engine queue; pinned by `crates/core-ffi/tests/surfaces.rs::surface_ports_and_learned_controls_stay_out_of_the_lanes`), so this plan adds nothing for it (Ruling 3). Task 10 amends the row to say so and to give the README numbers.

Plan 5g-2 (`docs/superpowers/plans/2026-11-02-mpe.md`) runs after this one and builds on `LiveMsg`.

Read the whole spec (its §3 rules bind here), 5f-1's Ruling 17 (`docs/superpowers/plans/2026-11-02-control-surfaces.md`: `crate::surfaces::takes`, called by `host::midi_from_source`, `Session::inject_midi` and `input.rs` before anything reaches the engine) and the Tracks plan's Ruling 7 (`docs/superpowers/plans/2026-11-05-tracks.md`: what the app shows says "track", never "lane").

Every Find excerpt below matches `a292d2c` exactly once, or the file an earlier task of this plan left (the step says which).

## Global Constraints

Every task's requirements include these. The implementer and reviewer contracts in `.superpowers/sdd/2026-11-02-midi-input-effects/` hold the rest of the house rules (shell, commits, waits).

- **Shell.** Every command runs after `export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools` and `cd /Users/lattawb/workspace/vizdaw`. Swift only through `scripts/build-ffi.sh` (after any FFI change), `scripts/swift.sh build|test` and `scripts/make-app.sh`; never `swift`, `xcodebuild` or `xcrun` directly.
- **Branch and commits.** Branch `sp5g-input-fx`; never switch branches, never push. Commit at the end of every task by explicit paths (never `git add -A`), never `.superpowers/`, `.DS_Store` or generated files (`app/Sources/VizDAWCore/vizdaw_ffi.swift`, `app/Sources/vizdaw_ffiFFI/include/`, `target/ffi/`, `dist/`). No manifest changes, so `Cargo.lock` never changes. Each task gives its message line and path list; every message ends with:
  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P
  ```
  Then check with `git log -1 --oneline && git show --stat HEAD` (a commit without `DEVELOPER_DIR` fails silently).
- **Clean room and names.** Never read `~/workspace/SVP/research` or any Studio Vision material. No Opcode, Logic, Apple, Midas, Soundcraft, NI, Microsoft, Ableton or Steinberg names in UI or repo text (the scans `NameScanTests.noThirdPartyNamesInWhatTheAppShows` and `core-control/tests/names.rs` enforce the app's half). Effect behaviour comes from the spec and public MIDI specifications only.
- **Track, not lane.** Text the app shows (labels, menus, status lines, alerts, help, README steps) says "track"; code identifiers, command ids (`lane.set_input_fx`), JSON keys and the file format keep `lane`. Enforced by `NameScanTests.noLaneInWhatTheAppShows`, `core-ffi/tests/wording.rs` and `core-control/tests/names.rs`.
- **Never run `auval`**, in any form.
- **No GUI tools, no real devices.**
  - Never use GUI, computer-use or accessibility-automation tools. The owner checks the views by eye from README 238–244.
  - Launch VizDAW only through the e2e (Task 10) with `VIZDAW_AUDIO=offline`; afterwards `pgrep -lf MacOS/VizDAW` prints nothing and `ls -d /tmp/vze*` prints nothing new.
  - Tests never open real MIDI ports, real audio input or devices, and never trigger the microphone prompt. MIDI reaches the engine through `MidiSender` (core-engine), `Session::inject_midi` or `host::debug_midi_from_source` (core-ffi) and `midi.inject` (e2e). Every core-ffi test that plays or records calls `host::force_offline()` through its file's `serial` helper.
  - Tests never touch the real `~/Library/Application Support/VizDAW`, preferences, Trash or devices: temp folders, `Session::debug_set_trash_dir`, `MemoryDefaults()`, the e2e's throwaway home.
- **Programmatic calls open nothing.** Control-channel commands and control-channel clicks never open dialogs, menus, popovers or plugin windows (Ruling 16).
- **Hostile input.** Every new control command gets hostile-input tests at every layer (Task 7 and Review Focus 5).
- **FFI safety.** No panic crosses the FFI: every new export returns `Result<_, VizError>` inside `ffi_guard` (`catch_unwind`); `input_fx_from_json` is one. `input_fx_json` is a plain `pub fn` used by `control.rs`, not exported.
- **UniFFI changes** (Task 6): enums `InputFxKindRow`, `ArpModeRow`, `FeelRow`; record `InputFxRow`; variant `LaneEdit::SetInputFx`; fields `LaneRow.input_fx` and `InputInfoRow.midi_fx_dropped` (both `#[uniffi(default)]`); free function `input_fx_from_json`. No new `Session` method.
- **Swift.** Generated bindings compile in Swift 5 mode; `VizDAWAppModel`, `VizDAWEditors`, `VizDAW` and the tests in Swift 6 mode with no new warnings, on CI's Swift 6.1 too:
  - no `@State` or `@FocusState` anywhere in this plan's code (the Command Line Tools build cannot expand them, and package code — `VizDAWAppModel`, `VizDAWEditors` — never uses them): view state lives in `InputFxModel`;
  - never capture `[weak self]` or a non-Sendable value inside `MainActor.assumeIsolated`; bind a strong `let` (or `nonisolated(unsafe) let`) first, as `DocumentWindow.swift` does;
  - colours, spacing, radii and fonts come from the theme only (Ruling 16, Task 9).
- **Lints.** Every crate keeps `#![deny(missing_docs)]` and `#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic))]`: `.get()`, iterators, `checked_*`/`saturating_*`. Rust stable, edition 2024, GPL-3.0-only; no new crate or dependency (Random uses an in-module xorshift, Ruling 9). `core-model`, `core-engine` and `core-control` stay free of OS APIs, and everything here runs on Linux.
- **TDD.** Every new test is run and seen to fail before the code that makes it pass.
- **Real-time rules (spec §3).**
  - The chain runs on the real-time thread: no allocation, free, lock or I/O there. Everything new on that thread is marked `// RT`: `InputChain::{process, tick, flush, carry_from, quantize, take_dropped}`, the stages, the arpeggiator, the merge, `LiveMsg`'s conversions, the renderer's `live_midi`, `emit_live` and the `SetInputs` carry, and `RecordTap::midi`.
  - `InputChain::new` allocates the chain's one `Box<ChainState>` on the control thread, inside `Engine::load`. Retired routes are freed on the retire thread as now.
  - Work is bounded: recursion by `MAX_INPUT_FX`, outputs *and stage visits* by `MAX_FANOUT` per input event or step (Ruling 7), arpeggiator steps by one per grid line per block with no catch-up (Ruling 10).
  - **rt-check gate** (Tasks 4 and 10, and CI from Task 4): `cargo clippy -p vizdaw-play --all-targets --features rt-check -- -D warnings` and `cargo test -p core-engine --features rt-check --test rt_alloc --test rt_alloc_graph --test rt_alloc_record --test rt_alloc_gestures --test rt_alloc_bounce --test rt_alloc_input_fx`.
- **Media is write-once.** Nothing here writes media.
- **Each task ends green.**
  - Always: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, the Linux clippy (`cargo clippy --workspace --all-targets --target x86_64-unknown-linux-gnu --target-dir target/linux-check -- -D warnings`; CI runs `ubuntu-latest` too, so this plan keeps it although the house rules defer Linux), and `cargo test` for the crates the task touches (the whole workspace in Tasks 1, 4, 6 and 10).
  - Tasks 6–10 also run `cargo clippy -p core-ffi --lib -- -D warnings`, `scripts/build-ffi.sh`, `scripts/swift.sh build` and `scripts/swift.sh test`.
  - Task 9 also runs the no-brand build (`VIZDAW_NO_BRAND=1 scripts/swift.sh build --scratch-path <a fresh temp dir>`, CI's "App builds without the branded glyphs").
  - Task 10 also runs `scripts/make-app.sh` and the e2e: `VIZDAW_E2E_APP=dist/VizDAW.app cargo test -p vizctl --test e2e -- --ignored --nocapture --test-threads 1`.
  - Known flaky: the `damaged_files` proptest, the MIDI listener tests (`platform-macos/tests/midi.rs`), `core-ffi` catalog tests under parallel load, the latency e2e timing. Re-run once before investigating, and say so in the report.
  - **Baseline at `a292d2c` (measured 2026-11-09):** Rust 2077 passed / 24 ignored (`cargo test --workspace`), Swift 1286, e2e 20, registry 162, README through 237, file `VERSION` 4. Each task's last step gives the counts it adds; report the measured counts (a slip in the writer's arithmetic is not a failure).
- **How code is given.** New test files are written whole. New implementation files get their public items with doc lines, and the algorithm where the signature and the tests leave a choice. Changes to existing files are **Find / Replace** excerpts applied in order with the Edit tool; if one does not match, make the same change to the current text and say so in the report.

## Review Focus

These are the five failure modes a user is most likely to hit that the spec implies but does not spell out, most likely first. Each is pinned by named tests in its task.

1. **A chain that changes while notes sound, or hears half a note.** Examples: the user bypasses the arpeggiator mid-phrase, drags a velocity slider while holding a chord, arms a second track while a latched arpeggio plays, disarms with keys down, or starts a MIDI learn (which takes a note-on but not its note-off) mid-phrase.
   - Expected: every note the changed chain was sounding ends once, in thru and in the recording; an unchanged chain keeps playing; a note-off for a note the chain never started is dropped; a message a learn takes never reaches a chain.
   - Pinned by `flush_ends_every_sounding_note_once` (Task 2); `a_changed_chain_ends_its_notes_and_a_kept_one_plays_on` and `bypassing_while_held_leaves_no_note_hanging` (Task 4); `a_learned_message_never_reaches_an_input_chain` (Task 6).
2. **Notes pushed out of range or onto each other.** Examples: Transpose +24 on the top octave, Chord intervals that land on a held key, two chord notes on one pitch, many sources on one pitch.
   - Expected: a pitch outside 0–127 is dropped, never folded; a pitch two sources reach sounds once and ends only when the last ends; chain order matters exactly where a drop applies.
   - Pinned by `out_of_range_pitches_are_dropped_not_folded`, `a_pitch_two_sources_reach_sounds_once_and_ends_once` and `chain_order_matters_where_a_drop_applies` (Task 2).
3. **Arpeggiator timing at the edges.** Examples: a chord pressed mid-step while playing, a stopped transport at another tempo, a locate, a held rewind or fast-forward (7d's winds are repeated jumps), a loop wrap, a gate over 100 % on one note, a triplet rate.
   - Expected: synced steps sit on the grid counted from tick 0, the first at the next step; a jump re-anchors with no burst and no silence; free-running starts at once at the project tempo; a long gate ends the note before it plays again.
   - Pinned by `a_synced_arp_waits_for_the_next_step`, `free_running_starts_at_once_at_the_project_tempo`, `a_long_gate_on_one_note_ends_before_it_retriggers`, `a_locate_moves_the_synced_grid`, `a_jump_while_held_neither_bursts_nor_stalls` and `rates_include_dotted_and_triplet` (Task 3); `arp_steps_follow_the_beat_while_playing` (Task 4).
4. **Too many notes.** Examples: a forearm on the keyboard (33 keys) into the arpeggiator, eight 6-note chords chained.
   - Expected: the newest note past a capacity is dropped and counted, and reaches `state.get input`'s `midi_fx_dropped`; the real-time thread does bounded work (no millions of dropped leaves per key), and nothing panics.
   - Pinned by `the_33rd_held_note_is_dropped_and_counted` (Task 3), `fan_out_is_capped_and_counted` and `eight_chained_chords_stop_at_the_cap` (Task 2), `overflow_reaches_the_counters` (Task 4) and `midi_fx_dropped_is_reported` (Task 6).
5. **A hostile or damaged chain.** Examples: 9 effects, octaves 5, gate 4 %, a seventh chord interval, an interval of 0, two Input Quantize effects, a grid that is not a snap value, malformed or huge JSON on the channel, a damaged project file.
   - Expected: each is refused with a range or invalid error (a file is Corrupt, naming the field), and nothing changes.
   - Pinned by `hostile_chains_are_refused_and_change_nothing` and `a_damaged_chain_in_a_file_is_refused` (Task 1), `hostile_chain_json_is_refused` (Task 6), `input_fx_commands_check_their_params` and `inputFxCommandsCheckTheirArguments` (Task 7).

## Plan Rulings

Each ruling decides something the spec leaves open: what — why — cost if wrong. Numbers are kept stable (3 is withdrawn) because tests and doc lines cite them.

1. **The split.** The former §13 row "5g — MIDI input effects and MPE" is two plans, in this order: **5g-1** (this plan: input effects, engine and UI) and **5g-2** (MPE input and output, `docs/superpowers/plans/2026-11-02-mpe.md`), which builds on `LiveMsg` and the chain's place in the renderer. — As one plan 5g would be about 19 tasks; each half has its own golden tests, e2e and README steps, and MPE also changes the schedule compiler, the recorder's per-note data and destinations. — One more plan header and final review.
2. **The MIDI input path's order (owner ruling, binding; supersedes this plan's first draft).**
   1. Raw MIDI 1.0 bytes from CoreMIDI or `midi.inject` reach `crate::surfaces::takes` first (5f-1 Ruling 17: `host::midi_from_source`, `Session::inject_midi`, and note input in `input.rs`). What it takes — a surface's whole input port, a learnable message (CC, bend, note-on) while a learn waits, and every learned map's messages — never reaches the engine queue, so it never reaches a chain, thru, the recorder, step entry or note input. A learned control writes only its target's automation (5f-1). The rest is queued.
   2. In the renderer, per listening lane, the message is matched (port and channel), then decoded: `LiveMsg::from_midi1` here, MPE in 5g-2.
   3. Then the input effect chain runs, then the output stage merges duplicate pitches (Ruling 6).
   4. Then the output plays thru (encoded as MPE for MPE destinations in 5g-2), and the recorder gets the same messages with the chain's Input Quantize.

   A learn takes a note-on but not its note-off (`surfaces::learnable`), so a chain can hear an orphan note-off; Ruling 6's merge drops it, and an arpeggiator never holds the note. — Spec §4.8 and the owner's ruling; learn maps name raw controller messages, and MPE settings belong to a lane's input, so decoding is per lane after consumption. — None: consumption already exists and is pinned by 5f-1's `a_learned_cc_is_neither_recorded_nor_played_thru` and `a_learned_cc_writes_its_targets_automation_in_touch` (`crates/core-ffi/tests/learn.rs`); Task 6 adds the chain's half.
3. **Withdrawn (fold, pre-flight I2).** The first draft's `MidiInputSpec.skip_ports` duplicated 5f-1 Ruling 17, which already keeps every surface port out of every lane before the queue (and the app's input picker hides them through `LaneInputs.claimedPorts`). Nothing in this plan skips ports.
4. **`LiveMsg`: the chain works on MIDI 2.0-shaped events, not MIDI 1.0 bytes.**
   - `NoteOn { pitch, velocity: u16, expr: NoteExpr }`, `NoteOff { pitch, velocity: u16 }`, `PerNote { pitch, expr: Expr, value: u32 }` (`Expr` is `Bend`, `Pressure` or `Timbre`) and `Channel(Midi1Msg)` for everything else.
   - `from_midi1` maps note-on with velocity > 0 to `NoteOn`, up-scaled by the MIDI 2.0 min-center-max rule (as `vizdaw_ffi::input::upscale_velocity`); note-off and note-on with velocity 0 to `NoteOff`; poly pressure to `PerNote { Pressure }` (up-scaled to 32 bits); every other channel message to `Channel`.
   - `to_midi1(ch)` is the inverse, using `midi1`'s down-scaling. `PerNote { Bend | Timbre }` has no MIDI 1.0 form: `None` (5g-2 counts these drops).
   - A note-on with velocity 0 now plays thru as a note-off (`8n pp 00`), not `9n pp 00`. No existing test expects `9n pp 00` thru (checked at `a292d2c`: `crates/core-engine/tests/live_input.rs`, `crates/core-ffi/tests/live_input.rs`, `learn.rs`, `input.rs`); `learn.rs::a_waiting_learn_lets_note_offs_through` reads only the pitch.
   - — MIDI 2.0 throughout (parent spec §1, §5); Transpose and Chord map one pitch field; 5g-2 decodes MPE into the same type. — One enum to widen.
5. **A chain change replaces the whole chain, and the old one first sends note-offs for every note it sounds.** A change is to its order, a setting, a bypass, adding or removing.
   - Every route change publishes new routes. A new route whose lane, port, channel, destination, thru flag, targets, destination channel and chain settings all equal an old route's takes over the old chain's state (`InputChain::carry_from`, a swap; no allocation). The arm flag may differ. The latch, the held keys, the step and the sounding notes go on.
   - Every other old route is flushed (`InputChain::flush`) to its old targets and to the recorder.
   - The renderer's blanket "release every thru note" on `SetInputs` now spares the targets of carried routes. The carried-target set is a `u64` bit mask, so `render.rs` gains `const _: () = assert!(MAX_TARGETS <= 64);` beside it.
   - — The spec's "A changed or bypassed effect first sends note-offs"; one rule for every change; carrying keeps a latched arpeggio alive when another track is armed. — A velocity tweak while a chord is held ends that chord; the player presses again.
6. **The output stage counts note-ons per output pitch.**
   - A note-on for a pitch already sounding is swallowed; a note-off goes out only when the count reaches 0; a note-off for a pitch the chain is not sounding is dropped. This applies to an empty chain too, so an omni lane hearing one pitch from two channels sounds it once.
   - Counts are `u8` with `saturating_add`/`saturating_sub`: a pitch reached 255 times or more ends on the last off counted.
   - Existing tests that send a note-off with no note-on before it and expect thru change: `crates/core-ffi/tests/learn.rs::a_waiting_learn_lets_note_offs_through` injects `90 3c 64` and `90 3e 64` before `learn_target`, so its offs end notes the lane did start (Task 6). Any other the compiler or a run names gets the same treatment, reported.
   - — Chords and transposes make duplicate pitches; a destination on one channel cannot tell them apart, and a second note-on then one note-off hangs some synths. — Two quick hits on one pitch from two sources sound as one long note.
7. **Capacities and bounded work.**
   - Each arpeggiator holds at most `MAX_HELD = 32` notes and sounds at most `MAX_SOUNDING = 128`.
   - One input event, or one arpeggiator step, sends at most `MAX_FANOUT = 128` messages into the merge (the leaves of the stage recursion; the merge may then swallow duplicates). The budget also bounds work: `run` returns at once when the budget is 0, and a stage stops its loop over outputs when it is spent, so eight chained 6-note chords visit at most `MAX_INPUT_FX × 7 × MAX_FANOUT` stages per event, not 7^8 leaves. A note-off fans out along the same leaves as its note-on, so every note the cap let start also ends.
   - `dropped` counts one per input event or step that the cap truncated (note-offs included), plus each note dropped for `MAX_HELD` or `MAX_SOUNDING` (`InputChain::take_dropped`, summed into `Counters.midi_fx_dropped`). A dropped note-on's later note-off is dropped too (Ruling 6).
   - Transpose, Velocity, Chord and Input Quantize are stateless: they recompute a note-off from their fixed settings.
   - — The spec's capacities; the render thread must not spend a block on dropped leaves. — A cap to raise.
8. **Pitch range: a note moved outside 0–127, with its note-off and its per-note messages, is dropped, never clamped or folded.** Transpose then Chord therefore differs from Chord then Transpose near the ends (spec §12.3's "chain order matters … where clamps apply"); the Velocity clamps are the other case. — Folding stacks unrelated notes on 0 or 127. — A pitch near the edge disappears instead of sounding an octave off.
9. **Arpeggiator patterns**, with held notes `h` (sorted by pitch, or in press order for As Played) and octaves `o`:
   - The pattern `P` is `h` played in each of `o` octaves, lowest first (pitches over 127 dropped). **Up** is `P`; **Down** is `P` reversed; **Up-Down** is `P` then `P` reversed without its ends (C E G E, …); **Down-Up** is its mirror; **As Played** is `P` in press order, octave by octave; **Random** draws `P[x % len]` each step from a 32-bit xorshift (`x ^= x << 13; x ^= x >> 17; x ^= x << 5`) seeded `0x2545_F491` when the chain is made; **Chord** plays all of `P` at each step.
   - The step index survives added or released notes and restarts at 0 when the held set empties. Per-note messages of held notes are dropped; channel messages pass at once.
   - — Conventional public arpeggiator behaviour; no doubled ends; a testable seed. — One function per mode.
10. **Arpeggiator time.**
    - **Rate.** A note value is `4 × PPQ / division` ticks, division 1, 2, 4, 8, 16, 32 or 64; dotted × 3/2, triplet × 2/3. At PPQ 15 360 every rate is whole ticks; the shortest is 1/64 triplet, 640 ticks.
    - **Synced (transport playing).** Steps fall on the multiples of the step counted from tick 0; their frames come from the schedule's `TempoTable::tick_to_frame`, so tempo changes and ramps are followed. Each block, the next synced step is the first grid multiple at or after the block's first tick (`frame_to_tick(transport.1)`): steps before the block are never played. A discontinuity — a locate, a wind (7d's held rewind and fast-forward are repeated jumps), a loop wrap — therefore re-anchors with no catch-up burst and no silence. A chord pressed mid-step waits for the next step.
    - **Free-running (stopped, including a count-in, which leaves the transport stopped).** The first step plays at once; then one every `step × 60 × rate ÷ (bpm × PPQ)` frames, at the schedule's bpm at the play position (120 without a schedule). When Play starts mid-hold the next step moves to the grid; when the transport stops mid-hold, free-running goes on one step after the last step played, or at once if that is already past.
    - **Frames.** Steps are placed in render frames (`transport.pos`, the block's position); generated outputs reach the recorder at their block offset plus `record_lead` (Ruling 11), so the latency is taken off exactly once.
    - **Gate.** The note-off comes `gate %` of the step after its note-on. When a pitch is due again while it still sounds (gate ≥ 100 %), its note-off goes first, on the same frame.
    - **Latch.** While latched, releasing keys keeps the held set; the first key pressed when none is down starts a new set.
    - **Velocity.** As Played uses each held note's velocity; Fixed uses `up7(v)`.
    - — Spec §6.8's Sync; the tick-0 grid matches play quantize (4e Ruling 10); integer ticks with the tempo table are sample-exact. — A whole-note rate in an odd meter drifts from the barline (the grid is in quarter notes, not bars).
11. **What the recorder hears.** Each output reaches `RecordTap::midi(lane, offset, msg, quantize)`: input-driven output keeps the input's offset (the run frame heard when it arrived, spec §6.3); generated output (steps, gate ends) gets its block offset plus the graph latency (`ChainClock.record_lead`), placing it where it sounds against playback. `RecordedMidi` gains `msg: LiveMsg` (replacing `Midi1Msg`) and `quantize: Option<RecordQuantize>`. Thru and the recorder get the same post-merge, post-cap messages; they differ only where thru cannot play (Monitor Off records but plays nothing; `to_midi1` answering `None` drops on thru only; an unroutable destination still records). — "The recorder receives exactly what thru sent"; a generated note is not late input. — Arpeggiated recordings one graph latency early; the e2e checks the grid.
12. **Input Quantize.** At most one per chain (refused otherwise); its place does not matter: it never changes thru and applies to the chain's recorded output. The grid is a shared snap value from 1/2 to 1/32, straight, dotted or triplet: 15 values (`input_quantize_grids`); Snap's "Bar" is excluded. The grid is counted from tick 0 as `QuantizeSpec::apply` counts it: these are the snap *values*, not the editor's bar-relative placement (`app/Sources/VizDAWAppModel/Snap.swift`), so in 3/4 a 1/2 grid differs from Snap's. `passes` moves each recorded note's start by `QuantizeSpec::grid(grid, strength).apply` and keeps its length; controllers never move. — "Input Quantize then applied to note starts"; the length keeps the articulation. — A note's end moves with its start, up to half a grid.
13. **Where the chain runs.** In `Renderer::live_midi`, once per block, before the graph renders: queued input through `process`; then `tick` for every route (steps and gate ends with no input); then thru at the output's block offset, with `Out.ns_per_frame` set from the rate so external MIDI is timed inside the block (it was 0). A lane's route exists only while it is armed or monitored Input (`host::input_spec`, unchanged). Playback never meets the chain, and neither does a bounce: `core_engine::bounce` builds no input routes (spec §6.8, "Playback … never passes through input effects"). Step entry and note input hear the raw input after `surfaces::takes` (`crates/core-ffi/src/input.rs`), not the chain's output: never wire the chain into them. — §6.8's "sample-accurate and synced". — A step to a plugin instrument in a block that automation splits into runs may land one run late.
14. **The model's shapes.**
    - `InputFxKind` is externally tagged, snake_case: `{"arpeggiator": {...}}`, `{"transpose": {"semitones": 12}}`, `{"velocity": {...}}`, `{"chord": {"intervals": [4, 7]}}`, `{"input_quantize": {"grid": 3840, "strength": 100}}`. `InputFx` is `{"kind": …, "bypass": true}`, `bypass` absent when false. The same JSON is `lane.set_input_fx`'s `chain` and `state.get document`'s per-lane `input_fx`.
    - Defaults: Arpeggiator Up, 1/16 straight, 1 octave, gate 50 %, velocity As Played, Latch off; Transpose +12; Velocity scale 100 %, offset 0, min 1, max 127, not Fixed; Chord [4, 7]; Input Quantize 1/16 at 100 %.
    - Chord intervals are distinct, not 0, at most 6, each ±48. Velocity's offset is in 7-bit steps, `min ≤ max`, both 1–127; the result is computed on 16 bits: `clamp(v × scale ÷ 100 + offset × 512, up7(min), up7(max))`, or `up7(fixed)`.
    - `input_fx_from_json` refuses text over 64 KiB, but the channel already caps a text parameter at `MAX_STRING = 4096` bytes (`crates/core-control/src/params.rs`); a full 8-effect chain is about 1.6 KB, so the channel's cap is the one users meet.
    - Duplicate Track copies the chain (it clones the lane); Same Tracks as copies none, as it copies no input settings.
    - — One JSON shape everywhere, by serde. — A shape to migrate before release.
15. **Undo and recording.** `LaneCmd::SetInputFx` is one undo step, "Set Input Effects"; a gesture in the MIDI FX section or its settings popover (a slider drag, a stepper press, a drag to reorder, an add, a bypass) is one call. Chain edits are allowed while recording: they do not change which tracks record, so 5d-1 Ruling 18's guard (`retries_input_cmd`) does not apply, and the change is heard at once (Ruling 5). While recording, the edit joins the take's Record step, as every edit made during a take does (`crates/core-ffi/tests/record.rs`, 5c): one Undo removes the take and the chain change together. — Spec §6.8, "one step per gesture"; changing the rate mid-take is a performance move. — One line in `retries_input_cmd`.
16. **The strip's MIDI FX section (owner ruling 2026-11-09, binding; it replaces the first draft's FX button and popover on the track header).**
    - The track header is unchanged: no FX control, and the name field keeps its full width (it ends at x 109).
    - The chain is edited in the channel strip. A "MIDI FX" section sits beneath Inserts (`StripRowKind.midiFx`, a shared row like the others) on instrument strips only: a plugin instrument's main strip and the shared Built-in Synth strip. Audio paths, buses, master and an instrument's extra outputs have no section.
    - Its slots behave like insert slots, up to 8:
      - a click on the empty "+" slot pops up a menu of the five effects;
      - a click on a filled slot opens that effect's settings in a popover anchored to the slot;
      - the slot's dot is its bypass toggle;
      - a drag reorders the slots within the track's chain.
      The heading lights in `theme.accent.accent` (the active-toggle colour) when `InputFxModel.isLit` holds for the track the section edits.
    - **Which track (controller ruling).** One strip can serve several tracks: several tracks playing one instrument destination, or every track on the shared Built-in Synth strip. The section therefore follows the AudioPath input controls' precedent (5d-2 Ruling 4, `MixerModel.inputLane(for:)`):
      - in the Mix view, the bottom pane and the mixer window, it edits the first armed track of the shown sequence that plays through the strip, else the first such track;
      - in the left pane, it edits the focused track (`focusLanes().first`, the track that `synthFocus` and `StripFocus` follow).
      - With no such track, the section shows its heading and a disabled "+".
      `MixerModel.midiFxLane(for:in:)` holds the rule, so it is tested in `VizDAWAppModelTests`.
    - **The channel.** With the channel on (`MediaModel.channelIsOn()`), a click on the empty slot or a filled slot opens no menu and no popover. It opens the editor on the track, selects the slot and sets the status line to "MIDI FX opens a menu or a popover: use lane.set_input_fx over the control channel." This follows the `channelOn:` pattern of `SynthSheets.beginTrackSynth` and the "+ Add Track" row's status. The bypass dot opens nothing, so the channel may press it. The section has no context menu, and no SwiftUI `Menu`, because those open on the press before code can check the channel.
    - **Known gap.** A MIDI track that plays an external destination, or none, has no channel strip (`StripFocus` shows the inspector). Until the owner places it, such a track's chain is reachable only through `lane.set_input_fx`. The model and the channel still accept a chain on any lane, including an audio-kind one (a track's kind follows its routing and can change); the chain only runs on MIDI inputs.
    - — The owner's placement; the controller's precedent for strips that serve several tracks; the channel rule. — A view to move, and one placement rule in `midiFxLane`.
17. **Counters.** `Counters` gains `midi_fx_dropped`; `InputInfoRow` gains `midi_fx_dropped`, which `state.get input` reports as `"midi_fx_dropped"`. — §6.8's "counts it" needs a place to be read. — A field to move.
18. **File format 5 (pre-flight I1, ledger ruling).** `core_model::file::VERSION` goes 4 → 5 with `migrate_v4_to_v5`, the identity ("lanes gain an optional `input_fx`"), following 7b Ruling 8: a pre-5g build then refuses a project with a chain with its `NewerFormat` message rather than an unknown-field Corrupt. `Lane.input_fx` stays `#[serde(default, skip_serializing_if = "Vector::is_empty")]`, so a version-4 project opens with empty chains and a project without chains differs from before only in its version. — The ledger's ruling, 7b's precedent. — None beyond one migration step.

## Carried items

- No carry from 5f, SP7 or Tracks names input effects. 5f-1's consumption (Ruling 2) is relied on, not changed.
- The 5c hook `core_engine::input::InputChain` (an empty pass-through) is replaced by `core_engine::input_fx::InputChain` in Task 4; 5c's `MidiRoute.chain` keeps its name.
- `docs/feature-backlog.md` gains no rows: the first draft's two (learned controls not consumed; a pre-5g build calling a chain Corrupt) are void under Rulings 2 and 18.
- Task 10 amends spec §13's 5g-1 row (README 238–244; the surface-port clause delivered by 5f-1) and 5g-2's README numbers (245–250).

## File map

| File | Task | Role |
|---|---|---|
| `crates/core-model/src/input_fx.rs`, `src/lane.rs`, `src/command/lane.rs`, `src/rules.rs`, `src/lib.rs`, `src/file.rs`, `tests/input_fx.rs`, `tests/file.rs`, `tests/learn.rs`, `fixtures/demo.vizdaw/project.json` | 1 | the model, format 5 |
| `crates/core-engine/src/input_fx/mod.rs`, `src/input_fx/arp.rs` (stub), `src/lib.rs`, `tests/input_fx.rs` | 2 | `LiveMsg`, the chain, the stateless effects, the merge, the cap |
| `crates/core-engine/src/input_fx/arp.rs`, `src/input_fx/mod.rs`, `tests/input_fx_arp.rs` | 3 | the arpeggiator |
| `crates/core-engine/src/input.rs`, `src/engine.rs`, `src/render.rs`, `src/record.rs`, `tests/input_fx_live.rs`, `tests/rt_alloc_input_fx.rs`, `tests/live_input.rs`, `tests/record_tap.rs`, `tests/rt_alloc_record.rs`; `crates/core-ffi/src/host.rs`, `src/passes.rs`, `tests/passes.rs` (stop-gaps); `.github/workflows/ci.yml` | 4 | the chain in the renderer and the recorder |
| `crates/core-ffi/src/passes.rs`, `tests/passes.rs` | 5 | recording `LiveMsg`, Input Quantize |
| `crates/core-ffi/src/input_fx.rs`, `src/lib.rs`, `src/edit.rs`, `src/snapshot.rs`, `src/host.rs`, `src/live.rs`, `src/session.rs`, `tests/input_fx.rs`, `tests/record.rs`, `tests/learn.rs`, `tests/control.rs` | 6 | the FFI |
| `crates/core-control/src/registry.rs`, `tests/commands.rs`, `crates/core-ffi/src/control.rs`, `tests/control.rs`, `app/Sources/VizDAWAppModel/InputFxCommands.swift`, `ControlCommands.swift`, `app/Tests/VizDAWAppModelTests/InputFxCommandsTests.swift` | 7 | the channel |
| `app/Sources/VizDAWAppModel/InputFxModel.swift`, `DocumentModel.swift`, `app/Tests/VizDAWAppModelTests/InputFxModelTests.swift` | 8 | the MIDI FX editor's model |
| `app/Sources/VizDAWAppModel/InputFxModel.swift`, `MixerModel.swift`, `StripLayout.swift`, `app/Sources/VizDAW/ChannelStrip.swift`, `MixerView.swift`, `StripMidiFx.swift`, `InputFxSettings.swift`, `app/Tests/VizDAWAppModelTests/MidiFxSectionTests.swift` | 9 | the strip's MIDI FX section |
| `crates/vizctl/tests/e2e.rs`, `app/README.md`, `docs/superpowers/specs/2026-10-18-recording-audio-design.md` | 10 | e2e, README 238–244, the spec row |

---

### Task 1: The input effects model (`core-model`)

**Files:**
- Create: `crates/core-model/src/input_fx.rs`
- Modify: `crates/core-model/src/lib.rs`, `crates/core-model/src/lane.rs`, `crates/core-model/src/command/lane.rs`, `crates/core-model/src/rules.rs`, `crates/core-model/src/file.rs` (format 5, Ruling 18)
- Modify (existing version pins): `crates/core-model/tests/file.rs`, `crates/core-model/tests/learn.rs`; regenerate `fixtures/demo.vizdaw/project.json`
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
  - `core_model::file::VERSION == 5`, and `pub fn migrate_v4_to_v5(v: Value) -> Result<Value>` (the identity), last in `MIGRATIONS`.

- [ ] **Step 1: Write the failing tests**

```bash
cat > crates/core-model/tests/input_fx.rs <<'VIZDAW_EOF'
//! MIDI input effects in the model (plan 5g-1 Task 1; sub-project 5 spec
//! §4.5, §6.8; Rulings 12, 14, 15): a lane's chain of up to 8 effects is
//! one undoable lane edit, checked by range, saved only when not empty.

use core_model::ModelError;
use core_model::command::{Command, LaneCmd, SequenceCmd, apply};
use core_model::document::Document;
use core_model::file::{VERSION, from_json, migrate_v4_to_v5, to_json};
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
    assert_eq!(VERSION, 5);
}

/// Ruling 18: a version-4 project opens with empty chains; the step is the
/// identity.
#[test]
fn a_version_4_project_opens_with_empty_chains() {
    let d = doc();
    let mut v: Value = serde_json::from_str(&to_json(&d).unwrap()).unwrap();
    assert_eq!(v["version"], json!(5));
    v["version"] = json!(4);
    let back = from_json(&serde_json::to_string(&v).unwrap()).unwrap();
    assert_eq!(back, d);
    assert!(back.lane(SeqId(0), LaneId(0)).unwrap().input_fx.is_empty());
    assert!(to_json(&back).unwrap().contains("\"version\": 5"));
    assert_eq!(migrate_v4_to_v5(v.clone()).unwrap(), v);
}

/// Ruling 18: a version-5 file with a chain reads back in this build, and
/// a newer version is refused by its number, not as Corrupt.
#[test]
fn a_version_5_file_with_a_chain_reads_back_and_a_newer_one_names_its_version() {
    let d = apply(&doc(), &set(vec![transpose(7)])).unwrap().doc;
    let text = to_json(&d).unwrap();
    assert!(text.contains("\"version\": 5"));
    assert_eq!(from_json(&text).unwrap(), d);
    let mut v: Value = serde_json::from_str(&text).unwrap();
    v["version"] = json!(6);
    assert!(matches!(
        from_json(&serde_json::to_string(&v).unwrap()),
        Err(ModelError::NewerFormat { found: 6 })
    ));
}

/// Ruling 14: Duplicate Track clones the lane, chain included.
#[test]
fn duplicate_track_copies_the_chain() {
    let d = apply(&doc(), &set(vec![transpose(7)])).unwrap().doc;
    let d = apply(&d, &Command::Lane(LaneCmd::Duplicate { seq: SeqId(0), lane: LaneId(0) })).unwrap().doc;
    let lanes = &d.sequence(SeqId(0)).unwrap().lanes;
    assert_eq!(lanes.len(), 2);
    assert_eq!(lanes[1].input_fx, lanes[0].input_fx);
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
               LaneCmd::SetMonitor { .. } => "Set Track Monitor",
   ```
   Replace:
   ```rust
               LaneCmd::SetMonitor { .. } => "Set Track Monitor",
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

   Any `match` over `LaneCmd` elsewhere in the workspace that does not compile gets the new arm (`edit.rs`'s `retries_input_cmd` uses `matches!` and needs nothing; chain edits never retry the input, Ruling 15).

   **Format 5 (Ruling 18)**, following 7b's `migrate_v3_to_v4`:
   - `crates/core-model/src/file.rs`: `VERSION` 4 → 5; the module doc's `"version": 4` → 5; add `pub fn migrate_v4_to_v5(v: Value) -> Result<Value> { Ok(v) }` above `migrate_v3_to_v4`, documented "Version 4 → 5 (plan 5g-1 Ruling 18): lanes gain an optional `input_fx`, and a lane without one has an empty chain, so the document is unchanged. The bump makes an older build refuse a version-5 project with its `NewerFormat` message rather than an unknown-field error."; `MIGRATIONS` ends `migrate_v3_to_v4, migrate_v4_to_v5]`.
   - `crates/core-model/tests/file.rs::version_3_projects_migrate_to_version_4`: `assert_eq!(VERSION, 4);` becomes `assert!(VERSION >= 4);`, and `.contains("\"version\": 4")` becomes `.contains(&format!("\"version\": {VERSION}"))`.
   - `crates/core-model/tests/learn.rs:209`: `assert_eq!(core_model::file::VERSION, 4);` becomes 5, its comment gaining "; version 5 is plan 5g-1's input effects (Ruling 18)".
   - Regenerate the demo fixture: `cargo run -p vizdaw-play -- --write-demo fixtures/demo.vizdaw`. `git diff --stat fixtures/` shows only `project.json` (its `"version": 5`), and `cargo test -p vizdaw-play` passes (`checked_in_demo_matches_a_fresh_build`).

- [ ] **Step 4: Run the tests and the gates.** Run `cargo test --workspace` (the version pins live in several crates' fixtures), both clippies and `cargo fmt --all -- --check`. Expected: core-model +9.

- [ ] **Step 5: Commit**

```bash
git add crates/core-model/src/input_fx.rs crates/core-model/src/lib.rs crates/core-model/src/lane.rs crates/core-model/src/command/lane.rs crates/core-model/src/rules.rs crates/core-model/src/file.rs crates/core-model/tests/input_fx.rs crates/core-model/tests/file.rs crates/core-model/tests/learn.rs fixtures/demo.vizdaw/project.json
git commit -m "core-model: MIDI input effect chains on lanes, one undoable edit; file format 5 (plan 5g-1 Task 1)

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
      /// Notes dropped for capacity since the last call (Ruling 7). // RT
      pub fn take_dropped(&mut self) -> u64;
      /// Stage calls since the last call: the work bound's probe (Ruling 7). // RT
      #[doc(hidden)]
      pub fn take_visits(&mut self) -> u64;
  }
  impl Default for InputChain   // the empty chain
  impl Clone for InputChain     // config cloned, fresh state (control thread)
  impl PartialEq for InputChain // configs only
  impl Eq for InputChain
  impl std::fmt::Debug for InputChain // config only
  ```
- Algorithm (the stages, which no signature determines):
  - `InputChain::new` builds one `Stage` per non-bypassed effect except Input Quantize: `Transpose(i8)`, `Velocity(VelocityFx)`, `Chord([i8; 6], u8)` or `Arp(Box<ArpState>)`. It also builds the merge (`[u8; 128]` counts, `saturating_add`/`saturating_sub`, Ruling 6), a chain-local frame counter, and the dropped and visit counters.
  - `process` runs `run(stages, msg, at)`. A stage maps the message and calls `run(rest, …)` once per output:
    - Transpose maps `pitch()` by `with_pitch`; Chord emits the original, then each interval;
    - a pitch outside 0–127 is dropped (Ruling 8);
    - Velocity changes `NoteOn` velocities only (Ruling 14).

    Every `run` call adds one visit. At the end of the stages a message is a leaf: it takes one from a budget of `MAX_FANOUT`, and the merge applies Ruling 6, then calls `out`. When a leaf finds the budget at 0, the event is truncated: `dropped` gains 1 (once per event), and from then on `run` returns at once and every stage's loop over its outputs breaks (Ruling 7).
  - `tick(clock)` gives each Arp stage `i` the clock and an emitter that runs `run(stages[i+1..], …)`. Each step gets a fresh fan-out budget. After the stages, the frame counter advances by `clock.frames`.
  - `PerNote` messages pass Transpose and Chord like notes and pass Velocity unchanged. `Channel` messages pass every stage unchanged.

- [ ] **Step 1: Write the failing tests**

```bash
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
    // Counts saturate at 255: many hits end on the last off counted.
    let mut c = InputChain::default();
    let mut many = vec![on_(60, 100); 300];
    many.extend(vec![off_(60); 300]);
    assert_eq!(feed(&mut c, &many), vec!["on 60 v100", "off 60"]);
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
    assert_eq!(c.take_dropped(), 1, "one truncated event");
    assert_eq!(c.take_dropped(), 0, "taken once");
}

/// Ruling 7: the cap bounds the work, not only the outputs. 7^8 leaves are
/// never visited; the note-off ends exactly the notes the cap let start.
#[test]
fn eight_chained_chords_stop_at_the_cap() {
    let mut c = chain(vec![chord(&[1, 2, 3, 4, 5, 6]); 8]);
    let got = feed(&mut c, &[on_(30, 100)]);
    assert!(!got.is_empty() && got.len() <= MAX_FANOUT, "{}", got.len());
    assert_eq!(c.take_dropped(), 1);
    let visits = c.take_visits();
    assert!(visits <= (8 * 7 * MAX_FANOUT) as u64, "{visits} stage visits");
    let offs = feed(&mut c, &[off_(30)]);
    assert_eq!(offs.len(), got.len(), "every note it started ends: {offs:?}");
    assert!(offs.iter().all(|t| t.starts_with("off ")));
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

- [ ] **Step 4: Run the tests and the gates.** Run `cargo test -p core-engine --test input_fx`, both clippies and fmt. Expected: core-engine +14.

- [ ] **Step 5: Commit**

```bash
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
  - Steps follow Rulings 9 and 10. Synced steps are not stored: each block plays the grid lines `k × step` whose `tick_to_frame` falls in `[transport.1, transport.1 + frames)`, so a jump re-anchors with no catch-up (Ruling 10). Free-running steps come from the step's frames at `clock.bpm`, kept as the next free-running frame on the chain's counter.
  - Generated outputs carry `At { block: k, record: k as i64 + clock.record_lead }`.

- [ ] **Step 1: Write the failing tests**

```bash
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

/// Ruling 10: a locate, a wind or a loop wrap while keys are held
/// re-anchors the grid: no burst of missed steps, no silence.
#[test]
fn a_jump_while_held_neither_bursts_nor_stalls() {
    let frames = |o: Vec<(u64, u8)>| o.into_iter().map(|(f, _)| f).collect::<Vec<_>>();
    let mut c = arp(|_| {});
    let o = ons(&run(&mut c, &press(0, &[60, 64]), 23_000, true, 0));
    assert_eq!(frames(o), vec![0, 6000, 12_000, 18_000]);
    // Back to schedule frame 5000: the next sixteenth is 1000 frames in.
    let back = ons(&run(&mut c, &[], 12_000, true, 5000));
    assert_eq!(frames(back), vec![1000, 7000]);
    // Forward 50 steps and a half: nothing at offset 0, then the grid.
    let fwd = ons(&run(&mut c, &[], 12_000, true, 50 * 6000 + 3000));
    assert_eq!(frames(fwd), vec![3000, 9000]);
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

- [ ] **Step 4: Run the tests and the gates.** Run `cargo test -p core-engine --test input_fx --test input_fx_arp`, both clippies and fmt. Expected: core-engine +14.

- [ ] **Step 5: Commit**

```bash
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
- Modify (stop-gaps so the workspace builds): `crates/core-ffi/src/host.rs`, `crates/core-ffi/src/passes.rs`, `crates/core-ffi/tests/passes.rs`
- Modify: `.github/workflows/ci.yml` (the rt-check step runs the new test)
- Test: create `crates/core-engine/tests/input_fx_live.rs` and `crates/core-engine/tests/rt_alloc_input_fx.rs`

**Interfaces:**
- Consumes: Tasks 2–3's `InputChain`, `ChainClock`, `At`, `LiveMsg`, `RecordQuantize`.
- Produces:
  - `MidiInputSpec` gains `pub fx: Vec<core_model::input_fx::InputFx>` (the lane's chain).
  - `MidiRoute.chain: core_engine::input_fx::InputChain`. The old `core_engine::input::InputChain` is removed. `MidiRoute::matches` is unchanged (Ruling 3).
  - `pub fn carries(&self, old: &MidiRoute) -> bool` (Ruling 5).
  - `RecordedMidi { lane, run, msg: LiveMsg, quantize: Option<RecordQuantize> }`.
  - `RecordTap::midi(&mut self, lane: LaneId, offset: i64, msg: LiveMsg, quantize: Option<RecordQuantize>)` — `// RT`.
  - `Counters.midi_fx_dropped: u64`.
  - `Renderer::live_midi(&mut self, frames: u32)` (private) is called as `self.live_midi(frames)`. `fn emit_live(…)` is a private free function in `render.rs` that sends one output thru and to the tap.

- [ ] **Step 1: Write the failing tests**

```bash
cat > crates/core-engine/tests/input_fx_live.rs <<'VIZDAW_EOF'
//! The input effect chain in the renderer (plan 5g-1 Task 4; spec §6.6,
//! §6.8, §12.3; Rulings 5, 10–13): thru plays the chain's output at once
//! (Input Quantize never delays it), arpeggiator steps land on their frames stopped and playing,
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
use core_model::time::Tick;

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
    let fx = vec![
        InputFx::new(InputFxKind::Transpose { semitones: 12 }),
        InputFx::new(InputFxKind::InputQuantize { grid: Tick(Q / 4), strength: 100 }),
    ];
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
        thru: true, armed: true, fx,
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

   In `impl MidiRoute`, after `matches`, add:
   ```rust
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
                   // Carried targets as bits (Ruling 5).
                   const _: () = assert!(MAX_TARGETS <= 64);
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
   - Every `MidiInputSpec { … }` literal the compiler names gains `fx: Vec::new(),` (at `a292d2c`: five in `tests/live_input.rs`, at :72, :204, :389, :593 and :658). The one in `tests/rt_alloc_record.rs` (:119) gains `fx: vec![InputFx::new(InputFxKind::Transpose { semitones: 12 })],` instead (importing `core_model::input_fx::{InputFx, InputFxKind}`), so the record tap's allocation test runs `LiveMsg` through a real chain.
   - In `tests/live_input.rs`:
     - `m.msg.bytes[1]` (twice) becomes `m.msg.pitch().unwrap_or(0)`;
     - `assert_eq!(got[0].msg, note(true, 60));` becomes `assert_eq!(got[0].msg, LiveMsg::from_midi1(note(true, 60)));`, importing `core_engine::input_fx::LiveMsg`.
   - In `tests/record_tap.rs`, `t.midi(lane, k, msg(n))` becomes `t.midi(lane, k, LiveMsg::from_midi1(msg(n)), None)`, and `m.msg.bytes[1]` becomes `m.msg.pitch().unwrap_or(0)`.
   - No existing test expects `9n pp 00` thru for a velocity-0 note-on (Ruling 4); if a run shows one, it now expects `8n pp 00`, reported.
   - In `.github/workflows/ci.yml`, the "Real-time allocation tests (rt-check)" step's command ends `--test rt_alloc_bounce --test rt_alloc_input_fx`.

- [ ] **Step 4: Run the tests and the gates.**
   - Run `cargo test --workspace`, both clippies and fmt.
   - Run the rt-check gate (Global Constraints), `rt_alloc_bounce` and `rt_alloc_input_fx` included.
   - `core-ffi` will not compile yet where it builds `MidiInputSpec` or `RecordedMidi`. Stop-gaps, each replaced later: `fx: Vec::new()` in `host::input_spec` (Task 6 fills it); in `src/passes.rs`, a temporary `match` on `m.msg` that keeps the old behaviour (Task 5 replaces it); in `tests/passes.rs`, `fn midi` (:285) builds `RecordedMidi { lane, run, msg: LiveMsg::from_midi1(msg), quantize: None }`, importing `core_engine::input_fx::LiveMsg` (its only literal).
   - Expected: core-engine +8.

- [ ] **Step 5: Commit**

```bash
git add crates/core-engine/src/input.rs crates/core-engine/src/engine.rs crates/core-engine/src/render.rs crates/core-engine/src/record.rs crates/core-engine/tests/input_fx_live.rs crates/core-engine/tests/rt_alloc_input_fx.rs crates/core-engine/tests/live_input.rs crates/core-engine/tests/record_tap.rs crates/core-engine/tests/rt_alloc_record.rs crates/core-ffi/src/host.rs crates/core-ffi/src/passes.rs crates/core-ffi/tests/passes.rs .github/workflows/ci.yml
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
   - In `crates/core-ffi/tests/passes.rs` (whose `fn midi` Task 4 already changed), widen the import to `core_engine::input_fx::{LiveMsg, RecordQuantize}`, then append:

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

- [ ] **Step 2: Run them to see them fail** with `cargo test -p core-ffi --test passes`. Expected: the two new tests FAIL: the quantize is ignored, and poly pressure through Task 4's temporary `match` records as nothing.

- [ ] **Step 3: Implement** the Interfaces in `passes.rs`. The note body's velocity and release velocity are the `LiveMsg`'s 16-bit values; there is no up-scaling, since `from_midi1` already up-scaled them. Remove Task 4's temporary `match`.

- [ ] **Step 4: Run the tests and the gates.** Run `cargo test -p core-ffi --test passes --test record`, both clippies and fmt. The existing MIDI pass tests still pass, with the same velocities. Expected: core-ffi +2.

- [ ] **Step 5: Commit**

```bash
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
- Test: create `crates/core-ffi/tests/input_fx.rs`; modify `crates/core-ffi/tests/record.rs`, `crates/core-ffi/tests/learn.rs`, `crates/core-ffi/tests/control.rs` (its `InputInfoRow` literal only)

**Interfaces:**
- Consumes: Task 1's model, Task 4's `MidiInputSpec.fx` and `Counters.midi_fx_dropped`; 5f-1's consumption (`crate::surfaces::takes`, unchanged).
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
  - `host::input_spec` fills `fx: l.input_fx.iter().cloned().collect()`.

- [ ] **Step 1: Write the failing tests**

```bash
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

   Append to the same file (Ruling 15):

```rust
/// Plan 5g-1 Ruling 15: a chain edit made while recording joins the
/// take's Record step, as every edit made during a take does.
#[test]
fn a_chain_edit_while_recording_joins_the_record_step() {
    use vizdaw_ffi::input_fx::{InputFxKindRow, InputFxRow};
    use vizdaw_ffi::{EditCommand, LaneEdit};
    let g = serial(Some(FakeInput::Signals));
    let d = doc(2, true, false);
    let s = untitled(&g, d.doc.clone());
    let before = s.lanes(d.seq.0).unwrap();
    s.record(d.seq.0, spec()).unwrap();
    wait_until("beat 1", || s.transport().unwrap().tick >= Q);
    assert!(s.inject_midi("Keys".into(), vec![0x90, 60, 100]).unwrap());
    let chain = vec![InputFxRow { kind: InputFxKindRow::Transpose { semitones: 12 }, bypass: false }];
    s.edit(EditCommand::Lane { edit: LaneEdit::SetInputFx { seq: d.seq.0, lane: d.keys.0, chain } }, None)
        .unwrap();
    assert!(s.inject_midi("Keys".into(), vec![0x80, 60, 0]).unwrap());
    wait_until("beat 2", || s.transport().unwrap().tick >= 2 * Q);
    s.stop().unwrap();
    assert_eq!(s.snapshot().unwrap().undo_label.as_deref(), Some("Record"));
    s.undo().unwrap();
    assert_eq!(s.lanes(d.seq.0).unwrap(), before, "one Undo removes the take and the chain");
    s.close().unwrap();
}
```

   Then, in `crates/core-ffi/tests/learn.rs` (it has `serial`, `song`, `synth_fader`, `wait_until` and imports `cancel_learn`), append Ruling 2's chain half:

```rust
/// Plan 5g-1 Ruling 2 (the owner's rule): what a learn takes never reaches
/// an input chain. A waiting learn takes a note-on but not its note-off:
/// the chain hears only the orphan off, which its merge drops, so the
/// arpeggiator never holds the note. After the learn, a note plays.
#[test]
fn a_learned_message_never_reaches_an_input_chain() {
    use vizdaw_ffi::host::{debug_midi_from_source, drain_offline_log};
    use vizdaw_ffi::input_fx::{ArpModeRow, FeelRow, InputFxKindRow, InputFxRow};
    use vizdaw_ffi::{EditCommand, LaneEdit, LaneInputRow};
    let _g = serial();
    let s = Session::new_demo().unwrap();
    s.focus_surfaces().unwrap();
    let seq = song(&s);
    let chords = s.lanes(seq).unwrap().into_iter().find(|l| l.name == "Chords").unwrap().id;
    let edit = |e: LaneEdit| s.edit(EditCommand::Lane { edit: e }, None).unwrap();
    edit(LaneEdit::SetInput { seq, lane: chords, input: Some(LaneInputRow::Midi { port: None, channel: None }) });
    let row = |kind| InputFxRow { kind, bypass: false };
    let arp = InputFxKindRow::Arpeggiator {
        mode: ArpModeRow::Up, division: 16, feel: FeelRow::Straight, octaves: 1, gate: 50, velocity: None, latch: false,
    };
    edit(LaneEdit::SetInputFx {
        seq,
        lane: chords,
        chain: vec![row(InputFxKindRow::Chord { intervals: vec![4, 7] }), row(arp)],
    });
    edit(LaneEdit::SetFlags { seq, lane: chords, mute: None, solo: None, arm: Some(true) });
    s.play(seq).unwrap();
    wait_until("the engine", || s.transport().unwrap().playing);
    s.stop().unwrap();
    s.learn_target(synth_fader(&s)).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(50));
    drain_offline_log();
    debug_midi_from_source("Keys", 0, &[0x90, 60, 100]);
    debug_midi_from_source("Keys", 0, &[0x80, 60, 0]);
    cancel_learn();
    debug_midi_from_source("Keys", 0, &[0x90, 62, 100]);
    let is_note = |b: &Vec<u8>| b.first().is_some_and(|x| matches!(x & 0xF0, 0x80 | 0x90));
    let mut log = Vec::new();
    wait_until("the free-running arpeggio's first step", || {
        log.extend(drain_offline_log());
        log.iter().any(|(_, b): &(u64, Vec<u8>)| is_note(b) && b.get(1) == Some(&62))
    });
    assert!(
        !log.iter().any(|(_, b)| is_note(b) && b.get(1).is_some_and(|p| [60, 64, 67].contains(p))),
        "the taken note-on reached the chain: {log:?}"
    );
}
```

   And change `a_waiting_learn_lets_note_offs_through` (Ruling 6: a chain drops an off it never started): before its `s.learn_target(…)`, inject `debug_midi_from_source("Keys", 0, &[0x90, 60, 100])` and `&[0x90, 62, 100]`, then `wait_until` their thru is in `drain_offline_log()`; its closing `wait_until` then looks for note-off statuses (`b[0] & 0xF0 == 0x80`, which `90 3e 00` now plays as, Ruling 4) on 60 and 62. Its doc line gains "(plan 5g-1: the offs end notes the lane started)".

- [ ] **Step 2: Run them to see them fail** with `cargo test -p core-ffi --test input_fx --test record --test learn`. Expected: FAIL to compile, because `vizdaw_ffi::input_fx` is not found.

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
         #[uniffi(default)]
         pub audio_out_name: Option<String>,
     }
     ```
     Replace:
     ```rust
         #[uniffi(default)]
         pub audio_out_name: Option<String>,
         /// Its MIDI input effect chain (plan 5g-1).
         #[uniffi(default)]
         pub input_fx: Vec<crate::input_fx::InputFxRow>,
     }
     ```

     Find:
     ```rust
                     .and_then(|o| doc.outputs().get(&o).map(|p| p.name.clone())),
     ```
     Replace:
     ```rust
                     .and_then(|o| doc.outputs().get(&o).map(|p| p.name.clone())),
                 input_fx: l.input_fx.iter().map(Into::into).collect(),
     ```
   - **`host.rs` `input_spec`** (:789): Task 4's `fx: Vec::new()` becomes `fx: l.input_fx.iter().cloned().collect()`. `pub fn counters(owner: u64) -> Option<core_engine::render::Counters>` follows `input_levels(owner)` (:3623): the owner's engine `counters()`, or `None`.
   - **`live.rs` and `session.rs`:** `InputInfoRow` gains `#[uniffi(default)] pub midi_fx_dropped: u64` after `sample_rate`. Both branches of `Session::input_info` (`session.rs:7765` and `:7776`) fill it: `host::counters(self.id).map_or(0, |c| c.midi_fx_dropped)`, and 0. `tests/control.rs:527`'s literal gains `midi_fx_dropped: 0` (Rust has no `#[uniffi(default)]`); Task 7 extends that file further.

- [ ] **Step 4: Run the tests and the gates.**
   - Run `cargo test --workspace`, both clippies, `cargo clippy -p core-ffi --lib -- -D warnings` and fmt.
   - Then run `scripts/build-ffi.sh`, `scripts/swift.sh build` and `scripts/swift.sh test`. Swift compiles unchanged: the new fields have defaults, and no Swift `switch` over `LaneEdit` is exhaustive without `default`. If one is, add the case and say so.
   - Expected: core-ffi +8.

- [ ] **Step 5: Commit**

```bash
git add crates/core-ffi/src/input_fx.rs crates/core-ffi/src/lib.rs crates/core-ffi/src/edit.rs crates/core-ffi/src/snapshot.rs crates/core-ffi/src/host.rs crates/core-ffi/src/live.rs crates/core-ffi/src/session.rs crates/core-ffi/tests/input_fx.rs crates/core-ffi/tests/record.rs crates/core-ffi/tests/learn.rs crates/core-ffi/tests/control.rs
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
  - Registry `lane.set_input_fx` (group "Track", title "Set Input Effects", no key; the other `lane.*` commands' group since the Tracks plan) with params:
    - `id_param("seq", "The track's sequence.")` and `id_param("lane", "The track.")`, as `SET_INPUT_PARAMS` spells them (`core-control/tests/names.rs` refuses "lane" in help text);
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
     - the registry count assertion (`tests/commands.rs:533`) moves from 162 to 163, its message gaining "; 5g-1 adds lane.set_input_fx";
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

   - In `crates/core-ffi/tests/control.rs`:
     - `lanes_show_takes_input_and_monitor` (:597): `lanes[0]` also gets `input_fx: vec![InputFxRow { kind: InputFxKindRow::Transpose { semitones: 12 }, bypass: false }]`, and the test asserts `l[0]["input_fx"] == json!([{"kind": {"transpose": {"semitones": 12}}}])` and `l[1]["input_fx"] == json!([])`;
     - `record_and_input_results_as_json` (:474): the `InputInfoRow` literal's `midi_fx_dropped: 0` (Task 6) becomes 3, and the test asserts `i["midi_fx_dropped"] == json!(3)`.
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

   `FakeSession.edits` and `editError` (`FakeSession.swift:818-819`), `demoFake()` and `DocumentStore(makeEmpty:makeDemo:open:)` are the tree's, as `AutomationCommandsTests` uses them.

- [ ] **Step 2: Run them to see them fail.** Run `cargo test -p core-control --test commands` and `cargo test -p core-ffi --test control`; then `scripts/build-ffi.sh` and `scripts/swift.sh test`. Expected: FAIL. The ID is unknown, there are no JSON keys, and `InputFxCommands` is not found.

- [ ] **Step 3: Implement.**
   - **The registry:** a `SET_INPUT_FX_PARAMS` constant beside `SET_INPUT_PARAMS`, and a `CommandSpec` after `lane.set_monitor`'s with `key_only(cmd("lane.set_input_fx", "Track", "Set Input Effects", None, true))`.
   - **`control.rs`:** the two JSON keys.
   - **`InputFxCommands.swift`:** the interface above. `ids` joins `ControlCommands.handled`: its last line `.union(ViewCommands.ids).union(AppearanceCommands.ids).union(TrackCommands.ids)` gains `.union(InputFxCommands.ids)`. Dispatch in `run`, beside SynthCommands:

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

- [ ] **Step 4: Run the tests and the gates.** Run the Rust tests for `core-control` and `core-ffi`, both clippies, `cargo clippy -p core-ffi --lib -- -D warnings`, `scripts/build-ffi.sh`, `scripts/swift.sh build` and `scripts/swift.sh test`. `everyRegisteredMenuCommandHasAHandler` passes. Expected: core-control +1, core-ffi +0 (two tests extended), Swift +2, registry 163.

- [ ] **Step 5: Commit**

```bash
git add crates/core-control/src/registry.rs crates/core-control/tests/commands.rs crates/core-ffi/src/control.rs crates/core-ffi/tests/control.rs app/Sources/VizDAWAppModel/InputFxCommands.swift app/Sources/VizDAWAppModel/ControlCommands.swift app/Tests/VizDAWAppModelTests/InputFxCommandsTests.swift
git commit -m "control: lane.set_input_fx, input_fx in state.get document, midi_fx_dropped in state.get input (plan 5g-1 Task 7)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD
```

---

### Task 8: `InputFxModel`, the MIDI FX editor's model

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
      /// Its document, as `LaneInputs` holds it (no retain cycle).
      @ObservationIgnored public weak var document: DocumentModel?
      public init(document: DocumentModel)
      /// The lane whose chain the editor shows; nil when closed.
      public private(set) var lane: UInt64?
      /// The selected row.
      public var selected: Int?
      /// The selected effect while a slider drags (committed at its end).
      public private(set) var draft: InputFxKindRow?
      public func open(lane: UInt64)
      public func close()
      /// The open lane's chain, the draft shown in place of the selected row;
      /// `[]` when the lane is no longer in `document.lanes` (an Undo removed it).
      public var chain: [InputFxRow] { get }
      /// The strip's MIDI FX section edits MIDI and instrument tracks, not audio ones (Ruling 16).
      nonisolated public static func showsSection(_ lane: LaneRow) -> Bool
      /// `showsSection`, and the chain has an effect that is not bypassed.
      nonisolated public static func isLit(_ lane: LaneRow) -> Bool
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
  - Every edit is `document.editor.edit(.lane(edit: .setInputFx(seq:lane:chain:)))` on the selected sequence. A refusal leaves the status line set by `EditorModel`. With the lane gone, an edit sends nothing.
  - `DocumentModel.select(_:)` calls `inputFx.close()` when the sequence changes (only if the model was made: `inputFxModel?.close()`).
  - The status texts are "A chain holds at most 8 effects." and "A chain holds one Input Quantize."

- [ ] **Step 1: Write the failing tests**

```bash
cat > app/Tests/VizDAWAppModelTests/InputFxModelTests.swift <<'VIZDAW_EOF'
import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

/// The input effects editor's model (plan 5g-1 Task 8; spec §6.8 UI):
/// add, remove, reorder, bypass and settings, each one undoable lane edit;
/// a slider drag is one edit at its end; the strip's MIDI FX section lit state.
@MainActor
@Suite(.serialized) struct InputFxModelTests {
    /// Song's Chords lane (31) with `chain`, the editor open on it.
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

    @Test func theSectionIsLitForAnActiveChain() {
        var l = lane(31, "Chords")
        #expect(!InputFxModel.isLit(l))
        l.inputFx = [InputFxRow(kind: .transpose(semitones: 3), bypass: true)]
        #expect(!InputFxModel.isLit(l), "all bypassed")
        l.inputFx.append(InputFxRow(kind: .chord(intervals: [4, 7]), bypass: false))
        #expect(InputFxModel.isLit(l))
        #expect(InputFxModel.showsSection(l))
        var audio = lane(40, "Vox", kind: .audio)
        audio.inputFx = l.inputFx
        #expect(!InputFxModel.showsSection(audio) && !InputFxModel.isLit(audio), "no MIDI FX section on an audio track")
    }

    /// Pre-flight I11: the editor's model follows the document. A lane an
    /// Undo removed shows no chain and takes no edit; another sequence
    /// closes it.
    @Test func theChainFollowsAnUndoneLaneAndClosesWithTheSequence() throws {
        let (m, f) = try model([up12])
        #expect(m.inputFx.chain == [up12])
        f.laneTable[3]?.removeAll { $0.id == 31 }
        var s = f.snap
        s.version += 1
        m.apply(s)
        #expect(m.inputFx.chain.isEmpty)
        let before = f.edits.count
        m.inputFx.add(.transpose)
        #expect(f.edits.count == before)
        let (m2, _) = try model([up12])
        m2.select(2)
        #expect(m2.inputFx.lane == nil)
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

- [ ] **Step 3: Implement** `InputFxModel` and `DocumentModel.inputFx` (the `inputs` pattern, `DocumentModel.swift:130,149-154`: an `@ObservationIgnored private var inputFxModel: InputFxModel?` made lazily with `InputFxModel(document: self)`), and the `close()` in `select`. `summary` follows the test's spellings: the minus sign is U+2212, and the octave word is "oct". No `@State`; nothing here touches AppKit.

- [ ] **Step 4: Run the tests and the gates** with `scripts/swift.sh build` and `scripts/swift.sh test`. Expected: Swift +6.

- [ ] **Step 5: Commit**

```bash
git add app/Sources/VizDAWAppModel/InputFxModel.swift app/Sources/VizDAWAppModel/DocumentModel.swift app/Tests/VizDAWAppModelTests/InputFxModelTests.swift
git commit -m "app: the input effects model: add, remove, reorder, bypass, settings, one edit per gesture (plan 5g-1 Task 8)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD
```

---

### Task 9: The strip's MIDI FX section

The owner's ruling (2026-11-09, binding): the chain is edited in the channel strip, in a "MIDI FX" section beneath Inserts, not from the track header. The header is not touched, so its name field keeps its full width (Ruling 16).

**Files:**
- Modify: `app/Sources/VizDAWAppModel/InputFxModel.swift` (the section's clicks, the settings slot, `chain(of:)`, drag and drop; `open` and `close` clear the settings slot), `app/Sources/VizDAWAppModel/MixerModel.swift` (`showsMidiFx`, `midiFxLane(for:in:)`), `app/Sources/VizDAWAppModel/StripLayout.swift` (`StripRowKind.midiFx`), `app/Sources/VizDAW/ChannelStrip.swift` (the row in `StripCells`; `StripHeading` gains `lit`; it and `StripAddFace` lose `private`; the doc line's row list), `app/Sources/VizDAW/MixerView.swift` (`InstrumentMenuAction` loses `private`; `MidiFxMenu` beside `InstrumentMenu`)
- Create: `app/Sources/VizDAW/StripMidiFx.swift` (the section and its slots), `app/Sources/VizDAW/InputFxSettings.swift` (the settings popover's content)
- Test: create `app/Tests/VizDAWAppModelTests/MidiFxSectionTests.swift`
- Not touched: `LaneHeaderView.swift`, `ArrangementLayout.swift`, `EditorTheme.swift`, `VizDAWApp.swift`.

**Interfaces:**
- Consumes: `InputFxModel` (Task 8: `open`, `close`, `selected`, `chain`, `showsSection`, `isLit`, `title`, `summary`, `defaultKind`, `quantizeGrids`, `add`, `remove`, `move`, `setBypass`, `update`, the draft calls). Also used: `StripFocus.strip(of:destinations:strips:)` (`PaneContent.swift`), `DocumentModel.focusLanes()`, `MixerModel.inputLane(for:)`'s rule (`MixerModel.swift:199-208`), `MediaModel.channelIsOn` and `SynthSheets.beginTrackSynth`'s `channelOn:` pattern (`SynthSheets.swift:77-82`), the inserts' slot look (`SlotView`, `MixerView.swift:220-262`), the inserts' popover binding (`StripInserts`, `ChannelStrip.swift:515-555`) and `InstrumentMenu.popUp` (`MixerView.swift:352-387`).
- Produces, in `VizDAWAppModel`:
  ```swift
  // StripLayout.swift: the section's row, between the inserts and the sends.
  public enum StripRowKind: Int, CaseIterable, Sendable {
      case header, instrument, input, eq, inserts, midiFx, sends, output, pan
  }

  extension MixerModel {  // or in the class body
      /// Whether `strip` has the MIDI FX section (Ruling 16): a plugin
      /// instrument's main strip or the shared Built-in Synth strip; not an
      /// instrument's extra output, an audio path, a bus or master.
      public func showsMidiFx(_ strip: StripRow) -> Bool
      /// The track `strip`'s MIDI FX section edits (Ruling 16); nil when
      /// the strip has no section or no track plays through it. A track
      /// plays through `strip` when `StripFocus.strip(of:)` names it and
      /// `InputFxModel.showsSection` holds. The left pane (`.strip`): the
      /// focused track (`document.focusLanes().first`) when it plays
      /// through `strip`. Every other placement: of the shown sequence's
      /// tracks that play through `strip`, the first armed one, else the
      /// first (5d-2 Ruling 4, as `inputLane(for:)`).
      public func midiFxLane(for strip: StripRow, in placement: MixerPlacement) -> LaneRow?
  }

  // InputFxModel.swift, in the class body:
  /// Where a slot's settings popover shows: one at a time (Ruling 16).
  public struct SettingsSlot: Equatable, Sendable {
      public let placement: MixerPlacement
      public let strip: UInt64
      public let index: Int
  }
  /// The slot whose settings popover is shown; nil when none is.
  public private(set) var settings: SettingsSlot?
  public static let sectionOnChannel =
      "MIDI FX opens a menu or a popover: use lane.set_input_fx over the control channel."
  /// `lane`'s chain as its slots show it: `chain` (the draft in place) when
  /// `lane` is the open one, else `lane.inputFx`.
  public func chain(of lane: LaneRow) -> [InputFxRow]
  /// A click on the empty slot. Opens the editor on `lane` when another
  /// lane (or none) is open. Answers whether the caller may pop up the
  /// effect menu: with the channel on it may not, and the status line is
  /// `sectionOnChannel`.
  public func clickEmpty(lane: UInt64, channelOn: Bool = MediaModel.channelIsOn()) -> Bool
  /// A click on slot `i`: opens on `lane` as above and selects `i`. Then it
  /// shows `i`'s settings in `placement`; with the channel on it sets the
  /// status line instead.
  public func clickSlot(
      _ i: Int, lane: UInt64, strip: UInt64, in placement: MixerPlacement,
      channelOn: Bool = MediaModel.channelIsOn())
  public func showsSettings(strip: UInt64, index: Int, in placement: MixerPlacement) -> Bool
  /// The popover closed. A draft still open is committed (one edit, as
  /// `commitDraft`), and `settings` becomes nil. The editor stays open.
  public func closeSettings()
  /// The slot's bypass dot: opens on `lane` as above, then `setBypass(i, !bypass)`.
  /// It is a plain toggle that opens nothing, so the channel may press it.
  public func toggleBypass(_ i: Int, lane: UInt64)
  /// A slot's drag payload: "midifx:<lane>:<index>".
  nonisolated public static func dragText(lane: UInt64, index: Int) -> String
  /// A payload dropped on slot `i` of `lane`. The dragged slot lands at `i`:
  /// one `move(from: [from], to: from < i ? i + 1 : i)`. Refused (false,
  /// no edit) for another lane's slot or a plugin slot's "slot:<id>".
  public func drop(_ text: String, onto i: Int, lane: UInt64) -> Bool
  ```
  `open(lane:)` and `close()` also set `settings = nil`, so `DocumentModel.select(_:)` closing the editor (Task 8) closes the popover too.
- Produces, in the app (`VizDAW`):
  - In `StripCells.body`, after the inserts' row: `if model.showsMidiFx(strip) { row(.midiFx, band: .dark) { StripMidiFx(model: model, strip: strip) } }`. The row shares its height across strips as every row does (`StripGrid`), and a strip without the section leaves it empty.
  - `StripHeading(text:lit:)`, where `lit` defaults to false. A lit heading is drawn in `theme.accent.accent.color`, the theme's active-toggle colour that the selected strip's edge also uses; unlit, in `theme.text.secondary.color` as now.
  - `struct StripMidiFx: View`, created in `StripMidiFx.swift`. It reads `@Environment(\.mixerPlacement)` and `\.symbioticTheme`, and gets `let lane = model.midiFxLane(for: strip, in: placement)` and `let fx = model.document.inputFx`. Its layout is `StripInserts`'s: a `VStack(alignment: .leading, spacing: theme.spacing.xxs)` holding:
    - `StripHeading(text: "MIDI FX", lit: lane.map(InputFxModel.isLit) ?? false)`;
    - with a lane, one `MidiFxSlot` per row of `fx.chain(of: lane)`. Then, while the chain has fewer than 8 rows, the empty slot: a `Button` showing `StripAddFace(text: "+")`, whose action is `if fx.clickEmpty(lane: lane.id) { MidiFxMenu.popUp(fx) }`. Its help is "Add a MIDI effect" and its VoiceOver label "Add a MIDI effect to \(lane.name)";
    - with no lane, the same face in a disabled button, with the help "No track in this sequence plays through this strip.";
    - on the whole section, `.accessibilityElement(children: .contain)`, the label "MIDI FX of \(lane?.name ?? strip.name)" and the value "on" when lit, else "off".
  - `struct MidiFxSlot: View`, in the same file, looks like `SlotView`: an `HStack(spacing: theme.spacing.xs)` on `theme.surface.inset` with a `theme.surface.separator` edge, `theme.radius.s` corners and `minHeight: 20`.
    - The bypass dot is a `.plain` `Button` holding a 6 × 6 `Circle`, filled with `theme.state.play.color` when the effect is on and `theme.text.disabled.color` when bypassed. Its frame is `ControlMetrics.hitSize(.toggle)`, and its action is `fx.toggleBypass(i, lane: lane.id)`.
    - `InputFxModel.title(kind)` in `theme.type.caption.font`, with `.strikethrough(bypass)`, then `InputFxModel.summary(kind)` in `theme.text.secondary.color`. Both use `.lineLimit(1)`.
    - `.onTapGesture { fx.clickSlot(i, lane: lane.id, strip: strip.id, in: placement) }`.
    - `.popover(isPresented: Binding(get: { fx.showsSettings(strip: strip.id, index: i, in: placement) }, set: { if !$0 { fx.closeSettings() } })) { InputFxSettings(model: fx) }`, presented as `StripInserts` presents its browser.
    - `.draggable(InputFxModel.dragText(lane: lane.id, index: i))` and `.dropDestination(for: String.self) { items, _ in items.first.map { fx.drop($0, onto: i, lane: lane.id) } ?? false }`.
    - For VoiceOver: `.accessibilityElement(children: .ignore)`, the label "\(title), \(summary)", the value "bypassed" or "on", `.isButton`, the hint "Opens its settings", and `.accessibilityAction(named: bypass ? "Turn On" : "Bypass") { fx.toggleBypass(i, lane: lane.id) }`.
    - No `.contextMenu`: a right-click from the channel would open it. Bypass is on the dot, and Remove is in the popover.
  - `@MainActor enum MidiFxMenu { static func popUp(_ fx: InputFxModel) }` in `MixerView.swift`. It is an `NSMenu` (`autoenablesItems = false`) with one item per `InputFxName.allCases`, titled `InputFxModel.title(InputFxModel.defaultKind(name))`, and each item calls `fx.add(name)`. It reuses `InstrumentMenuAction` and pops up at `NSEvent.mouseLocation`, as `InstrumentMenu.popUp` does. A SwiftUI `Menu` is not used: it opens on the press, before any code can apply the channel rule.
  - `struct InputFxSettings: View` in `InputFxSettings.swift` shows the effect `fx.selected` of `fx.chain`, or nothing when that index is gone:
    - a top row: the title in `theme.type.caption.font.weight(.semibold)`, a bypass `Toggle` (→ `setBypass`), and a Remove `.plain` button drawn with `AppIcon.remove` as `EqSlotView` draws `AppIcon.eq`. Remove calls `fx.remove(at: i)` and then `fx.closeSettings()`;
    - then the effect's controls. Arpeggiator: a mode `Picker`; rate `Picker`s (1/1 to 1/64; straight, dotted or triplet); an octaves `Stepper` 1–4; a gate `Slider` 5–200; a velocity `Picker` (As Played or Fixed) with a `Stepper` 1–127; a Latch `Toggle`. Transpose: a `Stepper` −48…48. Velocity: a scale `Slider` 0–400, an offset `Slider` −127…127, min and max `Stepper`s, and a Fixed `Toggle` with a `Stepper`. Chord: up to 6 interval `Stepper`s with add and remove (`AppIcon.add`, `AppIcon.remove`). Input Quantize: a grid `Picker` from `quantizeGrids` and a strength `Slider` 0–100;
    - sliders call `beginDraft` and `commitDraft` from `onEditingChanged`, and `setDraft` while they move. Pickers, steppers and toggles call `update`;
    - the theme and scan rules (7b, 7d). It reads `@Environment(\.symbioticTheme) private var theme`, and every spacing, radius, font and colour comes from it (`theme.spacing.*`, `theme.radius.*`, `theme.type.*`, as `RecordControls.swift` does). So: no `.padding()` without a token, no `spacing:` literal, no `.font(.caption)`, no `.bold()`, no colour literal. Icons come only through `AppIcon`, never `Image(systemName:)` or `systemImage:`. Typed numbers, if any, use the design package's `NumberBox` as `ChannelStrip.swift:633` does, never a `TextField`. No `@State`: the selection and the draft live in `InputFxModel`.

- [ ] **Step 1: Write the failing tests**

```bash
cat > app/Tests/VizDAWAppModelTests/MidiFxSectionTests.swift <<'VIZDAW_EOF'
import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

/// The strip's MIDI FX section (plan 5g-1 Task 9, Ruling 16; owner
/// 2026-11-09): which strips have it, which track it edits when one strip
/// serves several, and that a control-channel click opens nothing.
@MainActor
@Suite(.serialized) struct MidiFxSectionTests {
    let up12 = InputFxRow(kind: .transpose(semitones: 12), bypass: false)
    let down5 = InputFxRow(kind: .transpose(semitones: -5), bypass: false)

    /// Song (3): Keys A (30) and Keys B (31) play the built-in synth
    /// (destination 4, strip 1); Piano (32) plays a plugin instrument
    /// (destination 7, strip 5); Vocal (33) plays audio path 8 (strip 6).
    /// Bus 1 is strip 2, Master strip 0.
    func mixer(armB: Bool = false) throws -> (MixerModel, FakeSession) {
        let f = mixerFake()
        f.dests = [
            DestRow(id: 4, name: "Built-in Synth", plugin: nil),
            DestRow(
                id: 7, name: "Piano",
                plugin: PluginRefRow(format: .clap, id: "x.piano", name: "Piano", vendor: "V", version: "1")),
        ]
        f.mixerSnap.strips.insert(stripRow(5, "Piano", kind: .instrument(dest: 7)), at: 1)
        f.mixerSnap.strips.insert(stripRow(6, "Vocal", kind: .audioPath(path: 8)), at: 2)
        var a = lane(30, "Keys A")
        a.midiDest = 4
        var b = lane(31, "Keys B", kind: .instrument)
        b.midiDest = 4
        b.arm = armB
        var p = lane(32, "Piano")
        p.midiDest = 7
        var v = lane(33, "Vocal", kind: .audio)
        v.audioOut = 8
        f.laneTable[3] = [a, b, p, v]
        let m = try MixerModel(document: DocumentModel(session: f))
        m.document.select(3)
        return (m, f)
    }

    func strip(_ m: MixerModel, _ id: UInt64) throws -> StripRow {
        try #require(m.snapshot.strips.first { $0.id == id })
    }

    @Test func aStripTwoTracksShareEditsTheArmedOneInTheMixView() throws {
        let (m0, _) = try mixer()
        #expect(m0.midiFxLane(for: try strip(m0, 1), in: .view)?.id == 30, "none armed: the first")
        let (m, f) = try mixer(armB: true)
        let synth = try strip(m, 1)
        for p in [MixerPlacement.view, .pane, .window] {
            #expect(m.midiFxLane(for: synth, in: p)?.id == 31, "\(p): the armed one")
        }
        #expect(m.midiFxLane(for: try strip(m, 5), in: .view)?.id == 32, "a plugin instrument's own track")
        f.laneTable[3]?[1].inputFx = [up12]
        f.snap.version += 1
        m.document.refreshAfterJob()
        let edited = try #require(m.midiFxLane(for: synth, in: .view))
        #expect(InputFxModel.isLit(edited), "the section lights for the track it edits")
    }

    @Test func theLeftPaneEditsTheFocusedTrack() throws {
        let (m, _) = try mixer(armB: true)
        let synth = try strip(m, 1)
        #expect(m.midiFxLane(for: synth, in: .strip)?.id == 31, "nothing selected: the armed track is followed")
        m.document.editor.selectTrack(seq: 3, lane: 30, extend: false)
        #expect(m.midiFxLane(for: synth, in: .strip)?.id == 30, "the selected track, though another is armed")
        #expect(m.midiFxLane(for: synth, in: .view)?.id == 31, "the Mix view still edits the armed one")
        m.document.editor.selectTrack(seq: 3, lane: 32, extend: false)
        #expect(m.midiFxLane(for: synth, in: .strip) == nil, "the focused track plays another strip")
        #expect(m.midiFxLane(for: try strip(m, 5), in: .strip)?.id == 32)
    }

    @Test func audioStripsAndBusesHaveNoSection() throws {
        let (m, _) = try mixer()
        for id: UInt64 in [6, 2, 0] {  // Vocal (an audio path), Bus 1, Master
            let s = try strip(m, id)
            #expect(!m.showsMidiFx(s), "\(s.name)")
            #expect(m.midiFxLane(for: s, in: .view) == nil, "\(s.name)")
            #expect(m.midiFxLane(for: s, in: .strip) == nil, "\(s.name)")
        }
        #expect(!m.showsMidiFx(stripRow(9, "Piano 2", kind: .instrumentOutput(dest: 7, index: 1))), "an extra output")
        // An instrument strip that no track of the shown sequence plays:
        // the section is there, with nothing to edit.
        m.document.select(2)
        let synth = try strip(m, 1)
        #expect(m.showsMidiFx(synth))
        #expect(m.midiFxLane(for: synth, in: .view) == nil)
    }

    /// Programmatic calls open nothing (Global Constraints): with the
    /// channel on, a click on the empty slot or a filled one opens no menu
    /// and no popover, and edits nothing.
    @Test func aClickFromTheChannelOpensNothing() throws {
        let f = editorFake()
        f.laneTable[3]?[1].inputFx = [up12]
        let m = try DocumentModel(session: f)
        m.select(3)
        let fx = m.inputFx
        let before = f.edits.count
        #expect(!fx.clickEmpty(lane: 31, channelOn: true), "no effect menu")
        #expect(m.editor.status == InputFxModel.sectionOnChannel)
        #expect(fx.lane == 31)
        fx.clickSlot(0, lane: 31, strip: 1, in: .view, channelOn: true)
        #expect(fx.settings == nil, "no settings popover")
        #expect(fx.selected == 0)
        #expect(f.edits.count == before, "a click edits nothing")
        // The bypass dot is a plain toggle: the channel may press it.
        fx.toggleBypass(0, lane: 31)
        #expect(f.edits.count == before + 1)
        guard case .lane(edit: .setInputFx(seq: 3, lane: 31, let chain))? = f.edits.last else {
            Issue.record("no chain edit: \(f.edits)")
            return
        }
        #expect(chain == [InputFxRow(kind: up12.kind, bypass: true)])
    }

    @Test func aUsersClickShowsTheSlotsSettingsWhereItWasClicked() throws {
        let f = editorFake()
        f.laneTable[3]?[1].inputFx = [up12, down5]
        let m = try DocumentModel(session: f)
        m.select(3)
        let fx = m.inputFx
        #expect(fx.clickEmpty(lane: 31, channelOn: false), "the effect menu may open")
        // Dragging a slot moves it within its own track's chain only.
        #expect(InputFxModel.dragText(lane: 31, index: 1) == "midifx:31:1")
        var before = f.edits.count
        #expect(!fx.drop("midifx:30:1", onto: 0, lane: 31), "another track's slot")
        #expect(!fx.drop("slot:5", onto: 0, lane: 31), "a plugin slot")
        #expect(f.edits.count == before)
        #expect(fx.drop("midifx:31:1", onto: 0, lane: 31))
        guard case .lane(edit: .setInputFx(seq: 3, lane: 31, let moved))? = f.edits.last else {
            Issue.record("no chain edit: \(f.edits)")
            return
        }
        #expect(moved == [down5, up12])
        // A click on a filled slot shows its settings in that placement only.
        fx.clickSlot(0, lane: 31, strip: 1, in: .view, channelOn: false)
        #expect(fx.settings == InputFxModel.SettingsSlot(placement: .view, strip: 1, index: 0))
        #expect(fx.showsSettings(strip: 1, index: 0, in: .view))
        #expect(!fx.showsSettings(strip: 1, index: 0, in: .strip), "one popover, where it was clicked")
        // A slider's draft shows in the slot and is committed once when the popover closes.
        before = f.edits.count
        fx.beginDraft()
        fx.setDraft(.transpose(semitones: -3))
        #expect(fx.chain(of: m.lanes[1]).first?.kind == .transpose(semitones: -3), "the slot shows the draft")
        #expect(fx.chain(of: m.lanes[0]).isEmpty, "another track shows its own chain")
        fx.closeSettings()
        #expect(fx.settings == nil)
        #expect(f.edits.count == before + 1)
        // Another sequence closes the popover with the editor.
        fx.clickSlot(0, lane: 31, strip: 1, in: .view, channelOn: false)
        m.select(2)
        #expect(fx.settings == nil)
    }
}
VIZDAW_EOF
```

   `mixerFake()` and `editorFake()` both build on `demoFake()`: Song is sequence 3, and sequence 2 (Groove) has no track that plays an instrument. Lanes are refetched after a version bump with `refreshAfterJob()`, as `MixerModelTests.anAudioPathStripsInputIsItsLanes` does (`MixerModelTests.swift:38-61`). The fake records edits and does not apply them, so `drop` moves the committed `[up12, down5]`.

- [ ] **Step 2: Run them to see them fail** with `scripts/swift.sh test`. Expected: FAIL to compile, because `midiFxLane` is not found.

- [ ] **Step 3: Implement** the Interfaces, model first.
   - `midiFxLane` reads `destinations` and `snapshot.strips` (the whole list, master included, as `StripFocus` takes it) and `document.lanes`.
   - The click methods compare `self.lane` with `lane` before calling `open(lane:)`, so a second click on the same track keeps the selection and the draft.
   - Then the views. `StripRowKind.midiFx`'s place in `allCases` sets the row's place in `StripGrid.tops`. `StripGridPlacementTests` loops over `allCases`, so it covers the new case.
   - Update `ChannelStrip`'s doc line ("… the EQ slot, the inserts, the MIDI FX section on instrument strips, the sends …").

- [ ] **Step 4: Run the tests and the gates:** `scripts/swift.sh build`, `scripts/swift.sh test`, the no-brand build and `scripts/make-app.sh`. The scans must pass, by name: `ThemeScanTests.everyViewTakesItsSpacingFromTheTheme`, `noColourLiteralsOutsideTheThemeBridges`, `noCornerRadiusOrFontSizeLiterals`, `noSemanticFontsOrDiscardedWeights`; `AppIconTests.theAppUsesNoSystemSymbols`; `NameScanTests.noThirdPartyNamesInWhatTheAppShows` and `noLaneInWhatTheAppShows`. Do not launch the app: the owner checks the view from README 238 and 241–242. Expected: Swift +5.

- [ ] **Step 5: Commit**

```bash
git add app/Sources/VizDAWAppModel/InputFxModel.swift app/Sources/VizDAWAppModel/MixerModel.swift app/Sources/VizDAWAppModel/StripLayout.swift app/Sources/VizDAW/ChannelStrip.swift app/Sources/VizDAW/MixerView.swift app/Sources/VizDAW/StripMidiFx.swift app/Sources/VizDAW/InputFxSettings.swift app/Tests/VizDAWAppModelTests/MidiFxSectionTests.swift
git commit -m "app: the channel strip's MIDI FX section under Inserts, with per-slot settings and bypass (plan 5g-1 Task 9)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD
```

---

### Task 10: The arpeggiator e2e, README 238–244, the spec's rows, the full gates

**Files:**
- Modify: `crates/vizctl/tests/e2e.rs`, `app/README.md`, `docs/superpowers/specs/2026-10-18-recording-audio-design.md` (§13's 5g rows; §6.8's UI line, §6.9's header list and §2's Swift row for Ruling 16)

**Interfaces:**
- Consumes: everything above; the e2e helpers `App::start_with`, `wait_for_record`, `wait_for_undo`, `lane_named`, `WAIT_MS`.

- [ ] **Step 1: Write the e2e.** Append to `crates/vizctl/tests/e2e.rs`:

```rust
/// Spec §12.3's "`midi.inject` held chord into a lane with an arpeggiator
/// while recording: check the recorded notes are the arpeggio" (plan 5g-1;
/// README 239): a C major chord held for a bar into "Keys" with an Up
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
        LaneCmd::SetFlags { seq, lane: keys, mute: None, solo: None, arm: Some(true) },
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
    assert_eq!(lane["arm"], true, "{lane}");
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

   `record.set`'s `replace`, `loop`, `pre_roll` and `count_in` are `RECORD_SET_PARAMS`' names at `a292d2c`; the lane starts empty, so every recorded note is the arpeggio's.

- [ ] **Step 2: Write README 238–244** at the end of `app/README.md`, after "### Tracks (plan tracks)" (whose last step is 237):

```markdown
### MIDI input effects (plan 5g-1)

By ear and by eye; the chain is tested (`core-engine/tests/input_fx*.rs`),
the recording (`core-ffi/tests/passes.rs`, `record.rs`) and the e2e
`arpeggiates_an_injected_chord_while_recording`.

238. Track headers are unchanged: no FX control, and the name field
     keeps its full width. In the Mix view, a plugin instrument's strip
     and the Built-in Synth strip show a "MIDI FX" section under Inserts,
     its heading unlit while the chain is empty. Audio strips, buses and
     master have no such section. Click its empty "+" slot: a menu lists
     Arpeggiator, Transpose, Velocity, Chord and Input Quantize. Add a
     Transpose: the heading lights in the theme's accent colour. Click
     the slot's dot: it is bypassed and the heading is unlit again. Click
     the slot: its settings open in a popover beside it. Check this in
     each theme. Put two tracks on the Built-in Synth and arm one: the
     Mix view's section edits the armed track. Select the other track:
     the left pane's strip edits the selected one.
239. A real keyboard on a MIDI track, armed: add an Arpeggiator (Up, 1/16).
     Stopped, hold a chord: it arpeggiates at once at the project tempo;
     change the tempo and it follows. Play with the click on: the notes
     fall on the sixteenths, and a chord pressed between steps waits for
     the next one. Hold ⏪ or ⏩ and loop a bar while holding: no burst of
     notes and no gap. Try each mode, 2–4 octaves, a triplet and a dotted
     rate.
240. Latch: hold a chord and let go: it keeps playing; press a new chord
     with no key down: the pattern changes to it.
241. Transpose +12 then Chord (+4 +7) near the top of the keyboard, then
     drag the Chord slot above the Transpose slot: the top notes differ (notes past the
     highest key are dropped, never folded).
242. Hold a chord through the arpeggiator and bypass it (its slot's
     dot), change its rate, drag a velocity slider, arm another track,
     disarm this one: no note hangs, and a latched arpeggio survives arming another track. Start a
     MIDI Learn and press a key: the key is learned, and nothing plays or
     arpeggiates.
243. Add Input Quantize (1/16, 100%) and record a loose phrase: thru is
     never delayed; the recorded notes start on the sixteenths and keep
     their lengths. At 50% they move half way.
244. Each change in the MIDI FX section or a settings popover is one
     Undo ("Set Input Effects"); a slider drag is one step. A change made while recording is undone with
     the take. Save, close and reopen: the chain is there. Duplicate Track
     copies it.
```

- [ ] **Step 3: Amend the spec's §13 rows and its UI placement** in `docs/superpowers/specs/2026-10-18-recording-audio-design.md` (the plan's writer commit amended them before; this brings them to the tree):
   - the 5g-1 row: "surface ports skipped by every-source lanes" becomes "surface ports kept out of every lane (delivered by 5f-1 Ruling 17)"; "README 199–205 (189–195 in its plan, renumbered after 5f-4)" becomes "README 238–244"; "Needs 5f-4 (added before it, 2026-11-03)" gains "; runs after sub-project 7 and the Tracks plan";
   - the 5g-2 row: "README 206–211 (196–201 in its plan, renumbered after 5f-4)" becomes "README 245–250";
   - the owner's 2026-11-09 placement (Ruling 16). In §6.8, "**UI.** An "FX" button on the lane header (lit when the chain is non-empty and not all bypassed) opens a popover:" and its two sub-bullets become "**UI.** Instrument strips (a plugin instrument's and the Built-in Synth's) show a MIDI FX section beneath Inserts, lit when the chain is non-empty and not all bypassed: up to 8 slots; click an empty slot to add an effect, click a filled one for its parameters, and each slot has a bypass; drag to reorder. A strip several tracks play edits the shown sequence's armed track first, and the left pane's strip edits the focused track (owner, 2026-11-09)." In §6.9's lane-header list, delete "  - the FX button (§6.8);". In §2's Swift row, "input effects popover" becomes "the strip's MIDI FX section".

- [ ] **Step 4: Run the full gates:**
   - `cargo fmt --all -- --check`, both clippies and `cargo clippy -p core-ffi --lib -- -D warnings`;
   - `cargo test --workspace`;
   - the rt-check gate (Global Constraints);
   - `scripts/build-ffi.sh`, `scripts/swift.sh build`, `scripts/swift.sh test`, the no-brand build and `scripts/make-app.sh`;
   - the e2e: `VIZDAW_E2E_APP=dist/VizDAW.app cargo test -p vizctl --test e2e -- --ignored --nocapture --test-threads 1`.

   Then `pgrep -lf MacOS/VizDAW` prints nothing, and `ls -d /tmp/vze*` prints nothing new.

   Expected, relative to the `a292d2c` baseline (Rust 2077 / 24 ignored, Swift 1286, e2e 20, registry 162):
   - Rust +9 (model), +14 + 14 + 8 (engine), +2 + 8 (FFI), +1 (control) = **+56**, so 2133 passed and 25 ignored (the new e2e is `#[ignore]`);
   - Swift +2 + 6 + 5 = **+13**, so 1299;
   - e2e 21; registry 163; README through 244; file `VERSION` 5.

- [ ] **Step 5: Commit**

```bash
git add crates/vizctl/tests/e2e.rs app/README.md docs/superpowers/specs/2026-10-18-recording-audio-design.md
git commit -m "e2e: an injected chord through the arpeggiator records as the arpeggio; README 238-244; the spec's 5g rows and the MIDI FX section's place (plan 5g-1 Task 10)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD
```

---

## Self-Review

Rerun at the fold (2026-11-09, against `a292d2c`).

1. **Spec coverage.** §4.5 → Task 1. §6.6 → Task 4. §6.8: model and effects → Tasks 1–3; real-time, sample-accurate, synced → Tasks 3–4; capacities counted → Tasks 2–4, 6; note-offs on change or bypass → Tasks 2, 4; recording post-effect with Input Quantize → Tasks 4–5; listening lanes only, never playback or bounce → Ruling 13, Task 4; UI → Tasks 8–9 (the strip's MIDI FX section, Ruling 16). §6.9's FX button → Task 9's section; Task 10 amends §6.8 and §6.9. §12.1 → Task 7. §12.3: golden tests → Tasks 2–3; recording equals thru → Tasks 4–6; `rt-check` → Task 4 and CI; e2e → Task 10; manual arpeggiator check → README 239. The §13 row's surface-port clause is 5f-1's (Ruling 3). The owner's learn ruling → Ruling 2, pinned in Task 6. Format 5 → Ruling 18, Task 1. No gaps.
2. **Step scan.** Bodies are given only for `live_midi`, `emit_live` and the `SetInputs` carry (borrow and ordering rules), plus whole test files. One expectation may need the tree's block arithmetic, and its step says what to do: the latch test's last steps (Task 3).
3. **Type consistency.** `InputFx`/`InputFxKind` (Task 1) feed `InputChain::new` (Task 2), `MidiInputSpec.fx` (Task 4) and `InputFxRow` (Task 6, `rate` flattened into `division` and `feel`). `LiveMsg`, `At`, `ChainClock`, `RecordQuantize` (Task 2) are used by Tasks 3–5; `take_visits` only by Task 2's test. `RecordedMidi { msg: LiveMsg, quantize }` (Task 4) is read by `passes` (Task 5) and `tests/passes.rs` (Task 4's stop-gap). `input_fx_json` (Task 6) is used by `control.rs` (Task 7). `InputFxModel.{showsSection, isLit, open, close, chain, selected, draft calls}` and `DocumentModel.inputFx` (Task 8) are used by Task 9, which adds the section's clicks, `settings`, `chain(of:)`, `drop` and `MixerModel.midiFxLane(for:in:)`. `MidiInputSpec` has no `skip_ports` anywhere.
4. **Review Focus.** Each of the five lines names its tests, and each test is in its owning task.
5. **Proportion.** About 3 000 lines, most of them golden test files; Global Constraints and Rulings were trimmed at the fold, and the house rules point to the contracts.

## Execution handoff

The controller runs this plan subagent-driven under the standing authorization (spec §13), on `sp5g-input-fx`, before 5g-2. Tasks 2–4 build on each other's `LiveMsg` and `InputChain`, and Task 4 changes the renderer's real-time path: a fresh reviewer per task should replay Review Focus 1, 3 and 4 there. The final review runs the rt-check gate itself. Before dispatch, refresh `implementer-contract.md`'s "verified at" line to `a292d2c`.
