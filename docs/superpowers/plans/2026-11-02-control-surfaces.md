# VizDAW 5f-1 — Control Surfaces and MIDI Learn Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Hardware drives the mixer. An MCU-compatible unit (a main unit plus up to three extenders) or a HUI-compatible unit moves faders, pans, sends and plugin parameters through the 5e gesture channel, so riding a surface writes automation. Its buttons mute, solo, arm, select, run the transport and bank. Motor faders, LED rings, scribble strips, the position display and meters follow the mixer. Any automatable control can be MIDI-learned to a controller's CC, 14-bit CC pair, pitch bend or note; any registry command can be learned to a button. Surfaces are set up in Settings › Surfaces, and the control channel's `surface.*` commands drive all of it with no CoreMIDI port opened.

**Architecture:**
- **`core-surface`** is a new pure-Rust crate with no I/O and no clock reads (time is passed in as `Instant`). It holds the MCU, MCU XT and HUI state machines (`Surface::input`, `tick` and `feedback`), the 500 ms release-after-idle helper, MIDI-learn detection (`Learner`) and learned-map execution (`Mapper`).
- **`core-model`** gains `LearnMap` (spec §4.8) in `Settings.learn`, with `SettingsCmd::SetLearn`/`RemoveLearn`. The field is saved only when it is not empty, so `file::VERSION` stays 3.
- **`core-ffi`**:
  - **Carry fix.** A gesture's touch is held by *holders* (mouse, channel, plugin, each surface, learn), so a surface touch and a mouse touch on one target need both releases (carry m8).
  - **`surfaces` module.** It owns the process-wide registry: `surfaces.json` in the support folder, a port transport (fake in test builds and with `VIZDAW_AUDIO=offline`, CoreMIDI otherwise), the input queue, focus, and the service. The service runs in the focused session's `idle`: input → actions → gestures/commands; then feedback at most every 20 ms. Learn starts, maps and decisions also live here.
- **`core-control`, Swift, `vizctl`:**
  - `surface.add`, `surface.remove`, `surface.inject`, `surface.learn`, `surface.unlearn` and `state.get surfaces`.
  - Settings › Surfaces, the Learn menu items, the MIDI Learn Maps sheet, Settings › Keys "Learn MIDI".
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
  - Work on branch `sp5-audio` (HEAD `ae49da1` plus the plan commit). Never switch branches, never push.
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
  - Never touch the real `~/Library/Application Support/VizDAW`, the real preferences, the real Trash or any device. Tests use temporary folders: core-ffi's `support_dir()` defaults to a per-process temp folder under `test-hooks`, and Swift tests pass `MemoryDefaults()`. The e2e runs in its throwaway home.
- **Test waits.** Every `core-ffi` integration test that waits for the transport polls with a deadline from `common::wait_limit()` (`crates/core-ffi/tests/common/mod.rs`, which reads `VIZDAW_TEST_WAIT_SECS`), never with a fixed sleep for the result. Surface timing (20 ms throttle, 500 ms idle release, 1 s HUI ping, 300 ms learn window) is driven by passed `Instant`s in `core-surface`, and by the `surfaces::debug_advance(ms)` clock hook in `core-ffi`, never by real sleeps.
- **Crates and dependencies.**
  - Rust stable, edition 2024, license GPL-3.0-only.
  - New crate `core-surface`, named by the spec. Its dependencies are `core-model` and `serde` (already in the lock). `core-ffi` gains `core-surface`. No external crate.
- **Lints.** Every crate keeps `#![deny(missing_docs)]` and `#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic))]`; `core-surface` starts with both. New non-test code uses `.get()`, iterators and `checked_*`/`saturating_*`, never indexing or unwraps.
- **Platform-neutral cores.** `core-model`, `core-surface`, `core-engine`, `core-media` and `core-control` stay free of OS APIs.
- **Linux CI.**
  - CI runs `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` on `ubuntu-latest`.
  - CoreMIDI code (`surfaces::ports::CoreMidiPorts`) sits behind `#[cfg(all(target_os = "macos", not(feature = "test-hooks")))]`. A binding mutated only on macOS uses `#[cfg_attr(not(target_os = "macos"), allow(unused_mut))]`.
  - Every task that changes Rust checks `cargo clippy --workspace --all-targets --target x86_64-unknown-linux-gnu --target-dir target/linux-check -- -D warnings`.
  - No test in this plan needs CoreMIDI, so none is macOS-only.
- **TDD.** Every new test is run and seen to fail (RED) before the code that makes it pass.
- **Real-time rules (spec §3).**
  - Nothing in this plan runs on the real-time thread. Surface gestures enter the existing 5e-1 gesture queue through `Session` internals, exactly as the mixer's do.
  - CoreMIDI's input thread only pushes into a bounded queue: `try_lock`, drop and count, never wait, the pattern of `host::midi_from_source`.
  - **rt-check gates** run once, in Task 12, to prove nothing moved: `cargo clippy -p vizdaw-play --all-targets --features rt-check -- -D warnings` and `cargo test -p core-engine --features rt-check --test rt_alloc --test rt_alloc_graph --test rt_alloc_record --test rt_alloc_gestures`.
- **Media is write-once.** Nothing here writes media.
- **File compatibility.**
  - `core_model::file::VERSION` stays 3. `Settings.learn` is `#[serde(default, skip_serializing_if = "Vec::is_empty")]`: a project with no learn maps saves byte for byte as before, and older projects open with none (Ruling 6).
  - `surfaces.json` carries its own `"version": 1`. An unreadable or newer file is kept aside as `surfaces.json.bad` and VizDAW starts with no surfaces (Ruling 9).
- **FFI safety.**
  - No panic crosses the FFI. Every new exported function or method returns `Result<_, VizError>` and runs inside `ffi_guard`.
  - The exceptions are the infallible exports `surface_list`, `surface_ports`, `command_maps`, `learn_state` and `cancel_learn`. Each body runs inside `std::panic::catch_unwind` and answers its neutral value (empty list, empty ports, `LearnStateRow::default()`, nothing) on a panic.
- **UniFFI changes** (Tasks 6–9):
  - New enums: `GestureHolderRow`, `SurfaceKindRow`, `SurfaceEventRow`, `LearnKindRow`, `LearnModeRow`, `LearnForRow`.
  - New records: `SurfaceRow`, `SurfaceFeedbackRow`, `SurfaceStateRow`, `SurfacePortsRow`, `LearnSourceRow`, `LearnMapRow`, `CommandMapRow`, `LearnStateRow`.
  - New free functions: `surfaces_load`, `surface_list`, `surface_add`, `surface_remove`, `surface_ports`, `command_maps`, `remove_command_map`, `learn_command`, `cancel_learn`, `learn_state`.
  - New `Session` methods: `gesture_from`, `focus_surfaces`, `set_surface_selection`, `inject_surface`, `surface_state`, `take_surface_events`, `learn_target`, `learn_maps`, `set_learn_mode`, `remove_learn`.
  - New `StateTargetRow::Surfaces` and `ControlResult::Surfaces`.
  - `FakeSession` (`app/Tests/VizDAWAppModelTests/FakeSession.swift`) implements each new `Session` method in the task that adds it.
- **Swift.**
  - Generated bindings compile in Swift 5 mode; `VizDAWAppModel`, `VizDAWEditors`, `VizDAW` and the tests compile in Swift 6 mode with no new warnings.
  - No SwiftUI `@State`/`@FocusState` for model data (view-local presentation flags only).
  - New model tests go to `VizDAWAppModelTests`.
- **Control channel** (control spec §6, §7).
  - Programmatic calls never open dialogs.
  - Every command is in `core_control::registry::COMMANDS` and handled by `ControlCommands.run`; the Swift test `everyRegisteredMenuCommandHasAHandler` compares them.
  - The registry count test moves from 138 to 143 in Task 10.
  - Every new command gets hostile-input tests: by type and range in `core-control` (`surface_commands_check_their_params`), in the app (`surfaceCommandsCheckTheirArguments`) and in the cores (`hostile_surface_calls_are_refused`, `hostile_learn_calls_are_refused`, `malformed_bytes_are_ignored_and_counted`, `a_learn_map_is_checked_by_the_model`).
- **Generated files** are never committed: `app/Sources/VizDAWCore/vizdaw_ffi.swift`, `app/Sources/vizdaw_ffiFFI/include/`, `target/ffi/`, `dist/`.
- **Each task ends green.**
  - Always: `cargo fmt --all` then `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, the Linux clippy, and `cargo test` for the crates the task touches (the whole workspace in Tasks 6, 9 and 12).
  - Tasks 6–12 also run `cargo clippy -p core-ffi --lib -- -D warnings`, `scripts/build-ffi.sh`, `scripts/swift.sh build` and `scripts/swift.sh test`.
  - Tasks 11 and 12 also run `scripts/make-app.sh` and the e2e (`VIZDAW_E2E_APP=dist/VizDAW.app cargo test -p vizctl --test e2e -- --ignored --nocapture --test-threads 1`).
  - Known flaky tests: the `damaged_files` proptest, the MIDI listener tests (`platform-macos/tests/midi.rs`, the reconnect test), `core-ffi` catalog tests under parallel load, and the latency e2e timing. Re-run once before investigating, and say so in the report.
  - Baseline at `ae49da1`: Rust workspace 1632 passed, 0 failed, 14 ignored; Swift 899 tests; e2e 10 tests; registry 138. Each task's last step gives the counts it adds. Report the measured counts; a slip of the writer's arithmetic is not a failure.
- **How code is given.**
  - New files are written whole (`cat > … <<'VIZDAW_EOF'`) where their exact text matters (tests, byte tables). For new implementation files, the step gives the module's public items with their doc lines, and the algorithm where the signature and the tests leave a choice.
  - Changes to existing files are **Find / Replace** excerpts, applied in order with the Edit tool. Each Find occurs exactly once in its file at `ae49da1` (or in the file an earlier task of this plan left, which the step says).
  - If an excerpt does not match, make the same change to the current text. If a name differs from the tree, use the tree's name and say so in the report.
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

These are the five failure modes a user is most likely to hit that the spec implies but does not spell out, most likely first. Each is pinned by named tests in its task.

1. **Two hands on one control.** A touch-sensing fader is held while the same fader is grabbed with the mouse. A learned knob turns a pan while a `vizctl` script holds it. The surface lets go first, or the mouse does.
   - Expected: the target stays touched until the last holder releases. Releasing one holder does not commit or glide back the other's move. A static edit is one undo step. A second Touch from the same holder is a move, never a second hold.
   - Pinned by `a_surface_and_the_mouse_hold_one_target_until_both_let_go`, `each_holder_releases_only_its_own_touch` and `a_holder_touching_twice_holds_once` (Task 6), and by `a_surface_touch_and_a_mouse_touch_share_the_target` (Task 8).
2. **A motor fader fighting the hand, or lagging behind the mix.**
   - Expected:
     - no pitch-bend or fader CC goes to a fader while it is touched;
     - on release the unit gets the value the mixer now has once (an automated fader snaps to its automation);
     - a fader moved from another surface follows at once;
     - everything else follows within 20 ms, and only what changed is sent.
   - Pinned by `no_fader_feedback_while_touched_then_one_on_release` and `feedback_sends_only_what_changed` (Task 2), `hui_fader_touch_blocks_feedback` (Task 3), and `feedback_waits_20_ms_but_partners_go_at_once` (Task 8).
3. **Banks with the wrong number of strips or units.** A project with 3 strips, with 30 strips, or with strips added and deleted while banked. One MCU and three XTs in any position order. A fifth unit, or two main units.
   - Expected:
     - units show consecutive strips in position order;
     - Bank moves by 8 × units and Channel by 1, never past the last strip or below 0;
     - an empty slot shows a blank strip and ignores its controls (counted);
     - a deleted strip's controls do nothing;
     - a second main unit or a fourth extender is refused with a clear message.
   - Pinned by `banks_of_one_main_and_three_extenders` and `bank_and_channel_clamp_to_the_strips` (Task 2), `an_empty_slot_is_blank_and_ignored` (Task 2), `a_fifth_unit_or_a_second_main_is_refused` (Task 7) and `a_strip_deleted_while_banked_is_skipped` (Task 8).
4. **Garbage from a controller.** A truncated SysEx, running status, a data byte above 0x7F, a note on channel 16, a 14-bit pair whose LSB never comes, an encoder that sends 64, a port name of 300 characters, `surface.inject` with odd hex. A learn on a port that disappears mid-learn.
   - Expected: each bad message is ignored and counted in `state.get surfaces` (`ignored`). Hostile channel input is refused with `invalid_params`. The model refuses a learn map out of range. Nothing panics, and the app keeps running.
   - Pinned by `malformed_bytes_are_ignored_and_counted` (Tasks 1 and 3), `a_lone_msb_is_a_7_bit_value` (Task 5), `a_learn_map_is_checked_by_the_model` (Task 4), and `hostile_surface_calls_are_refused` and `hostile_learn_calls_are_refused` (Tasks 7 and 9).
5. **A surface or learned control that goes away and comes back.** A USB unit unplugged mid-ride. A port renamed. `surfaces.json` damaged, or written by a newer VizDAW. The focused document closes while a surface holds a fader.
   - Expected:
     - an unplugged unit shows "Disconnected", and its held touches are released at once (the ride lands as written so far);
     - it reconnects when the port returns and gets a full refresh;
     - a damaged `surfaces.json` is kept aside and VizDAW starts with no surfaces;
     - closing the document releases its surface touches.
   - Pinned by `a_vanished_port_disconnects_and_releases_its_touches`, `a_returning_port_reconnects_with_a_full_refresh`, `a_damaged_surfaces_file_is_kept_aside` and `closing_the_focused_session_releases_surface_touches` (Task 7).

## Plan Rulings

Each ruling decides something the spec leaves open, in the format "what — why — cost if wrong".

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
   - The holders are `Mouse`, `Channel`, `Plugin`, `Surface(id)` and `Learn`.
   - A Touch from a new holder joins; a Touch from a holder that already holds the target is a move.
   - A value from any holder moves the target (the last value wins).
   - A Release from a holder that does not hold the target does nothing.
   - The touch's path (static, live or write) and its `Source` are those of the first holder, as today.
   - `Session.gesture` keeps its signature and means `Mouse`. The channel's `automation.gesture` calls the new `Session.gesture_from(…, .channel)`. Surfaces and learn call the internals with their holder.
   - Closing a window, a vanished target and a vanished port release every holder (the existing `release_touches`/`release_vanished`, plus the port's holder).
   - Why: the carry asks for "their own touches or a reference count". A holder set is a reference count that cannot go negative and cannot be released twice by one source.
   - Cost if wrong: a surface whose touch-release message is lost holds the target until its port vanishes or the window closes. `state.get automation` lists it, as for a script today.
3. **Ruling (the surface API): `Surface::feedback(&mut self, view: &MixerView, first: usize, now: Instant) -> Vec<Vec<u8>>`, where `first` is the index of the first strip this unit shows, and `now` times the meters and the HUI ping.**
   - The spec's `feedback(state)` has no bank offset; the group manager (`core-ffi::surfaces`) owns banking.
   - `Surface::tick(&mut self, now) -> Vec<SurfaceAction>` is added for the 500 ms releases of V-pots, plus the HUI ping and connection timeouts.
   - Input addresses strips through the IDs the last feedback showed. Before any feedback, strip controls are ignored and counted.
   - Why: banking spans several units. One unit cannot know its neighbours, and input must address what the unit displays.
   - Cost if wrong: an API rename.
4. **Ruling (banks): all MCU and MCU XT units form one bank group, ordered by `position` (0–3). Each HUI unit is its own group of 8.**
   - A group shows `width = 8 × units` strips, starting at `offset`, from the mixer's display order (master excluded).
   - `Bank(±1)` moves `offset` by `width`; `Channel(±1)` moves it by 1.
   - `offset` is clamped to `0..=max(0, strips − width)` after every move and after every mixer change.
   - Only the main MCU has the master fader (pitch bend channel 9, touch note 0x70) and the transport and assignment buttons.
   - At most 1 MCU and 3 MCU XT units; more is refused ("VizDAW drives one MCU-compatible main unit and up to three extenders").
   - Why: this is spec §8.2's "banks of 8 × units"; HUI has no extender protocol.
   - Cost if wrong: Bank moves by 8 per press, a one-line change.
5. **Ruling (what the controls drive):**
   - Faders drive `StripFader`.
   - V-pots work in three assignments, chosen with the MCU's Pan, Send and Plug-in buttons, or HUI's V-Sel:
     - Pan drives `StripPan`;
     - Send drives `SendLevel(strip, 0)`, the strip's first send;
     - Plug-in drives the first 8 parameters (by the plugin's list order) of the selected strip's first insert, one per V-pot, with the strip names row showing parameter names.
   - Each V-pot detent moves the target by 0.01 of its normalised range, times the detent count (1–15).
   - Mute, solo and select are clicks (touch, value and release at once) through `Gesture`. Select and rec-arm are relayed to the app (`SurfaceEventRow::Select/Arm`, Ruling 12).
   - Transport and command buttons relay registry IDs (`Command`): Play `transport.play_stop`, Stop `transport.stop`, Record `transport.record`, Rewind `transport.previous_marker`, Fast Forward `transport.next_marker`, Cycle `transport.loop`, Undo `edit.undo`, Save `file.save`. The MCU automation buttons send Read/Off, Write, Trim, Touch and Latch as `automation.mode.read|write|trim|touch|latch` for the selected strips.
   - The jog wheel sends `Jog(detents)`. The app moves the position by one sixteenth note per detent.
   - Why: these are the spec's targets (fader, pan, send, plugin) with the registry's existing commands; no new transport command is needed.
   - Cost if wrong: Play while playing stops (the registry's toggle). If the owner wants a separate Play, add `transport.play` later.
6. **Ruling (where learn maps live): `Settings.learn: Vec<LearnMap>` in `core_model::document::Settings`, not a new `Document` field.**
   - Two commands: `SettingsCmd::SetLearn { map }` (adds, or replaces the map with the same `source`; undo label "MIDI Learn") and `SettingsCmd::RemoveLearn { source }` ("Remove MIDI Learn").
   - At most 512 maps.
   - Checks: the port name must be non-empty, without control characters, and at most 255 bytes; the channel must be 0–15; a CC must be 0–127, and 0–31 for `Cc14`; a note must be 0–127. The target must exist when the map is set.
   - A map whose target is later deleted stays and does nothing; the maps sheet shows it as "(missing)".
   - The field is skipped when empty, so the format stays at version 3.
   - Why:
     - `DocumentRepr` is built literally in 16 places, while `Settings` is built only in its `Default`;
     - learn maps are project settings ("target learn maps are in the project");
     - the 5e-1 precedent (Ruling 2) keeps the version.
   - Cost if wrong:
     - a pre-5f build opening a project with learn maps says Corrupt rather than NewerFormat (backlog row added in Task 12);
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
   - A learn waits until it completes, is cancelled, or another learn starts. There is no timeout.
   - Why: the spec's cluster rule, with an explicit window so one turn of an encoder decides.
   - Cost if wrong: an encoder learnt from one detent is Absolute; the maps sheet's mode picker fixes it.
8. **Ruling (learned maps in use):**
   - **Absolute values:**
     - `Cc7` → `v / 127`;
     - `Cc14` → `(msb << 7 | lsb) / 16383`, with the MSB held until its LSB arrives. An MSB alone is applied as `msb / 127` when no LSB follows within the same `input` call or before the next MSB;
     - `PitchBend` → `(lsb | msb << 7) / 16383`.
   - **Notes:** a note-on toggles a stepped target (mute, solo). On a continuous target, note-on sets `velocity / 127` and note-off does nothing.
   - **Relative modes:** each detent adds ±1/127 of the normalised range to the target's current value (the map's own last value while its idle touch is open, else `current`).
   - **Release:** every learned gesture uses holder `Learn` and releases 500 ms after its last value.
   - **Feedback:** absolute maps send the current value back to the output port with the same name as the source port, if one exists, whenever it differs from what was last sent. A map whose target is touched by the learn holder sends nothing.
   - **Command maps:** a note-on, or a CC rising through 64, runs the command once.
   - Why: §8.2 "Feedback"; one rule for every control without touch sensing (§7.1).
   - Cost if wrong: a controller that echoes its own values could fight. It would need a per-map "no feedback" switch, added later.
9. **Ruling (`surfaces.json`): the file lives at `support_dir()/surfaces.json`.**
   - Format: `{ "version": 1, "surfaces": [{ "id", "kind", "input", "output", "position" }], "commands": [{ "source", "mode", "command" }] }`.
   - It is written atomically (temp file, then rename) after every change, by the core.
   - It is read once by `surfaces_load()`, which the app calls at launch after `setSupportDir` (Task 10).
   - A file that does not parse, or has a newer version, is renamed `surfaces.json.bad` (replacing an older `.bad`) and VizDAW starts empty, with one problem line in `state.get surfaces` (`problems`).
   - Test builds default `support_dir()` to their per-process temp folder; the e2e uses its throwaway home.
   - Why: "beside the 4f key bindings" means beside the app's support files. The 4f bindings are in preferences, but the spec names this file and folder. The core owns it so the channel and the app see one state.
   - Cost if wrong: the owner keeps a copy of the `.bad` file by hand.
10. **Ruling (ports and the transport): the port transport is a trait, `surfaces::ports::Ports`.**
    - `send(port, bytes) -> bool`, `sources() -> Vec<String>`, `destinations() -> Vec<String>`, `refresh()`.
    - `FakePorts` is used in `test-hooks` builds and whenever `host::offline_mode()` is on (`VIZDAW_AUDIO=offline`, so the e2e opens no CoreMIDI client). It treats every named port as present unless `debug_set_missing` says otherwise, and records what is sent.
    - `CoreMidiPorts` (macOS live builds) uses `platform_macos::midi::SourceListener::connect_all("VizDAW Surfaces", sink)` for input and `MidiSystem::new("VizDAW Surfaces")` with `open_destination` for output.
    - It is created on the first `surface_add`, learn or loaded surface, and never before.
    - Its sink pushes into a bounded queue (4096 messages, `try_lock`, drop and count).
    - While any surface is disconnected, the service calls `refresh()` (the listener's `reconnect` and a reopen of the outputs) at most every 2 s.
    - Why: the spec asks for CoreMIDI notifications. The existing listener has no notification callback, and polling reuses tested code (`SourceListener::reconnect`).
    - Cost if wrong:
      - up to 2 s to reconnect;
      - a CoreMIDI query on the main thread every 2 s while a unit is unplugged (the backlog row "Input pickers' MIDI source list off the main thread" covers moving it).
11. **Ruling (which document the surfaces drive): the app's command target (`DocumentStore.noteTarget`), which the ticker passes to `Session.focus_surfaces()` every tick.**
    - Only the focused session's `idle` services the surfaces: it drains input, decides learns, maps, applies actions and sends feedback.
    - With no document open, nothing is serviced: input past the queue's capacity is dropped and counted, and Settings › Keys "Learn MIDI" is disabled ("Open a project to learn MIDI").
    - When focus moves, the old session's surface touches are released, and the units get a full refresh from the new one.
    - `surface.inject` services the target window's session at once (and focuses it).
    - Why: notes already follow the command target (plan 4c Ruling 11); one owner keeps the gesture holders simple.
    - Cost if wrong: learning a command needs an open project.
12. **Ruling (what the app does with surface events): `Session.take_surface_events()` returns `SurfaceEventRow`s, which `DocumentModel.idle` drains.**
    - `command(id)` runs the registry command through `KeyMonitor.perform(.command(id), doc)`, exactly as a key does.
    - `select(strip)` sets `selectedStrips` to that strip, and the app reports selection back through `Session.set_surface_selection`.
    - `arm(strip)` toggles the arm of `MixerModel.inputLane(for:)` (AudioPath strips only; other strips' rec buttons do nothing).
    - `jog(detents)` locates by `detents × ppq / 4` from the transport's tick.
    - The arm LED is computed in the core: an AudioPath strip is lit when any lane of any sequence playing through its path is armed.
    - Why: selection and the lane chooser are app state (5e-2 `selectedStrips`, 5d-2 Ruling 4), and commands are run by the app.
    - Cost if wrong: one tick (16 ms) of latency on surface buttons.
13. **Ruling (feedback timing): the service sends a unit's feedback at most every 20 ms. Values a holder moved go at once to every other surface and learn map showing that target.**
    - The spec's "touched faders' partners" are read as the other units and maps that display the moved target.
    - Feedback is diffed against what each unit was last sent (`feedback_sends_only_what_changed`). A full refresh happens on connect, on a bank change and on a focus change.
    - Meters are always resent (the unit decays them): MCU every 300 ms, HUI every 100 ms.
    - Why: §8.1, and diffing keeps a 4-unit MCU group within MIDI bandwidth.
    - Cost if wrong: one more message class to send at once.
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
    - Scribble strips: `F0 00 00 66 <id> 12 <offset 0–111> <ASCII…> F7`. Row 1 holds 7 characters of each strip's name plus a space; row 2 holds the V-pot value text (pan `<L50`, `C`, `R30>`; send dB `-6.0`; plugin value text).
    - Timecode/BBT display: CC `0x40`–`0x49` (0x49 leftmost). Each character is encoded as ASCII `0x40`–`0x5F` → `− 0x40`, `0x20`–`0x3F` as itself, plus `0x40` for a following dot. The display shows `MixerView.position` with separators turned into dots.
    - Assignment display: CC `0x4B` (left) and `0x4A` (right), showing `PN`, `SE` or `PL`.
    - Meters: `D0 (k << 4 | level)` with `level = 0` below −60 dBFS, else `min(12, floor((dB + 60) / 5) + 1)`.
    - Why: public protocol descriptions; units that skip the handshake are common.
    - Cost if wrong: a unit that insists on the handshake stays offline. README 175 checks a real unit if one is available.
15. **Ruling (HUI tables), from public HUI protocol descriptions:**
    - **Ping:** VizDAW sends `90 00 00` every 1 s; the unit's `90 00 7F` marks it online. With no reply for 3 s it is offline (still connected).
    - **Switches:** in, `B0 0F zz` then `B0 2F 4p` (press) or `0p` (release); out (LEDs), `B0 0C zz` then `B0 2C 4p` or `0p`.
      - Strip zones `0x00`–`0x07`, ports: 0 fader touch, 1 select, 2 mute, 3 solo, 4 auto, 5 v-sel, 6 insert, 7 rec-arm.
      - Zone `0x0A`, ports: 0 channel left, 1 bank left, 2 channel right, 3 bank right.
      - Zone `0x0E`, ports: 1 rewind, 2 fast forward, 3 stop, 4 play, 5 record.
    - **Faders:** in and out as `B0 0z hi`, `B0 2z lo`; `value = hi << 7 | lo` over 0–16383.
    - **V-pots:** in, `B0 4v d`, where `d & 0x40` set means clockwise and `d & 0x3F` is the detent count. LED rings out, `B0 1v p` with `p` 1–11.
    - **Meters:** `A0 0k (side << 4 | level)`, side 0 left and 1 right, level 0–12 as in Ruling 14.
    - **Displays:** names `F0 00 00 66 05 00 10 <k 0–7> c c c c F7`; the 40-character display in 8 zones of 10, `F0 00 00 66 05 00 12 <zone> <10 chars> F7`, zones 0–3 on line 1 (position, assignment) and 4–7 on line 2 (the selected strip's name and mode); timecode `F0 00 00 66 05 00 11 <8 digits, rightmost first> F7`.
    - **Jog:** `B0 0D d`, with 0x01–0x3F positive and 0x41–0x7F negative.
    - Why: §8.2's list.
    - Cost if wrong: a button maps differently on a real unit. The tables are one file (`hui.rs`) and README 176 checks a real unit if one is available.
16. **Ruling (`surface.learn`/`surface.unlearn` on the channel).** Two more commands beyond the spec's three. `surface.learn { target? , command? }` starts a learn, and the bytes come from `midi.inject` or a device. `surface.unlearn { target? , command? }` removes maps.
    - `midi.inject` now also feeds MIDI learn and learned maps, and answers `ran` when the engine *or* learn took the bytes.
    - Why: the Learn buttons need a channel twin (control spec: every command has one) and hostile-input tests.
    - Cost if wrong: two registry IDs to retire.

## Carried items

- **`.superpowers/sdd/2026-10-28-automation-ui/carry-to-5f.md`** asks that a surface touch and a mouse touch on the same target have their own touches or a reference count (final review m8). This is Ruling 2, done in Task 6, with Task 8's surface test.
- **README 165a's surface half** (`carry-to-5e-2.md`: "riding with … a surface in 5f"): README 177 (Task 12) rides a fader from a real or emulated unit through every mode.
- **`docs/feature-backlog.md`**: no row names 5f. Task 12 adds the row "A pre-5f build names a project with learn maps Corrupt" (Ruling 6).

## File map

| File | Task | Role |
|---|---|---|
| `crates/core-surface/{Cargo.toml,src/lib.rs,src/midi.rs,src/idle.rs,src/mcu.rs}` | 1–2 | crate, shared types, message splitting, 500 ms helper, MCU |
| `crates/core-surface/src/hui.rs` | 3 | HUI |
| `crates/core-model/src/learn.rs`, `document.rs`, `command/settings.rs`, `lib.rs` | 4 | `LearnMap`, `Settings.learn`, commands |
| `crates/core-surface/src/learn.rs` | 5 | `Learner`, `Mapper`, `CommandMap` |
| `crates/core-ffi/src/session.rs`, `automation.rs` | 6 | holders, `gesture_from` |
| `crates/core-ffi/src/surfaces.rs`, `crates/core-ffi/src/surfaces/ports.rs` | 7–8 | registry, ports, persistence, service, learn glue |
| `crates/core-control/src/{registry.rs,request.rs}`, `crates/core-ffi/src/control.rs` | 9–10 | channel |
| `app/Sources/VizDAWAppModel/{SurfaceCommands.swift,SurfaceSettings.swift,SurfaceRelay.swift,LearnModel.swift}` | 10–11 | app models |
| `app/Sources/VizDAW/{SurfacesSettingsView.swift,LearnMapsSheet.swift}` and edits | 10–11 | views |
| `crates/vizctl/tests/e2e.rs`, `app/README.md`, `docs/feature-backlog.md` | 11–12 | e2e, README, backlog |

---

### Task 1: `core-surface`: the crate, the shared types, message splitting, the idle release, MCU input

**Files:**
- Create: `crates/core-surface/Cargo.toml`, `crates/core-surface/src/lib.rs`, `crates/core-surface/src/midi.rs`, `crates/core-surface/src/idle.rs`, `crates/core-surface/src/mcu.rs`
- Test: create `crates/core-surface/tests/mcu_input.rs`
- Modify: `crates/core-ffi/Cargo.toml` (dependency, so the lock settles once), `Cargo.lock`

**Interfaces:**
- Consumes: `core_model::automation::{AutomationTarget, AutomationMode}`, `core_model::ids::StripId`, `core_model::mixer::{ParamId}`, `core_model::ids::PluginInstanceId`.
- Produces (`core_surface`):
  ```rust
  pub mod hui; pub mod learn; pub mod mcu; pub mod midi; mod idle;
  pub use idle::{IdleTouches, IDLE_RELEASE};
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
      /// A registry ID (Ruling 5) or a learned command.
      Command(String),
      Bank(i32), Channel(i32),
      Select(StripId), Arm(StripId),
      Jog(i32),
  }
  /// The V-pot assignment (Ruling 5).
  #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
  pub enum Assign { #[default] Pan, Send, Plugin }
  /// One plugin parameter a V-pot can drive.
  #[derive(Debug, Clone, PartialEq)]
  pub struct ParamView { pub target: AutomationTarget, pub name: String, pub value: f64, pub text: String }
  /// One strip as the surface shows it (normalised values as automation uses them).
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
      /// Faders it holds touched now (Ruling 2's holder, released on disconnect).
      fn touched(&self) -> Vec<AutomationTarget>;
  }
  /// A new state machine of `kind`.
  pub fn new_surface(kind: SurfaceKind) -> Box<dyn Surface>;
  // midi.rs
  /// Splits `bytes` into whole messages: channel messages by length, SysEx from F0 to F7; a byte that starts nothing, a cut message, a data byte above 0x7F or running status are reported as `Err(len)` of the bytes skipped.
  pub fn split(bytes: &[u8]) -> Vec<Result<&[u8], usize>>;
  pub fn hex(bytes: &[u8]) -> String; // "90 68 7f"
  // idle.rs
  pub const IDLE_RELEASE: Duration = Duration::from_millis(500);
  #[derive(Debug, Default)] pub struct IdleTouches { /* target → last value time */ }
  impl IdleTouches {
      /// Touch(Some(v)) the first time, else Value(v).
      pub fn value(&mut self, t: AutomationTarget, v: f64, now: Instant) -> SurfaceAction;
      /// Release for every target idle for IDLE_RELEASE.
      pub fn tick(&mut self, now: Instant) -> Vec<SurfaceAction>;
      pub fn held(&self) -> Vec<AutomationTarget>;
  }
  // mcu.rs
  pub struct Mcu { /* kind Mcu or McuXt; shown: [Option<StripId>; 8]; master shown; touched: [bool; 9]; assign; idle; last sent; online; ignored */ }
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
- **Faders.** Pitch bend `E<ch> lsb msb` (ch 0–7 strips, 8 master on the main unit only) → `Value((msb << 7 | lsb) / 16383)` while touched. An untouched fader that moves (a unit without touch sensing) goes through `IdleTouches`.
- **Fader touch.** Velocity ≥ 0x40 → `Touch(None)`, below → `Release`.
- **Buttons.** Buttons act on press (velocity > 0) only: mute and solo become `Gesture` clicks at the toggled value of the last view; select → `Select`; rec → `Arm`; transport and automation buttons → `Command`; bank and channel buttons → `Bank(±1)`/`Channel(±1)` (main unit only; an extender's are ignored, not counted).
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
mkdir -p crates/core-surface/src crates/core-surface/tests
cat > crates/core-surface/tests/mcu_input.rs <<'VIZDAW_EOF'
//! MCU input, byte by byte (plan 5f-1 Task 1; spec §8.2, §12.3).

use core_model::automation::{AutomationMode, AutomationTarget as T};
use core_model::ids::StripId;
use core_surface::mcu::{Mcu, handshake_response};
use core_surface::{
    IDLE_RELEASE, MixerView, Phase, StripView, Surface, SurfaceAction as A, midi,
};
use std::time::{Duration, Instant};

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
    assert_eq!(parts[0], Ok(&b[0..3]));
    assert_eq!(parts[1], Ok(&b[3..10]));
    assert_eq!(parts[2], Err(2), "a cut pitch bend is skipped and reported");
    assert_eq!(midi::split(&[0x68, 0x7F]), vec![Err(1), Err(1)], "running status is not supported");
    assert_eq!(midi::split(&[0x90, 0x80, 0x00]), vec![Err(3)], "a data byte above 0x7f");
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
    let a = m.input(&[0xE7, 0x00, 0x40], now);
    match a.as_slice() {
        [A::Gesture(T::StripFader(StripId(8)), Phase::Touch(Some(x)))] => {
            assert!((x - 8192.0 / 16383.0).abs() < 1e-9, "an untouched move is an idle touch")
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        m.input(&[0x90, 0x68, 0x00], now),
        vec![A::Gesture(T::StripFader(StripId(1)), Phase::Release)]
    );
    assert_eq!(
        m.tick(now + IDLE_RELEASE + Duration::from_millis(1)),
        vec![A::Gesture(T::StripFader(StripId(8)), Phase::Release)],
        "500 ms after its last value"
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
        (0x5E, "transport.play_stop"),
        (0x5D, "transport.stop"),
        (0x5F, "transport.record"),
        (0x5B, "transport.previous_marker"),
        (0x5C, "transport.next_marker"),
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
        &[0xFF][..],
        &[0x90, 0x05, 0x7F][..],
    ] {
        let _ = m.input(b, now);
    }
    assert_eq!(m.ignored(), before + 6, "cut, unterminated, unknown note, channel 2, a byte that starts nothing, rec on an empty slot");
    let mut fresh = Mcu::new(false);
    assert!(fresh.input(&[0x90, 0x10, 0x7F], now).is_empty(), "nothing shown yet");
    assert_eq!(fresh.ignored(), 1);
}
VIZDAW_EOF
```

The five first-strip constants above are the Ruling 5/14 tables; `0x90 0x05` is rec on strip 6, which `view(3)` leaves empty.

- [ ] **Step 2: Run them to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-surface --test mcu_input 2>&1 | tail -5
```
Expected: FAIL — package `core-surface` not found (no manifest yet).

- [ ] **Step 3: Write the crate.** Write `Cargo.toml` as shown, `lib.rs` (the doc header "`core-surface`: control-surface protocol state machines and MIDI learn (spec §8). Pure Rust, no I/O: bytes in, actions and bytes out; time is passed in", the lint attributes, the items in Interfaces), `midi.rs`, `idle.rs` and `mcu.rs`. `feedback` for this task returns only queued replies (the handshake) and records `shown` from `view.strips[first..first+8]` and the master. Task 2 adds the real feedback. Add `hui.rs` and `learn.rs` as stubs, each holding only their module doc, so the module tree compiles. `new_surface(Hui)` returns `Mcu::new(false)` until Task 3 replaces it, with a `// Task 3` comment.

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
Expected: 7 passed; clippy clean on both targets; `Cargo.lock` shows only the `core-surface` package and `core-ffi`'s new dependency line.

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-surface/Cargo.toml crates/core-surface/src/lib.rs crates/core-surface/src/midi.rs crates/core-surface/src/idle.rs crates/core-surface/src/mcu.rs crates/core-surface/src/hui.rs crates/core-surface/src/learn.rs crates/core-surface/tests/mcu_input.rs crates/core-ffi/Cargo.toml Cargo.lock
git commit -m "core-surface: the crate, message splitting, the 500 ms idle release and MCU input (plan 5f-1 Task 1)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -12
```
Counts: Rust +7.

---

### Task 2: MCU feedback: faders, LEDs, rings, scribble strips, displays, meters; banks of one main and three extenders

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
  ```

`Mcu::feedback` writes the messages for strips `view.strips[first..first+8]` (blank where there are fewer) and the master, and only for what differs from what this unit was last sent (a `Sent` cache per element):
- **Faders:** `E<ch> lsb msb` from `round(fader × 16383)`. Skipped while that fader is touched. On release, the cache entry for that fader is cleared, so the next feedback sends it once.
- **LEDs:** `90 <note> 7F|00` for rec, solo, mute and select per strip; play, stop, record and cycle from `playing`, `recording` and `looping` (main only); the assign buttons; and the automation buttons, lit for the first selected strip's mode.
- **V-pot rings:** `B0 <0x30+k> <mode << 4 | pos>`, with `pos = 1 + round(v × 10)`. Pan is in mode 1 (boost/cut), send in mode 2 (wrap), plugin in mode 0.
- **Scribble strips:** one SysEx per changed row (`F0 00 00 66 <id> 12 <0 or 56> <56 chars> F7`). Names are truncated to 7 characters plus a space. Row 2 holds the value text by Ruling 14.
- **Displays:** timecode/BBT (CC 0x40–0x49, main only) and assignment (CC 0x4A/0x4B, main only).
- **Meters:** `D0 <k << 4 | level>` for every shown strip, sent at most every 300 ms (Ruling 13).

`refresh()` clears the cache and makes the meters due.

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-surface/tests/mcu_feedback.rs <<'VIZDAW_EOF'
//! MCU feedback and banks (plan 5f-1 Task 2; spec §8.1, §8.2, §12.3).

#[path = "mcu_input.rs"]
#[allow(dead_code)]
mod input;

use core_model::automation::AutomationMode;
use core_model::ids::StripId;
use core_surface::mcu::Mcu;
use core_surface::{Bank, Surface, SurfaceAction as A, seg};
use input::view;
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

- [ ] **Step 3: Implement MCU feedback and `Bank`** in `mcu.rs` and `lib.rs`, following the encoding above and Rulings 13–14.

- [ ] **Step 4: Run the tests and the gates** (as in Task 1 Step 4; `cargo test -p core-surface`). Expected: 15 passed.

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-surface/src/mcu.rs crates/core-surface/src/lib.rs crates/core-surface/tests/mcu_feedback.rs
git commit -m "core-surface: MCU feedback (faders never while touched, LEDs, rings, scribble strips, displays, meters) and banks (plan 5f-1 Task 2)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -6
```
Counts: Rust +8.

---

### Task 3: HUI

**Files:**
- Modify: `crates/core-surface/src/hui.rs`, `crates/core-surface/src/lib.rs` (`new_surface(Hui)`)
- Test: create `crates/core-surface/tests/hui.rs`

**Interfaces:**
- Consumes: Tasks 1–2.
- Produces: `pub struct Hui` implementing `Surface` by Ruling 15; `pub const PING: [u8; 3] = [0x90, 0x00, 0x00];`, `pub const PING_EVERY: Duration = Duration::from_secs(1);`.

`Hui` keeps the last zone select (`B0 0F zz`) and decodes the next `B0 2F` as that zone's port. A port message with no zone select before it is ignored and counted. A fader value uses the pair of its hi CC (`0x00+z`) and lo CC (`0x20+z`): the hi value is held until the lo arrives, and applied alone (`hi << 7`) when another hi comes first. `tick` sends nothing itself: `feedback` emits `PING` whenever `PING_EVERY` has passed since the last one, and `online()` is true while a `90 00 7F` arrived within 3 s.

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-surface/tests/hui.rs <<'VIZDAW_EOF'
//! HUI, byte by byte (plan 5f-1 Task 3; spec §8.2, §12.3).

#[path = "mcu_input.rs"]
#[allow(dead_code)]
mod input;

use core_model::automation::AutomationTarget as T;
use core_model::ids::StripId;
use core_surface::hui::{Hui, PING};
use core_surface::{Phase, Surface, SurfaceAction as A};
use input::view;
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
    assert_eq!(h.input(&z(0x0E, 4), now), vec![A::Command("transport.play_stop".into())]);
    assert_eq!(h.input(&z(0x0E, 3), now), vec![A::Command("transport.stop".into())]);
    assert_eq!(h.input(&z(0x0E, 5), now), vec![A::Command("transport.record".into())]);
    assert_eq!(h.input(&[0xB0, 0x0D, 0x02], now), vec![A::Jog(2)]);
    assert_eq!(h.input(&[0xB0, 0x0D, 0x42], now), vec![A::Jog(-2)]);
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

- [ ] **Step 4: Run the tests and the gates.** Expected: 21 passed in `core-surface`.

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-surface/src/hui.rs crates/core-surface/src/lib.rs crates/core-surface/tests/hui.rs
git commit -m "core-surface: HUI (zones and ports, fader pairs with touch, ping, displays, meters) (plan 5f-1 Task 3)

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
  pub struct LearnMap { pub source: LearnSource, pub target: AutomationTarget, #[serde(default)] pub mode: LearnMode }
  impl LearnSource {
      /// Ruling 6's checks.
      pub fn check(&self) -> crate::error::Result<()>;
      /// "Port · CC 7 ch 1", "Port · CC 1/33 ch 1", "Port · Pitch Bend ch 2", "Port · Note C3 ch 1".
      pub fn describe(&self) -> String;
  }
  ```
  - `Settings.learn: Vec<LearnMap>`, `#[serde(default, skip_serializing_if = "Vec::is_empty")]`.
  - `SettingsCmd::SetLearn { map: LearnMap }`, named "MIDI Learn". It checks the source and that the target exists (`automation::target_exists`), refuses a `VelocityScale` target ("velocity scale cannot be learned"), replaces a map with an equal `source`, and refuses a new map past `MAX_LEARN_MAPS`.
  - `SettingsCmd::RemoveLearn { source: LearnSource }`, named "Remove MIDI Learn". It refuses an unknown source.

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-model/tests/learn.rs <<'VIZDAW_EOF'
//! Learn maps in the project (plan 5f-1 Task 4; spec §4.8; Ruling 6).

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

fn master_fader(d: &Document) -> T {
    T::StripFader(d.mixer().master())
}

#[test]
fn set_replaces_by_source_and_remove_takes_it_back() {
    let d = doc();
    let s = src("Knobs", LearnKind::Cc7 { channel: 0, cc: 7 });
    let map = LearnMap { source: s.clone(), target: master_fader(&d), mode: LearnMode::Absolute };
    let cmd = Command::Settings(SettingsCmd::SetLearn { map: map.clone() });
    assert_eq!(cmd.name(), "MIDI Learn");
    let d = apply(&d, &cmd).unwrap().doc;
    assert_eq!(d.settings().learn, vec![map.clone()]);
    let relative = LearnMap { mode: LearnMode::Relative2sComplement, ..map.clone() };
    let d = apply(&d, &Command::Settings(SettingsCmd::SetLearn { map: relative.clone() })).unwrap().doc;
    assert_eq!(d.settings().learn, vec![relative], "same source: replaced");
    let rm = Command::Settings(SettingsCmd::RemoveLearn { source: s.clone() });
    assert_eq!(rm.name(), "Remove MIDI Learn");
    let d = apply(&d, &rm).unwrap().doc;
    assert!(d.settings().learn.is_empty());
    assert!(apply(&d, &rm).is_err(), "nothing to remove");
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
        let map = LearnMap { source: bad.clone(), target: t, mode: LearnMode::Absolute };
        assert!(apply(&d, &Command::Settings(SettingsCmd::SetLearn { map })).is_err(), "{bad:?}");
    }
    let ok = src("K", LearnKind::Cc7 { channel: 0, cc: 1 });
    let gone = T::StripFader(core_model::ids::StripId(987_654));
    assert!(apply(&d, &Command::Settings(SettingsCmd::SetLearn {
        map: LearnMap { source: ok.clone(), target: gone, mode: LearnMode::Absolute }
    }))
    .is_err(), "the target must exist");
    assert!(apply(&d, &Command::Settings(SettingsCmd::SetLearn {
        map: LearnMap { source: ok, target: T::VelocityScale(core_model::ids::DestId(1)), mode: LearnMode::Absolute }
    }))
    .is_err());
}

#[test]
fn the_map_count_is_bounded() {
    let mut d = doc();
    let t = master_fader(&d);
    for i in 0..MAX_LEARN_MAPS {
        let s = src(&format!("P{}", i / 128), LearnKind::Cc7 { channel: 0, cc: (i % 128) as u8 });
        d = apply(&d, &Command::Settings(SettingsCmd::SetLearn {
            map: LearnMap { source: s, target: t, mode: LearnMode::Absolute }
        }))
        .unwrap()
        .doc;
    }
    let one_more = src("Extra", LearnKind::Note { channel: 0, note: 1 });
    assert!(apply(&d, &Command::Settings(SettingsCmd::SetLearn {
        map: LearnMap { source: one_more, target: t, mode: LearnMode::Absolute }
    }))
    .is_err());
}

#[test]
fn learn_maps_round_trip_and_are_absent_when_empty() {
    let d = doc();
    let json = serde_json::to_string(d.settings()).unwrap();
    assert!(!json.contains("learn"), "an empty list is not written: {json}");
    let map = LearnMap {
        source: src("Pads", LearnKind::Note { channel: 9, note: 36 }),
        target: T::StripMute(d.mixer().master()),
        mode: LearnMode::Absolute,
    };
    let d = apply(&d, &Command::Settings(SettingsCmd::SetLearn { map: map.clone() })).unwrap().doc;
    let json = serde_json::to_string(d.settings()).unwrap();
    let back: core_model::document::Settings = serde_json::from_str(&json).unwrap();
    assert_eq!(back.learn, vec![map]);
    assert_eq!(core_model::file::VERSION, 3);
}

#[test]
fn sources_describe_themselves() {
    assert_eq!(src("Knobs", LearnKind::Cc7 { channel: 0, cc: 7 }).describe(), "Knobs · CC 7 ch 1");
    assert_eq!(src("Knobs", LearnKind::Cc14 { channel: 1, cc: 1 }).describe(), "Knobs · CC 1/33 ch 2");
    assert_eq!(src("Keys", LearnKind::PitchBend { channel: 0 }).describe(), "Keys · Pitch Bend ch 1");
    assert_eq!(src("Pads", LearnKind::Note { channel: 9, note: 60 }).describe(), "Pads · Note C3 ch 10");
}
VIZDAW_EOF
```

Middle C is `C3` here, as the editors name note 60 (`PitchMath`). If the tree's note-name helper in `core-model` names it differently, use the tree's name in both the code and the test, and say so in the report.

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

- [ ] **Step 4: Run the tests and the gates.** Run `cargo test -p core-model` (the insta snapshots must be unchanged), `cargo test -p core-ffi --lib`, and both clippies. Expected: core-model +5 and every existing snapshot unchanged.

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-model/src/learn.rs crates/core-model/src/lib.rs crates/core-model/src/document.rs crates/core-model/src/command/settings.rs crates/core-model/tests/learn.rs
git commit -m "core-model: MIDI-learn maps in the project settings, set and removed as undo steps (plan 5f-1 Task 4)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -8
```
Counts: Rust +5.

---

### Task 5: `core-surface` learn: detection, learned maps, command maps

**Files:**
- Modify: `crates/core-surface/src/learn.rs`
- Test: create `crates/core-surface/tests/learn.rs`

**Interfaces:**
- Consumes: `core_model::learn::*` (Task 4), `IdleTouches` (Task 1).
- Produces:
  ```rust
  pub const LEARN_WINDOW: Duration = Duration::from_millis(300);
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
  #[derive(Debug, Default)] pub struct Mapper { /* maps, commands, idle, msb hold, last sent, cc ≥64 state */ }
  impl Mapper {
      pub fn set(&mut self, maps: Vec<LearnMap>, commands: Vec<CommandMap>);
      pub fn listens(&self, port: &str) -> bool;
      /// Ruling 8; `current` gives a target's value now (None: unknown → the message is dropped for relative maps).
      pub fn input(&mut self, port: &str, msg: &[u8], now: Instant, current: &dyn Fn(AutomationTarget) -> Option<f64>) -> Vec<SurfaceAction>;
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
//! MIDI learn (plan 5f-1 Task 5; spec §8.2, §12.3; Rulings 7, 8).

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

fn mapper(mode: M, kind: K, target: T) -> Mapper {
    let mut m = Mapper::default();
    m.set(vec![LearnMap { source: src(kind), target, mode }], vec![]);
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
    assert_eq!(m.tick(now + IDLE_RELEASE), vec![A::Gesture(f, Phase::Release)]);
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

#[test]
fn a_lone_msb_is_a_7_bit_value() {
    let f = T::StripFader(StripId(1));
    let mut m = mapper(M::Absolute, K::Cc14 { channel: 0, cc: 1 }, f);
    let now = Instant::now();
    match m.input("Knobs", &[0xB0, 1, 127], now, &|_| Some(0.0)).as_slice() {
        [A::Gesture(_, Phase::Touch(Some(x)))] => assert_eq!(*x, 1.0),
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
    assert!(m.input("Knobs", &[0xB0, 20, 64], now, &|_| Some(0.5)).len() <= 1, "64 is no move");
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
VIZDAW_EOF
```

- [ ] **Step 2: Run them to see them fail** (`cargo test -p core-surface --test learn`). Expected: FAIL — `Learner` not found.

- [ ] **Step 3: Implement `learn.rs`** by Rulings 7–8.

- [ ] **Step 4: Run the tests and the gates.** Expected: 30 passed in `core-surface`.

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-surface/src/learn.rs crates/core-surface/tests/learn.rs
git commit -m "core-surface: MIDI learn detection (14-bit pairs, encoder styles), learned maps with 500 ms release and feedback, command maps (plan 5f-1 Task 5)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -6
```
Counts: Rust +9.

---

### Task 6: Gesture holders (carry m8): `Session.gesture_from`

**Files:**
- Modify: `crates/core-ffi/src/automation.rs` (`Holder`, `Holders`, `GestureHolderRow`, `Touching.holders`), `crates/core-ffi/src/session.rs` (`gesture`, `gesture_from`, `touch`, `move_to`, `release_from`), `app/Sources/VizDAWAppModel/AutomationCommands.swift` (the channel's holder), `app/Tests/VizDAWAppModelTests/FakeSession.swift`, `app/Tests/VizDAWAppModelTests/AutomationCommandsTests.swift`
- Test: create `crates/core-ffi/tests/holders.rs`

**Interfaces:**
- Consumes: 5e-1's `Touching`, `Source`, `touch`, `move_to` and `release`.
- Produces:
  ```rust
  // automation.rs
  /// Who holds a touch (Ruling 2).
  #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
  pub(crate) enum Holder { Mouse, Channel, Plugin, Learn, Surface(u32) }
  /// A set of holders (a bit per holder; surfaces 0–55).
  #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
  pub(crate) struct Holders(u64);
  impl Holders { pub fn of(h: Holder) -> Holders; pub fn has(self, h: Holder) -> bool; pub fn with(self, h: Holder) -> Holders; pub fn without(self, h: Holder) -> Holders; pub fn is_empty(self) -> bool; }
  /// The app's holders (the channel is its own, Ruling 2).
  #[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
  pub enum GestureHolderRow { Mouse, Channel }
  ```
  - `Touching` gains `holders: Holders`.
  - `Session::touch(…, holder: Holder)` and `Session::move_to(…, holder: Holder)` take the holder (every call site passes one: the export passes its row's, and the plugin paths pass `Holder::Plugin`).
  - `fn release_from(&self, inner: &mut Inner, t: AutomationTarget, h: Holder, host_ns: Option<u64>)` removes `h`. When the set becomes empty it calls `release`; otherwise it only flushes the sampler.
  - `release` itself still ends the touch whoever holds it; `release_touches` and `release_vanished` keep calling it.
  - Exported: `Session::gesture_from(&self, target: AutomationTargetRow, phase: GesturePhaseRow, holder: GestureHolderRow) -> Result<(), VizError>`. `Session::gesture` is `self.gesture_from(target, phase, GestureHolderRow::Mouse)`.
  - For later tasks: `pub(crate) fn gesture_held(&self, inner: &mut Inner, t: AutomationTarget, phase: core_surface::Phase, h: Holder) -> Result<(), VizError>`, the same path for surfaces and learn.

Joining and plugin rules:
- In `touch`, the existing "Touched already" branch adds `holder` to `holders` before it moves the target.
- The plugin paths keep their 500 ms release through `plugin_touches`, which calls `release_from(…, Holder::Plugin, …)`.
- The plugin check at session.rs:6113 (`x.source == Source::Plugin`) stays.

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-ffi/tests/holders.rs <<'VIZDAW_EOF'
//! One target, several hands (plan 5f-1 Task 6; Ruling 2; 5e-2 carry m8).
//! Offline only.

mod common;

use std::sync::{Arc, Mutex, MutexGuard};
use vizdaw_ffi::automation::{GestureHolderRow as H, GesturePhaseRow as Phase};
use vizdaw_ffi::host::{force_offline, reset_host};
use vizdaw_ffi::{AutomationTargetRow as Target, Session};

static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    force_offline();
    reset_host();
    g
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
    let _ = common::wait_limit();
}
VIZDAW_EOF
```

The last line keeps the shared `common` module used, so no dead-code warning fires.

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
               let h = Holder::from(holder);
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

   Add `holders: Holders::of(holder),` to the `Touching { … }` literal at `inner.touches.insert(`. In the plugin paths, replace `self.release(inner, t, Some(ns))` and `self.release(inner, t, ns)` (session.rs:6055 and :6115) with `self.release_from(inner, t, Holder::Plugin, …)`.

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

   In `FakeSession.swift`, add `var holders: [GestureHolderRow] = []` and `func gestureFrom(target:phase:holder:)`, which records into `gestures` as `gesture` does and appends the holder. Then add a test to `AutomationCommandsTests.swift`:

   ```swift
   @Test func theChannelGesturesAsItsOwnHolder() throws {
       let (m, f) = try model()
       _ = run("automation.gesture", ["target": .text("fader:1"), "phase": .text("touch")], m)
       #expect(f.holders == [.channel])
   }
   ```
   Use that file's own `model()`/`run` helpers. If they are named differently, use its names.

- [ ] **Step 4: Run the tests and the gates.** Run `cargo test --workspace`, both clippies, `cargo clippy -p core-ffi --lib -- -D warnings`, `scripts/build-ffi.sh`, `scripts/swift.sh build` and `scripts/swift.sh test`. Expected: Rust +3, Swift +1, and every existing automation test green (`tests/automation.rs` uses `gesture`, which still means Mouse).

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-ffi/src/automation.rs crates/core-ffi/src/session.rs crates/core-ffi/tests/holders.rs app/Sources/VizDAWAppModel/AutomationCommands.swift app/Tests/VizDAWAppModelTests/FakeSession.swift app/Tests/VizDAWAppModelTests/AutomationCommandsTests.swift
git commit -m "core-ffi: a touch is held by each of its holders and ends with the last release (plan 5f-1 Task 6; 5e-2 carry m8)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -8
```
Counts: Rust +3, Swift +1.

---

### Task 7: `core-ffi::surfaces`: the registry, the ports, `surfaces.json`, add/remove, connection

**Files:**
- Create: `crates/core-ffi/src/surfaces.rs`, `crates/core-ffi/src/surfaces/ports.rs`
- Modify: `crates/core-ffi/src/lib.rs`
- Test: create `crates/core-ffi/tests/surfaces.rs`

**Interfaces:**
- Consumes: `core_surface::{new_surface, Surface, SurfaceKind, Bank, learn::CommandMap}`, `crate::recovery::support_dir`, `crate::host::offline_mode()` (public at `ae49da1`).
- Produces:
  ```rust
  // surfaces.rs
  #[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)] pub enum SurfaceKindRow { Mcu, McuXt, Hui }
  #[derive(Debug, Clone, PartialEq, uniffi::Record)]
  pub struct SurfaceRow { pub id: u32, pub kind: SurfaceKindRow, pub input: String, pub output: String, pub position: u8,
      /// Both ports present.
      pub connected: bool, /// Answered (MCU handshake or any valid message; HUI ping).
      pub online: bool, pub ignored: u64 }
  #[derive(Debug, Clone, PartialEq, Default, uniffi::Record)] pub struct SurfacePortsRow { pub sources: Vec<String>, pub destinations: Vec<String> }
  /// Reads surfaces.json (Ruling 9); problems go to `state.get surfaces`.
  #[uniffi::export] pub fn surfaces_load() -> Result<(), VizError>;
  #[uniffi::export] pub fn surface_list() -> Vec<SurfaceRow>;
  /// `position` None: the next free one in its group. Refused: a second MCU, a fourth XT, an empty or control-character or > 255-byte port.
  #[uniffi::export] pub fn surface_add(kind: SurfaceKindRow, input: String, output: String, position: Option<u8>) -> Result<u32, VizError>;
  #[uniffi::export] pub fn surface_remove(id: u32) -> Result<(), VizError>;
  #[uniffi::export] pub fn surface_ports() -> SurfacePortsRow;
  /// Raw input from a port (CoreMIDI's thread, `midi.inject`, tests): queued, never waits.
  pub(crate) fn input(port: &str, bytes: &[u8]) -> bool;  // true: a surface or learn listens to `port`
  /// Process-wide state (`surfaces::lock()`), the group banks and the input queue.
  pub(crate) struct Registry { … }
  #[cfg(feature = "test-hooks")] pub fn debug_reset();                 // forget everything, fresh FakePorts
  #[cfg(feature = "test-hooks")] pub fn debug_set_missing(ports: Vec<String>);
  #[cfg(feature = "test-hooks")] pub fn debug_sent(port: &str) -> Vec<Vec<u8>>;   // drains
  #[cfg(feature = "test-hooks")] pub fn debug_advance(ms: u64);         // the surfaces clock
  #[cfg(feature = "test-hooks")] pub fn debug_support_file() -> std::path::PathBuf;
  // surfaces/ports.rs
  pub(crate) trait Ports: Send { fn send(&mut self, port: &str, bytes: &[u8]) -> bool; fn sources(&self) -> Vec<String>; fn destinations(&self) -> Vec<String>; fn refresh(&mut self); }
  pub(crate) struct FakePorts { … }   // every name present unless missing; records sends
  #[cfg(all(target_os = "macos", not(feature = "test-hooks")))]
  pub(crate) struct CoreMidiPorts { … } // SourceListener + MidiSystem (Ruling 10)
  ```

Connection handling:
- A surface is `connected` when its input is in `sources()` and its output is in `destinations()`.
- `surface_add` (and `surfaces_load`) checks the ports at once; a connected surface is sent its `hello()` before the call returns.
- When a connected surface's port vanishes:
  - the registry records `release_owed` for its touched targets (the service releases them in the focused session, Task 8);
  - it marks the surface disconnected.
- When the port returns, the surface is `refresh()`ed and `hello()` is sent.
- Disconnected surfaces trigger `refresh()` at most every 2 s (Ruling 10), measured on the surfaces clock.

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
    debug_support_file, surface_add, surface_list, surface_remove, surfaces_load,
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
}

#[test]
fn a_fifth_unit_or_a_second_main_is_refused() {
    let _g = serial();
    surface_add(Kind::Mcu, "A".into(), "A".into(), None).unwrap();
    assert!(surface_add(Kind::Mcu, "B".into(), "B".into(), None).is_err());
    for n in 0..3 {
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
}

#[test]
fn a_vanished_port_disconnects_and_releases_its_touches() {
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
    assert_eq!(std::fs::read_to_string(f.with_extension("json.bad")).unwrap().contains("99"), true);
}

pub fn keep(_: Arc<vizdaw_ffi::Session>) {
    let _ = common::wait_limit();
}
VIZDAW_EOF
```

`with_extension("json.bad")` on `surfaces.json` gives `surfaces.json.bad`. The two `Session` calls here (`focus_surfaces`, and `idle`'s service) arrive with Task 8. In this task, give `Session::focus_surfaces` and the `idle` hook their minimal form (the hook only runs connection checks), and Task 8 fills them in.

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

   Write `surfaces.rs` (the registry behind a `Mutex`, `surfaces.json` by Ruling 9 with temp-and-rename writes, the exports above, the surfaces clock `now()` = `Instant` captured at first use + real elapsed + `debug_advance` offset) and `surfaces/ports.rs` (Ruling 10). Port names reuse `crate::live::parse_injection`'s rule: non-empty, at most 255 bytes, no control characters. In `session.rs`, add the minimal `Session::focus_surfaces(&self) -> Result<(), VizError>`, which sets the focus to `self.id`. Then:

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
   with `fn service_surfaces(&self)` in a new `impl Session` block in `surfaces.rs` (connection checks only in this task).

- [ ] **Step 4: Run the tests and the gates.** Expected: Rust +6.

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-ffi/src/surfaces.rs crates/core-ffi/src/surfaces/ports.rs crates/core-ffi/src/lib.rs crates/core-ffi/src/session.rs crates/core-ffi/tests/surfaces.rs
git commit -m "core-ffi: the surfaces registry, fake and CoreMIDI ports, surfaces.json, connect and reconnect (plan 5f-1 Task 7)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -8
```
Counts: Rust +6.

---

### Task 8: The service: actions into the focused session, the MixerView, feedback timing, learn

**Files:**
- Modify: `crates/core-ffi/src/surfaces.rs`, `crates/core-ffi/src/session.rs` (`inject_midi`)
- Test: modify `crates/core-ffi/tests/surfaces.rs`; create `crates/core-ffi/tests/learn.rs`

**Interfaces:**
- Consumes: Tasks 5–7; `Session::gesture_held` (Task 6); `crate::mixer::snapshot`, `host::levels`, `position_bbu`, `Session::params` (plugin parameters), `core_model::learn`.
- Produces:
  ```rust
  #[derive(Debug, Clone, PartialEq, uniffi::Enum)]
  pub enum SurfaceEventRow { Command { id: String }, Select { strip: u64 }, Arm { strip: u64 }, Jog { detents: i32 } }
  #[derive(Debug, Clone, PartialEq, uniffi::Record)] pub struct SurfaceFeedbackRow { pub surface: u32, pub messages: Vec<String> }  // hex, last 64
  #[derive(Debug, Clone, PartialEq, Default, uniffi::Record)]
  pub struct SurfaceStateRow { pub surfaces: Vec<SurfaceRow>, pub feedback: Vec<SurfaceFeedbackRow>, pub learning: Option<String>, pub problems: Vec<String> }
  #[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)] pub enum LearnKindRow { Cc7, Cc14, PitchBend, Note }
  #[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)] pub enum LearnModeRow { Absolute, Relative2sComplement, RelativeBinOffset }
  #[derive(Debug, Clone, PartialEq, uniffi::Record)] pub struct LearnSourceRow { pub port: String, pub kind: LearnKindRow, pub channel: u8, pub number: u8 }
  #[derive(Debug, Clone, PartialEq, uniffi::Record)] pub struct LearnMapRow { pub source: LearnSourceRow, pub text: String, pub target: AutomationTargetRow, pub missing: bool, pub mode: LearnModeRow }
  #[derive(Debug, Clone, PartialEq, uniffi::Record)] pub struct CommandMapRow { pub source: LearnSourceRow, pub text: String, pub command: String }
  #[derive(Debug, Clone, PartialEq, uniffi::Enum)] pub enum LearnForRow { Target { target: AutomationTargetRow }, Command { id: String } }
  #[derive(Debug, Clone, PartialEq, Default, uniffi::Record)] pub struct LearnStateRow { pub waiting: Option<String>, pub last: Option<String> }
  #[uniffi::export] pub fn command_maps() -> Vec<CommandMapRow>;
  #[uniffi::export] pub fn remove_command_map(command: String) -> Result<(), VizError>;
  /// Ruling 11: needs a focused session to complete.
  #[uniffi::export] pub fn learn_command(id: String) -> Result<(), VizError>;
  #[uniffi::export] pub fn cancel_learn();
  #[uniffi::export] pub fn learn_state() -> LearnStateRow;
  // Session (exported)
  pub fn focus_surfaces(&self) -> Result<(), VizError>;
  pub fn set_surface_selection(&self, strips: Vec<u64>) -> Result<(), VizError>;
  /// Feeds `bytes` to surface `surface` as if from its input and services this session at once.
  pub fn inject_surface(&self, surface: u32, bytes: Vec<u8>) -> Result<(), VizError>;
  pub fn surface_state(&self) -> Result<SurfaceStateRow, VizError>;
  pub fn take_surface_events(&self) -> Result<Vec<SurfaceEventRow>, VizError>;
  pub fn learn_target(&self, target: AutomationTargetRow) -> Result<(), VizError>;
  pub fn learn_maps(&self) -> Result<Vec<LearnMapRow>, VizError>;
  pub fn set_learn_mode(&self, source: LearnSourceRow, mode: LearnModeRow) -> Result<DocSnapshot, VizError>;
  pub fn remove_learn(&self, source: LearnSourceRow) -> Result<DocSnapshot, VizError>;
  ```

`service_surfaces` (focused session only) runs in this order:
1. Release `release_owed` targets (vanished ports, a focus change) through `release_from(…, Holder::Surface(id))`.
2. Drain the input queue. Each message goes to the surface whose input port it came from (`Surface::input`), else to the learner (when learning, and the port is not a surface's), else to the `Mapper` (maps from `Settings.learn` plus the command maps).
3. Apply the actions:
   - `Gesture` → `gesture_held` with the surface's or `Learn`'s holder. A refused gesture (a vanished target) is counted in that surface's `ignored`, never raised.
   - `Bank`/`Channel` → the group's `Bank::shift`, then a full refresh of that group.
   - `Select`/`Arm`/`Jog`/`Command` → queued `SurfaceEventRow`s for `take_surface_events`.
4. Run `tick` on every surface and the mapper.
5. Build the `MixerView` from the session (Ruling 12's arm LED; `select` from `set_surface_selection`; `plugin` from the first selected strip's first insert's first 8 parameters; meters from `host::levels(self.id)`).
6. Send feedback by Ruling 13: units whose last feedback was ≥ 20 ms ago, plus at once any unit or learn map showing a target that a holder other than it moved in this service.

A learn decision either executes `SettingsCmd::SetLearn` on this session (undo "MIDI Learn") or stores a `CommandMap` and saves `surfaces.json`.

In `session.rs`, `inject_midi` feeds learn and maps (Ruling 16):

Find:
```rust
            let msgs = crate::live::parse_injection(&port, &bytes)?;
            let Some(tx) = host::midi_sender(self.id) else {
                return Ok(false);
            };
```
Replace:
```rust
            let msgs = crate::live::parse_injection(&port, &bytes)?;
            // MIDI learn and learned maps hear injected bytes too (plan 5f-1
            // Ruling 16); serviced by the focused session's idle.
            let learnt = crate::surfaces::input(&port, &bytes);
            let Some(tx) = host::midi_sender(self.id) else {
                return Ok(learnt);
            };
```

- [ ] **Step 1: Write the failing tests.** Append to `crates/core-ffi/tests/surfaces.rs`:

```rust
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
        vec![E::Command { id: "transport.play_stop".into() }, E::Select { strip: synth }, E::Jog { detents: 2 }]
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
    s.idle().unwrap();
    assert!(debug_sent("T Out").iter().all(|m| m[0] & 0xF0 != 0xE0), "no fader feedback to the hand");
    let _ = b;
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
fn closing_the_focused_session_releases_surface_touches() {
    let _g = serial();
    let s = vizdaw_ffi::Session::new_demo().unwrap();
    let (_, k) = synth(&s);
    let id = mcu(&s);
    s.inject_surface(id, vec![0x90, 0x68 + k as u8, 0x7F]).unwrap();
    let other = vizdaw_ffi::Session::new_demo().unwrap();
    other.focus_surfaces().unwrap();
    other.idle().unwrap();
    s.idle().unwrap();
    assert!(s.automation_state().unwrap().touched.is_empty(), "focus moved: released");
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
}
```

`inject_surface` takes at most `core_ffi::live::MAX_INJECT_BYTES` bytes.

Then create `crates/core-ffi/tests/learn.rs`:

```rust
//! MIDI learn in the core (plan 5f-1 Task 8; spec §4.8, §8.2).

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
    assert!(learn_state().waiting.is_none());
    s.inject_midi("Knobs".into(), vec![0xB0, 7, 0]).unwrap();
    s.idle().unwrap();
    assert!(s.automation_state().unwrap().touched.contains(&t));
    debug_advance(501);
    s.idle().unwrap();
    assert!(s.automation_state().unwrap().touched.is_empty(), "released 500 ms after");
    assert_eq!(s.snapshot().unwrap().undo_label.as_deref(), Some("Set Fader"));
    s.set_learn_mode(maps[0].source.clone(), Mode::RelativeBinOffset).unwrap();
    assert_eq!(s.learn_maps().unwrap()[0].mode, Mode::RelativeBinOffset);
    s.remove_learn(maps[0].source.clone()).unwrap();
    assert!(s.learn_maps().unwrap().is_empty());
}

#[test]
fn a_command_map_is_learnt_into_the_app_preferences() {
    let _g = serial();
    let s = Session::new_demo().unwrap();
    s.focus_surfaces().unwrap();
    learn_command("transport.play_stop".into()).unwrap();
    s.inject_midi("Pads".into(), vec![0x99, 36, 100]).unwrap();
    debug_advance(301);
    s.idle().unwrap();
    assert_eq!(command_maps()[0].command, "transport.play_stop");
    s.inject_midi("Pads".into(), vec![0x99, 36, 100]).unwrap();
    s.idle().unwrap();
    use vizdaw_ffi::surfaces::SurfaceEventRow as E;
    assert_eq!(s.take_surface_events().unwrap(), vec![E::Command { id: "transport.play_stop".into() }]);
    remove_command_map("transport.play_stop".into()).unwrap();
    assert!(command_maps().is_empty());
}

#[test]
fn hostile_learn_calls_are_refused() {
    let _g = serial();
    let s = Session::new_demo().unwrap();
    assert!(s.learn_target(Target::StripFader { strip: 987_654 }).is_err());
    assert!(s.learn_target(Target::VelocityScale { dest: 1 }).is_err());
    assert!(learn_command("no.such_command".into()).is_err(), "registry IDs only");
    assert!(learn_command(String::new()).is_err());
    assert!(remove_command_map("transport.play_stop".into()).is_err());
    let bad = vizdaw_ffi::surfaces::LearnSourceRow {
        port: "x".into(),
        kind: vizdaw_ffi::surfaces::LearnKindRow::Cc7,
        channel: 99,
        number: 7,
    };
    assert!(s.set_learn_mode(bad.clone(), Mode::Absolute).is_err());
    assert!(s.remove_learn(bad).is_err());
    cancel_learn();
    assert!(learn_state().waiting.is_none());
    let _ = common::wait_limit();
}
```

The target-row variant names (`VelocityScale { dest }`) must match `crate::events::AutomationTargetRow` in the tree; adapt if they differ.

- [ ] **Step 2: Run them to see them fail** (`cargo test -p core-ffi --test surfaces --test learn`). Expected: FAIL — the new methods are not found.

- [ ] **Step 3: Implement the service, the learn glue and the exports**, and change `inject_midi` as shown.

- [ ] **Step 4: Run the tests and the gates.** Run `cargo test -p core-ffi`, both clippies and `cargo clippy -p core-ffi --lib -- -D warnings`. Then run `scripts/build-ffi.sh`, `scripts/swift.sh build` and `scripts/swift.sh test`: add `FakeSession` stubs for the eight new `Session` methods, recording calls and returning empty rows. Expected: Rust +11, and `live_input.rs`'s `!inject_midi(…)` stays false (no learn listens to "Keys").

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-ffi/src/surfaces.rs crates/core-ffi/src/session.rs crates/core-ffi/tests/surfaces.rs crates/core-ffi/tests/learn.rs app/Tests/VizDAWAppModelTests/FakeSession.swift
git commit -m "core-ffi: surfaces drive the focused session, feedback every 20 ms with partners at once, MIDI learn and learned maps (plan 5f-1 Task 8)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -8
```
Counts: Rust +11.

---

### Task 9: The control channel: `surface.*`, `state.get surfaces`

**Files:**
- Modify: `crates/core-control/src/registry.rs`, `crates/core-control/src/request.rs`, `crates/core-control/tests/commands.rs`, `crates/core-ffi/src/control.rs`, `app/Sources/VizDAWAppModel/ControlCommands.swift`
- Create: `app/Sources/VizDAWAppModel/SurfaceCommands.swift`, `app/Tests/VizDAWAppModelTests/SurfaceCommandsTests.swift`

**Interfaces:**
- Consumes: Tasks 7–8.
- Produces:
  - Registry entries, all `key_only`, in menu group "Surfaces":
    - `surface.add { type: mcu|mcu_xt|hui, input, output, position? 0–3 }`;
    - `surface.remove { surface }`;
    - `surface.inject { surface, bytes }` (hex text);
    - `surface.learn { target? | command? }` (exactly one);
    - `surface.unlearn { target? | command? }`.
  - `StateTarget::Surfaces` (`"surfaces"`), `StateTargetRow::Surfaces` and `ControlResult::Surfaces { doc, state: SurfaceStateRow }`, whose JSON is:
    `{ "doc", "surfaces": [{ "id", "type", "input", "output", "position", "connected", "online", "ignored" }], "feedback": [{ "surface", "messages" }], "learning", "problems" }`.
  - Swift `SurfaceCommands` (`ids`, `run`, `parseKind`), handled from `ControlCommands.run`. `surface.add` answers `{ "surface": id }` through the `ran` result's status (`"surface 3"`), as `destination.add_midi` answers its ID. Use the same mechanism the tree uses for that.

- [ ] **Step 1: Write the failing tests.** In `crates/core-control/tests/commands.rs`, change the count:

   Find:
   ```rust
           138,
   ```
   Replace:
   ```rust
           143,
   ```
   and add "; 5f-1 adds surface.add, surface.remove, surface.inject, surface.learn and surface.unlearn" at the end of the count message. Then add:

```rust
#[test]
fn surface_commands_check_their_params() {
    let run = |id: &str, params: Value| json!({ "id": id, "params": params });
    for (id, params) in [
        ("surface.add", json!({ "type": "mcu", "input": "A", "output": "B" })),
        ("surface.add", json!({ "type": "mcu_xt", "input": "A", "output": "B", "position": 3 })),
        ("surface.add", json!({ "type": "hui", "input": "A", "output": "B" })),
        ("surface.remove", json!({ "surface": 1 })),
        ("surface.inject", json!({ "surface": 1, "bytes": "90 68 7f" })),
        ("surface.learn", json!({ "target": "fader:3" })),
        ("surface.learn", json!({ "command": "transport.play_stop" })),
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
        assert!(err("command.run", run(id, params.clone())).code == ErrorCode::InvalidParams, "{id} {params}");
    }
    assert_eq!(
        ok("state.get", json!({ "target": "surfaces" })),
        Command::StateGet { target: StateTarget::Surfaces, window: None }
    );
}
```

Use the file's existing `ok`/`err` helpers and imports. The "exactly one of target and command" rule is checked in the app (below), because the registry has no "one of" kind.

Then create `app/Tests/VizDAWAppModelTests/SurfaceCommandsTests.swift`:

```swift
import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

/// `surface.*` from the channel (plan 5f-1 Task 9).
@MainActor
@Suite struct SurfaceCommandsTests {
    @Test func surfaceCommandsCheckTheirArguments() throws {
        let f = FakeSession()
        let m = try DocumentModel(session: f)
        for (id, args) in [
            ("surface.inject", [ControlArg(name: "surface", value: .int(1)), ControlArg(name: "bytes", value: .text("9g"))]),
            ("surface.inject", [ControlArg(name: "surface", value: .int(1)), ControlArg(name: "bytes", value: .text("90 68"))]),
            ("surface.learn", []),
            ("surface.learn", [ControlArg(name: "target", value: .text("fader:1")), ControlArg(name: "command", value: .text("edit.undo"))]),
            ("surface.learn", [ControlArg(name: "target", value: .text("fader"))]),
            ("surface.unlearn", []),
        ] {
            guard case .failed(let e) = SurfaceCommands.run(id, args: args, doc: UUID(), m) else {
                Issue.record("\(id) \(args) ran"); continue
            }
            #expect(e.code == .invalidParams, "\(id)")
        }
        #expect(f.surfaceInjections.isEmpty && f.learnTargets.isEmpty)
    }

    @Test func injectAndLearnReachTheSession() throws {
        let f = FakeSession()
        let m = try DocumentModel(session: f)
        _ = SurfaceCommands.run(
            "surface.inject", args: [ControlArg(name: "surface", value: .int(2)), ControlArg(name: "bytes", value: .text("90 68 7f e0 00 40"))],
            doc: UUID(), m)
        #expect(f.surfaceInjections.map(\.0) == [2])
        #expect(f.surfaceInjections.first?.1 == Data([0x90, 0x68, 0x7F, 0xE0, 0x00, 0x40]))
        _ = SurfaceCommands.run("surface.learn", args: [ControlArg(name: "target", value: .text("pan:3"))], doc: UUID(), m)
        #expect(f.learnTargets == [.stripPan(strip: 3)])
    }
}
```

Use the tree's `ControlArg` initializer and value case names; adapt the literals if they differ. `"90 68"` is a cut message, which the app refuses through `ControlLanes.hexBytes` plus a whole-message check (`core_surface::midi::split` has no Swift twin; the core refuses it with `Model`, which the app maps to `invalid_params`). Add `surfaceInjections` and `learnTargets` to `FakeSession`.

- [ ] **Step 2: Run them to see them fail** (`cargo test -p core-control --test commands`; `scripts/swift.sh test`). Expected: FAIL — count 138, unknown IDs.

- [ ] **Step 3: Implement.** In `registry.rs`, add the `SURFACE_ADD_PARAMS`, `SURFACE_ID_PARAMS`, `SURFACE_INJECT_PARAMS` and `SURFACE_LEARN_PARAMS` constants (the `target` text as `automation.gesture` spells it; `command` text) after `AUTOMATION_GESTURE_PARAMS`. Add the five entries after `automation.suspend`:

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
   and add ", surfaces" to the error text before " or automation" (it becomes "… input, automation or surfaces"). Mirror this in `control.rs`: `StateTargetRow::Surfaces`, the `StateTarget::Surfaces => StateTargetRow::Surfaces` arm after the Automation one, `ControlResult::Surfaces` and its JSON after `ControlResult::Automation`'s. In Swift, `ControlCommands.state` gets `case .surfaces:`, which answers `.surfaces(doc:state: try m.session.surfaceState())`. `handled` gains `.union(SurfaceCommands.ids)`, and `run` gains `case _ where SurfaceCommands.ids.contains(id): return SurfaceCommands.run(id, args: args, doc: docID, m)` beside the AutomationCommands case. `surface.add` and `surface.remove` take no document: handle them before the `guard let (docID, m) = doc` line, as `AutomationCommands.suspend` is.

- [ ] **Step 4: Run the tests and the gates.** Expected: Rust +1 (core-control), Swift +2, and `everyRegisteredMenuCommandHasAHandler` green.

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-control/src/registry.rs crates/core-control/src/request.rs crates/core-control/tests/commands.rs crates/core-ffi/src/control.rs app/Sources/VizDAWAppModel/ControlCommands.swift app/Sources/VizDAWAppModel/SurfaceCommands.swift app/Tests/VizDAWAppModelTests/SurfaceCommandsTests.swift app/Tests/VizDAWAppModelTests/FakeSession.swift
git commit -m "control: surface.add, surface.remove, surface.inject, surface.learn, surface.unlearn and state.get surfaces (plan 5f-1 Task 9)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -10
```
Counts: Rust +1, Swift +2.

---

### Task 10: The app: Settings › Surfaces, loading at launch, focus, the event relay

**Files:**
- Create: `app/Sources/VizDAWAppModel/SurfaceSettings.swift`, `app/Sources/VizDAWAppModel/SurfaceRelay.swift`, `app/Sources/VizDAW/SurfacesSettingsView.swift`, `app/Tests/VizDAWAppModelTests/SurfaceSettingsTests.swift`
- Modify: `app/Sources/VizDAW/VizDAWApp.swift` (the tab, `surfacesLoad` at launch, focus in the ticker), `app/Sources/VizDAWAppModel/DocumentModel.swift` (drain events; report selection)

**Interfaces:**
- Consumes: the free functions and `Session` methods of Tasks 7–8.
- Produces:
  ```swift
  /// The core's surfaces as Settings › Surfaces edits them (Rulings 9–11).
  public protocol SurfaceCore { func list() -> [SurfaceRow]; func ports() -> SurfacePortsRow; func add(_ k: SurfaceKindRow, _ i: String, _ o: String, _ p: UInt8?) throws -> UInt32; func remove(_ id: UInt32) throws }
  @MainActor @Observable public final class SurfaceSettings {
      public init(core: any SurfaceCore)
      public private(set) var rows: [SurfaceRow]
      public private(set) var ports: SurfacePortsRow
      public var alert: AlertItem?
      public func refresh()            // Settings polls it at 2 Hz while open
      public func add(_ kind: SurfaceKindRow, input: String, output: String, position: UInt8?)
      public func remove(_ id: UInt32)
      /// "MCU-compatible", "MCU XT-compatible", "HUI-compatible".
      nonisolated public static func title(_ k: SurfaceKindRow) -> String
      /// "Connected", "Connected · not answering", "Disconnected".
      nonisolated public static func status(_ r: SurfaceRow) -> String
  }
  @MainActor public enum SurfaceRelay {
      /// Ruling 12: select, arm and jog on `m`; commands through `run`.
      public static func apply(_ events: [SurfaceEventRow], _ m: DocumentModel, mixer: MixerModel?, run: (String) -> Void)
  }
  extension DocumentModel { public var surfaceCommandSink: ((String) -> Void)? { get set } }
  ```

Wiring:
- `DocumentModel.idle()` drains `session.takeSurfaceEvents()` into `SurfaceRelay.apply`.
- `selectedStrips`' `didSet` calls `session.setSurfaceSelection`.
- In `PluginTicker`, after the idle loop, `store.noteTarget?.session.focusSurfaces()` runs (`try?`).
- `AppSetup.openCatalog()` calls `try? surfacesLoad()` after `setSupportDir`.
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

final class FakeSurfaceCore: SurfaceCore, @unchecked Sendable {
    var rows: [SurfaceRow] = []
    var addError: VizError?
    func list() -> [SurfaceRow] { rows }
    func ports() -> SurfacePortsRow { SurfacePortsRow(sources: ["In A"], destinations: ["Out A"]) }
    func add(_ k: SurfaceKindRow, _ i: String, _ o: String, _ p: UInt8?) throws -> UInt32 {
        if let e = addError { throw e }
        let id = UInt32(rows.count + 1)
        rows.append(SurfaceRow(id: id, kind: k, input: i, output: o, position: p ?? 0, connected: true, online: false, ignored: 0))
        return id
    }
    func remove(_ id: UInt32) throws { rows.removeAll { $0.id == id } }
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

    @Test func eventsSelectArmJogAndRunCommands() throws {
        let f = mixerFake()
        let m = try DocumentModel(session: f)
        let mixer = try MixerModel(document: m)
        var ran: [String] = []
        SurfaceRelay.apply([.command(id: "transport.stop"), .select(strip: 2), .jog(detents: 4)], m, mixer: mixer) { ran.append($0) }
        #expect(ran == ["transport.stop"])
        #expect(m.selectedStrips == [2])
        #expect(f.locates.last == m.transport.tick + Int64(4) * Int64(MeterMath.ppq) / 4)
    }
}
VIZDAW_EOF
```

`mixerFake()` is the existing helper in `MixerModelTests.swift`. `locates` is FakeSession's record of `locate(tick:)`; add it if it is missing.

- [ ] **Step 2: Run them to see them fail** (`scripts/swift.sh test`). Expected: FAIL — `SurfaceCore` not found.

- [ ] **Step 3: Implement.** Write the two model files and the view. In `VizDAWApp.swift`:

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
                   for model in store.documents.values { model.idle() }
   ```
   Replace:
   ```swift
                   for model in store.documents.values { model.idle() }
                   // Surfaces drive the command target (plan 5f-1 Ruling 11).
                   try? store.noteTarget?.session.focusSurfaces()
   ```

   Find:
   ```swift
           try? setSupportDir(path: dir.path)
   ```
   Replace:
   ```swift
           try? setSupportDir(path: dir.path)
           // surfaces.json (plan 5f-1 Ruling 9).
           try? surfacesLoad()
   ```

   Add `@MainActor let surfaceSettings = SurfaceSettings(core: LiveSurfaceCore())` beside `editingPreferences` in `VizDAWApp.swift`, with `LiveSurfaceCore` calling the free functions. Set `surfaceCommandSink` when a document model is made, to run `KeyMonitor.perform(.command(id), (id: docID, model: m))`, wherever the store's window glue makes models (`ControlWindows.openDocument` or `DocumentStore`'s creation hook). Name the place in the report.

- [ ] **Step 4: Run the tests and the gates.** Expected: Swift +3.

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add app/Sources/VizDAWAppModel/SurfaceSettings.swift app/Sources/VizDAWAppModel/SurfaceRelay.swift app/Sources/VizDAWAppModel/DocumentModel.swift app/Sources/VizDAW/SurfacesSettingsView.swift app/Sources/VizDAW/VizDAWApp.swift app/Tests/VizDAWAppModelTests/SurfaceSettingsTests.swift app/Tests/VizDAWAppModelTests/FakeSession.swift
git commit -m "app: Settings > Surfaces, surfaces loaded at launch, the command target drives them, surface buttons select, arm, jog and run commands (plan 5f-1 Task 10)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -10
```
Counts: Swift +3.

---

### Task 11: The learn UI, and the surface e2e

**Files:**
- Create: `app/Sources/VizDAWAppModel/LearnModel.swift`, `app/Sources/VizDAW/LearnMapsSheet.swift`, `app/Tests/VizDAWAppModelTests/LearnModelTests.swift`
- Modify: `app/Sources/VizDAW/MixerView.swift` (a "MIDI Learn" item in each fader, pan, mute, solo and send control's context menu; the strip menu's "MIDI Learn Maps…"), `app/Sources/VizDAW/PluginWindows.swift` (a header "Learn" button acting on the last parameter touched in the window), `app/Sources/VizDAW/StripChartPane.swift` (a "MIDI Learn" item in the target menu), `app/Sources/VizDAW/KeysSettingsView.swift` ("Learn MIDI" next to the key recorder), `app/Sources/VizDAW/SurfacesSettingsView.swift` (the command maps' Remove), `crates/vizctl/tests/e2e.rs`

**Interfaces:**
- Produces:
  ```swift
  @MainActor @Observable public final class LearnModel {
      public init(document: DocumentModel, core: any LearnCore)
      /// "Waiting for MIDI… move a control (Esc cancels)"; nil when not learning.
      public private(set) var banner: String?
      public private(set) var maps: [LearnMapRow]
      public func learn(_ t: AutomationTargetRow)
      public func learnCommand(_ id: String)     // disabled without a document (Ruling 11)
      public func cancel()
      public func poll()                          // the 60 Hz ticker, while learning or the sheet is open
      public func setMode(_ m: LearnMapRow, _ mode: LearnModeRow)
      public func remove(_ m: LearnMapRow)
      nonisolated public static func modeTitle(_ m: LearnModeRow) -> String  // "Absolute", "Relative (2's complement)", "Relative (binary offset)"
  }
  public protocol LearnCore { func state() -> LearnStateRow; func learnCommand(_ id: String) throws; func cancel() }
  ```
  `DocumentModel.learn: LearnModel` (lazy).

The menu item titles are "MIDI Learn" (controls) and "MIDI Learn Maps…" (strip menu). The banner shows at the top of the mixer window and the plugin window while learning.

- [ ] **Step 1: Write the failing tests.** `LearnModelTests.swift` (with a `FakeLearnCore`):
  - `learningShowsABannerUntilTheMapLands`: `learn(.stripFader(strip: 1))` calls `f.learnTarget`; the banner is non-nil; once `core.state().waiting` turns nil, `poll()` clears the banner and refetches `maps`.
  - `cancelEndsTheLearn`: `cancel()` reaches the core and clears the banner.
  - `modesAndRemovalReachTheSession`: `setMode` and `remove` call `setLearnMode` and `removeLearn`, and a refusal raises the alert "MIDI Learn Failed".
  - `modeTitles`: the three titles above.

  Then add the e2e to `crates/vizctl/tests/e2e.rs`:

```rust
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn drives_a_fader_and_a_button_from_an_mcu() {
    let mut app = App::start();
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
    assert!(
        st["feedback"][0]["messages"].as_array().unwrap().iter().any(|x| x.as_str().is_some_and(|t| t.starts_with("e0 "))),
        "the fader's position went back after release: {st}"
    );
    let e = app.call_err("command.run", json!({"id": "surface.inject", "params": {"surface": id, "bytes": "zz"}, "window": w}));
    assert_eq!(e.code, core_control::ErrorCode::InvalidParams);
    run(&mut app, "surface.remove", json!({"surface": id}));
    app.quit();
}
```

`wait_for_state` is the file's helper (it polls `state.get` with a condition). If the demo's first strip is not the synth, or the document JSON's undo field has another name, read the strip index from `state.get surfaces`' feedback or the document, and adapt; say so in the report.

- [ ] **Step 2: Run them to see them fail.** Run `scripts/swift.sh test`, and the e2e after `scripts/make-app.sh`. Expected: FAIL.

- [ ] **Step 3: Implement the model, the sheet and the menu items.** The plugin window's Learn button learns `PluginWindowModel`'s last-touched parameter, and is disabled before any is touched. Settings › Keys "Learn MIDI" calls `learnCommand(id)` for the selected command row.

- [ ] **Step 4: Run the tests and the gates**, including `scripts/make-app.sh` and the e2e. Expected: Swift +4; e2e 11 passed.

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add app/Sources/VizDAWAppModel/LearnModel.swift app/Sources/VizDAWAppModel/DocumentModel.swift app/Sources/VizDAW/LearnMapsSheet.swift app/Sources/VizDAW/MixerView.swift app/Sources/VizDAW/PluginWindows.swift app/Sources/VizDAW/StripChartPane.swift app/Sources/VizDAW/KeysSettingsView.swift app/Sources/VizDAW/SurfacesSettingsView.swift app/Tests/VizDAWAppModelTests/LearnModelTests.swift crates/vizctl/tests/e2e.rs
git commit -m "app: MIDI Learn on mixer, plugin window and Strip Chart controls, the maps sheet, Learn MIDI for commands; e2e drives an MCU (plan 5f-1 Task 11)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -12
```
Counts: Swift +4, e2e +1.

---

### Task 12: README 173–179, the backlog row, the full gates

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
     it in: within 2 s it shows "Connected" and its display fills.
174. MIDI learn: in the mixer, right-click a fader › MIDI Learn: the
     banner says "Waiting for MIDI…". Turn a knob on a controller: the
     banner goes, Edit shows "Undo MIDI Learn", and the knob moves the
     fader; let go: half a second later the move is one "Set Fader" step.
     A 14-bit fader learns as "CC n/n+32"; an endless encoder learns as
     Relative; Mixer strip menu › MIDI Learn Maps… lists them, and the
     mode picker changes one.
175. A real MCU-compatible unit, if one is available (manual checklist
     "a real MCU or HUI unit"): faders move by motor to the mixer's
     values and do not fight your hand; names and pan values show on the
     scribble strips; bank and channel buttons move eight and one strip;
     mute, solo and select light up; Play, Stop and Record run the
     transport; the position display counts bars and beats; meters move.
176. A real HUI-compatible unit, if one is available: the same checks;
     the unit stays online (the ping), and the 40-character display
     shows the position and the selected strip.
177. README 165a, the surface half: on Touch, Latch, Write and Trim,
     play and ride the Built-in Synth fader from the unit (or a learnt
     knob): you hear it at once, the strip's ring shows the touch, and
     Stop lands one "Write Automation" step. Hold the same fader with the
     mouse and with the unit: it stays touched until you let go of both.
178. Settings › Keys › Learn MIDI next to a command, then press a pad:
     the pad runs the command (with a project open). Settings › Surfaces
     lists the command maps; Remove takes one away.
179. Pull a unit's cable while holding a fader: the unit shows
     "Disconnected", the fader is released (the ride lands as written);
     plug it back: the unit refreshes completely.
```

- [ ] **Step 2: Add the backlog row.** Add it after the row "A pre-5e build names a project with automation modes Corrupt":

```markdown
| A pre-5f build names a project with learn maps Corrupt | Plan 5f-1 Ruling 6: `Settings.learn` is written only when a project has MIDI-learn maps and the format VERSION stays 3, so a build from before 5f reports such a project Corrupt ("unknown field `learn`") rather than NewerFormat. Say so in the release notes, or bump the version when one is due anyway. |
```

- [ ] **Step 3: Run the full gates.** Run every Global Constraints gate, including the rt-check gates, the workspace tests, the Swift tests, `scripts/make-app.sh` and the e2e. Expected: Rust 1632 + 7 + 8 + 6 + 5 + 9 + 3 + 6 + 11 + 1 = **1688 passed**, 14 ignored; Swift 899 + 1 + 2 + 3 + 4 = **909**; e2e **11**; registry 143.

- [ ] **Step 4: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add app/README.md docs/feature-backlog.md
git commit -m "README 173-179: control surfaces and MIDI learn by eye; backlog row for learn maps in older builds (plan 5f-1 Task 12)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -4
```

---

## Self-Review

1. **Spec coverage.**
   - §4.8: Task 4; command maps in preferences, Tasks 5 and 7.
   - §7.1, sources without touch (500 ms): Tasks 1 and 5; touch-sensing faders: Tasks 1 and 3.
   - §8.1:
     - `Surface::input`/`feedback`: Tasks 1–3 (Ruling 3 adds `first` and `tick`);
     - `SurfaceAction`: Task 1;
     - MixerView: Task 8;
     - `core-ffi::surfaces` and CoreMIDI: Tasks 7–8;
     - 20 ms and partners: Task 8;
     - no fader feedback while touched: Tasks 2–3.
   - §8.2:
     - MCU (faders, touch, V-pots and rings, buttons, transport, bank and channel, jog, scribble strips, displays, meters, handshake): Tasks 1–2;
     - HUI (zones and ports, fader pairs, touch, V-pots, buttons, transport, names and display, meters, ping): Task 3;
     - learn (buttons, command maps in Keys, pairs and encoders, override in the list, feedback): Tasks 5, 8 and 11.
   - §8.3: Settings › Surfaces in Task 10, `surfaces.json` and disconnected/reconnect in Task 7.
   - §12.1:
     - `surface.add`, `surface.remove`, `surface.inject` and `state.get surfaces`: Task 9;
     - `midi.inject` feeding learn: Task 8.
   - §12.2: ignored and counted (Tasks 1, 3, 8); never stops the app (every core path is `get`/`match`, FFI guarded).
   - §12.3: the golden tests (Tasks 1–5) and the e2e (Task 11).
   - The manual checklist's "a real MCU or HUI unit": README 175–176.
   - No gaps.
2. **Step scan.** Every code step names its file, its signature and the spec values. The protocol tables are in Rulings 14–15 and in the tests. Two drafts in Task 2's test are corrected inline: the release bytes, and the scribble-strip offsets. Draft lines were also deleted:
   - Task 2's `first.len() + 0` should read `first.len()`;
   - Task 7's `keep` helper only keeps `common` used. Keep it, or drop it if the file uses `wait_limit` elsewhere.
3. **Type consistency.**
   - `Holder`/`Holders`/`GestureHolderRow` come from Task 6 and are used in Tasks 7–8.
   - `SurfaceKind` maps to `SurfaceKindRow`, and `LearnKind`/`LearnMode` to `LearnKindRow`/`LearnModeRow` (Task 8). `LearnSourceRow.number` is the CC or note (0 for pitch bend).
   - `Bank` (Task 2) is used by the registry (Task 7).
   - `SurfaceEventRow` cases match `SurfaceRelay` (Task 10).
   - `focus_surfaces` is introduced minimal in Task 7 and completed in Task 8. `service_surfaces` is likewise introduced in Task 7 and completed in Task 8.
4. **Review Focus.** Each of the five lines has its named tests in its owning task, listed in the line.
5. **Proportion.** The spec's §8 is about 50 lines, and this plan is long because it carries two byte-level protocol tables and their golden tests: the tests are the spec of the bytes. Implementation bodies are left to the implementer, except the handshake formula (Ruling 14), which no signature determines.

## Execution handoff

Plan complete and saved to `docs/superpowers/plans/2026-11-02-control-surfaces.md`. The controller runs it subagent-driven under the standing authorization (spec §13). Tasks 1–5 are pure and independent of the app, and Tasks 6–8 share the session internals; a fresh reviewer per task catches protocol slips before the service builds on them.
