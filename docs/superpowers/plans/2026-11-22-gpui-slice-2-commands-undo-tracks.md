# 27.1.0 Slice 2: Commands, Undo and the Track List Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The GPUI app gains the command system (menus and keys built from the existing Rust command registry, with an in-window menu bar on Linux), undo and redo with an unsaved state that stays true to the session, and the arrangement's track header column (add, duplicate, delete, rename, select, mute/solo/arm), all driven over the control channel with the Swift e2e tests for tracks and unsaved songs ported to the GPUI app.

**Architecture:** App logic goes in `vizdaw-app` (no GPUI): `DocSession` grows the session calls tracks need, `DocumentModel` keeps the shown sequence, its lanes and the track selection and polls `Session::idle`, and a new `commands` module runs registry IDs against a document the way the Swift `ControlCommands`/`TrackCommands` do. `vizdaw-gpui` builds one menu model from `core_control::registry` and the `Keymap`, shows it with `set_menus` on macOS and as an in-window bar elsewhere, routes every menu item and key through one `RunCommand` action into `vizdaw_app::commands::run`, and adds a `TrackHeaders` view. The control channel answers `state.get document` with the exact JSON the Swift app sends by reusing `vizdaw_ffi::control::result_json`.

**Tech Stack:** Rust 2024; GPUI at the slice 1 pin (`crates/vizdaw-gpui/Cargo.toml`); `core-ffi`, `core-control` (registry, `Keymap`, `ArgValue`), `vizctl`.

**Spec:** `docs/superpowers/specs/2026-11-20-cross-platform-27-1-design.md` (§8 architecture, §10 parity). Slice 1 plan and its Outcome section (rulings this slice must honour: "carry to slice 2 — unsaved-state guard reads a cached snapshot", "Linux in-window menu bar is slice 2", `add_field` test hook): `docs/superpowers/plans/2026-11-21-gpui-slice-1-foundation.md`. Swift reference (behaviour and tests to port): `app/Sources/VizDAWAppModel/{TrackCommands,ControlCommands,DocumentModel,DocumentModel+Sequences,ClosePrompts,CommandButtons}.swift`, `app/Sources/VizDAW/AppCommands.swift`, `app/Sources/VizDAWEditors/{LaneHeaderView,ArrangementLayout}.swift`, tests in `app/Tests/VizDAWAppModelTests/{TrackCommandsTests,DocumentModelTests,ClosePromptsTests,ControlCommandsTests,LaneHeaderInputTests,ArrangementViewTests}.swift`. Research notes with file:line citations (controller scratch, copied into each brief where needed): the slice 2 inventory.

## Global Constraints

- Song format unchanged (v11). A project saved by the Swift test build opens in the GPUI app and vice versa.
- `vizdaw-app` never depends on GPUI. It may depend on `core-control` (registry lookups, `ArgValue`, `ControlError`/`ErrorCode`).
- Command IDs, titles, default shortcuts and parameter names come from `core_control::registry::COMMANDS` and `core_control::bindings::Keymap`; never re-typed as string tables in the app.
- `command.run` answers keep the Swift contract: success body `{command, doc, window, undo, status}`; errors `unknown_id`, `no_document`, `unavailable`, `dialog_required`, `invalid_params`, `failed`, `busy`, with the Swift wording where the Swift app has one (quoted in the tasks).
- `state.get document` JSON is produced by `vizdaw_ffi::control::result_json(&ControlResult::Document { .. })`, so its fields match the Swift app byte for byte.
- Close and quit guards read `Session::has_unsaved_changes()` live (failure counts as unsaved), never a cached snapshot. The window title's "— Edited" mark follows the latest snapshot, refreshed after every edit and by `idle`.
- Single-key shortcuts carry `!TextInput` in their key context; a rename field is a `TextInput`.
- Every clickable control records role, label and bounds in the element registry while drawing; element paths come from the view tree. Track header labels match the Swift accessibility labels exactly (Task 5).
- Control calls handled per frame stay capped at 16 per pump tick.
- Subagents: no GUI, no computer-use tools, never `dangerouslyDisableSandbox`. Visible checks and e2e runs are done by the controller (Mac) or over SSH on the owner's Linux box (192.168.1.192; XWayland `DISPLAY=:0`, `XAUTHORITY=/run/user/1000/xauth_*`).
- Banned vendor/DAW names stay out of repo text (licence notices exempt). Never commit `.DS_Store`, `.superpowers/`, generated FFI files or brand assets.
- CI: `cargo fmt --all -- --check`, clippy `-D warnings`, tests on macOS and Linux runners.
- Commit trailers: `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>` and `Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P`.

## Review Focus

1. Undo back to the saved state must clear the "— Edited" mark and let quit go through without asking; redo must bring both back (Swift: `dirtyTitle`, core `clean` equality) — Task 2 test `undo_to_the_saved_state_is_clean`.
2. Hostile `track.*` parameters (negative or huge `seq`, unknown `lane`, missing or blank `name`) answer `invalid_params`/`failed`/`dialog_required`, never panic, and leave the track list unchanged (Swift: `trackCommandsRefuseHostileInput`) — Task 3.
3. Deleting the selected or the only track: the selection falls back, the "Add Track" row stays, and Track menu items that need a track disable (Swift: `deleteTheOnlyTrackLeavesTheAddRow`) — Tasks 2 and 5.
4. The inline rename: Escape cancels, Enter commits a trimmed name, blank or unchanged sends nothing, and transport keys work again even if the header is torn down mid-rename (Swift: `escapeInTheLaneRenameFieldCancelsTheRename`, `aHeaderTornDownMidRenameReEnablesTransportKeys`) — Task 5.
5. A change that arrives without an edit (plugin state, background job) marks the song unsaved through `idle`, and the close and quit guards see it even if no snapshot was refreshed (Swift: `has_unsaved_changes_reads_the_session`) — Task 2.

---

### Task 1: Parity checklist and baseline

**Files:**
- Create: `docs/superpowers/specs/2026-11-22-parity-checklist.md`
- Create: `crates/core-control/examples/dump_commands.rs` (prints the registry as Markdown rows; kept so the checklist can be regenerated)

**Interfaces:**
- Produces: the checklist later slices tick; row IDs are registry command IDs, window kinds and sheet names.

- [ ] **Step 1: Write `dump_commands.rs`**: for every `COMMANDS` entry print `| <id> | <menu> | <title> | <shortcut or blank> | <menu_item> | | |` (last two columns: Mac, Linux). Run `cargo run -p core-control --example dump_commands > /tmp/rows.md`. Expected: 238 rows.
- [ ] **Step 2: Write the checklist**: intro (spec §10: a feature is done when its row passes on the GPUI app; baseline tag `swift-parity-baseline`); section "Commands" grouped by registry menu with the dumped rows; section "Windows" (document window, torn-off editor windows by kind, Mixer, Plugin Manager, Media Pool, Song Notes, Licences, Settings tabs: Appearance, Editing, Keys, Audio, Recording, Automation, Surfaces, Plug-ins, Co-Producer — read `app/Sources/VizDAW/VizDAWApp.swift` scenes to confirm); section "Sheets and dialogs" (grep `.sheet(`, `.alert(`, `NSSavePanel`, `NSOpenPanel` in `app/Sources/VizDAW` and list each by its title); section "Manual checks" (each step in `docs/testing/manual-checks.md` and the README's manual-check list, by heading); section "e2e" (the 44 `crates/vizctl/tests/e2e.rs` tests by name); section "Linux gaps" (the three known gaps from spec §2a with their answers). Mark rows done in slice 1 (`file.new`, `file.open`, `file.create_demo`, `file.close`, `file.save`, `file.save_as`, `transport.play_stop`, `transport.stop`) with "S1" in both platform columns, and leave the slice 2 rows blank (the controller ticks them at the end).
- [ ] **Step 3: Check names**: copy text only from the repo (registry titles, Swift titles, test names), which already follows the naming rule; add no product or vendor names of your own. The controller runs the name check before review.
- [ ] **Step 4: Commit** `docs/superpowers/specs/2026-11-22-parity-checklist.md` and the example: `git commit -m "Parity checklist for 27.1.0 (commands, windows, sheets, manual checks, e2e)"`.

### Task 2: core-app — session calls, lanes, selection, live unsaved state

**Files:**
- Modify: `crates/vizdaw-app/src/session.rs`, `crates/vizdaw-app/src/document.rs`, `crates/vizdaw-app/src/testing.rs`
- Test: `crates/vizdaw-app/tests/document.rs`, new `crates/vizdaw-app/tests/tracks.rs`

**Interfaces:**
- Consumes: slice 1 `DocSession`, `DocumentModel`, `FakeSession`.
- Produces (exact):
  - `DocSession` adds `has_unsaved_changes(&self) -> Result<bool, VizError>`, `idle(&self) -> Result<Option<DocSnapshot>, VizError>` (the `IdleUpdate.snapshot`), `lanes(&self, seq: u64) -> Result<Vec<LaneRow>, VizError>`, `new_track(&self, seq: u64, kind: TrackKindRow, after: Option<u64>) -> Result<DocSnapshot, VizError>`, `duplicate_track(&self, seq: u64, lane: u64) -> Result<DocSnapshot, VizError>`, `edit(&self, cmd: EditCommand) -> Result<DocSnapshot, VizError>` (filter `None`), `selection(&self) -> Result<SelectionRow, VizError>`, `set_selection(&self, s: SelectionRow) -> Result<(), VizError>`; all forwarded to `Session`.
  - `DocumentModel` adds `pub fn apply(&mut self, s: DocSnapshot)`, `pub fn idle(&mut self) -> bool` (true when a new snapshot arrived), `pub fn has_unsaved_changes(&self) -> bool` (error ⇒ `true`), `pub fn selected_seq(&self) -> Option<u64>`, `pub fn lanes(&self) -> &[LaneRow]`, `pub fn selected_lanes(&self) -> Vec<u64>`, `pub fn selected_lane(&self) -> Option<u64>` (first selected lane in the shown sequence), `pub fn select_track(&mut self, lane: u64, extend: bool)`, `pub fn status(&self) -> Option<&str>`, `pub fn set_status(&mut self, s: Option<String>)`, `pub fn rename_track(&mut self, lane: u64, name: &str)`, `pub fn set_flags(&mut self, lane: u64, mute: Option<bool>, solo: Option<bool>, arm: Option<bool>)`, `pub fn toggle_mute(&mut self, lane: u64)`, `pub fn lanes_fetches(&self) -> u64` (test counter of `lanes()` calls; `#[doc(hidden)]`).
  - `FakeSession` scripts all new calls and records them (`calls()` names, as slice 1 does).

Behaviour to port (Swift `DocumentModel.swift`): the shown sequence is the first top-level sequence at start, `None` for an empty document; `apply` keeps the selected sequence if it still exists, else falls back to the first top-level one; lanes are refetched only when `snapshot.version` or the selected sequence changed (`LanesKey`); after every edit/undo/redo the selection is re-read from the session (`refreshSelection`); an edit error becomes the alert titled as Swift titles it ("Rename Failed", "Mute Failed", …), except `Busy`, which becomes the status text.

- [ ] **Step 1: Write the failing tests** in `tests/document.rs` and `tests/tracks.rs`, ported one for one from Swift with these names: `selection_survives_a_snapshot_that_keeps_the_sequence`, `selection_falls_back_when_the_sequence_is_deleted`, `lanes_are_refetched_only_when_version_or_selection_changes` (assert `lanes_fetches()` 1 → same version 1 → bumped version 2), `first_top_level_sequence_is_selected_at_start`, `empty_document_selects_nothing`, `blank_or_unchanged_rename_sends_no_command` (`"   "` and the current name → no `edit` call), `toggle_mute_flips_the_lane_flag`, `toggle_mute_unmutes_a_muted_lane`, `has_unsaved_changes_reads_the_session` (snapshot says clean, session says unsaved → `true`; session errors → `true`), `idle_applies_a_new_snapshot` (fake idle returns a dirty snapshot → `idle()` true, `title()` ends "— Edited"; returns `None` → false, title unchanged), `undo_to_the_saved_state_is_clean` (scripted: edit → dirty, undo → snapshot dirty false → title has no mark; redo → mark back), `a_selected_track_that_is_deleted_falls_back` (selection row from session no longer names the lane → `selected_lane()` `None`).
- [ ] **Step 2: Run** `cargo test -p vizdaw-app` → the new tests fail to compile (missing methods).
- [ ] **Step 3: Implement** the trait additions, the `Session` forwards, the fake scripting and the `DocumentModel` methods.
- [ ] **Step 4: Run** `cargo test -p vizdaw-app` → all pass, slice 1 tests unchanged.
- [ ] **Step 5: Wire the live guard in GPUI**: `AppState::is_dirty` becomes `has_unsaved_changes` (reads `DocumentModel::has_unsaved_changes`) for close and quit; the document window's existing transport poll timer also calls `DocumentModel::idle` (at most every 50 ms) and refreshes the window title when it returns true. Existing tests `closing_an_edited_window_asks_first`, `quit_with_unsaved_changes_asks_first` keep passing; add `quit_asks_when_only_the_session_knows_it_is_unsaved` to `crates/vizdaw-gpui/tests/window.rs` (fake: snapshot clean, `has_unsaved_changes` true → quit asks). Run `cargo test -p vizdaw-gpui` → pass.
- [ ] **Step 6: Commit**: `git commit -m "core-app: lanes, track selection, idle and live unsaved state"`.

### Task 3: core-app — the command runner

**Files:**
- Create: `crates/vizdaw-app/src/commands.rs` (and `pub mod commands;` in `lib.rs`)
- Modify: `crates/vizdaw-app/Cargo.toml` (add `core-control = { path = "../core-control" }`)
- Test: `crates/vizdaw-app/tests/commands.rs`

**Interfaces:**
- Consumes: Task 2's `DocumentModel` methods; `core_control::request::ArgValue`, `core_control::{ControlError, ErrorCode}` (use the paths the crate actually exports; `vizdaw-gpui/src/control.rs` already imports them).
- Produces (exact):
  - `pub const DOCUMENT_COMMANDS: &[&str]` = `edit.undo`, `edit.redo`, `track.new_audio`, `track.new_instrument`, `track.new_midi`, `track.duplicate`, `track.delete`, `track.rename`.
  - `pub struct Ran { pub undo: Option<String>, pub status: Option<String> }`
  - `pub fn run(id: &str, args: &[(String, ArgValue)], doc: &mut DocumentModel) -> Result<Ran, ControlError>` for the IDs above.
  - `pub fn enabled(id: &str, doc: &DocumentModel) -> bool`: `edit.undo`/`edit.redo` need a label; `track.new_*` need a selected sequence; `track.duplicate|delete|rename` need a selected lane.
  - `pub fn perform_from_menu(id: &str, doc: &mut DocumentModel) -> Result<Ran, ControlError>`: the menu/key path (Swift `TrackCommands.performFromMenu`): uses the selection; `edit.undo`/`edit.redo` swallow `Busy` silently; `track.rename` returns `Err(dialog_required "Rename Track")` so the UI starts the inline rename.

Behaviour to port (Swift `TrackCommands.run`, `ControlCommands.swift:433-451`):
- `edit.undo`/`edit.redo` with no label → `unavailable` "Nothing to undo." / "Nothing to redo."; `Busy` → `busy`.
- `track.new_*`: params `seq` (default the shown sequence), `after` (lane id; default the selected lane); negative or non-integer → `invalid_params`; unknown `seq` → `failed`; unknown `after` → `failed` "There is no track N in this sequence."; selects the new lane. A new MIDI track with no MIDI destination sets the status Swift sets (`noMidiDestinationYet`; copy its text from `TrackCommands.swift`).
- `track.duplicate`: param `lane` (default the selection's first); selects the copy.
- `track.delete`: `EditCommand::Lane { edit: LaneEdit::Remove { seq, lane } }`.
- `track.rename`: no `name` → `dialog_required` "Rename Track" naming `name`; blank name → core refusal → `failed`.
- No document target / no selected sequence → `unavailable` with Swift's `noSequence` "Select a sequence first."; no lane → `noTrack` "Select a track in the arrangement first."
- `Ran.undo` is the snapshot's undo label afterwards; `Ran.status` the model status.

- [ ] **Step 1: Write the failing tests** (names ported from Swift): `each_new_item_makes_its_kind_below_the_selection`, `track_items_need_a_sequence`, `a_midi_track_without_destination_says_so`, `delete_the_only_track_leaves_no_selection`, `track_commands_refuse_hostile_input` (each hostile case from Review Focus 2; assert the error code and that the fake recorded no session edit), `undo_and_redo_on_a_busy_core_are_busy_errors`, `undo_with_nothing_to_undo_is_unavailable`, `every_document_command_is_in_the_registry` (each `DOCUMENT_COMMANDS` id is `registry::find(id)` with `document == true`), `the_menu_path_swallows_busy_undo`, `rename_from_the_menu_asks_for_the_name`.
- [ ] **Step 2: Run** `cargo test -p vizdaw-app --test commands` → fails to compile.
- [ ] **Step 3: Implement** `commands.rs`.
- [ ] **Step 4: Run** `cargo test -p vizdaw-app` → pass.
- [ ] **Step 5: Commit**: `git commit -m "core-app: command runner for undo, redo and track commands"`.

### Task 4: GPUI — menus and keys from the registry, Linux in-window menu bar

**Files:**
- Modify: `crates/vizdaw-gpui/src/menus.rs`, `crates/vizdaw-gpui/src/shell.rs`, `crates/vizdaw-gpui/src/lib.rs`
- Create: `crates/vizdaw-gpui/src/menu_bar.rs` (the in-window bar)
- Test: `crates/vizdaw-gpui/tests/menus.rs`, `crates/vizdaw-gpui/tests/window.rs`

**Interfaces:**
- Consumes: `vizdaw_app::commands::{DOCUMENT_COMMANDS, enabled, perform_from_menu}`; `core_control::registry::{COMMANDS, find}`; `core_control::bindings::{Keymap, Chord}`.
- Produces (exact):
  - Action `RunCommand { id: SharedString }` (GPUI action with data, in the `vizdaw` namespace), handled by `DocumentWindow` → `perform_from_menu` on its document, then title/menu refresh; `dialog_required` "Rename Track" starts the header's inline rename (Task 5 hook `DocumentWindow::begin_rename(lane)`).
  - `pub struct MenuModel { pub menus: Vec<MenuSpec> }`, `pub struct MenuSpec { pub title: String, pub items: Vec<ItemSpec> }`, `pub enum ItemSpec { Command { id: &'static str, title: String, keys: Option<String>, enabled: bool }, Separator }` and `pub fn menu_model(front: Option<&DocumentModel>, keymap: &Keymap) -> MenuModel`. Both `set_menus` (macOS) and the in-window bar render from it.
  - `pub fn keystroke(chord: &Chord) -> Option<String>`: a registry chord as a GPUI keystroke string (`cmd-`/`ctrl-` per platform as slice 1's `secondary!`, `alt-`, `shift-`, key names GPUI uses).
  - `pub fn bindings(keymap: &Keymap) -> Vec<KeyBinding>`: slice 1 bindings plus one per chord of each `DOCUMENT_COMMANDS` id; a chord with no modifier gets context `UNMODIFIED_KEYS`.
- Menu order and contents (Swift `AppCommands.swift`): VizDAW (Quit VizDAW) | File (slice 1 items) | Edit (Undo `<label>`, Redo `<label>`, separator, Cut, Copy, Paste, Select All) | Track (New Audio Track, New Instrument Track, New MIDI Track, separator, Duplicate Track, Delete Track, Rename Track) | Transport (slice 1 items) | Window (empty for now; macOS adds the window list). Titles come from the registry except Undo/Redo, which come from `DocumentModel::undo_title`/`redo_title`. Menus not yet built are left out; the parity checklist tracks them.
- Refresh: after any action, edit or `idle` snapshot, the front window recomputes `menu_model`; on macOS `set_menus` is called again only when the model changed.
- Linux (`not(target_os = "macos")`): `DocumentWindow` draws `MenuBar` (from `menu_bar.rs`) above the transport bar: top-level titles in a row; clicking one opens its item list as an overlay; clicking an item dispatches `RunCommand` (or the slice 1 action for slice 1 items) and closes the list; Escape or a click outside closes it; disabled items draw dimmed and ignore clicks. Each title and item registers in the element registry (roles `menu` and `menuitem`, label = title). The bar is built by `MenuBar::new(model)` with no platform check inside, so tests can build it on macOS too.

- [ ] **Step 1: Write the failing tests** in `tests/menus.rs`: `the_menu_model_follows_the_swift_order` (titles VizDAW, File, Edit, Track, Transport, Window), `track_items_take_their_titles_and_keys_from_the_registry` (New Audio Track shows the keystroke of `registry::find("track.new_audio").shortcut` via `keystroke`), `undo_shows_its_label_and_disables_without_one`, `track_items_need_a_selected_track` (Duplicate/Delete/Rename disabled with no selected lane), `single_keys_are_off_in_text_fields` (every binding from `bindings` whose keystroke has no modifier has context `UNMODIFIED_KEYS`), `keystroke_spells_modifiers_for_gpui` (⌥⌘A → `cmd-alt-a` on macOS / `ctrl-alt-a` elsewhere). In `tests/window.rs`: `the_in_window_menu_bar_runs_an_item` (build a window with `MenuBar` forced on, open Track, click New Audio Track via the registry, assert the fake recorded `new_track`), `escape_closes_the_open_menu`, `a_disabled_item_does_nothing`.
- [ ] **Step 2: Run** `cargo test -p vizdaw-gpui` → fails to compile.
- [ ] **Step 3: Implement** `menu_model`, `keystroke`, `bindings`, `RunCommand`, the refresh, and `MenuBar`.
- [ ] **Step 4: Run** `cargo test -p vizdaw-gpui` and `cargo clippy -p vizdaw-gpui --all-targets -- -D warnings` → pass. Remove the `// slice 2: Linux menu bar` marker.
- [ ] **Step 5: Commit**: `git commit -m "GPUI: menus and keys from the registry; in-window menu bar off macOS"`.

### Task 5: GPUI — the track header column

**Files:**
- Create: `crates/vizdaw-gpui/src/track_headers.rs`
- Modify: `crates/vizdaw-gpui/src/shell.rs` (lay the column under the transport bar; remove the `add_field` test hook or mark it `#[doc(hidden)]`, per slice 1's deferred minor)
- Test: `crates/vizdaw-gpui/tests/tracks.rs`

**Interfaces:**
- Consumes: Task 2 `DocumentModel` (`lanes`, `selected_lanes`, `select_track`, `toggle_mute`, `set_flags`, `rename_track`, `status`); Task 3 `perform_from_menu`; Task 4 `RunCommand`; slice 1 `TextInput`, `RegistryScope`.
- Produces: `TrackHeaders` view; `DocumentWindow::begin_rename(lane: u64)`.

Layout and labels (Swift `LaneHeaderView.swift`, `ArrangementLayout.swift`):
- Column width 200 px; one row per lane of the shown sequence; 1 px separator per row; selected rows tinted with the theme's selection fill at 0.22 alpha (slice 2 uses one fixed colour; themes come later).
- Row: kind icon (text glyph is fine this slice), name, M, S, R buttons. Click on icon or name selects (shift or the platform's secondary modifier extends). Double-click on the name opens an inline `TextInput` with the name: Enter commits `rename_track` (trimmed; blank or unchanged sends nothing), Escape cancels, focus loss commits. M/S/R call `set_flags` and never change the selection.
- Below the last row: "+ Add Track". A mouse click opens a small menu with New Audio Track, New Instrument Track, New MIDI Track (each `RunCommand`). A control-channel click opens no menu and sets the status to Swift's `addTrackOnChannel`: "Add Track opens a menu: use track.new_audio, track.new_instrument or track.new_midi over the control channel."
- Registry: container role `group`, label "Track Headers"; one `button` per row, label `"<Kind> track, <header>"` where Kind is Audio/Instrument/MIDI and header joins with ", ": the name (`"<name> · <take>"` when more than one take), the routing (`"output <name>"` or `"output none"` for audio; else the destination name or `"no destination"`), then `"muted"`, `"soloed"`, `"armed"` when set (input/monitor words come with the input slice); the row's channel press toggles mute (Swift parity); last child `button` "Add Track". M/S/R are not separate registry elements (Swift parity: e2e counts children).
- A rename open while the header is torn down (window closed, lanes refetched without that lane) ends without a command and leaves single-key shortcuts working.

- [ ] **Step 1: Write the failing tests** in `tests/tracks.rs`: `the_headers_and_add_row_show_beside_the_canvas` (tree has "Track Headers" with one child per lane plus "Add Track"), `the_header_names_kind_lane_and_routing` (labels for an audio, instrument and MIDI lane, muted/soloed/armed words), `a_header_click_selects_its_track`, `a_channel_click_on_add_track_opens_no_menu` (status text exact), `a_channel_press_on_a_row_toggles_mute`, `rename_commits_a_trimmed_name`, `escape_in_the_rename_field_cancels`, `a_header_torn_down_mid_rename_re_enables_transport_keys`, `delete_the_only_track_leaves_the_add_row`.
- [ ] **Step 2: Run** `cargo test -p vizdaw-gpui --test tracks` → fails.
- [ ] **Step 3: Implement** `TrackHeaders` and `begin_rename`.
- [ ] **Step 4: Run** `cargo test -p vizdaw-gpui` → pass.
- [ ] **Step 5: Commit**: `git commit -m "GPUI: track header column with select, mute/solo/arm, rename and Add Track"`.

### Task 6: GPUI control — document state and the new commands

**Files:**
- Modify: `crates/vizdaw-gpui/src/control.rs`
- Test: `crates/vizdaw-gpui/tests/control.rs`

**Interfaces:**
- Consumes: Task 3 `commands::run`; Task 5 Add Track and row press; `vizdaw_ffi::control::{ControlResult, result_json}`.
- Produces:
  - `state.get document`: `result_json(&ControlResult::Document { doc, path, snapshot, selected_seq, lanes, markers, destinations, alert })` with the model's latest snapshot, selected sequence and lanes, the session's markers and destinations for that sequence (find the `Session` getters the Swift app uses for `state.get document` in `app/Sources/VizDAW/ControlChannel.swift`; if a getter is missing from `DocSession`, add it in this task), and the document alert.
  - `state.get editor`: `{doc, status}` plus `gesture: null`; other editor fields arrive with the editor slices (ledger the subset).
  - `command.run`: `DOCUMENT_COMMANDS` go through `commands::run`; add `file.new` (new empty document window; body's `doc`/`window` name it) and `file.close` (closes the target window without prompting — Swift `control_close_never_prompts`). The body is `{command, doc, window, undo, status}`.
  - `app.quit` and window close use the live unsaved state (Task 2).
- `other_commands_are_unavailable_in_slice_1` is renamed `commands_outside_the_slice_are_unavailable` and checks an ID no slice implements yet (e.g. `mixer.new_bus`).

- [ ] **Step 1: Write the failing tests**: `state_get_document_matches_the_swift_fields` (keys doc, title, path, dirty, version, undo, redo, selected_seq, sequences, lanes, markers, destinations, warnings, edit_notes, alert all present), `a_track_edit_marks_the_document_dirty` (run `track.new_midi` → `state.get document` dirty true, title in `state.get windows` ends "— Edited"), `track_commands_answer_with_the_undo_label`, `hostile_track_params_keep_the_list` (Review Focus 2 cases through the channel), `file_close_never_prompts`, `file_new_opens_an_empty_window`, `commands_outside_the_slice_are_unavailable`.
- [ ] **Step 2: Run** `cargo test -p vizdaw-gpui --test control` → fails.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test --workspace` (redirect to a file; read the tail) → pass.
- [ ] **Step 5: Commit**: `git commit -m "GPUI control: state.get document and editor, track/undo/file commands"`.

### Task 7: e2e — tracks and unsaved songs against the GPUI app

**Files:**
- Modify: `crates/vizctl/tests/e2e_gpui.rs`

**Interfaces:**
- Consumes: everything above, over the real socket (`vizctl::launch`, `core_control::client::Client`).

Port from `crates/vizctl/tests/e2e.rs`, keeping every assertion that does not need a screenshot (GPUI has no `ui.screenshot` yet; ledger the dropped pixel checks):
- `gpui_new_project_tracks_over_the_channel` ← `new_project_tracks_over_the_channel`: `file.new`; "Track Headers" in `ui.tree`; empty lanes; channel click on "Add Track" → status text; three `track.new_*` → kinds `["audio","instrument","midi"]`, tree labels starting "Audio track, Audio 1", "Instrument track, Inst 1", "MIDI track, MIDI 1"; `edit.undo` → 2 tracks; `track.duplicate {lane}` → 3 tracks with `"Audio 1 copy"`; the hostile params and their codes; the list unchanged after each.
- `gpui_control_close_of_a_dirty_song_does_not_prompt` ← `control_close_of_a_dirty_song_does_not_prompt`.
- `gpui_app_quit_refuses_while_a_song_is_unsaved` ← `app_quit_refuses_while_a_song_is_unsaved`.

- [ ] **Step 1: Write the three tests** (they are `#[ignore]` like the existing one, needing `VIZDAW_E2E_APP`).
- [ ] **Step 2: Build and run on the Mac (controller)**: `scripts/make-gpui-app.sh && VIZDAW_E2E_APP=target/VizDAW-GPUI.app cargo test -p vizctl --test e2e_gpui -- --ignored` → 4 passed.
- [ ] **Step 3: Run on Linux (controller, over SSH)** with the slice 1 environment (XWayland `DISPLAY=:0`, the `XAUTHORITY` file, a 0700 temp home) → 4 passed. CI already runs `e2e_gpui` under xvfb.
- [ ] **Step 4: Tick the parity checklist** rows for `edit.undo`, `edit.redo` and the six `track.*` IDs on both platforms ("S2"), and the three e2e rows.
- [ ] **Step 5: Commit**: `git commit -m "e2e: tracks, dirty close and quit refusal against the GPUI app"`.
