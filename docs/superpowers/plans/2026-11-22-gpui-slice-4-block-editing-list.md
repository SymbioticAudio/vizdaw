# 27.1.0 Slice 4: Block Editing and the List Editor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The GPUI app can edit blocks in the arrangement (select, shift-select, clear, drag to move, ⌥-drag to copy, drag onto another track to merge, erase, context-menu Open in List and Delete, Escape cancels, the Tool picker and Snap) and has the bottom detail pane with its first editor, the List editor (rows, columns, typed edits, insert, delete, filter), driven over the control channel, with the demo e2e test's List step restored.

**Architecture:** As in slices 2–3: behaviour lives in `vizdaw-app` (no GPUI). A new `editor` module holds what Swift's `EditorModel` holds (tool, snap, `ui.json` layout with the detail pane, selection API, edit-with-status, the gesture state machine); `arrangement` gains hit testing, the pointer tool machine and `BlockDrag`; a new `list` module ports `ListFormat`/`ListRows`/`ListEdit`/`ListEditorModel`. `vizdaw-gpui` adds the arrangement tool bar and pointer editing, a bottom pane under the arrangement, and the List table; the control channel answers the new commands and state.

**Tech Stack:** Rust 2024; GPUI at the slice 1 pin (zed rev `f4fe0be1`, `font-kit` on); `core-ffi`, `core-control`, `core-model`, `vizctl`.

**Spec:** `docs/superpowers/specs/2026-11-20-cross-platform-27-1-design.md` (§4, §8, §10). Parity checklist: `docs/superpowers/specs/2026-11-22-parity-checklist.md`. Slice 3 plan and Outcome (deferred minors this slice touches: the arrangement rows/`Arc` cache, `set_time_range` duplicating `set_selection`, a Busy `sequence_info` cached as `None`): `docs/superpowers/plans/2026-11-22-gpui-slice-3-transport-arrangement.md`. Swift reference: `app/Sources/VizDAWAppModel/{EditorModel,EditorModel+Gestures,EditorModel+Windows,EditorTypes,EditorLayout,Snap,ListEditorModel,ListRows,ListEdit,ListFormat,ListPreferences,PitchMath,ControlCommands}.swift`, `app/Sources/VizDAWEditors/{ToolMachine,ArrangementLayout,ArrangementView,EditorCanvas,ListEditorView}.swift`, `app/Sources/VizDAW/{ArrangementPane,DetailPane,ListEditorPane,DocumentShell}.swift`. Research notes with file:line citations, exact strings and the Swift test names per area: the slice 4 inventory (controller scratch; the relevant sections are copied into each brief).

## Global Constraints

- Song format unchanged (v11). `ui.json` stays compatible both ways: the editor layout is written with Swift's `EditorLayout` keys and merged into the existing JSON on save, so keys the GPUI app does not know yet (piano roll, torn windows, controllers, other panes) survive a save.
- `vizdaw-app` never depends on GPUI.
- Command IDs, titles, keys and params come from `core_control::registry::COMMANDS`; request parsing and wait conditions from `core_control`. Never re-typed.
- `command.run`, `state.get editor`, `state.get selection` keep the Swift JSON shape (`core-ffi/src/control.rs`: `DetailRow`, editor and selection rows). `state.get editor` keeps slice 3's `status` and `transport` fields.
- Exact Swift strings for every status, refusal, undo label, tree label and cell text (quoted in the inventory sections each brief carries).
- No new commands the Swift app lacks: there is no cut/copy/paste/select-all/duplicate and no Delete key for blocks. Block delete is the Eraser click and the context menu's Delete.
- Out of this slice (their menu items stay disabled, channel answers `not_yet`): the Piano Roll, Strip Chart, Drum Grid and Audio editors and every `_window` variant, torn-off windows, exact mode and the Modify panel, the Pencil tool's action, nudge, references (blocks, sequences, navigation), audio regions, takes, MIDI input in the list, the left/right panes and the Mix view, pane divider dragging.
- Real-time rule: nothing on the interface thread blocks on the engine; list rows are built from cached reads keyed by snapshot version, never per frame.
- Single-key shortcuts carry `!TextInput`; list cells and fields are `TextInput`s. A control with a key action records role, label, bounds and (new) `selected` in the registry while drawing.
- Subagents: no GUI, no computer-use tools, never `dangerouslyDisableSandbox`. Visible checks and e2e runs are done by the controller (Mac) or over SSH on the owner's Linux box.
- Banned vendor/DAW names stay out of repo text. UI says "track", never "lane". Never commit `.DS_Store`, `.superpowers/`, generated FFI files or brand assets.
- CI: `cargo fmt --all -- --check`, clippy `-D warnings` (with and without `--features screenshot`), tests on macOS and Linux.
- Commit trailers: `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>` and `Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P`.

## Review Focus

1. A drag that ends refused, Busy, or with its track deleted under it: no partial move stays in the song, at most one undo step exists ("Move Block"/"Copy Block"), the refused final's core message is the status, a vanished track says "The track was removed; the drag was cancelled.", and Escape also aborts a final edit still pending on Busy (Swift `GestureTests`) — Task 2.
2. Saving a project whose `ui.json` holds keys this app does not model (piano roll state, torn windows, controllers): every unknown key survives, and a damaged or newer `ui.json` opens with defaults without being overwritten until the next save (Swift `layoutDecodingIsTolerant`, `corruptLaneHeightAndZoomFallBackOrClamp`) — Task 1.
3. Typed list text that is hostile or edge: huge bars, bad pitches, `5..`, an unchanged cell written differently, a row whose event vanished mid-edit, deleting the first tempo or meter, a multi-row delete that the core partly refuses — no panic, no edit or exactly one undo step, the Swift message (Swift `ListEditTests`, `ListEditorModelTests`) — Task 5.
4. Keys while the list or a cell has the keyboard: a typed cell takes Space/Return/Delete as text; the table (no cell) keeps Space as play but Return no longer returns to start; Delete deletes rows only from the table — Tasks 5 and 6.
5. A long list (100 000 events) builds rows quickly and draws only the rows in view (Swift `buildingRowsForAHundredThousandEventsIsQuick`) — Tasks 5 and 6.

---

### Task 1: core-app — session calls, editor model, layout and selection

**Files:**
- Create: `crates/vizdaw-app/src/editor.rs` (or `editor/` with submodules if it grows past one responsibility: `layout.rs`, `snap.rs`, `detail.rs`)
- Modify: `crates/vizdaw-app/src/{session.rs,testing.rs,document.rs,commands.rs,lib.rs}`
- Test: new `crates/vizdaw-app/tests/{editor.rs,snap.rs,detail.rs}`

**Interfaces:**
- Consumes: slice 2–3 `DocSession`, `DocumentModel` (selection, `set_time_range`, `select_track`, status, text-editing owners, cached `blocks`/`lane_events` as `Arc<[T]>`), `commands`.
- Produces (exact):
  - `DocSession` adds, forwarded to `core_ffi::Session`: `begin_edit(&self, label: &str) -> Result<(), VizError>`, `edit_gesture(&self, cmd: EditCommand) -> Result<DocSnapshot, VizError>`, `end_edit(&self) -> Result<DocSnapshot, VizError>`, `abort_edit(&self) -> Result<DocSnapshot, VizError>`, `edit_all(&self, label: &str, cmds: Vec<EditCommand>) -> Result<DocSnapshot, VizError>`, `seconds_at(&self, seq: u64, ticks: i64) -> Result<f64, VizError>`, `ticks_at(&self, seq: u64, seconds: f64) -> Result<i64, VizError>`, `ui_layout(&self) -> Result<String, VizError>`, `set_ui_layout(&self, json: String) -> Result<(), VizError>` (match core-ffi's real signatures; adapt names to them and state it). `FakeSession` scripts and records each (Busy and refusal scripting included).
  - `editor::Tool { Arrow, Pencil, Eraser }` (titles "Arrow", "Pencil", "Eraser"; not persisted), `editor::Snap { division: Division, triplet: bool, dotted: bool, enabled: bool }` with `apply(tick, meter)`, `floor(tick, meter)`, `step(meter_at)`; `Division { Bar, Half, Quarter, Eighth, Sixteenth, ThirtySecond }` with raw values `bar, half, quarter, eighth, sixteenth, thirty_second` (and the old 32nd spelling still reads) and titles "Bar", "1/2", "1/4", "1/8", "1/16", "1/32"; default quarter, enabled.
  - `editor::EditorLayout` (serde, Swift keys and defaults from `EditorLayout.swift`: `snap`, `ticksPerPoint`, `laneHeight`, `blockRule`, `lightMode`, `follow`, `detail`, `lastKind`, `views` with the Swift pane defaults), tolerant decode (bad fields fall back or clamp), `merge_into(existing_json) -> String` keeping unknown keys, sorted keys on write.
  - `editor::{DetailEditorKind { PianoRoll, List, StripChart, DrumGrid }, EditorTarget { Block{seq,lane,start,end}, Lane{seq,lane}, Sequence{seq} }, DetailState { kind, target, list_filter: Option<ListFilter> }}` (snake_case kind names for the channel).
  - `DocumentModel` gains (or holds an `EditorModel` that has): `tool()/set_tool`, `layout()/update_layout(f)` (layout read from `ui_layout()` at open; written with `set_ui_layout(merge)` on save), `edit(cmd) -> bool` and `edit_all(cmds, label) -> bool` (refuse with status "Finish the drag first." while a gesture is open; a core refusal → status; success applies the snapshot and refreshes the selection), `select_block(lane, block, adding)`, `clear_selection()`, `is_selected(block) -> bool`, `set_selection(row)` with rollback to what the session holds (fold slice 3's duplicated `set_time_range` path into it), `detail_target()`, `open_detail(target, kind: Option<DetailEditorKind>)`, `show_detail(kind)`, `close_detail()`, `detail_title() -> String` (Swift `EditorModel+Windows.swift:128-141`, en dash), prune the detail when its sequence or track vanishes.
  - `commands` handles `editors.snap` (param `on`; absent toggles), `editors.list`, `editors.close_detail` (`unavailable` "No detail editor is open."), and refuses `edit.undo`/`edit.redo` with `unavailable` "Finish the drag first." while a gesture is open. `editors.insert_event` is wired in Task 5.

- [ ] **Step 1: Write the failing tests**, ported from Swift with snake_case names: all of `SnapTests`; from `EditorModelTests`: `selecting_a_block_selects_its_events_lane_and_range`, `a_ruler_range_selects_every_shown_lane`, `an_edit_applies_its_snapshot_and_refreshes_the_selection`, `a_refused_edit_shows_a_status`, `a_refused_selection_is_rolled_back`, `the_selection_is_what_the_session_kept`, `edit_warnings_show_in_the_status`, `the_layout_is_stored_on_save_and_read_at_open`, `layout_decoding_is_tolerant`, `corrupt_lane_height_and_zoom_fall_back_or_clamp`; from `NavigationTests`: `a_block_opens_in_the_last_used_detail_editor`; from `ControlCommandsTests.editorsAndWindowsGoWhereTheMenuSendsThem` the `editors.list`/`editors.close_detail` part; plus `saving_keeps_ui_json_keys_this_app_does_not_know` and `the_detail_closes_when_its_track_is_deleted`.
- [ ] **Step 2: Run** `cargo test -p vizdaw-app` → fails to compile.
- [ ] **Step 3: Implement.** The GPUI app keeps compiling (mechanical updates only).
- [ ] **Step 4: Run** `cargo test -p vizdaw-app -p vizdaw-gpui` and clippy → pass.
- [ ] **Step 5: Commit**: `git commit -m "core-app: editor model, snap, layout and block selection"`.

### Task 2: core-app — gestures, the tool machine, hit testing and block drags

**Files:**
- Modify: `crates/vizdaw-app/src/{editor.rs (or editor/gesture.rs),arrangement.rs,document.rs}`
- Test: new `crates/vizdaw-app/tests/{gesture.rs,tool_machine.rs,block_drag.rs}`

**Interfaces:**
- Consumes: Task 1.
- Produces (exact):
  - Gesture API on the document/editor model: `begin_gesture(label: &str, lanes: Vec<(u64,u64)>) -> bool`, `set_gesture_lanes(lanes)`, `update_gesture(target: Option<EditCommand>)`, `end_gesture(final_: Option<EditCommand>)`, `cancel_gesture()`, `tick()` (retries a Busy final/abort; called from the GPUI poll), `gesture_open() -> bool`. Rules exactly as `EditorModel+Gestures.swift` (inventory §1.6): intermediate refusals silent, Busy intermediate drops `applied` and marks uncertain, a `None` target rewinds (abort + begin again), a refused final cancels and sets the core's message, a vanished lane cancels with "The track was removed; the drag was cancelled.", one gesture at a time.
  - `arrangement::{ToolMachine<Hit>, MachineEvent<Hit> { Click{hit, at, clicks, modifiers}, Begin{hit, from, modifiers}, Move{hit, from, to}, End{hit, from, to, modifiers}, Cancel }, Modifiers}` (dead zone 3 points; Swift `ToolMachine.swift`).
  - `arrangement::{hit_test(geometry, axis, rows, point) -> ArrangementHit { Block{lane, index}, Empty{lane: Option<u64>, tick} }}` (topmost = last block in the row whose rect contains the point).
  - `arrangement::BlockDrag { events(pressed, lane_blocks, selected) , delta(axis, start_tick, to_x, block_start, snap, meter), command(...) -> Option<EditCommand>, label(copy) -> &'static str }` exactly as `ArrangementLayout.swift:103-149` ("Move Block", "Copy Block"; move / duplicate / insert-into-target; merge onto another track; option read only at begin).
- [ ] **Step 1: Write the failing tests**, ported one for one: all of `GestureTests`; all of `ToolMachineTests`; from `ArrangementLayoutTests`: `hitting_prefers_the_topmost_block`, `drag_delta_snaps_the_block_start`, `moves_stay_in_their_lane_or_merge_into_another`, `option_copies_shifted_rows_into_the_target_lane`, `option_copies_in_the_same_lane_duplicate_the_shifted_events`, `dragging_a_selected_block_carries_the_other_selected_blocks_of_its_lane`; the `ArrangementViewTests` drag cases as model tests: `a_plain_move_is_one_begin_end_pair`, `an_option_drag_copies_into_the_target_lane`, `a_drag_onto_another_row_merges_into_its_lane`, `escape_mid_drag_leaves_no_change`, `option_pressed_mid_drag_does_not_turn_a_move_into_a_copy`, `escape_aborts_a_final_edit_still_pending_on_busy`, `a_drag_watches_its_source_and_target_lanes`.
- [ ] **Step 2: Run** `cargo test -p vizdaw-app` → fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p vizdaw-app -p vizdaw-gpui` and clippy → pass.
- [ ] **Step 5: Commit**: `git commit -m "core-app: gestures, tool machine and block drags"`.

### Task 3: GPUI — arrangement editing

**Files:**
- Modify: `crates/vizdaw-gpui/src/{arrangement.rs,shell.rs,menus.rs,registry.rs,control.rs}`
- Test: `crates/vizdaw-gpui/tests/arrangement.rs`, `crates/vizdaw-gpui/tests/control.rs`

**Interfaces:**
- Consumes: Tasks 1–2.
- Produces: the arrangement tool bar gains a Tool picker (segmented "Arrow"/"Pencil"/"Eraser"; Pencil selectable but inert this slice, ledgered), a "Snap" toggle and a division menu ("Bar", "1/2", "1/4", "1/8", "1/16", "1/32", then "Triplet", "Dotted" toggles) beside the existing zoom buttons. Pointer editing through `ToolMachine`: click selects a block, ⇧-click adds, a click on empty space clears (⇧ keeps), a double-click opens the detail on the block (last-used kind; List is the only kind this slice, so it opens List), the Eraser deletes the clicked block's events, an Arrow drag moves / ⌥-drag copies / drags onto another track merge, with the live preview through the gesture API and auto-scroll near the edges; Escape cancels and swallows the matching mouse-up. Selected blocks draw with the selected stroke. Right-click on a block (selecting it first if unselected): menu "Open in List", separator, "Delete". Registry: block nodes gain `selected` and their element click selects the block (shift with `modifiers`, double with `count: 2`); the registry JSON gains `selected` for any control that sets it. Menu: Editors › Snap to Grid (G, checkmark); the G key toggles snap. The poll timer calls the gesture `tick()`. Control: `state.get editor` gains `detail` (null until Task 4 wires the pane; the model already knows), `windows: []`, `tool`, `snap {enabled, division}`, `exact_mode: false`, `gesture: bool`; `state.get selection` answers `{doc, time_range, lanes, events}`; `ui.click {x, y}` (point click) is implemented through the same hit-testing path as the mouse.
- [ ] **Step 1: Write the failing tests**: `a_click_selects_a_block_and_shift_adds`, `a_click_on_empty_space_clears_unless_shift`, `a_drag_moves_a_block_as_one_undo_step` ("Move Block"), `an_option_drag_copies` ("Copy Block"), `a_drag_onto_another_track_merges`, `escape_mid_drag_leaves_the_song_unchanged`, `the_eraser_deletes_the_clicked_block` ("Delete Events"), `the_block_menu_opens_the_list_and_deletes`, `g_toggles_snap_and_the_menu_shows_it`, `a_snapped_drag_lands_on_the_grid`, `selected_blocks_are_selected_in_the_tree`, `state_get_selection_follows_a_block_click`, `a_point_click_on_empty_space_clears_the_selection`, `undo_is_refused_while_a_drag_is_open`.
- [ ] **Step 2: Run** `cargo test -p vizdaw-gpui` → fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p vizdaw-app -p vizdaw-gpui` and clippy (with and without `--features screenshot`) → pass.
- [ ] **Step 5: Commit**: `git commit -m "gpui: block editing in the arrangement"`.

### Task 4: GPUI — the bottom detail pane and the editors commands

**Files:**
- Create: `crates/vizdaw-gpui/src/detail_pane.rs`
- Modify: `crates/vizdaw-gpui/src/{shell.rs,menus.rs,control.rs}`, `crates/vizctl/tests/e2e_gpui.rs`
- Test: new `crates/vizdaw-gpui/tests/detail.rs`, `crates/vizdaw-gpui/tests/control.rs`

**Interfaces:**
- Consumes: Task 1 (`layout().detail`, `views`, `open_detail`, `show_detail`, `close_detail`, `detail_title`).
- Produces: a bottom pane under the headers and arrangement, shown per the layout's Arrange bottom pane (default shown, 260 px; clamp to 120–900 and give way to the arrangement's 240×160 minimum as `PaneGeometry.swift` does; hidden keeps its content mounted at zero size). Header: a kind picker listing "List" only (the other kinds absent this slice; ledgered), the title (`detail_title`), and a close button labelled "Close Detail Editor". Empty state when no detail: "No Editor Open" and "Double-click a block, or press ⌘1 to ⌘4.". The List slot shows a placeholder until Task 6 (an empty `group` labelled "Event List"). Commands: `editors.list` (⌘2) → `show_detail(List)`; `editors.close_detail` (disabled when nothing is open; channel `unavailable` "No detail editor is open."); `ui.toggle_bottom` (⌃⌘↓, param `on`) shows/hides the bottom pane. Control: `wait detail_kind` reads the real kind ("none" iff no detail, regardless of pane visibility); `state.get editor.detail` is `DetailRow`-shaped. Append to `gpui_demo_play_screenshot_stop` the final step of the Swift test (`command.run editors.list`, `wait detail_kind list`) and fix its doc comment.
- [ ] **Step 1: Write the failing tests**: `editors_list_opens_the_list_on_the_shown_sequence` (target `Sequence(Song)`, title "Song"), `a_double_clicked_block_opens_its_range` (title "Song › Chords, bar 1"), `closing_the_detail_shows_the_empty_state`, `close_detail_with_nothing_open_is_unavailable`, `the_wait_reads_the_detail_kind`, `state_get_editor_reports_the_detail`, `toggle_bottom_hides_and_shows_the_pane`, `a_short_window_gives_the_arrangement_its_minimum`.
- [ ] **Step 2: Run** `cargo test -p vizdaw-gpui` → fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p vizdaw-app -p vizdaw-gpui` and clippy → pass; `cargo test -p vizctl --test e2e_gpui --no-run` compiles.
- [ ] **Step 5: Commit**: `git commit -m "gpui: the bottom detail pane and the editors commands"`.

### Task 5: core-app — the List editor logic

**Files:**
- Create: `crates/vizdaw-app/src/list/{mod.rs,format.rs,rows.rs,edit.rs,model.rs}`
- Modify: `crates/vizdaw-app/src/{lib.rs,commands.rs,document.rs,prefs.rs,position.rs}`
- Test: new `crates/vizdaw-app/tests/{list_format.rs,list_rows.rs,list_edit.rs,list_model.rs}`

**Interfaces:**
- Consumes: Task 1 (`edit`, `edit_all`, `set_selection`, detail target and `list_filter`), `position.rs` (`Bbu`, `parse_position` — reuse, do not duplicate), `DocSession::{lane_events, sequence_info, destinations, seconds_at, ticks_at}`.
- Produces: ports of `ListFormat` (positions, lengths, SMPTE at 24/25/30 fps, `Velocity7`/`Value7`/`Bend14` with the MIDI 2 upscale rule, `PitchName` (C4 = 60, octave −2…10), key names), `ListRows` (`ListKind` with the Swift titles, `ListFilter`, `ListColumns` with titles and the optional End/Length/Release/SMPTE and the Track column for multi-track lists, rows sorted `(tick, group, lane index, id)`, row ids `e<id>`/`t<i>`/`m<i>`/`k<i>`, cell text, `editable`, the row summary), `ListEdit::commit` (every editable cell kind in inventory §3.8 including tempo, meter and marker rows, SMPTE and release; reference/audio region/automation cells are not editable this slice), `ListInsert`/`ListDelete`, and `ListModel` (cursor, `select(ids)` replacing the document selection and clearing the range, commit with follow, insert with the last kind and the insert tick rules, delete with the first-tempo/meter statuses and one undo step "Delete" for several commands, filter per list and default/columns in `Prefs` under the Swift key "VizDAW.list" equivalent). `DocumentModel` gains `list_focused` (the table has the keyboard: Return no longer returns to start, Space still plays). `editors.insert_event` (⌘I): `unavailable` "Insert Event needs a list editor that has the keyboard." unless a list is open and no text is being edited. Rows are rebuilt only when the snapshot version, target, filter or columns change.
- [ ] **Step 1: Write the failing tests**, ported one for one: all of `ListFormatTests`; all of `ListRowsTests` (the 100 000-event case asserts a time bound generous enough for CI, e.g. 1 s in debug); all of `ListEditTests`; from `ListEditorModelTests`: `rows_follow_the_target`, `the_cursor_follows_a_moved_event`, `a_refused_commit_leaves_the_document_and_says_why`, `committing_a_row_that_vanished_does_nothing`, `insert_repeats_the_last_kind`, `filters_apply_to_all_lists_or_become_the_default`, `deleting_the_first_tempo_says_it_stays`, `deleting_mixed_rows_is_one_undo_step_all_or_nothing`, `insert_uses_the_transport_only_on_its_own_sequence`, `insert_ignores_a_playing_transport`, `a_core_error_from_an_edit_reaches_the_status_line`, `columns_and_smpte_come_from_the_preferences`; plus `insert_event_needs_a_list_with_the_keyboard`.
- [ ] **Step 2: Run** `cargo test -p vizdaw-app` → fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p vizdaw-app -p vizdaw-gpui` and clippy → pass.
- [ ] **Step 5: Commit**: `git commit -m "core-app: the List editor logic"`.

### Task 6: GPUI — the List editor and the e2e tests

**Files:**
- Create: `crates/vizdaw-gpui/src/list_editor.rs`
- Modify: `crates/vizdaw-gpui/src/{detail_pane.rs,shell.rs,menus.rs,control.rs,registry.rs}`, `crates/vizctl/tests/e2e_gpui.rs`
- Test: new `crates/vizdaw-gpui/tests/list.rs`

**Interfaces:**
- Consumes: Tasks 4–5.
- Produces: the List editor in the bottom pane: a bar (an "Insert <kind>" split button whose menu lists the insertable kinds; "Columns" menu with "End", "Length", "Release Velocity", "SMPTE" toggles and "SMPTE Frame Rate" 24/25/30 fps; "Show" menu with a toggle per kind, per destination, "Show Everything", "Save as Default"; "<n> rows" on the right; MIDI In and Modify… absent this slice), a header row, 18 px rows with the Swift column widths, alternating shade, selection fill, multi-selection (click, ⇧-click range, ⌘/Ctrl-click toggle), vertical scrolling that draws only the rows in view. Cell editing: double-click or Return edits (Return edits the Position cell of the selected row), Tab/⇧Tab move between editable cells, Return commits, Escape restores; read-only cells never take the keyboard; Delete/Forward Delete in the table deletes the selected rows. Registry: a `table` node labelled "Event List", one `row` node per visible row labelled by the summary with `selected`, and cell `textfield`/`text` nodes with `value`; the bar controls by their titles. ⌘I runs `editors.insert_event`.
  New GPUI e2e tests in `e2e_gpui.rs`:
  - `gpui_blocks_select_and_open_in_the_list`: click the Chords block (`ui.click` element), `state.get selection` (events and lane), ⇧-click the Groove block (lanes 2), click empty space by point (cleared), double-click Chords → `state.get editor.detail` is the block target, `wait detail_kind list`.
  - `gpui_the_list_edits_inserts_and_deletes`: `editors.list` on the demo, select a note row, Return, type `5` + Return → `state.get document.undo == "Move Events"`; `editors.insert_event` → "Insert Events"; Delete → "Delete Events"; `edit.undo` restores; `editors.close_detail` → `wait detail_kind none`.
  - `gpui_g_toggles_snap`: the snap step of `keys_drive_step_entry_locators_and_bindings` (`state.get editor.snap {enabled: true, division: "quarter"}`, `ui.key g`, `enabled: false`).
- [ ] **Step 1: Write the failing GPUI tests**: `rows_and_columns_show` (columns "Position", "Length", "Kind", "Destination", "Data 1", "Data 2", "Mute"; a cell "1.2.0", a pitch "E4"), `editing_a_cell_turns_the_transport_keys_off_until_it_ends`, `escape_leaves_the_event_alone`, `keys_delete_rows_and_return_edits_the_position`, `the_table_is_in_the_tree`, `a_read_only_cell_never_takes_the_keyboard`, `moving_straight_to_another_cell_commits`, `space_plays_from_the_table_but_return_does_not_locate`, `a_long_list_draws_only_the_rows_in_view`; then write the three e2e tests (`#[ignore]`, need `VIZDAW_E2E_APP`).
- [ ] **Step 2: Run** `cargo test -p vizdaw-gpui` → fail; `cargo test -p vizctl --test e2e_gpui --no-run` compiles.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p vizdaw-app -p vizdaw-gpui` and clippy → pass.
- [ ] **Step 5: Commit**: `git commit -m "gpui: the List editor; e2e for blocks, the list and snap"`.
- [ ] **Step 6 (controller):** build and run the GPUI e2e suite on the Mac and on Linux → all pass (11 tests); look at the arrangement and the List on the Mac screen; tick the parity checklist rows (`editors.list`, `editors.close_detail`, `editors.insert_event`, `editors.snap`, `ui.toggle_bottom`, the List window/editor rows, the demo e2e row's List step, the G-snap part of `keys_drive_step_entry_locators_and_bindings`), noting what stays for later slices.

## Outcome (2026-11-22)

All six tasks done (commits aeaaba3d..badc2bca). Evidence: workspace tests 4325 passed on macOS and 4194 on Linux; clippy `-D warnings` and fmt clean; GPUI e2e 11/11 on macOS and on Linux (XWayland), including the demo test's restored List step and three new tests (blocks select and open in the List; the List edits, inserts and deletes; G toggles snap); a visual check on the Mac of the tool bar, a selected block and the List pane.

Final review fixes (badc2bca): a List cell edit is discarded if its row changed under it (⌘Z / edit.undo); a hidden bottom pane gives up the keyboard; a stale gesture copy can no longer clear `reverting`; hostile `ui.json` ranges saturate. Found by running the e2e suite: the list table takes the keyboard on a row click on macOS; channel clicks hit the visible part of a partly scrolled element.

### Rulings

- Task 1: Ruling: session call shapes follow core-ffi (seconds_at/ticks_at Vecs; ui_layout Option<String>) — real API — none
- Task 1: Ruling: selection failure → status line, not slice 3's "Selection Failed" alert (one set_selection path, as Swift report()) — parity — a user loses an alert they never had in Swift
- Task 1: Ruling: editors.snap/list/close_detail in EDITOR_COMMANDS; GPUI routing is Tasks 3/4 — none
- Task 1: Ruling: refused set_ui_layout on save is silent (Swift try?) — parity — a lost layout write goes unnoticed
- Task 2: Ruling: block-drag glue (ArrangementFrame, BlockDragState, begin_block_drag) lives in vizdaw-app, not a test helper — keeps behaviour headless and tested; GPUI only forwards machine events — none
- Task 2: Ruling: idle() returns true while a final/abort waits on Busy so the poll redraws — needed for retry visibility — extra redraws while Busy
- Task 3: Ruling: double-click opens List (sets lastKind=list) while List is the only editor; revert to last-used kind when the Piano Roll lands — only editor this slice — a Swift build opening the song then double-clicks into List instead of Piano Roll until the user picks again
- Task 3: Ruling: only editors.snap routed here; List/Close Detail in Task 4 — none
- Task 3: Ruling: point click {"point":{x,y}} dispatches synthetic mouse down/up; element clicks now use the same click_at path — one real input path — element-click behaviour of slices 2–3 changed (reviewer to check)
- Task 3: Ruling: block and division menus open on a channel click (GPUI overlays are not modal) — no new strings — differs from slice 3's » status note
- Task 3: Ruling (revises earlier): double-click opens List WITHOUT changing saved lastKind — avoids overwriting a Swift user's setting — none
- Task 3: Ruling: transport punch_in/out are 0 (TransportMarksRow i64, Swift same type) until punch lands — type is Swift's — none
- Task 4: Ruling: ui.toggle_bottom added to EDITOR_COMMANDS (app model) and routed — needs a handler — none
- Task 4: Ruling: View menu has one checkmark item "Bottom Pane" (⌃⌘↓); pane group/picker labelled "Bottom Pane" — registry title — may clash with the transport bar's "Bottom Pane" button when the panes item lands
- Task 5: Ruling: tests written with the code (no RED seen beyond compile) — reviewer to judge strength — weak tests could hide bugs
- Task 5: Ruling: list prefs pushed into each document as a value (set_list_prefs); Task 6 must push on create and on change — none if Task 6 wires it
- Task 5: Ruling: filtersApplyToAllLists test adapted to close/reopen (no torn windows) — scope — none
- Task 5: Ruling: list cursor clears on every detail-target change (Swift keeps it) — safe, avoids map ids naming an unrelated row — next Insert uses transport/start instead of old cursor row
- Task 5: Ruling: reference/audio-region/automation cells not editable this slice (Swift allows position/SMPTE/mute/data 2) — features out of slice — those rows are read-only until their slices
- Task 6: Ruling: while the table has the keyboard, ui.key Return/Enter/Delete/Forward Delete go to the table, not the keymap — otherwise refused Return-to-Start swallows them — channel keys behave as real keys there
- Task 6: Ruling: Insert split button's menu half labelled "Insert Kind" — brief gave no label — a test naming it differently would miss
- Task 6: Ruling: channel clicks hit the centre of an element's visible part; refused only when wholly out of view (slice 3 header test window shrunk to keep a true refusal) — a partly drawn control is clickable as with a mouse — none
- Final: Ruling: declined — save while a drag's final is pending on Busy writes the mid-drag document (core save_to has no transaction check; Swift same) — stands, core/spec question — a mid-drag save could store a half move
- Final: Ruling: declined — track/add/delete commands during an open gesture not gated in the app (core acting_refusal covers) — stands — none if core refuses
- Final: Ruling: declined — map-row ids after a Tab that re-sorts rows open the next field on the re-sorted row (Swift design) — stands — visible, not silent
- Final: Ruling: declined — ⌥ copy flag read when the drag leaves the dead zone rather than at press — stands pending Swift check — a ⌥ pressed during the dead zone copies
- Final: Ruling: declined — edit_gesture snapshot cost on huge songs — core, later — drag stutter on huge songs
- Final: Ruling: declined — Escape on a Busy-pending drag after the keyboard moved to the List table — too unlikely — a pending drop survives that Escape

### Deferred minors

- Task 1: minor (deferred): prune drops only the detail (Swift also prunes detail.lanes, windows, controllers)
- Task 1: minor (deferred): an undecodable detail is removed from ui.json on next save (Swift same)
- Task 1: minor (deferred): ui_layout() failing at open and save writes only modelled keys
- Task 1: minor (deferred): force_gesture_open public doc(hidden) hook — replace in Task 2
- Task 1: minor (deferred): target_title reads session.lanes for another sequence (per-frame read if drawn per frame)
- Task 2: minor (deferred): stale gesture copy can clear `reverting` set by after_sync's Busy cancel (Swift shares it; needs Busy during the snapshot that removes the track)
- Task 2: minor (deferred → Task 3 brief): add a model method for Escape that forwards cancel and calls cancel_gesture when open
- Task 2: minor (deferred): no test for a Busy-pending final whose track vanishes on the next idle
- Task 2: minor (deferred): set_gesture_lanes writes on every move
- Task 3: minor (deferred): canvas Escape cancels any open gesture, not only its own (fine while block drags are the only gestures)
- Task 3: minor (deferred): ⌃-click on macOS selects instead of opening the block menu
- Task 3: minor (deferred): right press during a left drag opens the block menu mid-drag
- Task 3: minor (deferred): state.get editor transport marks are neutral constants, not the song's locators/punch (until the marks slice)
- Task 3: minor (deferred): a test saves to a fixed shared temp path
- Task 4: minor (deferred): pane picker shows last_kind (default "Piano Roll") while menu lists only List
- Task 4: minor (deferred): pane absent on the first frame until measured (one-frame flash)
- Task 4: minor (deferred): control.rs module doc paragraph needs reflow
- Task 4: minor (deferred): picker role/label "Bottom Pane" button may clash with a future transport panes button
- Task 5: minor (deferred): vanished-row commit test uses a never-existing id (faithful to Swift)
- Task 5: minor (deferred): list_row/list_index linear scan + clone (needs an index if Task 6 looks up per frame)
- Task 5: minor (deferred): SMPTE rounding unchecked arithmetic on the core's tick
- Task 5: minor (deferred): Rust float parse accepts "+5", ".5" where Swift may differ (no bypass)
- Task 6: minor (deferred): cells labelled by column title share labels with transport fields (e2e lookups by label alone could be ambiguous once a List is open)
- Task 6: minor (deferred): hidden bottom pane keeps list_focused (Return/Delete act on hidden table; matches Swift mounted pane)
- Task 6: minor (deferred): list_editor.rs 1 189 lines — split bar/menus later
- Task 6: minor (deferred): Insert split button width 191 vs Swift 190; "Apply to All Lists" absent (single list)
- Final: minor (deferred): snap decodes as one unit (one bad sub-field resets all)
- Final: minor (deferred): channel click on a block half under the ruler can land in the ruler (record rect ∩ body)
- Final: minor (deferred): deferred closure scheduled each frame while no List is open
- Final: minor (deferred): List rebuilds on every drag move (new version per move; Swift same)
- Final: minor (deferred): start = i64::MIN target may overflow in arrangement::bar_at in debug (untested)
- Final: minor (deferred): a commit dropped because its row changed ends silently (no status)

### Owner's manual checks

- Drag blocks with the mouse: move, ⌥-copy, drop onto the other track, Escape mid-drag; check the undo names.
- In the List: double-click a cell, type, Tab between cells, Escape; Delete rows; Insert a Tempo row.
- ⌘Z while typing in a List cell or a transport field undoes the song, not the text (app-wide decision pending).
- ⌃-click a block on the Mac (selects today; the Swift app opens the menu).
