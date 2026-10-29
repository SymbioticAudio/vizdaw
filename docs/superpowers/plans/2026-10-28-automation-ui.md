# VizDAW 5e-2 — Automation UI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Automation is recorded from the app's own windows: every mixer fader, pan, send level, mute and solo move is a gesture (heard at once, written while its strip writes); each strip has an automation-mode popup with its own colour and touched controls get a ring; "Set All Strips to…" and the Automation menu (`automation.mode.<mode>` for the selected strips, Suspend Automation) set modes; Settings › Automation keeps After Write, the return time and thinning; the plugin window's own sliders are gestures too; the Strip Chart draws the write in progress live and opens an automation lane on its target; and the 5d-2 record UI's remaining polish and test items land.

**Architecture:** Swift on top of plan 5e-1's cores; the only Rust change is six registry entries.
- **`VizDAWAppModel`** (tested with `FakeSession`): `AutomationPreferences` (the app's preferences, handed to the core at launch and on change, as `AudioPreferences` is); `MixerModel` gains the gesture API (`touch`, `move`, `release`, `click`), live values while touched, the touched set from `automation_state` (rings), modes per strip and for all, and a strip selection kept on `DocumentModel.selectedStrips`; `AutomationModes` (titles and colours); `AutomationCommands` becomes `MenuCommands` for `automation.mode.<mode>` and `automation.suspend`; `PluginWindowModel`'s sliders call the gesture path; `StripChartModel.preview` and the automation lane's own row; the 5d-2 carries (`DocumentModel.saveAction`, `RecordModel.punchFocus`, `AudioPreferences.inputHolder`, `LaneInputs.refreshMonitoring`, `LaneHeaderLayout.showsMeter`, `GrowingTake.slice`).
- **`VizDAWEditors`**: `StripChartView` draws the preview; `LaneHeaderView` gets the monitor button's tooltip; `ArrangementView` uses `GrowingTake.slice`.
- **`VizDAW`**: the mixer's popups, rings, selection and "Set All Strips to…"; the Automation menu; Settings › Automation; the record popover's focus-driven text editing; Settings › Audio's live readout.
- **`core-control`**: `automation.mode.off` … `automation.mode.trim` and `automation.suspend` as menu items.

**Tech Stack:** Swift 6 / SwiftPM (SwiftUI, AppKit, Observation, Swift Testing) on macOS 14+; Rust 2024 for the registry (UniFFI 0.32.2 unchanged).

**Spec:** `docs/superpowers/specs/2026-10-18-recording-audio-design.md`, §13 row "5e-2 — Automation UI" (split from the former 5e row by plan 5e-1 Ruling 1). This plan delivers:
- §7.1 "On-screen mixer (`MixerView`): mouse down, drag and mouse up. Click-to-toggle controls (mute) send touch, value and release at once"; the plugin window's parameter controls (its generic sliders; plugin editors' own gestures are 5e-1's);
- §7.4 "The Strip Chart draws the in-progress write live from `Session.automation_preview(target)`" and the new lane's "Strip Chart view set to T";
- §7.5 the mixer's mode popup with a colour per mode and touched rings, the strip menu's "Set all strips to…", Settings › Automation, `automation.mode.<mode>` for the selected strips and `automation.suspend` as commands;
- the 5d-2 carries that belong to the record UI (see "Carried items").

Read the spec (§3 and §7), plan 5e-1 (`docs/superpowers/plans/2026-10-28-automation-engine.md`: its Interfaces blocks are what this plan calls) and plan 5d-2 (`docs/superpowers/plans/2026-10-26-record-ui.md`).

**Revision.** This text follows plan 5e-1 as revised for its pre-flight at `1f6057a`: a touch with no value is the value heard now and the mixer sends none (5e-1 Ruling 26); overlapping static touches share one edit, so two controls touched at once are fine against the real core (Ruling 14); the gesture path has a source, and the core itself sends a UI value of a plugin parameter to the plugin on every path (Ruling 15); `automation_state().touched` lists active touches only (M8); all three automation settings are read when a pass starts (Ruling 19); the app's `body` has room for one more top-level menu (pre-flight M22).

## Global Constraints

- **Shell setup.**
  - Every shell command runs after `export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools`. Every bash block below starts with that line and `cd /Users/lattawb/workspace/vizdaw`.
  - A `git commit` without `DEVELOPER_DIR` fails silently: check every commit with `git log -1 --oneline` and `git show --stat HEAD`.
- **Swift only through the scripts, from the repo root.** `scripts/build-ffi.sh` (after any Rust change that touches the FFI, before Swift), `scripts/swift.sh build`, `scripts/swift.sh test`, `scripts/make-app.sh`. Swift tests run only via `scripts/swift.sh test` from the repo root. Never call `swift`, `xcodebuild` or `xcrun` directly.
- **Branch and commits.**
  - Work on branch `sp5-audio`, after plan 5e-1's last commit (plan 5e-1 starts from `1f6057a`). Never switch branches, never push.
  - Commit at the end of every task, adding only the files the task lists, by path (never `git add -A`). No manifest changes, so `Cargo.lock` never changes. Never commit `.superpowers/`, `.DS_Store` or generated files.
  - End every commit message with the two attribution lines your dispatch gives you. The blocks below show:
    ```
    Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
    Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P
    ```
    Use the `Co-Authored-By` line naming the model you actually are.
- **Clean room.** Never read or copy anything from `~/workspace/SVP/research`, the Studio Vision binary or any research material. No Opcode, Midas, Steinberg, Apple Logic or Soundcraft assets, names, artwork or branding anywhere. SF Symbols used: `slider.vertical.3`, `checkmark`.
- **No GUI tools, no real devices.**
  - Never use GUI, computer-use or accessibility-automation tools. The views are checked by the owner by eye from the README steps Task 9 writes; your checks are the model tests, the build and the e2e. Launch VizDAW only through the e2e (Task 9, `VIZDAW_AUDIO=offline`); at the end of a task that launched it, `pgrep -lf MacOS/VizDAW` prints nothing and no new `/tmp/vze*` home is left.
  - Never touch the real `~/Library/Application Support/VizDAW`, the real preferences, the real Trash or any audio device's settings. Swift tests use `MemoryDefaults()` for every `AutomationPreferences` and `AudioPreferences` and a fake sink, never the real setters; the app's `automationPreferences` (Task 1) is the only object that calls them, and under `VIZDAW_SUPPORT_DIR` (vizctl, the e2e) it keeps its settings in memory.
  - Never save a project in a `vizctl`-launched app.
- **Tests never open real audio input and never trigger the microphone prompt (TCC).** Swift tests never call the FFI's device functions; the statics that read them (`LaneInputs.softwareMonitoring`, `RecordModel.freeSpace`, …) are replaced in the tests that read them and put back. The one real core setter a test calls is `setAutomationSuspended(on:)` (plan 5e-1; process-wide state with no device or file behind it), reset with `defer`.
- **Crates and dependencies.** No new crate and no new dependency. License GPL-3.0-only. Rust lints as in plan 5e-1.
- **Linux CI.** CI also runs `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` on `ubuntu-latest`. The one Rust change here (Task 4's registry) is platform-neutral; Tasks 4 and 9 check it with `cargo clippy --workspace --all-targets --target x86_64-unknown-linux-gnu --target-dir target/linux-check -- -D warnings`.
- **TDD.** Every new test is run and seen to fail before the code that makes it pass; Task 8's tests of existing behaviour (the 5d-2 test gaps) are the exception the task names, and say so in the report when they pass at once.
- **Real-time rules (spec §3).** Nothing in this plan runs on the real-time thread; Task 9 runs the rt-check gates to show nothing changed there.
- **Media is write-once.** Nothing here writes media.
- **File compatibility.** `core_model::file::VERSION` stays 3; nothing here is saved in a project. The automation settings are app preferences (key `VizDAW.automation`, JSON); the strip selection is per window session, never saved.
- **FFI safety.** No FFI change (the registry is `core-control`'s data).
- **Swift.**
  - Generated bindings compile in Swift 5 mode; `VizDAWAppModel`, `VizDAWEditors`, `VizDAW` and the tests in Swift 6 mode with no new warnings (the one `MemoryDefaults` Sendable warning is older than this plan).
  - No SwiftUI `@State` or `@FocusState` in new views (the Command Line Tools cannot expand them reliably): view state lives in the models. The record popover's existing `@FocusState` stays; its new use goes through `RecordModel.punchFocusChanged`.
  - Models are `@Observable @MainActor` classes; pure helpers a non-isolated context calls are `nonisolated static`.
  - New model tests go to `VizDAWAppModelTests`; a suite that replaces a static is `.serialized` and puts it back with `defer`.
- **Control channel** (control spec §6, §7). Programmatic calls never open dialogs: no new command has one. Every registered command has a handler (`everyRegisteredMenuCommandHasAHandler`); the registry count moves from 132 to 138 in Task 4. Every new command gets hostile-input tests: `automation_menu_commands_take_no_params` (`core-control`) and `modeCommandsSetTheSelectedStrips` (app).
- **Generated files** are never committed: `app/Sources/VizDAWCore/vizdaw_ffi.swift`, `app/Sources/vizdaw_ffiFFI/include/`, `target/ffi/`, `dist/`.
- **Each task ends green.**
  - Always `scripts/swift.sh build` (no new warning) and `scripts/swift.sh test`.
  - Task 4 also `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test -p core-control`, `scripts/build-ffi.sh`.
  - Task 9 runs every gate of plan 5e-1 (the rt-check gates and the workspace included) and the whole e2e.
  - Known flaky tests: the `damaged_files` proptest, the MIDI listener tests (the reconnect test), `core-ffi` catalog tests under parallel load and the latency e2e timing. Re-run once before investigating, and say so in the report.
  - Baseline at plan 5e-1's end (its Task 11): Rust about 1521 passed, 0 failed, 14 ignored; Swift 848 tests; e2e 10 tests. Report the measured counts.
- **How code is given.** As in plan 5e-1: tests whole; new files as their public items with doc lines and the rules the tests leave open; changes to existing files as Find / Replace excerpts (replayed against `1f6057a`, where each matches exactly once; plan 5e-1's excerpts do not touch them) or "after the Find, add …". If an excerpt does not match, make the same change to the current text and say so in the report.
- **Spec values used verbatim:**
  - "each strip gets an automation-mode popup (Off / Read / Touch / Latch / Write / Trim), each mode with its own colour, and touched controls get a ring";
  - "**Strip menu:** a "Set all strips to…" command";
  - "**Settings › Automation:** After Write mode, return time, thinning, and "Write plugin parameters only when touched" (always on, shown for clarity)";
  - "**Commands:** `automation.mode.<mode>` for the selected strips, plus `automation.suspend` (a global toggle that makes every strip behave as Read without changing its mode)";
  - "return time (Setting, 0–2000 ms, default 250 ms)"; "After Write … (default Latch)"; "0.25 % of range for others (Setting: Low / Medium / High)";
  - "Click-to-toggle controls (mute) send touch, value and release at once".

## Review Focus

The five failure modes a user is most likely to hit that the spec implies but does not spell out; each is pinned by named tests in its task.

1. **A drag the window never finishes** (the mixer closes mid-drag, the dragged strip is deleted by an Undo, the core refuses the touch because a non-gesture edit is open, two faders dragged at once with a trackpad and a surface). Expected: every touch is released exactly once; a refused touch sends no release; concurrent touches on different targets are fine (plan 5e-1 Ruling 14: they share one static edit in the core, so this holds against the real core too, not only `FakeSession`). Pinned by `touchesAreReleasedOnceWhenTheWindowClosesOrTheStripGoes`, `aRefusedTouchSendsNoRelease` and `twoTargetsTouchedAtOnceAreIndependent` (Task 2), and in the core by plan 5e-1's `two_static_gestures_at_once_are_one_step`.
2. **What the fader shows and sends while writing** (grabbing a fader whose automation is moving it, Write's held value, a Read strip's snap-back on release). Expected: mouse-down sends a touch with no value, so the core starts from the value heard now and nothing jumps (plan 5e-1 Ruling 26); while touched the control follows the hand from its first move; after release it shows the core's value again at the next refetch. Pinned by `aFaderDragIsAGestureOnTheCore` and `aTouchedControlShowsItsLiveValue` (Task 2), and in the core by plan 5e-1's `a_touch_without_a_value_starts_at_the_heard_value`.
3. **Settings changed at the wrong moment or damaged** (a return time typed as 3000, After Write set to Write, a damaged preferences file, a change while a pass is writing). Expected: out-of-range values are clamped or refused and say why; damaged fields fall back one by one; a change applies from the next Play, thinning included (plan 5e-1 Ruling 19: all three are read when a pass starts). Pinned by `damagedAutomationSettingsKeepTheirDefaults` and `settingsAreKeptOnlyOnceTheCoreTookThem` (Task 1).
4. **Mode commands with nothing or something odd selected** (no strip selected, master selected, the selection's strip deleted, the mixer window closed). Expected: `unavailable` with "Select strips in the mixer first." when none; master's mode can be set; a deleted strip leaves the selection at the next refetch. Pinned by `modeCommandsSetTheSelectedStrips` and `theSelectionForgetsStripsThatGo` (Tasks 3–4).
5. **The Strip Chart while a write runs** (the shown lane is not the one written, the write's target is not in the rows, the transport stops, the lane appears mid-pass). Expected: the preview is drawn only for rows whose target is being written, disappears at Stop, and a lane holding only one target's automation opens on that target. Pinned by `thePreviewShowsOnlyWhileItsTargetIsWritten` and `anAutomationLaneOpensOnItsTarget` (Task 6).

## Plan Rulings

1. **Ruling (mixer moves are gestures):** `MixerModel`'s fader, pan and send-level drags and its mute and solo clicks go through `Session.gesture` (plan 5e-1): mouse down → `touch(value: nil)` (the core starts from the value heard now, plan 5e-1 Ruling 26: the mixer shows static values, so its own value would jump an automated fader), each move → `move`, mouse up → `release`; a click → touch at the toggled value, value and release at once. `setGain`, `setPan`, `dragStarted`, `dragEnded` go (the core opens and closes the static edit); a double-click on the dB label is a click at 0 dB. Send "Pre" and "On" and every routing control stay plain edits (no automation target).
   - Why: §7.1 "It replaces direct `set_gain`/`set_pan`/`set_param` calls for user moves".
   - Cost if wrong: none (the undo labels stay "Set Fader", "Set Pan", "Set Send Level", "Mute", "Solo").
2. **Ruling (what a touched control shows):** while touched, a control shows the value the hand set (`MixerModel.live`, from its first move; until then what it showed), not the snapshot; after release, the snapshot's (refetched as today). Showing the heard (automated or held) value while playing is a backlog row (plan 5e-1 Task 11). Values go to the core as automation's normalised values: faders and sends through `AutomationLaw.faderNorm` of the dB the slider position gives (`MixerMath.faderDb`), pan as `(pan + 1) / 2`.
   - Cost if wrong: a fader in Read jumps back to the static value at release instead of following its automation (the next refetch shows the static value; automation drives what is heard).
3. **Ruling (rings):** a control has a ring while its target is touched here or in the core (`automation_state().touched`, polled with the meters at 30 Hz), so a channel or plugin touch shows too. The core lists active touches only, never Write holds or released Latch holds (plan 5e-1 M8), so a Write strip's controls are not all ringed.
4. **Ruling (colours):** Off gray, Read green, Touch yellow, Latch orange, Write red, Trim purple (system colours), the popup's title tinted; Read's green is the dim default so a strip that writes stands out.
   - Cost if wrong: a theme pass (SP7) recolours.
5. **Ruling (the selected strips):** the mixer keeps a strip selection on `DocumentModel.selectedStrips`: a click on a strip's name selects it, ⌘-click adds or removes; a double-click still renames. `automation.mode.<mode>` sets the selected strips' mode (one undo step), `unavailable` "Select strips in the mixer first." with none. Strips that go leave the selection at the next refetch.
   - Why: §7.5 names "the selected strips" and the mixer had no selection.
   - Cost if wrong: one more click before a menu command.
6. **Ruling (the Automation menu):** a new menu "Automation" with the six `automation.mode.<mode>` items (titles "Off" … "Trim" under a "Selected Strips" header) and "Suspend Automation" (a checkmark toggle, `automation.suspend`); no default keys. "Set All Strips to ▸ <mode>" is in every strip's name menu and calls `automation.set_mode` with all strips.
   - Cost if wrong: the menu moves later.
7. **Ruling (Settings › Automation):** a tab "Automation" with After Write (Off, Read, Touch, Latch, Trim: Write is refused by the core, plan 5e-1 Ruling 19), Return time (a stepper 0–2000 ms in 50 ms steps, the typed value clamped), Thinning (Low, Medium, High) and "Write plugin parameters only when touched" (on, disabled, with the spec's note). Kept in the preferences (`VizDAW.automation`), handed to the core at launch and on change; a refusal keeps the old value and says why.
8. **Ruling (the plugin window's sliders):** `PluginWindowModel.setParam` touches the parameter at the slider's value on its first move and moves it after; `commitParam` releases it; values normalised by the parameter's range. The core's gesture path (source UI) sends each value to the plugin on every path and commits "Change Plugin Parameter" only on the static path (plan 5e-1 Ruling 15), so the window no longer calls `setParam`/`commitParams` on the session itself.
9. **Ruling (the preview):** the Strip Chart asks `automation_preview` for each automation row while the document plays and its shown sequence is the playing one, at the transport poll's rate, and draws the points as a red line over the row; nothing when stopped.
   - Cost if wrong: a little more work per poll while playing.
10. **Ruling (an automation lane's own row):** when the Strip Chart's shown lane holds automation events of exactly one target and nothing else, and its saved rows have no automation row, that target's row comes first (the spec's "with its Strip Chart view set to T"); editing a row then saves it as usual.
    - Why: the Strip Chart's rows are per pane, not per lane (4d), so "set to T" is read from the lane's content.
    - Cost if wrong: a hand-made lane holding only one target also opens on it (which is what it holds).
11. **Ruling (5d-2 T2 M1/M2: saving while recording):** `DocumentModel.saveAction(forceAs:) -> SaveAction` (`.panel`, `.save`, `.refused(String)`), pure: it reads the path once and sets no status; the caller shows a refusal. Untitled Save while recording says "Stop recording to save this new project: its take is being written into it." (Save As keeps its message).
12. **Ruling (5d-2 T3 M1: the record popover's keys):** the popover holds text editing only while a punch field has focus (`RecordModel.punchFocusChanged(from:to:)` commits the old field's draft and begins or ends text editing), so R, Space and Return reach the window otherwise. The owner checks it by eye (README 166): a transient popover's key events may not reach the main window at all.
13. **Ruling (5d-2 T7 Minor 2–4):** Settings › Audio re-reads the device once a second while shown (the round trip appears when the input opens); the holder is chosen by `AudioPreferences.inputHolder(_:)` (the first session whose input names a device); Software Monitoring reaches every open document's meters at once (`LaneInputs.refreshMonitoring()`).
14. **Ruling (5d-2 T5 M3, N2):** the lane header's monitor button gets the tooltip `LaneInputs.help(mode)` (an `NSView` tool-tip rectangle per row); the meter's doc comment moves onto `meter(_:rowTop:inputs:)`.
15. **Ruling (5d-2 T4 M4's "I3 restart"):** the restart half is the core's (`a_new_input_device_reopens_an_open_input`, 5d-2 pre-flight I3) and is covered there; this plan tests the app half ("opening" cleared on disarm).

## Carried items

| Item (source) | Where |
|---|---|
| T2 M1, M2: untitled Cmd-S wording; `wantsSavePanel` side effect, stale read, `needsSaveAs` twice (5d-2 parked) | Task 7 (Ruling 11) |
| T3 M1: the popover holds text editing while open (5d-2 parked; owner eye check first) | Task 7 (Ruling 12), README 166 |
| T5 M3 other half: the monitor button's tooltip; N2: the meter's doc comment (5d-2 parked; re-review) | Task 7 (Ruling 14) |
| T7 Minor 2, 3, 4: the round trip readout, `readDevice`'s holder, the Software Monitoring toggle (5d-2 parked) | Task 7 (Ruling 13) |
| T2 M3/M4, T4 M4, T5 M5, T6 Minor 3, T9 M3: test gaps (5d-2 parked) | Task 8 |
| m3, m4, m7, T5 M2, T5 M4 (other half), T6 Minor 2, T9 drawing items, I4, owner hardware checks | Not here: backlog rows, SP7 and the README's owner checks, as the carry list says |

## File map

| Folder | New | Changed |
|---|---|---|
| `crates/core-control` | — | `src/registry.rs`; tests `commands.rs` |
| `app/Sources/VizDAWAppModel` | `AutomationPreferences.swift`, `AutomationModes.swift` | `MixerModel.swift`, `DocumentModel.swift`, `AutomationCommands.swift`, `PluginWindowModel.swift`, `StripChartModel.swift`, `RecordModel.swift`, `RecordOptions.swift`, `AudioPreferences.swift`, `LaneInputs.swift`, `GrowingTake.swift`, `ControlCommands.swift` |
| `app/Sources/VizDAWEditors` | — | `StripChartView.swift`, `LaneHeaderView.swift`, `ArrangementView.swift`, `ArrangementLayout.swift` |
| `app/Sources/VizDAW` | `AutomationSetup.swift`, `AutomationSettingsView.swift` | `VizDAWApp.swift`, `MixerView.swift`, `AppCommands.swift`, `RecordControls.swift`, `AudioSettingsView.swift` |
| `app/Tests/VizDAWAppModelTests` | `AutomationPreferencesTests.swift`, `MixerGestureTests.swift`, `RecordUiCarryTests.swift` | `MixerModelTests.swift`, `AutomationCommandsTests.swift`, `PluginWindowModelTests.swift`, `StripChartModelTests.swift`, `RecordOptionsTests.swift`, `LaneHeaderInputTests.swift`, `GrowingTakeTests.swift` |
| docs | — | `app/README.md` |

### Task 1: Settings › Automation, kept in the preferences

**Files:**
- Create: `app/Sources/VizDAWAppModel/AutomationPreferences.swift`, `app/Sources/VizDAW/AutomationSetup.swift`, `app/Sources/VizDAW/AutomationSettingsView.swift`
- Modify: `app/Sources/VizDAW/VizDAWApp.swift` (apply at launch; the tab)
- Test: create `app/Tests/VizDAWAppModelTests/AutomationPreferencesTests.swift`

**Interfaces:**
- Consumes: plan 5e-1's `automationSettings()`, `setAutomationSettings(settings:)`, `AutomationSettingsRow`, `AutomationModeRow`, `ThinningRow`.
- Produces:
  ```swift
  public struct AutomationSettings: Codable, Equatable, Sendable {
      /// "off", "read", "touch", "latch" or "trim" (not "write").
      public var afterWrite = "latch"
      public var returnMs: UInt32 = 250
      /// "low", "medium" or "high".
      public var thinning = "medium"
      public init()
      /// Tolerant: a missing or damaged field keeps its default.
      public init(from decoder: Decoder) throws
      public var row: AutomationSettingsRow { get }
      public static let afterWriteModes: [String]   // off, read, touch, latch, trim
      public static let maxReturnMs: UInt32         // 2000
  }
  @MainActor public protocol AutomationSink: AnyObject { func apply(_ row: AutomationSettingsRow) throws }
  @Observable @MainActor public final class AutomationPreferences {
      public static let key = "VizDAW.automation"
      public private(set) var settings: AutomationSettings
      public var problem: String?
      public init(defaults: UserDefaults, sink: any AutomationSink)
      public func applyAll()
      public func setAfterWrite(_ mode: String)
      public func setReturnMs(_ ms: UInt32)   // clamped to 0…2000
      public func setThinning(_ t: String)
  }
  ```
  App globals `automationPreferences` and `LiveAutomationSink` (calls `setAutomationSettings(settings:)`), as `audioPreferences`/`LiveAudioSink` are.

Ruling 7. A change goes to the core first and is kept only once it took (`AudioPreferences`' rule); an unknown mode or thinning name changes nothing.

- [ ] **Step 1: Write the failing test**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Tests/VizDAWAppModelTests/AutomationPreferencesTests.swift <<'VIZDAW_EOF'
import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

/// Stands in for the core's automation setter.
@MainActor
final class FakeAutomationSink: AutomationSink {
    var applied: [AutomationSettingsRow] = []
    var error: VizError?
    func apply(_ row: AutomationSettingsRow) throws {
        if let e = error { throw e }
        applied.append(row)
    }
}

/// Settings › Automation (plan 5e-2 Task 1, Ruling 7; spec §7.5): kept in
/// the preferences, handed to the core at launch and on change, kept only
/// once the core took them; damaged values keep their defaults.
@MainActor
@Suite struct AutomationPreferencesTests {
    @Test func launchHandsTheSettingsToTheCore() {
        let sink = FakeAutomationSink()
        let p = AutomationPreferences(defaults: MemoryDefaults(), sink: sink)
        #expect(p.settings == AutomationSettings())
        p.applyAll()
        #expect(sink.applied == [AutomationSettingsRow(afterWrite: .latch, returnMs: 250, thinning: .medium)])
    }

    @Test func settingsAreKeptOnlyOnceTheCoreTookThem() {
        let defaults = MemoryDefaults()
        let sink = FakeAutomationSink()
        let p = AutomationPreferences(defaults: defaults, sink: sink)
        p.setAfterWrite("touch")
        p.setReturnMs(3000)
        p.setThinning("high")
        #expect(p.settings.afterWrite == "touch" && p.settings.returnMs == 2000 && p.settings.thinning == "high")
        #expect(sink.applied.last == AutomationSettingsRow(afterWrite: .touch, returnMs: 2000, thinning: .high))
        let again = AutomationPreferences(defaults: defaults, sink: FakeAutomationSink())
        #expect(again.settings == p.settings, "read back from the preferences")
        p.setAfterWrite("write")
        #expect(p.settings.afterWrite == "touch", "Write is no After Write mode")
        sink.error = .Model(message: "the return time is 0 to 2000 ms")
        p.setReturnMs(100)
        #expect(p.settings.returnMs == 2000)
        #expect(p.problem == "the return time is 0 to 2000 ms")
        sink.error = nil
        p.setReturnMs(100)
        #expect(p.settings.returnMs == 100 && p.problem == nil)
        p.setThinning("extreme")
        #expect(p.settings.thinning == "high", "an unknown name changes nothing")
    }

    @Test func damagedAutomationSettingsKeepTheirDefaults() {
        let defaults = MemoryDefaults()
        let damaged = #"{"afterWrite": "write", "returnMs": 99999, "thinning": 7}"#
        defaults.set(Data(damaged.utf8), forKey: AutomationPreferences.key)
        #expect(AutomationPreferences(defaults: defaults, sink: FakeAutomationSink()).settings == AutomationSettings())
        defaults.set(Data(#"{"returnMs": 500}"#.utf8), forKey: AutomationPreferences.key)
        var want = AutomationSettings()
        want.returnMs = 500
        #expect(AutomationPreferences(defaults: defaults, sink: FakeAutomationSink()).settings == want)
        defaults.set(Data("not json".utf8), forKey: AutomationPreferences.key)
        #expect(AutomationPreferences(defaults: defaults, sink: FakeAutomationSink()).settings == AutomationSettings())
    }
}
VIZDAW_EOF
```

- [ ] **Step 2: Run the test to verify it fails**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh test 2>&1 | grep -E "error:|Test run" | head -5
```

Expected: FAIL to compile (`AutomationPreferences` does not exist).

- [ ] **Step 3: The preferences, the sink, the view**

Create `AutomationPreferences.swift` with the items of the Interfaces block (decoding field by field as `AudioSettings` does, with `KeyedDecodingContainer.field`), `AutomationSetup.swift` with `LiveAutomationSink` and `@MainActor let automationPreferences = AutomationPreferences(defaults: MemoryDefaults.appStore(), sink: LiveAutomationSink())`, and `AutomationSettingsView.swift`: a `Form` (`.formStyle(.grouped)`, width 500) with a Picker "After Write" (Off, Read, Touch, Latch, Trim, by `AutomationModes.title`, Task 3, or plain titles until then), a Stepper "Return time: <n> ms" (0…2000 by 50), a Picker "Thinning" (Low, Medium, High) with the caption "How closely written automation follows the move: 0.1 dB on faders and sends, 0.25 % elsewhere at Medium.", a disabled Toggle "Write plugin parameters only when touched" (on) with the caption "Always on: writing every parameter of every plugin would bury the lanes.", and the problem line in red.

In `app/Sources/VizDAW/VizDAWApp.swift`:

Find:
```swift
        audioPreferences.applyAll()
```
Replace:
```swift
        audioPreferences.applyAll()
        automationPreferences.applyAll()
```

Find:
```swift
                RecordingSettingsView(prefs: audioPreferences, store: appStore)
                    .tabItem { Label("Recording", systemImage: "record.circle") }
```
Replace:
```swift
                RecordingSettingsView(prefs: audioPreferences, store: appStore)
                    .tabItem { Label("Recording", systemImage: "record.circle") }
                AutomationSettingsView(prefs: automationPreferences)
                    .tabItem { Label("Automation", systemImage: "slider.vertical.3") }
```

- [ ] **Step 4: Run the tests to verify they pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/build-ffi.sh && scripts/swift.sh build 2>&1 | grep -E "error:|warning:" | grep -v MemoryDefaults | sort -u
scripts/swift.sh test 2>&1 | grep -E "Test run"
```

Expected: no error or new warning; "Test run with 851 tests … passed".

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add app/Sources/VizDAWAppModel/AutomationPreferences.swift app/Sources/VizDAW/AutomationSetup.swift \
  app/Sources/VizDAW/AutomationSettingsView.swift app/Sources/VizDAW/VizDAWApp.swift \
  app/Tests/VizDAWAppModelTests/AutomationPreferencesTests.swift
git commit -m "app: Settings › Automation, kept in the preferences and handed to the core (5e-2 Task 1)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -3
```

### Task 2: The mixer's moves are gestures

**Files:**
- Modify: `app/Sources/VizDAWAppModel/MixerModel.swift` (the gesture API; `live`, `touched`, `pollAutomation`, modes; `setGain`, `setPan`, `dragStarted`, `dragEnded` go), `app/Sources/VizDAWAppModel/DocumentModel.swift` (`selectedStrips`)
- Create: `app/Sources/VizDAWAppModel/AutomationModes.swift`
- Test: create `app/Tests/VizDAWAppModelTests/MixerGestureTests.swift`; modify `app/Tests/VizDAWAppModelTests/MixerModelTests.swift` (the four drag tests are replaced by Task 2's)

**Interfaces:**
- Consumes: `Session.gesture(target:phase:)`, `setAutomationMode(strips:mode:)`, `automationState()`, `StripRow.automation` (plan 5e-1); `AutomationLaw.faderNorm`/`faderDb` (4d); `MixerMath.faderDb`.
- Produces:
  ```swift
  public enum AutomationModes {
      public static let all: [AutomationModeRow]          // off, read, touch, latch, write, trim
      nonisolated public static func title(_ m: AutomationModeRow) -> String   // "Off" … "Trim"
      /// Ruling 4: "gray", "green", "yellow", "orange", "red", "purple".
      nonisolated public static func colorName(_ m: AutomationModeRow) -> String
  }
  extension MixerModel {
      /// Shown values of touched targets (normalised) once moved, Ruling 2.
      public var live: [AutomationTargetRow: Double] { get }
      /// Mouse-down: `touch(value: nil)` on the core (the value heard now, plan 5e-1 Ruling 26).
      public func touch(_ t: AutomationTargetRow)
      public func move(_ t: AutomationTargetRow, to v: Double)
      public func release(_ t: AutomationTargetRow)
      /// Touch at `v`, value and release at once (mute, solo, the 0 dB double-click).
      public func click(_ t: AutomationTargetRow, _ v: Double)
      public func isTouched(_ t: AutomationTargetRow) -> Bool
      /// The fader dB, pan and send level as shown (live while touched).
      public func gainDb(_ s: StripRow) -> Double
      public func pan(_ s: StripRow) -> Double
      public func sendLevelDb(_ s: StripRow, _ index: Int) -> Double
      public func pollAutomation()
      public func setMode(_ strip: UInt64, _ mode: AutomationModeRow)
      public func setAllModes(_ mode: AutomationModeRow)
      public func select(_ strip: UInt64, adding: Bool)
      public func isSelected(_ strip: UInt64) -> Bool
  }
  ```
  `toggleMute(_:)` and `toggleSolo(_:)` become clicks; `endOpenDrag()` releases every local touch; `DocumentModel.selectedStrips: Set<UInt64>`.

Rulings 1–3 and 5. `touch` sends `.touch(value: nil)` and keeps the target touched without a `live` value; the first `move` sets `live`. A click's value is the toggled one (mute and solo 1 or 0, the double-click `faderNorm(0)`). For values the model computes itself (pan from the shown pan): pan `(pan + 1) / 2`, a fader or send `AutomationLaw.faderNorm(db)`. A touch the core refuses raises the alert "Fader Failed"/"Pan Failed"/"Send Failed"/"Mute Failed"/"Solo Failed" (as today) and is not kept (so no release is sent); `refetch` drops touches and selection of strips that went (releasing their touches once).

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Tests/VizDAWAppModelTests/MixerGestureTests.swift <<'VIZDAW_EOF'
import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

/// The mixer's moves are gestures (plan 5e-2 Task 2; spec §7.1, §7.5):
/// drags are touch, values and release; clicks are all three at once; a
/// touched control shows the hand's value and has a ring; modes per strip
/// and for all; the strip selection.
@MainActor
@Suite struct MixerGestureTests {
    func model(_ f: FakeSession = mixerFake()) throws -> (MixerModel, FakeSession) {
        (try MixerModel(document: DocumentModel(session: f)), f)
    }

    let fader = AutomationTargetRow.stripFader(strip: 1)

    @Test func aFaderDragIsAGestureOnTheCore() throws {
        let (m, f) = try model()
        m.touch(fader)
        m.move(fader, to: 0.5)
        m.move(fader, to: 0.4)
        m.release(fader)
        #expect(
            f.gestures.map(\.1) == [.touch(value: nil), .value(value: 0.5), .value(value: 0.4), .release],
            "mouse-down sends no value: the core starts from the value heard now (5e-1 Ruling 26)")
        #expect(f.gestures.allSatisfy { $0.0 == fader })
        #expect(f.calls.isEmpty, "no direct setGain any more")
    }

    @Test func clicksAreTouchValueAndRelease() throws {
        let (m, f) = try model()
        let strip = try #require(m.strips.first)
        m.toggleMute(strip)
        #expect(f.gestures.map(\.1) == [.touch(value: 1), .value(value: 1), .release])
        #expect(f.gestures.allSatisfy { $0.0 == .stripMute(strip: 1) })
        f.gestures = []
        m.toggleSolo(strip)
        #expect(f.gestures.first?.0 == .stripSolo(strip: 1))
    }

    @Test func aTouchedControlShowsItsLiveValue() throws {
        let (m, _) = try model()
        let strip = try #require(m.strips.first)
        #expect(m.gainDb(strip) == 0)
        m.touch(fader)
        #expect(m.isTouched(fader) && m.gainDb(strip) == 0, "touched, still showing what it showed")
        m.move(fader, to: AutomationLaw.faderNorm(-6))
        #expect(abs(m.gainDb(strip) + 6) < 1e-9, "the hand's value")
        #expect(m.isTouched(fader))
        m.release(fader)
        #expect(m.gainDb(strip) == 0, "the snapshot's again")
        #expect(!m.isTouched(fader))
        let pan = AutomationTargetRow.stripPan(strip: 1)
        m.touch(pan)
        m.move(pan, to: 0.25)
        #expect(m.pan(strip) == -0.5)
    }

    @Test func ringsFollowTheCoresTouchedTargets() throws {
        let (m, f) = try model()
        f.automationRow.touched = [.stripPan(strip: 2)]
        m.pollAutomation()
        #expect(m.isTouched(.stripPan(strip: 2)), "a channel or plugin touch has a ring")
        f.automationRow.touched = []
        m.pollAutomation()
        #expect(!m.isTouched(.stripPan(strip: 2)))
    }

    /// Two controls touched at once (plan 5e-1 Ruling 14: one shared
    /// static edit in the core, so the real core takes both).
    @Test func twoTargetsTouchedAtOnceAreIndependent() throws {
        let (m, f) = try model()
        let other = AutomationTargetRow.stripFader(strip: 2)
        m.touch(fader)
        m.touch(other)
        m.release(fader)
        #expect(m.isTouched(other) && !m.isTouched(fader))
        #expect(f.gestures.filter { $0.1 == .release }.map(\.0) == [fader])
    }

    @Test func touchesAreReleasedOnceWhenTheWindowClosesOrTheStripGoes() throws {
        let (m, f) = try model()
        m.touch(fader)
        m.endOpenDrag()  // the window closes
        m.endOpenDrag()
        m.release(fader)
        #expect(f.gestures.filter { $0.1 == .release }.count == 1)
        f.gestures = []
        let other = AutomationTargetRow.stripFader(strip: 2)
        m.touch(other)
        f.mixerSnap.strips.removeAll { $0.id == 2 }
        m.load([])  // a refetch sees strip 2 gone
        #expect(f.gestures.map(\.1) == [.touch(value: nil), .release])
        m.release(other)
        #expect(f.gestures.filter { $0.1 == .release }.count == 1)
    }

    @Test func aRefusedTouchSendsNoRelease() throws {
        let (m, f) = try model()
        f.gestureError = .Busy(message: "Finish the open edit first.")
        m.touch(fader)
        #expect(m.document.alert?.message == "Finish the open edit first.")
        f.gestureError = nil
        m.move(fader, to: 0.3)
        m.release(fader)
        #expect(f.gestures.isEmpty, "nothing after a refused touch")
    }

    @Test func modesAreSetPerStripAndForAll() throws {
        let (m, f) = try model()
        m.setMode(2, .touch)
        #expect(f.modeCalls.last?.0 == [2] && f.modeCalls.last?.1 == .touch)
        m.setAllModes(.latch)
        #expect(f.modeCalls.last?.0 == [1, 2, 0] && f.modeCalls.last?.1 == .latch)
        #expect(Set(AutomationModes.all.map(AutomationModes.colorName)).count == 6, "one colour each")
        #expect(AutomationModes.all.map(AutomationModes.title) == ["Off", "Read", "Touch", "Latch", "Write", "Trim"])
    }

    @Test func theSelectionForgetsStripsThatGo() throws {
        let (m, f) = try model()
        m.select(1, adding: false)
        m.select(2, adding: true)
        #expect(m.document.selectedStrips == [1, 2])
        m.select(2, adding: true)
        #expect(m.document.selectedStrips == [1], "⌘-click removes")
        m.select(0, adding: false)
        #expect(m.document.selectedStrips == [0], "master can be selected")
        m.select(2, adding: true)
        f.mixerSnap.strips.removeAll { $0.id == 2 }
        m.load([])
        #expect(m.document.selectedStrips == [0])
    }
}
VIZDAW_EOF
```

In `app/Tests/VizDAWAppModelTests/MixerModelTests.swift`, delete `aFaderDragIsOneEditAndHeardOnEveryMove`, `aDragIsEndedOnceWhenTheWindowClosesOrTheStripGoes`, `aFailedBeginMeansNoEnd` and `aNewDragEndsTheOldOne` (their behaviour moved to the core's static path, plan 5e-1, and to the tests above).

- [ ] **Step 2: Run the tests to verify they fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh test 2>&1 | grep -E "error:|Test run" | head -5
```

Expected: FAIL to compile (`MixerModel.touch` does not exist).

- [ ] **Step 3: The gesture API and the modes**

Create `AutomationModes.swift`. In `MixerModel.swift`, replace `dragStarted`, `dragEnded`, `endOpenDrag` (which now releases every local touch once), `setGain`, `setPan` and the private `dragOpen`/`dragStrip` with the API of the Interfaces block, kept in `private(set) var live: [AutomationTargetRow: Double]` and `private var coreTouched: Set<AutomationTargetRow>`; `toggleMute`/`toggleSolo` call `click`; `editSend` stays for Pre and On; `refetch` releases touches of strips that went and drops them from `document.selectedStrips`. `pollAutomation` reads `session.automationState()` (Busy and errors keep the last). Add `public var selectedStrips: Set<UInt64> = []` to `DocumentModel` (doc: "The mixer's selected strips (plan 5e-2 Ruling 5), for `automation.mode.<mode>`.").

- [ ] **Step 4: Run the tests to verify they pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh build 2>&1 | grep -E "error:|warning:" | grep -v MemoryDefaults | sort -u
scripts/swift.sh test 2>&1 | grep -E "Test run"
```

Expected: the build fails in `MixerView.swift` only where it calls the removed methods (Task 3 rewrites those lines): make the minimal change now (`FaderView`, `SendView` and the pan slider call `touch`/`move`/`release` as Task 3 Step 3 says) so the build is clean; Swift tests pass (851 − 4 + 9 = 856).

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add app/Sources/VizDAWAppModel/MixerModel.swift app/Sources/VizDAWAppModel/DocumentModel.swift \
  app/Sources/VizDAWAppModel/AutomationModes.swift app/Sources/VizDAW/MixerView.swift \
  app/Tests/VizDAWAppModelTests/MixerGestureTests.swift app/Tests/VizDAWAppModelTests/MixerModelTests.swift
git commit -m "app: the mixer's moves are gestures; live values, rings, modes and the strip selection in the model (5e-2 Task 2)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -3
```

### Task 3: The mixer's mode popups, rings, selection and "Set All Strips to…"

**Files:**
- Modify: `app/Sources/VizDAW/MixerView.swift`

**Interfaces:**
- Consumes: Task 2's API and `AutomationModes`.
- Produces: the views only (checked by eye, README 165).

Rulings 2–6. No new model logic: every behaviour is Task 2's, tested there.

- [ ] **Step 1: The controls**

In `app/Sources/VizDAW/MixerView.swift`:
- `FaderView`: the slider's value is `MixerMath.faderPosition(model.gainDb(strip))`; `set` calls `model.move(.stripFader(strip: strip.id), to: AutomationLaw.faderNorm(MixerMath.faderDb($0)))`; its editing closure calls `model.touch(…)` when editing begins and `model.release(…)` when it ends; the dB label shows `model.gainDb(strip)`; its double-click calls `model.click(.stripFader(strip: strip.id), AutomationLaw.faderNorm(0))`. The slider gets `.overlay(RoundedRectangle(cornerRadius: 4).stroke(Color.accentColor, lineWidth: 2).opacity(model.isTouched(.stripFader(strip: strip.id)) ? 1 : 0))` (the ring, Ruling 3).
- The pan slider and `SendView`'s level slider likewise (`.stripPan(strip:)` with `(pan + 1) / 2`, `.sendLevel(strip:send:)` with `UInt16(index)`), with their rings; the M and S toggles get a ring when their target is touched.
- Above the fader, a `Menu(AutomationModes.title(strip.automation))` with one `Button` per mode (a checkmark on the current one) calling `model.setMode(strip.id, $0)`, `.tint(color(strip.automation))`, `.controlSize(.small)`, `.help("Automation: \(AutomationModes.title(strip.automation))")`, where `color(_:)` maps `AutomationModes.colorName` to `Color.gray`, `.green`, `.yellow`, `.orange`, `.red`, `.purple`.
- The strip's name: `.onTapGesture { model.select(strip.id, adding: NSEvent.modifierFlags.contains(.command)) }` after the existing double-click rename, and a selected strip's background uses `Color.accentColor.opacity(0.25)`; its context menu gains `Menu("Set All Strips to") { ForEach(AutomationModes.all, id: \.self) { m in Button(AutomationModes.title(m)) { model.setAllModes(m) } } }`.
- `MixerView`'s 30 Hz task calls `model.pollAutomation()` after `model.pollLevels()`.

- [ ] **Step 2: Build and test**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh build 2>&1 | grep -E "error:|warning:" | grep -v MemoryDefaults | sort -u
scripts/swift.sh test 2>&1 | grep -E "Test run"
```

Expected: no error or new warning; Swift 856 tests pass.

- [ ] **Step 3: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add app/Sources/VizDAW/MixerView.swift
git commit -m "app: the mixer's automation-mode popups, touched rings, strip selection and Set All Strips to (5e-2 Task 3)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -3
```

### Task 4: The Automation menu: `automation.mode.<mode>` and Suspend Automation

**Files:**
- Modify: `crates/core-control/src/registry.rs` (six commands; `automation.suspend` becomes a menu item), `crates/core-control/tests/commands.rs`
- Modify: `app/Sources/VizDAWAppModel/AutomationCommands.swift` (`MenuCommands`), `app/Sources/VizDAWAppModel/ControlCommands.swift` (nothing if `AutomationCommands.ids` already covers them: it must), `app/Sources/VizDAW/AppCommands.swift` (the menu)
- Test: `app/Tests/VizDAWAppModelTests/AutomationCommandsTests.swift`

**Interfaces:**
- Consumes: plan 5e-1's `AutomationCommands` and `automation.suspend`; Task 2's `selectedStrips`.
- Produces: registry `automation.mode.off`, `.read`, `.touch`, `.latch`, `.write`, `.trim` (menu "Automation", titles "Off" … "Trim", `document: true`, no params, menu items); `automation.suspend` with `menu_item: true` (menu "Automation", "Suspend Automation"). `extension AutomationCommands: MenuCommands` (`enabled`: a mode with some strip selected, suspend always; `isOn`: suspend → `automationSuspended()`, modes false; `run`: a mode sets `m.selectedStrips` (sorted) and applies the snapshot, `.unavailable("Select strips in the mixer first.")` with none; suspend toggles or sets). `AutomationCommands.modeIDs: [String: AutomationModeRow]`; `ids` includes them.

Rulings 5–6.

- [ ] **Step 1: Write the failing tests**

In `crates/core-control/tests/commands.rs`, change the count to 138 and append to its message "; 5e-2 adds the six automation.mode commands". Append:

```rust
/// Plan 5e-2 Task 4: the mode commands are Automation menu items with no
/// parameters; Suspend Automation is a menu item now.
#[test]
fn automation_menu_commands_take_no_params() {
    for m in ["off", "read", "touch", "latch", "write", "trim"] {
        let id = format!("automation.mode.{m}");
        let spec = registry::find(&id).unwrap();
        assert!(spec.menu_item && spec.document && spec.menu == "Automation", "{id}");
        assert!(matches!(ok("command.run", json!({ "id": id })), Command::CommandRun { .. }));
        let (code, _) = err("command.run", json!({ "id": id, "params": { "strips": "all" } }));
        assert_eq!(code, ErrorCode::InvalidParams, "{id}");
    }
    let s = registry::find("automation.suspend").unwrap();
    assert!(s.menu_item && s.menu == "Automation" && !s.document);
}
```

Append to `AutomationCommandsTests`:

```swift
    @Test func modeCommandsSetTheSelectedStrips() throws {
        let (_, m, f) = try doc()
        #expect(!AutomationCommands.enabled("automation.mode.write", m))
        #expect(AutomationCommands.run("automation.mode.write", on: nil, m) == .unavailable("Select strips in the mixer first."))
        m.selectedStrips = [2, 0]
        #expect(AutomationCommands.enabled("automation.mode.write", m))
        #expect(AutomationCommands.run("automation.mode.write", on: nil, m) == .done)
        #expect(f.modeCalls.last?.0 == [0, 2] && f.modeCalls.last?.1 == .write)
        #expect(AutomationCommands.enabled(AutomationCommands.suspend, m))
        #expect(Set(AutomationCommands.modeIDs.keys).isSubset(of: ControlCommands.handled))
    }
```

(`KeyOutcome` is `Equatable`, as other command tests compare it.)

- [ ] **Step 2: Run the tests to verify they fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-control --test commands 2>&1 | tail -3
```

Expected: FAIL (the count is 132; `automation.mode.off` is unknown).

- [ ] **Step 3: The registry, the handlers, the menu**

In `crates/core-control/src/registry.rs`, add the six `cmd("automation.mode.<m>", "Automation", "<Title>", None, true)` entries and change `automation.suspend`'s spec to a menu item in menu "Automation". In `AutomationCommands.swift`, add `modeIDs`, include them in `ids`, and the `MenuCommands` conformance; `run(_:args:doc:_:)` (the channel's) sends a mode ID to the menu `run` (mapping its `KeyOutcome` with `ControlCommands.failure`). In `app/Sources/VizDAW/AppCommands.swift`, after the `CommandMenu("Step") { … }` block (its `body` has 8 top-level builders at `1f6057a`: the `Group` and seven `CommandMenu`s; this menu is the 9th, and older SDKs' `CommandsBuilder` takes at most 10, so add no other top-level item: pre-flight M22):

```swift
        CommandMenu("Automation") {
            Text("Selected Strips")
            ForEach(["off", "read", "touch", "latch", "write", "trim"], id: \.self) {
                commandItem("automation.mode.\($0)", AutomationCommands.self)
            }
            Divider()
            commandToggle(AutomationCommands.suspend, AutomationCommands.self)
        }
```

- [ ] **Step 4: Run the tests to verify they pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -2
cargo clippy --workspace --all-targets --target x86_64-unknown-linux-gnu --target-dir target/linux-check -- -D warnings 2>&1 | tail -2
cargo test -p core-control 2>&1 | grep -E "test result|FAILED" | grep -v " 0 failed"
scripts/build-ffi.sh && scripts/swift.sh build 2>&1 | grep -E "error:|warning:" | grep -v MemoryDefaults | sort -u
scripts/swift.sh test 2>&1 | grep -E "Test run"
```

Expected: all pass; Swift 857 (`everyRegisteredMenuCommandHasAHandler` sees the six IDs handled).

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-control/src/registry.rs crates/core-control/tests/commands.rs app/Sources/VizDAWAppModel/AutomationCommands.swift \
  app/Sources/VizDAWAppModel/ControlCommands.swift app/Sources/VizDAW/AppCommands.swift app/Tests/VizDAWAppModelTests/AutomationCommandsTests.swift
git commit -m "core-control, app: the Automation menu: automation.mode.<mode> for the selected strips and Suspend Automation (5e-2 Task 4)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -3
```

### Task 5: The plugin window's sliders are gestures

**Files:**
- Modify: `app/Sources/VizDAWAppModel/PluginWindowModel.swift` (`setParam`, `commitParam`, `endOpenDrags`)
- Test: `app/Tests/VizDAWAppModelTests/PluginWindowModelTests.swift`

**Interfaces:**
- Consumes: `Session.gesture` with `.pluginParam(slot:param:)`.
- Produces: no new API. `setParam(_ id:, _ value:)` (plain value) touches the parameter at its normalised value on the first move of a drag and moves it after; `commitParam(_ id:)` releases it; `endOpenDrags()` releases what is still dragged. The shown value follows the slider as today.

Ruling 8. The normalised value is `(v − min) / (max − min)` clamped to 0–1 (a parameter with `max <= min` is 0).

- [ ] **Step 1: Update the tests**

In `PluginWindowModelTests.swift`, replace `aParameterDragIsHeardAndCommittedOnce`'s body after `let w = …` with:

```swift
        w.setParam(0, 0.7)
        w.setParam(0, 0.2)
        w.commitParam(0)
        #expect(f.gestures.map(\.1) == [.touch(value: 0.7), .value(value: 0.2), .release])
        #expect(f.gestures.allSatisfy { $0.0 == .pluginParam(slot: 10, param: 0) })
        #expect(f.calls.isEmpty, "the core commits the step at the release")
        #expect(w.params.first?.value == 0.2)
```

and `aDragLeftOpenByTheClosingWindowIsCommitted`'s expectations with:

```swift
        w.setParam(0, 0.9)
        #expect(w.closed())
        #expect(f.gestures.map(\.1) == [.touch(value: 0.9), .release])
        w.endOpenDrags()
        #expect(f.gestures.filter { $0.1 == .release }.count == 1, "released once")
```

and add:

```swift
    /// Ruling 8: values are normalised by the parameter's range.
    @Test func sliderValuesAreNormalisedByTheRange() throws {
        let f = mixerFake()
        f.paramTable[10] = [
            ParamRow(
                id: 3, name: "Cutoff", module: "", min: 20, max: 20_020, default: 1000, value: 1000,
                stepped: false, automatable: true, hidden: false)
        ]
        let w = PluginWindowModel(document: try DocumentModel(session: f), slot: 10)
        w.setParam(3, 10_020)
        w.setParam(3, 99_999)
        w.commitParam(3)
        #expect(f.gestures.map(\.1) == [.touch(value: 0.5), .value(value: 1), .release])
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh test 2>&1 | grep -E "PluginWindowModel|Test run" | head -6
```

Expected: the three tests FAIL (`gestures` is empty; `liveSets` and `commitParams` are used).

- [ ] **Step 3: Gestures in the window model**

Rewrite `setParam`, `commitParam` and `endOpenDrags` as the Interfaces block says (a refused touch shows "Parameter Failed" and is not kept). The window no longer sends the value to the session's `setParam` or commits `SetPluginParams` itself: the core's gesture path does both (plan 5e-1 Ruling 15).

- [ ] **Step 4: Run the tests to verify they pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh build 2>&1 | grep -E "error:|warning:" | grep -v MemoryDefaults | sort -u
scripts/swift.sh test 2>&1 | grep -E "Test run"
```

Expected: Swift 858 tests pass.

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add app/Sources/VizDAWAppModel/PluginWindowModel.swift app/Tests/VizDAWAppModelTests/PluginWindowModelTests.swift
git commit -m "app: the plugin window's sliders are gestures (5e-2 Task 5)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -3
```

### Task 6: The Strip Chart: the write in progress, and an automation lane's own row

**Files:**
- Modify: `app/Sources/VizDAWAppModel/StripChartModel.swift` (`preview`, `rows`), `app/Sources/VizDAWEditors/StripChartView.swift` (draws the preview; redraws with the transport poll while playing)
- Test: `app/Tests/VizDAWAppModelTests/StripChartModelTests.swift`

**Interfaces:**
- Consumes: `Session.automationPreview(target:)` (plan 5e-1), `AutoTarget.row`, `AutomationLaw.v32`, `DocumentModel.transport`.
- Produces: `StripChartModel.preview(_ row: StripLane) -> [StripValue]` (empty unless the row is an automation row, the document plays and its playing sequence is the chart's; each point `StripValue(id: nil, tick: p.tick, length: 0, value: AutomationLaw.v32(p.value), ramp: true)`); `rows` puts the shown lane's only target first (Ruling 10).

Rulings 9–10. `StripChartView.observe()` also tracks `document?.transport.tick` while the chart's sequence plays (so the preview moves with the poll); `drawContent` draws, after a row's values, `model.preview(lane)` as a 1.5-point `NSColor.systemRed` polyline through the scale's points.

- [ ] **Step 1: Write the failing tests**

Append to `StripChartModelTests.swift`, inside `StripChartModelTests` (its `make(_:)` opens the pane's chart on `rollFake()`, whose Song is sequence 3 with lanes 30 Groove, 31 Chords (two notes) and 32 Bass; the preview fake is plan 5e-1's `FakeSession.previewRows`):

```swift
    /// An automation point of `target` at `tick`.
    func autoRow(_ id: UInt64, tick: Int64, _ target: AutomationTargetRow, _ v: Double) -> EventRow {
        var r = noteRow(id, tick: tick)
        r.kind = .automation
        r.duration = 0
        r.automation = AutomationRow(target: target, value: v)
        r.value = AutomationLaw.v32(v)
        return r
    }

    /// Plan 5e-2 Ruling 9: the write in progress shows on its row while
    /// the chart's sequence plays, and nowhere else.
    @Test func thePreviewShowsOnlyWhileItsTargetIsWritten() throws {
        let (f, m, s) = try make()
        defer { withExtendedLifetime(m) {} }
        let row = StripLane.automation(.fader(strip: 1))
        f.previewRows = [PreviewPointRow(tick: 0, value: 0.5), PreviewPointRow(tick: 960, value: 1)]
        #expect(s.preview(row).isEmpty, "stopped")
        f.state.playing = true
        f.state.seq = 3
        m.poll()
        let p = s.preview(row)
        #expect(p.map(\.tick) == [0, 960])
        #expect(p.map(\.value) == [AutomationLaw.v32(0.5), AutomationLaw.v32(1)])
        #expect(p.allSatisfy { $0.id == nil && $0.ramp && $0.length == 0 })
        #expect(s.preview(.velocity).isEmpty, "only automation rows")
        f.state.seq = 4
        m.poll()
        #expect(s.preview(row).isEmpty, "another sequence plays")
    }

    /// Groove (lane 30) holding `events`, shown in the pane's chart.
    func groove(_ events: [EventRow]) throws -> (DocumentModel, StripChartModel) {
        let f = rollFake()
        f.laneEventTable["3/30"] = LaneEvents(seq: 3, lane: 30, rev: 0, events: events)
        let m = try DocumentModel(session: f)
        m.editor.openDetail(.lane(seq: 3, lane: 30), kind: .stripChart)
        return (m, StripChartModel.pane(m))
    }

    /// Ruling 10 (spec §7.4 "with its Strip Chart view set to T"): a lane
    /// holding only one target's automation opens on that target's row.
    @Test func anAutomationLaneOpensOnItsTarget() throws {
        let pan = AutomationTargetRow.stripPan(strip: 1)
        let (m, s) = try groove([autoRow(1, tick: 0, pan, 0.5), autoRow(2, tick: 960, pan, 0.25)])
        defer { withExtendedLifetime(m) {} }
        #expect(s.rows == [.automation(.pan(strip: 1)), .velocity], "its target first, then the saved rows")
        let (m2, s2) = try groove([autoRow(1, tick: 0, pan, 0.5), noteRow(3, tick: 0)])
        defer { withExtendedLifetime(m2) {} }
        #expect(s2.rows == [.velocity], "a lane with notes keeps its rows")
        let fader = AutomationTargetRow.stripFader(strip: 1)
        let (m3, s3) = try groove([autoRow(1, tick: 0, pan, 0.5), autoRow(2, tick: 0, fader, 0.5)])
        defer { withExtendedLifetime(m3) {} }
        #expect(s3.rows == [.velocity], "two targets: no guess")
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh test 2>&1 | grep -E "error:|Test run" | head -5
```

Expected: FAIL to compile (`preview` does not exist).

- [ ] **Step 3: The preview and the row**

Write `preview` and the `rows` rule in `StripChartModel.swift`, and the drawing and observation in `StripChartView.swift`, as the paragraph above Step 1 says.

- [ ] **Step 4: Run the tests to verify they pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh build 2>&1 | grep -E "error:|warning:" | grep -v MemoryDefaults | sort -u
scripts/swift.sh test 2>&1 | grep -E "Test run"
```

Expected: Swift 860 tests pass (the Strip Chart's existing tests unchanged: their lanes hold notes or no single target).

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add app/Sources/VizDAWAppModel/StripChartModel.swift app/Sources/VizDAWEditors/StripChartView.swift \
  app/Tests/VizDAWAppModelTests/StripChartModelTests.swift
git commit -m "app: the Strip Chart draws the write in progress; an automation lane opens on its target (5e-2 Task 6)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -3
```

### Task 7: The record UI's polish carries (5d-2): saving while recording, the popover's keys, the monitor tooltip, Settings › Audio's readout

**Files:**
- Modify: `app/Sources/VizDAWAppModel/DocumentModel.swift` (`SaveAction`, `saveAction(forceAs:)`; `wantsSavePanel` goes), `app/Sources/VizDAWAppModel/RecordOptions.swift` (`saveUntitledWhileRecording`), `app/Sources/VizDAWAppModel/RecordModel.swift` (`punchFocusChanged`; `showsPopover` ends text editing), `app/Sources/VizDAWAppModel/AudioPreferences.swift` (`inputHolder`), `app/Sources/VizDAWAppModel/LaneInputs.swift` (`refreshMonitoring`)
- Modify: `app/Sources/VizDAW/AppCommands.swift` (`save(as:)`), `app/Sources/VizDAW/RecordControls.swift` (the popover's focus), `app/Sources/VizDAW/AudioSettingsView.swift` (the readout's refresh, the holder, Software Monitoring at once), `app/Sources/VizDAWEditors/LaneHeaderView.swift` (the tooltip; N2)
- Test: `app/Tests/VizDAWAppModelTests/RecordOptionsTests.swift`, create `app/Tests/VizDAWAppModelTests/RecordUiCarryTests.swift`

**Interfaces:**
- Consumes: plan 5d-2's `RecordModel`, `AudioPreferences`, `LaneInputs`, `LaneHeaderView`, `EditorModel.beginTextEditing/endTextEditing`.
- Produces:
  ```swift
  public enum SaveAction: Equatable, Sendable { case panel, save, refused(String) }
  extension DocumentModel { public func saveAction(forceAs: Bool) -> SaveAction }   // reads the path once, refreshes the record state quietly, sets no status
  extension RecordModel {
      public static let saveUntitledWhileRecording = "Stop recording to save this new project: its take is being written into it."
      /// The popover's punch focus moved (Ruling 12): the old field's draft is committed; text editing is held while a field has focus.
      public func punchFocusChanged(from old: TransportField?, to new: TransportField?)
  }
  extension AudioPreferences { public static func inputHolder(_ sessions: [any SessionProtocol]) -> (any SessionProtocol)? }
  extension LaneInputs { public func refreshMonitoring() }   // pollMeters' first two lines
  extension LaneHeaderView { func toolTipText(at p: CGPoint) -> String? }   // the monitor button's `LaneInputs.help`
  ```

Rulings 11–14.

- [ ] **Step 1: Write the failing tests**

In `RecordOptionsTests.swift`, `saveWhileRecordingSaysWhenTheTakeIsSaved` becomes:

```swift
    @Test func saveWhileRecordingSaysWhenTheTakeIsSaved() throws {
        let (m, f) = try armed()
        f.savedPath = "/tmp/P.vizdaw"
        _ = m.record.toggle()
        f.savesAfterPassRow = true
        #expect(m.saveAction(forceAs: false) == .save)
        m.save()
        #expect(m.editor.status == RecordModel.savedDuringPass)
        #expect(m.alert == nil)
        // Save As cannot move a recording project: it waits for Stop.
        m.editor.clearStatus()
        #expect(m.saveAction(forceAs: true) == .refused(RecordModel.saveAsWhileRecording))
        #expect(m.editor.status == nil, "no side effect (T2 M2)")
        // Untitled: its own words (T2 M1).
        f.savedPath = nil
        #expect(m.saveAction(forceAs: false) == .refused(RecordModel.saveUntitledWhileRecording))
        _ = m.record.toggle()
        f.savesAfterPassRow = false
        #expect(m.saveAction(forceAs: false) == .panel)
        f.savedPath = "/tmp/P.vizdaw"
        #expect(m.saveAction(forceAs: true) == .panel)
        #expect(m.saveAction(forceAs: false) == .save)
    }

    /// 5d-2 T2 M2: a pass that ended by itself since the last poll does
    /// not refuse a Save As.
    @Test func saveActionReadsTheRecordingNow() throws {
        let (m, f) = try armed()
        f.savedPath = "/tmp/P.vizdaw"
        _ = m.record.toggle()
        f.recordRow.recording = false  // it ended at the song's end; no poll since
        #expect(m.saveAction(forceAs: true) == .panel)
    }

    /// 5d-2 T3 M1 (Ruling 12): text editing is held only while a punch
    /// field has focus, so R, Space and Return reach the window otherwise.
    @Test func textEditingIsHeldOnlyWhileAPunchFieldHasFocus() throws {
        let (m, _) = try demo()
        m.select(3)
        let r = m.record
        r.showsPopover = true
        #expect(!m.editor.textEditing)
        r.punchFocusChanged(from: nil, to: .punchIn)
        #expect(m.editor.textEditing)
        r.setPunchText(.punchIn, "3")
        r.punchFocusChanged(from: .punchIn, to: .punchOut)
        #expect(m.editor.textEditing && m.editor.layout.transport.punchIn == 122_880, "the old field's draft is in")
        r.punchFocusChanged(from: .punchOut, to: nil)
        #expect(!m.editor.textEditing)
        r.punchFocusChanged(from: nil, to: .punchOut)
        r.showsPopover = false
        #expect(!m.editor.textEditing, "closing ends it")
    }
```

Create `app/Tests/VizDAWAppModelTests/RecordUiCarryTests.swift`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Tests/VizDAWAppModelTests/RecordUiCarryTests.swift <<'VIZDAW_EOF'
import AppKit
import Testing
import VizDAWCore
@testable import VizDAWAppModel
@testable import VizDAWEditors

/// The 5d-2 record UI's carries (plan 5e-2 Tasks 7–8): Settings › Audio's
/// holder and Software Monitoring, the monitor button's tooltip, and the
/// test gaps the 5d-2 reviews parked.
@MainActor
@Suite(.serialized) struct RecordUiCarryTests {
    /// T7 Minor 3: the device is read from the first session whose input
    /// names one.
    @Test func theInputHolderIsTheFirstSessionWithADevice() throws {
        let quiet = FakeSession([])
        let holder = FakeSession([])
        holder.inputRow = InputInfoRow(
            device: "Fake", channels: 2, names: ["L", "R"], inputLatencyFrames: 0, outputLatencyFrames: 0)
        #expect(AudioPreferences.inputHolder([quiet, holder]) === holder)
        #expect(AudioPreferences.inputHolder([quiet]) == nil)
        #expect(AudioPreferences.inputHolder([]) == nil)
    }

    /// T7 Minor 4: Software Monitoring reaches the meters at once, not at
    /// the next poll.
    @Test func softwareMonitoringReachesTheMetersAtOnce() throws {
        let saved = LaneInputs.softwareMonitoring
        defer { LaneInputs.softwareMonitoring = saved }
        var on = true
        LaneInputs.softwareMonitoring = { on }
        let m = try DocumentModel(session: editorFake())
        #expect(m.inputs.monitoringOn)
        on = false
        m.inputs.refreshMonitoring()
        #expect(!m.inputs.monitoringOn)
    }

    /// T5 M3 (Ruling 14): the monitor button explains its mode.
    @Test func theMonitorButtonHasItsTooltip() throws {
        let f = editorFake()
        f.laneTable[3]?[1].monitor = .input
        let m = try DocumentModel(session: f)
        let h = LaneHeaderView(document: m)
        let r = try #require(LaneHeaderLayout.controls(rowTop: h.rowTop(1)).first { $0.0 == .monitor }?.1)
        #expect(h.toolTipText(at: CGPoint(x: r.midX, y: r.midY)) == LaneInputs.help(.input))
        let name = try #require(LaneHeaderLayout.controls(rowTop: h.rowTop(1)).first { $0.0 == .name }?.1)
        #expect(h.toolTipText(at: CGPoint(x: name.midX, y: name.midY)) == nil)
    }
}
VIZDAW_EOF
```

(`editorFake()`'s Song is sequence 3, shown; `FakeSession.inputRow` is 5d-2's.)

- [ ] **Step 2: Run the tests to verify they fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh test 2>&1 | grep -E "error:|Test run" | head -5
```

Expected: FAIL to compile (`saveAction`, `punchFocusChanged`, `inputHolder`, `refreshMonitoring`, `toolTipText` do not exist).

- [ ] **Step 3: The models**

Add the items of the Interfaces block. `saveAction`: `record.refresh(quiet: true)` first (a read), then one `needsSaveAs` read; forceAs while recording → `.refused(saveAsWhileRecording)`; untitled while recording → `.refused(saveUntitledWhileRecording)`; forceAs or untitled → `.panel`; else `.save`. `RecordModel.showsPopover`'s `didSet` also calls `document?.editor.endTextEditing("recordPopover")` when it closes. `refreshMonitoring` holds `pollMeters`' first two lines, which `pollMeters` now calls. Delete `wantsSavePanel`.

- [ ] **Step 4: The views**

In `app/Sources/VizDAW/AppCommands.swift`:

Find:
```swift
        if doc.wantsSavePanel(forceAs: forceAs) {
            if let path = Panels.chooseSaveLocation(title: doc.snapshot.title) {
                doc.saveAs(path: path)
            }
        } else if !(forceAs || doc.needsSaveAs) {
            doc.save()
        }
```
Replace:
```swift
        switch doc.saveAction(forceAs: forceAs) {
        case .panel:
            if let path = Panels.chooseSaveLocation(title: doc.snapshot.title) {
                doc.saveAs(path: path)
            }
        case .save:
            doc.save()
        case .refused(let why):
            doc.editor.status = why
        }
```

In `app/Sources/VizDAW/RecordControls.swift`:

Find:
```swift
        .onChange(of: focusedPunch) { old, _ in
            if let old { record.commitPunch(old) }
        }
        .onAppear { model.editor.beginTextEditing("recordPopover") }
        .onDisappear { model.editor.endTextEditing("recordPopover") }
```
Replace:
```swift
        .onChange(of: focusedPunch) { old, new in
            record.punchFocusChanged(from: old, to: new)
        }
        .onDisappear { model.editor.endTextEditing("recordPopover") }
```

In `app/Sources/VizDAW/AudioSettingsView.swift`: the Software Monitoring toggle's `set` becomes `{ prefs.setSoftwareMonitoring($0); for d in store.documents.values { d.inputs.refreshMonitoring() } }`; `.onAppear { readDevice() }` becomes `.task { while !Task.isCancelled { readDevice(); try? await Task.sleep(for: .seconds(1)) } }`; `readDevice` becomes `prefs.readDevice(AudioPreferences.inputHolder(store.documents.values.map(\.session)))`.

In `app/Sources/VizDAWEditors/LaneHeaderView.swift`: conform to `NSViewToolTipOwner`; after each reload (where the view's rows change, and in `layout()`), `removeAllToolTips()` and `addToolTip(_:owner:userData:)` for each row's monitor rect; `view(_:stringForToolTip:point:userData:)` returns `toolTipText(at:) ?? ""`. Move the two-line doc comment "The thin input meter and its clip mark (spec §6.9); the track is yellow while the input is still opening (pre-flight I5)." from above `oneLine` to above `meter(_:rowTop:inputs:)` (N2).

- [ ] **Step 5: Run the tests to verify they pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh build 2>&1 | grep -E "error:|warning:" | grep -v MemoryDefaults | sort -u
scripts/swift.sh test 2>&1 | grep -E "Test run"
```

Expected: Swift 865 tests pass.

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add app/Sources/VizDAWAppModel/DocumentModel.swift app/Sources/VizDAWAppModel/RecordOptions.swift app/Sources/VizDAWAppModel/RecordModel.swift \
  app/Sources/VizDAWAppModel/AudioPreferences.swift app/Sources/VizDAWAppModel/LaneInputs.swift app/Sources/VizDAW/AppCommands.swift \
  app/Sources/VizDAW/RecordControls.swift app/Sources/VizDAW/AudioSettingsView.swift app/Sources/VizDAWEditors/LaneHeaderView.swift \
  app/Tests/VizDAWAppModelTests/RecordOptionsTests.swift app/Tests/VizDAWAppModelTests/RecordUiCarryTests.swift
git commit -m "app: 5d-2 carries: save actions without side effects, the popover's keys, the monitor tooltip, Settings › Audio's live readout (5e-2 Task 7)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -3
```

### Task 8: The record UI's test gaps (5d-2 parked items)

**Files:**
- Modify: `app/Sources/VizDAWAppModel/GrowingTake.swift` (`GrowingTake.slice`), `app/Sources/VizDAWEditors/ArrangementView.swift` (uses it), `app/Sources/VizDAWEditors/LaneHeaderView.swift` and `app/Sources/VizDAWEditors/ArrangementLayout.swift` (`LaneHeaderLayout.showsMeter(laneHeight:)`)
- Test: `app/Tests/VizDAWAppModelTests/RecordUiCarryTests.swift`, `app/Tests/VizDAWAppModelTests/RecordOptionsTests.swift`, `app/Tests/VizDAWAppModelTests/MixerModelTests.swift`, `app/Tests/VizDAWAppModelTests/GrowingTakeTests.swift`

**Interfaces:**
- Consumes: plan 5d-2's `RecordModel.remainingText`, `LaneInputs.pollMeters`, `MixerModel.inputLane(for:)`, `ArrangementView.drawGrowingTakes`.
- Produces: `GrowingTake.slice(rect: CGRect, visible: CGRect, from: UInt64, to: UInt64) -> (from: UInt64, to: UInt64, columns: UInt32)?` (`nonisolated static`: the file frames under `visible ∩ rect` and the columns to ask for, at most 8192; nil when nothing is visible or the frames are empty), and `LaneHeaderLayout.showsMeter(laneHeight: CGFloat) -> Bool` (46 points or more).

The carry list's test-only items: T2 M3/M4 (an untitled save while recording is Task 7's test; here `save()` reaching the session, nil and 0 free bytes, mono, 16- and 32-bit depths), T4 M4 (opening cleared on disarm; the I3 restart is the core's, Ruling 15), T5 M5 (the under-46 hide, the monitor click through `mouseDown`, other heights), T6 Minor 3 (a lane on another path; a lane in a sequence that is not shown), T9 M3 (the visible-slice math). The two extractions move existing arithmetic unchanged into testable functions.

- [ ] **Step 1: Write the tests**

Append to `RecordOptionsTests`:

```swift
    /// 5d-2 T2 M4: the time left counts the depth and mono lanes, and
    /// says nothing without a reading.
    @Test func theTimeLeftCountsEveryDepthAndMonoLanes() throws {
        let (m, f) = try armed()
        let r = m.record
        defer { RecordModel.freeSpace = RecordModel.volumeFreeSpace }
        f.laneTable[3]?[0].input = .audio(first: 0, stereo: false, returnOf: nil)
        f.snap.version += 1
        m.refreshAfterJob()
        f.info.format = ProjectFormatRow(sampleRate: 44_100, depth: .int16)
        m.refreshProjectInfo()
        // 44.1 kHz, 16-bit, one channel: 88 200 bytes a second.
        RecordModel.freeSpace = { _ in 88_200 * 600 + RecordModel.stopBytes }
        #expect(r.remainingText == "10 min left")
        f.info.format = ProjectFormatRow(sampleRate: 48_000, depth: .float32)
        m.refreshProjectInfo()
        RecordModel.freeSpace = { _ in 192_000 * 3_600 + RecordModel.stopBytes }
        #expect(r.remainingText == "1 h left")
        RecordModel.freeSpace = { _ in nil }
        #expect(r.remainingText == nil, "no reading, no readout")
        RecordModel.freeSpace = { _ in 0 }
        #expect(r.remainingText == "Less than a minute left")
    }

    /// 5d-2 T2 M3: a titled Save while recording reaches the session.
    @Test func saveWhileRecordingReachesTheSession() throws {
        let (m, f) = try armed()
        f.savedPath = "/tmp/P.vizdaw"
        f.snap.dirty = true
        _ = m.record.toggle()
        m.save()
        #expect(!f.snap.dirty, "the session saved")
    }
```

Append to `RecordUiCarryTests`:

```swift
    /// 5d-2 T4 M4: disarming clears "opening" and its status line.
    @Test func openingIsClearedOnDisarm() throws {
        let f = editorFake()
        f.laneTable[3]?[1].arm = true
        f.laneTable[3]?[1].input = .audio(first: 0, stereo: false, returnOf: nil)
        f.inputRow = InputInfoRow(
            device: "Fake", channels: 2, names: ["L", "R"], inputLatencyFrames: 0, outputLatencyFrames: 0)
        f.inputRow.opening = true
        let m = try DocumentModel(session: f)
        m.inputs.pollMeters()
        #expect(m.inputs.opening && m.editor.status == LaneInputs.openingNote)
        f.laneTable[3]?[1].arm = false
        f.snap.version += 1
        m.refreshAfterJob()
        m.inputs.pollMeters()
        #expect(!m.inputs.opening && m.editor.status == nil)
    }

    /// 5d-2 T5 M5: the meter hides under 46 points; a click on the
    /// monitor button goes through `mouseDown` to one undo step.
    @Test func theMeterHidesUnder46PointsAndTheMonitorClickCycles() throws {
        #expect(!LaneHeaderLayout.showsMeter(laneHeight: 45))
        for h in [46.0, 47, 48, 96] { #expect(LaneHeaderLayout.showsMeter(laneHeight: h), "\(h)") }
        let f = editorFake()
        let m = try DocumentModel(session: f)
        m.select(3)
        let h = LaneHeaderView(document: m)
        let r = try #require(LaneHeaderLayout.controls(rowTop: h.rowTop(1)).first { $0.0 == .monitor }?.1)
        let at = h.convert(NSPoint(x: r.midX, y: r.midY), to: nil)
        let e = try #require(
            NSEvent.mouseEvent(
                with: .leftMouseDown, location: at, modifierFlags: [], timestamp: 0, windowNumber: 0,
                context: nil, eventNumber: 1, clickCount: 1, pressure: 1))
        h.mouseDown(with: e)
        #expect(f.edits.last == .lane(edit: .setMonitor(seq: 3, lane: 31, monitor: .input)))
    }
```

Append to `MixerModelTests`:

```swift
    /// 5d-2 T6 Minor 3: a strip's input lane plays through its own path,
    /// in the shown sequence only.
    @Test func anAudioPathStripIgnoresOtherPathsAndOtherSequences() throws {
        let f = mixerFake()
        f.mixerSnap.strips.insert(stripRow(4, "Vocal", kind: .audioPath(path: 7)), at: 0)
        var other = lane(30, "Elsewhere")
        other.audioOut = 8
        f.laneTable[3] = [other]
        var hidden = lane(40, "Hidden")
        hidden.audioOut = 7
        f.laneTable[4] = [hidden]
        let m = try model(f)
        m.document.select(3)
        let strip = try #require(m.strips.first)
        #expect(m.inputLane(for: strip) == nil, "another path, and another sequence's lane")
    }
```

Append to `GrowingTakeTests`:

```swift
    /// 5d-2 T9 M3: only the visible slice's frames are asked for, at most
    /// 8192 columns.
    @Test func theVisibleSliceAsksForItsFramesAndAtMost8192Columns() {
        let rect = CGRect(x: 0, y: 0, width: 1000, height: 40)
        let all = GrowingTake.slice(rect: rect, visible: rect, from: 0, to: 48_000)
        #expect(all?.from == 0 && all?.to == 48_000 && all?.columns == 1000)
        let half = GrowingTake.slice(rect: rect, visible: CGRect(x: 500, y: 0, width: 600, height: 40), from: 0, to: 48_000)
        #expect(half?.from == 24_000 && half?.to == 48_000 && half?.columns == 500)
        let wide = CGRect(x: 0, y: 0, width: 20_000, height: 40)
        #expect(GrowingTake.slice(rect: wide, visible: wide, from: 0, to: 960_000)?.columns == 8192)
        #expect(GrowingTake.slice(rect: rect, visible: CGRect(x: 2000, y: 0, width: 10, height: 40), from: 0, to: 48_000) == nil)
        #expect(GrowingTake.slice(rect: rect, visible: rect, from: 5, to: 5) == nil)
    }
```

(`f.edits` is `FakeSession`'s record of editor edits, as `LaneHeaderInputTests` reads it; Song's lane 31 starts on Auto, so one click asks for Input.)

- [ ] **Step 2: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh test 2>&1 | grep -E "error:|Test run" | head -5
```

Expected: FAIL to compile (`GrowingTake.slice` and `LaneHeaderLayout.showsMeter` do not exist); the other new tests compile once they do.

- [ ] **Step 3: The two extractions**

Move `drawGrowingTakes`' slice arithmetic (from its `let vis = …` to `let columns = …`, `ArrangementView.swift:173–184` at `1f6057a`; `drawRegions` has another `let vis`, not this one) into `GrowingTake.slice` and call it there; move the `laneHeight >= 46` check in `LaneHeaderView.draw` into `LaneHeaderLayout.showsMeter(laneHeight:)` (in `ArrangementLayout.swift`, beside `meterRect`) and call it.

- [ ] **Step 4: Run the tests to verify they pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh build 2>&1 | grep -E "error:|warning:" | grep -v MemoryDefaults | sort -u
scripts/swift.sh test 2>&1 | grep -E "Test run"
```

Expected: Swift 871 tests pass. A test that finds a real fault (not a test slip) is reported, and fixed in this task only if the fix is local to the code under test.

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add app/Sources/VizDAWAppModel/GrowingTake.swift app/Sources/VizDAWEditors/ArrangementView.swift \
  app/Sources/VizDAWEditors/LaneHeaderView.swift app/Sources/VizDAWEditors/ArrangementLayout.swift \
  app/Tests/VizDAWAppModelTests/RecordUiCarryTests.swift app/Tests/VizDAWAppModelTests/RecordOptionsTests.swift \
  app/Tests/VizDAWAppModelTests/MixerModelTests.swift app/Tests/VizDAWAppModelTests/GrowingTakeTests.swift
git commit -m "app: the 5d-2 record UI's parked test gaps; the visible slice and the meter threshold made testable (5e-2 Task 8)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -3
```

### Task 9: The README: automation by eye; the full gates

**Files:**
- Modify: `app/README.md` (the "Automation recording, the UI (plan 5e-2)" section, steps 165–171)

**Interfaces:**
- Consumes: Tasks 1–8 and plan 5e-1.
- Produces: the owner's by-eye checklist.

- [ ] **Step 1: The checks**

Append to `app/README.md`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat >> app/README.md <<'VIZDAW_EOF'

### Automation recording, the UI (plan 5e-2)

By eye; the models are tested (`MixerGestureTests`,
`AutomationPreferencesTests`, `AutomationCommandsTests`,
`StripChartModelTests`, `RecordUiCarryTests`) and the e2e
`touch_writes_a_fader_over_the_channel` drives the same core through the
channel.

165. Mixer: each strip shows its automation mode above the fader ("Read",
     green); the popup lists Off, Read, Touch, Latch, Write, Trim, each in
     its colour (gray, green, yellow, orange, red, purple). Choose Touch:
     one undo step "Set Automation Mode". Click a strip's name: it is
     selected (tinted); ⌘-click adds another. The name's menu has "Set All
     Strips to ▸ …": every strip changes in one undo step. While you drag
     a fader, pan or send level, or hold a mute, a ring shows around it; a
     touch from `vizctl` (`automation.gesture`) or a plugin window rings
     too. Stopped, a fader drag is one "Set Fader" step, a mute click one
     "Mute" step, as before.
166. Record popover keys (5d-2 T3 M1): open the record popover without
     clicking a field: R records and Space plays (if the popover passes
     keys on; note it if it does not). Click Punch In: typing R or Space
     types into the field; Tab to Punch Out, then click outside the field:
     the keys work again.
167. Writing from the mixer: on Touch, play and move the Built-in Synth
     fader: you hear it at once; let go: it glides back over a quarter of
     a second. Stop: "Undo Write Automation"; the lane "Built-in Synth
     Fader" appears at the end of the sequence and the Strip Chart opens on
     its fader row. While playing, the Strip Chart on that lane draws the
     move in red as you make it. Play again and grab the fader while its
     new automation moves it: the sound does not jump (the write starts at
     the heard value); the fader's knob shows the static value until you
     move it (showing the heard value is a backlog row). Latch: the fader holds where you let go
     until Stop. Write: play without touching: at Stop the strip's fader,
     pan and mute lanes hold their values from the play point, and the
     strip's popup reads Latch (Settings › Automation's After Write). Trim
     on a strip with a fader ramp: moving the fader up 3 dB raises the
     whole ramp under it by 3 dB, keeping its shape.
168. Automation menu: with two strips selected, Automation › Selected
     Strips › Write sets both (one step); with none selected it is dimmed.
     Suspend Automation (checked) makes every strip play as Read: Off
     strips play their automation again and nothing is written; uncheck it.
169. Settings › Automation: After Write (Off, Read, Touch, Latch, Trim),
     Return time (0–2000 ms; at 0 the fader jumps back), Thinning (High
     writes fewer points: compare a lane's points in the List editor), and
     "Write plugin parameters only when touched", on and dimmed. Quit and
     relaunch: kept.
170. Plugin windows: a plugin without its own editor shows sliders; on
     Touch, playing, dragging one writes a lane "Built-in Synth <parameter>"
     (no "Change Plugin Parameter" step); stopped, a drag is one "Change
     Plugin Parameter" step.
171. Settings › Audio: with Settings open, arm an audio lane: within a
     second the round-trip readout shows the device's latencies (5d-2 T7
     Minor 2). Turn Software Monitoring off: an Input-monitored lane's
     meter goes at once. Hover the lane header's monitor button (A, I or
     –): its tooltip explains the mode. ⌘S in an untitled project while
     recording says "Stop recording to save this new project: its take is
     being written into it."
VIZDAW_EOF
```

- [ ] **Step 2: The full gates and the e2e**

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

Expected: everything clean (the Linux clippy too); "workspace done" with no failure lines (Rust about 1522 passed, 14 ignored: plan 5e-1's and this plan's registry test); Swift "Test run with 871 tests … passed"; the 10 e2e tests pass; `pgrep` prints nothing; no new `/tmp/vze*` home.

- [ ] **Step 3: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add app/README.md
git commit -m "docs: README checks for the automation UI and the 5d-2 carries (5e-2 Task 9)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -3
```

## Self-Review

- **Spec coverage.** §7.1's on-screen mixer (Tasks 2–3) and the plugin window's own controls (Task 5; editors' gestures are plan 5e-1 Task 9's). §7.4's live preview and the new lane's row (Task 6). §7.5: popups, colours, rings (Tasks 2–3), "Set all strips to…" (Task 3), Settings › Automation (Task 1), `automation.mode.<mode>` and `automation.suspend` as commands (Task 4). The 5d-2 carries that belong to the record UI (Tasks 7–8; "Carried items").
- **Placeholders.** None: every step names its file, its signatures and its test.
- **Plan 5e-1's revision.** Checked against its interface as revised: `GesturePhaseRow.touch(value: nil)` from the mixer (Task 2, Ruling 1), clicks and the plugin window's first move with explicit values; concurrent static touches sharing one edit (Review Focus 1); the core sending a UI value of a plugin parameter to the plugin (Task 5, Ruling 8); `touched` as active touches only (Ruling 3); settings read at pass start (Review Focus 3); one top-level menu left in `AppCommands` (Task 4). No other 5e-1 name this plan uses changed.
- **Type consistency.** `AutomationModes` (Task 2) is what Tasks 1, 3 and 4 show; `MixerModel`'s gesture API (Task 2) is what Task 3's views call; `DocumentModel.selectedStrips` (Task 2) is what Task 4's commands read; `SaveAction` (Task 7) is what `AppCommands.save(as:)` switches on; plan 5e-1's `FakeSession` fields (`gestures`, `gestureError`, `automationRow`, `modeCalls`, `previewRows`) are what Tasks 2, 4, 5 and 6 read.
- **Proportion.** Tests are whole; views are described by what they bind to (no view test can drive them); the extractions of Task 8 move code unchanged.
- **Review Focus.** Each line names its tests: Task 2 (1, 2), Task 1 (3), Tasks 2 and 4 (4), Task 6 (5).
