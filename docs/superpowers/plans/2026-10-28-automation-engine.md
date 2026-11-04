# VizDAW 5e-1 — Automation Recording Engine Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Automation is recorded in the cores and over the control channel: every user move of a mixer control or a plugin parameter (moves in AU and CLAP plugin windows included) is a gesture that is heard at once and wins over automation playback; each strip has an automation mode (Off, Read, Touch, Latch, Write, Trim) saved in the project; while the transport plays, writing strips capture their gestures every 10 ms at the heard position, thin them and place them as automation points in the right lane at Stop as one undo step (inside "Record" when audio or MIDI records too); automation on read-ahead (PDC-led) paths is read at the frame its audio is at; and `automation.set_mode`, `automation.gesture`, `automation.suspend` and `state.get automation` drive and show it.

**Architecture:** Bottom-up, one crate layer per task group; Swift only for the control channel's handlers (the views are plan 5e-2's).
- **`core-model`**: `AutomationMode` on `Strip` (saved only when not Read, so older files and the file version stay as they are) and `MixerCmd::SetAutomationMode`; `RecordPass.automation: Vec<AutomationWrite>`, placed by the model when the pass applies (the first lane of the sequence that already automates the target, else a new lane at the end); `automation::strip_of` (which strip owns a target).
- **`core-engine`**: the schedule skips the automation of Off strips (unless automation is suspended). Every strip control is read at its *arrival* (the PDC plan's output time of its node; an insert's parameter at the time its input arrives), through `RegionRun::read_frame`, so automation lines up with led audio (5d-1 controller ruling C2). A **gesture queue** (control → RT) feeds a fixed, sorted **live table** of up to 1024 touched targets whose values override automation (touched, held for Latch/Write, gliding back for Touch, offsets for Trim; holds end at Stop, a wrap and a Locate). A **gesture tap** (installed at every Play and Record) stamps each gesture with the run frame of the block that took it, marks rolls, wraps and stops with the record tap's own run clock (`RunClock`, extracted from `RecordTap`) and says when it has rolled.
- **`core-ffi`**: process-wide automation settings (After Write mode, return time, thinning) and suspend; the pure writer `autowrite` (modes, Ramer–Douglas–Peucker thinning, Trim, edge points, a segment broken at a jump, frames → ticks, writes split at the model's limit), golden-tested from scripted marks and stamps; the session's gesture path (one shared static edit in Off/Read and while stopped, live overrides always, a touch with no value at the value heard now, 10 ms sampling, writes while playing, UI and plugin sources), its pass lifecycle (ended only after the roll; a Locate or a new Play lands the pass and starts another), its landing at Stop (bad writes dropped, never the pass), preview and state; plugin-window gestures from `PluginHost` (every raw parameter event including the ends the host makes itself, the 500 ms release for plugins without gesture events, echoes of playback ignored, no parameter undo step while writing).
- **`core-control`, Swift, `vizctl`**: the three commands and `state.get automation`, with hostile-input tests at every layer, `FakeSession`, the e2e "Touch-write a fader via `automation.gesture` while playing and check the points", and the e2e harness removing its homes.

**Tech Stack:** Rust 2024 (`core-model`, `core-engine`, `core-host`, `core-ffi` with UniFFI 0.32.2, `core-control`, `vizctl`), Swift 6 / SwiftPM (Swift Testing) for the channel handlers, macOS 14+.

**Spec:** `docs/superpowers/specs/2026-10-18-recording-audio-design.md`, §13 rows "5e-1 — Automation recording engine" and "5e-2 — Automation UI" (split from the former 5e row by Ruling 1 of this plan; the writer's commit amends §13). This plan delivers:
- §7.1 the gesture channel: `Session.gesture(target, phase)`, the on-screen mixer's and the channel's moves through it (the mixer views call it in 5e-2), AU and CLAP plugin windows forwarded by the host, the 500 ms release-after-idle for plugins that send values without gesture events;
- §7.2 what a gesture does (live override, static value in Off/Read, snap back in Read);
- §7.3 the four writing modes with capture, loop wraps, thinning, replacing and edge points;
- §7.4 lane placement, "Write Automation" or inside the `RecordPass`, `Session.automation_preview(target)`;
- §4.6 `Strip.automation` and its undoable command; §4.7 automation writes in the `RecordPass`;
- §7.5's Settings › Automation values in the core (the view is 5e-2's) and `automation.suspend`;
- §12.1 `automation.set_mode { strips, mode }`, `automation.gesture { target, phase, value? }`, `automation.suspend`, `state.get automation` (mode per strip, touched targets and suspend);
- §12.3 the automation golden tests, `rt-check` over gesture overrides, and the e2e "Touch-write a fader via `automation.gesture` while playing and check the points";
- 5d-1 controller ruling C2 (backlog row "Strip automation aligned by each strip's arrival time"): automation on led paths is read at the led frame.

Read the whole spec (its §3 rules bind here), plan 5d-1 (`docs/superpowers/plans/2026-10-26-monitoring-engine.md`) and plan 5d-2 (`docs/superpowers/plans/2026-10-26-record-ui.md`), and the 5d-2 carry list `.superpowers/sdd/2026-10-26-record-ui/carry-to-5e.md` (see "Carried items" below).

**Revision.** This text folds in the pre-flight at `1f6057a` (`.superpowers/sdd/2026-10-28-automation-engine/preflight.md`, 2 Critical, 14 Important, 23 Minor; finding tags such as C1, I4, M7 below are its): every adopted ruling is in the task it binds, its code, tests, Interfaces and counts. Rulings 4, 6, 10, 14, 15 and 20 are amended and Rulings 23–28 are new. The pre-flight's addenda file is historical; this plan is the brief.

## Global Constraints

- **Shell setup.**
  - Every shell command runs after `export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools`. Every bash block below starts with that line and `cd /Users/lattawb/workspace/vizdaw`.
  - A `git commit` without `DEVELOPER_DIR` fails silently: check every commit with `git log -1 --oneline` and `git show --stat HEAD`.
- **Swift only through the scripts, from the repo root.** `scripts/build-ffi.sh` (after any Rust change that touches the FFI, before Swift), `scripts/swift.sh build`, `scripts/swift.sh test`, `scripts/make-app.sh`. Swift tests run only via `scripts/swift.sh test` from the repo root. Never call `swift`, `xcodebuild` or `xcrun` directly.
- **Branch and commits.**
  - Work on branch `sp5-audio` (HEAD `1f6057a` plus this plan's revision commit). Never switch branches, never push.
  - Commit at the end of every task, adding only the files the task lists, by path (never `git add -A`). No manifest changes, so `Cargo.lock` never changes. Never commit `.superpowers/`, `.DS_Store` or generated files.
  - End every commit message with the two attribution lines your dispatch gives you. The blocks below show:
    ```
    Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
    Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P
    ```
    Use the `Co-Authored-By` line naming the model you actually are.
- **Clean room.** Never read or copy anything from `~/workspace/SVP/research`, the Studio Vision binary or any research material. No Opcode, Midas, Steinberg, Apple Logic or Soundcraft assets, names, artwork or branding anywhere.
- **No GUI tools, no real devices.**
  - Never use GUI, computer-use or accessibility-automation tools. Launch VizDAW only through the e2e test (Task 11) with `VIZDAW_AUDIO=offline`; at the end of a task that launched it, `pgrep -lf MacOS/VizDAW` prints nothing and `ls -d /tmp/vze*` prints nothing new (Task 11 makes the harness remove its homes).
  - Never touch the real `~/Library/Application Support/VizDAW`, the real preferences, the real Trash or any audio device's settings. Tests use temp directories; sessions that save get a trash folder (`Session::debug_set_trash_dir`); the e2e sets `VIZDAW_TRASH_DIR` inside its throwaway home.
  - Never save a project in a `vizctl`-launched app.
- **Tests never open real audio input and never trigger the microphone prompt (TCC).** Every `core-ffi` test that plays, records or measures calls `vizdaw_ffi::host::force_offline()` first (through its file's `serial` helper) and chooses its fake input with `host::set_fake_input`, never the environment. The e2e runs with `VIZDAW_AUDIO=offline`. **Plugin gestures are tested without third-party plugins:** CLAP through the repo's test plugin (`test-plugins`, `vizdaw.test.gain`'s "Wiggle" parameter makes it perform a UI gesture), the host's main-thread path (the one AU uses) through `core-host`'s scripted fake instance, and AU's own Begin/Value/End capture stays covered by `platform-macos`'s existing tests on Apple's built-in units (`ui_gestures_are_captured`).
- **Test waits.** Every `core-ffi` integration test that waits for the transport, a pass, the engine or the input polls with a deadline from `common::wait_limit()` (`crates/core-ffi/tests/common/mod.rs`, which reads `VIZDAW_TEST_WAIT_SECS`), never a fixed sleep for the result itself; short sleeps only pace gesture samples. The e2e polls with its `WAIT_MS` conditions.
- **Crates and dependencies.** Rust stable, edition 2024. No new crate and no new dependency. License GPL-3.0-only.
- **Lints.** Every crate keeps `#![deny(missing_docs)]` and `#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic))]`. New non-test Rust code uses `.get()`, iterators and `checked_*`/`saturating_*`, never indexing or unwraps.
- **Platform-neutral cores.** `core-model`, `core-engine`, `core-media` and `core-control` stay free of OS APIs.
- **Linux CI.** CI runs `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` on `ubuntu-latest` as well as macOS (`.github/workflows/ci.yml`). Every task's Rust code and tests compile warning-free and pass without macOS: macOS-only items sit behind `#[cfg(target_os = "macos")]`, and a binding only mutated on macOS uses `#[cfg_attr(not(target_os = "macos"), allow(unused_mut))]` (the pattern of `crates/core-ffi/src/host.rs:1968`). Every task that changes Rust checks it with `cargo clippy --workspace --all-targets --target x86_64-unknown-linux-gnu --target-dir target/linux-check -- -D warnings` (the target is installed; nothing is linked, `target/` is ignored by git).
- **TDD.** Every new test is run and seen to fail (RED) before the code that makes it pass. Two exemptions: Task 1's `a_device_chosen_during_a_measure_reaches_the_input_at_its_end` is a missing test for behaviour that already works (the 5d-2 re-reviewer proved it times out with the old `end_measuring`); record that it passes before any change and cite that proof. Task 4's `each_point_cuts_once_on_a_latent_session` guards a design choice the tree at `1f6057a` (one arrival) already meets; record that it passes before the change.
- **Real-time rules (spec §3).** The real-time thread never allocates, frees, locks or does file I/O. Everything new on it is marked `// RT`: `LiveTable` (`apply`, `value`, `holds`, `advance`, `end_holds`, `take_released`, `take_ended_plugins`; sorted, binary-searched, shifted in place), `GestureTap` (`begin_block`, `stop`, `wrap`, `resync`, `set_latency`, `end_block`, `stamp`), `RunClock`, the renderer's `live_gestures`, `apply_live`, `auto_frame` and the arrival reads in `automate`/`next_run`, and `Graph::strip_arrival`/`insert_arrival`/`static_value`. **rt-check gates** (Tasks 4, 5, 6 and 11): `cargo clippy -p vizdaw-play --all-targets --features rt-check -- -D warnings` and `cargo test -p core-engine --features rt-check --test rt_alloc --test rt_alloc_graph --test rt_alloc_record --test rt_alloc_gestures` (the last is new in Task 5).
- **Media is write-once.** Nothing here writes media.
- **File compatibility.** `core_model::file::VERSION` stays 3. `Strip.automation` is `#[serde(default, skip_serializing_if = "AutomationMode::is_read")]`, so a project whose strips are all Read saves byte-for-byte as before and older projects open as Read (Ruling 2). Automation points are ordinary automation events (4d).
- **FFI safety.** No panic crosses the FFI: every new exported function or method returns `Result<_, VizError>` and runs inside `ffi_guard`, except the infallible atomics-only exports `automation_settings`, `set_automation_suspended`, `automation_suspended`, `automation_target_text` and `parse_automation_target` (pure string work), each of whose bodies runs inside `std::panic::catch_unwind` and answers its neutral value (the default settings, nothing, false, an empty string, `None`) on a panic.
- **UniFFI changes.**
  - New records and enums (Task 8): `AutomationModeRow`, `GesturePhaseRow`, `StripModeRow`, `AutomationStateRow`, `PreviewPointRow`, `ThinningRow`, `AutomationSettingsRow`. `StripRow` gains `automation: AutomationModeRow` (its only Swift constructor, `stripRow(…)` in `FakeSession.swift`, is updated in the same task).
  - New exported functions: `automation_settings`, `set_automation_settings`, `set_automation_suspended`, `automation_suspended` (Task 8), `automation_target_text`, `parse_automation_target` (Task 10). New `Session` methods: `gesture`, `set_automation_mode`, `automation_state`, `automation_preview` (Task 8). New `StateTargetRow::Automation` and `ControlResult::Automation` (Task 10).
  - `FakeSession` (`app/Tests/VizDAWAppModelTests/FakeSession.swift`) implements each new `Session` method in the task that adds it.
- **Swift.** Generated bindings compile in Swift 5 mode; `VizDAWAppModel`, `VizDAWEditors`, `VizDAW` and the tests in Swift 6 mode with no new warnings (the one `MemoryDefaults` Sendable warning is older than this plan). No new views (they are 5e-2's). No SwiftUI `@State`/`@FocusState`. New model tests go to `VizDAWAppModelTests`.
- **Control channel** (control spec §6, §7). Programmatic calls never open dialogs: none of the new commands has one. Every command is in `core_control::registry::COMMANDS` and handled by `ControlCommands.run`; the Swift test `everyRegisteredMenuCommandHasAHandler` compares them. The registry count test moves from 129 to 132 in Task 10. Every new command gets hostile-input tests: by type and range in `core-control` (`automation_commands_check_their_params`), in the app (`automationCommandsCheckTheirArguments`) and in the cores (`hostile_gestures_are_refused`, `hostile_automation_settings_are_refused`, `bad_writes_refuse_the_whole_pass`, `targets_parse_only_in_their_own_spelling`).
- **Generated files** are never committed: `app/Sources/VizDAWCore/vizdaw_ffi.swift`, `app/Sources/vizdaw_ffiFFI/include/`, `target/ffi/`, `dist/`.
- **Each task ends green.**
  - Always: `cargo fmt --all` then `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, the Linux clippy of "Linux CI" above (every task that changes Rust), and `cargo test` for the crates the task touches (the workspace in Tasks 2, 6, 8 and 11).
  - Tasks 4, 5, 6 and 11 also run the rt-check gates.
  - Tasks 1 and 8–11 also run `cargo clippy -p core-ffi --lib -- -D warnings` (code under `cfg(not(feature = "test-hooks"))` is linted only by it), `scripts/build-ffi.sh`, `scripts/swift.sh build` and `scripts/swift.sh test`.
  - Task 11 also runs `scripts/make-app.sh` and the e2e (`VIZDAW_E2E_APP=dist/VizDAW.app cargo test -p vizctl --test e2e -- --ignored --nocapture --test-threads 1`).
  - Known flaky tests: the `damaged_files` proptest, the MIDI listener tests (`platform-macos/tests/midi.rs`, the reconnect test), `core-ffi` catalog tests under parallel load and the latency e2e timing. Re-run once before investigating, and say so in the report.
  - Baseline at `1f6057a`: Rust workspace 1462 passed, 0 failed, 13 ignored (plus the rt tests); Swift 845 tests; e2e 9 tests. The counts each task adds are in its last step; report the measured counts (a slip of the writer's arithmetic is not a failure).
- **How code is given.**
  - New files are written whole (`cat > … <<'VIZDAW_EOF'`) where their exact text matters (tests, golden values); for new implementation files the step gives the module's public items with their doc lines and the algorithm where the signature and the tests leave a choice.
  - Changes to existing files are **Find / Replace** excerpts of the tree the previous task left, applied in order with an editor (the Edit tool), or "after the Find, add …" with the exact code. Each Find occurs exactly once in the file when it is applied. Every Find was replayed against `1f6057a` (the excerpts that a later task's Find relies on are created by an earlier task of this plan, and say so: Task 6's `let before = rt_check::count();` is in the file Task 5 creates). Two prose anchors occur more than once and mean one place each: "the `self.apply_pending_pass(&mut inner);` that follows in `idle`" is `crates/core-ffi/src/session.rs:1608`, and "`self.commit_held(&mut inner);` in `end_edit`" is `session.rs:1834` (the other three are not it). If an excerpt does not match, make the same change to the current text; if a name differs from the tree (a helper, a field), use the tree's; if the compiler finds a slip (a missing import, a borrow), fix it the obvious way. Say so in the report in each case.
- **Spec values used verbatim:**
  - "`Strip` gains `automation: AutomationMode { Off, Read, Touch, Latch, Write, Trim }`, default `Read`"; "`Off` plays the static values and ignores automation. `Read` plays automation";
  - "A plugin that sends values without gesture events is treated as touched by its first value and released **500 ms** after its last";
  - "While a target is touched, its live value is applied at once as the engine override … so it is heard immediately and wins over automation playback for that target"; "In Read, while playing, an automated target snaps back to its automation on release";
  - "Writing happens only while the transport is playing. Recording audio or MIDI is not needed";
  - Write: "From play start to Stop (or the next loop wrap), every target of the strip: fader, pan, mute and sends, at their current values, touched or not. Plugin parameters of the strip's inserts are written only once touched … After the pass, the strip's mode becomes the Settings › Automation "After Write" mode (default Latch)";
  - Touch: "From touch to release. On release, the target returns to its existing automation over the **return time** (Setting, 0–2000 ms, default 250 ms). The glide is written as a straight ramp";
  - Latch: "From touch until Stop or the next loop wrap, holding the last value after release";
  - Trim: "measured in dB for fader and send targets and in normalised units for pan and plugin parameters, is *added* to the existing automation under it … Where the target has no automation, the offset applies to the static value, writing absolute points. Stepped targets (mute, solo, stepped plugin params) treat Trim as Touch";
  - "**Loop wrap** ends the writing of every target. The next cycle starts again in the strip's mode, and for Write that is now the "After Write" mode";
  - "Values are sampled on the control thread every **10 ms** while they change, and once at touch and release";
  - Thinning: "tolerance **0.1 dB** for fader and send targets (in the dB domain); **0.25 %** of range for others (Setting: Low / Medium / High); stepped targets keep only their changes";
  - "The written range replaces existing automation for that target in `[first sample, last sample]`. A point is added at each edge so that values outside the range are unchanged";
  - "A write for target T goes into the first lane of the top-level playing sequence that already holds automation for T. If there is none, a new lane named "<strip> <parameter>" is created at the end of that sequence"; "otherwise as their own `Command::Record` with label "Write Automation", one undo step per pass, at Stop";
  - "`automation.suspend` (a global toggle that makes every strip behave as Read without changing its mode)".

## Review Focus

The five failure modes a user is most likely to hit that the spec implies but does not spell out, most likely first; each is pinned by named tests in its task.

1. **The transport moves under a pass** (the first `idle` after Play runs before the renderer has rolled; a click in the ruler, a marker step or `transport.locate` while writing; Play or Play Selection pressed again while playing; an xrun's resync). Expected: the pass is not ended before it has rolled; a Locate or a new Play lands what was written so far as its own "Write Automation" step and writing goes on from the new position; held Latch and Write values do not play on over the new material; no write spans the jump. Pinned by `a_pass_ends_only_after_its_roll`, `writing_survives_the_idle_before_the_roll`, `a_locate_before_the_roll_keeps_writing`, `a_locate_while_writing_lands_and_starts_a_new_pass` and `a_play_while_writing_lands_and_starts_a_new_pass` (Task 8), `a_jump_inside_a_pass_breaks_the_segment` (Task 7), `a_locate_ends_held_values` (Task 5), and the e2e's `writing` check right after Play (Task 11).
2. **A gesture that outlives its context** (the transport stops while the fader is held and is released later, a loop wraps while it is held, the engine is rebuilt by a device change mid-pass, the window closes, the touched strip is deleted mid-pass, a fader and a pan dragged at once while stopped). Expected: what was written up to then lands; a late release leaves nothing held for the next Play; nothing hangs touched forever; a deleted target's write is dropped with a problem line while the rest lands; overlapping static drags are one undo step. Pinned by `a_target_held_over_a_wrap_writes_again_from_its_next_move` (Task 7, renamed in Task 12), `a_device_rebuild_mid_pass_keeps_what_was_written`, `a_stop_while_touched_lands_the_write_and_keeps_the_touch`, `writes_to_a_target_deleted_mid_pass_are_dropped_and_the_rest_lands`, `writes_land_when_the_recording_records_nothing`, `a_touch_held_over_stop_takes_its_strips_mode_at_the_next_play` and `two_static_gestures_at_once_are_one_step` (Task 8).
3. **Automation on a strip behind latency** (a track whose audio is led because a linear-phase EQ sits on another bus, a bus behind a latent insert, the master). Expected: a mute or a fader move written at bar 3 changes what is heard exactly where bar 3 is heard, with and without region leads, through loop wraps. Pinned by `a_led_paths_mute_lands_on_its_audio` and `arrivals_follow_the_plan` (Task 4).
4. **Plugins that report gestures badly** (values with no Begin/End, an End lost or never sent because the host closed the gesture itself, echoes of the automation the plugin is playing, a gesture while the plugin restarts). Expected: a value-only plugin writes from its first value to 500 ms after its last; a gesture the host closes releases its touch; playback echoes are not touches; the plugin's move never becomes a parameter undo step while it is being written as automation, and still does when not writing. Pinned by `values_without_gesture_events_release_500_ms_after_the_last`, `idle_reports_every_param_event_in_order`, `a_gesture_the_host_closes_reports_its_end`, `a_plugin_windows_gesture_writes_its_parameter` and `a_ui_gesture_on_a_plugin_parameter_reaches_the_plugin` (Task 9).
5. **Writing where the user does not expect, or from careless input** (the target automated in two lanes, a lane in a referenced sequence, a lane whose active take is not the first; a grab of an automated fader; a target spelt wrong or naming a strip that does not exist, a value of `nan`, `1e400`, `-0.5` or `1e-1`, a release with no touch, a strip listed twice). Expected: the first lane of the playing sequence that plays and holds the target gets the write and the target's other playing lanes lose their points in the range (final review I1: muted and solo-silenced lanes are skipped and keep theirs; with no playing lane the first unmuted, untransformed lane holding the target that only a solo silences takes it, so under a solo all writes for the target go into one lane: Task 12 review I2; with none, a new one is made, not soloed, so under another lane's solo it is silent until the solo clears: owner ruling 2026-10-29, Task 12; a lane with a play shift or its own length or loop is never written into: re-review O1), so the written value is what plays; a grab starts at the value heard, not the static one; `invalid_params` or a `failed` reply naming the problem before anything changes. Pinned by `a_write_goes_into_the_first_lane_holding_its_target` and `a_write_leaves_other_lanes_outside_its_range` (Task 3), `a_touch_without_a_value_starts_at_the_heard_value`, `a_trim_grab_does_not_jump` and `hostile_gestures_are_refused` (Task 8), `automation_commands_check_their_params`, `automationCommandsCheckTheirArguments` and `targets_parse_only_in_their_own_spelling` (Task 10).

## Plan Rulings

Decisions the spec leaves open, in the format "what — why — cost if wrong".

1. **Ruling (the split):** the former §13 row "5e — Automation recording" is two plans: **5e-1** (this plan: model, engine, core-ffi, control channel, e2e) and **5e-2** (`docs/superpowers/plans/2026-10-28-automation-ui.md`: the mixer's gestures, mode popups and rings, Settings › Automation, the `automation.mode.<mode>` menu commands, the plugin window's generic sliders, the Strip Chart's live preview, the 5d-2 UI carries, README checks). The writer's commit amends spec §13 with the two rows.
   - Why: one plan would be about 19 tasks; the RT and placement work must be reviewed apart from the views, as 5d was.
   - Cost if wrong: one more plan header and final review.
2. **Ruling (where the mode lives):** `Strip.automation: AutomationMode` in `core_model::automation`, serde default Read and skipped when Read; the spec's `StripsCmd::SetAutomationMode` is `MixerCmd::SetAutomationMode { strips: Vec<StripId>, mode }` (the code keeps strip settings in `MixerCmd`; `command/strips.rs` holds only the strip bookkeeping helpers), one undo step "Set Automation Mode" for any number of strips, refused for no strips or an unknown one; a strip listed twice is set once (M21).
   - Why: §4.6 makes it document state and undoable; one command serves "Set all strips to…" (5e-2) and `automation.set_mode` in one step.
   - Cost if wrong: a rename of the command.
3. **Ruling (Off and suspend):** the schedule skips every automation point whose target belongs to an Off strip (`automation::strip_of`: a strip's own controls and sends; an insert's parameter → its strip; a plugin instrument's parameter → its Instrument strip; velocity scale has no strip and always plays). `automation.suspend` is process-wide (spec: "a global toggle"); while on, Off strips play their automation too (every strip behaves as Read) and no touch writes. A change applies to touches from then on: writes already holding (Latch, Write) finish as they would.
   - Why: §4.6 and §7.5; checking at touch keeps a pass's writes whole.
   - Cost if wrong: turning suspend on mid-pass does not end a held Latch until Stop or a wrap.
4. **Ruling (writes in the model; amended by pre-flight I5):** `RecordPass` gains `automation: Vec<AutomationWrite>`; `AutomationWrite { seq, target, start, end, points, lane_name }`. Applying a pass places each write after the lanes. It removes the target's points with `start ≤ tick ≤ end` from the active take of **every** lane of `seq` that holds an automation event of the target (the schedule merges every lane's points per destination, so a point left in another lane would still play inside the range). It inserts `points` (all inside `[start, end]`) only into the first such lane (lane order), else into a new lane at the end named `lane_name`. Automation inside referenced sequences is left (their lanes cannot record, spec Ruling 15; README 161 and a backlog row say so). Writes apply in order, so a later pass of a loop wins where passes overlap. A pass with only automation is named "Write Automation". Lane names are made by `core-ffi` ("<strip> Fader", "<strip> Pan", "<strip> Mute", "<strip> Solo", "<strip> Send <n>" with n from 1, "<strip> <parameter name>", "<strip> Parameter <id>" when the plugin's name for it is unknown).
   - Why: §4.7, §7.4; the model already owns "first lane with T" (it is a document query) and applies the pass atomically.
   - Cost if wrong: a write lands in a lane the user did not expect when a target is automated in several lanes; Undo takes it back.
5. **Ruling (5d-1 C2, automation at each strip's arrival):** a strip's fader, pan, mute, solo and send levels are read at frame `read_frame(t_out)` of the run's `RegionRun`, `t_out` being the strip node's output time in the PDC plan (after its inserts); an insert's parameter at `t_in + latency of the inserts before it`; a plugin instrument's parameter at the renderer's frame (it follows its MIDI). `read_frame` maps through the roll's floor and the loop seam exactly as led regions read; where it reads nothing (before the roll, past the end) automation is read at `frame − arrival` (saturating). Runs are cut at each lane's next automation point at that lane's arrival (pre-flight I13), so points still switch on run boundaries and each point cuts once. *Refined (Task 4 controller ruling):* mute, solo and send levels are edge gains, applied after the fed edge's delay line, so each is read at the arrival of the edge where it acts: a strip's mute and solo at its main output edge's, a send's level at that send edge's (`Graph::feed_arrival`); fader and pan stay at the strip's. *Final review m1:* a lane whose value is the same on both sides of its read's jump (loop seam, roll floor, end) does not cut the run there.
   - Why: the audio at a strip's fader in the block at frame `f` is the timeline at `f − t_out`; reading the automation there lines them up for led paths, buses and the master alike.
   - Cost if wrong: none for strips without latency (arrival 0 keeps today's behaviour exactly).
6. **Ruling (the gesture queue and the live table; amended by pre-flight I3, I4):** one control → RT queue per engine (`GestureSender`, 4096 messages), drained at most 256 messages per block. `GestureSender::send` takes the producer with a blocking `lock()` (poison-tolerant), never `try_lock`: only control-side threads send, the RT thread only consumes; it returns false only when the queue is full. The renderer keeps at most 1024 live targets (`MAX_LIVE`) in a fixed boxed array kept sorted by `AutoDest`, found by binary search; insert and remove shift elements and never allocate. A message that does not fit, or a touch with no free entry, is dropped and counted (`Engine::gestures_dropped`); an *end* (Release, Glide, Hold) the session could not send is kept and re-sent at each `idle` (Ruling 10). The live value of a touched target replaces its automation (or static) value in the run's ramped commit, so moves ramp over a run instead of stepping; a plugin parameter that is touched or held is not sent automation at all (the plugin holds the user's value).
   - Why: §3's RT rules need fixed capacities; Ruling 9 holds every control of every Write strip, so "Set all strips to Write" on a 16-strip session already needs more than 64 entries; a dropped end would leave a target overriding its automation for good.
   - Cost if wrong: a 1025th simultaneous live target is ignored until another ends.
   - Tests: `a_full_queue_refuses_and_counts` and the floor `const` (Task 5), `an_unsent_end_is_resent_…` (Task 8). The problem lines for lost stamps, marks and drops and for a missing tap (M10) have no test: reaching them needs the engine's rings to overflow or `start_gestures` to fail, which no test hook provides; they are one `push` each (pre-flight check m7).
7. **Ruling (stamping):** the renderer stamps each gesture message with the output run frame of the block that took it, offset by its host time exactly as MIDI input is (`(host_ns − block_ns) × rate`), and a dedicated **gesture tap** marks rolls, wraps, stops and resyncs with the record tap's own run clock (`RunClock`, extracted from `RecordTap` without changing its behaviour); its status says when it has rolled (`GestureStatus::rolled`, for Ruling 24). The control side turns runs into heard schedule frames and passes with the record placement's pieces (`passes::pieces`, made `pub(crate)`), so automation and recorded MIDI share one mapping ("the same mapping as MIDI", §7.3). `host_ns` is `clock::now_ns()` on a live device and 0 offline (placed at the block that takes it, as `midi.inject` is: the offline device's clock is not the host's). The tap is installed at every Play and Record start and removed at Stop.
   - Why: one mapping for MIDI and automation; independence from recording ("Recording audio or MIDI is not needed").
   - Cost if wrong: offline, a sample lands up to one block (about 10 ms) late.
8. **Ruling (10 ms sampling):** the session sends a target's value to the engine at most once per 10 ms; a value arriving sooner is kept, with the host time it arrived at (M6), and sent when the next value comes 10 ms or more after the last sent, at the touch's release, or at the session's next `idle` once 10 ms have passed; a kept value is stamped with its arrival time, not the send time. Touch and release are always sent, a pending value first.
   - Why: §7.3 "every 10 ms while they change"; a still control sends nothing.
   - Cost if wrong: none audible; thinning removes the rest.
9. **Ruling (Write):** at Play, every Write strip's fader, pan, mute and send levels are held in the engine from the roll at the value each has at the play-start tick (its automation there, else its static value), and written from the roll to the end of the first pass, a touch changing the held value from then on (held after release). Plugin parameters of a Write strip write from their touch to the pass's end. From the first wrap on the strip writes in the After Write mode: stamps tagged Write in a pass after the first are written in that mode (`AutoCapture.after_write`; an After Write of Off or Read writes nothing there, M3). When the pass lands, the strip's mode becomes the After Write mode in the same undo step, for the Write strips that still exist and only when some write landed (Ruling 25). A strip switched to Write while playing writes as Latch until the next Play.
   - Why: §7.3's Write and "for Write that is now the After Write mode"; holding from before the roll means the first frames already play the held values.
   - Cost if wrong: a strip set to Write mid-play does not overwrite its untouched controls until the next Play.
10. **Ruling (releases per mode; amended by pre-flight I2, I3, I10):** Touch glides back over the return time (the engine ramps the live value to the automation, or to the static value where none drives it; the write ends with a ramp to the existing value at release + return time). Latch and Write hold until Stop, the next wrap or a Locate (the engine keeps the value; the write ends at the pass's end with the last value). Trim releases like Touch on its offset (the offset returns to 0 over the return time). A stepped target (mute, solo, a stepped plugin parameter) in Touch or Trim is held as in Latch: a click has no duration to write. *Amended by Task 12 (owner, 2026-10-29):* a wrap ends the writing of every target; a target still touched writes again from its next move (Ruling 20), not from the new pass's start, and a glide the wrap cuts is written as far as it went. While looping, each pass lands at its wrap into the looped pass's one undo step (spec §7.4).
    - A release while the session has no writing pass (stopped, or the pass already landed) sends `Release`, whatever the touch's path: a Hold or Glide sent then would override the target through the whole next Play.
    - An end (Release, Glide, Hold) that `host::gesture` could not send while this session holds the engine is kept per target and re-sent at each `idle` until it goes through; a later message for that target replaces it.
    - When a live entry for a `Plugin` dest ends (release, the end of a glide, `end_holds`), the renderer sets that lane's `auto_last` to NaN, so flat or stepped automation is sent again (spec §7.2 "snaps back to its automation on release").
    - Why: §7.3 plus the cases it leaves open; momentary mute writes would be invisible.
    - Cost if wrong: a mute clicked in Touch mode stays written until Stop or the wrap.
11. **Ruling (Trim):** where the playing sequence automates the target, the touch goes to the engine as `Offset(0.0)` (pre-flight I12: an absolute touch value would play for a block before the first offset) and the session sends offsets (the control's movement since touch: dB for fader and send targets via `fader_db`, normalised units otherwise) and the engine plays automation + offset; the write's points are the existing points inside the range and the thinned offset samples, each at `existing(t) + offset(t)` (offsets interpolated linearly), clamped to 0–1 (−∞ … +12 dB). Where the target has no automation, Trim sends and writes absolute values (static value + offset), as Touch would.
    - Why: §7.3 "modifies existing points in place, plus the edge points" keeps the shape; adding the offset samples keeps a moving offset's shape too.
    - Cost if wrong: more points than the strict reading (thinning bounds them).
12. **Ruling (thinning):** Ramer–Douglas–Peucker on vertical distance (value units, not geometric distance), per segment, in frames before ticks: tolerance Low/Medium/High = 0.04/0.1/0.2 dB for fader and send targets (in the dB domain, −144 dB floor) and 0.1/0.25/0.5 % of range for others; stepped targets keep only the samples where the value changes (and the first). Default Medium. On equal maximum distance the earliest index splits; the algorithm is iterative with an explicit stack (M7), so the result is deterministic and 100 000 samples cannot overflow the stack.
    - Why: §7.3 names the Medium values and a three-step setting; vertical distance has the unit the tolerance is given in.
    - Cost if wrong: Low/High are a factor 2.5 apart from Medium.
13. **Ruling (edges and ticks):** samples are thinned in frames, then turned into ticks with the playing sequence's tempo (`TempoTable::frame_to_tick`, rounding); equal ticks keep the last value. The write's range is `[first tick − 1, last tick + 1]` with a point at each edge holding the value the target had there before the write: its existing automation (the playing sequence flattened as the engine plays it, Plan 4d's lane rule) or, with none, its static value.
    - Why: §7.3 "A point is added at each edge so that values outside the range are unchanged"; a tick outside is the nearest place that keeps them.
    - Cost if wrong: the value changes within one tick of each edge (a ramp of one tick, under a millisecond).
14. **Ruling (the static path; amended by pre-flight I1):** a gesture that starts while the transport is stopped, while suspended, or on an Off or Read strip changes the static value. Static-path gestures share one edit: the first static touch opens it, named after its target ("Set Fader", "Set Pan", "Mute", "Solo", "Set Send Level", "Change Plugin Parameter"); later static touches join it; each value runs the static command inside it; the last static release closes it. Overlapping static gestures (the mixer and a plugin slider, two surface faders in 5f) are therefore one undo step. `Busy` "Finish the open edit first." is returned only when an edit the gesture path did not open is open (`begin_edit`, `edit_gesture`, step entry). A gesture keeps the path it started with until its release (Ruling 20 for Play). In every path the engine also gets the live value, so an automated target is heard while touched (and in Read snaps back on release).
    - Why: §7.2; the per-gesture edit is today's `begin_edit`/`set_*`/`end_edit` moved into the core, and History has one transaction, so a second edit per gesture would refuse the second hand.
    - Cost if wrong: two overlapping drags undo together.
15. **Ruling (plugin windows; amended by pre-flight I7, I9, M12, M13, M14; accepted by the owner 2026-10-28):** `PluginHost` reports every raw parameter event of a plugin (`IdleReport.param_events`, in arrival order, beside the grouped gestures), including an `End` for every open parameter of a gesture the host closes itself (the timeout flush, a closed editor, an instance that fails, restarts or leaves). The session forwards them as gestures on `PluginParam(slot, param)` with the value normalised by the parameter's range, stamped at the host time the host drained them (at most one timer tick late, a burst collapsing to its first and last value). A plugin value outside Begin/End touches the target and releases it 500 ms after its last value (at that last value's time + 500 ms on a live device). While a slot's strip is writing, the plugin's finished gestures are not committed as "Change Plugin Parameter" undo steps (the static value stays; the written automation plays over it).
    - The gesture path has a **source**, `Ui` (the mixer, the plugin window's generic sliders, the channel) or `Plugin` (the plugin's own editor). A `Ui` value of a `PluginParam` goes to the plugin through the host's `set_param` (plain value) on every path; it is committed as `SetPluginParams` only on the static path. `Plugin`-source events are never sent back and never committed by the gesture path: the host's own grouped `Gesture` is the static step.
    - A kept host `Gesture` drops the parameters that were on a writing path before it is committed (it is dropped when none remain).
    - At landing, each written plugin parameter is sent the landed automation's value at the transport tick (`set_param`, no commit), so a save does not bake the hand's last value into the plugin state.
    - On the release of a Touch or Trim path `PluginParam` that no schedule lane drives, the session sets the plugin to its static value (plain, no commit), so what is heard matches what is written.
    - The session releases plugin touches of slots that left the document or stopped running.
    - Why: §7.1; a parameter undo step on top of the written automation would be undone separately and confuse the static value; it is how the common DAWs behave (pre-flight, "Ruling 15").
    - Cost if wrong: a plugin knob moved while writing leaves the parameter's static value where it was.
16. **Ruling (what cannot be gestured):** velocity scale (no control moves it) is refused with `Model`; so is an unknown strip, send, slot or a parameter the slot does not have, and a non-finite value. Values are clamped to 0–1 only after a finiteness check; the channel refuses values outside 0–1 before they reach the core.
    - Cost if wrong: none.
17. **Ruling (channel spelling):** targets are text: `fader:<strip>`, `pan:<strip>`, `mute:<strip>`, `solo:<strip>`, `send:<strip>:<n>` (n from 1, as the mixer shows "Send 1"), `param:<slot>:<param>`; the same spelling in `state.get automation`. Parsing and formatting live in `core-ffi` (`parse_automation_target`, `automation_target_text`) so the app and the state JSON never drift. Values are text (`"0.5"`), as `destination.set_return_latency`'s milliseconds are (the registry has no number kind): plain decimals only, ASCII digits with at most one `.` (no sign, exponent, hex or spaces), 0 to 1. `automation.set_mode`'s `strips` is `"all"` or comma-separated strip IDs.
    - Cost if wrong: scripts spell targets one way only.
18. **Ruling (preview):** `Session.automation_preview(target)` answers the write in progress for the target in the current pass, as it would land now (thinned with the pass's thinning, edges included), as `(tick, value)` in the playing sequence; empty when the target is not being written. The pass keeps its stamps per target, so a preview runs the writer over that target's stamps and the marks only (M15).
    - Cost if wrong: the Strip Chart preview (5e-2) recomputes a little per poll.
19. **Ruling (settings):** `AutomationSettingsRow { after_write, return_ms, thinning }`, process-wide like the audio settings (the app keeps them in its preferences, 5e-2): `after_write` any mode but Write (default Latch), `return_ms` 0–2000 (default 250), `thinning` Low/Medium/High (default Medium); a refusal (`Model`) changes nothing. All three are read when a pass starts and kept with it, so the preview equals the landing (M4; `settings_changed_mid_pass_apply_from_the_next_play`, Task 8); the return time is turned into frames at the engine's current sample rate, else the project rate (M5; not tested: the offline device runs at the project rate, and a device-rate change rebuilds the engine and ends the pass, so no test can glide at another rate; pre-flight check m7). "Write plugin parameters only when touched" is always on and has no field.
    - Cost if wrong: a setting changed mid-pass applies from the next Play.
20. **Ruling (touches that never end; amended by pre-flight M2, M9):** touches from the mixer and the channel last until their release; only plugin (and, in 5f, surface) sources release by themselves. A Stop lands what was written and keeps a touch that is still held. A touch still held at the next Play writes from its next value (messages sent while stopped are not stamped); its path is re-picked at Play: Write when its strip writes now, else live-only (no static edit is opened mid-touch); a static touch whose edit is open keeps the static path. Touches whose target no longer exists (an Undo of a strip, a removed insert) are released after every document change the session makes. Closing the document releases every touch. A release with no touch does nothing; a value with no touch is a touch first.
    - Cost if wrong: a script that never releases keeps its target overriding automation until the window closes.
21. **Ruling (the engine changes mid-pass):** a rebuilt engine (device change) drops its gesture tap and live table: the session lands what was captured up to then at its next `idle` as if stopped there, and touches still held are sent to the new engine at their next value. A rebuild while stopped seeds the old engine's overrides into the new one, so a released Latch value can linger until the next roll's `forget_unautomated` (M23, accepted).
    - Cost if wrong: a held value is unheard for up to one 10 ms sample after a device change.
22. **Ruling (existing automation):** the "existing automation" a glide, Trim or an edge reads, and the value Write holds from, is the target's points in the playing sequence flattened as the engine plays it (`query::flatten`, references included), valued by Plan 4d's lane rule (the first value before the first point, the last after the last, linear between unless stepped).
    - Cost if wrong: none (it is what the user hears).
23. **Ruling (a transport jump lands the pass; pre-flight C2):** while a pass is writing, `locate`, `play`, `play_from` and `start_recording` first finish the writing pass and land it as Stop does ("Write Automation", After Write included); then they act, and the new position starts a new pass with a new tap. After a Locate the new pass starts when the transport plays, or when the old pass had not rolled yet and this session still holds the engine (a Locate queued while `play` waited for the disk lands before the renderer rolls: pre-flight check I-2). The renderer ends live holds at `EngineCmd::Locate` as at a wrap. The writer breaks a segment at any non-wrap mark whose frame does not continue the piece before it (an xrun's resync), as a wrap does but without After Write.
    - Why: the record placement's pieces assume the transport cannot jump while recording (5c refuses Locate then), but writing allows it; a write spanning a jump would erase the automation between or be refused whole, and held values would play over unrelated material.
    - Cost if wrong: one undo step per contiguous stretch played.
24. **Ruling (when a pass ends; pre-flight C1, M17; check m3):** a writing pass ends only when its tap's status reads closed; when this session no longer holds the engine (`host::transport_stopped(self.id) == None`, rolled or not: another session's engine never closes this tap); when the tap has rolled and the transport reads stopped (`Some(true)`); at Stop, at Ruling 23's jumps, or at close. A `host::finish_play` that fails finishes writing.
    - Why: `transport_stopped` reads stopped until the renderer applies the Play, a block after `play` returns, and the idle timer that queued during `play`'s disk wait fires first.
    - Cost if wrong: none (recording guards the same way with `status.run() > 0`).
    - Tests: `a_pass_ends_only_after_its_roll`, `writing_survives_the_idle_before_the_roll` and `a_locate_before_the_roll_keeps_writing` (Task 8, the last two holding the offline renderer with `debug_hold_offline`). Stop's wait for the tap to close before the last drain (M1) is not tested: what it saves is at most one block at the end of a held value, under the transport poll's resolution (pre-flight check m7).
25. **Ruling (a landing never refuses more than the bad write; pre-flight I6):** the session drops writes whose target (`target_exists`) or sequence no longer exists, one problem line each: "Automation of <describe> was not written: it no longer exists." `to_model` splits a write longer than `MAX_WRITE_POINTS` into consecutive writes. When a recording's pass cannot be built, or the model refuses it, its automation lands as its own step ("Write Automation") inside the recording's group. After Write's `SetAutomationMode` lists only strips that still exist, and runs only when some write landed.
    - Cost if wrong: none (the model still refuses a bad pass whole; the session no longer hands it one).
26. **Ruling (what `Touch { value: None }` means; pre-flight I11):** it is the value heard now. For strip and send targets: the engine's override snapshot (`Engine::overrides()`) when this session holds the engine and the snapshot has the control; else, while playing, the existing automation at the transport tick; else the static value. For plugin parameters: the plugin's current value. The mixer sends `touch(value: nil)` on mouse-down (plan 5e-2).
    - Why: the mixer shows static values, so a touch at the shown value would jump the heard value, and the write, to the static value when an automated fader is grabbed.
    - Cost if wrong: none; a backlog row covers showing the heard value in the mixer (Task 11).
    - Tests: the strip half by `a_touch_without_a_value_starts_at_the_heard_value` (Task 8). The plugin half (a `Begin` touches at the plugin's value) has none: the test plugin's Wiggle changes its value in the same drain as its `Begin`, so "the value before" cannot be observed (pre-flight check m8).
27. **Ruling (value-only plugin events on automated parameters; pre-flight I8):** a plugin `Value` with no open `Begin`, on a parameter the playing schedule automates, is not a touch: the gesture path ignores it. Begin/End gestures touch automated parameters as before; value-only plugins still write parameters that have no automation.
    - Why: the AU echo filter remembers 16 host writes per parameter and a ramp writes more per drain, so the surplus echoes of playback arrive as user values; as touches they would pause the parameter's automation and record its own playback.
    - Cost if wrong: a plugin that never sends Begin/End cannot overwrite a parameter that is already automated (its first pass, on an unautomated parameter, still writes).
    - Tests: `PluginTouches`' unit test pins the rule; the session's `automated` flag has no end-to-end test, because no plugin the core-ffi tests can load sends value-only events (the CLAP test plugin's Wiggle always sends Begin and End; the scripted fake lives in `core-host`'s tests). Not worth a new test plugin (pre-flight check m8).
28. **Ruling (the record range does not bound automation; pre-flight M16):** automation writes ignore the punch and record range ("Recording audio or MIDI is not needed"). Write holds are heard from Play, including during a count-in.
    - Cost if wrong: a punch-in take writes automation outside its punch range.

## Carried items

| Item (source) | Where |
|---|---|
| N1: a refused punch draft (typo, past the song limit) does not stop Record (5d-2 final re-review; carry "Task 1") | Task 1 |
| I1, the Measure half untested (5d-2 final review) | Task 1 (`a_device_chosen_during_a_measure_reaches_the_input_at_its_end`) |
| The plugin-engine gap: a Measure refused in window B retires window A's plugin engine (5d-2 m1 and re-review Minor) | Task 1 (the channel check before the claim) |
| 5d-1 controller ruling C2 / backlog row "Strip automation aligned by each strip's arrival time" | Task 4 (Ruling 5; the row goes) |
| T4 M3: `state.get input`'s held peaks and stale length undocumented for the channel | Task 10 (README, `StateTarget::Input` docs) |
| T10 Minor 6: the e2e leaves its `/tmp/vze*` homes | Task 11 (the harness owns a `TempDir`) |
| N3: the device read on mixer hover runs on the main thread | Task 11 folds it into the backlog row "Input pickers' MIDI source list off the main thread" |
| T2 M1/M2, T3 M1, T5 M3, N2, T7 Minor 2/4 and the 5d-2 test-pass items (T2 M3/M4, T4 M4, T5 M5, T6 Minor 3, T7 Minor 3, T9 M3) | Plan 5e-2 (UI polish and test pass) |
| T5 M2 (the meter invalidates the whole header), T5 M4 other half (the meter at lane heights 46–47), T6 Minor 2 (the strip's lane picker names the lane), T9's tempo change inside a growing take (pre-flight I14: no tracked home) | Task 11 (backlog rows tagged SP7) |
| m3 (devices by UID), m4 (CoreMIDI list), m7 (two count-in clicks), I4 hardware checks, owner README checks | Not this sub-plan: existing backlog rows as the carry list says; owner checks stay in the README |

## File map

| Folder | New | Changed |
|---|---|---|
| `crates/core-model` | tests `cmd_automation.rs` | `src/automation.rs`, `src/mixer.rs`, `src/command/mixer.rs`, `src/command/record.rs`, `src/command/mod.rs`; tests `mixer_commands.rs`, `cmd_record.rs` |
| `crates/core-engine` | `src/gesture.rs`; tests `automation_arrival.rs`, `gestures.rs`, `rt_alloc_gestures.rs` | `src/lib.rs`, `src/record.rs`, `src/render.rs`, `src/engine.rs`, `src/graph/exec.rs`, `src/graph/controls.rs` (`base_of`, `OverrideSnapshot::value`), `src/schedule/automation.rs`, `src/schedule/compile.rs`; tests `schedule.rs`, `schedule_automation.rs` |
| `crates/core-host` | — | `src/host.rs` (`param_events`, the ends it makes itself); tests `host.rs` |
| `crates/core-ffi` | `src/automation.rs`, `src/autowrite.rs`; tests `automation.rs`, `autowrite.rs` | `src/lib.rs`, `src/audio.rs`, `src/host.rs`, `src/session.rs`, `src/passes.rs`, `src/synth.rs`, `src/mixer.rs`, `src/control.rs`; tests `synth.rs`, `params.rs`, `control.rs` |
| `crates/core-control` | — | `src/registry.rs`, `src/request.rs`; tests `commands.rs` |
| `crates/vizctl` | — | tests `e2e.rs` |
| `app/Sources/VizDAWAppModel` | `AutomationCommands.swift` | `RecordModel.swift`, `RecordOptions.swift`, `ControlCommands.swift` |
| `app/Tests/VizDAWAppModelTests` | `AutomationCommandsTests.swift` | `RecordOptionsTests.swift`, `FakeSession.swift` |
| docs | — | `app/README.md`, `docs/feature-backlog.md` (one row goes in Task 4; six rows and a note in Task 11) |

No Swift file outside the generated bindings switches over `StateTargetRow` or `ControlResult`, so `app/Sources/VizDAW/ControlChannel.swift` needs no change (M19).

### Task 1: 5d carries — a refused punch draft refuses Record; Measure checks the channel before its claim; the Measure half of I1

**Files:**
- Modify: `app/Sources/VizDAWAppModel/RecordOptions.swift` (`commitPunch`, `commitPunchDrafts` return whether they took), `app/Sources/VizDAWAppModel/RecordModel.swift` (`toggle`)
- Modify: `crates/core-ffi/src/host.rs` (`start_measure`)
- Test: `app/Tests/VizDAWAppModelTests/RecordOptionsTests.swift`, `crates/core-ffi/tests/synth.rs`

**Interfaces:**
- Consumes: `DocumentModel.commitField(_:_:) -> Bool` (4f), `audio_input_channels()` and `record::channel_refusal` (5d-2, 5c).
- Produces: `RecordModel.commitPunch(_:) -> Bool` and `commitPunchDrafts() -> Bool` (both `@discardableResult`: true when nothing was typed or every draft was taken). Nothing else changes shape.

The carry list's three items for "an early 5d carries task". N1: a punch draft that is not a position (or lies past the song limit) left the old punch range in place and Record recorded with it; now Record refuses and the status line keeps saying why. The gap: `start_measure`'s `claim_host` retires another window's plugin engine before a channel the device lacks is refused; the channel count is known without an engine (`audio_input_channels`), so it is checked first. I1's Measure half: the record-hold half has a test (`a_device_chosen_while_recording_reaches_the_input_at_stop`); `end_measuring` goes through the same `settle_input`, and now has one too.

- [ ] **Step 1: Write the failing tests**

In `app/Tests/VizDAWAppModelTests/RecordOptionsTests.swift`:

Find:
```swift
    @Test func theTimeLeftCountsTheArmedAudioChannels() throws {
```
Replace:
```swift
    /// 5d-2 final re-review N1: a punch draft that is not a position (or
    /// is past the song limit) refuses Record, so the pass never punches
    /// at the old range; the status line says why. Once the field is
    /// right, Record records at the typed time.
    @Test func aRefusedPunchDraftRefusesRecord() throws {
        let (m, f) = try armed()
        let r = m.record
        r.setPunch(true)
        r.showsPopover = true
        r.setPunchText(.punchIn, "x")
        r.pressRecord()
        #expect(!r.isRecording && f.recordCalls.isEmpty)
        #expect(m.editor.status?.contains("is not a position") == true)
        r.setPunchText(.punchIn, "2")
        m.editor.clearStatus()
        r.pressRecord()
        #expect(r.isRecording)
        #expect(f.recordCalls.count == 1 && f.recordCalls.last?.spec.punch?.start == 61_440)
    }

    @Test func theTimeLeftCountsTheArmedAudioChannels() throws {
```

Append to `crates/core-ffi/tests/synth.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat >> crates/core-ffi/tests/synth.rs <<'VIZDAW_EOF'

/// 5d-2 final review I1, the Measure half (carry to 5e): a device chosen
/// while a Measure runs is not opened under it, but when it ends: the
/// input is closed, then reopened on the new device (two input changes).
#[test]
fn a_device_chosen_during_a_measure_reaches_the_input_at_its_end() {
    let g = serial(Some(DELAY_MS), None);
    vizdaw_ffi::host::set_audio_input_device(None);
    let d = doc(true, 2);
    let s = untitled(&g, d.doc.clone());
    let s2 = Arc::clone(&s);
    let dest = d.synth.0;
    vizdaw_ffi::host::debug_hold_measure(true);
    let t = std::thread::spawn(move || s2.measure_return(dest));
    wait_until("the measurement to start", vizdaw_ffi::host::measuring);
    wait_until("the input to open", || s.debug_input_open() == Some(true));
    let n = s.debug_input_changes().unwrap();
    vizdaw_ffi::host::set_audio_input_device(Some("Other In".into()));
    assert_eq!(
        s.debug_input_changes(),
        Some(n),
        "not under the measurement"
    );
    vizdaw_ffi::host::debug_hold_measure(false);
    assert_eq!(t.join().unwrap().unwrap().onsets, 5);
    wait_until("the input to reopen", || {
        s.debug_input_changes() == Some(n + 2) && s.debug_input_open() == Some(true)
    });
    vizdaw_ffi::host::set_audio_input_device(None);
    s.close().unwrap();
}

/// 5d-2 re-review Minor (carry to 5e): a Measure refused because its
/// lane records a channel the device lacks is refused before it claims
/// the engine, so another window's plugin engine (and with it that
/// window's monitoring) is left as it was.
#[test]
fn a_measure_refused_for_its_channel_leaves_another_windows_plugin_engine() {
    let g = serial(Some(DELAY_MS), None);
    vizdaw_ffi::catalog::add_test_bundle(&test_plugins::bundle()).unwrap();
    let d = doc(true, 2);
    let (with_gain, slot) = with_gain(&d);
    let a = untitled(&g, with_gain);
    a.play(d.seq.0).unwrap();
    idle_until(&a, |s| {
        s.plugin_slots()
            .unwrap()
            .iter()
            .any(|r| r.id == slot.0 && r.status == SlotStatus::Live)
            && s.transport().unwrap().playing
    });
    a.stop().unwrap();
    wait_until("A to stop", || !a.transport().unwrap().playing);
    let a_id = vizdaw_ffi::host::factory_owner().unwrap();
    let created = vizdaw_ffi::host::hosts_created();
    // B's return lane records fake input 8 of 3, set before B exists (an
    // input edit in B would claim the engine itself).
    let mut b = Builder::from_doc(d.doc.clone());
    b.run(Command::Lane(LaneCmd::SetInput {
        seq: d.seq,
        lane: d.rec,
        input: Some(LaneInput::Audio {
            channels: InputChannels::Mono(7),
            return_of: Some(d.synth),
        }),
    }))
    .unwrap();
    let other = untitled(&g, b.finish());
    let o2 = Arc::clone(&other);
    let dest = d.synth.0;
    let r = std::thread::spawn(move || o2.measure_return(dest))
        .join()
        .unwrap();
    match r {
        Err(VizError::Model { message }) => {
            assert!(message.contains("the audio input has 3 channel(s)"), "{message}")
        }
        other => panic!("expected the channel refusal, got {other:?}"),
    }
    assert_eq!(
        vizdaw_ffi::host::factory_owner(),
        Some(a_id),
        "A's plugin engine stays"
    );
    assert_eq!(vizdaw_ffi::host::holder(), Some(a_id));
    assert_eq!(vizdaw_ffi::host::hosts_created(), created, "no host made");
    other.close().unwrap();
    a.close().unwrap();
}
VIZDAW_EOF
```

- [ ] **Step 2: Run the tests to verify they fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-ffi --test synth -- a_device_chosen_during_a_measure a_measure_refused_for_its_channel 2>&1 | grep -E "^test |test result"
scripts/build-ffi.sh >/dev/null && scripts/swift.sh test 2>&1 | grep -E "aRefusedPunchDraftRefusesRecord|Test run"
```

Expected: `a_measure_refused_for_its_channel_leaves_another_windows_plugin_engine` FAILS (`A's plugin engine stays`: the claim retired it); `aRefusedPunchDraftRefusesRecord` FAILS (Record recorded with the old range). `a_device_chosen_during_a_measure_reaches_the_input_at_its_end` PASSES already: it is the plan's one exemption from the RED rule (Global Constraints, "TDD"). It is the missing test for behaviour that already works: the 5d-2 re-reviewer proved it times out with the old `end_measuring` at `39a4cc1`. Record in the report that it passed before any change, and cite that proof.

- [ ] **Step 3: Refuse Record on a refused draft**

In `app/Sources/VizDAWAppModel/RecordOptions.swift`, `commitPunch(_:)` becomes `@discardableResult public func commitPunch(_ f: TransportField) -> Bool` (true when there was no draft or `document?.commitField(f, text)` returned true) and `commitPunchDrafts()` becomes `@discardableResult public func commitPunchDrafts() -> Bool`, committing In then Out (both, even when In fails, so each refusal is said once and the last message stays in the status line) and returning false when **either** was refused. Keep their doc lines and add "Returns whether every draft was taken." to both; `commitPunch`'s doc also gains "A refused draft is dropped: the field shows the old value again." (so a second Record press records at the old range, which the field now shows; accepted, pre-flight M18).

In `app/Sources/VizDAWAppModel/RecordModel.swift`:

Find:
```swift
        commitPunchDrafts()
        do {
            try m.session.record(seq: seq, spec: spec())
```
Replace:
```swift
        // A draft that is not a position refuses the pass (5d-2 final
        // re-review N1): it must not punch at the old range.
        guard commitPunchDrafts() else {
            return .refused(m.editor.status ?? "The punch time is not a position.")
        }
        do {
            try m.session.record(seq: seq, spec: spec())
```

- [ ] **Step 4: Check the channel before Measure's claim**

In `crates/core-ffi/src/host.rs`:

Find:
```rust
    crate::synth::external_dest(doc, dest)?;
    let (generation, holder, asked) = {
```
Replace:
```rust
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
```

- [ ] **Step 5: Run the tests to verify they pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -2
cargo clippy --workspace --all-targets --target x86_64-unknown-linux-gnu --target-dir target/linux-check -- -D warnings 2>&1 | tail -2
cargo clippy -p core-ffi --lib -- -D warnings 2>&1 | tail -2
cargo test -p core-ffi --test synth 2>&1 | grep -E "test result|FAILED"
scripts/build-ffi.sh && scripts/swift.sh build 2>&1 | grep -E "error:|warning:" | grep -v MemoryDefaults | sort -u
scripts/swift.sh test 2>&1 | grep -E "Test run"
```

Expected: every `synth` test passes (`measure_refuses_what_it_cannot_measure` still finds "the audio input has 3 channel(s)": the same text, now from before the claim); Swift "Test run with 846 tests … passed".

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add app/Sources/VizDAWAppModel/RecordOptions.swift app/Sources/VizDAWAppModel/RecordModel.swift \
  app/Tests/VizDAWAppModelTests/RecordOptionsTests.swift crates/core-ffi/src/host.rs crates/core-ffi/tests/synth.rs
git commit -m "app, core-ffi: a refused punch draft refuses Record; Measure checks its channel before the claim; the Measure half of I1 tested (5e-1 Task 1)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -3
```

### Task 2: The strip automation mode; Off plays no automation; suspend

**Files:**
- Modify: `crates/core-model/src/automation.rs` (`AutomationMode`, `strip_of`, `target_exists`), `crates/core-model/src/mixer.rs` (`Strip.automation`), `crates/core-model/src/command/mixer.rs` (`MixerCmd::SetAutomationMode`)
- Modify: `crates/core-engine/src/schedule/compile.rs` (`CompileOptions.suspend_automation`, the Off filter), `crates/core-engine/src/engine.rs` (`set_automation_suspended`)
- Test: `crates/core-model/tests/mixer_commands.rs`, `crates/core-engine/tests/schedule_automation.rs`, `crates/core-engine/tests/schedule.rs` (four `CompileOptions` literals gain the field)

**Interfaces:**
- Consumes: `AutomationTarget`, `Document::find_slot`, `SlotOwner`, `Mixer::find_kind` (4d, 3a).
- Produces:
  - `core_model::automation::AutomationMode { Off, Read, Touch, Latch, Write, Trim }` (`Default` = Read, serde snake_case) with `const ALL: [AutomationMode; 6]` (menu order: Off, Read, Touch, Latch, Write, Trim), `fn is_read(&self) -> bool`, `fn writes(self) -> bool` (Touch, Latch, Write, Trim), `fn name(self) -> &'static str` ("Off" … "Trim").
  - `core_model::automation::strip_of(doc: &Document, t: AutomationTarget) -> Option<StripId>` and `target_exists(doc: &Document, t: AutomationTarget) -> bool` (moved from `compile.rs`, which then calls it).
  - `Strip.automation: AutomationMode`; `MixerCmd::SetAutomationMode { strips: Vec<StripId>, mode: AutomationMode }`, named "Set Automation Mode".
  - `CompileOptions.suspend_automation: bool` (`CompileOptions::new` sets false); `Engine::set_automation_suspended(&mut self, on: bool)` (the next `load` uses it).

Rulings 2 and 3. A project whose strips are all Read saves exactly as before.

- [ ] **Step 1: Write the failing tests**

Append to `crates/core-model/tests/mixer_commands.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat >> crates/core-model/tests/mixer_commands.rs <<'VIZDAW_EOF'

/// Sub-project 5 spec §4.6, plan 5e-1 Ruling 2: the mode is set on any
/// number of strips as one undo step, refused for none or an unknown
/// strip (a strip listed twice is set once), and saved only when it is
/// not Read (older files and all-Read projects are unchanged).
#[test]
fn the_automation_mode_is_one_undo_step_and_saved_only_when_not_read() {
    use core_model::automation::AutomationMode;
    use core_model::file::{from_json, to_json};
    let d = base();
    let plain = to_json(&d).unwrap();
    assert!(!plain.contains("\"automation\""), "Read is not written");
    assert!(d.mixer().strips().values().all(|s| s.automation == AutomationMode::Read));
    let mut h = History::new(d.clone());
    h.execute(
        &Command::Mixer(MixerCmd::SetAutomationMode {
            strips: vec![StripId(1), StripId(4)],
            mode: AutomationMode::Touch,
        }),
        &ViewFilter::default(),
    )
    .unwrap();
    assert_eq!(h.undo_label(), Some("Set Automation Mode"));
    let modes = |d: &Document| -> Vec<(u64, AutomationMode)> {
        d.mixer().strips().values().map(|s| (s.id.0, s.automation)).collect()
    };
    let touched = modes(h.doc());
    assert!(touched.contains(&(1, AutomationMode::Touch)) && touched.contains(&(4, AutomationMode::Touch)));
    assert!(touched.contains(&(5, AutomationMode::Read)));
    // Saved and read back.
    let text = to_json(h.doc()).unwrap();
    assert!(text.contains("\"automation\": \"touch\""), "{text}");
    assert_eq!(modes(&from_json(&text).unwrap()), touched);
    h.undo().unwrap();
    assert_eq!(to_json(h.doc()).unwrap(), plain);
    // A strip listed twice is set once (pre-flight M21). Setting it twice
    // to one mode would look the same, so this half of the dedupe is not
    // observable here; the app's half is pinned by `"1,1"` (Task 10).
    let twice = mx(&d, MixerCmd::SetAutomationMode { strips: vec![StripId(1), StripId(1)], mode: AutomationMode::Latch })
        .unwrap();
    assert_eq!(twice.mixer().strip(StripId(1)).unwrap().automation, AutomationMode::Latch);
    assert!(twice.mixer().strips().values().filter(|s| s.automation == AutomationMode::Latch).count() == 1);
    // Refused: no strips, an unknown strip (nothing changes).
    for strips in [vec![], vec![StripId(1), StripId(99)]] {
        let e = mx(&d, MixerCmd::SetAutomationMode { strips: strips.clone(), mode: AutomationMode::Off });
        assert!(e.is_err(), "{strips:?}");
    }
    assert_eq!(AutomationMode::ALL.map(AutomationMode::name), ["Off", "Read", "Touch", "Latch", "Write", "Trim"]);
    assert!(AutomationMode::ALL.iter().filter(|m| m.writes()).count() == 4);
}

/// Plan 5e-1 Ruling 3: a target's strip: its own controls and sends, an
/// insert's strip, a plugin instrument's strip; velocity scale has none.
#[test]
fn every_target_names_the_strip_whose_mode_governs_it() {
    use core_model::automation::{AutomationTarget as T, SendIndex, strip_of, target_exists};
    let mut d = base();
    let plugin = PluginRef {
        format: core_model::mixer::PluginFormat::Clap,
        id: "vizdaw.test:gain=1".into(),
        name: "Gain".into(),
        vendor: "V".into(),
        version: "1".into(),
    };
    d = mx(&d, MixerCmd::AddInsert { strip: StripId(4), index: None, plugin }).unwrap();
    let insert = d.mixer().strip(StripId(4)).unwrap().inserts[0].id;
    let pad = match &d.instruments().get(&DestId(1)).unwrap().kind {
        DestKind::PluginInstrument { slot } => slot.id,
        _ => unreachable!(),
    };
    assert_eq!(strip_of(&d, T::StripFader(StripId(3))), Some(StripId(3)));
    assert_eq!(strip_of(&d, T::SendLevel(StripId(1), SendIndex(0))), Some(StripId(1)));
    assert_eq!(strip_of(&d, T::PluginParam(insert, ParamId(0))), Some(StripId(4)));
    assert_eq!(strip_of(&d, T::PluginParam(pad, ParamId(0))), Some(StripId(2)));
    assert_eq!(strip_of(&d, T::VelocityScale(DestId(0))), None);
    assert_eq!(strip_of(&d, T::PluginParam(PluginInstanceId(77), ParamId(0))), None);
    assert!(target_exists(&d, T::StripMute(StripId(5))));
    assert!(!target_exists(&d, T::StripMute(StripId(50))));
    assert!(!target_exists(&d, T::SendLevel(StripId(1), SendIndex(3))));
}
VIZDAW_EOF
```

`base()`'s strip numbering is in its doc line (master 0, synth 1, Pad 2 with plugin slot 0, audio path 3, buses 4 and 5).

Append to `crates/core-engine/tests/schedule_automation.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat >> crates/core-engine/tests/schedule_automation.rs <<'VIZDAW_EOF'

/// Sub-project 5 spec §4.6 and §7.5, plan 5e-1 Ruling 3: an Off strip's
/// automation is not played (its fader, its insert's parameter); other
/// strips' still is; suspended, every strip plays its automation.
#[test]
fn an_off_strip_plays_no_automation_unless_suspended() {
    use core_model::automation::AutomationMode;
    use core_model::command::MixerCmd;
    let mut b = Builder::new();
    let d = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, notes) = b.sequence("S", "Notes", Some(d)).unwrap();
    b.notes(seq, notes, &[(0, 60, 0x8000, 4 * Q)]).unwrap();
    let synth = b.strip_of(StripKind::BuiltinSynth).unwrap();
    let bus = b.bus("Bus").unwrap();
    let lane = b.lane(seq, "Auto", None).unwrap();
    b.events(
        seq,
        lane,
        vec![
            auto(0, AutomationTarget::StripFader(synth), 0.25),
            auto(0, AutomationTarget::StripFader(bus), 0.75),
        ],
    )
    .unwrap();
    b.mixer(MixerCmd::SetAutomationMode { strips: vec![synth], mode: AutomationMode::Off })
        .unwrap();
    let dests = |opts: &CompileOptions| -> Vec<AutoDest> {
        let mut table = TargetTable::new();
        let (sch, _) = compile(b.doc(), seq, opts, &mut |k: &TargetKind| table.intern(k.clone()).map(Ok)).unwrap();
        sch.automation.lanes().iter().map(|l| l.dest).collect()
    };
    let mut opts = CompileOptions::new(SR);
    assert_eq!(dests(&opts), vec![AutoDest::Strip(bus, StripParam::Fader)]);
    opts.suspend_automation = true;
    assert_eq!(
        dests(&opts),
        vec![AutoDest::Strip(synth, StripParam::Fader), AutoDest::Strip(bus, StripParam::Fader)]
    );
}
VIZDAW_EOF
```

(The order of the two dests is `Automation`'s, by destination: the synth's strip ID is lower than the bus's in this builder.)

- [ ] **Step 2: Run the tests to verify they fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test mixer_commands 2>&1 | tail -3
cargo test -p core-engine --test schedule_automation 2>&1 | tail -3
```

Expected: both FAIL to compile (`AutomationMode`, `SetAutomationMode`, `strip_of`, `suspend_automation` do not exist).

- [ ] **Step 3: The mode, the command and the owner of a target**

In `crates/core-model/src/automation.rs`, add `AutomationMode` with the items of the Interfaces block and `#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)] #[serde(rename_all = "snake_case")]` (`#[default]` on `Read`), its doc line quoting spec §4.6. Add `strip_of` (Ruling 3: `StripFader/Pan/Mute/Solo(s)` and `SendLevel(s, _)` → `s` when the strip exists; `PluginParam(p, _)` → `doc.find_slot(p)`: `SlotOwner::Insert(s, _)` → `s`, `SlotOwner::Instrument(dest)` → `doc.mixer().find_kind(StripKind::Instrument(dest))`; `VelocityScale` → `None`) and move `target_exists` here from `crates/core-engine/src/schedule/compile.rs` (unchanged, now `pub`); `compile.rs` calls `core_model::automation::target_exists`.

In `crates/core-model/src/mixer.rs`:

Find:
```rust
    /// Main output.
    pub output: Output,
}
```
Replace:
```rust
    /// Main output.
    pub output: Output,
    /// Its automation mode (sub-project 5 spec §4.6); not written when
    /// Read, so all-Read projects save as before (plan 5e-1 Ruling 2).
    #[serde(default, skip_serializing_if = "AutomationMode::is_read")]
    pub automation: AutomationMode,
}
```
and in `Strip::new`, after `output: Output::Master,` add `automation: AutomationMode::Read,`; import `crate::automation::AutomationMode`.

In `crates/core-model/src/command/mixer.rs`, add the variant after `AddInstrumentOutput` (doc: "Set the automation mode of `strips` (spec §4.6), one undo step.", fields `strips: Vec<StripId>` "The strips (at least one).", `mode: AutomationMode` "The mode."), its name `"Set Automation Mode"` in `name()`, and its arm in `apply` before `MixerCmd::AddInstrumentOutput { dest, index, name } => {`: `ModelError::invalid("set the automation mode of at least one strip")` for an empty list; every strip must exist (`d.mixer_mut().strip_mut(*s)?` sets it, so an unknown one fails the whole command, which works on a copy); a repeated ID is skipped, keeping the first occurrence (M21).

- [ ] **Step 4: Off and suspend in the schedule**

In `crates/core-engine/src/schedule/compile.rs`, `CompileOptions` gains (doc: "Automation is suspended (spec §7.5): Off strips play theirs too (plan 5e-1 Ruling 3).") `pub suspend_automation: bool`, false in `CompileOptions::new`. Then:

Find:
```rust
        if !target_exists(doc, target) {
            report.automation_missing += 1;
            continue;
        }
```
Replace:
```rust
        if !target_exists(doc, target) {
            report.automation_missing += 1;
            continue;
        }
        // An Off strip plays its static values (spec §4.6), unless
        // automation is suspended (every strip then behaves as Read).
        let off = strip_of(doc, target)
            .and_then(|s| doc.mixer().strip(s).ok())
            .is_some_and(|s| s.automation == AutomationMode::Off);
        if off && !opts.suspend_automation {
            continue;
        }
```
(import `core_model::automation::{AutomationMode, strip_of, target_exists}`; delete the local `target_exists`). The four `CompileOptions { … }` literals in `crates/core-engine/tests/schedule.rs` gain `suspend_automation: false,`.

In `crates/core-engine/src/engine.rs`, `Engine` gains `automation_suspended: bool` (`automation_suspended: false,` in `Engine::with_platform`'s literal, right after `midi_tx,`; Task 5 adds `gesture_tx` after it), `pub fn set_automation_suspended(&mut self, on: bool)` (doc: "Whether automation is suspended (spec §7.5): the next [`Engine::load`] compiles with it.") and `suspend_automation: self.automation_suspended,` in `load`'s `CompileOptions`.

- [ ] **Step 5: Run the tests to verify they pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -2
cargo clippy --workspace --all-targets --target x86_64-unknown-linux-gnu --target-dir target/linux-check -- -D warnings 2>&1 | tail -2
cargo test --workspace --no-fail-fast 2>&1 | grep -E "test result|FAILED|panicked" | grep -v " 0 failed"; echo "workspace done"
```

Expected: "workspace done" with no failure lines; Rust 1467 passed (1462, Task 1's two, this task's three).

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-model/src/automation.rs crates/core-model/src/mixer.rs crates/core-model/src/command/mixer.rs \
  crates/core-model/tests/mixer_commands.rs crates/core-engine/src/schedule/compile.rs crates/core-engine/src/engine.rs \
  crates/core-engine/tests/schedule_automation.rs crates/core-engine/tests/schedule.rs
git commit -m "core-model, core-engine: the strip automation mode; Off plays no automation unless suspended (5e-1 Task 2)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -3
```

### Task 3: Automation writes in the record pass, placed by the model

**Files:**
- Modify: `crates/core-model/src/command/record.rs` (`AutomationWrite`, `MAX_WRITE_POINTS`, `RecordPass.automation`, `RecordPass::name`, `apply`), `crates/core-model/src/command/mod.rs` (export)
- Modify (the new field in every literal): `crates/core-model/tests/cmd_record.rs`, `crates/core-ffi/src/synth.rs`, `crates/core-ffi/src/passes.rs`
- Create: `crates/core-model/tests/cmd_automation.rs`

**Interfaces:**
- Consumes: Task 2's `target_exists`; `Lane::active`, `Take.events`, `Document::{insert_event, remove_event}`, `rules::check_ticks`, `automation::check_value`.
- Produces:
  ```rust
  /// One target's automation a pass wrote (spec §7.3, §7.4; plan 5e-1 Ruling 4).
  pub struct AutomationWrite {
      /// The top-level sequence that played.
      pub seq: SeqId,
      /// What it drives.
      pub target: AutomationTarget,
      /// First tick of the replaced range (included).
      pub start: Tick,
      /// Last tick of the replaced range (included).
      pub end: Tick,
      /// The new points, inside `[start, end]`, by tick.
      pub points: Vec<(Tick, f64)>,
      /// The name of the lane made when no lane of `seq` holds the target's automation.
      pub lane_name: String,
  }
  pub const MAX_WRITE_POINTS: usize = 100_000;
  ```
  (`#[derive(Debug, Clone, PartialEq)]`), `RecordPass.automation: Vec<AutomationWrite>` ("Automation written while it played, applied after the lanes, in order."), exported from `core_model::command`. `RecordPass::name()` answers "Write Automation" for a pass with no media and no lane parts but some automation.

Ruling 4 as amended (pre-flight I5). Applying a write: `seq` must exist; `lane_name` non-empty (trimmed); `start <= end`, both within `check_ticks`; at most `MAX_WRITE_POINTS` points, each value `check_value` and each tick within `[start, end]`; the target must exist (`target_exists`). Then:
- from the active take of **every** lane of `seq` whose active take holds an `EventBody::Automation` of the target, remove the target's automation events with `start <= tick <= end` (each reported removed in `r.removed`);
- insert the points (sorted by tick, stable) as automation events through `check_event` (reported created) into the first such lane (lane order); with none, into a new lane at the end (`c.alloc_lane()`, `c.alloc_take()`, `Lane::new(lane, take, lane_name)`, reported created as `LaneCmd::Add` reports it).
Lanes inside referenced sequences are not looked at (doc line on `write_automation`: "Automation inside referenced sequences is left: their lanes cannot record (spec Ruling 15)."). Any failure fails the whole pass (the command works on a copy); the session never hands the model a write whose target is gone (Ruling 25, Task 8).

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-model/tests/cmd_automation.rs <<'VIZDAW_EOF'
//! Automation a pass wrote (plan 5e-1 Task 3; sub-project 5 spec §4.7,
//! §7.3, §7.4): each write goes into the first lane of its sequence that
//! already holds its target, else into a new lane at the end; it replaces
//! the target's points in its range in every lane that holds the target
//! (pre-flight I5), and nothing else; a bad write refuses the whole pass;
//! a pass with only automation is "Write Automation".

use core_model::automation::AutomationTarget;
use core_model::command::{AutomationWrite, Command, EventCmd, LaneCmd, RecordPass, SequenceCmd, apply};
use core_model::document::Document;
use core_model::event::{EventBody, EventSpec};
use core_model::history::History;
use core_model::ids::{LaneId, SeqId, StripId};
use core_model::selection::ViewFilter;
use core_model::time::{PPQ, Tick};

const Q: i64 = PPQ;

fn fader(d: &Document) -> AutomationTarget {
    AutomationTarget::StripFader(d.mixer().master())
}

fn point(t: i64, target: AutomationTarget, v: f64) -> EventSpec {
    EventSpec::new(Tick(t), EventBody::Automation { target, value: v })
}

/// Sequence 0 with lanes A (0, a note), B (1, the master fader at 0 and
/// 8Q) and C (2, the master fader at 2Q).
fn doc() -> Document {
    let mut d = apply(&Document::new(), &Command::Sequence(SequenceCmd::New { name: "S".into() }))
        .unwrap()
        .doc;
    for name in ["B", "C"] {
        d = apply(&d, &Command::Lane(LaneCmd::Add { seq: SeqId(0), name: name.into(), index: None }))
            .unwrap()
            .doc;
    }
    let f = fader(&d);
    for (lane, events) in [
        (0, vec![EventSpec::new(Tick(0), EventBody::note(60, 30_000, Tick(Q)))]),
        (1, vec![point(0, f, 0.2), point(8 * Q, f, 0.8)]),
        (2, vec![point(2 * Q, f, 0.5)]),
    ] {
        d = apply(&d, &Command::Event(EventCmd::Insert { seq: SeqId(0), lane: LaneId(lane), events }))
            .unwrap()
            .doc;
    }
    d
}

fn write(d: &Document, start: i64, end: i64, points: &[(i64, f64)]) -> AutomationWrite {
    AutomationWrite {
        seq: SeqId(0),
        target: fader(d),
        start: Tick(start),
        end: Tick(end),
        points: points.iter().map(|(t, v)| (Tick(*t), *v)).collect(),
        lane_name: "Master Fader".into(),
    }
}

fn pass(writes: Vec<AutomationWrite>) -> RecordPass {
    RecordPass { automation: writes, ..RecordPass::default() }
}

/// `(tick, value)` of lane `lane`'s automation, by tick.
fn points(d: &Document, lane: u64) -> Vec<(i64, f64)> {
    let mut v: Vec<(i64, f64)> = d
        .lane(SeqId(0), LaneId(lane))
        .unwrap()
        .active()
        .unwrap()
        .events
        .values()
        .filter_map(|e| match e.body {
            EventBody::Automation { value, .. } => Some((e.tick.0, value)),
            _ => None,
        })
        .collect();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v
}

#[test]
fn a_write_goes_into_the_first_lane_holding_its_target() {
    let d = doc();
    let w = write(&d, Q - 1, 4 * Q + 1, &[(Q - 1, 0.3), (Q, 0.9), (4 * Q, 0.9), (4 * Q + 1, 0.4)]);
    let out = apply(&d, &Command::Record(pass(vec![w]))).unwrap().doc;
    // B (the first holding the master fader) gets it; its points outside
    // the range stay; C's point inside the range goes too (every lane
    // plays, pre-flight I5); A is untouched; no lane is added.
    assert_eq!(points(&out, 1), vec![(0, 0.2), (Q - 1, 0.3), (Q, 0.9), (4 * Q, 0.9), (4 * Q + 1, 0.4), (8 * Q, 0.8)]);
    assert!(points(&out, 2).is_empty(), "C's point inside the range is replaced too: {:?}", points(&out, 2));
    assert_eq!(out.sequence(SeqId(0)).unwrap().lanes.len(), 3);
    // A second write at C's old tick still goes to B (C is not first).
    let w = write(&d, 2 * Q, 2 * Q, &[(2 * Q, 0.1)]);
    let out = apply(&out, &Command::Record(pass(vec![w]))).unwrap().doc;
    assert!(points(&out, 2).is_empty(), "C's point inside the range is replaced too: {:?}", points(&out, 2));
    assert!(points(&out, 1).contains(&(2 * Q, 0.1)));
}

/// Amended Ruling 4: a write removes the target's points in its range
/// from every lane holding the target, and leaves their points outside
/// it.
#[test]
fn a_write_leaves_other_lanes_outside_its_range() {
    let d = doc();
    let f = fader(&d);
    let d = apply(
        &d,
        &Command::Event(EventCmd::Insert { seq: SeqId(0), lane: LaneId(2), events: vec![point(6 * Q, f, 0.7)] }),
    )
    .unwrap()
    .doc;
    let w = write(&d, Q - 1, 4 * Q + 1, &[(Q - 1, 0.3), (4 * Q + 1, 0.4)]);
    let out = apply(&d, &Command::Record(pass(vec![w]))).unwrap().doc;
    assert_eq!(points(&out, 2), vec![(6 * Q, 0.7)], "C keeps its point outside the range");
    assert_eq!(points(&out, 1), vec![(0, 0.2), (Q - 1, 0.3), (4 * Q + 1, 0.4), (8 * Q, 0.8)]);
}

#[test]
fn a_write_with_no_lane_for_its_target_makes_one_at_the_end() {
    let d = doc();
    let pan = AutomationTarget::StripPan(StripId(99));
    let mut w = write(&d, 0, Q, &[(0, 0.5), (Q, 0.5)]);
    w.target = pan;
    assert!(apply(&d, &Command::Record(pass(vec![w.clone()]))).is_err(), "strip 99 does not exist");
    w.target = AutomationTarget::StripPan(d.mixer().master());
    w.lane_name = "Master Pan".into();
    let mut h = History::new(d.clone());
    h.execute(&Command::Record(pass(vec![w])), &ViewFilter::default()).unwrap();
    assert_eq!(h.undo_label(), Some("Write Automation"));
    let s = h.doc().sequence(SeqId(0)).unwrap();
    assert_eq!(s.lanes.len(), 4);
    let last = s.lanes.last().unwrap();
    assert_eq!(last.name, "Master Pan");
    assert_eq!(points(h.doc(), last.id.0), vec![(0, 0.5), (Q, 0.5)]);
    h.undo().unwrap();
    assert_eq!(h.doc(), &d, "one undo takes the lane and its points back");
}

#[test]
fn a_loop_passes_later_write_wins_where_they_overlap() {
    let d = doc();
    let first = write(&d, 0, 4 * Q, &[(0, 0.1), (4 * Q, 0.1)]);
    let second = write(&d, 2 * Q, 6 * Q, &[(2 * Q, 0.6), (6 * Q, 0.6)]);
    let out = apply(&d, &Command::Record(pass(vec![first, second]))).unwrap().doc;
    assert_eq!(points(&out, 1), vec![(0, 0.1), (2 * Q, 0.6), (6 * Q, 0.6), (8 * Q, 0.8)]);
}

#[test]
fn bad_writes_refuse_the_whole_pass() {
    let d = doc();
    let good = write(&d, 0, Q, &[(0, 0.5)]);
    let mut cases: Vec<AutomationWrite> = Vec::new();
    let mut w = good.clone();
    w.points = vec![(Tick(0), f64::NAN)];
    cases.push(w);
    let mut w = good.clone();
    w.points = vec![(Tick(0), 1.5)];
    cases.push(w);
    let mut w = good.clone();
    w.points = vec![(Tick(2 * Q), 0.5)];
    cases.push(w); // outside its range
    let mut w = good.clone();
    w.start = Tick(Q);
    w.end = Tick(0);
    cases.push(w);
    let mut w = good.clone();
    w.end = Tick(1 << 41);
    cases.push(w);
    let mut w = good.clone();
    w.seq = SeqId(7);
    cases.push(w);
    let mut w = good.clone();
    w.target = AutomationTarget::StripMute(StripId(99));
    cases.push(w);
    let mut w = good.clone();
    w.points = (0..=core_model::command::MAX_WRITE_POINTS as i64).map(|t| (Tick(t.min(Q)), 0.5)).collect();
    cases.push(w);
    let mut w = AutomationWrite { lane_name: "  ".into(), ..good.clone() };
    w.target = AutomationTarget::StripSolo(d.mixer().master());
    cases.push(w); // a new lane needs a name
    for bad in cases {
        let r = apply(&d, &Command::Record(pass(vec![good.clone(), bad.clone()])));
        assert!(r.is_err(), "{bad:?}");
    }
}

#[test]
fn a_pass_with_lanes_keeps_its_record_label() {
    let d = doc();
    let mut p = pass(vec![write(&d, 0, Q, &[(0, 0.5)])]);
    p.lanes.push(core_model::command::LaneRecord {
        seq: SeqId(0),
        lane: LaneId(0),
        clear: Vec::new(),
        active: core_model::command::RecordedContent {
            events: vec![EventSpec::new(Tick(Q), EventBody::note(62, 30_000, Tick(Q)))],
            regions: Vec::new(),
        },
        takes: Vec::new(),
    });
    assert_eq!(Command::Record(p.clone()).name(), "Record");
    p.lanes.clear();
    assert_eq!(Command::Record(p).name(), "Write Automation");
}
VIZDAW_EOF
```

- [ ] **Step 2: Run the tests to verify they fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test cmd_automation 2>&1 | tail -3
```

Expected: FAIL to compile (`AutomationWrite` does not exist).

- [ ] **Step 3: The writes**

In `crates/core-model/src/command/record.rs`:

Find:
```rust
    /// Each lane's part.
    pub lanes: Vec<LaneRecord>,
}
```
Replace:
```rust
    /// Each lane's part.
    pub lanes: Vec<LaneRecord>,
    /// Automation written while it played (spec §7.4), applied after the
    /// lanes, in order (plan 5e-1 Ruling 4).
    pub automation: Vec<AutomationWrite>,
}
```

Add `AutomationWrite` and `MAX_WRITE_POINTS` as in the Interfaces block above the `RecordPass` struct; `name()` becomes:

```rust
    /// The undo label: "Record", "Record (with gaps)", or "Write
    /// Automation" for a pass that wrote only automation (spec §7.4).
    pub fn name(&self) -> &'static str {
        if self.media.is_empty() && self.lanes.is_empty() && !self.automation.is_empty() {
            "Write Automation"
        } else if self.with_gaps {
            "Record (with gaps)"
        } else {
            "Record"
        }
    }
```

In `apply`, Find:
```rust
            insert_content(d, l.seq, l.lane, Some(take), content, &files, r)?;
        }
    }
    Ok(())
}
```
Replace:
```rust
            insert_content(d, l.seq, l.lane, Some(take), content, &files, r)?;
        }
    }
    for w in &pass.automation {
        write_automation(d, w, r)?;
    }
    Ok(())
}
```
and add `fn write_automation(d: &mut Document, w: &AutomationWrite, r: &mut Report) -> Result<()>` doing what this task's opening paragraph says. Export `AutomationWrite` and `MAX_WRITE_POINTS` from `crates/core-model/src/command/mod.rs`'s `pub use record::{…}`.

Every existing `RecordPass { … }` literal gains `automation: Vec::new(),`: the fifteen in `crates/core-model/tests/cmd_record.rs` (lines 103, 123, 159, 190, 212, 218, 224, 230, 239, 246, 306, 396, 411, 449 and 521 at `1f6057a`), the one in `crates/core-ffi/src/synth.rs` (`:342`) and the one at the end of `crates/core-ffi/src/passes.rs::plan` (`:679`).

- [ ] **Step 4: Run the tests to verify they pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -2
cargo clippy --workspace --all-targets --target x86_64-unknown-linux-gnu --target-dir target/linux-check -- -D warnings 2>&1 | tail -2
cargo test -p core-model 2>&1 | grep -E "test result|FAILED" | grep -v " 0 failed"
cargo test -p core-ffi --test passes --test record 2>&1 | grep -E "test result|FAILED"
```

Expected: every `core-model` test passes (the `cmd_record` proptest included: an empty `automation` changes nothing); `cmd_automation` 6 passed (Rust 1473).

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-model/src/command/record.rs crates/core-model/src/command/mod.rs crates/core-model/tests/cmd_automation.rs \
  crates/core-model/tests/cmd_record.rs crates/core-ffi/src/synth.rs crates/core-ffi/src/passes.rs
git commit -m "core-model: automation writes in the record pass, placed in the first lane holding the target (5e-1 Task 3)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -3
```

### Task 4: Automation at each strip's arrival (5d-1 controller ruling C2)

**Files:**
- Modify: `crates/core-engine/src/graph/exec.rs` (arrivals in `Graph::build`; `strip_arrival`, `insert_arrival`, `arrivals`), `crates/core-engine/src/render.rs` (`auto_frame`, `next_run`, `automate`, the run loop's order)
- Modify: `docs/feature-backlog.md` (the row goes)
- Create: `crates/core-engine/tests/automation_arrival.rs`

**Interfaces:**
- Consumes: `Plan.t_in`, `Plan.t_out` (PDC), `StripSpec.strip`, `InsertSpec.{instance, latency}`, `RegionRun::read_frame` (5d-1).
- Produces:
  - `Graph::strip_arrival(&self, strip: StripId) -> u64` (the strip node's `t_out`; 0 when unknown), `Graph::insert_arrival(&self, id: PluginInstanceId) -> u64` (its strip's `t_in` plus the latency of the inserts before it; 0 when unknown), `Graph::arrivals(&self) -> &[u64]` (every distinct arrival, sorted, 0 included; kept for its test and for diagnostics, `next_run` does not use it). All `// RT` (binary searches over boxed slices built in `build`).
  - In `render.rs`: `fn auto_frame(run: &RegionRun, pos: u64, arrival: u64) -> u64` (`pos` for 0, else `run.read_frame(arrival).unwrap_or(pos.saturating_sub(arrival))`); `next_run(&self, length, left, regions: &RegionRun)` and `automate(&mut self, length, run, regions: &RegionRun)`.

Ruling 5. A lane's arrival: `AutoDest::Strip(s, _)` and `AutoDest::Send(s, _)` → `strip_arrival(s)`; `AutoDest::Plugin(id, _)` with `lane.instrument == None` (an insert) → `insert_arrival(id)`; a plugin instrument's lane → 0. `automate` reads each lane at `f0 = auto_frame(regions, pos, arrival)` (start), `f0 + run / 2` (the mute/solo switch point) and `f0 + run` (ramp end); a plugin lane's sub-steps at `f0 + f`. `next_run` cuts **per lane** (pre-flight I13: cutting at the global next point once per distinct arrival would cut every point up to A times, down to `MIN_RUN` slices on a latent session): for each lane, `r = auto_frame(regions, pos, arrival(lane))`, its next point frame after `r` by binary search in `auto.points(lane)` (`partition_point(|p| p.frame <= r)`), and the cut `next − r`; the run is the minimum over lanes (at least `MIN_RUN`), then the loop and end boundary as now. Each point therefore cuts once, as at arrival 0 today: O(lanes · log points) per run, `MAX_AUTO_LANES = 1024`. Doc line on `next_run`: "Cut at each lane's next point at its own arrival (pre-flight I13). // RT". The run loop builds `regions` before `next_run` (it depends only on the transport and the offset) and passes it to both.

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-engine/tests/automation_arrival.rs <<'VIZDAW_EOF'
//! Automation at each strip's arrival (plan 5e-1 Ruling 5; 5d-1
//! controller ruling C2): a strip's controls are read at the frame its
//! audio is at, so a point at bar 3 changes what is heard where bar 3 is
//! heard, on a led (read-ahead) path as on a delayed one.

use core_engine::backend::{AudioBackend, StereoBuf};
use core_engine::engine::{Engine, EngineConfig};
use core_engine::fixtures::{Builder, Q};
use core_engine::graph::mixer::mixer_spec;
use core_engine::graph::{
    EffectNode, Graph, InsertFactory, InsertInit, InstrumentFeed, ProcessStatus, TestInserts, plan,
};
use core_engine::offline::{OfflineBackend, OfflineSinks};
use core_engine::schedule::TargetIdx;
use core_media::caf::{CafWriter, WriteMode};
use core_media::stream::{DiskMode, MediaFile, StreamConfig};
use core_model::automation::AutomationTarget;
use core_model::document::Document;
use core_model::event::{EventBody, Fade};
use core_model::format::BitDepth;
use core_model::ids::{MediaId, SeqId};
use core_model::mixer::{ParamId, PluginSlot, StripKind};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

const QF: u64 = 24_000;
const FILE_FRAMES: u64 = 480_000;
const BLOCK: u32 = 512;

fn caf(path: &Path) {
    let mut w = CafWriter::create(path, 48_000, 2, BitDepth::Float32, WriteMode::Normal).unwrap();
    w.write(&vec![0.25f32; 2 * FILE_FRAMES as usize]).unwrap();
    w.finish().unwrap();
}

/// A region from beat 1 to beat 7 on lane "Audio"; a latent bus (latency
/// `l`) so the audio path leads by `l`; the audio path's strip unmuted at
/// 0 and muted from beat 3.
fn doc(l: u32) -> (Document, SeqId) {
    let mut b = Builder::new();
    let (seq, lane) = b.sequence("Song", "Audio", None).unwrap();
    let m = b.media("Media/a.caf", FILE_FRAMES, 2).unwrap();
    b.region(seq, lane, Q, m, 0, 6 * QF, 0.0, Fade::default(), Fade::default())
        .unwrap();
    let bus = b.bus("Latent").unwrap();
    b.insert(bus, TestInserts::plugin(&format!("latency={l}"))).unwrap();
    let out = b.doc().lane(seq, lane).unwrap().audio_out.unwrap();
    let path = b.strip_of(StripKind::AudioPath(out)).unwrap();
    let mute = AutomationTarget::StripMute(path);
    b.event(seq, lane, 0, EventBody::Automation { target: mute, value: 0.0 }).unwrap();
    b.event(seq, lane, 3 * Q, EventBody::Automation { target: mute, value: 1.0 }).unwrap();
    (b.finish(), seq)
}

/// The left channel of `blocks` blocks from bar 1, with or without leads.
fn render(doc: &Document, seq: SeqId, leads: bool, blocks: usize) -> Vec<f32> {
    let dir = tempfile::tempdir().unwrap();
    caf(&dir.path().join("a.caf"));
    let mut cfg = EngineConfig::new(48_000.0);
    cfg.disk = DiskMode::Manual;
    cfg.stream = StreamConfig { cache_file_bytes: 0, ..StreamConfig::default() };
    cfg.region_leads = leads;
    let (mut engine, renderer) = Engine::new(cfg, Box::new(OfflineSinks::new(64))).unwrap();
    engine.set_insert_factory(Box::new(TestInserts::new()));
    engine.set_media(vec![MediaFile { id: MediaId(0), path: dir.path().join("a.caf") }]);
    engine.load(doc, seq).unwrap();
    let mut out = OfflineBackend::new(48_000.0, BLOCK);
    out.start(renderer).unwrap();
    out.reserve(4096);
    engine.play().unwrap();
    let mut left = Vec::new();
    for _ in 0..blocks {
        engine.streamer().service();
        out.block(BLOCK);
        left.extend_from_slice(out.output(BLOCK).0);
    }
    left
}

/// Review Focus 1: the mute at beat 3 silences the path where beat 3 of
/// its audio is heard, `l` frames after beat 3, whether the path leads or
/// is delayed (before plan 5e-1 it fell `l` frames early on the led
/// path, whose strip's audio arrives `l` behind the renderer).
#[test]
fn a_led_paths_mute_lands_on_its_audio() {
    for l in [512u32, 4800] {
        let (d, seq) = doc(l);
        for leads in [true, false] {
            let left = render(&d, seq, leads, 400);
            let at = (3 * QF + u64::from(l)) as usize;
            assert!(left[at - 1] != 0.0, "heard up to beat 3: l {l}, leads {leads}");
            assert!(
                left[at..at + 4800].iter().all(|x| *x == 0.0),
                "silent from beat 3 as heard: l {l}, leads {leads}"
            );
            assert!(left[(QF + u64::from(l)) as usize + 10] != 0.0, "the region plays before");
        }
    }
}

/// Ruling 5's arrivals: a strip's controls at its node's output time, an
/// insert's parameter at its strip's input time plus the inserts before
/// it, 0 for strips without latency.
#[test]
fn arrivals_follow_the_plan() {
    let mut b = Builder::new();
    b.dest("Synth", "builtin:synth", 0).unwrap();
    let synth = b.strip_of(StripKind::BuiltinSynth).unwrap();
    let bus = b.bus("Bus").unwrap();
    let first = b.insert(bus, TestInserts::plugin("latency=100")).unwrap();
    let second = b.insert(bus, TestInserts::plugin("latency=50")).unwrap();
    let master = b.doc().mixer().master();
    let d = b.finish();
    let mut latency = |s: &PluginSlot| if s.plugin.id.ends_with("100") { 100 } else { 50 };
    let mg = mixer_spec(&d, &[InstrumentFeed::builtin(TargetIdx(0), 0)], &mut latency);
    let p = plan(&mg.spec, 96_000).unwrap();
    let g = Graph::build(&mg.spec, &p, 512, |_, _| InsertInit::Placeholder);
    assert_eq!(g.strip_arrival(synth), 0);
    assert_eq!(g.strip_arrival(bus), 150);
    assert_eq!(g.insert_arrival(first), 0);
    assert_eq!(g.insert_arrival(second), 100);
    assert_eq!(g.strip_arrival(master), 150, "master's inputs arrive aligned to the bus");
    assert_eq!(g.arrivals(), &[0, 100, 150]);
    assert_eq!(g.strip_arrival(core_model::ids::StripId(999)), 0);
}

/// An insert that counts its `process` calls: one per run the renderer
/// makes.
struct Counted(Arc<AtomicU64>, Box<dyn EffectNode>);

impl EffectNode for Counted {
    fn latency(&self) -> u32 {
        self.1.latency()
    }
    fn process(&mut self, frames: u32, io: &mut StereoBuf, sc: Option<&StereoBuf>) -> ProcessStatus {
        self.0.fetch_add(1, Ordering::Relaxed);
        self.1.process(frames, io, sc)
    }
    fn reset(&mut self) {
        self.1.reset();
    }
    fn automate(&mut self, frame: u32, param: ParamId, v: f64) -> bool {
        self.1.automate(frame, param, v)
    }
}

struct Counting(TestInserts, Arc<AtomicU64>);

impl InsertFactory for Counting {
    fn make(&mut self, slot: &PluginSlot, sr: f64, max: u32) -> Result<Box<dyn EffectNode>, String> {
        Ok(Box::new(Counted(Arc::clone(&self.1), self.0.make(slot, sr, max)?)))
    }
}

/// Pre-flight I13 (Ruling 5): each automation point cuts a run once,
/// however many distinct arrivals the graph has, so a latent session is
/// not rendered in slices around every point. Two inserts on a bus count
/// the runs; with latencies 100 and 150 (arrivals 0, 100, 250) the runs
/// are the same as with latencies 0 and 0 (arrival 0 only).
#[test]
fn each_point_cuts_once_on_a_latent_session() {
    let runs = |latencies: [u32; 2]| -> u64 {
        let mut b = Builder::new();
        let d = b.dest("Synth", "builtin:synth", 0).unwrap();
        let (seq, notes) = b.sequence("S", "Notes", Some(d)).unwrap();
        b.notes(seq, notes, &[(0, 60, 0x8000, 64 * Q)]).unwrap();
        let synth = b.strip_of(StripKind::BuiltinSynth).unwrap();
        let bus = b.bus("A").unwrap();
        for l in latencies {
            b.insert(bus, TestInserts::plugin(&format!("latency={l}"))).unwrap();
        }
        let fader = AutomationTarget::StripFader(synth);
        for (t, v) in [(0, 0.2), (Q / 4, 0.8), (Q / 2, 0.3), (3 * Q / 4, 0.6)] {
            b.event(seq, notes, t, EventBody::Automation { target: fader, value: v }).unwrap();
        }
        let doc = b.finish();
        let count = Arc::new(AtomicU64::new(0));
        let (mut engine, renderer) = Engine::new(EngineConfig::new(48_000.0), Box::new(OfflineSinks::new(64))).unwrap();
        engine.set_insert_factory(Box::new(Counting(TestInserts::new(), Arc::clone(&count))));
        engine.load(&doc, seq).unwrap();
        let mut out = OfflineBackend::new(48_000.0, BLOCK);
        out.start(renderer).unwrap();
        out.reserve(4096);
        engine.play().unwrap();
        out.block(BLOCK);
        let before = count.load(Ordering::Relaxed);
        for _ in 0..60 {
            out.block(BLOCK);
        }
        count.load(Ordering::Relaxed) - before
    };
    let plain = runs([0, 0]);
    assert!(plain > 2 * 60, "the points cut some runs: {plain}");
    assert_eq!(runs([100, 150]), plain, "a latent session cuts at the same points");
}
VIZDAW_EOF
```

(`Builder::insert` returns the slot's `PluginInstanceId`; `TestInserts::plugin("latency=…")`'s node reports that latency, and the test's `latency` function gives the plan the same.)

- [ ] **Step 2: Run the tests to verify they fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-engine --test automation_arrival 2>&1 | tail -3
```

Expected: FAIL to compile (`strip_arrival` does not exist). With the accessors stubbed to 0, `a_led_paths_mute_lands_on_its_audio` fails "heard up to beat 3" (the mute fell `l` early). `each_point_cuts_once_on_a_latent_session` is a guard on the design, not on today's code: at `1f6057a` there is one arrival, so it passes; it fails against the per-arrival cut the pre-revision plan wrote (each of the four points cut again at 100 and 250 frames after it). It is the plan's second RED exemption (Global Constraints, "TDD"); record that it passed before the change, and the reviewer checks that `next_run` cuts per lane.

- [ ] **Step 3: Arrivals in the graph**

In `crates/core-engine/src/graph/exec.rs`, `Graph` gains, after `max_lead: u64,`:

```rust
    /// Per strip, the frame its controls act at behind the renderer: its
    /// node's output time (plan 5e-1 Ruling 5), by strip.
    strip_arrive: Box<[(StripId, u64)]>,
    /// Per insert instance, its strip's input time plus the inserts
    /// before it, by instance.
    insert_arrive: Box<[(PluginInstanceId, u64)]>,
    /// Every distinct arrival, sorted, 0 included.
    arrivals: Box<[u64]>,
```

In `build`, before `insert_ids.sort_by_key(|x| x.0);`, compute them from `spec.nodes` and `plan` (for each `NodeSpec::Strip(s)` at `i`: `(s.strip, t_out[i])` when `s.strip` is set; walking `s.inserts` from `t_in[i]`, `(ins.instance, at)` when set, then `at += latency`; `.get()` with 0 for a missing entry), sort both by key, collect the distinct values with 0; store them in the literal. Add the three accessors after `max_lead()` (`binary_search_by_key`, 0 when absent).

- [ ] **Step 4: Read automation at the arrival**

In `crates/core-engine/src/render.rs`:

Find:
```rust
            let run = self.next_run(length, frames - offset);
```
Replace:
```rust
            // Built before the run's length: automation is read at each
            // strip's arrival through it (plan 5e-1 Ruling 5).
            let regions = self.region_run(length, monitored, offset);
            let run = self.next_run(length, frames - offset, &regions);
```

Find:
```rust
            self.automate(length, run);
            let info = self.block_info();
            let regions = self.region_run(length, monitored, offset);
```
Replace:
```rust
            self.automate(length, run, &regions);
            let info = self.block_info();
```

Then give `next_run` and `automate` their `regions: &crate::graph::RegionRun` parameter and the reads of the Interfaces block: in `next_run`, the single `s.automation.next_cut(pos)` becomes the per-lane minimum of the Interfaces paragraph (arrival 0 for every lane without a graph); in `automate`, each lane's `pos`/`mid`/`end` become `f0`/`f0 + run / 2`/`f0 + run` with `f0 = auto_frame(regions, pos, arrival(lane))`, and the plugin loop's `pos.saturating_add(u64::from(f))` becomes `f0.saturating_add(u64::from(f))`. Add the free function `auto_frame` (doc: "The frame automation is read at for a control whose audio is `arrival` frames behind the renderer (plan 5e-1 Ruling 5): where a led region with that lead reads, else that far back. // RT").

In `docs/feature-backlog.md`, delete the row that starts `| Strip automation aligned by each strip's arrival time |` (done by this task).

- [ ] **Step 5: Run the tests to verify they pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -2
cargo clippy --workspace --all-targets --target x86_64-unknown-linux-gnu --target-dir target/linux-check -- -D warnings 2>&1 | tail -2
cargo test -p core-engine 2>&1 | grep -E "test result|FAILED|panicked" | grep -v " 0 failed"; echo "engine done"
cargo clippy -p vizdaw-play --all-targets --features rt-check -- -D warnings 2>&1 | tail -2
cargo test -p core-engine --features rt-check --test rt_alloc --test rt_alloc_graph --test rt_alloc_record 2>&1 | grep "test result"
```

Expected: every `core-engine` test passes (`automation.rs`, `graph_controls.rs`, `region_leads.rs` unchanged: their strips have arrival 0, or no automation); `automation_arrival` 3 passed (Rust 1476); the rt-check gates pass.

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-engine/src/graph/exec.rs crates/core-engine/src/render.rs crates/core-engine/tests/automation_arrival.rs docs/feature-backlog.md
git commit -m "core-engine: automation is read at each strip's arrival, so led paths line up (5e-1 Task 4, 5d-1 ruling C2)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -3
```

### Task 5: Live gestures on the real-time thread: the queue and the live table

**Files:**
- Create: `crates/core-engine/src/gesture.rs` (this task's half: `GesturePhase`, `GestureMsg`, `GestureSender`, `LiveTable`)
- Modify: `crates/core-engine/src/lib.rs` (`pub mod gesture;`), `crates/core-engine/src/render.rs` (`gesture_in`, `live`, `live_gestures`, `apply_live`, the lane reads in `automate`, `end_holds` at Stop, wraps and Locate, the plugin lanes' `auto_last` reset, `Shared::gestures_dropped`), `crates/core-engine/src/engine.rs` (`gesture_sender`, `gestures_dropped`), `crates/core-engine/src/graph/exec.rs` (`Graph::static_value`), `crates/core-engine/src/graph/controls.rs` (`Controls::base_of`, `OverrideSnapshot::value`), `crates/core-engine/src/schedule/automation.rs` (`AutoDest::target`, `AutoDest::is_db`)
- Create: `crates/core-engine/tests/gestures.rs`, `crates/core-engine/tests/rt_alloc_gestures.rs`

**Interfaces:**
- Consumes: `AutoDest`, `StripParam` (4d), `Graph::{automate_strip, automate_send, release_overrides, commit_controls}`, `core_model::automation::{fader_db, fader_norm, unit}`, Task 4's arrivals.
- Produces (`core_engine::gesture`):
  ```rust
  pub const GESTURE_CAPACITY: usize = 4096;
  pub const MAX_LIVE: usize = 1024;
  pub const GESTURES_PER_BLOCK: usize = 256;
  /// What a gesture message does (spec §7.1; plan 5e-1 Rulings 6, 10, 11).
  #[derive(Debug, Clone, Copy, PartialEq)]
  pub enum GesturePhase {
      /// Grabbed, at this normalised value.
      Touch(f64),
      /// Moved to this normalised value.
      Value(f64),
      /// Moved by this much since the touch, added to the automation
      /// (Trim: dB for fader and send targets, normalised units otherwise).
      Offset(f64),
      /// Let go: the override ends at once (Read, Off, stopped).
      Release,
      /// Let go: the value glides back to the automation over this many frames (Touch, Trim).
      Glide(u32),
      /// Let go: the value is held until the transport stops or wraps (Latch, Write).
      Hold,
  }
  #[derive(Debug, Clone, Copy, PartialEq)]
  pub struct GestureMsg {
      /// What it moves.
      pub dest: AutoDest,
      /// What it does.
      pub phase: GesturePhase,
      /// When it happened, host nanoseconds; 0: at the block that takes it.
      pub host_ns: u64,
      /// The control side's tag, copied to the stamp (Task 6; its write mode).
      pub tag: u8,
  }
  pub struct GestureSender { /* Arc<Mutex<Producer<GestureMsg>>>, Arc<AtomicU64> */ }
  impl GestureSender {
      /// Queues `m`, waiting for another sender (control-side threads only; never the RT thread); false only when the queue is full (dropped and counted).
      pub fn send(&self, m: GestureMsg) -> bool;
      /// Messages dropped so far.
      pub fn dropped(&self) -> u64;
  }
  pub(crate) fn gesture_queue() -> (GestureSender, rtrb::Consumer<GestureMsg>);
  /// The renderer's touched targets (Ruling 6), sorted by dest. // RT
  pub struct LiveTable { /* Box<[Live]> of MAX_LIVE + len, sorted by dest; `released: bool`; `ended_plugins: [AutoDest; 64]` + len + overflow */ }
  impl LiveTable {
      pub fn new() -> LiveTable;
      /// Applies `phase` to `dest`; false when a touch found no free entry (dropped). // RT
      pub fn apply(&mut self, dest: AutoDest, phase: GesturePhase) -> bool;
      /// The value `dest` plays `at` frames into the run instead of `under` (its automation or static value); None when it is not live. // RT
      pub fn value(&self, dest: AutoDest, under: f64, at: u32) -> Option<f64>;
      /// `dest` is touched or held: a plugin keeps the user's value, so no automation is sent to it. // RT
      pub fn holds(&self, dest: AutoDest) -> bool;
      /// Glides move on by `frames`; finished glides end. // RT
      pub fn advance(&mut self, frames: u32);
      /// Held and gliding entries end (a Stop, a loop wrap, a Locate). // RT
      pub fn end_holds(&mut self);
      /// Whether an entry ended since the last call (its override must go). // RT
      pub fn take_released(&mut self) -> bool;
      /// Calls `f` with each `Plugin` dest whose entry ended since the last call, then forgets them (pre-flight I10); true when more ended than it kept (64), so every plugin lane must be reset. // RT
      pub fn take_ended_plugins(&mut self, f: impl FnMut(AutoDest)) -> bool;
      /// The live entries' destinations. // RT
      pub fn dests(&self) -> impl Iterator<Item = AutoDest> + '_;
  }
  ```
  `AutoDest::target(self) -> AutomationTarget` (the inverse of `AutoDest::of`) and `AutoDest::is_db(self) -> bool` (fader and send levels). `Graph::static_value(&self, dest: AutoDest) -> Option<f64>` (the mixer's value for a strip control or send, normalised: `fader_norm(db)`, `pan_norm(pan)`, 1.0/0.0 for mute and solo; `None` for plugins and unknown controls). `OverrideSnapshot::value(&self, dest: AutoDest) -> Option<f64>` (the override the controls held at their last commit for a strip control or send, in the same normalised units; `None` for plugins and controls without one; not RT, read by the session for Ruling 26). `Engine::gesture_sender(&self) -> GestureSender` (a clone); `Engine::gestures_dropped(&self) -> u64` (queue drops plus live-table drops).

Ruling 6 and Ruling 10's engine half, as amended (pre-flight I3, I4, I10) and Ruling 23's engine half. The table's states: Touch and Value set *touched* at the value; Offset sets *offset* (value = `fader_norm(fader_db(under) + d)` for `is_db` dests, else `unit(under + d)`); Release removes the entry (and marks `released`); Hold turns it *held* at its last value (an offset entry holds its offset); Glide turns it *gliding* from its last value (`under + (from − under) × (1 − done/frames)` for absolute values; an offset glides its offset to 0); a glide of 0 frames is a Release. A Value or Offset for a dest with no entry is a touch.

The table is a fixed boxed array kept sorted by `AutoDest` (`Ord`), with `apply`, `value` and `holds` finding entries by binary search; inserting or removing shifts the entries after it (`copy_within`-style on the boxed slice), never allocating. Every entry that ends (Release, a finished glide, `end_holds`) with a `Plugin` dest is also pushed to `ended_plugins` (64 slots; one more sets an overflow flag instead).

In the renderer: `live_gestures()` runs right after `live_midi()` (at most `GESTURES_PER_BLOCK` messages; a refused touch counts in `Shared`); in `automate`, every strip and send lane's value `v` becomes `live.value(dest, v, 0)` in the start commit and `live.value(dest, v, run)` in the ramped commit, and a plugin lane whose dest `live.holds(..)` is skipped (an offset or a glide sends `live.value(..)`); then `apply_live(run)` sets, for every live strip or send dest no schedule lane drives (and for every live strip or send dest while not playing), `live.value(dest, static_value(dest), …)` the same way, commits, and, when `take_released()`, calls `release_overrides(|d| schedule drives d || live has d)` and commits; then `live.advance(run)`, then `live.take_ended_plugins(|d| …)` sets `auto_last[k]` to NaN for the schedule lane `k` whose dest is `d` (binary search over `auto.lanes()`, whose order is by dest), and on `true` sets it for every plugin lane, so flat or stepped automation is sent again (pre-flight I10). `EngineCmd::Stop`, every `Step::Wrap` and `EngineCmd::Locate` call `live.end_holds()` (Ruling 23).

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-engine/tests/gestures.rs <<'VIZDAW_EOF'
//! Live gestures (plan 5e-1 Tasks 5–6; sub-project 5 spec §7.1, §7.2): a
//! touched target's value overrides its automation at once, glides back,
//! holds until a wrap, a Locate or Stop, trims in dB; a touched plugin
//! parameter gets no automation and gets it again once released; the
//! gesture tap stamps run frames, marks and says when it rolled.

use core_engine::backend::AudioBackend;
use core_engine::backend::StereoBuf;
use core_engine::engine::{Engine, EngineConfig};
use core_engine::fixtures::{Builder, Q};
use core_engine::gesture::{GESTURE_CAPACITY, GESTURES_PER_BLOCK, GestureMsg, GesturePhase, MAX_LIVE};
use core_engine::graph::{EffectNode, InsertFactory, ProcessStatus, TestInserts};
use core_engine::offline::{OfflineBackend, OfflineSinks};
use core_engine::schedule::{AutoDest, StripParam};
use core_model::automation::{AutomationTarget, fader_norm};
use core_model::document::Document;
use core_model::event::EventBody;
use core_model::ids::{PluginInstanceId, SeqId, StripId};
use core_model::mixer::{ParamId, PluginSlot, StripKind};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

const BLOCK: u32 = 512;

/// A plugin whose last automated value is kept in `.0` (`f64` bits; NaN
/// none yet) and whose automation calls are counted in `.1`.
struct Rec(Arc<AtomicU64>, Arc<AtomicU64>, Box<dyn EffectNode>);

impl EffectNode for Rec {
    fn latency(&self) -> u32 {
        self.2.latency()
    }
    fn process(&mut self, frames: u32, io: &mut StereoBuf, sc: Option<&StereoBuf>) -> ProcessStatus {
        self.2.process(frames, io, sc)
    }
    fn reset(&mut self) {
        self.2.reset();
    }
    fn automate(&mut self, _frame: u32, _param: ParamId, v: f64) -> bool {
        self.0.store(v.to_bits(), Ordering::Relaxed);
        self.1.fetch_add(1, Ordering::Relaxed);
        true
    }
}

struct RecInserts(TestInserts, Arc<AtomicU64>, Arc<AtomicU64>);

impl InsertFactory for RecInserts {
    fn make(&mut self, slot: &PluginSlot, sr: f64, max: u32) -> Result<Box<dyn EffectNode>, String> {
        Ok(Box::new(Rec(Arc::clone(&self.1), Arc::clone(&self.2), self.0.make(slot, sr, max)?)))
    }
}

struct Rig {
    engine: Engine,
    out: OfflineBackend,
    synth: StripId,
    bus: StripId,
    insert: PluginInstanceId,
    last: Arc<AtomicU64>,
    sent: Arc<AtomicU64>,
}

fn rig() -> Rig {
    rig_with(false)
}

/// The built-in synth plays a long note; its fader is automated at −12 dB
/// from 0; a bus without automation; an insert on the bus whose parameter
/// 0 ramps 0 → 1 over 16 beats, or (`flat`) stays at 0.3 (one point at 0).
fn rig_with(flat: bool) -> Rig {
    let mut b = Builder::new();
    let d = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, notes) = b.sequence("S", "Notes", Some(d)).unwrap();
    b.notes(seq, notes, &[(0, 60, 0x8000, 64 * Q)]).unwrap();
    let synth = b.strip_of(StripKind::BuiltinSynth).unwrap();
    let bus = b.bus("Bus").unwrap();
    let insert = b.insert(bus, TestInserts::plugin("gain=1")).unwrap();
    let auto = b.lane(seq, "Auto", None).unwrap();
    let fader = AutomationTarget::StripFader(synth);
    let param = AutomationTarget::PluginParam(insert, ParamId(0));
    b.event(seq, auto, 0, EventBody::Automation { target: fader, value: fader_norm(-12.0) }).unwrap();
    if flat {
        b.event(seq, auto, 0, EventBody::Automation { target: param, value: 0.3 }).unwrap();
    } else {
        b.event(seq, auto, 0, EventBody::Automation { target: param, value: 0.0 }).unwrap();
        b.event(seq, auto, 16 * Q, EventBody::Automation { target: param, value: 1.0 }).unwrap();
    }
    let doc = b.finish();
    rig_of(&doc, seq, synth, bus, insert)
}

fn rig_of(doc: &Document, seq: SeqId, synth: StripId, bus: StripId, insert: PluginInstanceId) -> Rig {
    let cfg = EngineConfig::new(48_000.0);
    let (mut engine, renderer) = Engine::new(cfg, Box::new(OfflineSinks::new(1 << 16).with_tone())).unwrap();
    let last = Arc::new(AtomicU64::new(f64::NAN.to_bits()));
    let sent = Arc::new(AtomicU64::new(0));
    engine.set_insert_factory(Box::new(RecInserts(TestInserts::new(), Arc::clone(&last), Arc::clone(&sent))));
    engine.load(doc, seq).unwrap();
    let mut out = OfflineBackend::new(48_000.0, BLOCK);
    out.start(renderer).unwrap();
    out.reserve(4096);
    Rig { engine, out, synth, bus, insert, last, sent }
}

impl Rig {
    fn blocks(&mut self, n: usize) {
        for _ in 0..n {
            self.out.block(BLOCK);
        }
    }
    fn send(&self, dest: AutoDest, phase: GesturePhase) {
        assert!(self.engine.gesture_sender().send(GestureMsg { dest, phase, host_ns: 0, tag: 0 }));
    }
    fn fader_db(&self, s: StripId) -> Option<f64> {
        self.engine.overrides()?.fader_db(s)
    }
    fn last(&self) -> f64 {
        f64::from_bits(self.last.load(Ordering::Relaxed))
    }
    fn sent(&self) -> u64 {
        self.sent.load(Ordering::Relaxed)
    }
}

fn near(a: Option<f64>, b: f64) -> bool {
    a.is_some_and(|a| (a - b).abs() < 1e-6)
}

fn fader(s: StripId) -> AutoDest {
    AutoDest::Strip(s, StripParam::Fader)
}

/// Spec §7.2: while touched the live value wins over the automation; on
/// release (Read) it snaps back.
#[test]
fn a_touched_fader_wins_over_its_automation_and_snaps_back() {
    let mut r = rig();
    r.engine.play().unwrap();
    r.blocks(4);
    assert!(near(r.fader_db(r.synth), -12.0), "{:?}", r.fader_db(r.synth));
    r.send(fader(r.synth), GesturePhase::Touch(fader_norm(-3.0)));
    r.blocks(1);
    assert!(near(r.fader_db(r.synth), -3.0), "{:?}", r.fader_db(r.synth));
    r.send(fader(r.synth), GesturePhase::Value(fader_norm(-6.0)));
    r.blocks(1);
    assert!(near(r.fader_db(r.synth), -6.0));
    r.send(fader(r.synth), GesturePhase::Release);
    r.blocks(1);
    assert!(near(r.fader_db(r.synth), -12.0));
}

/// Plan 5e-1 Ruling 10: Touch's release glides back over the return time;
/// a glide of 0 frames is a release.
#[test]
fn a_glide_returns_to_the_automation_over_its_frames() {
    let mut r = rig();
    r.engine.play().unwrap();
    r.blocks(2);
    r.send(fader(r.synth), GesturePhase::Touch(fader_norm(-3.0)));
    r.blocks(1);
    r.send(fader(r.synth), GesturePhase::Glide(8 * BLOCK));
    r.blocks(2);
    let mid = r.fader_db(r.synth).unwrap();
    assert!(mid < -3.0 - 1e-6 && mid > -12.0 + 1e-6, "{mid}");
    r.blocks(8);
    assert!(near(r.fader_db(r.synth), -12.0));
    r.send(fader(r.synth), GesturePhase::Touch(fader_norm(0.0)));
    r.send(fader(r.synth), GesturePhase::Glide(0));
    r.blocks(1);
    assert!(near(r.fader_db(r.synth), -12.0), "a 0-frame glide releases");
}

/// Ruling 10: Latch and Write hold until the next loop wrap or Stop.
#[test]
fn held_values_last_until_a_wrap_or_a_stop() {
    let mut r = rig();
    r.engine.set_loop_range(Some((0, 16 * u64::from(BLOCK)))).unwrap();
    r.engine.play().unwrap();
    r.blocks(2);
    r.send(fader(r.synth), GesturePhase::Touch(fader_norm(-3.0)));
    r.send(fader(r.synth), GesturePhase::Hold);
    r.blocks(4);
    assert!(near(r.fader_db(r.synth), -3.0), "held after the release");
    r.blocks(12);
    assert!(near(r.fader_db(r.synth), -12.0), "the wrap ended the hold");
    r.send(fader(r.synth), GesturePhase::Touch(fader_norm(-3.0)));
    r.send(fader(r.synth), GesturePhase::Hold);
    r.blocks(1);
    r.engine.stop().unwrap();
    r.blocks(2);
    r.engine.locate_frame(0).unwrap();
    r.engine.play().unwrap();
    r.blocks(2);
    assert!(near(r.fader_db(r.synth), -12.0), "the Stop ended the hold");
}

/// Spec §7.2: a gesture while stopped is heard at once; on a control no
/// automation drives, its release gives the static value back (no
/// override left).
#[test]
fn a_touch_while_stopped_is_heard_at_once() {
    let mut r = rig();
    r.blocks(2);
    assert_eq!(r.fader_db(r.bus), None);
    r.send(fader(r.bus), GesturePhase::Touch(fader_norm(-3.0)));
    r.blocks(1);
    assert!(near(r.fader_db(r.bus), -3.0));
    // The snapshot in automation's units, as the session reads it for a
    // touch with no value (Ruling 26).
    let snap = r.engine.overrides().unwrap();
    assert!(near(snap.value(fader(r.bus)), fader_norm(-3.0)), "{:?}", snap.value(fader(r.bus)));
    assert_eq!(snap.value(AutoDest::Plugin(r.insert, ParamId(0))), None);
    r.send(fader(r.bus), GesturePhase::Release);
    r.blocks(1);
    assert_eq!(r.fader_db(r.bus), None);
}

/// Ruling 11: Trim plays automation plus the offset, in dB for a fader.
#[test]
fn an_offset_trims_the_automation_in_db() {
    let mut r = rig();
    r.engine.play().unwrap();
    r.blocks(2);
    r.send(fader(r.synth), GesturePhase::Touch(fader_norm(-12.0)));
    r.send(fader(r.synth), GesturePhase::Offset(4.0));
    r.blocks(1);
    assert!(near(r.fader_db(r.synth), -8.0), "{:?}", r.fader_db(r.synth));
}

/// Ruling 6: a touched plugin parameter gets no automation (the plugin
/// keeps the user's value); released, its automation is sent again.
#[test]
fn plugin_lanes_pause_while_touched() {
    let mut r = rig();
    let dest = AutoDest::Plugin(r.insert, ParamId(0));
    r.engine.play().unwrap();
    r.blocks(8);
    let before = r.last();
    assert!(before > 0.0 && before < 1.0, "{before}");
    r.send(dest, GesturePhase::Touch(0.9));
    r.blocks(1);
    let touched = r.last();
    r.blocks(16);
    assert_eq!(r.last().to_bits(), touched.to_bits(), "no automation while touched");
    r.send(dest, GesturePhase::Release);
    r.blocks(2);
    assert!(r.last() > touched, "the ramp is sent again");
}

/// Pre-flight I10, Ruling 10: a released plugin parameter whose lane is
/// flat is sent its automation again (the renderer had not sent it while
/// it equalled the last value sent).
#[test]
fn a_released_plugin_parameter_gets_flat_automation_again() {
    let mut r = rig_with(true);
    let dest = AutoDest::Plugin(r.insert, ParamId(0));
    r.engine.play().unwrap();
    r.blocks(4);
    assert!((r.last() - 0.3).abs() < 1e-9, "{}", r.last());
    r.send(dest, GesturePhase::Touch(0.9));
    r.blocks(4);
    let during = r.sent();
    r.send(dest, GesturePhase::Release);
    r.blocks(2);
    assert!(r.sent() > during, "sent again after the release");
    assert!((r.last() - 0.3).abs() < 1e-9, "{}", r.last());
    // More plugin entries end in one block than the renderer keeps (64):
    // the real one ends last, after 64 others, and is still reset.
    for k in 0..64u64 {
        r.send(AutoDest::Plugin(r.insert, ParamId(100 + k)), GesturePhase::Touch(0.5));
    }
    r.send(dest, GesturePhase::Touch(0.9));
    r.blocks(2);
    let during = r.sent();
    for k in 0..64u64 {
        r.send(AutoDest::Plugin(r.insert, ParamId(100 + k)), GesturePhase::Release);
    }
    r.send(dest, GesturePhase::Release);
    r.blocks(2);
    assert!(r.sent() > during, "the overflow resets every plugin lane");
}

/// Ruling 6 (pre-flight I3): a sender waits for another sender instead of
/// dropping; only a full queue refuses, and that is counted.
#[test]
fn a_full_queue_refuses_and_counts() {
    let r = rig();
    let m = GestureMsg { dest: fader(r.bus), phase: GesturePhase::Value(0.5), host_ns: 0, tag: 0 };
    let (tx, tx2) = (r.engine.gesture_sender(), r.engine.gesture_sender());
    // Two threads send at once into a queue nothing drains (no block runs).
    let other = std::thread::spawn(move || (0..GESTURE_CAPACITY / 2).all(|_| tx2.send(m)));
    let mine = (0..GESTURE_CAPACITY / 2).all(|_| tx.send(m));
    assert!(other.join().unwrap() && mine, "no send is refused while the queue has room");
    assert_eq!(r.engine.gestures_dropped(), 0);
    assert!(!tx.send(m), "the queue is full");
    assert_eq!(r.engine.gestures_dropped(), 1);
}

/// Ruling 23: a Locate ends held values, as a wrap does.
#[test]
fn a_locate_ends_held_values() {
    let mut r = rig();
    r.engine.play().unwrap();
    r.blocks(2);
    r.send(fader(r.synth), GesturePhase::Touch(fader_norm(-3.0)));
    r.send(fader(r.synth), GesturePhase::Hold);
    r.blocks(2);
    assert!(near(r.fader_db(r.synth), -3.0), "held");
    r.engine.locate_frame(0).unwrap();
    r.blocks(2);
    assert!(near(r.fader_db(r.synth), -12.0), "the Locate ended the hold");
}

/// Ruling 6: at most `MAX_LIVE` live targets; the next touch is dropped
/// and counted, and the others still work.
#[test]
fn a_full_live_table_drops_and_counts() {
    let mut r = rig();
    for k in 0..=MAX_LIVE as u64 {
        r.send(AutoDest::Plugin(r.insert, ParamId(100 + k)), GesturePhase::Touch(0.5));
    }
    // 256 messages are taken per block.
    r.blocks(MAX_LIVE / GESTURES_PER_BLOCK + 2);
    assert_eq!(r.engine.gestures_dropped(), 1);
    r.send(fader(r.bus), GesturePhase::Touch(fader_norm(-3.0)));
    r.blocks(1);
    assert_eq!(r.fader_db(r.bus), None, "the table is still full");
    r.send(AutoDest::Plugin(r.insert, ParamId(100)), GesturePhase::Release);
    r.send(fader(r.bus), GesturePhase::Touch(fader_norm(-3.0)));
    r.blocks(1);
    assert!(near(r.fader_db(r.bus), -3.0), "a freed entry is reused");
}
VIZDAW_EOF
```

(`AutoDest`, `StripParam` and `TargetIdx` are re-exported from `core_engine::schedule`, as `schedule_automation.rs` imports them; `Engine::overrides()` is the 4d snapshot of the controls' last commit.)

Create `crates/core-engine/tests/rt_alloc_gestures.rs` (one test, so no other test thread allocates while the counter is compared):

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-engine/tests/rt_alloc_gestures.rs <<'VIZDAW_EOF'
//! Gestures never allocate on the render path (plan 5e-1 Tasks 5–6; spec
//! §3, §12.3 "`rt-check` covers … gesture overrides"): the counting
//! allocator is armed only around `OfflineBackend::block` while every
//! gesture phase arrives (strip, send and plugin targets, a full table, a
//! full queue, glides, holds, offsets, released plugin lanes), across loop
//! wraps, a Locate, a Stop and a graph whose latency changes (so arrivals
//! change), and (Task 6) while the gesture tap is installed, stamps and is
//! replaced and removed.

use core_engine::backend::AudioBackend;
use core_engine::engine::{Engine, EngineConfig};
use core_engine::fixtures::{Builder, Q};
use core_engine::gesture::{GESTURE_CAPACITY, GestureMsg, GesturePhase, MAX_LIVE};
use core_engine::graph::TestInserts;
use core_engine::offline::{OfflineBackend, OfflineSinks};
use core_engine::rt_check::{self, CountingAlloc};
use core_engine::schedule::{AutoDest, StripParam};
use core_model::automation::{AutomationTarget, SendIndex, fader_norm};
use core_model::command::MixerCmd;
use core_model::event::EventBody;
use core_model::ids::PluginInstanceId;
use core_model::mixer::{AuxSend, ParamId, StripKind};

#[global_allocator]
static ALLOC: CountingAlloc = CountingAlloc;

fn armed_block(b: &mut OfflineBackend, frames: u32) {
    rt_check::arm();
    b.block(frames);
    rt_check::disarm();
}

#[test]
fn gestures_never_allocate() {
    let mut bld = Builder::new();
    let synth_dest = bld.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, notes) = bld.sequence("S", "Notes", Some(synth_dest)).unwrap();
    bld.notes(seq, notes, &[(0, 60, 0x8000, 64 * Q)]).unwrap();
    let synth = bld.strip_of(StripKind::BuiltinSynth).unwrap();
    let bus = bld.bus("Bus").unwrap();
    bld.mixer(MixerCmd::AddSend { strip: synth, send: AuxSend::new(bus) }).unwrap();
    let insert = bld.insert(bus, TestInserts::plugin("latency=64")).unwrap();
    let auto = bld.lane(seq, "Auto", None).unwrap();
    for (t, target, v) in [
        (0, AutomationTarget::StripFader(synth), fader_norm(-12.0)),
        (8 * Q, AutomationTarget::StripFader(synth), fader_norm(0.0)),
        (0, AutomationTarget::SendLevel(synth, SendIndex(0)), 0.5),
        (0, AutomationTarget::PluginParam(insert, ParamId(0)), 0.0),
        (8 * Q, AutomationTarget::PluginParam(insert, ParamId(0)), 1.0),
        (Q, AutomationTarget::StripMute(bus), 0.0),
    ] {
        bld.event(seq, auto, t, EventBody::Automation { target, value: v }).unwrap();
    }
    let doc = bld.finish();
    let mut lb = Builder::from_doc(doc.clone());
    let master = lb.strip_of(StripKind::Master).unwrap();
    lb.insert(master, TestInserts::plugin("latency=480")).unwrap();
    let latent = lb.finish();
    let (mut engine, renderer) =
        Engine::new(EngineConfig::new(48_000.0), Box::new(OfflineSinks::new(1 << 20).with_tone())).unwrap();
    engine.set_insert_factory(Box::new(TestInserts::new()));
    engine.load(&doc, seq).unwrap();
    let tx = engine.gesture_sender();
    let mut b = OfflineBackend::new(48_000.0, 512);
    b.start(renderer).unwrap();
    b.reserve(1 << 16);
    let send = |dest: AutoDest, phase: GesturePhase| {
        let _ = tx.send(GestureMsg { dest, phase, host_ns: 0, tag: 1 });
    };
    let dests = [
        AutoDest::Strip(synth, StripParam::Fader),
        AutoDest::Strip(bus, StripParam::Pan),
        AutoDest::Strip(bus, StripParam::Mute),
        AutoDest::Send(synth, 0),
        AutoDest::Plugin(insert, ParamId(0)),
    ];
    // Warm up outside the count.
    engine.set_loop_range(Some((0, 4 * 24_000))).unwrap();
    engine.play().unwrap();
    for _ in 0..8 {
        b.block(512);
    }
    let before = rt_check::count();
    for round in 0..6u32 {
        if round == 1 {
            // A full queue (pre-flight M11): sent outside the count; what
            // does not fit is dropped. Drained 256 a block.
            for _ in 0..GESTURE_CAPACITY + 16 {
                send(dests[0], GesturePhase::Value(0.5));
            }
            for _ in 0..20 {
                armed_block(&mut b, 512);
            }
        }
        if round == 3 {
            engine.locate_frame(0).unwrap();
        }
        for (k, d) in dests.iter().enumerate() {
            let v = f64::from(round * 7 + k as u32 % 10) / 70.0;
            send(*d, GesturePhase::Touch(v));
            send(*d, GesturePhase::Value(v / 2.0));
            send(*d, GesturePhase::Offset(1.5));
            send(
                *d,
                match round % 4 {
                    0 => GesturePhase::Release,
                    1 => GesturePhase::Glide(3000),
                    2 => GesturePhase::Hold,
                    _ => GesturePhase::Glide(0),
                },
            );
            armed_block(&mut b, 512);
        }
        // A full table.
        for k in 0..(MAX_LIVE as u64 + 4) {
            send(AutoDest::Plugin(PluginInstanceId(900), ParamId(k)), GesturePhase::Touch(0.5));
        }
        for _ in 0..40 {
            armed_block(&mut b, 512);
        }
        for k in 0..(MAX_LIVE as u64 + 4) {
            send(AutoDest::Plugin(PluginInstanceId(900), ParamId(k)), GesturePhase::Release);
        }
        if round == 2 {
            engine.load(&latent, seq).unwrap();
        }
        if round == 4 {
            engine.load(&doc, seq).unwrap();
            engine.stop().unwrap();
            armed_block(&mut b, 512);
            send(dests[0], GesturePhase::Touch(0.3));
            armed_block(&mut b, 512);
            engine.play().unwrap();
        }
        for _ in 0..30 {
            armed_block(&mut b, 512);
        }
    }
    assert_eq!(rt_check::count(), before, "the render path allocated");
}
VIZDAW_EOF
```

The engine's `load` runs between armed blocks, as `rt_alloc_record.rs` loads its latent document: only `OfflineBackend::block` is armed.

- [ ] **Step 2: Run the tests to verify they fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-engine --test gestures 2>&1 | tail -3
```

Expected: FAIL to compile (`core_engine::gesture` does not exist).

- [ ] **Step 3: The queue and the table**

Create `crates/core-engine/src/gesture.rs` with the module doc "Live gestures (sub-project 5 spec §7.1, §7.2; plan 5e-1 Rulings 6, 10, 11): the control side's messages, the renderer's table of touched targets and (Task 6) the gesture tap. // RT (the table and the tap)" and the items of the Interfaces block. `GestureSender` is shaped like `input::MidiSender` (an `Arc<Mutex<Producer>>` and a drop counter) but takes the producer with `lock().unwrap_or_else(|e| e.into_inner())`, never `try_lock` (pre-flight I3), with the doc "Control-side threads only; never the RT thread."; only a full queue drops and counts. `LiveTable` keeps a boxed slice of `MAX_LIVE` entries (filled with placeholders in `new`, off the RT thread) and a length, the first `len` sorted by `dest`, with `Live { dest, state }` and `enum LiveState { Touched(f64), Offset(f64), Held(f64), HeldOffset(f64), Gliding { from: f64, done: u32, frames: u32 }, GlidingOffset { from: f64, done: u32, frames: u32 } }`; lookups by `binary_search_by_key` over the first `len` entries (pre-flight I4). (`AutoDest` is `Ord`: `Automation::drives` already searches by it.) `#[derive(Debug)]` on all. Right after `MAX_LIVE`, pin its floor at compile time (pre-flight check m4; a test assertion on a constant would trip `clippy::assertions_on_constants`): `const _: () = assert!(MAX_LIVE >= 1024, "Ruling 6: every control of 16 Write strips fits");`.

In `crates/core-engine/src/schedule/automation.rs`, add `AutoDest::target` and `AutoDest::is_db` (`Strip(_, Fader)` and `Send(..)`).

In `crates/core-engine/src/graph/exec.rs`, add `Graph::static_value` reading the controls' static values (`Controls` gains `pub(crate) fn base_of(&self, dest: AutoDest) -> Option<f64>` over its `CtlStrip.base` and `CtlFeed.send`), `// RT`. In `crates/core-engine/src/graph/controls.rs`, add `OverrideSnapshot::value` (doc: "The override `dest` held at the last commit, normalised as automation is (plan 5e-1 Ruling 26). Not RT.") reading `SnapStrip.over` (fader dB → `fader_norm`, pan → `pan_norm`, mute and solo 0/1) and the strip's `SnapFeed.over_db` for a send, found as `base_of` finds the control.

- [ ] **Step 4: The renderer and the engine**

In `crates/core-engine/src/render.rs`:

Find:
```rust
    /// MIDI from sources (spec §6.6).
    midi_in: Consumer<MidiIn>,
```
Replace:
```rust
    /// MIDI from sources (spec §6.6).
    midi_in: Consumer<MidiIn>,
    /// Gestures from the control side (spec §7.1; plan 5e-1 Ruling 6).
    gesture_in: Consumer<crate::gesture::GestureMsg>,
    /// The touched targets (boxed: the renderer moves between threads by
    /// value).
    live: Box<crate::gesture::LiveTable>,
```
`Renderer::new` takes `gesture_in: Consumer<crate::gesture::GestureMsg>` after `midi_in` and fills both fields (`live: Box::new(LiveTable::new())`); `Engine::with_platform` makes the queue next to `midi_queue()` (`let (gesture_tx, gesture_rx) = crate::gesture::gesture_queue();`), passes `gesture_rx` after `midi_rx` and keeps `gesture_tx` in a new `Engine` field beside `midi_tx`.

After `        self.live_midi();` in `process_inner`, add `        self.live_gestures();`. Write `live_gestures` (doc: "Takes the queued gestures into the live table (plan 5e-1 Ruling 6). // RT"), the lane substitutions in `automate` and `apply_live` (called right after `self.automate(length, run, &regions);`) as the paragraph above the steps says, `self.live.end_holds()` in the `EngineCmd::Stop` arm, the `EngineCmd::Locate` arm and the `Step::Wrap` arm (Ruling 23), and `Shared`'s `gestures_dropped: AtomicU64` (zero in `Default`) with `pub(crate) fn count_gesture_drop(&self)` and `pub fn gestures_dropped(&self) -> u64`. `Engine::gesture_sender` and `Engine::gestures_dropped` are as in the Interfaces block.

- [ ] **Step 5: Run the tests to verify they pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -2
cargo clippy --workspace --all-targets --target x86_64-unknown-linux-gnu --target-dir target/linux-check -- -D warnings 2>&1 | tail -2
cargo test -p core-engine 2>&1 | grep -E "test result|FAILED|panicked" | grep -v " 0 failed"; echo "engine done"
cargo clippy -p vizdaw-play --all-targets --features rt-check -- -D warnings 2>&1 | tail -2
cargo test -p core-engine --features rt-check --test rt_alloc --test rt_alloc_graph --test rt_alloc_record --test rt_alloc_gestures 2>&1 | grep "test result"
```

Expected: every `core-engine` test passes; `gestures` 10 passed (Rust 1487 with `rt_alloc_gestures`); the four rt-check tests pass. `a_full_queue_refuses_and_counts` is the RED for the blocking `lock()`: with `try_lock` two senders at once drop messages (run it a few times if a first run happens to pass).

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-engine/src/gesture.rs crates/core-engine/src/lib.rs crates/core-engine/src/render.rs crates/core-engine/src/engine.rs \
  crates/core-engine/src/graph/exec.rs crates/core-engine/src/graph/controls.rs crates/core-engine/src/schedule/automation.rs \
  crates/core-engine/tests/gestures.rs crates/core-engine/tests/rt_alloc_gestures.rs
git commit -m "core-engine: live gestures override automation: touch, glide, hold, trim offsets (5e-1 Task 5)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -3
```

### Task 6: The gesture tap: run frames and marks, shared with the record tap

**Files:**
- Modify: `crates/core-engine/src/record.rs` (`RunClock` extracted; `RecordTap` uses it, behaviour unchanged), `crates/core-engine/src/gesture.rs` (`Stamped`, `GestureStatus`, `GestureHandles`, `GestureTap`, `gesture_tap`), `crates/core-engine/src/render.rs` (`EngineCmd::Gestures`, `Retired::Gestures`, the tap's hooks, stamping in `live_gestures`), `crates/core-engine/src/engine.rs` (`start_gestures`, `stop_gestures`)
- Test: `crates/core-engine/tests/gestures.rs`, `crates/core-engine/tests/rt_alloc_gestures.rs`

**Interfaces:**
- Consumes: `record::{Mark, MarkReader, MARK_CAPACITY}` (5c), Task 5's `GestureMsg`, `live_gestures`.
- Produces:
  ```rust
  // core_engine::record
  /// A tap's run frames and marks (plan 5e-1 Ruling 7): the record tap's
  /// clock, shared by the gesture tap. // RT
  pub(crate) struct RunClock { /* the RecordTap fields rolling, run, remark, stopped, stop_at, resumed_at, last_mark, latency and the marks producer */ }
  // core_engine::gesture
  pub const STAMP_CAPACITY: usize = 16_384;
  /// A gesture as the renderer took it (Ruling 7).
  #[derive(Debug, Clone, Copy, PartialEq)]
  pub struct Stamped {
      pub dest: AutoDest,
      /// The output run frame it was heard at (graph latency not yet taken off), as `RecordedMidi.run`.
      pub run: i64,
      pub phase: GesturePhase,
      pub tag: u8,
  }
  pub struct GestureStatus { /* run, rolled, closed, lost_marks, lost_stamps: atomics */ }
  impl GestureStatus {
      pub fn run(&self) -> u64;
      /// The tap has pushed its roll mark (set with Release ordering when it does; pre-flight C1, Ruling 24).
      pub fn rolled(&self) -> bool;
      pub fn closed(&self) -> bool;
      pub fn lost_marks(&self) -> u64;
      pub fn lost_stamps(&self) -> u64;
  }
  pub struct GestureHandles { pub marks: MarkReader, pub stamps: rtrb::Consumer<Stamped>, pub status: Arc<GestureStatus> }
  pub struct GestureTap { /* RunClock, Producer<Stamped>, Arc<GestureStatus> */ }
  /// A tap and its control side, its marks carrying graph latency `latency`. Allocates; call off the RT thread.
  pub fn gesture_tap(latency: u64) -> (GestureTap, GestureHandles);
  impl GestureTap { // all // RT
      pub fn begin_block(&mut self, frame: u64);
      pub fn stop(&mut self, offset: u32, frame: u64);
      pub fn wrap(&mut self, offset: u32, to: u64);
      pub fn resync(&mut self);
      pub fn set_latency(&mut self, latency: u64);
      pub fn end_block(&mut self, frames: u32, advanced: bool);
      /// Stamps a message heard at block offset `offset` (negative: before the block), by `RecordTap::midi`'s rule.
      pub fn stamp(&mut self, dest: AutoDest, offset: i64, phase: GesturePhase, tag: u8);
      pub fn close(&self);
  }
  // Dropping a GestureTap (removed, or with its renderer when the engine goes) makes its status read closed, as RecordTap's Drop does.
  impl Drop for GestureTap { fn drop(&mut self); }
  // core_engine::engine
  impl Engine {
      /// Installs a gesture tap (replacing one installed): from the next block that plays, gestures are stamped and the transport marked. Returns the control side.
      pub fn start_gestures(&mut self) -> Result<GestureHandles, EngineError>;
      /// Removes the gesture tap; its status reads closed.
      pub fn stop_gestures(&mut self) -> Result<(), EngineError>;
  }
  ```

Ruling 7. `RunClock` holds exactly what `RecordTap` uses for its run and marks, with methods `begin_block(frame) -> Begun` (`Rolled`, `Resumed`, `Going`; the record tap stores the input delay on the first two), `stop(offset, frame)`, `wrap(offset, to)`, `resync()`, `set_latency(latency)`, `stamp_run(offset: i64) -> Option<i64>` (the run of a message at `offset`, `None` where `RecordTap::midi` drops it) and `end_block(frames, advanced) -> Option<(u64, u32)>` (the run the block's counted frames start at and how many count, advancing the run); each push that does not fit returns false and the tap counts it in its status. `RecordTap` keeps every public method and its observable behaviour. In the renderer, every place the record tap's hook is called (`begin_block` at the block's start, `stop` at `EngineCmd::Stop` and at `Step::End`, `wrap` at `Step::Wrap`, `resync` at `SetSchedule` and `Locate`, `set_latency` at `SetGraph`, `end_block` in `end_tap`) calls the gesture tap's hook the same way; `EngineCmd::Gestures(Option<Box<GestureTap>>)` installs one (with `set_latency(self.latency)`) and retires the old through `Retired::Gestures` (deferred like `Record` when the retire ring is full); `live_gestures` stamps every message it takes with `offset` computed from `host_ns` exactly as `live_midi` computes MIDI's. When `begin_block` answers `Rolled` the tap pushes its roll mark and sets `status.rolled` (Release; read with Acquire), so the session can tell "not rolled yet" from "stopped" (Ruling 24). The module doc of `gesture.rs` gains one line: "The status says when the tap has rolled: until then the transport still reads stopped (plan 5e-1 Ruling 24).".

- [ ] **Step 1: Write the failing tests**

Append to `crates/core-engine/tests/gestures.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat >> crates/core-engine/tests/gestures.rs <<'VIZDAW_EOF'

fn drain(h: &mut core_engine::gesture::GestureHandles) -> (Vec<core_engine::record::Mark>, Vec<core_engine::gesture::Stamped>) {
    let mut marks = Vec::new();
    while let Ok(m) = h.marks.pop() {
        marks.push(m);
    }
    let mut stamps = Vec::new();
    while let Ok(s) = h.stamps.pop() {
        stamps.push(s);
    }
    (marks, stamps)
}

/// Ruling 7: a gesture with no host time is stamped at the run frame of
/// the block that took it; the roll, a wrap and the Stop are marked as
/// the record tap marks them; nothing is stamped while stopped; the
/// status says when the tap has rolled (Ruling 24).
#[test]
fn stamps_carry_the_run_frame_of_the_block_that_took_them() {
    let mut r = rig();
    let mut h = r.engine.start_gestures().unwrap();
    let d = fader(r.synth);
    r.send(d, GesturePhase::Touch(0.5));
    r.blocks(1);
    assert!(drain(&mut h).1.is_empty(), "stopped: nothing is stamped");
    assert!(!h.status.rolled(), "not rolled while stopped");
    let lp = 16 * u64::from(BLOCK);
    r.engine.set_loop_range(Some((0, lp))).unwrap();
    r.engine.play().unwrap();
    r.blocks(10);
    assert!(h.status.rolled(), "rolled once a block played");
    r.send(d, GesturePhase::Value(0.6));
    r.blocks(1);
    r.send(d, GesturePhase::Glide(100));
    r.blocks(10);
    r.engine.stop().unwrap();
    r.blocks(1);
    r.send(d, GesturePhase::Touch(0.1));
    r.blocks(1);
    let (marks, stamps) = drain(&mut h);
    // The Play is applied at the block after the call, so the roll is the
    // first block that played: run 0 plays frame 0.
    assert_eq!((marks[0].run, marks[0].frame, marks[0].wrap), (0, 0, false));
    assert!(marks.iter().any(|m| m.wrap && m.run == lp && m.frame == 0), "{marks:?}");
    assert!(!marks.last().unwrap().wrap, "the Stop is marked last");
    let runs: Vec<(i64, GesturePhase)> = stamps.iter().map(|s| (s.run, s.phase)).collect();
    assert_eq!(
        runs,
        vec![
            (10 * i64::from(BLOCK), GesturePhase::Value(0.6)),
            (11 * i64::from(BLOCK), GesturePhase::Glide(100)),
        ]
    );
    assert!(stamps.iter().all(|s| s.dest == d && s.tag == 0));
    assert_eq!(h.status.run(), 21 * u64::from(BLOCK));
    r.engine.stop_gestures().unwrap();
    r.blocks(1);
    assert!(h.status.closed());
}
VIZDAW_EOF
```

In `crates/core-engine/tests/rt_alloc_gestures.rs`, Find:
```rust
    let before = rt_check::count();
```
Replace:
```rust
    let mut handles = engine.start_gestures().unwrap();
    for _ in 0..2 {
        b.block(512);
    }
    let before = rt_check::count();
```
and at the end of `round == 4`'s block (after `engine.play().unwrap();`) add a tap replaced while playing: `handles = engine.start_gestures().unwrap();`; after the loop, before the assertion, add `engine.stop_gestures().unwrap(); armed_block(&mut b, 512); while handles.stamps.pop().is_ok() {}` (draining is the control side's, outside the count). Update the file's doc line to say the tap is exercised.

- [ ] **Step 2: Run the tests to verify they fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-engine --test gestures 2>&1 | tail -3
```

Expected: FAIL to compile (`start_gestures` does not exist).

- [ ] **Step 3: Extract the run clock, add the gesture tap**

Do what the paragraph above Step 1 says. Then confirm the record tap is unchanged:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-engine --test record_tap --test live_input --test count_in 2>&1 | grep -E "test result|FAILED"
cargo test -p core-ffi --test record --test passes --test synth 2>&1 | grep -E "test result|FAILED"
```

Expected: all pass, unchanged.

- [ ] **Step 4: Run the tests to verify they pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -2
cargo clippy --workspace --all-targets --target x86_64-unknown-linux-gnu --target-dir target/linux-check -- -D warnings 2>&1 | tail -2
cargo test --workspace --no-fail-fast 2>&1 | grep -E "test result|FAILED|panicked" | grep -v " 0 failed"; echo "workspace done"
cargo clippy -p vizdaw-play --all-targets --features rt-check -- -D warnings 2>&1 | tail -2
cargo test -p core-engine --features rt-check --test rt_alloc --test rt_alloc_graph --test rt_alloc_record --test rt_alloc_gestures 2>&1 | grep "test result"
```

Expected: "workspace done" with no failure lines (Rust 1488); the rt-check gates pass.

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-engine/src/record.rs crates/core-engine/src/gesture.rs crates/core-engine/src/render.rs crates/core-engine/src/engine.rs \
  crates/core-engine/tests/gestures.rs crates/core-engine/tests/rt_alloc_gestures.rs
git commit -m "core-engine: the gesture tap stamps run frames and marks the transport with the record tap's run clock (5e-1 Task 6)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -3
```

### Task 7: The writer: modes, thinning, Trim and edges, golden-tested

**Files:**
- Create: `crates/core-ffi/src/autowrite.rs`, `crates/core-ffi/tests/autowrite.rs`
- Modify: `crates/core-ffi/src/lib.rs` (`pub mod autowrite;`), `crates/core-ffi/src/passes.rs` (`Piece`, its `pass`, `pieces`, `piece_of`, `place` become `pub(crate)`, plus `pub(crate) fn pass_window(pieces: &[Piece], pass: usize) -> Option<(i64, i64)>`)

**Interfaces:**
- Consumes: Task 6's `Stamped`, `GesturePhase`, `record::Mark`; `passes::{pieces, piece_of, place}` (5c); Task 3's `AutomationWrite`; `TempoTable::frame_to_tick`; `core_model::automation::{fader_db, fader_norm, unit, AutomationMode, AutomationTarget}`.
- Produces (`vizdaw_ffi::autowrite`, pure: no I/O, no locks, no engine):
  ```rust
  /// The writing mode a gesture was made in, carried as the stamp's tag (0: not writing).
  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  pub enum WriteMode { Touch, Latch, Write, Trim }
  impl WriteMode {
      pub fn tag(self) -> u8;                         // 1, 2, 3, 4
      pub fn of_tag(tag: u8) -> Option<WriteMode>;     // 0 and others: None
      pub fn of(mode: AutomationMode) -> Option<WriteMode>; // Off, Read: None
  }
  #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
  pub enum Thinning { Low, #[default] Medium, High }
  impl Thinning {
      /// Ruling 12: 0.04/0.1/0.2 dB for fader and send targets, 0.001/0.0025/0.005 otherwise.
      pub fn tolerance(self, target: AutomationTarget) -> f64;
  }
  /// What one pass captured, and what the writer reads of the document.
  pub struct AutoCapture<'a> {
      pub marks: &'a [Mark],
      /// The gesture tap's run when the pass ended (or now, for a preview).
      pub end_run: u64,
      pub stamps: &'a [Stamped],
      /// Write strips' controls held from the roll, with the value each had then (Ruling 9).
      pub holds: &'a [(AutomationTarget, f64)],
      pub thinning: Thinning,
      /// The After Write mode as a writing mode (None for Off and Read): Write-tagged stamps in a pass after the first are written in it, or not at all (pre-flight M3).
      pub after_write: Option<WriteMode>,
      pub stepped: &'a dyn Fn(AutomationTarget) -> bool,
      /// The target's existing automation at a schedule frame (None: it has none).
      pub existing: &'a dyn Fn(AutomationTarget, u64) -> Option<f64>,
      /// The frames of its existing points in `[from, to]`.
      pub existing_points: &'a dyn Fn(AutomationTarget, u64, u64) -> Vec<u64>,
      /// Its static value, normalised.
      pub fixed: &'a dyn Fn(AutomationTarget) -> f64,
  }
  /// One write of one target in one pass: thinned points in schedule frames, no edges yet.
  #[derive(Debug, Clone, PartialEq)]
  pub struct TargetWrite { pub target: AutomationTarget, pub pass: usize, pub points: Vec<(u64, f64)> }
  /// Every write the capture makes, by pass, then the piece it starts in, then first frame (Rulings 9–12, 23): the order they were made in, so a later one wins where they overlap.
  pub fn writes(c: &AutoCapture<'_>) -> Vec<TargetWrite>;
  /// Ramer–Douglas–Peucker on vertical distance (Ruling 12): in dB when `db` (normalised fader values), keeping only changes when `stepped`; the first and last samples always stay; on equal maximum distance the earliest index splits; iterative, with an explicit stack (pre-flight M7).
  pub fn thin(samples: &[(u64, f64)], tolerance: f64, db: bool, stepped: bool) -> Vec<(u64, f64)>;
  /// The model's writes (Ruling 13): ticks through `tempo` (equal ticks keep the last value), the range one tick beyond the first and last, an edge point at each holding `outside(target, tick)`; a write longer than `MAX_WRITE_POINTS` is split into consecutive writes (Ruling 25).
  pub fn to_model(
      writes: &[TargetWrite],
      seq: SeqId,
      tempo: &TempoTable,
      outside: &dyn Fn(AutomationTarget, Tick) -> f64,
      name: &dyn Fn(AutomationTarget) -> String,
  ) -> Vec<AutomationWrite>;
  ```

**The algorithm `writes` follows** (the signature and the tests leave these choices):
1. `pieces(marks, end_run)`; none → no writes. Each stamp whose tag is a `WriteMode` is placed like recorded MIDI: `piece_of(pieces, run)` gives its pass and piece, `place` its frame (a stamp no piece covers is dropped). A stamp tagged Write in a pass after the first takes the mode `after_write`, and is dropped when that is `None` (pre-flight M3). Stamps are taken per target in their order.
2. A target's events make *segments* `{ pass, mode, offsets: bool, samples: Vec<(frame, value)>, held: bool }`:
   - `Touch(v)` opens one at its frame with `v` (a touch while a segment of the same pass is held continues it); `Value(v)` appends (opening one first when none is open: Ruling 20); `Offset(d)` appends `d` to an *offset* segment (opened with `(frame, 0.0)` when none is open; a segment whose first non-touch stamp is an Offset becomes an offset segment, its touch sample 0.0).
   - A sample at the same frame as the segment's last one replaces it.
   - `Release` closes the segment at its frame (a sample there with its last value). `Glide(n)` closes it the same way and adds a last sample at `frame + n` (clamped to the pass's end): the existing value there (else the static value) for an absolute segment, 0.0 for an offset segment. `Hold` marks it held: it then ends at the pass's end with its last value.
   - A stamp of a later pass than the open or held segment first ends that segment at its pass's end with its last value and, if it was not released, opens one at the new pass's start with that value (Ruling 10).
   - **A jump inside a pass** (Ruling 23): the pieces of a pass are walked in order, and a non-wrap piece whose start frame (`place(p, p.run)`) is more than one frame away from where the piece before it had got to (`place(prev, prev.end)`) is a jump. At a jump every open or held segment of every target ends at the earlier piece's end with its last value; a segment that was not released (its target still touched) reopens at the new piece's start with that value, in the same pass and mode. A held (released) one does not reopen, as at a wrap. So no segment's frames go back or skip.
   - At the end, every open or held segment ends at its pass's end (its last piece's end) with its last value.
3. Holds (Ruling 9): for each `(target, h)`, pass 0's segments of that target with mode Write merge into one from pass 0's start: `(start, h)`, then before each Write segment `(its first frame − 1, the value so far)` and its samples, then `(pass 0's end, the last value)`.
4. Offset segments become absolute: their samples are thinned in their own unit (dB offsets for `is_db` targets with the dB tolerance, `db = false`), then valued at the union of their frames and `existing_points(target, first, last)` as `fader_norm(fader_db(e) + d)` (fader and send targets) or `unit(e + d)` (others), `e` the existing value there and `d` the offset interpolated linearly.
5. Absolute segments are thinned with `thinning.tolerance(target)` (`db` for fader and send targets; `stepped(target)` keeps changes).

Pass windows come from `pass_window`: from the frame where the pass's first piece starts being heard (`place(p, p.from)`) to where its last piece ends (`place(p, p.end)`).

**`to_model`'s split** (Ruling 25): when a write's points with its two edges would exceed `MAX_WRITE_POINTS`, its points (after the tick conversion) are cut into chunks of at most `MAX_WRITE_POINTS − 2`, in order, each its own `AutomationWrite` with the same `seq`, `target` and `lane_name`. The first chunk's range starts at the write's `start` (first tick − 1, with the edge point there); every chunk but the last ends one tick before the next chunk's first tick; every chunk but the first starts at its own first tick; the last ends at the write's `end` (last tick + 1, with the edge point there). So the ranges are contiguous and disjoint (no existing point between two chunks survives inside the written stretch; pre-flight check m2, amending the addendum's "own first and last ticks"), and only the outer ends carry edges.

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-ffi/tests/autowrite.rs <<'VIZDAW_EOF'
//! The automation writer's golden tests (plan 5e-1 Task 7; sub-project 5
//! spec §12.3 "Automation: golden tests per mode from scripted gesture
//! sequences (touch, values and release at known frames)"): Touch's glide,
//! Latch until Stop and at a wrap, Write's held controls and After Write,
//! Trim on a ramp, stepped targets, thinning bounds, edge points, latency,
//! a jump inside a pass, the split at the model's limit.

use core_engine::gesture::{GesturePhase, Stamped};
use core_engine::record::Mark;
use core_engine::schedule::{AutoDest, StripParam, TempoTable};
use core_model::automation::{AutomationTarget, fader_db, fader_norm};
use core_model::command::{Command, EventCmd, RecordPass, SequenceCmd, apply};
use core_model::document::Document;
use core_model::event::{EventBody, EventSpec};
use core_model::ids::{LaneId, SeqId, StripId};
use core_model::time::{PPQ, Tick};
use vizdaw_ffi::autowrite::{AutoCapture, TargetWrite, Thinning, WriteMode, thin, to_model, writes};

const S: StripId = StripId(1);
const PAN: AutomationTarget = AutomationTarget::StripPan(S);
const FADER: AutomationTarget = AutomationTarget::StripFader(S);
const MUTE: AutomationTarget = AutomationTarget::StripMute(S);

fn roll(frame: u64, latency: u64) -> Mark {
    Mark { run: 0, frame, wrap: false, latency }
}

fn wrap(run: u64, to: u64) -> Mark {
    Mark { run, frame: to, wrap: true, latency: 0 }
}

fn dest(t: AutomationTarget) -> AutoDest {
    match t {
        AutomationTarget::StripPan(s) => AutoDest::Strip(s, StripParam::Pan),
        AutomationTarget::StripFader(s) => AutoDest::Strip(s, StripParam::Fader),
        AutomationTarget::StripMute(s) => AutoDest::Strip(s, StripParam::Mute),
        _ => unreachable!(),
    }
}

fn st(t: AutomationTarget, run: i64, phase: GesturePhase, mode: WriteMode) -> Stamped {
    Stamped { dest: dest(t), run, phase, tag: mode.tag() }
}

/// Runs the writer with existing automation `existing` (constant 0.5 by
/// default), points `points`, static value 0.3, Medium thinning, After
/// Write Latch.
fn run(
    marks: &[Mark],
    end_run: u64,
    stamps: &[Stamped],
    holds: &[(AutomationTarget, f64)],
    existing: &dyn Fn(AutomationTarget, u64) -> Option<f64>,
    points: &dyn Fn(AutomationTarget, u64, u64) -> Vec<u64>,
) -> Vec<TargetWrite> {
    run_after(Some(WriteMode::Latch), marks, end_run, stamps, holds, existing, points)
}

/// [`run`] with After Write `after_write`.
fn run_after(
    after_write: Option<WriteMode>,
    marks: &[Mark],
    end_run: u64,
    stamps: &[Stamped],
    holds: &[(AutomationTarget, f64)],
    existing: &dyn Fn(AutomationTarget, u64) -> Option<f64>,
    points: &dyn Fn(AutomationTarget, u64, u64) -> Vec<u64>,
) -> Vec<TargetWrite> {
    writes(&AutoCapture {
        marks,
        end_run,
        stamps,
        holds,
        thinning: Thinning::Medium,
        after_write,
        stepped: &|t| t.is_stepped(),
        existing,
        existing_points: points,
        fixed: &|_| 0.3,
    })
}

fn half(_: AutomationTarget, _: u64) -> Option<f64> {
    Some(0.5)
}

fn no_points(_: AutomationTarget, _: u64, _: u64) -> Vec<u64> {
    Vec::new()
}

fn close(got: &[(u64, f64)], want: &[(u64, f64)]) {
    assert_eq!(got.len(), want.len(), "{got:?} vs {want:?}");
    for (g, w) in got.iter().zip(want) {
        assert!(g.0 == w.0 && (g.1 - w.1).abs() < 1e-9, "{got:?} vs {want:?}");
    }
}

/// Spec §7.3 Touch: from touch to release, then a straight ramp back to
/// the existing automation over the return time (12 000 frames = 250 ms).
#[test]
fn touch_writes_from_touch_to_release_and_glides_back() {
    let mut stamps = vec![st(PAN, 4800, GesturePhase::Touch(0.8), WriteMode::Touch)];
    for k in 1..=10 {
        let r = 4800 + 480 * k;
        stamps.push(st(PAN, r, GesturePhase::Value(0.8 + 0.01 * k as f64), WriteMode::Touch));
    }
    stamps.push(st(PAN, 9600, GesturePhase::Glide(12_000), WriteMode::Touch));
    let w = run(&[roll(48_000, 0)], 96_000, &stamps, &[], &half, &no_points);
    assert_eq!(w.len(), 1);
    assert_eq!((w[0].target, w[0].pass), (PAN, 0));
    close(&w[0].points, &[(52_800, 0.8), (57_600, 0.9), (69_600, 0.5)]);
}

/// Latch: from touch to the pass's end holding the last value; a loop
/// wrap ends it and the next pass writes nothing.
#[test]
fn latch_holds_to_the_end_of_its_pass() {
    let stamps = [
        st(PAN, 48_000, GesturePhase::Touch(0.7), WriteMode::Latch),
        st(PAN, 50_000, GesturePhase::Value(0.75), WriteMode::Latch),
        st(PAN, 60_000, GesturePhase::Hold, WriteMode::Latch),
    ];
    let w = run(&[roll(0, 0), wrap(96_000, 0)], 150_000, &stamps, &[], &half, &no_points);
    assert_eq!(w.len(), 1, "{w:?}");
    close(&w[0].points, &[(48_000, 0.7), (50_000, 0.75), (96_000, 0.75)]);
    // Without the wrap it lasts until Stop.
    let w = run(&[roll(0, 0)], 150_000, &stamps, &[], &half, &no_points);
    close(&w[0].points, &[(48_000, 0.7), (50_000, 0.75), (150_000, 0.75)]);
}

/// Review Focus 2, Ruling 10: a target still touched at a wrap writes to
/// the end of that pass and again from the next pass's start.
#[test]
fn a_target_held_over_a_wrap_writes_both_passes() {
    let stamps = [
        st(PAN, 80_000, GesturePhase::Touch(0.6), WriteMode::Latch),
        st(PAN, 100_000, GesturePhase::Value(0.65), WriteMode::Latch),
        st(PAN, 110_000, GesturePhase::Hold, WriteMode::Latch),
    ];
    let w = run(&[roll(0, 0), wrap(96_000, 0)], 150_000, &stamps, &[], &half, &no_points);
    assert_eq!(w.iter().map(|x| x.pass).collect::<Vec<_>>(), vec![0, 1]);
    close(&w[0].points, &[(80_000, 0.6), (96_000, 0.6)]);
    close(&w[1].points, &[(0, 0.6), (4_000, 0.65), (54_000, 0.65)]);
}

/// Write (Ruling 9): a held control writes from the roll at its value,
/// a touch changes it from then on, held to the pass's end; after a wrap
/// (After Write) an untouched control writes nothing.
#[test]
fn write_writes_held_controls_from_the_roll() {
    let stamps = [
        st(PAN, 24_000, GesturePhase::Touch(0.9), WriteMode::Write),
        st(PAN, 30_000, GesturePhase::Hold, WriteMode::Write),
    ];
    let holds = [(PAN, 0.6)];
    let w = run(&[roll(0, 0)], 96_000, &stamps, &holds, &half, &no_points);
    assert_eq!(w.len(), 1, "{w:?}");
    close(&w[0].points, &[(0, 0.6), (23_999, 0.6), (24_000, 0.9), (96_000, 0.9)]);
    let w = run(&[roll(0, 0), wrap(48_000, 0)], 96_000, &stamps, &holds, &half, &no_points);
    assert_eq!(w.len(), 1, "only the first pass: {w:?}");
    close(&w[0].points, &[(0, 0.6), (23_999, 0.6), (24_000, 0.9), (48_000, 0.9)]);
    // Untouched: the held value alone.
    let w = run(&[roll(0, 0)], 96_000, &[], &holds, &half, &no_points);
    close(&w[0].points, &[(0, 0.6), (96_000, 0.6)]);
}

/// Pre-flight M3, Ruling 9: a Write strip's touch after a wrap is written
/// in the After Write mode (Latch: held to the pass's end); with After
/// Write Off or Read it writes nothing.
#[test]
fn write_stamps_after_a_wrap_take_the_after_write_mode() {
    let stamps = [
        st(PAN, 120_000, GesturePhase::Touch(0.4), WriteMode::Write),
        st(PAN, 130_000, GesturePhase::Hold, WriteMode::Write),
    ];
    let marks = [roll(0, 0), wrap(96_000, 0)];
    let w = run(&marks, 150_000, &stamps, &[], &half, &no_points);
    assert_eq!(w.len(), 1, "{w:?}");
    assert_eq!(w[0].pass, 1);
    close(&w[0].points, &[(24_000, 0.4), (54_000, 0.4)]);
    assert!(run_after(None, &marks, 150_000, &stamps, &[], &half, &no_points).is_empty());
}

/// Ruling 23 (pre-flight C2): a mark that jumps inside a pass (a resync
/// whose frame does not continue the piece) ends the open segment where
/// the earlier piece ended and reopens it at the new piece's start while
/// the target is still touched, so no write spans the jump; a held
/// (released) segment ends there, as at a wrap.
#[test]
fn a_jump_inside_a_pass_breaks_the_segment() {
    let jump = Mark { run: 48_000, frame: 10_000, wrap: false, latency: 0 };
    let stamps = [
        st(PAN, 40_000, GesturePhase::Touch(0.7), WriteMode::Latch),
        st(PAN, 50_000, GesturePhase::Value(0.8), WriteMode::Latch),
        st(PAN, 60_000, GesturePhase::Hold, WriteMode::Latch),
    ];
    let w = run(&[roll(0, 0), jump], 96_000, &stamps, &[], &half, &no_points);
    assert_eq!(w.len(), 2, "{w:?}");
    assert!(w.iter().all(|x| x.pass == 0 && x.target == PAN));
    close(&w[0].points, &[(40_000, 0.7), (48_000, 0.7)]);
    close(&w[1].points, &[(10_000, 0.7), (12_000, 0.8), (58_000, 0.8)]);
    assert!(w.iter().all(|x| x.points.windows(2).all(|p| p[0].0 < p[1].0)), "frames rise in each write");
    // Released (held) before the jump: it ends there and does not reopen.
    let held = [
        st(PAN, 40_000, GesturePhase::Touch(0.7), WriteMode::Latch),
        st(PAN, 41_000, GesturePhase::Hold, WriteMode::Latch),
    ];
    let w = run(&[roll(0, 0), jump], 96_000, &held, &[], &half, &no_points);
    assert_eq!(w.len(), 1, "{w:?}");
    close(&w[0].points, &[(40_000, 0.7), (48_000, 0.7)]);
}

/// Trim (Ruling 11): +6 dB over a ramp from −20 dB to 0 dB keeps the
/// ramp's shape: the existing point at 48 000 is moved in place, the
/// offset glides back to 0 over the return time.
#[test]
fn trim_adds_its_offset_to_an_existing_ramp() {
    let ramp = |_: AutomationTarget, f: u64| Some(fader_norm(-20.0 + 20.0 * f as f64 / 96_000.0));
    let pts = |_: AutomationTarget, a: u64, b: u64| -> Vec<u64> {
        [0, 48_000, 96_000].into_iter().filter(|f| (a..=b).contains(f)).collect()
    };
    let mut stamps = vec![st(FADER, 24_000, GesturePhase::Touch(ramp(FADER, 24_000).unwrap()), WriteMode::Trim)];
    for r in (24_000..=72_000).step_by(4_800) {
        stamps.push(st(FADER, r, GesturePhase::Offset(6.0), WriteMode::Trim));
    }
    stamps.push(st(FADER, 72_000, GesturePhase::Glide(12_000), WriteMode::Trim));
    let w = run(&[roll(0, 0)], 96_000, &stamps, &[], &ramp, &pts);
    assert_eq!(w.len(), 1);
    let got: Vec<(u64, f64)> = w[0].points.iter().map(|(f, v)| (*f, fader_db(*v))).collect();
    let want = [(24_000, -9.0), (48_000, -4.0), (72_000, 1.0), (84_000, -2.5)];
    assert_eq!(got.len(), want.len(), "{got:?}");
    for (g, x) in got.iter().zip(want) {
        assert!(g.0 == x.0 && (g.1 - x.1).abs() < 1e-6, "{got:?}");
    }
}

/// Spec §7.3 stepped targets keep only their changes (and the last
/// sample); in Touch a click is held (Ruling 10's Hold).
#[test]
fn stepped_targets_keep_only_their_changes() {
    let stamps = [
        st(MUTE, 12_000, GesturePhase::Touch(1.0), WriteMode::Touch),
        st(MUTE, 12_480, GesturePhase::Value(1.0), WriteMode::Touch),
        st(MUTE, 12_960, GesturePhase::Value(1.0), WriteMode::Touch),
        st(MUTE, 13_000, GesturePhase::Hold, WriteMode::Touch),
    ];
    let w = run(&[roll(0, 0)], 96_000, &stamps, &[], &half, &no_points);
    close(&w[0].points, &[(12_000, 1.0), (96_000, 1.0)]);
}

/// Ruling 12: every thinned-away sample lies within the tolerance of the
/// line through the kept ones (in dB for faders), and points go.
#[test]
fn thinning_stays_within_its_tolerance() {
    let db: Vec<(u64, f64)> = (0..400u64)
        .map(|k| (k * 480, fader_norm(-10.0 + 10.0 * (k as f64 / 23.0).sin())))
        .collect();
    let pan: Vec<(u64, f64)> = (0..400u64).map(|k| (k * 480, 0.5 + 0.4 * (k as f64 / 31.0).sin())).collect();
    for th in [Thinning::Low, Thinning::Medium, Thinning::High] {
        for (samples, target, in_db) in [(&db, FADER, true), (&pan, PAN, false)] {
            let tol = th.tolerance(target);
            let kept = thin(samples, tol, in_db, false);
            assert!(kept.len() < samples.len() / 2, "{th:?} {target:?}: {}", kept.len());
            assert_eq!(kept.first(), samples.first());
            assert_eq!(kept.last(), samples.last());
            let y = |v: f64| if in_db { fader_db(v) } else { v };
            for (f, v) in samples.iter() {
                let i = kept.partition_point(|k| k.0 <= *f);
                let (a, b) = (kept[i.saturating_sub(1)], kept[i.min(kept.len() - 1)]);
                let line = if b.0 == a.0 {
                    y(a.1)
                } else {
                    y(a.1) + (y(b.1) - y(a.1)) * (*f - a.0) as f64 / (b.0 - a.0) as f64
                };
                assert!((y(*v) - line).abs() <= tol + 1e-9, "{th:?} {target:?} at {f}");
            }
        }
    }
    assert!((Thinning::Medium.tolerance(FADER) - 0.1).abs() < 1e-12);
    assert!((Thinning::Medium.tolerance(PAN) - 0.0025).abs() < 1e-12);
    assert!((Thinning::High.tolerance(PAN) - 0.005).abs() < 1e-12);
    assert!((Thinning::Low.tolerance(FADER) - 0.04).abs() < 1e-12);
}

/// Ruling 12 (pre-flight M7): on equal maximum distance the earliest
/// index splits, so the result is deterministic.
#[test]
fn thinning_splits_the_earliest_of_equal_distances() {
    let samples = [(0, 0.0), (100, 1.0), (200, 1.0), (300, 0.0)];
    assert_eq!(thin(&samples, 0.6, false, false), vec![(0, 0.0), (100, 1.0), (300, 0.0)]);
}

/// Ruling 7: stamps are heard the graph latency after their run, as
/// recorded MIDI is.
#[test]
fn latency_moves_stamps_to_the_heard_position() {
    let stamps = [
        st(PAN, 9_600, GesturePhase::Touch(0.2), WriteMode::Touch),
        st(PAN, 14_400, GesturePhase::Release, WriteMode::Touch),
    ];
    let w = run(&[roll(48_000, 4_800)], 96_000, &stamps, &[], &half, &no_points);
    close(&w[0].points, &[(52_800, 0.2), (57_600, 0.2)]);
    // Stamps without a write mode (the static path) write nothing.
    let mut quiet = stamps;
    for s in quiet.iter_mut() {
        s.tag = 0;
    }
    assert!(run(&[roll(48_000, 0)], 96_000, &quiet, &[], &half, &no_points).is_empty());
    assert_eq!(WriteMode::of_tag(WriteMode::Trim.tag()), Some(WriteMode::Trim));
    assert_eq!(WriteMode::of_tag(0), None);
}

/// The lane rule of Plan 4d (linear; first value before the first point).
fn lane_value(points: &[(i64, f64)], t: i64) -> f64 {
    let n = points.partition_point(|p| p.0 <= t);
    match (n.checked_sub(1).map(|i| points[i]), points.get(n)) {
        (None, Some(b)) => b.1,
        (Some(a), Some(b)) if b.0 > a.0 => a.1 + (b.1 - a.1) * (t - a.0) as f64 / (b.0 - a.0) as f64,
        (Some(a), _) => a.1,
        (None, None) => 0.0,
    }
}

/// Spec §7.3, Ruling 13: a point one tick beyond each end holds the value
/// the target had there, so every value outside the written range is what
/// it was; with no automation, the static value.
#[test]
fn edge_points_leave_the_values_outside_unchanged() {
    let q = PPQ;
    let doc = apply(&Document::new(), &Command::Sequence(SequenceCmd::New { name: "S".into() }))
        .unwrap()
        .doc;
    let master = doc.mixer().master();
    let target = AutomationTarget::StripFader(master);
    let before = [(0, 0.2), (8 * q, 0.8)];
    let doc = apply(
        &doc,
        &Command::Event(EventCmd::Insert {
            seq: SeqId(0),
            lane: LaneId(0),
            events: before
                .iter()
                .map(|(t, v)| EventSpec::new(Tick(*t), EventBody::Automation { target, value: *v }))
                .collect(),
        }),
    )
    .unwrap()
    .doc;
    let s = doc.sequence(SeqId(0)).unwrap();
    let tempo = TempoTable::build(&s.tempo, Tick(-s.start_offset.0), 48_000.0);
    // Beats 2 to 4 (24 000 frames a beat at 120 BPM).
    let tw = TargetWrite { target, pass: 0, points: vec![(48_000, 0.9), (96_000, 0.9)] };
    let outside = |_: AutomationTarget, t: Tick| lane_value(&before, t.0);
    let model = to_model(&[tw.clone()], SeqId(0), &tempo, &outside, &|_| "Master Fader".into());
    assert_eq!(model.len(), 1);
    let w = &model[0];
    assert_eq!((w.start, w.end), (Tick(2 * q - 1), Tick(4 * q + 1)));
    assert_eq!(w.points.first().unwrap().0, Tick(2 * q - 1));
    assert_eq!(w.points.last().unwrap().0, Tick(4 * q + 1));
    let out = apply(&doc, &Command::Record(RecordPass { automation: model, ..RecordPass::default() })).unwrap().doc;
    let mut after: Vec<(i64, f64)> = out
        .lane(SeqId(0), LaneId(0))
        .unwrap()
        .active()
        .unwrap()
        .events
        .values()
        .filter_map(|e| match e.body {
            EventBody::Automation { value, .. } => Some((e.tick.0, value)),
            _ => None,
        })
        .collect();
    after.sort_by(|a, b| a.partial_cmp(b).unwrap());
    for t in [0, q, 2 * q - 1, 4 * q + 1, 6 * q, 8 * q, 9 * q] {
        assert!((lane_value(&after, t) - lane_value(&before, t)).abs() < 1e-12, "at {t}: {after:?}");
    }
    assert!((lane_value(&after, 3 * q) - 0.9).abs() < 1e-12);
    // No automation: the edges hold the static value.
    let model = to_model(&[tw], SeqId(0), &tempo, &|_, _| 0.3, &|_| "Master Fader".into());
    let p = &model[0].points;
    assert!(p.first().unwrap().1 == 0.3 && p.last().unwrap().1 == 0.3);
}

/// Ruling 25 (pre-flight I6): a write longer than the model's limit is
/// split into consecutive writes the model takes: none over the limit,
/// ranges apart, edges only at the outer ends, no point lost.
#[test]
fn a_write_longer_than_the_model_limit_is_split() {
    use core_model::command::MAX_WRITE_POINTS;
    let doc = apply(&Document::new(), &Command::Sequence(SequenceCmd::New { name: "S".into() }))
        .unwrap()
        .doc;
    let s = doc.sequence(SeqId(0)).unwrap();
    let tempo = TempoTable::build(&s.tempo, Tick(-s.start_offset.0), 48_000.0);
    // 4 frames apart is 2.56 ticks at 120 BPM: no two share a tick.
    let input: Vec<(u64, f64)> = (0..250_000u64).map(|k| (k * 4, if k % 2 == 0 { 0.2 } else { 0.8 })).collect();
    let tw = TargetWrite { target: PAN, pass: 0, points: input.clone() };
    let model = to_model(&[tw], SeqId(0), &tempo, &|_, _| 0.3, &|_| "Pan".into());
    assert!(model.len() >= 3, "{}", model.len());
    for w in &model {
        assert!(w.points.len() <= MAX_WRITE_POINTS, "{}", w.points.len());
        assert!(w.points.iter().all(|(t, _)| *t >= w.start && *t <= w.end));
    }
    assert!(model.windows(2).all(|p| p[0].end.0 + 1 == p[1].start.0), "the ranges are contiguous and disjoint");
    let all: Vec<(Tick, f64)> = model.iter().flat_map(|w| w.points.iter().copied()).collect();
    assert_eq!(all.len(), input.len() + 2, "the input and the two edges");
    assert!(all.windows(2).all(|p| p[0].0 < p[1].0), "ticks rise");
    assert!(all[0].1 == 0.3 && all[all.len() - 1].1 == 0.3, "the edges hold the value outside");
    let inner: Vec<f64> = all[1..all.len() - 1].iter().map(|p| p.1).collect();
    assert_eq!(inner, input.iter().map(|p| p.1).collect::<Vec<_>>());
}
VIZDAW_EOF
```

(`AutomationTarget::is_stepped` is 4d's; `Mark`'s fields are public, as `passes.rs`'s tests build them.)

- [ ] **Step 2: Run the tests to verify they fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-ffi --test autowrite 2>&1 | tail -3
```

Expected: FAIL to compile (`vizdaw_ffi::autowrite` does not exist).

- [ ] **Step 3: Write `autowrite.rs`**

Create `crates/core-ffi/src/autowrite.rs` with the module doc "Placing recorded automation (sub-project 5 spec §7.3, §7.4; plan 5e-1 Rulings 7, 9–13): turns one pass's gesture stamps and marks into automation writes. Pure: no I/O, no locks, no engine; control-side code." and the items of the Interfaces block, following the algorithm above. RDP: iterative (an explicit stack of index ranges), so 100 000 samples cannot overflow the stack; on equal maximum distance the earliest index splits (M7). `to_model` drops a write whose points collapse to nothing and splits one over the limit as above. In `crates/core-ffi/src/passes.rs`, make `Piece` (and its `pass` field), `pieces`, `piece_of` and `place` `pub(crate)` and add `pass_window` (doc: "The schedule frames pass `pass` was heard over: from its first piece's start to its last piece's end, as heard (plan 5e-1 Task 7).").

- [ ] **Step 4: Run the tests to verify they pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -2
cargo clippy --workspace --all-targets --target x86_64-unknown-linux-gnu --target-dir target/linux-check -- -D warnings 2>&1 | tail -2
cargo test -p core-ffi --test autowrite --test passes 2>&1 | grep -E "test result|FAILED"
```

Expected: `autowrite` 13 passed (Rust 1501); `passes` unchanged.

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-ffi/src/autowrite.rs crates/core-ffi/src/lib.rs crates/core-ffi/src/passes.rs crates/core-ffi/tests/autowrite.rs
git commit -m "core-ffi: the automation writer: Touch, Latch, Write and Trim, thinning and edge points, golden-tested (5e-1 Task 7)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -3
```

### Task 8: The session: gestures, writing while playing, the landing at Stop, settings, state and preview

**Files:**
- Create: `crates/core-ffi/src/automation.rs`, `crates/core-ffi/tests/automation.rs`
- Modify: `crates/core-ffi/src/lib.rs` (`pub mod automation;` and its re-exports), `crates/core-ffi/src/audio.rs` (`pub(crate) fn bump_epoch()`), `crates/core-ffi/src/host.rs` (`gesture`, `start_gestures`, `stop_gestures`, `heard`, `gestures_dropped`, the test hook `debug_hold_offline`; `Host::load` passes suspend), `crates/core-ffi/src/session.rs` (the gesture path, the writing lifecycle, the jumps, the new methods), `crates/core-ffi/src/mixer.rs` (`StripRow.automation`)
- Modify: `app/Tests/VizDAWAppModelTests/FakeSession.swift` (the four new methods; `stripRow(…)` passes `automation: .read`)

**Interfaces:**
- Consumes: Tasks 2–7: `AutomationMode`, `MixerCmd::SetAutomationMode`, `AutomationWrite`, `RecordPass.automation`, `target_exists`, `Engine::{gesture_sender, gestures_dropped, overrides, start_gestures, stop_gestures, set_automation_suspended}`, `OverrideSnapshot::value`, `GestureMsg`, `GestureHandles`, `GestureStatus::{run, rolled, closed, lost_marks, lost_stamps}`, `autowrite::{writes, to_model, AutoCapture, WriteMode, Thinning}`; `events::{automation_target_row, automation_target_of}`; `query::flatten` (for existing automation, Ruling 22); `core_model::automation::param_plain`.
- Produces (UniFFI):
  ```rust
  #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
  pub enum AutomationModeRow { Off, Read, Touch, Latch, Write, Trim }   // From/Into AutomationMode
  #[derive(Debug, Clone, Copy, PartialEq, uniffi::Enum)]
  pub enum GesturePhaseRow {
      /// Grabbed; `value` absent: at the value heard now (Ruling 26; the mixer sends none).
      Touch { value: Option<f64> },
      /// Moved (normalised 0–1).
      Value { value: f64 },
      /// Let go.
      Release,
  }
  #[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
  pub struct StripModeRow { pub strip: u64, pub mode: AutomationModeRow }
  #[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
  pub struct AutomationStateRow {
      /// Every strip's mode, in the mixer's display order (master last).
      pub modes: Vec<StripModeRow>,
      /// The targets touched now: active touches only, never Write holds or released Latch holds (pre-flight M8; 5e-2's rings).
      pub touched: Vec<AutomationTargetRow>,
      /// Automation is suspended (process-wide).
      pub suspended: bool,
      /// This session is playing and its gesture tap is installed.
      pub writing: bool,
  }
  #[derive(Debug, Clone, Copy, PartialEq, uniffi::Record)]
  pub struct PreviewPointRow { pub tick: i64, pub value: f64 }
  #[derive(Debug, Clone, Copy, PartialEq, Eq, Default, uniffi::Enum)]
  pub enum ThinningRow { Low, #[default] Medium, High }
  #[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
  pub struct AutomationSettingsRow { pub after_write: AutomationModeRow, pub return_ms: u32, pub thinning: ThinningRow }
  #[uniffi::export] pub fn automation_settings() -> AutomationSettingsRow;
  /// `Model` for `return_ms` over 2000 or `after_write` Write; nothing changes then (Ruling 19).
  #[uniffi::export] pub fn set_automation_settings(settings: AutomationSettingsRow) -> Result<(), VizError>;
  /// Ruling 3: process-wide; the holder reloads at its next idle (the audio settings' epoch).
  #[uniffi::export] pub fn set_automation_suspended(on: bool);
  #[uniffi::export] pub fn automation_suspended() -> bool;
  #[cfg(feature = "test-hooks")] pub fn reset_automation_settings();
  impl Session {
      /// A gesture on `target` from the app or the channel (spec §7.1; Rulings 14, 16, 20, 26). `Model` for a target that cannot be gestured or a non-finite value; `Busy` for a static touch while an edit the gesture path did not open is open.
      pub fn gesture(&self, target: AutomationTargetRow, phase: GesturePhaseRow) -> Result<(), VizError>;
      /// Sets the mode of `strips` (spec §4.6) as one undo step "Set Automation Mode".
      pub fn set_automation_mode(&self, strips: Vec<u64>, mode: AutomationModeRow) -> Result<DocSnapshot, VizError>;
      pub fn automation_state(&self) -> Result<AutomationStateRow, VizError>;
      /// Ruling 18.
      pub fn automation_preview(&self, target: AutomationTargetRow) -> Result<Vec<PreviewPointRow>, VizError>;
  }
  ```
  `StripRow.automation: AutomationModeRow`. Internal (`crate::automation`): `pub(crate) enum Source { Ui, Plugin }` (Ruling 15); `pub(crate) fn writing_ended(closed: bool, rolled: bool, transport_stopped: Option<bool>) -> bool` (Ruling 24: `closed || transport_stopped.is_none() || (rolled && transport_stopped == Some(true))`); `pub(crate) struct UnsentEnds` (pre-flight I3: `fn put(&mut self, t: AutomationTarget, m: GestureMsg)`, `fn clear(&mut self, t: AutomationTarget)`, `fn resend(&mut self, send: impl FnMut(GestureMsg) -> bool)` keeping what is refused, `fn len(&self) -> usize`); `Sampler` (Ruling 8: `INTERVAL = 10 ms`; `fn value(&mut self, t, v, now: Duration, host_ns: u64) -> Option<f64>`, `fn due(&mut self, now: Duration) -> Vec<(AutomationTarget, f64, u64)>` and `fn flush(&mut self, t) -> Option<(f64, u64)>`, each kept value with the host time it arrived at, M6), `fn lane_name(doc: &Document, t: AutomationTarget, param_name: Option<&str>) -> String` (Ruling 4), `fn existing(doc, seq, t) -> Vec<(Tick, f64)>` (the target's points in `seq` flattened, by tick) with `fn value_at(points: &[(Tick, f64)], tick: Tick, stepped: bool) -> Option<f64>` (Plan 4d's lane rule; `None` with no points), `fn fixed_value(doc, t, default: Option<f64>) -> f64` (the static value, normalised; a plugin parameter's from the slot's overlay, else its default from the host's parameter list normalised by its range, else 0.5), `fn static_command(doc, t, v) -> Option<Command>` (SetGain/SetPan/SetMute/SetSolo/EditSend; `None` for plugin parameters). Host: `host::gesture(owner, msg: GestureMsg) -> bool`, `host::start_gestures(owner) -> Option<GestureHandles>`, `host::stop_gestures(owner)`, `host::heard(owner, t: AutomationTarget) -> Option<f64>` (the engine's override snapshot value for a strip or send target, through `AutoDest::of` and `OverrideSnapshot::value`, when `owner` holds the engine; Ruling 26), `host::gestures_dropped(owner) -> Option<u64>` (M10); test hook `#[cfg(feature = "test-hooks")] pub fn debug_hold_offline(on: bool)` (check I-1: while on, the offline driver renders nothing, so a test can run code before the roll).

**How the session does it** (Rulings 3, 7–10, 14, 15, 19–28):
- **State** in `Inner`:
  - `touches: BTreeMap<AutomationTarget, Touching>` (`Touching { path: Path, value: f64, base: f64, source: Source }`): `Path::Static` (inside the shared static edit), `Path::Live` (heard only: a plugin-source touch off a writing path, or a touch re-picked at Play on a strip that does not write), `Path::Write(WriteMode, release: Release)` where `Release` is `Glide`, `Hold` or `Offsets`;
  - `static_edit: Option<(String, usize)>`: the shared static edit's label and its open static touches (Ruling 14);
  - `sampler: Sampler`; `unsent_ends: UnsentEnds` (Ruling 10);
  - `writing: Option<Writing>`: `Writing { seq, handles, marks: Vec<Mark>, stamps: BTreeMap<AutomationTarget, Vec<Stamped>>` (per target, M15)`, holds: Vec<(AutomationTarget, f64)>, write_strips: BTreeSet<StripId>, wrapped: bool, return_frames: u64, after_write: AutomationMode, thinning: Thinning, dropped_at_start: u64 }`;
  - `pending_automation: Option<(Vec<AutomationWrite>, Vec<StripId>)>` and `after_write_pending: Vec<StripId>` (the Write strips whose mode changes when a recording's pass lands).
- **gesture(target, phase)** calls the internal path with `Source::Ui` and no host time: `touch(inner, target, value: Option<f64>, source, host_ns: Option<u64>)`, `move_to(inner, target, v, source, host_ns)`, `release(inner, target, source, host_ns)`. It refuses (Ruling 16) velocity scale, a target `target_exists` denies, a plugin parameter the host's running instance does not list, non-finite values; clamps values to 0–1. A `Value` with no touch is a touch first (at that value); a `Release` with no touch does nothing.
- A **touch**:
  - resolves its value (Ruling 26): `Some(v)` is `v`; `None` is the value heard now: for strip and send targets `host::heard(self.id, t)`, else while playing `value_at(existing(doc, seq, t), tick, stepped)` at the transport tick, else `fixed_value`; for a plugin parameter the slot's current value from the host, normalised by its range (else `fixed_value`);
  - picks the path once (Ruling 14): `Write(mode)` when this session holds the engine and plays, `writing` is set, automation is not suspended and the target's strip writes (`WriteMode::of`; a strip that was not Write at Play but is now writes as Latch, Ruling 9); else `Live` for `Source::Plugin` (the host's grouped `Gesture` is the static step, Ruling 15); else `Static`;
  - on `Static`: with `static_edit` `None`, `Busy` "Finish the open edit first." when `history.in_transaction()` (an edit the gesture path did not open: `begin_edit`, `edit_gesture`, step entry), else `history.begin(label)` with the target's label ("Set Fader", "Set Pan", "Mute", "Solo", "Set Send Level", "Change Plugin Parameter") and `static_edit = Some((label, 1))`; with it `Some`, the count goes up (the edit keeps the first label);
  - goes to the engine as `GestureMsg { Touch(v) }`, or `Offset(0.0)` on a Trim path with offsets (pre-flight I12; `base` is the resolved value), tagged `WriteMode::tag` (0 off a writing path), `host_ns` the given time, else `clock::now_ns()` unless `host::offline_mode()` (0) (Ruling 7).
- Each **value** goes through the sampler (send now, or keep with its arrival time). A sent value:
  - on the static path runs `static_command` inside the edit (`history.execute`, `version += 1`, `reload_host`, `mark`);
  - for a `Source::Ui` `PluginParam`, on every path, goes to the plugin (`plugins::with(self.id, |p| p.set_param(slot, param, param_plain(v, min, max, stepped)))`); the static path remembers it for the release's `SetPluginParams` (Ruling 15);
  - goes to the engine as `Value(v)` (or `Offset(d)` on a Trim path with offsets: `d = fader_db(v) − fader_db(base)` for `is_db` targets, `v − base` otherwise), tagged as the touch, stamped with the value's own arrival time (M6).
- A **release** first sends the kept value, then:
  - with `writing` `None` (stopped, or the pass already landed) it sends `Release`, whatever the path (pre-flight I2);
  - static → a `Ui` plugin parameter's `SetPluginParams` is executed inside the edit; the count goes down and at 0 the edit commits (`static_edit = None`, then `apply_pending_pass` and `apply_pending_automation`); `Release`;
  - live → `Release`; Touch → `Glide(return_frames)`; Latch, Write → `Hold`; Trim → `Glide(return_frames)`; a stepped target on Touch or Trim → `Hold` (Ruling 10);
  - a Touch or Trim path `PluginParam` that no schedule lane drives is set to its static value (`fixed_value`, plain; `set_param`, no commit; pre-flight M14).
  Trim's path uses offsets only where the playing sequence automates the target (`existing` not empty); otherwise it sends absolute values like Touch (Ruling 11).
- **Unsent ends** (pre-flight I3): a `Release`, `Glide` or `Hold` that `host::gesture` could not send while this session holds the engine goes to `unsent_ends.put(target, msg)`; `idle` calls `unsent_ends.resend(|m| host::gesture(self.id, m))` first; any later message sent for that target calls `unsent_ends.clear(target)`.
- **Play and Record** (`start_playing`, `start_recording`): a pass still writing is finished and landed first, before `host::play` (Ruling 23). Once `host::play` succeeded and before `finish_play`, `begin_writing(inner, seq)`:
  - `host::start_gestures(self.id)`; none: the problem line "Automation is not being written this pass: the engine was busy." (M10) and no `writing`;
  - the settings read now and kept (After Write, thinning, the return time in frames at the engine's current rate: `host::device(self.id)`'s rate, else the project rate; M4, M5), and `dropped_at_start = host::gestures_dropped(self.id)`;
  - every Write strip's fader, pan (not master), mute and send levels held: the value at the play-start tick (`existing` there, else `fixed_value`) sent as `Touch(v)` then `Hold`, tagged Write, recorded in `holds` (Ruling 9); the punch and record range play no part (Ruling 28);
  - every still-held touch has its path re-picked (Ruling 20 as amended, M2): a static touch whose edit is open keeps `Static`; else `Write(mode)` when its strip writes now, else `Live`; its next value is tagged accordingly (nothing is stamped at the roll).
  A `host::finish_play` that fails finishes writing (`finish_writing`, `apply_pending_automation`; Ruling 24).
- **locate** (Ruling 23): with `writing` set, its `seq` and whether its tap had rolled are kept, then `finish_writing` and `apply_pending_automation`, then `host::locate`; then `begin_writing(inner, seq)` again when the transport plays (`host::transport_stopped(self.id) == Some(false)`) or when the old pass had not rolled and this session still holds the engine (`Some(_)`; check I-2).
- **idle** (`service_writing`, right after `service_recording`): re-sends `unsent_ends`; drains the tap's marks and stamps into `writing` (stamps by target); sends the sampler's due values; a wrap mark sets `wrapped` (touches from then on on a Write strip are tagged with After Write's mode); ends the pass, `finish_writing`, when `writing_ended(status.closed(), status.rolled(), host::transport_stopped(self.id))` (Ruling 24: never before the roll while this session holds the engine; `None`, another session took it, ends the pass at once, M17). `apply_pending_automation` runs in `idle` after the `self.apply_pending_pass(&mut inner);` at `session.rs:1608` and in `end_edit` after the `self.commit_held(&mut inner);` at `session.rs:1834`.
- **take_writes(inner) -> (Vec<AutomationWrite>, Vec<StripId>)** ends the pass and answers its writes and the Write strips that still exist (none without `writing`; `writing` is `None` after it). **finish_writing** (called by `stop` after `host::stop`, by the jumps of Ruling 23, by a failed `finish_play` and by `close`) is `take_writes` plus `pending_automation`; **writes_into_pass** (called by `finish_recording` before its pass is set pending) is `take_writes` plus the pass. `take_writes`:
  - `host::stop_gestures`, then up to 250 ms of 2 ms polls for `status.closed()` (M1: the renderer marks the Stop first), a last drain, `end_run = status.run()`;
  - one problem line "Some automation moves were lost while writing." when `lost_stamps` or `lost_marks` > 0 or `host::gestures_dropped` grew since `dropped_at_start` (M10);
  - `autowrite::writes` with the session's closures (`existing`/`value_at` in frames through the playing sequence's `TempoTable`, `existing_points`, `fixed_value`, `stepped` = `is_stepped` or a stepped plugin parameter) and `after_write: WriteMode::of(after_write)`, then `to_model` with `lane_name` (the plugin's parameter name from the host when it has one) and the pass's thinning;
  - drops (Ruling 25) every write whose target fails `target_exists` or whose `seq` is gone, with "Automation of <describe> was not written: it no longer exists." (`AutomationTarget::describe`);
  - answers the Write strips only when some write is left. `finish_writing` sets `pending_automation = Some((writes, strips))` when some write is left; with none, nothing is pending and no undo step is made (a Play with nothing touched on Touch, Latch or Trim strips leaves the history as it was).
- **writes_into_pass(inner, pass: Option<RecordPass>) -> Option<RecordPass>**: `after_write_pending` takes the strips; no writes → `pass` as it was; a pass → its `automation`; no pass (`record::build` failed or recorded nothing) → `RecordPass { automation, ..Default::default() }`, which is named "Write Automation" and still lands inside the recording's group (Ruling 25).
- **apply_pending_pass** (existing): when `execute(Command::Record(p))` errs, the problem line is kept and the pass's `automation` alone is executed as `RecordPass { automation, ..Default::default() }` inside the same group before `end_group`; `after_write_in_group` executes `SetAutomationMode { strips: after_write_pending, mode: after_write }` only when automation landed.
- **apply_pending_automation**: waits while `history.in_transaction()` or `static_edit` is open; with Write strips, `begin_group`, `Command::Record(RecordPass { automation, .. })`, `SetAutomationMode { strips, mode: after_write }`, `end_group("Write Automation")`; with only writes, a plain `execute`; then `version += 1`, `news.doc_changed`, `reload_host`, `mark`. A refusal is a problem line "The automation could not be written ({e})." and nothing lands.
- **After a landing** (either path), each written `PluginParam` is sent the landed automation's value at the transport tick (`set_param`, plain, no commit; Ruling 15, M13).
- **Vanished targets** (M9): after every document change the session makes (`execute`, `undo`, `redo`, a landing), touches whose target fails `target_exists` are released (`release`, which ends a static edit's share).
- **close**: every touch is released (the static edit commits) and a pass in progress lands, before the engine is released (a pass landing in a window already past its save prompt is lost unsaved; the app stops before closing in practice, M20).
- **automation_state**: the doc's strips in display order with their modes, the keys of `touches` (M8), `automation_suspended()`, `writing.is_some()`. **automation_preview(target)**: with `writing`, the writer over that target's stamps, its holds and the marks (`end_run` = the tap's status run, the pass's thinning and After Write), that target's writes in the last pass that has one, `to_model`ed, as rows; else empty.

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-ffi/tests/automation.rs <<'VIZDAW_EOF'
//! Automation recording through a session (plan 5e-1 Tasks 8–10;
//! sub-project 5 spec §7, §12.3): gestures on the static path and while
//! writing, each mode's landing at Stop as one undo step, writes joining
//! a recording, Off and suspend, hostile gestures and settings, a device
//! rebuild mid-pass, the idle before the roll, a Locate mid-pass, two
//! static gestures at once, a touch at the heard value, a target deleted
//! mid-pass, the preview and the state, the channel's spelling.
//! Offline only: no audio device is opened and no microphone asked.

#![recursion_limit = "256"]

mod common;

use core_engine::backend::BackendEvent;
use core_model::automation::{fader_db, fader_norm, pan_of};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use vizdaw_ffi::automation::{
    AutomationModeRow as Mode, AutomationSettingsRow, GesturePhaseRow as Phase, ThinningRow,
    reset_automation_settings, set_automation_settings, set_automation_suspended,
};
use vizdaw_ffi::host::{debug_hold_offline, fader_override, force_offline, reset_host, simulate_event};
use vizdaw_ffi::{AutomationTargetRow as Target, EventRow, Session, VizError};

const Q: i64 = 15_360;

static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    force_offline();
    reset_host();
    reset_automation_settings();
    set_automation_suspended(false);
    debug_hold_offline(false);
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
    let song = s.snapshot().unwrap().sequences.iter().find(|r| r.name == "Song").unwrap().id;
    (s, song)
}

fn strip(s: &Session, name: &str) -> u64 {
    s.mixer().unwrap().strips.iter().find(|x| x.name == name).unwrap().id
}

fn tick(s: &Session) -> i64 {
    s.transport().unwrap().tick
}

fn lane_named(s: &Session, seq: u64, name: &str) -> Option<u64> {
    s.lanes(seq).unwrap().into_iter().find(|l| l.name == name).map(|l| l.id)
}

/// `(tick, value)` of a lane's automation points, by tick.
fn points(s: &Session, seq: u64, lane: u64) -> Vec<(i64, f64)> {
    let mut v: Vec<(i64, f64)> = s
        .lane_events(seq, lane)
        .unwrap()
        .events
        .iter()
        .filter_map(|e: &EventRow| e.automation.map(|a| (e.tick, a.value)))
        .collect();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v
}

fn stop_and_land(s: &Session) {
    s.stop().unwrap();
    wait_until("the transport to stop", || !s.transport().unwrap().playing);
    s.idle().unwrap();
}

/// Spec §7.3 Touch and §7.4: a fader moved while playing writes into a
/// new lane "<strip> Fader" at the end of the playing sequence, glides
/// back to the static value, and lands at Stop as one undo step "Write
/// Automation"; the static value is unchanged.
#[test]
fn touch_writes_a_fader_while_playing_as_one_undo_step() {
    let _g = serial();
    let (s, song) = demo();
    let synth = strip(&s, "Built-in Synth");
    let gain = s.mixer().unwrap().strips.iter().find(|x| x.id == synth).unwrap().gain_db;
    s.set_automation_mode(vec![synth], Mode::Touch).unwrap();
    assert_eq!(s.snapshot().unwrap().undo_label.as_deref(), Some("Set Automation Mode"));
    s.play(song).unwrap();
    wait_until("beat 1", || tick(&s) > Q);
    let t = Target::StripFader { strip: synth };
    s.gesture(t, Phase::Touch { value: Some(0.5) }).unwrap();
    assert!(s.automation_state().unwrap().touched.contains(&t));
    for v in [0.55, 0.6, 0.65, 0.7] {
        pace(&s);
        s.gesture(t, Phase::Value { value: v }).unwrap();
    }
    pace(&s);
    assert!(!s.automation_preview(t).unwrap().is_empty(), "the write in progress");
    s.gesture(t, Phase::Release).unwrap();
    let at = tick(&s);
    wait_until("the glide to pass", || tick(&s) > at + Q);
    stop_and_land(&s);
    assert_eq!(s.snapshot().unwrap().undo_label.as_deref(), Some("Write Automation"));
    let lane = lane_named(&s, song, "Built-in Synth Fader").expect("a new lane");
    let p = points(&s, song, lane);
    assert!(p.len() >= 4, "{p:?}");
    let values: Vec<f64> = p.iter().map(|x| x.1).collect();
    assert!(values.iter().any(|v| (v - 0.5).abs() < 1e-9) && values.iter().any(|v| (v - 0.7).abs() < 1e-9), "{p:?}");
    // The edges and the glide's end hold the static value.
    let fixed = fader_norm(gain);
    assert!((p.first().unwrap().1 - fixed).abs() < 1e-9 && (p.last().unwrap().1 - fixed).abs() < 1e-9, "{p:?}");
    let g2 = s.mixer().unwrap().strips.iter().find(|x| x.id == synth).unwrap().gain_db;
    assert_eq!(g2, gain, "writing leaves the static value");
    s.undo().unwrap();
    assert_eq!(lane_named(&s, song, "Built-in Synth Fader"), None, "one step");
    assert!(s.automation_preview(t).unwrap().is_empty());
    s.close().unwrap();
}

/// Spec §7.2: in Read (and while stopped) a gesture changes the static
/// value as one undo step named after the control; the engine hears it
/// while touched.
#[test]
fn read_gestures_change_the_static_value_as_one_step() {
    let _g = serial();
    let (s, song) = demo();
    let synth = strip(&s, "Built-in Synth");
    let t = Target::StripFader { strip: synth };
    s.gesture(t, Phase::Touch { value: Some(0.5) }).unwrap();
    for v in [0.4, 0.3] {
        pace(&s);
        s.gesture(t, Phase::Value { value: v }).unwrap();
    }
    s.gesture(t, Phase::Release).unwrap();
    let snap = s.snapshot().unwrap();
    assert_eq!(snap.undo_label.as_deref(), Some("Set Fader"));
    let gain = s.mixer().unwrap().strips.iter().find(|x| x.id == synth).unwrap().gain_db;
    assert!((gain - fader_db(0.3)).abs() < 1e-9, "{gain}");
    s.undo().unwrap();
    assert_eq!(s.snapshot().unwrap().undo_label, None, "one step for the gesture");
    // While playing in Read, a touch is heard over the static value.
    s.play(song).unwrap();
    wait_until("playing", || tick(&s) > 0);
    s.gesture(t, Phase::Touch { value: Some(fader_norm(-20.0)) }).unwrap();
    wait_until("the override", || fader_override(synth).is_some_and(|d| (d + 20.0).abs() < 1e-6));
    s.gesture(t, Phase::Release).unwrap();
    stop_and_land(&s);
    s.close().unwrap();
}

/// Spec §7.3 Write and Latch; Ruling 9: an untouched Write strip writes
/// its controls from the roll at their values; at landing it becomes the
/// After Write mode (Latch) in the same undo step; Latch holds a touched
/// pan until Stop.
#[test]
fn write_and_latch_land_at_stop_and_write_becomes_latch() {
    let _g = serial();
    let (s, song) = demo();
    let synth = strip(&s, "Built-in Synth");
    s.set_automation_mode(vec![synth], Mode::Write).unwrap();
    s.play(song).unwrap();
    wait_until("two beats", || tick(&s) > 2 * Q);
    // Pre-flight M8: Write holds are not touches (5e-2's rings).
    assert!(s.automation_state().unwrap().touched.is_empty(), "{:?}", s.automation_state().unwrap().touched);
    stop_and_land(&s);
    assert_eq!(s.snapshot().unwrap().undo_label.as_deref(), Some("Write Automation"));
    for name in ["Built-in Synth Fader", "Built-in Synth Pan", "Built-in Synth Mute"] {
        let lane = lane_named(&s, song, name).unwrap_or_else(|| panic!("{name}"));
        assert!(points(&s, song, lane).len() >= 2, "{name}");
    }
    let state = s.automation_state().unwrap();
    assert!(state.modes.iter().any(|m| m.strip == synth && m.mode == Mode::Latch), "After Write");
    s.undo().unwrap();
    let state = s.automation_state().unwrap();
    assert!(state.modes.iter().any(|m| m.strip == synth && m.mode == Mode::Write), "one step");
    assert_eq!(lane_named(&s, song, "Built-in Synth Pan"), None);
    // Latch: a pan touched and let go holds to Stop.
    s.set_automation_mode(vec![synth], Mode::Latch).unwrap();
    s.play(song).unwrap();
    wait_until("beat 1", || tick(&s) > Q);
    let pan = Target::StripPan { strip: synth };
    s.gesture(pan, Phase::Touch { value: Some(0.2) }).unwrap();
    pace(&s);
    s.gesture(pan, Phase::Release).unwrap();
    let released = tick(&s);
    wait_until("a beat after the release", || tick(&s) > released + Q);
    stop_and_land(&s);
    let lane = lane_named(&s, song, "Built-in Synth Pan").unwrap();
    let p = points(&s, song, lane);
    let held: Vec<_> = p.iter().filter(|x| (x.1 - 0.2).abs() < 1e-9).collect();
    assert!(held.last().unwrap().0 > released + Q / 2, "held past the release: {p:?}");
    s.close().unwrap();
}

/// Review Focus 2, Ruling 20: a Stop while a fader is held lands what
/// was written; the touch stays until its release, which writes nothing
/// more and adds no undo step.
#[test]
fn a_stop_while_touched_lands_the_write_and_keeps_the_touch() {
    let _g = serial();
    let (s, song) = demo();
    let synth = strip(&s, "Built-in Synth");
    let gain = s.mixer().unwrap().strips.iter().find(|x| x.id == synth).unwrap().gain_db;
    s.set_automation_mode(vec![synth], Mode::Latch).unwrap();
    s.play(song).unwrap();
    wait_until("beat 1", || tick(&s) > Q);
    let t = Target::StripFader { strip: synth };
    s.gesture(t, Phase::Touch { value: Some(0.4) }).unwrap();
    pace(&s);
    stop_and_land(&s);
    assert_eq!(s.snapshot().unwrap().undo_label.as_deref(), Some("Write Automation"));
    assert!(s.automation_state().unwrap().touched.contains(&t));
    let version = s.snapshot().unwrap().version;
    s.gesture(t, Phase::Release).unwrap();
    s.idle().unwrap();
    assert!(!s.automation_state().unwrap().touched.contains(&t));
    assert_eq!(s.snapshot().unwrap().version, version, "no step for the late release");
    // Pre-flight I2: the late release sent Release, so nothing is held
    // through the next Play. The landed lane goes first, so nothing drives
    // the fader; no Locate (a Locate would end a wrong hold by itself).
    s.undo().unwrap();
    let from = tick(&s);
    s.play(song).unwrap();
    wait_until("a quarter beat on", || tick(&s) > from + Q / 4);
    let heard = fader_override(synth);
    assert!(heard.is_none_or(|d| (d - fader_db(0.4)).abs() > 1e-3), "nothing held from the late release: {heard:?}");
    assert!(heard.is_none_or(|d| (d - gain).abs() < 1e-6), "{heard:?}");
    stop_and_land(&s);
    s.close().unwrap();
}

/// §4.7, §7.4: with MIDI recording too, the writes are part of the one
/// "Record" step.
#[test]
fn writes_join_the_record_pass() {
    use core_engine::fixtures::{Builder, Q as FQ};
    use core_model::command::{Command, LaneCmd, SequenceCmd};
    use core_model::lane::LaneInput;
    use core_model::sequence::SeqLength;
    use core_model::time::Tick;
    let _g = serial();
    let mut b = Builder::new();
    let synth_dest = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, keys) = b.sequence("S", "Keys", Some(synth_dest)).unwrap();
    b.run(Command::Lane(LaneCmd::SetInput {
        seq,
        lane: keys,
        input: Some(LaneInput::Midi { port: Some("Keys".into()), channel: None }),
    }))
    .unwrap();
    b.run(Command::Lane(LaneCmd::SetFlags { seq, lane: keys, mute: None, solo: None, arm: Some(true) })).unwrap();
    b.run(Command::Sequence(SequenceCmd::SetLength { seq, length: SeqLength::Locked(Tick(32 * FQ)) })).unwrap();
    let s = Session::from_document(b.finish()).unwrap();
    let dir = tempfile::tempdir().unwrap();
    s.debug_set_trash_dir(Some(dir.path().display().to_string()));
    let synth = strip(&s, "Built-in Synth");
    s.set_automation_mode(vec![synth], Mode::Touch).unwrap();
    s.record(
        seq.0,
        vizdaw_ffi::RecordSpecRow {
            replace: true,
            punch: None,
            looped: false,
            count_in_bars: 0,
            pre_roll_bars: 0,
            passes_as_takes: true,
        },
    )
    .unwrap();
    wait_until("beat 1", || tick(&s) > Q);
    assert!(s.inject_midi("Keys".into(), vec![0x90, 60, 100]).unwrap());
    let t = Target::StripFader { strip: synth };
    s.gesture(t, Phase::Touch { value: Some(0.5) }).unwrap();
    pace(&s);
    s.gesture(t, Phase::Value { value: 0.6 }).unwrap();
    pace(&s);
    s.gesture(t, Phase::Release).unwrap();
    assert!(s.inject_midi("Keys".into(), vec![0x80, 60, 0]).unwrap());
    let at = tick(&s);
    wait_until("a beat on", || tick(&s) > at + Q);
    stop_and_land(&s);
    assert_eq!(s.snapshot().unwrap().undo_label.as_deref(), Some("Record"));
    assert!(lane_named(&s, seq.0, "Built-in Synth Fader").is_some());
    s.undo().unwrap();
    assert_eq!(lane_named(&s, seq.0, "Built-in Synth Fader"), None, "one Record step");
    s.close().unwrap();
}

/// §4.6, §7.5, Ruling 3: an Off strip plays no automation; suspended, it
/// does (the holder reloads at its next idle).
#[test]
fn off_ignores_automation_and_suspend_plays_it() {
    let _g = serial();
    let (s, song) = demo();
    let synth = strip(&s, "Built-in Synth");
    s.set_automation_mode(vec![synth], Mode::Touch).unwrap();
    s.play(song).unwrap();
    wait_until("beat 1", || tick(&s) > Q);
    let t = Target::StripFader { strip: synth };
    s.gesture(t, Phase::Touch { value: Some(fader_norm(-18.0)) }).unwrap();
    pace(&s);
    s.gesture(t, Phase::Release).unwrap();
    stop_and_land(&s);
    s.set_automation_mode(vec![synth], Mode::Off).unwrap();
    s.play(song).unwrap();
    wait_until("beat 1", || tick(&s) > Q);
    let start = Instant::now();
    while start.elapsed() < Duration::from_millis(300) {
        s.idle().unwrap();
        assert!(fader_override(synth).is_none(), "Off plays no automation");
        std::thread::sleep(Duration::from_millis(10));
    }
    set_automation_suspended(true);
    wait_until("suspended: the automation plays", || {
        s.idle().unwrap();
        fader_override(synth).is_some()
    });
    set_automation_suspended(false);
    stop_and_land(&s);
    s.close().unwrap();
}

/// Ruling 16, Review Focus 5: what cannot be gestured is refused and
/// changes nothing; a release with no touch does nothing.
#[test]
fn hostile_gestures_are_refused() {
    let _g = serial();
    let (s, _) = demo();
    let synth = strip(&s, "Built-in Synth");
    let version = s.snapshot().unwrap().version;
    let model = |r: Result<(), VizError>| assert!(matches!(r, Err(VizError::Model { .. })), "{r:?}");
    model(s.gesture(Target::StripFader { strip: 999 }, Phase::Touch { value: Some(0.5) }));
    model(s.gesture(Target::SendLevel { strip: synth, send: 7 }, Phase::Touch { value: Some(0.5) }));
    model(s.gesture(Target::PluginParam { slot: 999, param: 0 }, Phase::Touch { value: Some(0.5) }));
    model(s.gesture(Target::VelocityScale { dest: 0 }, Phase::Touch { value: Some(0.5) }));
    for v in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        model(s.gesture(Target::StripFader { strip: synth }, Phase::Touch { value: Some(v) }));
        model(s.gesture(Target::StripFader { strip: synth }, Phase::Value { value: v }));
    }
    s.gesture(Target::StripPan { strip: synth }, Phase::Release).unwrap();
    assert!(s.automation_state().unwrap().touched.is_empty());
    assert!(s.set_automation_mode(vec![], Mode::Touch).is_err());
    assert!(s.set_automation_mode(vec![synth, 999], Mode::Touch).is_err());
    assert_eq!(s.snapshot().unwrap().version, version, "nothing changed");
    s.close().unwrap();
}

/// Ruling 19: settings out of range are refused and change nothing.
#[test]
fn hostile_automation_settings_are_refused() {
    let _g = serial();
    let good = vizdaw_ffi::automation::automation_settings();
    assert_eq!(good, AutomationSettingsRow { after_write: Mode::Latch, return_ms: 250, thinning: ThinningRow::Medium });
    for bad in [
        AutomationSettingsRow { return_ms: 2001, ..good },
        AutomationSettingsRow { return_ms: u32::MAX, ..good },
        AutomationSettingsRow { after_write: Mode::Write, ..good },
    ] {
        assert!(matches!(set_automation_settings(bad), Err(VizError::Model { .. })), "{bad:?}");
        assert_eq!(vizdaw_ffi::automation::automation_settings(), good);
    }
    let ok = AutomationSettingsRow { after_write: Mode::Touch, return_ms: 0, thinning: ThinningRow::High };
    set_automation_settings(ok).unwrap();
    assert_eq!(vizdaw_ffi::automation::automation_settings(), ok);
}

/// Review Focus 2, Ruling 21: a device change mid-pass rebuilds the
/// engine; what was written up to then lands.
#[test]
fn a_device_rebuild_mid_pass_keeps_what_was_written() {
    let _g = serial();
    let (s, song) = demo();
    let synth = strip(&s, "Built-in Synth");
    s.set_automation_mode(vec![synth], Mode::Latch).unwrap();
    s.play(song).unwrap();
    wait_until("beat 1", || tick(&s) > Q);
    let t = Target::StripFader { strip: synth };
    s.gesture(t, Phase::Touch { value: Some(0.45) }).unwrap();
    pace(&s);
    s.gesture(t, Phase::Release).unwrap();
    pace(&s);
    simulate_event(BackendEvent::SampleRateChanged(44_100.0));
    wait_until("the write to land", || {
        let _ = s.transport();
        s.idle().unwrap();
        s.snapshot().unwrap().undo_label.as_deref() == Some("Write Automation")
    });
    let lane = lane_named(&s, song, "Built-in Synth Fader").unwrap();
    assert!(points(&s, song, lane).iter().any(|p| (p.1 - 0.45).abs() < 1e-9));
    let _ = s.stop();
    s.close().unwrap();
}

/// Pre-flight C1, Ruling 24: the idles that run right after Play, before
/// the renderer has rolled, do not end the pass; what is then written
/// lands at Stop.
#[test]
fn writing_survives_the_idle_before_the_roll() {
    let _g = serial();
    let (s, song) = demo();
    let synth = strip(&s, "Built-in Synth");
    s.set_automation_mode(vec![synth], Mode::Touch).unwrap();
    // The renderer is held, so these idles certainly run before the roll
    // (pre-flight check I-1).
    debug_hold_offline(true);
    s.play(song).unwrap();
    assert!(!s.transport().unwrap().playing, "not rolled yet");
    for _ in 0..3 {
        s.idle().unwrap();
    }
    assert!(s.automation_state().unwrap().writing, "still writing after the idles before the roll");
    debug_hold_offline(false);
    wait_until("beat 1", || tick(&s) > Q);
    let t = Target::StripFader { strip: synth };
    s.gesture(t, Phase::Touch { value: Some(0.5) }).unwrap();
    for v in [0.6, 0.7] {
        pace(&s);
        s.gesture(t, Phase::Value { value: v }).unwrap();
    }
    pace(&s);
    s.gesture(t, Phase::Release).unwrap();
    pace(&s);
    stop_and_land(&s);
    assert_eq!(s.snapshot().unwrap().undo_label.as_deref(), Some("Write Automation"));
    let lane = lane_named(&s, song, "Built-in Synth Fader").expect("the write landed");
    assert!(points(&s, song, lane).iter().any(|p| (p.1 - 0.7).abs() < 1e-9));
    s.close().unwrap();
}

/// Pre-flight C2, Ruling 23: a Locate while writing lands what was
/// written so far as its own step; writing goes on from the new position
/// as a new pass; each pass undoes on its own.
#[test]
fn a_locate_while_writing_lands_and_starts_a_new_pass() {
    let _g = serial();
    let (s, song) = demo();
    let synth = strip(&s, "Built-in Synth");
    s.set_automation_mode(vec![synth], Mode::Latch).unwrap();
    let t = Target::StripFader { strip: synth };
    s.play(song).unwrap();
    wait_until("beat 2", || tick(&s) > 2 * Q);
    s.gesture(t, Phase::Touch { value: Some(0.4) }).unwrap();
    pace(&s);
    s.gesture(t, Phase::Value { value: 0.5 }).unwrap();
    pace(&s);
    s.gesture(t, Phase::Release).unwrap();
    let at = tick(&s);
    wait_until("a beat on", || tick(&s) > at + Q);
    s.locate(0).unwrap();
    s.idle().unwrap();
    assert_eq!(s.snapshot().unwrap().undo_label.as_deref(), Some("Write Automation"));
    let lane = lane_named(&s, song, "Built-in Synth Fader").expect("the first pass landed");
    let first = points(&s, song, lane);
    assert!(first.iter().all(|p| p.0 >= Q), "nothing is written across the jump: {first:?}");
    assert!(s.automation_state().unwrap().writing, "a new pass from the new position");
    wait_until("the jump back", || tick(&s) < Q);
    s.gesture(t, Phase::Touch { value: Some(0.8) }).unwrap();
    pace(&s);
    s.gesture(t, Phase::Release).unwrap();
    pace(&s);
    stop_and_land(&s);
    assert_eq!(s.snapshot().unwrap().undo_label.as_deref(), Some("Write Automation"));
    assert!(points(&s, song, lane).iter().any(|p| (p.1 - 0.8).abs() < 1e-9));
    s.undo().unwrap();
    assert_eq!(points(&s, song, lane), first, "one step per pass");
    s.undo().unwrap();
    assert_eq!(lane_named(&s, song, "Built-in Synth Fader"), None, "both passes undone");
    s.close().unwrap();
}

/// Pre-flight check I-2, Ruling 23: a Locate that arrives after Play
/// returned but before the renderer rolled (a ruler click queued during
/// the disk wait) starts the new pass all the same.
#[test]
fn a_locate_before_the_roll_keeps_writing() {
    let _g = serial();
    let (s, song) = demo();
    let synth = strip(&s, "Built-in Synth");
    s.set_automation_mode(vec![synth], Mode::Latch).unwrap();
    debug_hold_offline(true);
    s.play(song).unwrap();
    s.locate(Q).unwrap();
    s.idle().unwrap();
    debug_hold_offline(false);
    wait_until("playing", || s.transport().unwrap().playing);
    assert!(s.automation_state().unwrap().writing, "a pass after the Locate");
    let t = Target::StripFader { strip: synth };
    wait_until("past beat 1", || tick(&s) > Q + Q / 4);
    s.gesture(t, Phase::Touch { value: Some(0.4) }).unwrap();
    pace(&s);
    s.gesture(t, Phase::Release).unwrap();
    pace(&s);
    stop_and_land(&s);
    assert_eq!(s.snapshot().unwrap().undo_label.as_deref(), Some("Write Automation"));
    let lane = lane_named(&s, song, "Built-in Synth Fader").expect("the write landed");
    assert!(points(&s, song, lane).iter().any(|p| (p.1 - 0.4).abs() < 1e-9));
    s.close().unwrap();
}

/// Pre-flight C2 (its third failure), Ruling 23: Play pressed again while
/// writing lands what was written (the new tap would drop it) and writes
/// on as a new pass; each pass undoes on its own.
#[test]
fn a_play_while_writing_lands_and_starts_a_new_pass() {
    let _g = serial();
    let (s, song) = demo();
    let synth = strip(&s, "Built-in Synth");
    s.set_automation_mode(vec![synth], Mode::Latch).unwrap();
    let t = Target::StripFader { strip: synth };
    s.play(song).unwrap();
    wait_until("beat 2", || tick(&s) > 2 * Q);
    s.gesture(t, Phase::Touch { value: Some(0.4) }).unwrap();
    pace(&s);
    s.gesture(t, Phase::Release).unwrap();
    let at = tick(&s);
    wait_until("a beat on", || tick(&s) > at + Q);
    s.play(song).unwrap();
    s.idle().unwrap();
    assert_eq!(s.snapshot().unwrap().undo_label.as_deref(), Some("Write Automation"));
    let lane = lane_named(&s, song, "Built-in Synth Fader").expect("the first pass landed");
    let first = points(&s, song, lane);
    assert!(first.iter().any(|p| (p.1 - 0.4).abs() < 1e-9), "{first:?}");
    assert!(s.automation_state().unwrap().writing, "a new pass");
    wait_until("playing", || s.transport().unwrap().playing);
    s.gesture(t, Phase::Touch { value: Some(0.8) }).unwrap();
    pace(&s);
    s.gesture(t, Phase::Release).unwrap();
    pace(&s);
    stop_and_land(&s);
    assert_eq!(s.snapshot().unwrap().undo_label.as_deref(), Some("Write Automation"));
    assert!(points(&s, song, lane).iter().any(|p| (p.1 - 0.8).abs() < 1e-9));
    s.undo().unwrap();
    assert_eq!(points(&s, song, lane), first, "one step per pass");
    s.undo().unwrap();
    assert_eq!(lane_named(&s, song, "Built-in Synth Fader"), None, "both passes undone");
    s.close().unwrap();
}

/// Pre-flight M2, Ruling 20 as amended: a touch held over Stop has its
/// path picked again at the next Play: its strip set to Read meanwhile,
/// its next value writes nothing and changes no static value.
#[test]
fn a_touch_held_over_stop_takes_its_strips_mode_at_the_next_play() {
    let _g = serial();
    let (s, song) = demo();
    let synth = strip(&s, "Built-in Synth");
    let gain = s.mixer().unwrap().strips.iter().find(|x| x.id == synth).unwrap().gain_db;
    s.set_automation_mode(vec![synth], Mode::Latch).unwrap();
    let t = Target::StripFader { strip: synth };
    s.play(song).unwrap();
    wait_until("beat 1", || tick(&s) > Q);
    s.gesture(t, Phase::Touch { value: Some(0.4) }).unwrap();
    pace(&s);
    stop_and_land(&s);
    assert_eq!(s.snapshot().unwrap().undo_label.as_deref(), Some("Write Automation"));
    let lane = lane_named(&s, song, "Built-in Synth Fader").unwrap();
    let landed = points(&s, song, lane);
    s.set_automation_mode(vec![synth], Mode::Read).unwrap();
    s.play(song).unwrap();
    wait_until("a beat on", || s.transport().unwrap().playing);
    pace(&s);
    s.gesture(t, Phase::Value { value: 0.6 }).unwrap();
    pace(&s);
    s.gesture(t, Phase::Release).unwrap();
    stop_and_land(&s);
    assert_eq!(s.snapshot().unwrap().undo_label.as_deref(), Some("Set Automation Mode"), "no step for the held touch");
    assert_eq!(points(&s, song, lane), landed, "nothing more written");
    let g2 = s.mixer().unwrap().strips.iter().find(|x| x.id == synth).unwrap().gain_db;
    assert_eq!(g2, gain, "and no static change");
    s.close().unwrap();
}

/// Pre-flight M4, Ruling 19: a setting changed mid-pass applies from the
/// next Play; the landing equals the preview taken before the change.
#[test]
fn settings_changed_mid_pass_apply_from_the_next_play() {
    let _g = serial();
    let (s, song) = demo();
    let synth = strip(&s, "Built-in Synth");
    s.set_automation_mode(vec![synth], Mode::Touch).unwrap();
    let t = Target::StripFader { strip: synth };
    s.play(song).unwrap();
    wait_until("beat 1", || tick(&s) > Q);
    // Steps of 0.15 dB: kept at Medium (0.1 dB), thinned away at High (0.2 dB).
    s.gesture(t, Phase::Touch { value: Some(fader_norm(-6.0)) }).unwrap();
    for k in 1..=6 {
        pace(&s);
        s.gesture(t, Phase::Value { value: fader_norm(-6.0 + 0.15 * f64::from(k % 2)) }).unwrap();
    }
    pace(&s);
    s.gesture(t, Phase::Release).unwrap();
    let at = tick(&s);
    wait_until("the glide to pass", || tick(&s) > at + Q);
    let preview = s.automation_preview(t).unwrap();
    assert!(preview.len() >= 6, "{preview:?}");
    let high = AutomationSettingsRow { thinning: ThinningRow::High, ..vizdaw_ffi::automation::automation_settings() };
    set_automation_settings(high).unwrap();
    stop_and_land(&s);
    let lane = lane_named(&s, song, "Built-in Synth Fader").unwrap();
    let landed = points(&s, song, lane);
    let want: Vec<(i64, f64)> = preview.iter().map(|p| (p.tick, p.value)).collect();
    assert_eq!(landed, want, "landed with the thinning read at Play");
    s.close().unwrap();
}

/// Pre-flight I1, Ruling 14: two static gestures at once (a fader and a
/// pan, stopped) share one edit: one undo step, named after the first
/// touch, and one Undo puts both back.
#[test]
fn two_static_gestures_at_once_are_one_step() {
    let _g = serial();
    let (s, _) = demo();
    let synth = strip(&s, "Built-in Synth");
    let row = |s: &Session| s.mixer().unwrap().strips.into_iter().find(|x| x.id == synth).unwrap();
    let before = row(&s);
    let (fader, pan) = (Target::StripFader { strip: synth }, Target::StripPan { strip: synth });
    s.gesture(fader, Phase::Touch { value: Some(0.5) }).unwrap();
    s.gesture(pan, Phase::Touch { value: Some(0.2) }).unwrap();
    pace(&s);
    s.gesture(fader, Phase::Value { value: 0.4 }).unwrap();
    s.gesture(pan, Phase::Value { value: 0.1 }).unwrap();
    s.gesture(pan, Phase::Release).unwrap();
    s.gesture(fader, Phase::Release).unwrap();
    assert_eq!(s.snapshot().unwrap().undo_label.as_deref(), Some("Set Fader"));
    let after = row(&s);
    assert!((after.gain_db - fader_db(0.4)).abs() < 1e-9, "{after:?}");
    assert!((after.pan - pan_of(0.1)).abs() < 1e-9, "{after:?}");
    s.undo().unwrap();
    let undone = row(&s);
    assert_eq!((undone.gain_db, undone.pan), (before.gain_db, before.pan), "one Undo restores both");
    assert_eq!(s.snapshot().unwrap().undo_label, None, "one step");
    s.close().unwrap();
}

/// Pre-flight I11, Ruling 26: a touch with no value starts at the value
/// heard now (here the bus fader's flat automation, 0.3), not at the
/// static value, so grabbing an automated fader writes no jump.
/// A session whose bus fader is automated flat at `v` in lane "Auto":
/// `(session, sequence, bus strip, lane)`.
fn automated_bus(v: f64) -> (Arc<Session>, u64, u64, u64) {
    use core_engine::fixtures::{Builder, Q as FQ};
    use core_model::automation::AutomationTarget;
    use core_model::event::EventBody;
    let mut b = Builder::new();
    let synth_dest = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, notes) = b.sequence("S", "Notes", Some(synth_dest)).unwrap();
    b.notes(seq, notes, &[(0, 60, 0x8000, 64 * FQ)]).unwrap();
    let bus = b.bus("Bus").unwrap();
    let auto = b.lane(seq, "Auto", None).unwrap();
    b.event(seq, auto, 0, EventBody::Automation { target: AutomationTarget::StripFader(bus), value: v }).unwrap();
    (Session::from_document(b.finish()).unwrap(), seq.0, bus.0, auto.0)
}

#[test]
fn a_touch_without_a_value_starts_at_the_heard_value() {
    let _g = serial();
    let (s, seq, bus, auto) = automated_bus(0.3);
    let t = Target::StripFader { strip: bus };
    assert!((fader_norm(0.0) - 0.3).abs() > 0.1, "the static value is not the automation's");
    s.set_automation_mode(vec![bus], Mode::Touch).unwrap();
    s.play(seq).unwrap();
    wait_until("beat 1", || tick(&s) > Q);
    s.gesture(t, Phase::Touch { value: None }).unwrap();
    pace(&s);
    s.gesture(t, Phase::Value { value: 0.6 }).unwrap();
    pace(&s);
    s.gesture(t, Phase::Release).unwrap();
    let at = tick(&s);
    wait_until("the glide to pass", || tick(&s) > at + Q);
    stop_and_land(&s);
    assert_eq!(s.snapshot().unwrap().undo_label.as_deref(), Some("Write Automation"));
    let p = points(&s, seq, auto);
    let moved = p.iter().position(|x| (x.1 - 0.6).abs() < 1e-9).expect("the move is written");
    assert!(p[..moved].iter().all(|x| (x.1 - 0.3).abs() < 1e-6), "no jump before the move: {p:?}");
    s.close().unwrap();
}

/// Pre-flight I12, Ruling 11: a Trim grab is sent as an offset of 0, so
/// the automation is heard unchanged until the hand moves (an absolute
/// touch value would play for a block first); a move of +3 dB adds 3 dB.
#[test]
fn a_trim_grab_does_not_jump() {
    let _g = serial();
    let (s, seq, bus, _) = automated_bus(fader_norm(-12.0));
    let t = Target::StripFader { strip: bus };
    s.set_automation_mode(vec![bus], Mode::Trim).unwrap();
    s.play(seq).unwrap();
    wait_until("beat 1", || tick(&s) > Q);
    s.gesture(t, Phase::Touch { value: Some(fader_norm(0.0)) }).unwrap();
    let start = Instant::now();
    while start.elapsed() < Duration::from_millis(50) {
        let d = fader_override(bus);
        assert!(d.is_some_and(|d| (d + 12.0).abs() < 1e-6), "the grab is heard as the automation: {d:?}");
        std::thread::sleep(Duration::from_millis(2));
    }
    pace(&s);
    s.gesture(t, Phase::Value { value: fader_norm(3.0) }).unwrap();
    wait_until("+3 dB on the automation", || fader_override(bus).is_some_and(|d| (d + 9.0).abs() < 1e-6));
    s.gesture(t, Phase::Release).unwrap();
    stop_and_land(&s);
    s.close().unwrap();
}

/// Pre-flight I6, Ruling 25: a target deleted mid-pass drops its own
/// write with a problem line; the rest of the pass lands.
#[test]
fn writes_to_a_target_deleted_mid_pass_are_dropped_and_the_rest_lands() {
    let _g = serial();
    let (s, song) = demo();
    s.add_bus("Gone".into()).unwrap();
    let synth = strip(&s, "Built-in Synth");
    let bus = strip(&s, "Gone");
    // The bus writes (Write) and becomes After Write at landing, unless
    // it is gone (Ruling 25: After Write leaves out deleted strips).
    s.set_automation_mode(vec![synth], Mode::Latch).unwrap();
    s.set_automation_mode(vec![bus], Mode::Write).unwrap();
    let targets = [Target::StripFader { strip: synth }, Target::StripFader { strip: bus }];
    s.play(song).unwrap();
    wait_until("beat 1", || tick(&s) > Q);
    for t in targets {
        s.gesture(t, Phase::Touch { value: Some(0.45) }).unwrap();
    }
    pace(&s);
    s.gesture(targets[0], Phase::Release).unwrap();
    pace(&s);
    s.remove_bus(bus).unwrap();
    // Pre-flight M9: the bus fader's touch goes with its strip.
    assert!(!s.automation_state().unwrap().touched.contains(&targets[1]));
    pace(&s);
    s.stop().unwrap();
    wait_until("the transport to stop", || !s.transport().unwrap().playing);
    let news = s.idle().unwrap();
    assert_eq!(s.snapshot().unwrap().undo_label.as_deref(), Some("Write Automation"));
    let lane = lane_named(&s, song, "Built-in Synth Fader").expect("the synth's write landed");
    assert!(points(&s, song, lane).iter().any(|p| (p.1 - 0.45).abs() < 1e-9));
    assert_eq!(lane_named(&s, song, "Gone Fader"), None);
    assert!(
        news.problems.iter().any(|p| p.contains("was not written: it no longer exists")),
        "{:?}",
        news.problems
    );
    let modes = s.automation_state().unwrap().modes;
    assert!(modes.iter().any(|m| m.strip == synth && m.mode == Mode::Latch), "{modes:?}");
    assert!(modes.iter().all(|m| m.strip != bus));
    s.close().unwrap();
}

/// Ruling 25, Ruling 28 (pre-flight I6, M16): a recording that records
/// nothing (its punch range is never reached) still lands the automation
/// written meanwhile, as "Write Automation", outside the punch range.
#[test]
fn writes_land_when_the_recording_records_nothing() {
    use core_engine::fixtures::{Builder, Q as FQ};
    use core_model::command::{Command, LaneCmd, SequenceCmd};
    use core_model::lane::LaneInput;
    use core_model::sequence::SeqLength;
    use core_model::time::Tick;
    let _g = serial();
    let mut b = Builder::new();
    let synth_dest = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, keys) = b.sequence("S", "Keys", Some(synth_dest)).unwrap();
    b.run(Command::Lane(LaneCmd::SetInput {
        seq,
        lane: keys,
        input: Some(LaneInput::Midi { port: Some("Keys".into()), channel: None }),
    }))
    .unwrap();
    b.run(Command::Lane(LaneCmd::SetFlags { seq, lane: keys, mute: None, solo: None, arm: Some(true) })).unwrap();
    b.run(Command::Sequence(SequenceCmd::SetLength { seq, length: SeqLength::Locked(Tick(32 * FQ)) })).unwrap();
    let s = Session::from_document(b.finish()).unwrap();
    let dir = tempfile::tempdir().unwrap();
    s.debug_set_trash_dir(Some(dir.path().display().to_string()));
    let synth = strip(&s, "Built-in Synth");
    s.set_automation_mode(vec![synth], Mode::Touch).unwrap();
    let punch = vizdaw_ffi::selection::TickRange { start: 16 * Q, end: 17 * Q };
    s.record(
        seq.0,
        vizdaw_ffi::RecordSpecRow {
            replace: true,
            punch: Some(punch),
            looped: false,
            count_in_bars: 0,
            pre_roll_bars: 0,
            passes_as_takes: true,
        },
    )
    .unwrap();
    wait_until("beat 1", || tick(&s) > Q);
    let t = Target::StripFader { strip: synth };
    s.gesture(t, Phase::Touch { value: Some(0.5) }).unwrap();
    pace(&s);
    s.gesture(t, Phase::Value { value: 0.6 }).unwrap();
    pace(&s);
    s.gesture(t, Phase::Release).unwrap();
    let at = tick(&s);
    wait_until("a beat on", || tick(&s) > at + Q);
    stop_and_land(&s);
    assert_eq!(s.snapshot().unwrap().undo_label.as_deref(), Some("Write Automation"));
    let lane = lane_named(&s, seq.0, "Built-in Synth Fader").expect("the automation landed");
    let p = points(&s, seq.0, lane);
    assert!(p.iter().any(|x| (x.1 - 0.6).abs() < 1e-9), "{p:?}");
    assert!(p.iter().all(|x| x.0 < punch.start), "written outside the punch range: {p:?}");
    s.close().unwrap();
}
VIZDAW_EOF
```

The `pace` sleeps only space the gesture samples past the 10 ms sampler; every result is waited for with `wait_until` (`common::wait_limit()`).

In `crates/core-ffi/src/automation.rs`'s `#[cfg(test)]` module (written in Step 3), the sampler's, the pass end's and the unsent ends' unit tests:

```rust
    /// Ruling 8: at most one value per 10 ms; the last kept value is sent
    /// once 10 ms passed, or first at the release, with the host time it
    /// arrived at (pre-flight M6).
    #[test]
    fn values_are_sent_at_most_every_10_ms_and_the_last_is_kept() {
        let t = AutomationTarget::StripFader(StripId(1));
        let ms = Duration::from_millis;
        let mut s = Sampler::default();
        assert_eq!(s.value(t, 0.1, ms(0), 100), Some(0.1));
        assert_eq!(s.value(t, 0.2, ms(4), 104), None);
        assert_eq!(s.value(t, 0.3, ms(8), 108), None);
        assert!(s.due(ms(9)).is_empty());
        assert_eq!(s.due(ms(10)), vec![(t, 0.3, 108)]);
        assert!(s.due(ms(30)).is_empty(), "sent once");
        assert_eq!(s.value(t, 0.4, ms(31), 131), Some(0.4));
        assert_eq!(s.value(t, 0.5, ms(33), 133), None);
        assert_eq!(s.flush(t), Some((0.5, 133)));
        assert_eq!(s.flush(t), None);
    }

    /// Ruling 24 (pre-flight C1, M17): a pass ends when its tap closed, or
    /// once it has rolled and the transport no longer reads playing for
    /// this session; never before the roll.
    #[test]
    fn a_pass_ends_only_after_its_roll() {
        for (closed, rolled, stopped, ended) in [
            (false, false, Some(true), false), // right after Play: still reads stopped
            (false, false, None, true),        // another session took the engine before the roll
            (false, false, Some(false), false),
            (false, true, Some(false), false), // playing
            (false, true, Some(true), true),   // stopped by itself
            (false, true, None, true),         // another session took the engine
            (true, false, Some(true), true),   // the tap went with the engine
            (true, true, Some(false), true),
        ] {
            assert_eq!(writing_ended(closed, rolled, stopped), ended, "{closed} {rolled} {stopped:?}");
        }
    }

    /// Pre-flight I3, Ruling 10: an end the queue refused is re-sent until
    /// it goes; a later message sent for its target replaces it.
    #[test]
    fn an_unsent_end_is_resent_until_it_goes_and_a_later_message_replaces_it() {
        use core_engine::gesture::{GestureMsg, GesturePhase};
        use core_engine::schedule::AutoDest;
        let (t, u) = (AutomationTarget::StripFader(StripId(1)), AutomationTarget::StripPan(StripId(1)));
        let msg = |x: AutomationTarget, phase| GestureMsg { dest: AutoDest::of(x).unwrap(), phase, host_ns: 0, tag: 2 };
        let mut q = UnsentEnds::default();
        q.put(t, msg(t, GesturePhase::Hold));
        q.put(u, msg(u, GesturePhase::Release));
        let mut tried = Vec::new();
        q.resend(|m| {
            tried.push(m.phase);
            false
        });
        assert_eq!((tried.len(), q.len()), (2, 2), "both kept while the queue is full");
        q.clear(u); // a later message for the pan went through
        tried.clear();
        q.resend(|m| {
            tried.push(m.phase);
            true
        });
        assert_eq!(tried, vec![GesturePhase::Hold]);
        assert_eq!(q.len(), 0);
        q.resend(|_| panic!("nothing is left"));
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-ffi --test automation 2>&1 | tail -3
```

Expected: FAIL to compile (`vizdaw_ffi::automation` does not exist). Each new behaviour is RED for its own reason once the module compiles with the pre-revision lifecycle (record it if you build in that order): `writing_survives_the_idle_before_the_roll` ends the pass at the first idle; `a_locate_while_writing_lands_and_starts_a_new_pass` lands nothing at the Locate; `two_static_gestures_at_once_are_one_step` gets `Busy`; `a_touch_without_a_value_starts_at_the_heard_value` writes the static value first; `writes_to_a_target_deleted_mid_pass_are_dropped_and_the_rest_lands` lands nothing; `a_stop_while_touched_lands_the_write_and_keeps_the_touch` hears 0.4 at the next Play (the late release's Hold, with nothing driving the fader); `a_locate_before_the_roll_keeps_writing` and `a_play_while_writing_lands_and_starts_a_new_pass` write nothing (or lose the first pass); `a_touch_held_over_stop_takes_its_strips_mode_at_the_next_play` writes 0.6; `settings_changed_mid_pass_apply_from_the_next_play` lands fewer points than the preview; `a_trim_grab_does_not_jump` hears 0 dB; `writes_land_when_the_recording_records_nothing` lands nothing.

- [ ] **Step 3: Rows, settings, the sampler and the helpers**

Create `crates/core-ffi/src/automation.rs` (module doc: "Automation recording in the session (sub-project 5 spec §7; plan 5e-1 Rulings 3, 7–10, 14–22): rows, the process-wide settings and suspend, the 10 ms sampler, and the helpers the session's gesture path and landing use.") with the rows, the four settings exports (statics: `AFTER_WRITE: AtomicU8` (Latch), `RETURN_MS: AtomicU32` (250), `THINNING: AtomicU8` (Medium), `SUSPENDED: AtomicBool`; `set_automation_suspended` calls `crate::audio::bump_epoch()` on a change; the infallible exports wrap their bodies in `catch_unwind`), `pub(crate) fn suspended() -> bool`, `reset_automation_settings` (test hook) and the internal helpers of the Interfaces block (`Source`, `writing_ended`, `Sampler`, `lane_name`, `existing`, `value_at`, `fixed_value`, `static_command`). Re-export the rows from `crates/core-ffi/src/lib.rs` (`pub use automation::{AutomationModeRow, AutomationSettingsRow, AutomationStateRow, GesturePhaseRow, PreviewPointRow, StripModeRow, ThinningRow};`). In `crates/core-ffi/src/audio.rs` add `pub(crate) fn bump_epoch()` (one `EPOCH.fetch_add`).

In `crates/core-ffi/src/mixer.rs`:

Find:
```rust
    /// Outputs of its multi-output instrument that have no strip yet
    /// ("Add Outputs…").
    pub free_outputs: Vec<u16>,
}
```
Replace:
```rust
    /// Outputs of its multi-output instrument that have no strip yet
    /// ("Add Outputs…").
    pub free_outputs: Vec<u16>,
    /// Its automation mode (sub-project 5 spec §4.6).
    pub automation: crate::automation::AutomationModeRow,
}
```
and set it from `s.automation` in `strip_row`'s literal. In `FakeSession.swift`'s `stripRow(…)`, pass `automation: .read` after `freeOutputs: []`.

- [ ] **Step 4: The host's three calls**

In `crates/core-ffi/src/host.rs`, after `midi_sender`, add `gesture`, `start_gestures`, `stop_gestures`, `heard` and `gestures_dropped` with the same holder check (`h.owner == owner`): `gesture` sends through `h.engine.gesture_sender()` (false when another session holds the engine or the queue is full); `start_gestures` returns `h.engine.start_gestures().ok()`; `stop_gestures` ignores a full queue (the tap goes with the engine anyway); `heard` reads `h.engine.overrides()?.value(AutoDest::of(t)?)`; `gestures_dropped` answers `h.engine.gestures_dropped()`. None of them is `cfg`-gated: they build and are linted on Linux. Add `static OFFLINE_HOLD: AtomicBool` (not gated) and the gated setter `debug_hold_offline`; `OfflineDriver`'s loop, while it is set, renders nothing and moves its clock origin on by the time held, so the held time is skipped, not caught up in one burst at the release. `finish_play`'s `pending.wait()` is the disk prefetch, which does not need the render thread; if it turns out to wait on the renderer (`play` then never returns while held), stop and report it rather than weaken the tests. `serial()` in the tests resets it. In `Host::load`:

Find:
```rust
        self.engine.set_return_dests(return_dests(doc, seq));
```
Replace:
```rust
        self.engine.set_return_dests(return_dests(doc, seq));
        // Off strips play no automation unless it is suspended (plan 5e-1
        // Ruling 3).
        self.engine
            .set_automation_suspended(crate::automation::suspended());
```

- [ ] **Step 5: The session**

In `crates/core-ffi/src/session.rs`, add the `Inner` fields of "How the session does it" (after `save_after_pass: bool,` and `save_after_pass: false,`), then:

Find (in `start_playing`):
```rust
        let id = seq_id(inner.doc(), seq)?;
        let start = start.or(match (inner.cursor, inner.cursor_seq) {
```
Replace:
```rust
        // A Play while a pass writes lands it first (plan 5e-1 Ruling 23):
        // the new position is a new pass.
        self.finish_writing(&mut inner);
        self.apply_pending_automation(&mut inner);
        let id = seq_id(inner.doc(), seq)?;
        let start = start.or(match (inner.cursor, inner.cursor_seq) {
```

Find:
```rust
        inner.cursor = None;
        inner.cursor_seq = Some(id);
        // The disk wait (up to 250 ms) holds no lock: this session's
        // transport poll and every other session go on meanwhile.
        drop(inner);
        host::finish_play(pending)
```
Replace:
```rust
        inner.cursor = None;
        inner.cursor_seq = Some(id);
        // The gesture tap goes in before the Play, so the roll is its
        // first mark (plan 5e-1 Ruling 7); Write strips' controls are held
        // from the roll (Ruling 9).
        self.begin_writing(&mut inner, id);
        // The disk wait (up to 250 ms) holds no lock: this session's
        // transport poll and every other session go on meanwhile.
        drop(inner);
        let played = host::finish_play(pending);
        if played.is_err() {
            // A Play that did not start writes nothing (Ruling 24).
            let mut inner = lock(&self.inner);
            self.finish_writing(&mut inner);
            self.apply_pending_automation(&mut inner);
        }
        played
```

Find (in `start_recording`):
```rust
        // Review I-1: the take would be planned at the old rate and land
```
Replace:
```rust
        // A Record while a pass writes lands it first (plan 5e-1 Ruling
        // 23), before the recording is planned on the document.
        self.finish_writing(&mut inner);
        self.apply_pending_automation(&mut inner);
        // Review I-1: the take would be planned at the old rate and land
```

Find (in `start_recording`):
```rust
        inner.cursor = None;
        inner.cursor_seq = Some(id);
        drop(inner);
        // A Play that cannot be sent ends the recording it started
```
Replace:
```rust
        inner.cursor = None;
        inner.cursor_seq = Some(id);
        self.begin_writing(&mut inner, id);
        drop(inner);
        // A Play that cannot be sent ends the recording it started
```

Find (in `stop`):
```rust
            let _inner = lock(&self.inner);
            host::stop(self.id)
```
Replace:
```rust
            let mut inner = lock(&self.inner);
            let stopped = host::stop(self.id);
            // What was written lands now (spec §7.4).
            self.finish_writing(&mut inner);
            self.apply_pending_automation(&mut inner);
            stopped
```

Find (in `locate`):
```rust
            if !host::locate(self.id, Tick(tick))? {
                inner.cursor = Some(Tick(tick));
                inner.cursor_seq = inner.selected_seq();
            }
            Ok(())
```
Replace:
```rust
            // A Locate while a pass writes lands it; the new position is a
            // new pass (plan 5e-1 Ruling 23).
            let writing = inner.writing.as_ref().map(|w| (w.seq, w.handles.status.rolled()));
            self.finish_writing(&mut inner);
            self.apply_pending_automation(&mut inner);
            if !host::locate(self.id, Tick(tick))? {
                inner.cursor = Some(Tick(tick));
                inner.cursor_seq = inner.selected_seq();
            }
            // Again from the new position: when it plays, or when the old
            // pass had not rolled yet (check I-2: the Play is still on its
            // way to the renderer).
            let stopped = host::transport_stopped(self.id);
            if let Some((seq, _)) = writing.filter(|(_, rolled)| stopped == Some(false) || (!rolled && stopped.is_some())) {
                self.begin_writing(&mut inner, seq);
            }
            Ok(())
```

Find (in `finish_recording`):
```rust
        inner.pending_pass = Some((pass, files));
```
Replace:
```rust
        // Automation written while it recorded joins the pass (spec §7.4).
        let pass = self.writes_into_pass(&mut inner, pass);
        inner.pending_pass = Some((pass, files));
```

Find (in `apply_pending_pass`):
```rust
                Err(e) => {
                    inner.news.problems.push(format!(
                        "The recording could not be added ({e}). Its files are kept."
                    ));
                    inner.keep_recordings.extend(files);
                }
```
Replace:
```rust
                Err(e) => {
                    inner.news.problems.push(format!(
                        "The recording could not be added ({e}). Its files are kept."
                    ));
                    inner.keep_recordings.extend(files);
                    // Its automation still lands, inside the group (plan
                    // 5e-1 Ruling 25).
                    self.land_automation_alone(inner, automation);
                }
```
and, before that `match`, after `let mutes = crate::synth::mutes_after(&p, &inner.synth_mutes);`, add `let automation = p.automation.clone();` (`land_automation_alone` executes `RecordPass { automation, ..Default::default() }` when it is not empty, with the problem line "The automation could not be written ({e})." on a refusal, and records whether automation landed for `after_write_in_group`).

Find (in `apply_pending_pass`):
```rust
        let _ = inner.history.end_group(label);
```
Replace:
```rust
        // Write strips become the After Write mode in the same step (plan
        // 5e-1 Ruling 9).
        self.after_write_in_group(inner);
        let _ = inner.history.end_group(label);
```

Find (in `idle`):
```rust
            self.service_recording()?;
```
Replace:
```rust
            self.service_recording()?;
            self.service_writing();
```

and after the `self.apply_pending_pass(&mut inner);` that follows in `idle` (`session.rs:1608`, not the one in `finish_recording`), and after `self.commit_held(&mut inner);` in `end_edit` (`session.rs:1834`, not the other three at 2901, 2909 and 2967), call `self.apply_pending_automation(&mut inner);`. In `close`, Find:
```rust
            // (Task 9 parked m3; a failure is printed below).
            self.save_after_pass_now(&mut inner);
```
Replace:
```rust
            // (Task 9 parked m3; a failure is printed below).
            self.save_after_pass_now(&mut inner);
            // Touches end and a pass in progress lands (plan 5e-1 Ruling 20).
            self.release_touches(&mut inner);
            self.finish_writing(&mut inner);
            self.apply_pending_automation(&mut inner);
```

In `Session::execute`'s success path, in `undo` and in `redo`, call `self.release_vanished(&mut inner)` (M9; a landing calls it too). Write the private methods named above (`begin_writing`, `service_writing`, `take_writes`, `finish_writing`, `writes_into_pass`, `after_write_in_group`, `land_automation_alone`, `apply_pending_automation`, `release_touches`, `release_vanished`, `resend_ends`, `sync_written_plugins`, and the gesture path's `touch`/`move_to`/`release` with their `source` and `host_ns`) as "How the session does it" says, and the four `#[uniffi::export]` methods in a new `impl Session` block titled "Automation recording (plan 5e-1; sub-project 5 spec §7)", placed after the synth-tracking block.

In `FakeSession.swift` add, under a `// MARK: Automation (plan 5e-1)` heading: `var gestures: [(AutomationTargetRow, GesturePhaseRow)] = []`, `var gestureError: VizError?`, `var automationRow = AutomationStateRow(modes: [], touched: [], suspended: false, writing: false)`, `var modeCalls: [([UInt64], AutomationModeRow)] = []`, `var previewRows: [PreviewPointRow] = []`, and the four methods (`gesture` throws `gestureError` or records; `setAutomationMode` records and returns `commandResult()`; `automationState` returns the row; `automationPreview` returns `previewRows`).

- [ ] **Step 6: Run the tests to verify they pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -2
cargo clippy --workspace --all-targets --target x86_64-unknown-linux-gnu --target-dir target/linux-check -- -D warnings 2>&1 | tail -2
cargo clippy -p core-ffi --lib -- -D warnings 2>&1 | tail -2
cargo test --workspace --no-fail-fast 2>&1 | grep -E "test result|FAILED|panicked" | grep -v " 0 failed"; echo "workspace done"
scripts/build-ffi.sh && scripts/swift.sh build 2>&1 | grep -E "error:|warning:" | grep -v MemoryDefaults | sort -u
scripts/swift.sh test 2>&1 | grep -E "Test run"
```

Expected: "workspace done" with no failure lines (Rust 1524: `automation` 20 and the three unit tests); Swift 846 tests pass.

- [ ] **Step 7: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-ffi/src/automation.rs crates/core-ffi/src/lib.rs crates/core-ffi/src/audio.rs crates/core-ffi/src/host.rs \
  crates/core-ffi/src/session.rs crates/core-ffi/src/mixer.rs crates/core-ffi/tests/automation.rs app/Tests/VizDAWAppModelTests/FakeSession.swift
git commit -m "core-ffi: gestures, writing while playing and the landing at Stop; automation settings, suspend, state and preview (5e-1 Task 8)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -3
```

### Task 9: Plugin windows: every parameter event from the host, the 500 ms release, no parameter step while writing

**Files:**
- Modify: `crates/core-host/src/host.rs` (`IdleReport.param_events`, collected with the gestures; an `End` for every gesture the host closes itself)
- Modify: `crates/core-ffi/src/automation.rs` (`PluginTouches`, `PluginMove`), `crates/core-ffi/src/session.rs` (`pump` forwards the events with `Source::Plugin` and strips written parameters from the gestures; `service_writing` releases idle value-only touches and touches of slots that went)
- Test: `crates/core-host/tests/host.rs`, `crates/core-ffi/tests/params.rs`

**Interfaces:**
- Consumes: `ParamEvent` (3b), `PluginInstance::info().params` (`ParamInfo { id, name, min, max, default, stepped, … }`), Task 8's gesture path (`touch`/`move_to`/`release` with `Source::Plugin` and a host time), `existing` (Ruling 27's "automated"), `sync_written_plugins`.
- Produces:
  - `IdleReport.param_events: Vec<(PluginInstanceId, ParamEvent)>` ("Every parameter event the plugins reported, in order, beside the grouped gestures, with an `End` for each parameter of a gesture the host closed itself (plan 5e-1 Ruling 15)."), merged like `gestures`; every event `collect_gestures` reads is appended to a host-wide `raw` in the order read, from `idle`, `service` and a closed window's `end_gestures` (held in the host until the next report, as `ended` is).
  - `crate::automation::PluginMove { Begin, Value(f64), End }` and `PluginTouches` with `pub(crate) const IDLE: Duration = 500 ms`, `fn event(&mut self, t: AutomationTarget, m: PluginMove, now: Duration, automated: bool) -> Vec<Move>`, `fn idle(&mut self, now: Duration) -> Vec<(AutomationTarget, Duration)>` (each value-only touch whose last value is 500 ms old, with its release time `last + 500 ms`) and `fn release_slots(&mut self, live: impl Fn(PluginInstanceId) -> bool) -> Vec<AutomationTarget>` (forgets and answers the touches of slots `live` denies), `Move { Touch(Option<f64>), Value(f64), Release }`. Begin touches (`Touch(None)`: the plugin's value now, Ruling 26) and marks the touch as gesture-driven (no idle release); a Value with no touch touches at that value (value-driven), except when `automated` (Ruling 27: an echo of playback, nothing); End releases; a second Begin while touched is ignored.

Ruling 15 as amended (pre-flight I7, I8, I9, M12, M13), Ruling 27.
- **The host's own ends** (I7). A helper `fn end_open(id: PluginInstanceId, l: &Live, raw: &mut Vec<(PluginInstanceId, ParamEvent)>)` pushes `(id, ParamEvent::End(p))` for every `p` in `l.open` (its `BTreeSet` order). It is called wherever the host closes a gesture itself, before `open` is cleared or the `Live` goes: the `GESTURE_TIMEOUT` flush in `service_instance`, `end_gestures`, `fail`, the restart path and a slot `sync` removes. Nothing changes for the grouped `gestures`.
- **Forwarding.** A plugin's own gesture on the static path changes nothing in the document by itself: the host's grouped `Gesture` is its undo step, as in 3b. In `pump`, before the gestures are held for commit, each `(slot, event)` becomes a `PluginParam(slot, id)` target, its value normalised with the parameter's range (`(v − min) / (max − min)`, clamped; a parameter the instance does not list is skipped). It goes through `PluginTouches::event` with `automated` = this session holds the engine and `existing(doc, playing seq, t)` is not empty (Ruling 27), into the gesture path with `Source::Plugin` and `host_ns` = the drain time (`clock::now_ns()` live, 0 offline; up to one 60 Hz tick late, M12). With `Source::Plugin` the path is `Write` when the strip writes, else `Live`: the engine gets the live value (automation pauses while the knob is held), no edit is opened, nothing is `set_param`'d back and nothing is committed (I9).
- **Mixed gestures** (M13). Each finished `Gesture` loses the values of parameters that were on a writing path (touched as a write in this report, or still touched as one); it is dropped when none remain, else held and committed as before.
- **Releases.** In `service_writing` (and in `idle` while not writing), `PluginTouches::idle` releases value-only touches, the release stamped at its time on a live device, and `release_slots` releases the touches of slots that are no longer `SlotStatus::Live` in this session (I7).
- After landing, Task 8's `sync_written_plugins` sends each written plugin parameter the landed value (M13), whatever the source.

- [ ] **Step 1: Write the failing tests**

In `crates/core-host/tests/host.rs`, after `unpaired_gesture_events_still_become_gestures`, add:

```rust
/// Plan 5e-1 Ruling 15: beside the grouped gestures, the host reports
/// every parameter event in the order the plugin sent it, from the main
/// thread (the path an AU's listener takes) and from the audio side (a
/// CLAP plugin's output events).
#[test]
fn idle_reports_every_param_event_in_order() {
    let (hist, song, id, mut host, script) = with_fake();
    let (mut engine, mut be) = engine(&host, 4096);
    engine.load(hist.doc(), song).unwrap();
    script.emit(&[
        ParamEvent::Begin(GAIN),
        ParamEvent::Value(GAIN, 0.5),
        ParamEvent::End(GAIN),
        ParamEvent::Value(OTHER, 3.0),
    ]);
    let r = tick(&mut host, &mut engine, &mut be, &hist, song);
    assert_eq!(
        r.param_events,
        vec![
            (id, ParamEvent::Begin(GAIN)),
            (id, ParamEvent::Value(GAIN, 0.5)),
            (id, ParamEvent::End(GAIN)),
            (id, ParamEvent::Value(OTHER, 3.0)),
        ]
    );
    assert_eq!(r.gestures.len(), 2, "the gestures are grouped as before");
    drop(engine);
    // The CLAP test plugin's "Wiggle": a gesture from its process call.
    let (hist, song, gain) = with_gain();
    let mut host = host(4096);
    host.sync(hist.doc());
    host.wait_ready(WAIT);
    let (mut engine, mut be) = engine(&host, 4096);
    engine.load(hist.doc(), song).unwrap();
    host.set_param(gain, ParamId(2), 0.4);
    peak(&mut be, 512);
    let r = host.service(&mut engine, hist.doc(), song).unwrap();
    assert_eq!(
        r.param_events,
        vec![
            (gain, ParamEvent::Begin(ParamId(0))),
            (gain, ParamEvent::Value(ParamId(0), 0.4)),
            (gain, ParamEvent::End(ParamId(0))),
        ]
    );
    drop(engine);
}

/// Pre-flight I7, plan 5e-1 Ruling 15: a gesture the host closes by
/// itself (its End lost and the timeout passed, or its editor closed)
/// reports an End for its open parameter, so the session's touch is
/// released.
#[test]
fn a_gesture_the_host_closes_reports_its_end() {
    let (mut hist, song, id, mut host, script) = with_fake();
    host.set_gesture_timeout(Duration::from_millis(50));
    let (mut engine, mut be) = engine(&host, 4096);
    engine.load(hist.doc(), song).unwrap();
    script.emit(&[ParamEvent::Begin(GAIN), ParamEvent::Value(GAIN, 0.5)]);
    let r = tick(&mut host, &mut engine, &mut be, &hist, song);
    assert_eq!(r.param_events, vec![(id, ParamEvent::Begin(GAIN)), (id, ParamEvent::Value(GAIN, 0.5))]);
    std::thread::sleep(Duration::from_millis(60));
    let r = tick(&mut host, &mut engine, &mut be, &hist, song);
    assert_eq!(r.param_events.last(), Some(&(id, ParamEvent::End(GAIN))), "{:?}", r.param_events);
    script.emit(&[ParamEvent::Begin(GAIN), ParamEvent::Value(GAIN, 0.25)]);
    let _ = tick(&mut host, &mut engine, &mut be, &hist, song);
    host.end_gestures(id);
    let r = tick(&mut host, &mut engine, &mut be, &hist, song);
    assert_eq!(r.param_events.last(), Some(&(id, ParamEvent::End(GAIN))), "{:?}", r.param_events);
    // A slot removed while its gesture is open (check m9).
    script.emit(&[ParamEvent::Begin(GAIN), ParamEvent::Value(GAIN, 0.75)]);
    let _ = tick(&mut host, &mut engine, &mut be, &hist, song);
    run(&mut hist, MixerCmd::RemoveInsert { slot: id });
    host.sync(hist.doc());
    let r = tick(&mut host, &mut engine, &mut be, &hist, song);
    assert_eq!(r.param_events.last(), Some(&(id, ParamEvent::End(GAIN))), "{:?}", r.param_events);
    drop(engine);
}
```

In `crates/core-ffi/tests/params.rs`, append:

```rust
/// Spec §7.1, plan 5e-1 Ruling 15: a gesture in a CLAP plugin's own
/// window while its strip writes (Touch) becomes automation of that
/// parameter, normalised by its range (Gain 0–2: 0.4 is 0.2), in a lane
/// named after it, and no "Change Plugin Parameter" step; in Read it is
/// still one.
#[test]
fn a_plugin_windows_gesture_writes_its_parameter() {
    use vizdaw_ffi::automation::AutomationModeRow;
    let _g = serial();
    let (_d, s, id) = playing_gain();
    let synth = s.mixer().unwrap().strips[0].id;
    let song = s.transport().unwrap().seq.unwrap();
    s.set_automation_mode(vec![synth], AutomationModeRow::Touch).unwrap();
    s.set_param(id, WIGGLE, 0.4).unwrap();
    let t0 = Instant::now();
    idle_until(&s, |_| t0.elapsed() > Duration::from_millis(400));
    s.stop().unwrap();
    idle_until(&s, |s| s.snapshot().unwrap().undo_label.as_deref() == Some("Write Automation"));
    let lane = s
        .lanes(song)
        .unwrap()
        .into_iter()
        .find(|l| l.name == "Built-in Synth Gain")
        .expect("a lane named after the parameter");
    let values: Vec<f64> = s
        .lane_events(song, lane.id)
        .unwrap()
        .events
        .iter()
        .filter_map(|e| e.automation.map(|a| a.value))
        .collect();
    assert!(values.iter().any(|v| (v - 0.2).abs() < 1e-9), "{values:?}");
    s.undo().unwrap();
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Set Automation Mode"),
        "no parameter step under the write"
    );
    s.set_automation_mode(vec![synth], AutomationModeRow::Read).unwrap();
    s.play(song).unwrap();
    s.set_param(id, WIGGLE, 0.8).unwrap();
    idle_until(&s, |s| s.snapshot().unwrap().undo_label.as_deref() == Some("Change Plugin Parameter"));
    s.stop().unwrap();
}

/// Pre-flight I9, Ruling 15 (and M13, M14): a gesture from the app on a
/// plugin parameter (plan 5e-2's sliders) reaches the plugin on every
/// path. While writing it is automation and no parameter step, and after
/// landing the plugin plays the landed value (the glide back: its value
/// before); stopped, it is one "Change Plugin Parameter" step. Gain is
/// 0–2, so 0.3 is 0.6.
#[test]
fn a_ui_gesture_on_a_plugin_parameter_reaches_the_plugin() {
    use vizdaw_ffi::AutomationTargetRow as Target;
    use vizdaw_ffi::automation::{AutomationModeRow, GesturePhaseRow as Phase};
    let _g = serial();
    let (_d, s, id) = playing_gain();
    let synth = s.mixer().unwrap().strips[0].id;
    let before = param(&s, id, GAIN).value;
    s.set_automation_mode(vec![synth], AutomationModeRow::Touch).unwrap();
    let t = Target::PluginParam { slot: id, param: GAIN };
    s.gesture(t, Phase::Touch { value: Some(0.25) }).unwrap();
    std::thread::sleep(Duration::from_millis(15));
    s.gesture(t, Phase::Value { value: 0.3 }).unwrap();
    idle_until(&s, |s| (param(s, id, GAIN).value - 0.6).abs() < 1e-6);
    s.gesture(t, Phase::Release).unwrap();
    let t0 = Instant::now();
    idle_until(&s, |_| t0.elapsed() > Duration::from_millis(400));
    s.stop().unwrap();
    idle_until(&s, |s| s.snapshot().unwrap().undo_label.as_deref() == Some("Write Automation"));
    idle_until(&s, |s| (param(s, id, GAIN).value - before).abs() < 1e-6);
    s.undo().unwrap();
    assert_eq!(s.snapshot().unwrap().undo_label.as_deref(), Some("Set Automation Mode"), "no parameter step");
    // Stopped: the static path, one step.
    s.gesture(t, Phase::Touch { value: Some(0.25) }).unwrap();
    std::thread::sleep(Duration::from_millis(15));
    s.gesture(t, Phase::Value { value: 0.25 }).unwrap();
    s.gesture(t, Phase::Release).unwrap();
    assert_eq!(s.snapshot().unwrap().undo_label.as_deref(), Some("Change Plugin Parameter"));
    idle_until(&s, |s| (param(s, id, GAIN).value - 0.5).abs() < 1e-6);
    s.undo().unwrap();
    idle_until(&s, |s| (param(s, id, GAIN).value - before).abs() < 1e-6);
}
```

In `crates/core-ffi/src/automation.rs`'s tests:

```rust
    /// Spec §7.1 (Ruling 15): a plugin that sends values without gesture
    /// events is touched by its first value and released 500 ms after its
    /// last; one with Begin/End is released by its End only; on an
    /// automated parameter a value with no Begin is an echo, not a touch
    /// (Ruling 27); a slot that went releases its touches.
    #[test]
    fn values_without_gesture_events_release_500_ms_after_the_last() {
        let t = AutomationTarget::PluginParam(PluginInstanceId(3), ParamId(0));
        let ms = Duration::from_millis;
        let mut p = PluginTouches::default();
        assert_eq!(p.event(t, PluginMove::Value(0.2), ms(0), false), vec![Move::Touch(Some(0.2))]);
        assert_eq!(p.event(t, PluginMove::Value(0.3), ms(100), false), vec![Move::Value(0.3)]);
        assert!(p.idle(ms(599)).is_empty());
        assert_eq!(p.idle(ms(600)), vec![(t, ms(600))]);
        assert!(p.idle(ms(2000)).is_empty(), "released once");
        assert_eq!(p.event(t, PluginMove::Begin, ms(3000), false), vec![Move::Touch(None)]);
        assert!(p.event(t, PluginMove::Begin, ms(3001), false).is_empty(), "a repeated Begin");
        assert_eq!(p.event(t, PluginMove::Value(0.5), ms(3010), false), vec![Move::Value(0.5)]);
        assert!(p.idle(ms(9000)).is_empty(), "gesture-driven: no idle release");
        assert_eq!(p.event(t, PluginMove::End, ms(9001), false), vec![Move::Release]);
        assert!(p.event(t, PluginMove::End, ms(9002), false).is_empty(), "an End with no touch");
        // Pre-flight I8: an automated parameter's value-only events are
        // echoes of playback; a Begin still touches it.
        let t2 = AutomationTarget::PluginParam(PluginInstanceId(3), ParamId(1));
        assert!(p.event(t2, PluginMove::Value(0.2), ms(10_000), true).is_empty(), "an echo");
        assert!(p.idle(ms(20_000)).is_empty(), "no touch was made");
        assert_eq!(p.event(t2, PluginMove::Begin, ms(20_001), true), vec![Move::Touch(None)]);
        assert_eq!(p.event(t2, PluginMove::Value(0.4), ms(20_002), true), vec![Move::Value(0.4)]);
        assert_eq!(p.event(t2, PluginMove::End, ms(20_003), true), vec![Move::Release]);
        // Pre-flight I7: the touches of a slot that went are released once.
        assert_eq!(p.event(t2, PluginMove::Value(0.5), ms(21_000), false), vec![Move::Touch(Some(0.5))]);
        assert_eq!(p.release_slots(|_| false), vec![t2]);
        assert!(p.idle(ms(30_000)).is_empty(), "released once");
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-host --test host -- idle_reports_every a_gesture_the_host_closes 2>&1 | tail -3
cargo test -p core-ffi --lib automation 2>&1 | tail -3
```

Expected: both FAIL to compile (`param_events`, `PluginTouches`); once `param_events` exists, `a_gesture_the_host_closes_reports_its_end` FAILS until `end_open` is called (no `End` is reported).

- [ ] **Step 3: Report the events; forward them; hold back written gestures**

In `crates/core-host/src/host.rs`:

Find:
```rust
    /// Finished UI gestures.
    pub gestures: Vec<Gesture>,
```
Replace:
```rust
    /// Finished UI gestures.
    pub gestures: Vec<Gesture>,
    /// Every parameter event the plugins reported, in order, beside the
    /// grouped gestures, with an `End` for each parameter of a gesture the
    /// host closed itself (plan 5e-1 Ruling 15): automation writes from
    /// them.
    pub param_events: Vec<(PluginInstanceId, ParamEvent)>,
```
and in `IdleReport::merge`, after `self.gestures.extend(o.gestures);`, add `self.param_events.extend(o.param_events);`. `collect_gestures` gains a `raw: &mut Vec<(PluginInstanceId, ParamEvent)>` parameter and appends each event it reads there (callers pass `&mut self.raw`, a new `PluginHost` field like `ended`: the borrows of `self.slots` and `self.raw` are of separate fields); `idle` and `service` move `raw` into their report's `param_events`. Add `end_open` and call it where the opening paragraph says.

In `crates/core-ffi/src/session.rs`'s `pump`:

Find:
```rust
        inner.held.extend(r.gestures);
```
Replace:
```rust
        // A plugin window's moves are gestures too (spec §7.1); the
        // parameters they write as automation are not parameter steps
        // (plan 5e-1 Ruling 15, pre-flight M13).
        let written = self.plugin_events(inner, &r.param_events);
        inner.held.extend(r.gestures.into_iter().filter_map(|mut g| {
            let slot = g.slot;
            g.values.retain(|(p, _)| !written.contains(&(slot, *p)));
            (!g.values.is_empty()).then_some(g)
        }));
```
with `fn plugin_events(&self, inner: &mut Inner, events: &[(PluginInstanceId, ParamEvent)]) -> BTreeSet<(PluginInstanceId, ParamId)>` (the parameters on a writing path), reading the ranges once through `plugins::with` and feeding `PluginTouches` (with `automated`) and the gesture path (`Source::Plugin`, the drain time). `pump`'s `r` must be bound `mut` where it is matched (`let Ok(Some(mut r)) = report`) if the borrow needs it.

- [ ] **Step 4: Run the tests to verify they pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -2
cargo clippy --workspace --all-targets --target x86_64-unknown-linux-gnu --target-dir target/linux-check -- -D warnings 2>&1 | tail -2
cargo clippy -p core-ffi --lib -- -D warnings 2>&1 | tail -2
cargo test -p core-host 2>&1 | grep -E "test result|FAILED" | grep -v " 0 failed"
cargo test -p core-ffi --lib --test params --test automation 2>&1 | grep -E "test result|FAILED"
scripts/build-ffi.sh && scripts/swift.sh build 2>&1 | grep -E "error:|warning:" | grep -v MemoryDefaults | sort -u
scripts/swift.sh test 2>&1 | grep -E "Test run"
```

Expected: every `core-host` test passes (the existing gesture tests unchanged); `params` and `automation` pass (Rust 1529); Swift 846.

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-host/src/host.rs crates/core-host/tests/host.rs crates/core-ffi/src/automation.rs crates/core-ffi/src/session.rs crates/core-ffi/tests/params.rs
git commit -m "core-host, core-ffi: plugin windows write automation; value-only plugins release after 500 ms (5e-1 Task 9)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -3
```

### Task 10: The control channel: `automation.set_mode`, `automation.gesture`, `automation.suspend`, `state.get automation`

**Files:**
- Modify: `crates/core-control/src/registry.rs` (three commands and their params), `crates/core-control/src/request.rs` (`StateTarget::Automation`; the `Input` doc names the held peaks)
- Modify: `crates/core-ffi/src/control.rs` (`StateTargetRow::Automation`, `ControlResult::Automation`, its JSON), `crates/core-ffi/src/automation.rs` (`automation_target_text`, `parse_automation_target`)
- Create: `app/Sources/VizDAWAppModel/AutomationCommands.swift`, `app/Tests/VizDAWAppModelTests/AutomationCommandsTests.swift`
- Modify: `app/Sources/VizDAWAppModel/ControlCommands.swift` (handled ids, dispatch, `state.get automation`), `app/README.md` (the channel paragraph)
- Test: `crates/core-control/tests/commands.rs`, `crates/core-ffi/tests/automation.rs`, `crates/core-ffi/tests/control.rs`

**Interfaces:**
- Consumes: Task 8's session methods and rows; `ControlCommands.run`'s structure (5d-1's `AudioCommands.set` is the app-wide precedent, `SynthCommands` the per-document one).
- Produces:
  - Registry (`key_only`, `document: true` except `automation.suspend`): `automation.set_mode { strips: Text (required: "all" or comma-separated strip IDs), mode: Choice(off, read, touch, latch, write, trim) (required) }` (menu "Mixer", title "Set Automation Mode"); `automation.gesture { target: Text (required), phase: Choice(touch, value, release) (required), value: Text }` ("Mixer", "Automation Gesture"); `automation.suspend { on: Flag }` (`ON`; "Mixer", "Suspend Automation", `document: false`).
  - `StateTarget::Automation` ("automation"); `StateTargetRow::Automation`; `ControlResult::Automation { doc: String, state: AutomationStateRow }` answering `{"doc", "suspended", "writing", "modes": [{"strip", "mode"}], "touched": ["fader:3", …]}` (modes as the Choice spells them).
  - `#[uniffi::export] pub fn automation_target_text(target: AutomationTargetRow) -> String` and `#[uniffi::export] pub fn parse_automation_target(text: String) -> Option<AutomationTargetRow>` (Ruling 17; at most 64 characters, ASCII digits, no sign, no spaces; `send:` counts from 1; velocity scale has no spelling).
  - Swift `@MainActor enum AutomationCommands { static let setMode = "automation.set_mode"; static let gesture = "automation.gesture"; static let suspend = "automation.suspend"; static let ids: Set<String>; static func runSuspend(_ args: [ControlArg]) -> ControlOutcome; static func run(_ id: String, args: [ControlArg], doc: UUID, _ m: DocumentModel) -> ControlOutcome; nonisolated static func mode(_ text: String?) -> AutomationModeRow?; nonisolated static func strips(_ text: String, all: [UInt64]) -> [UInt64]?; nonisolated static func value(_ text: String?) -> Double? }` (`strips`: `"all"` or IDs, a repeated ID kept once at its first place, M21; `value`: a plain decimal, ASCII digits with at most one `.` and nothing else (no sign, exponent, hex or spaces), in 0–1, else nil).

The app checks every argument before calling the core (`invalid_params` naming the parameter); the core's refusal (an unknown strip, an open edit) is the call's `failed` (`busy` for `Busy`). `automation.gesture` needs `value` for `value` and takes it optionally for `touch`. A Release answers `ran` also when nothing was touched (Ruling 20).

- [ ] **Step 1: Write the failing tests**

In `crates/core-control/tests/commands.rs`, change the registry count to 132 and append to its message "; 5e-1 adds automation.set_mode, automation.gesture and automation.suspend". Then append:

```rust
/// Plan 5e-1 Task 10: the automation commands take their parameters by
/// type and range; hostile values never reach the app.
#[test]
fn automation_commands_check_their_params() {
    let run = |id: &str, params: Value| json!({ "id": id, "params": params });
    for (id, params) in [
        ("automation.set_mode", json!({ "strips": "all", "mode": "touch" })),
        ("automation.set_mode", json!({ "strips": "1,2,3", "mode": "off" })),
        ("automation.gesture", json!({ "target": "fader:3", "phase": "touch" })),
        ("automation.gesture", json!({ "target": "send:3:1", "phase": "value", "value": "0.25" })),
        ("automation.gesture", json!({ "target": "param:9:100", "phase": "release" })),
        ("automation.suspend", json!({})),
        ("automation.suspend", json!({ "on": true })),
    ] {
        assert!(matches!(ok("command.run", run(id, params.clone())), Command::CommandRun { .. }), "{id} {params}");
    }
    for (id, params) in [
        ("automation.set_mode", json!({ "mode": "touch" })),
        ("automation.set_mode", json!({ "strips": "all" })),
        ("automation.set_mode", json!({ "strips": "all", "mode": "loud" })),
        ("automation.set_mode", json!({ "strips": [1, 2], "mode": "read" })),
        ("automation.set_mode", json!({ "strips": "1".repeat(100_000), "mode": "read" })),
        ("automation.gesture", json!({ "phase": "touch" })),
        ("automation.gesture", json!({ "target": "fader:3" })),
        ("automation.gesture", json!({ "target": "fader:3", "phase": "wiggle" })),
        ("automation.gesture", json!({ "target": "fader:3", "phase": "value", "value": 0.5 })),
        ("automation.gesture", json!({ "target": 3, "phase": "touch" })),
        ("automation.suspend", json!({ "on": "yes" })),
        ("automation.suspend", json!({ "now": true })),
    ] {
        let (code, _) = err("command.run", run(id, params.clone()));
        assert_eq!(code, ErrorCode::InvalidParams, "{id} {params}");
    }
    let spec = |id: &str| registry::find(id).unwrap();
    assert!(!spec("automation.set_mode").menu_item && spec("automation.set_mode").document);
    assert!(spec("automation.gesture").document);
    assert!(!spec("automation.suspend").document);
    assert_eq!(
        ok("state.get", json!({"target": "automation"})),
        Command::StateGet { target: StateTarget::Automation, window: None }
    );
}
```

Append to `crates/core-ffi/tests/automation.rs`:

```rust
/// Ruling 17, Review Focus 5: targets are spelt one way; anything else
/// is refused (signs, spaces, zero sends, overflow, velocity scale, long
/// or non-ASCII text).
#[test]
fn targets_parse_only_in_their_own_spelling() {
    use vizdaw_ffi::automation::{automation_target_text, parse_automation_target as parse};
    for t in [
        Target::StripFader { strip: 3 },
        Target::StripPan { strip: 0 },
        Target::StripMute { strip: 12 },
        Target::StripSolo { strip: 4 },
        Target::SendLevel { strip: 3, send: 0 },
        Target::PluginParam { slot: 9, param: 4_294_967_295 },
    ] {
        let text = automation_target_text(t);
        assert_eq!(parse(text.clone()), Some(t), "{text}");
    }
    assert_eq!(automation_target_text(Target::SendLevel { strip: 3, send: 0 }), "send:3:1");
    assert_eq!(automation_target_text(Target::VelocityScale { dest: 1 }), "");
    for bad in [
        "", "fader", "fader:", "fader:-1", "fader:+1", "fader: 3", "fader:3 ", "Fader:3", "fader:3:1",
        "send:3", "send:3:0", "send:3:65537", "param:9", "velocity:1", "fader:99999999999999999999999",
        "fader:３", "pan:1\u{0}", &"9".repeat(100),
    ] {
        assert_eq!(parse(bad.to_string()), None, "{bad:?}");
    }
}
```

In `crates/core-ffi/tests/control.rs`, add a test `automation_state_answers_in_the_channels_spelling` that builds `ControlResult::Automation { doc: "d".into(), state: AutomationStateRow { modes: vec![StripModeRow { strip: 1, mode: AutomationModeRow::Touch }, StripModeRow { strip: 0, mode: AutomationModeRow::Read }], touched: vec![AutomationTargetRow::SendLevel { strip: 1, send: 0 }], suspended: true, writing: false } }` and asserts `result_json` equals `json!({"doc": "d", "suspended": true, "writing": false, "modes": [{"strip": 1, "mode": "touch"}, {"strip": 0, "mode": "read"}], "touched": ["send:1:1"]})`.

Create `app/Tests/VizDAWAppModelTests/AutomationCommandsTests.swift`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Tests/VizDAWAppModelTests/AutomationCommandsTests.swift <<'VIZDAW_EOF'
import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

/// The automation commands over the control channel (plan 5e-1 Task 10):
/// arguments checked before the core is called; the core's refusals are
/// the call's failures; nothing opens a dialog.
@MainActor
@Suite(.serialized) struct AutomationCommandsTests {
    func doc() throws -> (UUID, DocumentModel, FakeSession) {
        let f = mixerFake()
        let s = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { f }, open: { _ in demoFake() })
        let id = try #require(s.newDemo())
        return (id, try #require(s.model(id)), f)
    }

    func arg(_ name: String, _ v: String) -> ControlArg { ControlArg(name: name, value: .text(v)) }

    func code(_ o: ControlOutcome) -> ControlErrorCode? {
        if case .failed(let f) = o { return f.code }
        return nil
    }

    @Test func automationCommandsCheckTheirArguments() throws {
        let (id, m, f) = try doc()
        let run = { (cmd: String, args: [ControlArg]) in AutomationCommands.run(cmd, args: args, doc: id, m) }
        // set_mode: the strips and the mode.
        #expect(code(run(AutomationCommands.setMode, [arg("strips", "all"), arg("mode", "touch")])) == nil)
        #expect(f.modeCalls.last?.0 == [1, 2, 0] && f.modeCalls.last?.1 == .touch)
        #expect(code(run(AutomationCommands.setMode, [arg("strips", "2,1"), arg("mode", "off")])) == nil)
        #expect(f.modeCalls.last?.0 == [2, 1])
        #expect(code(run(AutomationCommands.setMode, [arg("strips", "1,1"), arg("mode", "read")])) == nil)
        #expect(f.modeCalls.last?.0 == [1], "a strip listed twice is set once")
        for bad in ["", "1,,2", "1,x", "-1", "1.5", " 1", "99999999999999999999999"] {
            #expect(
                code(run(AutomationCommands.setMode, [arg("strips", bad), arg("mode", "read")])) == .invalidParams,
                "\(bad)")
        }
        // gesture: the target, the phase, the value.
        #expect(code(run(AutomationCommands.gesture, [arg("target", "fader:1"), arg("phase", "touch")])) == nil)
        #expect(f.gestures.last?.1 == .touch(value: nil))
        #expect(
            code(run(AutomationCommands.gesture, [arg("target", "fader:1"), arg("phase", "value"), arg("value", "0.25")]))
                == nil)
        #expect(f.gestures.last?.1 == .value(value: 0.25))
        for (target, phase, value) in [
            ("fader:x", "touch", nil), ("velocity:1", "touch", nil), ("fader:1", "value", nil),
            ("fader:1", "value", "nan"), ("fader:1", "value", "1e400"), ("fader:1", "value", "-0.5"),
            ("fader:1", "value", "1.5"), ("fader:1", "value", "0,5"), ("fader:1", "touch", "inf"),
            ("fader:1", "value", "1e-1"), ("fader:1", "value", "0x1p-2"), ("fader:1", "value", " 0.5"),
        ] as [(String, String, String?)] {
            var args = [arg("target", target), arg("phase", phase)]
            if let value { args.append(arg("value", value)) }
            #expect(code(run(AutomationCommands.gesture, args)) == .invalidParams, "\(target) \(phase) \(value ?? "-")")
        }
        // The core's refusal is the call's failure.
        f.gestureError = .Model(message: "unknown id strip9")
        #expect(code(run(AutomationCommands.gesture, [arg("target", "fader:9"), arg("phase", "touch")])) == .failed)
        f.gestureError = .Busy(message: "Finish the open edit first.")
        #expect(code(run(AutomationCommands.gesture, [arg("target", "fader:1"), arg("phase", "touch")])) == .busy)
    }

    @Test func suspendTogglesTheGlobalSwitch() throws {
        setAutomationSuspended(on: false)
        defer { setAutomationSuspended(on: false) }
        _ = AutomationCommands.runSuspend([])
        #expect(automationSuspended())
        _ = AutomationCommands.runSuspend([ControlArg(name: "on", value: .flag(true))])
        #expect(automationSuspended())
        _ = AutomationCommands.runSuspend([])
        #expect(!automationSuspended())
        #expect(ControlCommands.handled.isSuperset(of: AutomationCommands.ids))
    }
}
VIZDAW_EOF
```

(`mixerFake()`'s mixer has strips 1, 2 and master 0, in that display order; `automation.set_mode`'s `"all"` reads them from `session.mixer()`. The suspend test calls the real core setter: it is process-wide state with no device or file behind it, reset by `defer`.)

- [ ] **Step 2: Run the tests to verify they fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-control --test commands 2>&1 | tail -3
cargo test -p core-ffi --test automation targets_parse 2>&1 | tail -3
```

Expected: FAIL (the count is 129; `StateTarget::Automation` and `parse_automation_target` do not exist).

- [ ] **Step 3: The registry, the state target and the spelling**

In `crates/core-control/src/registry.rs`, add `AUTOMATION_MODE_PARAMS` and `AUTOMATION_GESTURE_PARAMS` (help lines: "`all`, or strip IDs separated by commas (see `state.get automation`)."; "off, read, touch, latch, write or trim."; "`fader:<strip>`, `pan:<strip>`, `mute:<strip>`, `solo:<strip>`, `send:<strip>:<n>` (n from 1) or `param:<slot>:<param>`."; "touch, value or release."; "The normalised value 0–1 as text (\"0.5\"); needed for value, optional for touch.") and the three `CommandSpec`s after `destination.add_midi`'s, as the Interfaces block says. In `crates/core-control/src/request.rs`, add `StateTarget::Automation` (doc: "Its strips' automation modes, the touched targets and suspend (plan 5e-1).") parsed from `"automation"` (the error message lists it), and extend `Input`'s doc: "Its `levels` are each channel's peak since the last read (held, so a quiet read can show a recent peak), as long as the channel count at the last poll (one poll stale after a device change; 5d-2 T4 M3)."

In `crates/core-ffi/src/control.rs`, `StateTargetRow::Automation` (mapped next to `StateTarget::Input => StateTargetRow::Input`), `ControlResult::Automation { doc: String, state: crate::automation::AutomationStateRow }` and its arm in `result_json`. In `crates/core-ffi/src/automation.rs`, the two spelling exports (bodies in `catch_unwind`).

- [ ] **Step 4: The app's handlers**

Create `app/Sources/VizDAWAppModel/AutomationCommands.swift` with the enum of the Interfaces block. `run` checks the arguments (`.failed(.of(.invalidParams, "automation.set_mode: `strips` is all or strip IDs such as \"1,2\""))`, `"… `mode` is off, read, touch, latch, write or trim"`, `"automation.gesture: `target` is …"` naming the spellings, `"… `value` is a number from 0 to 1"`), parses the target with `parseAutomationTarget(text:)`, calls `m.session.setAutomationMode(strips:mode:)` (applying the snapshot with `m.apply`) or `m.session.gesture(target:phase:)`, and maps a thrown error with `ControlFailure.thrown(error, "Automation Failed")`. `runSuspend` sets `setAutomationSuspended(on: args.flag("on") ?? !automationSuspended())` and answers `.done(ControlCommands.ran(id, nil))`.

In `app/Sources/VizDAWAppModel/ControlCommands.swift`:

Find:
```swift
            .union(TransportCommands.ids).union(StepCommands.ids).union(AudioCommands.ids)
```
Replace:
```swift
            .union(TransportCommands.ids).union(StepCommands.ids).union(AudioCommands.ids)
            .union(AutomationCommands.ids)
```

Find:
```swift
        case AudioCommands.set:
            return AudioCommands.runSet(args, host: host)
```
Replace:
```swift
        case AudioCommands.set:
            return AudioCommands.runSet(args, host: host)
        case AutomationCommands.suspend:
            return AutomationCommands.runSuspend(args)
```

Find:
```swift
        case _ where SynthCommands.ids.contains(id):
            return SynthCommands.run(id, args: args, doc: docID, m)
```
Replace:
```swift
        case _ where SynthCommands.ids.contains(id):
            return SynthCommands.run(id, args: args, doc: docID, m)
        case _ where AutomationCommands.ids.contains(id):
            return AutomationCommands.run(id, args: args, doc: docID, m)
```

Find:
```swift
        case .input:
            guard let info = try? m.session.inputInfo() else { return nil }
```
Replace:
```swift
        case .automation:
            guard let s = try? m.session.automationState() else { return nil }
            return .automation(doc: doc, state: s)
        case .input:
            guard let info = try? m.session.inputInfo() else { return nil }
```

In `app/README.md`'s "Agent control channel" section, after the paragraph that ends "…refuse tick positions or lengths beyond +-2^40." (its last line, "edits and loads refuse tick positions or lengths beyond +-2^40."), add:

```markdown
Automation (plan 5e-1): `automation.set_mode {"strips": "all" | "1,2",
"mode": "touch"}`, `automation.gesture {"target": "fader:3", "phase":
"touch" | "value" | "release", "value": "0.5"}` (targets `fader:`, `pan:`,
`mute:`, `solo:` with a strip ID, `send:<strip>:<n>` with n from 1,
`param:<slot>:<param>`; values are text, 0 to 1) and `automation.suspend`
(absent `on` toggles; it is app-wide). A `touch` without `value` starts at
the value heard now. A touch from the channel lasts until its release, a
Stop's landing keeps it, closing the window ends it; a Locate or a Play
while writing lands what was written so far.
`state.get automation` lists each strip's mode, the touched targets and
whether automation is suspended or being written. `state.get input`'s
`levels` are each channel's peak since the last read (held, so a quiet
read can still show a recent peak) and its list is as long as the channel
count at the last poll (one poll stale after a device change).
```

- [ ] **Step 5: Run the tests to verify they pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -2
cargo clippy --workspace --all-targets --target x86_64-unknown-linux-gnu --target-dir target/linux-check -- -D warnings 2>&1 | tail -2
cargo clippy -p core-ffi --lib -- -D warnings 2>&1 | tail -2
cargo test -p core-control 2>&1 | grep -E "test result|FAILED" | grep -v " 0 failed"
cargo test -p core-ffi --test automation --test control 2>&1 | grep -E "test result|FAILED"
scripts/build-ffi.sh && scripts/swift.sh build 2>&1 | grep -E "error:|warning:" | grep -v MemoryDefaults | sort -u
scripts/swift.sh test 2>&1 | grep -E "Test run"
```

Expected: all pass (Rust 1532); Swift "Test run with 848 tests … passed" (`everyRegisteredMenuCommandHasAHandler` sees the three new IDs handled). `app/Sources/VizDAW/ControlChannel.swift` is not touched (M19).

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-control/src/registry.rs crates/core-control/src/request.rs crates/core-control/tests/commands.rs \
  crates/core-ffi/src/control.rs crates/core-ffi/src/automation.rs crates/core-ffi/tests/automation.rs crates/core-ffi/tests/control.rs \
  app/Sources/VizDAWAppModel/AutomationCommands.swift app/Sources/VizDAWAppModel/ControlCommands.swift \
  app/Tests/VizDAWAppModelTests/AutomationCommandsTests.swift app/README.md
git commit -m "core-control, core-ffi, app: automation.set_mode, automation.gesture, automation.suspend and state.get automation (5e-1 Task 10)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -3
```

### Task 11: The e2e, the harness's homes, the README and the backlog; the full gates

**Files:**
- Modify: `crates/vizctl/tests/e2e.rs` (the harness owns its home; the new test)
- Modify: `app/README.md` (the "Automation recording (plan 5e-1)" section), `docs/feature-backlog.md` (N3 folded into the CoreMIDI row; six new rows, pre-flight I14)

**Interfaces:**
- Consumes: Tasks 1–10.
- Produces: the e2e `touch_writes_a_fader_over_the_channel`; README steps 160–164.

Spec §12.3's e2e "Touch-write a fader via `automation.gesture` while playing and check the points" (with the C1 guard: the pass still writes right after Play), the 5d-2 carry T10 Minor 6 (the harness leaves its `/tmp/vze*` homes), and the carried items that had no tracked home (pre-flight I14).

- [ ] **Step 1: The harness removes its home**

In `crates/vizctl/tests/e2e.rs`, `App` gains `_home: tempfile::TempDir` (last field: it goes after the process and the client); `start_with` keeps the `TempDir` (no `.keep()`), passes `home: Some(home.path().to_path_buf())` and stores it. Nothing else reads the field.

- [ ] **Step 2: Write the e2e**

Append to `crates/vizctl/tests/e2e.rs`:

```rust
/// Spec §12.3 through the channel (plan 5e-1): with the Built-in Synth
/// strip on Touch, a fader touched, moved and released while playing
/// lands at Stop as one "Write Automation" step in a lane "Built-in Synth
/// Fader"; hostile gestures are refused before anything changes.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn touch_writes_a_fader_over_the_channel() {
    let mut app = App::start();
    let w = app.call("app.new_demo", json!({}))["window"].clone();
    app.wait(&w, json!({"condition": "documents_at_least", "count": 1}));
    let state = app.call("state.get", json!({"target": "automation", "window": w}));
    assert_eq!(state["suspended"], false, "{state}");
    let synth = state["modes"][0]["strip"].as_u64().expect("the synth strip first");
    let run = |app: &mut App, id: &str, params: Value| {
        app.call("command.run", json!({"id": id, "params": params, "window": w}))
    };
    run(&mut app, "automation.set_mode", json!({"strips": synth.to_string(), "mode": "touch"}));
    let state = app.call("state.get", json!({"target": "automation", "window": w}));
    assert_eq!(state["modes"][0]["mode"], "touch", "{state}");
    // Hostile input: refused, nothing touched.
    for (params, code) in [
        (json!({"target": "fader:999999", "phase": "touch"}), core_control::ErrorCode::Failed),
        (json!({"target": format!("fader:{synth}"), "phase": "value", "value": "nan"}), core_control::ErrorCode::InvalidParams),
        (json!({"target": "fader", "phase": "touch"}), core_control::ErrorCode::InvalidParams),
    ] {
        let e = app.call_err("command.run", json!({"id": "automation.gesture", "params": params, "window": w}));
        assert_eq!(e.code, code, "{e}");
    }
    run(&mut app, "transport.play_stop", json!({}));
    app.wait(&w, json!({"condition": "playing"}));
    // Pre-flight C1 (Ruling 24): the first idle after Play must not end
    // the pass.
    let state = app.call("state.get", json!({"target": "automation", "window": w}));
    assert_eq!(state["writing"], true, "{state}");
    let target = format!("fader:{synth}");
    run(&mut app, "automation.gesture", json!({"target": target, "phase": "touch", "value": "0.5"}));
    for v in ["0.55", "0.6", "0.65", "0.7"] {
        std::thread::sleep(Duration::from_millis(30));
        run(&mut app, "automation.gesture", json!({"target": target, "phase": "value", "value": v}));
    }
    let state = app.call("state.get", json!({"target": "automation", "window": w}));
    assert_eq!(state["touched"][0], target, "{state}");
    assert_eq!(state["writing"], true, "{state}");
    run(&mut app, "automation.gesture", json!({"target": target, "phase": "release"}));
    std::thread::sleep(Duration::from_millis(600));
    run(&mut app, "transport.play_stop", json!({}));
    app.wait(&w, json!({"condition": "stopped"}));
    let doc = wait_for_state(&mut app, &w, "document", |d| d["undo"] == "Write Automation");
    assert!(
        doc["lanes"].as_array().unwrap().iter().any(|l| l["name"] == "Built-in Synth Fader"),
        "{doc}"
    );
    let state = app.call("state.get", json!({"target": "automation", "window": w}));
    assert_eq!(state["touched"], json!([]), "{state}");
    app.quit();
}
```

(`wait_for_state` and `App::call_err` are the file's; `ErrorCode::Failed` is what a core `Model` refusal becomes through `ControlFailure.thrown`.)

- [ ] **Step 3: The README and the backlog**

Append to `app/README.md`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat >> app/README.md <<'VIZDAW_EOF'

### Automation recording, the engine (plan 5e-1)

Through the control channel (`vizctl launch … --env VIZDAW_AUDIO=offline`
for steps 160–162; plan 5e-2 adds the mixer's mode popups, rings and
Settings › Automation, steps 165 on). `$V` is `target/debug/vizctl`.
The e2e `touch_writes_a_fader_over_the_channel` runs step 160.

160. Touch: `$V state automation` lists each strip's mode (all `read`).
     `$V run automation.set_mode '{"strips": "<synth>", "mode": "touch"}'`,
     play, `$V run automation.gesture '{"target": "fader:<synth>", "phase":
     "touch", "value": "0.4"}'`, a few `"phase": "value"` calls, then
     `"release"`; the fader is heard at 0.4 at once and glides back over a
     quarter of a second after the release. Stop: Edit > Undo reads "Undo
     Write Automation"; a lane "Built-in Synth Fader" was added at the end
     of Song; the Strip Chart on it shows the move and a ramp back. One
     Undo removes the lane. A Locate (or Play) while writing lands what
     was written so far as its own Write Automation step.
161. Write and Latch: set the strip to `write` and play a few bars without
     touching anything, Stop: lanes for its fader, pan and mute hold their
     values from the play point to Stop, and `$V state automation` shows
     the strip on `latch` (Settings › Automation's After Write, plan 5e-2);
     one Undo puts both back. On `latch`, touch and release the pan: it
     holds until Stop, and with Loop on it holds until the loop wraps.
     Automation inside a referenced sequence is not replaced by a write.
162. Off and suspend: set the strip to `off`: its automation is not
     heard (the fader stays where the mixer has it). `$V run
     automation.suspend`: it is heard again (every strip behaves as Read);
     run it again to resume.
163. Plugin windows (manual checklist "plugin-window automation"; a real
     AU and a real CLAP plugin): put a plugin on the Built-in Synth strip,
     set the strip to Touch, play, and move a knob in the plugin's own
     window: the move is written into a lane named "Built-in Synth
     <parameter>", and there is no "Change Plugin Parameter" step under
     it. A plugin that reports no gesture start or end (some older AUs)
     writes from its first value to half a second after its last. In
     Read, a move in its window is one "Change Plugin Parameter" step, as
     before. Moves in a plugin's window are placed when VizDAW reads them,
     up to a sixtieth of a second late.
164. Latency (5d-1 ruling C2): put a plugin with latency (a linear-phase
     EQ or a look-ahead limiter) on a bus, so audio tracks are read ahead;
     write a mute on an audio track at a bar line: it cuts exactly at the
     bar as heard, with and without the plugin. Zoom in on a bounce
     (plan 5f) to check to the sample.
VIZDAW_EOF
```

In `docs/feature-backlog.md`, at the end of the row `| Input pickers' MIDI source list off the main thread |`'s notes, add: "The mixer's input controls also read the device's channels (`audio_input_channels()`) on the main thread when a strip renders, e.g. on hover (5d-2 re-review N3): same fix."

Then append the six rows (pre-flight I14: the carried items with no tracked home, and two limits of this plan) after the table's last row, `| Input devices keyed by UID | … |`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat >> docs/feature-backlog.md <<'VIZDAW_EOF'
| Lane header meter invalidates only its rect | Plan 5d-2 T5 M2 (carried by 5e-1): a meter change invalidates the whole lane header; invalidate the meter's rect only. Performance only. SP7 (drawing). |
| Lane header meter clear of the separator at heights 46–47 | Plan 5d-2 T5 M4, the other half (carried by 5e-1): at lane heights 46–47 points the meter touches the row separator. SP7, after the owner's eye check (README 147). |
| Mixer strip's lane picker names the lane it edits | Plan 5d-2 T6 Minor 2 (carried by 5e-1): with several lanes on one path, the lane the strip's input picker edits changes silently as arming changes (by design, plan 5d-2 Ruling 4). Show the lane's name in the picker. SP7 (mixer). |
| Growing take's waveform follows a tempo change | Plan 5d-2 T9 (carried by 5e-1): with a tempo change inside a take being recorded, its waveform is drawn linear in frames while its rect is in ticks. Cosmetic, and only until Stop. SP7 (drawing). |
| Mixer controls show the heard (automated or held) value while playing | Plan 5e-1 pre-flight I11: the mixer shows static values; grabbing an automated fader in a writing mode starts from its heard value (plan 5e-1 Ruling 26), so the display should follow the heard value too. |
| A write also replaces the target's automation inside referenced sequences | Plan 5e-1 pre-flight I5: a write replaces the target's points in its range in every lane of the playing sequence, but points inside referenced sequences still play there. |
VIZDAW_EOF
```

- [ ] **Step 4: The full gates and the e2e**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -3
cargo clippy --workspace --all-targets --target x86_64-unknown-linux-gnu --target-dir target/linux-check -- -D warnings 2>&1 | tail -2
cargo clippy -p core-ffi --lib -- -D warnings 2>&1 | tail -2
cargo clippy -p vizdaw-play --all-targets --features rt-check -- -D warnings 2>&1 | tail -2
cargo test -p core-engine --features rt-check --test rt_alloc --test rt_alloc_graph --test rt_alloc_record --test rt_alloc_gestures 2>&1 | grep "test result"
cargo test --workspace --no-fail-fast 2>&1 | grep -E "test result|FAILED|panicked" | grep -v " 0 failed"; echo "workspace done"
scripts/build-ffi.sh && scripts/swift.sh build 2>&1 | grep -E "error:|warning:|Build comp" | grep -v "MemoryDefaults" | sort -u
scripts/swift.sh test 2>&1 | grep -E "Test run"
scripts/make-app.sh 2>&1 | tail -2
VIZDAW_E2E_APP=dist/VizDAW.app cargo test -p vizctl --test e2e -- --ignored --nocapture --test-threads 1 2>&1 | grep -E "^test |test result"
pgrep -lf MacOS/VizDAW; ls -d /tmp/vze* 2>/dev/null | wc -l
```

Expected: everything clean (the Linux clippy too); "workspace done" with no failure lines (Rust about 1532 passed, 14 ignored with the new e2e; re-run a known flake once); Swift "Test run with 848 tests … passed"; the 10 e2e tests pass; `pgrep` prints nothing and no new `/tmp/vze*` home is left (the count is what it was before the run; remove older ones with `rm -rf /tmp/vze*`).

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/vizctl/tests/e2e.rs app/README.md docs/feature-backlog.md
git commit -m "vizctl, docs: the automation e2e; the e2e removes its homes; README steps 160-164 (5e-1 Task 11)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -3
```

## Self-Review

- **Spec coverage.** §7.1: `Session.gesture` (Task 8), the channel (Task 10), AU and CLAP windows through the host (Task 9), the 500 ms rule (Task 9); the mixer's mouse gestures are plan 5e-2's calls into Task 8. §7.2 (Tasks 5, 8; the snap-back of plugin parameters, Task 5). §7.3: modes, loop wraps, capture, thinning, replacing (in every lane, Task 3) and edges (Tasks 5–8, golden tests in Task 7). §7.4: placement and labels (Task 3), "Write Automation" vs. the RecordPass (Task 8), `automation_preview` (Task 8; the Strip Chart draws it in 5e-2). §7.5: settings and suspend in the core (Task 8), the channel's `automation.suspend` (Task 10); the popups, colours, rings, "Set all strips to…", Settings › Automation and `automation.mode.<mode>` are plan 5e-2's. §4.6 (Task 2), §4.7 (Task 3). §12.1 `automation.*` and `state.get automation` (Task 10). §12.3 the golden tests (Task 7), `rt-check` over gesture overrides (Tasks 5–6), the e2e (Task 11). §13 rows 5e-1/5e-2 (Ruling 1). 5d-1 C2 (Task 4). The carry list (Task 1, Tasks 10–11; the rest is 5e-2's or the backlog's, "Carried items").
- **Pre-flight coverage.** C1: Rulings 7, 24, Tasks 6, 8, 11. C2: Ruling 23, Tasks 5, 7, 8, 11. I1: Ruling 14, Task 8. I2, I3, I10: Ruling 10, Tasks 5, 8. I4: Ruling 6, Task 5. I5: Ruling 4, Tasks 3, 11. I6: Ruling 25, Tasks 7, 8. I7, I9: Ruling 15, Tasks 8, 9. I8: Ruling 27, Task 9. I11: Ruling 26, Tasks 5, 8, 11. I12: Ruling 11, Task 8. I13: Ruling 5, Task 4. I14: Task 11. M1, M4–M6, M8–M10, M15, M17, M20: Task 8. M2: Ruling 20, Task 8. M3: Ruling 9, Task 7. M7: Ruling 12, Task 7. M11: Task 5. M12–M14: Ruling 15, Tasks 8, 9, 11. M16: Ruling 28. M18: Global Constraints, Task 1. M19: header, File map, Tasks 3, 10. M21: Ruling 2, Tasks 2, 10. M22: plan 5e-2 Task 4. M23: Ruling 21.
- **Placeholders.** None: every step names its file, its signatures, the values the spec fixes and its test; the algorithm `autowrite` follows (with the jump and the split) and the session's lifecycle are written out where the tests leave a choice.
- **Type consistency.** `AutomationMode` (Task 2) is what `WriteMode::of` (Task 7), `AutomationModeRow` (Task 8) and the registry's Choice (Task 10) convert; `AutomationWrite` and `MAX_WRITE_POINTS` (Task 3) are what `to_model` makes and splits at (Task 7) and the session lands (Task 8); `GesturePhase`, `GestureMsg`, `Stamped`, `GestureHandles`, `GestureStatus::rolled` (Tasks 5–6) are what Tasks 7–9 consume; `OverrideSnapshot::value` (Task 5) is what `host::heard` reads (Task 8); `AutoCapture.after_write` (Task 7) is filled from `Writing.after_write` (Task 8); `Source`, `writing_ended`, `Sampler`'s host times (Task 8) are what Task 9's forwarding uses; `PluginTouches::{event(…, automated), idle, release_slots}`/`Move` (Task 9) feed Task 8's gesture path; `parse_automation_target`/`automation_target_text` (Task 10) are the spelling in the e2e and the README.
- **Proportion.** Tests and golden values are given whole; implementations are given as signatures, rules and the algorithms the tests leave open.
- **Review Focus.** Each of the five lines names its tests: Tasks 5, 7, 8 and 11 (1), Tasks 7–8 (2), Task 4 (3), Task 9 (4), Tasks 3, 8 and 10 (5).
- **Counts.** Rust 1462 → 1464 (Task 1) → 1467 (2) → 1473 (3) → 1476 (4) → 1487 (5) → 1488 (6) → 1501 (7) → 1524 (8) → 1529 (9) → 1532 (10); Task 11 adds one ignored e2e (14 ignored). Swift 845 → 846 (Task 1) → 848 (Task 10). e2e 9 → 10.
- **Revision check (`plan-revision-check.md`).** I-1: `debug_hold_offline` (Task 8). I-2: Ruling 23, `locate`, `a_locate_before_the_roll_keeps_writing`. I-3: `a_play_while_writing_…`. I-4: the I2 test rewritten without a Locate, its RED line corrected. I-5: `a_full_queue_refuses_and_counts`, `UnsentEnds` and its unit test. I-6: the deleted strip is a Write strip, `writes_land_when_the_recording_records_nothing`. I-7: `a_ui_gesture_on_a_plugin_parameter_reaches_the_plugin`. I-8: `a_trim_grab_does_not_jump`. I-9: `each_point_cuts_once_on_a_latent_session` (a RED exemption). m1 Task 5 Step 4; m2 contiguous split ranges; m3 Ruling 24 and its table; m4 the `const` floor; m5 a test comment; m6 the overflow phase; m7 M2, M4, M7, M8, M9, M13/M14, M16 tested, M1, M5, M10 said untested in Rulings 6, 19, 24; m8 Rulings 26, 27; m9 a removed slot in the host test.
