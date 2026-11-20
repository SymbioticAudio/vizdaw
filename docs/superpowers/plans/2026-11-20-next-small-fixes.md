# Next 1: Small Fixes After the Beta Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix five small backlog rows on the `next` branch while the owner tests the 26.0.0 beta: B-001 (a VCA's automation track in DAWproject), the doubled "VCA VCA 1" names, read-only and non-real-time AU parameters, editing a destination's name, port and channel, the zero-crossing snap's file open on every drag move, and three save-prompt cases.

**Architecture:** Each task is self-contained. B-001 is in `crates/core-io/src/dawproject` (export and import). The VCA name fix adds one core helper and one Swift mirror. The AU fix adds a `read_only` flag to the host's `ParamInfo`, which flows through `ParamRow` to the generic plug-in window. The destination editor gets one new FFI call and one new control command that wrap `RoutingCmd::RenameDestination` and `RoutingCmd::EditDestination` in one undo transaction (`History::begin`/`commit`). The zero-crossing fix caches the last `CafReader` in the session. The save prompts change `Session::unsaved`, the quit's discard path and `app.quit`.

**Tech Stack:** Rust 2024 (workspace, uniffi FFI), Swift 6.1 / SwiftUI + AppKit (macOS 14+), Swift Testing.

**Spec:** There is no separate design document. Each task's spec is the Notes of its rows in `docs/feature-backlog.md`: "DAWproject: a VCA's automation track (B-001)", "Greyed-out read-only AU sliders", "Destination editor: name, kind and channel", "Zero-crossing snapping for Split and per channel; edge-fade length; edge fade drawn" (the reader part only) and "Release-gate review leftovers: save prompts" (the first three cases). Also the owner's instruction of 2026-11-20 to work through the backlog on `next` while the beta is tested. Rulings made here without a spec are listed under Rulings as provisional.

## Branch rules (owner, 2026-11-20)

- Work happens on `next`, in the worktree `~/workspace/vizdaw-next`. `release-gate` takes only beta bug fixes; the controller merges `release-gate` into `next` after each one. Nothing moves from `next` into the beta.
- **The project file format stays at v11.** No task here changes what a song file holds. If a task finds it must, stop and ask the controller.
- Keep clear of the areas under the owner's manual tests: no changes to the mixer's layout, the transport bar or Co-Producer's behaviour. Task 1's label change in `VcaStrip` is the one exception (labels only).

## Global Constraints

- Version: `26.0.0` stays the version shown; do not bump it.
- Programmatic and control-channel calls never open dialogs. The save prompts are user-only.
- New commands get hostile-input tests. New FFI goes inside `ffi_guard`.
- Swift: build and test only through `scripts/swift.sh`, `scripts/build-ffi.sh` and `scripts/make-app.sh`. Swift 6.1 rules:
  - no `weak self` or non-Sendable values inside `MainActor.assumeIsolated`;
  - no `@State` or `@FocusState`;
  - AppKit main-actor globals (`NSApp` and friends) only from `@MainActor` code; CI's Xcode 16.4 refuses other uses.
- UI: accessibility labels and theme tokens on every control. The UI says "track", never "lane".
- Tests:
  - never open real audio or MIDI devices, and never trigger mic/TCC prompts;
  - never touch the real `~/Library` (Application Support/VizDAW, prefs, Trash, Audio/Plug-Ins, Keychain) or the real UserDefaults;
  - Swift tests that make a real Session set `TestSupportDir.shared`;
  - the e2e uses vizctl's throwaway home and `VIZDAW_TEST_HOOKS=1`;
  - on Linux CI, AU/CF/importer tests are macOS-gated.
- Never run `auval`. No GUI or computer-use tools. Never use dangerouslyDisableSandbox.
- Names: banned vendor and DAW names appear only in the name scans (`app/Tests/VizDAWAppModelTests/NameScanTests.swift`, `crates/core-control/tests/names.rs`) and in licence notices.
- Commits:
  - by explicit paths;
  - never commit `.DS_Store`, `.superpowers/` or generated FFI files;
  - each commit ends with the two trailer lines `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>` and `Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P`.
- Env: `export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools`.
- Linux clippy: `cargo clippy --workspace --all-targets --target x86_64-unknown-linux-gnu --target-dir target/linux-check -- -D warnings`.
- Pace (owner): fix crashes, data loss, wrong overwrites, security holes and rule violations. Narrow edge cases go to the backlog.

## Review Focus

1. **A lane that holds VCA moves and anything else stays a track.** Notes, audio or another target's automation keep the lane as its own DAWproject track; only a lane whose every event is a VCA fader move is folded away. (Task 2: `a_lane_with_vca_and_strip_moves_stays_a_track`.)
2. **Editing a destination whose MIDI port is offline keeps working.** Keeping the current port is allowed even when it is not in the system's port list; picking a new port that is not there is refused. (Task 4: `an_offline_port_can_be_kept_but_not_chosen`.)
3. **A destination edit is one undo step.** One ⌘Z puts back the name, the port and the channel together. (Task 4: `a_destination_edit_is_one_undo_step`.)
4. **Don't Save while recording never saves the pass.** A clean song that is recording now asks on close; Don't Save closes it and the song on disk is unchanged. (Task 6: `dont_save_while_recording_leaves_the_file_as_it_was`.)
5. **A cached reader never snaps to an old file.** Relinking or replacing a media file drops the cached reader. (Task 5: `the_cached_reader_follows_a_relink`.)

## Rulings (provisional, for the owner's review of this plan)

- **R1 (B-001).** A lane whose every event is a `VcaFader` move is not exported as a track. Its Points go at the arrangement level with no track (they already target the VCA channel's volume, `vca{N}v`). On import, trackless Points that drive a VCA land in one lane per VCA, named with Task 1's label ("VCA 1 Fader"), with no instrument (`midi_dest: None`).
- **R2 (names).** A VCA's label is its name when the name is "VCA" or starts with "VCA " (case-sensitive); otherwise "VCA ‹name›". So "VCA 1" stays "VCA 1 Fader", and "Strings" becomes "VCA Strings Fader".
- **R3 (AU).** `read_only` is a new host fact, separate from `automatable`: a CLAP parameter can be non-automatable yet still set by hand. AU: `read_only = !Flag_IsWritable`; `automatable = Flag_IsWritable && !Flag_NonRealTime`. CLAP: `read_only = IS_READONLY`. Only `read_only` greys the slider.
- **R4 (destination kind).** The editor edits the name of any destination, and the MIDI port and channel of an external MIDI destination. Changing the kind (external MIDI ↔ plug-in instrument) is left out: the plug-in picker already swaps a plug-in destination's plug-in, and a kind switch changes what the destination's strip is. The backlog row keeps "kind" with this note.
- **R5 (zero crossings).** Only the reader cache is built. Snapping a Split (no UI caller), snapping per channel, an edge-fade length setting, drawing the edge fade and Arrange-view trims (the Arrange view has no trim gesture to snap) stay in the backlog row, which is reworded to say so.
- **R6 (quit while a save waits).** On a quit with two or more songs, Discard Changes discards the songs with unsaved edits but still saves a song whose Save is waiting for its recording pass: the owner pressed Save for it. The quit alert now says so ("… will still be saved when its recording ends").
- **R7 (`app.quit`).** Over the developer channel, `app.quit` with any unsaved song replies the error `unsaved_changes` (listing the songs' names) and keeps running, unless its params are `{"discard": true}`. `VIZDAW_TEST_HOOKS=1` does not bypass this; the e2e helper `App::quit` sends `discard: true`. `vizctl quit` gains `--discard`.

---

### Task 1: One label for a VCA

**Files:**
- Modify: `crates/core-model/src/groups.rs` (new helper)
- Modify: `crates/core-ffi/src/automation.rs:1046` (`lane_name`)
- Modify: `crates/core-model/src/diff.rs:717`
- Modify: `app/Sources/VizDAWAppModel/MixerGroups.swift:7-19` (`VcaStripLabels`)
- Test: `crates/core-model/tests/groups.rs`, `crates/core-ffi/src/automation.rs` tests (around :1687), `app/Tests/VizDAWAppModelTests/MixerGroupsTests.swift`

**Interfaces:**
- Produces: `pub fn vca_label(name: &str) -> String` in `core_model::groups` (re-exported where `Vca` is). Swift: `static func title(_ name: String) -> String` on `VcaStripLabels`. Task 2 uses `vca_label` for imported lane names.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn a_vca_label_never_says_vca_twice() {
    assert_eq!(vca_label("VCA 1"), "VCA 1");
    assert_eq!(vca_label("VCA"), "VCA");
    assert_eq!(vca_label("Strings"), "VCA Strings");
    assert_eq!(vca_label("VCAs and more"), "VCA VCAs and more");
}
```

In `crates/core-ffi/src/automation.rs` tests: `lane_name` for `VcaFader` of a VCA named "VCA 1" is `"VCA 1 Fader"`, and of one named "Strings" is `"VCA Strings Fader"`. In `diff.rs`'s tests, the described VCA is "VCA 1", not "VCA VCA 1".

Swift (`MixerGroupsTests`): `VcaStripLabels(name: "VCA 1").fader == "Fader of VCA 1"`, and for "Strings" `"Fader of VCA Strings"`; same for `level`, `mute`, `solo`. Update `vcaStripControlsAreLabelled` to match.

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p core-model --test groups a_vca_label && cargo test -p core-ffi --lib automation && scripts/swift.sh test --filter MixerGroupsTests`
Expected: FAIL (`vca_label` not defined; labels say "VCA VCA 1").

- [ ] **Step 3: Implement `vca_label` and use it in the three Rust places and `VcaStripLabels`**

`lane_name` becomes `format!("{} Fader", vca_label(&x.name))`. The Swift labels use `title(name)` in place of `"VCA \(name)"`.

- [ ] **Step 4: Run the tests to verify they pass**

Same commands. Expected: PASS. Also run `cargo test -p core-model -p core-ffi` for other tests that pinned the old text.

- [ ] **Step 5: Commit**

```bash
git add crates/core-model/src/groups.rs crates/core-model/tests/groups.rs crates/core-ffi/src/automation.rs crates/core-model/src/diff.rs app/Sources/VizDAWAppModel/MixerGroups.swift app/Tests/VizDAWAppModelTests/MixerGroupsTests.swift
git commit -m "fix(vca): never say VCA twice in a VCA's labels"
```

### Task 2: B-001, a VCA's automation in DAWproject

**Files:**
- Modify: `crates/core-io/src/dawproject/export.rs:769-806` (`layout`, the half detection)
- Modify: `crates/core-io/src/dawproject/export_timeline.rs:140` (`arrange`) and `:1225` (`automation`)
- Modify: `crates/core-io/src/dawproject/import/timeline/automation.rs:57` (`automation`, the no-owner branch)
- Test: `crates/core-io/tests/dawproject_round_trip.rs`, `crates/core-io/tests/dawproject_timeline_out.rs`

**Interfaces:**
- Consumes: `core_model::groups::vca_label` (Task 1).
- Produces: nothing other tasks use.

- [ ] **Step 1: Write the failing tests**

In `dawproject_round_trip.rs`, beside `vca_channels_round_trip` (:1097), using the same `Builder`, `song`, `GroupCmd::NewVca`, `b.events` and `trip` helpers:

```rust
#[test]
fn a_vca_only_lane_is_not_a_track() {
    // a lane whose only events are VcaFader moves (3 points)
    // export: project_xml has no Track named "VCA 1 Fader", and has 3 Points with parameter="vca{N}v"
    // re-import: no instrument track; one lane named "VCA 1 Fader" with midi_dest == None
    //   holding 3 VcaFader(placed vca) events whose values are near() the originals
}

#[test]
fn a_lane_with_vca_and_strip_moves_stays_a_track() {
    // the "Keys" note lane holds VcaFader and StripFader moves (as vca_channels_round_trip)
    // export still writes the Keys track; re-import keeps both targets' points on Keys
}
```

In `dawproject_timeline_out.rs`, next to `vca_fader_automation_is_the_vca_channels_volume` (:957): a VCA-only lane's Points sit in the arrangement's Lanes with no `track` attribute.

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p core-io --test dawproject_round_trip a_vca && cargo test -p core-io --test dawproject_timeline_out vca`
Expected: FAIL (an extra "VCA 1 Fader" track with a built-in-synth Note feed).

- [ ] **Step 3: Export: fold away a VCA-only lane**

In `layout`, a lane with at least one event and every event an `EventBody::Automation { target: AutomationTarget::VcaFader(_), .. }` makes no Part. Collect those events per sequence and write them as trackless `DpTimeline::Points` from `arrange`, through the existing `VcaFader` branch of `automation` (no `pi`; pull that branch into a helper that takes no part). The report's automation-point count is unchanged.

- [ ] **Step 4: Import: one lane per VCA for trackless VCA Points**

In `automation`, when there is no owner and the target is `AutoAt::Vca(v)`, create (or reuse, per `v`) a lane named `format!("{} Fader", vca_label(name))` through `new_track`, with no instrument, instead of the shared "Automation" track. Other trackless Points keep going to "Automation".

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p core-io`
Expected: PASS, including the existing VCA and merge tests.

- [ ] **Step 6: Commit**

```bash
git add crates/core-io/src/dawproject/export.rs crates/core-io/src/dawproject/export_timeline.rs crates/core-io/src/dawproject/import/timeline/automation.rs crates/core-io/tests/dawproject_round_trip.rs crates/core-io/tests/dawproject_timeline_out.rs
git commit -m "fix(dawproject): a VCA's moves are the VCA channel's automation, not a track (B-001)"
```

### Task 3: Read-only and non-real-time AU parameters

**Files:**
- Modify: `crates/core-host/src/info.rs:42-63` (`ParamInfo`)
- Modify: `crates/platform-macos/src/au/mod.rs:612-629` (`read_info`)
- Modify: `crates/core-host/src/clap/instance.rs:159-172`
- Modify: `crates/core-ffi/src/snapshot.rs:456-477` (`ParamRow`), `crates/core-ffi/src/session.rs:2992-3006` (`params`)
- Modify: `app/Sources/VizDAW/PluginWindows.swift:316-348` (`ParameterList`)
- Modify: every other `ParamInfo { .. }` literal (`crates/test-plugins/src/plugin.rs`, `crates/core-host/tests/{host,engine_plugins,types}.rs`, `crates/core-ffi/src/automation.rs`)
- Test: `crates/platform-macos/src/au/mod.rs` (unit test), `crates/core-host/tests/types.rs`, `app/Tests/VizDAWAppModelTests/PluginWindowModelTests.swift`

**Interfaces:**
- Produces: `ParamInfo.read_only: bool` with `#[serde(default, skip_serializing_if = "std::ops::Not::not")]`, so files written before it still load under `deny_unknown_fields`. `ParamRow.read_only: bool` (Swift `readOnly`). `fn au_param_facts(flags: AudioUnitParameterOptions) -> (automatable: bool, read_only: bool, hidden: bool)` in `au/mod.rs` (a plain function so it is tested without an AU).

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn au_flags_give_automatable_read_only_and_hidden() {
    use AudioUnitParameterOptions as F;
    let rw = F::Flag_IsWritable | F::Flag_IsReadable;
    assert_eq!(au_param_facts(rw), (true, false, false));
    assert_eq!(au_param_facts(F::Flag_IsReadable), (false, true, false));
    assert_eq!(au_param_facts(rw | F::Flag_NonRealTime), (false, false, false));
    assert_eq!(au_param_facts(F::Flag_IsWritable), (true, false, true));
}
```

In `core-host/tests/types.rs`: a `ParamInfo` JSON without `read_only` loads with `read_only == false`, and one with `read_only: false` is written without the key.

Swift: give the `param(...)` helper in `PluginWindowModelTests` `automatable:` and `readOnly:` parameters (default `true`/`false`). New test `aReadOnlyParameterIsShownButNotEditable`: `model.visibleParams` still lists it, and `model.isEditable(p) == false`; a non-automatable but writable one is editable.

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p platform-macos --lib au_flags && cargo test -p core-host --test types && scripts/swift.sh test --filter PluginWindowModelTests`
Expected: FAIL (`au_param_facts`, `read_only`, `isEditable` not defined).

- [ ] **Step 3: Implement**

`read_info` uses `au_param_facts`. CLAP sets `read_only` from `IS_READONLY`. `params` copies it into `ParamRow`. `PluginWindowModel.isEditable(_ p: ParamRow) -> Bool` is `!p.readOnly`; `ParameterList`'s `Slider` gets `.disabled(!model.isEditable(p))` and the accessibility value suffix ", read-only" when not editable. Run `scripts/build-ffi.sh` after the FFI change; update `FakeSession` for the new field.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p core-host -p core-ffi -p platform-macos && scripts/swift.sh test`
Expected: PASS, including `read_only_parameters_and_host_echoes_are_not_gestures` and `a_non_automatable_parameter_never_writes_a_lane`.

- [ ] **Step 5: Commit**

```bash
git add crates/core-host crates/platform-macos/src/au/mod.rs crates/core-ffi/src/snapshot.rs crates/core-ffi/src/session.rs crates/core-ffi/src/automation.rs crates/test-plugins/src/plugin.rs app/Sources/VizDAW/PluginWindows.swift app/Sources/VizDAWAppModel/PluginWindowModel.swift app/Tests/VizDAWAppModelTests/PluginWindowModelTests.swift app/Tests/VizDAWAppModelTests/FakeSession.swift
git commit -m "feat(plugins): grey out read-only parameters; non-real-time AU parameters are not automatable"
```

### Task 4: Edit a destination's name, port and channel

**Files:**
- Modify: `crates/core-ffi/src/session.rs` (new `edit_midi_destination`, beside `add_midi_destination` at :3515), `crates/core-ffi/src/mixer.rs:261-288` (`DestRow`)
- Modify: `crates/core-control/src/registry.rs` (new command and params beside :3393-3460 and :2171-2378)
- Modify: `app/Sources/VizDAWAppModel/SynthCommands.swift`, `app/Sources/VizDAWAppModel/SynthSheets.swift:46-57,269-302`, `app/Sources/VizDAW/SynthSheetViews.swift:129-178`
- Test: `crates/core-ffi/tests/synth.rs`, `crates/core-control/tests/commands.rs`, `app/Tests/VizDAWAppModelTests/SynthSheetsTests.swift`, `app/Tests/VizDAWAppModelTests/FakeSession.swift`

**Interfaces:**
- Produces:
  - `pub fn edit_midi_destination(&self, dest: u64, name: String, port: String, channel: u8) -> Result<DocSnapshot, VizError>`. Same checks as `add_midi_destination` (name and port 1–255 characters, no control characters, name trimmed, channel ≤ 15, not the built-in or virtual ports), except the port may also be the destination's current one when that port is offline. Refuses a destination that is not external MIDI. Runs `RenameDestination` (when the name changed) and `EditDestination` (when port or channel changed, keeping `group` and `default_patch`) inside one `History::begin("Edit Destination")` … `commit()`; on an error it `abort()`s. No change at all makes no undo step.
  - `DestRow.port: Option<String>` (the endpoint ref of an external MIDI destination).
  - Control command `destination.edit_midi`, `Class::Change`, params `dest` (id), `name` (Text), `port` (Text), `channel` (Int 1–16, 1-based like `destination.add_midi`).
  - Swift: `EditDestinationDraft` gains `var name`, `var port`, `var channel` (1-based); `commitEdit() -> Bool` replaces `commitLatency()` and makes the edit then the latency change.

- [ ] **Step 1: Write the failing tests**

`crates/core-ffi/tests/synth.rs`:

```rust
#[test]
fn a_destination_edit_is_one_undo_step() {
    // add_midi_destination("Synth A", port0, 0); edit to ("Synth B", port1, 9)
    // destinations() shows name "Synth B", port Some(port1), channel Some(9)
    // undo once: back to "Synth A", port0, channel 0; undo_label was "Edit Destination"
}
#[test]
fn an_offline_port_can_be_kept_but_not_chosen() {
    // a destination on a port not in the port list: renaming it while keeping the port succeeds;
    // editing it to another port not in the list fails with InvalidArgument
}
#[test]
fn editing_a_plugin_destination_is_refused() {}
#[test]
fn an_edit_that_changes_nothing_adds_no_undo_step() {}
```

Tests that use MIDI ports must use the session's test port list the existing `add_midi_destination` tests use (see `synth.rs:1276`), never real CoreMIDI.

`crates/core-control/tests/commands.rs`: add `destination.edit_midi` to the bad-param tables (:1284-1310): missing `dest`, unknown id, empty or 256-character name, control characters, channel 0 and 17, non-integer channel.

Swift (`SynthSheetsTests`): `beginEditDestination` fills name, port and 1-based channel from the `DestRow`; `commitEdit` calls the fake's `editMidiDestination` with the 0-based channel, then `setReturnLatency`; an empty name keeps the sheet open with an error.

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p core-ffi --test synth destination && cargo test -p core-control --test commands && scripts/swift.sh test --filter SynthSheetsTests`
Expected: FAIL (functions not defined).

- [ ] **Step 3: Implement the FFI call, `DestRow.port` and the control command**

Dispatch `destination.edit_midi` in `SynthCommands.swift` like `destination.set_return_latency` (:150). Run `scripts/build-ffi.sh`; update `FakeSession`.

- [ ] **Step 4: Add the fields to `DestinationEditorSheet`**

A name `TextField`, a port `Picker` over the session's MIDI destination ports (plus the current port, marked "(offline)" when absent), and a channel `Picker` 1–16, above the latency row. Each with an accessibility label ("Destination name", "MIDI port", "MIDI channel") and theme tokens. Done calls `commitEdit()`.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p core-ffi -p core-control -p core-model && scripts/swift.sh test`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/core-ffi/src/session.rs crates/core-ffi/src/mixer.rs crates/core-ffi/tests/synth.rs crates/core-control/src/registry.rs crates/core-control/tests/commands.rs app/Sources/VizDAWAppModel/SynthCommands.swift app/Sources/VizDAWAppModel/SynthSheets.swift app/Sources/VizDAW/SynthSheetViews.swift app/Tests/VizDAWAppModelTests/SynthSheetsTests.swift app/Tests/VizDAWAppModelTests/FakeSession.swift
git commit -m "feat(destinations): edit an external destination's name, port and channel"
```

### Task 5: The zero-crossing snap reuses its reader

**Files:**
- Modify: `crates/core-ffi/src/session/fixes.rs:120-190` (`zero_crossing`)
- Modify: the session's `Inner` (new field)
- Test: `crates/core-ffi/tests/zero_crossing.rs`

**Interfaces:**
- Produces: `Inner.crossing_reader: Option<(u64 /* media */, PathBuf, CafReader)>`, private. `zero_crossing`'s signature and results are unchanged.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn repeated_snaps_open_the_file_once() {
    // call zero_crossing 50 times on one media; the test hook counting CafReader opens
    // (add one, test-only, e.g. a #[cfg(test)]/test-hooks counter) reads 1
}
#[test]
fn the_cached_reader_follows_a_relink() {
    // snap on media M (file A, crossing at frame 100); relink M to file B (crossing at 300)
    // the next snap answers from B
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p core-ffi --test zero_crossing`
Expected: FAIL (50 opens).

- [ ] **Step 3: Implement**

Reuse the cached reader when both the media id and its resolved path match; otherwise open and replace it. An open failure clears the cache and keeps the frame, as today. Undo, relink, media removal and document close need nothing extra: the path check catches a change.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p core-ffi --test zero_crossing && scripts/swift.sh test --filter AudioEditor`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/core-ffi/src/session/fixes.rs crates/core-ffi/src/session.rs crates/core-ffi/tests/zero_crossing.rs
git commit -m "perf(audio-editor): a trim drag's zero-crossing snap opens the file once"
```

### Task 6: Save prompts while recording, and `app.quit`

**Files:**
- Modify: `crates/core-ffi/src/session.rs:861` (`unsaved`), and a new `cancel_save_after_pass`
- Modify: `app/Sources/VizDAW/VizDAWApp.swift:353-460` (`QuitPrompts`), `app/Sources/VizDAWAppModel/ClosePrompts.swift`
- Modify: `app/Sources/VizDAW/ControlChannel.swift:502-507` (`case .quit`), `crates/core-control/src/registry.rs` (`app.quit` params), `crates/vizctl/src/cli.rs:387-393`, `crates/vizctl/tests/e2e.rs:117` (`App::quit`)
- Test: `crates/core-ffi/tests/record.rs`, `crates/core-ffi/tests/persistence.rs`, `app/Tests/VizDAWAppModelTests/ClosePromptsTests.swift`, `crates/core-control/tests/commands.rs`

**Interfaces:**
- Produces:
  - `Session::unsaved` is also true while `recording.is_some() || finishing || pending_pass.is_some()`.
  - `pub fn cancel_save_after_pass(&self) -> Result<(), VizError>` clears `save_after_pass` (Don't Save on a song that is recording; see Step 3).
  - `ClosePrompts.quitAlertText(discardable: [String], savingAfterPass: [String]) -> (message: String, info: String)`.
  - `app.quit` params: optional `discard: Bool`. Error code `unsaved_changes` with the songs' names.

- [ ] **Step 1: Write the failing tests**

`crates/core-ffi/tests/record.rs`, using the existing recording helpers there (:2288 `a_put_off_save_at_stop_is_made_by_close` is the pattern):

```rust
#[test]
fn a_clean_song_that_is_recording_has_unsaved_changes() {
    // save, start recording: has_unsaved_changes() == true; stop and save: false
}
#[test]
fn dont_save_while_recording_leaves_the_file_as_it_was() {
    // save, start recording, cancel_save_after_pass(), close(): the file on disk is byte-identical to the saved one
}
```

`ClosePromptsTests.swift`: `quitAlertText(discardable: ["A"], savingAfterPass: ["B"])`'s info names B and says it "will still be saved when its recording ends"; with no song waiting, the text is today's.

`core-control/tests/commands.rs`: `app.quit` with `discard: "yes"` (not a Bool) is a bad param.

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p core-ffi --test record recording && scripts/swift.sh test --filter ClosePromptsTests && cargo test -p core-control --test commands`
Expected: FAIL.

- [ ] **Step 3: Implement the core and the prompts**

`unsaved` counts the pass. The close sheet's Don't Save (shown only when no save is pending, as today) calls `cancel_save_after_pass` before closing, so a close never saves a pass nobody asked to save. In `QuitPrompts`, Discard Changes calls `cancel_save_after_pass` on the songs with unsaved edits and no pending save, and leaves the songs waiting to save alone (R6); the alert uses `quitAlertText`.

- [ ] **Step 4: Implement `app.quit`'s refusal**

Before `QuitPrompts.quiet = true`, if any open song has unsaved changes and `discard` is not `true`, reply `unsaved_changes` (names in the message) and do not terminate. `App::quit` in the e2e sends `{"discard": true}`; `vizctl quit --discard` sends it.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --workspace && scripts/swift.sh test && cargo test -p vizctl --test e2e`
Expected: PASS. The e2e needs the window server; if it cannot run here, say so in the report and the controller runs it.

- [ ] **Step 6: Commit**

```bash
git add crates/core-ffi/src/session.rs crates/core-ffi/tests/record.rs crates/core-ffi/tests/persistence.rs app/Sources/VizDAW/VizDAWApp.swift app/Sources/VizDAWAppModel/ClosePrompts.swift app/Tests/VizDAWAppModelTests/ClosePromptsTests.swift app/Sources/VizDAW/ControlChannel.swift crates/core-control/src/registry.rs crates/core-control/tests/commands.rs crates/vizctl/src/cli.rs crates/vizctl/tests/e2e.rs
git commit -m "fix(prompts): a recording counts as unsaved; app.quit asks the caller before discarding"
```

### Task 7: Backlog, README checks and the manual-check list

**Files:**
- Modify: `docs/feature-backlog.md` (the five rows), `app/README.md` (new steps after 456), `docs/testing/manual-checks.md`, `docs/testing/bugs.md` (B-001 status)

- [ ] **Step 1: Update the backlog rows**

Mark what each task did as **Done (next, ‹commit›)**. Keep what R4 and R5 left out in the rows: destination kind, Split snapping, snapping per channel, the edge-fade setting and drawing, Arrange-view trims. In the save-prompts row keep the two untouched notes (`closeQuietly` unit test, `QuitPrompts.quiet`). B-001 in `bugs.md`: fixed on `next`, verify in the next build.

- [ ] **Step 2: Add README steps 457 and on**

Each step is concrete (action → expected result):
- export a song with a VCA ride on its own track to DAWproject and re-import: no extra instrument track, the VCA plays its moves;
- a VCA named "VCA 1" reads "VCA 1 Fader" in the track list and VoiceOver says "Fader of VCA 1";
- a read-only AU parameter's slider is greyed out;
- edit an external destination's name, port and channel, then ⌘Z once;
- drag an audio-editor trim quickly: it snaps and does not stutter;
- record into a saved song and press ⌘W: it asks; Don't Save leaves the file as it was;
- two songs, one waiting to save after recording, ⌘Q, Discard Changes: the alert says that song will still be saved, and it is.

- [ ] **Step 3: Add them to `docs/testing/manual-checks.md`**

A new section "`next` branch (after the beta)", all **open**, noting they apply to a build from `next`, not the beta test build.

- [ ] **Step 4: Commit**

```bash
git add docs/feature-backlog.md app/README.md docs/testing/manual-checks.md docs/testing/bugs.md
git commit -m "docs: next-1 backlog status, README checks 457 on, manual-check list"
```
