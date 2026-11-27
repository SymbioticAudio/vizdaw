# 27.1.0 Slice 10b: Modify (the Select and Modify panel, Templates, Modify Again, Exact Mode, nudges) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The GPUI app has the Swift app's Modify menu and its Select and Modify panel.
- Modify › Select by Rule…, Transpose…, Quantize…, Change Values… and Move… open one floating "Select and Modify — ‹title›" panel per document. Apply runs one `SelectAndModify` edit.
- Templates are saved in the project, loaded, run from Modify › Templates, and deleted.
- Modify Again repeats the last modification in place. Its menu title names it.
- With Exact Mode on, a note or block drag opens the panel filled with the drag's exact amounts and leaves no undo step.
- The list bar has "Modify…".
- The three key-only chores and Editors › Nudge Earlier/Later/Up/Down (⌥←→↑↓) work.
- Every `modify.*` and `editors.nudge_*` id works over the control channel. `state.get editor` reports the real `exact_mode`.

**Architecture:**
- The core is complete (recon §2): `EditCommand::SelectAndModify`, `ModifyAgain`, `Settings{SaveTemplate|DeleteTemplate}`, `Session::modify_templates`, `last_modify`, `edit(cmd, filter)` and `run_chore`. Templates live in the project (`Document.settings.templates`), so there is no app-wide store.
- Everything Swift kept in `VizDAWAppModel` goes into `vizdaw-app` as pure code with unit tests, in a new `select_modify` module:
  - `mod.rs`: the vocabulary, the config, presets, the command, the template mapping, titles, scale masks and reseeding.
  - `fields.rs`: field lists and field text.
  - `model.rs`: `SelectModifyModel`, one per `DocumentModel`.
  - `exact.rs`: Exact Mode's request mapping.
- New code outside that module: `modify_commands.rs` (the command arms) and `editor/nudge.rs`.
- The session seam (`DocSession`, live and `FakeSession`) gains three calls.
- In GPUI, `modify_panel/` is a secondary window (`WindowKind::ModifyPanel`, "modify_panel"), opened the way `DocumentWindow::open_mixer` and `open_plugin_manager` open theirs, never through the sheet host. Its views only draw the model and send intents to it. `menus.rs` gains the Modify menu. The list bar and the piano-roll bar each gain one control.

**Tech Stack:** Rust 2024; GPUI at the slice 1 pin; `core-ffi` (`modify`, `edit`, `session`), `core-model` (`modify`, `rule`), `core-control`, `vizctl`.

**Spec:** `docs/superpowers/specs/2026-11-20-cross-platform-27-1-design.md`.
- Parity rows in `docs/superpowers/specs/2026-11-22-parity-checklist.md`:
  - `editors.nudge_earlier`…`nudge_down` (:83-86)
  - `modify.select_by_rule`…`modify.velocity_notes` (:93-103)
  - the window row "Modify panel (floating, one per document)" (:389)
  - README "List editor and Select and Modify (plan 4c)" (:504)
  - No Sheets or Alerts row belongs to Modify. Every panel message goes to the status line.
- **Recon with every Swift file:line and exact string:** `.superpowers/sdd/modify-slice-recon.md` (git-ignored). It has §1 Swift (1.1 menu, 1.2 control arms, 1.3 window, 1.4 layout and strings, 1.5 model, 1.6 Exact Mode, 1.7 tests, 1.9 README 62-66), §2 core, §3 control and wiring, §4 the seam, §5 e2e, §7 nudge and §8 risks.
  - Implementers read the sections their task names.
  - `S/` = `app/Sources/VizDAW/`, `M/` = `app/Sources/VizDAWAppModel/`, `T/` = `app/Tests/VizDAWAppModelTests/`, all at tag `swift-parity-baseline`. Read them with `git show "swift-parity-baseline:<path>"`, quoted (zsh reads `:a` as a history modifier).

## Global Constraints

- **Rules from slices 7a–10a that still apply:**
  - The Swift app is retired: match its behaviour, but don't keep compatibility with it.
  - Use Swift's exact strings, citing file:line (the recon has them, including the em dash in "Select and Modify — ‹title›" and the curly quotes in `Load “‹name›”`).
  - The UI says "track", never "lane". This includes Swift strings: write "Select tracks to search first: drag in a ruler or select a block." as Swift has it.
  - No vendor or DAW brand names in repo text (Opcode, Logic, Apple beyond platform facts, Midas, Soundcraft, Native Instruments, Microsoft, Ableton, Steinberg/Cubase, ROLI). Licence notices are exempt.
  - GPUI tests press `secondary-` keys. Linux CI runs the GPUI tests and the e2e on the offline backend, and every 10b test must pass there.
- **Before every commit:** `export PATH="$HOME/.cargo/bin:$PATH"`, then `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and the tests the task names. CI stops at Format.
- **Running the e2e on the Mac:** `cargo build -p vizdaw-gpui -p vizdaw-scan`, then `VIZDAW_E2E_APP=target/debug/vizdaw-gpui VIZDAW_AUDIO=offline cargo test -p vizctl --test e2e_gpui -- --ignored --exact <name>`.
- **Safety:**
  - Never launch the app against the real home directory: tests use `temp_home()`.
  - No GUI or computer-use tools.
  - Never kill a process you did not start.
  - Never commit `.DS_Store`, `.superpowers/` or `target/`.
- **Commit trailers:**
  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P
  ```
- **Seams:** no app or GPUI unit test opens a real session. They use `FakeSession`. All panel logic is in `vizdaw-app` and tested there. GPUI views hold no rules beyond layout.
- **The panel is a window, not a sheet.** It is never shown through `sheet_host`. It opens under the control channel too, like the Mixer window. ui.tree, ui.click and ui.key must reach it.
- **Out of scope; goes to the backlog** (they need the unported Strip Chart and its target menus): the "Strip Chart Line" operation (`ModifyKind.line`, `S/LineModifyEditor.swift`), the "Automation Target" condition, and the Strip Chart origin of Exact Mode.
  - The panel leaves them out of the Operation picker and the Add Condition menu. It does not show them disabled.
  - A template or `last_modify` that holds one is still data: it loads, runs, and is named by Modify Again.
  - Its row shows its title with no editor: Operation "Strip Chart Line", or an "Automation Target" condition row with only the remove button.
- **Also out of scope:**
  - Swift's `hidesOnDeactivate`. GPUI has no such option. Backlog it if manual checks miss it.
  - The Co-Producer chore options (backlog rows 129-130).
- **Review pace:** fix crashes, data loss and wrong overwrites. Log other edge cases to the backlog.

## Review Focus

1. **The panel and the transport keys.** While the panel window is active, its document's bare transport keys are off, so Space and Return typed in a field do not play or locate. Return in the panel runs Apply only. When the panel is deactivated, closed or its document closes, the keys come back and no text owner is left behind.
   - App tests: `a_key_panel_turns_the_transport_keys_off` and `closing_the_panel_clears_focus_key_and_text_owners` (Task 3).
   - GPUI test: `deactivating_the_panel_gives_the_transport_keys_back` (Task 5).
2. **Apply on the wrong events.**
   - When the selection changed after an Exact Mode drag, or a dragged event is gone, the pending request is dropped, and Apply acts on the current selection.
   - A failed edit puts the selection back.
   - Modify Again uses the filter of the surface that opened the panel, so kinds hidden in the list are not modified.
   - Tests: `a_changed_selection_drops_the_stale_request`, `a_vanished_event_drops_the_request`, `a_failed_edit_restores_the_selection` and `modify_again_uses_the_lists_filter_like_the_original_apply` (Task 3).
3. **7-bit fields over 16- and 32-bit values.**
   - "to 127" for velocity is `127<<9 | 0x1FF`, so the loudest notes match.
   - Positions are read in the shown sequence's meter.
   - An empty groove field is a plain grid, not an error.
   - Choosing "is" on a range sets high to low.
   - Tests: `the_high_velocity_edge_covers_the_top_step`, `positions_parse_in_the_given_meter`, `an_empty_groove_field_is_a_plain_grid` (Task 2), `choosing_is_on_a_range_sets_high_to_low` (Task 1).
4. **Exact Mode reverts racing the core.** A drag that ends while the core is Busy keeps the gesture, and the tick's retry opens the panel once. A failed revert opens no panel. Esc during an exact drag opens nothing. The menu check mark, the roll's "Exact" toggle and `state.get editor` agree.
   - Tests: `a_busy_revert_keeps_the_gesture_and_the_retry_opens_the_panel`, `a_failed_revert_opens_no_panel`, `escape_in_exact_mode_opens_nothing` (Task 8), the GPUI test `an_exact_block_drag_in_the_window_opens_the_panel` (Task 8), and the e2e's `exact_mode` read (Task 10).
5. **Panels and documents.**
   - With two projects open, each has its own panel, and the menu acts on the active panel's document.
   - Closing a document closes its panel. No orphan window is left to Apply against a dropped document.
   - Reopening a closed panel shows the config as it was.
   - GPUI tests: `one_panel_per_document_and_it_closes_with_its_document` and `a_reopened_panel_keeps_its_config` (Task 5).

---

### Task 1: The panel vocabulary and its config

**Files:**
- Create: `crates/vizdaw-app/src/select_modify/mod.rs`
- Modify: `crates/vizdaw-app/src/{lib.rs,editing_prefs.rs}` (`ScaleKind::mask`)
- Test: `crates/vizdaw-app/tests/select_modify.rs`

**Interfaces:**
- Produces (all ports of `M/SelectModify.swift`, recon §1.4–1.5; use the FFI rows from `vizdaw_ffi::modify` directly as the data):
  - **Tasks:** `pub enum PanelTask { SelectByRule, Transpose, Quantize, Change, Move }` with `ALL`, `title()` ("Select by Rule", "Transpose", "Quantize", "Change Values", "Move") and `command_id()` (`modify.select_by_rule`…).
  - **Halves and config:**
    - `pub struct SelectHalf { enabled, match_all, mode: SelectModeRow, conditions: Vec<ConditionRow> }`.
    - `pub struct ModifyHalf { enabled, op: ModifyRow, to_copy }`. Its defaults are as Swift's: `enabled: true`, `TransposeInterval{0}`, `to_copy: false`.
    - `pub struct ModifyConfig { select, modify }` with:
      - `preset(task, range: Option<(i64,i64)>) -> Self`, the values in recon §1.5. `Default` is `preset(Change, None)`.
      - `rule() -> Option<RuleRow>` and `modification() -> Option<ModifyRow>`.
      - `command() -> Option<EditCommand>`. `to_copy` is sent only when there is a modification and it is not a `Line`.
      - `from_template(&ModifyTemplateRow) -> Self` and `template(name: &str) -> ModifyTemplateRow`.
  - **Condition kinds:** `pub enum ConditionKind` (13 cases: Swift's 11 plus `Bracketed` and `Automation`). It has:
    - `title()`, with the titles in recon §1.4.
    - `of(&ConditionTest)`.
    - `default_test(range: Option<(i64,i64)>) -> ConditionTest`, with the defaults in recon §1.4.
    - `pub const OFFERED: [ConditionKind; 12]`, every kind except `Automation`.
  - **Comparisons:** `pub enum Comparison { Is, IsNot, Inside, Outside }` with `title()` ("is", "is not", "inside", "outside"), `choices(&ConditionTest) -> &'static [Comparison]`, `of(&ConditionRow)` and `applied(self, &ConditionRow) -> ConditionRow`.
  - **Operations:** `pub enum ModifyKind` (16 cases) with `title()`, `of(&ModifyRow)`, `default_op() -> ModifyRow`, and `pub const OFFERED: [ModifyKind; 15]` (every kind except `Line`). The titles and defaults are in `M/SelectModify.swift:228-298`. `Line` has no `default_op` caller in this slice, so it returns the op unchanged through `of`.
  - **Change pickers:** `ChangeTarget`, `ChangeOpKind`, `AmountKind`, `CurveKind`, `WaveKind` and `SubstituteChoice`, each with `ALL`, `title()`, and a conversion to and from its row. Their titles are in recon §1.4: "Velocity"…, "Set to"…, "Constant"…, "Straight"…, "Sine"…, and "Controller", "Pitch Bend", "Channel Pressure", "One Pitch".
  - **Modify Again:** `pub fn modify_title(op: &ModifyRow) -> &'static str` is Swift's `ModifyKind.title(_:)` with the line titles (`M/SelectModify.swift:300-340`). `pub fn modify_again_title(last: Option<&ModifyRow>) -> String` gives "Modify Again: ‹title›", else "Modify Again".
  - **Reseeding:** `pub fn reseeded(cmd: EditCommand, seed: u64) -> EditCommand` replaces the seed of every `AmountRow::Random` in a `SelectAndModify` and leaves any other command as it was.
  - **Strings:**
    - `MODIFY_AGAIN_HELP` = "Modify Again changes the selection in place."
    - `PANEL_TITLE_PREFIX` = "Select and Modify — "
  - **Scales:** `ScaleKind::mask(self) -> u16`, from the masks in `M/PitchMath.swift:16-45`. `ScaleKind::ALL` gives the panel's Scale picker. `PitchName` (list/format.rs) gives the Root picker.
- [ ] **Step 1: Write the failing tests.** Port `T/SelectModifyTests.swift` by name:
  - `presets_make_their_tasks_command`. For example, `preset(Transpose).command()` is `SelectAndModify{rule: None, mode: Select, modify: Some(TransposeInterval{semitones: 0}), to_copy: false}`. Select by Rule with a range has the conditions `[Kinds[Note], TimeRange{s,e}]` and modify `None`.
  - `nothing_turned_on_is_no_command`.
  - `templates_round_trip`: `from_template(c.template("Octave Up")) == c`.
  - `comparisons_read_and_set_ranges`.
  - `kinds_round_trip_through_their_defaults`: `ConditionKind::of(&k.default_test(None)) == k` for every kind, and the same for `ModifyKind` except `Line`.

  New tests:
  - `choosing_is_on_a_range_sets_high_to_low` (Review Focus 3): `Comparison::Is.applied` on `Pitch{60,72}` gives `Pitch{60,60}`, negate false.
  - `the_line_and_automation_kinds_are_not_offered`.
  - `a_line_modification_is_never_sent_to_a_copy`.
  - `modify_again_names_the_last_modify`: `TransposeDiatonic` gives "Modify Again: Transpose", `Change{Velocity,..}` gives "Modify Again: Change Velocity", and `None` gives "Modify Again".
  - `random_amounts_get_the_given_seed`.
  - `scale_masks_match_swift`: Major is `0b1010_1011_0101`, and so on, copied from PitchMath.
- [ ] **Step 2: Run** `cargo test -p vizdaw-app --test select_modify`. Expected: FAIL (missing module).
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the test, then `cargo test -p vizdaw-app`. Expected: PASS.
- [ ] **Step 5: Commit** `select_modify: the panel's tasks, config, presets, templates and titles`.

### Task 2: Field lists and field text

**Files:**
- Create: `crates/vizdaw-app/src/select_modify/fields.rs`
- Test: `crates/vizdaw-app/tests/select_modify_fields.rs`

**Interfaces:**
- Consumes: Task 1's types. It reuses `list::format::{DurationText, Velocity7, Value7, PitchName}` and `position::{Bbu, parse_position}`.
- Produces (ports `M/PanelFields.swift`):
  - **Types:** `pub enum Edge { Low, High, Delta, Exact }`, `pub enum FieldKind { Count, Bounded(i64,i64), Percent, Pitch, Velocity(Edge), Value(Edge), Length, Position, Bpm }` and `pub struct Field { pub label: &'static str, pub kind: FieldKind }`.
  - **Text:** `pub fn field_text(n: i64, k: FieldKind, meter: &[MeterEntryRow]) -> String` and `pub fn parse_field(s: &str, k: FieldKind, meter: &[MeterEntryRow]) -> Option<i64>`. Port the bounds exactly from `PanelFields.swift:46-107`:
    - count: |v| ≤ 1 000 000
    - percent: 0–10 000, with a "%" or without
    - velocity: low `v<<9`, high `v<<9|0x1FF`, delta `v*512` in ±127, exact `Velocity7::up`
    - value: `<<25`, high `|0x1FF_FFFF`
    - length: may be signed
    - bpm: "120.00", ±999, stored ×1000
  - **Fields:** `pub fn condition_fields(&ConditionTest) -> Vec<Field>`, `condition_numbers(&ConditionTest) -> Vec<i64>` and `condition_with(&ConditionTest, &[i64]) -> Option<ConditionTest>`. Likewise `modify_fields`, `modify_numbers` and `modify_with` for `&ModifyRow`. The labels are in recon §1.4 ("semitones", "steps", "around", "grid", "strength", "amount", "from", "to", "low", "high", "period", "phase", "lowest", "highest", "by", "times", "divided by", "controller", "pitch", "spacing", "every", "starting at", "offset", "within"). `Line` has no fields (out of scope).
  - **Note map and groove offsets:**
    - `pub fn note_map_text(&[NoteMapEntry]) -> String` writes "C2>D2, E2>F#2". `parse_note_map(&str) -> Option<Vec<NoteMapEntry>>` reads it back.
    - `pub fn offsets_text(&[i64]) -> String` and `parse_offsets(&str) -> Option<Vec<i64>>`. Empty or whitespace gives `Some(vec![])`.
  - **Strings:** `NOTE_MAP_PLACEHOLDER` = "C2>D2, E2>F#2", `GROOVE_LABEL` = "Groove offsets", `GROOVE_HELP` = "Groove offsets in ticks, one per grid line; empty for a plain grid".
- [ ] **Step 1: Write the failing tests.** Port by name: `fields_and_numbers_agree` (for every offered kind's default, `fields().len() == numbers().len()` and `with(numbers()) == Some(self)`), `field_text_round_trips`, `note_map_and_groove_text`, `typed_numbers_outside_their_range_are_rejected` ("128" velocity, "-1" pitch, "10001%") and `every_kind_round_trips_its_text_on_grid`. New tests:
  - `the_high_velocity_edge_covers_the_top_step`: `parse_field("127", Velocity(High), &[]) == Some(0xFFFF)`.
  - `positions_parse_in_the_given_meter`: "2.1.0" is 1920 ticks in 4/4 and 1440 ticks in 3/4, at 480 ppq.
  - `an_empty_groove_field_is_a_plain_grid`.
  - `a_negative_length_reads_and_writes_with_a_minus`.
- [ ] **Step 2: Run** `cargo test -p vizdaw-app --test select_modify_fields`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the test and `cargo test -p vizdaw-app`. Expected: PASS.
- [ ] **Step 5: Commit** `select_modify: field lists and field text`.

### Task 3: The session seam and the per-document panel model

**Files:**
- Create: `crates/vizdaw-app/src/select_modify/model.rs`
- Modify:
  - `crates/vizdaw-app/src/session.rs`: the trait near 94, the live impl near 536.
  - `crates/vizdaw-app/src/testing.rs`: `FakeSession` near 835, `fn edit` near 2309.
  - `crates/vizdaw-app/src/document.rs`: the new field, `transport_keys_enabled` at 732, and closing.
  - `crates/vizdaw-app/src/editor/mod.rs`: `exact_mode`.
- Test: `crates/vizdaw-app/tests/select_modify_session.rs` (the seam), `crates/vizdaw-app/tests/select_modify_model.rs` (the model)

**Interfaces:**
- Consumes: Tasks 1–2.
- Produces, first the seam: three calls on `DocSession`, each with a one-line live impl:
  - `fn modify_templates(&self) -> Result<Vec<ModifyTemplateRow>, VizError>`
  - `fn last_modify(&self) -> Result<Option<ModifyRow>, VizError>`
  - `fn edit_filtered(&self, cmd: EditCommand, filter: Option<ViewFilterRow>) -> Result<DocSnapshot, VizError>`. The live impl calls `Session::edit(self, cmd, filter)`. `edit` stays as it is.
- `FakeSession`:
  - Scripted `set_templates(Vec<ModifyTemplateRow>)` and `set_last_modify(Option<ModifyRow>)`.
  - `filtered_edits() -> Vec<(EditCommand, Option<ViewFilterRow>)>`. Every filtered edit also lands in `edits()`.
  - A small model of the core:
    - `Settings{SaveTemplate}` inserts or replaces by name. `DeleteTemplate` removes it.
    - `SelectAndModify` with a modification sets `last_modify`.
    - `ModifyAgain` with no `last_modify` fails `Refused("There is no modification to repeat.")`.
    - Busy while an edit is open, as `edit` already is.
  - `fail_template_reads: Option<VizError>` makes `modify_templates` and `last_modify` fail (for the Busy read test).
- Produces (ports `M/SelectModifyModel.swift` and `M/EditorModel+Modify.swift`, recon §1.5):
  - **State:** `pub struct SelectModifyModel { pub task: PanelTask, pub config: ModifyConfig, pub template_name: String, filter: Option<ViewFilterRow>, drafts: BTreeMap<String, String>, invalid: BTreeSet<String>, pending: Option<ModifyRequest>, baseline: Option<SelectionRow>, notice: Option<String>, is_open: bool, is_key: bool, wants_present: bool }`. Exact Mode's `ModifyRequest` is defined here and filled in by Task 8: `{ task, config, events: Option<Vec<u64>>, lanes: Vec<(u64,u64)>, range: Option<(i64,i64)>, dropped: Option<DroppedAxis> }`.
  - **On `DocumentModel`:**
    - `select_modify() -> &SelectModifyModel` and `select_modify_mut()`.
    - `open_modify(task, filter: Option<ViewFilterRow>)`: presets with the selection's range, clears drafts, invalid, pending and notice, and sets `wants_present`.
    - `take_modify_present() -> bool`: the GPUI shell polls it after each command or event, like `Ran.open_mixer_window`.
    - `apply_modify() -> bool`, `save_modify_template()`, `delete_modify_template(name: &str)`, `load_modify_template(&ModifyTemplateRow)`, `run_modify_template(name: &str) -> Result<(), String>` and `modify_again() -> Result<(), String>`.
    - `modify_templates() -> &[ModifyTemplateRow]`, `last_modify() -> Option<&ModifyRow>` and `can_modify_again() -> bool`, which is `last_modify().is_some() && gesture().is_none()`.
    - The reads are cached per snapshot version. A Busy or failed read keeps the last values (`ModifyReadTests`).
    - **Fields:**
      - `set_modify_field(key: &str, text: &str)` sets the draft. When the text parses, it updates the config and clears `invalid[key]`. Otherwise it marks the key invalid.
      - `modify_field_text(key) -> String` gives the draft, else the formatted number.
      - `modify_field_invalid(key) -> bool`.
      - Keys are `c<i>.<j>` (condition i, field j), `m<j>`, `map` and `offsets`.
    - **Panel focus:**
      - `modify_panel_focused(on: bool)`: on sets `is_key`. Off clears it and ends every `select_modify.*` text owner.
      - `modify_panel_closed()`: `is_open` false, `is_key` false, owners ended.
      - `transport_keys_enabled()` is also false while `is_key`.
      - Text owners are `"selectModify." + key`.
    - `apply_target_caption() -> &'static str`: "Apply acts on the dragged events." when a request is pending, else "Apply acts on the current selection."
    - `modify_again_filter() -> Option<ViewFilterRow>` follows `M/SelectModifyModel.swift:260-264`. Its order is: the panel's filter if it is open and key; else the list's `view_filter()` if a list is the command surface; else the panel's filter if it is open; else none.
    - `selection_hidden() -> bool`: the main view is Mix and the bottom pane shows neither Editor nor Audio Editor (`M/DocumentModel.swift:587-591`).
    - `editor.exact_mode: bool`, not saved, with `set_exact_mode(on)` and `exact_mode()`.
  - **Messages** (pub consts in `model.rs`, verbatim; each one goes to `editor.status`):
    - `NOTHING_ON` = "Turn on Select or Modify first."
    - `NO_TRACKS` = "Select tracks to search first: drag in a ruler or select a block."
    - `FIX_FIELDS` = "Fix the fields marked in red first."
    - `NOT_SELECTED` = "The dragged notes could not be selected."
    - `NO_NAME` = "Name the template first."
    - `GESTURE_OPEN` is reused from `editor/mod.rs:24` ("Finish the drag first.").
    - `HIDDEN_SELECTION` = "The Mix view shows no editor: press ⌃1, or show the Editor in the bottom pane, to edit the selection." On Linux, render ⌃1 as `Ctrl-1` the way the menus render keys.
    - `NO_MODIFICATION` = "There is no modification to repeat."
    - `NO_TEMPLATES` = "There are no modify templates."
  - **Apply order** (`apply()`, Swift 164-212):
    1. `invalid` is not empty: `FIX_FIELDS`.
    2. No command: `NOTHING_ON`.
    3. A gesture is open: `GESTURE_OPEN`.
    4. Drop a stale pending request.
    5. When a request is pending, select its events and check they took, else `NOT_SELECTED`.
    6. The track check: a rule, a mode other than Refine, and no tracks in the selection give `NO_TRACKS`, and the selection goes back.
    7. Send `edit_filtered(reseeded(cmd, next_seed()), filter)`. `next_seed` mixes a counter, so two Applies always differ.
    8. A failure puts the pre-request selection back.
- [ ] **Step 1: Write the failing tests.** For the seam, in `select_modify_session.rs`: `the_fake_saves_lists_and_deletes_templates`, `a_modify_sets_last_modify_and_modify_again_needs_one` and `filtered_edits_record_their_filter`.

  For the model, port `T/SelectModifyModelTests.swift` except its exact-mode tests, which Task 8 ports:
  - `opening_a_task_presets_and_presents_the_panel`
  - `apply_sends_one_command_with_the_opening_lists_filter`
  - `apply_without_tracks_says_what_to_select`
  - `apply_with_nothing_turned_on_says_so`
  - `typed_fields_keep_drafts_and_refuse_bad_text` ("abc" then Apply gives `FIX_FIELDS` and no edit)
  - `conditions_and_change_operations_edit`
  - `templates_save_load_run_and_delete`
  - `modify_again_uses_the_lists_filter_like_the_original_apply` (Review Focus 2)
  - `modify_again_sends_modify_again`
  - `random_amounts_get_a_new_seed_at_each_apply`
  - `a_key_panel_turns_the_transport_keys_off` (Review Focus 1)
  - `delete_template_trims_the_name`
  - `a_refused_selection_sends_no_edit`
  - `a_failed_edit_restores_the_selection`
  - `a_track_check_failure_restores_the_selection`
  - `an_open_drag_refuses_apply_without_touching_the_selection`
  - `a_second_apply_acts_on_the_current_selection`
  - `a_changed_selection_drops_the_stale_request`
  - `an_unrelated_snapshot_keeps_the_request`
  - `a_vanished_event_drops_the_request`

  The stale-request tests set `pending` with a test helper `set_modify_request_for_test`. Port these from `ModifyPanelWiringTests`/`ModifyReadTests`:
  - `closing_the_panel_clears_focus_key_and_text_owners`
  - `resigning_key_gives_the_keyboard_back_to_the_transport`
  - `the_apply_target_follows_the_dragged_request`
  - `modify_again_names_the_last_modify_and_waits_for_an_open_drag`
  - `text_owners_are_unique_per_field`
  - `modify_reads_happen_once_per_snapshot_version`
  - `a_failed_read_keeps_the_last_values`

  New tests:
  - `save_as_template_without_a_name_says_so`.
  - `the_mix_view_hides_the_selection`.
  - `exact_mode_starts_off_and_toggles`.
- [ ] **Step 2: Run** `cargo test -p vizdaw-app --test select_modify_session --test select_modify_model`. Expected: FAIL.
- [ ] **Step 3: Implement.** Use `ModifyConfig::preset` and `reseeded` from Task 1, and the fields from Task 2. Don't repeat them.
- [ ] **Step 4: Run** the test and `cargo test -p vizdaw-app`. Expected: PASS.
- [ ] **Step 5: Commit** `select_modify: the session seam and the per-document panel model`.

### Task 4: The Modify commands and the control channel

**Files:**
- Create: `crates/vizdaw-app/src/modify_commands.rs`
- Modify:
  - `crates/vizdaw-app/src/commands.rs`: `Ran` (226-236), the three entry points `enabled`, `perform_from_menu` and `run`.
  - `crates/vizdaw-gpui/src/control.rs`: `exact_mode` at 1014, the `not_yet` list near 768, and the deferred-window handling at 691.
  - `crates/vizdaw-gpui/src/shell.rs` (744-752).
- Test: `crates/vizdaw-app/tests/modify_commands.rs`; a GPUI control test

**Interfaces:**
- Consumes: Task 3.
- Produces:
  - **Ids:** `pub const MODIFY_COMMANDS: &[&str]` lists the five panel ids, `modify.again`, `modify.template`, `modify.exact_mode` and the three chores.
  - **Entry points:** `modify_commands::{enabled, perform_from_menu, run}` follow the existing `run_editor` pattern (commands.rs 743-773).
  - **`Ran`:** gains `open_modify_panel: bool`. A panel id calls `open_modify(task, None)` and returns it set. The GPUI shell and control.rs open the panel after the command, as they do for `open_mixer_window`, and Task 5 supplies the opener. Until Task 5 lands, the GPUI side leaves a `// Task 5` no-op.
  - **Arms** (recon §1.2, verbatim):
    - `modify.again`:
      - Hidden selection: `unavailable(HIDDEN_SELECTION)`.
      - `!can_modify_again()`: `unavailable(NO_MODIFICATION)`.
      - Refused: `failed(status or "Modify Again was refused.")`.
      - It sends `edit_filtered(ModifyAgain, modify_again_filter())`.
    - `modify.template {name}`:
      - No templates: `unavailable(NO_TEMPLATES)`.
      - Unknown name: `invalid_params("modify.template: no template named \"‹name›\"")`.
      - Refused: `failed(status or "The template was refused.")`.
    - `modify.exact_mode {on?}`: with `on` absent, it toggles.
    - The three chores each make one `doc.session.run_chore(id, args)` call, as `record::run_record` does (record.rs:333-342). The registry's validation stays where it is.
  - **Enabled:**
    - The panel ids need a document.
    - `modify.again` needs `can_modify_again() && !selection_hidden()`.
    - `modify.template` needs a non-empty template list.
    - `modify.exact_mode` needs a document.
  - **control.rs:** `state.get editor` reports `exact_mode: doc.editor().exact_mode()`. None of the 11 ids reaches `not_yet` any more.
- [ ] **Step 1: Write the failing tests:**
  - `each_panel_command_opens_the_panel_on_its_task`
  - `modify_again_refusals_say_why` (hidden, none, refused)
  - `modify_again_sends_the_panel_filter`
  - `modify_template_runs_by_name_and_refuses_unknown_and_none`
  - `exact_mode_toggles_or_takes_on`
  - `each_chore_makes_one_run_chore_call`
  - `the_menu_disables_modify_again_and_templates_when_empty`

  GPUI control test: `state_get_editor_reports_exact_mode`.
- [ ] **Step 2: Run** `cargo test -p vizdaw-app --test modify_commands`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p vizdaw-app` and `cargo test -p vizdaw-gpui`. Expected: PASS.
- [ ] **Step 5: Commit** `modify: the Modify commands, chores and exact mode over the channel`.

### Task 5: The Modify panel window and its frame

**Files:**
- Create: `crates/vizdaw-gpui/src/modify_panel/mod.rs` (the window, its opener, the top row and the bottom row)
- Modify:
  - `crates/vizdaw-gpui/src/app_state.rs`: `WindowKind` 21-50, `WindowEntry::registry` 76-81, `window_title` 310-317, and closing a document.
  - `crates/vizdaw-gpui/src/{shell.rs,control.rs,lib.rs}`: remove Task 4's no-ops.
- Test: GPUI tests in `modify_panel/mod.rs`

**Interfaces:**
- Consumes: Tasks 3–4.
- Produces:
  - **Window kind:** `WindowKind::ModifyPanel`, whose `name()` is "modify_panel". `is_document_window()` stays false.
  - **Opener:** `pub fn open_modify_panel(doc: DocId, cx: &mut App)`. It reuses the document's live panel window (`modify_panel_of(doc)`, with the stale-handle check of `open_plugin_manager`), else opens one:
    - `gpui::WindowKind::Floating`, 620×460, `window_min_size` 580×320, centred on the document window.
    - Title "Select and Modify — ‹document title›".
    - A failed open shows the "Window Failed" alert, as the Plugin Manager's does.
    - Then it activates the window and calls `store.focus_document(doc)`.
  - **Kept when closed:** the GPUI window is destroyed on close, but the config, drafts and template name live in `SelectModifyModel`, so a reopened panel shows them unchanged. `on_release` calls `modify_panel_closed()`.
  - **Closes with its document:** closing a document closes its panel window first.
  - **Title:** `window_title` gives the panel title and follows renames and Save As.
  - **Registry:** `WindowEntry::registry` learns the panel's `SharedRegistry`, so ui.tree, ui.click and ui.key reach it. `Command::UiKey` in `control.rs` (600-611) special-cases the Plugin Manager. Route it to the panel's text fields the same way, so a `ui.key` with `element` types into a panel field.
  - **Focus:** window activation calls `modify_panel_focused(true)` and deactivation calls it with false (`cx.observe_window_activation`). While it is active, `RunCommand` and the menu target its document: the panel view handles `RunCommand` by forwarding to its document as `DocumentWindow` does.
  - **The frame** (recon §1.4, verbatim labels; each label is its a11y label):
    - The top row has the "Task" picker (260 wide; changing it calls `open_modify(task, filter)`) and the "Templates" menu (120 wide). The menu shows "No Templates" when there are none. Otherwise it shows `Load “‹name›”` for each, a divider, then `Delete “‹name›”` for each.
    - The body hosts the two halves (Task 6).
    - The notice line.
    - The bottom row:
      - The "Template name" field (180) and "Save as Template".
      - The caption from `apply_target_caption()`, in the accent colour while a request is pending.
      - "Modify Again", disabled unless `can_modify_again()`, with help `MODIFY_AGAIN_HELP`.
      - "Apply", the default button: Return in the panel runs it.
      - The document's status line.
  - Theme tokens only. `no_source_file_keeps_a_colour_literal` stays green.
- [ ] **Step 1: Write the failing GPUI tests:**
  - `modify_transpose_opens_one_panel_titled_for_the_document`: `state.get windows` lists kind "modify_panel", and the title is "Select and Modify — Untitled".
  - `one_panel_per_document_and_it_closes_with_its_document` (Review Focus 5).
  - `a_reopened_panel_keeps_its_config` (Review Focus 5).
  - `deactivating_the_panel_gives_the_transport_keys_back` (Review Focus 1).
  - `return_in_the_panel_applies_and_does_not_locate`.
  - `the_templates_menu_says_no_templates_when_empty`.
  - `ui_tree_lists_the_panels_apply_button`.
- [ ] **Step 2: Run** `cargo test -p vizdaw-gpui modify_panel`. Expected: FAIL.
- [ ] **Step 3: Implement.** Copy the window plumbing from `shell.rs:242-296` and `plugin_manager_window.rs:489-530`.
- [ ] **Step 4: Run** `cargo test -p vizdaw-gpui`. Expected: PASS.
- [ ] **Step 5: Commit** `modify: the floating Select and Modify panel window`.

### Task 6: The panel's Select and Modify halves

**Files:**
- Create: `crates/vizdaw-gpui/src/modify_panel/{select_half.rs,modify_half.rs}`
- Test: GPUI tests in those files

**Interfaces:**
- Consumes: Tasks 1, 2, 3 and 5. Text fields are `text_input.rs` (`TextInput::new(label, cx).compact()`). Pickers are `controls/menu_face.rs` and toggles are `toggle.rs`.
- Produces (recon §1.4; labels and a11y verbatim):
  - **Select half:**
    - The "Select" toggle.
    - The "Result" picker: "Replace the Selection", "Add to the Selection", "Refine the Selection".
    - The "Match" picker: "All Conditions", "Any Condition".
    - One row per condition:
      - The kind title, 120 wide.
      - The comparison picker (a11y "Comparison"), from `Comparison::choices`.
      - Then, depending on the kind:
        - Kind: the Kinds menu. It reads "No Kind" or the joined kind titles, with a toggle per kind of `PanelKinds.all`, using the `ListKind` titles.
        - Destination: the "Destination" picker, with "No Destination" and then the session's destinations.
        - The other kinds: text fields with a11y "‹Kind› ‹label›", red while invalid.
      - The remove button (help and a11y "Remove Condition").
    - The "Add Condition" menu offers `ConditionKind::OFFERED`.
  - **Modify half:**
    - The "Modify" toggle.
    - The "Operation" picker offers `ModifyKind::OFFERED`. A loaded `Line` shows its title with no editor.
    - The segmented "Apply" picker: "In Place" / "To a Copy".
    - The specials per operation:
      - "Root" and "Scale" for Transpose in Scale and Snap to Scale.
      - The "Note Map" field.
      - "Groove offsets" for Quantize.
      - For Change Values: "Value to Change", "Operation" and "Amount", plus "Curve" for a line amount and "Wave" for an oscillating one.
      - The "Destination" picker for Set Destination.
      - "Substitute With" for Substitute.
    - The numeric fields come from `modify_fields`.
  - Each field's text goes through `set_modify_field` and owns the text owner `selectModify.‹key›` while it is focused.
- [ ] **Step 1: Write the failing GPUI tests:**
  - `typing_semitones_and_apply_sends_one_transpose`: FakeSession records `SelectAndModify{modify: TransposeInterval{12}}`.
  - `bad_text_turns_the_field_red_and_apply_says_fix_the_fields`.
  - `add_condition_offers_no_automation_target_and_remove_drops_the_row`.
  - `the_operation_picker_offers_no_strip_chart_line`.
  - `change_values_along_a_line_shows_from_to_and_curve`.
  - `to_a_copy_sends_to_copy`.
- [ ] **Step 2: Run** `cargo test -p vizdaw-gpui modify_panel`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p vizdaw-gpui`. Expected: PASS.
- [ ] **Step 5: Commit** `modify: the panel's select and modify halves`.

### Task 7: The Modify menu, the list bar's "Modify…" and the roll's "Exact"

**Files:**
- Modify:
  - `crates/vizdaw-gpui/src/menus.rs`: `menu_model` 600-; the dynamic titles near 555; a new action.
  - `crates/vizdaw-gpui/src/list_editor.rs`: `bar` 468.
  - `crates/vizdaw-gpui/src/piano_roll.rs`: `bar` 552.
- Test: GPUI tests in `menus.rs`, `list_editor.rs` and `piano_roll.rs`

**Interfaces:**
- Consumes: Tasks 3–5.
- Produces:
  - **The menu:** a "Modify" menu between Editors and Track (`S/AppCommands.swift:228-254`):
    - The five panel items, titled "‹task title›…".
    - A separator.
    - `modify.again`, whose title is `modify_again_title(doc.last_modify())`, like `edit.undo`'s `undo_title`.
    - The "Templates" submenu: one item per template, each dispatching `RunModifyTemplate { name: SharedString }` (a new `#[action(namespace = vizdaw, no_json)]` handled by the document window and the panel; it calls `run_modify_template`). The submenu is disabled when there are none.
    - A separator.
    - "Exact Mode", with its check mark from `exact_mode()`.

    The keys come from the registry (⌃⌘S/T/U/M/V/R/E). Disabled items use `UnavailableCommand`, as elsewhere.
  - **The list bar:** a "Modify…" button after the existing items. It calls `open_modify(PanelTask::Change, list.view_filter())` and opens the panel (`S/ListEditorPane.swift:102`).
  - **The roll bar:** an "Exact" toggle bound to `exact_mode`, with help "Exact mode: a drag opens Select and Modify with its amounts (⌃⌘E)" (`S/PianoRollPane.swift:68-69`).
- [ ] **Step 1: Write the failing GPUI tests:**
  - `the_modify_menu_lists_every_item_with_its_key`
  - `modify_again_is_titled_by_the_last_modify`
  - `the_templates_submenu_runs_a_template_by_name_and_is_disabled_when_empty`
  - `exact_mode_shows_a_check_mark_that_matches_the_roll_toggle`
  - `the_list_bars_modify_opens_change_values_with_the_lists_filter`
- [ ] **Step 2: Run** `cargo test -p vizdaw-gpui -- menus list_editor piano_roll`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p vizdaw-gpui`. Expected: PASS.
- [ ] **Step 5: Commit** `modify: the Modify menu, the list's Modify… and the roll's Exact toggle`.

### Task 8: Exact Mode drags

**Files:**
- Create: `crates/vizdaw-app/src/select_modify/exact.rs`
- Modify:
  - `crates/vizdaw-app/src/editor/gesture.rs`: `Gesture` 19-41, `begin_gesture` 71, `end_gesture` 139, `tick` 183.
  - `crates/vizdaw-app/src/document/block_drag.rs` (110).
  - `crates/vizdaw-app/src/piano_roll/drag.rs`: the note drags at 757, and the `Band` end at 810-824.
- Test: `crates/vizdaw-app/tests/exact_mode.rs`; one GPUI test in `crates/vizdaw-gpui/src/arrangement.rs`'s tests

**Interfaces:**
- Consumes: Task 3's `ModifyRequest`, `pending` and `open_modify`.
- Produces (ports `M/ExactMode.swift` and `M/EditorModel+Gestures.swift:80-125`, recon §1.6):
  - **Types:** `pub enum GestureOrigin { Notes, Blocks, RegionStrip, Expression, Other }`. There is no `StripChart` (out of scope). `Gesture.origin` defaults to `Other`. `pub fn begin_gesture_from(&mut self, label, lanes, origin) -> bool`, and `begin_gesture` delegates with `Other`. Block drags pass `Blocks` and roll note drags pass `Notes`.
  - `pub enum DroppedAxis { Vertical, Horizontal }` with `message()`: "The vertical part of the drag was left out." / "The horizontal part of the drag was left out."
  - **Mapping:** `pub fn exact_request(cmd: &EditCommand, lanes: &BTreeSet<(u64,u64)>, origin: GestureOrigin) -> Option<ModifyRequest>`. It follows `ExactMode.swift:54-105`:
    - A roll scale-time is Change with scaleTime.
    - Move is Move.
    - Duplicate is Move or Transpose with `to_copy`.
    - Nudge is Move or Transpose.
    - Resize is Change Length Add.
    - Time wins when `|ticks| / (PPQ/4) >= |semitones|`, and the other axis is dropped.
    - Anything else is `None`.
  - `pub fn select_request(range: (i64,i64), pitches: (u8,u8), lanes: Vec<(u64,u64)>, adding: bool) -> ModifyRequest` gives the rubber band.
  - **End of a gesture:** in `end_gesture`, when `exact_mode()` is on, the gesture is not finishing and a request exists, cancel the gesture instead of applying it. Once the revert succeeds (at once, or on `tick` after Busy), set the selection to the request's range, tracks and events, and call `open_modify_request(r)`. That sets the task, the config, `pending` when there are events, the notice and `wants_present`. A failed revert opens nothing. Esc (`cancel_gesture`) opens nothing.
  - **Rubber band:** in Exact Mode, the roll's band end calls `open_modify_request(select_request(..))` instead of selecting. With ⇧, the request is Add and the selection is kept.
- [ ] **Step 1: Write the failing tests.**
  - Port `T/ExactModeTests.swift` by name: `a_move_opens_move_on_the_dragged_events`, `copies_and_transposes`, `a_same_track_block_copy_opens_move_to_a_copy`, `other_drags_apply_as_usual`, `region_moves_apply_as_usual`, `diagonal_drags_keep_their_stronger_axis`, `the_panel_carries_the_drags_exact_amounts`, `a_rubber_band_becomes_a_select_rule`.
  - Port `T/ExactGestureTests.swift` by name: `an_exact_drag_leaves_no_undo_step_and_opens_the_panel`, `the_baseline_is_taken_after_the_revert_and_the_selection`, `a_diagonal_drag_shows_the_dropped_axis_notice`, `only_note_and_block_drags_go_exact`, `a_cross_track_move_applies_as_usual`, `escape_in_exact_mode_opens_nothing`, `a_pencil_click_in_exact_mode_still_inserts`, `with_exact_mode_off_a_drag_applies`, `a_busy_revert_keeps_the_gesture_and_the_retry_opens_the_panel`, `a_failed_revert_opens_no_panel`, `escape_on_a_busy_abort_is_retried_by_the_tick`, `a_shift_band_in_exact_mode_keeps_the_selection`, `an_arrangement_block_drag_goes_exact`, `escape_during_an_arrangement_exact_drag_opens_nothing`, `a_piano_roll_note_drag_goes_exact_but_a_pencil_click_does_not`, `a_region_move_applies_in_exact_mode`, `an_expression_stroke_applies_in_exact_mode`.
  - Port the two exact tests of `SelectModifyModelTests`: `exact_mode_selects_the_dragged_events_and_applies_off_grid_amounts_unedited` and `exact_mode_selection_is_set_before_the_command`.
  - GPUI test: `an_exact_block_drag_in_the_window_opens_the_panel`. With Exact Mode on, a simulated mouse drag of a block in a `DocumentWindow` over `FakeSession` opens a "modify_panel" window on Task "Move" and leaves no edit in `edits()`.
- [ ] **Step 2: Run** `cargo test -p vizdaw-app --test exact_mode`. Expected: FAIL.
- [ ] **Step 3: Implement.** The GPUI shell already polls `take_modify_present()` (Task 5), so no GPUI change is needed beyond what the tests reveal.
- [ ] **Step 4: Run** `cargo test -p vizdaw-app` and `cargo test -p vizdaw-gpui`. Expected: PASS.
- [ ] **Step 5: Commit** `modify: exact mode drags open the panel with their amounts`.

### Task 9: Nudges

**Files:**
- Create: `crates/vizdaw-app/src/editor/nudge.rs`
- Modify:
  - `crates/vizdaw-app/src/editing_prefs.rs`: `NudgeSettings::ticks` and `key`; `key_root`.
  - `crates/vizdaw-app/src/commands.rs`: `EDITOR_COMMANDS` 41-47 and `run_editor`.
  - `crates/vizdaw-gpui/src/menus.rs`: the Editors menu.
  - The GPUI command path that hands the app's `EditingPrefs` (`app_state.rs:100`) to the arm.
- Test: `crates/vizdaw-app/tests/nudge.rs`

**Interfaces:**
- Consumes: Task 3's `selection_hidden()`. The `NudgeSettings` and `ScaleKind::mask` from Task 1.
- Produces (ports `M/Nudge.swift:49-178`, recon §7):
  - **Settings:**
    - `NudgeSettings::ticks(&self, tick: i64, meter: &[MeterEntryRow], snap: &Snap) -> i64` gives at least 1. `Grid` is the snap step with snap forced on, `Division` is that note value's step, and `Ticks(n)` is n.
    - `NudgeSettings::key(&self, tick, meter) -> (u8, u16)`. It follows the key signature, `key_root(sharps, minor)` with the Major or NaturalMinor mask, unless `follow_key_signature` is off. In that case it gives `(root % 12, scale.mask())`.
    - `pub fn key_root(sharps: i8, minor: bool) -> u8` is ported from Swift `ScaleMath.keyRoot`.
  - **Nudging:** `pub enum NudgeDirection { Earlier, Later, Up, Down }`.
    - `DocumentModel::nudge_anchor() -> Option<(u64, i64)>`: the range start, else the earliest selected event, else 0. It is `None` without selected tracks.
    - `DocumentModel::nudge(dir, &NudgeSettings) -> bool`. Time sends `Event::Nudge{events, ticks: ±t, semitones: 0}`. Pitch sends `Nudge{semitones: ±amount}`, or `TransposeInScale{steps: ±amount, root, mask}` when `pitch_scale_steps` is on.
  - **Commands:** `editors.nudge_earlier`, `nudge_later`, `nudge_up` and `nudge_down` join `EDITOR_COMMANDS`.
    - They are disabled with no document, with the transport keys off, with no selected events, or with a hidden selection.
    - The channel's refusals: `unavailable(HIDDEN_SELECTION)`; `unavailable("Nudge needs selected events and no text being edited.")`; `failed("The nudge was refused.")` (`M/ControlCommands.swift:640-648`).
  - **Menu:** "Nudge Earlier", "Nudge Later", "Nudge Up" and "Nudge Down" go in the Editors menu with ⌥←→↑↓ from the registry.
- [ ] **Step 1: Write the failing tests.** Port `T/NudgeTests.swift` by name: `time_steps`, `keys_follow_the_signature_or_are_fixed`, `nudge_sends_time_and_pitch_commands`, `scale_steps_read_the_key_at_the_anchor`, `nothing_selected_nothing_sent`, `refused_while_a_drag_is_open`, `nudges_wait_while_the_mix_view_hides_the_selection` and `the_anchor_is_the_range_start_else_the_earliest_selected_event`. New test: `each_nudge_refusal_says_why`. GPUI test: `the_editors_menu_lists_the_four_nudges_with_their_keys`.
- [ ] **Step 2: Run** `cargo test -p vizdaw-app --test nudge`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p vizdaw-app` and `cargo test -p vizdaw-gpui`. Expected: PASS.
- [ ] **Step 5: Commit** `editors: nudge the selection earlier, later, up and down`.

### Task 10: The e2e and the docs

**Files:**
- Modify:
  - `crates/vizctl/tests/e2e_gpui.rs`: reuse `temp_home` 134, `start_demo` 480, `window_numbers` 227, `wait_for_undo` 919 and `find_role`.
  - `docs/superpowers/specs/2026-11-22-parity-checklist.md`
  - `docs/testing/manual-checks.md`
  - `docs/feature-backlog.md`

- [ ] **Step 1: Write the e2e tests** (each `#[ignore = "needs a window server and VIZDAW_E2E_APP"]`, temp home):
  - `gpui_modify_panel_transposes_saves_a_template_and_repeats`:
    1. Select the Chords block.
    2. `command.run modify.transpose`, and find the window whose kind is "modify_panel" in `state.get windows`. Its title is "Select and Modify — ‹demo title›".
    3. `ui.key` "12" into the textfield "semitones", click "Apply", then `wait_for_undo(.., "Transpose")`.
    4. Type "Octave Up" into "Template name" and click "Save as Template".
    5. `command.run modify.template {"name":"Octave Up"}` succeeds. With `{"name":"Nope"}`, it is `invalid_params` naming "Nope".
    6. `command.run modify.again` succeeds, and the undo label is still "Undo Transpose".
    7. Save As into the temp home, reopen, and `modify.template "Octave Up"` still runs.
    8. Typing "abc" into "semitones" and clicking Apply shows the status "Fix the fields marked in red first.".
  - `gpui_modify_exact_mode_toggles_and_move_applies` (no drag, per Ruling 9):
    1. In the fresh demo, `modify.again` is `unavailable` "There is no modification to repeat.".
    2. `modify.exact_mode {"on": true}`: `state.get editor` shows `exact_mode: true`.
    3. `modify.exact_mode` with no params: it shows `false`.
    4. Select the Chords block and run `modify.move`. The panel's Task is "Move".
    5. Type "1.0" into "by" and click "Apply": `wait_for_undo(.., "Move")`.
  - `gpui_nudge_moves_the_selection_and_says_why_not`: with a note selected, `editors.nudge_later` gives "Undo Nudge Events" (the core's label, `command/event.rs:166`). With nothing selected, it is `unavailable` "Nudge needs selected events and no text being edited.".
- [ ] **Step 2: Run** each on the Mac (Global Constraints command). Expected: PASS. When a step fails for an app reason, fix the app with a test that fails first, not the e2e.
- [ ] **Step 3: Docs.**
  - **Checklist:** put "S10b" in the Mac cells of rows 83-86 and 93-103, with the e2e name where one covers the row. Row 389 gets "S10b (`gpui_modify_panel_transposes_saves_a_template_and_repeats`; on screen: M27)". Row 504 gets "S10b (Select and Modify; Strip Chart Line and Automation Target wait for the Strip Chart)". Leave the Linux cells for the controller's post-CI tick.
  - **Manual checks:** the next free numbers are M27 and L33.
    - M27 (Mac, README 62-66): the panel floats above the document windows; Return applies and does not play; To a Copy +4 makes a harmonizing copy; the Change shapes; a template survives Save As and reopen; Exact Mode's block and note drags and the rubber band; ⌥←→↑↓ nudges.
    - L33 (Linux, XWayland): the panel opens as a free window over its document, draws with the system font, and takes the keyboard without the transport reacting; closing the project closes it.
  - **Backlog:**
    - A new row "Strip Chart Line and the Automation Target condition in the Modify panel": they wait for the GPUI Strip Chart; the Strip Chart origin of Exact Mode is in this row too.
    - A new row for Swift's `hidesOnDeactivate`, unless M27 finds the panel fine without it.
    - Update row 342 ("Editing settings have no consumers yet"): the nudge settings now have consumers.
    - Update row 283: the `modify.transpose` panels note.
    - Add a "GPUI slice 10b: review leftovers" row for the minors found in review.
- [ ] **Step 4: Commit** the e2e with `e2e: the Modify panel, templates, Modify Again, exact mode and nudges`. Commit the docs with `docs: slice 10b checklist, manual checks and backlog`.

## Rulings made while planning

1. **Strip Chart Line and Automation Target are left out of the panel's menus, not shown disabled.** Swift showed nothing for items it could not offer, and a disabled entry would invite a bug report for an unported editor. A template or `last_modify` holding one still loads, runs and is named. Cost if wrong: two menu entries.
2. **"Kept when closed" keeps the model, not the window.** GPUI closes and destroys a window. The config, drafts and template name live in `SelectModifyModel`, so a reopened panel looks as it was left. Cost if wrong: the panel's scroll position.
3. **The panel uses `gpui::WindowKind::Floating`; Swift's `hidesOnDeactivate` is not ported.** On Linux under XWayland, how it floats is whatever the window manager does; L33 checks it. Cost if wrong: a panel that stays visible when the app is in the background.
4. **Save as Template replaces a visible template of the same name, as Swift did.** It is one "Set Modify Template" undo step, so nothing is lost. The core already refuses a hidden one (`HIDDEN_TEMPLATE_MESSAGE`). Cost if wrong: an Undo.
5. **Modify Again's title uses Swift's own title table** (`M/SelectModify.swift:300-340`), not the core's `Modify::name()`. The two differ only for controller values and legato, and the menu must match Swift. Cost if wrong: one menu word.
6. **`selection_hidden` is ported here**, because Modify Again and the nudges share it. A torn-off editor window counts as no editor, as backlog row 283 notes.
7. **Nudges are their own task (Task 9) before the e2e.** They do not depend on the panel, so they can move to the next editors slice if the slice runs long.
8. **The panel view is two tasks**: its frame (Task 5) and its halves (Task 6). A reviewer can reject the halves' strings while approving the window plumbing. The session seam is folded into the model task (Task 3), and the e2e and the docs share Task 10.
9. **The e2e does not drag.** The GPUI control channel has no `ui.drag` (`control.rs:618` answers `not_yet`), and this slice does not add one.
   - The exact-mode drag is covered by the app tests and one GPUI test that drives the mouse in the test context (Task 8), and on screen by M27.
   - The e2e checks the `modify.exact_mode` toggle through `state.get editor` and runs the panel's Move from `modify.move`.
   - Cost if wrong: a wiring bug between the real window's mouse events and `end_gesture` that only M27 would catch.
