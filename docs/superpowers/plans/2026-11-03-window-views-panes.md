# VizDAW 7a — Window, Main Views and Panes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Each document gets a single window: a transport bar along the top and four regions in layout A (left, centre, bottom, right). The centre shows one of two main views:
- **Arrange:** the arrangement;
- **Mix:** the full mixer.

Each pane has a view picker limited to its own list, and pane toggles and keys show and hide the panes. Each main view's pane setup is saved with the document. The left pane shows the selected lane's channel strip. The mixer appears in the bottom pane, in the Mix view and in its own window, which has its own transport bar. The control channel reads and sets all of this through `state.get layout` and new `ui.*` commands.

**Architecture:**
- **`VizDAWAppModel`** (unit-tested) holds all layout and selection logic:
  - `WindowLayout.swift`: the per-view pane setups saved in `EditorLayout.views` (`ui.json`), with tolerant decoding;
  - `PaneGeometry.swift`: layout A's frames, clamping and the saved window frame;
  - `PaneContent.swift`: the left pane's strip rule (`StripFocus`), the inspector's rows, the markers list, the plug-in browser's insert and the controllers chart;
  - `ViewCommands.swift`: the `ui.*` commands for menus, keys and the channel;
  - `MixerModel.tick`: one poll per frame, however many mixers are on screen;
  - `MixerModel`'s placements: each popover, rename field and maps sheet shows in the one placement that opened it, and open touches end only when the last placement goes.
- **`core-control` / `core-ffi`:** six registry commands (147 → 153), the `layout` state target, and `ControlResult::Layout` with its JSON. No new FFI function is exported.
- **`VizDAW`** (views, checked by eye):
  - `DocumentShell` replaces the `NavigationSplitView`;
  - `DocumentTransportBar` replaces the toolbar;
  - `PaneFrame` gives each pane its picker;
  - `MixerView` carries its own `MixerBar` instead of a toolbar, so it works in a pane, in the Mix view and in its window.

**Tech Stack:** Rust 2024 (`core-control`, `core-ffi` with UniFFI 0.32.2, `vizctl`), Swift 6 / SwiftPM (Swift Testing; SwiftUI and AppKit in the `VizDAW` target only), macOS 14+.

**Spec:** `docs/superpowers/specs/2026-11-03-ui-polish-design.md`, §7 row 7a. This plan delivers:
- §2 items 1–4 in full;
- §2 item 8 (strips and lanes) for the left pane;
- §2 item 11's 7a part (the root README);
- §4's "breaking agents and tests" anti-goal for windows and panes;
- §5's key constraint;
- §6 items 1 and 3.

Every "(owner)" and "(controller's call)" in the spec binds. 7b (themes, icons, strip redesign), 7c (audio editor) and 7d (clean-up) are out of scope. Today's strip component and look stay.

## Global Constraints

- **Shell setup.**
  - Every bash block starts with `export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools` and `cd /Users/lattawb/workspace/vizdaw`.
  - A `git commit` without `DEVELOPER_DIR` fails silently: check every commit with `git log -1 --oneline` and `git show --stat HEAD`.
- **Swift only through the scripts, from the repo root** (`scripts/build-ffi.sh`, `scripts/swift.sh build`, `scripts/swift.sh test`, `scripts/make-app.sh`). Never call `swift`, `xcodebuild` or `xcrun` directly.
- **Branch and commits.**
  - Work on branch `sp7-ui` at or after `38c231d`. Never switch branches, never push.
  - Commit at the end of every task, adding only the files the task lists, by path (never `git add -A`). No manifest changes: `Cargo.lock`, every `Cargo.toml` and `Package.swift` stay as they are.
  - Never commit `.superpowers/`, `.DS_Store` or generated files (`app/Sources/VizDAWCore/vizdaw_ffi.swift` and the FFI headers).
  - End every commit message with these two lines (use the `Co-Authored-By` line naming the model you actually are):
    ```
    Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
    Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P
    ```
- **Clean room.**
  - Never read `~/workspace/SVP/research` or any Studio Vision material.
  - No Logic, Apple, Midas, Soundcraft, Steinberg or Opcode names or artwork in the UI. Main views are "Arrange" and "Mix", and panes are "Left Pane", "Bottom Pane" and "Right Pane".
  - SF Symbols already in use stay. New symbols are generic (`sidebar.left`, `rectangle.bottomthird.inset.filled`, `sidebar.right`).
- **No GUI tools, no real devices.**
  - Never use GUI, computer-use or accessibility-automation tools. The owner checks the views by eye from README steps 199–206 (Task 9).
  - Tests never open real audio or MIDI and never trigger the microphone prompt:
    - Swift tests use `FakeSession`;
    - every `core-ffi` test uses its `serial()` helper;
    - the e2e runs with `VIZDAW_AUDIO=offline`, as `App::start` sets it.
  - Afterwards `pgrep -lf MacOS/VizDAW` prints nothing and `ls -d /tmp/vze*` prints nothing new.
  - **Never touch the real `~/Library/Application Support/VizDAW`, the real preferences or the real Trash.**
    - Swift tests use `MemoryDefaults()` and `scripts/swift.sh test`'s own `VIZDAW_TRASH_DIR` and `VIZDAW_SCRATCH_DIR`.
    - The e2e saves only inside its throwaway home.
- **Programmatic and channel calls never open dialogs.** No `ui.*` command opens a sheet, panel or alert. A refusal is `invalid_params` or `unavailable`, and a key's refusal shows in the status line. Showing the media pool or the plug-in browser in a pane opens nothing. The media pool's Rename, Relink and Delete still ask only from their buttons, as today.
- **New commands get hostile-input tests:**
  - `ui_commands_check_their_params` (core-control);
  - `uiCommandsCheckTheirArguments` (app);
  - the e2e's hostile calls (Task 9).
- **FFI.** No new exported function. The new UniFFI types are the record `LayoutStateRow` (with `ViewPanesRow` and `PaneRow`), the enum case `StateTargetRow::Layout` and the enum case `ControlResult::Layout`. Should a task find it needs an export after all, it goes inside `ffi_guard` (which is `catch_unwind`) and returns `Result<_, VizError>`.
- **Swift 6.1 (CI, macos-15) as well as the local 6.4.**
  - Inside a `MainActor.assumeIsolated` closure, use a strong `let` captured before it, never a weak `self` box. This is the pattern of commit `8678649`, and applies to `WindowFrameKeeper`'s notification observers in Task 7.
  - Use no API newer than macOS 14.
  - No `@State`, `@GestureState` or `@Entry` (the Command Line Tools cannot expand them): spell out `State(initialValue:)` as `MixerWindow` does, and declare environment values with an `EnvironmentKey` struct as `OpenPluginWindowKey` does.
  - `AppCommands.body` already has ten top-level items, the most an older SDK's builder takes. New menu items go inside its existing `Group { … }`, never as an eleventh item (Ruling 4).
- **Lints** as in every plan: `#![deny(missing_docs)]`, plus no unwrap, expect, indexing or panic outside tests in Rust.
- **TDD.** Every new test is run and seen to fail (RED) before the code that makes it pass.
- **File compatibility.**
  - `core_model::file::VERSION` is unchanged; the layout is UI state in `ui.json`.
  - `EditorLayout.version` stays 1: the new `views` field decodes tolerantly, so a missing or malformed field gives the default (Ruling 1).
- **Each task ends green.**
  - Every task runs `cargo fmt --all -- --check`.
  - Rust tasks (5, 9) also run both clippies, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo clippy --workspace --all-targets --target x86_64-unknown-linux-gnu --target-dir target/linux-check -- -D warnings`, and `cargo test --workspace`.
  - Every task that touches Swift runs `scripts/build-ffi.sh`, `scripts/swift.sh build` and `scripts/swift.sh test`.
  - Task 9 runs everything, plus `scripts/make-app.sh` and the e2e: `VIZDAW_E2E_APP=dist/VizDAW.app cargo test -p vizctl --test e2e -- --ignored --test-threads 1`.
- **Baselines and expected counts.** Measured at plan 5f-4's end (its `progress.md`; `7bbaec7`, `82e8746` and `38c231d` added no tests): **Rust 1993 passed / 18 ignored (macOS), Swift 993, e2e 14, registry 147, app README last step 198.**

  | Task | Rust (macOS and Linux) | Swift | Other |
  |---|---|---|---|
  | 1 per-view layouts | — | +10 | |
  | 2 geometry | — | +6 | |
  | 3 left pane, mixer tick, placements | — | +11 | |
  | 4 right pane | — | +7 | |
  | 5 registry, `layout` target | +3 | +0 (compiles) | registry 153 |
  | 6 `ViewCommands` | — | +7 | |
  | 7 shell, transport bar, View menu | — | +0 | |
  | 8 pane contents | — | +0 | |
  | 9 e2e, READMEs | ignored +1 | — | e2e 15; README 199–206 and earlier steps corrected |
  | **After** | **1996 / 19 ignored** | **1034** | |

  Running Swift totals: 1003 (Task 1), 1009 (2), 1020 (3), 1027 (4 and 5), 1034 (6 to 9).

  Report the measured counts at every task's end. A difference is explained, never hidden.
- **How code is given.** Signatures, test names and assertions, not bodies. Every name marked "today" is the tree's exact name at `38c231d`. Where the tree differs, use the tree's name and say so in the report.

## Review Focus

These are the five failure modes a user is most likely to hit that the spec implies but no task's own feature tests exercise, most likely first. Each is pinned by the named test in its task.

1. **A hand-edited, older or corrupt `ui.json`.** Possible faults:
   - no `views` field;
   - an unknown view name;
   - a bottom view in the left pane;
   - the mixer below in the Mix view;
   - a size that is NaN, negative or 1e9;
   - an unknown main view;
   - a window frame off every screen.

   Expected: the document opens. Each bad field is replaced by its default and nothing else changes; a size is clamped; a frame that is off-screen or malformed is ignored. Pinned by `aCorruptViewsFieldDecodesToDefaults` and `anOlderLayoutWithoutViewsDecodes` (Task 1), and `aSavedFrameOffEveryScreenIsIgnored` (Task 2).
2. **A small window or a laptop screen.**
   - The window shrinks to 700×450 with every pane shown, or a divider is dragged past the window's edge.
   - Expected:
     - panes shrink towards their minimum first (right, then bottom, then left, each only as far as needed), so the centre keeps at least 240×160 and the bottom pane at least `bottomMinimumWidth` (240) beside the right pane;
     - if that still does not fit, the right pane hides first, then the left, then the bottom. Their `shown` flags stay saved, so they come back when the window grows;
     - a dragged size is clamped to the pane's range, and a drag does not run away (Ruling 2's `dragStart`).
   - Pinned by `aSmallWindowShrinksPanesBeforeTheCentre` and `aDividerDragIsClamped` (Task 2).
3. **The mixer on screen twice.** The bottom pane's mixer is shown with the mixer window open, or the Mix view with the left strip.
   - Expected:
     - meters decay at the normal rate, not twice as fast;
     - one poll per frame;
     - the MIDI Learn Maps sheet, the insert and Instruments popovers and a strip's rename field open in one place only, the place whose button asked for them;
     - hiding one placement (the bottom pane) does not end a Touch ride in another (the mixer window).
   - Pinned by `twoMixerViewsPollOncePerFrame`, `learnMapsShowInOnePlacement` and `popoversAndRenameShowInOnePlacement` (Task 3).
4. **Selections the strip rule must handle.**
   - The cases: no lane selected; an external-MIDI lane; two lanes on one output path; two lanes on different paths; a lane in another sequence; a lane deleted while it is shown.
   - Expected:
     - no lane selected: the first record-armed lane's strip shows, else the placeholder "Select a lane to see its channel strip.";
     - an external-MIDI lane: the inspector shows;
     - two lanes on one path: that one strip;
     - two lanes on different paths: the first selected lane's strip;
     - a lane in another sequence is ignored;
     - a deleted lane falls back to the placeholder without a crash.
   - Pinned by Task 3's seven `StripFocus` tests. `noSelectionShowsTheArmedLaneElseNothing` also pins the lane in another sequence and the deleted lane.
5. **Hostile or careless `ui.*` calls.** Examples:
   - `ui.pane` with no field, an unknown pane, a view off that pane's list (an editor kind in the right pane too), the mixer below in the Mix view;
   - `size` as text or as 1e12;
   - `main` as `edit`;
   - extra parameters;
   - any call with no document open.

   Expected: `invalid_params` (types and ranges in core-control, lists in the app) or `no_document`, and the layout is unchanged: a refused call opens no detail and changes no `lastKind`. No dialog opens: `state.get windows` lists the same windows and no alert afterwards, and `state.get document`'s `alert` is null. Pinned by `ui_commands_check_their_params` (Task 5), `uiCommandsCheckTheirArguments` (Task 6), and the e2e's hostile block (Task 9).

## Plan Rulings

Each ruling decides something the spec leaves to the builder, in the format "what — why — cost if wrong". Spec "(controller's call)" items are restated only where this plan makes them concrete.

1. **Ruling (where the layout lives): `EditorLayout.views: ViewLayouts`, saved in `ui.json` with the rest of the editors' UI state.**
   - `ViewLayouts` holds:
     - `main: MainView` (`.arrange` or `.mix`);
     - `arrange: PaneSetup`;
     - `mix: PaneSetup`;
     - `frame: [Double]?`, the document window's `[x, y, width, height]` (Ruling 11).
   - `PaneSetup` holds `left`, `bottom` and `right`, each a `PaneState { shown: Bool, view: PaneView, size: Double }`.
   - Decoding is tolerant, field by field, using the existing `KeyedDecodingContainer.field`. A `PaneState` cannot know its side or main view, so it is not decoded on its own:
     - its coding is a private `PanePatch { shown: Bool?; view: PaneView?; size: Double? }`, each field decoded through `field(…, nil)`. An unknown `view` raw value decodes as nil;
     - `PaneSetup.init(from:)` and `ViewLayouts.init(from:)` merge each side's patch onto that side's default, `ViewLayouts().arrange[side]` or `ViewLayouts().mix[side]`. A missing or undecodable pane is that default whole.
   - Then `ViewLayouts.sanitized()` repairs:
     - a view not on its side's list for that main view, which becomes that side's default;
     - a size that is not finite, which becomes the default, or is out of range, which is clamped (−5 becomes 160, not the default). `EditorLayout.sane` is not reused, because it turns an out-of-range value into the default.
   - Defaults:
     - Arrange: left shown, `.strip`, 220; bottom shown, `.editor`, 260; right hidden, `.controllers`, 260.
     - Mix: left hidden, `.strip`, 220; bottom hidden, `.editor`, 260; right hidden, `.plugins`, 260.
   - Why: spec §6.3 ("saved per document, in the project's UI state with the window size") and §6.1 ("a new document's Arrange view opens with the channel strip there"). Hiding the right pane by default keeps a laptop screen usable.
   - Cost if wrong: the defaults change in one place, `ViewLayouts.init()`.
2. **Ruling (layout A's geometry): `PaneGeometry.frames(in: CGSize, setup: PaneSetup) -> PaneFrames`.**
   - The left pane runs the full height under the transport bar. The centre is above the bottom row, and the bottom row holds the bottom pane with the right pane beside it.
   - If the right pane is shown and the bottom pane is hidden, the right pane runs beside the centre for the column's full height. Its size is its width in both cases, so a saved size means the same thing. There is no L-shaped centre.
   - Size ranges:
     - left width 160…480;
     - bottom height 120…900;
     - right width 180…520.
   - The centre minimum is 240×160 (`PaneGeometry.centreMinimum`). The bottom pane is at least `PaneGeometry.bottomMinimumWidth` (240) wide: without it a 520-point right pane leaves the bottom pane a negative width.
   - The rule, in order:
     1. clamp each size to its range;
     2. shrink right, then bottom, then left towards their minimums, each only as far as needed (given the current sizes of the others), so that the centre gets 240×160 and the bottom row gets `bottomMinimumWidth + 1 + right` (when the bottom and right panes are both shown);
     3. if it still does not fit, hide right and redo step 2 from the saved sizes; then left; then bottom.

     Hiding is for that frame only (`PaneFrames.hidden`); the saved `shown` flags are not changed. In layout A the right pane sits under the centre, so it costs the bottom row's width, not the centre's.
   - The 1-point dividers are draggable (`PaneGeometry.drag(_:side:by:in:) -> Double`, clamped). `delta` signs: left grows with a positive `delta` (rightwards), right with a negative one (leftwards), bottom with a **positive** one. The bottom divider passes `-translation.height`, because dragging up is negative in SwiftUI.
   - A divider keeps `dragStart: Double?` in a spelled-out `State` (Global Constraints: no `@State`). `onChanged` sets the size to `PaneGeometry.drag(setup with that side's size = dragStart, …, by: translation)`, because `DragGesture`'s `translation` is cumulative from the drag's start. While dragging, the divider shows a view-local live size and calls `setPane` once, in `onEnded`, so `updateLayout` (which bumps `drawVersion` and redraws every editor) does not run at pointer rate.
   - Why: spec §2 item 1 and the chosen mockup A ("the editor and the controller pane share the bottom right"). SwiftUI's `HSplitView` cannot report or restore sizes.
   - Cost if wrong: a reviewer who wants the right pane under the centre only can change one branch of `frames`.
3. **Ruling (the bottom pane's list): its views are `.editor` and `.mixer`.**
   - `.editor` shows today's detail editor (`layout.detail`) in its kind. That kind is shared by both main views because `layout.detail` is.
   - The picker (`PaneFrame` for `.bottom`) lists the four `DetailEditorKind.allCases` titles (Piano Roll, List, Strip Chart, Drum Grid), then Mixer (Arrange only). When the view is `.editor`, the selection is the kind (`layout.detail?.kind ?? layout.lastKind`). Choosing a kind sets the view to `.editor` and opens the detail in that kind (`openDetail`, or `showDetail` on the current target).
   - `DetailPane` gains `showsKindPicker: Bool`: false in the bottom pane, true in torn windows. So the bottom pane shows one kind picker, not two.
   - The Mix view's bottom list leaves out Mixer: the Mix view is the mixer.
   - ⌘1–⌘4, a double-clicked block and every `openDetail` show the bottom pane with `.editor`: the current view's, or the view `openDetail(_:kind:in:)` names. `editors.close_detail` leaves the pane shown with today's "No Editor Open" placeholder.
   - The bottom pane's `DetailArea` stays mounted while the pane is hidden (zero size, hidden from accessibility), so the editor keeps its scroll and zoom (spec §2 item 2: "switching views and back restores").
   - **The audio editor is left out of the bottom list until 7c.** 7a adds no placeholder entry: an entry that does nothing would hide nothing and help no one.
   - Why: spec §2 item 3's bottom list ("automation (the Strip Chart)") and §3 ("the existing editors move into panes unchanged").
   - Cost if wrong: 7c adds one picker entry and one `PaneView` case.
4. **Ruling (keys and menus).**
   - New keys:
     - Arrange View `⌃1` and Mix View `⌃2` (spec §5's controller's call);
     - Left Pane `⌃⌘←`, Bottom Pane `⌃⌘↓` and Right Pane `⌃⌘→`. None of these collides with the registry's keys at `38c231d`.
   - ⌘1–⌘4 stay on the editors and ⌥⌘1–4 stay on tear-off.
   - Settings › Keys lists the five under "View", as it lists every registry command by its menu.
   - The items go into the system View menu as `CommandGroup(before: .toolbar)`, inside `AppCommands`' existing `Group { … }` (Global Constraints: ten-item builder limit):
     - "Arrange View" and "Mix View" as check-marked toggles;
     - a divider;
     - "Left Pane", "Bottom Pane" and "Right Pane" as toggles.
   - A check-marked view item chosen again passes `on: false`. For `ui.view_*`, `ViewCommands.run` therefore ignores `on` and always shows that view.
   - With Mission Control's "Switch to Desktop" shortcuts turned on, macOS takes ⌃1/⌃2 first. README step 200 says to rebind them in Settings › Keys.
   - Commands win over keyboard slots (`KeyRouter`), so a saved project with a sequence slot on ⌃1, ⌃2 or ⌃⌘←/↓/→ loses that slot silently. Task 9 adds a backlog row and one clause in README 200.
   - Why: spec §5.
   - Cost if wrong: a rebinding, or a default changed in `registry.rs`.
5. **Ruling (the left pane's strip): `StripFocus.strip(selected:armed:destinations:strips:) -> StripFocus.Result`.**
   - A lane's strip:
     - `audioOut == p`: the strip with `kind == .audioPath(path: p)`;
     - else `midiDest == d`, where `d` is a plugin destination (`DestRow.plugin != nil`): `.instrument(dest: d)`;
     - a non-external destination without a plugin (the built-in synth): `.builtinSynth`;
     - an external destination, or no destination: no strip.
   - The lanes considered are the selection's lanes in the shown sequence, in selection order. If there are none, the first record-armed lane of the shown sequence is used.
   - Result:
     - `.strip(id)`, the first considered lane's strip. Two lanes on one path give that one strip; lanes on different paths give the first lane's strip (controller's call);
     - `.inspector`, when the first considered lane has no strip (spec §2 item 8);
     - `.nothing`, when no lane is considered.
   - Arm, input and monitor follow plan 5d-2 Ruling 4 through today's `StripView`, which reads `MixerModel.inputLane(for:)`.
   - Why: spec §2 item 8.
   - Cost if wrong: one function.
6. **Ruling (the inspector): read-only in 7a.**
   - `Inspector.rows(lane:region:destinations:strips:) -> [InspectorRow]` (`label`, `value`) describes:
     - the first considered lane: Name, Plays (destination name or "Audio: ‹strip name›"), Input, Monitor, Takes, Armed;
     - a single selected region: Region (its media name), Gain (`Formatting` as today), Fade In, Fade Out.
   - With nothing selected it shows "Select a lane or a region."
   - Edits stay in the lane header, the mixer and the Region menu.
   - Why: spec §2 item 3's "(controller's call)", and §3's "no new … editing features".
   - Cost if wrong: editable fields are a later plan.
7. **Ruling (the right pane's views).**
   - **Controllers:** its own `StripChartModel`, `DocumentModel.controllersChart`, made once and not in the `DetailSlot` maps, so the four slot switches stay unchanged.
     - Its `read` gives `DetailState(kind: .stripChart, target: .lane(selected lane), stripRows: layout.controllers[laneKey])`, or nil when no lane is selected.
     - Its `write` stores `stripRows` in `EditorLayout.controllers: [String: [StripLane]]`, keyed `"seq:lane"`.
     - The pane shows "Select a lane to see its controllers." when there is no target.
   - **Media pool:** today's `MediaPoolView` with a new `compact: Bool`. Compact shows the Name, Duration and Offline columns, and puts the action buttons in one "Actions" menu. `minWidth` 620 and `minHeight` 280 apply only when not compact: compact drops both, because beside the 260-point bottom pane a 280-point minimum overflows.
   - **Plug-ins:** today's `PluginBrowserView` with a new `width: CGFloat?` (nil fills the pane; the popover passes 320), listing effects.
     - `PluginsPane` keeps its own `PluginBrowserModel`, never `mixer.browserModel(.effect)`, whose cache is cleared each time `browser` becomes nil (`MixerModel.swift:100`), which `pick` does on every insert.
     - A pick calls `PaneContent.insertPlugin(_:document:mixer:) -> String?`. Its strip is the single selected mixer strip when `document.selectedStrips.count == 1` and that strip is not master; else the left-pane rule's strip. The Mix view has no arrangement, so its lane selection cannot change there, but strips can be selected in its mixer. It adds the plug-in as that strip's last insert, through `mixer.browser = .insert(strip:)` and `mixer.pick(_:)`.
     - With no strip it returns "Select a lane or one mixer strip first." (`PaneContent.noStripMessage`), which goes to the status line.
   - **Markers:** `MarkerList.rows(info:marks:) -> [MarkerListRow]`. It lists the shown sequence's markers (`SequenceInfo.markers`) and the set locators (`TransportMarks.locators`, named "Locator ‹n›"), merged by tick, each with its bar.beat position (`MeterMath`).
     - A click calls `DocumentModel.moveTo(_:)`. A failure goes to the status line, as `transport.next_marker`'s does.
   - Why: spec §2 item 3's right list and §2 item 4.
   - Cost if wrong: each is one view.
8. **Ruling (one mixer, three placements).**
   - `MixerView(model:placement:)`, where `placement` is the new `MixerPlacement`: `.window`, `.pane`, `.view` or `.strip` (the left pane's single strip).
   - Today's `.toolbar` items move into a `MixerBar` row at the top of `MixerView`: the load text, Add Bus and the Instruments menu with its popover. A toolbar item in a pane would land in the document window's toolbar, which no longer exists.
   - Polling goes through `MixerModel.tick(now:) -> Bool`, which does `refreshIfNeeded`, `pollLevels` and `pollAutomation` at most once per `MixerModel.tickInterval` (0.030 s). Every placement's `.task` calls it every 33 ms, so meter decay is once per frame.
   - The MIDI Learn Maps sheet:
     - The new `LearnModel.mapsPlacement: MixerPlacement` is set by the button that opens it: StripView's context menu reads the environment value `\.mixerPlacement`.
     - Each placement presents the sheet only when `learn.showsMaps(in: placement)`.
     - A placement's `onDisappear` closes the sheet only if it is the one showing it.
   - The rest of the mixer's shared UI state follows the same pattern:
     - `MixerModel.browserPlacement: MixerPlacement` and `renamePlacement: MixerPlacement` are set by the button that opens them (`openBrowser(_:in:)`, `beginRename(_:in:)`), as `mapsPlacement` is. The insert popover, the Instruments popover and the rename field are each shown only where `== placement` (`showsBrowser(in:)`, `isRenaming(_:in:)`);
     - `MixerModel.placements: Set<MixerPlacement>` is tracked on appear and disappear (`appeared(_:)`, `disappeared(_:)`). `endOpenDrag()` runs only when the set becomes empty, so hiding the bottom pane during a Touch ride in the mixer window does not end the ride. `DocumentStore`'s close still ends every drag;
     - the faults popover is attached only to the transport bar with `showsViews` true (the document window), so `showsFaults` opens one popover.
   - `.learnAlert` is attached once in the document window, on `DocumentShell`. Views inside the shell (`MixerView` in the `.pane`, `.view` and `.strip` placements, and `StripChartEditor` in the bottom and right panes) skip their own, through the environment value `\.learnAlertHosted` (an `EnvironmentKey`, default false) that `DocumentShell` sets to true. The mixer window and torn windows keep theirs.
   - `openPluginWindow` is set once, at `DocumentShell`'s root: `.environment(\.openPluginWindow, { slot in PluginWindows.show(appStore, doc: docID, slot: slot) })`. It covers the Mix view, the bottom pane's mixer and the left strip. `MixerWindow` keeps its own.
   - The left strip polls with `mixer.tick(now:)` only while its window is key or the document plays or records, as `DocumentWindow`'s `PollKey` does, so a background document does not poll `levels()` at 30 Hz.
   - Why: spec §2 item 2 ("the pane, the view and the window all show the same mixer state") and §2 item 4 (plug-in windows open from the selected track).
   - Cost if wrong: a second sheet, a second popover or a faster meter, all pinned by tests.
9. **Ruling (the transport bar).**
   - `DocumentTransportBar(model:docID:showsViews:)` is an ordinary SwiftUI view at the top of the window's content, not an `NSToolbar`. It holds:
     - today's `TransportToolbar` items: Play/Stop, Loop, `RecordControls`, the position, the device, the faults button (with `showsViews` only, Ruling 8);
     - today's `TransportBar`: the `TransportFieldsHost` and step entry's summary, moved out of `ArrangementPane`;
     - with `showsViews`, the main-view tabs (a segmented picker "Arrange" | "Mix") and the three pane toggles.
   - The mixer window shows the bar with `showsViews: false`. Its tabs and toggles would act on another window, and it has no faults popover (Ruling 8).
   - The Arrange centre stays mounted while the Mix view shows: a `ZStack` keeps `ArrangementPane` with `.opacity(0)`, `.allowsHitTesting(false)` and `.accessibilityHidden(true)`. `ArrangementHost` remakes `ArrangementContainer` on every appearance (`ArrangementPane.swift:28`), and the scroll lives in the view (`TimeAxis.originTick`, `EditorCanvas.verticalOffset`), so ⌃2 then ⌃1 would otherwise lose the scroll and zoom.
   - `StatusLine` moves from `ArrangementPane` to the bottom of `DocumentShell`, so the Mix view shows messages too. Torn windows keep their own.
   - `ProjectBanners` stays under the transport bar, full width.
   - Why: spec §2 items 1 and 2 ("the transport stays visible in all three").
   - Cost if wrong: none expected. The e2e's `play_button` finds an `AXButton` titled "Play" anywhere in the tree.
10. **Ruling (the control channel).**
    - Six registry commands under menu "View":
      - `ui.view_arrange` (⌃1) and `ui.view_mix` (⌃2);
      - `ui.toggle_left` (⌃⌘←), `ui.toggle_bottom` (⌃⌘↓) and `ui.toggle_right` (⌃⌘→), each taking an optional `on` flag (the existing `ON` params);
      - `ui.pane`, keys and channel only (`key_only`), with params:
        - `pane`, Choice `left|bottom|right`, required;
        - `view`, Choice of `PaneView`'s channel names plus the four editor kind names;
        - `shown`, Flag;
        - `size`, Int 0…10 000;
        - `main`, Choice `arrange|mix`, the setup to change (default: the current one, which is not switched).
    - The app refuses with `invalid_params`, before it changes anything:
      - a `ui.pane` call with none of `view`, `shown` and `size`;
      - a view that is not on that pane's list for that main view, with the message "‹view› is not on the ‹pane› pane's list in the ‹main› view.", where ‹view› is the **channel name the caller sent** (`piano_roll`, not `editor`).
    - `runPane`'s order: parse → refuse a missing field → check `ViewLayouts.views(side, in: main).contains(mapped)` → apply `setPane` → only then, for an editor kind, `openDetail(target, kind:, in: main)`. A refused call therefore never touches `layout.detail`, `lastKind` or `bottom.shown`.
    - `size` is clamped to the pane's range. The answer's status names what was set: `"left strip 220 shown"`. `runPane` returns it as `.done(.ran(…, status: text))` itself and leaves `editor.status` nil, so a success does not show in the status line as if it were a failure.
    - Before any change to what a pane or the centre shows, `ViewCommands.run` and `runPane` call `DocumentModel.endPaneEdits()`. It commits a sequence rename (`commitRename(renameDraft)`) if one is open, ends the editor's text edits (`editor.endTextEditing(prefix: "")`) and commits the store's mixer's strip rename (`MixerModel.commitRename()`) if that mixer exists. Otherwise a field that vanished mid-edit keeps `textFieldHasKeyboard` true, and Space and Return answer "A text field has the keyboard." until the next edit.
    - `state.get layout` answers `ControlResult::Layout { doc, layout: LayoutStateRow }`, in this JSON shape:
      ```json
      {"doc": "…", "main": "arrange", "left_strip": 3,
       "left_shows": "strip",
       "views": [{"main": "arrange", "panes": [{"pane": "left", "shown": true, "view": "strip", "size": 220.0, "views": ["strip", "sequences", "inspector"]}, …]}, {"main": "mix", …}]}
      ```
      - `left_shows` is what the left pane really shows: `"hidden"` when the pane is not shown, else the shown view, with `strip` resolved by the rule to `strip`, `inspector` or `nothing` (so `strip`, `inspector`, `sequences`, `nothing` or `hidden`);
      - `left_strip` is the rule's strip whatever the pane shows, hidden or Sequences included (null when the rule gives none).
      - The bottom pane's `view` is its editor kind name (`piano_roll` … from `layout.detail?.kind ?? layout.lastKind`) when `.editor`, else `mixer`.
    - `state.get windows`, `window.mixer`, `window.media_pool` and every window kind are unchanged (spec §2 item 4).
    - Why: spec §4 ("window and pane state … `state.get` and the `ui.*` commands") and §5. Registry commands give menus, keys and the channel one handler, as for every other command since plan 4f.
    - Cost if wrong: names in one table.
11. **Ruling (the window size): the document window's frame is saved in `ViewLayouts.frame`.**
    - `WindowFrameKeeper` (an `NSViewRepresentable` in `DocumentWindow.swift`) applies the saved frame once, when the view joins its window, and only if `PaneGeometry.sanitizedFrame(_:screens:)` accepts it: four finite numbers, at least 700×450, intersecting a screen by at least 100×100.
    - It applies the frame on the next main-queue turn after `viewDidMoveToWindow`, so that SwiftUI's own window restoration does not overwrite it.
    - It saves the frame on `NSWindow.didMoveNotification` and `didEndLiveResizeNotification` through `EditorModel.setDocumentFrame(_:)`, which does nothing when the frame is unchanged. The observer reads the window from `note.object as? NSWindow` and captures no `NSWindow`, which would pin the window until dismantle.
    - `DocumentShell` sets `.frame(minWidth: 700, minHeight: 450)`, so the window cannot be resized below the size `sanitizedFrame` accepts: a smaller frame would be saved and then ignored on reopen.
    - Why: spec §6.3.
    - Cost if wrong: SwiftUI's own restoration wins for an unsaved document, which is harmless.
12. **Ruling (the sequence list): today's `SequenceSidebar` becomes the left pane's "Sequences" view, unchanged.**
    - Picking a sequence still works there. It no longer shows by default.
    - A sequence rename open when the left pane hides or changes view is committed (`endPaneEdits`, Ruling 10), because the row's `onChange(of: focused)` (`SequenceSidebar.swift:83`) never runs once the row is gone.
    - The app README's earlier steps that say "the sidebar" get one note in Task 9, which also covers "the toolbar", "the mixer's toolbar" and "the detail pane". The steps that name the window's toolbar or the detail pane are also corrected one by one in Task 9 (step 3a), because the owner tests from them.
    - Why: spec §6.1 (owner).
    - Cost if wrong: none expected.

## Spec checks (no conflict needing the owner)

- The `mixview.html` mockup shows a short arrange strip above the mixer. Spec §2 item 2 (owner, revised 2026-11-03) says "with no arrangement". The spec wins.
- Spec §2 item 1 puts the right pane beside the bottom one. With the bottom pane hidden it does not say where the right pane goes. Ruling 2 decides.
- Spec §6.3 says "with the window size", but today's `ui.json` holds no document window frame. Ruling 11 adds it.
- The root README lists "SMF, OMF and AAF import", but no import of any of the three exists today. Task 9 lists SMF as planned and says OMF and AAF may come after launch (spec §2 item 11).

## File map

| File | Task | Role |
|---|---|---|
| `app/Sources/VizDAWAppModel/WindowLayout.swift` (new), `EditorLayout.swift`, `EditorModel.swift`; `app/Tests/VizDAWAppModelTests/WindowLayoutTests.swift` (new) | 1 | per-view setups, intents |
| `app/Sources/VizDAWAppModel/PaneGeometry.swift` (new); `app/Tests/VizDAWAppModelTests/PaneGeometryTests.swift` (new) | 2 | frames, clamping, saved frame |
| `app/Sources/VizDAWAppModel/PaneContent.swift` (new), `MixerModel.swift`, `LearnModel.swift`; `app/Tests/VizDAWAppModelTests/LeftPaneTests.swift` (new) | 3 | strip rule, inspector, tick, maps, popover and rename placement |
| `app/Sources/VizDAWAppModel/PaneContent.swift`, `DocumentModel.swift`, `EditorLayout.swift`, `EditorModel+Windows.swift` (`pruneLayout`), `StripChartModel.swift`; `app/Tests/VizDAWAppModelTests/RightPaneTests.swift` (new) | 4 | markers, plug-in insert, controllers chart |
| `crates/core-control/src/{registry.rs,request.rs}`, `crates/core-control/tests/commands.rs`; `crates/core-ffi/src/control.rs`, `crates/core-ffi/tests/control.rs`; `app/Sources/VizDAWAppModel/ControlCommands.swift` (stub case) | 5 | registry, `layout` target, JSON |
| `app/Sources/VizDAWAppModel/ViewCommands.swift` (new), `ControlCommands.swift`, `TransportCommands.swift` (conformance line), `DocumentModel.swift` (`storeMixer`), `DocumentStore.swift` (sets it); `app/Tests/VizDAWAppModelTests/ViewCommandsTests.swift` (new) | 6 | `ui.*` handlers, `state.get layout`, `endPaneEdits` |
| `app/Sources/VizDAW/DocumentShell.swift` (new), `DocumentView.swift`, `DocumentWindow.swift`, `DetailPane.swift`, `ArrangementPane.swift`, `MixerView.swift`, `AppCommands.swift`, `StripChartPane.swift` (`learnAlertHosted`) | 7 | shell, transport bar, Mix view, mixer window bar, View menu, frame keeper |
| `app/Sources/VizDAW/PaneViews.swift` (new), `DocumentShell.swift`, `MediaPoolWindow.swift`, `PluginBrowserView.swift` | 8 | left, bottom and right pane contents |
| `crates/vizctl/tests/e2e.rs`, `app/README.md`, `README.md`, `docs/feature-backlog.md` | 9 | e2e, by-eye steps and earlier steps corrected, root README, backlog row |

---

### Task 1: Per-view pane setups in the layout

**Files:**
- Create: `app/Sources/VizDAWAppModel/WindowLayout.swift`, `app/Tests/VizDAWAppModelTests/WindowLayoutTests.swift`
- Modify: `app/Sources/VizDAWAppModel/EditorLayout.swift` (field, `CodingKeys`, `init(from:)`), `app/Sources/VizDAWAppModel/EditorModel.swift` (`openDetail`). `showDetail` (`EditorModel+Windows.swift`) gets the new behaviour through `openDetail` and is not changed.

**Interfaces:**
- Consumes: `EditorLayout`, `KeyedDecodingContainer.field`, `EditorModel.updateLayout`, `DetailEditorKind`.
- Produces (all `public`, `Codable, Equatable, Sendable` where they are data):
  - `enum MainView: String, CaseIterable { case arrange, mix }`, with `title` "Arrange" or "Mix";
  - `enum PaneSide: String, CaseIterable { case left, bottom, right }`, with `title` "Left Pane", "Bottom Pane" or "Right Pane";
  - `enum PaneView: String, CaseIterable`, whose cases with their raw (channel) names are `strip`, `sequences`, `inspector`, `editor`, `mixer`, `controllers`, `mediaPool = "media_pool"`, `plugins` and `markers`. Its `title` values are "Channel Strip", "Sequences", "Inspector", "Editor", "Mixer", "Controllers", "Media Pool", "Plug-ins" and "Markers";
  - `enum MixerPlacement: String, Sendable { case window, pane, view, strip }`, used by Task 3;
  - `struct PaneState { var shown: Bool; var view: PaneView; var size: Double }`;
  - `struct PaneSetup { var left, bottom, right: PaneState; subscript(side: PaneSide) -> PaneState { get set } }`;
  - `struct ViewLayouts { var main: MainView; var arrange: PaneSetup; var mix: PaneSetup; var frame: [Double]?; subscript(view: MainView) -> PaneSetup { get set }; var current: PaneSetup { get set }; static func views(_ side: PaneSide, in main: MainView) -> [PaneView]; static let sizeRange: [PaneSide: ClosedRange<Double>]; func sanitized() -> ViewLayouts }`, with the defaults of Ruling 1;
  - private, in `WindowLayout.swift`: `struct PanePatch: Decodable { var shown: Bool?; var view: PaneView?; var size: Double? }` (Ruling 1). `PaneState` encodes its three fields as today's types do; it is decoded only through a patch;
  - `EditorLayout.views: ViewLayouts` (coding key `views`);
  - on `EditorModel`, in `WindowLayout.swift`:
    - `func showMainView(_ v: MainView)`;
    - `func setPaneShown(_ side: PaneSide, _ on: Bool?, in main: MainView? = nil)`, where nil `on` toggles;
    - `func setPane(_ side: PaneSide, view: PaneView?, shown: Bool?, size: Double?, in main: MainView? = nil) -> String?`, which returns the refusal message, or nil when applied;
    - `func setDocumentFrame(_ f: [Double])`;
  - `EditorModel.openDetail(_ target: EditorTarget, kind: DetailEditorKind? = nil, in main: MainView? = nil)`: today's signature gains `in main`, the view whose bottom pane it shows (nil: the current one).

- [ ] **Step 1: Write the failing tests** in `WindowLayoutTests.swift` (`@MainActor @Suite struct WindowLayoutTests`; the `EditorModel` comes from a `FakeSession`, as in `EditorModelTests`).

```swift
@Test func aNewLayoutOpensArrangeWithTheStripLeftAndTheEditorBelow() {
    let v = EditorLayout().views
    #expect(v.main == .arrange)
    #expect(v.arrange.left == PaneState(shown: true, view: .strip, size: 220))
    #expect(v.arrange.bottom == PaneState(shown: true, view: .editor, size: 260))
    #expect(v.arrange.right == PaneState(shown: false, view: .controllers, size: 260))
    #expect([v.mix.left.shown, v.mix.bottom.shown, v.mix.right.shown] == [false, false, false])
    #expect(v.mix.right.view == .plugins)
}
@Test func eachPaneOffersOnlyItsOwnList() {
    #expect(ViewLayouts.views(.left, in: .arrange) == [.strip, .sequences, .inspector])
    #expect(ViewLayouts.views(.bottom, in: .arrange) == [.editor, .mixer])
    #expect(ViewLayouts.views(.right, in: .arrange) == [.controllers, .mediaPool, .plugins, .markers])
}
@Test func theMixViewOffersNoMixerBelow() {
    #expect(ViewLayouts.views(.bottom, in: .mix) == [.editor])
    let e = editor()
    #expect(e.setPane(.bottom, view: .mixer, shown: nil, size: nil, in: .mix)
        == "mixer is not on the bottom pane's list in the mix view.")
    #expect(e.layout.views.mix.bottom.view == .editor)
}
@Test func switchingViewsRestoresEachSetup() {
    let e = editor()
    _ = e.setPane(.right, view: .markers, shown: true, size: 300, in: nil)
    e.showMainView(.mix)
    _ = e.setPane(.left, view: nil, shown: true, size: 400, in: nil)
    e.showMainView(.arrange)
    #expect(e.layout.views.current.right == PaneState(shown: true, view: .markers, size: 300))
    #expect(e.layout.views.mix.left == PaneState(shown: true, view: .strip, size: 400))
}
@Test func togglingAPaneKeepsItsViewAndSize() {
    let e = editor()
    _ = e.setPane(.left, view: .inspector, shown: nil, size: 300, in: nil)
    e.setPaneShown(.left, nil)
    e.setPaneShown(.left, nil)
    #expect(e.layout.views.arrange.left == PaneState(shown: true, view: .inspector, size: 300))
}
@Test func aCorruptViewsFieldDecodesToDefaults() {
    let json = #"{"views":{"main":"edit","arrange":{"left":{"shown":true,"view":"mixer","size":-5},"#
        + #""bottom":{"shown":"yes","view":"editor","size":1e9},"right":{"view":"nope"}},"#
        + #""mix":{"bottom":{"shown":true,"view":"mixer","size":200}}},"laneHeight":60}"#
    let l = EditorLayout.decode(json)
    #expect(l.views.main == .arrange)
    #expect(l.views.arrange.left == PaneState(shown: true, view: .strip, size: 160))
    #expect(l.views.arrange.bottom == PaneState(shown: true, view: .editor, size: 900))
    #expect(l.views.arrange.right == ViewLayouts().arrange.right)
    #expect(l.views.mix.bottom == PaneState(shown: true, view: .editor, size: 200))
    #expect(l.laneHeight == 60)
}
@Test func anOlderLayoutWithoutViewsDecodes() {
    let l = EditorLayout.decode(#"{"version":1,"laneHeight":80}"#)
    #expect(l.views == ViewLayouts())
    #expect(l.laneHeight == 80)
}
@Test func openingADetailShowsTheBottomEditor() {
    let e = editor()
    _ = e.setPane(.bottom, view: .mixer, shown: false, size: nil, in: nil)
    e.openDetail(.sequence(1), kind: .list)
    #expect(e.layout.views.arrange.bottom.shown && e.layout.views.arrange.bottom.view == .editor)
    #expect(e.layout.detail?.kind == .list)
}
@Test func setPaneRefusesAViewOffItsList() {
    let e = editor()
    let before = e.layout
    #expect(e.setPane(.left, view: .markers, shown: nil, size: nil, in: nil)
        == "markers is not on the left pane's list in the arrange view.")
    #expect(e.layout == before)
}
@Test func viewsRoundTripThroughTheEncodedLayout() {
    var l = EditorLayout()
    l.views.main = .mix
    l.views.mix.right = PaneState(shown: true, view: .mediaPool, size: 333)
    l.views.frame = [10, 20, 1200, 800]
    #expect(EditorLayout.decode(l.encoded()) == l)
}
```

`editor()` is a file-private helper returning an `EditorModel` over a fresh `FakeSession` (use the existing helper of `EditorModelTests` if it is reachable).

- [ ] **Step 2: Run them and see them fail.** `scripts/build-ffi.sh && scripts/swift.sh test --filter WindowLayoutTests`. Expected: they do not compile, because `MainView` is not defined.

- [ ] **Step 3: Implement `WindowLayout.swift` and the `EditorLayout.views` field.**
  - Each struct decodes field by field through `field` (Ruling 1):
    - a pane is read as a `PanePatch`, each field through `field(…, nil)`, so `"shown": "yes"` or `"view": "nope"` gives nil for that field alone;
    - `PaneSetup.init(from:)` and `ViewLayouts.init(from:)` merge each patch onto `ViewLayouts().arrange[side]` or `ViewLayouts().mix[side]`. So arrange `right: {"view": "nope"}` comes back as the arrange right default, and arrange `bottom: {"shown": "yes", "view": "editor", "size": 1e9}` keeps the default `shown` (true) and its size (clamped to 900);
    - do not reuse `EditorLayout.sane`, which turns −5 into the default where the test wants 160.
  - `EditorLayout.init(from:)` sets `views = c.field(.views, d.views).sanitized()`.
  - `sanitized()` applies Ruling 1:
    - an off-list view becomes that side's default for that main view;
    - a non-finite size becomes the default;
    - a finite size is clamped to `sizeRange[side]`;
    - a `frame` that is not exactly four finite numbers becomes nil.
  - The refusal text is exactly `"\(view.rawValue) is not on the \(side.rawValue) pane's list in the \(main.rawValue) view."`. `setPane` clamps `size` to the range.
  - `openDetail` (`EditorModel.swift`) sets `l.views[main ?? l.views.main].bottom.shown = true` and `.view = .editor` inside its existing `updateLayout`. `showDetail` gets this through `openDetail`, for the current view.

- [ ] **Step 4: Run them and see them pass,** then the whole suite: `scripts/swift.sh test`. Expected: 1003 tests pass (993 + 10). `EditorModelTests`' existing round trip still passes.

- [ ] **Step 5: Commit**

```bash
git add app/Sources/VizDAWAppModel/WindowLayout.swift app/Sources/VizDAWAppModel/EditorLayout.swift \
  app/Sources/VizDAWAppModel/EditorModel.swift app/Tests/VizDAWAppModelTests/WindowLayoutTests.swift
git commit -m "app model: per-view pane setups saved in ui.json, with tolerant decoding (plan 7a Task 1)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -7
```

---

### Task 2: Layout A's geometry and the saved window frame

**Files:**
- Create: `app/Sources/VizDAWAppModel/PaneGeometry.swift`, `app/Tests/VizDAWAppModelTests/PaneGeometryTests.swift`

**Interfaces:**
- Consumes: `PaneSetup`, `PaneSide`, `ViewLayouts.sizeRange` (Task 1).
- Produces:
  - `public struct PaneFrames: Equatable { public var centre: CGRect; public var left, bottom, right: CGRect?; public var hidden: [PaneSide] }`. The centre is always set (non-optional), and `hidden` lists the panes dropped for this frame only;
  - `public enum PaneGeometry`:
    - `static let centreMinimum = CGSize(width: 240, height: 160)`;
    - `static let bottomMinimumWidth: CGFloat = 240`;
    - `static let divider: CGFloat = 1`;
    - `static func frames(in size: CGSize, setup: PaneSetup) -> PaneFrames`. The frames are in the region under the transport bar, with a top-left origin;
    - `static func drag(_ setup: PaneSetup, side: PaneSide, by delta: CGFloat, in size: CGSize) -> Double`, the new clamped size;
    - `static func sanitizedFrame(_ f: [Double]?, screens: [CGRect]) -> CGRect?` (Ruling 11).

- [ ] **Step 1: Write the failing tests** in `PaneGeometryTests.swift`. The setups below are built from `ViewLayouts().arrange` with the sizes shown.

```swift
@Test func layoutAPutsTheLeftPaneFullHeightAndTheRightBesideTheBottom() {
    var s = ViewLayouts().arrange; s.right.shown = true   // left 220, bottom 260, right 260
    let f = PaneGeometry.frames(in: CGSize(width: 1400, height: 900), setup: s)
    #expect(f.left == CGRect(x: 0, y: 0, width: 220, height: 900))
    #expect(f.centre == CGRect(x: 221, y: 0, width: 1179, height: 639))
    #expect(f.bottom == CGRect(x: 221, y: 640, width: 918, height: 260))
    #expect(f.right == CGRect(x: 1140, y: 640, width: 260, height: 260))
    #expect(f.hidden.isEmpty)
}
@Test func onlyTheRightPaneRunsBesideTheCentre() {
    var s = ViewLayouts().arrange; s.bottom.shown = false; s.right.shown = true
    let f = PaneGeometry.frames(in: CGSize(width: 1400, height: 900), setup: s)
    #expect(f.bottom == nil)
    #expect(f.right == CGRect(x: 1140, y: 0, width: 260, height: 900))
    #expect(f.centre == CGRect(x: 221, y: 0, width: 918, height: 900))
}
@Test func hiddenPanesGiveTheCentreTheirSpace() {
    let f = PaneGeometry.frames(in: CGSize(width: 1000, height: 700), setup: ViewLayouts().mix)
    #expect(f.centre == CGRect(x: 0, y: 0, width: 1000, height: 700))
    #expect(f.left == nil && f.bottom == nil && f.right == nil)
}
@Test func aSmallWindowShrinksPanesBeforeTheCentre() {
    var s = ViewLayouts().arrange
    s.left.size = 480; s.bottom.size = 900; s.right.shown = true; s.right.size = 520
    let f = PaneGeometry.frames(in: CGSize(width: 700, height: 450), setup: s)
    #expect(f.centre.width >= 240 && f.centre.height >= 160)
    #expect(f.left?.width == 278 && f.right?.width == 180 && f.bottom?.height == 289)
    #expect((f.bottom?.width ?? 0) == PaneGeometry.bottomMinimumWidth)   // 421 − 1 − 180
    #expect(f.hidden.isEmpty)
    let narrow = PaneGeometry.frames(in: CGSize(width: 560, height: 450), setup: s)
    #expect(narrow.hidden == [.right])
    #expect(narrow.left?.width == 319 && narrow.bottom?.height == 289)
    let tiny = PaneGeometry.frames(in: CGSize(width: 300, height: 200), setup: s)
    #expect(tiny.hidden == [.right, .left, .bottom])
}
@Test func aDividerDragIsClamped() {
    let s = ViewLayouts().arrange
    let size = CGSize(width: 1400, height: 900)
    #expect(PaneGeometry.drag(s, side: .left, by: 10_000, in: size) == 480)
    #expect(PaneGeometry.drag(s, side: .left, by: -10_000, in: size) == 160)
    #expect(PaneGeometry.drag(s, side: .bottom, by: 40, in: size) == 300)   // dragging up grows it
    #expect(PaneGeometry.drag(s, side: .bottom, by: .nan, in: size) == 260)
}
@Test func aSavedFrameOffEveryScreenIsIgnored() {
    let screens = [CGRect(x: 0, y: 0, width: 1512, height: 982)]
    #expect(PaneGeometry.sanitizedFrame([100, 100, 1200, 800], screens: screens)
        == CGRect(x: 100, y: 100, width: 1200, height: 800))
    #expect(PaneGeometry.sanitizedFrame([5000, 5000, 1200, 800], screens: screens) == nil)
    #expect(PaneGeometry.sanitizedFrame([0, 0, 300, 200], screens: screens) == nil)
    #expect(PaneGeometry.sanitizedFrame([0, 0, .nan, 800], screens: screens) == nil)
    #expect(PaneGeometry.sanitizedFrame([0, 0, 1200], screens: screens) == nil)
    #expect(PaneGeometry.sanitizedFrame(nil, screens: screens) == nil)
}
```

- [ ] **Step 2: Run them and see them fail** (`--filter PaneGeometryTests`): `PaneGeometry` is not defined.

- [ ] **Step 3: Implement `PaneGeometry.swift`.**
  - Ruling 2's rules. Dividers are 1 point and sit between the regions.
  - The rule of Ruling 2, in order:
    1. clamp each size to its range;
    2. shrink right, then bottom, then left towards their minimums, each only as far as needed given the others' current sizes, so that the centre gets 240×160 and the bottom row gets `bottomMinimumWidth + 1 + right`;
    3. if it still does not fit, hide right and redo step 2 from the saved sizes; then left; then bottom.
  - Worked through the test's setup (left 480, bottom 900, right 520, all shown):
    - 700×450: right to 180 (the row needs 240 + 1 + 180 = 421), bottom to 289 (450 − 1 − 160), left to 278 (700 − 1 − 421). Nothing hides;
    - 560×450: left would need 138, below 160, so right hides; redone, left is 319 (560 − 1 − 240) and bottom 289;
    - 300×200: right, then left, then bottom hide.
  - The drag sign: left grows with a positive `delta`, right with a negative one, and bottom with a **positive** one (the divider passes `-translation.height`). A non-finite `delta` keeps the size.

- [ ] **Step 4: Run them and see them pass,** then the suite: 1009.

- [ ] **Step 5: Commit** `PaneGeometry.swift` and its tests. Message: "app model: layout A's pane frames, clamping and the saved window frame (plan 7a Task 2)", with the two trailer lines.

---

### Task 3: The left pane's strip, the inspector, one mixer poll per frame

**Files:**
- Create: `app/Sources/VizDAWAppModel/PaneContent.swift`, `app/Tests/VizDAWAppModelTests/LeftPaneTests.swift`
- Modify: `app/Sources/VizDAWAppModel/MixerModel.swift` (add `tick`, the browser and rename placements, `placements`), `app/Sources/VizDAWAppModel/LearnModel.swift` (add `mapsPlacement`, `showsMaps(in:)`)

**Interfaces:**
- Consumes: `LaneRow` (`audioOut`, `midiDest`, `arm`), `DestRow` (`plugin`, `external`), `StripRow` / `StripKindRow`, `SelectionRow.lanes`, `DocumentModel.lanes`, `selectedSeq`, `MixerPlacement` (Task 1).
- Produces:
  - `public enum StripFocus`:
    - `enum Result: Equatable { case strip(UInt64), inspector, nothing }`;
    - `static func strip(of lane: LaneRow, destinations: [DestRow], strips: [StripRow]) -> UInt64?`;
    - `static func strip(selected: [LaneRow], armed: [LaneRow], destinations: [DestRow], strips: [StripRow]) -> Result`;
    - `static let nothingMessage = "Select a lane to see its channel strip."`.
  - `@MainActor extension DocumentModel { public func leftStrip(strips: [StripRow], destinations: [DestRow]) -> StripFocus.Result }`. It gathers the selection's lanes in the shown sequence, in selection order, from `editor.selection.lanes`, matched by ID against `lanes`. A lane the snapshot no longer has is skipped. Then it calls the rule with `armed: lanes.filter(\.arm)`.
  - `public struct InspectorRow: Equatable, Sendable { public let label: String; public let value: String }`, and `public enum Inspector { static func rows(lane: LaneRow?, region: EventRow?, regionName: String?, destinations: [DestRow], strips: [StripRow]) -> [InspectorRow]; static let emptyMessage = "Select a lane or a region." }`. Use the tree's type for a region event (`EventRow` or its equivalent) and say which in the report.
  - `MixerModel`:
    - `public static let tickInterval: Double = 0.030`;
    - `@discardableResult public func tick(now: Double) -> Bool`, true when it polled. `now` is seconds from any monotonic clock, and a view passes `ProcessInfo.processInfo.systemUptime`;
    - `public var browserPlacement: MixerPlacement = .window` and `public func openBrowser(_ t: BrowserTarget, in p: MixerPlacement)`, which sets the placement, then `browser`; `public func showsBrowser(in p: MixerPlacement) -> Bool`, true when `browser != nil && browserPlacement == p` (Ruling 8);
    - `public private(set) var renamePlacement: MixerPlacement = .window`; today's `beginRename(_ strip: StripRow)` becomes `beginRename(_ strip: StripRow, in p: MixerPlacement = .window)`; `public func isRenaming(_ strip: UInt64, in p: MixerPlacement) -> Bool`;
    - `public private(set) var placements: Set<MixerPlacement> = []`, `public func appeared(_ p: MixerPlacement)` and `public func disappeared(_ p: MixerPlacement)`. `disappeared` removes `p` and calls `endOpenDrag()` only when the set becomes empty.
  - `LearnModel`:
    - `public var mapsPlacement: MixerPlacement = .window`;
    - `public func showMaps(in p: MixerPlacement)`, which sets the placement, then `showsMaps = true`;
    - `public func showsMaps(in p: MixerPlacement) -> Bool`;
    - `public func closeMaps(from p: MixerPlacement)`, which sets `showsMaps = false` only when `mapsPlacement == p`.

- [ ] **Step 1: Write the failing tests** in `LeftPaneTests.swift`. Build rows with small file-private helpers that fill the other fields with neutral values:
  - `lane(_ id: UInt64, audioOut: UInt64? = nil, midiDest: UInt64? = nil, arm: Bool = false) -> LaneRow`, named `"Lane \(id)"`. It overloads `FakeSession.swift`'s `lane(_:_:mute:)` by its labels, so both stay callable;
  - `dest(_ id: UInt64, plugin: Bool = false, external: Bool = false, name: String = "Dest") -> DestRow`;
  - `strip(_ id: UInt64, _ kind: StripKindRow) -> StripRow`, over `FakeSession.swift`'s `stripRow`.

```swift
@Test func anAudioLaneShowsItsPathsStrip() {
    let r = StripFocus.strip(selected: [lane(1, audioOut: 7)], armed: [], destinations: [],
                             strips: [strip(10, .audioPath(path: 7)), strip(11, .master)])
    #expect(r == .strip(10))
}
@Test func aPluginLaneShowsItsInstrumentStrip() {
    #expect(StripFocus.strip(selected: [lane(1, midiDest: 4)], armed: [], destinations: [dest(4, plugin: true)],
                             strips: [strip(12, .instrument(dest: 4))]) == .strip(12))
}
@Test func aBuiltinSynthLaneShowsTheSynthStrip() {
    #expect(StripFocus.strip(selected: [lane(1, midiDest: 2)], armed: [], destinations: [dest(2)],
                             strips: [strip(13, .builtinSynth)]) == .strip(13))
}
@Test func anExternalMidiLaneShowsTheInspector() {
    #expect(StripFocus.strip(selected: [lane(1, midiDest: 3)], armed: [], destinations: [dest(3, external: true)],
                             strips: [strip(13, .builtinSynth)]) == .inspector)
    #expect(StripFocus.strip(selected: [lane(1)], armed: [], destinations: [], strips: []) == .inspector)
}
@Test func twoLanesOnOnePathShowOneStrip() {
    #expect(StripFocus.strip(selected: [lane(1, audioOut: 7), lane(2, audioOut: 7)], armed: [], destinations: [],
                             strips: [strip(10, .audioPath(path: 7))]) == .strip(10))
}
@Test func differentPathsShowTheFirstSelectedLanesStrip() {
    #expect(StripFocus.strip(selected: [lane(2, audioOut: 8), lane(1, audioOut: 7)], armed: [], destinations: [],
                             strips: [strip(10, .audioPath(path: 7)), strip(11, .audioPath(path: 8))]) == .strip(11))
}
@Test func noSelectionShowsTheArmedLaneElseNothing() {
    let strips = [strip(10, .audioPath(path: 7))]
    #expect(StripFocus.strip(selected: [], armed: [lane(1, audioOut: 7, arm: true)], destinations: [],
                             strips: strips) == .strip(10))
    #expect(StripFocus.strip(selected: [], armed: [], destinations: [], strips: strips) == .nothing)
    // A selected lane the document no longer has is skipped (leftStrip, over a FakeSession):
    let m = documentWithLanes([lane(1, audioOut: 7)])
    m.editor.setSelection(SelectionRow(timeRange: nil, lanes: [LaneKey(m.selectedSeq!, 99).ref], events: []))
    #expect(m.leftStrip(strips: strips, destinations: []) == .nothing)
    // A lane in another sequence is ignored, even with the same ID:
    let otherSeq = m.selectedSeq! + 1
    m.editor.setSelection(SelectionRow(timeRange: nil, lanes: [LaneKey(otherSeq, 1).ref], events: []))
    #expect(m.leftStrip(strips: strips, destinations: []) == .nothing)
}
@Test func theInspectorDescribesTheLaneAndRegion() {
    let rows = Inspector.rows(lane: lane(1, midiDest: 3), region: nil, regionName: nil,
                              destinations: [dest(3, external: true, name: "Ext Synth")], strips: [])
    #expect(rows.first == InspectorRow(label: "Name", value: "Lane 1"))
    #expect(rows.contains(InspectorRow(label: "Plays", value: "Ext Synth")))
    #expect(Inspector.rows(lane: nil, region: nil, regionName: nil, destinations: [], strips: []).isEmpty)
}
@Test func twoMixerViewsPollOncePerFrame() throws {
    let f = demoFake()
    let mixer = try MixerModel(document: DocumentModel(session: f))
    #expect(mixer.tick(now: 10.000))
    #expect(f.levelsCalls == 1)
    #expect(!mixer.tick(now: 10.010))   // the second placement, same frame
    #expect(f.levelsCalls == 1)
    #expect(mixer.tick(now: 10.031))
    #expect(f.levelsCalls == 2)
}
@Test func learnMapsShowInOnePlacement() {
    let learn = fakeDocument().learn
    learn.showMaps(in: .pane)
    #expect(learn.showsMaps(in: .pane) && !learn.showsMaps(in: .window))
    learn.closeMaps(from: .window)
    #expect(learn.showsMaps)
    learn.closeMaps(from: .pane)
    #expect(!learn.showsMaps)
}
@Test func popoversAndRenameShowInOnePlacement() throws {
    let mixer = try MixerModel(document: DocumentModel(session: mixerFake()))   // strip 1 "Synth"
    let synth = try #require(mixer.strips.first)
    mixer.openBrowser(.insert(strip: 1), in: .strip)
    #expect(mixer.showsBrowser(in: .strip) && !mixer.showsBrowser(in: .window))
    mixer.openBrowser(.instrument, in: .window)
    #expect(mixer.showsBrowser(in: .window) && !mixer.showsBrowser(in: .strip))
    mixer.browser = nil
    mixer.beginRename(synth, in: .pane)
    #expect(mixer.isRenaming(1, in: .pane) && !mixer.isRenaming(1, in: .window))
    let fader = AutomationTargetRow.stripFader(strip: 1)
    mixer.appeared(.window)
    mixer.appeared(.pane)
    mixer.touch(fader)
    mixer.disappeared(.pane)
    #expect(mixer.isTouched(fader))    // the ride in the mixer window goes on
    mixer.disappeared(.window)
    #expect(!mixer.isTouched(fader))   // the last placement gone ends it
}
```

`fakeDocument()` and `documentWithLanes(_:)` are helpers over `FakeSession`. Use the existing ones of `MixerModelTests` and `LearnModelTests` where they exist.

- [ ] **Step 2: Run them and see them fail** (`--filter LeftPaneTests`).

- [ ] **Step 3: Implement.**
  - Ruling 5 in `StripFocus`.
  - Ruling 6's rows in `Inspector`:
    - "Plays" is the destination name, `"Audio: \(strip.name)"`, or "Nothing";
    - Input and Monitor use the names `LaneInputs` already shows;
    - Region rows use `Formatting`'s gain text and the fades in ms as the Region menu shows them.
  - `tick` returns false when `now - last < tickInterval`, and true after calling `refreshIfNeeded`, `pollLevels` and `pollAutomation`. The first call always polls.
  - The placements of Ruling 8. `beginRename`'s default `.window` keeps today's callers and tests unchanged.
  - `MixerView`'s `.task` loop, popovers and `onDisappear` change in Task 7, not here.

- [ ] **Step 4: Run them and see them pass;** suite 1020 (1009 + 11).

- [ ] **Step 5: Commit** the three model files and the test file. Message: "app model: the left pane's strip rule, the inspector, one mixer poll per frame, the maps, popover and rename placements (plan 7a Task 3)", with the trailers.

---

### Task 4: The right pane's models: markers, the plug-in browser's insert, the controllers chart

**Files:**
- Modify: `app/Sources/VizDAWAppModel/PaneContent.swift`, `app/Sources/VizDAWAppModel/DocumentModel.swift` (`controllersChart`), `app/Sources/VizDAWAppModel/EditorLayout.swift` (`controllers` field), `app/Sources/VizDAWAppModel/StripChartModel.swift` (`static func controllers(_:)`), `app/Sources/VizDAWAppModel/EditorModel+Windows.swift` (`pruneLayout`)
- Create: `app/Tests/VizDAWAppModelTests/RightPaneTests.swift`

**Interfaces:**
- Consumes: `SequenceInfo.markers` / `MarkerRow`, `TransportMarks.locators`, `MeterMath.bar(at:_:)`, `DocumentModel.moveTo(_:)`, `DocumentModel.selectedStrips`, `MixerModel.browser` / `pick(_:)` / `master`, `StripFocus` and `leftStrip` (Task 3), `StripChartModel.init(document:read:write:)`, `FakeSession.calls` (which already records `"addInsert <strip> end <plugin id>"`).
- Produces:
  - `public struct MarkerListRow: Equatable, Identifiable, Sendable { public let id: String; public let name: String; public let tick: Int64; public let position: String }`, where `id` is `"m<i>"` or `"l<n>"`;
  - `public enum MarkerList`:
    - `static func rows(info: SequenceInfo?, marks: TransportMarks) -> [MarkerListRow]`, ordered by tick, markers before locators at equal ticks;
    - `@MainActor static func go(_ row: MarkerListRow, _ m: DocumentModel)`, which calls `moveTo` and puts a failure in `m.editor.status`;
  - `@MainActor public enum PaneContent`:
    - `static func insertPlugin(_ p: PluginRefRow, document: DocumentModel, mixer: MixerModel) -> String?`, whose strip is the single selected non-master mixer strip, else the left-pane rule's (Ruling 7);
    - `static let noStripMessage = "Select a lane or one mixer strip first."`;
  - `EditorLayout.controllers: [String: [StripLane]]` (coding key `controllers`, tolerant);
  - `StripChartModel.controllers(_ document: DocumentModel) -> StripChartModel`;
  - `DocumentModel.controllersChart: StripChartModel`, lazy and made once;
  - test-side, internal so that Task 6 can use it, in `RightPaneTests.swift`: `func laneOnStrip10() -> FakeSession`. It builds on `demoFake()`:
    - `f.laneTable[3]?[0].audioOut = 7` (Song's first lane, Groove, ID 30);
    - `f.mixerSnap = MixerSnapshot(version: f.snap.version, strips: [stripRow(10, "Out 1", kind: .audioPath(path: 7)), stripRow(0, "Master", kind: .master)], buses: [])`.

- [ ] **Step 1: Write the failing tests** in `RightPaneTests.swift`. `fakeDocumentAndSession(_ f: FakeSession = demoFake()) -> (DocumentModel, FakeSession)`, `selectFirstLane(_:)` (selects `m.lanes[0]`) and `let delay = PluginRefRow(format: .clap, id: "x.Delay", name: "Delay", vendor: "V", version: "1")` are file-private helpers.

```swift
@Test func markersListTheSequencesMarkersAndSetLocators() {
    var marks = TransportMarks(); marks.setLocator(2, 1920); marks.setLocator(0, 0)
    let rows = MarkerList.rows(info: info(markers: [MarkerRow(tick: 1920, name: "Chorus"), MarkerRow(tick: 0, name: "Intro")]), marks: marks)
    #expect(rows.map(\.name) == ["Intro", "Locator 0", "Chorus", "Locator 2"])
    #expect(rows.first?.position == "1.1.0")
}
@Test func choosingAMarkerMovesThePlayhead() {
    let (m, session) = fakeDocumentAndSession()
    MarkerList.go(MarkerListRow(id: "m0", name: "Chorus", tick: 1920, position: "1.2.0"), m)
    #expect(session.locates.last == 1920)
}
@Test func aPickedPluginGoesOnTheSelectedLanesStrip() throws {
    let (m, session) = fakeDocumentAndSession(laneOnStrip10())   // lane 30 plays through strip 10
    selectFirstLane(m)
    let mixer = try MixerModel(document: m)
    #expect(PaneContent.insertPlugin(delay, document: m, mixer: mixer) == nil)
    #expect(session.calls.last == "addInsert 10 end \(delay.id)")
}
@Test func aPickedPluginGoesOnTheSelectedMixerStrip() throws {
    let (m, session) = fakeDocumentAndSession(laneOnStrip10())   // no lane selected, none armed
    let mixer = try MixerModel(document: m)
    m.selectedStrips = [10]
    #expect(PaneContent.insertPlugin(delay, document: m, mixer: mixer) == nil)
    #expect(session.calls.last == "addInsert 10 end \(delay.id)")
    m.selectedStrips = [0]   // master alone falls back to the lane rule, which has no lane
    #expect(PaneContent.insertPlugin(delay, document: m, mixer: mixer) == PaneContent.noStripMessage)
}
@Test func aPluginWithNoStripIsRefusedInTheStatusLine() throws {
    let (m, _) = fakeDocumentAndSession()
    let mixer = try MixerModel(document: m)
    #expect(PaneContent.insertPlugin(delay, document: m, mixer: mixer) == PaneContent.noStripMessage)
}
@Test func theControllersChartFollowsTheSelectedLane() {
    let (m, _) = fakeDocumentAndSession()
    #expect(m.controllersChart.state == nil)
    selectFirstLane(m)
    #expect(m.controllersChart.state?.kind == .stripChart)
    #expect(m.controllersChart.state?.target == .lane(seq: m.selectedSeq!, lane: m.lanes[0].id))
}
@Test func theControllersRowsAreKeptPerLane() {
    let (m, _) = fakeDocumentAndSession()
    selectFirstLane(m)
    var d = m.controllersChart.state!; d.stripRows = [.velocity, .controller(1)]   // the tree's StripLane cases
    m.controllersChart.write(d)
    #expect(EditorLayout.decode(m.editor.layout.encoded()).controllers["\(m.selectedSeq!):\(m.lanes[0].id)"]
        == [.velocity, .controller(1)])
}
```

- [ ] **Step 2: Run them and see them fail** (`--filter RightPaneTests`).

- [ ] **Step 3: Implement** Ruling 7's model parts.
  - `controllers(_:)` reads through `leftStrip`'s lane gathering: the first considered lane, selection first and then armed. Its `write` stores only `stripRows`, under the `"seq:lane"` key.
  - `insertPlugin` first takes the single selected mixer strip (`document.selectedStrips.count == 1`, not `mixer.master`'s ID). Else it uses `leftStrip(strips: mixer.snapshot.strips, destinations: mixer.destinations)`, where only `.strip(id)` proceeds. Then `mixer.browser = .insert(strip: id)` and `mixer.pick(p)`.
  - `pruneLayout` drops `controllers` keys whose lane is gone, inside its existing single `updateLayout`. Its early `guard` gains a `controllersGone` term, so a layout whose only stale part is a `controllers` key is still pruned.

- [ ] **Step 4: Run them and see them pass;** suite 1027 (1020 + 7).

- [ ] **Step 5: Commit** the five model files and the test file. Message: "app model: the right pane's markers, plug-in insert and controllers chart (plan 7a Task 4)", with the trailers.

---

### Task 5: The registry's `ui.*` commands and `state.get layout`

**Files:**
- Modify: `crates/core-control/src/registry.rs`, `crates/core-control/src/request.rs`, `crates/core-control/tests/commands.rs`, `crates/core-ffi/src/control.rs`, `crates/core-ffi/tests/control.rs`, `app/Sources/VizDAWAppModel/ControlCommands.swift` (one stub case)

**Interfaces:**
- Consumes: `cmd`, `key_only`, `ParamSpec`, `ParamKind::{Choice, Flag, Int}`, the existing `ON` params, `StateTarget`, `StateTargetRow`, `ControlResult`, `result_json`.
- Produces:
  - registry entries, in a new block after `window.media_pool`, all `document: true`, menu "View":
    - `ui.view_arrange` "Arrange View" `⌃1`;
    - `ui.view_mix` "Mix View" `⌃2`;
    - `ui.toggle_left` "Left Pane" `⌃⌘←`, `ui.toggle_bottom` "Bottom Pane" `⌃⌘↓` and `ui.toggle_right` "Right Pane" `⌃⌘→`, each with `params: ON`;
    - `key_only(cmd("ui.pane", "View", "Set Pane", None, true))`, with `params: UI_PANE_PARAMS`.
  - `const UI_PANE_PARAMS: &[ParamSpec]`:
    - `pane`: `Choice(&["left", "bottom", "right"])`, required;
    - `view`: `Choice(&["strip", "sequences", "inspector", "piano_roll", "list", "strip_chart", "drum_grid", "mixer", "controllers", "media_pool", "plugins", "markers"])`;
    - `shown`: `Flag`;
    - `size`: `Int { min: 0, max: 10_000 }`;
    - `main`: `Choice(&["arrange", "mix"])`.

    Each has a one-line `help`.
  - `StateTarget::Layout` (doc: "Its window's main view and panes (plan 7a)"), parsed from `"layout"`. The error text gains it: "`target` must be transport, document, editor, selection, windows, media, project, record, input, automation, surfaces or layout".
  - In core-ffi:
    - `StateTargetRow::Layout`;
    - `pub struct PaneRow { pub pane: String, pub shown: bool, pub view: String, pub size: f64, pub views: Vec<String> }`;
    - `pub struct ViewPanesRow { pub main: String, pub panes: Vec<PaneRow> }`;
    - `pub struct LayoutStateRow { pub main: String, pub views: Vec<ViewPanesRow>, pub left_strip: Option<u64>, pub left_shows: String }`;
    - `ControlResult::Layout { doc: String, layout: LayoutStateRow }`;
    - its JSON in `result_json` in Ruling 10's shape.
  - Swift: `ControlStates.state`'s switch gains `case .layout: return nil` (filled by Task 6), so the app still builds.

- [ ] **Step 1: Write the failing tests.** In `crates/core-control/tests/commands.rs`:

```rust
/// Plan 7a Task 5: the view and pane commands refuse hostile params by type and range.
#[test]
fn ui_commands_check_their_params() {
    let run = |id: &str, params: Value| json!({ "id": id, "params": params });
    for good in [
        json!({ "pane": "left" }),
        json!({ "pane": "bottom", "view": "piano_roll", "shown": true, "size": 300, "main": "mix" }),
        json!({ "pane": "right", "size": 0 }),
    ] {
        assert!(matches!(ok("command.run", run("ui.pane", good.clone())), Command::CommandRun { .. }), "{good}");
    }
    for bad in [
        json!({}),
        json!({ "pane": "top" }),
        json!({ "pane": "left", "view": "Mixer" }),
        json!({ "pane": "left", "view": 3 }),
        json!({ "pane": "left", "shown": "yes" }),
        json!({ "pane": "left", "size": 1e12 }),
        json!({ "pane": "left", "size": -1 }),
        json!({ "pane": "left", "size": "300" }),
        json!({ "pane": "left", "main": "edit" }),
        json!({ "pane": "left", "width": 300 }),
        json!({ "pane": "x".repeat(70_000) }),
    ] {
        assert_eq!(err("command.run", run("ui.pane", bad.clone())).0, ErrorCode::InvalidParams, "{bad}");
    }
    for id in ["ui.view_arrange", "ui.view_mix"] {
        assert_eq!(err("command.run", run(id, json!({ "on": true }))).0, ErrorCode::InvalidParams, "{id}");
    }
    for id in ["ui.toggle_left", "ui.toggle_bottom", "ui.toggle_right"] {
        assert!(matches!(ok("command.run", run(id, json!({ "on": false }))), Command::CommandRun { .. }));
        assert_eq!(err("command.run", run(id, json!({ "on": "no" }))).0, ErrorCode::InvalidParams, "{id}");
    }
    let keys: Vec<_> = ["ui.view_arrange", "ui.view_mix", "ui.toggle_left", "ui.toggle_bottom", "ui.toggle_right"]
        .iter().map(|id| COMMANDS.iter().find(|c| c.id == *id).unwrap().shortcut).collect();
    assert_eq!(keys, [Some("⌃1"), Some("⌃2"), Some("⌃⌘←"), Some("⌃⌘↓"), Some("⌃⌘→")]);
    assert!(!COMMANDS.iter().find(|c| c.id == "ui.pane").unwrap().menu_item);
}

#[test]
fn state_get_reads_layout() {
    assert_eq!(
        ok("state.get", json!({ "target": "layout" })),
        Command::StateGet { target: StateTarget::Layout, window: None }
    );
    let (code, msg) = err("state.get", json!({ "target": "panes" }));
    assert_eq!(code, ErrorCode::InvalidParams);
    assert!(msg.contains("surfaces or layout"), "{msg}");
}
```

Change the registry count test's `147` to `153`, with the comment "plan 7a: six ui.* commands".

In `crates/core-ffi/tests/control.rs`:

```rust
#[test]
fn layout_result_json() {
    let pane = |p: &str, v: &str| PaneRow { pane: p.into(), shown: p != "right", view: v.into(), size: 220.0,
                                            views: vec![v.into()] };
    let j = result_json(&ControlResult::Layout {
        doc: "D".into(),
        layout: LayoutStateRow {
            main: "arrange".into(), left_strip: Some(3), left_shows: "strip".into(),
            views: vec![ViewPanesRow { main: "arrange".into(),
                                       panes: vec![pane("left", "strip"), pane("bottom", "piano_roll"), pane("right", "markers")] }],
        },
    });
    assert_eq!(j["doc"], "D");
    assert_eq!(j["main"], "arrange");
    assert_eq!(j["left_strip"], 3);
    assert_eq!(j["left_shows"], "strip");
    assert_eq!(j["views"][0]["panes"][1], json!({"pane": "bottom", "shown": true, "view": "piano_roll", "size": 220.0, "views": ["piano_roll"]}));
    assert_eq!(j["views"][0]["panes"][2]["shown"], false);
}
```

- [ ] **Step 2: Run them and see them fail:** `cargo test -p core-control --test commands ui_commands state_get_reads_layout` and `cargo test -p core-ffi --test control layout_result_json`.

- [ ] **Step 3: Implement** the entries, the parse and the `core-ffi` types, the mapping in `control.rs`'s `Command::StateGet` match (`StateTarget::Layout => StateTargetRow::Layout`) and the JSON arm. Then add the Swift stub case.

- [ ] **Step 4: Run the gates.**
  - fmt, both clippies and `cargo test --workspace`. Expected: 1996 passed / 18 ignored on macOS (1993 + 3).
  - `ControlCommandsTests`' check that `handled` covers every registry ID would fail on the six new IDs. So that this task ends green, add them to `ControlCommands.other` with a case answering `unavailable` "Not yet." (Task 6 removes them).
  - `scripts/build-ffi.sh && scripts/swift.sh build && scripts/swift.sh test`. Expected: 1027, all passing.

- [ ] **Step 5: Commit** the five Rust files and `ControlCommands.swift`. Message: "control: the ui.* view and pane commands and state.get layout (plan 7a Task 5)", with the trailers. Counts: Rust +3, registry 153.

---

### Task 6: `ViewCommands`: menus, keys and the channel

**Files:**
- Create: `app/Sources/VizDAWAppModel/ViewCommands.swift`, `app/Tests/VizDAWAppModelTests/ViewCommandsTests.swift`
- Modify: `app/Sources/VizDAWAppModel/ControlCommands.swift` (remove Task 5's six IDs from `other`; `handled` gains `ViewCommands.ids`; route them; `ControlStates.state` `.layout`), `app/Sources/VizDAWAppModel/TransportCommands.swift` (add `extension ViewCommands: MenuCommands {}` beside the other two), `app/Sources/VizDAWAppModel/DocumentModel.swift` (`storeMixer`), `app/Sources/VizDAWAppModel/DocumentStore.swift` (`mixer(_:)` sets it)

**Interfaces:**
- Consumes: Task 1's intents (`openDetail(_:kind:in:)` included), `EditorModel.detailTarget(shown:)`, `StripFocus` and `leftStrip` (Task 3), `laneOnStrip10()` (Task 4), the `[ControlArg]` readers (`args.text`, `args.flag`, `args.int`; use the tree's names), `ControlOutcome`, `ControlResult.ran`, `ControlFailure.of`, `KeyOutcome`, `MenuCommands`, `DocumentModel.commitRename(_:)` / `renameDraft` / `renamingSeq`, `EditorModel.endTextEditing(prefix:)`, `MixerModel.commitRename()`, `stripRow` (`FakeSession.swift`).
- Produces:
  - `@MainActor public enum ViewCommands`:
    - `static let ids: Set<String>`, the six IDs;
    - `static func enabled(_ id: String, _ m: DocumentModel) -> Bool`, always true;
    - `static func isOn(_ id: String, _ m: DocumentModel) -> Bool`: the current main view for the two view IDs, the current setup's `shown` for the three toggles;
    - `static func run(_ id: String, on: Bool?, _ m: DocumentModel) -> KeyOutcome`. For `ui.view_*` it ignores `on` (Ruling 4);
    - `static func runPane(_ args: [ControlArg], _ m: DocumentModel, doc: UUID) -> ControlOutcome`, which answers `.done(.ran(command: "ui.pane", doc: doc.uuidString, window: nil, undoLabel: m.snapshot.undoLabel, status: text))` itself (Ruling 10);
    - `static func layoutRow(_ m: DocumentModel, strips: [StripRow], destinations: [DestRow]) -> LayoutStateRow`;
    - `static func viewName(_ side: PaneSide, _ state: PaneState, _ layout: EditorLayout) -> String`;
  - `extension DocumentModel { public func endPaneEdits() }` in `ViewCommands.swift` (Ruling 10), with `@ObservationIgnored public weak var storeMixer: MixerModel?` on `DocumentModel`, set by `DocumentStore.mixer(_:)` when it makes the document's mixer. The menu path (`commandToggle`) passes only the `DocumentModel`, so the mixer is reached through it;
  - `ControlStates.state(.layout, id, m)` returns `.layout(doc:, layout: ViewCommands.layoutRow(m, strips: (try? m.session.mixer().strips) ?? [], destinations: (try? m.session.destinations()) ?? []))`.

- [ ] **Step 1: Write the failing tests** in `ViewCommandsTests.swift`. They run through `ControlCommands.run` with the existing `FakeControlHost` and a store document, as `ExternalCommandsTests` does. `storeDocument()` makes a `DocumentStore` whose demo is `laneOnStrip10()` and returns `(doc: (id: UUID, model: DocumentModel), store: DocumentStore)`.

`ControlArg` is a struct (`ControlArg(name:value:)`), so the file starts with a file-private extension for the literals below:

```swift
fileprivate extension ControlArg {
    static func text(_ n: String, _ v: String) -> ControlArg { ControlArg(name: n, value: .text(value: v)) }
    static func flag(_ n: String, _ v: Bool) -> ControlArg { ControlArg(name: n, value: .flag(value: v)) }
    static func int(_ n: String, _ v: Int64) -> ControlArg { ControlArg(name: n, value: .int(value: v)) }
}
```

```swift
@Test func viewKeysSwitchTheMainView() {
    let (doc, store) = storeDocument()
    #expect(run("ui.view_mix", [], doc, store).isDone)
    #expect(doc.model.editor.layout.views.main == .mix)
    #expect(ViewCommands.isOn("ui.view_mix", doc.model))
    _ = run("ui.view_arrange", [], doc, store)
    #expect(doc.model.editor.layout.views.main == .arrange)
    _ = ViewCommands.run("ui.view_mix", on: true, doc.model)
    _ = ViewCommands.run("ui.view_mix", on: false, doc.model)   // the checked menu item chosen again
    #expect(doc.model.editor.layout.views.main == .mix)
}
@Test func paneTogglesShowAndHide() {
    let (doc, store) = storeDocument()
    _ = run("ui.toggle_right", [], doc, store)
    #expect(doc.model.editor.layout.views.arrange.right.shown)
    _ = run("ui.toggle_right", [.flag("on", true)], doc, store)
    #expect(doc.model.editor.layout.views.arrange.right.shown)
    _ = run("ui.toggle_left", [.flag("on", false)], doc, store)
    #expect(!doc.model.editor.layout.views.arrange.left.shown)
}
@Test func uiPaneSetsViewShownAndSize() {
    let (doc, store) = storeDocument()
    let r = run("ui.pane", [.text("pane", "left"), .text("view", "sequences"), .int("size", 9_000)], doc, store)
    #expect(r.status == "left sequences 480 shown")
    #expect(doc.model.editor.status == nil)   // the answer carries it, not the status line
    _ = run("ui.pane", [.text("pane", "bottom"), .text("view", "list")], doc, store)
    #expect(doc.model.editor.layout.views.arrange.bottom.view == .editor)
    #expect(doc.model.editor.layout.lastKind == .list)
    _ = run("ui.pane", [.text("pane", "right"), .text("view", "markers"), .flag("shown", true), .text("main", "mix")], doc, store)
    #expect(doc.model.editor.layout.views.main == .arrange)
    #expect(doc.model.editor.layout.views.mix.right == PaneState(shown: true, view: .markers, size: 260))
}
@Test func uiCommandsCheckTheirArguments() {
    let (doc, store) = storeDocument()
    let before = doc.model.editor.layout
    for args: [ControlArg] in [
        [.text("pane", "left")],
        [.text("pane", "left"), .text("view", "mixer")],
        [.text("pane", "right"), .text("view", "piano_roll")],
        [.text("pane", "bottom"), .text("view", "mixer"), .text("main", "mix")],
        [.text("pane", "middle"), .flag("shown", true)],
        [.text("pane", "left"), .text("main", "edit"), .flag("shown", true)],
    ] {
        #expect(run("ui.pane", args, doc, store).code == .invalidParams, "\(args)")
    }
    #expect(run("ui.pane", [.text("pane", "right"), .text("view", "piano_roll")], doc, store).message
        == "piano_roll is not on the right pane's list in the arrange view.")
    #expect(doc.model.editor.layout == before)   // no detail opened, lastKind and bottom.shown unchanged
    #expect(ControlCommands.run("ui.view_mix", args: [], doc: nil, store: store, host: FakeControlHost()).code == .noDocument)
}
@Test func stateGetLayoutReportsBothViewsAndTheLeftStrip() {
    let (doc, store) = storeDocument()   // laneOnStrip10(): lane 30 plays through strip 10
    selectFirstLane(doc.model)
    let strips = [stripRow(10, "Out 1", kind: .audioPath(path: 7))]
    let row = ViewCommands.layoutRow(doc.model, strips: strips, destinations: [])
    #expect(row.main == "arrange")
    #expect(row.views.map(\.main) == ["arrange", "mix"])
    #expect(row.views[0].panes.map(\.pane) == ["left", "bottom", "right"])
    #expect(row.views[0].panes[1].view == "piano_roll")
    #expect(row.views[1].panes[1].views == ["piano_roll", "list", "strip_chart", "drum_grid"])
    #expect(row.leftStrip == 10 && row.leftShows == "strip")
    _ = run("ui.toggle_left", [.flag("on", false)], doc, store)
    let hidden = ViewCommands.layoutRow(doc.model, strips: strips, destinations: [])
    #expect(hidden.leftShows == "hidden" && hidden.leftStrip == 10)
}
@Test func switchingAViewEndsTextEdits() throws {
    let (doc, store) = storeDocument()
    let m = doc.model
    let mixer = try #require(store.mixer(doc.id))
    m.beginRename(try #require(m.selectedSeq))
    m.editor.beginTextEditing("lane-name")
    mixer.beginRename(try #require(mixer.strips.first), in: .strip)
    #expect(m.textFieldHasKeyboard)
    _ = run("ui.toggle_left", [], doc, store)
    #expect(m.renamingSeq == nil && !m.editor.textEditing && mixer.renamingStrip == nil)
    #expect(!m.textFieldHasKeyboard)
}
@Test func mainViewKeysDoNotCollideWithTheEditorKeys() {
    let keys = KeyBindings(defaults: MemoryDefaults())
    #expect(keys.row("ui.view_arrange")?.menuKey == "1" && keys.row("ui.view_arrange")?.menuModifiers.control == true)
    #expect(keys.row("editors.piano_roll")?.menuModifiers.command == true)
    #expect(keys.row("editors.piano_roll")?.menuModifiers.control == false)
}
```

`run`, `.isDone`, `.status`, `.code` and `.message` are small file-private helpers over `ControlOutcome`, and `selectFirstLane` a file-private copy of Task 4's. Reuse `ExternalCommandsTests`' helpers if they are shared.

- [ ] **Step 2: Run them and see them fail** (`--filter ViewCommandsTests`).

- [ ] **Step 3: Implement.**
  - `runPane` steps, in this order (Ruling 10), so that nothing changes before every check has passed:
    1. Parse `pane`, `view`, `shown`, `size` and `main`. Core-control has already checked types, so the app checks only that `pane` and `main` are known, and refuses unknown values as `invalid_params`.
    2. Refuse a call with none of `view`, `shown` and `size`.
    3. Map an editor kind name to `.editor`. Refuse a view with `!ViewLayouts.views(side, in: main).contains(mapped)` as `invalid_params`, with the message naming the channel name the caller sent.
    4. Call `m.endPaneEdits()`, then `setPane`.
    5. Only then, for an editor kind: `openDetail(t, kind:, in: main)` on `t = layout.detail?.target ?? editor.detailTarget(shown: m.selectedSeq)` (Ruling 3's fallback to the selection or sequence). With no target, set `lastKind` alone through `updateLayout`. If the call gave `shown`, that value wins over `openDetail`'s "show the bottom pane" (`setPaneShown(.bottom, shown, in: main)` again).
  - `ViewCommands.run` calls `m.endPaneEdits()` before it switches a view or toggles a pane.
  - The answer status is `"\(pane) \(viewName) \(Int(size)) \(shown ? "shown" : "hidden")"`, returned in `.ran`'s `status`; `editor.status` stays nil.
  - `layoutRow` follows Ruling 10's `left_shows` and `left_strip` rules.
  - `ControlCommands.run` dispatches `ViewCommands.ids` like `TransportCommands.ids`, through the existing `KeyOutcome`-to-`ControlOutcome` path, with `ui.pane` going to `runPane`.

- [ ] **Step 4: Run them and see them pass;** suite 1034 (1027 + 7), with `ControlCommandsTests`' `handled` check passing.

- [ ] **Step 5: Commit** the five Swift files and the test file. Message: "app model: ViewCommands for the View menu, keys and ui.*, and state.get layout (plan 7a Task 6)", with the trailers.

---

### Task 7: The single window: shell, transport bar, Mix view, mixer window bar, View menu

**Files:**
- Create: `app/Sources/VizDAW/DocumentShell.swift`
- Modify:
  - `app/Sources/VizDAW/DocumentView.swift`: its body's `NavigationSplitView { … } detail: { … }` becomes `DocumentShell`; `.toolbar { TransportToolbar(model: model) }` is removed; `TransportToolbar: ToolbarContent` becomes `DocumentTransportBar: View`;
  - `app/Sources/VizDAW/ArrangementPane.swift`: remove the `TransportBar(model: model)` and `StatusLine(editor: model.editor)` lines from `ArrangementPane.body`. `TransportBar` and `StatusLine` stay defined there;
  - `app/Sources/VizDAW/DetailPane.swift`: `EditorSplit` is deleted, because `DocumentShell` replaces it. `DetailArea` stays. `DetailPane` gains `showsKindPicker: Bool` (Ruling 3): `DetailArea` passes false, `EditorWindows`' torn windows keep the picker (the parameter defaults to true);
  - `app/Sources/VizDAW/MixerView.swift`: `MixerWindow.content` puts the transport bar above `MixerView`; `MixerView(model:placement:)`; `.toolbar { … }` becomes `MixerBar`; the `.task` loop calls `model.tick(now:)`; the maps sheet goes through `learn.showsMaps(in:)` and `closeMaps(from:)`; StripView's "MIDI Learn Maps…" calls `learn.showMaps(in: placement)`, with `placement` read from `@Environment(\.mixerPlacement)`; the insert and Instruments popovers and the rename field go through the placements (Ruling 8); `onDisappear` calls `model.disappeared(placement)` instead of `endOpenDrag()`; `.learnAlert` only when `\.learnAlertHosted` is false;
  - `app/Sources/VizDAW/StripChartPane.swift`: `StripChartLearnAlert` only when `\.learnAlertHosted` is false (Ruling 8);
  - `app/Sources/VizDAW/DocumentWindow.swift`: `WindowFrameKeeper`;
  - `app/Sources/VizDAW/AppCommands.swift`: the View menu group.

**Interfaces:**
- Consumes: Tasks 1, 2, 3 and 6 (`ViewCommands.run` and `isOn` for the menu toggles and the bar's tabs and toggles; `PaneGeometry.frames`, `drag` and `sanitizedFrame`; `MixerModel.tick`; `MixerPlacement`).
- Produces (the `VizDAW` target, used by Task 8):
  - `struct DocumentShell: View { let model: DocumentModel; let docID: UUID }`, which lays out the transport bar, the banners, the regions and `StatusLine`;
  - `struct PaneFrame<Content: View>: View { let side: PaneSide; let model: DocumentModel; @ViewBuilder let content: () -> Content }`. It shows the view picker (a menu-style `Picker` labelled with the pane's `title`) and the content. The left and right pickers list `ViewLayouts.views(side, in: main)`'s titles. The bottom picker lists `DetailEditorKind.allCases`' titles, then "Mixer" (Arrange only), with the kind selected when the view is `.editor` (Ruling 3). A choice calls `model.endPaneEdits()` first, then `setPane`, and for a kind `openDetail` (or sets `lastKind` with no target), as `runPane` does. Task 8 fills the content through `PaneHost(side:model:docID:)`;
  - `struct PaneHost: View { let side: PaneSide; let model: DocumentModel; let docID: UUID }`. In Task 7 it shows `DetailArea` for the bottom pane's `.editor`, and a `ContentUnavailableView` with the view's title for every other view; Task 8 replaces those;
  - `struct DocumentTransportBar: View { @Bindable var model: DocumentModel; let docID: UUID; let showsViews: Bool }`;
  - `struct MixerBar: View { @Bindable var model: MixerModel; let placement: MixerPlacement }`;
  - `EnvironmentValues.mixerPlacement: MixerPlacement`, default `.window`, through `struct MixerPlacementKey: EnvironmentKey`;
  - `EnvironmentValues.learnAlertHosted: Bool`, default false, through `struct LearnAlertHostedKey: EnvironmentKey`;
  - `struct PaneDivider: View { let side: PaneSide; let model: DocumentModel; let size: CGSize }`, holding `dragStart` and the live size in spelled-out `State(initialValue:)`;
  - `struct WindowFrameKeeper: NSViewRepresentable { let model: DocumentModel }`.

- [ ] **Step 1: Build the shell.**
  - `DocumentShell` uses `GeometryReader`. It lays out `PaneGeometry.frames(in:setup: model.editor.layout.views.current)` with `.frame` and `.position` (or a `ZStack` with offsets), and puts a 1-point `PaneDivider` on each shown pane's inner edge.
  - A divider's `DragGesture` (Ruling 2): on the first change it stores `dragStart` (the side's saved size); each change sets the live size to `PaneGeometry.drag(setup with that side's size = dragStart, side:, by: translation, in:)`, with `-translation.height` for the bottom divider; `onEnded` calls `model.editor.setPane(side, view: nil, shown: nil, size: live, in: nil)` once and clears `dragStart`.
  - The centre, in a `ZStack` (Ruling 9):
    - Arrange: `ArrangementPane(model:)`, always mounted. While the Mix view shows, it has `.opacity(0)`, `.allowsHitTesting(false)` and `.accessibilityHidden(true)`;
    - Mix: `if let mixer = appStore.mixer(docID) { MixerView(model: mixer, placement: .view) }`, as `MixerWindow` does, else a `ContentUnavailableView` (`appStore.mixer` has already put "Mixer Unavailable" in `doc.alert`).
  - Each shown pane is `PaneFrame(side:model:) { PaneHost(side:model:docID:) }`. The bottom pane's `DetailArea` stays mounted while the pane is hidden, with zero size and `.accessibilityHidden(true)` (Ruling 3).
  - At its root, `DocumentShell` sets `.environment(\.openPluginWindow, { slot in PluginWindows.show(appStore, doc: docID, slot: slot) })` (Ruling 8: the Mix view, the bottom pane's mixer and the left strip), `.environment(\.learnAlertHosted, true)` with one `.learnAlert(model.learn, shown: !model.learn.showsMaps)`, and `.frame(minWidth: 700, minHeight: 450)` (Ruling 11).

- [ ] **Step 2: Build the transport bar** (Ruling 9).
  - Move `TransportToolbar`'s three item groups into one `HStack`, in their order, with `TransportBar(model:)` after the position.
  - With `showsViews`, add:
    - a segmented `Picker("Main View", …)` over `MainView.allCases`, titled "Arrange" and "Mix", bound through `ViewCommands.run("ui.view_*", on: nil, model)`;
    - three `Toggle`s with `.toggleStyle(.button)`, with SF Symbols `sidebar.left`, `rectangle.bottomthird.inset.filled` and `sidebar.right`, each with `.help("Left Pane (⌃⌘←)")` and so on, and the accessibility labels "Left Pane", "Bottom Pane" and "Right Pane".
  - The faults popover is attached only with `showsViews` true (Ruling 8), so `showsFaults` opens one popover, in the document window. The mixer window's bar (`showsViews: false`) leaves the faults button out, because without its popover it would do nothing.
  - `DocumentView` keeps every `.sheet` and `.alert` it has today, unchanged, attached to `DocumentShell`.

- [ ] **Step 3: Rework the mixer for three placements** (Ruling 8).
  - `MixerWindow.content` becomes `VStack(spacing: 0) { DocumentTransportBar(model: mixer.document, docID: id, showsViews: false); Divider(); MixerView(model: mixer, placement: .window) }`, keeping its `.environment`, `.focusedSceneValue` and `.navigationTitle`.
  - `MixerBar` holds today's toolbar items in an `HStack`, right-aligned, with the instrument popover still attached to the Instruments menu. `MixerBar` takes the placement too (`MixerBar(model:placement:)`): its Instruments items call `model.openBrowser(_:in: placement)`, and its popover shows on `model.showsBrowser(in: placement) && model.browser?.wantsInstrument == true`.
  - StripView's "+" calls `model.openBrowser(.insert(strip:), in: placement)`, and its popover binds `model.showsBrowser(in: placement) && model.browser == .insert(strip:)`. Its rename calls `model.beginRename(strip, in: placement)`, and the field shows on `model.isRenaming(strip.id, in: placement)`.
  - `MixerView` calls `model.appeared(placement)` on appear and `model.disappeared(placement)` on disappear, replacing today's `onDisappear { model.endOpenDrag() }`.
  - The `.task` loop becomes `while !Task.isCancelled { model.tick(now: ProcessInfo.processInfo.systemUptime); try? await Task.sleep(for: .milliseconds(33)) }`.
  - `MixerView.body` sets `.environment(\.mixerPlacement, placement)` on its strips.
  - `.learnAlert` on `MixerView`, and `StripChartLearnAlert` on `StripChartEditor`, apply only when `\.learnAlertHosted` is false: the mixer window and torn windows keep theirs, and the document window has the one on `DocumentShell` (Ruling 8).

- [ ] **Step 4: Add the View menu** inside `AppCommands`' existing `Group { … }`:

```swift
CommandGroup(before: .toolbar) {
    ForEach(["ui.view_arrange", "ui.view_mix"], id: \.self) { commandToggle($0, ViewCommands.self) }
    Divider()
    ForEach(["ui.toggle_left", "ui.toggle_bottom", "ui.toggle_right"], id: \.self) { commandToggle($0, ViewCommands.self) }
    Divider()
}
```

- [ ] **Step 5: Add `WindowFrameKeeper`** (Ruling 11), attached in `DocumentWindow` beside `WindowTag`.
  - It applies the frame once, on the next main-queue turn after `viewDidMoveToWindow` (`DispatchQueue.main.async`), so that SwiftUI's own restoration does not overwrite it, through `PaneGeometry.sanitizedFrame(model.editor.layout.views.frame, screens: NSScreen.screens.map(\.frame))`.
  - It observes `NSWindow.didMoveNotification` and `NSWindow.didEndLiveResizeNotification` for that window (`object: w`), with `queue: .main`, and saves through `model.editor.setDocumentFrame`.
  - **Swift 6.1:** the observer closure captures only `let editor = model.editor`, a strong `let` made before `MainActor.assumeIsolated { … }`, and reads the window inside as `note.object as? NSWindow`. It captures no `NSWindow`, which would pin the window until dismantle, and never a weak `self`. The deferred apply follows the same pattern.
  - The observers are removed in `dismantleNSView`.

- [ ] **Step 6: Run the gates.** `scripts/build-ffi.sh && scripts/swift.sh build && scripts/swift.sh test` (1034, no new warnings), then `scripts/make-app.sh`. Run the existing e2e now (`VIZDAW_E2E_APP=dist/VizDAW.app cargo test -p vizctl --test e2e -- --ignored --test-threads 1`). Expected: 14 passed. `click_play_and_press_space` finds Play in the transport bar. A failure here is a layout fault to fix in this task, not in the test.

- [ ] **Step 7: Commit** the eight `VizDAW` files. Message: "app: one window per document: the transport bar, layout A's panes, the Mix view, the mixer window's transport, the View menu, the saved frame (plan 7a Task 7)", with the trailers.

---

### Task 8: The pane contents

**Files:**
- Create: `app/Sources/VizDAW/PaneViews.swift`
- Modify: `app/Sources/VizDAW/DocumentShell.swift` (`PaneHost` switches to the views below), `app/Sources/VizDAW/MediaPoolWindow.swift` (`MediaPoolView.compact`), `app/Sources/VizDAW/PluginBrowserView.swift` (`width`)

**Interfaces:**
- Consumes: Tasks 3, 4 and 7 (`MixerModel.appeared` / `disappeared`, `\.mixerPlacement`, `\.learnAlertHosted`, the root `openPluginWindow`), `SequenceSidebar(model:)`, `DetailArea(model:docID:)`, `StripView(model:strip:)`, `StripChartEditor(model:)`, `MediaPoolView(media:)`, `PluginBrowserView(model:pick:)`, `PluginBrowserModel(kind:_:)`, `appPlugins.browser(_:)`, `appStore.mixer(_:)`.
- Produces:
  - `struct LeftStripPane: View { let model: DocumentModel; let docID: UUID }`. It gets its mixer with `if let mixer = appStore.mixer(docID)` (else a `ContentUnavailableView`, as the Mix view does). It shows `StripView` for `.strip(id)`, `InspectorPane` for `.inspector`, and `StripFocus.nothingMessage` for `.nothing`. It sets `.environment(\.mixerPlacement, .strip)`, attaches the maps sheet for `.strip`, and calls `mixer.appeared(.strip)` / `disappeared(.strip)`. It polls with `mixer.tick(now:)` every 33 ms only while its window is key or the document plays or records: a `.task(id:)` over the window's `controlActiveState == .key` and `model.isPlaying || model.record.isRecording`, as `DocumentWindow`'s `PollKey` (Ruling 8);
  - `struct InspectorPane: View { let model: DocumentModel; let docID: UUID }`, a two-column `Grid` of `Inspector.rows(…)`, or `Inspector.emptyMessage`;
  - `struct MarkersPane: View { let model: DocumentModel }`, a `List` of `MarkerList.rows`. A row's click calls `MarkerList.go`. With no rows it says "No markers or locators.";
  - `struct ControllersPane: View { let model: DocumentModel }`, `StripChartEditor(model: model.controllersChart)`, or "Select a lane to see its controllers.";
  - `struct PluginsPane: View { let model: DocumentModel; let docID: UUID }`, `PluginBrowserView(model:…, width: nil)` for effects. It keeps its own `PluginBrowserModel(kind: .effect) { appPlugins.browser(.effect) }` in a spelled-out `State`, never `mixer.browserModel(.effect)`, whose cache `pick` clears (Ruling 7). A pick puts `PaneContent.insertPlugin`'s refusal in `model.editor.status`;
  - `MediaPoolView(media:compact:)`, where `compact` defaults to false; compact drops both `minWidth` 620 and `minHeight` 280 (Ruling 7);
  - `PluginBrowserView(model:width:pick:)`, where `width` defaults to 320.

- [ ] **Step 1: Write the views.** Each `PaneView` gets exactly one host:

  | `PaneView` | Content |
  |---|---|
  | `.strip` | `LeftStripPane` |
  | `.sequences` | `SequenceSidebar` |
  | `.inspector` | `InspectorPane` |
  | `.editor` | `DetailArea` |
  | `.mixer` | `if let mixer = appStore.mixer(docID)`: `MixerView(model: mixer, placement: .pane)` |
  | `.controllers` | `ControllersPane` |
  | `.mediaPool` | `MediaPoolView(media: model.media, compact: true)` |
  | `.plugins` | `PluginsPane` |
  | `.markers` | `MarkersPane` |

  - A click in a plug-in slot opens its plug-in window as today (spec §2 item 4), in every placement, through the `openPluginWindow` that `DocumentShell` sets at its root (Task 7). No pane sets its own.
  - The media pool's Delete, Rename and Relink stay on their buttons and menu, which open their panels as today. No programmatic path reaches them.

- [ ] **Step 2: Run the gates:** Swift build and test (1034), `scripts/make-app.sh`, then the e2e (14 passed).

- [ ] **Step 3: Commit** the four files (`PaneViews.swift`, `DocumentShell.swift`, `MediaPoolWindow.swift`, `PluginBrowserView.swift`). Message: "app: the panes' views: strip, sequences, inspector, editors, mixer, controllers, media pool, plug-ins, markers (plan 7a Task 8)", with the trailers.

---

### Task 9: The e2e, the by-eye steps, the root README and the full gates

**Files:**
- Modify: `crates/vizctl/tests/e2e.rs`, `app/README.md`, `README.md`, `docs/feature-backlog.md`

**Interfaces:**
- Consumes: everything above. From the e2e at `38c231d`: `App::start`, `app.call`, `app.call_err`, `app.wait`, `play_button`, `WAIT_MS`, and the `ui.key` modifier list as `keys_drive_step_entry_locators_and_bindings` passes it.

- [ ] **Step 1: Write the e2e** `window_views_and_panes_over_the_channel`, marked `#[ignore = "needs a window server and VIZDAW_E2E_APP"]`.
  1. `app.new_demo`. `state.get layout` matches Ruling 1's defaults: `main == "arrange"`; arrange's left `{shown: true, view: "strip"}`; bottom shown; right hidden; mix's three panes hidden.
  2. Record `state.get windows`' window numbers and kinds.
     - Hostile calls, each with its expected code and no layout change:
       - `ui.pane {}` → `invalid_params`;
       - `{pane: "left", view: "mixer"}` → `invalid_params`;
       - `{pane: "bottom", view: "mixer", main: "mix"}` → `invalid_params`;
       - `{pane: "left", size: "wide"}` → `invalid_params`;
       - `{pane: "left"}` → `invalid_params` (no field);
       - `{pane: "right", view: "piano_roll"}` → `invalid_params`, and the message names `piano_roll` (I3: a refused editor kind opens no detail);
       - `state.get {target: "panes"}` → `invalid_params`.
     - After them `state.get layout` is unchanged, `state.get windows` lists the same numbers and kinds, its `alerts` is empty, and `state.get document`'s `alert` is null: no dialog opened.
  3. `ui.key` `"2"` with `["control"]`, then wait until `state.get layout`'s `main == "mix"`. `ui.key` `"1"` with `["control"]` brings back `"arrange"`. Then `command.run ui.toggle_right`, and arrange's right is shown.
  4. `ui.pane {pane: "right", view: "markers", size: 300, shown: true, main: "mix"}` answers `status == "right markers 300 shown"`. `main` is still `"arrange"`.
  5. `ui.pane {pane: "bottom", view: "mixer"}`: arrange's bottom view is `"mixer"`. `ui.tree` of the document window holds a node labelled like the mixer's "Add Bus" button.
  6. `command.run window.mixer`: a `kind == "mixer"` window appears. `play_button` finds Play in **its** tree. `ui.click` it, wait `playing` on the document window, then `transport.play_stop`, and wait `stopped`. The mixer window's own transport bar drives the same transport.
  7. `command.run file.save_as {path: <throwaway home>/views.vizdaw}`, `file.close`, then `app.open` that path. `state.get layout` shows step 4's mix right pane `{shown: true, view: "markers", size: 300.0}` and step 5's bottom `"mixer"`: saved per document.
  8. `app.quit()`.

- [ ] **Step 2: Run it and see it pass** (it was RED until Tasks 7–8 existed; run it against the Task 8 build first and record the result in the report). Then run all 15 e2e tests.

- [ ] **Step 3: Write README steps 199–206** in `app/README.md`, under a new section after "Kontrol S-Series MK3 keyboards (plan 5f-4)":

```markdown
### Window, main views and panes (plan 7a)

Checked by hand; the layout rules are tested (`WindowLayoutTests`,
`PaneGeometryTests`, `LeftPaneTests`, `RightPaneTests`, `ViewCommandsTests`)
and the e2e `window_views_and_panes_over_the_channel`. Where earlier steps
say the sidebar, use Left Pane › Sequences; the toolbar is now the
transport bar; the mixer's toolbar is the bar along the top of the mixer;
the detail pane is the bottom pane's editor.

199. File > Create Demo Project. The window shows the transport bar along
     the top (Play, Loop, Record, the position, Arrange | Mix, three pane
     buttons), the channel strip on the left running the full height, the
     arrangement in the centre and the "No Editor Open" pane below it. The
     right pane is hidden.
200. Scroll the arrangement to bar 9 and zoom in once. Press ⌃2: the
     mixer fills the window under the transport bar, with no arrangement.
     Press ⌃1: the arrangement comes back where you left it (bar 9, the
     same zoom), with the panes as they were. (If macOS switches desktops
     instead, Mission Control owns ⌃1/⌃2: rebind Arrange View and Mix View
     in Settings › Keys › View.) A sequence's keyboard slot on ⌃1, ⌃2 or
     ⌃⌘←/↓/→ no longer fires: the View command wins (backlog).
201. Press ⌃⌘→: the right pane opens beside the bottom pane, under the
     arrangement. Press ⌃⌘↓: the bottom pane hides and the right pane runs
     beside the arrangement for the full height. Press ⌃⌘↓: the bottom
     pane comes back. Press ⌃⌘← twice: the left pane hides and comes back
     with the same view and size. The View menu's Left, Bottom and Right
     Pane items show the same check marks. Choose Left Pane › Sequences,
     double-click Song, type `Verse` and press ⌃⌘←: the left pane hides
     and the rename is kept, as Return would keep it (⌃⌘← again shows the
     new name). Press Space: the transport plays, and the status line
     does not say "A text field has the keyboard.". Undo the rename.
202. Drag the line between the arrangement and the bottom pane up and down:
     the pane follows the pointer, without running ahead of it, and stops
     at its smallest and largest size. With the right pane shown, shrink
     the window as far as it goes (700×450): the panes shrink to their
     smallest and the arrangement keeps a usable area; nothing hides at
     this size. Grow it again: the panes go back to their sizes.
203. Click the Chords lane header: the left pane shows the Built-in Synth's
     strip. Move its fader: the mixer (⌥⌘M) and the Mix view (⌃2) show the
     same level, and the meters fall at the same speed in all three. With
     the mixer window open, click the left strip's insert "+": the browser
     opens on the left strip only, not in the mixer window. Right-click
     the Chords lane, choose New MIDI Destination… (any port, such as the
     IAC Driver) and Add, which points the lane at it (step 156): the
     left pane shows the Inspector, naming that destination. Undo twice:
     Chords plays Built-in Piano again and the strip comes back.
204. Open each pane's picker: the left lists Channel Strip, Sequences and
     Inspector; the bottom lists Piano Roll, List, Strip Chart, Drum Grid
     and Mixer (the Mix view's bottom has no Mixer), and the bottom pane
     shows no second row of editor buttons; the right lists Controllers,
     Media Pool, Plug-ins and Markers. Choose each one: each shows its
     view, and nothing opens a dialog. Press ⌘1: the bottom pane
     shows the piano roll on the selection; ⌥⌘1 still opens it in a window.
205. In the right pane, Plug-ins: with the Chords lane selected, choose an
     effect: it appears as the last insert on the left strip and in the
     mixer; double-click its slot: its window opens floating. Markers:
     click a row: the position moves there. Controllers: the Strip Chart shows the
     selected lane's controller. Press ⌃2 and ⌃⌘→ (Plug-ins), click the
     Built-in Synth strip's name to select it, choose an effect: it
     appears as that strip's last insert.
206. Window > Mixer opens the mixer in its own window with its own
     transport bar: Play there plays the document. Set the Mix view's
     right pane to Markers, save, close and reopen the project: both views
     come back as you left them, and the window at the same size and place.
```

- [ ] **Step 3a: Correct the earlier steps that name the window's toolbar or the detail pane** (I12), in `app/README.md`. Each Find matches exactly once at `5da4996`; apply them in this order. The note above stays for the steps that say "the sidebar". Steps whose "toolbar" is an editor's own bar (the arrangement's, the piano roll's) are named after that bar, so that no step sends the owner to a toolbar that is gone.

- **Editors (plan 4a), the window bullet** (detail pane; M14: the panes saved). Find:

```text
- The document window shows the arrangement (the selected sequence's
  lanes and their blocks) over a detail pane. The piano roll (plan 4b)
```

  Replace with:

```text
- The document window shows the arrangement (the selected sequence's
  lanes and their blocks) over the bottom pane's editor (plan 7a: Left,
  Bottom and Right Pane, see steps 199-206). The piano roll (plan 4b)
```

- **Editors (plan 4a), the block rule bullet** (the arrangement's own bar, not the window's toolbar). Find:

```text
- Blocks follow the toolbar's rule (silence gap, fixed bars, whole lane);
```

  Replace with:

```text
- Blocks follow the arrangement bar's rule (silence gap, fixed bars,
  whole lane);
```

- **Editors (plan 4a), the saved layout bullet** (detail pane, sidebar; M14: the panes saved in ui.json). Find:

```text
- The editor layout (detail pane, editor windows and their frames, the
  sidebar order, block rule, light mode, snap, zoom) is saved in the
  project's `ui.json` when you save; it is not an undo step.
```

  Replace with:

```text
- The editor layout (the bottom pane's editor, editor windows and their
  frames, the sequence list's order, block rule, light mode, snap, zoom,
  and from plan 7a each main view's panes and the document window's
  frame) is saved in the project's `ui.json` when you save; it is not an
  undo step.
```

- **step 10** (the mixer window's toolbar is now its bar). Find:

```text
    moves while playing. The toolbar shows the DSP load.
```

  Replace with:

```text
    moves while playing. The bar along the top of the mixer shows the
    DSP load.
```

- **step 11** (Mixer toolbar). Find:

```text
11. Mixer toolbar > Instruments > "Built-in Piano: MIDI → Plugin…": the
```

  Replace with:

```text
11. The mixer's bar > Instruments > "Built-in Piano: MIDI → Plugin…": the
```

- **step 32** (detail pane). Find:

```text
32. Detail pane: double-click the Chords block: the pane below shows the
```

  Replace with:

```text
32. Bottom pane: double-click the Chords block: the pane below shows the
```

- **step 35** (the arrangement's own bar). Find:

```text
    block selects it. The lane headers read "Chords, Built-in Piano"; the
    toolbar controls (tool, Snap, Blocks, Light, Follow, zoom) are labelled.
```

  Replace with:

```text
    block selects it. The lane headers read "Chords, Built-in Piano"; the
    arrangement bar's controls (tool, Snap, Blocks, Light, Follow, zoom)
    are labelled.
```

- **step 36** (detail pane; the piano roll's own bar). Find:

```text
36. Double-click the Chords block: the detail pane shows the piano roll with
    a toolbar (Arrow/Pencil/Eraser, Keyboard, Lanes, Insert Into, Audio,
```

  Replace with:

```text
36. Double-click the Chords block: the bottom pane shows the piano roll with
    its bar (Arrow/Pencil/Eraser, Keyboard, Lanes, Insert Into, Audio,
```

- **step 51** (detail pane). Find:

```text
51. List: double-click the Chords block and press Cmd-2: the detail pane
```

  Replace with:

```text
51. List: double-click the Chords block and press Cmd-2: the bottom pane
```

- **step 66** (the piano roll's own bar). Find:

```text
    Exact Mode (Ctrl-Cmd-E; the toolbar's Exact toggle shows it too). Drag a
    chord note's left edge one beat right: it follows the pointer and, on
    release, goes back while the panel opens on Move with "by" 1.0 and "Apply
```

  Replace with:

```text
    Exact Mode (Ctrl-Cmd-E; the piano roll bar's Exact toggle shows it
    too). Drag a chord note's left edge one beat right: it follows the
    pointer and, on release, goes back while the panel opens on Move with
    "by" 1.0 and "Apply
```

- **step 85** (the window's toolbar is now the transport bar). Find:

```text
85. Input: `$V find Play` gives the toolbar button's ID; `$V click <ID>`
    plays without the pointer moving or VizDAW coming to the front;
```

  Replace with:

```text
85. Input: `$V find Play` gives the transport bar's Play button ID;
    `$V click <ID>` plays without the pointer moving or VizDAW coming to
    the front;
```

- **step 87** (detail pane). Find:

```text
87. Open: click Beat's block and press Cmd-4. The detail pane shows
```

  Replace with:

```text
87. Open: click Beat's block and press Cmd-4. The bottom pane shows
```

- **Recording (plan 5c), the hardware note** (toolbar). Find:

```text
pickers, monitor buttons and meters, the toolbar's Record button and
```

  Replace with:

```text
pickers, monitor buttons and meters, the transport bar's Record button and
```

- **step 144** (toolbar). Find:

```text
144. Record button and popover: the toolbar shows a Record button (a
```

  Replace with:

```text
144. Record button and popover: the transport bar shows a Record button (a
```

- **step 145** (toolbar). Find:

```text
     first higher) play while the toolbar shows "Count-in" and the
```

  Replace with:

```text
     first higher) play while the transport bar shows "Count-in" and the
```

- **step 146, first line** (toolbar). Find:

```text
     toolbar shows the recording time left (for 48 kHz / 24-bit stereo,
```

  Replace with:

```text
     transport bar shows the recording time left (for 48 kHz / 24-bit stereo,
```

- **step 146, loop record** (toolbar). Find:

```text
     record: the toolbar counts "Pass 2", then "Pass 3 · 1 kept" after
```

  Replace with:

```text
     record: the transport bar counts "Pass 2", then "Pass 3 · 1 kept" after
```

- **step 158** (toolbar). Find:

```text
     input In 1+2, monitoring input"; the toolbar's buttons read "Record"
```

  Replace with:

```text
     input In 1+2, monitoring input"; the transport bar's buttons read "Record"
```

- [ ] **Step 3b: Add the channel paragraph** (M14) to `app/README.md`'s "Agent control channel" section, after its automation paragraph. Find:

```text
count at the last poll (one poll stale after a device change).
```

Replace with:

```text
count at the last poll (one poll stale after a device change).

Windows and panes (plan 7a): `state.get layout` answers the document
window's main view (`arrange` or `mix`), each view's left, bottom and
right panes (shown, view, size and the views on that pane's list) and
what the left pane shows (`left_shows`: `strip`, `inspector`, `sequences`,
`nothing` or `hidden`; `left_strip`: the strip the rule picks).
`ui.view_arrange` and `ui.view_mix` switch the main view;
`ui.toggle_left`, `ui.toggle_bottom` and `ui.toggle_right` show and hide
a pane (absent `on` toggles); `ui.pane {"pane": "right", "view":
"markers", "size": 300, "shown": true, "main": "mix"}` sets one pane of
one view, refusing a view off that pane's list with `invalid_params` and
changing nothing. None of them opens a dialog; `window.mixer` and
`window.media_pool` still open their own windows.
```

- [ ] **Step 3c: Add a backlog row** (M11) at the end of `docs/feature-backlog.md`'s table, after its last row:

```markdown
| Keyboard slots on keys that plan 7a gives to View commands | Plan 7a pre-flight M11: commands win over keyboard slots (`KeyRouter`), so a saved project whose sequence has a ⌃1, ⌃2 or ⌃⌘←/↓/→ slot loses that slot silently. Warn when such a project opens, or list the shadowed slots in Settings › Keys. |
```

- [ ] **Step 4: Rewrite the root `README.md`** (spec §2 item 11, the 7a part). Keep it user-facing and in plain words, with no plan names. Keep the title paragraph, the License section and the Disclaimer section verbatim.
  - Replace the "**Status:** design phase …" line with a status paragraph: VizDAW is in development, not yet released. Today it records, edits and mixes MIDI and audio in one window. The interface's look (themes) and an audio editor are next.
  - Replace the bullet list with "What works today":
    - sequences, subsequences and takes;
    - the arrangement, piano roll, list, Strip Chart and drum grid editors, with step entry and key bindings;
    - audio and MIDI recording with monitoring and count-in;
    - audio import, regions, fades and normalize;
    - the mixer with inserts, buses, sends and automation;
    - AU (v2/v3) and CLAP plug-ins (no VST3);
    - control surfaces (Mackie Control with its XT extenders, HUI, and Kontrol S-Series MK3) and MIDI learn;
    - bounce to WAV, AIFF or CAF;
    - an external sample editor round trip;
    - MIDI 2.0 throughout, on Apple Silicon with macOS 14+.

    Then "Planned": Standard MIDI File import; OMF and AAF import, which may come after the first release.
  - Add "## Getting around", in at most 10 lines: one window per project; the transport bar; the Arrange (⌃1) and Mix (⌃2) views; the left, bottom and right panes, their pickers and ⌃⌘←/↓/→; ⌘1–⌘4 for the editors; Window > Mixer for a separate mixer window; keys can be changed in Settings › Keys.
  - Add "## Building from source": what you need (Rust 1.85 or newer, as `Cargo.toml`'s `rust-version` says; the Xcode Command Line Tools; macOS 14+), and the four commands from `app/README.md`'s "Build and run" block, with a link to `app/README.md` for more.

- [ ] **Step 5: Run the full gates.**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --workspace --all-targets --target x86_64-unknown-linux-gnu --target-dir target/linux-check -- -D warnings
cargo test --workspace
scripts/build-ffi.sh && scripts/swift.sh build && scripts/swift.sh test
scripts/make-app.sh
VIZDAW_E2E_APP=dist/VizDAW.app cargo test -p vizctl --test e2e -- --ignored --test-threads 1
pgrep -lf MacOS/VizDAW; ls -d /tmp/vze* 2>/dev/null
```

Expected:
- Rust 1996 passed / 19 ignored on macOS, Linux clippy clean;
- Swift 1034;
- e2e 15 passed;
- registry 153;
- `pgrep` and `ls` print nothing.

- [ ] **Step 6: Commit** `crates/vizctl/tests/e2e.rs`, `app/README.md`, `README.md` and `docs/feature-backlog.md`. Message: "e2e: views and panes over the channel; README 199-206 and the earlier steps' toolbar and detail pane; the root README's status, features, getting around and building (plan 7a Task 9)", with the trailers. Counts: ignored +1 (19), e2e 15.

---

## Self-Review

1. **Spec coverage.**
   - §2 item 1, the window and layout A: Tasks 2 and 7.
   - §2 item 2:
     - the Arrange and Mix views: Tasks 1, 6 and 7;
     - the mixer in the bottom pane and its window, with the window's transport bar: Tasks 7–8;
     - per-view setups, with switching restoring both: Task 1;
     - the shared transport, selection and playhead: one `DocumentModel`, Task 7.
   - §2 item 3:
     - each pane's list and picker: Tasks 1 and 7–8;
     - the toggles and keys: Tasks 5–7.
   - §2 item 4, floating windows kept (`window.mixer`, `window.media_pool`, plug-in windows, sheets, torn editors): Tasks 7–8, and the e2e step 6. Plug-in windows open from every placement through `DocumentShell`'s root `openPluginWindow` (Ruling 8).
   - §2 item 8, for the left pane: Task 3.
   - §2 item 11, 7a's README part: Task 9.
   - §4, nothing hidden: the sequence list stays reachable (Ruling 12), and every sheet stays on `DocumentView`. The earlier README steps that name the toolbar or the detail pane are corrected (Task 9 step 3a), and a field that vanishes mid-edit gives the keyboard back (`endPaneEdits`). The channel and the e2e work: Tasks 5, 6 and 9.
   - §5, the keys: Ruling 4.
   - §6.1 and §6.3: Rulings 1, 11 and 12.
   - Out of scope, as asked: themes, the strip redesign and icons (7b); the audio editor, left out of the list (Ruling 3; 7c); clean-up (7d).
2. **Step scan.** Every test step gives names and assertions. Every code step gives signatures, files and the pinned values. View tasks (7–8) give the structure and the exact strings, and are checked by the e2e and README 199–206. Task 5's mid-plan `handled` gap is settled by its executor ruling.
3. **Type consistency.**
   - `PaneView`'s raw names are the channel's names, and `ViewCommands.viewName` adds the editor kind names (`ControlStates.kindName`). Task 5's `view` Choice lists exactly that union.
   - `MixerPlacement` (Task 1) is used by `LearnModel` and `MixerModel`'s browser, rename and `placements` (Task 3), `MixerView` and `MixerBar` (Task 7) and `LeftStripPane` (Task 8).
   - `openDetail(_:kind:in:)` (Task 1) is used by `runPane` (Task 6) and the bottom `PaneFrame` (Task 7). `laneOnStrip10()` (Task 4) is used by `ViewCommandsTests` (Task 6). `endPaneEdits` and `storeMixer` (Task 6) are used by `PaneFrame` (Task 7).
   - `StripFocus.Result` (Task 3) feeds `ControllersPane`, `insertPlugin` (Task 4, after the single selected mixer strip), `layoutRow`'s `left_strip` and `left_shows` (Task 6), and `LeftStripPane` (Task 8).
   - `LayoutStateRow`'s field names match the JSON keys (Task 5) and Swift's generated `leftStrip` and `leftShows` (Task 6).
4. **Review Focus.** Each of the five lines names its tests in their tasks.
5. **Proportion.** About 1200 short lines, against a 137-line spec whose 7a row spans §2 items 1–4, 8 and 11. The code blocks are tests and the exact README text. Bodies are left to the signatures.

## Execution handoff

Plan complete and saved to `docs/superpowers/plans/2026-11-03-window-views-panes.md`. The controller runs it subagent-driven, as in SP5, with one reviewer per task. Tasks 7 and 8 are view work that only the e2e and the owner's by-eye steps check, so their reviews should read `DocumentShell` against Rulings 2, 3, 8 and 9 line by line (the divider's `dragStart`, the mounted Arrange centre and `DetailArea`, the placements, the root `openPluginWindow` and `learnAlertHosted`).
