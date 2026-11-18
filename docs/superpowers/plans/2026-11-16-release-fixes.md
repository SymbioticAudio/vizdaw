# 26.0.0 (1.0) Release Fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the 26.0.0 (1.0) rows of `docs/feature-backlog.md` (lines 10–20) by Friday 2026-11-20. This covers every row except sub-projects 6 and 8, the release gate, and the "Dropped at 1.0" line.

**Architecture:**
- **New `Session` methods go in a child module.** It is `crates/core-ffi/src/session/fixes.rs`, declared by one line, `mod fixes;`, in `session.rs`. A child module can use `Session`'s private items (`lock_inner`, `execute`, `release_from`, `surface_heard`, …), so `session.rs` itself gains only that line and the few one-line hooks named per task. `crates/core-ffi/src/surfaces/` is the precedent for a module directory beside its file.
- **Engine and device fixes** stay in `core-engine` (`count_in.rs`, `render.rs`, `schedule/audio.rs`) and `core-ffi/src/host.rs`. `host.rs` is not on the other branch's list.
- **Swift fixes** stay in the models (`VizDAWAppModel`) and views (`VizDAWEditors`, `VizDAW`).
- **Four new control commands:** `automation.release_all`, `region.set_fade_shape`, `lane.set_output` and `destination.set_sound`.

**Tech Stack:** Rust 2024 (`core-engine`, `core-ffi` with UniFFI 0.32.2, `core-control`, `core-surface`, `platform-macos`, `vizctl`). Swift 6.1 / SwiftPM with Swift Testing, macOS 14+. No new crate and no new dependency. Task 9 may bump the `symbiotic-design` pin (see its gate).

**Spec:** the 26.0.0 block of `docs/feature-backlog.md` (lines 10–20) and each named row in full. Find a row by its Feature name. Two rows cite older specs: §4.5a and §6.7 of `docs/superpowers/specs/2026-10-18-recording-audio-design.md` (destination editor), and Ruling 8 of `docs/superpowers/plans/2026-11-11-crossfades.md` (edge fades). Executors read the rows and those passages.

**Merge order.** `release-fixes` merges **after** `takes-and-co-producer`, which carries Co-Producer plan 8a and rewrites `session.rs` heavily, including a lock helper used at about 194 sites. Both branches start at `8b05c79`. At merge:
- `session/fixes.rs` switches from `self.lock_inner()` to 8a's helper;
- the one-line hooks in `session.rs` are re-applied by hand where they conflict;
- the registry count tests take 8a's count plus this plan's 4.

## Scope Calls (what moves to 27.1.0)

These rows are bigger than their line. The 1.0 version is stated first, then what moves. Task 10 files each moved part as a new backlog row under 27.1.0.

1. **Built-in Synth sounds and a window.**
   - **1.0:** five General MIDI sounds on the DLS synth: Piano, Bass, Pad, Lead and Drums.
     - A sound is stored in the destination's existing `default_patch` and channel, so the file format does not change.
     - A small window opens on a double-click of the strip's Built-in Synth slot, and `destination.set_sound` sets the sound over the channel.
     - The sound belongs to the destination. Every instrument track shares the first Built-in Synth destination (`tracks.rs` `synth_dest`), so choosing a sound changes every track that plays it, and the window says so.
   - **27.1.0:** a Built-in Synth per track, more sounds or a GM list, synth parameters, and VizDAW's own synth DSP.
2. **Zero-crossing snapping and edge fades.**
   - **1.0:** a fixed 1.5 ms edge fade, always on, at every region start and end that meets silence and has no fade or crossfade of its own (Ruling 8). An edge that butts against or is overlapped by another region of the same track (a split, a plain overlap), within a frame, does not ramp: the switch is instant, as before (Task 7 fix round 1). A "Snap audio edits to zero crossings" setting, on by default, moves a trimmed start or end to the nearest zero crossing of the summed signal within 3 ms. It applies to the trim drag and to the typed start and length.
   - **27.1.0:**
     - snapping a Split, which has no UI caller today;
     - snapping per channel;
     - a setting for the edge-fade length;
     - drawing the edge fade in the overview, where it is below a pixel at every zoom shipped.
3. **Input pickers' MIDI source list.**
   - **1.0:** a cache that is refreshed off the main thread whenever a picker or strip asks.
   - **27.1.0:** CoreMIDI's setup-changed notification (`kMIDIMsgSetupChanged`). Today every `MIDIClientCreate` passes no notify proc.
4. **Arm's engine claim.**
   - **1.0:** the 300 ms drain and the MIDI connect leave the main thread (`host.rs` only).
   - **27.1.0:** dropping the session lock around `take_idle`. That is lock code in `session.rs`, which 8a rewrites.
5. **Non-automatable parameters.**
   - **1.0:** in the core, a touch on such a parameter never writes a lane.
   - **27.1.0:** greying out read-only AU sliders in the generic plug-in window.
6. **Destination editor.**
   - **1.0:** the return latency field and Measure, for external destinations (§6.7).
   - **27.1.0:** editing a destination's name, kind or channel from it.
7. **A slightly darker orange swatch.**
   - **Superseded.** Since `7bc5d73` the monitor-input fill is `state.warning`, not the orange swatch (`ThemeScanTests.swift:316`). No legend sits on orange in white ink any more.
   - **1.0:** a test that every swatch carries a legend (Task 9). The row is marked done as superseded.
   - **Owner's choice:** darkening the tokens anyway (Decision 7).
8. **A different sans for the Console theme's legends.**
   - **1.0, only with the owner's yes to Decision 8:** a design-repo release 0.2.3 that adds a legend family.
   - **27.1.0:** without that yes, the whole row.
9. **`streamed_render_equals_cached_render`.**
   - **Probably stale.** `ec7ea23` (2026-11-03) found that the streamed render was wrong after a locate past a region, fixed `core-media/src/stream.rs`, and pinned the seed as `a_locate_past_a_region_keeps_its_loop_seam`.
   - **1.0:** Task 5 verifies the fix with many cases and closes the row. It fixes the streamer only if the test still fails.

## Decisions for the owner

Each is the conservative option, already built into the tasks. Each says what changes if the owner says otherwise.

1. **A device change during a count-in resumes at the next beat.** The beats already clicked do not repeat. The bar's accents land where they would have. Otherwise: restart the whole count-in, which is a one-line change in `rebuild`.
2. **`automation.release_all` releases only the control channel's touches.** A held mouse, surface, learn or plug-in touch is not released. Otherwise: release every holder (`release_touches`).
3. **Built-in Synth sounds are per destination,** shared by the tracks on it (Scope 1).
4. **The edge fade is fixed at 1.5 ms and always on, with no setting.** Zero-crossing snapping is on by default and searches 3 ms each side on the summed signal.
5. **In the Mix view, nudges and Modify Again are refused** with a status line when no editor is shown. They are not just documented in README 200. A torn-off editor window does not count as showing the selection.
6. **The keyboard surfaces' LEDs use a fixed table of eight bright hues,** one per swatch (Task 6). Tokens are theme-dependent, and LEDs need saturated colour.
7. **The orange tokens stay.** The design repo would change Classic Light `#C25A00` (4.42:1 on white) to `#BD5800` (4.61:1), and Symbiotic `#BF5F00` (4.33:1) to `#B95C00` (4.57:1). Say yes and Task 9 does it with Decision 8's release.
8. **The Console legend font: the design repo change needs a yes.** `symbiotic-design` is shared with the plug-in app. Adding a `LegendFamily` case is source-breaking for its exhaustive switches.
   - **The proposal:** a new case, `technical`, mapped to DIN Alternate.
     - DIN Alternate ships with macOS (`/System/Library/Fonts/Supplemental/DIN Alternate Bold.ttf`), so nothing is bundled and no licence file is needed.
     - It falls back to the system sans where the font is missing.
     - Console switches to it.
   - **Without a yes by the start of Task 9,** the row moves to 27.1.0.
9. **`region.set_fade_shape` refuses a crossfaded fade-in** and points to `region.crossfade`'s `shape`. Otherwise: set the crossfade's shape, as the canvas menu does.

## Global Constraints

Every task's requirements include these.

- **Shell.** Every command runs after `export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools` and `cd /Users/lattawb/workspace/vizdaw-fixes`. Never touch `/Users/lattawb/workspace/vizdaw`, which is another worktree with an agent working in it.
- **Swift.**
  - Build only through `scripts/build-ffi.sh` (after any FFI change) and `scripts/swift.sh build|test`, plus `scripts/make-app.sh` in Task 10. Never run `swift`, `xcodebuild` or `xcrun` directly.
  - Swift 6.1:
    - no `[weak self]`, and no non-Sendable value captured inside `MainActor.assumeIsolated`;
    - no `@State` or `@FocusState`: view state lives in the `@Observable` models.
  - Every new control has an accessibility label. Colours, fonts and spacing come from theme tokens.
- **Tests never touch the machine.**
  - They never open real audio or MIDI devices, and never trigger the microphone or TCC prompt.
  - They never touch the real `~/Library`. Use temp folders, `MemoryDefaults()` and fakes.
  - Every `core-ffi` test file calls `force_offline()` through its `serial` helper.
  - Tests that load real plug-ins run in a child process with `HOME` and `CFFIXED_USER_HOME` set to a temp dir, as `crates/core-ffi/tests/params.rs` does.
  - Swift tests that make a real `Session` set `TestSupportDir.shared`.
  - Linux CI has no AU and no CoreFoundation, so such tests are `#[cfg(target_os = "macos")]`.
- **Tools.**
  - Never run `auval`.
  - No GUI, computer-use or accessibility-automation tools. Launch VizDAW only through the e2e with `VIZDAW_AUDIO=offline`. Afterwards `pgrep -lf MacOS/VizDAW` prints nothing.
  - Never use `dangerouslyDisableSandbox`, and never read `~/workspace/SVP/research`.
- **Programmatic calls open nothing.** Control-channel commands and programmatic calls never open dialogs, sheets, alerts, panels, menus or windows. The Built-in Synth window and the destination editor open only from a click or the menu.
- **Hostile input.**
  - New FFI goes inside `ffi_guard` (and so `catch_unwind`).
  - Each new control command gets hostile-input tests: wrong types, unknown params, missing required params, a 70 000-character string, a NUL, and IDs out of range or unknown.
  - The registry count goes up by exactly 4, in `crates/core-control/tests/commands.rs` (186 when written, with its history message) and in `crates/core-control/tests/co_producer.rs`. Add each id to that file's class list.
- **Names.**
  - The repo and UI never name the vendors and DAWs on the project's banned list. That list lives only in the name scans (`app/Tests/VizDAWAppModelTests/NameScanTests.swift`, `crates/core-control/tests/names.rs`). Read them, keep new text clear of the list, and never copy it.
  - Text the app shows says "track", never "lane". Identifiers and JSON keys keep `lane`.
- **`session.rs` stays small.** It gains only `mod fixes;` and the one-line hooks that Tasks 1, 4, 6 and 8 name. Anything longer goes in `session/fixes.rs`.
- **Commits.**
  - Stay on branch `release-fixes` (at `8b05c79` when written). Never switch branches, and never push.
  - Commit at the end of every task by explicit paths, never `git add -A`. Never commit `.superpowers/`, `.DS_Store`, `target/`, generated FFI (`app/Sources/VizDAWCore/vizdaw_ffi.swift`, `app/Sources/vizdaw_ffiFFI/include/`, `target/ffi/`) or `dist/`. Brand assets are never in the repo.
  - Every message ends with exactly these two lines:
    ```
    Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
    Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P
    ```
  - Check each commit with `git log -1 --oneline && git show --stat HEAD`.
  - `Cargo.lock` does not change.
- **Lints.** Every crate keeps `#![deny(missing_docs)]` and the `clippy::{unwrap_used, expect_used, indexing_slicing, panic}` denials outside tests. `core-model` stays free of OS APIs.
- **Format.** `core_model::file::VERSION` stays 9. No task changes the file format.
- **TDD.** Every new or changed test is run, and seen to fail, before the code that makes it pass.
- **Review pace.** In fix rounds, fix only crashes, data loss and wrong overwrites. Narrow edge cases go to the backlog (Task 10).
- **Each task ends green.**
  - Always:
    - `cargo fmt --all -- --check`;
    - `cargo clippy --workspace --all-targets -- -D warnings`;
    - the Linux clippy, `cargo clippy --workspace --all-targets --target x86_64-unknown-linux-gnu --target-dir target/linux-check -- -D warnings`;
    - `cargo test` for the crates the task touches.
  - Tasks that touch FFI or Swift also run `cargo clippy -p core-ffi --lib -- -D warnings`, `scripts/build-ffi.sh`, `scripts/swift.sh build` and `scripts/swift.sh test`.
  - Known flaky: the `damaged_files` and `dawproject_fuzz` proptests, `platform-macos/tests/midi.rs`, `core-ffi` catalog tests under load, and the e2e timing. Re-run once before investigating, and say so in the report.
  - **Baseline.** Measure at the start of Task 1: the registry (186 when written), the last README step (333), the ignored e2e count (32) and `VERSION` (9). Each task reports the counts it adds.

## Review Focus

These are the five failure modes a user is most likely to hit, most likely first. Each is pinned by named tests in its task.

1. **Edits land where nobody can see them.**
   - Examples: ⌥→ or ⌃⌘R in the Mix view with a region selected in Arrange; the same over the channel.
   - Expected: refused with the status line, with nothing changed and no undo step. With the editor shown in the bottom pane they work.
   - Pinned by Task 5's `nudgesWaitWhileTheMixViewHidesTheSelection` and `modifyAgainWaitsInTheMixView`.
2. **A count-in goes wrong across a device change.**
   - Examples:
     - another device is chosen after beat 1 of a four-beat count-in;
     - a sample-rate change mid-count;
     - the output latency drops by two beats.
   - Expected: the remaining beats click once each, recording starts on the bar, and no two clicks share an onset.
   - Pinned by Task 1's `a_rebuild_during_the_count_in_keeps_counting`, `a_resumed_count_in_plays_only_the_beats_left` and `a_latency_drop_of_two_beats_plays_one_click_per_beat`.
3. **The wrong input device after an update.**
   - Examples:
     - settings saved by name only, by an earlier build;
     - two interfaces with the same name;
     - a saved UID that is not connected.
   - Expected:
     - an old name still selects its device;
     - same-named devices are listed apart and kept apart;
     - a missing device shows "(not connected)" and is not replaced silently.
   - Pinned by Task 2's `anOldNameOnlySettingStillSelectsTheDevice`, `twoDevicesWithOneNameAreKeptApart` and `pick_prefers_the_uid_and_refuses_an_ambiguous_name`.
4. **Edge fades change what was already right.**
   - Examples: a region with its own 10 ms fade; a crossfaded start; a bounce of an old project.
   - Expected:
     - a region's own fade and any crossfade sound exactly as before;
     - only edges that meet silence get the 1.5 ms ramp; a split plays seamlessly, and a plain overlap switches instantly;
     - streamed and cached renders still match.
   - Pinned by Task 7's `a_region_fade_overrides_the_edge_fade`, `a_crossfaded_start_gets_no_edge_fade`, `only_edges_that_meet_silence_ramp` and `a_split_region_plays_seamlessly`, and Task 5's proptest run.
5. **Choosing a synth sound does more than it says.**
   - Examples: choosing Drums then Undo; choosing a sound for a destination that is not a Built-in Synth.
   - Expected:
     - Drums plays on channel 10;
     - one Undo brings back both the channel and the program;
     - a non-built-in destination is refused with nothing changed.
   - Pinned by Task 8's `set_builtin_sound_undoes_as_one_step` and `set_builtin_sound_refuses_other_destinations`.

---

### Task 1: The count-in survives a device rebuild, with one click per beat

Rows: "Count-in survives a device rebuild" and "Two count-in clicks in one block after a latency drop".

**Files:**
- Modify:
  - `crates/core-engine/src/count_in.rs` (`Counting::new`'s `done`; the click floor in `render`, L134–151);
  - `crates/core-engine/src/render.rs` (the `Play` handler L1862–1872; publish the clicks started through `Shared`, beside `counting` in `publish` L2436);
  - `crates/core-engine/src/engine.rs` (`EngineCmd::Play`'s `count_in_done`; `PendingPlay::resume_count_in`; `Engine::count_in_progress`);
  - `crates/core-ffi/src/host.rs` (`Host.count_in`; `Rebuild.count_in`; `handle_events` L2473; `rebuild` L2522–2577; `finish_play` L2836; `stop` L2851);
  - `crates/core-engine/tests/count_in.rs`;
  - `crates/core-ffi/tests/transport.rs`.

**Interfaces:**
- Produces (`core-engine`):
  ```rust
  impl Counting {
      /// `done`: clicks already played before a rebuild; clicks 0..done are
      /// skipped and click `done` sounds first, at the start. Accents still
      /// follow the click's index k (k % per_bar == 0).
      pub fn new(c: CountIn, until: Option<u64>, rate: f64, latency: u64, done: u32) -> Self;
  }
  impl PendingPlay {
      /// A count-in resumed after `done` clicks (plan release-fixes Task 1).
      pub fn resume_count_in(&mut self, c: CountIn, done: u32);
  }
  impl Engine {
      /// `Some((count_in, clicks_started))` while counting in, else `None`.
      pub fn count_in_progress(&self) -> Option<(CountIn, u32)>;
  }
  ```
  `EngineCmd::Play` gains `count_in_done: u32`. It is 0 everywhere but `resume_count_in`. `Engine::play()` and `play_until()` do not change, and `session.rs` does not change.
- **The click floor.** In `Counting::render`, click k's onset is `max(click_at(k), from, last_onset + beat_frames)`. So a latency drop can never put two clicks less than a beat apart. `done()` still waits for the last click to finish.
- **The rebuild.**
  - `handle_events` records `count_in: Option<(CountIn, u32)>` in `Rebuild` from `old.engine.count_in_progress()`. When the renderer has not yet applied a pending Play (`want_play`, state `Stopped`, `Host.count_in` set), it records `(c, 0)`.
  - `rebuild` then plays through `begin_play(until)`, `resume_count_in(c, done)` and `finish_play`, in place of `play()`/`play_until()`.
  - An audio record tap still closes on a rebuild, as today (`record.rs` L626–636). MIDI recording continues after the remaining beats.

- [ ] **Step 1: Write the failing engine tests** in `crates/core-engine/tests/count_in.rs`. Use `counted()` and `bursts()`, as `a_latency_change_mid_count_in_neither_repeats_nor_skips_a_click` (L197) does.
  - `a_latency_drop_of_two_beats_plays_one_click_per_beat`:
    - `CountIn { beats: 4, per_bar: 4, beat_frames: 4800.0 }`, with the latency 12000 frames, dropping to 0 after the first burst;
    - exactly 4 bursts;
    - every pair of onsets is at least 4800 frames apart;
    - no 512-frame block holds two onsets.
  - `a_resumed_count_in_plays_only_the_beats_left`:
    - `Counting::new(c, None, 48000.0, 0, 1)` with 4 beats plays 3 bursts, the first at frame 0;
    - with `done = 4`, it rolls at once and plays no burst;
    - if `bursts()` tells accents apart, the first of the three is unaccented.
- [ ] **Step 2: Run** `cargo test -p core-engine --test count_in`. Expected: FAIL (two onsets in one block; `Counting::new` takes 4 arguments).
- [ ] **Step 3: Implement** `done`, the floor and `count_in_progress` (publish the clicks started as an `AtomicU32` beside `counting`), `count_in_done` and `resume_count_in`.
- [ ] **Step 4: Run** `cargo test -p core-engine`. Expected: PASS.
- [ ] **Step 5: Write the failing `core-ffi` test** `a_rebuild_during_the_count_in_keeps_counting` in `crates/core-ffi/tests/transport.rs`. Model it on `a_device_change_rebuilds_and_resumes_at_the_same_position` (L283), with `simulate_event(BackendEvent::DefaultDeviceChanged)`.
  - Record with a 4-beat count-in at 120 BPM on an armed MIDI track.
  - After the first click, simulate the device change.
  - After the rebuild:
    - `counting_in()` is still true;
    - the position is still the record start;
    - the transport rolls only after about 3 more beats, give or take one block;
    - the MIDI pass then records.
  - A second case uses `SampleRateChanged` and asserts the same.
- [ ] **Step 6: Run** `cargo test -p core-ffi --test transport a_rebuild_during`. Expected: FAIL (it rolls at once).
- [ ] **Step 7: Implement** `Host.count_in` (set in `finish_play`, cleared in `stop`), `Rebuild.count_in` and the counted replay in `rebuild`.
- [ ] **Step 8: Run** `cargo test -p core-ffi --test transport --test record`. Expected: PASS, including `stop_during_a_count_in_records_nothing` and `a_device_that_goes_away_ends_the_pass_cleanly`.
- [ ] **Step 9: End-of-task checks.** **Commit** `fix(engine): a count-in survives a device rebuild and never doubles a click (plan release-fixes Task 1)`.

### Task 2: Input devices keyed by UID

Row: "Input devices keyed by UID".

**Files:**
- Modify:
  - `crates/platform-macos/src/hal.rs` (`InputDevice`, `input_device_list`, `pick`, `find_device` L405, the open path L1119–1121, `input_channels_of` L582);
  - `crates/core-engine/src/backend.rs` L59 (doc: "the device's UID, else its name, from older settings");
  - `crates/core-ffi/src/host.rs` (`InputDeviceRow`, `audio_input_device_list`; `set_audio_input_device` L490 keeps its signature and takes a UID or a name);
  - `crates/core-control/src/registry.rs` L1381 (`input_device`'s help: "The input device's uid, or its name if no other device has that name.");
  - `app/Sources/VizDAWAppModel/AudioPreferences.swift` (`inputDeviceUID`; `setInputDevice`; `applyAll`);
  - `app/Sources/VizDAWAppModel/AudioReadouts.swift` (`inputDeviceList`; `inputDeviceChoices`);
  - `app/Sources/VizDAW/AudioSettingsView.swift` L17–25 (tag by UID);
  - `app/Sources/VizDAWAppModel/AudioCommands.swift` L62–65;
  - wherever `state.get audio` lists `input_devices` (it gains `input_device_list`);
  - tests: `crates/core-ffi/tests/audio_settings.rs`, `app/Tests/VizDAWAppModelTests/AudioPreferencesTests.swift`, `AudioReadoutsTests.swift`, `AudioCommandsTests.swift`.

**Interfaces:**
- Produces (Rust):
  ```rust
  // platform-macos hal.rs
  pub struct InputDevice { pub name: String, pub uid: String }
  pub fn input_device_list() -> Vec<InputDevice>;
  /// The device whose uid is `key`, else the only device named `key`.
  /// Two devices with that name and no uid match is None.
  pub fn pick<'a>(list: &'a [InputDevice], key: &str) -> Option<&'a InputDevice>;
  // core-ffi host.rs
  #[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
  pub struct InputDeviceRow { pub name: String, pub uid: String }
  #[uniffi::export] pub fn audio_input_device_list() -> Vec<InputDeviceRow>; // empty offline
  ```
  The old `audio_input_devices() -> Vec<String>` stays.
- Produces (Swift):
  ```swift
  // AudioSettings: inputDeviceUID: String? (key "inputDeviceUID"); inputDevice (the name) stays,
  // for "(not connected)" titles and old settings.
  // AudioReadouts
  static var inputDeviceList: () -> [InputDeviceRow]   // test seam, default audioInputDeviceList()
  static func inputDeviceChoices() -> [(title: String, uid: String?, name: String?)]
  ```
- **Picker titles.** Two devices with one name are titled "<name>" and "<name> (2)", in list order. A saved device that is not listed shows "<name> (not connected)".
- **`applyAll` and `setInputDevice`** send the UID if one is set, else the name.
- **`audio.set input_device`** resolves a UID first, then a unique name. An ambiguous name is `failed` with "Two input devices are named “<name>”: give its uid (state.get audio input_device_list)."

- [ ] **Step 1: Write the failing Rust test** `pick_prefers_the_uid_and_refuses_an_ambiguous_name`. It is a unit test in `hal.rs`, pure data, with no HAL call.
  - The list is `[{A, u1}, {A, u2}, {B, u3}]`:
    - `pick("u2")` gives the second device;
    - `pick("B")` gives u3;
    - `pick("A")` is `None`;
    - `pick("")` and `pick("a\u{0}")` are `None`.
- [ ] **Step 2: Write the failing Swift tests.**
  - `AudioPreferencesTests.anOldNameOnlySettingStillSelectsTheDevice`: `{"inputDevice": "Desk"}` decodes with `inputDeviceUID == nil`, and `applyAll` sends "Desk".
  - `AudioPreferencesTests.inputDeviceUidIsKept`: `setInputDevice(uid: "u2", name: "A")` stores both, and `applyAll` sends "u2".
  - `AudioReadoutsTests.twoDevicesWithOneNameAreKeptApart`:
    - with the seam giving `[A/u1, A/u2]`, the titles are `["A", "A (2)"]` and the UIDs are `["u1", "u2"]`;
    - a saved `u9` named "Old" adds "Old (not connected)".
  - `AudioCommandsTests.audioSetRefusesAnAmbiguousDeviceName`: `audio.set {input_device: "A"}` is `.failed` with the sentence above, and `{input_device: "u2"}` is `.done`.
- [ ] **Step 3: Run** `cargo test -p platform-macos --lib pick` and `scripts/swift.sh test --filter 'AudioPreferencesTests|AudioReadoutsTests|AudioCommandsTests'`. Expected: FAIL.
- [ ] **Step 4: Implement** the list, `pick` (used by `find_device`, the open path and `input_channels_of`), the FFI row and export, and the Swift side. `uid_of` (hal.rs L177) becomes the source of `uid`.
- [ ] **Step 5: Run** the Step 3 commands, then `cargo test -p core-ffi --test audio_settings` (`audio_input_device_list()` is empty offline). Expected: PASS.
- [ ] **Step 6: End-of-task checks**, including the Swift build and test. **Commit** `fix(audio): input devices are kept by UID, with names for older settings (plan release-fixes Task 2)`.

### Task 3: The MIDI source list and the arm's claim leave the main thread

Rows: "Input pickers' MIDI source list off the main thread" and "Arm's engine claim off the session lock".

**Files:**
- Modify:
  - `app/Sources/VizDAWAppModel/LaneInputs.swift` (`midiPorts` L45, `deviceChannels` L51, `refreshInfo` L73–77, `groups(for:)` L93/L120);
  - `app/Sources/VizDAW/StripInputView.swift` L16, L43, L47;
  - `app/Sources/VizDAWEditors/LaneHeaderView.swift` `inputMenu` L480–485;
  - `crates/core-ffi/src/host.rs` (`claim_host` L2661–2739, `retire` L1265, the `rebuild` call at L2706; `RETIRING`, `reap_retired`, `retiring_count`);
  - `app/Tests/VizDAWAppModelTests/LaneInputsTests.swift`;
  - `crates/core-ffi/tests/engine_claim.rs`.

**Interfaces:**
- Produces (Swift):
  ```swift
  // LaneInputs
  nonisolated(unsafe) static var midiPorts: @Sendable () -> [String]   // was MainActor-bound
  nonisolated(unsafe) static var deviceChannels: @Sendable () -> UInt32
  public private(set) var midiSources: [String]      // the cache groups(for:) reads
  public private(set) var deviceChannelCount: UInt32
  /// Reads both off the main thread (Task.detached), then sets the cache on the main actor.
  public func refreshSources() async
  ```
  - `groups(for:)` reads only the cache.
  - `refreshInfo()` starts `Task { await refreshSources() }`.
  - The strip's `.onAppear`/`.onHover` and the header's `inputMenu` call `refreshInfo()` as today. The menu shows the cache, which can be one refresh old.
- Produces (`core-ffi` host.rs):
  ```rust
  /// Hosts waiting out their drain: (host, drop after). Dropped on the
  /// main thread by reap_retired, never on a background thread.
  static RETIRING: Mutex<Vec<(Host, Instant)>>;
  pub(crate) fn reap_retired();
  #[doc(hidden)] pub fn retiring_count() -> usize;
  ```
  - With `Idle::Untouched`, `claim_host` parks the old host in `RETIRING` (deadline now + `DRAIN`) in place of `retire`'s and `rebuild`'s sleep.
  - It connects MIDI sources through `listen_to_midi_sources_soon` (L425, already threaded) in place of the inline `connect_midi_sources`.
  - `reap_retired()` runs at the top of `claim_host` and from `host::state()`, which `Session::idle` already reaches. Confirm that call path, and if it does not reach `host::state()`, add `host::reap_retired()` beside the owed-claim retry (session.rs L2715), one line.
  - Other `Idle` cases keep their drain as today.

- [ ] **Step 1: Write the failing Swift test** `LaneInputsTests.theSourceListIsReadOffTheMainThread`.
  - `midiPorts` is swapped for a closure that records `Thread.isMainThread` and returns `["Keys", "Pads"]`, as `midiChoicesAreSourcesAndChannels` (L122) does.
  - `groups(for:)` before any refresh does not call the closure, and lists no source.
  - After `await inputs.refreshSources()`:
    - the closure ran once, off the main thread;
    - `groups(for:)` lists "Keys" and "Pads";
    - a second `groups(for:)` does not call it again.
  - Update `midiChoicesAreSourcesAndChannels`, `aSurfacesPortIsMarkedInThePicker` and `thePickerListsTheDevicesChannelsBeforeTheEngine` to `await refreshSources()` first, and name them in the report.
- [ ] **Step 2: Write the failing Rust test** `arming_while_an_engine_retires_does_not_wait_for_the_drain` in `engine_claim.rs`, with `arm()` and `wait_until`.
  - Set up the state in which arming retires an idle host, as `arming_claims_an_idle_engine_and_hears_the_input` (L120) does.
  - `claim_for_input` returns in under 150 ms (`DRAIN` is 300 ms).
  - The input is heard once `wait_until` sees the claim.
  - `retiring_count()` goes back to 0 after the drain, through idle ticks.
- [ ] **Step 3: Run** `scripts/swift.sh test --filter LaneInputsTests` and `cargo test -p core-ffi --test engine_claim`. Expected: FAIL.
- [ ] **Step 4: Implement** the cache and the parked retire.
- [ ] **Step 5: Run** the Step 3 commands. Expected: PASS, including `an_arm_during_another_windows_open_claims_once_it_ends` and `a_redone_arm_is_heard_before_play`. Then `cargo test -p core-ffi --test transport`, where `a_device_rebuild_drains_without_holding_the_host_lock` must still pass.
- [ ] **Step 6: End-of-task checks**, including the Swift build and test. **Commit** `fix(perf): the MIDI source list and the arm's claim leave the main thread (plan release-fixes Task 3)`.

### Task 4: Automation: no lanes for non-automatable parameters, `automation.release_all`, and the mixer shows heard values

Rows: "No automation lanes for non-automatable plugin parameters", "automation.release_all / stuck channel touch" and "Mixer controls show the heard (automated or held) value while playing".

**Files:**
- Create: `crates/core-ffi/src/session/fixes.rs`, which this task starts and Tasks 6–8 add to.
- Modify:
  - `crates/core-ffi/src/session.rs`, with two one-line changes:
    - `mod fixes;`;
    - first line of `write_mode_of` (L7946): `if !self.may_write(inner, t) { return None; }`;
  - `crates/core-ffi/src/automation.rs` (`ParamInfo.automatable` L933, filled in `param_info` L1009–1024);
  - `crates/core-control/src/registry.rs` (`automation.release_all`, beside `automation.gesture` L2463);
  - `crates/core-control/tests/commands.rs`, `co_producer.rs` (count +1; the id in `CHANGE`);
  - `app/Sources/VizDAWAppModel/AutomationCommands.swift` (`releaseAll` id, `ids`, `run`);
  - `app/Sources/VizDAWAppModel/MixerModel.swift` (`heard`; `pollAutomation` L706; `gainDb` L684, `pan` L689, `sendLevelDb` L697);
  - `app/Tests/VizDAWAppModelTests/FakeSession.swift`, `AutomationCommandsTests.swift`, `MixerGestureTests.swift`;
  - tests: `crates/core-ffi/tests/automation.rs`, `crates/core-ffi/tests/holders.rs`.

**Interfaces:**
- Produces (`session/fixes.rs`):
  ```rust
  impl Session {
      /// False for a PluginParam its plug-in marks non-automatable: the
      /// touch then takes the static/live path, never Write (no lane).
      pub(super) fn may_write(&self, inner: &Inner, t: AutomationTarget) -> bool;
  }
  #[uniffi::export]
  impl Session {
      /// Releases every touch the control channel holds (Decision 2).
      /// Returns how many were released. Ends an open static edit as a
      /// release does, so the step commits.
      pub fn release_channel_touches(&self) -> Result<u32, VizError>;
      /// While playing: each target's heard value (surface_existing +
      /// surface_heard, Ruling 26 of plan 5e-1). Stopped: all None.
      /// An unknown target is None; more than 4096 targets is Err(Invalid).
      pub fn heard_values(&self, targets: Vec<AutomationTargetRow>) -> Result<Vec<Option<f64>>, VizError>;
  }
  ```
- **The registry entry.** `automation.release_all` has no params and is Class `Change`, `document: true`, `key_only` (no menu item). Its help is "Release every touch the control channel holds, e.g. after a script stopped mid-gesture."
  - It answers status "Released 1 touch.", "Released <n> touches." (n ≥ 2) or "The channel holds no touch." (0).
  - It applies the snapshot afterwards when the version changed, as `runGesture` does.
- **The mixer.** `MixerModel.heard: [AutomationTargetRow: Double]` is filled by `pollAutomation` at 30 Hz while the transport plays, and emptied when it stops. Each of the three getters reads `live[t] ?? heard[t] ?? snapshot`.

- [ ] **Step 1: Write the failing `core-ffi` tests.**
  - `automation.rs` `a_non_automatable_parameter_never_writes_a_lane`:
    - runs in the child-process harness `params.rs` uses (L220, `a_plugin_windows_gesture_writes_its_parameter`), with the test plug-in's `PARAM_WIGGLE` (no flags, `crates/test-plugins/src/plugin.rs:262–295`);
    - the strip is in Touch mode and playing;
    - begin, value 0.7, end on `PluginParam(slot, WIGGLE)`;
    - no automation lane exists afterwards;
    - the plug-in saw 0.7;
    - the same gesture on an automatable parameter makes a lane, as a control.
  - `holders.rs` `release_all_frees_only_the_channels_touches`:
    - stopped transport;
    - the channel holds the fader of strip A (a static touch, value −6 dB), and the mouse holds strip B's pan;
    - `release_channel_touches()` is `Ok(1)`;
    - `automation_state().touched` holds only B's pan;
    - the undo label is "Set Fader", and `undo` works (no longer "nothing to undo");
    - a second call is `Ok(0)`.
  - `holders.rs` `heard_values_follow_automation_while_playing`:
    - a Read lane on strip A's fader at −12 dB;
    - while playing, `heard_values([fader A, pan A])` gives `[Some(-12.0 ± 0.01), Some(static pan)]`;
    - stopped, it gives `[None, None]`;
    - 4097 targets is `Err`;
    - an unknown strip is `None`.
- [ ] **Step 2: Run** `cargo test -p core-ffi --test automation --test holders`. Expected: FAIL.
- [ ] **Step 3: Implement** `ParamInfo.automatable`, the module, the two hooks and the two exports, each inside `ffi_guard`.
- [ ] **Step 4: Run** `cargo test -p core-ffi`. Expected: PASS, including `hostile_gestures_are_refused`.
- [ ] **Step 5: Write the failing command and Swift tests.**
  - `commands.rs`:
    - the count +1, with "; release-fixes adds automation.release_all" appended to the message;
    - `release_all_takes_no_params`: `{}` parses, and `{x: 1}` is `invalid_params`.
  - `co_producer.rs`: the count +1, and the id in `CHANGE`.
  - `AutomationCommandsTests.releaseAllReleasesTheChannelsTouches`:
    - the fake records the call, and the answers are status "Released 2 touches." and "The channel holds no touch." for 0;
    - `{"x": 1}` and `{"all": "\u{0}"}` are `.invalidParams`, with no call made.
  - `MixerGestureTests.aPlayingFaderShowsItsHeardValue`:
    - the fake plays and returns −12 for fader 1;
    - after `pollAutomation()`, `gainDb(1) == -12`;
    - a live touch at −3 wins;
    - after Stop and a poll, the snapshot value is back.
- [ ] **Step 6: Run** `cargo test -p core-control --test commands --test co_producer` and `scripts/build-ffi.sh && scripts/swift.sh test --filter 'AutomationCommandsTests|MixerGestureTests'`. Expected: FAIL, then implement the registry entry, `AutomationCommands.releaseAll`, the fake's methods and `heard`, and run again. Expected: PASS.
- [ ] **Step 7: End-of-task checks**, including the Swift build and test. **Commit** `fix(automation): no lanes for non-automatable parameters, automation.release_all, and the mixer shows heard values (plan release-fixes Task 4)`.

### Task 5: The streamed render after a locate, and no hidden edits from the Mix view

Rows: "streamed_render_equals_cached_render seed after a locate" and "Menu edits in the Mix view act on the hidden selection".

**Files:**
- Modify:
  - `app/Sources/VizDAWAppModel/DocumentModel.swift` (`selectionHidden`, `hiddenSelection`);
  - `app/Sources/VizDAW/AppCommands.swift` (the nudges L192–195, Modify Again L206–213);
  - `app/Sources/VizDAWAppModel/ControlCommands.swift` (the nudge case L549, `modify.again` L558);
  - `app/README.md` step 200;
  - tests: `app/Tests/VizDAWAppModelTests/NudgeTests.swift`, `ViewCommandsTests.swift`.
- Only if Step 1 fails: `crates/core-media/src/stream.rs` and `crates/core-engine/tests/audio_playback.proptest-regressions`.

**Interfaces:**
- Produces (Swift):
  ```swift
  // DocumentModel
  /// True in the Mix view when the bottom pane shows neither the Editor nor
  /// the Audio Editor (Decision 5): views.main == .mix && !(views.mix.bottom.shown
  /// && [.editor, .audioEditor].contains(views.mix.bottom.view)).
  public var selectionHidden: Bool { get }
  public static let hiddenSelection =
      "The Mix view shows no editor: press ⌃1, or show the Editor in the bottom pane, to edit the selection."
  ```
  - The four nudge items and Modify Again are disabled while `selectionHidden`.
  - The channel's nudge commands and `modify.again` answer `ControlCommands.unavailable(DocumentModel.hiddenSelection)`.
  - A key press shows `hiddenSelection` in the status line.

- [ ] **Step 1: Verify the streamer.** Run `PROPTEST_CASES=2000 cargo test -p core-engine --test audio_playback -- streamed_render_equals_cached_render a_locate_past_a_region_keeps_its_loop_seam`. Expected: PASS. The row is then closed in Task 10 as "fixed by `ec7ea23`: the streamed render was wrong after a locate". If it fails, use superpowers:systematic-debugging:
  - add the shrunk seed to the regressions file;
  - pin it as a named `#[test]`;
  - fix the streamer in `stream.rs`;
  - report which side was wrong.
- [ ] **Step 2: Write the failing Swift tests.**
  - `NudgeTests.nudgesWaitWhileTheMixViewHidesTheSelection`. Use a document with a selected region and run `ui.view_mix`:
    - each of the nudge commands over `ControlCommands` is `.failed(unavailable)` with `hiddenSelection`;
    - the fake records no edit;
    - with `views.mix.bottom = shown, .editor`, the same nudge sends its edit.
  - `ViewCommandsTests.modifyAgainWaitsInTheMixView`:
    - after one Modify, `ui.view_mix` makes `modify.again` refused with `hiddenSelection`, with no edit;
    - after `ui.view_arrange`, it runs.
  - `ViewCommandsTests.selectionHiddenFollowsTheView`: arrange gives false; mix with no bottom pane gives true; mix with the Audio Editor shown gives false.
- [ ] **Step 3: Run** `scripts/swift.sh test --filter 'NudgeTests|ViewCommandsTests'`. Expected: FAIL.
- [ ] **Step 4: Implement** `selectionHidden`, the menus' disabling and the two channel cases. Amend README step 200 with: "In the Mix view with no editor showing, the nudges and Modify Again are greyed out, and their keys say why in the status line."
- [ ] **Step 5: Run** the Step 3 command. Expected: PASS.
- [ ] **Step 6: End-of-task checks**, including the Swift build and test. **Commit** `fix(mix): no edits to a selection the Mix view hides; the streamer check after a locate (plan release-fixes Task 5)`.

### Task 6: Region names, the audio output picker, and track colours on surface LEDs

Rows: "Draw region names in the arrangement", "An output picker on an audio track's header" and "User track colours".

**Files:**
- Modify:
  - `app/Sources/VizDAWEditors/RegionDrawing.swift` (`draw` L85–158 gains `name:`; `nameLabelRect`);
  - `app/Sources/VizDAWEditors/ArrangementView.swift` `drawRegions` L263–286;
  - `app/Sources/VizDAWEditors/LaneHeaderView.swift` (`outputMenu`; `mouseDown` L336);
  - `crates/core-ffi/src/session/fixes.rs` (`output_paths`);
  - `crates/core-ffi/src/mixer.rs` (`OutputPathRow`);
  - `crates/core-control/src/registry.rs` (`lane.set_output`, beside `lane.set_input` L2354);
  - `app/Sources/VizDAWAppModel/TrackCommands.swift` or the file holding `lane.set_input`'s handler (`lane.set_output`);
  - `crates/core-surface/src/lib.rs` `StripView` L225 (`colour`);
  - `crates/core-surface/src/kontrol.rs` (`swatch_color`; `send_state` L759–763);
  - `crates/core-ffi/src/session.rs` `surface_view` L12537–12550, one line: `colour: s.colour,`;
  - the other `StripView` constructors: `crates/core-ffi/src/surfaces.rs:2754` and `crates/core-surface/tests/common/mod.rs`;
  - `app/README.md` step 266;
  - tests: `app/Tests/VizDAWEditorsTests/RegionEditingTests.swift`, `app/Tests/VizDAWAppModelTests/LaneHeaderInputTests.swift`, `crates/core-surface/tests/kontrol_feedback.rs`, `crates/core-ffi/tests/tracks.rs` (or the file holding the lane edit tests), `crates/core-control/tests/commands.rs`, `co_producer.rs`.

**Interfaces:**
- Produces (Swift, `VizDAWEditors`):
  ```swift
  // RegionDrawing.draw(..., name: String? = nil, ...): the arrangement passes
  // RegionA11y.name(r, mediaName: media.row(r.media)?.name); the strip and the audio editor pass nil.
  /// The name's rect at the region's top left, after the fade-in handle and
  /// under the gain band (as offlineLabelRect). The text is truncated with "…";
  /// nil when fewer than 3 characters fit.
  public static func nameLabelRect(_ name: String, in rect: CGRect, font: NSFont) -> CGRect?
  ```
  - The ink is `theme.legend(on:)` of the region's fill.
  - An offline region draws its name, then the "Offline" pill. When both do not fit, only the pill is drawn.
- Produces (`core-ffi`):
  ```rust
  /// One output path of the document, in id order.
  #[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
  pub struct OutputPathRow { pub id: u64, pub name: String }
  #[uniffi::export] impl Session { pub fn output_paths(&self) -> Result<Vec<OutputPathRow>, VizError>; } // doc.outputs()
  ```
- **The output menu.** `LaneHeaderView.outputMenu(seq: UInt64, lane: LaneRow) -> NSMenu` holds "No Output", a separator, then each path by name, with `lane.audioOut` checked. Choosing one runs `.lane(edit: .setOutput(seq:lane:output:))`, one undo step "Set Track Output". It has the busy and failed items of `destinationMenu` (L445–478). A click on the "→ …" destination area of an audio header opens it.
- **The `lane.set_output` command.**
  - Params: `seq` and `lane` (`opt_id_param`, defaulting as `lane.set_input` does), and `output` (an id, or JSON `null` for none). Class `Change`, `document: true`, `key_only`.
  - It answers status with the path's name, or "No Output".
  - An unknown output is `unknown_id` "There is no output <n>."
  - A MIDI or instrument track is `failed` "Only an audio track has an output."
- Produces (`core-surface`):
  ```rust
  // StripView gains:
  /// The strip's colour, if the user chose one (plan 7b C-1).
  pub colour: Option<Swatch>,
  /// A fixed LED hue per swatch (Decision 6).
  pub fn swatch_color(s: Swatch) -> &'static str;
  ```
  The table is red "#E6463C", orange "#E68C28", yellow "#E6C83C", green "#46B45A", teal "#28B4B4", blue "#3C8CE6", purple "#9650DC" and pink "#E650A0". `send_state` sends `swatch_color` when `colour` is set, else `color(kind)`.

- [ ] **Step 1: Write the failing Swift tests.**
  - `RegionEditingTests.nameLabelRectFitsOrHides`:
    - in a 200×60 rect, "Vocal take" gives a rect inside the region, starting after the fade-in handle;
    - in a 20×60 rect it is nil;
    - "" is nil.
  - `RegionEditingTests.theArrangementDrawsTheRegionsName`. Use whatever seam the `offlineLabelRect` tests (L196–205) use:
    - a renamed region draws its own name;
    - an unnamed one draws its file's name.
  - `LaneHeaderInputTests.anAudioHeadersOutputClickOffersThePaths` replaces `anAudioHeaderShowsItsOutputAndItsClickDoesNothing` (L130–150):
    - the fake's `outputPaths` is `[(5, "Audio 1"), (6, "Audio 2")]`;
    - the menu titles are `["No Output", "Audio 1", "Audio 2"]`, with "Audio 1" checked;
    - choosing "Audio 2" sends `setOutput(seq:lane:output: 6)`;
    - "No Output" sends `nil`.
  - `TrackCommandsTests.setOutputRefusesHostileInput`, or the matching file:
    - `output: 999` is `.unknownId`;
    - `output: "5"`, `-1` and `1e30` are `.invalidParams`;
    - on a MIDI track it is `.failed` with the sentence above;
    - while recording it is `.busy`;
    - no edit is sent for any of these.
- [ ] **Step 2: Write the failing Rust tests.**
  - `kontrol_feedback.rs` `a_coloured_strip_lights_its_colour`: a strip with `colour: Some(Swatch::Purple)` sends `sysex(0x4B, 0, slot, b"#9650DC")`. The existing `#46B45A` case (L59) still passes with `colour: None`.
  - `core-ffi` `output_paths_list_the_documents_paths`: two paths in id order with their names, and `edit(Lane(SetOutput{output: Some(u64::MAX)}))` is `Err` with the document unchanged.
  - `commands.rs` `set_output_checks_its_params`:
    - `{output: 3}` and `{output: null}` parse;
    - missing `output`, `"3"`, `true`, `-1`, a 70 000-character string and an unknown param are each `invalid_params`.
  - Both counts +1, and the id in `CHANGE`.
- [ ] **Step 3: Run** `cargo test -p core-surface -p core-control` and the new `core-ffi` test, then `scripts/build-ffi.sh && scripts/swift.sh test --filter 'RegionEditingTests|LaneHeaderInputTests|TrackCommandsTests'`. Expected: FAIL.
- [ ] **Step 4: Implement** the name drawing, `output_paths`, `outputMenu`, `lane.set_output`, `StripView.colour` (with the one `session.rs` line) and `swatch_color`. Amend README step 266 to "…the arrangement shows the new name at the region's top left; clearing it shows the file's name again."
- [ ] **Step 5: Run** the Step 3 commands. Expected: PASS.
- [ ] **Step 6: End-of-task checks**, including the Swift build and test. **Commit** `feat(arrange): region names, an output picker on audio tracks, and track colours on surface LEDs (plan release-fixes Task 6)`.

### Task 7: Edge fades, zero-crossing snapping, and `region.set_fade_shape`

Rows: "Zero-crossing snapping and edge fades for audio edits" and "A control-channel path to set a fade shape".

**Files:**
- Modify:
  - `crates/core-engine/src/schedule/audio.rs` (`EDGE_FADE_SECONDS`; `Segment.edge_frames`; `build` L293–378; `gain_at` L69–87);
  - `crates/core-ffi/src/session/fixes.rs` (`zero_crossing`);
  - `app/Sources/VizDAWAppModel/Nudge.swift` `EditingPreferences` L78–124 (`zeroCrossings`);
  - `app/Sources/VizDAW/EditingSettingsView.swift` (the toggle, after "Crossfades" L103–113);
  - `app/Sources/VizDAWEditors/AudioEditorGeometry.swift` `trimCommand` L147–170 (`snap:`), and its callers;
  - `app/Sources/VizDAWAppModel/AudioEditorModel.swift` L273–287;
  - `crates/core-control/src/registry.rs` (`region.set_fade_shape`, beside `region.crossfade` L2066–2108);
  - `app/Sources/VizDAWAppModel/RegionCommands.swift` (the id; `runChannel` L215–290);
  - tests: `crates/core-engine/tests/schedule_audio.rs`, `crates/core-ffi/tests/` (a new `zero_crossing.rs`), `app/Tests/VizDAWAppModelTests/NudgeTests.swift`, `RegionCommandsTests.swift`, `app/Tests/VizDAWEditorsTests/` (the trim-geometry tests), `crates/core-control/tests/commands.rs`, `co_producer.rs`.
- Create: `app/Sources/VizDAWAppModel/ZeroCrossing.swift`.

**Interfaces:**
- Produces (`core-engine`):
  ```rust
  /// The automatic fade at a hard region edge (plan release-fixes Task 7).
  pub const EDGE_FADE_SECONDS: f64 = 0.0015;
  ```
  - `Segment` gains `edge_frames: u64`, which is `round(rate * EDGE_FADE_SECONDS)` and is set in `build`.
  - `gain_at` multiplies by a linear ramp over `edge_frames` at the region's start, when `fade_in.length_frames == 0` and the start meets silence (`ramp_start`). It does the same at the end, when `fade_out.length_frames == 0` and the end meets silence (`ramp_end`).
  - **Fix round 1 (I1, controller ruling).** `build` sets `ramp_start`/`ramp_end` false at a crossfaded edge, and at any written edge that butts against or is overlapped by another piece in the same group, within ±1 frame (split rounding). Joins switch instantly, as before: a split of continuous media renders sample for sample as the unsplit region, and a plain overlap's later region starts at full level. The earlier region is never ramped out at the cut (that would still notch a split). Tests: `only_edges_that_meet_silence_ramp` (schedule), `a_split_region_plays_seamlessly` and the restored `a_region_without_a_crossfade_renders_as_before` (render).
  - A region's own fade of any length replaces the edge fade.
  - `overviews.rs` `fade_gain` does not change (Scope 2).
  - Existing tests that pin a hard first or last sample get the ramp. List them in the report.
- Produces (`session/fixes.rs`):
  ```rust
  #[uniffi::export]
  impl Session {
      /// The frame of `media` nearest `frame` where the channel-summed signal
      /// changes sign (s[k-1] and s[k] differ in sign, or s[k] == 0), within
      /// `radius` frames each side; ties go to the earlier frame. None found,
      /// or the media offline: `frame`. `frame` past the end clamps to the length.
      /// Unknown media: UnknownId; radius > 48_000: Invalid. Reads with
      /// CafReader as Normalize does (session.rs ~6431).
      pub fn zero_crossing(&self, media: u64, frame: u64, radius: u64) -> Result<u64, VizError>;
  }
  ```
- Produces (Swift):
  ```swift
  // EditingPreferences: zeroCrossings: Bool, default true, stored like crossfadeMs.
  @MainActor public enum ZeroCrossing {
      public static let radiusSeconds = 0.003
      /// Identity when the preference is off; else session.zeroCrossing(media:frame:radius:)
      /// with radius = radiusSeconds × the media's rate; an error keeps the frame.
      public static func snapper(_ m: DocumentModel, media: UInt64) -> (UInt64) -> UInt64
  }
  // AudioEditorGeometry.trimCommand(..., snap: (UInt64) -> UInt64 = { $0 }) snaps the
  // region's new start and end, in media frames, before it computes front/back.
  ```
  - The Settings toggle is "Snap audio edits to zero crossings", with the help "Moves a trimmed start or end to the nearest zero crossing within 3 ms, so the cut does not click."
  - The accessibility label is the same as the title.
- **The `region.set_fade_shape` command.**
  - Params: `seq`, `lane` and `event` (optional and all-or-none, as `CROSSFADE_PARAMS`), `fade` (Choice `["in", "out"]`, required) and `shape` (Choice `["linear", "equal_power", "exponential", "s_curve"]`, required). Class `Change`, `document: true`, `key_only`.
  - It sends `AudioEdit::SetFades` with the lengths unchanged, one step "Set Region Fades".
  - It answers status "Fade In: S-Curve", with the shape's menu title.
  - A crossfaded start with `fade: in` is `failed` "This region's start is a crossfade: set its shape with region.crossfade." (Decision 9).

- [ ] **Step 1: Write the failing engine tests** in `schedule_audio.rs`, beside `fades_are_relative_to_the_region_as_written` (L232). Use a DC-1.0 region at 48 kHz, so `edge_frames == 72`.
  - `a_region_without_fades_ramps_its_edges`:
    - the gain at frame 0 is 0;
    - at frame 36 it is 0.5 ± 0.02;
    - at frame 72 and after, it is 1.0;
    - the last frame's gain is below 0.02.
  - `a_region_fade_overrides_the_edge_fade`: with a 10 ms Linear fade-in, the gain at frame 72 is `72/480 ± 0.001`, not 1.0.
  - `a_crossfaded_start_gets_no_edge_fade`: with two regions crossfaded as `crossfades.rs` builds them, the incoming region's start follows the crossfade curve only (Ruling 8).
- [ ] **Step 2: Run** `cargo test -p core-engine --test schedule_audio`. Expected: FAIL.
- [ ] **Step 3: Implement** the edge fade. Run `cargo test -p core-engine`, updating any test that pinned a hard edge, and `PROPTEST_CASES=500 cargo test -p core-engine --test audio_playback`. Expected: PASS.
- [ ] **Step 4: Write the failing `core-ffi` test** `zero_crossing.rs` `zero_crossing_finds_the_nearest_sign_change`. Use a pool item written as a CAF of a 100 Hz sine at 48 kHz, as the Normalize tests write one. Its crossings fall every 240 frames.
  - `zero_crossing(m, 250, 144)` is `240`;
  - `zero_crossing(m, 250, 5)` is `250`;
  - a stereo file with opposite-phase channels sums to 0, so it gives `250`;
  - `radius: 48_001` is `Err`;
  - an unknown media is `Err`;
  - a frame past the end is clamped.
- [ ] **Step 5: Write the failing Swift tests.**
  - `NudgeTests.zeroCrossingsDefaultOnAndKept`: the default is true; it stores false; a damaged value reads as true.
  - The geometry test `trimSnapsBothEdges`: `trimCommand` with `snap: { $0 / 100 * 100 }` gives front and back from the snapped frames.
  - `RegionCommandsTests.setFadeShapeKeepsTheLength`: on a region with a 10 ms fade-out, `{fade: "out", shape: "s_curve"}` sends `setFades` with fade_out length unchanged and shape `.sCurve`, and answers "Fade Out: S-Curve".
  - `RegionCommandsTests.setFadeShapeRefusesHostileArguments`:
    - a missing `fade` or `shape`;
    - `fade: "middle"`;
    - `shape: 3`;
    - a 70 000-character `shape`;
    - a NUL;
    - only `seq` given;
    - an unknown param;
    - `event: 999` (`unknown_id`);
    - the crossfaded case (`failed`);
    - no edit is sent for any of these.
  - `commands.rs` `set_fade_shape_checks_its_params`, modelled on `crossfade_commands_check_their_params` (L2065–2130). Both counts +1, and the id in `CHANGE`.
- [ ] **Step 6: Run** `cargo test -p core-ffi --test zero_crossing`, `cargo test -p core-control`, and `scripts/build-ffi.sh && scripts/swift.sh test --filter 'NudgeTests|RegionCommandsTests|AudioEditorGeometry'`. Expected: FAIL.
- [ ] **Step 7: Implement** `zero_crossing`, the preference, the toggle, `ZeroCrossing`, the `snap:` parameter and its callers (the drag and `AudioEditorModel`'s typed trim), and `region.set_fade_shape`.
- [ ] **Step 8: Run** the Step 6 commands. Expected: PASS.
- [ ] **Step 9: End-of-task checks**, including the Swift build and test. **Commit** `feat(audio): edge fades, zero-crossing snapping for trims, and region.set_fade_shape (plan release-fixes Task 7)`.

### Task 8: Built-in Synth sounds and its window, and the destination editor

Rows: "Built-in Synth sounds and a window" and "Destination editor (return latency, Measure)".

**Files:**
- Create:
  - `crates/core-ffi/src/builtin_sound.rs` (`BuiltinSound`);
  - `app/Sources/VizDAW/BuiltinSynthWindow.swift`;
  - `crates/core-ffi/tests/builtin_sound.rs`.
- Modify:
  - `crates/core-ffi/src/lib.rs` (`mod builtin_sound;`);
  - `crates/core-ffi/src/session/fixes.rs` (`set_builtin_sound`);
  - `crates/core-ffi/src/mixer.rs` `DestRow` L193 (`builtin_sound`);
  - `crates/core-ffi/src/session.rs` `destinations()` L3302–3331, one line filling `builtin_sound`;
  - `crates/core-control/src/registry.rs` (`destination.set_sound`, beside `destination.set_return_latency` L2401);
  - `app/Sources/VizDAWAppModel/SynthSheets.swift` (`sound`, `setSound`, `EditDestinationDraft`, `beginEditDestination`, `commitLatency`);
  - `app/Sources/VizDAWAppModel/SynthCommands.swift` (`destination.set_sound`);
  - `app/Sources/VizDAW/MixerView.swift` `BuiltinSynthSlotView` L406–433 (a double-tap opens the window);
  - `app/Sources/VizDAW/SynthSheetViews.swift` (`DestinationEditorSheet`);
  - `app/Sources/VizDAW/DocumentView.swift` L84–96 (attach the sheet);
  - `app/Sources/VizDAWEditors/LaneHeaderView.swift` `laneMenu` L382–396 ("Edit Destination…");
  - tests: `app/Tests/VizDAWAppModelTests/SynthSheetsTests.swift`, `SynthCommandsTests.swift`, `FakeSession.swift`, `crates/core-control/tests/commands.rs`, `co_producer.rs`.

**Interfaces:**
- Produces (`core-ffi`):
  ```rust
  /// A Built-in Synth sound (plan release-fixes Scope 1): a GM program and channel.
  #[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
  pub enum BuiltinSound { Piano, Bass, Pad, Lead, Drums }
  impl BuiltinSound {
      /// Piano (0, ch 0), Bass (33, ch 0), Pad (89, ch 0), Lead (80, ch 0), Drums (0, ch 9).
      pub fn program_and_channel(self) -> (u8, u8);
      /// The sound a destination's (default_patch program, channel) is, if any.
      /// A default_patch of None on channel 0 is Piano.
      pub fn of(program: Option<u8>, channel: u8) -> Option<BuiltinSound>;
  }
  // DestRow gains #[uniffi(default)] pub builtin_sound: Option<BuiltinSound>,
  // None for every destination that is not a Built-in Synth.
  #[uniffi::export]
  impl Session {
      /// One RoutingCmd::EditDestination: the kind's channel and
      /// default_patch = Some(Patch { program, bank: None }); undo label
      /// "Set Built-in Synth Sound". Another destination: Invalid
      /// "Only a Built-in Synth has these sounds."; unknown: UnknownId.
      pub fn set_builtin_sound(&self, dest: u64, sound: BuiltinSound) -> Result<(), VizError>;
  }
  ```
- Produces (Swift):
  ```swift
  // SynthSheets
  public func sound(of dest: UInt64) -> BuiltinSound?
  public func setSound(_ s: BuiltinSound, dest: UInt64) -> KeyOutcome
  public struct EditDestinationDraft: Equatable, Sendable { public var dest: UInt64; public var latencyText: String }
  public var editDestination: EditDestinationDraft?
  public func beginEditDestination(_ dest: UInt64)        // only an external destination
  /// Parses with SynthCommands.parseMs; 0–500; one undo step via session.setReturnLatency.
  public func commitLatency() -> Bool
  // VizDAW
  @MainActor enum BuiltinSynthWindows { static func show(_ store: DocumentStore, doc: UUID, dest: UInt64) }
  ```
- **The window.**
  - Its title is "Built-in Synth".
  - It holds one radio `Picker("Sound", …)` of the five sounds, in the order above, with the accessibility label "Sound".
  - Under it is the line "Changes every track that plays this Built-in Synth."
  - It follows `PluginWindows`' window style: one window per destination, and a second double-click brings it forward.
  - `BuiltinSynthSlotView` gets `.onTapGesture(count: 2)` before its single tap, as `SlotView` (MixerView.swift L259) does. The destination comes from `MixerModel.instrumentDestination(for:)`.
- **The `destination.set_sound` command.** Params: `dest` (`id_param`, required) and `sound` (Choice `["piano", "bass", "pad", "lead", "drums"]`, required). Class `Change`, `document: true`, `key_only`. It answers status with the sound's title, e.g. "Drums".
- **The destination editor.**
  - "Edit Destination…" in the track menu, after "Track Synth…", is enabled when the track's destination is external.
  - The sheet's title is the destination's name.
  - It has a field "Return latency (ms)", committed on Return or Done.
  - It has a "Measure" button (`measure(dest)`, with the same `message` lines as the Track Synth sheet), and "Done".
  - Under the control channel no sheet opens: the menu item's action shows the status "Use destination.set_return_latency or destination.measure_return." as `trackSynthOnChannel` does.

- [ ] **Step 1: Write the failing `core-ffi` tests** in `builtin_sound.rs`. Use a document with one Built-in Synth destination on channel 0 and one external destination.
  - `set_builtin_sound_sets_program_and_channel`:
    - `Drums` gives the destination's kind channel 9 and `default_patch == Some(Patch { program: 0, bank: None })`;
    - `destinations()` row `builtin_sound == Some(Drums)`;
    - `Pad` gives channel 0, program 89, `Some(Pad)`;
    - the external row's `builtin_sound` is `None`.
  - `set_builtin_sound_undoes_as_one_step`: after `Drums`, the undo label is "Set Built-in Synth Sound", and one `undo` brings back channel 0 and `default_patch == None`.
  - `set_builtin_sound_refuses_other_destinations`:
    - the external destination is `Err` with the sentence above;
    - `u64::MAX` is `Err`;
    - the document is unchanged and no undo step is made.
  - `builtin_sound_of_round_trips`: for each sound, `of(program_and_channel)` gives it back, and `of(Some(5), 0)` is `None`.
- [ ] **Step 2: Run** `cargo test -p core-ffi --test builtin_sound`. Expected: FAIL.
- [ ] **Step 3: Implement** `BuiltinSound`, `set_builtin_sound` and the `DestRow` field. Confirm with `cargo test -p core-engine --test schedule default_patch_plays_first_on_frame_zero` that the patch plays at frame 0. The README checks it live.
- [ ] **Step 4: Write the failing Swift and command tests.**
  - `SynthSheetsTests.soundsRoundTripThroughTheSession`: `setSound(.bass, dest: 7)` calls the fake's `setBuiltinSound(7, .bass)`, and `sound(of: 7)` reads the fake's row.
  - `SynthSheetsTests.theDestinationEditorCommitsLatencyAndMeasures`:
    - `beginEditDestination(9)` on the external destination gives a draft with `latencyText` from its row;
    - "12.5" commits `setReturnLatency(9, 12.5)`;
    - "abc", "-1", "501" and "1\u{0}" each return false, with no call;
    - `measure(9)` runs off the main thread, as `measureRunsOffTheMainThread…` (L73) pins;
    - `beginEditDestination` on the Built-in Synth leaves `editDestination == nil`.
  - `SynthCommandsTests.setSoundRefusesHostileInput`:
    - `{dest: 7, sound: "drums"}` is `.done` "Drums";
    - each of these is `.invalidParams`:
      - `sound: "kazoo"`;
      - `sound: 3`;
      - `dest: "7"`;
      - missing `dest`;
      - a 70 000-character `sound`;
      - a NUL;
      - an unknown param;
    - `dest: 999` is `.unknownId`;
    - the external destination is `.failed`;
    - no call is made for any refusal, and no window opens.
  - `commands.rs` `set_sound_checks_its_params`. Both counts +1, and the id in `CHANGE`.
- [ ] **Step 5: Run** `cargo test -p core-control` and `scripts/build-ffi.sh && scripts/swift.sh test --filter 'SynthSheetsTests|SynthCommandsTests'`. Expected: FAIL.
- [ ] **Step 6: Implement** the model members, the command, the window, the double-tap, the sheet and the menu item.
- [ ] **Step 7: Run** the Step 5 commands. Expected: PASS.
- [ ] **Step 8: End-of-task checks**, including the Swift build and test. **Commit** `feat(synth): Built-in Synth sounds and its window, and the destination editor (plan release-fixes Task 8)`.

### Task 9: Polish: every swatch carries a legend, and the Console legend font

Rows: "A slightly darker orange swatch" and "A different sans for the Console theme's legends".

**Gate.** Step 4 onward runs only with the owner's yes to Decision 8, and Decision 7 rides along only with its own yes. Without the yes, do Steps 1–3, and Task 10 moves the Console row to 27.1.0. The design repo is `/Users/lattawb/workspace/symbiotic-design` (tag 0.2.2). Its changes go on a branch there named `release-fixes`, are committed with the same trailers, and are tagged `0.2.3` only on the owner's yes. Never push.

**Files:**
- Modify:
  - `app/Tests/VizDAWEditorsTests/ThemeScanTests.swift` (a new test);
  - with the yes: `app/Package.swift` L10 and `app/Package.resolved` (0.2.3), `app/Sources/VizDAW/ChannelStrip.swift` `stripLabel` L1013, and `app/Sources/VizDAWAppModel/UserThemes.swift` L372, L483, L565 (parse and write `"technical"`);
  - in the design repo, with the yes: `tokens/tokens.json` (L142 schema, L252 Console `legendFamily: technical`; with Decision 7, L189 and L297 orange), the generator's output, `Sources/SymbioticControls/Palettes.swift` `themedLabelFont` L164–168, `Tests/SymbioticTokensTests/ThemeTests.swift` L57, `tools/tests/test_mockup.py` L30, `tokens/contrast.md`.

**Interfaces:**
- Produces (design repo, with the yes):
  ```swift
  // ThemeStyle.LegendFamily gains .technical: "DIN Alternate" (macOS ships it in
  // /System/Library/Fonts/Supplemental), falling back to Font.system when absent.
  ```
  - VizDAW's strip legends read `theme.style.legendFamily` through `themedLabelFont`.
  - User themes accept `"technical"`.

- [ ] **Step 1: Write the test** `ThemeScanTests.everySwatchCarriesALegend`:
  - for every theme and each of the eight swatches, `Theme.contrast(t.legend(on: s), s) >= 4.5`;
  - the region names (Task 6) and lit legends draw on these fills.
- [ ] **Step 2: Run** `scripts/swift.sh test --filter ThemeScanTests`. Expected: PASS at once, because black ink reaches 4.75:1 on `#C25A00`. Confirm that it fails when the threshold is raised to 5.0, then restore it. That is this step's red check, and the report says so.
- [ ] **Step 3: Commit** `test(theme): every swatch carries a legend; the orange row is superseded (plan release-fixes Task 9)`.
- [ ] **Step 4: (With the yes.) Make the design-repo change.**
  - In the design repo, write the failing tests: `ThemeTests` expects Console's `legendFamily == .technical`, and `test_mockup.py`'s `FAMILIES` gains it.
  - Run them and see them fail.
  - Change the tokens, regenerate, and map `.technical` in `themedLabelFont`.
  - Run the design repo's own tests until they pass.
  - Commit there, and tag `0.2.3`.
- [ ] **Step 5: (With the yes.) Update VizDAW.**
  - Bump the pin.
  - Write the failing `UserThemesTests.technicalLegendsRoundTrip` (`"legendFamily": "technical"` reads and writes back) and `ThemeScanTests.consoleLegendsUseTheTechnicalFamily`.
  - Implement `stripLabel`'s font and the parser.
  - Run `scripts/swift.sh test`, and the no-brand build in Task 10. Expected: PASS.
- [ ] **Step 6: End-of-task checks**, including the Swift build and test. **Commit** `feat(theme): Console legends in the technical family (symbiotic-design 0.2.3) (plan release-fixes Task 9)`.

### Task 10: README checks, the e2e, the backlog and the gates

**Files:**
- Modify:
  - `app/README.md` (new steps after the last one);
  - `crates/vizctl/tests/e2e.rs` (a new test only);
  - `docs/feature-backlog.md`.

**Interfaces:**
- Consumes: Tasks 1–9. No production code changes, unless a step finds a bug. Fix such a bug in the module that owns it, with a regression test.

- [ ] **Step 1: Add the e2e** `release_fixes_over_the_channel`, `#[ignore]`, after the last e2e. It uses a fixture with one audio track holding a region with a 10 ms fade-out, two output paths, a MIDI track, and an instrument track on the Built-in Synth.
  - `region.set_fade_shape {fade: "out", shape: "s_curve"}` answers "Fade Out: S-Curve". The region's fade-out shape in `state.get` events is `s_curve`, and `edit.undo` brings back the old shape.
  - `lane.set_output {lane: <audio>, output: <path 2>}` changes the track's output (`audio_out`). `{output: null}` gives "No Output". On the MIDI track it is `failed`.
  - `destination.set_sound {dest: <synth>, sound: "drums"}` answers "Drums", and `state.get` destinations shows it. Undo brings back the old sound.
  - With the transport stopped:
    - `automation.gesture {phase: "begin"}` on a fader, with no release, then `automation.release_all`, answers "Released 1 touch.";
    - `state.get automation` has an empty `touched`;
    - `edit.undo` works.
  - `ui.view_mix`, then a nudge command, is `unavailable` with `hiddenSelection`'s text. `ui.view_arrange` restores it.
- [ ] **Step 2: Write the README steps** in `app/README.md`. After the last step (`N` = 333 when written), add a heading "26.0.0 release fixes:". Each step states what to do and what is seen, and says "track":
  - **N+1.** Arm a MIDI track with a 1-bar count-in and press Record. On beat 2, choose another output device in Settings › Audio: the count-in carries on from the next beat and recording starts on the bar, with no doubled click.
  - **N+2.** Connect two interfaces with the same name, if you have them: Settings › Audio lists "<name>" and "<name> (2)", and your choice survives a relaunch.
  - **N+3.** With a slow MIDI setup (or at first launch), open a MIDI track's input picker: the window never stalls. A source plugged in appears the next time the picker opens.
  - **N+4.** Arm a track while another document's engine winds down: the window stays responsive, and the input is heard within about half a second.
  - **N+5.** Open a plug-in with a read-only meter parameter, set its strip to Touch, play, and drag that slider: no automation track appears.
  - **N+6.** Write fader automation, play, and watch the mixer: the fader follows what you hear. Grab it: it starts from there. Stop: it shows the static value.
  - **N+7.** In the Mix view with no editor pane, press ⌥→: nothing moves, and the status line says why. Edit › Nudge is greyed out.
  - **N+8.** Rename a region in the inspector: its name shows at the region's top left in the arrangement. Narrow the region: the name truncates, then hides.
  - **N+9.** Click "→ Audio 1" on an audio track's header: pick another output. One Undo puts it back.
  - **N+10.** With a keyboard surface connected, colour a track: its LED takes the colour. "No colour" brings back the kind's colour.
  - **N+11.** Double-click the Built-in Synth slot on an instrument strip: the Built-in Synth window opens. Choose Bass, then Drums, playing a few notes after each: the sound changes, and Drums plays a kit. Undo brings back Bass.
  - **N+12.** On a track with an external destination, choose Edit Destination…: type a return latency, or press Measure with the synth's return input connected. The field shows the measured value, and one Undo puts the old one back.
  - **N+13.** Trim the start of an audio region in the middle of a waveform with "Snap audio edits to zero crossings" on (Settings › Editing): zoom in, and the edge sits on a zero crossing. Play: no click at either edge. With the setting off, the edge stays exactly where you dropped it, still without a click.
  - **N+14.** Give a region a 10 ms fade-in: it sounds exactly as before this build. A crossfaded start sounds as before too.
  - **N+15.** (With Task 9's yes.) Choose the Console theme: the strip legends use the new face. Every other theme is unchanged.
- [ ] **Step 3: Update the backlog** (`docs/feature-backlog.md`):
  - Mark every row this plan closed as done, naming the commit and "plan release-fixes Task n". The orange swatch row is "superseded (7bc5d73; pinned by `everySwatchCarriesALegend`)". The streamer row is "fixed by ec7ea23; verified in Task 5" unless Task 5 found more.
  - **Add the moved parts as 27.1.0 rows (Scope Calls):**
    - a Built-in Synth per track, more sounds and parameters;
    - zero-crossing snapping for Split and per channel, a setting for the edge-fade length, and the edge fade drawn;
    - CoreMIDI's setup-changed notification for the source list;
    - the session lock around `take_idle`;
    - greyed-out read-only AU sliders;
    - editing a destination's name, kind and channel;
    - and, without Task 9's yes, the Console legend font.
  - Add any narrow edge cases the task reports listed, at the owner's review pace.
  - In the 26.0.0 block (lines 10–20), note "release fixes: plan 2026-11-16-release-fixes.md".
- [ ] **Step 4: Run the gates:**
  - `cargo test --workspace`;
  - both clippies;
  - `cargo clippy -p core-ffi --lib -- -D warnings`;
  - `scripts/build-ffi.sh`;
  - `scripts/swift.sh build`;
  - `scripts/swift.sh test`;
  - the no-brand build, `VIZDAW_NO_BRAND=1 scripts/swift.sh build --scratch-path <fresh temp dir>`;
  - `scripts/make-app.sh`;
  - the e2e, `VIZDAW_E2E_APP=dist/VizDAW.app cargo test -p vizctl --test e2e -- --ignored --nocapture --test-threads 1`: every ignored test runs, one more than the baseline, and all pass.

  Then `pgrep -lf MacOS/VizDAW` prints nothing. Report against the baseline: registry +4, README +14 or +15, e2e +1, `VERSION` 9, and the `session.rs` lines changed. That should be `mod fixes;` and at most five one-line hooks.
- [ ] **Step 5: Commit** `docs(app): release-fixes README checks, e2e and backlog (plan release-fixes Task 10)`.

## Self-Review

1. **Spec coverage.** Every 26.0.0 row in scope has a task:
   - **Recording and devices:** Task 1 (count-in rebuild, two clicks) and Task 2 (UID).
   - **Responsiveness:** Task 3 (both rows).
   - **Correctness:**
     - Task 5: the streamed render and the Mix view;
     - Task 4: heard values, non-automatable parameters and `release_all`.
   - **Gaps:**
     - Task 6: region names, the output picker and track colours;
     - Task 8: the Built-in Synth and the destination editor;
     - Task 7: zero crossings, edge fades and the fade-shape command.
   - **Polish:** Task 9.
   - **Left out, per the brief:** sub-projects 6 and 8, the release gate, and the Dropped-at-1.0 line.
2. **Steps.** Each test step names its tests and assertions with the plan's values: frames, IDs, sentences and hex values. Each code step gives signatures, and the Interfaces block fixes the behaviour.
3. **Type consistency.** These names are spelled the same in every task:
   - `Counting::new(.., done)`, `resume_count_in`, `count_in_progress`;
   - `InputDevice`, `pick`, `InputDeviceRow`, `audio_input_device_list`, `inputDeviceUID`;
   - `refreshSources`, `RETIRING`, `reap_retired`, `retiring_count`;
   - `may_write`, `release_channel_touches`, `heard_values`, `MixerModel.heard`;
   - `selectionHidden`, `hiddenSelection`;
   - `nameLabelRect`, `OutputPathRow`, `output_paths`, `outputMenu`, `StripView.colour`, `swatch_color`;
   - `EDGE_FADE_SECONDS`, `edge_frames`, `zero_crossing`, `ZeroCrossing.snapper`, `zeroCrossings`;
   - `BuiltinSound`, `set_builtin_sound`, `DestRow.builtin_sound`, `EditDestinationDraft`, `beginEditDestination`, `commitLatency`, `BuiltinSynthWindows`;
   - the four commands: `automation.release_all`, `lane.set_output`, `region.set_fade_shape` and `destination.set_sound`.
4. **Review Focus.** Each of the five lines names the tests that pin it, in Tasks 1, 2, 5, 7 and 8.
5. **Proportion.** Ten tasks for nineteen rows. Code appears only as signatures and the algorithms the tests do not fix: the click floor, the zero-crossing rule and the edge ramp. The plan reuses `default_patch`, `EditDestination`, `SetFades`, `SetOutput`, `set_return_latency`, `measure_return`, `release_from`, `surface_heard`, `RegionA11y.name` and `listen_to_midi_sources_soon`.
6. **`session.rs`.** It gains `mod fixes;` and one-line hooks: `write_mode_of` (Task 4), `surface_view` (Task 6) and `destinations()` (Task 8), plus a possible `reap_retired` call (Task 3). Everything else is in `session/fixes.rs`, `host.rs` or the other modules named.
