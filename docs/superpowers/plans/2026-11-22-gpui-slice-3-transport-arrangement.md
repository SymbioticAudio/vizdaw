# 27.1.0 Slice 3: The Transport and the Arrangement View Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The GPUI app gains the whole transport that does not need recording (every non-record `transport.*` command, Return to Start / Rewind / Fast Forward with hold-to-wind and ⇧-click to markers, Loop, the four readouts, the Position and Edit In/Out fields, the customizable bar with » folding) and the arrangement canvas (ruler, grid, blocks on each track row, a moving playhead, click-to-locate, ruler range selection, scroll and zoom), plus the control-channel calls the transport e2e tests need (`wait`, `ui.key`, `ui.click` modifiers, `ui.screenshot`), with four Swift e2e tests ported to the GPUI app.

**Architecture:** As in slice 2: behaviour lives in `vizdaw-app` (no GPUI). `DocSession` grows the transport, marker, loop, sequence-info and block calls; `TransportModel`/`DocumentModel` gain looping, locate, bar steps, markers, the shuttle state machine, the playhead clock and the "a text field has the keyboard" gate; a `transport_bar` module holds the bar layout (items, parse, fit) and its preference; `commands` runs the transport and `ui.*_transport_bar` IDs. `vizdaw-gpui` draws the bar from the layout and an `Arrangement` canvas element beside the track headers, and the control channel answers the new calls by reusing `core_control::{request, keys, wait}`.

**Tech Stack:** Rust 2024; GPUI at the slice 1 pin (`crates/vizdaw-gpui/Cargo.toml`, zed rev `f4fe0be1`); `core-ffi`, `core-control`, `core-editor` (via core-ffi), `vizctl`.

**Spec:** `docs/superpowers/specs/2026-11-20-cross-platform-27-1-design.md` (§8, §10). Parity checklist: `docs/superpowers/specs/2026-11-22-parity-checklist.md`. Slice 2 plan and its Outcome (rulings and deferred minors this slice touches: the Linux menu bar shows ⌘ for Ctrl; `ui.tree` is draw-time only; a channel click on a scrolled-out track header can land on the transport bar): `docs/superpowers/plans/2026-11-22-gpui-slice-2-commands-undo-tracks.md`. Swift reference: `app/Sources/VizDAWAppModel/{TransportCommands,TransportShuttle,TransportClock,TransportFields,TransportItems,TransportReadouts,TransportLayoutCommands,DocumentModel,ControlCommands,KeyBindings}.swift`, `app/Sources/VizDAWEditors/{EditorCanvas,ArrangementView,ArrangementLayout,TimeAxis,Scrolling,Grid,TransportReadoutsView,TransportFieldsView,TransportRowView}.swift`, `app/Sources/VizDAW/{TransportItemViews,ArrangementPane,ControlChannel}.swift`. Research notes with file:line citations, exact strings and the Swift test names per area: the slice 3 inventory (controller scratch; the relevant section is copied into each brief).

## Global Constraints

- Song format unchanged (v11).
- `vizdaw-app` never depends on GPUI.
- Command IDs, titles, default shortcuts and parameter names come from `core_control::registry::COMMANDS` and `core_control::bindings::Keymap`. Request parsing and validation for `wait`, `ui.key`, `ui.click`, `ui.screenshot` and `command.run` params come from `core_control::request`; key names from `core_control::keys`; wait conditions from `core_control::wait` (`WaitCondition`, `Facts`, `met`, `timeout_details`). Never re-typed.
- `command.run` keeps the Swift contract (`{command, doc, window, undo, status}`; error codes and Swift wording quoted in the tasks).
- `state.get transport` and `state.get windows` keep the Swift JSON shape (`core-ffi/src/control.rs` `transport_json` and the windows arm): `transport` gains `click` (`null` until the click is ported), `windows` gains `theme`.
- Tree labels and values match the Swift accessibility labels exactly: "Return to Start", "Rewind", "Fast Forward", "Play"/"Stop", "Loop", "Bar Readout", "Time Readout", "Tempo Readout", "Signature Readout", "Position", "Edit In", "Edit Out", "More Transport Items", "Arrangement". Readouts and fields expose their text as the node's `value`; fields expose `focused`.
- Real-time rule: nothing on the interface thread blocks on the engine; the playhead moves by extrapolating the last poll (Swift `TransportClock`), never by polling every frame.
- Recording stays out: `transport.record`, `transport.arm_*`, `record.*`, Punch fields, Record cluster, overdub/punch toggles and the metronome click (`transport.click*`) are later slices; their menu items show disabled (handler-less, as slice 2's `UnavailableCommand`).
- Single-key shortcuts carry `!TextInput` in their key context; the transport fields are `TextInput`s.
- Every clickable control records role, label and bounds in the element registry while drawing. Control calls handled per frame stay capped at 16 per pump tick.
- Subagents: no GUI, no computer-use tools, never `dangerouslyDisableSandbox`. Visible checks and e2e runs are done by the controller (Mac) or over SSH on the owner's Linux box (192.168.1.192; XWayland `DISPLAY=:0`, `XAUTHORITY=/run/user/1000/xauth_*`).
- Banned vendor/DAW names stay out of repo text (licence notices exempt). UI says "track", never "lane". Never commit `.DS_Store`, `.superpowers/`, generated FFI files or brand assets.
- CI: `cargo fmt --all -- --check`, clippy `-D warnings`, tests on macOS and Linux runners.
- Commit trailers: `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>` and `Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P`.

## Review Focus

1. Bare transport keys while a field (Position, Edit In/Out, a track rename) has the keyboard: the key types into the field and no command runs; over the channel the command answers `unavailable` "A text field has the keyboard." (Swift `bareKeyCommandsWaitForTextFields`) — Tasks 1 and 3.
2. A field edit that is abandoned (Escape, a click on another view, the window losing focus, the bar refolding so the field hides) shows the value in use again and commits nothing; Return commits once (Swift `clickingAwayCancelsATypedField`, `foldedFieldsAreHiddenNotRemade`) — Task 3.
3. Hostile positions and zoom: a typed position beyond ±2^40 ticks, NaN or zero window widths, `ticksPerPoint` at its 2 and 61 440 limits, origins near ±2^50 — no panic, clamped (Swift `extremeOffsetsClampInsteadOfTrapping`, `visibleRangeWithNaNWidthDoesNotTrap`, `hostileTransportValuesReadAsDashes`) — Tasks 1, 2 and 4.
4. A held Rewind/Fast Forward that is interrupted (a text field takes the keyboard mid-hold, the core refuses a wind, the window closes) stops winding and lands once (Swift `aTextFieldTakingTheKeyboardMidHoldStopsTheHold`, `aRefusedWindStopsTheHoldAndSaysWhy`) — Tasks 1 and 3.
5. A `wait` that never comes true times out with the Swift `timeout` error and details, and many waits at once do not stall the pump or other calls (Swift waits on a repeating `later`) — Task 5.

---

### Task 1: core-app — transport calls, commands, shuttle and the playhead clock

**Files:**
- Modify: `crates/vizdaw-app/src/{session.rs,transport.rs,document.rs,commands.rs,testing.rs}`
- Test: `crates/vizdaw-app/tests/transport.rs`, new `crates/vizdaw-app/tests/transport_commands.rs`, new `crates/vizdaw-app/tests/shuttle.rs`

**Interfaces:**
- Consumes: slice 2 `DocSession`, `DocumentModel` (`status`/`set_status`, `selected_seq`, `selection`), `commands::{run, enabled, perform_from_menu, Ran}`.
- Produces (exact):
  - `DocSession` adds, forwarded to `core_ffi::Session`: `set_loop(&self, on: bool) -> Result<(), VizError>`, `marker_step(&self, seq: u64, tick: i64, forward: bool) -> Result<Option<i64>, VizError>`, `transport_step_bar(&self, forward: bool) -> Result<i64, VizError>`, `transport_key_step(&self, forward: bool) -> Result<i64, VizError>`, `transport_wind(&self, quarters: f64) -> Result<i64, VizError>`, `transport_hold_end(&self) -> Result<i64, VizError>`, `selection_span(&self, seq: u64) -> Result<Option<TickRange>, VizError>`, `sequence_info(&self, seq: u64) -> Result<SequenceInfo, VizError>`, `blocks(&self, seq: u64, lane: u64, rule: BlockRule) -> Result<Vec<BlockRow>, VizError>`. `FakeSession` scripts and records each.
  - `DocumentModel` adds: `pub fn looping(&self) -> bool`, `pub fn toggle_loop(&mut self)` (alert "Loop Failed"), `pub fn locate(&mut self, tick: i64)` (alert "Locate Failed"), `pub fn move_to(&mut self, tick: i64) -> Result<(), VizError>` (the bar button path; the caller shows the error in the status), `pub fn return_to_start(&mut self)`, `pub fn sequence_info(&self, seq: u64) -> Option<&SequenceInfo>` (cached per snapshot version), `pub fn begin_text_editing(&mut self, owner: &str)`, `pub fn end_text_editing(&mut self, owner: &str)`, `pub fn transport_keys_enabled(&self) -> bool`, `pub fn return_key_enabled(&self) -> bool`. Slice 2's track rename becomes a text-editing owner (`"track-rename"`).
  - `commands::TRANSPORT_COMMANDS: &[&str]` = `transport.play_stop`, `transport.stop`, `transport.play`, `transport.play_selection`, `transport.play_from_selection`, `transport.play_from_edit_in`, `transport.scrub`, `transport.return_to_start`, `transport.rewind`, `transport.fast_forward`, `transport.previous_marker`, `transport.next_marker`, `transport.loop`, `transport.go_position`, `transport.go_edit_in`. `commands::run`/`enabled`/`perform_from_menu` handle them.
  - `pub enum FieldFocus { Position, EditIn }` and `Ran.focus: Option<FieldFocus>`: `go_position`/`go_edit_in` succeed by asking the UI to focus that field (the UI answers `unavailable` "The window shows no transport fields." when it is folded or absent).
  - `transport::Shuttle` (port of `TransportShuttle.swift`): `press(forward: bool, shift: bool, at: Instant)`, `tick(at: Instant, doc: &mut DocumentModel)` (called every 100 ms while held), `release(inside: bool, at: Instant, doc: &mut DocumentModel)`, `cancel(doc: &mut DocumentModel)`, `key_down(forward: bool, doc: &mut DocumentModel)`, `key_up(doc: &mut DocumentModel)`. Nothing winds below 0.4 s; the wind amount is `core_ffi::surfaces::shuttle_quarters(from_ms, to_ms)` (use the path the crate exports).
  - `transport::Clock` (port of `TransportClock.swift`): `sample(&mut self, tick: i64, playing: bool, seq: Option<u64>, tps: f64, at: Instant)`, `tick_at(&self, at: Instant) -> i64`; extrapolates at most one poll interval past the sample, never steps back less than 0.1 s of ticks, snaps on locate, loop wrap, stop and sequence change.
  - `TransportModel::readout` fills tempo and signature from `sequence_info` (the slice 1 formats `tempo_text`, `signature_text` stay).

Behaviour to port (Swift `TransportCommands.swift`, `ControlCommands.swift:684-724`, `DocumentModel.swift:578-812`; exact strings): `transport.play_stop` needs a sequence ("Select a sequence to play.") and the transport keys ("A text field has the keyboard."); stop here does not locate back. `transport.stop` = stop and return to where playing started; stopped → `unavailable` "Nothing is playing.". `transport.play` only starts; at or past the sequence end it restarts at 0. `play_selection`/`play_from_selection` → "Select something in the shown sequence first."; `play_from_edit_in` → "Set an edit-in point (select a time range) first."; `scrub` → "Scrubbing is not available yet." and disabled. `return_to_start` → "A text field or list has the keyboard." when the Return key is not free. `rewind`/`fast_forward` step one bar (stop at 0); `previous_marker`/`next_marker` → "No marker before the position." / "No marker after the position."; no sequence → "Select a sequence first."; `Busy` → `busy`; core refusals → `failed` with the core text. `transport.loop` takes optional `on` (absent toggles). Every listed ID refuses any other params as `invalid_params` (the registry's `params`).

- [ ] **Step 1: Write the failing tests**, ported one for one from Swift with snake_case names: `TransportCommandsTests` (all except `recordArmMovesAndARefusalShows`, `clickTogglesAndShowsItsCheck`, `clickLevelAndModeReachTheSession`, `locatorsAreSetJumpedToAndCleared`, `locatorRefusalsMenusAndStopMatchTheirRun`, `modesToggleOrTakeTheirFlag`, `keyBindingFilesNeedAPathAndNeedNoDocument`), all of `TransportShuttleTests`, all of `TransportClockTests`, `TransportReadoutsTests.theReadoutsReadThePositionTimeTempoAndMeter`, `theSignatureFollowsTheMeterMap`, `hostileTransportValuesReadAsDashes`; plus `every_transport_command_is_in_the_registry` and `transport_commands_refuse_extra_params` (`transport.rewind {bars: 2}` and `transport.fast_forward "x"` → `invalid_params`, no session call recorded).
- [ ] **Step 2: Run** `cargo test -p vizdaw-app` → new tests fail to compile.
- [ ] **Step 3: Implement** the trait additions, forwards, fake scripting, model methods, commands, `Shuttle` and `Clock`.
- [ ] **Step 4: Run** `cargo test -p vizdaw-app` → all pass, slice 1–2 tests unchanged.
- [ ] **Step 5: Commit**: `git commit -m "core-app: transport commands, shuttle, loop and the playhead clock"`.

### Task 2: core-app — the transport bar layout and its preference

**Files:**
- Create: `crates/vizdaw-app/src/transport_bar.rs`, `crates/vizdaw-app/src/prefs.rs` (only if `vizdaw-app` has no preferences store yet; check `store.rs` first and extend what exists)
- Modify: `crates/vizdaw-app/src/{lib.rs,commands.rs}`
- Test: new `crates/vizdaw-app/tests/transport_bar.rs`

**Interfaces:**
- Produces (exact):
  - `pub enum TransportItem { Cluster, Loop, Click, ReplaceOverdub, RecordOptions, CountIn, Summary, Bar(Size), Time(Size), Tempo(Size), Signature, PositionField, EditFields, PunchFields, DspLoad, Device, Faults, MainView, Panes, Space, Separator, Command { id: String, label: Option<String> } }`, `pub enum Size { Small, Large }`; tokens exactly as Swift `TransportItems.swift:60` (`cluster`, `loop`, `click`, `replace_overdub`, `record_options`, `count_in`, `summary`, `bar[:small|:large]`, `time[…]`, `tempo[…]`, `signature`, `position_field`, `edit_fields`, `punch_fields`, `dsp_load`, `device`, `faults`, `main_view`, `panes`, `space`, `separator`, `command:<id>[:<label>]`).
  - `pub struct TransportLayout { pub items: Vec<TransportItem> }` with `standard()` (Swift `TransportLayout.standard`), `parse(&str) -> Result<Self, String>` (Swift errors verbatim: `unknown item X`, `an empty item`, `X has no size`, `at most 64 items`, exactly one `cluster`, `command needs a command id: command:<id>`, label 1–32 chars), `tokens(&self) -> String`.
  - `pub fn fit(widths: &[(f32, bool /*fixed*/, ItemKind)], width: f32, spacing: f32, chevron: f32) -> Fit { shown: Vec<usize>, folded: Vec<usize>, hidden_separators: Vec<usize> }` (port of `TransportRowFit`: fold last first; fixed never folds; spaces/separators never listed as folded; a separator by an edge, the », or another separator hides; NaN/≤0 width folds all but fixed; +∞ folds nothing).
  - Preference key `transportBar` stored with the app's preferences under the launch home (so e2e temp homes are isolated); a damaged value gives the standard layout and is kept (Swift `aDamagedPreferenceGivesTheDefaultAndIsKept`); a stored standard follows future defaults.
  - `commands` handles `ui.set_transport_bar {items}` and `ui.reset_transport_bar` app-wide (no document): `invalid_params` `"ui.set_transport_bar: <msg>"`; missing `items` → ``ui.set_transport_bar: `items` lists the bar's items, such as "cluster,loop"``; status `"transport bar: N items"` / `"transport bar: the default"`.
  - Items that this slice does not draw (`click`, `replace_overdub`, `record_options`, `count_in`, `summary`, `punch_fields`, `dsp_load`, `device`, `faults`, `main_view`, `panes`, `command`) parse and persist but draw nothing in Task 3 (ledgered as later-slice rows).
- [ ] **Step 1: Write the failing tests**, ported from `TransportRowTests`: `nothing_is_cut`, `folds_the_last_item_first`, `spaces_and_separators_are_never_listed`, `the_default_layout_is_todays_bar`, `a_damaged_preference_gives_the_default_and_is_kept`, `a_stored_standard_follows_the_default`, `items_text_parses`, `set_transport_bar_over_the_channel` (through `commands::run`), plus `fit_survives_nan_zero_and_infinite_widths`.
- [ ] **Step 2: Run** `cargo test -p vizdaw-app --test transport_bar` → fails to compile.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p vizdaw-app` → pass.
- [ ] **Step 5: Commit**: `git commit -m "core-app: transport bar layout, folding and its preference"`.

### Task 3: GPUI — the transport bar and the Transport menu

**Files:**
- Modify: `crates/vizdaw-gpui/src/{transport_bar.rs,shell.rs,menus.rs,registry.rs,text_input.rs,app_state.rs}`
- Test: `crates/vizdaw-gpui/tests/transport.rs` (new), `crates/vizdaw-gpui/tests/menus.rs`

**Interfaces:**
- Consumes: Tasks 1–2.
- Produces: `registry::RegistryScope::control_with(role, label, enabled, value: Option<&str>, focused: bool)` (the tree JSON gains `value` and `focused` when set; existing `control` keeps working); the bar drawn from `TransportLayout` with: cluster = Return to Start, Rewind, Fast Forward, Play/Stop (label flips; Swift has no separate "Stop and Return" button: remove it, `S` stays `transport.stop`); Loop toggle; readouts Bar, Time, Tempo, Signature (role `text`, value = the readout text, fixed widths for the widest texts "9999.4.960", "999:59.999", "999.99", "255/128"); Position field and Edit In/Edit Out fields (`TextInput`); the » button "More Transport Items" (a channel click sets the status "The » menu does not open for the control channel; use ui.set_transport_bar."; a real click opens a GPUI menu of the folded items).

Behaviour to port (Swift `TransportItemViews.swift`, `TransportFieldsView.swift`, `TransportRowView.swift`): Rewind/Fast Forward drive `Shuttle` (press on mouse-down, a 100 ms timer while held, release on mouse-up, ⇧-click = marker step with the outcome shown in the status line); held ⇧, / ⇧. keys drive `key_down`/`key_up`. Return to Start is disabled with no sequence and shows its error in the status. Fields: one click selects all; a click inside an editing field places the caret; Return/Tab/⇧Tab commit (only when the text changed), Tab moves to the next field; Escape, a click outside, the window losing focus or the field folding away cancels; while editing the field is a text-editing owner (`begin_text_editing("transport")`); bad text → status `“<text>” is not a position (bar.beat.unit).`; Edit Out → "Set Edit In first." / "Edit Out must be after Edit In.". Parsing uses the Rust `PositionInput` the Swift app already calls (find it via `core_ffi`). Keys P and E (`go_position`, `go_edit_in`) focus their field with all text selected. The Transport menu lists every `transport.*` registry ID in registry order; record/click/overdub/punch/arm items are disabled. Keypad −/+ (the `extra_keys` of rewind/fast_forward): GPUI cannot tell the keypad from the main keys at this pin; ledger it as a Linux/Mac gap for the keyboard, and Task 5 maps the channel's `keypad_minus`/`keypad_plus` to the commands.

- [ ] **Step 1: Write the failing tests** (GPUI test context, as slice 2's `tests/window.rs`): `the_bar_names_its_controls_as_the_swift_bar` (tree labels and values above, from the demo), `fast_forward_click_steps_a_bar`, `shift_click_fast_forward_without_markers_says_so`, `a_hold_winds_and_lands_once`, `loop_toggles_and_shows_on`, `the_first_click_into_a_field_selects_all`, `return_commits_the_typed_position`, `escape_and_clicking_away_cancel_the_field`, `a_field_that_folds_away_cancels_its_edit`, `bare_keys_wait_while_a_field_has_the_keyboard`, `a_narrow_window_folds_the_last_items_into_more`, `the_transport_menu_lists_every_transport_command` (in `tests/menus.rs`; record items disabled).
- [ ] **Step 2: Run** `cargo test -p vizdaw-gpui` → fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p vizdaw-gpui` and `cargo clippy -p vizdaw-gpui --all-targets -- -D warnings` → pass.
- [ ] **Step 5: Commit**: `git commit -m "gpui: the transport bar, fields and the Transport menu"`.

### Task 4: GPUI — the arrangement canvas

**Files:**
- Create: `crates/vizdaw-gpui/src/arrangement.rs`, `crates/vizdaw-app/src/arrangement.rs` (geometry, time axis, grid, scrolling maths: no GPUI)
- Modify: `crates/vizdaw-gpui/src/{shell.rs,track_headers.rs,lib.rs}`, `crates/vizdaw-app/src/lib.rs`
- Test: new `crates/vizdaw-app/tests/arrangement.rs`, new `crates/vizdaw-gpui/tests/arrangement.rs`

**Interfaces:**
- Consumes: Task 1 (`blocks`, `sequence_info`, `Clock`, `locate`, selection), slice 2 `TrackHeaders` (shares the vertical offset).
- Produces: in `vizdaw-app::arrangement`: `TimeAxis { ticks_per_point: f64, origin: i64 }` (default 384; clamp 2..=61_440; origin clamp ±2^50; user scroll never left of 0) with `x(tick)`, `tick_at(x)` (floor), `zoom_about(factor, x)`, `scrolled_by(points)`, `visible_range(width)`; `Geometry { lane_height: f32 /*48*/, vertical_offset: f32 }` with `row_top`, `block_rect` (inset 2, min width 4), `content_height(rows)` (rows + 1 for the Add Track row); `grid_lines(axis, width, meter) -> Vec<GridLine>` (beat lines only ≥ 6 pt apart; bar lines thinned to every 2^n-th bar; max 4000) and `label_every`; `PageFollow` (margin 16), `AutoScroll` (edge 24, rate 20, max 1200 pt/s), `RulerSelection::range`, `WheelScroll` (lines = 12 pt; zoom 1.01^dy precise, 1.1^dy notched); `a11y_label(block, lane_name)` = Swift `ArrangementA11y.label` verbatim. The block rule is `silenceGap(beats: 4)` (Swift default).
- GPUI `Arrangement` view: ruler 22 px, grid, row separators, blocks (rounded rect fill and stroke, a mini-note sketch when wider than 24 px), reference blocks "↻ <name>", the selection range shaded, a 1 px playhead drawn from `Clock::tick_at` on every animation frame while playing (shown only when the transport's sequence is the shown one), page-follow while playing. Registry: a `group` node labelled "Arrangement" whose children are one `button` per visible block with the a11y label. Mouse: a click takes keyboard focus (ending any field edit — Task 3's click-away rule); a ruler click locates (snapped when Snap is on); a ruler drag sets the time selection (auto-scroll near the edges); the wheel scrolls (vertical shared with the headers), ⌘-wheel (Ctrl on Linux) zooms about the pointer; zoom out/in buttons ×1.5 above the canvas. Escape cancels a ruler drag. Block editing (select, drag, open, erase, menus) is a later slice: blocks are drawn and listed, not editable. Light mode, audio regions and takes are later slices (ledger).
- [ ] **Step 1: Write the failing tests**: in `vizdaw-app` ported from `TimeAxisTests`, `ScrollingTests`, `GridTests`, `ArrangementLayoutTests` (`blocks_sit_inside_their_rows`, `accessibility_labels_name_kind_lane_and_bars`; skip hit-testing and light mode); in `vizdaw-gpui`: `the_arrangement_is_in_the_tree_with_its_blocks` (demo), `a_ruler_click_locates`, `a_ruler_drag_selects_a_range`, `locating_while_stopped_moves_the_playhead` (Swift `ArrangementViewTests:57`), `wheel_scroll_moves_headers_and_canvas_together`, `the_headers_and_add_row_show_beside_the_canvas`.
- [ ] **Step 2: Run** `cargo test -p vizdaw-app --test arrangement && cargo test -p vizdaw-gpui --test arrangement` → fail.
- [ ] **Step 3: Implement.** Replace the slice 1 placeholder fields column in `shell.rs` (and drop the `add_field` test hook the slice 1 outcome deferred, if nothing else uses it).
- [ ] **Step 4: Run** `cargo test -p vizdaw-app -p vizdaw-gpui` and clippy → pass.
- [ ] **Step 5: Commit**: `git commit -m "gpui: the arrangement canvas with ruler, blocks and playhead"`.

### Task 5: GPUI control — wait, keys, modifier clicks, screenshots and transport commands

**Files:**
- Modify: `crates/vizdaw-gpui/src/{control.rs,shell.rs,app_state.rs}`; new `crates/vizdaw-gpui/src/screenshot.rs`
- Test: `crates/vizdaw-gpui/tests/control.rs`

**Interfaces:**
- Consumes: Tasks 1–4.
- Produces:
  - `wait`: parsed by `core_control::request`; each pump tick builds `core_control::wait::Facts` (documents, window titles, the target document's `playing`, `tick`, `detail_kind` = `"none"` this slice) and replies `{condition, elapsed_ms}` when `met`; deadline → `timeout` with `timeout_details`. Pending waits are held in a list and checked every pump tick (no blocking; they do not count towards the 16-call cap).
  - `ui.key {window?, key | text, modifiers?, element?}`: `element` first clicks it; each stroke from `core_control::keys` is routed like a real key: through the focused text input when one has the keyboard, else through the keymap to a `RunCommand` (a command failing with `unavailable`, `no_document` or `unknown_id` is delivered as a plain key, Swift `channelDelivers`; other failures end the call). `keypad_minus`/`keypad_plus` map to `transport.rewind`/`transport.fast_forward` directly (Task 3 gap). Reply `{window, events, x, y}`.
  - `ui.click` honours `modifiers` (`shift`, `control`, `option`, `command`) and `count`; a click on a text field starts editing with all text selected.
  - `ui.screenshot {window?, path?, scale?, live?}` → writes a PNG atomically, replies `{window, path, width, height}` in pixels (≥ the window's point size × scale). macOS: GPUI's `Window::render_to_image` exists at this pin only behind `test-support`; first choice is a crate feature `screenshot` that enables it for the app build — check whether `test-support` pulls anything unacceptable into the shipped binary; if it does, use the platform window capture of the app's own window instead. Linux: GPUI has no `render_to_image`; capture the X11 window with `x11rb` `GetImage` (works under XWayland and Xvfb); on a native Wayland window answer `unavailable` "Screenshots need an X11 window on Linux." and add a Linux-gaps row. Ledger the choice.
  - `state.get transport` adds `click: null`; `state.get windows` adds `theme` (the current theme token; the slice 1 fixed theme's token until themes are ported).
  - `command.run` routes `TRANSPORT_COMMANDS` and `ui.set_transport_bar`/`ui.reset_transport_bar` through `commands::run`; a `Ran.focus` asks the window to focus that field, else `unavailable` "The window shows no transport fields.".
  - Fix the slice 2 deferred minor "a channel click on a scrolled-out track header can land on the transport bar": `ui.click` refuses an element whose bounds are clipped out of its scroll container with `failed` "<label> is scrolled out of view." (Swift clicks are by frame; this is the GPUI-side guard).
- [ ] **Step 1: Write the failing tests** in `tests/control.rs`: `wait_playing_answers_when_play_starts`, `a_wait_that_never_comes_true_times_out_with_details`, `many_waits_do_not_stall_other_calls`, `ui_key_space_plays_and_stops`, `ui_key_text_types_into_the_focused_field`, `ui_key_shift_dot_and_keypad_minus_step_bars`, `ui_click_shift_on_fast_forward_steps_to_the_marker`, `ui_screenshot_writes_a_png_or_answers_unavailable` (the test context has no window server: assert the parse errors, the path checks and the reply shape through a stub capture), `state_get_transport_has_click`, `state_get_windows_has_theme`, `transport_go_position_focuses_the_field`, `a_scrolled_out_element_is_not_clicked`.
- [ ] **Step 2: Run** `cargo test -p vizdaw-gpui --test control` → fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p vizdaw-gpui` and clippy (macOS; also `cargo clippy -p vizdaw-gpui --target x86_64-unknown-linux-gnu` is not available locally, so the controller checks Linux over SSH) → pass.
- [ ] **Step 5: Commit**: `git commit -m "gpui control: wait, keys, modifier clicks, screenshots and transport commands"`.

### Task 6: e2e — the transport tests against the GPUI app

**Files:**
- Modify: `crates/vizctl/tests/e2e_gpui.rs`

Port from `crates/vizctl/tests/e2e.rs`, using `wait` now that the GPUI app answers it, and accepting role `button` where the Swift test wants `AXButton`:
- `gpui_demo_play_screenshot_stop` ← `demo_play_screenshot_stop`, without its last step (`editors.list` + `wait detail_kind list`: the List editor is a later slice; ledger).
- `gpui_click_play_and_press_space` ← `click_play_and_press_space`.
- `gpui_loop_wraps_at_the_end` ← `loop_wraps_at_the_end`.
- `gpui_transport_jumps_readouts_and_fields_over_the_channel` ← steps 0–6 of `transport_jumps_readouts_and_fields_over_the_channel` (step 7 is Punch/Record: later slice; ledger).

- [ ] **Step 1: Write the four tests** (`#[ignore]`, need `VIZDAW_E2E_APP`).
- [ ] **Step 2: Build and run on the Mac (controller, screen unlocked)**: `scripts/make-gpui-app.sh && VIZDAW_E2E_APP=target/VizDAW-GPUI.app cargo test -p vizctl --test e2e_gpui -- --ignored --test-threads 1` → 8 passed (also re-runs slice 2's tracks test unlocked).
- [ ] **Step 3: Run on Linux (controller, over SSH)** with the slice 2 environment → 8 passed.
- [ ] **Step 4: Tick the parity checklist**: the 15 transport command rows, `ui.set_transport_bar`/`ui.reset_transport_bar`, the four e2e rows (noting the dropped List-editor step and step 7), and any Linux-gap rows from Tasks 3 and 5.
- [ ] **Step 5: Commit**: `git commit -m "e2e: transport tests against the GPUI app"`.

## Outcome (2026-11-22)

All six tasks done (commits e2095bfa..ae0438df). Evidence: workspace tests 4143 passed on macOS, 4012 on Linux; clippy `-D warnings` and fmt clean on both (with and without `--features screenshot`); GPUI e2e 8/8 on Linux (XWayland) and 8/8 on macOS with the screen locked (the control channel now draws a fresh frame before reading the tree, which also closes slice 2's "ui.tree is draw-time only" minor).

Final review fixes: ⇧Space/⌥Space typed into fields (539a810c); the arrangement shares its rows and builds only the rows in view (539a810c); rows exposed by a taller canvas are built (ae0438df).

### Rulings

- Task 1: Ruling: Shuttle::press takes doc (text-field refusal sets status at press) — matches Swift — none
- Task 1: Ruling: commands do not poll after running; UI poll timer refreshes — bar may lag one poll (≤250 ms) — e2e uses waits, fine
- Task 1: Ruling: control.rs routing of TRANSPORT_COMMANDS stays for Task 5 as planned — none
- Task 2: Ruling: implementer wrote tests and code in one pass (no RED seen) — reviewer to weigh test strength — weak tests could hide bugs
- Task 3: Ruling: PositionInput/Bbu were Swift-only; ported into vizdaw-app (position.rs, transport_fields.rs) with a direct core-model dependency — plan assumed a Rust one existed — extra ported code to keep in sync with nothing (Swift retired)
- Task 3: Ruling: Transport menu lists menu items only (key-only ids omitted, as Swift) — matches Swift — none
- Task 3: Ruling: S / transport.stop follow Swift (stopped → "Nothing is playing." status) — parity — none
- Task 4: Ruling: headers and canvas share one vertical offset (headers' own scroll removed; 22 px top band added as Swift) — parity — none
- Task 4: Ruling: add_field test hook dropped; window test types into Position instead — slice 1 deferred minor closed — none
- Task 4: Ruling: zoom buttons in a 32 px row above headers+canvas; Arrangement children are blocks only — none
- Task 5: Ruling: macOS screenshot = gpui render_to_image behind crate feature `screenshot` (enables gpui test-support; implementer audited: test helpers + deps only); Linux = x11rb GetImage, Wayland → unavailable — only route at this pin — test-support in shipped builds could change behaviour on a GPUI upgrade; re-audit on upgrade
- Task 5: Ruling: ui.key decides command vs plain key from the app keymap (as Swift), else GPUI dispatch_keystroke — keeps command failures and keypad keys — a binding GPUI handles differently from the table could diverge
- Task 6: Ruling: subagent writes and compiles the e2e tests; controller runs them (subagents have no GUI) — as slice 2 — none
- Task 6: Ruling: fix now rather than defer — control calls draw a fresh frame (public Window::draw) before reading the registry — e2e and Co-Producer must not depend on the window being presented — a draw per tree call costs a frame
- Final: Ruling: declined item — slice 2 ⌘-chords in WINDOW_CONTEXT while a field has focus (⌘Z undoes the song, as Swift) — stands, set in slice 2 — a field-level undo expectation would be unmet
- Final: Ruling: declined item — plugin idle every 250 ms while stopped — stands (slice 2) — slower plugin-state dirty mark while stopped
- Final: Ruling: declined item — ui.screenshot writes where the local client names — stands, Swift same, socket owner-only — none
- Final: Ruling: declined item — GPUI Window::draw outside a platform frame — stands on 8/8 e2e evidence both platforms — a GPUI upgrade could change it; e2e would catch
- Final: Ruling: residual regression introduced by the fix wave is fixed (not parked) — a blank lane after a status line clears is a frequent visible defect — one extra fixer round beyond the single wave

### Deferred minors

- Task 1: minor (deferred): refused sequence_info (Busy) cached as None until next snapshot
- Task 1: minor (deferred): Clock poll cap fixed at 33 ms instead of current poll interval
- Task 1: minor (deferred): no poll after shuttle actions (bar lags ≤ one poll)
- Task 1: minor (deferred): fake transport_hold_end cannot be scripted to fail
- Task 1: minor (deferred): ms_since truncates (Swift rounds)
- Task 1: minor (deferred): stale module doc in document.rs ("transport comes with Task 3")
- Task 1: minor (deferred): play_stop reads transport up to 5 times
- Task 1: minor (deferred): TransportModel.state lags shared state one poll after channel commands
- Task 1: minor (deferred): DocumentModel::poll_transport reads do not feed the playhead Clock
- Task 2: minor (deferred): "other keys survive a write" comment untested; non-JSON prefs file untested; write-failure rollback untested
- Task 2: minor (deferred): run_app unknown id says "There is no command X." (Swift "VizDAW has no handler for X.")
- Task 2: minor (deferred): standard tokens spell bar:large where Swift writes bare bar; bar == bar:large here
- Task 3: minor (deferred): Transport menu says "Play / Stop" (Swift flips Play/Stop) and Loop has no checkmark
- Task 3: minor (deferred): set_time_range duplicates set_selection's set/alert/refresh
- Task 3: minor (deferred): AppState::stop_and_return now unused
- Task 3: minor (deferred): held ⇧, ⇧. act only while the window root has focus
- Task 3: minor (deferred): no GPUI test for caret placement in an editing field or a field taking the keyboard mid-hold
- Task 3: minor (deferred): vizdaw-app position/field tests not written test-first
- Task 3: minor (deferred): Loop tree value "on"/"off" is not a Swift-parity value
- Task 3: minor (deferred): Linux/Mac keyboard gap — keypad −/+ not bound (GPUI cannot tell keypad)
- Task 4: minor (deferred): blocks/events Vecs cloned every render while playing (use Arc)
- Task 4: minor (deferred): selected-block check O(events×selected) per frame
- Task 4: minor (deferred): a refused blocks read returns empty and retries every frame (Swift keeps last)
- Task 4: minor (deferred): events read twice per snapshot (blocks + sketch)
- Task 4: minor (deferred): grid bar_at rebuilds with_bars per bar
- Task 4: minor (deferred): vertical offset clamps one frame late after a resize
- Task 4: minor (deferred): ⌘-wheel over headers zooms about a point outside the canvas
- Task 4: minor (deferred): channel click on Arrangement locates if the canvas is under 44 px tall
- Task 4: minor (deferred): fallback ArrangementState literal duplicated (derive Default)
- Task 4: minor (deferred): block nodes precede their group in the flat ui.tree list
- Task 5: minor (deferred): duplicate UiScreenshot arm in answer() without default path
- Task 5: minor (deferred): wait facts read twice in the first tick
- Task 5: minor (deferred): X11 GetImage BadMatch on a partly off-screen window reads as "could not be rendered"
- Task 5: minor (deferred): test repeats NEEDS_X11 literal
- Task 5: minor (deferred): ⌘Q via ui.key quits through GPUI Quit (same delivery path)
- Final: minor (deferred): channel failure reported as success while an alert is already showing (queued alert; also slice 2 paths)
- Final: minor (deferred, fix before plugin windows): Busy sequence_info cached as None → 4/4 used for fields/ruler until next snapshot
- Final: minor (deferred): folded field's key answers "The window shows no transport fields." (Swift opens » popover) — parity gap to log
- Final: minor (deferred): keypad −/+ not bound — feature loss for keypad users, logged as gap
- Final: minor (deferred): unverified: mouse-up outside the window during a hold; 978 px default bar on Linux
- Final: minor (deferred): non-JSON preferences.json treated as empty and rewritten
- Final: minor (deferred): stale module doc document.rs:1-2

### Owner's manual checks

- Look at the arrangement and transport bar on a real display (drawn only on the test platform and in e2e so far).
- Hold Rewind/Fast Forward and release the mouse outside the window.
- Default 1000 px window on Linux with the in-window menu bar: does the standard bar fit, or do Edit In/Out fold into »?
- Typing a position into a 3/4 song lands on the right bar.
