# VizDAW 5f-1 — Control Surfaces and MIDI Learn Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Hardware drives the mixer. An MCU-compatible unit (a main unit plus up to three extenders) or a HUI-compatible unit moves faders, pans, sends and plugin parameters through the 5e gesture channel, so riding a surface writes automation. Its buttons mute, solo, arm, select, run the transport (Play only starts; Rewind and Fast Forward skip on a tap and wind while held) and bank. Motor faders, LED rings, scribble strips, the position display and meters follow what the mixer *plays*, automation included. Any automatable control can be MIDI-learned to a controller's CC, 14-bit CC pair, pitch bend or note, with pick-up for absolute controls; any key-bindable registry command can be learned to a button. Surface ports and learned messages stay out of the lanes (no recording, thru, step entry or note input). Surfaces are set up in Settings › Surfaces, and the control channel's `surface.*` commands drive all of it with no CoreMIDI port opened.

**Architecture:**
- **`core-surface`** is a new pure-Rust crate with no I/O and no clock reads (time is passed in as `Instant`). It holds the MCU, MCU XT and HUI state machines (`Surface::input`, `tick`, `feedback` and `release_all`), the 500 ms release-after-idle helper, the Rewind/Fast Forward shuttle, MIDI-learn detection (`Learner`) and learned-map execution with pick-up (`Mapper`).
- **`core-model`** gains `LearnMap` (spec §4.8) in `Settings.learn`, with `SettingsCmd::SetLearn`/`RemoveLearn`. The field is saved only when it is not empty, so `file::VERSION` stays 3.
- **`core-ffi`**:
  - **Carry fix.** A gesture's touch is held by *holders* (mouse, channel, plugin, each surface, learn), so a surface touch and a mouse touch on one target need both releases (carry m8). A touch opens the static undo step at its first value, never at the touch, so a hand resting on a touch-sensing fader holds nothing open.
  - **`surfaces` module.** It owns the process-wide registry: `surfaces.json` in the support folder, a port transport (fake in test builds and with `VIZDAW_AUDIO=offline`, CoreMIDI on macOS, none elsewhere), its own input queue, focus, the releases each session is owed, and the claims that keep surface and learned traffic out of the lanes. Every session's `idle` takes its owed releases; the focused session's `idle` also runs the service: input → actions → gestures, locates and app events; then feedback of the *heard* values at most every 20 ms. Learn starts, maps and decisions also live here, each learn bound to the session that asked.
- **`core-control`, Swift, `vizctl`:**
  - `transport.play`, `surface.add`, `surface.remove`, `surface.inject`, `surface.learn`, `surface.unlearn` and `state.get surfaces`.
  - Settings › Surfaces, the Learn menu items, the MIDI Learn Maps sheet (mode and pick-up), Settings › Keys "Learn MIDI".
  - The surface-event relay (commands, select, arm, jog).
  - The e2e "drive a fader and a button from `surface.inject` (MCU) and check the feedback".
  - README 173–179, including README 165a's surface half.

**Tech Stack:** Rust 2024 (`core-surface` new; `core-model`, `core-ffi` with UniFFI 0.32.2, `core-control`, `vizctl`), Swift 6 / SwiftPM (Swift Testing), macOS 14+.

**Spec:** `docs/superpowers/specs/2026-10-18-recording-audio-design.md`, §13 row "5f-1 — Control surfaces and MIDI learn" (split from the former 5f row by Ruling 1 of this plan; the writer's commit amends §13). This plan delivers:
- §4.8 `LearnMap` in the project;
- §7.1 gestures from MIDI learn and surfaces (the 500 ms release for sources without touch, real touch for touch-sensing faders);
- §8.1 `core-surface`, `SurfaceAction`, the MixerView snapshot, `core-ffi::surfaces`, feedback at most every 20 ms and never to a touched fader;
- §8.2 MCU + XT, HUI and MIDI learn (detection of pairs and encoders, feedback for absolute maps);
- §8.3 Settings › Surfaces, `surfaces.json`, command learn maps in the same file, disconnected/reconnect;
- §12.1 `surface.add`, `surface.remove`, `surface.inject` and `state.get surfaces`; `midi.inject` reaching MIDI learn;
- §12.2 surface errors (a malformed or unknown message is ignored and counted; a surface never stops the app);
- §12.3 the surface golden tests and the surface e2e.

Read the whole spec (its §3 rules bind here), the 5e-1 plan's Rulings 14–20 and 26 (`docs/superpowers/plans/2026-10-28-automation-engine.md`), and the carries named in "Carried items" below.

## Global Constraints

- **Shell setup.**
  - Every shell command runs after `export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools`. Every bash block below starts with that line and `cd /Users/lattawb/workspace/vizdaw`.
  - A `git commit` without `DEVELOPER_DIR` fails silently: check every commit with `git log -1 --oneline` and `git show --stat HEAD`.
- **Swift only through the scripts, from the repo root.**
  - The scripts: `scripts/build-ffi.sh` (after any Rust change that touches the FFI, before Swift), `scripts/swift.sh build`, `scripts/swift.sh test` and `scripts/make-app.sh`.
  - Never call `swift`, `xcodebuild` or `xcrun` directly.
- **Branch and commits.**
  - Work on branch `sp5-audio`. The code base is the tree at `276c855`, the last code commit (`e633e1c`, `c13432c` and `276c855` landed after the 5f plans' commit `f3291d0`); the plan revisions on top (`4d29b4c` and its successor) change no code. Never switch branches, never push.
  - Commit at the end of every task, adding only the files the task lists, by path (never `git add -A`).
  - `Cargo.lock` changes once, in Task 1: it gains the `core-surface` workspace package and `core-ffi`'s dependency on it. No registry crate is added.
  - Never commit `.superpowers/`, `.DS_Store` or generated files.
  - End every commit message with these two attribution lines (use the `Co-Authored-By` line naming the model you actually are):
    ```
    Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
    Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P
    ```
- **Clean room.**
  - Never read or copy anything from `~/workspace/SVP/research`, the Studio Vision binary or any Studio Vision material.
  - No Opcode, Midas, Mackie, Avid, Steinberg, Apple Logic or Soundcraft assets, names, artwork or branding anywhere. UI text says "MCU-compatible", "MCU XT-compatible" and "HUI-compatible".
  - Protocol behaviour comes from public protocol descriptions and is written here as byte tables. The code names the protocols `Mcu`/`Hui` only.
- **No GUI tools, no real devices.**
  - Never use GUI, computer-use or accessibility-automation tools. Views are checked by the owner by eye from README steps 173–179.
  - Launch VizDAW only through the e2e (Task 11) with `VIZDAW_AUDIO=offline`. At the end of a task that launched it, `pgrep -lf MacOS/VizDAW` prints nothing and `ls -d /tmp/vze*` prints nothing new.
  - **Tests never open real MIDI ports or devices, never open real audio input and never trigger the microphone prompt.**
    - Surfaces are tested through the fake port transport (`surfaces::ports::FakePorts`, Task 7). Test builds (`test-hooks`) and `VIZDAW_AUDIO=offline` always use it; no CoreMIDI client is created.
    - `core-surface` has no I/O at all.
    - Every `core-ffi` test that plays calls `vizdaw_ffi::host::force_offline()` first (through its file's `serial` helper).
    - **Swift tests link the FFI built without `test-hooks` and `IntegrationTests` makes real `Session`s**, so the Swift side never reaches the live surface free functions from `VizDAWAppModel`: everything process-wide (surfaces, ports, command maps, learn state) goes through the `SurfaceCore` protocol (Task 9). The app passes `LiveSurfaceCore` (the free functions, in the `VizDAW` target); `ControlHost`, `SurfaceSettings`, `LearnModel` and `LaneInputs.claimedPorts` default to nothing live, and the tests pass `FakeSurfaceCore`. No Swift test creates a CoreMIDI client or writes `surfaces.json` (pre-flight M25).
  - Never touch the real `~/Library/Application Support/VizDAW`, the real preferences, the real Trash or any device. Tests use temporary folders: core-ffi's `support_dir()` defaults to a per-process temp folder under `test-hooks`, and Swift tests pass `MemoryDefaults()`. The e2e runs in its throwaway home.
- **Test waits.** Every `core-ffi` integration test that waits for the transport polls with a deadline from `common::wait_limit()` (`crates/core-ffi/tests/common/mod.rs`, which reads `VIZDAW_TEST_WAIT_SECS`), never with a fixed sleep for the result. Surface timing (20 ms throttle, 500 ms idle release, 1 s HUI ping, 300 ms learn window, the shuttle's 400 ms hold and 100 ms wind steps, the 2 s port check, the 100 ms echo window) is driven by passed `Instant`s in `core-surface`, by the `surfaces::debug_advance(ms)` clock hook in `core-ffi`, and by an injected clock in Swift (`SurfaceRelay.now`), never by real sleeps.
- **Crates and dependencies.**
  - Rust stable, edition 2024, license GPL-3.0-only.
  - New crate `core-surface`, named by the spec. Its dependencies are `core-model` and `serde` (already in the lock). `core-ffi` gains `core-surface`. No external crate.
- **Lints.** Every crate keeps `#![deny(missing_docs)]` and `#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic))]`; `core-surface` starts with both. New non-test code uses `.get()`, iterators and `checked_*`/`saturating_*`, never indexing or unwraps. Tests use `assert!(x)`, never `assert_eq!(x, true)` (`clippy::bool_assert_comparison` under `-D warnings`).
- **Platform-neutral cores.** `core-model`, `core-surface`, `core-engine`, `core-media` and `core-control` stay free of OS APIs.
- **Linux CI.**
  - CI runs `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` on `ubuntu-latest` and `macos-14`.
  - The port transport has three implementations, chosen once by cfg and mode (Ruling 10):
    - `FakePorts` under `test-hooks`, and whenever `host::offline_mode()` holds;
    - `CoreMidiPorts` behind `#[cfg(all(target_os = "macos", not(feature = "test-hooks")))]`;
    - `NoPorts` (no sources, no destinations: every surface reads Disconnected) on other live builds, `#[cfg(all(not(target_os = "macos"), not(feature = "test-hooks")))]`.
  - A gate that unifies `test-hooks` (the workspace tests) never compiles `CoreMidiPorts` or `NoPorts`; `cargo clippy -p core-ffi --lib` (no features) compiles the live one of the host. A binding mutated only on macOS uses `#[cfg_attr(not(target_os = "macos"), allow(unused_mut))]`.
  - Every task that changes Rust checks `cargo clippy --workspace --all-targets --target x86_64-unknown-linux-gnu --target-dir target/linux-check -- -D warnings`.
  - No test in this plan needs CoreMIDI, so none is macOS-only.
- **TDD.** Every new test is run and seen to fail (RED) before the code that makes it pass.
- **Real-time rules (spec §3).**
  - Nothing in this plan runs on the real-time thread. Surface gestures enter the existing 5e-1 gesture queue through `Session` internals, exactly as the mixer's do.
  - CoreMIDI's input threads only push or look up, never wait:
    - the surfaces' sink pushes into the registry's own bounded input queue (a `Mutex<VecDeque>` separate from the registry, `try_lock`, drop and count, never wait), the pattern of `host::midi_from_source`;
    - the lane and note-input listeners ask `surfaces::takes` through a `RwLock::try_read` (a message is passed on as before when the claims are being rewritten, which happens only when a surface or map changes);
    - every sink closure wraps its body in `std::panic::catch_unwind` (a panic must not unwind into CoreMIDI).
  - **Lock order:** `Session.inner` → the surfaces registry → the input queue. No code takes a session's `inner` while holding the registry or the queue: the service copies what it needs from the registry, drops it, then applies under `inner`.
  - **rt-check gates** run once, in Task 12, to prove nothing moved: `cargo clippy -p vizdaw-play --all-targets --features rt-check -- -D warnings` and `cargo test -p core-engine --features rt-check --test rt_alloc --test rt_alloc_graph --test rt_alloc_record --test rt_alloc_gestures`.
- **Media is write-once.** Nothing here writes media.
- **File compatibility.**
  - `core_model::file::VERSION` stays 3. `Settings.learn` is `#[serde(default, skip_serializing_if = "Vec::is_empty")]`: a project with no learn maps saves byte for byte as before, and older projects open with none (Ruling 6).
  - `surfaces.json` carries its own `"version": 1`. An unreadable, newer or invalid file is kept aside as `surfaces.json.bad` and VizDAW starts with no surfaces (Ruling 9).
- **FFI safety.**
  - No panic crosses the FFI. Every new exported function or method returns `Result<_, VizError>` and runs inside `ffi_guard`.
  - The exceptions are the infallible exports `surface_list`, `surface_ports`, `surfaces_state`, `command_maps`, `learn_state` and `cancel_learn`. Each body runs inside `std::panic::catch_unwind` and answers its neutral value (empty list, empty ports, `SurfaceStateRow::default()`, `LearnStateRow::default()`, nothing) on a panic.
- **UniFFI changes** (Tasks 6–9):
  - New enums: `GestureHolderRow`, `SurfaceKindRow`, `SurfaceEventRow`, `LearnKindRow`, `LearnModeRow`, `LearnForRow`.
  - New records: `SurfaceRow`, `SurfaceFeedbackRow`, `SurfaceStateRow`, `SurfacePortsRow`, `LearnSourceRow`, `LearnMapRow`, `CommandMapRow`, `LearnStateRow`.
  - New free functions: `surfaces_load`, `surface_list`, `surface_add`, `surface_remove`, `surface_ports`, `surfaces_state`, `command_maps`, `remove_command_map`, `learn_command`, `cancel_learn`, `learn_state`.
  - New `Session` methods: `gesture_from` (Task 6), `focus_surfaces` (Task 7), and `set_surface_selection`, `inject_surface`, `surface_state`, `take_surface_events`, `learn_target`, `learn_maps`, `set_learn_mode`, `set_learn_pickup`, `remove_learn`, `unlearn_target` (Task 8: ten).
  - New `StateTargetRow::Surfaces` and `ControlResult::Surfaces`.
  - **Every task that adds an exported `Session` method adds its `FakeSession` stub (`app/Tests/VizDAWAppModelTests/FakeSession.swift`) in the same commit**, and lists the file, because the generated `SessionProtocol` breaks `scripts/swift.sh build` otherwise.
- **Swift.**
  - Generated bindings compile in Swift 5 mode; `VizDAWAppModel`, `VizDAWEditors`, `VizDAW` and the tests compile in Swift 6 mode with no new warnings.
  - No SwiftUI `@State`/`@FocusState` for model data (view-local presentation flags only).
  - New model tests go to `VizDAWAppModelTests`.
- **Control channel** (control spec §6, §7).
  - Programmatic calls never open dialogs. Commands that surface bytes or learned maps run because of `surface.inject` or `midi.inject` are programmatic too (Ruling 12).
  - Every command is in `core_control::registry::COMMANDS` and handled by `ControlCommands.run`; the Swift test `everyRegisteredMenuCommandHasAHandler` compares them.
  - The registry count test moves from 138 to 144 in Task 9 (`transport.play` and the five `surface.*` commands).
  - Every new command gets hostile-input tests: by type and range in `core-control` (`surface_commands_check_their_params`), in the app (`surfaceCommandsCheckTheirArguments`) and in the cores (`hostile_surface_calls_are_refused`, `hostile_learn_calls_are_refused`, `malformed_bytes_are_ignored_and_counted`, `a_learn_map_is_checked_by_the_model`, `a_damaged_surfaces_file_is_kept_aside`).
- **Generated files** are never committed: `app/Sources/VizDAWCore/vizdaw_ffi.swift`, `app/Sources/vizdaw_ffiFFI/include/`, `target/ffi/`, `dist/`.
- **Each task ends green.**
  - Always: `cargo fmt --all` then `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, the Linux clippy, and `cargo test` for the crates the task touches (the whole workspace in Tasks 6, 9 and 12).
  - Tasks 6–12 also run `cargo clippy -p core-ffi --lib -- -D warnings`, `scripts/build-ffi.sh`, `scripts/swift.sh build` and `scripts/swift.sh test`. Tasks 7–12 also run `cargo clippy -p core-ffi --lib --target x86_64-unknown-linux-gnu --target-dir target/linux-check -- -D warnings`, the one gate that compiles `NoPorts` (the workspace clippies unify `test-hooks`).
  - Tasks 11 and 12 also run `scripts/make-app.sh` and the e2e (`VIZDAW_E2E_APP=dist/VizDAW.app cargo test -p vizctl --test e2e -- --ignored --nocapture --test-threads 1`).
  - Known flaky tests: the `damaged_files` proptest, the MIDI listener tests (`platform-macos/tests/midi.rs`, the reconnect test), `core-ffi` catalog tests under parallel load, and the latency e2e timing. Re-run once before investigating, and say so in the report.
  - Baseline at `276c855`: Rust workspace 1634 passed, 0 failed, 14 ignored (1632 measured at `8d38c51`, plus the two core-engine gesture tests `e633e1c` added; the other commits since only gate tests to macOS or fix them); Swift 899 tests; e2e 10 tests; registry 138. Each task's last step gives the counts it adds. Report the measured counts; a slip of the writer's arithmetic is not a failure.
- **How code is given.**
  - New files are written whole (`cat > … <<'VIZDAW_EOF'`) where their exact text matters (tests, byte tables). For new implementation files, the step gives the module's public items with their doc lines, and the algorithm where the signature and the tests leave a choice.
  - Changes to existing files are **Find / Replace** excerpts, applied in order with the Edit tool. Each Find occurs exactly once in its file at `276c855` (or in the file an earlier task of this plan left, which the step says).
  - If an excerpt does not match, make the same change to the current text. If a name differs from the tree, use the tree's name and say so in the report.
  - Tests added to an existing test file are given whole, as code, and appended or inserted where the step says.
- **Spec values used verbatim:**
  - "`fn input(&mut self, bytes: &[u8], now: Instant) -> Vec<SurfaceAction>`"; "`fn feedback(&mut self, state: &MixerView) -> Vec<Vec<u8>>`" (Ruling 3 adds the bank offset);
  - "`Gesture(target, phase)` … `Command(registry ID)` … `Bank(±n)`, `Channel(±1)` … `Select(strip)` … `Jog(delta)`";
  - "strip names, fader, pan, mute, solo, arm, select, automation mode, meters, and the position as bars·beats or timecode";
  - "`feedback` is sent at most every 20 ms, or at once for touched faders' partners (motor faders must not fight a hand: no fader feedback while touched)";
  - MCU: "The main unit plus up to 3 extenders (XT), giving banks of 8 × units"; "Faders are 14-bit pitch bend per channel with touch notes (0x68–0x70)"; "V-pots are relative CC 0x10–0x17 in pan, send or plugin assignment mode, with LED-ring feedback"; "Scribble-strip LCD via SysEx (names and values, 2 × 56 characters per unit); 2-digit assignment and timecode/BBT displays"; "Meters via channel pressure"; "The device-query handshake is answered";
  - HUI: "Zones and ports via CC 0x0F/0x2F select-plus-port messages"; "Faders as hi/lo CC pairs with touch via zone 0x0..0x7 port 0"; "4-character scribble names and the 40-character display via SysEx"; "Meters via polyphonic pressure (0xA0)"; "The 1 s ping is answered";
  - MIDI learn: "waits for the next incoming CC, pitch bend or note"; "14-bit pairs are detected when CC n and n+32 arrive together. Relative encoders are detected when values cluster around 1/127 or 63/65, and can be overridden in the mapping list"; "Absolute mappings send the value back so that motorised and LED-ring controllers follow";
  - "A plugin that sends values without gesture events is treated as touched by its first value and released **500 ms** after its last" (and the same rule for "MIDI learn and HUI/MCU faders without touch sensing");
  - "`LearnMode` is `Absolute`, `Relative2sComplement` or `RelativeBinOffset`";
  - "Settings › Surfaces lists surfaces: type (MCU, MCU XT, HUI), input port, output port, and the unit's position in the bank"; "`~/Library/Application Support/VizDAW/surfaces.json`"; "A missing port leaves the surface listed as "disconnected", and it reconnects when the port appears";
  - "a malformed or unknown message is ignored and counted. A surface never stops the app".

## Review Focus

These are the five failure modes a user is most likely to hit that the spec implies but does not spell out, most likely first. Each is pinned by named tests in its task. Banks and garbage from a controller are checked in the task reviews of Tasks 2, 3, 5, 7 and 8, where their tests are (`banks_of_one_main_and_three_extenders`, `bank_and_channel_clamp_to_the_strips`, `banks_clamp_after_strips_are_removed`, `positions_out_of_order_show_consecutive_strips`, `malformed_bytes_are_ignored_and_counted`, `an_msb_and_its_lsb_in_separate_calls_make_one_value`, `hostile_surface_calls_are_refused`).

1. **Two hands on one control.** A touch-sensing fader is held while the same fader is grabbed with the mouse. A learned knob turns a pan while a `vizctl` script holds it. The surface lets go first, or the mouse does, while stopped or while a pass writes.
   - Expected: the target stays touched until the last holder releases. Releasing one holder does not commit, glide back or hold the other's move: while writing, the write follows the remaining hand. A static edit is one undo step. A second Touch from the same holder is a move, never a second hold.
   - Pinned by `a_surface_and_the_mouse_hold_one_target_until_both_let_go`, `each_holder_releases_only_its_own_touch`, `a_holder_touching_twice_holds_once` and `two_holders_on_a_writing_strip_end_only_at_the_last_release` (Task 6), and by `a_surface_touch_and_a_mouse_touch_share_the_target` and `a_learned_cc_writes_its_targets_automation_in_touch` (Task 8).
2. **Motor faders that do not follow what plays.** Automation plays a fader ramp in Read; another unit or the mouse rides a fader in Touch; a muted-by-automation strip's LED.
   - Expected:
     - every fader, ring and LED shows the *heard* value (the engine's live value, else the automation at the transport, else the static value), so a motor fader moves with its automation;
     - no pitch-bend or fader CC goes to a fader while it is touched; on release the unit gets the value the mixer now plays (in Touch, the glide back);
     - a fader moved from another surface follows at once; everything else within 20 ms, and only what changed is sent.
   - Pinned by `motor_faders_follow_automation_playback`, `a_touch_ride_moves_the_partner_and_glides_back_on_release` and `feedback_waits_20_ms_but_partners_go_at_once` (Task 8), and `no_fader_feedback_while_touched_then_one_on_release` (Task 2) and `hui_fader_touch_blocks_feedback` (Task 3).
3. **A resting hand.** A hand rests on a touch-sensing MCU or HUI fader for minutes while the user edits notes, adds a bus, learns a knob or changes an automation mode.
   - Expected: a touch with no value opens no undo step and blocks nothing: drags begin, one-shot edits are their own undo steps, Write Automation landings and mode changes are not held up. The first moved value opens the touch's own step ("Set Fader"), and Undo of it restores only the fader.
   - Expected too: a hand resting on a Read strip's fader when Play starts keeps the static path, and its move is a static edit; on a writing strip it writes, and a Trim strip grabs at an offset of 0.
   - Pinned by `a_resting_touch_does_not_open_an_edit` and `a_resting_touch_held_over_play_keeps_its_path` (Task 6) and `a_resting_surface_touch_does_not_open_an_edit` (Task 8).
4. **Surface and learned traffic leaking into the lanes.** A MIDI lane armed on All MIDI Inputs while an MCU fader is touched (note 0x68, pitch bend on channel 1); a keyboard knob learned to a fader; step entry while pressing Play on the unit.
   - Expected: nothing a surface port sends, and no message a learn map (or a waiting learn) takes, is recorded, played thru, step-entered or heard as note input; unlearned messages from the same keyboard pass as before; the lane input picker marks surface ports "(used by a surface)".
   - Expected too: after Undo of "MIDI Learn" (or Redo, Revert, Open), the knob no longer drives the target, and its messages reach the lanes again.
   - Pinned by `surface_ports_and_learned_controls_stay_out_of_the_lanes`, `a_learned_cc_is_neither_recorded_nor_played_thru` and `an_undone_learn_stops_driving_and_taking` (Task 8), `the_maps_say_which_messages_they_take` (Task 5) and `aSurfacesPortIsMarkedInThePicker` (Task 10).
5. **A surface or learned control that goes away and comes back.** A USB unit unplugged mid-ride (while it counts as connected), a unit removed in Settings while held, a port renamed, `surfaces.json` damaged or written by a newer VizDAW, focus moving to another window, or the focused window closing while a surface or a learned knob holds a fader.
   - Expected:
     - an unplugged unit is seen within 2 s, shows "Disconnected", and its held touches (touch-sensing faders and idle holds) are released at once (the ride lands as written so far);
     - it reconnects when the port returns, gets a full refresh, and its formerly held fader gets motor feedback again;
     - removing a surface releases what it held;
     - a damaged or invalid `surfaces.json` is kept aside and VizDAW starts with no surfaces;
     - a focus move releases the old document's surface and learn touches; closing the focused window releases them and blanks the units.
   - Pinned by `a_vanished_port_is_seen_while_connected` and `a_damaged_surfaces_file_is_kept_aside` (Task 7), and `a_surface_touch_is_released_when_its_port_goes`, `a_returning_port_reconnects_with_a_full_refresh`, `a_unit_unplugged_while_touched_gets_fader_feedback_after_reconnect`, `removing_a_surface_releases_its_touches`, `focus_moving_releases_the_old_sessions_surface_and_learn_touches` and `closing_the_focused_window_releases_and_blanks` (Task 8).

## Plan Rulings

Each ruling decides something the spec leaves open, in the format "what — why — cost if wrong". The plan's pre-flight (2 Critical, 12 Important, 31 Minor) and the owner's answers of 2026-11-02 are folded into them; where the owner answered a question the pre-flight left open, the owner's answer is the ruling, and the ruling says so.

1. **Ruling (the split): the former §13 row "5f — Surfaces, bounce, external editor" becomes three plans.**
   - **5f-1** is this plan: surfaces and MIDI learn, engine and UI.
   - **5f-2** is bounce and export (`docs/superpowers/plans/2026-11-02-bounce-export.md`).
   - **5f-3** is the external sample editor (`docs/superpowers/plans/2026-11-02-external-editor.md`).
   - They run in that order. 5f-3 reuses 5f-2's WAV writer; 5f-1 and 5f-2 are independent.
   - The writer's commit amends spec §13 with the three rows.
   - Why:
     - as one plan, 5f would be about 26 tasks across three unrelated subsystems: protocol state machines, an offline renderer with plugin copies, and a file watcher;
     - each of the three is testable alone and has its own e2e or README section;
     - surfaces and bounce share no code.
   - Cost if wrong: two more plan headers and final reviews.
2. **Ruling (holders, carry m8): a touched target keeps a set of holders and is released when the last holder lets go.**
   - The holders are `Mouse`, `Channel`, `Plugin`, `Learn` and `Surface(SurfaceId)`, where `SurfaceId(u8)` is below 16 by construction (Ruling 4), so every holder has a bit: bits 0–3 for Mouse, Channel, Plugin and Learn, bits 4–19 for surfaces 0–15.
   - A Touch from a new holder joins; a Touch from a holder that already holds the target is a move.
   - A value from any holder moves the target (the last value wins).
   - A Release from a holder that does not hold the target does nothing. A Release that is not the last flushes the sampler and sends the flushed value (`send_value`), as `release` does, so the joining holder's last pending move is not lost; it sends no Glide or Hold.
   - The touch's path (static, live or write) is that of the first holder, as today. Its `Source` is `Source::Plugin` for the plugin holder only, and `Source::Ui` for Mouse, Channel, Surface and Learn, so a V-pot in plug-in mode reaches the plugin (`set_plugin`).
   - **The static edit opens at the first value, never at the touch (pre-flight C2).** On the static path (stopped, or Off/Read while playing) a touch opens no history transaction. The touch's first static value opens the shared static edit (or joins it), counted then, and marks the touch `shared`; a touch that ends with no value opens and commits nothing; while a touch rests with no value, other edits, Write Automation landings and mode changes run normally. A first value while another (non-static) edit is open is refused with Busy as the touch is today: the mouse and the channel get the Busy; a surface or learn holder counts it in its unit's `ignored`. **At a pass's start** (Play) a static touch converts by its strip, whether or not it shares the edit: on a strip that does not write it stays static (its first value opens the static edit while playing, as stopped); on a writing strip it writes (a Trim strip grabs at an offset of 0), and only a touch that is `shared` ends a share of the static edit then. Keying the conversion on "some static edit is open", as 5e-1 could while every static touch held a share, would turn a resting touch on a Read strip into a heard-only touch whose ride is lost, and write absolutes on a Trim strip (fold check C1).
   - `Session.gesture` keeps its signature and means `Mouse`. The channel's `automation.gesture` calls the new `Session.gesture_from(…, .channel)`, so a mouse release no longer ends a touch only the channel holds. Surfaces and learn call the internals (`gesture_held`) with their holder, `Source::Ui` and the message's host time.
   - Releases after idle (V-pots, untouched faders, learned maps) are stamped at their last value's time, as `release_plugin_touches` back-dates, so Touch does not write a 500 ms flat before the glide.
   - Closing a window, a vanished target, a vanished port and a removed surface release every holder concerned (the existing `release_touches`/`release_vanished`, plus the releases a session is owed, Ruling 11).
   - Why: the carry asks for "their own touches or a reference count". A holder set is a reference count that cannot go negative and cannot be released twice by one source. Hands rest on capacitive faders for minutes, where a mouse drag lasts seconds: a touch that held the undo transaction open would freeze editing.
   - Cost if wrong: a surface whose touch-release message is lost holds the target until its port vanishes, it is removed, or the window closes. `state.get automation` lists it, as for a script today; with the edit opening at the first value, nothing else is blocked meanwhile.
3. **Ruling (the surface API): `Surface::feedback(&mut self, view: &MixerView, first: usize, now: Instant) -> Vec<Vec<u8>>`, where `first` is the index of the first strip this unit shows, and `now` times the meters and the HUI ping.**
   - The spec's `feedback(state)` has no bank offset; the group manager (`core-ffi::surfaces`) owns banking.
   - `Surface::tick(&mut self, now) -> Vec<SurfaceAction>` is added for the 500 ms releases of V-pots and untouched faders, the shuttle's wind, plus the HUI ping and connection timeouts.
   - `Surface::release_all(&mut self) -> Vec<AutomationTarget>` is added for a disconnect or a removal: it returns every target the unit holds (touch-sensing faders and `IdleTouches::held()`), then forgets them, its touch flags, the HUI pending hi bytes and the shuttle, so a reconnected unit's formerly held fader gets feedback again.
   - Input addresses strips through the IDs the last feedback showed. Before any feedback, strip controls are ignored and counted.
   - Why: banking spans several units. One unit cannot know its neighbours, and input must address what the unit displays.
   - Cost if wrong: an API rename.
4. **Ruling (banks and how many units): all MCU and MCU XT units form one bank group, ordered by `position` (0–3). Each HUI unit is its own group of 8.**
   - A group shows `width = 8 × units` strips, starting at `offset`, from the mixer's display order (master excluded). Units show consecutive strips in position order, whatever order they were added in.
   - `Bank(±1)` moves `offset` by `width`; `Channel(±1)` moves it by 1.
   - `offset` is clamped to `0..=max(0, strips − width)` after every move and after every mixer change.
   - Only the main MCU has the master fader (pitch bend channel 9, touch note 0x70) and the transport and assignment buttons.
   - At most 1 MCU and 3 MCU XT units ("VizDAW drives one MCU-compatible main unit and up to three extenders"), an XT position another XT has is refused ("position 2 is taken"), and at most **16 surfaces** in all ("VizDAW drives at most 16 surfaces": one MCU group plus up to 12 HUIs).
   - A surface's ID is the lowest free one in 0..16 (`surface_add`), so IDs never pass 15 however often surfaces come and go.
   - No two surfaces share an input or an output port ("“X” is used by surface 2").
   - Why: this is spec §8.2's "banks of 8 × units"; HUI has no extender protocol. A bounded ID keeps every surface a holder bit (Ruling 2).
   - Cost if wrong: Bank moves by 8 per press, a one-line change; a 17th unit needs the bound raised.
5. **Ruling (what the controls drive):**
   - Faders drive `StripFader`.
   - MCU V-pots work in three assignments, chosen with the MCU's Pan, Send and Plug-in buttons:
     - Pan drives `StripPan`;
     - Send drives `SendLevel(strip, 0)`, the strip's first send;
     - Plug-in drives the first 8 parameters (by the plugin's list order) of the selected strip's first insert, one per V-pot, with the strip names row showing parameter names.
   - HUI V-pots drive pan only in 5f-1; HUI's V-Sel and assignment switches are ignored, not counted (pre-flight M19; backlog row "HUI V-pot assignment").
   - Each V-pot detent moves the target by 0.01 of its normalised range, times the detent count (1–15).
   - Mute, solo and select are clicks (touch, value and release at once) through `Gesture`. Select and rec-arm are relayed to the app (`SurfaceEventRow::Select/Arm`, Ruling 12).
   - **Transport (owner answer 2026-11-02, replacing the plan's first `transport.play_stop` reading and the pre-flight's Q1 options):**
     - Play sends `transport.play`, a new registry ID (Task 9; with the five `surface.*` IDs the registry count goes 138 → 144, one more than the five alone): it only starts. While playing it does nothing; a stopped transport plays from where it stands.
     - Stop sends `transport.stop`, Record `transport.record`, Cycle `transport.loop`.
     - Rewind and Fast Forward run the shuttle (`core_surface::shuttle`), on the main MCU and on a HUI:
       - a **tap** (released within `SHUTTLE_HOLD` = 400 ms of the press) is `Skip(−1)`/`Skip(+1)`: the core locates to the previous/next marker of the transport's sequence, or, with no marker that way, to the previous/next bar line (back: the last bar line before the position; forward: the first after it), never below tick 0;
       - a **hold** winds while held, faster the longer it is held. The rate after holding for `h` (from the press) is `wind_rate(h)` quarter notes per second: 0 below 400 ms, then **4 from 400 ms, 8 from 1.4 s, 16 from 2.4 s and 32 from 3.4 s on** (doubling every second of winding, capped at 32). The distance is the integral of that rate. `tick` emits `Wind(quarters)` (negative for Rewind) at most every `WIND_EVERY` = 100 ms, and the release emits the rest since the last one; a release after a hold never skips;
       - the core applies `Skip` and `Wind` itself, as Locates on the session (Ruling 12), so a wind step uses the position the last step went to, not a stale transport reading. While recording, or with no sequence loaded in the engine, they are ignored and counted.
     - Undo `edit.undo`, Save `file.save`. The MCU automation buttons send Read/Off, Write, Trim, Touch and Latch as `automation.mode.read|write|trim|touch|latch` for the selected strips.
   - The jog wheel sends `Jog(detents)`. The app moves the position by one sixteenth note per detent, coalescing detents (Ruling 12).
   - Why: these are the spec's targets (fader, pan, send, plugin) with the registry's commands. The owner wants a surface's Play to behave as on hardware DAW controllers (it never stops), and Rewind/Fast Forward to be useful in a project without markers. A doubling curve reaches a song's end in seconds while the first second stays fine-grained; 100 ms steps keep the locates (each re-seeks the streamer) at ten a second.
   - Cost if wrong:
     - the curve's four constants (one function, `wind_rate`, and its test);
     - winding while a pass writes lands one Write Automation step per wind step (each Locate lands the pass, 5e-1 Ruling 23), at most ten a second; nobody rides and winds at once.
6. **Ruling (where learn maps live): `Settings.learn: Vec<LearnMap>` in `core_model::document::Settings`, not a new `Document` field (accepted by the owner, 2026-11-02).**
   - Two commands: `SettingsCmd::SetLearn { map }` (adds, or replaces the map with the same `source`; undo label "MIDI Learn") and `SettingsCmd::RemoveLearn { source, target }` ("Remove MIDI Learn"). `RemoveLearn` takes exactly one of `source: Option<LearnSource>` (that map) and `target: Option<AutomationTarget>` (every map to that target, as one step); it refuses when both or neither is given, or when nothing matches.
   - `LearnMap` carries `mode` and `pickup` (Ruling 18).
   - At most 512 maps.
   - Checks: the port name must be non-empty, without control characters, and at most 255 bytes; the channel must be 0–15; a CC must be 0–127, and 0–31 for `Cc14`; a note must be 0–127. The target must exist when the map is set, and cannot be a `VelocityScale`.
   - A map whose target is later deleted stays and does nothing; the maps sheet shows it as "(missing)".
   - The field is skipped when empty, so the format stays at version 3.
   - Why:
     - `DocumentRepr` is built literally in 16 places, while `Settings` is built only in its `Default`;
     - learn maps are project settings ("target learn maps are in the project");
     - the 5e-1 precedent (Ruling 2) keeps the version.
   - Cost if wrong:
     - a pre-5f build opening a project with learn maps says Corrupt rather than NewerFormat (backlog row added in Task 12; the owner accepted it);
     - dangling maps need a manual Remove.
7. **Ruling (learn detection):** after the first CC, pitch bend or note-on on any port, `Learner` collects that port's messages for **300 ms** or until 8 messages, then decides:
   - a pitch bend → `PitchBend`;
   - a note → `Note`;
   - CC `n < 32` and `n + 32` on the same channel → `Cc14`;
   - otherwise `Cc7` of the first CC seen;
   - the mode, for a CC with at least 2 values:
     - every value in `{1, 2, 3, 125, 126, 127}` → `Relative2sComplement`;
     - every value in `61..=67` except 64 → `RelativeBinOffset`;
     - anything else, or a single message → `Absolute`.
   - Ports that a surface uses are not learned.
   - A learn belongs to the session that started it (Ruling 11): only that session's service decides it and executes it; if that session closes, the learn is cancelled.
   - A learn waits until it completes, is cancelled, or another learn starts. There is no timeout. While it waits, every channel message it would take (CC, pitch bend, note on/off from a port no surface uses) stays out of the lanes (Ruling 17).
   - Why: the spec's cluster rule, with an explicit window so one turn of an encoder decides.
   - Cost if wrong: an encoder learnt from one detent is Absolute, and an absolute knob turned at the very top or bottom of its travel (values clustering at 1–3 or 125–127) can be learnt as Relative; the maps sheet's mode picker fixes both.
8. **Ruling (learned maps in use):**
   - **Absolute values:**
     - `Cc7` → `v / 127`;
     - `Cc14` → `(msb << 7 | lsb) / 16383`. An MSB is held until its LSB arrives (in any later `input` call), until the next MSB of the same pair, or until `Mapper::tick` (the end of the service pass) with no LSB since. Only then is it applied, as `(msb << 7 | last_lsb) / 16383`, where `last_lsb` is the last LSB this map saw (0 before any). A lone MSB is never `msb / 127` (pre-flight I6: messages arrive one per call, so that rule wrote a coarse value before every fine one, a zigzag);
     - `PitchBend` → `(lsb | msb << 7) / 16383`.
   - **Pick-up** applies to absolute CC, 14-bit and pitch-bend maps (Ruling 18).
   - **Notes:** a note-on toggles a stepped target (mute, solo). On a continuous target, note-on sets `velocity / 127` and note-off does nothing.
   - **Relative modes:** each detent adds ±1/127 of the normalised range to the target's current value (the map's own last value while its idle touch is open, else `current`). A value of 64 in binary offset (or 0 in 2's complement) is no move and sends nothing.
   - **`current`** is the heard value of the target (Ruling 13), the same value the surfaces show.
   - **Release:** every learned gesture uses holder `Learn` and releases 500 ms after its last value, stamped at that value's time; it writes by the strip's mode like any other hand (`a_learned_cc_writes_its_targets_automation_in_touch`).
   - **Feedback:** absolute maps send the current value back to the output port with the same name as the source port, if one exists, whenever it differs from what was last sent. A map whose target is touched by the learn holder sends nothing.
   - **Command maps:** a note-on, or a CC rising through 64, runs the command once. Only IDs that Settings › Keys can bind can be learnt (no required parameter, no `always` dialog), so a learned pad never runs `automation.gesture` or `surface.add` without arguments.
   - **What maps take stays out of the lanes (owner answer 2026-11-02, Ruling 17).**
   - Why: §8.2 "Feedback"; one rule for every control without touch sensing (§7.1).
   - Cost if wrong: a controller that echoes its own values could fight. It would need a per-map "no feedback" switch, added later.
9. **Ruling (`surfaces.json`): the file lives at `support_dir()/surfaces.json`.**
   - Format: `{ "version": 1, "surfaces": [{ "id", "kind", "input", "output", "position" }], "commands": [{ "source", "mode", "command" }] }`.
   - It is written atomically (temp file, then rename) after every change, by the core.
   - It is read once by `surfaces_load()`, which the app calls at launch after `setSupportDir` (Task 10).
   - Every loaded entry is checked by `surface_add`'s rules (kinds, positions, ports, the counts of Ruling 4), plus unique IDs below 16 and unique ports; every command map by `learn_command`'s and the source's checks.
   - A file that does not parse, has a newer version, or fails a check, is renamed `surfaces.json.bad` (replacing an older `.bad`) and VizDAW starts empty, with one problem line in `state.get surfaces` (`problems`).
   - Test builds default `support_dir()` to their per-process temp folder; the e2e uses its throwaway home.
   - Why: "beside the 4f key bindings" means beside the app's support files. The 4f bindings are in preferences, but the spec names this file and folder. The core owns it so the channel and the app see one state. A hand-edited file must not smuggle in IDs or counts the registry cannot hold.
   - Cost if wrong: the owner keeps a copy of the `.bad` file by hand.
10. **Ruling (ports and the transport): the port transport is a trait, `surfaces::ports::Ports`.**
    - `send(port, bytes) -> bool`, `sources() -> Vec<String>`, `destinations() -> Vec<String>`, `refresh()`.
    - `FakePorts` is used in `test-hooks` builds and whenever `host::offline_mode()` is on (`VIZDAW_AUDIO=offline`, so the e2e opens no CoreMIDI client). It treats every named port as present unless `debug_set_missing` says otherwise, and records what is sent. With `debug_stale_sources(true)`, `sources()` and `destinations()` answer what the last `refresh()` saw, as CoreMIDI's listener does.
    - `CoreMidiPorts` (macOS live builds) uses `platform_macos::midi::SourceListener::connect_all("VizDAW Surfaces", sink)` for input and `MidiSystem::new("VizDAW Surfaces")` with `open_destination` for output. Its sink gets the source's port key (`core_engine::input::port_key` of its name) and maps it to a name through a snapshot that `refresh()` updates. The sink's body runs in `catch_unwind`.
    - `NoPorts` (other live builds) has no sources and no destinations.
    - It is created on the first `surface_add`, learn or loaded surface, and never before.
    - Its sink pushes `(port, host time, bytes)` into the registry's input queue: its own `Mutex<VecDeque>` (4096 messages), separate from the registry, held only to push and to `mem::take` the whole batch; `try_lock`, drop and count. Drops show in `state.get surfaces` `problems` ("N MIDI messages from surfaces were dropped"). Injected bytes carry host time 0 (now). Each item also carries the surfaces clock's `Instant` at push (`input()` reads `now()`): the learner, the mapper and the shuttle use it; `host_ns` only stamps gestures.
    - **While any surface exists**, the service calls `refresh()` (the listener's `reconnect` and a reopen of the outputs) at most every 2 s, so a unit unplugged while connected is seen within 2 s. A `send` that fails also marks its surface disconnected at once.
    - Why: the spec asks for CoreMIDI notifications. The existing listener has no notification callback, and its `sources()` changes only at `reconnect()`, so polling only while something is disconnected would never see a connected unit vanish (pre-flight I2). Polling reuses tested code (`SourceListener::reconnect`).
    - Cost if wrong:
      - up to 2 s to see a vanished or returning port;
      - a CoreMIDI query on the main thread every 2 s while any surface is configured (the backlog row "Input pickers' MIDI source list off the main thread" covers moving it).
11. **Ruling (which document the surfaces drive): the app's command target (`DocumentStore.noteTarget`). The ticker calls `Session.focus_surfaces()` when the command target changes (not every tick).**
    - The focused session's `idle` services the surfaces: it drains input, decides its learns, maps, applies actions and sends feedback.
    - **Owed releases.** The registry keeps `owed: session id → Vec<(TouchHolder, AutomationTarget)>`. A focus change owes the old session every surface holder's targets and every `Mapper::held()` target; a disconnect and a `surface_remove` owe the unit's `release_all()` to the session that holds them. **Every** session's `idle` (focused or not) first takes and applies its own owed releases; an unfocused session then decides a learn it asked for (Ruling 7) and stops.
    - With no document open, nothing is serviced: input past the queue's capacity is dropped and counted, and Settings › Keys "Learn MIDI" is disabled ("Open a project to learn MIDI").
    - When focus moves, the units get a full refresh from the new session. When the focused session closes, its touches end (`release_touches`), its learn is cancelled, focus goes to none, and the units get a blank refresh (names cleared, faders down, LEDs off, displays blank; a fader the unit still reports touched is left alone, §8.1) until a session is focused again.
    - `surface.inject` focuses the target window's session and services it at once. Because the ticker refocuses only when the command target changes, an injected window keeps the focus until the user picks a window.
    - A learn records the session that asked (Ruling 7).
    - Why: notes already follow the command target (plan 4c Ruling 11); one owner keeps the gesture holders simple. Owed releases must reach the session that holds them, which may no longer be the focused one.
    - Cost if wrong: learning a command needs an open project.
12. **Ruling (what the app does with surface events): `Session.take_surface_events()` returns `SurfaceEventRow`s, which `DocumentModel.idle` drains into its `SurfaceRelay`.**
    - `command { id, programmatic }`: a command from a real device runs through `KeyMonitor.perform(.command(id), doc)`, exactly as a key does. A command caused by `surface.inject` or `midi.inject` (`programmatic: true`) runs through `ControlCommands.run(id, args: [], …)`, which answers `dialog_required` instead of opening a dialog; a refusal's message goes to the status line (`editor.status`) and nothing opens.
    - `select { strip }` sets `selectedStrips` to that strip, and the app reports selection back through `Session.set_surface_selection`.
    - `arm { strip, path }` toggles the arm of the strip's input lane: of the shown sequence's lanes that play through `path`, the first armed one, else the first (`DocumentModel.inputLane(path:)`, which `MixerModel.inputLane(for:)` now calls). Strips that are not AudioPaths carry `path: nil` and do nothing. No mixer window is needed.
    - `jog { detents }` locates by `detents × ppq / 4` from the transport's tick. Jog events within 150 ms of the first unapplied one are summed into **one** locate (a Locate lands a writing pass, 5e-1 Ruling 23, so a spun wheel must not make a step per detent).
    - `Skip` and `Wind` (Ruling 5) are locates the core makes itself; they are not relayed.
    - The arm LED is computed in the core: an AudioPath strip is lit when any lane of any sequence playing through its path is armed.
    - Why: selection and the lane chooser are app state (5e-2 `selectedStrips`, 5d-2 Ruling 4), and commands are run by the app; a programmatic caller must never see a dialog (control spec §7).
    - Cost if wrong: one tick (16 ms) of latency on surface buttons; up to 150 ms on the jog wheel.
13. **Ruling (feedback: what it shows and when): every value a unit or map shows is the *heard* value, sent at most every 20 ms; values a holder moved go at once to every other surface and learn map showing that target.**
    - **Heard values (pre-flight C1).** Every `StripView` value (fader, pan, mute, solo, send 1) and every `ParamView.value` is `Session::heard_value(inner, t)`: the engine's live value, else the automation at the transport tick while playing, else the static value. So motor faders, rings and LEDs follow automation playback, partners follow a Touch/Latch/Write ride (which leaves the static value alone, 5e-1 Ruling 15), and a released fader gets the glide back. It is computed only for the strips some unit or map shows, and only when a unit is due; `existing(doc, seq, t)` is cached per (document version, seq).
    - The spec's "touched faders' partners" are read as the other units and maps that display the moved target.
    - Feedback is diffed against what each unit was last sent (`feedback_sends_only_what_changed`). A full refresh happens on connect, on a bank change and on a focus change.
    - Meters are always resent (the unit decays them): MCU every 300 ms, HUI every 100 ms. **The meters come from the session's meter holds (pre-flight I4):** `Engine::meters()` reads and clears, so one take per `idle` (or per `levels()` call, whichever comes first) feeds two max-holds, `Mixer` and `Surfaces`, each cleared only by its own reader. `Session.levels` reads the Mixer hold; the service reads the Surfaces hold and never reads the DSP load. The on-screen meters keep every peak.
    - `SurfaceFeedbackRow.messages` (the last 64, hex) leaves meters out; `meters_sent` counts them.
    - **Echo guard (pre-flight M8):** an untouched MCU or HUI fader value within 1 LSB (of 16383) of what was last sent to that fader in the last 100 ms is an echo of the motor move, ignored and not counted. Without it, a unit that echoes motor moves turns Read playback into a "Set Fader" step every 500 ms.
    - Why: §8.1; diffing keeps a 4-unit MCU group within MIDI bandwidth; a motor fader that shows the static value while automation plays is the first thing a user sees wrong.
    - Cost if wrong: one more message class to send at once; the echo window too short for a slow unit.
14. **Ruling (MCU handshake and display encodings):**
    - On connect, VizDAW sends Device Query `F0 00 00 66 <id> 00 F7` (id `0x14` main, `0x15` XT).
    - A Host Connection Query `F0 00 00 66 <id> 01 <serial ×7> <challenge ×4> F7` is answered with `F0 00 00 66 <id> 02 <serial ×7> <r ×4> F7`, where:
      - `r0 = 0x7F & (c0 + (c1 ^ 0x0A) - c3)`;
      - `r1 = 0x7F & ((c2 >> 4) ^ (c0 + c3))`;
      - `r2 = 0x7F & ((c3 - (c2 << 2)) ^ (c0 | c1))`;
      - `r3 = 0x7F & (c1 - c2 + (0xF0 ^ (c3 << 4)))`;
      - all in wrapping `u8` arithmetic.
    - Online state:
      - `F0 00 00 66 <id> 03 … F7` (confirmation), or any other valid message, marks the unit online;
      - `… 04 … F7` marks it offline.
    - Units that never handshake still work.
    - Scribble strips: `F0 00 00 66 <id> 12 <offset 0–111> <ASCII…> F7`. Row 1 holds 7 characters of each strip's name plus a space (in plug-in mode, the parameter names); row 2 holds the V-pot value text (pan `<L50`, `C`, `R30>`; send dB `-6.0`; plugin value text).
    - Timecode/BBT display: CC `0x40`–`0x49` (0x49 leftmost). Each character is encoded as ASCII `0x40`–`0x5F` → `− 0x40`, `0x20`–`0x3F` as itself, plus `0x40` for a following dot. The display shows `MixerView.position` with separators turned into dots.
    - Assignment display: CC `0x4B` (left) and `0x4A` (right), showing `PN`, `SE` or `PL`.
    - Meters: `D0 (k << 4 | level)` with `level = 0` below −60 dBFS, else `min(12, floor((dB + 60) / 5) + 1)`.
    - Why: public protocol descriptions; units that skip the handshake are common.
    - Cost if wrong: a unit that insists on the handshake stays offline. README 175 checks a real unit if one is available.
15. **Ruling (HUI tables), from public HUI protocol descriptions:**
    - **Ping:** VizDAW sends `90 00 00` every 1 s; the unit's `90 00 7F` marks it online. With no reply for 3 s it is offline (still connected).
    - **Switches:** in, `B0 0F zz` then `B0 2F 4p` (press) or `0p` (release); out (LEDs), `B0 0C zz` then `B0 2C 4p` or `0p`.
      - Strip zones `0x00`–`0x07`, ports: 0 fader touch, 1 select, 2 mute, 3 solo, 4 auto, 5 v-sel (ignored in 5f-1, Ruling 5), 6 insert, 7 rec-arm.
      - Zone `0x0A`, ports: 0 channel left, 1 bank left, 2 channel right, 3 bank right.
      - Zone `0x0E`, ports: 1 rewind, 2 fast forward, 3 stop, 4 play, 5 record. Rewind and fast forward run the shuttle on press and release (Ruling 5).
    - **Faders:** in and out as `B0 0z hi`, `B0 2z lo`; `value = hi << 7 | lo` over 0–16383.
    - **V-pots:** in, `B0 4v d`, where `d & 0x40` set means clockwise and `d & 0x3F` is the detent count; they drive pan. LED rings out, `B0 1v p` with `p` 1–11.
    - **Meters:** `A0 0k (side << 4 | level)`, side 0 left and 1 right, level 0–12 as in Ruling 14.
    - **Displays:** names `F0 00 00 66 05 00 10 <k 0–7> c c c c F7`; the 40-character display in 8 zones of 10, `F0 00 00 66 05 00 12 <zone> <10 chars> F7`, zones 0–3 on line 1 (position, assignment) and 4–7 on line 2 (the selected strip's name and mode); timecode `F0 00 00 66 05 00 11 <8 digits, rightmost first> F7`.
    - **Jog:** `B0 0D d`, with 0x01–0x3F positive and 0x41–0x7F negative.
    - Why: §8.2's list.
    - Cost if wrong: a button maps differently on a real unit. The tables are one file (`hui.rs`) and README 176 checks a real unit if one is available.
16. **Ruling (`surface.learn`/`surface.unlearn` on the channel).** Two more commands beyond the spec's three. `surface.learn { target? , command? }` starts a learn, and the bytes come from `midi.inject` or a device. `surface.unlearn { target? , command? }` removes maps: `target` removes **every** map to that target as one undo step "Remove MIDI Learn" (`SettingsCmd::RemoveLearn { target }`); `command` removes every command map for that command.
    - `midi.inject` now also feeds surfaces, MIDI learn and learned maps, and answers `ran` when the engine *or* those took the bytes. What they take is not sent to the engine (Ruling 17).
    - Why: the Learn buttons need a channel twin (control spec: every command has one) and hostile-input tests.
    - Cost if wrong: two registry IDs to retire.
17. **Ruling (surface and learned traffic stays out of the lanes; pre-flight I5 for surfaces, owner answer 2026-11-02 for learned messages).**
    - A message is **taken** when it comes from a configured surface's input port; or it is a channel message (CC, pitch bend, note on/off) from a port no surface uses while a learn waits; or it matches a command map, or a learn map of the focused session (`Cc7 {ch, cc}`: `Bn cc`; `Cc14 {ch, cc}`: `Bn cc` and `Bn cc+32`; `PitchBend {ch}`: `En`; `Note {ch, note}`: `9n note` and `8n note`).
    - Taken messages are not recorded, not played thru, not step-entered and not note input. Every consumer asks one process-wide function, `surfaces::takes(port_key, bytes) -> bool`, which reads a claims set (`RwLock`, `try_read`) the registry rewrites when a surface, a map, a learn or the focus changes. The focused session's maps are also re-read when its document changes (undo, redo, revert and load change `Settings.learn` without the learn API): at step 3 the service compares `Settings.learn` with the Mapper's maps; when they differ it calls `Mapper::set`, keeping the state (engaged, MSB holds, idle) of maps whose source and target are unchanged, owes `release_from(TouchHolder::Learn)` for held targets no map drives any more, and rewrites the claims. The consumers:
      - the lane path: `host::midi_from_source` (record, thru, monitoring, "All MIDI Inputs" and named-source lanes alike);
      - the note-input path (note input and step entry): `NoteInput` listens through `SourceListener` (which knows each message's port) instead of `NoteListener`, and its sink drops what `takes` claims;
      - `Session.inject_midi`: taken messages go only to `surfaces::input`.
    - Unlearned messages, and every message of an unclaimed port, pass as before.
    - The lane input picker lists a surface's input port as "<name> (used by a surface)".
    - Why: Logic-style "monitoring" keeps control-surface ports out of track input, and a keyboard knob learned to a fader must not also bend the synth and land in the take (the owner's answer). One function keeps the three consumers in step.
    - Cost if wrong: a user who wants a learned CC also recorded unlearns it, or records from a second port.
18. **Ruling (pick-up for absolute learned controls; owner answer 2026-11-02, in 5f-1).**
    - Pick-up is **per map**: `LearnMap.pickup: bool`, **on by default** for every new map. It applies to absolute `Cc7`, `Cc14` and `PitchBend` maps; relative maps, note maps and command maps ignore it.
    - A map with pick-up on is *engaged* while its idle touch is open. Not engaged, it takes its messages (they stay out of the lanes) but moves nothing until the control **passes** the target's current (heard) value: its new value is within `PICKUP_TOLERANCE` = 0.02 of the normalised range of the current value, or the current value lies between the control's previous value (the last this map saw, engaged or not) and the new one. The value that passes is the touch (`Touch(Some(v))`), and the map follows anywhere from then on.
    - When its idle touch is released (500 ms after its last value), the map disengages, so after automation, the mouse or another control moved the target, the control has to pass the new value again. A control left where it was re-engages on its first move (it is within the tolerance).
    - A map with pick-up off moves the target to the control's value at once (the jump).
    - The learn UI: the MIDI Learn Maps sheet has a "Pick-up" checkbox per map, enabled for absolute CC, 14-bit and pitch-bend maps (and shown dimmed, unticked, for the others), with the help text "The control takes over only once it reaches the current value". Changing it is an undo step "MIDI Learn" (`Session.set_learn_pickup`, a `SetLearn` of the changed map). The channel's `state.get surfaces` needs nothing new; the maps list carries `pickup`.
    - Why on: a knob or fader with no motor is almost never where its target is, and a jump writes a cliff into a Touch, Latch or Write ride; motorised controllers get the value back (Ruling 8), so they already sit at the value and pick up on their first move, for nothing. Off is for a user who wants a slider to jump, one click away.
    - Cost if wrong: a user who expects the jump ticks the box off per map; a control that is noisy near the value engages a little early (within 0.02).

## Carried items

- **`.superpowers/sdd/2026-10-28-automation-ui/carry-to-5f.md`** asks that a surface touch and a mouse touch on the same target have their own touches or a reference count (final review m8). This is Ruling 2, done in Task 6, with Task 8's surface test.
- **README 165a's surface half** (`carry-to-5e-2.md`: "riding with … a surface in 5f"): README 177 (Task 12) rides a fader from a real or emulated unit through every mode.
- **`docs/feature-backlog.md`**: no row names 5f. Task 12 adds the rows "A pre-5f build names a project with learn maps Corrupt" (Ruling 6) and "HUI V-pot assignment" (Ruling 5).

## File map

| File | Task | Role |
|---|---|---|
| `crates/core-surface/{Cargo.toml,src/lib.rs,src/midi.rs,src/idle.rs,src/shuttle.rs,src/mcu.rs}`, `tests/common/mod.rs` | 1–2 | crate, shared types, message splitting, 500 ms helper, shuttle, MCU, shared test views |
| `crates/core-surface/src/hui.rs` | 3 | HUI |
| `crates/core-model/src/learn.rs`, `document.rs`, `command/settings.rs`, `lib.rs` | 4 | `LearnMap`, `Settings.learn`, commands |
| `crates/core-surface/src/learn.rs` | 5 | `Learner`, `Mapper` (with pick-up and `takes`), `CommandMap` |
| `crates/core-ffi/src/session.rs`, `automation.rs`; `AutomationCommands.swift` | 6 | holders, `gesture_from`, the static edit at the first value |
| `crates/core-ffi/src/surfaces.rs`, `crates/core-ffi/src/surfaces/ports.rs` | 7–8 | registry, ports, persistence, owed releases, claims, service, learn glue |
| `crates/core-ffi/src/host.rs`, `input.rs` | 8 | the lanes and note input skip what surfaces take |
| `crates/core-control/src/{registry.rs,request.rs}`, `crates/core-ffi/src/control.rs`, `TransportCommands.swift` | 9 | channel, `transport.play` |
| `app/Sources/VizDAWAppModel/{SurfaceCommands.swift,SurfaceSettings.swift,SurfaceRelay.swift,LearnModel.swift}` | 9–11 | app models (`SurfaceCore` in Task 9) |
| `app/Sources/VizDAW/{SurfacesSettingsView.swift,LearnMapsSheet.swift}` and edits | 9–11 | views, `LiveSurfaceCore` |
| `crates/vizctl/tests/e2e.rs`, `app/README.md`, `docs/feature-backlog.md` | 11–12 | e2e, README, backlog |

---

### Task 1: `core-surface`: the crate, the shared types, message splitting, the idle release, the shuttle, MCU input

**Files:**
- Create: `crates/core-surface/Cargo.toml`, `crates/core-surface/src/lib.rs`, `crates/core-surface/src/midi.rs`, `crates/core-surface/src/idle.rs`, `crates/core-surface/src/shuttle.rs`, `crates/core-surface/src/mcu.rs`
- Test: create `crates/core-surface/tests/common/mod.rs` (the shared strip views), `crates/core-surface/tests/mcu_input.rs`
- Modify: `crates/core-ffi/Cargo.toml` (dependency, so the lock settles once), `Cargo.lock`

**Interfaces:**
- Consumes: `core_model::automation::{AutomationTarget, AutomationMode}`, `core_model::ids::StripId`, `core_model::mixer::{ParamId}`, `core_model::ids::PluginInstanceId`.
- Produces (`core_surface`):
  ```rust
  pub mod hui; pub mod learn; pub mod mcu; pub mod midi; pub mod shuttle; mod idle;
  pub use idle::{IdleTouches, IDLE_RELEASE};
  pub use shuttle::{Shuttle, SHUTTLE_HOLD, WIND_EVERY, wind_rate};
  /// Unit types (spec §8.3).
  #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
  #[serde(rename_all = "snake_case")]
  pub enum SurfaceKind { Mcu, McuXt, Hui }
  /// A gesture phase (spec §7.1); `Touch(None)`: at the value heard now.
  #[derive(Debug, Clone, Copy, PartialEq)]
  pub enum Phase { Touch(Option<f64>), Value(f64), Release }
  /// What a surface asks for (spec §8.1).
  #[derive(Debug, Clone, PartialEq)]
  pub enum SurfaceAction {
      Gesture(AutomationTarget, Phase),
      /// A release after idle (Ruling 2), stamped at the target's last value.
      IdleRelease(AutomationTarget, Instant),
      /// A registry ID (Ruling 5) or a learned command.
      Command(String),
      Bank(i32), Channel(i32),
      Select(StripId), Arm(StripId),
      Jog(i32),
      /// Rewind (−1) / Fast Forward (+1) tapped: the previous/next marker, else bar line (Ruling 5).
      Skip(i32),
      /// Rewind / Fast Forward held: move by this many quarter notes (negative: back).
      Wind(f64),
  }
  /// The V-pot assignment (Ruling 5).
  #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
  pub enum Assign { #[default] Pan, Send, Plugin }
  /// One plugin parameter a V-pot can drive.
  #[derive(Debug, Clone, PartialEq)]
  pub struct ParamView { pub target: AutomationTarget, pub name: String, pub value: f64, pub text: String }
  /// One strip as the surface shows it (normalised heard values, as automation uses them; Ruling 13).
  #[derive(Debug, Clone, PartialEq)]
  pub struct StripView {
      pub id: StripId, pub name: String, pub fader: f64, pub pan: f64,
      pub send: Option<f64>, pub mute: bool, pub solo: bool, pub arm: bool, pub select: bool,
      pub mode: AutomationMode, /// Linear peaks since the last view.
      pub meter: (f32, f32),
  }
  /// The MixerView snapshot (spec §8.1).
  #[derive(Debug, Clone, PartialEq, Default)]
  pub struct MixerView {
      pub strips: Vec<StripView>, pub master: Option<StripView>,
      /// The selected strip's first insert's first 8 parameters (Ruling 5).
      pub plugin: Vec<ParamView>,
      /// Bars·beats as the transport shows it ("12.3.2.120").
      pub position: String,
      pub playing: bool, pub recording: bool, pub looping: bool,
  }
  /// A protocol state machine (spec §8.1; Ruling 3).
  pub trait Surface: Send {
      fn kind(&self) -> SurfaceKind;
      fn input(&mut self, bytes: &[u8], now: Instant) -> Vec<SurfaceAction>;
      fn tick(&mut self, now: Instant) -> Vec<SurfaceAction>;
      fn feedback(&mut self, view: &MixerView, first: usize, now: Instant) -> Vec<Vec<u8>>;
      /// Forget what was sent: the next feedback is complete.
      fn refresh(&mut self);
      /// Messages to send on connect (MCU device query; HUI first ping).
      fn hello(&mut self) -> Vec<Vec<u8>>;
      fn online(&self) -> bool;
      /// Malformed or unknown messages so far (spec §12.2).
      fn ignored(&self) -> u64;
      /// Faders it holds touched now (touch-sensing faders only).
      fn touched(&self) -> Vec<AutomationTarget>;
      /// Every target it holds (touched faders and idle holds), then forgets
      /// them, its touch flags, pending hi bytes and the shuttle (Ruling 3:
      /// a disconnect or a removal).
      fn release_all(&mut self) -> Vec<AutomationTarget>;
  }
  /// A new state machine of `kind`.
  pub fn new_surface(kind: SurfaceKind) -> Box<dyn Surface>;
  // midi.rs
  /// Splits `bytes` into whole messages: channel messages by length, SysEx from F0 to F7.
  /// System Real-Time bytes (F8–FF) are one-byte messages legal anywhere, even inside
  /// another message or a SysEx: they are skipped and neither returned nor reported.
  /// A byte that starts nothing (F4, F5, a lone F7), a cut message, a data byte above
  /// 0x7F or running status are reported as `Err(len)` of the bytes skipped.
  pub fn split(bytes: &[u8]) -> Vec<Result<Vec<u8>, usize>>;
  pub fn hex(bytes: &[u8]) -> String; // "90 68 7f"
  // idle.rs
  pub const IDLE_RELEASE: Duration = Duration::from_millis(500);
  #[derive(Debug, Default)] pub struct IdleTouches { /* target → last value time */ }
  impl IdleTouches {
      /// Touch(Some(v)) the first time, else Value(v).
      pub fn value(&mut self, t: AutomationTarget, v: f64, now: Instant) -> SurfaceAction;
      /// `IdleRelease(t, last value's time)` for every target idle for IDLE_RELEASE.
      pub fn tick(&mut self, now: Instant) -> Vec<SurfaceAction>;
      pub fn held(&self) -> Vec<AutomationTarget>;
      /// `held()`, then forgets them all.
      pub fn take_all(&mut self) -> Vec<AutomationTarget>;
  }
  // shuttle.rs
  /// A press shorter than this is a tap (Ruling 5).
  pub const SHUTTLE_HOLD: Duration = Duration::from_millis(400);
  /// At most one Wind per this.
  pub const WIND_EVERY: Duration = Duration::from_millis(100);
  /// Quarter notes per second after holding for `held` (from the press):
  /// 0 below SHUTTLE_HOLD, then 4, 8 from 1.4 s, 16 from 2.4 s, 32 from 3.4 s on.
  pub fn wind_rate(held: Duration) -> f64;
  #[derive(Debug, Default)] pub struct Shuttle { /* direction, press time, last wind time */ }
  impl Shuttle {
      pub fn press(&mut self, forward: bool, now: Instant);
      /// After a tap `Skip(±1)`; after a hold the `Wind` since the last one (none when zero).
      pub fn release(&mut self, forward: bool, now: Instant) -> Vec<SurfaceAction>;
      /// A `Wind` when held past SHUTTLE_HOLD and WIND_EVERY passed since the last.
      pub fn tick(&mut self, now: Instant) -> Vec<SurfaceAction>;
      /// Forget a press (release_all).
      pub fn clear(&mut self);
  }
  // mcu.rs
  pub struct Mcu { /* kind Mcu or McuXt; shown: [Option<StripId>; 8]; master shown; touched: [bool; 9]; assign; idle; shuttle; last sent (with its time, for the echo guard); online; ignored */ }
  impl Mcu { pub fn new(extender: bool) -> Mcu; pub fn device_id(&self) -> u8; }
  pub fn handshake_response(challenge: [u8; 4]) -> [u8; 4];
  ```

`lib.rs` re-exports and the trait (`input` must never panic on any bytes: every path is `get`/`match`). In `Mcu::input`, each `midi::split` item is decoded by the Ruling 5 and Ruling 14 tables, and the MCU uses these constants:
- `FADER_TOUCH` 0x68–0x70;
- `REC` 0x00–0x07, `SOLO` 0x08, `MUTE` 0x10, `SELECT` 0x18, `VPOT_PUSH` 0x20;
- `ASSIGN_SEND` 0x29, `ASSIGN_PAN` 0x2A, `ASSIGN_PLUGIN` 0x2B;
- `BANK_LEFT` 0x2E, `BANK_RIGHT` 0x2F, `CHANNEL_LEFT` 0x30, `CHANNEL_RIGHT` 0x31;
- `READ` 0x4A, `WRITE` 0x4B, `TRIM` 0x4C, `TOUCH` 0x4D, `LATCH` 0x4E;
- `SAVE` 0x50, `UNDO` 0x51, `CYCLE` 0x56;
- `REWIND` 0x5B, `FFWD` 0x5C, `STOP` 0x5D, `PLAY` 0x5E, `RECORD` 0x5F;
- `JOG` CC 0x3C;
- `VPOT` CC 0x10–0x17 (bit 6 set = counter-clockwise, low 6 bits = detents).

How input maps to actions:
- **Faders.** Pitch bend `E<ch> lsb msb` (ch 0–7 strips, 8 master on the main unit only) → `Value((msb << 7 | lsb) / 16383)` while touched. An untouched fader that moves (a unit without touch sensing) goes through `IdleTouches`, unless it is an echo (Ruling 13: within 1 LSB of what was last sent to it in the last 100 ms; Task 2 sends, so Task 2 tests it).
- **Fader touch.** Velocity ≥ 0x40 → `Touch(None)`, below → `Release`.
- **Buttons.** Buttons act on press (velocity > 0) only, except Rewind and Fast Forward: mute and solo become `Gesture` clicks at the toggled value of the last view; select → `Select`; rec → `Arm`; Play → `Command("transport.play")`, and the other transport and automation buttons → `Command` by Ruling 5; bank and channel buttons → `Bank(±1)`/`Channel(±1)` (main unit only; an extender's are ignored, not counted).
- **Rewind and Fast Forward** (main unit only) go to the `Shuttle`: a press calls `press`, a release (velocity 0) returns `release`'s actions; `tick` adds the shuttle's.
- **V-pots.** A V-pot turn is a value through `IdleTouches` at `current ± 0.01 × detents`, clamped to 0–1. `current` is the V-pot's own last value while its idle touch is open, else the last view's value.

`Cargo.toml`:

```toml
[package]
name = "core-surface"
version = "0.1.0"
edition.workspace = true
license.workspace = true
rust-version.workspace = true
publish = false

[dependencies]
core-model = { path = "../core-model" }
serde = { version = "1", features = ["derive"] }
```

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p crates/core-surface/src crates/core-surface/tests/common
cat > crates/core-surface/tests/common/mod.rs <<'VIZDAW_EOF'
//! Strip views shared by the surface tests (plan 5f-1). A module, not a
//! test file, so its helpers are not run as tests of their own.
#![allow(dead_code)]

use core_model::automation::AutomationMode;
use core_model::ids::StripId;
use core_surface::{MixerView, StripView};

pub fn strip(id: u64, name: &str) -> StripView {
    StripView {
        id: StripId(id),
        name: name.into(),
        fader: 0.5,
        pan: 0.5,
        send: Some(0.25),
        mute: false,
        solo: false,
        arm: false,
        select: false,
        mode: AutomationMode::Read,
        meter: (0.0, 0.0),
    }
}

pub fn view(n: u64) -> MixerView {
    MixerView {
        strips: (1..=n).map(|i| strip(i, &format!("Strip {i}"))).collect(),
        master: Some(strip(100, "Master")),
        position: "1.1.1.0".into(),
        ..MixerView::default()
    }
}
VIZDAW_EOF
cat > crates/core-surface/tests/mcu_input.rs <<'VIZDAW_EOF'
//! MCU input, byte by byte (plan 5f-1 Task 1; spec §8.2, §12.3).

mod common;

use common::view;
use core_model::automation::AutomationTarget as T;
use core_model::ids::StripId;
use core_surface::mcu::{Mcu, handshake_response};
use core_surface::{IDLE_RELEASE, MixerView, Phase, Surface, SurfaceAction as A, midi, wind_rate};
use std::time::{Duration, Instant};

/// A unit that has shown `v` from strip 0.
fn shown(extender: bool, v: &MixerView) -> (Mcu, Instant) {
    let now = Instant::now();
    let mut m = Mcu::new(extender);
    let _ = m.feedback(v, 0, now);
    (m, now)
}

#[test]
fn split_finds_whole_messages_and_skips_garbage() {
    let b = [0x90, 0x68, 0x7F, 0xF0, 0x00, 0x00, 0x66, 0x14, 0x00, 0xF7, 0xE0, 0x10];
    let parts = midi::split(&b);
    assert_eq!(parts.len(), 3);
    assert_eq!(parts[0], Ok(b[0..3].to_vec()));
    assert_eq!(parts[1], Ok(b[3..10].to_vec()));
    assert_eq!(parts[2], Err(2), "a cut pitch bend is skipped and reported");
    assert_eq!(midi::split(&[0x68, 0x7F]), vec![Err(1), Err(1)], "running status is not supported");
    assert_eq!(midi::split(&[0x90, 0x80, 0x00]), vec![Err(3)], "a data byte above 0x7f");
    assert_eq!(
        midi::split(&[0xF8, 0x90, 0xFE, 0x68, 0x7F, 0xF0, 0x00, 0xF8, 0xF7, 0xFF]),
        vec![Ok(vec![0x90, 0x68, 0x7F]), Ok(vec![0xF0, 0x00, 0xF7])],
        "real-time bytes are skipped anywhere, even inside a message, and not reported"
    );
    assert_eq!(midi::split(&[0xF4]), vec![Err(1)], "an undefined status starts nothing");
    assert_eq!(midi::hex(&[0x90, 0x68, 0x7F]), "90 68 7f");
}

#[test]
fn a_touched_fader_moves_and_releases() {
    let v = view(8);
    let (mut m, now) = shown(false, &v);
    assert_eq!(
        m.input(&[0x90, 0x68, 0x7F], now),
        vec![A::Gesture(T::StripFader(StripId(1)), Phase::Touch(None))]
    );
    assert_eq!(m.touched(), vec![T::StripFader(StripId(1))]);
    let a = m.input(&[0xE0, 0x7F, 0x7F], now);
    assert_eq!(a, vec![A::Gesture(T::StripFader(StripId(1)), Phase::Value(1.0))]);
    let a = m.input(&[0xE7, 0x00, 0x20], now);
    match a.as_slice() {
        [A::Gesture(T::StripFader(StripId(8)), Phase::Touch(Some(x)))] => {
            assert!((x - 4096.0 / 16383.0).abs() < 1e-9, "an untouched move is an idle touch")
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        m.input(&[0x90, 0x68, 0x00], now),
        vec![A::Gesture(T::StripFader(StripId(1)), Phase::Release)]
    );
    assert_eq!(
        m.tick(now + IDLE_RELEASE + Duration::from_millis(1)),
        vec![A::IdleRelease(T::StripFader(StripId(8)), now)],
        "500 ms after its last value, stamped at that value"
    );
}

#[test]
fn the_master_fader_is_the_main_units_only() {
    let v = view(8);
    let (mut m, now) = shown(false, &v);
    assert_eq!(
        m.input(&[0x90, 0x70, 0x7F], now),
        vec![A::Gesture(T::StripFader(StripId(100)), Phase::Touch(None))]
    );
    let (mut xt, now) = shown(true, &v);
    assert!(xt.input(&[0x90, 0x70, 0x7F, 0xE8, 0x00, 0x40], now).is_empty());
    assert_eq!(xt.ignored(), 2, "an extender has no master");
}

#[test]
fn buttons_click_select_arm_and_run_commands() {
    let mut v = view(8);
    v.strips[1].mute = true;
    let (mut m, now) = shown(false, &v);
    assert_eq!(
        m.input(&[0x90, 0x11, 0x7F], now),
        vec![
            A::Gesture(T::StripMute(StripId(2)), Phase::Touch(Some(0.0))),
            A::Gesture(T::StripMute(StripId(2)), Phase::Value(0.0)),
            A::Gesture(T::StripMute(StripId(2)), Phase::Release),
        ],
        "a lit mute unmutes"
    );
    assert!(m.input(&[0x90, 0x11, 0x00], now).is_empty(), "releases do nothing");
    assert_eq!(m.input(&[0x90, 0x08, 0x7F], now)[0], A::Gesture(T::StripSolo(StripId(1)), Phase::Touch(Some(1.0))));
    assert_eq!(m.input(&[0x90, 0x1A, 0x7F], now), vec![A::Select(StripId(3))]);
    assert_eq!(m.input(&[0x90, 0x03, 0x7F], now), vec![A::Arm(StripId(4))]);
    for (note, id) in [
        (0x5E, "transport.play"),
        (0x5D, "transport.stop"),
        (0x5F, "transport.record"),
        (0x56, "transport.loop"),
        (0x51, "edit.undo"),
        (0x50, "file.save"),
        (0x4A, "automation.mode.read"),
        (0x4B, "automation.mode.write"),
        (0x4C, "automation.mode.trim"),
        (0x4D, "automation.mode.touch"),
        (0x4E, "automation.mode.latch"),
    ] {
        assert_eq!(m.input(&[0x90, note, 0x7F], now), vec![A::Command(id.into())], "{note:#x}");
    }
    assert_eq!(m.input(&[0x90, 0x2E, 0x7F], now), vec![A::Bank(-1)]);
    assert_eq!(m.input(&[0x90, 0x2F, 0x7F], now), vec![A::Bank(1)]);
    assert_eq!(m.input(&[0x90, 0x30, 0x7F], now), vec![A::Channel(-1)]);
    assert_eq!(m.input(&[0x90, 0x31, 0x7F], now), vec![A::Channel(1)]);
    assert_eq!(m.input(&[0xB0, 0x3C, 0x03], now), vec![A::Jog(3)]);
    assert_eq!(m.input(&[0xB0, 0x3C, 0x41], now), vec![A::Jog(-1)]);
}

/// Owner answer 2026-11-02 (Ruling 5): a tap skips; a hold winds, faster
/// the longer it is held; the rate curve on the passed clock.
#[test]
fn rewind_and_forward_skip_on_a_tap_and_wind_while_held() {
    let ms = Duration::from_millis;
    let v = view(8);
    let (mut m, now) = shown(false, &v);
    assert!(m.input(&[0x90, 0x5C, 0x7F], now).is_empty(), "a press waits: tap or hold");
    assert_eq!(m.input(&[0x90, 0x5C, 0x00], now + ms(150)), vec![A::Skip(1)], "a tap: the next marker or bar");
    assert!(m.input(&[0x90, 0x5B, 0x7F], now).is_empty());
    assert!(m.tick(now + ms(399)).is_empty(), "not held long enough");
    assert!(m.tick(now + ms(450)).is_empty(), "one wind per 100 ms");
    let wind = |a: Vec<A>| match a.as_slice() {
        [A::Wind(q)] => *q,
        other => panic!("{other:?}"),
    };
    assert!((wind(m.tick(now + ms(500))) + 0.1 * 4.0).abs() < 1e-9, "0.1 s at 4 quarter notes a second, back");
    assert!(
        (wind(m.tick(now + ms(1_900))) + (0.9 * 4.0 + 0.5 * 8.0)).abs() < 1e-9,
        "the rate doubles after a second of winding"
    );
    assert!(
        (wind(m.input(&[0x90, 0x5B, 0x00], now + ms(1_950))) + 0.05 * 8.0).abs() < 1e-9,
        "the release winds the rest, and does not skip"
    );
    assert!(m.tick(now + ms(3_000)).is_empty(), "released");
    assert_eq!(wind_rate(ms(399)), 0.0);
    assert_eq!(
        [wind_rate(ms(400)), wind_rate(ms(1_399)), wind_rate(ms(1_400)), wind_rate(ms(2_400)), wind_rate(ms(3_400)), wind_rate(ms(60_000))],
        [4.0, 4.0, 8.0, 16.0, 32.0, 32.0]
    );
    let (mut xt, now) = shown(true, &v);
    assert!(xt.input(&[0x90, 0x5C, 0x7F, 0x90, 0x5C, 0x00], now).is_empty(), "an extender has no transport");
}

#[test]
fn vpots_move_pan_send_and_plugin_parameters() {
    let v = view(8);
    let (mut m, now) = shown(false, &v);
    match m.input(&[0xB0, 0x10, 0x05], now).as_slice() {
        [A::Gesture(T::StripPan(StripId(1)), Phase::Touch(Some(x)))] => assert!((x - 0.55).abs() < 1e-9),
        other => panic!("{other:?}"),
    }
    match m.input(&[0xB0, 0x10, 0x42], now).as_slice() {
        [A::Gesture(T::StripPan(StripId(1)), Phase::Value(x))] => {
            assert!((x - 0.53).abs() < 1e-9, "counter-clockwise from its own last value while held")
        }
        other => panic!("{other:?}"),
    }
    let _ = m.input(&[0x90, 0x29, 0x7F], now);
    match m.input(&[0xB0, 0x11, 0x01], now).as_slice() {
        [A::Gesture(T::SendLevel(StripId(2), s), Phase::Touch(Some(x)))] => {
            assert_eq!(s.0, 0);
            assert!((x - 0.26).abs() < 1e-9);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn release_all_forgets_every_hold() {
    let v = view(8);
    let (mut m, now) = shown(false, &v);
    let _ = m.input(&[0x90, 0x68, 0x7F], now);
    let _ = m.input(&[0xE7, 0x00, 0x20], now);
    let _ = m.input(&[0xB0, 0x11, 0x01], now);
    let _ = m.input(&[0x90, 0x5C, 0x7F], now);
    let mut held = m.release_all();
    held.sort();
    let mut want = vec![T::StripFader(StripId(1)), T::StripFader(StripId(8)), T::StripPan(StripId(2))];
    want.sort();
    assert_eq!(held, want, "the touched fader and both idle holds");
    assert!(m.touched().is_empty());
    assert!(m.tick(now + IDLE_RELEASE * 2).is_empty(), "nothing left to release, and no wind");
    assert!(m.release_all().is_empty());
}

#[test]
fn the_handshake_is_answered_and_marks_the_unit_online() {
    let v = view(8);
    let (mut m, now) = shown(false, &v);
    assert_eq!(m.hello(), vec![vec![0xF0, 0x00, 0x00, 0x66, 0x14, 0x00, 0xF7]]);
    assert!(!m.online());
    let mut q = vec![0xF0, 0x00, 0x00, 0x66, 0x14, 0x01];
    q.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7]);
    q.extend_from_slice(&[0x10, 0x20, 0x30, 0x40]);
    q.push(0xF7);
    assert!(m.input(&q, now).is_empty());
    let r = handshake_response([0x10, 0x20, 0x30, 0x40]);
    assert_eq!(
        r,
        [
            0x7F & 0x10u8.wrapping_add(0x20 ^ 0x0A).wrapping_sub(0x40),
            0x7F & ((0x30 >> 4) ^ 0x10u8.wrapping_add(0x40)),
            0x7F & (0x40u8.wrapping_sub(0x30 << 2) ^ (0x10 | 0x20)),
            0x7F & 0x20u8.wrapping_sub(0x30).wrapping_add(0xF0 ^ (0x40u8 << 4)),
        ]
    );
    let mut fb = m.feedback(&v, 0, now);
    let mut want = vec![0xF0, 0x00, 0x00, 0x66, 0x14, 0x02, 1, 2, 3, 4, 5, 6, 7];
    want.extend_from_slice(&r);
    want.push(0xF7);
    assert!(fb.contains(&want), "the reply goes out with the next feedback");
    fb.clear();
    assert!(m.online(), "a valid query marks it online");
    assert_eq!(Mcu::new(true).device_id(), 0x15);
}

#[test]
fn malformed_bytes_are_ignored_and_counted() {
    let v = view(3);
    let (mut m, now) = shown(false, &v);
    let before = m.ignored();
    for b in [
        &[0x90, 0x68][..],
        &[0xF0, 0x00, 0x00][..],
        &[0x90, 0x7F, 0x7F][..],
        &[0x91, 0x68, 0x7F][..],
        &[0xF4][..],
        &[0x90, 0x05, 0x7F][..],
    ] {
        let _ = m.input(b, now);
    }
    assert_eq!(m.ignored(), before + 6, "cut, unterminated, unknown note, channel 2, an undefined status, rec on an empty slot");
    let _ = m.input(&[0xF8, 0xFE, 0xFE], now);
    assert_eq!(m.ignored(), before + 6, "clock and active sensing are not counted");
    let mut fresh = Mcu::new(false);
    assert!(fresh.input(&[0x90, 0x10, 0x7F], now).is_empty(), "nothing shown yet");
    assert_eq!(fresh.ignored(), 1);
}
VIZDAW_EOF
```

The first-strip constants above are the Ruling 5/14 tables; `0x90 0x05` is rec on strip 6, which `view(3)` leaves empty. The untouched fader moves go to 0.25 (`E7 00 20`), away from the 0.5 the views show, so Task 2's echo guard (a move within 1 LSB of what was just sent) never mistakes them for echoes. The wind values are the integral of `wind_rate`: 400–500 ms at 4, 500–1400 ms at 4 and 1400–1900 ms at 8, 1900–1950 ms at 8, all negative for Rewind.

- [ ] **Step 2: Run them to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-surface --test mcu_input 2>&1 | tail -5
```
Expected: FAIL — package `core-surface` not found (no manifest yet).

- [ ] **Step 3: Write the crate.** Write `Cargo.toml` as shown, `lib.rs` (the doc header "`core-surface`: control-surface protocol state machines and MIDI learn (spec §8). Pure Rust, no I/O: bytes in, actions and bytes out; time is passed in", the lint attributes, the items in Interfaces), `midi.rs`, `idle.rs`, `shuttle.rs` and `mcu.rs`.
   - `shuttle.rs`: `wind_rate` is the step function of Ruling 5; the wind of an interval `[a, b)` of held time is the exact integral of that step function (sum each constant piece). `tick` emits a `Wind` only once `WIND_EVERY` passed since the last emitted one (or since `SHUTTLE_HOLD` for the first) and covers everything since it; `release` after a hold emits the rest, after a tap `Skip`.
   - `feedback` for this task returns only queued replies (the handshake) and records `shown` from `view.strips[first..first+8]` and the master. Task 2 adds the real feedback.
   - Add `hui.rs` and `learn.rs` as stubs, each holding only their module doc, so the module tree compiles. `new_surface(Hui)` returns `Mcu::new(false)` until Task 3 replaces it, with a `// Task 3` comment.

   In `crates/core-ffi/Cargo.toml`:

   Find:
   ```toml
   core-media = { path = "../core-media" }
   ```
   Replace:
   ```toml
   core-media = { path = "../core-media" }
   core-surface = { path = "../core-surface" }
   ```

- [ ] **Step 4: Run the tests and the gates**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-surface 2>&1 | tail -3
cargo fmt --all && cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -2
cargo clippy --workspace --all-targets --target x86_64-unknown-linux-gnu --target-dir target/linux-check -- -D warnings 2>&1 | tail -2
git diff --stat Cargo.lock
```
Expected: 9 passed; clippy clean on both targets; `Cargo.lock` shows only the `core-surface` package and `core-ffi`'s new dependency line.

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-surface/Cargo.toml crates/core-surface/src/lib.rs crates/core-surface/src/midi.rs crates/core-surface/src/idle.rs crates/core-surface/src/shuttle.rs crates/core-surface/src/mcu.rs crates/core-surface/src/hui.rs crates/core-surface/src/learn.rs crates/core-surface/tests/common/mod.rs crates/core-surface/tests/mcu_input.rs crates/core-ffi/Cargo.toml Cargo.lock
git commit -m "core-surface: the crate, message splitting, the 500 ms idle release, the transport shuttle and MCU input (plan 5f-1 Task 1)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -14
```
Counts: Rust +9.

---

### Task 2: MCU feedback: faders, LEDs, rings, scribble strips, displays, meters, the echo guard; banks of one main and three extenders

**Files:**
- Modify: `crates/core-surface/src/mcu.rs`, `crates/core-surface/src/lib.rs` (the `Bank` helper)
- Test: create `crates/core-surface/tests/mcu_feedback.rs`

**Interfaces:**
- Consumes: Task 1's types.
- Produces:
  ```rust
  /// A bank group's layout (Ruling 4).
  #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
  pub struct Bank { pub offset: usize, pub units: usize }
  impl Bank {
      pub fn width(&self) -> usize;                       // 8 × units
      /// The first strip unit `k` (0-based, in position order) shows.
      pub fn first(&self, k: usize) -> usize;             // offset + 8k
      /// Applies Bank(±n) / Channel(±n) and clamps to `0..=strips − width` (0 when fewer).
      pub fn shift(&mut self, a: &SurfaceAction, strips: usize);
      pub fn clamp(&mut self, strips: usize);
  }
  /// The 7-segment code of one display character (Ruling 14).
  pub fn seg(c: char, dot: bool) -> u8;
  /// How long a sent fader value is an echo (Ruling 13).
  pub const ECHO_WINDOW: Duration = Duration::from_millis(100);
  ```

`Mcu::feedback` writes the messages for strips `view.strips[first..first+8]` (blank where there are fewer) and the master, and only for what differs from what this unit was last sent (a `Sent` cache per element):
- **Faders:** `E<ch> lsb msb` from `round(fader × 16383)`. Skipped while that fader is touched. On release, the cache entry for that fader is cleared, so the next feedback sends it once. Each sent fader value is kept with its time for the echo guard.
- **LEDs:** `90 <note> 7F|00` for rec, solo, mute and select per strip; play, stop, record and cycle from `playing`, `recording` and `looping` (main only); the assign buttons; and the automation buttons, lit for the first selected strip's mode.
- **V-pot rings:** `B0 <0x30+k> <mode << 4 | pos>`, with `pos = 1 + round(v × 10)`. Pan is in mode 1 (boost/cut), send in mode 2 (wrap), plugin in mode 0.
- **Scribble strips:** one SysEx per changed row (`F0 00 00 66 <id> 12 <0 or 56> <56 chars> F7`). Names are truncated to 7 characters plus a space; in plug-in mode row 1 holds the 8 parameter names the same way. Row 2 holds the value text by Ruling 14.
- **Displays:** timecode/BBT (CC 0x40–0x49, main only) and assignment (CC 0x4A/0x4B, main only).
- **Meters:** `D0 <k << 4 | level>` for every shown strip, sent at most every 300 ms (Ruling 13).
- **The echo guard (input side):** an untouched fader's pitch bend within 1 LSB of the value last sent to that fader, less than `ECHO_WINDOW` after it was sent, is ignored and not counted.

`refresh()` clears the cache and makes the meters due.

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-surface/tests/mcu_feedback.rs <<'VIZDAW_EOF'
//! MCU feedback and banks (plan 5f-1 Task 2; spec §8.1, §8.2, §12.3).

mod common;

use common::view;
use core_model::automation::{AutomationMode, AutomationTarget as T};
use core_model::ids::{PluginInstanceId, StripId};
use core_model::mixer::ParamId;
use core_surface::mcu::Mcu;
use core_surface::{Bank, ParamView, Phase, Surface, SurfaceAction as A, seg};
use std::time::{Duration, Instant};

fn has(fb: &[Vec<u8>], m: &[u8]) -> bool {
    fb.iter().any(|x| x.as_slice() == m)
}

#[test]
fn faders_leds_and_rings_follow_the_view() {
    let mut v = view(8);
    v.strips[0].fader = 1.0;
    v.strips[1].mute = true;
    v.strips[2].solo = true;
    v.strips[3].select = true;
    v.strips[3].mode = AutomationMode::Touch;
    v.strips[0].pan = 0.0;
    v.playing = true;
    let now = Instant::now();
    let mut m = Mcu::new(false);
    let fb = m.feedback(&v, 0, now);
    assert!(has(&fb, &[0xE0, 0x7F, 0x7F]), "strip 1 at the top");
    assert!(has(&fb, &[0xE1, 0x00, 0x40]), "0.5 → 8192 (lsb 0, msb 0x40)");
    assert!(has(&fb, &[0x90, 0x11, 0x7F]), "mute 2 lit");
    assert!(has(&fb, &[0x90, 0x0A, 0x7F]), "solo 3 lit");
    assert!(has(&fb, &[0x90, 0x1B, 0x7F]), "select 4 lit");
    assert!(has(&fb, &[0x90, 0x4D, 0x7F]), "Touch lit for the selected strip");
    assert!(has(&fb, &[0x90, 0x5E, 0x7F]), "play lit");
    assert!(has(&fb, &[0xB0, 0x30, 0x11]), "pan hard left: mode 1, position 1");
    assert!(has(&fb, &[0xB0, 0x31, 0x16]), "centre: position 6");
}

#[test]
fn feedback_sends_only_what_changed() {
    let mut v = view(8);
    let now = Instant::now();
    let mut m = Mcu::new(false);
    let first = m.feedback(&v, 0, now);
    assert!(first.len() > 20);
    assert!(m.feedback(&v, 0, now + Duration::from_millis(20)).is_empty());
    v.strips[4].mute = true;
    assert_eq!(
        m.feedback(&v, 0, now + Duration::from_millis(40)),
        vec![vec![0x90, 0x14, 0x7F]]
    );
    m.refresh();
    assert_eq!(m.feedback(&v, 0, now + Duration::from_millis(60)).len(), first.len());
}

#[test]
fn no_fader_feedback_while_touched_then_one_on_release() {
    let mut v = view(8);
    let now = Instant::now();
    let mut m = Mcu::new(false);
    let _ = m.feedback(&v, 0, now);
    let _ = m.input(&[0x90, 0x68, 0x7F], now);
    v.strips[0].fader = 0.25;
    let fb = m.feedback(&v, 0, now + Duration::from_millis(20));
    assert!(fb.iter().all(|x| x.first() != Some(&0xE0)), "{fb:?}");
    let _ = m.input(&[0x90, 0x68, 0x00], now);
    let fb = m.feedback(&v, 0, now + Duration::from_millis(40));
    assert_eq!(fb.iter().filter(|x| x.first() == Some(&0xE0)).count(), 1);
    assert!(has(&fb, &[0xE0, 0x00, 0x20]), "0.25 → 4096: lsb 0, msb 0x20");
}

#[test]
fn scribble_strips_hold_names_and_values() {
    let mut v = view(2);
    v.strips[0].name = "Lead Vocal Double".into();
    let now = Instant::now();
    let mut m = Mcu::new(true);
    let fb = m.feedback(&v, 0, now);
    let row1 = fb
        .iter()
        .find(|x| x.starts_with(&[0xF0, 0x00, 0x00, 0x66, 0x15, 0x12, 0x00]))
        .expect("row 1 for the extender");
    let text: String = row1[7..row1.len() - 1].iter().map(|b| *b as char).collect();
    assert_eq!(text.len(), 56);
    assert!(text.starts_with("Lead Vo Strip 2 "), "{text:?}");
    assert!(text[16..].chars().all(|c| c == ' '), "blank slots");
    let row2 = fb
        .iter()
        .find(|x| x.starts_with(&[0xF0, 0x00, 0x00, 0x66, 0x15, 0x12, 56]))
        .expect("row 2");
    let text: String = row2[7..row2.len() - 1].iter().map(|b| *b as char).collect();
    assert!(text.starts_with("   C   "), "pan centre: {text:?}");
}

#[test]
fn displays_and_meters() {
    let mut v = view(2);
    v.position = "12.3.2.120".into();
    v.strips[0].meter = (1.0, 0.5);
    let now = Instant::now();
    let mut m = Mcu::new(false);
    let fb = m.feedback(&v, 0, now);
    assert!(has(&fb, &[0xB0, 0x4B, seg('P', false)]));
    assert!(has(&fb, &[0xB0, 0x4A, seg('N', false)]));
    assert!(has(&fb, &[0xB0, 0x40, seg('0', false)]), "rightmost digit");
    assert!(has(&fb, &[0xD0, 0x0C]), "strip 1 at 0 dBFS: level 12");
    assert_eq!(seg('A', false), 0x01);
    assert_eq!(seg('7', true), 0x77);
    v.strips[0].meter = (0.0, 0.0);
    let soon = m.feedback(&v, 0, now + Duration::from_millis(100));
    assert!(soon.iter().all(|x| x.first() != Some(&0xD0)), "meters wait 300 ms");
    let later = m.feedback(&v, 0, now + Duration::from_millis(310));
    assert!(has(&later, &[0xD0, 0x00]));
}

#[test]
fn banks_of_one_main_and_three_extenders() {
    let v = view(40);
    let mut bank = Bank { offset: 0, units: 4 };
    assert_eq!(bank.width(), 32);
    assert_eq!((0..4).map(|k| bank.first(k)).collect::<Vec<_>>(), vec![0, 8, 16, 24]);
    let now = Instant::now();
    let mut xt = Mcu::new(true);
    let _ = xt.feedback(&v, bank.first(2), now);
    assert_eq!(
        xt.input(&[0x90, 0x18, 0x7F], now),
        vec![A::Select(StripId(17))],
        "the third unit's first strip"
    );
    bank.shift(&A::Bank(1), v.strips.len());
    assert_eq!(bank.offset, 8, "40 − 32: clamped");
}

#[test]
fn bank_and_channel_clamp_to_the_strips() {
    let mut b = Bank { offset: 0, units: 1 };
    b.shift(&A::Bank(-1), 20);
    assert_eq!(b.offset, 0);
    b.shift(&A::Bank(1), 20);
    assert_eq!(b.offset, 8);
    b.shift(&A::Bank(1), 20);
    assert_eq!(b.offset, 12);
    b.shift(&A::Channel(-1), 20);
    assert_eq!(b.offset, 11);
    b.clamp(3);
    assert_eq!(b.offset, 0, "fewer strips than the unit");
}

#[test]
fn an_empty_slot_is_blank_and_ignored() {
    let v = view(3);
    let now = Instant::now();
    let mut m = Mcu::new(false);
    let fb = m.feedback(&v, 0, now);
    assert!(has(&fb, &[0xE5, 0x00, 0x00]), "an empty slot's fader goes down");
    assert!(has(&fb, &[0x90, 0x15, 0x00]), "and its mute is dark");
    let n = m.ignored();
    assert!(m.input(&[0x90, 0x15, 0x7F, 0xE5, 0x00, 0x40], now).is_empty());
    assert_eq!(m.ignored(), n + 2);
}

/// Pre-flight M18: V-pots in Plug-in drive the selected strip's first
/// insert's parameters, whose names fill row 1.
#[test]
fn plugin_mode_drives_the_selected_inserts_parameters() {
    let mut v = view(8);
    v.plugin = (0..8)
        .map(|i| ParamView {
            target: T::PluginParam(PluginInstanceId(5), ParamId(i)),
            name: format!("Prm{i}"),
            value: 0.5,
            text: format!("{i}"),
        })
        .collect();
    let now = Instant::now();
    let mut m = Mcu::new(false);
    let _ = m.feedback(&v, 0, now);
    let _ = m.input(&[0x90, 0x2B, 0x7F], now);
    match m.input(&[0xB0, 0x12, 0x01], now).as_slice() {
        [A::Gesture(T::PluginParam(PluginInstanceId(5), ParamId(2)), Phase::Touch(Some(x)))] => {
            assert!((x - 0.51).abs() < 1e-9, "V-pot 3: the third parameter, one detent up")
        }
        other => panic!("{other:?}"),
    }
    let fb = m.feedback(&v, 0, now + Duration::from_millis(20));
    let row1 = fb
        .iter()
        .find(|x| x.starts_with(&[0xF0, 0x00, 0x00, 0x66, 0x14, 0x12, 0x00]))
        .expect("row 1 changes to the parameter names");
    let text: String = row1[7..row1.len() - 1].iter().map(|b| *b as char).collect();
    assert!(text.starts_with("Prm0    Prm1    Prm2    "), "{text:?}");
    assert!(has(&fb, &[0x90, 0x2B, 0x7F]), "Plug-in lit");
    assert!(has(&fb, &[0xB0, 0x4B, seg('P', false)]) && has(&fb, &[0xB0, 0x4A, seg('L', false)]), "PL");
}

/// Pre-flight M8: a unit that echoes motor moves does not ride its own fader.
#[test]
fn an_echo_of_a_motor_move_is_ignored() {
    let v = view(8);
    let now = Instant::now();
    let mut m = Mcu::new(false);
    let _ = m.feedback(&v, 0, now);
    let n = m.ignored();
    assert!(
        m.input(&[0xE0, 0x01, 0x40], now + Duration::from_millis(50)).is_empty(),
        "8193 within 1 LSB of the 8192 sent 50 ms ago: an echo"
    );
    assert_eq!(m.ignored(), n, "and not counted");
    match m.input(&[0xE0, 0x00, 0x40], now + Duration::from_millis(150)).as_slice() {
        [A::Gesture(T::StripFader(StripId(1)), Phase::Touch(Some(_)))] => {}
        other => panic!("after the echo window it is a move: {other:?}"),
    }
}
VIZDAW_EOF
```

In the scribble test, `Strip 2` fills characters 8–15, because each name takes 7 characters plus a space.

- [ ] **Step 2: Run them to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-surface --test mcu_feedback 2>&1 | tail -5
```
Expected: FAIL — `Bank` and `seg` not found.

- [ ] **Step 3: Implement MCU feedback, the echo guard and `Bank`** in `mcu.rs` and `lib.rs`, following the encoding above and Rulings 13–14.

- [ ] **Step 4: Run the tests and the gates** (as in Task 1 Step 4; `cargo test -p core-surface`). Expected: 19 passed.

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-surface/src/mcu.rs crates/core-surface/src/lib.rs crates/core-surface/tests/mcu_feedback.rs
git commit -m "core-surface: MCU feedback (faders never while touched, LEDs, rings, scribble strips, displays, meters, the echo guard) and banks (plan 5f-1 Task 2)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -6
```
Counts: Rust +10.

---

### Task 3: HUI

**Files:**
- Modify: `crates/core-surface/src/hui.rs`, `crates/core-surface/src/lib.rs` (`new_surface(Hui)`)
- Test: create `crates/core-surface/tests/hui.rs`

**Interfaces:**
- Consumes: Tasks 1–2 (`Shuttle`, `IdleTouches`, `ECHO_WINDOW`).
- Produces: `pub struct Hui` implementing `Surface` by Ruling 15; `pub const PING: [u8; 3] = [0x90, 0x00, 0x00];`, `pub const PING_EVERY: Duration = Duration::from_secs(1);`.

`Hui` keeps the last zone select (`B0 0F zz`) and decodes the next `B0 2F` as that zone's port. A port message with no zone select before it is ignored and counted. A fader value uses the pair of its hi CC (`0x00+z`) and lo CC (`0x20+z`): the hi value is held until the lo arrives, and applied alone (`hi << 7`) when another hi comes first. The echo guard of Ruling 13 applies to the pair's value. Rewind and fast forward (zone 0x0E ports 1 and 2) go to a `Shuttle` on press and release, as on the MCU. V-Sel (port 5 of a strip zone) is ignored, not counted. `tick` returns idle releases and the shuttle's winds; `feedback` emits `PING` whenever `PING_EVERY` has passed since the last one, and `online()` is true while a `90 00 7F` arrived within 3 s. `release_all` also drops a pending hi byte.

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-surface/tests/hui.rs <<'VIZDAW_EOF'
//! HUI, byte by byte (plan 5f-1 Task 3; spec §8.2, §12.3).

mod common;

use common::view;
use core_model::automation::AutomationTarget as T;
use core_model::ids::StripId;
use core_surface::hui::{Hui, PING};
use core_surface::{Phase, Surface, SurfaceAction as A};
use std::time::{Duration, Instant};

fn has(fb: &[Vec<u8>], m: &[u8]) -> bool {
    fb.iter().any(|x| x.as_slice() == m)
}

fn shown() -> (Hui, Instant) {
    let now = Instant::now();
    let mut h = Hui::new();
    let _ = h.feedback(&view(8), 0, now);
    (h, now)
}

#[test]
fn zone_and_port_pairs_touch_and_move_a_fader() {
    let (mut h, now) = shown();
    assert_eq!(
        h.input(&[0xB0, 0x0F, 0x02, 0xB0, 0x2F, 0x40], now),
        vec![A::Gesture(T::StripFader(StripId(3)), Phase::Touch(None))]
    );
    let a = h.input(&[0xB0, 0x02, 0x7F, 0xB0, 0x22, 0x7F], now);
    assert_eq!(a, vec![A::Gesture(T::StripFader(StripId(3)), Phase::Value(1.0))]);
    assert_eq!(
        h.input(&[0xB0, 0x0F, 0x02, 0xB0, 0x2F, 0x00], now),
        vec![A::Gesture(T::StripFader(StripId(3)), Phase::Release)]
    );
}

#[test]
fn switches_mute_solo_select_arm_bank_and_transport() {
    let (mut h, now) = shown();
    let z = |zone: u8, port: u8| [0xB0, 0x0F, zone, 0xB0, 0x2F, 0x40 | port];
    assert_eq!(h.input(&z(0, 2), now)[0], A::Gesture(T::StripMute(StripId(1)), Phase::Touch(Some(1.0))));
    assert_eq!(h.input(&z(1, 3), now)[0], A::Gesture(T::StripSolo(StripId(2)), Phase::Touch(Some(1.0))));
    assert_eq!(h.input(&z(4, 1), now), vec![A::Select(StripId(5))]);
    assert_eq!(h.input(&z(5, 7), now), vec![A::Arm(StripId(6))]);
    assert_eq!(h.input(&z(0x0A, 1), now), vec![A::Bank(-1)]);
    assert_eq!(h.input(&z(0x0A, 3), now), vec![A::Bank(1)]);
    assert_eq!(h.input(&z(0x0A, 0), now), vec![A::Channel(-1)]);
    assert_eq!(h.input(&z(0x0E, 4), now), vec![A::Command("transport.play".into())]);
    assert_eq!(h.input(&z(0x0E, 3), now), vec![A::Command("transport.stop".into())]);
    assert_eq!(h.input(&z(0x0E, 5), now), vec![A::Command("transport.record".into())]);
    assert!(h.input(&z(0x0E, 2), now).is_empty(), "fast forward waits: tap or hold");
    assert_eq!(h.input(&[0xB0, 0x0F, 0x0E, 0xB0, 0x2F, 0x02], now), vec![A::Skip(1)], "a tap");
    assert_eq!(h.input(&[0xB0, 0x0D, 0x02], now), vec![A::Jog(2)]);
    assert_eq!(h.input(&[0xB0, 0x0D, 0x42], now), vec![A::Jog(-2)]);
    let n = h.ignored();
    assert!(h.input(&z(0, 5), now).is_empty(), "V-Sel: HUI V-pots are pan only in 5f-1");
    assert_eq!(h.ignored(), n, "and it is not counted");
}

#[test]
fn vpots_turn_by_their_detents() {
    let (mut h, now) = shown();
    match h.input(&[0xB0, 0x40, 0x43], now).as_slice() {
        [A::Gesture(T::StripPan(StripId(1)), Phase::Touch(Some(x)))] => assert!((x - 0.53).abs() < 1e-9),
        other => panic!("{other:?}"),
    }
    match h.input(&[0xB0, 0x40, 0x01], now).as_slice() {
        [A::Gesture(T::StripPan(StripId(1)), Phase::Value(x))] => assert!((x - 0.52).abs() < 1e-9, "from its own last value"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn ping_display_meters_and_leds() {
    let mut v = view(8);
    v.strips[0].mute = true;
    v.strips[1].meter = (1.0, 0.0);
    v.strips[0].name = "Bass Guitar".into();
    let now = Instant::now();
    let mut h = Hui::new();
    let fb = h.feedback(&v, 0, now);
    assert!(has(&fb, &PING));
    assert!(has(&fb, &[0xB0, 0x0C, 0x00]) && has(&fb, &[0xB0, 0x2C, 0x42]), "zone 0, mute LED on");
    assert!(has(&fb, &[0xA0, 0x01, 0x0C]), "strip 2 left at 0 dBFS");
    assert!(has(&fb, &[0xA0, 0x01, 0x10]), "strip 2 right silent: side 1, level 0");
    assert!(has(&fb, &[0xF0, 0x00, 0x00, 0x66, 0x05, 0x00, 0x10, 0x00, b'B', b'a', b's', b's', 0xF7]));
    assert!(
        fb.iter().any(|x| x.starts_with(&[0xF0, 0x00, 0x00, 0x66, 0x05, 0x00, 0x12, 0x00]) && x.len() == 19),
        "a 10-character zone of the 40-character display"
    );
    assert!(!h.online());
    assert!(h.input(&[0x90, 0x00, 0x7F], now).is_empty());
    assert!(h.online(), "the ping is answered");
    let fb = h.feedback(&v, 0, now + Duration::from_millis(500));
    assert!(!has(&fb, &PING), "once a second");
    let fb = h.feedback(&v, 0, now + Duration::from_millis(1001));
    assert!(has(&fb, &PING));
    let _ = h.feedback(&v, 0, now + Duration::from_millis(4100));
    assert!(!h.online(), "no reply for 3 s");
}

#[test]
fn hui_fader_touch_blocks_feedback() {
    let mut v = view(8);
    let (mut h, now) = shown();
    let _ = h.input(&[0xB0, 0x0F, 0x00, 0xB0, 0x2F, 0x40], now);
    v.strips[0].fader = 0.0;
    let fb = h.feedback(&v, 0, now + Duration::from_millis(20));
    assert!(fb.iter().all(|x| !(x.first() == Some(&0xB0) && x.get(1) == Some(&0x00))), "{fb:?}");
    let _ = h.input(&[0xB0, 0x0F, 0x00, 0xB0, 0x2F, 0x00], now);
    let fb = h.feedback(&v, 0, now + Duration::from_millis(40));
    assert!(has(&fb, &[0xB0, 0x00, 0x00]) && has(&fb, &[0xB0, 0x20, 0x00]));
}

#[test]
fn malformed_bytes_are_ignored_and_counted() {
    let (mut h, now) = shown();
    let n = h.ignored();
    let _ = h.input(&[0xB0, 0x2F, 0x40], now);
    let _ = h.input(&[0xB0, 0x0F, 0x0C, 0xB0, 0x2F, 0x40], now);
    let _ = h.input(&[0xF0, 0x00], now);
    assert_eq!(h.ignored(), n + 3, "a port with no zone, an unknown zone, a cut SysEx");
}
VIZDAW_EOF
```

- [ ] **Step 2: Run them to see them fail** (`cargo test -p core-surface --test hui`). Expected: FAIL — `Hui` not found.

- [ ] **Step 3: Implement `Hui`** by Ruling 15, and make `new_surface(SurfaceKind::Hui)` return it.

- [ ] **Step 4: Run the tests and the gates.** Expected: 25 passed in `core-surface`.

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-surface/src/hui.rs crates/core-surface/src/lib.rs crates/core-surface/tests/hui.rs
git commit -m "core-surface: HUI (zones and ports, fader pairs with touch, the shuttle, ping, displays, meters) (plan 5f-1 Task 3)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -6
```
Counts: Rust +6.

---

### Task 4: Learn maps in the project (`core-model`)

**Files:**
- Create: `crates/core-model/src/learn.rs`
- Modify: `crates/core-model/src/lib.rs`, `crates/core-model/src/document.rs` (`Settings.learn`), `crates/core-model/src/command/settings.rs`
- Test: create `crates/core-model/tests/learn.rs`

**Interfaces:**
- Produces (`core_model::learn`):
  ```rust
  /// The most learn maps a project holds (Ruling 6).
  pub const MAX_LEARN_MAPS: usize = 512;
  /// What a control sends (spec §4.8); channels 0–15.
  #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
  #[serde(rename_all = "snake_case")]
  pub enum LearnKind { Cc7 { channel: u8, cc: u8 }, Cc14 { channel: u8, cc: u8 }, PitchBend { channel: u8 }, Note { channel: u8, note: u8 } }
  #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
  #[serde(deny_unknown_fields)]
  pub struct LearnSource { pub port: String, pub kind: LearnKind }
  #[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
  #[serde(rename_all = "snake_case")]
  pub enum LearnMode { #[default] Absolute, Relative2sComplement, RelativeBinOffset }
  #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
  #[serde(deny_unknown_fields)]
  pub struct LearnMap {
      pub source: LearnSource, pub target: AutomationTarget,
      #[serde(default)] pub mode: LearnMode,
      /// Ruling 18: on unless switched off; absolute CC, 14-bit and pitch-bend maps only.
      #[serde(default = "pickup_default")] pub pickup: bool,
  }
  /// `true` (Ruling 18).
  pub fn pickup_default() -> bool;
  impl LearnSource {
      /// Ruling 6's checks.
      pub fn check(&self) -> crate::error::Result<()>;
      /// "Port · CC 7 ch 1", "Port · CC 1/33 ch 1", "Port · Pitch Bend ch 2", "Port · Note C4 ch 1".
      pub fn describe(&self) -> String;
  }
  impl LearnMap {
      /// Whether pick-up applies to it (absolute CC, 14-bit or pitch bend).
      pub fn picks_up(&self) -> bool;
  }
  ```
  - `Settings.learn: Vec<LearnMap>`, `#[serde(default, skip_serializing_if = "Vec::is_empty")]`.
  - `SettingsCmd::SetLearn { map: LearnMap }`, named "MIDI Learn". It checks the source and that the target exists (`automation::target_exists`), refuses a `VelocityScale` target ("velocity scale cannot be learned"), replaces a map with an equal `source`, and refuses a new map past `MAX_LEARN_MAPS`.
  - `SettingsCmd::RemoveLearn { source: Option<LearnSource>, target: Option<AutomationTarget> }`, named "Remove MIDI Learn". Exactly one of the two: `source` removes that map, `target` every map to that target. It refuses both, neither, and a removal that matches nothing.
  - `describe` names notes with `command::event::note_name` (middle C, 60, is "C4" there and in the editors' `PitchMath`).

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-model/tests/learn.rs <<'VIZDAW_EOF'
//! Learn maps in the project (plan 5f-1 Task 4; spec §4.8; Rulings 6, 18).

use core_model::automation::AutomationTarget as T;
use core_model::command::{Command, SettingsCmd, apply};
use core_model::document::Document;
use core_model::learn::{LearnKind, LearnMap, LearnMode, LearnSource, MAX_LEARN_MAPS};

fn doc() -> Document {
    Document::new()
}

fn src(port: &str, kind: LearnKind) -> LearnSource {
    LearnSource { port: port.into(), kind }
}

fn map(source: LearnSource, target: T) -> LearnMap {
    LearnMap { source, target, mode: LearnMode::Absolute, pickup: true }
}

fn set(map: LearnMap) -> Command {
    Command::Settings(SettingsCmd::SetLearn { map })
}

fn master_fader(d: &Document) -> T {
    T::StripFader(d.mixer().master())
}

#[test]
fn set_replaces_by_source_and_remove_takes_it_back() {
    let d = doc();
    let s = src("Knobs", LearnKind::Cc7 { channel: 0, cc: 7 });
    let m = map(s.clone(), master_fader(&d));
    let cmd = set(m.clone());
    assert_eq!(cmd.name(), "MIDI Learn");
    let d = apply(&d, &cmd).unwrap().doc;
    assert_eq!(d.settings().learn, vec![m.clone()]);
    let relative = LearnMap { mode: LearnMode::Relative2sComplement, pickup: false, ..m.clone() };
    let d = apply(&d, &set(relative.clone())).unwrap().doc;
    assert_eq!(d.settings().learn, vec![relative], "same source: replaced");
    let rm = Command::Settings(SettingsCmd::RemoveLearn { source: Some(s.clone()), target: None });
    assert_eq!(rm.name(), "Remove MIDI Learn");
    let d = apply(&d, &rm).unwrap().doc;
    assert!(d.settings().learn.is_empty());
    assert!(apply(&d, &rm).is_err(), "nothing to remove");
}

/// Pre-flight M14: `surface.unlearn target` removes every map to the
/// target, as one command.
#[test]
fn removing_by_target_takes_every_map_to_it_as_one_step() {
    let mut d = doc();
    let t = master_fader(&d);
    for cc in [7, 8] {
        d = apply(&d, &set(map(src("Knobs", LearnKind::Cc7 { channel: 0, cc }), t))).unwrap().doc;
    }
    let other = T::StripMute(d.mixer().master());
    let pad = src("Pads", LearnKind::Note { channel: 9, note: 36 });
    d = apply(&d, &set(map(pad.clone(), other))).unwrap().doc;
    let rm = Command::Settings(SettingsCmd::RemoveLearn { source: None, target: Some(t) });
    assert_eq!(rm.name(), "Remove MIDI Learn");
    let d = apply(&d, &rm).unwrap().doc;
    assert_eq!(d.settings().learn.len(), 1, "both maps to the fader went");
    assert_eq!(d.settings().learn[0].target, other);
    assert!(apply(&d, &rm).is_err(), "nothing maps to it any more");
    let both = Command::Settings(SettingsCmd::RemoveLearn { source: Some(pad), target: Some(other) });
    assert!(apply(&d, &both).is_err(), "exactly one of source and target");
    let neither = Command::Settings(SettingsCmd::RemoveLearn { source: None, target: None });
    assert!(apply(&d, &neither).is_err());
}

#[test]
fn a_learn_map_is_checked_by_the_model() {
    let d = doc();
    let t = master_fader(&d);
    for bad in [
        src("", LearnKind::Cc7 { channel: 0, cc: 7 }),
        src(&"x".repeat(256), LearnKind::Cc7 { channel: 0, cc: 7 }),
        src("a\u{7}b", LearnKind::Cc7 { channel: 0, cc: 7 }),
        src("K", LearnKind::Cc7 { channel: 16, cc: 7 }),
        src("K", LearnKind::Cc7 { channel: 0, cc: 128 }),
        src("K", LearnKind::Cc14 { channel: 0, cc: 32 }),
        src("K", LearnKind::Note { channel: 0, note: 200 }),
        src("K", LearnKind::PitchBend { channel: 99 }),
    ] {
        assert!(apply(&d, &set(map(bad.clone(), t))).is_err(), "{bad:?}");
    }
    let ok = src("K", LearnKind::Cc7 { channel: 0, cc: 1 });
    let gone = T::StripFader(core_model::ids::StripId(987_654));
    assert!(apply(&d, &set(map(ok.clone(), gone))).is_err(), "the target must exist");
    // A destination the document has, so only the velocity-scale rule can refuse it.
    let d = apply(
        &d,
        &Command::Routing(core_model::command::RoutingCmd::AddDestination {
            name: "Plug".into(),
            kind: core_model::document::DestKind::plugin("p"),
            default_patch: None,
        }),
    )
    .unwrap()
    .doc;
    let dest = *d.instruments().keys().next().unwrap();
    assert!(apply(&d, &set(map(ok, T::VelocityScale(dest)))).is_err(), "velocity scale cannot be learned");
}

#[test]
fn the_map_count_is_bounded() {
    let mut d = doc();
    let t = master_fader(&d);
    for i in 0..MAX_LEARN_MAPS {
        let s = src(&format!("P{}", i / 128), LearnKind::Cc7 { channel: 0, cc: (i % 128) as u8 });
        d = apply(&d, &set(map(s, t))).unwrap().doc;
    }
    let one_more = src("Extra", LearnKind::Note { channel: 0, note: 1 });
    assert!(apply(&d, &set(map(one_more, t))).is_err());
}

#[test]
fn learn_maps_round_trip_and_are_absent_when_empty() {
    let d = doc();
    let json = serde_json::to_string(d.settings()).unwrap();
    assert!(!json.contains("learn"), "an empty list is not written: {json}");
    let m = map(src("Pads", LearnKind::Note { channel: 9, note: 36 }), T::StripMute(d.mixer().master()));
    let d = apply(&d, &set(m.clone())).unwrap().doc;
    let json = serde_json::to_string(d.settings()).unwrap();
    let back: core_model::document::Settings = serde_json::from_str(&json).unwrap();
    assert_eq!(back.learn, vec![m.clone()]);
    assert_eq!(core_model::file::VERSION, 3);
    let bare = serde_json::to_string(&m).unwrap().replace(",\"pickup\":true", "");
    assert!(!bare.contains("pickup"), "{bare}");
    let old: LearnMap = serde_json::from_str(&bare).unwrap();
    assert!(old.pickup, "pick-up is on when the field is absent (Ruling 18)");
    assert!(!m.picks_up(), "a note map never picks up");
    assert!(map(src("K", LearnKind::Cc14 { channel: 0, cc: 1 }), T::StripMute(d.mixer().master())).picks_up());
}

#[test]
fn sources_describe_themselves() {
    assert_eq!(src("Knobs", LearnKind::Cc7 { channel: 0, cc: 7 }).describe(), "Knobs · CC 7 ch 1");
    assert_eq!(src("Knobs", LearnKind::Cc14 { channel: 1, cc: 1 }).describe(), "Knobs · CC 1/33 ch 2");
    assert_eq!(src("Keys", LearnKind::PitchBend { channel: 0 }).describe(), "Keys · Pitch Bend ch 1");
    assert_eq!(src("Pads", LearnKind::Note { channel: 9, note: 60 }).describe(), "Pads · Note C4 ch 10");
}
VIZDAW_EOF
```

Middle C is `C4` here, as `core_model::command::event::note_name(60)` and the editors' `PitchMath` name it.

- [ ] **Step 2: Run them to see them fail** (`cargo test -p core-model --test learn`). Expected: FAIL — `core_model::learn` not found.

- [ ] **Step 3: Implement.** In `crates/core-model/src/lib.rs`:

   Find:
   ```rust
   pub mod automation;
   ```
   Replace:
   ```rust
   pub mod automation;
   pub mod learn;
   ```

   In `crates/core-model/src/document.rs`:

   Find:
   ```rust
       pub format: AudioFormat,
   }
   ```
   Replace:
   ```rust
       pub format: AudioFormat,
       /// MIDI-learn maps whose targets are project objects (sub-project 5
       /// spec §4.8; plan 5f-1 Ruling 6). Absent in files when empty.
       #[serde(default, skip_serializing_if = "Vec::is_empty")]
       pub learn: Vec<crate::learn::LearnMap>,
   }
   ```

   Find:
   ```rust
               format: AudioFormat::DEFAULT,
   ```
   Replace:
   ```rust
               format: AudioFormat::DEFAULT,
               learn: Vec::new(),
   ```

   In `crates/core-model/src/command/settings.rs`, add the two variants after `SetFormat { … }` in the enum, then add their names:

   Find:
   ```rust
               SettingsCmd::SetFormat { .. } => "Change Project Format",
   ```
   Replace:
   ```rust
               SettingsCmd::SetFormat { .. } => "Change Project Format",
               SettingsCmd::SetLearn { .. } => "MIDI Learn",
               SettingsCmd::RemoveLearn { .. } => "Remove MIDI Learn",
   ```

   In `apply`, handle both before `let s = d.settings_mut();`, as `SetFormat` is (the target check needs `&Document`). Then:

   Find:
   ```rust
           // Handled above.
           SettingsCmd::SetFormat { .. } => {}
   ```
   Replace:
   ```rust
           // Handled above.
           SettingsCmd::SetFormat { .. }
           | SettingsCmd::SetLearn { .. }
           | SettingsCmd::RemoveLearn { .. } => {}
   ```

- [ ] **Step 4: Run the tests and the gates.** Run `cargo test -p core-model` (the insta snapshots must be unchanged), `cargo test -p core-ffi --lib`, and both clippies. Expected: core-model +6 and every existing snapshot unchanged.

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-model/src/learn.rs crates/core-model/src/lib.rs crates/core-model/src/document.rs crates/core-model/src/command/settings.rs crates/core-model/tests/learn.rs
git commit -m "core-model: MIDI-learn maps (with pick-up) in the project settings, set and removed as undo steps (plan 5f-1 Task 4)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -8
```
Counts: Rust +6.

---

### Task 5: `core-surface` learn: detection, learned maps with pick-up, command maps, what maps take

**Files:**
- Modify: `crates/core-surface/src/learn.rs`
- Test: create `crates/core-surface/tests/learn.rs`

**Interfaces:**
- Consumes: `core_model::learn::*` (Task 4), `IdleTouches` (Task 1).
- Produces:
  ```rust
  pub const LEARN_WINDOW: Duration = Duration::from_millis(300);
  /// How near the target's value a picking-up control must come (Ruling 18).
  pub const PICKUP_TOLERANCE: f64 = 0.02;
  /// A learned command (app preferences, Ruling 9).
  #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
  pub struct CommandMap { pub source: LearnSource, pub command: String }
  #[derive(Debug, Default)] pub struct Learner { /* first port, first time, messages */ }
  impl Learner {
      /// One message from `port` (channel messages only; others ignored).
      pub fn feed(&mut self, port: &str, msg: &[u8], now: Instant);
      /// The decision once LEARN_WINDOW passed since the first message or 8 arrived (Ruling 7).
      pub fn decide(&mut self, now: Instant) -> Option<(LearnSource, LearnMode)>;
  }
  #[derive(Debug, Default)] pub struct Mapper { /* maps, commands, idle, msb holds and last lsb, last control value, engaged, last sent, cc ≥64 state */ }
  impl Mapper {
      pub fn set(&mut self, maps: Vec<LearnMap>, commands: Vec<CommandMap>);
      pub fn listens(&self, port: &str) -> bool;
      /// Whether a map or command map takes `msg` from `port` (Ruling 17).
      pub fn takes(&self, port: &str, msg: &[u8]) -> bool;
      /// Rulings 8 and 18; `current` gives a target's heard value now (None: unknown → the message is dropped for relative and picking-up maps).
      pub fn input(&mut self, port: &str, msg: &[u8], now: Instant, current: &dyn Fn(AutomationTarget) -> Option<f64>) -> Vec<SurfaceAction>;
      /// Held MSBs (Ruling 8), then idle releases (`IdleRelease`), which disengage picking-up maps.
      pub fn tick(&mut self, now: Instant) -> Vec<SurfaceAction>;
      /// `(output port, message)` for absolute maps whose value changed.
      pub fn feedback(&mut self, current: &dyn Fn(AutomationTarget) -> Option<f64>) -> Vec<(String, Vec<u8>)>;
      pub fn held(&self) -> Vec<AutomationTarget>;
  }
  ```

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-surface/tests/learn.rs <<'VIZDAW_EOF'
//! MIDI learn (plan 5f-1 Task 5; spec §8.2, §12.3; Rulings 7, 8, 17, 18).

use core_model::automation::AutomationTarget as T;
use core_model::ids::StripId;
use core_model::learn::{LearnKind as K, LearnMap, LearnMode as M, LearnSource};
use core_surface::learn::{CommandMap, LEARN_WINDOW, Learner, Mapper};
use core_surface::{IDLE_RELEASE, Phase, SurfaceAction as A};
use std::time::{Duration, Instant};

fn learn(msgs: &[&[u8]]) -> Option<(LearnSource, M)> {
    let now = Instant::now();
    let mut l = Learner::default();
    for m in msgs {
        l.feed("Knobs", m, now);
    }
    assert!(l.decide(now).is_none() || msgs.len() >= 8, "waits for the window");
    l.decide(now + LEARN_WINDOW)
}

fn src(kind: K) -> LearnSource {
    LearnSource { port: "Knobs".into(), kind }
}

#[test]
fn the_next_cc_bend_or_note_is_learnt() {
    assert_eq!(learn(&[&[0xB0, 7, 100]]), Some((src(K::Cc7 { channel: 0, cc: 7 }), M::Absolute)));
    assert_eq!(learn(&[&[0xE2, 0, 64]]), Some((src(K::PitchBend { channel: 2 }), M::Absolute)));
    assert_eq!(learn(&[&[0x99, 36, 90]]), Some((src(K::Note { channel: 9, note: 36 }), M::Absolute)));
    assert_eq!(learn(&[&[0x89, 36, 0]]), None, "a note-off alone teaches nothing");
}

#[test]
fn fourteen_bit_pairs_are_detected() {
    assert_eq!(
        learn(&[&[0xB0, 1, 64], &[0xB0, 33, 5], &[0xB0, 1, 65], &[0xB0, 33, 0]]),
        Some((src(K::Cc14 { channel: 0, cc: 1 }), M::Absolute))
    );
    assert_eq!(
        learn(&[&[0xB0, 40, 64], &[0xB0, 72, 5]]),
        Some((src(K::Cc7 { channel: 0, cc: 40 }), M::Absolute)),
        "no pair above CC 31"
    );
}

#[test]
fn encoder_styles_are_detected() {
    assert_eq!(
        learn(&[&[0xB0, 20, 1], &[0xB0, 20, 1], &[0xB0, 20, 127]]),
        Some((src(K::Cc7 { channel: 0, cc: 20 }), M::Relative2sComplement))
    );
    assert_eq!(
        learn(&[&[0xB0, 20, 65], &[0xB0, 20, 63], &[0xB0, 20, 66]]),
        Some((src(K::Cc7 { channel: 0, cc: 20 }), M::RelativeBinOffset))
    );
    assert_eq!(
        learn(&[&[0xB0, 20, 1]]),
        Some((src(K::Cc7 { channel: 0, cc: 20 }), M::Absolute)),
        "one message is absolute"
    );
    let now = Instant::now();
    let mut l = Learner::default();
    for _ in 0..8 {
        l.feed("Knobs", &[0xB0, 20, 65], now);
    }
    assert!(l.decide(now).is_some(), "eight messages decide at once");
}

/// A mapper with one map, pick-up off (the jump), as the tests of the
/// values need; `pick_up_waits_until_the_control_reaches_the_value` turns
/// it on.
fn mapper(mode: M, kind: K, target: T) -> Mapper {
    let mut m = Mapper::default();
    m.set(vec![LearnMap { source: src(kind), target, mode, pickup: false }], vec![]);
    m
}

#[test]
fn absolute_maps_touch_move_and_release_after_500_ms() {
    let f = T::StripFader(StripId(3));
    let mut m = mapper(M::Absolute, K::Cc7 { channel: 0, cc: 7 }, f);
    let now = Instant::now();
    let cur = |_: T| Some(0.0);
    assert_eq!(m.input("Knobs", &[0xB0, 7, 127], now, &cur), vec![A::Gesture(f, Phase::Touch(Some(1.0)))]);
    assert_eq!(m.input("Knobs", &[0xB0, 7, 0], now, &cur), vec![A::Gesture(f, Phase::Value(0.0))]);
    assert!(m.input("Other", &[0xB0, 7, 0], now, &cur).is_empty());
    assert!(m.input("Knobs", &[0xB1, 7, 0], now, &cur).is_empty(), "another channel");
    assert!(m.tick(now + Duration::from_millis(100)).is_empty());
    assert_eq!(m.tick(now + IDLE_RELEASE), vec![A::IdleRelease(f, now)], "stamped at its last value");
}

#[test]
fn fourteen_bit_and_pitch_bend_values() {
    let f = T::StripFader(StripId(1));
    let mut m = mapper(M::Absolute, K::Cc14 { channel: 0, cc: 1 }, f);
    let now = Instant::now();
    let cur = |_: T| Some(0.0);
    let a = m.input("Knobs", &[0xB0, 1, 0x40, 0xB0, 33, 0x00], now, &cur);
    match a.as_slice() {
        [A::Gesture(_, Phase::Touch(Some(x)))] => assert!((x - 8192.0 / 16383.0).abs() < 1e-9),
        other => panic!("{other:?}"),
    }
    let mut m = mapper(M::Absolute, K::PitchBend { channel: 0 }, f);
    match m.input("Knobs", &[0xE0, 0x7F, 0x7F], now, &cur).as_slice() {
        [A::Gesture(_, Phase::Touch(Some(x)))] => assert_eq!(*x, 1.0),
        other => panic!("{other:?}"),
    }
}

/// Pre-flight I6: messages arrive one per call, so the MSB must wait for
/// its LSB, or every move writes a coarse value before the fine one.
#[test]
fn an_msb_and_its_lsb_in_separate_calls_make_one_value() {
    let f = T::StripFader(StripId(1));
    let mut m = mapper(M::Absolute, K::Cc14 { channel: 0, cc: 1 }, f);
    let now = Instant::now();
    let cur = |_: T| Some(0.0);
    assert!(m.input("Knobs", &[0xB0, 1, 0x40], now, &cur).is_empty(), "the MSB waits for its LSB");
    match m.input("Knobs", &[0xB0, 33, 0x00], now, &cur).as_slice() {
        [A::Gesture(_, Phase::Touch(Some(x)))] => assert!((x - 8192.0 / 16383.0).abs() < 1e-9),
        other => panic!("{other:?}"),
    }
    assert!(m.tick(now).is_empty(), "nothing is held over");
}

#[test]
fn a_lone_msb_applies_at_tick() {
    let f = T::StripFader(StripId(1));
    let mut m = mapper(M::Absolute, K::Cc14 { channel: 0, cc: 1 }, f);
    let now = Instant::now();
    let cur = |_: T| Some(0.0);
    assert!(m.input("Knobs", &[0xB0, 1, 127], now, &cur).is_empty());
    match m.tick(now).as_slice() {
        [A::Gesture(_, Phase::Touch(Some(x)))] => {
            assert!((x - 127.0 * 128.0 / 16383.0).abs() < 1e-9, "msb << 7 over 16383, not msb / 127")
        }
        other => panic!("{other:?}"),
    }
    let _ = m.input("Knobs", &[0xB0, 1, 0x40], now, &cur);
    match m.input("Knobs", &[0xB0, 33, 5], now, &cur).as_slice() {
        [A::Gesture(_, Phase::Value(x))] => assert!((x - f64::from(0x40 << 7 | 5) / 16383.0).abs() < 1e-9),
        other => panic!("{other:?}"),
    }
    assert!(m.input("Knobs", &[0xB0, 1, 0x41], now, &cur).is_empty());
    match m.tick(now).as_slice() {
        [A::Gesture(_, Phase::Value(x))] => {
            assert!((x - f64::from(0x41 << 7 | 5) / 16383.0).abs() < 1e-9, "a lone MSB keeps the last LSB")
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn relative_maps_step_from_the_current_value() {
    let p = T::StripPan(StripId(2));
    let now = Instant::now();
    let mut m = mapper(M::Relative2sComplement, K::Cc7 { channel: 0, cc: 20 }, p);
    match m.input("Knobs", &[0xB0, 20, 127], now, &|_| Some(0.5)).as_slice() {
        [A::Gesture(_, Phase::Touch(Some(x)))] => assert!((x - (0.5 - 1.0 / 127.0)).abs() < 1e-9),
        other => panic!("{other:?}"),
    }
    let mut m = mapper(M::RelativeBinOffset, K::Cc7 { channel: 0, cc: 20 }, p);
    match m.input("Knobs", &[0xB0, 20, 66], now, &|_| Some(0.5)).as_slice() {
        [A::Gesture(_, Phase::Touch(Some(x)))] => assert!((x - (0.5 + 2.0 / 127.0)).abs() < 1e-9),
        other => panic!("{other:?}"),
    }
    assert!(m.input("Knobs", &[0xB0, 20, 64], now, &|_| Some(0.5)).is_empty(), "64 is no move");
}

#[test]
fn notes_toggle_switches_and_run_commands() {
    let mute = T::StripMute(StripId(4));
    let mut m = mapper(M::Absolute, K::Note { channel: 9, note: 36 }, mute);
    let now = Instant::now();
    assert_eq!(
        m.input("Knobs", &[0x99, 36, 100], now, &|_| Some(0.0)),
        vec![
            A::Gesture(mute, Phase::Touch(Some(1.0))),
            A::Gesture(mute, Phase::Value(1.0)),
            A::Gesture(mute, Phase::Release),
        ]
    );
    assert!(m.input("Knobs", &[0x89, 36, 0], now, &|_| Some(1.0)).is_empty());
    let mut c = Mapper::default();
    c.set(vec![], vec![CommandMap { source: src(K::Cc7 { channel: 0, cc: 64 }), command: "transport.play_stop".into() }]);
    assert!(c.listens("Knobs") && !c.listens("Other"));
    assert_eq!(c.input("Knobs", &[0xB0, 64, 127], now, &|_| None), vec![A::Command("transport.play_stop".into())]);
    assert!(c.input("Knobs", &[0xB0, 64, 120], now, &|_| None).is_empty(), "once per rise through 64");
    let _ = c.input("Knobs", &[0xB0, 64, 0], now, &|_| None);
    assert_eq!(c.input("Knobs", &[0xB0, 64, 127], now, &|_| None).len(), 1);
}

#[test]
fn absolute_maps_send_their_value_back() {
    let f = T::StripFader(StripId(3));
    let mut m = mapper(M::Absolute, K::Cc7 { channel: 2, cc: 7 }, f);
    assert_eq!(m.feedback(&|_| Some(1.0)), vec![("Knobs".to_string(), vec![0xB2, 7, 127])]);
    assert!(m.feedback(&|_| Some(1.0)).is_empty(), "unchanged");
    let mut r = mapper(M::RelativeBinOffset, K::Cc7 { channel: 0, cc: 20 }, f);
    assert!(r.feedback(&|_| Some(1.0)).is_empty(), "relative maps send nothing");
    let _ = m.input("Knobs", &[0xB2, 7, 10], Instant::now(), &|_| Some(1.0));
    assert!(m.feedback(&|_| Some(0.5)).is_empty(), "nothing while learn holds it");
}

/// Owner answer 2026-11-02 (Ruling 18): an absolute control takes over
/// only once it passes the target's value, and again after each release.
#[test]
fn pick_up_waits_until_the_control_reaches_the_value() {
    let f = T::StripFader(StripId(3));
    let mut m = Mapper::default();
    m.set(
        vec![LearnMap { source: src(K::Cc7 { channel: 0, cc: 7 }), target: f, mode: M::Absolute, pickup: true }],
        vec![],
    );
    let now = Instant::now();
    let half = |_: T| Some(0.5);
    assert!(m.input("Knobs", &[0xB0, 7, 10], now, &half).is_empty(), "far below the value: no jump");
    assert!(m.input("Knobs", &[0xB0, 7, 40], now, &half).is_empty(), "still below it");
    match m.input("Knobs", &[0xB0, 7, 70], now, &half).as_slice() {
        [A::Gesture(t, Phase::Touch(Some(x)))] if *t == f => {
            assert!((x - 70.0 / 127.0).abs() < 1e-9, "it crossed 0.5: it takes over")
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(m.input("Knobs", &[0xB0, 7, 0], now, &half), vec![A::Gesture(f, Phase::Value(0.0))], "engaged: it follows anywhere");
    assert_eq!(m.tick(now + IDLE_RELEASE), vec![A::IdleRelease(f, now)]);
    let later = now + IDLE_RELEASE * 2;
    let moved = |_: T| Some(0.8);
    assert!(m.input("Knobs", &[0xB0, 7, 6], later, &moved).is_empty(), "released: automation moved the target away");
    match m.input("Knobs", &[0xB0, 7, 101], later, &moved).as_slice() {
        [A::Gesture(_, Phase::Touch(Some(x)))] => {
            assert!((x - 101.0 / 127.0).abs() < 1e-9, "within 0.02 of 0.8: it meets the value")
        }
        other => panic!("{other:?}"),
    }
    let mut jump = mapper(M::Absolute, K::Cc7 { channel: 0, cc: 7 }, f);
    assert_eq!(jump.input("Knobs", &[0xB0, 7, 0], now, &half), vec![A::Gesture(f, Phase::Touch(Some(0.0)))], "pick-up off: the jump");
}

/// Owner answer 2026-11-02 (Ruling 17): what a map takes stays out of
/// the lanes; everything else from the same port passes.
#[test]
fn the_maps_say_which_messages_they_take() {
    let map = |kind: K, target: T| LearnMap { source: src(kind), target, mode: M::Absolute, pickup: true };
    let mut m = Mapper::default();
    m.set(
        vec![
            map(K::Cc7 { channel: 0, cc: 7 }, T::StripFader(StripId(1))),
            map(K::Cc14 { channel: 1, cc: 2 }, T::StripPan(StripId(1))),
            map(K::PitchBend { channel: 2 }, T::StripFader(StripId(2))),
            map(K::Note { channel: 9, note: 36 }, T::StripMute(StripId(1))),
        ],
        vec![CommandMap { source: src(K::Note { channel: 9, note: 37 }), command: "edit.undo".into() }],
    );
    for msg in [&[0xB0, 7, 1][..], &[0xB1, 2, 1], &[0xB1, 34, 1], &[0xE2, 0, 64], &[0x99, 36, 100], &[0x89, 36, 0], &[0x99, 37, 1]] {
        assert!(m.takes("Knobs", msg), "{msg:02x?}");
    }
    for msg in [&[0xB0, 8, 1][..], &[0xB1, 7, 1], &[0xE0, 0, 64], &[0x99, 38, 100], &[0x90, 36, 100], &[0xA9, 36, 5]] {
        assert!(!m.takes("Knobs", msg), "unlearned: {msg:02x?}");
    }
    assert!(!m.takes("Other", &[0xB0, 7, 1]), "another port");
}
VIZDAW_EOF
```

- [ ] **Step 2: Run them to see them fail** (`cargo test -p core-surface --test learn`). Expected: FAIL — `Learner` not found.

- [ ] **Step 3: Implement `learn.rs`** by Rulings 7, 8, 17 and 18. Pick-up keeps, per map, the last control value it saw (engaged or not) and whether it is engaged; `tick` disengages a map when it emits its `IdleRelease`.

- [ ] **Step 4: Run the tests and the gates.** Expected: 37 passed in `core-surface`.

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-surface/src/learn.rs crates/core-surface/tests/learn.rs
git commit -m "core-surface: MIDI learn detection (14-bit pairs, encoder styles), learned maps with pick-up, 500 ms release and feedback, command maps, what maps take (plan 5f-1 Task 5)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -6
```
Counts: Rust +12.

---

### Task 6: Gesture holders (carry m8): `Session.gesture_from`, the static edit at the first value

**Files:**
- Modify: `crates/core-ffi/src/automation.rs` (`SurfaceId`, `TouchHolder`, `TouchHolders`, `GestureHolderRow`, `Touching.holders`, `Touching.shared`), `crates/core-ffi/src/session.rs` (`gesture`, `gesture_from`, `gesture_held`, `touch`, `move_to`, `release_from`, `share_static`), `app/Sources/VizDAWAppModel/AutomationCommands.swift` (the channel's holder; its doc comment), `app/Tests/VizDAWAppModelTests/FakeSession.swift`, `app/Tests/VizDAWAppModelTests/AutomationCommandsTests.swift`
- Test: create `crates/core-ffi/tests/holders.rs`

**Interfaces:**
- Consumes: 5e-1's `Touching`, `Source`, `touch`, `move_to`, `release`, `run_static`, `end_static_share`, `keep_static_param`.
- Produces:
  ```rust
  // automation.rs
  /// A surface's ID, below 16 by construction (Rulings 2, 4).
  #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
  pub(crate) struct SurfaceId(u8);
  impl SurfaceId { pub const COUNT: u8 = 16; pub fn new(id: u32) -> Option<SurfaceId>; pub fn get(self) -> u32; }
  /// Who holds a touch (Ruling 2). Not `Holder`: `crate::automation::Holder`
  /// (`This`, `Other`, `Nobody`, automation.rs:517) is the engine's, and stays.
  #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
  pub(crate) enum TouchHolder { Mouse, Channel, Plugin, Learn, Surface(SurfaceId) }
  /// A set of holders: bits 0–3 Mouse, Channel, Plugin, Learn; bits 4–19 surfaces 0–15.
  #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
  pub(crate) struct TouchHolders(u32);
  impl TouchHolders { pub fn of(h: TouchHolder) -> TouchHolders; pub fn has(self, h: TouchHolder) -> bool; pub fn with(self, h: TouchHolder) -> TouchHolders; pub fn without(self, h: TouchHolder) -> TouchHolders; pub fn is_empty(self) -> bool; }
  /// The app's holders (the channel is its own, Ruling 2).
  #[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
  pub enum GestureHolderRow { Mouse, Channel }
  ```
  - `Touching` gains `holders: TouchHolders` and `shared: bool` (whether this touch holds a share of the static edit).
  - `Session::touch(…, holder: TouchHolder)` and `Session::move_to(…, holder: TouchHolder)` take the holder (every call site passes one: the export passes its row's, and the plugin paths pass `TouchHolder::Plugin`).
  - `fn share_static(&self, inner: &mut Inner, t: AutomationTarget) -> Result<(), VizError>`: on a static touch not yet `shared`, opens the shared static edit (or joins it: `n += 1`) and sets `shared`; Busy ("Finish the open edit first.") when a non-static transaction is open; nothing on a touch already sharing.
  - `fn release_from(&self, inner: &mut Inner, t: AutomationTarget, h: TouchHolder, host_ns: Option<u64>)` removes `h`. When the set becomes empty it calls `release`; otherwise it flushes the sampler and sends the flushed value through `send_value`, and sends no Glide or Hold.
  - `release` itself still ends the touch whoever holds it; `release_touches` and `release_vanished` keep calling it. It ends a static share (`end_static_share`, `keep_static_param`) only when the touch is `shared`.
  - Exported: `Session::gesture_from(&self, target: AutomationTargetRow, phase: GesturePhaseRow, holder: GestureHolderRow) -> Result<(), VizError>`. `Session::gesture` is `self.gesture_from(target, phase, GestureHolderRow::Mouse)`.
  - For later tasks: `pub(crate) fn gesture_held(&self, inner: &mut Inner, t: AutomationTarget, phase: core_surface::Phase, h: TouchHolder, host_ns: Option<u64>) -> Result<(), VizError>`, the same path for surfaces and learn, with `Source::Ui` (Ruling 2) and the message's host time (an idle release passes its last value's time, converted as `release_plugin_touches` converts it: 0 in offline mode).

Joining, the static edit and the plugin rules:
- In `touch`, the existing "Touched already" branch adds `holder` to `holders` before it moves the target.
- **The static edit opens at the first value (pre-flight C2).** The block in `touch` that opens the shared static edit for every static touch becomes a call to `share_static` that runs only when the touch carries a value (`Touch(Some(v))`), still before the touch is inserted, so a Busy refuses that touch as today. `move_to` calls `share_static` for a static touch that is not yet `shared` before the value reaches the sampler; on Busy the value is not applied and `move_to` returns the Busy (the mouse and the channel see it; Task 8's surfaces and learn count it in `ignored`). `Touch(None)` opens nothing.
- The plugin paths keep their 500 ms release through `plugin_touches`, which calls `release_from(…, TouchHolder::Plugin, …)`, and keep `Source::Plugin`.
- The plugin check at session.rs:6119 (`x.source == Source::Plugin`) stays.
- **A resting touch held over Play** (session.rs, the pass-start loop "Touches held over the pass's start"): the conversion keys on the touch's own path and share, not on whether some static edit is open (Ruling 2; Step 3 gives the Find). A resting touch on a strip that does not write stays Static; on a writing strip it converts, and `shared` is cleared when it does.

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-ffi/tests/holders.rs <<'VIZDAW_EOF'
//! One target, several hands (plan 5f-1 Task 6; Ruling 2; 5e-2 carry m8):
//! each holder releases its own touch, stopped and while writing; a
//! resting touch opens no edit. Offline only.

mod common;

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use vizdaw_ffi::automation::{AutomationModeRow as Mode, GestureHolderRow as H, GesturePhaseRow as Phase};
use vizdaw_ffi::host::{force_offline, reset_host};
use vizdaw_ffi::{AutomationTargetRow as Target, Session, VizError};

const Q: i64 = 15_360;

static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    force_offline();
    reset_host();
    g
}

fn wait_until(what: &str, mut ok: impl FnMut() -> bool) {
    let until = Instant::now() + common::wait_limit();
    while !ok() {
        assert!(Instant::now() < until, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Paces gesture samples past the 10 ms sampler.
fn pace(s: &Session) {
    std::thread::sleep(Duration::from_millis(12));
    s.idle().unwrap();
}

fn demo() -> (Arc<Session>, u64) {
    let s = Session::new_demo().unwrap();
    let synth = s
        .mixer()
        .unwrap()
        .strips
        .iter()
        .find(|x| x.name == "Built-in Synth")
        .unwrap()
        .id;
    (s, synth)
}

fn song(s: &Session) -> u64 {
    s.snapshot().unwrap().sequences.iter().find(|r| r.name == "Song").unwrap().id
}

fn gain(s: &Session, strip: u64) -> f64 {
    s.mixer().unwrap().strips.iter().find(|x| x.id == strip).unwrap().gain_db
}

#[test]
fn a_surface_and_the_mouse_hold_one_target_until_both_let_go() {
    let _g = serial();
    let (s, synth) = demo();
    let t = Target::StripFader { strip: synth };
    let undo_before = s.snapshot().unwrap().undo_label;
    s.gesture(t, Phase::Touch { value: Some(0.5) }).unwrap();
    s.gesture_from(t, Phase::Touch { value: None }, H::Channel).unwrap();
    s.gesture(t, Phase::Release).unwrap();
    assert!(s.automation_state().unwrap().touched.contains(&t), "the channel still holds it");
    assert_eq!(s.snapshot().unwrap().undo_label, undo_before, "no step yet");
    s.gesture_from(t, Phase::Value { value: 0.25 }, H::Channel).unwrap();
    s.gesture_from(t, Phase::Release, H::Channel).unwrap();
    assert!(s.automation_state().unwrap().touched.is_empty());
    assert_eq!(s.snapshot().unwrap().undo_label.as_deref(), Some("Set Fader"));
    let db = gain(&s, synth);
    s.undo().unwrap();
    assert!((gain(&s, synth) - db).abs() > 0.1, "one step undoes both hands' moves");
}

#[test]
fn each_holder_releases_only_its_own_touch() {
    let _g = serial();
    let (s, synth) = demo();
    let t = Target::StripPan { strip: synth };
    s.gesture_from(t, Phase::Touch { value: Some(0.3) }, H::Channel).unwrap();
    s.gesture(t, Phase::Release).unwrap();
    assert!(s.automation_state().unwrap().touched.contains(&t), "the mouse never held it");
    s.gesture_from(t, Phase::Release, H::Channel).unwrap();
    assert!(s.automation_state().unwrap().touched.is_empty());
    s.gesture_from(t, Phase::Release, H::Channel).unwrap();
}

#[test]
fn a_holder_touching_twice_holds_once() {
    let _g = serial();
    let (s, synth) = demo();
    let t = Target::StripFader { strip: synth };
    s.gesture(t, Phase::Touch { value: Some(0.5) }).unwrap();
    s.gesture(t, Phase::Touch { value: Some(0.6) }).unwrap();
    s.gesture(t, Phase::Release).unwrap();
    assert!(s.automation_state().unwrap().touched.is_empty(), "one release ends it");
}

/// Pre-flight C2: a touch with no value holds no edit open; its first
/// value opens its own step.
#[test]
fn a_resting_touch_does_not_open_an_edit() {
    let _g = serial();
    let (s, synth) = demo();
    let t = Target::StripFader { strip: synth };
    let before = gain(&s, synth);
    s.gesture_from(t, Phase::Touch { value: None }, H::Channel).unwrap();
    s.begin_edit("Drag".into()).unwrap();
    assert!(
        matches!(s.gesture_from(t, Phase::Value { value: 0.4 }, H::Channel), Err(VizError::Busy { .. })),
        "a first value waits for the open drag"
    );
    s.end_edit().unwrap();
    s.add_bus("Rest".into()).unwrap();
    assert_eq!(s.snapshot().unwrap().undo_label.as_deref(), Some("Add Bus"), "its own step while the touch rests");
    s.gesture_from(t, Phase::Value { value: 0.25 }, H::Channel).unwrap();
    s.gesture_from(t, Phase::Release, H::Channel).unwrap();
    assert_eq!(s.snapshot().unwrap().undo_label.as_deref(), Some("Set Fader"));
    s.undo().unwrap();
    assert!((gain(&s, synth) - before).abs() < 1e-9, "Undo restores only the fader");
    assert!(s.mixer().unwrap().strips.iter().any(|x| x.name == "Rest"), "and keeps the bus");
    s.gesture_from(t, Phase::Touch { value: None }, H::Channel).unwrap();
    s.gesture_from(t, Phase::Release, H::Channel).unwrap();
    assert_eq!(s.snapshot().unwrap().undo_label.as_deref(), Some("Add Bus"), "a touch with no value commits nothing");
}

/// Pre-flight I12: Ruling 2 while writing. The first hand's release
/// neither glides nor holds; the write follows the second hand to its
/// release.
#[test]
fn two_holders_on_a_writing_strip_end_only_at_the_last_release() {
    let _g = serial();
    let (s, synth) = demo();
    let seq = song(&s);
    let t = Target::StripFader { strip: synth };
    s.set_automation_mode(vec![synth], Mode::Touch).unwrap();
    s.play(seq).unwrap();
    wait_until("beat 1", || s.transport().unwrap().tick > Q);
    let t0 = s.transport().unwrap().tick;
    s.gesture(t, Phase::Touch { value: Some(0.5) }).unwrap();
    pace(&s);
    s.gesture(t, Phase::Value { value: 0.45 }).unwrap();
    s.gesture_from(t, Phase::Touch { value: None }, H::Channel).unwrap();
    pace(&s);
    s.gesture(t, Phase::Release).unwrap();
    assert!(s.automation_state().unwrap().touched.contains(&t), "the channel still holds it");
    for v in [0.4, 0.35, 0.3] {
        pace(&s);
        s.gesture_from(t, Phase::Value { value: v }, H::Channel).unwrap();
    }
    pace(&s);
    let t1 = s.transport().unwrap().tick;
    s.gesture_from(t, Phase::Release, H::Channel).unwrap();
    wait_until("the glide to pass", || s.transport().unwrap().tick > t1 + Q);
    s.stop().unwrap();
    wait_until("the stop", || !s.transport().unwrap().playing);
    s.idle().unwrap();
    assert_eq!(s.snapshot().unwrap().undo_label.as_deref(), Some("Write Automation"));
    let lane = s.lanes(seq).unwrap().into_iter().find(|l| l.name == "Built-in Synth Fader").expect("a new lane").id;
    let mut p: Vec<(i64, f64)> = s
        .lane_events(seq, lane)
        .unwrap()
        .events
        .iter()
        .filter_map(|e| e.automation.map(|a| (e.tick, a.value)))
        .collect();
    p.sort_by(|a, b| a.partial_cmp(b).unwrap());
    for v in [0.5, 0.3] {
        assert!(p.iter().any(|x| (x.1 - v).abs() < 1e-9), "{v} from both hands: {p:?}");
    }
    assert!(
        p.iter().any(|x| x.0 > t0 && x.0 < t1 && x.1 < 0.5 - 1e-9 && x.1 > 0.4 - 1e-9),
        "the mouse's second value is in the ride (thinning may move it): {p:?}"
    );
    let ride: Vec<f64> = p.iter().filter(|x| x.0 > t0 && x.0 < t1).map(|x| x.1).collect();
    assert!(
        ride.windows(2).all(|w| w[1] <= w[0] + 1e-9),
        "falling all the way: no glide back toward the static value at the first release: {p:?}"
    );
}

/// Pre-flight C2 held over Play (fold check C1): a hand resting on a Read
/// strip's fader when Play starts keeps the static path; its move is a
/// static edit.
#[test]
fn a_resting_touch_held_over_play_keeps_its_path() {
    let _g = serial();
    let (s, synth) = demo();
    let seq = song(&s);
    let t = Target::StripFader { strip: synth };
    let before = gain(&s, synth);
    s.gesture_from(t, Phase::Touch { value: None }, H::Channel).unwrap();
    s.play(seq).unwrap();
    wait_until("beat 1", || s.transport().unwrap().tick > Q);
    s.gesture_from(t, Phase::Value { value: 0.25 }, H::Channel).unwrap();
    s.gesture_from(t, Phase::Release, H::Channel).unwrap();
    s.stop().unwrap();
    wait_until("the stop", || !s.transport().unwrap().playing);
    s.idle().unwrap();
    assert_eq!(s.snapshot().unwrap().undo_label.as_deref(), Some("Set Fader"), "a Read strip's move is a static edit");
    assert!((gain(&s, synth) - before).abs() > 0.1, "and it stays after the release");
}
VIZDAW_EOF
```

The ride falls from 0.5 to 0.3 while the synth's static fader sits near unity (well above 0.5), so a glide or a hold at the mouse's release would show as a rise or a plateau inside the ride.

- [ ] **Step 2: Run them to see them fail** (`cargo test -p core-ffi --test holders`). Expected: FAIL — `GestureHolderRow` and `gesture_from` not found.

- [ ] **Step 3: Implement.** In `crates/core-ffi/src/session.rs`, replace the body of `pub fn gesture(` (from `ffi_guard(|| {` through its closing `})`) with `self.gesture_from(target, phase, crate::automation::GestureHolderRow::Mouse)`. Then add `gesture_from` after it, holding the old body with `holder` threaded through:

   Find:
   ```rust
               if phase == GesturePhaseRow::Release {
                   self.release(&mut inner, t, None);
                   return Ok(());
               }
   ```
   Replace (inside the new `gesture_from`):
   ```rust
               let h = TouchHolder::from(holder);
               if phase == GesturePhaseRow::Release {
                   self.release_from(&mut inner, t, h, None);
                   return Ok(());
               }
   ```

   Find:
   ```rust
               // Touched already: a touch at a value moves it.
   ```
   Replace:
   ```rust
               // Touched already: the holder joins, and a touch at a value
               // moves it (plan 5f-1 Ruling 2).
               if let Some(x) = inner.touches.get_mut(&t) {
                   x.holders = x.holders.with(holder);
               }
   ```

   The static edit at the first value (in `touch`):

   Find:
   ```rust
           if path == TouchPath::Static {
               match inner.static_edit.as_mut() {
                   Some((_, n)) => *n += 1,
                   None => {
                       if inner.history.in_transaction() {
                           return Err(VizError::Busy {
                               message: "Finish the open edit first.".into(),
                           });
                       }
                       let label = crate::automation::static_label(t);
                       inner.history.begin(label)?;
                       inner.static_edit = Some((label.to_string(), 1));
                   }
               }
           }
   ```
   Replace:
   ```rust
           // The static edit opens at the first value, never at the touch
           // (plan 5f-1 Ruling 2, pre-flight C2): a hand resting on a
           // touch-sensing fader holds no transaction open.
           let shared = path == TouchPath::Static && value.is_some();
           if shared {
               self.open_static_share(inner, t)?;
           }
   ```
   where `open_static_share` holds the removed block's body (join with `n += 1`, or Busy, or `begin` with `static_label(t)`), `share_static` calls it for a touch that is not yet `shared` and then sets the flag, and the `Touching { … }` literal at `inner.touches.insert(` gains `holders: TouchHolders::of(holder),` and `shared,`. `move_to` calls `share_static(inner, t)?` when the touch's path is `Static` and it is not `shared`, before `inner.sampler.value(…)`. `release` calls `keep_static_param` and `end_static_share` only when `touching.shared`.

   The pass start (Ruling 2; fold check C1):

   Find:
   ```rust
               if x.path == TouchPath::Static && inner.static_edit.is_some() {
                   if self.write_mode_of(inner, t).is_none() {
                       continue;
                   }
                   self.keep_static_param(inner, t, x);
                   left_static += 1;
                   converted = true;
               }
   ```
   Replace:
   ```rust
               // By the touch's own path and share (plan 5f-1 Ruling 2): a
               // resting static touch holds no share, yet stays static on a
               // strip that does not write and converts on one that does.
               if x.path == TouchPath::Static {
                   if self.write_mode_of(inner, t).is_none() {
                       continue;
                   }
                   if x.shared {
                       self.keep_static_param(inner, t, x);
                       left_static += 1;
                   }
                   converted = true;
               }
   ```

   Find:
   ```rust
               if let Some(y) = inner.touches.get_mut(&t) {
                   y.path = path;
                   if !is_offset || converted {
                       y.base = y.value;
                   }
               }
   ```
   Replace:
   ```rust
               if let Some(y) = inner.touches.get_mut(&t) {
                   y.path = path;
                   if !is_offset || converted {
                       y.base = y.value;
                   }
                   // A converted touch leaves the static edit.
                   if converted {
                       y.shared = false;
                   }
               }
   ```

   In the plugin paths, replace `self.release(inner, t, Some(ns))` and `self.release(inner, t, ns)` (session.rs:6061 and :6121) with `self.release_from(inner, t, TouchHolder::Plugin, …)`.

   In `app/Sources/VizDAWAppModel/AutomationCommands.swift`:

   Find:
   ```swift
               try m.session.gesture(target: target, phase: phase)
   ```
   Replace:
   ```swift
               // The channel is its own holder (plan 5f-1 Ruling 2).
               try m.session.gestureFrom(target: target, phase: phase, holder: .channel)
   ```

   Find:
   ```swift
   /// own connection): a touch lasts until a release of the same target from
   /// any client, or until its window closes (Ruling 20). A release is never
   ```
   Replace:
   ```swift
   /// own connection): a touch the channel holds lasts until a channel release
   /// of the same target (from any client), or until its window closes (Ruling
   /// 20); the mouse releases only its own hold (plan 5f-1 Ruling 2). A release is never
   ```

   In `FakeSession.swift`, add `var holders: [GestureHolderRow] = []` and `func gestureFrom(target:phase:holder:)`, whose body is `gesture`'s (it throws `gestureError`, then `valueError` for a value phase, and appends to `gestures`; `AutomationCommandsTests` relies on both) and then appends the holder. Then add this test to `AutomationCommandsTests.swift`, after `automationCommandsCheckTheirArguments`:

   ```swift
   @Test func theChannelGesturesAsItsOwnHolder() throws {
       let (id, m, f) = try doc()
       let o = AutomationCommands.run(
           AutomationCommands.gesture, args: [arg("target", "fader:1"), arg("phase", "touch")], doc: id, m)
       #expect(code(o) == nil)
       #expect(f.holders == [.channel])
       #expect(f.gestures.last?.1 == .touch(value: nil), "recorded as `gesture` records it")
   }
   ```

- [ ] **Step 4: Run the tests and the gates.** Run `cargo test --workspace`, both clippies, `cargo clippy -p core-ffi --lib -- -D warnings`, `scripts/build-ffi.sh`, `scripts/swift.sh build` and `scripts/swift.sh test`. Expected: Rust +6, Swift +1, and every existing automation test green (`tests/automation.rs`, `autowrite.rs` and `params.rs` use `gesture`, which still means Mouse; a mouse click with no move committed nothing before and commits nothing now).

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-ffi/src/automation.rs crates/core-ffi/src/session.rs crates/core-ffi/tests/holders.rs app/Sources/VizDAWAppModel/AutomationCommands.swift app/Tests/VizDAWAppModelTests/FakeSession.swift app/Tests/VizDAWAppModelTests/AutomationCommandsTests.swift
git commit -m "core-ffi: a touch is held by each of its holders and ends with the last release; the static edit opens at the first value (plan 5f-1 Task 6; 5e-2 carry m8)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -8
```
Counts: Rust +6, Swift +1.

---

### Task 7: `core-ffi::surfaces`: the registry, the ports, `surfaces.json`, add/remove, connection

**Files:**
- Create: `crates/core-ffi/src/surfaces.rs`, `crates/core-ffi/src/surfaces/ports.rs`
- Modify: `crates/core-ffi/src/lib.rs`, `crates/core-ffi/src/session.rs` (the `idle` hook, `focus_surfaces`), `app/Tests/VizDAWAppModelTests/FakeSession.swift` (the `focusSurfaces()` stub)
- Test: create `crates/core-ffi/tests/surfaces.rs`

**Interfaces:**
- Consumes: `core_surface::{new_surface, Surface, SurfaceKind, Bank, learn::CommandMap}`, `crate::automation::{TouchHolder, SurfaceId}`, `crate::recovery::support_dir`, `crate::host::offline_mode()` (public at `276c855`), `core_engine::input::port_key`.
- Produces:
  ```rust
  // surfaces.rs
  #[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)] pub enum SurfaceKindRow { Mcu, McuXt, Hui }
  #[derive(Debug, Clone, PartialEq, uniffi::Record)]
  pub struct SurfaceRow { /// 0–15 (Ruling 4).
      pub id: u32, pub kind: SurfaceKindRow, pub input: String, pub output: String, pub position: u8,
      /// Both ports present.
      pub connected: bool, /// Answered (MCU handshake or any valid message; HUI ping).
      pub online: bool, pub ignored: u64 }
  #[derive(Debug, Clone, PartialEq, Default, uniffi::Record)] pub struct SurfacePortsRow { pub sources: Vec<String>, pub destinations: Vec<String> }
  /// Reads surfaces.json (Ruling 9); problems go to `state.get surfaces`.
  #[uniffi::export] pub fn surfaces_load() -> Result<(), VizError>;
  #[uniffi::export] pub fn surface_list() -> Vec<SurfaceRow>;
  /// `position` None: the next free one in its group. The ID is the lowest free one (Ruling 4).
  /// Refused: a second MCU, a fourth XT, a 17th surface, a taken position, a port another
  /// surface uses, an empty or control-character or > 255-byte port.
  #[uniffi::export] pub fn surface_add(kind: SurfaceKindRow, input: String, output: String, position: Option<u8>) -> Result<u32, VizError>;
  /// Owes the unit's `release_all()` to the session holding it (Task 8 applies it).
  #[uniffi::export] pub fn surface_remove(id: u32) -> Result<(), VizError>;
  #[uniffi::export] pub fn surface_ports() -> SurfacePortsRow;
  /// Raw input from a port (CoreMIDI's thread, `midi.inject`, tests): queued with its host
  /// time (0: now), never waits. True when a surface, a waiting learn or a map listens to `port`.
  pub(crate) fn input(port: &str, bytes: &[u8], host_ns: u64) -> bool;
  /// Process-wide state (`surfaces::lock()`): surfaces, the group banks, focus,
  /// `owed: BTreeMap<u64, Vec<(TouchHolder, AutomationTarget)>>`, the ports, the clock.
  pub(crate) struct Registry { … }
  /// The input queue, its own lock (Ruling 10): `(port, host_ns, bytes)`, at most 4096, drops counted.
  static INPUT: Mutex<Queue>;
  #[cfg(feature = "test-hooks")] pub fn debug_reset();                 // forget everything, fresh FakePorts, stale off
  #[cfg(feature = "test-hooks")] pub fn debug_set_missing(ports: Vec<String>);
  #[cfg(feature = "test-hooks")] pub fn debug_stale_sources(on: bool);  // sources() answer the last refresh()
  #[cfg(feature = "test-hooks")] pub fn debug_sent(port: &str) -> Vec<Vec<u8>>;   // drains
  #[cfg(feature = "test-hooks")] pub fn debug_advance(ms: u64);         // the surfaces clock
  #[cfg(feature = "test-hooks")] pub fn debug_support_file() -> std::path::PathBuf;
  // surfaces/ports.rs
  pub(crate) trait Ports: Send { fn send(&mut self, port: &str, bytes: &[u8]) -> bool; fn sources(&self) -> Vec<String>; fn destinations(&self) -> Vec<String>; fn refresh(&mut self); }
  pub(crate) struct FakePorts { … }   // every name present unless missing; records sends; optional stale lists
  #[cfg(all(target_os = "macos", not(feature = "test-hooks")))]
  pub(crate) struct CoreMidiPorts { … } // SourceListener + MidiSystem (Ruling 10)
  #[cfg(all(not(target_os = "macos"), not(feature = "test-hooks")))]
  pub(crate) struct NoPorts;           // no sources, no destinations
  ```

Connection handling:
- A surface is `connected` when its input is in `sources()` and its output is in `destinations()`.
- `surface_add` (and `surfaces_load`) calls `refresh()` first and stamps the 2 s timer, then checks the ports; a connected surface is sent its `hello()` before the call returns.
- **While any surface exists**, the service calls `refresh()` at most every 2 s (Ruling 10), measured on the surfaces clock, then re-checks every surface. A `send` that returns false also marks its surface disconnected at once.
- When a connected surface's port vanishes:
  - the registry records `owed` for the unit's `release_all()` under the session that holds its touches (the focused one), which Task 8 applies;
  - it marks the surface disconnected.
- When the port returns, the surface is `refresh()`ed and `hello()` is sent.
- `surfaces_load` validates every entry by Ruling 9 (and Ruling 4's counts, positions, unique ports, unique IDs below 16); one failure keeps the whole file aside as `.bad`.
- Locks follow Global Constraints: `Session.inner` → registry → input queue.

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-ffi/tests/surfaces.rs <<'VIZDAW_EOF'
//! Surfaces in the core (plan 5f-1 Tasks 7–8; spec §8, §12.1–§12.3).
//! The fake port transport only: no CoreMIDI client is created.

mod common;

use std::sync::{Arc, Mutex, MutexGuard};
use vizdaw_ffi::surfaces::{
    SurfaceKindRow as Kind, debug_advance, debug_reset, debug_sent, debug_set_missing,
    debug_stale_sources, debug_support_file, surface_add, surface_list, surface_remove,
    surfaces_load,
};

static SERIAL: Mutex<()> = Mutex::new(());

pub fn serial() -> MutexGuard<'static, ()> {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    vizdaw_ffi::host::force_offline();
    vizdaw_ffi::host::reset_host();
    debug_reset();
    let _ = std::fs::remove_file(debug_support_file());
    g
}

#[test]
fn surfaces_are_added_listed_saved_and_removed() {
    let _g = serial();
    let a = surface_add(Kind::Mcu, "MCU In".into(), "MCU Out".into(), None).unwrap();
    let b = surface_add(Kind::McuXt, "XT In".into(), "XT Out".into(), None).unwrap();
    let list = surface_list();
    assert_eq!(list.len(), 2);
    assert_eq!((list[0].id, list[0].position, list[1].position), (a, 0, 1));
    assert!(list.iter().all(|s| s.connected), "fake ports are present");
    assert_eq!(
        debug_sent("MCU Out").first(),
        Some(&vec![0xF0, 0x00, 0x00, 0x66, 0x14, 0x00, 0xF7]),
        "the device query on connect"
    );
    let saved = std::fs::read_to_string(debug_support_file()).unwrap();
    assert!(saved.contains("\"version\": 1") || saved.contains("\"version\":1"), "{saved}");
    debug_reset();
    surfaces_load().unwrap();
    assert_eq!(surface_list().len(), 2, "read back");
    surface_remove(b).unwrap();
    assert_eq!(surface_list().len(), 1);
    assert!(surface_remove(b).is_err());
    let c = surface_add(Kind::Hui, "H In".into(), "H Out".into(), None).unwrap();
    assert_eq!((a, b, c), (0, 1, 1), "IDs are the lowest free ones");
}

#[test]
fn a_fifth_unit_or_a_second_main_is_refused() {
    let _g = serial();
    surface_add(Kind::Mcu, "A".into(), "A".into(), None).unwrap();
    assert!(surface_add(Kind::Mcu, "B".into(), "B".into(), None).is_err());
    surface_add(Kind::McuXt, "X0".into(), "X0".into(), Some(2)).unwrap();
    let e = surface_add(Kind::McuXt, "X1".into(), "X1".into(), Some(2)).unwrap_err();
    assert!(e.to_string().contains("position 2 is taken"), "{e}");
    for n in 1..3 {
        surface_add(Kind::McuXt, format!("X{n}"), format!("X{n}"), None).unwrap();
    }
    let e = surface_add(Kind::McuXt, "X4".into(), "X4".into(), None).unwrap_err();
    assert!(e.to_string().contains("up to three extenders"), "{e}");
    surface_add(Kind::Hui, "H".into(), "H".into(), None).unwrap();
}

#[test]
fn hostile_surface_calls_are_refused() {
    let _g = serial();
    for (i, o) in [
        ("", "x"),
        ("x", ""),
        (&*"p".repeat(256), "x"),
        ("a\nb", "x"),
    ] {
        assert!(surface_add(Kind::Hui, i.to_string(), o.to_string(), None).is_err(), "{i:?} {o:?}");
    }
    assert!(surface_add(Kind::McuXt, "x".into(), "x".into(), Some(9)).is_err(), "positions 0–3");
    assert!(surface_remove(u32::MAX).is_err());
    assert!(surface_list().is_empty());
    surface_add(Kind::Hui, "P In".into(), "P Out".into(), None).unwrap();
    let e = surface_add(Kind::Hui, "P In".into(), "Other Out".into(), None).unwrap_err();
    assert!(e.to_string().contains("is used by surface"), "{e}");
    assert!(surface_add(Kind::Hui, "Other In".into(), "P Out".into(), None).is_err(), "outputs too");
    for n in 1..16 {
        surface_add(Kind::Hui, format!("H{n} In"), format!("H{n} Out"), None).unwrap();
    }
    let e = surface_add(Kind::Hui, "H16 In".into(), "H16 Out".into(), None).unwrap_err();
    assert!(e.to_string().contains("at most 16 surfaces"), "{e}");
    assert!(surface_list().iter().all(|s| s.id < 16));
}

#[test]
fn a_vanished_port_disconnects() {
    let _g = serial();
    let id = surface_add(Kind::Mcu, "U In".into(), "U Out".into(), None).unwrap();
    debug_set_missing(vec!["U In".into()]);
    debug_advance(2_100);
    let s = vizdaw_ffi::Session::new_demo().unwrap();
    s.focus_surfaces().unwrap();
    s.idle().unwrap();
    let row = surface_list().into_iter().find(|r| r.id == id).unwrap();
    assert!(!row.connected);
    // The touch half is Task 8's `a_surface_touch_is_released_when_its_port_goes`.
}

/// Pre-flight I2: CoreMIDI's source list changes only at a refresh, so
/// the refresh must run while every unit is connected.
#[test]
fn a_vanished_port_is_seen_while_connected() {
    let _g = serial();
    debug_stale_sources(true);
    let id = surface_add(Kind::Mcu, "V In".into(), "V Out".into(), None).unwrap();
    let s = vizdaw_ffi::Session::new_demo().unwrap();
    s.focus_surfaces().unwrap();
    debug_set_missing(vec!["V In".into()]);
    debug_advance(100);
    s.idle().unwrap();
    let connected = || surface_list().into_iter().find(|r| r.id == id).unwrap().connected;
    assert!(connected(), "the stale source list still has it");
    debug_advance(2_100);
    s.idle().unwrap();
    assert!(!connected(), "the 2 s refresh runs while connected and sees it go");
}

#[test]
fn a_damaged_surfaces_file_is_kept_aside() {
    let _g = serial();
    let f = debug_support_file();
    std::fs::create_dir_all(f.parent().unwrap()).unwrap();
    std::fs::write(&f, "{ not json").unwrap();
    surfaces_load().unwrap();
    assert!(surface_list().is_empty());
    assert!(f.with_extension("json.bad").exists());
    std::fs::write(&f, r#"{"version": 99, "surfaces": [], "commands": []}"#).unwrap();
    surfaces_load().unwrap();
    assert!(std::fs::read_to_string(f.with_extension("json.bad")).unwrap().contains("99"));
    for (bad, why) in [
        (
            r#"{"version": 1, "surfaces": [{"id": 1000, "kind": "hui", "input": "a", "output": "b", "position": 0}], "commands": []}"#,
            "an ID above 15",
        ),
        (
            r#"{"version": 1, "surfaces": [{"id": 0, "kind": "mcu", "input": "a", "output": "b", "position": 0}, {"id": 1, "kind": "mcu", "input": "c", "output": "d", "position": 1}], "commands": []}"#,
            "two main units",
        ),
        (
            r#"{"version": 1, "surfaces": [{"id": 0, "kind": "hui", "input": "a", "output": "b", "position": 0}, {"id": 1, "kind": "hui", "input": "a", "output": "d", "position": 0}], "commands": []}"#,
            "a shared port",
        ),
    ] {
        let _ = std::fs::remove_file(f.with_extension("json.bad"));
        std::fs::write(&f, bad).unwrap();
        surfaces_load().unwrap();
        assert!(surface_list().is_empty(), "{why}");
        assert!(f.with_extension("json.bad").exists(), "{why}");
    }
}

pub fn keep(_: Arc<vizdaw_ffi::Session>) {
    let _ = common::wait_limit();
}
VIZDAW_EOF
```

`with_extension("json.bad")` on `surfaces.json` gives `surfaces.json.bad`. The `Session` calls here (`focus_surfaces`, and `idle`'s service) arrive with Task 8. In this task, give `Session::focus_surfaces` and the `idle` hook their minimal form (the hook only runs the connection checks of step 0), and Task 8 fills them in. `a_returning_port_reconnects_with_a_full_refresh` needs feedback, so it is Task 8's.

- [ ] **Step 2: Run them to see them fail** (`cargo test -p core-ffi --test surfaces`). Expected: FAIL — module `surfaces` not found.

- [ ] **Step 3: Implement.** Add `pub mod surfaces;` to `crates/core-ffi/src/lib.rs`:

   Find:
   ```rust
   pub mod step;
   ```
   Replace:
   ```rust
   pub mod step;
   pub mod surfaces;
   ```

   Write `surfaces.rs` (the registry behind a `Mutex`, the input queue behind its own, `surfaces.json` by Ruling 9 with temp-and-rename writes and the load checks, the exports above, the surfaces clock `now()` = `Instant` captured at first use + real elapsed + `debug_advance` offset) and `surfaces/ports.rs` (Ruling 10; the three transports, chosen as Global Constraints says). Port names reuse `crate::live::parse_injection`'s rule: non-empty, at most 255 bytes, no control characters. In `session.rs`, add the minimal `Session::focus_surfaces(&self) -> Result<(), VizError>`, which sets the focus to `self.id`. Then:

   Find:
   ```rust
               self.service_writing();
   ```
   Replace:
   ```rust
               self.service_writing();
               // Control surfaces and MIDI learn (plan 5f-1 Ruling 11).
               self.service_surfaces();
   ```
   with `fn service_surfaces(&self)` (connection checks only in this task) in an `impl Session` block in `session.rs`, or in a child module `session/surfaces.rs`, because `Inner`, `Session.inner`, `heard_value`, `gesture_held` and `release_from` are private to `session`; it calls `crate::surfaces` for the registry. Task 8's `take_meters` and its exported `Session` methods go in the same block.

   In `FakeSession.swift`, add `var focusCalls = 0` and `func focusSurfaces() throws { focusCalls += 1 }`.

- [ ] **Step 4: Run the tests and the gates** (with `scripts/build-ffi.sh`, `scripts/swift.sh build` and `scripts/swift.sh test`, since `focus_surfaces` is exported). Expected: Rust +6; Swift unchanged.

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-ffi/src/surfaces.rs crates/core-ffi/src/surfaces/ports.rs crates/core-ffi/src/lib.rs crates/core-ffi/src/session.rs crates/core-ffi/tests/surfaces.rs app/Tests/VizDAWAppModelTests/FakeSession.swift
git commit -m "core-ffi: the surfaces registry, fake, CoreMIDI and no ports, surfaces.json checked on load, connect and reconnect (plan 5f-1 Task 7)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -9
```
Counts: Rust +6.

---

### Task 8: The service: owed releases, actions into the focused session, heard values, feedback timing, meters, the claims, learn

**Files:**
- Modify: `crates/core-ffi/src/surfaces.rs`, `crates/core-ffi/src/surfaces/ports.rs`, `crates/core-ffi/src/session.rs` (`inject_midi`, `levels`, `close`), `crates/core-ffi/src/host.rs` (`midi_from_source`), `crates/core-ffi/src/input.rs` (`NoteInput` through `SourceListener`), `app/Tests/VizDAWAppModelTests/FakeSession.swift`
- Test: modify `crates/core-ffi/tests/surfaces.rs`; create `crates/core-ffi/tests/learn.rs`

**Interfaces:**
- Consumes: Tasks 5–7; `Session::gesture_held`, `release_from` (Task 6); `Session::heard_value` (session.rs:5631), `crate::automation::existing`, `host::levels`, `host::position`, `position_bbu`, `Session::params` (plugin parameters), `core_editor::transport::marker_step`, `core_model::meter::MeterMap`, `core_model::learn`.
- Produces:
  ```rust
  #[derive(Debug, Clone, PartialEq, uniffi::Enum)]
  pub enum SurfaceEventRow {
      /// `programmatic`: caused by `surface.inject` or `midi.inject` (Ruling 12).
      Command { id: String, programmatic: bool },
      Select { strip: u64 },
      /// `path`: the strip's output path when it is an AudioPath, else None.
      Arm { strip: u64, path: Option<u64> },
      Jog { detents: i32 },
  }
  #[derive(Debug, Clone, PartialEq, uniffi::Record)]
  pub struct SurfaceFeedbackRow { pub surface: u32, /// Hex, the last 64, meters left out.
      pub messages: Vec<String>, pub meters_sent: u64 }
  #[derive(Debug, Clone, PartialEq, Default, uniffi::Record)]
  pub struct SurfaceStateRow { pub surfaces: Vec<SurfaceRow>, pub feedback: Vec<SurfaceFeedbackRow>, pub learning: Option<String>, pub problems: Vec<String> }
  #[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)] pub enum LearnKindRow { Cc7, Cc14, PitchBend, Note }
  #[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)] pub enum LearnModeRow { Absolute, Relative2sComplement, RelativeBinOffset }
  #[derive(Debug, Clone, PartialEq, uniffi::Record)] pub struct LearnSourceRow { pub port: String, pub kind: LearnKindRow, pub channel: u8, pub number: u8 }
  #[derive(Debug, Clone, PartialEq, uniffi::Record)]
  pub struct LearnMapRow { pub source: LearnSourceRow, pub text: String, pub target: AutomationTargetRow, pub missing: bool, pub mode: LearnModeRow,
      /// Ruling 18; `picks_up`: whether the map's kind and mode use it.
      pub pickup: bool, pub picks_up: bool }
  #[derive(Debug, Clone, PartialEq, uniffi::Record)] pub struct CommandMapRow { pub source: LearnSourceRow, pub text: String, pub command: String }
  #[derive(Debug, Clone, PartialEq, uniffi::Enum)] pub enum LearnForRow { Target { target: AutomationTargetRow }, Command { id: String } }
  #[derive(Debug, Clone, PartialEq, Default, uniffi::Record)] pub struct LearnStateRow { pub waiting: Option<String>, pub last: Option<String> }
  /// `state.get surfaces` without a document (pre-flight M29): no feedback rows.
  #[uniffi::export] pub fn surfaces_state() -> SurfaceStateRow;
  #[uniffi::export] pub fn command_maps() -> Vec<CommandMapRow>;
  /// Removes every command map for `command` (pre-flight M14).
  #[uniffi::export] pub fn remove_command_map(command: String) -> Result<(), VizError>;
  /// Ruling 11: needs a focused session to complete; only key-bindable IDs (Ruling 8).
  #[uniffi::export] pub fn learn_command(id: String) -> Result<(), VizError>;
  #[uniffi::export] pub fn cancel_learn();
  #[uniffi::export] pub fn learn_state() -> LearnStateRow;
  /// Whether surfaces or learn take `bytes` from the port with this key (Ruling 17). Never waits.
  pub(crate) fn takes(port_key: u32, bytes: &[u8]) -> bool;
  /// The session closes: cancel its learn; if it was focused, blank the units (Ruling 11).
  pub(crate) fn session_closed(id: u64);
  // Session (exported)
  pub fn focus_surfaces(&self) -> Result<(), VizError>;
  /// Unknown IDs are ignored; more than 1024 strips is refused (`Model`).
  pub fn set_surface_selection(&self, strips: Vec<u64>) -> Result<(), VizError>;
  /// Feeds `bytes` to surface `surface` as if from its input, focuses this session and services it at once.
  pub fn inject_surface(&self, surface: u32, bytes: Vec<u8>) -> Result<(), VizError>;
  pub fn surface_state(&self) -> Result<SurfaceStateRow, VizError>;
  pub fn take_surface_events(&self) -> Result<Vec<SurfaceEventRow>, VizError>;
  pub fn learn_target(&self, target: AutomationTargetRow) -> Result<(), VizError>;
  pub fn learn_maps(&self) -> Result<Vec<LearnMapRow>, VizError>;
  pub fn set_learn_mode(&self, source: LearnSourceRow, mode: LearnModeRow) -> Result<DocSnapshot, VizError>;
  /// Ruling 18: a `SetLearn` of the changed map ("MIDI Learn").
  pub fn set_learn_pickup(&self, source: LearnSourceRow, on: bool) -> Result<DocSnapshot, VizError>;
  pub fn remove_learn(&self, source: LearnSourceRow) -> Result<DocSnapshot, VizError>;
  /// Every map to `target`, as one step (`RemoveLearn { target }`).
  pub fn unlearn_target(&self, target: AutomationTargetRow) -> Result<DocSnapshot, VizError>;
  ```

`service_surfaces` runs in this order (every session's `idle` runs steps 1 and 6; the focused session's runs all):
0. **Connection checks** (Task 7): the 2 s refresh while any surface exists, vanishes (owing `release_all()`), returns (refresh and hello).
1. **Owed releases**: take `owed[self.id]` from the registry, drop the registry, and apply each through `release_from(…, holder)`. An unfocused session then runs only step 6, for a learn it asked for, and stops.
2. **Input**: `mem::take` the input queue's batch (port, host time, bytes).
3. **Route** each message: to the surface whose input port it came from (`Surface::input`); else to the learner, when a learn waits and this port is no surface's; else to the `Mapper` (this session's `Settings.learn` plus the command maps). Before routing, the service compares `Settings.learn` with the Mapper's maps and re-sets them when the document changed them (undo, redo, revert, open; Ruling 17), keeping the state of unchanged maps, owing `release_from(TouchHolder::Learn)` for held targets no map drives any more, and rewriting the claims. The learner is fed by whichever session is focused; the decision belongs to the session that asked (step 6).
4. **Apply** the actions (copied out of the registry first):
   - `Gesture` → `gesture_held` with the unit's `TouchHolder::Surface(id)` or `TouchHolder::Learn`, `Source::Ui`, and the message's host time. A refused gesture (a vanished target, a Busy first value) is counted in that surface's `ignored`, never raised.
   - `IdleRelease(t, at)` → `gesture_held(…, Release, …)` stamped at `at` (Ruling 2).
   - `Bank`/`Channel` → the group's `Bank::shift`, then a full refresh of that group.
   - `Skip(±1)` → a locate of the transport's sequence (`host::position`) to `marker_step`, else the previous/next bar line of the sequence's meter map, never below 0. `Wind(q)` → a locate to `base + round(q × PPQ)`, where `base` is the tick the last wind or skip went to when it was less than 250 ms ago, else the transport's tick. Both go through the same internal path as `Session::locate` (split its body out as `locate_in(inner, tick)` if the export does the work inline). While recording, or with no sequence loaded, they are counted in `ignored`.
   - `Select`/`Arm`/`Jog`/`Command` → queued `SurfaceEventRow`s for `take_surface_events` (`programmatic` set when the bytes came from `inject_surface` or `inject_midi`).
5. **Tick** every surface and the mapper, and apply their actions as in step 4.
6. **Learn decisions**: a learn this session asked for decides (`Learner::decide`) and executes `SettingsCmd::SetLearn` (undo "MIDI Learn", `pickup: true`) on this session, or stores a `CommandMap` and saves `surfaces.json`.
7. **The MixerView** (Ruling 13), built only when a unit is due: every value is `heard_value(inner, t)`, with `existing(doc, seq, t)` cached per (document version, seq); `select` from `set_surface_selection`; `plugin` from the first selected strip's first insert's first 8 parameters (their heard values); the arm LED by Ruling 12; meters from the session's Surfaces meter hold.
8. **Feedback** by Ruling 13: units whose last feedback was ≥ 20 ms ago, plus at once any unit or learn map showing a target that a holder other than it moved in this pass; the mapper's feedback with the same heard values.

The rest of the core:
- **The claims** (Ruling 17): a `RwLock<Claims>` holding the port keys of configured surfaces' inputs, whether a learn waits, and the command maps and the focused session's learn maps by port key. The registry rewrites it on every surface, map, learn or focus change, and when step 3 finds the focused document's maps changed. `takes(port_key, bytes)` reads it with `try_read` and answers false when it cannot (Global Constraints). `input(port, …)` answers whether a surface, learn or map listens to `port`, from the same claims.
- **The lanes:** in `host.rs`, `midi_from_source` returns early when `takes` claims the message:

   Find:
   ```rust
   fn midi_from_source(port: u32, host_ns: u64, bytes: &[u8]) {
       let Some(msg) = core_engine::input::channel_message(bytes) else {
           return;
       };
   ```
   Replace:
   ```rust
   fn midi_from_source(port: u32, host_ns: u64, bytes: &[u8]) {
       // Surface ports and learned messages stay out of the lanes (plan
       // 5f-1 Ruling 17).
       if crate::surfaces::takes(port, bytes) {
           return;
       }
       let Some(msg) = core_engine::input::channel_message(bytes) else {
           return;
       };
   ```
- **Note input and step entry:** in `input.rs`, `NoteInput` listens through `SourceListener`:

   Find:
   ```rust
       listener: Mutex<Option<platform_macos::midi::NoteListener>>,
   ```
   Replace:
   ```rust
       listener: Mutex<Option<platform_macos::midi::SourceListener>>,
   ```

   Find:
   ```rust
       let l = platform_macos::midi::NoteListener::connect_all(
           "VizDAW Note Input",
           Box::new(move |b: &[u8]| push(&q, b)),
       )?;
       let n = l.sources();
   ```
   Replace:
   ```rust
       // Through `SourceListener`, which knows each message's port, so what
       // surfaces and learned maps take stays out of note input and step
       // entry (plan 5f-1 Ruling 17).
       let l = platform_macos::midi::SourceListener::connect_all(
           "VizDAW Note Input",
           Box::new(move |port: u32, _: u64, b: &[u8]| {
               // A panic must not unwind into CoreMIDI.
               let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                   if !crate::surfaces::takes(port, b) {
                       push(&q, b);
                   }
               }));
           }),
       )?;
       let n = u32::try_from(l.sources().len()).unwrap_or(u32::MAX);
   ```
- **`inject_midi`** feeds surfaces, learn and maps, and sends the engine only what they do not take (Rulings 16, 17):

   Find:
   ```rust
               let msgs = crate::live::parse_injection(&port, &bytes)?;
               let Some(tx) = host::midi_sender(self.id) else {
                   return Ok(false);
               };
               let key = core_engine::input::port_key(&port);
               for msg in msgs {
   ```
   Replace:
   ```rust
               let msgs = crate::live::parse_injection(&port, &bytes)?;
               // Surfaces, MIDI learn and learned maps hear injected bytes
               // too (plan 5f-1 Ruling 16), serviced by the session's idle;
               // what they take stays out of the lanes (Ruling 17).
               let learnt = crate::surfaces::input(&port, &bytes, 0);
               let key = core_engine::input::port_key(&port);
               let msgs: Vec<_> = msgs
                   .into_iter()
                   .filter(|m| !crate::surfaces::takes(key, m.as_slice()))
                   .collect();
               if msgs.is_empty() {
                   return Ok(learnt);
               }
               let Some(tx) = host::midi_sender(self.id) else {
                   return Ok(learnt);
               };
               for msg in msgs {
   ```
- **The meter holds** (Ruling 13): `Inner` gains `meters: MeterHolds { mixer, surfaces }`; `take_meters(&self, inner)` calls `host::levels(self.id)` once and folds its peaks (and, into the Mixer hold only, the DSP peak) into both holds as maxima. `idle` calls it before the service; `levels` reads and clears the Mixer hold:

   Find:
   ```rust
           ffi_guard(|| Ok(host::levels(self.id).unwrap_or_else(MixerLevels::empty)))
   ```
   Replace:
   ```rust
           // The surfaces read the same peaks (plan 5f-1 Ruling 13): one take
           // feeds both holds, each cleared only by its own reader.
           ffi_guard(|| {
               let mut inner = lock(&self.inner);
               self.take_meters(&mut inner);
               Ok(inner.meters.take_mixer().unwrap_or_else(MixerLevels::empty))
           })
   ```
- **Closing:** `Session::close` calls `crate::surfaces::session_closed(self.id)` after `release_touches` (its learn is cancelled; if it was focused, focus goes to none and every connected unit gets a blank refresh).
- **Feedback rows:** `SurfaceFeedbackRow.messages` keeps the last 64 non-meter messages in hex; `meters_sent` counts meter messages; `problems` includes the input queue's drops (Ruling 10).

- [ ] **Step 1: Write the failing tests.** Append to `crates/core-ffi/tests/surfaces.rs`:

```rust
fn wait_until(what: &str, mut ok: impl FnMut() -> bool) {
    let until = std::time::Instant::now() + common::wait_limit();
    while !ok() {
        assert!(std::time::Instant::now() < until, "timed out waiting for {what}");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

fn song(s: &vizdaw_ffi::Session) -> u64 {
    s.snapshot().unwrap().sequences.iter().find(|r| r.name == "Song").unwrap().id
}

fn synth(s: &vizdaw_ffi::Session) -> (u64, usize) {
    let m = s.mixer().unwrap();
    let i = m.strips.iter().position(|x| x.name == "Built-in Synth").unwrap();
    (m.strips[i].id, i)
}

fn mcu(s: &vizdaw_ffi::Session) -> u32 {
    let id = surface_add(Kind::Mcu, "T In".into(), "T Out".into(), None).unwrap();
    s.focus_surfaces().unwrap();
    debug_advance(30);
    s.idle().unwrap();
    let _ = debug_sent("T Out");
    id
}

/// The values (0–16383) sent to fader channel `ch` on `port`, in order.
fn faders(port: &str, ch: u8) -> Vec<u16> {
    debug_sent(port)
        .iter()
        .filter(|m| m.len() == 3 && m[0] == 0xE0 + ch)
        .map(|m| u16::from(m[1]) | u16::from(m[2]) << 7)
        .collect()
}

/// A note held for 64 beats on the synth, and a bus whose fader ramps
/// 0.2 → 0.8 over 32 beats (`tests/automation.rs`' `ramped_bus`):
/// (session, sequence, bus).
fn ramped_bus() -> (Arc<vizdaw_ffi::Session>, u64, u64) {
    use core_engine::fixtures::{Builder, Q};
    use core_model::automation::AutomationTarget;
    use core_model::event::EventBody;
    let mut b = Builder::new();
    let synth = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, notes) = b.sequence("S", "Notes", Some(synth)).unwrap();
    b.notes(seq, notes, &[(0, 60, 0x8000, 64 * Q)]).unwrap();
    let bus = b.bus("Bus").unwrap();
    let auto = b.lane(seq, "Auto", None).unwrap();
    for (tick, v) in [(0, 0.2), (32 * Q, 0.8)] {
        b.event(seq, auto, tick, EventBody::Automation { target: AutomationTarget::StripFader(bus), value: v })
            .unwrap();
    }
    (vizdaw_ffi::Session::from_document(b.finish()).unwrap(), seq.0, bus.0)
}

#[test]
fn an_mcu_fader_ride_is_one_static_step_and_its_mute_a_click() {
    let _g = serial();
    let s = vizdaw_ffi::Session::new_demo().unwrap();
    let (synth, k) = synth(&s);
    let id = mcu(&s);
    let ch = k as u8; // the synth's index in the display order (the demo's strips fit one bank)
    s.inject_surface(id, vec![0x90, 0x68 + ch, 0x7F, 0xE0 + ch, 0x00, 0x20]).unwrap();
    let st = s.automation_state().unwrap();
    assert_eq!(st.touched, vec![vizdaw_ffi::AutomationTargetRow::StripFader { strip: synth }]);
    s.inject_surface(id, vec![0x90, 0x68 + ch, 0x00]).unwrap();
    assert_eq!(s.snapshot().unwrap().undo_label.as_deref(), Some("Set Fader"));
    s.inject_surface(id, vec![0x90, 0x10 + ch, 0x7F]).unwrap();
    assert!(s.mixer().unwrap().strips[k].mute);
    debug_advance(30);
    s.idle().unwrap();
    let sent = debug_sent("T Out");
    assert!(sent.contains(&vec![0x90, 0x10 + ch, 0x7F]), "the mute LED follows: {sent:?}");
    let st = s.surface_state().unwrap();
    assert!(st.feedback[0].messages.iter().any(|m| m == &format!("90 {:02x} 7f", 0x10 + ch)));
    assert!(st.feedback[0].messages.iter().all(|m| !m.starts_with("d0 ")), "meters are counted, not listed");
}

#[test]
fn buttons_become_events_for_the_app() {
    let _g = serial();
    let s = vizdaw_ffi::Session::new_demo().unwrap();
    let (synth, k) = synth(&s);
    let id = mcu(&s);
    s.inject_surface(id, vec![0x90, 0x5E, 0x7F, 0x90, 0x18 + k as u8, 0x7F, 0xB0, 0x3C, 0x02]).unwrap();
    use vizdaw_ffi::surfaces::SurfaceEventRow as E;
    assert_eq!(
        s.take_surface_events().unwrap(),
        vec![
            E::Command { id: "transport.play".into(), programmatic: true },
            E::Select { strip: synth },
            E::Jog { detents: 2 },
        ]
    );
    assert!(s.take_surface_events().unwrap().is_empty());
}

#[test]
fn feedback_waits_20_ms_but_partners_go_at_once() {
    let _g = serial();
    let s = vizdaw_ffi::Session::new_demo().unwrap();
    let (_, k) = synth(&s);
    let a = mcu(&s);
    let b = surface_add(Kind::Hui, "H In".into(), "H Out".into(), None).unwrap();
    debug_advance(30);
    s.idle().unwrap();
    let _ = (debug_sent("T Out"), debug_sent("H Out"));
    let ch = k as u8;
    s.inject_surface(a, vec![0x90, 0x68 + ch, 0x7F, 0xE0 + ch, 0x7F, 0x7F]).unwrap();
    let hui = debug_sent("H Out");
    assert!(hui.contains(&vec![0xB0, ch, 0x7F]), "the HUI fader follows at once: {hui:?}");
    debug_advance(30);
    s.idle().unwrap();
    assert!(debug_sent("T Out").iter().all(|m| m[0] & 0xF0 != 0xE0), "no fader feedback to the hand");
    let _ = b;
}

/// Pre-flight C1: what plays, not the static value, drives the motor.
#[test]
fn motor_faders_follow_automation_playback() {
    let _g = serial();
    let (s, seq, bus) = ramped_bus();
    let k = s.mixer().unwrap().strips.iter().position(|x| x.id == bus).unwrap() as u8;
    let _ = mcu(&s);
    s.play(seq).unwrap();
    let mut seen = Vec::new();
    wait_until("four beats of the ramp", || {
        debug_advance(30);
        s.idle().unwrap();
        seen.extend(faders("T Out", k));
        s.transport().unwrap().tick > 4 * 15_360
    });
    s.stop().unwrap();
    assert!(seen.len() >= 3, "the fader moved with its automation: {seen:?}");
    assert!(seen.windows(2).all(|w| w[1] > w[0]), "rising along the ramp: {seen:?}");
    assert!(
        seen[0] >= (0.2 * 16383.0) as u16 - 2 && *seen.last().unwrap() < (0.3 * 16383.0) as u16,
        "0.2 at the start, under 0.3 four beats in: {seen:?}"
    );
}

/// Pre-flight C1: a Touch ride leaves the static value alone, so the
/// partner must follow the heard value, out and back.
#[test]
fn a_touch_ride_moves_the_partner_and_glides_back_on_release() {
    let _g = serial();
    let (s, seq, bus) = ramped_bus();
    let k = s.mixer().unwrap().strips.iter().position(|x| x.id == bus).unwrap() as u8;
    let id = mcu(&s);
    surface_add(Kind::Hui, "H In".into(), "H Out".into(), None).unwrap();
    s.set_automation_mode(vec![bus], vizdaw_ffi::automation::AutomationModeRow::Touch).unwrap();
    s.play(seq).unwrap();
    wait_until("the roll", || s.transport().unwrap().tick > 15_360);
    debug_advance(30);
    s.idle().unwrap();
    let _ = (debug_sent("T Out"), debug_sent("H Out"));
    s.inject_surface(id, vec![0x90, 0x68 + k, 0x7F, 0xE0 + k, 0x7F, 0x7F]).unwrap();
    assert!(debug_sent("H Out").contains(&vec![0xB0, k, 0x7F]), "the HUI follows the ride at once");
    s.inject_surface(id, vec![0x90, 0x68 + k, 0x00]).unwrap();
    let mut his: Vec<u8> = Vec::new();
    wait_until("the glide back to the automation", || {
        debug_advance(30);
        s.idle().unwrap();
        his.extend(debug_sent("H Out").into_iter().filter(|m| m.len() == 3 && m[0] == 0xB0 && m[1] == k).map(|m| m[2]));
        his.last().is_some_and(|hi| *hi < 0x40)
    });
    s.stop().unwrap();
    assert!(his.windows(2).all(|w| w[1] <= w[0]), "falling from the top toward the ramp: {his:?}");
}

#[test]
fn a_surface_touch_and_a_mouse_touch_share_the_target() {
    let _g = serial();
    let s = vizdaw_ffi::Session::new_demo().unwrap();
    let (synth, k) = synth(&s);
    let id = mcu(&s);
    let ch = k as u8;
    let t = vizdaw_ffi::AutomationTargetRow::StripFader { strip: synth };
    s.inject_surface(id, vec![0x90, 0x68 + ch, 0x7F]).unwrap();
    s.gesture(t, vizdaw_ffi::automation::GesturePhaseRow::Touch { value: None }).unwrap();
    s.inject_surface(id, vec![0x90, 0x68 + ch, 0x00]).unwrap();
    assert!(s.automation_state().unwrap().touched.contains(&t), "the mouse still holds it");
    s.gesture(t, vizdaw_ffi::automation::GesturePhaseRow::Release).unwrap();
    assert!(s.automation_state().unwrap().touched.is_empty());
}

/// Pre-flight C2, on a real unit's touch.
#[test]
fn a_resting_surface_touch_does_not_open_an_edit() {
    let _g = serial();
    let s = vizdaw_ffi::Session::new_demo().unwrap();
    let (_, k) = synth(&s);
    let id = mcu(&s);
    let ch = k as u8;
    let gain = |s: &vizdaw_ffi::Session| s.mixer().unwrap().strips[k].gain_db;
    let before = gain(&s);
    s.inject_surface(id, vec![0x90, 0x68 + ch, 0x7F]).unwrap();
    s.begin_edit("Drag".into()).unwrap();
    s.end_edit().unwrap();
    s.add_bus("Rest".into()).unwrap();
    assert_eq!(s.snapshot().unwrap().undo_label.as_deref(), Some("Add Bus"), "its own step while the hand rests");
    s.inject_surface(id, vec![0xE0 + ch, 0x00, 0x20, 0x90, 0x68 + ch, 0x00]).unwrap();
    assert_eq!(s.snapshot().unwrap().undo_label.as_deref(), Some("Set Fader"));
    s.undo().unwrap();
    assert!((gain(&s) - before).abs() < 1e-9, "Undo restores only the fader");
    assert!(s.mixer().unwrap().strips.iter().any(|x| x.name == "Rest"), "and keeps the bus");
}

#[test]
fn a_surface_touch_is_released_when_its_port_goes() {
    let _g = serial();
    let s = vizdaw_ffi::Session::new_demo().unwrap();
    let (_, k) = synth(&s);
    let id = mcu(&s);
    s.inject_surface(id, vec![0x90, 0x68 + k as u8, 0x7F]).unwrap();
    debug_set_missing(vec!["T In".into()]);
    debug_advance(2_100);
    s.idle().unwrap();
    assert!(s.automation_state().unwrap().touched.is_empty());
}

#[test]
fn a_returning_port_reconnects_with_a_full_refresh() {
    let _g = serial();
    surface_add(Kind::Mcu, "R In".into(), "R Out".into(), None).unwrap();
    let s = vizdaw_ffi::Session::new_demo().unwrap();
    s.focus_surfaces().unwrap();
    debug_advance(30);
    s.idle().unwrap();
    let full = debug_sent("R Out").len();
    assert!(full > 20);
    debug_set_missing(vec!["R Out".into()]);
    debug_advance(2_100);
    s.idle().unwrap();
    debug_set_missing(vec![]);
    debug_advance(2_100);
    s.idle().unwrap();
    let again = debug_sent("R Out");
    assert!(again.len() >= full, "everything again: {}", again.len());
    assert_eq!(again.first(), Some(&vec![0xF0, 0x00, 0x00, 0x66, 0x14, 0x00, 0xF7]));
}

/// Pre-flight I2: the unit forgets its touch at the unplug, so the fader
/// it held moves again after the reconnect.
#[test]
fn a_unit_unplugged_while_touched_gets_fader_feedback_after_reconnect() {
    let _g = serial();
    let s = vizdaw_ffi::Session::new_demo().unwrap();
    let (_, k) = synth(&s);
    let id = mcu(&s);
    let ch = k as u8;
    s.inject_surface(id, vec![0x90, 0x68 + ch, 0x7F]).unwrap();
    debug_set_missing(vec!["T In".into()]);
    debug_advance(2_100);
    s.idle().unwrap();
    assert!(s.automation_state().unwrap().touched.is_empty(), "released at the vanish");
    debug_set_missing(vec![]);
    debug_advance(2_100);
    s.idle().unwrap();
    let sent = debug_sent("T Out");
    assert!(sent.iter().any(|m| m.first() == Some(&(0xE0 + ch))), "the fader held at the unplug moves again: {sent:?}");
}

#[test]
fn removing_a_surface_releases_its_touches() {
    let _g = serial();
    let s = vizdaw_ffi::Session::new_demo().unwrap();
    let (_, k) = synth(&s);
    let id = mcu(&s);
    s.inject_surface(id, vec![0x90, 0x68 + k as u8, 0x7F, 0xB0, 0x10 + k as u8, 0x01]).unwrap();
    assert_eq!(s.automation_state().unwrap().touched.len(), 2, "a touched fader and a V-pot's idle hold");
    surface_remove(id).unwrap();
    s.idle().unwrap();
    assert!(s.automation_state().unwrap().touched.is_empty());
}

/// Pre-flight I1: the old session is owed its releases, and takes them
/// in its own idle although it is no longer focused.
#[test]
fn focus_moving_releases_the_old_sessions_surface_and_learn_touches() {
    let _g = serial();
    let s = vizdaw_ffi::Session::new_demo().unwrap();
    let (synth, k) = synth(&s);
    let id = mcu(&s);
    s.learn_target(vizdaw_ffi::AutomationTargetRow::StripPan { strip: synth }).unwrap();
    s.inject_midi("Knobs".into(), vec![0xB0, 7, 64]).unwrap();
    debug_advance(301);
    s.idle().unwrap();
    let map = s.learn_maps().unwrap()[0].source.clone();
    s.set_learn_pickup(map, false).unwrap();
    s.inject_midi("Knobs".into(), vec![0xB0, 7, 100]).unwrap();
    s.inject_surface(id, vec![0x90, 0x68 + k as u8, 0x7F]).unwrap();
    s.idle().unwrap();
    assert_eq!(s.automation_state().unwrap().touched.len(), 2, "the fader and the learned pan");
    let other = vizdaw_ffi::Session::new_demo().unwrap();
    other.focus_surfaces().unwrap();
    other.idle().unwrap();
    s.idle().unwrap();
    assert!(s.automation_state().unwrap().touched.is_empty(), "focus moved: released");
}

/// Pre-flight I1 and M9: closing the focused window ends its touches and
/// its learn, and the units go blank.
#[test]
fn closing_the_focused_window_releases_and_blanks() {
    let _g = serial();
    let s = vizdaw_ffi::Session::new_demo().unwrap();
    let (synth, k) = synth(&s);
    let id = mcu(&s);
    s.inject_surface(id, vec![0x90, 0x68 + k as u8, 0x7F]).unwrap();
    s.learn_target(vizdaw_ffi::AutomationTargetRow::StripPan { strip: synth }).unwrap();
    let _ = debug_sent("T Out");
    s.close().unwrap();
    let sent = debug_sent("T Out");
    assert!(sent.contains(&vec![0xE1, 0x00, 0x00]), "faders down: {sent:?}");
    assert!(
        !sent.iter().any(|m| m.first() == Some(&(0xE0 + k as u8))),
        "but not the one under the hand (spec §8.1): {sent:?}"
    );
    assert!(
        sent.iter().any(|m| m.starts_with(&[0xF0, 0x00, 0x00, 0x66, 0x14, 0x12, 0x00])
            && m[7..m.len() - 1].iter().all(|b| *b == b' ')),
        "names cleared: {sent:?}"
    );
    assert!(vizdaw_ffi::surfaces::learn_state().waiting.is_none(), "its learn is cancelled");
}

#[test]
fn a_strip_deleted_while_banked_is_skipped() {
    let _g = serial();
    let s = vizdaw_ffi::Session::new_demo().unwrap();
    let id = mcu(&s);
    s.add_bus("Gone".into()).unwrap();
    debug_advance(30);
    s.idle().unwrap();
    let m = s.mixer().unwrap();
    let k = m.strips.iter().position(|x| x.name == "Gone").unwrap();
    assert!(k < 8, "the demo's strips and the bus fit the main unit");
    s.remove_bus(m.strips[k].id).unwrap();
    let before = s.surface_state().unwrap().surfaces[0].ignored;
    s.inject_surface(id, vec![0x90, 0x10 + k as u8, 0x7F]).unwrap();
    assert_eq!(s.surface_state().unwrap().surfaces[0].ignored, before + 1, "its mute does nothing");
}

/// Pre-flight M20: the offset is clamped after a mixer change.
#[test]
fn banks_clamp_after_strips_are_removed() {
    let _g = serial();
    let s = vizdaw_ffi::Session::new_demo().unwrap();
    let id = mcu(&s);
    for i in 0..12 {
        s.add_bus(format!("Extra {i}")).unwrap();
    }
    s.inject_surface(id, vec![0x90, 0x2F, 0x7F]).unwrap();
    debug_advance(30);
    s.idle().unwrap();
    let buses: Vec<u64> =
        s.mixer().unwrap().strips.iter().filter(|x| x.name.starts_with("Extra ")).map(|x| x.id).collect();
    for b in buses {
        s.remove_bus(b).unwrap();
    }
    debug_advance(30);
    s.idle().unwrap();
    let first = s.mixer().unwrap().strips[0].id;
    s.inject_surface(id, vec![0x90, 0x18, 0x7F]).unwrap();
    use vizdaw_ffi::surfaces::SurfaceEventRow as E;
    assert_eq!(s.take_surface_events().unwrap(), vec![E::Select { strip: first }], "back at the first bank");
}

/// Pre-flight M20: units show consecutive strips in position order,
/// whatever order they were added in.
#[test]
fn positions_out_of_order_show_consecutive_strips() {
    let _g = serial();
    let s = vizdaw_ffi::Session::new_demo().unwrap();
    for i in 0..12 {
        s.add_bus(format!("Extra {i}")).unwrap();
    }
    let xt = surface_add(Kind::McuXt, "X In".into(), "X Out".into(), Some(1)).unwrap();
    let main = surface_add(Kind::Mcu, "M In".into(), "M Out".into(), Some(0)).unwrap();
    s.focus_surfaces().unwrap();
    debug_advance(30);
    s.idle().unwrap();
    let strips = s.mixer().unwrap().strips;
    s.inject_surface(main, vec![0x90, 0x18, 0x7F]).unwrap();
    s.inject_surface(xt, vec![0x90, 0x18, 0x7F]).unwrap();
    use vizdaw_ffi::surfaces::SurfaceEventRow as E;
    assert_eq!(
        s.take_surface_events().unwrap(),
        vec![E::Select { strip: strips[0].id }, E::Select { strip: strips[8].id }],
        "the extender at position 1 shows strips 9–16 though it was added first"
    );
}

/// Pre-flight I4: the service reads the meters without taking them from
/// the mixer window.
#[test]
fn surface_meters_leave_the_mixers_peaks() {
    let _g = serial();
    let (s, seq, _) = ramped_bus();
    let _ = mcu(&s);
    s.play(seq).unwrap();
    let mut loud = None;
    wait_until("a strip to sound", || {
        loud = s.levels().unwrap().meters.into_iter().find(|m| m.peak_l.max(m.peak_r) > 0.0).map(|m| m.strip);
        loud.is_some()
    });
    let strip = loud.unwrap();
    let peak = |s: &vizdaw_ffi::Session| {
        s.levels().unwrap().meters.iter().find(|m| m.strip == strip).map_or(0.0, |m| m.peak_l.max(m.peak_r))
    };
    let mut heard = 0;
    for _ in 0..20 {
        std::thread::sleep(std::time::Duration::from_millis(20));
        debug_advance(300);
        s.idle().unwrap();
        if peak(&s) > 0.0 {
            heard += 1;
        }
    }
    s.stop().unwrap();
    assert!(heard >= 18, "the mixer still sees the peaks the surfaces read: {heard} of 20");
}

/// Pre-flight I5 and the owner's answer (Ruling 17): a surface port and
/// a learned control reach no lane; unlearned messages still do.
#[test]
fn surface_ports_and_learned_controls_stay_out_of_the_lanes() {
    use vizdaw_ffi::host::{debug_midi_from_source, drain_offline_log};
    use vizdaw_ffi::{EditCommand, LaneEdit, LaneInputRow};
    let _g = serial();
    let s = vizdaw_ffi::Session::new_demo().unwrap();
    let seq = song(&s);
    let chords = s.lanes(seq).unwrap().into_iter().find(|l| l.name == "Chords").unwrap().id;
    let lane = |e: LaneEdit| s.edit(EditCommand::Lane { edit: e }, None).unwrap();
    lane(LaneEdit::SetInput { seq, lane: chords, input: Some(LaneInputRow::Midi { port: None, channel: None }) });
    lane(LaneEdit::SetFlags { seq, lane: chords, mute: None, solo: None, arm: Some(true) });
    s.play(seq).unwrap();
    wait_until("the engine", || s.transport().unwrap().playing);
    s.stop().unwrap();
    let _ = mcu(&s);
    let (synth, _) = synth(&s);
    s.learn_target(vizdaw_ffi::AutomationTargetRow::StripMute { strip: synth }).unwrap();
    s.inject_midi("Pads".into(), vec![0x99, 36, 100]).unwrap();
    debug_advance(301);
    s.idle().unwrap();
    assert_eq!(s.learn_maps().unwrap().len(), 1, "a pad learned to the synth's mute");
    std::thread::sleep(std::time::Duration::from_millis(50));
    drain_offline_log();
    debug_midi_from_source("T In", 0, &[0x90, 0x68, 0x7F]);
    debug_midi_from_source("T In", 0, &[0xE0, 0x00, 0x40]);
    assert!(s.inject_midi("T In".into(), vec![0x90, 0x5E, 0x7F]).unwrap(), "the surface took it");
    debug_midi_from_source("Pads", 0, &[0x99, 36, 100]);
    assert!(s.inject_midi("Pads".into(), vec![0x89, 36, 0]).unwrap(), "the map took it");
    debug_midi_from_source("Pads", 0, &[0x99, 38, 100]);
    let mut log = Vec::new();
    wait_until("the unlearned pad's thru", || {
        log.extend(drain_offline_log());
        log.iter().any(|(_, b): &(u64, Vec<u8>)| b == &vec![0x90, 38, 100])
    });
    assert!(!log.iter().any(|(_, b)| b.get(1) == Some(&0x68) || b.get(1) == Some(&0x5E)), "nothing from the surface port: {log:?}");
    assert!(!log.iter().any(|(_, b)| b.first().is_some_and(|x| x & 0xF0 == 0xE0)), "no pitch bend: {log:?}");
    assert!(!log.iter().any(|(_, b)| b.get(1) == Some(&36)), "nothing the map takes: {log:?}");
}

/// Pre-flight M18, through the core: a V-pot in Plug-in moves the
/// selected strip's first insert's first parameter (`Source::Ui`).
#[test]
fn a_vpot_in_plugin_mode_moves_the_plugin() {
    use vizdaw_ffi::{PluginFormatTag, PluginRefRow, SlotStatus};
    let _g = serial();
    vizdaw_ffi::catalog::add_test_bundle(&test_plugins::bundle()).unwrap();
    let s = vizdaw_ffi::Session::new_demo().unwrap();
    let synth = s.mixer().unwrap().strips[0].id;
    s.add_insert(
        synth,
        None,
        PluginRefRow {
            format: PluginFormatTag::Clap,
            id: "vizdaw.test.gain".into(),
            name: "Test Gain".into(),
            vendor: "VizDAW".into(),
            version: "1.0.0".into(),
        },
    )
    .unwrap();
    let slot = s.plugin_slots().unwrap()[0].id;
    s.play(song(&s)).unwrap();
    wait_until("the plugin to run", || {
        s.idle().unwrap();
        s.plugin_slots().unwrap()[0].status == SlotStatus::Live
    });
    let gain = |s: &vizdaw_ffi::Session| s.params(slot).unwrap().into_iter().find(|p| p.id == 0).unwrap().value;
    let before = gain(&s);
    let id = mcu(&s);
    s.set_surface_selection(vec![synth]).unwrap();
    s.inject_surface(id, vec![0x90, 0x2B, 0x7F]).unwrap();
    debug_advance(30);
    s.idle().unwrap();
    s.inject_surface(id, vec![0xB0, 0x10, 0x05]).unwrap();
    s.stop().unwrap();
    assert!(gain(&s) > before, "V-pot 1 moved the Test Gain's first parameter");
}

/// Owner answer 2026-11-02 (Ruling 5): a tap skips to the next marker,
/// else bar; a hold winds.
#[test]
fn rewind_and_forward_skip_to_markers_then_bars_and_wind_while_held() {
    use vizdaw_ffi::{EditCommand, MarkerRow, SequenceEdit};
    const BAR: i64 = 4 * 15_360; // the demo is in 4/4
    let _g = serial();
    let s = vizdaw_ffi::Session::new_demo().unwrap();
    let seq = song(&s);
    s.edit(
        EditCommand::Sequence {
            edit: SequenceEdit::SetMarkers { seq, markers: vec![MarkerRow { tick: 2 * BAR, name: "B".into() }] },
        },
        None,
    )
    .unwrap();
    s.play(seq).unwrap();
    wait_until("the engine", || s.transport().unwrap().playing);
    s.stop().unwrap();
    wait_until("the stop", || !s.transport().unwrap().playing);
    s.locate(0).unwrap();
    let id = mcu(&s);
    let at = |tick: i64| wait_until(&format!("tick {tick}"), || s.transport().unwrap().tick == tick);
    at(0);
    let tap = |note: u8| s.inject_surface(id, vec![0x90, note, 0x7F, 0x90, note, 0x00]).unwrap();
    tap(0x5C);
    at(2 * BAR);
    tap(0x5C);
    at(3 * BAR);
    tap(0x5B);
    at(2 * BAR);
    s.inject_surface(id, vec![0x90, 0x5C, 0x7F]).unwrap();
    debug_advance(500);
    s.idle().unwrap();
    at(2 * BAR + 15_360 * 4 / 10);
    s.inject_surface(id, vec![0x90, 0x5C, 0x00]).unwrap();
    // Back: the marker, then bar lines (no marker before), never below 0.
    tap(0x5B);
    at(2 * BAR);
    tap(0x5B);
    at(BAR);
    tap(0x5B);
    at(0);
    tap(0x5B);
    at(0);
}

#[test]
fn malformed_bytes_are_ignored_and_counted() {
    let _g = serial();
    let s = vizdaw_ffi::Session::new_demo().unwrap();
    let id = mcu(&s);
    s.inject_surface(id, vec![0xF0, 0x01, 0x90]).unwrap();
    assert!(s.surface_state().unwrap().surfaces[0].ignored >= 1);
    assert!(s.inject_surface(id, vec![]).is_err(), "no bytes");
    assert!(s.inject_surface(id, vec![0; 70_000]).is_err(), "too many");
    assert!(s.inject_surface(9_999, vec![0x90, 0, 0]).is_err(), "no such surface");
    s.set_surface_selection(vec![987_654]).unwrap();
    assert!(s.set_surface_selection(vec![1; 100_000]).is_err(), "at most 1024 strips");
}
```

`inject_surface` takes at most `core_ffi::live::MAX_INJECT_BYTES` bytes. The wind test's last locate is 0.1 s of winding at 4 quarter notes a second (0.4 × 15360 ticks) after the 400 ms hold. The tests that read the display order (`synth`, `k`, `strips[0]`, `strips[8]`) assume `MixerSnapshot.strips` lists the strips in display order with master last; if the tree orders them otherwise, index the strips as the MixerView does and say so in the report.

Then create `crates/core-ffi/tests/learn.rs`:

```rust
//! MIDI learn in the core (plan 5f-1 Task 8; spec §4.8, §8.2; Rulings 7,
//! 8, 11, 18).

mod common;

use std::sync::{Mutex, MutexGuard};
use vizdaw_ffi::surfaces::{
    LearnModeRow as Mode, cancel_learn, command_maps, debug_advance, debug_reset, learn_command,
    learn_state, remove_command_map,
};
use vizdaw_ffi::{AutomationTargetRow as Target, Session};

static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    vizdaw_ffi::host::force_offline();
    vizdaw_ffi::host::reset_host();
    debug_reset();
    g
}

fn synth_fader(s: &Session) -> Target {
    let id = s.mixer().unwrap().strips.iter().find(|x| x.name == "Built-in Synth").unwrap().id;
    Target::StripFader { strip: id }
}

#[test]
fn a_learnt_cc_rides_its_target_and_is_one_undo_step() {
    let _g = serial();
    let s = Session::new_demo().unwrap();
    s.focus_surfaces().unwrap();
    let t = synth_fader(&s);
    s.learn_target(t).unwrap();
    assert!(learn_state().waiting.is_some());
    assert!(s.inject_midi("Knobs".into(), vec![0xB0, 7, 100]).unwrap(), "learn took it");
    debug_advance(301);
    s.idle().unwrap();
    assert_eq!(s.snapshot().unwrap().undo_label.as_deref(), Some("MIDI Learn"));
    let maps = s.learn_maps().unwrap();
    assert_eq!(maps.len(), 1);
    assert_eq!(maps[0].text, "Knobs · CC 7 ch 1");
    assert!(maps[0].pickup && maps[0].picks_up, "pick-up is on for a new absolute map (Ruling 18)");
    assert!(learn_state().waiting.is_none());
    s.inject_midi("Knobs".into(), vec![0xB0, 7, 0]).unwrap();
    s.idle().unwrap();
    assert!(s.automation_state().unwrap().touched.is_empty(), "far from the fader's value: it waits to pick up");
    s.set_learn_pickup(maps[0].source.clone(), false).unwrap();
    assert!(!s.learn_maps().unwrap()[0].pickup);
    s.inject_midi("Knobs".into(), vec![0xB0, 7, 0]).unwrap();
    s.idle().unwrap();
    assert!(s.automation_state().unwrap().touched.contains(&t));
    debug_advance(501);
    s.idle().unwrap();
    assert!(s.automation_state().unwrap().touched.is_empty(), "released 500 ms after");
    assert_eq!(s.snapshot().unwrap().undo_label.as_deref(), Some("Set Fader"));
    s.set_learn_mode(maps[0].source.clone(), Mode::RelativeBinOffset).unwrap();
    assert_eq!(s.learn_maps().unwrap()[0].mode, Mode::RelativeBinOffset);
    assert!(!s.learn_maps().unwrap()[0].picks_up, "relative maps do not pick up");
    s.unlearn_target(t).unwrap();
    assert_eq!(s.snapshot().unwrap().undo_label.as_deref(), Some("Remove MIDI Learn"));
    assert!(s.learn_maps().unwrap().is_empty());
    assert!(s.remove_learn(maps[0].source.clone()).is_err(), "nothing left to remove");
    assert!(s.unlearn_target(t).is_err());
}

#[test]
fn a_command_map_is_learnt_into_the_app_preferences() {
    let _g = serial();
    let s = Session::new_demo().unwrap();
    s.focus_surfaces().unwrap();
    for pad in [36, 37] {
        learn_command("transport.play_stop".into()).unwrap();
        s.inject_midi("Pads".into(), vec![0x99, pad, 100]).unwrap();
        debug_advance(301);
        s.idle().unwrap();
    }
    assert_eq!(command_maps().len(), 2);
    assert_eq!(command_maps()[0].command, "transport.play_stop");
    s.inject_midi("Pads".into(), vec![0x99, 36, 100]).unwrap();
    s.idle().unwrap();
    use vizdaw_ffi::surfaces::SurfaceEventRow as E;
    assert_eq!(
        s.take_surface_events().unwrap(),
        vec![E::Command { id: "transport.play_stop".into(), programmatic: true }]
    );
    remove_command_map("transport.play_stop".into()).unwrap();
    assert!(command_maps().is_empty(), "every map for the command");
}

/// Pre-flight I10: the map lands in the project that asked, not the
/// focused one.
#[test]
fn a_learn_lands_in_the_session_that_asked() {
    let _g = serial();
    let a = Session::new_demo().unwrap();
    let b = Session::new_demo().unwrap();
    a.focus_surfaces().unwrap();
    a.learn_target(synth_fader(&a)).unwrap();
    b.focus_surfaces().unwrap();
    b.inject_midi("Knobs".into(), vec![0xB0, 7, 100]).unwrap();
    debug_advance(301);
    b.idle().unwrap();
    a.idle().unwrap();
    assert_eq!(a.learn_maps().unwrap().len(), 1, "in the project that asked");
    assert!(b.learn_maps().unwrap().is_empty(), "not in the focused one");
}

fn wait_until(what: &str, mut ok: impl FnMut() -> bool) {
    let until = std::time::Instant::now() + common::wait_limit();
    while !ok() {
        assert!(std::time::Instant::now() < until, "timed out waiting for {what}");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

fn song(s: &Session) -> u64 {
    s.snapshot().unwrap().sequences.iter().find(|r| r.name == "Song").unwrap().id
}

/// Knobs CC 7 learned to the synth's fader, pick-up off (the jump).
fn learned_fader(s: &Session) -> Target {
    let t = synth_fader(s);
    s.learn_target(t).unwrap();
    s.inject_midi("Knobs".into(), vec![0xB0, 7, 64]).unwrap();
    debug_advance(301);
    s.idle().unwrap();
    let source = s.learn_maps().unwrap()[0].source.clone();
    s.set_learn_pickup(source, false).unwrap();
    t
}

/// The owner's rule (2026-11-02), first half: the learned control drives
/// its target as a gesture (holder Learn), so the strip's mode writes it.
#[test]
fn a_learned_cc_writes_its_targets_automation_in_touch() {
    let _g = serial();
    let s = Session::new_demo().unwrap();
    s.focus_surfaces().unwrap();
    let t = learned_fader(&s);
    let Target::StripFader { strip } = t else { unreachable!() };
    let seq = song(&s);
    s.set_automation_mode(vec![strip], vizdaw_ffi::automation::AutomationModeRow::Touch).unwrap();
    s.play(seq).unwrap();
    wait_until("beat 1", || s.transport().unwrap().tick > 15_360);
    for v in [20u8, 40, 60] {
        std::thread::sleep(std::time::Duration::from_millis(12));
        assert!(s.inject_midi("Knobs".into(), vec![0xB0, 7, v]).unwrap(), "the map takes it");
        s.idle().unwrap();
    }
    assert!(s.automation_state().unwrap().touched.contains(&t), "held by Learn");
    debug_advance(501);
    s.idle().unwrap();
    assert!(s.automation_state().unwrap().touched.is_empty(), "released 500 ms after its last value");
    s.stop().unwrap();
    wait_until("the stop", || !s.transport().unwrap().playing);
    s.idle().unwrap();
    assert_eq!(s.snapshot().unwrap().undo_label.as_deref(), Some("Write Automation"));
    let lane = s.lanes(seq).unwrap().into_iter().find(|l| l.name == "Built-in Synth Fader").expect("the fader's lane").id;
    let written: Vec<f64> =
        s.lane_events(seq, lane).unwrap().events.iter().filter_map(|e| e.automation.map(|a| a.value)).collect();
    assert!(written.iter().any(|v| (v - 60.0 / 127.0).abs() < 1e-9), "the learned ride is the lane's: {written:?}");
}

/// The owner's rule (2026-11-02), second half: the learned CC itself is
/// never recorded or played thru; an unlearned note from the same port is.
#[test]
fn a_learned_cc_is_neither_recorded_nor_played_thru() {
    use vizdaw_ffi::host::{debug_midi_from_source, drain_offline_log};
    use vizdaw_ffi::{EditCommand, EventKindRow, LaneEdit, LaneInputRow};
    let _g = serial();
    let s = Session::new_demo().unwrap();
    s.focus_surfaces().unwrap();
    let seq = song(&s);
    let chords = s.lanes(seq).unwrap().into_iter().find(|l| l.name == "Chords").unwrap().id;
    let edit = |e: LaneEdit| s.edit(EditCommand::Lane { edit: e }, None).unwrap();
    edit(LaneEdit::SetInput { seq, lane: chords, input: Some(LaneInputRow::Midi { port: None, channel: None }) });
    edit(LaneEdit::SetFlags { seq, lane: chords, mute: None, solo: None, arm: Some(true) });
    let _ = learned_fader(&s);
    let kinds = |s: &Session| {
        let ev = s.lane_events(seq, chords).unwrap().events;
        (
            ev.iter().filter(|e| e.kind == EventKindRow::Cc).count(),
            ev.iter().filter(|e| e.kind == EventKindRow::Note).count(),
        )
    };
    let (ccs, notes) = kinds(&s);
    s.record(
        seq,
        vizdaw_ffi::record::RecordSpecRow {
            replace: false,
            punch: None,
            looped: false,
            count_in_bars: 0,
            pre_roll_bars: 0,
            passes_as_takes: false,
        },
    )
    .unwrap();
    wait_until("beat 1", || s.transport().unwrap().tick > 15_360);
    drain_offline_log();
    for v in [20u8, 40, 60] {
        assert!(s.inject_midi("Knobs".into(), vec![0xB0, 7, v]).unwrap(), "the map takes it");
        debug_midi_from_source("Knobs", 0, &[0xB0, 7, v + 1]);
        s.idle().unwrap();
    }
    assert!(s.inject_midi("Knobs".into(), vec![0x90, 62, 100]).unwrap());
    let mut log = Vec::new();
    wait_until("the unlearned note's thru", || {
        log.extend(drain_offline_log());
        log.iter().any(|(_, b): &(u64, Vec<u8>)| b == &vec![0x90, 62, 100])
    });
    assert!(s.inject_midi("Knobs".into(), vec![0x80, 62, 0]).unwrap());
    wait_until("a beat on", || s.transport().unwrap().tick > 2 * 15_360);
    s.stop().unwrap();
    wait_until("the stop", || !s.transport().unwrap().playing);
    s.idle().unwrap();
    log.extend(drain_offline_log());
    assert!(
        !log.iter().any(|(_, b)| b.first().is_some_and(|x| x & 0xF0 == 0xB0) && b.get(1) == Some(&7)),
        "no CC 7 thru: {log:?}"
    );
    let (ccs_after, notes_after) = kinds(&s);
    assert_eq!(ccs_after, ccs, "the learned CC is not in the take");
    assert_eq!(notes_after, notes + 1, "the unlearned note is");
}

/// Fold check I5: Undo changes `Settings.learn` without the learn API; the
/// map stops driving its target and taking its messages.
#[test]
fn an_undone_learn_stops_driving_and_taking() {
    let _g = serial();
    let s = Session::new_demo().unwrap();
    s.focus_surfaces().unwrap();
    let t = synth_fader(&s);
    s.learn_target(t).unwrap();
    s.inject_midi("Knobs".into(), vec![0xB0, 7, 100]).unwrap();
    debug_advance(301);
    s.idle().unwrap();
    s.undo().unwrap();
    assert!(s.learn_maps().unwrap().is_empty());
    s.idle().unwrap();
    assert!(!s.inject_midi("Knobs".into(), vec![0xB0, 7, 0]).unwrap(), "no map takes it (and no engine yet)");
    s.idle().unwrap();
    assert!(s.automation_state().unwrap().touched.is_empty());
}

#[test]
fn hostile_learn_calls_are_refused() {
    let _g = serial();
    let s = Session::new_demo().unwrap();
    assert!(s.learn_target(Target::StripFader { strip: 987_654 }).is_err());
    let dest = s.destinations().unwrap()[0].id;
    assert!(s.learn_target(Target::VelocityScale { dest }).is_err(), "an existing destination: only the rule refuses it");
    assert!(learn_command("no.such_command".into()).is_err(), "registry IDs only");
    assert!(learn_command(String::new()).is_err());
    for id in ["automation.gesture", "surface.add"] {
        assert!(learn_command(id.into()).is_err(), "{id}: only commands a key can run");
    }
    assert!(remove_command_map("transport.play_stop".into()).is_err());
    let bad = vizdaw_ffi::surfaces::LearnSourceRow {
        port: "x".into(),
        kind: vizdaw_ffi::surfaces::LearnKindRow::Cc7,
        channel: 99,
        number: 7,
    };
    assert!(s.set_learn_mode(bad.clone(), Mode::Absolute).is_err());
    assert!(s.set_learn_pickup(bad.clone(), true).is_err());
    assert!(s.remove_learn(bad).is_err());
    cancel_learn();
    assert!(learn_state().waiting.is_none());
    let _ = common::wait_limit();
}
```

`a_learned_cc_is_neither_recorded_nor_played_thru` reads the take as `tests/record.rs`' `notes` helper finds a MIDI take; if the take lands elsewhere (a take lane rather than "Chords"), read it there and say so in the report. The target-row variant names (`VelocityScale { dest }`, `StripMute { strip }`) must match `crate::events::AutomationTargetRow` in the tree; adapt if they differ.

- [ ] **Step 2: Run them to see them fail** (`cargo test -p core-ffi --test surfaces --test learn`). Expected: FAIL — the new methods are not found.

- [ ] **Step 3: Implement the service, the claims, the meter holds, the learn glue and the exports**, and change `midi_from_source`, `NoteInput`, `inject_midi`, `levels` and `close` as shown.

- [ ] **Step 4: Run the tests and the gates.** Run `cargo test -p core-ffi`, both clippies and `cargo clippy -p core-ffi --lib -- -D warnings` (it compiles `CoreMidiPorts` and the new `NoteInput` sink on macOS). Then run `scripts/build-ffi.sh`, `scripts/swift.sh build` and `scripts/swift.sh test`: add `FakeSession` stubs for the ten new `Session` methods, recording calls and returning empty rows. Expected: Rust +28; `live_input.rs`'s `!inject_midi(…)` stays false (no surface, learn or map listens to "Keys", and there is no engine yet), and its thru and source tests stay green.

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-ffi/src/surfaces.rs crates/core-ffi/src/surfaces/ports.rs crates/core-ffi/src/session.rs crates/core-ffi/src/host.rs crates/core-ffi/src/input.rs crates/core-ffi/tests/surfaces.rs crates/core-ffi/tests/learn.rs app/Tests/VizDAWAppModelTests/FakeSession.swift
git commit -m "core-ffi: surfaces drive the focused session with heard values, owed releases, feedback every 20 ms with partners at once, meters shared with the mixer, surface and learned MIDI kept out of the lanes, MIDI learn with pick-up (plan 5f-1 Task 8)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -10
```
Counts: Rust +28.

---

### Task 9: The control channel: `transport.play`, `surface.*`, `state.get surfaces`, `SurfaceCore`

**Files:**
- Modify: `crates/core-control/src/registry.rs`, `crates/core-control/src/request.rs`, `crates/core-control/tests/commands.rs`, `crates/core-ffi/src/control.rs`, `app/Sources/VizDAWAppModel/ControlCommands.swift`, `app/Sources/VizDAWAppModel/TransportCommands.swift`, `app/Sources/VizDAW/ControlChannel.swift` (`AppControlHost.surfaces`, `state.get surfaces` without a document), `app/Tests/VizDAWAppModelTests/ControlCommandsTests.swift` (`FakeControlHost.surfaces`), `app/Tests/VizDAWAppModelTests/TransportCommandsTests.swift`, `app/Tests/VizDAWAppModelTests/FakeSession.swift`
- Create: `app/Sources/VizDAWAppModel/SurfaceCommands.swift`, `app/Sources/VizDAW/LiveSurfaceCore.swift`, `app/Tests/VizDAWAppModelTests/SurfaceCommandsTests.swift`

**Interfaces:**
- Consumes: Tasks 7–8.
- Produces:
  - Registry entries:
    - `transport.play` (Transport, "Play", key-only, no default key): plays when stopped, does nothing while playing (Ruling 5);
    - all `key_only`, in menu group "Surfaces": `surface.add { type: mcu|mcu_xt|hui, input, output, position? 0–3 }`; `surface.remove { surface }`; `surface.inject { surface, bytes }` (hex text); `surface.learn { target? | command? }` (exactly one); `surface.unlearn { target? | command? }` (exactly one).
  - `StateTarget::Surfaces` (`"surfaces"`), `StateTargetRow::Surfaces` and `ControlResult::Surfaces { doc: Option<String>, state: SurfaceStateRow }`, whose JSON is:
    `{ "doc", "surfaces": [{ "id", "type", "input", "output", "position", "connected", "online", "ignored" }], "feedback": [{ "surface", "messages", "meters_sent" }], "learning", "problems" }` (`doc` null and `feedback` empty without a document, pre-flight M29).
  - Swift:
    ```swift
    /// The core's process-wide surfaces, command maps and learn state (plan 5f-1
    /// Rulings 9–11, 16). The app passes `LiveSurfaceCore` (the free functions);
    /// tests pass a fake, so no Swift test creates a CoreMIDI client or writes
    /// surfaces.json (pre-flight M25).
    public protocol SurfaceCore: AnyObject {
        func list() -> [SurfaceRow]
        func ports() -> SurfacePortsRow
        func add(_ k: SurfaceKindRow, _ i: String, _ o: String, _ p: UInt8?) throws -> UInt32
        func remove(_ id: UInt32) throws
        /// `state.get surfaces` without a document.
        func state() -> SurfaceStateRow
        func commandMaps() -> [CommandMapRow]
        func removeCommandMap(_ command: String) throws
        func learnCommand(_ id: String) throws
        func cancelLearn()
        func learnState() -> LearnStateRow
    }
    /// Lists nothing and refuses every change ("Surfaces are not available here.").
    public final class NoSurfaceCore: SurfaceCore { public init() }
    @MainActor public enum SurfaceCommands {
        public static let ids: Set<String>
        /// `doc` nil: `surface.add` and `surface.remove` still run; the others are `no_document`.
        public static func run(_ id: String, args: [ControlArg], doc: (UUID, DocumentModel)?, surfaces: any SurfaceCore) -> ControlOutcome
        nonisolated public static func parseKind(_ s: String) -> SurfaceKindRow?
    }
    // ControlHost gains:
    var surfaces: any SurfaceCore { get }
    ```
  - `surface.add` answers `{ "surface": id }` through the `ran` result's status (`"surface 3"`), as `destination.add_midi` answers its ID. Use the same mechanism the tree uses for that.
  - Core refusals (`Model`, `Busy`) are the call's `failed`/`busy` through `ControlFailure.thrown`, as `AutomationCommands` maps them; the app's own argument checks are `invalid_params`.

- [ ] **Step 1: Write the failing tests.** In `crates/core-control/tests/commands.rs`, change the count:

   Find:
   ```rust
           138,
   ```
   Replace:
   ```rust
           144,
   ```
   and add "; 5f-1 adds transport.play, surface.add, surface.remove, surface.inject, surface.learn and surface.unlearn" at the end of the count message. Then add:

```rust
#[test]
fn surface_commands_check_their_params() {
    let run = |id: &str, params: Value| json!({ "id": id, "params": params });
    for (id, params) in [
        ("transport.play", json!({})),
        ("surface.add", json!({ "type": "mcu", "input": "A", "output": "B" })),
        ("surface.add", json!({ "type": "mcu_xt", "input": "A", "output": "B", "position": 3 })),
        ("surface.add", json!({ "type": "hui", "input": "A", "output": "B" })),
        ("surface.remove", json!({ "surface": 1 })),
        ("surface.inject", json!({ "surface": 1, "bytes": "90 68 7f" })),
        ("surface.inject", json!({ "surface": 1, "bytes": "90 68" })),
        ("surface.learn", json!({ "target": "fader:3" })),
        ("surface.learn", json!({ "command": "transport.play_stop" })),
        ("surface.unlearn", json!({ "target": "fader:3" })),
        ("surface.unlearn", json!({ "command": "transport.play_stop" })),
    ] {
        assert!(matches!(ok("command.run", run(id, params.clone())), Command::CommandRun { .. }), "{id} {params}");
    }
    for (id, params) in [
        ("surface.add", json!({ "type": "mackie", "input": "A", "output": "B" })),
        ("surface.add", json!({ "type": "mcu", "input": "A" })),
        ("surface.add", json!({ "type": "mcu", "input": "A", "output": "B", "position": 4 })),
        ("surface.add", json!({ "type": "mcu", "input": 7, "output": "B" })),
        ("surface.remove", json!({ "surface": -1 })),
        ("surface.remove", json!({})),
        ("surface.inject", json!({ "surface": 1 })),
        ("surface.inject", json!({ "surface": 1, "bytes": "x".repeat(5000) })),
        ("surface.learn", json!({ "target": 3 })),
    ] {
        assert!(err("command.run", run(id, params.clone())).0 == ErrorCode::InvalidParams, "{id} {params}");
    }
    assert_eq!(
        ok("state.get", json!({ "target": "surfaces" })),
        Command::StateGet { target: StateTarget::Surfaces, window: None }
    );
}
```

Use the file's existing `ok`/`err` helpers and imports (`err` returns `(ErrorCode, String)`). "90 68" is well-formed hex: the registry passes it, and the core ignores and counts the cut message (spec §12.2; Task 11's e2e checks it). The "exactly one of target and command" rule is checked in the app (below), because the registry has no "one of" kind.

Then create `app/Tests/VizDAWAppModelTests/SurfaceCommandsTests.swift`:

```swift
import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

/// The process-wide surfaces as the tests see them (plan 5f-1 Task 9): no
/// CoreMIDI client, no surfaces.json (pre-flight M25).
final class FakeSurfaceCore: SurfaceCore, @unchecked Sendable {
    var rows: [SurfaceRow] = []
    var addError: VizError?
    var removed: [UInt32] = []
    var stateRow = SurfaceStateRow(surfaces: [], feedback: [], learning: nil, problems: [])
    var maps: [CommandMapRow] = []
    var removedCommands: [String] = []
    var learnCommands: [String] = []
    var learnError: VizError?
    var cancels = 0
    var learnRow = LearnStateRow(waiting: nil, last: nil)
    func list() -> [SurfaceRow] { rows }
    func ports() -> SurfacePortsRow { SurfacePortsRow(sources: ["In A"], destinations: ["Out A"]) }
    func add(_ k: SurfaceKindRow, _ i: String, _ o: String, _ p: UInt8?) throws -> UInt32 {
        if let e = addError { throw e }
        let id = UInt32(rows.count)
        rows.append(SurfaceRow(id: id, kind: k, input: i, output: o, position: p ?? 0, connected: true, online: false, ignored: 0))
        return id
    }
    func remove(_ id: UInt32) throws {
        removed.append(id)
        rows.removeAll { $0.id == id }
    }
    func state() -> SurfaceStateRow { stateRow }
    func commandMaps() -> [CommandMapRow] { maps }
    func removeCommandMap(_ command: String) throws { removedCommands.append(command) }
    func learnCommand(_ id: String) throws {
        if let e = learnError { throw e }
        learnCommands.append(id)
    }
    func cancelLearn() { cancels += 1 }
    func learnState() -> LearnStateRow { learnRow }
}

/// `surface.*` from the channel (plan 5f-1 Task 9).
@MainActor
@Suite struct SurfaceCommandsTests {
    func arg(_ name: String, _ v: String) -> ControlArg { ControlArg(name: name, value: .text(value: v)) }
    func int(_ name: String, _ v: Int64) -> ControlArg { ControlArg(name: name, value: .int(value: v)) }

    @Test func surfaceCommandsCheckTheirArguments() throws {
        let f = FakeSession()
        let m = try DocumentModel(session: f)
        let c = FakeSurfaceCore()
        for (id, args) in [
            ("surface.inject", [int("surface", 1), arg("bytes", "9g")]),
            ("surface.inject", [int("surface", 1), arg("bytes", "")]),
            ("surface.learn", []),
            ("surface.learn", [arg("target", "fader:1"), arg("command", "edit.undo")]),
            ("surface.learn", [arg("target", "fader")]),
            ("surface.unlearn", []),
            ("surface.unlearn", [arg("target", "fader:1"), arg("command", "edit.undo")]),
        ] {
            guard case .failed(let e) = SurfaceCommands.run(id, args: args, doc: (UUID(), m), surfaces: c) else {
                Issue.record("\(id) \(args) ran"); continue
            }
            #expect(e.code == .invalidParams, "\(id)")
        }
        #expect(f.surfaceInjections.isEmpty && f.learnTargets.isEmpty && f.unlearnTargets.isEmpty)
        #expect(c.learnCommands.isEmpty && c.removedCommands.isEmpty)
    }

    @Test func injectAndLearnReachTheSession() throws {
        let f = FakeSession()
        let m = try DocumentModel(session: f)
        let c = FakeSurfaceCore()
        _ = SurfaceCommands.run(
            "surface.inject", args: [int("surface", 2), arg("bytes", "90 68 7f e0 00 40")], doc: (UUID(), m), surfaces: c)
        #expect(f.surfaceInjections.map(\.0) == [2])
        #expect(f.surfaceInjections.first?.1 == Data([0x90, 0x68, 0x7F, 0xE0, 0x00, 0x40]))
        _ = SurfaceCommands.run("surface.inject", args: [int("surface", 2), arg("bytes", "90 68")], doc: (UUID(), m), surfaces: c)
        #expect(f.surfaceInjections.count == 2, "a cut message is the core's to count")
        _ = SurfaceCommands.run("surface.learn", args: [arg("target", "pan:3")], doc: (UUID(), m), surfaces: c)
        #expect(f.learnTargets == [.stripPan(strip: 3)])
        _ = SurfaceCommands.run("surface.learn", args: [arg("command", "edit.undo")], doc: (UUID(), m), surfaces: c)
        #expect(c.learnCommands == ["edit.undo"])
        _ = SurfaceCommands.run("surface.unlearn", args: [arg("target", "pan:3")], doc: (UUID(), m), surfaces: c)
        #expect(f.unlearnTargets == [.stripPan(strip: 3)])
        _ = SurfaceCommands.run("surface.unlearn", args: [arg("command", "edit.undo")], doc: (UUID(), m), surfaces: c)
        #expect(c.removedCommands == ["edit.undo"])
        c.learnError = .Model(message: "“automation.gesture” cannot be learned")
        guard case .failed(let e) = SurfaceCommands.run(
            "surface.learn", args: [arg("command", "automation.gesture")], doc: (UUID(), m), surfaces: c)
        else { Issue.record("a core refusal ran"); return }
        #expect(e.code == .failed, "a core refusal is the call's failure")
    }

    /// Pre-flight M25 and M29: the process-wide commands go through the
    /// host's `SurfaceCore`, with or without a document.
    @Test func addAndRemoveGoThroughTheHostWithoutADocument() throws {
        let c = FakeSurfaceCore()
        let o = SurfaceCommands.run(
            "surface.add", args: [arg("type", "mcu_xt"), arg("input", "In A"), arg("output", "Out A"), int("position", 2)],
            doc: nil, surfaces: c)
        guard case .done(let r) = o else { Issue.record("\(o)"); return }
        #expect(c.rows.map(\.kind) == [.mcuXt] && c.rows.first?.position == 2)
        #expect("\(r)".contains("surface 0"))
        _ = SurfaceCommands.run("surface.remove", args: [int("surface", 0)], doc: nil, surfaces: c)
        #expect(c.removed == [0])
        guard case .failed(let e) = SurfaceCommands.run(
            "surface.inject", args: [int("surface", 0), arg("bytes", "90 68 7f")], doc: nil, surfaces: c)
        else { Issue.record("inject ran without a document"); return }
        #expect(e.code == .noDocument)
        #expect(SurfaceCommands.parseKind("hui") == .hui && SurfaceCommands.parseKind("mackie") == nil)
    }
}
```

Add to `TransportCommandsTests.swift`, after `stopGoesBackToWherePlayingStarted`:

```swift
    /// Plan 5f-1 Ruling 5 (owner answer 2026-11-02): Play only starts.
    @Test func playOnlyStarts() throws {
        let (s, id, m, f) = try demo()
        #expect(failure(run("transport.play", s, id)) == nil)
        #expect(f.state.playing && m.isPlaying)
        f.playError = .Model(message: "played twice")
        #expect(failure(run("transport.play", s, id)) == nil, "Play while playing does nothing")
        #expect(f.state.playing)
        _ = run("transport.stop", s, id)
        f.playError = nil
        #expect(failure(run("transport.play", s, id)) == nil)
        #expect(f.state.playing, "a stopped transport plays")
    }
```

`FakeSession`'s `injectSurface`, `learnTarget` and `unlearnTarget` stubs (Task 8) record into `surfaceInjections: [(UInt32, Data)]`, `learnTargets: [AutomationTargetRow]` and `unlearnTargets: [AutomationTargetRow]`; add the arrays now if Task 8's stubs only returned. Use the tree's `ControlArg` value case names (`.text(value:)`, `.int(value:)` at `276c855`) and its `ControlOutcome` cases; adapt the literals if they differ.

- [ ] **Step 2: Run them to see them fail** (`cargo test -p core-control --test commands`; `scripts/swift.sh test`). Expected: FAIL — count 138, unknown IDs, `SurfaceCore` not found.

- [ ] **Step 3: Implement.** In `registry.rs`, add `transport.play` after `transport.stop`:

   Find:
   ```rust
       cmd("transport.stop", "Transport", "Stop", Some("S"), true),
   ```
   Replace:
   ```rust
       cmd("transport.stop", "Transport", "Stop", Some("S"), true),
       // A surface's Play: only starts (plan 5f-1 Ruling 5).
       key_only(cmd("transport.play", "Transport", "Play", None, true)),
   ```

   Add the `SURFACE_ADD_PARAMS`, `SURFACE_ID_PARAMS`, `SURFACE_INJECT_PARAMS` and `SURFACE_LEARN_PARAMS` constants (the `target` text as `automation.gesture` spells it; `command` text) after `AUTOMATION_GESTURE_PARAMS`. Add the five entries after `automation.suspend`:

   Find:
   ```rust
       CommandSpec {
           params: AUDIO_SET_PARAMS,
   ```
   Replace:
   ```rust
       CommandSpec {
           params: SURFACE_ADD_PARAMS,
           ..key_only(cmd("surface.add", "Surfaces", "Add Surface", None, false))
       },
       CommandSpec {
           params: SURFACE_ID_PARAMS,
           ..key_only(cmd("surface.remove", "Surfaces", "Remove Surface", None, false))
       },
       CommandSpec {
           params: SURFACE_INJECT_PARAMS,
           ..key_only(cmd("surface.inject", "Surfaces", "Inject Surface Bytes", None, true))
       },
       CommandSpec {
           params: SURFACE_LEARN_PARAMS,
           ..key_only(cmd("surface.learn", "Surfaces", "MIDI Learn", None, true))
       },
       CommandSpec {
           params: SURFACE_LEARN_PARAMS,
           ..key_only(cmd("surface.unlearn", "Surfaces", "Remove MIDI Learn", None, true))
       },
       CommandSpec {
           params: AUDIO_SET_PARAMS,
   ```

   In `request.rs`, add `Surfaces` to `StateTarget` (doc "Control surfaces and MIDI learn (plan 5f-1)"):

   Find:
   ```rust
                   "automation" => StateTarget::Automation,
   ```
   Replace:
   ```rust
                   "automation" => StateTarget::Automation,
                   "surfaces" => StateTarget::Surfaces,
   ```
   and add ", surfaces" to the error text before " or automation" (it becomes "… input, automation or surfaces"). Mirror this in `control.rs`: `StateTargetRow::Surfaces`, the `StateTarget::Surfaces => StateTargetRow::Surfaces` arm after the Automation one, `ControlResult::Surfaces` and its JSON after `ControlResult::Automation`'s.

   In `TransportCommands.swift`:

   Find:
   ```swift
               "transport.stop", "transport.play_selection", "transport.play_from_selection",
   ```
   Replace:
   ```swift
               "transport.stop", "transport.play", "transport.play_selection", "transport.play_from_selection",
   ```

   Find:
   ```swift
           case "transport.stop":
               m.poll()
               guard m.isPlaying else { return .unavailable("Nothing is playing.") }
   ```
   Replace:
   ```swift
           case "transport.play":
               // Play only starts (plan 5f-1 Ruling 5): while playing it does
               // nothing; a stopped transport plays from where it stands.
               m.poll()
               guard !m.isPlaying else { return .done }
               guard m.canPlay else { return .unavailable("Select a sequence to play.") }
               let started = attempt { try m.startPlaying() }
               m.poll()
               return started
           case "transport.stop":
               m.poll()
               guard m.isPlaying else { return .unavailable("Nothing is playing.") }
   ```

   In `ControlCommands.swift`, the surface commands run before the document guard (`surface.add` and `surface.remove` need none):

   Find:
   ```swift
           case AutomationCommands.suspend:
               return AutomationCommands.runSuspend(args)
   ```
   Replace:
   ```swift
           case AutomationCommands.suspend:
               return AutomationCommands.runSuspend(args)
           case _ where SurfaceCommands.ids.contains(id):
               // Process-wide or per document (plan 5f-1 Task 9).
               return SurfaceCommands.run(id, args: args, doc: doc, surfaces: host.surfaces)
   ```
   `handled` gains `.union(SurfaceCommands.ids)`. `ControlHost` gains `var surfaces: any SurfaceCore { get }`. In `ControlStates.state`:

   Find:
   ```swift
           case .automation:
               guard let s = try? m.session.automationState() else { return nil }
               return .automation(doc: doc, state: s)
   ```
   Replace:
   ```swift
           case .automation:
               guard let s = try? m.session.automationState() else { return nil }
               return .automation(doc: doc, state: s)
           case .surfaces:
               guard let s = try? m.session.surfaceState() else { return nil }
               return .surfaces(doc: doc, state: s)
   ```

   In `ControlChannel.swift`, `state.get surfaces` without a document answers from the host (M29):

   Find:
   ```swift
               } else {
                   answer(server, token, .failed(.of(.noDocument, "No document is open in that window.")))
               }
   ```
   Replace:
   ```swift
               } else if target == .surfaces {
                   // Surfaces are app-wide (plan 5f-1, pre-flight M29).
                   answer(server, token, .done(.surfaces(doc: nil, state: AppControlHost.shared.surfaces.state())))
               } else {
                   answer(server, token, .failed(.of(.noDocument, "No document is open in that window.")))
               }
   ```
   and:

   Find:
   ```swift
       static let shared = AppControlHost()
   ```
   Replace:
   ```swift
       static let shared = AppControlHost()
       /// The live surfaces (plan 5f-1; tests pass a fake, pre-flight M25).
       let surfaces: any SurfaceCore = LiveSurfaceCore()
   ```

   Write `LiveSurfaceCore.swift` (in the `VizDAW` target): each method calls its free function (`surfaceList()`, `surfacePorts()`, `surfaceAdd(…)`, `surfaceRemove(id:)`, `surfacesState()`, `commandMaps()`, `removeCommandMap(command:)`, `learnCommand(id:)`, `cancelLearn()`, `learnState()`). In `ControlCommandsTests.swift`:

   Find:
   ```swift
       var nudge = NudgeSettings()
   ```
   Replace:
   ```swift
       var nudge = NudgeSettings()
       var surfaces: any SurfaceCore = FakeSurfaceCore()
   ```

   `SurfaceCommands.run` checks every argument first (`invalid_params` naming the parameter): `surface` an ID, `bytes` through `ControlLanes.hexBytes` (non-hex, empty or over 1024 bytes refused; any hex goes to the core, which ignores and counts malformed MIDI), exactly one of `target` and `command`, `target` spelt as `automation.gesture` spells it.

- [ ] **Step 4: Run the tests and the gates.** Expected: Rust +1 (core-control), Swift +4, registry 144, and `everyRegisteredMenuCommandHasAHandler` green.

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-control/src/registry.rs crates/core-control/src/request.rs crates/core-control/tests/commands.rs crates/core-ffi/src/control.rs app/Sources/VizDAWAppModel/ControlCommands.swift app/Sources/VizDAWAppModel/TransportCommands.swift app/Sources/VizDAWAppModel/SurfaceCommands.swift app/Sources/VizDAW/ControlChannel.swift app/Sources/VizDAW/LiveSurfaceCore.swift app/Tests/VizDAWAppModelTests/SurfaceCommandsTests.swift app/Tests/VizDAWAppModelTests/ControlCommandsTests.swift app/Tests/VizDAWAppModelTests/TransportCommandsTests.swift app/Tests/VizDAWAppModelTests/FakeSession.swift
git commit -m "control: transport.play, surface.add, surface.remove, surface.inject, surface.learn, surface.unlearn and state.get surfaces, through the host's SurfaceCore (plan 5f-1 Task 9)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -15
```
Counts: Rust +1, Swift +4.

---

### Task 10: The app: Settings › Surfaces, loading at launch, focus, the event relay, the input picker

**Files:**
- Create: `app/Sources/VizDAWAppModel/SurfaceSettings.swift`, `app/Sources/VizDAWAppModel/SurfaceRelay.swift`, `app/Sources/VizDAW/SurfacesSettingsView.swift`, `app/Tests/VizDAWAppModelTests/SurfaceSettingsTests.swift`
- Modify: `app/Sources/VizDAW/VizDAWApp.swift` (the tab, `surfacesLoad` at launch, focus in the ticker when the command target changes, the command sinks, `LaneInputs.claimedPorts`), `app/Sources/VizDAWAppModel/DocumentModel.swift` (drain events into the relay; report selection; `inputLane(path:)`; `surfaceCore`), `app/Sources/VizDAWAppModel/MixerModel.swift` (`inputLane(for:)` through `DocumentModel`), `app/Sources/VizDAWAppModel/LaneInputs.swift` (claimed ports), `app/Tests/VizDAWAppModelTests/LaneInputsTests.swift`, `app/Tests/VizDAWAppModelTests/FakeSession.swift` (`locates`)

**Interfaces:**
- Consumes: Task 9's `SurfaceCore`; the `Session` methods of Tasks 7–8.
- Produces:
  ```swift
  /// The core's surfaces as Settings › Surfaces edits them (Rulings 9–11).
  @MainActor @Observable public final class SurfaceSettings {
      public init(core: any SurfaceCore)
      public private(set) var rows: [SurfaceRow]
      public private(set) var ports: SurfacePortsRow
      public private(set) var commandMaps: [CommandMapRow]
      public var alert: AlertItem?
      public func refresh()            // Settings polls it at 2 Hz while open
      public func add(_ kind: SurfaceKindRow, input: String, output: String, position: UInt8?)
      public func remove(_ id: UInt32)
      /// "MCU-compatible", "MCU XT-compatible", "HUI-compatible".
      nonisolated public static func title(_ k: SurfaceKindRow) -> String
      /// "Connected", "Connected · not answering", "Disconnected".
      nonisolated public static func status(_ r: SurfaceRow) -> String
  }
  /// What the app does with surface events (Ruling 12).
  @MainActor public final class SurfaceRelay {
      public init(now: @escaping () -> ContinuousClock.Instant = { ContinuousClock.now })
      /// How long jog detents are summed into one locate.
      public static let jogWindow: Duration = .milliseconds(150)
      /// Select and arm on `m`; a device's commands through `perform`, programmatic ones through `quietly`; jog detents summed.
      public func apply(_ events: [SurfaceEventRow], _ m: DocumentModel, perform: (String) -> Void, quietly: (String) -> Void)
      /// Locates by the summed detents once `jogWindow` passed since the first unapplied one (DocumentModel.idle calls it).
      public func flush(_ m: DocumentModel)
  }
  // DocumentModel gains:
  public var surfaceRelay: SurfaceRelay                 // lazy
  /// A real device's command (`KeyMonitor.perform`) and a programmatic one (`ControlCommands.run`, no dialog), set by the app where models are made.
  public var surfaceCommandSink: ((String) -> Void)?
  public var quietCommandSink: ((String) -> Void)?
  /// NoSurfaceCore until the app passes LiveSurfaceCore (pre-flight M25).
  public var surfaceCore: any SurfaceCore
  /// Of the shown sequence's lanes that play through `path`, the first armed one, else the first (5d-2 Ruling 4).
  public func inputLane(path: UInt64) -> LaneRow?
  // LaneInputs gains:
  /// The surfaces' input ports (the app sets it; tests replace it). Listed "<name> (used by a surface)".
  public static var claimedPorts: () -> Set<String> = { [] }
  ```

Wiring:
- `DocumentModel.idle()` drains `session.takeSurfaceEvents()` into `surfaceRelay.apply(…, perform: surfaceCommandSink ?? { _ in }, quietly: quietCommandSink ?? { _ in })`, then calls `surfaceRelay.flush(self)`.
- `selectedStrips`' `didSet` calls `session.setSurfaceSelection`.
- `MixerModel.inputLane(for:)` returns `document.inputLane(path:)` for an AudioPath strip, nil otherwise.
- The relay's `arm(strip, path)` toggles `inputLane(path:)`'s arm through `m.editor.edit(.lane(edit: .setFlags(seq:lane:mute: nil, solo: nil, arm: !lane.arm)))`, as the lane header does; `path: nil` does nothing.
- In `PluginTicker`, after the idle loop, the ticker calls `focusSurfaces()` on `store.noteTarget` when the command target changed since the last tick (Ruling 11; pre-flight M22).
- `AppSetup.openCatalog()` calls `try? surfacesLoad()` after `setSupportDir`.
- The app sets `LaneInputs.claimedPorts = { Set(surfaceList().map(\.input)) }` at launch, and, where the store's window glue makes models (`ControlWindows.openDocument` or `DocumentStore`'s creation hook; name the place in the report), each model's `surfaceCore = AppControlHost.shared.surfaces`, `surfaceCommandSink` (`KeyMonitor.perform(.command(id), (id: docID, model: m))`) and `quietCommandSink` (`ControlCommands.run(id, args: [], doc: (docID, m), store: appStore, host: AppControlHost.shared)`; a `.failed` outcome's message goes to `m.editor.status`, and nothing opens).
- The view has a list, the Add sheet (type picker, input and output pickers from `ports`, position stepper 0–3 for MCU XT) and Remove. It also shows the command maps (Task 11 adds their learn and remove).

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Tests/VizDAWAppModelTests/SurfaceSettingsTests.swift <<'VIZDAW_EOF'
import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

/// A clock the relay's jog window runs on.
@MainActor final class RelayClock {
    var now = ContinuousClock.now
}

/// Settings › Surfaces and the event relay (plan 5f-1 Task 10).
@MainActor
@Suite struct SurfaceSettingsTests {
    @Test func addListAndRemove() {
        let c = FakeSurfaceCore()
        let s = SurfaceSettings(core: c)
        s.add(.mcu, input: "In A", output: "Out A", position: nil)
        #expect(s.rows.map(\.kind) == [.mcu])
        #expect(SurfaceSettings.title(.mcuXt) == "MCU XT-compatible")
        #expect(SurfaceSettings.status(s.rows[0]) == "Connected · not answering")
        s.remove(s.rows[0].id)
        #expect(s.rows.isEmpty)
    }

    @Test func aRefusedAddIsAnAlert() {
        let c = FakeSurfaceCore()
        c.addError = .Model(message: "VizDAW drives one MCU-compatible main unit and up to three extenders")
        let s = SurfaceSettings(core: c)
        s.add(.mcu, input: "In A", output: "Out A", position: nil)
        #expect(s.alert?.title == "Add Surface Failed")
    }

    @Test func eventsSelectJogAndRunCommands() throws {
        let f = mixerFake()
        let m = try DocumentModel(session: f)
        let clock = RelayClock()
        let relay = SurfaceRelay(now: { clock.now })
        var performed: [String] = []
        var quiet: [String] = []
        let start = m.transport.tick
        relay.apply(
            [.command(id: "transport.stop", programmatic: false), .select(strip: 2), .jog(detents: 4)], m,
            perform: { performed.append($0) }, quietly: { quiet.append($0) })
        #expect(performed == ["transport.stop"] && quiet.isEmpty)
        #expect(m.selectedStrips == [2])
        clock.now += SurfaceRelay.jogWindow
        relay.flush(m)
        #expect(f.locates == [start + Int64(4) * Int64(MeterMath.ppq) / 4])
    }

    /// Pre-flight M3: a spun wheel is one locate (each locate lands a
    /// writing pass).
    @Test func jogsWithin150MsAreOneLocate() throws {
        let f = mixerFake()
        let m = try DocumentModel(session: f)
        let clock = RelayClock()
        let relay = SurfaceRelay(now: { clock.now })
        let start = m.transport.tick
        relay.apply([.jog(detents: 2)], m, perform: { _ in }, quietly: { _ in })
        clock.now += .milliseconds(100)
        relay.apply([.jog(detents: 3)], m, perform: { _ in }, quietly: { _ in })
        relay.flush(m)
        #expect(f.locates.isEmpty, "still inside the window")
        clock.now += .milliseconds(60)
        relay.flush(m)
        #expect(f.locates == [start + Int64(5) * Int64(MeterMath.ppq) / 4], "one locate for the spin")
    }

    /// Pre-flight M21: arm needs no mixer window.
    @Test func armTogglesTheStripsInputLane() throws {
        let f = demoFake()
        var vox = lane(40, "Vox")
        vox.audioOut = 7
        f.laneTable[3] = [vox]
        let s = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { f }, open: { _ in demoFake() })
        let m = try #require(s.newDemo().flatMap(s.model))
        m.select(3)
        #expect(m.inputLane(path: 7)?.id == 40)
        let n = f.edits.count
        SurfaceRelay().apply([.arm(strip: 9, path: 7), .arm(strip: 10, path: nil)], m, perform: { _ in }, quietly: { _ in })
        #expect(f.edits.count == n + 1, "a strip that is no AudioPath does nothing")
        #expect(f.edits.last == .lane(edit: .setFlags(seq: 3, lane: 40, mute: nil, solo: nil, arm: true)))
    }

    /// Pre-flight I9: a command `surface.inject` or `midi.inject` caused
    /// never takes the key path, which would open the Save panel.
    @Test func anInjectedSaveOpensNoDialog() throws {
        let m = try DocumentModel(session: mixerFake())
        var performed: [String] = []
        var quiet: [String] = []
        SurfaceRelay().apply(
            [.command(id: "file.save", programmatic: true), .command(id: "edit.undo", programmatic: false)], m,
            perform: { performed.append($0) }, quietly: { quiet.append($0) })
        #expect(quiet == ["file.save"])
        #expect(performed == ["edit.undo"], "a real button still runs as a key")
    }
}
VIZDAW_EOF
```

Add to `LaneInputsTests.swift`, after `midiChoicesAreSourcesAndChannels`:

```swift
    /// Plan 5f-1 Ruling 17: a surface's port hears nothing in a lane, and
    /// the picker says why.
    @Test func aSurfacesPortIsMarkedInThePicker() throws {
        LaneInputs.midiPorts = { ["Keystation", "MCU In"] }
        LaneInputs.claimedPorts = { ["MCU In"] }
        defer {
            LaneInputs.midiPorts = { midiSourcePorts() }
            LaneInputs.claimedPorts = { [] }
        }
        var l = lane(31, "Keys")
        l.midiDest = 1
        let (m, _) = try doc([l])
        let g = titles(m.inputs.groups(for: l))
        #expect(g["MIDI Input"] == ["All MIDI Inputs", "Keystation", "MCU In (used by a surface)"])
    }
```

`mixerFake()` is the existing helper in `MixerModelTests.swift`, and `lane(_:_:)` the shared lane-row helper `LaneInputsTests` uses. `locates` is FakeSession's record of `locate(tick:)`, added in this task:

   Find (`FakeSession.swift`):
   ```swift
       func locate(tick: Int64) throws { state.tick = tick }
   ```
   Replace:
   ```swift
       /// `locate` calls (plan 5f-1 Task 10).
       var locates: [Int64] = []
       func locate(tick: Int64) throws {
           locates.append(tick)
           state.tick = tick
       }
   ```

- [ ] **Step 2: Run them to see them fail** (`scripts/swift.sh test`). Expected: FAIL — `SurfaceSettings` not found.

- [ ] **Step 3: Implement.** Write the two model files and the view. In `MixerModel.swift`:

   Find:
   ```swift
           let through = document.lanes.filter { $0.audioOut == path }
           return through.first(where: \.arm) ?? through.first
   ```
   Replace:
   ```swift
           // One rule for the mixer and the surfaces' rec buttons (plan 5f-1
           // Ruling 12).
           return document.inputLane(path: path)
   ```
   with `DocumentModel.inputLane(path:)` holding the two removed lines.

   In `LaneInputs.swift`:

   Find:
   ```swift
       public static var midiPorts: () -> [String] = { midiSourcePorts() }
   ```
   Replace:
   ```swift
       public static var midiPorts: () -> [String] = { midiSourcePorts() }
       /// The surfaces' input ports, which reach no lane (plan 5f-1 Ruling
       /// 17); the app sets it, tests replace it.
       public static var claimedPorts: () -> Set<String> = { [] }
   ```

   Find:
   ```swift
                   + Self.midiPorts().map { InputChoice($0, .midi(port: $0, channel: channel)) }
   ```
   Replace:
   ```swift
                   + Self.midiPorts().map { p in
                       InputChoice(
                           claimed.contains(p) ? "\(p) (used by a surface)" : p, .midi(port: p, channel: channel))
                   }
   ```
   with `let claimed = Self.claimedPorts()` before `let ports`.

   In `VizDAWApp.swift`:

   Find:
   ```swift
                   AutomationSettingsView(prefs: automationPreferences)
                       .tabItem { Label("Automation", systemImage: "slider.vertical.3") }
   ```
   Replace:
   ```swift
                   AutomationSettingsView(prefs: automationPreferences)
                       .tabItem { Label("Automation", systemImage: "slider.vertical.3") }
                   SurfacesSettingsView(settings: surfaceSettings, keys: keyBindings)
                       .tabItem { Label("Surfaces", systemImage: "dial.medium") }
   ```

   Find:
   ```swift
       private static var ticks = 0
   ```
   Replace:
   ```swift
       private static var ticks = 0
       /// The command target the surfaces were last focused on (plan 5f-1
       /// Ruling 11): focus moves only when it changes (pre-flight M22).
       private static var surfaceFocus: ObjectIdentifier?
   ```

   Find:
   ```swift
                   for model in store.documents.values { model.idle() }
   ```
   Replace:
   ```swift
                   for model in store.documents.values { model.idle() }
                   // Surfaces drive the command target (plan 5f-1 Ruling 11),
                   // refocused only when it changes, so `surface.inject` into
                   // another window keeps the focus it took.
                   let target = store.noteTarget.map(ObjectIdentifier.init)
                   if target != surfaceFocus {
                       surfaceFocus = target
                       try? store.noteTarget?.session.focusSurfaces()
                   }
   ```

   Find:
   ```swift
           try? setSupportDir(path: dir.path)
   ```
   Replace:
   ```swift
           try? setSupportDir(path: dir.path)
           // surfaces.json (plan 5f-1 Ruling 9); lanes mark surface ports.
           try? surfacesLoad()
           LaneInputs.claimedPorts = { Set(surfaceList().map(\.input)) }
   ```

   Add `@MainActor let surfaceSettings = SurfaceSettings(core: AppControlHost.shared.surfaces)` beside `editingPreferences` in `VizDAWApp.swift`. Set each model's `surfaceCore`, `surfaceCommandSink` and `quietCommandSink` where the store's window glue makes models, as Wiring says.

- [ ] **Step 4: Run the tests and the gates.** Expected: Swift +7.

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add app/Sources/VizDAWAppModel/SurfaceSettings.swift app/Sources/VizDAWAppModel/SurfaceRelay.swift app/Sources/VizDAWAppModel/DocumentModel.swift app/Sources/VizDAWAppModel/MixerModel.swift app/Sources/VizDAWAppModel/LaneInputs.swift app/Sources/VizDAW/SurfacesSettingsView.swift app/Sources/VizDAW/VizDAWApp.swift app/Tests/VizDAWAppModelTests/SurfaceSettingsTests.swift app/Tests/VizDAWAppModelTests/LaneInputsTests.swift app/Tests/VizDAWAppModelTests/FakeSession.swift
git commit -m "app: Settings > Surfaces, surfaces loaded at launch, the command target drives them, surface buttons select, arm, jog and run commands without dialogs, surface ports marked in the input picker (plan 5f-1 Task 10)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -12
```
Counts: Swift +7.

---

### Task 11: The learn UI (with pick-up), and the surface e2e

**Files:**
- Create: `app/Sources/VizDAWAppModel/LearnModel.swift`, `app/Sources/VizDAW/LearnMapsSheet.swift`, `app/Tests/VizDAWAppModelTests/LearnModelTests.swift`
- Modify: `app/Sources/VizDAWAppModel/DocumentModel.swift` (`learn`), `app/Sources/VizDAWAppModel/PluginWindowModel.swift` (the last-touched parameter), `app/Sources/VizDAW/MixerView.swift` (a "MIDI Learn" item in each fader, pan, mute, solo and send control's context menu; the strip menu's "MIDI Learn Maps…"), `app/Sources/VizDAW/PluginWindows.swift` (a header "Learn" button acting on the last parameter touched in the window), `app/Sources/VizDAW/StripChartPane.swift` (a "MIDI Learn" item in the target menu), `app/Sources/VizDAW/KeysSettingsView.swift` ("Learn MIDI" next to the key recorder), `app/Sources/VizDAW/SurfacesSettingsView.swift` (the command maps' Remove), `app/Tests/VizDAWAppModelTests/FakeSession.swift`, `crates/vizctl/tests/e2e.rs`

**Interfaces:**
- Produces:
  ```swift
  @MainActor @Observable public final class LearnModel {
      public init(document: DocumentModel, core: any SurfaceCore)
      /// "Waiting for MIDI… move a control (Esc cancels)"; nil when not learning.
      public private(set) var banner: String?
      public private(set) var maps: [LearnMapRow]
      public var alert: AlertItem?
      public func learn(_ t: AutomationTargetRow)
      public func learnCommand(_ id: String)     // disabled without a document (Ruling 11)
      public func cancel()
      public func poll()                          // the 60 Hz ticker, while learning or the sheet is open
      public func setMode(_ m: LearnMapRow, _ mode: LearnModeRow)
      /// Ruling 18; does nothing for a map whose kind or mode does not pick up.
      public func setPickup(_ m: LearnMapRow, _ on: Bool)
      public func remove(_ m: LearnMapRow)
      nonisolated public static func modeTitle(_ m: LearnModeRow) -> String  // "Absolute", "Relative (2's complement)", "Relative (binary offset)"
      /// "The control takes over only once it reaches the current value".
      nonisolated public static let pickupHelp: String
  }
  ```
  - `DocumentModel.learn: LearnModel` (lazy, with `surfaceCore`).
  - `PluginWindowModel` gains `public private(set) var lastTouched: (slot: UInt64, param: UInt64)?`, set wherever the window reports a parameter gesture (it has none at `276c855`).

The menu item titles are "MIDI Learn" (controls) and "MIDI Learn Maps…" (strip menu). The banner shows at the top of the mixer window and the plugin window while learning. The MIDI Learn Maps sheet lists each map (source text, target or "(missing)", the mode picker, a "Pick-up" checkbox enabled when `picksUp`, with `pickupHelp` as its help, and Remove).

- [ ] **Step 1: Write the failing tests.** Create `app/Tests/VizDAWAppModelTests/LearnModelTests.swift`:

```swift
import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

/// The learn UI's model (plan 5f-1 Task 11; Rulings 7, 8, 11, 18).
@MainActor
@Suite struct LearnModelTests {
    func model() throws -> (LearnModel, FakeSession, FakeSurfaceCore) {
        let f = mixerFake()
        let m = try DocumentModel(session: f)
        let c = FakeSurfaceCore()
        return (LearnModel(document: m, core: c), f, c)
    }

    func row(_ mode: LearnModeRow = .absolute, pickup: Bool = true, picksUp: Bool = true) -> LearnMapRow {
        LearnMapRow(
            source: LearnSourceRow(port: "Knobs", kind: .cc7, channel: 0, number: 7), text: "Knobs · CC 7 ch 1",
            target: .stripFader(strip: 1), missing: false, mode: mode, pickup: pickup, picksUp: picksUp)
    }

    @Test func learningShowsABannerUntilTheMapLands() throws {
        let (l, f, c) = try model()
        c.learnRow = LearnStateRow(waiting: "Fader 1", last: nil)
        l.learn(.stripFader(strip: 1))
        #expect(f.learnTargets == [.stripFader(strip: 1)])
        #expect(l.banner == "Waiting for MIDI… move a control (Esc cancels)")
        f.learnMapRows = [row()]
        c.learnRow = LearnStateRow(waiting: nil, last: "Knobs · CC 7 ch 1")
        l.poll()
        #expect(l.banner == nil)
        #expect(l.maps.map(\.text) == ["Knobs · CC 7 ch 1"])
    }

    @Test func cancelEndsTheLearn() throws {
        let (l, _, c) = try model()
        c.learnRow = LearnStateRow(waiting: "Undo", last: nil)
        l.learnCommand("edit.undo")
        #expect(c.learnCommands == ["edit.undo"] && l.banner != nil)
        l.cancel()
        #expect(c.cancels == 1 && l.banner == nil)
    }

    @Test func modesAndRemovalReachTheSession() throws {
        let (l, f, _) = try model()
        let r = row()
        l.setMode(r, .relativeBinOffset)
        #expect(f.learnModes.map(\.1) == [.relativeBinOffset] && f.learnModes.first?.0 == r.source)
        l.remove(r)
        #expect(f.removedLearns == [r.source])
        f.learnError = .Model(message: "unknown learn source")
        l.setMode(r, .absolute)
        #expect(l.alert?.title == "MIDI Learn Failed")
    }

    /// Owner answer 2026-11-02 (Ruling 18): pick-up per map, set in the
    /// sheet, only where it applies.
    @Test func pickUpIsShownAndSet() throws {
        let (l, f, _) = try model()
        l.setPickup(row(), false)
        #expect(f.learnPickups.map(\.1) == [false])
        l.setPickup(row(.relativeBinOffset, pickup: false, picksUp: false), true)
        #expect(f.learnPickups.count == 1, "a relative map has no pick-up")
        #expect(LearnModel.pickupHelp == "The control takes over only once it reaches the current value")
    }

    @Test func modeTitles() {
        #expect(LearnModel.modeTitle(.absolute) == "Absolute")
        #expect(LearnModel.modeTitle(.relative2sComplement) == "Relative (2's complement)")
        #expect(LearnModel.modeTitle(.relativeBinOffset) == "Relative (binary offset)")
    }
}
```

`FakeSession`'s learn stubs (Task 8) now record: `learnMaps()` returns `learnMapRows`; `setLearnMode`, `setLearnPickup` and `removeLearn` throw `learnError` when set, else append to `learnModes: [(LearnSourceRow, LearnModeRow)]`, `learnPickups: [(LearnSourceRow, Bool)]` and `removedLearns: [LearnSourceRow]` and return the snapshot.

  Then add the e2e to `crates/vizctl/tests/e2e.rs`:

```rust
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn drives_a_fader_and_a_button_from_an_mcu() {
    let mut app = App::start();
    // Pre-flight M29: surfaces are app-wide, so `state.get surfaces` answers without a document.
    let st = app.call("state.get", json!({"target": "surfaces"}));
    assert!(st["doc"].is_null() && st["feedback"].as_array().is_some_and(|f| f.is_empty()), "{st}");
    let w = app.call("app.new_demo", json!({}))["window"].clone();
    app.wait(&w, json!({"condition": "documents_at_least", "count": 1}));
    let run = |app: &mut App, id: &str, params: Value| {
        app.call("command.run", json!({"id": id, "params": params, "window": w}))
    };
    for (params, code) in [
        (json!({"type": "mcu", "input": "", "output": "E2E Out"}), core_control::ErrorCode::Failed),
        (json!({"type": "nope", "input": "a", "output": "b"}), core_control::ErrorCode::InvalidParams),
    ] {
        let e = app.call_err("command.run", json!({"id": "surface.add", "params": params, "window": w}));
        assert_eq!(e.code, code, "{e}");
    }
    run(&mut app, "surface.add", json!({"type": "mcu", "input": "E2E In", "output": "E2E Out"}));
    let st = app.call("state.get", json!({"target": "surfaces", "window": w}));
    assert_eq!(st["surfaces"][0]["connected"], true, "{st}");
    let id = st["surfaces"][0]["id"].as_u64().unwrap();
    let auto = app.call("state.get", json!({"target": "automation", "window": w}));
    let synth = auto["modes"][0]["strip"].as_u64().unwrap();
    // The synth is the demo's first strip: fader 1, mute 1.
    run(&mut app, "surface.inject", json!({"surface": id, "bytes": "90 68 7f e0 00 20"}));
    let auto = app.call("state.get", json!({"target": "automation", "window": w}));
    assert_eq!(auto["touched"][0], format!("fader:{synth}"), "{auto}");
    run(&mut app, "surface.inject", json!({"surface": id, "bytes": "90 68 00"}));
    wait_for_state(&mut app, &w, "document", |d| d["undo"] == "Set Fader");
    run(&mut app, "surface.inject", json!({"surface": id, "bytes": "90 10 7f"}));
    let st = wait_for_state(&mut app, &w, "surfaces", |s| {
        s["feedback"][0]["messages"].as_array().is_some_and(|m| m.iter().any(|x| x == "90 10 7f"))
    });
    let messages = st["feedback"][0]["messages"].as_array().unwrap();
    assert!(
        messages.iter().any(|x| x == "e0 00 20"),
        "the fader got the ride's value (0.25) once, after the release: {st}"
    );
    assert!(messages.iter().all(|x| x.as_str().is_some_and(|t| !t.starts_with("d0 "))), "meters are counted, not listed");
    let ignored = st["surfaces"][0]["ignored"].as_u64().unwrap();
    run(&mut app, "surface.inject", json!({"surface": id, "bytes": "90 68"}));
    let st = app.call("state.get", json!({"target": "surfaces", "window": w}));
    assert_eq!(st["surfaces"][0]["ignored"].as_u64(), Some(ignored + 1), "a cut message is ignored and counted: {st}");
    let e = app.call_err("command.run", json!({"id": "surface.inject", "params": {"surface": id, "bytes": "zz"}, "window": w}));
    assert_eq!(e.code, core_control::ErrorCode::InvalidParams);
    run(&mut app, "surface.remove", json!({"surface": id}));
    app.quit();
}
```

`wait_for_state` is the file's helper (it polls `state.get` with a condition). The first `state.get` assumes the app starts with no document window; if it opens one at launch, make the call against a window that has none (or after closing it) and say so in the report. If the demo's first strip is not the synth, or the document JSON's undo field has another name, read the strip index from `state.get surfaces`' feedback or the document, and adapt; say so in the report.

- [ ] **Step 2: Run them to see them fail.** Run `scripts/swift.sh test`, and the e2e after `scripts/make-app.sh`. Expected: FAIL.

- [ ] **Step 3: Implement the model, the sheet, the menu items and the plugin window's last-touched parameter.** The plugin window's Learn button learns `PluginWindowModel.lastTouched`, and is disabled before any parameter is touched. Settings › Keys "Learn MIDI" calls `learnCommand(id)` for the selected command row and is disabled for rows a key cannot run (Ruling 8) and without a document. A refusal raises "MIDI Learn Failed".

- [ ] **Step 4: Run the tests and the gates**, including `scripts/make-app.sh` and the e2e. Expected: Swift +5; e2e 11 passed.

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add app/Sources/VizDAWAppModel/LearnModel.swift app/Sources/VizDAWAppModel/DocumentModel.swift app/Sources/VizDAWAppModel/PluginWindowModel.swift app/Sources/VizDAW/LearnMapsSheet.swift app/Sources/VizDAW/MixerView.swift app/Sources/VizDAW/PluginWindows.swift app/Sources/VizDAW/StripChartPane.swift app/Sources/VizDAW/KeysSettingsView.swift app/Sources/VizDAW/SurfacesSettingsView.swift app/Tests/VizDAWAppModelTests/LearnModelTests.swift app/Tests/VizDAWAppModelTests/FakeSession.swift crates/vizctl/tests/e2e.rs
git commit -m "app: MIDI Learn on mixer, plugin window and Strip Chart controls, the maps sheet with mode and pick-up, Learn MIDI for commands; e2e drives an MCU (plan 5f-1 Task 11)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -14
```
Counts: Swift +5, e2e +1.

---

### Task 12: README 173–179, the backlog rows, the full gates

**Files:**
- Modify: `app/README.md`, `docs/feature-backlog.md`

- [ ] **Step 1: Write the README section.** Add this at the end of `app/README.md`:

```markdown
### Control surfaces and MIDI learn (plan 5f-1)

By eye and by hand; the protocols are tested byte by byte
(`core-surface` tests), the core through the fake ports
(`core-ffi/tests/surfaces.rs`, `learn.rs`, `holders.rs`) and the e2e
`drives_a_fader_and_a_button_from_an_mcu`.

173. Settings › Surfaces: Add Surface lists MCU-compatible, MCU
     XT-compatible and HUI-compatible; the port pickers list your MIDI
     ports. A unit with no ports plugged in shows "Disconnected"; plug
     it in: within 2 s it shows "Connected" and its display fills. A
     lane's input picker now shows the unit's port as "(used by a
     surface)". Record-arm a MIDI lane on All MIDI Inputs and touch a
     fader or press Play on the unit: nothing is recorded, played thru or
     step-entered.
174. MIDI learn: in the mixer, right-click a fader › MIDI Learn: the
     banner says "Waiting for MIDI…". Turn a knob on a controller: the
     banner goes, Edit shows "Undo MIDI Learn". Turn the knob from far
     below the fader's position: nothing moves until the knob passes the
     fader (pick-up), then the fader follows; let go: half a second later
     the move is one "Set Fader" step. Mixer strip menu › MIDI Learn
     Maps… lists the map; untick Pick-up and the fader jumps to the knob
     at once. A 14-bit fader learns as "CC n/n+32" and moves smoothly; an
     endless encoder learns as Relative (no Pick-up box); the mode picker
     changes one. With a MIDI lane armed on that keyboard, the learned
     knob is not recorded; the keys and other knobs are.
175. A real MCU-compatible unit, if one is available (manual checklist
     "a real MCU or HUI unit"): faders move by motor to the mixer's
     values and do not fight your hand; with fader automation in Read,
     play: the motor faders follow it; names and pan values show on the
     scribble strips; bank and channel buttons move eight and one strip;
     mute, solo and select light up; Play starts the transport (pressing
     it while playing does nothing), Stop stops, Record records; tap
     Rewind or Fast Forward: the position jumps to the previous or next
     marker (or bar line, without markers); hold one: the position winds,
     faster the longer you hold; the jog wheel moves by sixteenths; the
     position display counts bars and beats; meters move, and the mixer
     window's meters keep moving too.
176. A real HUI-compatible unit, if one is available: the same checks
     (its V-pots move pan only); the unit stays online (the ping), and
     the 40-character display shows the position and the selected strip.
177. README 165a, the surface half: on Touch, Latch, Write and Trim,
     play and ride the Built-in Synth fader from the unit (or a learnt
     knob): you hear it at once, the strip's ring shows the touch, and
     Stop lands one "Write Automation" step. Hold the same fader with the
     mouse and with the unit: it stays touched until you let go of both,
     and letting go of the mouse first neither glides back nor holds; a
     mouse release never ends a hold the unit or a `vizctl` script has.
     Rest a hand on a stopped unit's fader and edit notes, add a bus and
     change an automation mode: each works and is its own undo step.
178. Settings › Keys › Learn MIDI next to a command, then press a pad:
     the pad runs the command (with a project open). Settings › Surfaces
     lists the command maps; Remove takes one away.
179. Pull a unit's cable while holding a fader: within 2 s the unit
     shows "Disconnected", the fader is released (the ride lands as
     written); plug it back: the unit refreshes completely and that fader
     moves by motor again. Remove a unit in Settings › Surfaces while
     holding a fader: it is released. Close the project the unit drives:
     its displays go blank and its faders go down.
```

- [ ] **Step 2: Add the backlog rows.** Add them after the row "A pre-5e build names a project with automation modes Corrupt":

```markdown
| A pre-5f build names a project with learn maps Corrupt | Plan 5f-1 Ruling 6: `Settings.learn` is written only when a project has MIDI-learn maps and the format VERSION stays 3, so a build from before 5f reports such a project Corrupt ("unknown field `learn`") rather than NewerFormat. The owner accepted this (2026-11-02). Say so in the release notes, or bump the version when one is due anyway. |
| HUI V-pot assignment | Plan 5f-1 Ruling 5: HUI V-pots drive pan only; the HUI's V-Sel and assignment switches (send, plug-in) are ignored. Map them from a public HUI description and a real unit. |
```

- [ ] **Step 3: Run the full gates.** Run every Global Constraints gate, including the rt-check gates, the workspace tests, the Swift tests, `scripts/make-app.sh` and the e2e. Expected: Rust 1634 + 9 + 10 + 6 + 6 + 12 + 6 + 6 + 28 + 1 = **1718 passed**, 15 ignored (the new e2e is one more `#[ignore]` test); Swift 899 + 1 + 4 + 7 + 5 = **916**; e2e **11**; registry **144**.

- [ ] **Step 4: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add app/README.md docs/feature-backlog.md
git commit -m "README 173-179: control surfaces and MIDI learn by eye; backlog rows for learn maps in older builds and HUI V-pot assignment (plan 5f-1 Task 12)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -4
```

---

## Self-Review

1. **Spec coverage.**
   - §4.8: Task 4; command maps in preferences, Tasks 5 and 7.
   - §7.1, sources without touch (500 ms): Tasks 1 and 5; touch-sensing faders: Tasks 1 and 3; the static edit at the first value: Task 6.
   - §8.1:
     - `Surface::input`/`feedback`: Tasks 1–3 (Ruling 3 adds `first`, `tick` and `release_all`);
     - `SurfaceAction`: Task 1;
     - MixerView of heard values: Task 8;
     - `core-ffi::surfaces` and CoreMIDI: Tasks 7–8;
     - 20 ms and partners: Task 8;
     - no fader feedback while touched: Tasks 2–3.
   - §8.2:
     - MCU (faders, touch, V-pots and rings, buttons, transport with Play, skip and wind, bank and channel, jog, scribble strips, displays, meters, handshake): Tasks 1–2;
     - HUI (zones and ports, fader pairs, touch, V-pots, buttons, transport, names and display, meters, ping): Task 3;
     - learn (buttons, command maps in Keys, pairs and encoders, override in the list, feedback, pick-up): Tasks 5, 8 and 11.
   - §8.3: Settings › Surfaces in Task 10; `surfaces.json` and disconnected/reconnect in Task 7 (the 2 s refresh while connected) and Task 8 (releases and refresh).
   - §12.1:
     - `surface.add`, `surface.remove`, `surface.inject` and `state.get surfaces`: Task 9;
     - `midi.inject` feeding learn: Task 8.
   - §12.2: ignored and counted (Tasks 1, 3, 8, and the e2e's cut message); never stops the app (every core path is `get`/`match`, FFI guarded, CoreMIDI sinks in `catch_unwind`).
   - §12.3: the golden tests (Tasks 1–5) and the e2e (Task 11).
   - The manual checklist's "a real MCU or HUI unit": README 175–176.
   - The owner's answers of 2026-11-02: Play only starts and Rewind/Fast Forward skip or wind (Ruling 5; Tasks 1, 3, 8, 9); learned messages stay out of the lanes (Ruling 17; Tasks 5, 8, 10); pick-up (Ruling 18; Tasks 4, 5, 8, 11); learn maps in the project (Ruling 6; Task 12's backlog row).
   - No gaps.
2. **Step scan.** Every code step names its file, its signature and the spec values. The protocol tables are in Rulings 14–15 and in the tests. Every test is given as code. Draft lines were deleted:
   - Task 2's `first.len() + 0` reads `first.len()`;
   - Task 7's `keep` helper only keeps `common` used in that task. Keep it, or drop it once Task 8's tests use `wait_limit`.
3. **Type consistency.**
   - `SurfaceId`/`TouchHolder`/`TouchHolders`/`GestureHolderRow` come from Task 6 and are used in Tasks 7–8; `SurfaceRow.id` (u32) is always below 16.
   - `SurfaceKind` maps to `SurfaceKindRow`, and `LearnKind`/`LearnMode` to `LearnKindRow`/`LearnModeRow` (Task 8). `LearnSourceRow.number` is the CC or note (0 for pitch bend). `LearnMap.pickup` and `picks_up()` become `LearnMapRow.pickup`/`picks_up`.
   - `SurfaceAction::{IdleRelease, Skip, Wind}` (Task 1) are applied by the service (Task 8); `Skip`/`Wind` never reach the app.
   - `Bank` (Task 2) is used by the registry (Task 7).
   - `SurfaceEventRow` cases (`Command { id, programmatic }`, `Select`, `Arm { strip, path }`, `Jog`) match `SurfaceRelay` (Task 10).
   - `SurfaceCore` (Task 9) is used by `ControlHost`, `SurfaceSettings` (Task 10) and `LearnModel` (Task 11); `FakeSurfaceCore` (Task 9's test file) serves all three test files.
   - `focus_surfaces` is introduced minimal in Task 7 and completed in Task 8. `service_surfaces` is likewise introduced in Task 7 and completed in Task 8.
4. **Review Focus.** Each of the five lines has its named tests in its owning task, listed in the line.
5. **Proportion.** The spec's §8 is about 50 lines, and this plan is long because it carries two byte-level protocol tables and their golden tests: the tests are the spec of the bytes. Implementation bodies are left to the implementer, except the handshake formula (Ruling 14), the shuttle curve (Ruling 5) and the pick-up rule (Ruling 18), which no signature determines.

## Execution handoff

Plan complete and saved to `docs/superpowers/plans/2026-11-02-control-surfaces.md`. The controller runs it subagent-driven under the standing authorization (spec §13). Tasks 1–5 are pure and independent of the app, and Tasks 6–8 share the session internals; a fresh reviewer per task catches protocol slips before the service builds on them. Task 8 is the largest (the service, the claims, the meter holds and 28 tests); its reviewer should replay the lock order (Global Constraints) and the owed releases across two sessions.
