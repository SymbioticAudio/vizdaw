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
  - `MixerModel.tick`: one poll per frame, however many mixers are on screen.
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
  | 3 left pane, mixer tick | — | +10 | |
  | 4 right pane | — | +6 | |
  | 5 registry, `layout` target | +3 | +0 (compiles) | registry 153 |
  | 6 `ViewCommands` | — | +6 | |
  | 7 shell, transport bar, View menu | — | +0 | |
  | 8 pane contents | — | +0 | |
  | 9 e2e, READMEs | ignored +1 | — | e2e 15; README 199–206 |
  | **After** | **1996 / 19 ignored** | **1031** | |

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
     - panes shrink to their minimum first and the centre keeps at least 240×160;
     - if that still does not fit, the right pane hides first, then the left, then the bottom. Their `shown` flags stay saved, so they come back when the window grows;
     - a dragged size is clamped to the pane's range.
   - Pinned by `aSmallWindowShrinksPanesBeforeTheCentre` and `aDividerDragIsClamped` (Task 2).
3. **The mixer on screen twice.** The bottom pane's mixer is shown with the mixer window open, or the Mix view with the left strip.
   - Expected:
     - meters decay at the normal rate, not twice as fast;
     - one poll per frame;
     - the MIDI Learn Maps sheet opens in one place only, the place whose button asked for it.
   - Pinned by `twoMixerViewsPollOncePerFrame` and `learnMapsShowInOnePlacement` (Task 3).
4. **Selections the strip rule must handle.**
   - The cases: no lane selected; an external-MIDI lane; two lanes on one output path; two lanes on different paths; a lane in another sequence; a lane deleted while it is shown.
   - Expected:
     - no lane selected: the first record-armed lane's strip shows, else the placeholder "Select a lane to see its channel strip.";
     - an external-MIDI lane: the inspector shows;
     - two lanes on one path: that one strip;
     - two lanes on different paths: the first selected lane's strip;
     - a lane in another sequence is ignored;
     - a deleted lane falls back to the placeholder without a crash.
   - Pinned by Task 3's six `StripFocus` tests.
5. **Hostile or careless `ui.*` calls.** Examples:
   - `ui.pane` with no field, an unknown pane, a view off that pane's list, the mixer below in the Mix view;
   - `size` as text or as 1e12;
   - `main` as `edit`;
   - extra parameters;
   - any call with no document open.

   Expected: `invalid_params` (types and ranges in core-control, lists in the app) or `no_document`, and the layout is unchanged. No dialog opens, and `state.get windows` lists the same windows afterwards. Pinned by `ui_commands_check_their_params` (Task 5), `uiCommandsCheckTheirArguments` (Task 6), and the e2e's hostile block (Task 9).

## Plan Rulings

Each ruling decides something the spec leaves to the builder, in the format "what — why — cost if wrong". Spec "(controller's call)" items are restated only where this plan makes them concrete.

1. **Ruling (where the layout lives): `EditorLayout.views: ViewLayouts`, saved in `ui.json` with the rest of the editors' UI state.**
   - `ViewLayouts` holds:
     - `main: MainView` (`.arrange` or `.mix`);
     - `arrange: PaneSetup`;
     - `mix: PaneSetup`;
     - `frame: [Double]?`, the document window's `[x, y, width, height]` (Ruling 11).
   - `PaneSetup` holds `left`, `bottom` and `right`, each a `PaneState { shown: Bool, view: PaneView, size: Double }`.
   - Decoding is tolerant, field by field, using the existing `KeyedDecodingContainer.field`. Then `ViewLayouts.sanitized()` repairs:
     - a view not on its side's list for that main view, which becomes that side's default;
     - a size that is not finite or is out of range, which becomes the default or is clamped.
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
   - The centre minimum is 240×160 (`PaneGeometry.centreMinimum`).
   - A window too small for that shrinks panes to their minimums first. Then it hides panes for that frame only, in the order right, then left, then bottom (`PaneFrames.hidden`). The saved `shown` flags are not changed.
   - The 1-point dividers are draggable (`PaneGeometry.drag(_:side:by:in:) -> Double`, clamped).
   - Why: spec §2 item 1 and the chosen mockup A ("the editor and the controller pane share the bottom right"). SwiftUI's `HSplitView` cannot report or restore sizes.
   - Cost if wrong: a reviewer who wants the right pane under the centre only can change one branch of `frames`.
3. **Ruling (the bottom pane's list): its views are `.editor` and `.mixer`.**
   - `.editor` shows today's detail editor (`layout.detail`) in its kind. That kind is shared by both main views because `layout.detail` is.
   - The picker lists the four `DetailEditorKind.allCases` (Piano Roll, List, Strip Chart, Drum Grid), then Mixer. Choosing a kind sets the view to `.editor` and opens the detail in that kind (`openDetail`, or `showDetail` on the current target).
   - The Mix view's bottom list leaves out Mixer: the Mix view is the mixer.
   - ⌘1–⌘4, a double-clicked block and every `openDetail` show the current view's bottom pane with `.editor`. `editors.close_detail` leaves the pane shown with today's "No Editor Open" placeholder.
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
   - With Mission Control's "Switch to Desktop" shortcuts turned on, macOS takes ⌃1/⌃2 first. README step 200 says to rebind them in Settings › Keys.
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
   - **Media pool:** today's `MediaPoolView` with a new `compact: Bool`. Compact shows the Name, Duration and Offline columns, and puts the action buttons in one "Actions" menu. `minWidth` is 620 only when not compact.
   - **Plug-ins:** today's `PluginBrowserView` with a new `width: CGFloat?` (nil fills the pane; the popover passes 320), listing effects.
     - A pick calls `PaneContent.insertPlugin(_:document:mixer:) -> String?`. It adds the plug-in as the last insert of the left-pane rule's strip, through `mixer.browser = .insert(strip:)` and `mixer.pick(_:)`.
     - With no strip it returns "Select a lane with a channel strip first.", which goes to the status line.
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
   - Why: spec §2 item 2 ("the pane, the view and the window all show the same mixer state").
   - Cost if wrong: a second sheet or a faster meter, both pinned by tests.
9. **Ruling (the transport bar).**
   - `DocumentTransportBar(model:docID:showsViews:)` is an ordinary SwiftUI view at the top of the window's content, not an `NSToolbar`. It holds:
     - today's `TransportToolbar` items: Play/Stop, Loop, `RecordControls`, the position, the device, the faults button;
     - today's `TransportBar`: the `TransportFieldsHost` and step entry's summary, moved out of `ArrangementPane`;
     - with `showsViews`, the main-view tabs (a segmented picker "Arrange" | "Mix") and the three pane toggles.
   - The mixer window shows the bar with `showsViews: false`. Its tabs and toggles would act on another window.
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
    - The app refuses with `invalid_params`:
      - a `ui.pane` call with none of `view`, `shown` and `size`;
      - a view that is not on that pane's list for that main view, with the message "‹view› is not on the ‹pane› pane's list in the ‹main› view.".
    - `size` is clamped to the pane's range. The answer's status names what was set: `"left strips 220 shown"`.
    - `state.get layout` answers `ControlResult::Layout { doc, layout: LayoutStateRow }`, in this JSON shape:
      ```json
      {"doc": "…", "main": "arrange", "left_strip": 3,
       "left_shows": "strip",
       "views": [{"main": "arrange", "panes": [{"pane": "left", "shown": true, "view": "strip", "size": 220.0, "views": ["strip", "sequences", "inspector"]}, …]}, {"main": "mix", …}]}
      ```
      - `left_shows` is what the left pane really shows: `strip`, `inspector`, `sequences` or `nothing`.
      - The bottom pane's `view` is its editor kind name (`piano_roll` … from `layout.detail?.kind ?? layout.lastKind`) when `.editor`, else `mixer`.
    - `state.get windows`, `window.mixer`, `window.media_pool` and every window kind are unchanged (spec §2 item 4).
    - Why: spec §4 ("window and pane state … `state.get` and the `ui.*` commands") and §5. Registry commands give menus, keys and the channel one handler, as for every other command since plan 4f.
    - Cost if wrong: names in one table.
11. **Ruling (the window size): the document window's frame is saved in `ViewLayouts.frame`.**
    - `WindowFrameKeeper` (an `NSViewRepresentable` in `DocumentWindow.swift`) applies the saved frame once, when the view joins its window, and only if `PaneGeometry.sanitizedFrame(_:screens:)` accepts it: four finite numbers, at least 700×450, intersecting a screen by at least 100×100.
    - It saves the frame on `NSWindow.didMoveNotification` and `didEndLiveResizeNotification` through `EditorModel.setDocumentFrame(_:)`, which does nothing when the frame is unchanged.
    - Why: spec §6.3.
    - Cost if wrong: SwiftUI's own restoration wins for an unsaved document, which is harmless.
12. **Ruling (the sequence list): today's `SequenceSidebar` becomes the left pane's "Sequences" view, unchanged.**
    - Picking a sequence still works there. It no longer shows by default.
    - The app README's earlier steps that say "the sidebar" get one note in Task 9 ("the sidebar is now Left Pane › Sequences") instead of being rewritten one by one.
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
| `app/Sources/VizDAWAppModel/WindowLayout.swift` (new), `EditorLayout.swift`, `EditorModel.swift`, `EditorModel+Windows.swift`; `app/Tests/VizDAWAppModelTests/WindowLayoutTests.swift` (new) | 1 | per-view setups, intents |
| `app/Sources/VizDAWAppModel/PaneGeometry.swift` (new); `app/Tests/VizDAWAppModelTests/PaneGeometryTests.swift` (new) | 2 | frames, clamping, saved frame |
| `app/Sources/VizDAWAppModel/PaneContent.swift` (new), `MixerModel.swift`, `LearnModel.swift`; `app/Tests/VizDAWAppModelTests/LeftPaneTests.swift` (new) | 3 | strip rule, inspector, tick, maps placement |
| `app/Sources/VizDAWAppModel/PaneContent.swift`, `DocumentModel.swift`, `EditorLayout.swift`; `app/Tests/VizDAWAppModelTests/RightPaneTests.swift` (new) | 4 | markers, plug-in insert, controllers chart |
| `crates/core-control/src/{registry.rs,request.rs}`, `crates/core-control/tests/commands.rs`; `crates/core-ffi/src/control.rs`, `crates/core-ffi/tests/control.rs`; `app/Sources/VizDAWAppModel/ControlCommands.swift` (stub case) | 5 | registry, `layout` target, JSON |
| `app/Sources/VizDAWAppModel/ViewCommands.swift` (new), `ControlCommands.swift`, `TransportCommands.swift` (conformance line); `app/Tests/VizDAWAppModelTests/ViewCommandsTests.swift` (new) | 6 | `ui.*` handlers, `state.get layout` |
| `app/Sources/VizDAW/DocumentShell.swift` (new), `DocumentView.swift`, `DocumentWindow.swift`, `DetailPane.swift`, `ArrangementPane.swift`, `MixerView.swift`, `AppCommands.swift` | 7 | shell, transport bar, Mix view, mixer window bar, View menu, frame keeper |
| `app/Sources/VizDAW/PaneViews.swift` (new), `MediaPoolWindow.swift`, `PluginBrowserView.swift`, `MixerView.swift` | 8 | left, bottom and right pane contents |
| `crates/vizctl/tests/e2e.rs`, `app/README.md`, `README.md` | 9 | e2e, by-eye steps, root README |

---

### Task 1: Per-view pane setups in the layout

**Files:**
- Create: `app/Sources/VizDAWAppModel/WindowLayout.swift`, `app/Tests/VizDAWAppModelTests/WindowLayoutTests.swift`
- Modify: `app/Sources/VizDAWAppModel/EditorLayout.swift` (field, `CodingKeys`, `init(from:)`), `app/Sources/VizDAWAppModel/EditorModel.swift` (`openDetail`), `app/Sources/VizDAWAppModel/EditorModel+Windows.swift` (`showDetail`)

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
  - `EditorLayout.views: ViewLayouts` (coding key `views`);
  - on `EditorModel`, in `WindowLayout.swift`:
    - `func showMainView(_ v: MainView)`;
    - `func setPaneShown(_ side: PaneSide, _ on: Bool?, in main: MainView? = nil)`, where nil `on` toggles;
    - `func setPane(_ side: PaneSide, view: PaneView?, shown: Bool?, size: Double?, in main: MainView? = nil) -> String?`, which returns the refusal message, or nil when applied;
    - `func setDocumentFrame(_ f: [Double])`.

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
  - Each struct decodes field by field through `field`. A `PaneState` that does not decode is that side's default.
  - `EditorLayout.init(from:)` sets `views = c.field(.views, d.views).sanitized()`.
  - `sanitized()` applies Ruling 1:
    - an off-list view becomes that side's default for that main view;
    - a non-finite size becomes the default;
    - a finite size is clamped to `sizeRange[side]`;
    - a `frame` that is not exactly four finite numbers becomes nil.
  - The refusal text is exactly `"\(view.rawValue) is not on the \(side.rawValue) pane's list in the \(main.rawValue) view."`. `setPane` clamps `size` to the range.
  - `openDetail` (`EditorModel.swift`) sets `l.views.current.bottom.shown = true` and `.view = .editor` inside its existing `updateLayout`. `showDetail` gets this through `openDetail`.

- [ ] **Step 4: Run them and see them pass,** then the whole suite: `scripts/swift.sh test`. Expected: 1003 tests pass (993 + 10). `EditorModelTests`' existing round trip still passes.

- [ ] **Step 5: Commit**

```bash
git add app/Sources/VizDAWAppModel/WindowLayout.swift app/Sources/VizDAWAppModel/EditorLayout.swift \
  app/Sources/VizDAWAppModel/EditorModel.swift app/Sources/VizDAWAppModel/EditorModel+Windows.swift \
  app/Tests/VizDAWAppModelTests/WindowLayoutTests.swift
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
  - `public struct PaneFrames: Equatable { public var left, centre, bottom, right: CGRect?; public var hidden: [PaneSide] }`. The centre is always set, and `hidden` lists the panes dropped for this frame only;
  - `public enum PaneGeometry`:
    - `static let centreMinimum = CGSize(width: 240, height: 160)`;
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
    #expect(f.left?.width == 160)
    #expect(f.hidden == [.right])
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
  - Shrinking order: shrink every shown pane towards its minimum, the last-added first (right, bottom, left), until the centre minimum fits. Then hide panes in the order right, left, bottom.
  - The drag sign: left and right grow with a positive drag towards the centre (right grows with negative `delta`), and bottom grows with negative `delta` (upwards). A non-finite `delta` keeps the size.

- [ ] **Step 4: Run them and see them pass,** then the suite: 1009.

- [ ] **Step 5: Commit** `PaneGeometry.swift` and its tests. Message: "app model: layout A's pane frames, clamping and the saved window frame (plan 7a Task 2)", with the two trailer lines.

---

### Task 3: The left pane's strip, the inspector, one mixer poll per frame

**Files:**
- Create: `app/Sources/VizDAWAppModel/PaneContent.swift`, `app/Tests/VizDAWAppModelTests/LeftPaneTests.swift`
- Modify: `app/Sources/VizDAWAppModel/MixerModel.swift` (add `tick`), `app/Sources/VizDAWAppModel/LearnModel.swift` (add `mapsPlacement`, `showsMaps(in:)`)

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
    - `@discardableResult public func tick(now: Double) -> Bool`, true when it polled. `now` is seconds from any monotonic clock, and a view passes `ProcessInfo.processInfo.systemUptime`.
  - `LearnModel`:
    - `public var mapsPlacement: MixerPlacement = .window`;
    - `public func showMaps(in p: MixerPlacement)`, which sets the placement, then `showsMaps = true`;
    - `public func showsMaps(in p: MixerPlacement) -> Bool`;
    - `public func closeMaps(from p: MixerPlacement)`, which sets `showsMaps = false` only when `mapsPlacement == p`.

- [ ] **Step 1: Write the failing tests** in `LeftPaneTests.swift`. Build rows with small file-private helpers `lane(id:audioOut:midiDest:arm:)`, `dest(id:plugin:external:)` and `strip(id:kind:)` that fill the other fields with neutral values.

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
}
@Test func theInspectorDescribesTheLaneAndRegion() {
    let rows = Inspector.rows(lane: lane(1, midiDest: 3), region: nil, regionName: nil,
                              destinations: [dest(3, external: true, name: "Ext Synth")], strips: [])
    #expect(rows.first == InspectorRow(label: "Name", value: "Lane 1"))
    #expect(rows.contains(InspectorRow(label: "Plays", value: "Ext Synth")))
    #expect(Inspector.rows(lane: nil, region: nil, regionName: nil, destinations: [], strips: []).isEmpty)
}
@Test func twoMixerViewsPollOncePerFrame() throws {
    let mixer = try MixerModel(document: fakeDocument())
    #expect(mixer.tick(now: 10.000))
    #expect(!mixer.tick(now: 10.010))   // the second placement, same frame
    #expect(mixer.tick(now: 10.031))
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
  - `MixerView`'s `.task` loop changes in Task 7, not here.

- [ ] **Step 4: Run them and see them pass;** suite 1019.

- [ ] **Step 5: Commit** the three model files and the test file. Message: "app model: the left pane's strip rule, the inspector, one mixer poll per frame, the maps sheet's placement (plan 7a Task 3)", with the trailers.

---

### Task 4: The right pane's models: markers, the plug-in browser's insert, the controllers chart

**Files:**
- Modify: `app/Sources/VizDAWAppModel/PaneContent.swift`, `app/Sources/VizDAWAppModel/DocumentModel.swift` (`controllersChart`), `app/Sources/VizDAWAppModel/EditorLayout.swift` (`controllers` field), `app/Sources/VizDAWAppModel/StripChartModel.swift` (`static func controllers(_:)`)
- Create: `app/Tests/VizDAWAppModelTests/RightPaneTests.swift`

**Interfaces:**
- Consumes: `SequenceInfo.markers` / `MarkerRow`, `TransportMarks.locators`, `MeterMath.bar(at:_:)`, `DocumentModel.moveTo(_:)`, `MixerModel.browser` / `pick(_:)`, `StripFocus` and `leftStrip` (Task 3), `StripChartModel.init(document:read:write:)`.
- Produces:
  - `public struct MarkerListRow: Equatable, Identifiable, Sendable { public let id: String; public let name: String; public let tick: Int64; public let position: String }`, where `id` is `"m<i>"` or `"l<n>"`;
  - `public enum MarkerList`:
    - `static func rows(info: SequenceInfo?, marks: TransportMarks) -> [MarkerListRow]`, ordered by tick, markers before locators at equal ticks;
    - `@MainActor static func go(_ row: MarkerListRow, _ m: DocumentModel)`, which calls `moveTo` and puts a failure in `m.editor.status`;
  - `@MainActor public enum PaneContent`:
    - `static func insertPlugin(_ p: PluginRefRow, document: DocumentModel, mixer: MixerModel) -> String?`;
    - `static let noStripMessage = "Select a lane with a channel strip first."`;
  - `EditorLayout.controllers: [String: [StripLane]]` (coding key `controllers`, tolerant);
  - `StripChartModel.controllers(_ document: DocumentModel) -> StripChartModel`;
  - `DocumentModel.controllersChart: StripChartModel`, lazy and made once.

- [ ] **Step 1: Write the failing tests** in `RightPaneTests.swift`.

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
    let (m, session) = fakeDocumentAndSession()   // its demo lane plays through strip 10
    selectFirstLane(m)
    let mixer = try MixerModel(document: m)
    #expect(PaneContent.insertPlugin(delay, document: m, mixer: mixer) == nil)
    #expect(session.insertedOn.last == 10)   // the fake's record of insert edits; use the tree's name
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

If `FakeSession` keeps no record of insert edits, add one (`insertedOn: [UInt64]`, appended by its insert-edit method) in `FakeSession.swift`, and list that file in the commit.

- [ ] **Step 2: Run them and see them fail** (`--filter RightPaneTests`).

- [ ] **Step 3: Implement** Ruling 7's model parts.
  - `controllers(_:)` reads through `leftStrip`'s lane gathering: the first considered lane, selection first and then armed. Its `write` stores only `stripRows`, under the `"seq:lane"` key.
  - `insertPlugin` uses `leftStrip(strips: mixer.snapshot.strips, destinations: mixer.destinations)`. Only `.strip(id)` proceeds, with `mixer.browser = .insert(strip: id)` and `mixer.pick(p)`.
  - `pruneLayout` drops `controllers` keys whose lane is gone, inside its existing single `updateLayout`.

- [ ] **Step 4: Run them and see them pass;** suite 1025.

- [ ] **Step 5: Commit** the model files, `FakeSession.swift` if changed, and the tests. Message: "app model: the right pane's markers, plug-in insert and controllers chart (plan 7a Task 4)", with the trailers.

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
  - `scripts/build-ffi.sh && scripts/swift.sh build && scripts/swift.sh test`. Expected: 1025, all passing.

- [ ] **Step 5: Commit** the five Rust files and `ControlCommands.swift`. Message: "control: the ui.* view and pane commands and state.get layout (plan 7a Task 5)", with the trailers. Counts: Rust +3, registry 153.

---

### Task 6: `ViewCommands`: menus, keys and the channel

**Files:**
- Create: `app/Sources/VizDAWAppModel/ViewCommands.swift`, `app/Tests/VizDAWAppModelTests/ViewCommandsTests.swift`
- Modify: `app/Sources/VizDAWAppModel/ControlCommands.swift` (remove Task 5's six IDs from `other`; `handled` gains `ViewCommands.ids`; route them; `ControlStates.state` `.layout`), `app/Sources/VizDAWAppModel/TransportCommands.swift` (add `extension ViewCommands: MenuCommands {}` beside the other two)

**Interfaces:**
- Consumes: Task 1's intents, `StripFocus` and `leftStrip` (Task 3), `ControlArg` helpers (`args.text`, `args.flag`, `args.int`; use the tree's names), `ControlOutcome`, `ControlFailure.of`, `KeyOutcome`, `MenuCommands`.
- Produces:
  - `@MainActor public enum ViewCommands`:
    - `static let ids: Set<String>`, the six IDs;
    - `static func enabled(_ id: String, _ m: DocumentModel) -> Bool`, always true;
    - `static func isOn(_ id: String, _ m: DocumentModel) -> Bool`: the current main view for the two view IDs, the current setup's `shown` for the three toggles;
    - `static func run(_ id: String, on: Bool?, _ m: DocumentModel) -> KeyOutcome`;
    - `static func runPane(_ args: [ControlArg], _ m: DocumentModel) -> ControlOutcome`;
    - `static func layoutRow(_ m: DocumentModel, strips: [StripRow], destinations: [DestRow]) -> LayoutStateRow`;
    - `static func viewName(_ side: PaneSide, _ state: PaneState, _ layout: EditorLayout) -> String`.
  - `ControlStates.state(.layout, id, m)` returns `.layout(doc:, layout: ViewCommands.layoutRow(m, strips: (try? m.session.mixer().strips) ?? [], destinations: (try? m.session.destinations()) ?? []))`.

- [ ] **Step 1: Write the failing tests** in `ViewCommandsTests.swift`. They run through `ControlCommands.run` with the existing `FakeControlHost` and a store document, as `ExternalCommandsTests` does.

```swift
@Test func viewKeysSwitchTheMainView() {
    let (doc, store) = storeDocument()
    #expect(run("ui.view_mix", [], doc, store).isDone)
    #expect(doc.model.editor.layout.views.main == .mix)
    #expect(ViewCommands.isOn("ui.view_mix", doc.model))
    _ = run("ui.view_arrange", [], doc, store)
    #expect(doc.model.editor.layout.views.main == .arrange)
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
    #expect(doc.model.editor.layout == before)
    #expect(ControlCommands.run("ui.view_mix", args: [], doc: nil, store: store, host: FakeControlHost()).code == .noDocument)
}
@Test func stateGetLayoutReportsBothViewsAndTheLeftStrip() {
    let (doc, _) = storeDocument()   // FakeSession: lane 1 plays through strip 10
    selectFirstLane(doc.model)
    let row = ViewCommands.layoutRow(doc.model, strips: [strip(10, .audioPath(path: 7))], destinations: [])
    #expect(row.main == "arrange")
    #expect(row.views.map(\.main) == ["arrange", "mix"])
    #expect(row.views[0].panes.map(\.pane) == ["left", "bottom", "right"])
    #expect(row.views[0].panes[1].view == "piano_roll")
    #expect(row.views[1].panes[1].views == ["piano_roll", "list", "strip_chart", "drum_grid"])
    #expect(row.leftStrip == 10 && row.leftShows == "strip")
}
@Test func mainViewKeysDoNotCollideWithTheEditorKeys() {
    let keys = KeyBindings(defaults: MemoryDefaults())
    #expect(keys.row("ui.view_arrange")?.menuKey == "1" && keys.row("ui.view_arrange")?.menuModifiers.control == true)
    #expect(keys.row("editors.piano_roll")?.menuModifiers.command == true)
    #expect(keys.row("editors.piano_roll")?.menuModifiers.control == false)
}
```

`run`, `.isDone`, `.status` and `.code` are small file-private helpers over `ControlOutcome`. Reuse `ExternalCommandsTests`' helpers if they are shared.

- [ ] **Step 2: Run them and see them fail** (`--filter ViewCommandsTests`).

- [ ] **Step 3: Implement.**
  - `runPane` steps:
    1. Parse `pane`, `view`, `shown`, `size` and `main`. Core-control has already checked types, so the app checks only that `pane` and `main` are known, and refuses unknown values as `invalid_params`.
    2. Map an editor kind name to `.editor`, then call `openDetail(target, kind:)` on the current detail target. With no target, set `lastKind` alone through `updateLayout`.
    3. Call `setPane`. A refusal becomes `invalid_params` with its message.
  - The answer status is `"\(pane) \(viewName) \(Int(size)) \(shown ? "shown" : "hidden")"`.
  - `ControlCommands.run` dispatches `ViewCommands.ids` like `TransportCommands.ids`, through the existing `KeyOutcome`-to-`ControlOutcome` path, with `ui.pane` going to `runPane`.

- [ ] **Step 4: Run them and see them pass;** suite 1031, with `ControlCommandsTests`' `handled` check passing.

- [ ] **Step 5: Commit** the three Swift files and the test file. Message: "app model: ViewCommands for the View menu, keys and ui.*, and state.get layout (plan 7a Task 6)", with the trailers.

---

### Task 7: The single window: shell, transport bar, Mix view, mixer window bar, View menu

**Files:**
- Create: `app/Sources/VizDAW/DocumentShell.swift`
- Modify:
  - `app/Sources/VizDAW/DocumentView.swift`: its body's `NavigationSplitView { … } detail: { … }` becomes `DocumentShell`; `.toolbar { TransportToolbar(model: model) }` is removed; `TransportToolbar: ToolbarContent` becomes `DocumentTransportBar: View`;
  - `app/Sources/VizDAW/ArrangementPane.swift`: remove the `TransportBar(model: model)` and `StatusLine(editor: model.editor)` lines from `ArrangementPane.body`. `TransportBar` and `StatusLine` stay defined there;
  - `app/Sources/VizDAW/DetailPane.swift`: `EditorSplit` is deleted, because `DocumentShell` replaces it. `DetailArea` stays;
  - `app/Sources/VizDAW/MixerView.swift`: `MixerWindow.content` puts the transport bar above `MixerView`; `MixerView(model:placement:)`; `.toolbar { … }` becomes `MixerBar`; the `.task` loop calls `model.tick(now:)`; the maps sheet goes through `learn.showsMaps(in:)` and `closeMaps(from:)`; StripView's "MIDI Learn Maps…" calls `learn.showMaps(in: placement)`, with `placement` read from `@Environment(\.mixerPlacement)`;
  - `app/Sources/VizDAW/DocumentWindow.swift`: `WindowFrameKeeper`;
  - `app/Sources/VizDAW/AppCommands.swift`: the View menu group.

**Interfaces:**
- Consumes: Tasks 1, 2, 3 and 6 (`ViewCommands.run` and `isOn` for the menu toggles and the bar's tabs and toggles; `PaneGeometry.frames`, `drag` and `sanitizedFrame`; `MixerModel.tick`; `MixerPlacement`).
- Produces (the `VizDAW` target, used by Task 8):
  - `struct DocumentShell: View { let model: DocumentModel; let docID: UUID }`, which lays out the transport bar, the banners, the regions and `StatusLine`;
  - `struct PaneFrame<Content: View>: View { let side: PaneSide; let model: DocumentModel; @ViewBuilder let content: () -> Content }`. It shows the view picker (a menu-style `Picker` labelled with the pane's `title`, listing `ViewLayouts.views(side, in: main)`'s titles) and the content. Task 8 fills the content through `PaneHost(side:model:docID:)`;
  - `struct PaneHost: View { let side: PaneSide; let model: DocumentModel; let docID: UUID }`. In Task 7 it shows `DetailArea` for the bottom pane's `.editor`, and a `ContentUnavailableView` with the view's title for every other view; Task 8 replaces those;
  - `struct DocumentTransportBar: View { @Bindable var model: DocumentModel; let docID: UUID; let showsViews: Bool }`;
  - `struct MixerBar: View { @Bindable var model: MixerModel }`;
  - `EnvironmentValues.mixerPlacement: MixerPlacement`, default `.window`;
  - `struct WindowFrameKeeper: NSViewRepresentable { let model: DocumentModel }`.

- [ ] **Step 1: Build the shell.**
  - `DocumentShell` uses `GeometryReader`. It lays out `PaneGeometry.frames(in:setup: model.editor.layout.views.current)` with `.frame` and `.position` (or a `ZStack` with offsets), and puts a 1-point `PaneDivider` on each shown pane's inner edge.
  - A divider's `DragGesture` calls `model.editor.setPane(side, view: nil, shown: nil, size: PaneGeometry.drag(…), in: nil)` on change.
  - The centre:
    - Arrange: `ArrangementPane(model:)`;
    - Mix: `MixerView(model: appStore.mixer(docID)!, placement: .view)` with `.environment(\.openPluginWindow, …)` as `MixerWindow` sets it. A nil mixer shows the existing "Mixer Unavailable" alert path, via `store.mixer`, and a `ContentUnavailableView`.
  - Each shown pane is `PaneFrame(side:model:) { PaneHost(side:model:docID:) }`.

- [ ] **Step 2: Build the transport bar** (Ruling 9).
  - Move `TransportToolbar`'s three item groups into one `HStack`, in their order, with `TransportBar(model:)` after the position.
  - With `showsViews`, add:
    - a segmented `Picker("Main View", …)` over `MainView.allCases`, titled "Arrange" and "Mix", bound through `ViewCommands.run("ui.view_*", on: nil, model)`;
    - three `Toggle`s with `.toggleStyle(.button)`, with SF Symbols `sidebar.left`, `rectangle.bottomthird.inset.filled` and `sidebar.right`, each with `.help("Left Pane (⌃⌘←)")` and so on, and the accessibility labels "Left Pane", "Bottom Pane" and "Right Pane".
  - `DocumentView` keeps every `.sheet` and `.alert` it has today, unchanged, attached to `DocumentShell`.

- [ ] **Step 3: Rework the mixer for three placements** (Ruling 8).
  - `MixerWindow.content` becomes `VStack(spacing: 0) { DocumentTransportBar(model: mixer.document, docID: id, showsViews: false); Divider(); MixerView(model: mixer, placement: .window) }`, keeping its `.environment`, `.focusedSceneValue` and `.navigationTitle`.
  - `MixerBar` holds today's toolbar items in an `HStack`, right-aligned, with the instrument popover still attached to the Instruments menu.
  - The `.task` loop becomes `while !Task.isCancelled { model.tick(now: ProcessInfo.processInfo.systemUptime); try? await Task.sleep(for: .milliseconds(33)) }`.
  - `MixerView.body` sets `.environment(\.mixerPlacement, placement)` on its strips.

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
  - It applies the frame once, in `viewDidMoveToWindow`, through `PaneGeometry.sanitizedFrame(model.editor.layout.views.frame, screens: NSScreen.screens.map(\.frame))`.
  - It observes `NSWindow.didMoveNotification` and `NSWindow.didEndLiveResizeNotification` for that window, with `queue: .main`, and saves through `model.editor.setDocumentFrame`.
  - **Swift 6.1:** the observer closure captures `let editor = model.editor` and `let window = w`, both strong `let`s made before `MainActor.assumeIsolated { editor.setDocumentFrame(…) }`. It never captures a weak `self`.
  - The observers are removed in `dismantleNSView`.

- [ ] **Step 6: Run the gates.** `scripts/build-ffi.sh && scripts/swift.sh build && scripts/swift.sh test` (1031, no new warnings), then `scripts/make-app.sh`. Run the existing e2e now (`VIZDAW_E2E_APP=dist/VizDAW.app cargo test -p vizctl --test e2e -- --ignored --test-threads 1`). Expected: 14 passed. `click_play_and_press_space` finds Play in the transport bar. A failure here is a layout fault to fix in this task, not in the test.

- [ ] **Step 7: Commit** the seven `VizDAW` files. Message: "app: one window per document: the transport bar, layout A's panes, the Mix view, the mixer window's transport, the View menu, the saved frame (plan 7a Task 7)", with the trailers.

---

### Task 8: The pane contents

**Files:**
- Create: `app/Sources/VizDAW/PaneViews.swift`
- Modify: `app/Sources/VizDAW/DocumentShell.swift` (`PaneHost` switches to the views below), `app/Sources/VizDAW/MediaPoolWindow.swift` (`MediaPoolView.compact`), `app/Sources/VizDAW/PluginBrowserView.swift` (`width`)

**Interfaces:**
- Consumes: Tasks 3, 4 and 7, `SequenceSidebar(model:)`, `DetailArea(model:docID:)`, `StripView(model:strip:)`, `StripChartEditor(model:)`, `MediaPoolView(media:)`, `PluginBrowserView(model:pick:)`, `PluginBrowserModel(kind:_:)`, `appPlugins.browser(_:)`.
- Produces:
  - `struct LeftStripPane: View { let model: DocumentModel; let docID: UUID }`. It shows `StripView` for `.strip(id)`, `InspectorPane` for `.inspector`, and `StripFocus.nothingMessage` for `.nothing`. It polls with `mixer.tick(now:)` every 33 ms, sets `.environment(\.mixerPlacement, .strip)`, and attaches the maps sheet for `.strip`;
  - `struct InspectorPane: View { let model: DocumentModel; let docID: UUID }`, a two-column `Grid` of `Inspector.rows(…)`, or `Inspector.emptyMessage`;
  - `struct MarkersPane: View { let model: DocumentModel }`, a `List` of `MarkerList.rows`. A row's click calls `MarkerList.go`. With no rows it says "No markers or locators.";
  - `struct ControllersPane: View { let model: DocumentModel }`, `StripChartEditor(model: model.controllersChart)`, or "Select a lane to see its controllers.";
  - `struct PluginsPane: View { let model: DocumentModel; let docID: UUID }`, `PluginBrowserView(model:…, width: nil)` for effects. A pick puts `PaneContent.insertPlugin`'s refusal in `model.editor.status`;
  - `MediaPoolView(media:compact:)`, where `compact` defaults to false;
  - `PluginBrowserView(model:width:pick:)`, where `width` defaults to 320.

- [ ] **Step 1: Write the views.** Each `PaneView` gets exactly one host:

  | `PaneView` | Content |
  |---|---|
  | `.strip` | `LeftStripPane` |
  | `.sequences` | `SequenceSidebar` |
  | `.inspector` | `InspectorPane` |
  | `.editor` | `DetailArea` |
  | `.mixer` | `MixerView(placement: .pane)` with `openPluginWindow` |
  | `.controllers` | `ControllersPane` |
  | `.mediaPool` | `MediaPoolView(media: model.media, compact: true)` |
  | `.plugins` | `PluginsPane` |
  | `.markers` | `MarkersPane` |

  - A click in a plug-in slot opens its plug-in window as today (spec §2 item 4).
  - The media pool's Delete, Rename and Relink stay on their buttons and menu, which open their panels as today. No programmatic path reaches them.

- [ ] **Step 2: Run the gates:** Swift build and test (1031), `scripts/make-app.sh`, then the e2e (14 passed).

- [ ] **Step 3: Commit** the four files. Message: "app: the panes' views: strip, sequences, inspector, editors, mixer, controllers, media pool, plug-ins, markers (plan 7a Task 8)", with the trailers.

---

### Task 9: The e2e, the by-eye steps, the root README and the full gates

**Files:**
- Modify: `crates/vizctl/tests/e2e.rs`, `app/README.md`, `README.md`

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
       - `state.get {target: "panes"}` → `invalid_params`.
     - After them `state.get layout` is unchanged, `state.get windows` lists the same numbers and kinds, and `alerts` is empty: no dialog opened.
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
say "the sidebar", it is now Left Pane › Sequences.

199. File > Create Demo Project. The window shows the transport bar along
     the top (Play, Loop, Record, the position, Arrange | Mix, three pane
     buttons), the channel strip on the left running the full height, the
     arrangement in the centre and the "No Editor Open" pane below it. The
     right pane is hidden.
200. Press ⌃2: the mixer fills the window under the transport bar, with no
     arrangement. Press ⌃1: the arrangement comes back with the panes as
     they were. (If macOS switches desktops instead, Mission Control owns
     ⌃1/⌃2: rebind Arrange View and Mix View in Settings › Keys › View.)
201. Press ⌃⌘→: the right pane opens beside the bottom pane, under the
     arrangement. Press ⌃⌘↓: the bottom pane hides and the right pane runs
     beside the arrangement for the full height. Press ⌃⌘↓ and ⌃⌘← twice:
     each pane hides and comes back with the same view and size. The View
     menu's Left, Bottom and Right Pane items show the same check marks.
202. Drag the line between the arrangement and the bottom pane up and down:
     the pane resizes and stops at its smallest and largest size. Shrink
     the window to its smallest: the arrangement stays usable and the right
     pane hides first; grow it again: the right pane comes back.
203. Click the Chords lane header: the left pane shows the Built-in Synth's
     strip. Move its fader: the mixer (⌥⌘M) and the Mix view (⌃2) show the
     same level, and the meters fall at the same speed in all three. A lane
     playing to external MIDI shows the Inspector instead, naming its
     destination.
204. Open each pane's picker: the left lists Channel Strip, Sequences and
     Inspector; the bottom lists Piano Roll, List, Strip Chart, Drum Grid
     and Mixer (the Mix view's bottom has no Mixer); the right lists
     Controllers, Media Pool, Plug-ins and Markers. Choose each one: each
     shows its view, and nothing opens a dialog. Press ⌘1: the bottom pane
     shows the piano roll on the selection; ⌥⌘1 still opens it in a window.
205. In the right pane, Plug-ins: with the Chords lane selected, choose an
     effect: it appears as the last insert on the left strip and in the
     mixer; click its slot: its window opens floating. Markers: click a
     row: the position moves there. Controllers: the Strip Chart shows the
     selected lane's controller.
206. Window > Mixer opens the mixer in its own window with its own
     transport bar: Play there plays the document. Set the Mix view's
     right pane to Markers, save, close and reopen the project: both views
     come back as you left them, and the window at the same size and place.
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
    - control surfaces (Mackie Control and Kontrol S-Series MK3) and MIDI learn;
    - bounce to WAV or AIFF;
    - an external sample editor round trip;
    - MIDI 2.0 throughout, on Apple Silicon with macOS 14+.

    Then "Planned": Standard MIDI File import; OMF and AAF import, which may come after the first release.
  - Add "## Getting around", in at most 10 lines: one window per project; the transport bar; the Arrange (⌃1) and Mix (⌃2) views; the left, bottom and right panes, their pickers and ⌃⌘←/↓/→; ⌘1–⌘4 for the editors; Window > Mixer for a separate mixer window; keys can be changed in Settings › Keys.
  - Add "## Building from source": what you need (Rust stable, the Xcode Command Line Tools, macOS 14+), and the four commands from `app/README.md`'s "Build and run" block, with a link to `app/README.md` for more.

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
- Swift 1031;
- e2e 15 passed;
- registry 153;
- `pgrep` and `ls` print nothing.

- [ ] **Step 6: Commit** `crates/vizctl/tests/e2e.rs`, `app/README.md` and `README.md`. Message: "e2e: views and panes over the channel; README 199-206; the root README's status, features, getting around and building (plan 7a Task 9)", with the trailers. Counts: ignored +1 (19), e2e 15.

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
   - §2 item 4, floating windows kept (`window.mixer`, `window.media_pool`, plug-in windows, sheets, torn editors): Tasks 7–8, and the e2e step 6.
   - §2 item 8, for the left pane: Task 3.
   - §2 item 11, 7a's README part: Task 9.
   - §4, nothing hidden: the sequence list stays reachable (Ruling 12), and every sheet stays on `DocumentView`. The channel and the e2e work: Tasks 5, 6 and 9.
   - §5, the keys: Ruling 4.
   - §6.1 and §6.3: Rulings 1, 11 and 12.
   - Out of scope, as asked: themes, the strip redesign and icons (7b); the audio editor, left out of the list (Ruling 3; 7c); clean-up (7d).
2. **Step scan.** Every test step gives names and assertions. Every code step gives signatures, files and the pinned values. View tasks (7–8) give the structure and the exact strings, and are checked by the e2e and README 199–206. Task 5's mid-plan `handled` gap is settled by its executor ruling.
3. **Type consistency.**
   - `PaneView`'s raw names are the channel's names, and `ViewCommands.viewName` adds the editor kind names (`ControlStates.kindName`). Task 5's `view` Choice lists exactly that union.
   - `MixerPlacement` (Task 1) is used by `LearnModel` (Task 3), `MixerView` (Task 7) and `LeftStripPane` (Task 8).
   - `StripFocus.Result` (Task 3) feeds `ControllersPane`, `insertPlugin` (Task 4), `layoutRow`'s `left_strip` and `left_shows` (Task 6), and `LeftStripPane` (Task 8).
   - `LayoutStateRow`'s field names match the JSON keys (Task 5) and Swift's generated `leftStrip` and `leftShows` (Task 6).
4. **Review Focus.** Each of the five lines names its tests in their tasks.
5. **Proportion.** About 1200 short lines, against a 137-line spec whose 7a row spans §2 items 1–4, 8 and 11. The code blocks are tests and the exact README text. Bodies are left to the signatures.

## Execution handoff

Plan complete and saved to `docs/superpowers/plans/2026-11-03-window-views-panes.md`. The controller runs it subagent-driven, as in SP5, with one reviewer per task. Tasks 7 and 8 are view work that only the e2e and the owner's by-eye steps check, so their reviews should read `DocumentShell` against Ruling 2 and Ruling 8 line by line.
