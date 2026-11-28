# 27.1.0 Slice 10c: The Strip Chart (the editor, the controllers pane, automation rows, and the Modify panel's Strip Chart Line and Automation Target) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The GPUI app has the Swift app's Strip Chart.
- Editors › Strip Chart (⌘3, Ctrl+3 off macOS) shows it in the bottom pane. The bottom pane's picker offers it, and a song saved with it as the last editor opens it.
- It has a bar (Mode, Operation, Shape, Curve, Wave, Period Kind, Period, Phase, Step, Track, Rows) and up to four stacked rows. Each row shows velocity, release velocity, length, a controller, pitch bend, channel or poly pressure, program, tempo or an automation target.
- Draw, Modify (lines and Compress rectangles) and Erase strokes are each one undo step, and Escape leaves none. ⌥ (Alt off macOS) at the press limits a stroke to the selected events.
- Each row's title opens its menu: what the row shows, MIDI Learn for an automation row, Add Row and Remove Row.
- The write in progress shows on its automation row while the chart's sequence plays.
- The right pane's Controllers view is the focused track's Strip Chart.
- The List editor's automation rows show their target and a percent, and a typed percent replaces the value.
- The Modify panel offers "Strip Chart Line", with its target, operation and shape pickers and its fields, and the "Automation Target" condition with its target picker. In Exact Mode a Strip Chart stroke opens the panel with the stroke's line.
- The bar settings and each chart's rows are saved with the project.

**Architecture:**
- **The core is complete.** It has `ModifyRow::Line`, `LineRow`, `LineTargetRow`, `LineOpRow`, `LineShapeRow`, `PeriodRow` and `SeriesRow` (`core-ffi/src/strip.rs`), `ConditionTest::Automation`, `EventRow.automation`, `Session::automation_preview` (`core-ffi/src/session.rs:8823`), and the normalised-value laws in `core_model::automation` (`unit`, `fader_db`, `fader_norm`, `pan_of`, `velocity_scale_percent`, `to_u32`, `from_u32`, `FULL_SCALE`) with `core_model::mixer::GAIN_FLOOR_DB`.
- **App (`vizdaw-app`).** Everything Swift kept in `VizDAWAppModel` becomes pure code with unit tests in a new `strip_chart` module:
  - `lane.rs`: the rows, names and value text.
  - `scale.rs`: value ↔ y, modes and operations.
  - `settings.rs`: the bar's settings and choices.
  - `line.rs`: strokes as lines and commands.
  - `targets.rs`: the mixer's automation targets.
  - `model.rs`: per-chart state on `DocumentModel`.
  - `frame.rs`: the canvas geometry.
- **The layout** gains `stripChart` and `controllers` (`editor/layout.rs`). `DetailState` gains `stripRows` and `stripRowsLane` accessors over its `extra` (`editor/detail.rs`).
- **The session seam** gains one call, `automation_preview`.
- **Modify** gains the line fields, the line and automation setters, and the Strip Chart origin of Exact Mode (`select_modify/`).
- **GPUI.** `strip_chart.rs` is the view: the bar, a ruler, the rows canvas and the row menu. There are two instances on `DocumentWindow`: the bottom pane's, and the right pane's Controllers. `cascade_menu.rs` is a menu with nested submenus; the row menu and the panel's target pickers use it. `modify_panel/` gains the line editor and the automation target picker. Views draw the model and send intents to it; they hold no rules beyond layout.

**Tech Stack:** Rust 2024; GPUI at the slice 1 pin; `core-ffi` (`strip`, `modify`, `edit`, `events`, `automation`, `session`), `core-model` (`automation`, `mixer`), `core-control`, `vizctl`.

**Spec:** `docs/superpowers/specs/2026-11-20-cross-platform-27-1-design.md`.
- Strip Chart design: `docs/superpowers/specs/2026-10-16-editors-design.md` §4d (lines 108-111) and `docs/superpowers/plans/2026-10-28-automation-ui.md` (Plan 4d Rulings 15-26).
- Parity rows in `docs/superpowers/specs/2026-11-22-parity-checklist.md`:
  - `editors.strip_chart` (:74)
  - README "Strip Chart and automation (plan 4d)" (:505)
  - README "List editor and Select and Modify (plan 4c)" (:504), whose note this slice amends
- Swift sources, all at tag `swift-parity-baseline`. Read them with `git show "swift-parity-baseline:<path>"`, quoted (zsh reads `:a` as a history modifier).
  - `S/` = `app/Sources/VizDAW/`, `M/` = `app/Sources/VizDAWAppModel/`, `E/` = `app/Sources/VizDAWEditors/`, `T/` = `app/Tests/VizDAWAppModelTests/`.
  - The files: `S/StripChartPane.swift`, `S/LineModifyEditor.swift`, `S/ModifyPanel.swift:155-200,385-386`, `S/PaneViews.swift:223-235`, `M/StripChart.swift`, `M/StripChartModel.swift`, `M/StripLane.swift`, `M/AutomationText.swift`, `M/ExactMode.swift:56,107-120`, `M/SelectModifyModel.swift:434-560`, `M/PanelFields.swift:183-260`, `M/EditorLayout.swift:26-67`, `E/StripChartView.swift`.
  - The tests: `T/StripChartTests.swift`, `T/StripChartModelTests.swift`, `T/StripChartViewTests.swift`, `T/StripLaneTests.swift`, `T/LinePanelTests.swift`, `T/AutomationRowsTests.swift`.
  - The README's manual steps 68-80 (`app/README.md:658-752`).
  - Every string below is copied from those files; cite the file:line next to each constant.

## Global Constraints

- **Rules from slices 7a–10b that still apply:**
  - The Swift app is retired: match its behaviour, but don't keep compatibility with it.
  - Use Swift's exact strings, citing file:line. This includes the "·" in "Fader · Synth", the "▾" after a row title, the "–" in "0–31", "‰" and "°".
  - The UI says "track", never "lane". This includes Swift's "Lane" wording.
  - No vendor or DAW brand names in repo text (Opcode, Logic, Apple beyond platform facts, Midas, Soundcraft, NI/Native Instruments, Microsoft, Ableton, Steinberg/Cubase, ROLI). Licence notices are exempt.
  - GPUI tests press `secondary-` keys.
  - Linux CI runs the GPUI tests and the e2e on the offline backend, and every 10c test must pass there.
- **Keys off macOS** follow the codebase's existing mapping (10b Outcome 3):
  - ⌘3 is Ctrl+3. No ⌃3 chord exists, so nothing yields.
  - ⌥ at a press is Alt (GPUI `Modifiers::alt`).
- **Before every commit:** `export PATH="$HOME/.cargo/bin:$PATH"`, then `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and the tests the task names. CI stops at Format.
- **Running the e2e on the Mac:** `cargo build -p vizdaw-gpui -p vizdaw-scan`, then `VIZDAW_E2E_APP=target/debug/vizdaw-gpui VIZDAW_AUDIO=offline cargo test -p vizctl --test e2e_gpui -- --ignored --exact <name>`.
- **Safety:**
  - Never launch the app against the real home directory: tests use `temp_home()`.
  - No GUI or computer-use tools.
  - Never kill a process you did not start.
  - Never commit `.DS_Store`, `.superpowers/`, generated FFI files, `target/` or brand assets. This slice adds no FFI.
- **Commit trailers:**
  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P
  ```
- **Seams:** no app or GPUI unit test opens a real session. They use `FakeSession` (`roll_fake()` for Chords 3/31 with notes 1 at 0 and 2 at one quarter; `mixer_fake()` for the strips Synth 1, Bus 1 2 and Master 0).
- **The look:** theme tokens only (`crate::arrangement::colors`, `transport_bar::colors`, `strip_palette::tokens()`). `no_source_file_keeps_a_colour_literal` stays green.
- **Nothing per frame touches the session** beyond what the piano roll already reads:
  - The automation targets are read once per snapshot version and slots version.
  - Values are drawn and recorded only in view (at most 200 per row in the tree, `E/StripChartView.swift:404`).
- **Out of scope:**
  - Torn-off editor windows: `editors.strip_chart_window` (⌥⌘3), the rows "Torn-off editor: …", "Open in Window", and the Swift tests `aTornWindowHasItsOwnStripChart` and `aChartWhoseSlotStopsShowingOneGoes` (window half). These come in a later slice, and `editors.strip_chart_window` keeps answering `not_yet` until then.
  - The Drum Grid and the Audio Editor: a later slice.
  - `automation.write`. It is Swift's Co-Producer creative command (`M/CreativeCommands.swift:4-20`, plan 8e), not a Strip Chart command, so it stays for the Co-Producer slice (Ruling 1).
- **Review pace:** fix crashes, data loss and wrong overwrites. Log other edge cases to the backlog.

## Review Focus

1. **A stroke that never ends cleanly.** Each of these leaves no open session edit and no undo step: a mouse-up that never arrives, Escape, the chart leaving the pane (the detail switched to another editor, or closed), the window losing focus, and the document closing.
   - A new press cancels a stroke whose release was lost.
   - Tests: `a_new_stroke_cancels_one_whose_mouse_up_was_lost` and `a_chart_that_stops_showing_drops_its_stroke` (Task 4); `leaving_the_pane_mid_stroke_leaves_no_undo_step` and `escape_before_the_dead_zone_sends_nothing` (Task 6).
2. **A hostile or old `ui.json`.** None of these panics, and each falls back field by field:
   - a row height of 5000, an unknown shape, zero periods, a phase of 4000, a negative zoom or one of 1e12;
   - a `stripRows` list with an unknown entry, or more than four rows;
   - rows naming a strip or slot that is gone;
   - a `controllers` map with a bad entry.

   Tests: `settings_decode_tolerantly` and `malformed_rows_read_as_one_velocity_row` (Task 2), `saved_rows_of_deleted_targets_are_dropped` (Task 3).
3. **Rows written to the wrong track (wrong overwrite).** The Controllers chart saves its rows under the key of the track it read, never another one.
   - With no focused track it shows nothing and writes nothing.
   - An automation-only track's own row gives way to rows saved for that track, and rows saved on another track don't stop it opening on its target.
   - Tests: `the_controllers_chart_keeps_rows_per_track` and `with_no_focused_track_the_controllers_chart_writes_nothing` (Task 3), `rows_saved_on_another_lane_let_an_automation_lane_open_on_its_target` (Task 3).
4. **A busy mixer, and per-frame reads.**
   - A Busy read is not cached: the next read retries, and later Busy keeps the last menus.
   - A 20-move stroke and its frames read the mixer at most once.
   - The row menu's Automation item is disabled, with its help, while nothing could be read.
   - Tests: `a_busy_read_is_not_cached_and_the_next_call_fills_the_menus` (Task 3), `a_stroke_loads_the_automation_targets_once` and `an_empty_automation_menu_says_why` (Task 6).
5. **Edge values.**
   - A line or erase ending at `i64::MAX` does not overflow (`after`).
   - Non-finite tempo is skipped.
   - A parameter with an infinite or 1e300 range still reads as text.
   - A pointer above or below a row pins at its range's ends, and a tick left of the origin is 0.
   - Tests: `rules_at_the_last_tick_do_not_overflow`, `tempo_skips_non_finite_bpm`, `law_edges_and_texts` and `scale_clamps_outside_the_row` (Task 1); `a_line_at_the_last_tick_does_not_overflow` (Task 2); `points_follow_the_scrolled_offsets_and_clamp_at_the_edges` and `ticks_left_of_the_origin_clamp_to_zero` (Task 4); `a_line_request_at_the_last_tick_does_not_overflow` (Task 7).

---

### Task 1: Rows, names, value text, the row scale, and the List editor's automation rows

**Files:**
- Create: `crates/vizdaw-app/src/strip_chart/{mod.rs,lane.rs,scale.rs}`
- Modify:
  - `crates/vizdaw-app/src/lib.rs`
  - `crates/vizdaw-app/src/list/rows.rs`: `data1` 674-677, `data2` 716, `editable` 733.
  - `crates/vizdaw-app/src/list/edit.rs`: `commit`.
- Test: `crates/vizdaw-app/tests/strip_lane.rs`; additions to `crates/vizdaw-app/tests/list_edit.rs`

**Interfaces:**
- Produces (ports `M/StripLane.swift`, `M/AutomationText.swift`):
  - **Rows:** `pub enum StripLane { Velocity, ReleaseVelocity, Duration, Controller(u8), PitchBend, ChannelPressure, PolyPressure(u8), Program, Tempo, Automation(AutomationTargetRow) }`. It derives `Debug, Clone, Copy, PartialEq, Eq, Hash` and has hand-written serde in Swift's `Codable` shapes (Ruling 5):
    - `{"velocity":{}}`, `{"controller":{"_0":7}}`, `{"polyPressure":{"_0":60}}`
    - `{"automation":{"_0":{"fader":{"strip":1}}}}`, with the targets `fader`, `pan`, `mute`, `solo`, `send{strip,index}`, `param{slot,param}`, `velocityScale{dest}` and `vcaFader{vca}`

    Its methods (Swift `StripLane`, :206-393):
    - `pub const CONTROLLERS: [u8; 8] = [1, 2, 4, 7, 10, 11, 64, 74]`, with the names Modulation, Breath, Foot, Volume, Pan, Expression, Sustain and Brightness.
    - `from_line_target(LineTargetRow) -> Option<StripLane>`: `Value{Value}` is `None`.
    - `line_target(self) -> LineTargetRow` and `is_note_value(self) -> bool`.
    - `style(self) -> StripStyle { Bars, Steps, Ramps }`.
    - `range(self) -> (i64, i64)`: velocities `(0, 0xFFFF)`, duration `(0, 4*PPQ)`, tempo `(30_000, 300_000)`, else `(0, u32::MAX)`.
    - `center(self) -> Option<i64>`: `0x8000_0000` for pitch bend and `StripPan`.
    - `values(self, &[EventRow], &[TempoPointRow]) -> Vec<StripValue>`.
    - `erase_rule(self, from, to) -> Option<RuleRow>`.
    - `title(self, &StripNames) -> String` and `text(self, v: i64, &StripNames) -> String`.
  - **Values:** `pub struct StripValue { pub id: Option<u64>, pub tick: i64, pub length: i64, pub value: i64, pub ramp: bool }`.
  - `pub fn is_switch(t: &AutomationTargetRow) -> bool`: mute and solo.
  - **Names:** `pub struct StripNames { strips, sends, slots, params, dests, vcas }` (BTreeMaps as in Swift :146-152), with:
    - `param(slot, id) -> Option<&ParamRow>`
    - `knows(&AutomationTargetRow) -> bool`: with no strips, every target passes.
    - `title(&AutomationTargetRow) -> String`:
      - "Fader · ‹strip›", "Send ‹n› to ‹bus› · ‹strip›", "‹param› · ‹plugin›", "Velocity Scale · ‹dest›", "VCA ‹name› Fader"
      - The fallbacks "Strip ‹id›", "Parameter ‹id›", "Plugin ‹id›", "Destination ‹id›".
  - **Text** (Swift `AutomationLaw`, over the core's laws, Ruling 3):
    - `pub fn after(t: i64) -> i64` saturates at `i64::MAX`.
    - `pub fn v32(v: f64) -> i64` and `pub fn normalized(v32: i64) -> f64`.
    - `pub fn automation_text(t: &AutomationTargetRow, v: f64, param: Option<&ParamRow>) -> String`:
      - "-∞ dB" below floor+0.05, else "%.1f dB"
      - "C"/"L50"/"R50"
      - "On"/"Off" at 0.5
      - `plain_text` for a parameter
      - "%.0f%%" for a velocity scale
    - `pub fn plain_text(x: f64) -> String` (places by magnitude, trailing zeros trimmed, "-0" is "0").
    - `pub fn percent(v: f64) -> String`: "%.1f%%" of `unit(v)`.
    - `pub fn parse_percent(&str) -> Option<f64>`: "0"…"100", "%" optional.
    - `pub fn short_title(t: &AutomationTargetRow) -> String` is `StripNames::default().title(t)`.
  - **Scale** (`scale.rs`):
    - `pub struct StripScale { pub range: (i64, i64), pub top: f32, pub height: f32, pub inset: f32 }`. `new(range, top, height)` makes `inset` 3, non-finite top 0 and negative height 0. Its methods are `y(v)`, `fraction(y)`, `value_at(y)` and `amount_at(y, op: StripOp)`: Scale 0–200, Add ± half the range, else the value.
    - `pub enum StripMode { Draw, Modify, Erase }`, serde lowercase, with `ALL` and `title()`.
    - `pub enum StripOp { Set, Scale, Add, LimitMax, LimitMin, Compress, Thin, Thicken, Legato }`, serde camelCase ("limitMax"), with:
      - `ALL`
      - `title()`: "Set", "Scale", "Add", "Max Limit", "Min Limit", "Compress", "Thin", "Thicken", "Legato"
      - `row() -> LineOpRow`
      - `choices(lane) -> Vec<StripOp>`: Legato only for Duration; Thin and Thicken not for note values
  - **The List editor** (`T/AutomationRowsTests.swift`):
    - An automation row's Data 1 is `short_title(target)`, and its Data 2 is `percent(value)`.
    - Data 2 is editable and Data 1 is not.
    - Committing "50" gives `ListChange::Edit(Event{Replace{event, row}})` with `automation.value` 0.5 and `value = v32(0.5)`. "150" is a `ListEditError`.
- [ ] **Step 1: Write the failing tests.** Port `T/StripLaneTests.swift` by name, with the Swift values:
  - `laws_match_the_core`
  - `law_edges_and_texts`
  - `rules_at_the_last_tick_do_not_overflow`: only its `after` and `erase_rule` parts. The line part is Task 2's and the exact part Task 7's.
  - `targets_round_trip_their_json`, the serde version of `targetsRoundTripTheirRows`: every target of `all` round-trips as an `Automation` lane. `{"automation":{"_0":{"send":{"strip":5,"index":2}}}}` decodes to `Automation(SendLevel{strip:5, send:2})`.
  - `lanes_read_their_series`
  - `lanes_name_targets_text_values_and_build_rules`
  - `tempo_skips_non_finite_bpm`
  - `scale_clamps_outside_the_row`

  Port `T/AutomationRowsTests.swift`:
  - `automation_rows_show_their_target_and_percent`: "Send 2 · Strip 3", "25.0%", and the summary "Automation, bar 1 beat 1, Send 2 · Strip 3, 25.0%".
  - `typing_a_percent_replaces_the_value`.
  - The names half of `vca_fader_targets_are_named`: "VCA Drums Fader", "VCA 3 Fader", "-6.0 dB", and `knows`.
- [ ] **Step 2: Run** `cargo test -p vizdaw-app --test strip_lane --test list_edit`. Expected: FAIL (missing module).
- [ ] **Step 3: Implement.** Use `core_model::automation` for every law. Don't port Swift's `AutomationLaw` maths.
- [ ] **Step 4: Run** the tests, then `cargo test -p vizdaw-app`. Expected: PASS.
- [ ] **Step 5: Commit** `strip_chart: rows, names, value text and the row scale; automation rows in the list`.

### Task 2: The bar's settings, strokes as lines and commands, and the saved layout

**Files:**
- Create: `crates/vizdaw-app/src/strip_chart/{settings.rs,line.rs}`
- Modify:
  - `crates/vizdaw-app/src/editor/layout.rs`: `EditorLayout` 482-519, `decode` 565, `entries` 597, `merge_key` 653.
  - `crates/vizdaw-app/src/editor/detail.rs`: `DetailState` 123-160.
  - `crates/vizdaw-app/src/select_modify/mod.rs`: make `line_title` `pub`, and add serde to `WaveKind`.
- Test: `crates/vizdaw-app/tests/strip_chart.rs`

**Interfaces:**
- Consumes: Task 1.
- Produces (ports `M/StripChart.swift`):
  - **Pickers:**
    - `pub enum ShapeKind { Straight, Curved, Flat, Free, Random, Oscillator }`, serde lowercase, with `ALL`, `title()` ("Straight", "Curved", "Flat", "Free", "Random", "Oscillator") and `of(&LineShapeRow)`.
    - `pub enum LineCurve { EaseIn, EaseOut, SCurve }`, serde "easeIn"/"easeOut"/"sCurve", with `ALL`, `title()` ("Ease In", "Ease Out", "S-Curve") and `row() -> CurveRow`.
    - `pub enum PeriodKind { Musical, Clock, Bars }`, with `title()` ("Musical", "Clock", "Bars (follow meter)") and `of(&PeriodRow)`.
    - The wave is `select_modify::WaveKind` with `#[serde(rename_all = "camelCase")]`.
  - **Settings:** `pub struct StripChartSettings { mode, op, shape, curve: LineCurve, wave: WaveKind, period: PeriodKind, period_ticks: i64, period_ms: u32, period_bars: u32, phase: u16, step: i64, row_height: f64, ticks_per_point: f64 }`.
    - Its defaults: Draw, Set, Straight, EaseOut, Sine, Musical, PPQ, 500, 1, 0, PPQ/8, 96 and 96.
    - `ROW_HEIGHT_RANGE: RangeInclusive<f64> = 40.0..=400.`
    - `pub fn decode(&Value) -> Self` is tolerant, as `PianoRollSettings::decode` is:
      - Each field that is missing or malformed keeps its default.
      - Periods and step are at least 1, and the phase is at most 999.
      - The row height and zoom go through `EditorLayout::sane` with `ROW_HEIGHT_RANGE` and `TICKS_PER_POINT_RANGE`.
      - The JSON keys are Swift's: `mode`, `op`, `shape`, `curve`, `wave`, `period`, `periodTicks`, `periodMs`, `periodBars`, `phase`, `step`, `rowHeight`, `ticksPerPoint`.
    - `period_row() -> PeriodRow` and `shape_row(seed: u64) -> LineShapeRow` (`M/StripChart.swift:136-157`).
  - **Lines:** `pub struct StripPoint { pub tick: i64, pub value: i64 }`, `pub fn free_points(&[StripPoint]) -> Vec<LinePointRow>` and `pub fn stroke_line(a: StripPoint, b: StripPoint, stroke: &[StripPoint], s: &StripChartSettings, seed: u64) -> LineRow`. The line follows `M/StripChart.swift:182-197`:
    - A backward drag is turned around.
    - Flat holds `a`'s value.
    - Free goes through `stroke`, or `[a, b]` when it is empty.
  - **Commands:**
    - `pub fn stroke_op(lane, &StripChartSettings) -> LineOpRow`.
    - `pub fn line_command(lane, op, line: LineRow, selected_only: bool) -> EditCommand`: a `SelectAndModify` whose rule is `TimeRange{from, after(to)}`, with mode Refine when `selected_only`, else Select, and `to_copy` false.
    - `pub fn compress_command(lane, a, b, step, selected_only) -> EditCommand`.
    - `pub fn erase_command(lane, from, to) -> Option<EditCommand>`.
    - `pub fn stroke_label(lane, op) -> &'static str` is `select_modify::line_title(&lane.line_target(), op)`.
  - **Bar choices:** `pub struct StripBarChoices` with Swift's lists (:237-273):
    - steps "1/4"…"1/128"
    - musical periods "1 Bar of 4/4", "1/2"…"1/32"
    - clock 25…4000 ms
    - bars 1, 2, 4, 8, 16
    - phases 0, 125, 250, 500, 750

    `steps(current) -> Vec<(String, i64)>` and its siblings add an out-of-list current value as "‹n› ticks", "‹n› ms", "‹n› bar(s)" or "‹deg›°" (one decimal when not whole).
  - **Layout:**
    - `EditorLayout.strip_chart: StripChartSettings` under "stripChart", merged as a struct.
    - `EditorLayout.controllers: BTreeMap<String, Vec<StripLane>>` under "controllers", replaced whole. An entry that fails to decode is dropped.
    - `pub fn controllers_key(seq: u64, lane: u64) -> String` gives "3:31".
  - **Detail:** on `DetailState`:
    - `strip_rows() -> Option<Vec<StripLane>>`: `None` when missing or malformed. A list with an unknown entry is `None`, and so is one with more than `MAX_ROWS + 1 = 5` rows.
    - `set_strip_rows(Option<Vec<StripLane>>)`.
    - `strip_rows_lane() -> Option<(u64, u64)>`: JSON `{"seq":3,"lane":30}`.
    - `set_strip_rows_lane(Option<(u64, u64)>)`.
- [ ] **Step 1: Write the failing tests.** Port `T/StripChartTests.swift` by name, with `q = 15_360`:
  - `a_backward_drag_is_turned_around`
  - `a_click_draws_one_point`
  - `shapes_come_from_the_settings`
  - `commands_follow_the_mode_operation_and_modifier`: the labels are "Draw Controllers", "Draw Tempo", "Draw Automation" and "Change Velocity".
  - `a_zero_width_compress_is_one_tick`
  - `erase_deletes_the_rows_series_only`
  - `a_backward_compress_rectangle_is_turned_around`
  - `a_flat_line_drawn_right_to_left_holds_the_pressed_value`
  - `settings_decode_tolerantly`

  The exact-mode tests are Task 7's. Port from `T/StripChartModelTests.swift`: `bar_pickers_keep_an_out_of_list_value` and `a_detail_saved_without_its_rows_lane_decodes`.

  New tests:
  - `a_line_at_the_last_tick_does_not_overflow`: `line_command` over `to: i64::MAX` has `TimeRange{0, i64::MAX}`.
  - `malformed_rows_read_as_one_velocity_row` (Review Focus 2): `strip_rows()` is `None` for `[{"bogus":{}}]` and for six rows.
  - `the_settings_and_rows_survive_a_save`: `merge_into` keeps an unknown key inside "stripChart". "controllers" and "stripRows" round-trip.
- [ ] **Step 2: Run** `cargo test -p vizdaw-app --test strip_chart`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the test, `cargo test -p vizdaw-app --test detail`, and `cargo test -p vizdaw-app`. Expected: PASS.
- [ ] **Step 5: Commit** `strip_chart: the bar's settings, lines and commands, and the saved layout`.

### Task 3: Automation targets and what a chart shows

**Files:**
- Create: `crates/vizdaw-app/src/strip_chart/{targets.rs,model.rs}`
- Modify:
  - `crates/vizdaw-app/src/session.rs`: the trait near 466, the live impl near 1008.
  - `crates/vizdaw-app/src/testing.rs`: `FakeSession`.
  - `crates/vizdaw-app/src/document.rs`: the charts and the target cache.
  - `crates/vizdaw-app/src/learn.rs`: `target_title` 65.
- Test: `crates/vizdaw-app/tests/strip_chart_model.rs`

**Interfaces:**
- Consumes: Tasks 1–2.
- Produces:
  - **The seam:** `fn automation_preview(&self, target: AutomationTargetRow) -> Result<Vec<PreviewPointRow>, VizError>` on `DocSession`, with a one-line live impl. `FakeSession` gets `set_preview_rows(Vec<PreviewPointRow>)`, and the call is recorded as `"automation_preview"`.
  - **Targets** (`M/StripChartModel.swift:5-60`):
    - `pub struct StripMenu { pub title: String, pub lanes: Vec<StripLane> }` and `pub struct AutomationTargets { pub names: StripNames, pub menus: Vec<StripMenu> }`.
    - `pub fn load(s: &dyn DocSession) -> Result<AutomationTargets, VizError>`. Busy is an `Err`; any other error reads that part as empty.
    - Each strip gets a menu titled with its name: fader, pan (not on master), mute, solo, then each send.
    - Each slot with automatable, visible parameters gets "‹plugin› · ‹strip›".
    - Then "Velocity Scale", one item per destination, and "VCAs".
  - **The cache:** `DocumentModel::automation_targets(&self) -> Arc<AutomationTargets>`, one per document (Ruling 4).
    - It is cached on `(snapshot().version, slots_version())`.
    - While a gesture is open, the cache is used as it is.
    - Busy keeps the previous value, or an empty one, and caches nothing, so the next call retries.
    - `learn::target_title` returns `doc.automation_targets().names.title(t)`. Its tests (`tests/learn.rs`) stay green.
  - **Slots:** `pub enum ChartSlot { Pane, Controllers }` (no window slot, Ruling 12). `DocumentModel` holds one `StripChartModel { chosen_lane: Option<(u64,u64)>, stroke: Option<StripStroke>, seed: Option<u64> }` per slot. Task 4 fills `stroke`.
  - **What is shown,** as methods on `DocumentModel` (`M/StripChartModel.swift:180-330`):
    - `strip_state(slot) -> Option<DetailState>`:
      - The pane's is `layout().detail`.
      - The Controllers' needs `selected_seq()` and `focus_lanes().first()`. It gives `DetailState{kind: StripChart, target: Lane{seq, lane}}` with `strip_rows` from `layout().controllers[controllers_key]` and `strip_rows_lane = (seq, lane)`.
      - Otherwise it is `None`.
    - `strip_target(slot) -> Option<EditorTarget>`.
    - `strip_lane(slot) -> Option<(u64, u64)>`: the target's track, else the chosen one if it still exists, else the sequence's first.
    - `set_strip_chosen_lane(slot, (u64, u64))`.
    - `strip_rows(slot) -> Vec<StripLane>`:
      - The saved rows (else `[Velocity]`) lose the automation rows `names.knows` rejects. Nothing left is `[Velocity]`.
      - Unless the rows were saved for this very track, a track holding only one target's automation opens on `opening(t, kept)`.
    - `set_strip_row(slot, i, lane)` and `add_strip_row(slot)`: adds `Controller(7)` while there are fewer than `MAX_ROWS = 4`.
    - `remove_strip_row(slot, i)`: one row stays.
      - Each writes `strip_rows` and `strip_rows_lane` = the shown track. For the pane that is into the detail. For the Controllers it goes into `layout.controllers` under the key of the `DetailState` it read.
    - `strip_settings() -> &StripChartSettings` and `update_strip_settings(impl FnOnce(&mut StripChartSettings))`.
    - `strip_meter(slot) -> Vec<MeterEntryRow>`, `strip_events(slot) -> Arc<[EventRow]>` and `strip_values(slot, row: StripLane) -> Vec<StripValue>` (the tempo comes from `sequence_info`).
    - `strip_preview(slot, row, playing_seq: Option<u64>) -> Vec<StripValue>`. It is empty unless `row` is automation and `playing_seq` is the chart's sequence. Otherwise it is the session's preview: `id: None`, `length: 0`, `ramp: true`, `value: v32(v)`. An error is empty.
    - `pub fn only_target(&[EventRow]) -> Option<AutomationTargetRow>` and `pub fn opening(t, kept: &[StripLane]) -> Vec<StripLane>`.
    - `pub const MAX_ROWS: usize = 4`.
- [ ] **Step 1: Write the failing tests.** Port `T/StripChartModelTests.swift` by name:
  - `shows_the_target_lane_and_its_rows`
  - `a_sequence_target_shows_the_chosen_lane_else_the_first`
  - `automation_targets_come_from_the_mixer`: the menus are `["Synth", "Gain · Synth", "Bus 1", "Master", "Velocity Scale"]`, and hidden parameters are left out.
  - `saved_rows_of_deleted_targets_are_dropped`
  - `a_busy_read_is_not_cached_and_the_next_call_fills_the_menus`, using `set_call_error("mixer", Some(busy))`.
  - `the_preview_shows_only_while_its_target_is_written`
  - `an_automation_lane_opens_on_its_target`
  - `an_automation_lanes_own_row_gives_way_to_saved_rows`
  - `rows_saved_on_another_lane_let_an_automation_lane_open_on_its_target`
  - `the_targets_row_joins_the_saved_rows_once`

  Port from `T/LinePanelTests.swift`: `automation_targets_refresh_when_a_plugin_finishes_loading`, against the shared cache. Port the loading half of `vca_fader_targets_are_named`: the last menu is `StripMenu{"VCAs", [Automation(VcaFader{3})]}`.

  New tests (Review Focus 3):
  - `the_controllers_chart_keeps_rows_per_track`: the rows set while Chords is focused are under "3:31". Focus Bass, then add a row, and "3:32" changes while "3:31" does not.
  - `with_no_focused_track_the_controllers_chart_writes_nothing`: `strip_state(Controllers)` is `None`, and `add_strip_row(Controllers)` leaves `layout().controllers` empty.
- [ ] **Step 2: Run** `cargo test -p vizdaw-app --test strip_chart_model`. Expected: FAIL.
- [ ] **Step 3: Implement.** Follow the roll's `Mutex` cache pattern (`piano_roll/model.rs:88,177-200`) for the `&self` reads.
- [ ] **Step 4: Run** the test, `cargo test -p vizdaw-app --test learn`, and `cargo test -p vizdaw-app`. Expected: PASS.
- [ ] **Step 5: Commit** `strip_chart: automation targets and what a chart shows`.

### Task 4: Strokes and the canvas geometry

**Files:**
- Create: `crates/vizdaw-app/src/strip_chart/frame.rs`
- Modify:
  - `crates/vizdaw-app/src/strip_chart/model.rs`
  - `crates/vizdaw-app/src/select_modify/exact.rs`: `GestureOrigin` 20.
  - `crates/vizdaw-app/src/document/editing.rs`: `show_detail` 207, `close_detail` 233, and the detail-opening paths.
- Test: `crates/vizdaw-app/tests/strip_chart_model.rs` (more tests), `crates/vizdaw-app/tests/strip_chart_frame.rs`

**Interfaces:**
- Consumes: Tasks 1–3.
- Produces (ports `M/StripChartModel.swift:62-80,333-394` and `E/StripChartView.swift`'s geometry):
  - **Origin:** `GestureOrigin::StripChart`. Until Task 7, `exact_request` returns `None` for it, so its strokes apply as usual.
  - **The stroke:** `pub struct StripStroke { pub row: usize, pub lane: StripLane, pub start: StripPoint, pub current: StripPoint, pub points: Vec<StripPoint>, pub mode: StripMode, pub compress: bool, pub selected_only: bool, pub seed: u64, pub open: bool }`.
  - **On `DocumentModel`:**
    - `strip_stroke(slot) -> Option<&StripStroke>`.
    - `strip_stroke_began(slot, row: usize, p: StripPoint, selected_only: bool)`: a stroke still held is cancelled first.
    - `strip_stroke_moved(slot, p)`: opens the gesture on the first move, then calls `update_gesture`.
    - `strip_stroke_ended(slot, p)`: a click in Draw or Erase is a one-tick gesture. In Modify it does nothing.
    - `strip_stroke_cancelled(slot)`.
    - `set_strip_seed_for_test(slot, u64)`.
  - **Opening a stroke** follows `M/StripChartModel.swift:362-393`:
    - Erase on a row whose `erase_rule` is `None` sets the status `ERASE_NOTES` = "Erase works on controller, program and automation rows." (`:84`) and opens nothing.
    - Modify keeps the selection's tracks when there are any. Otherwise the stroke works on the shown track.
    - The label is "Erase Events" or `stroke_label`.
    - It calls `begin_gesture_from(label, lanes, GestureOrigin::StripChart)`. A refusal while another gesture is open sets `GESTURE_OPEN`.
    - Only a stroke that opened moves the selection to its track.
  - **Dropping a stroke:** whenever the pane's detail stops being a Strip Chart (another kind, or closed), the pane's stroke is cancelled. Do this in `show_detail`, `close_detail` and the `open_detail` paths.
  - **Geometry** (`frame.rs`; content points are below the ruler):
    - `pub const TITLE_HEIGHT: f32 = 18.` and `pub const TITLE_WIDTH: f32 = 200.`
    - `pub struct StripFrame { pub axis: TimeAxis, pub row_height: f32, pub offset: f32 }`, with:
      - `row_top(i) -> f32`
      - `row_at(y, rows: usize) -> Option<usize>`
      - `in_title(x, y, i) -> bool`
      - `scale(i, lane) -> StripScale`
      - `point(x, y, i, lane, &StripChartSettings, &Snap, meter) -> StripPoint`: snapped when snap is on, at least 0, and in Modify other than Compress the operation's amount
      - `lane_point(x, y) -> (i64, f32)` and `content_point((i64, f32)) -> (f32, f32)`: the preview path
      - `content_height(rows) -> f32`
- [ ] **Step 1: Write the failing tests.** Port the stroke tests of `T/StripChartModelTests.swift`, where the fake's calls are `begin_edit(‹label›)`, `end_edit` and `abort_edit`:
  - `a_draw_drag_is_one_gesture_on_the_shown_lane`
  - `a_click_in_draw_mode_draws_one_point`
  - `a_modify_line_keeps_the_selected_lanes_and_option_refines`
  - `a_refused_stroke_leaves_the_selection_alone`
  - `a_new_stroke_cancels_one_whose_mouse_up_was_lost` (Review Focus 1)
  - `escape_leaves_no_undo_step`
  - `erase_on_a_note_row_says_why`

  New: `a_chart_that_stops_showing_drops_its_stroke` (Review Focus 1). Open a stroke, then `show_detail(List)`: the last call is `abort_edit`, there is no gesture and the stroke is `None`. Do the same with `close_detail`.

  In `strip_chart_frame.rs`, port the geometry of `T/StripChartViewTests.swift` at the default zoom (160 points a quarter) with 96-point rows:
  - `rows_map_points`: with two rows, `row_at(10) == 0`, `row_at(100) == 1` and `row_at(250) == None`. `point(170, 3)` on row 0 is `(q, 0xFFFF)`.
  - `points_follow_the_scrolled_offsets_and_clamp_at_the_edges` (Review Focus 5).
  - `ticks_left_of_the_origin_clamp_to_zero`.
  - `modify_mode_reads_the_operations_amount`: Scale at y 3 is 200, at y 93 is 0, and x 220…330 snaps to q…2q.
  - `the_stroke_preview_stays_on_its_values_when_the_view_scrolls`: `content_point(lane_point(p))` moves by −160 and −10 when the origin moves 160 points and the offset moves 10.
- [ ] **Step 2: Run** `cargo test -p vizdaw-app --test strip_chart_model --test strip_chart_frame`. Expected: FAIL.
- [ ] **Step 3: Implement.** Use `stroke_line`, `line_command`, `compress_command` and `erase_command` from Task 2. Don't repeat them.
- [ ] **Step 4: Run** the tests, `cargo test -p vizdaw-app --test exact_mode`, and `cargo test -p vizdaw-app`. Expected: PASS.
- [ ] **Step 5: Commit** `strip_chart: strokes as one undo step, and the canvas geometry`.

### Task 5: The chart in the panes, its bar, ⌘3 and the cascading menu

**Files:**
- Create:
  - `crates/vizdaw-gpui/src/strip_chart.rs`: the view state, the bar and `render`. Task 6 adds the canvas.
  - `crates/vizdaw-gpui/src/cascade_menu.rs`.
- Modify:
  - `crates/vizdaw-gpui/src/detail_pane.rs`: the `editor` match at 117-142.
  - `crates/vizdaw-gpui/src/panes.rs`: `KINDS` at 40, and `content` 781 for `PaneView::Controllers`.
  - `crates/vizdaw-gpui/src/shell.rs`: two `StripChart` fields on `DocumentWindow`, constructed and prepared like `piano_roll`.
  - `crates/vizdaw-gpui/src/menus.rs`: `ROUTED_EDITOR_COMMANDS` 186, and the Editors menu at 700.
  - `crates/vizdaw-gpui/src/{control.rs,lib.rs}`.
  - `crates/vizdaw-app/src/commands.rs`: `EDITOR_COMMANDS` 43, and `run_editor` 783.
  - `crates/vizdaw-app/src/editor/detail.rs`: `is_available` 32.
- Test: `crates/vizdaw-gpui/tests/strip_chart.rs` (new); update `crates/vizdaw-gpui/tests/piano_roll.rs:1342` and `crates/vizdaw-app/tests/detail.rs`

**Interfaces:**
- Consumes: Tasks 2–4.
- Produces:
  - **Command:** `editors.strip_chart` joins `EDITOR_COMMANDS` and `ROUTED_EDITOR_COMMANDS`, and runs `show_detail(StripChart)`. The Editors menu lists it after "List", with its key from the registry. `is_available` is true for `StripChart`. The bottom pane's picker offers "Strip Chart". `control.rs` no longer answers `not_yet` for it, and `editors.strip_chart_window` still does.
  - **Cascading menu:** `cascade_menu.rs`:
    - `pub(crate) enum CascadeEntry { Item { title: String, checked: bool, enabled: bool, pick: Pick }, Separator, Submenu { title: String, enabled: bool, help: Option<String>, items: Vec<CascadeEntry> } }`, where `Pick = Rc<dyn Fn(&mut Window, &mut App)>`.
    - `pub(crate) fn cascade(id: &'static str, entries: Vec<CascadeEntry>, open: &[usize], at: Option<Point<Pixels>>, on_open: Rc<dyn Fn(Vec<usize>, &mut Window, &mut App)>, on_close: Rc<dyn Fn(&mut Window, &mut App)>, scope: &mut RegistryScope) -> Vec<AnyElement>`:
      - Submenus nest to any depth. `open` is the path of open rows, and hovering or clicking a submenu row opens it.
      - In the tree, items are `menuitem` and submenus are `menuitem` with a ▸.
      - A disabled submenu shows its `help`.
      - It looks like `mixer_view.rs:3844-3930`.
  - **The view:** `pub struct StripChart { slot: ChartSlot, focus: FocusHandle, axis: TimeAxis, offset: f32, layout: Rc<Cell<Bounds<Pixels>>>, mounted: bool, shown_target: Option<EditorTarget>, menu: Option<ChartMenu>, menu_path: Vec<usize>, … }`.
    - `StripChart::new(slot, cx)`.
    - `prepare(&mut self, d: &DocumentModel) -> bool`: whether it shows. It takes the saved zoom and, at the first layout for a block target, scrolls to the block's start (`E/StripChartView.swift:111-119`). Leaving the pane sets a detach the window acts on in Task 6.
    - `pub struct StripChartState` with `StripChartState::of(doc, slot, transport: Option<&TransportModel>, now) -> Option<Self>`.
    - `pub(crate) fn render(chart, state, scope, window, cx) -> AnyElement`.
  - **The bar** (`S/StripChartPane.swift:50-176`, widths and a11y verbatim):
    - "Mode": three segmented `bar_button`s "Draw", "Modify" and "Erase" (190 in all).
    - "Operation" (120, Modify only) offers all nine `StripOp`s, as Swift's bar did.
    - Unless the operation draws no shape, "Shape" (110). The shapeless cases are Erase, and Modify with Compress, Thin, Thicken or Legato. Then:
      - "Curve" (100) for Curved: `LineCurve::ALL`.
      - For Oscillator:
        - "Wave" (90).
        - "Period Kind" (150).
        - "Period" (140 Musical, 120 Clock or Bars), from `StripBarChoices`.
        - "Phase" (100).
    - "Step" (110), with help "Distance between drawn points; the spacing of Thin and Thicken".
    - For a sequence target, the "Track" menu (150): its face "Track: ‹name›" ("—" with none) and an item per track (Ruling 13).
    - A spacer, then the text "Rows: ‹n›" with the stepper buttons "Decrease Rows" (removes the last row) and "Increase Rows" (`add_strip_row`). They are disabled at 1 and at `MAX_ROWS`.
    - Pickers are `menu` in the tree, their value the current title. They open `bar_menu` lists (`list_editor.rs:688`).
    - Every change goes through `update_strip_settings`, `set_strip_chosen_lane` or the row calls.
  - **The panes:**
    - `detail_pane::render` hosts the chart for `DetailEditorKind::StripChart`.
    - The right pane's Controllers view shows the Controllers chart while `strip_state(Controllers)` is `Some`. Otherwise it shows the note "Select a track to see its controllers." (`S/PaneViews.swift:223-235`).
- [ ] **Step 1: Write the failing tests.** GPUI tests in `tests/strip_chart.rs`, with `roll_fake()` and a 360-point bottom pane, as `tests/piano_roll.rs` sets up:
  - `editors_strip_chart_opens_the_chart`. After `secondary-3`, `detail_kind_name()` is "strip_chart" and the tree has the `group` "Strip Chart" and the `text` "Rows: 1". "Shape" has the value "Straight", "Step" has "1/8", and `button` "Draw" is selected.
  - `the_bar_shows_only_the_pickers_its_mode_uses`: Erase hides "Shape", Modify shows "Operation", Modify with Compress hides "Shape", Curved shows "Curve", and Oscillator shows "Wave", "Period Kind", "Period" and "Phase".
  - `bar_changes_are_saved_with_the_project`: after choosing Modify, `saved_layout(..)["stripChart"]["mode"] == "modify"`.
  - `a_sequence_target_shows_the_track_menu`: picking "Bass" makes `strip_lane(Pane) == Some((3, 32))`.
  - `the_rows_stepper_adds_and_removes_rows`.
  - `the_controllers_pane_shows_the_focused_tracks_chart`: with nothing focused it shows the note, and with Chords selected the Controllers `group` "Strip Chart" shows.
  - `a_saved_strip_chart_last_editor_opens_the_chart`: `{"lastKind":"stripChart"}` and a double-clicked block give "strip_chart".

  Change `an_unported_last_editor_double_click_opens_the_roll` (`tests/piano_roll.rs:1342`) to loop over `["drumGrid"]` only. Change `tests/detail.rs` so that `is_available` covers the Strip Chart.

  Add a `cascade_menu` unit test, `a_nested_submenu_opens_along_its_path`: a three-deep path shows the leaf's `menuitem`, and picking it calls `pick` and then `on_close`.
- [ ] **Step 2: Run** `cargo test -p vizdaw-gpui --test strip_chart`. Expected: FAIL.
- [ ] **Step 3: Implement.** Copy the bar and pane plumbing from `piano_roll.rs:137-560`.
- [ ] **Step 4: Run** `cargo test -p vizdaw-gpui` and `cargo test -p vizdaw-app`. Expected: PASS.
- [ ] **Step 5: Commit** `strip_chart: the chart in the bottom and right panes, its bar and ⌘3`.

### Task 6: The rows canvas, strokes, the row menu and the tree

**Files:**
- Modify: `crates/vizdaw-gpui/src/strip_chart.rs`, and `crates/vizdaw-gpui/src/shell.rs` (detach and focus, as `roll_detach` and `sync_roll_focus` do)
- Test: `crates/vizdaw-gpui/tests/strip_chart.rs`

**Interfaces:**
- Consumes: Tasks 3–5. The row menu uses `cascade_menu`. MIDI Learn uses `learn.rs:312` `learn_target`.
- Produces (ports `E/StripChartView.swift`):
  - **The layout:**
    - A 22-point ruler (`RULER_HEIGHT`) sits over the rows.
    - A ruler click locates, snapped. A ruler drag past `DEAD_ZONE` selects the range over the shown track (`select_range` with the track).
    - The rows are stacked at `row_height`.
    - The wheel scrolls in time and vertically, clamped to `content_height`.
    - The secondary-wheel and a pinch zoom about the pointer and save `strip_chart.ticks_per_point`.
    - While the chart's sequence plays and the layout follows, the view pages with the playhead, as `PianoRoll::follow` does. The chart redraws every frame while its sequence plays, so the write preview moves (Ruling 10).
  - **Drawing,** per row (`:200-310`):
    - The separator at the row's bottom, and the centre line (0.6 alpha) when `center()` is `Some`.
    - Values, culled to the view plus one value on each side:
      - Bars: 5 wide from the value down to the range's bottom, at 0.45 alpha, the selected ones in the selection colour.
      - Steps and ramps: a 1.5-point line, held to the next point unless `ramp`, and 4-point dots.
    - The preview: a 1.5-point line in the arm colour.
    - The title "‹title› ▾" at (6, top + 3) in the secondary text colour.
  - **The overlay:** a Compress rectangle (0.12 fill and a stroke), else a line from the first point to the last, or the whole path for Free. It is drawn from the lane-space path, so it stays on its values while the view scrolls.
  - **The pointer:**
    - A press in a row's title area opens that row's menu at the pointer.
    - Otherwise a press calls `strip_stroke_began(slot, row, frame.point(..), modifiers.alt)`.
    - Moves past `DEAD_ZONE` call `strip_stroke_moved`, and the release calls `strip_stroke_ended`.
    - Escape calls `strip_stroke_cancelled`.
    - Each of these does the same and clears the path: the window losing the keyboard, the chart leaving its pane, and its document closing.
  - **The row menu** (`:313-381`):
    - "Velocity", "Release Velocity", "Length", then a separator.
    - "Controller" ▸:
      - The eight named controllers and a separator.
      - "0–31", "32–63", "64–95", "96–127" ▸, each "Controller ‹n›".
    - "Pitch Bend", "Channel Pressure".
    - "Poly Pressure" ▸: 11 octaves titled `PitchName` "‹lo›–‹hi›" ▸, each "Poly Pressure ‹pitch›".
    - "Program", "Tempo", then a separator.
    - "Automation" ▸: the target groups ▸ their targets.
      - It is disabled when there are none, with the help "The mixer's targets could not be read just now (busy); open the menu again."
    - "MIDI Learn", for an automation row except a velocity scale.
    - A separator.
    - "Add Row" (enabled below `MAX_ROWS`) and "Remove Row" (enabled above 1).
    - The current lane is checked.
  - **The tree:**
    - The canvas is the `group` "Strip Chart". It holds a `button` per value in view, at most 200 per row, labelled "‹row title› ‹text›, bar ‹b› beat ‹beat›" and selected when its event is selected.
    - Each row title is a `menu` "Row ‹i+1›", its value the row's title (Ruling 6).
- [ ] **Step 1: Write the failing GPUI tests.** Port `T/StripChartViewTests.swift`. Points are canvas points below the ruler, and `q = 15_360`.
  - `a_drag_in_draw_mode_sets_velocities_along_the_line`. Drag from (0, 30) to (160, 48). Then:
    - `gesture_edits().last() == line_command(Velocity, Set, LineRow{0, q, s.value_at(30), s.value_at(48), q/8, Straight}, false)`, where `s` is the row-0 scale.
    - The calls start with `begin_edit(Change Velocity)` and end with `end_edit`.
  - `a_click_in_draw_mode_draws_one_point`: (165, 48) gives `from == to == q`.
  - `escape_leaves_no_undo_step`.
  - `modify_mode_reads_the_operations_amount`: with Scale and Alt, a drag from (220, 3) to (330, 93) gives op Scale, mode Refine, start 200, end 0, from q and to 2q.
  - `an_erase_drag_deletes_the_rows_series`: Controller 7, (220, 30) to (320, 40), gives `erase_command(Controller(7), q, 2q)` and `begin_edit(Erase Events)`.
  - `escape_before_the_dead_zone_sends_nothing` (Review Focus 1).
  - `leaving_the_pane_mid_stroke_leaves_no_undo_step` (Review Focus 1): `show_detail(List)` mid-drag makes the last call `abort_edit`.
  - `the_tree_reads_each_value`: "Velocity 64, bar 1 beat 1" and "Velocity 64, bar 1 beat 2". Use the fake's velocity if `note_row` differs, and say so in the test.
  - `the_tree_reads_the_values_on_screen`: at origin 2q there are none, and at q/2 only beat 2.
  - `the_row_menu_lists_every_kind`: pressing `menu` "Row 1" lists "Velocity", "Release Velocity", "Length", "Controller", "Pitch Bend", "Channel Pressure", "Poly Pressure", "Program", "Tempo", "Automation", "Add Row" and "Remove Row". Picking "Tempo" makes `strip_rows(Pane) == [Tempo]`.
  - `an_empty_automation_menu_says_why` (Review Focus 4).
  - `a_stroke_loads_the_automation_targets_once` (Review Focus 4): across a 20-move drag with `run_until_parked` after each move, `mixer_reads()` grows by at most 1.

  New tests:
  - `midi_learn_on_an_automation_row`: with `mixer_fake()`, the row set to `Automation(StripFader{1})`, then "MIDI Learn", gives `learn_targets() == [StripFader{strip:1}]`.
  - `a_controllers_chart_stroke_draws_on_the_focused_track`: with Chords selected and the right pane on Controllers, a drag in its chart sends a Velocity line whose gesture lanes are `{(3, 31)}`, and it writes nothing to the bottom pane's detail.
- [ ] **Step 2: Run** `cargo test -p vizdaw-gpui --test strip_chart`. Expected: FAIL.
- [ ] **Step 3: Implement.** Copy the canvas, ruler, wheel and detach plumbing from `piano_roll.rs:794-1200`. All the geometry is Task 4's `StripFrame`.
- [ ] **Step 4: Run** `cargo test -p vizdaw-gpui`. Expected: PASS.
- [ ] **Step 5: Commit** `strip_chart: the rows canvas, strokes, the row menu and the tree`.

### Task 7: The Modify panel's Strip Chart Line, Automation Target and Exact Mode origin

**Files:**
- Modify:
  - `crates/vizdaw-app/src/select_modify/fields.rs`: `modify_fields` 322-349, `modify_numbers` 392, `modify_with` 500.
  - `crates/vizdaw-app/src/select_modify/mod.rs`: `OFFERED` 271 and 502.
  - `crates/vizdaw-app/src/select_modify/model.rs`: the setters.
  - `crates/vizdaw-app/src/select_modify/exact.rs`.
- Test: `crates/vizdaw-app/tests/line_panel.rs` (new); additions to `tests/exact_mode.rs`, `tests/strip_chart_model.rs` and `tests/strip_lane.rs`; update `tests/select_modify.rs` and `tests/select_modify_fields.rs`

**Interfaces:**
- Consumes: Tasks 1–4.
- Produces:
  - **Fields** (`M/PanelFields.swift:183-260`):
    - `pub fn line_unit(t: &LineTargetRow, op: LineOpRow) -> FieldKind`:
      - Scale is Percent.
      - Tempo is Bpm.
      - Velocities are `Velocity(Delta)` for Add, else `Velocity(Exact)`.
      - Duration is Length.
      - Anything else is `Value(Delta)` or `Value(Exact)`.
    - Line fields are "from" and "to" (Position), "start" and "end" (the unit) and "step" (Length). An oscillator adds "period" (Length), "period (ms)" (`Bounded(1, 600_000)`) or "period (bars)" (`Bounded(1, 1_000)`), then "phase (‰)" (`Bounded(0, 999)`).
    - Numbers and `with` follow `lineNumbers`/`lineWith`:
      - "to" is at least "from", and "step" at least 1.
      - The phase is clamped to 0–999.
      - A Free line's points move with its range through `pub fn remap(t, from: (i64,i64), to: (i64,i64)) -> i64`.
  - **Offered:** `ModifyKind::OFFERED` becomes all 16 kinds and `ConditionKind::OFFERED` all 13, as Swift's `allCases` (Ruling 8).
  - **Line targets:** `pub const LINE_TARGET_BASICS: [StripLane; 7]` (Velocity, ReleaseVelocity, Duration, PitchBend, ChannelPressure, Program, Tempo), `pub fn line_target_controllers() -> Vec<StripLane>` (0–127), and `pub fn line_target_octaves() -> Vec<(String, Vec<StripLane>)>` (11 octaves, 128 lanes in all, titled as in the row menu).
  - **Line operations:** `pub const LINE_OPS: [(&str, LineOpRow); 10]` ("Draw", "Set", "Scale", "Add", "Max Limit", "Min Limit", "Compress", "Thin", "Thicken", "Legato", `S/LineModifyEditor.swift:13-16`).
  - **Setters** on `DocumentModel` (`M/SelectModifyModel.swift:459-528`). Each clears the drafts and the invalid fields, as Swift does:
    - `set_modify_line_target(lane: StripLane)`: Legato is kept only for Duration, else it becomes Set.
    - `set_modify_line_op(op: LineOpRow)`: the same Legato rule.
    - `set_modify_line_shape(k: ShapeKind)`:
      - Free makes a two-point line from start to end, and a Free line keeps its points.
      - Other shapes take `strip_settings()` with `shape = k`, through `shape_row(next_seed())`.
    - `set_modify_line_curve(c: CurveRow)` and `set_modify_line_wave(w: WaveRow)`.
    - `set_modify_line_period_kind(k: PeriodKind)`: the period from `strip_settings().period_row()` with that kind.
    - `set_modify_condition_target(i: usize, t: AutomationTargetRow)`.

    The names and menus for the pickers are `automation_targets()` (Task 3).
  - **Exact Mode:** `exact_request(cmd, lanes, GestureOrigin::StripChart)` is `line_request` (`M/ExactMode.swift:107-120`):
    - It needs `SelectAndModify{modify: Some(Line{..})}`.
    - The config is `preset(Change)`, with `select.enabled = rule.is_some()`, the stroke's mode and conditions, the line, and `to_copy` false.
    - `events: None` and `range: Some((line.from, after(line.to)))`.
    - Any other command is `None`.
- [ ] **Step 1: Write the failing tests.** Port `T/LinePanelTests.swift` by name into `tests/line_panel.rs`. Set it up with `mixer_fake()`, `open_modify(Change)` and `set_modify_kind(Line)`, with seed 9.
  - `line_pickers_edit_the_line`: after `Tempo`, `modify_fields(..)[2].kind == FieldKind::Bpm`, and the last two oscillator labels are `["period (bars)", "phase (‰)"]`.
  - `automation_conditions_pick_their_target`: "Mute · Synth", and the first menu is "Synth".
  - `shape_defaults_come_from_the_strip_chart_bar`
  - `editing_a_free_lines_range_moves_its_points`: `[0, q, 4q]` over 0…4q becomes `[4q, 6q, 12q]` over 4q…12q.
  - `every_automation_target_can_be_a_condition_and_poly_pressure_a_line`
  - `applying_a_line_sends_one_select_and_modify`: `to_copy` is false even when it was set.
  - `the_kind_condition_can_pick_automation`

  Port `exact_mode_keeps_the_option_refine` and `exact_mode_opens_the_line_in_the_panel` from `T/StripChartTests.swift` into `tests/exact_mode.rs`. New: `a_line_request_at_the_last_tick_does_not_overflow`: the range is `(0, i64::MAX)`.

  Port `exact_mode_opens_the_panel_instead_of_applying` from `T/StripChartModelTests.swift` into `tests/strip_chart_model.rs`. The last call is `abort_edit`, `take_modify_present()` is true, the task is Change, and the line has target `Series{Controller{7}}` and op Draw.

  New: `escape_during_an_exact_strip_stroke_opens_nothing`.

  Update:
  - `the_line_and_automation_kinds_are_not_offered` becomes `the_line_and_automation_kinds_are_offered`.
  - `fields_and_numbers_agree` and `kinds_round_trip_through_their_defaults` now include `Line`.
- [ ] **Step 2: Run** `cargo test -p vizdaw-app --test line_panel --test exact_mode --test strip_chart_model`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p vizdaw-app`. Expected: PASS.
- [ ] **Step 5: Commit** `modify: Strip Chart Line, Automation Target and the Strip Chart's exact mode`.

### Task 8: The panel's line editor and automation target picker

**Files:**
- Modify:
  - `crates/vizdaw-gpui/src/modify_panel/mod.rs`: `PanelMenu` 94, and the cascade's open path.
  - `crates/vizdaw-gpui/src/modify_panel/modify_half.rs`
  - `crates/vizdaw-gpui/src/modify_panel/select_half.rs`: `condition_row` 205-300.
- Test: GPUI tests in those files; replace `the_operation_picker_offers_no_strip_chart_line` (`modify_half.rs:414`) and `add_condition_offers_no_automation_target_and_remove_drops_the_row` (`select_half.rs:335`)

**Interfaces:**
- Consumes: Task 7's setters and lists, Task 5's `cascade_menu`, and Task 3's `automation_targets()`.
- Produces (`S/LineModifyEditor.swift`, `S/ModifyPanel.swift:155-200,385-386`; labels and a11y verbatim):
  - **The line editor,** for `ModifyRow::Line`:
    - The "Line Target" cascade (210). Its face is `StripLane::from_line_target(t).title(names)`, else "Any Value". It lists `LINE_TARGET_BASICS`, then "Controller" ▸ (128), "Poly Pressure" ▸ octaves ▸, and "Automation" ▸ groups ▸.
    - The "Operation" picker (110, `LINE_OPS`). Its id is its own, separate from the half's Operation picker (10b Outcome 2).
    - The "Shape" picker (110, `ShapeKind::ALL`).
    - For Curved, "Curve" (100, `LineCurve::ALL`).
    - For Oscillator, "Wave" (90) and "Period Kind" (150).
    - For Free, the text "Drawn line (‹n› points)".
    - The numeric fields come from `modify_fields`, as for every kind.
  - **The Automation Target condition row:**
    - Its kind title.
    - The "Comparison" picker (Is / Is Not), now shown for this kind too, as in Swift.
    - The "Automation Target" cascade (220). Its face is `names.title(t)`, and it lists groups ▸ each group's targets.
    - The remove button.
  - `PanelMenu` gains `LineTarget`, `LineOp`, `LineShape`, `LineCurve`, `LineWave`, `LinePeriodKind` and `AutomationTarget(usize)`.
- [ ] **Step 1: Write the failing GPUI tests:**
  - `the_operation_picker_offers_strip_chart_line_and_its_editor_edits_the_line`: picking "Strip Chart Line", then Line Target "Tempo", then Shape "Oscillator", shows the fields "period" and "phase (‰)" and the pickers "Wave" and "Period Kind".
  - `the_line_target_menu_reaches_a_poly_pressure_pitch_and_an_automation_target`: Poly Pressure ▸ "C4–B4" ▸ "Poly Pressure C4", then Automation ▸ "Synth" ▸ "Mute · Synth".
  - `a_free_line_shows_its_point_count`: "Drawn line (2 points)".
  - `add_condition_offers_automation_target_and_its_picker_sets_the_target`: its value becomes "Fader · Synth". "Remove Condition" still drops the row.
  - `apply_sends_the_line_from_the_panel`: one `SelectAndModify{modify: Some(Line{..}), to_copy: false}` in `edits()`.
- [ ] **Step 2: Run** `cargo test -p vizdaw-gpui modify_panel`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p vizdaw-gpui`. Expected: PASS.
- [ ] **Step 5: Commit** `modify: the panel's Strip Chart Line editor and Automation Target picker`.

### Task 9: The e2e and the docs

**Files:**
- Modify:
  - `crates/vizctl/tests/e2e_gpui.rs`: reuse `temp_home` 134, `start_demo` 480, `window_numbers` 227, `wait_for_undo` 919, `find_role` 4427, and the 10b panel helpers near 5924.
  - `docs/superpowers/specs/2026-11-22-parity-checklist.md`
  - `docs/testing/manual-checks.md`
  - `docs/feature-backlog.md`

- [ ] **Step 1: Write the e2e tests.** Each is `#[ignore = "needs a window server and VIZDAW_E2E_APP"]` with a temp home.
  - `gpui_strip_chart_opens_draws_rows_and_saves_them`:
    1. Select the Chords block, then `command.run editors.strip_chart`. `state.get editor` gives `detail_kind: "strip_chart"`.
    2. `ui.tree` has the `group` "Strip Chart", with at least one `button` whose label starts "Velocity ", and the `text` "Rows: 1".
    3. Click "Increase Rows". The text is "Rows: 2", and `menu` "Row 2" has the value "Controller 7 (Volume)".
    4. Click `menu` "Row 2", then `menuitem` "Pitch Bend". Its value is "Pitch Bend".
    5. Click `button` "Modify". `menu` "Operation" shows.
    6. Save As into the temp home, close, and reopen. The chart shows "Rows: 2", Row 2 "Pitch Bend" and "Modify" selected.
  - `gpui_modify_strip_chart_line_sets_velocities` (no drag, Ruling 9):
    1. Select the Chords block, then `command.run modify.change`.
    2. In the panel, click the half's `menu` "Operation", then `menuitem` "Strip Chart Line". `menu` "Line Target" has the value "Velocity".
    3. `ui.key` "100" into the textfield "start" and "100" into "end", then click "Apply". `wait_for_undo(.., "Change Velocity")`.
    4. `command.run editors.strip_chart`. Every Chords value in bar 1 (the default line runs 0 to 4q) reads "Velocity 100, bar 1 …".
    5. `command.run modify.again` succeeds.
    6. Add Condition › "Automation Target" adds a `menu` "Automation Target".
- [ ] **Step 2: Run** each on the Mac (the Global Constraints command). Expected: PASS. When a step fails for an app reason, fix the app with a test that fails first, not the e2e.
- [ ] **Step 3: Docs.**
  - **Checklist** (Mac cells only; the Linux cells are the controller's post-CI tick):
    - Row 74, `editors.strip_chart`: "S10c (`gpui_strip_chart_opens_draws_rows_and_saves_them`)".
    - Row 505: "S10c (Strip Chart, Controllers pane, automation rows in the list, the panel's Strip Chart Line and Automation Target; torn-off windows: later slice; on screen: M28)".
    - Amend row 504's Mac cell to "S4 (List editor), S10b (Select and Modify), S10c (Strip Chart Line and Automation Target: `gpui_modify_strip_chart_line_sets_velocities`)".
    - Leave row 79 (`editors.strip_chart_window`), row 236 (`automation.write`, Ruling 1) and row 385 blank.
  - **Manual checks:** the next free numbers are M28 and L34.
    - M28 (Mac): README 68-80 without the windows half of 80. That covers:
      - ⌘3, the rows and Rows +
      - a Draw drag, a right-to-left drag and a single click
      - each shape, Step 1/64 against 1/8
      - Modify Scale, Add, Max Limit, Min Limit, Compress, Legato, Thin and Thicken, and ⌥ at the press
      - Erase, and its note-row message
      - the Auto track's fader fade on playback, and the List's two Automation rows at "0.0%" and about "63%"
      - mute, pan and sends; a plug-in parameter; velocity scale and a tempo ramp
      - the panel's Strip Chart Line and Automation Target; Exact Mode from a chart drag
      - the tree's "Velocity 56, bar 1 beat 1" labels
      - the right pane's Controllers view
    - L34 (Linux, XWayland):
      - The chart draws with the system font.
      - Ctrl+3 opens it.
      - An Alt-press drag limits the change to the selected notes. Note whether the window manager takes Alt-drag to move the window (Ruling 11).
      - The row menu's nested submenus open on hover and stay on screen at the right edge.
  - **Backlog:**
    - Close row 355 ("Strip Chart Line and the Automation Target condition in the Modify panel") and row 346 ("MIDI Learn in the Strip Chart rows") as done in S10c.
    - New row: move `mixer_view.rs`'s strip menus onto `cascade_menu` (Ruling 7).
    - New row: the Strip Chart in torn-off windows (`editors.strip_chart_window`, and Swift's `aTornWindowHasItsOwnStripChart`), with the other torn-off editors.
    - Add a "GPUI slice 10c: review leftovers" row for the minors found in review.
- [ ] **Step 4: Commit** the e2e with `e2e: the Strip Chart and the panel's Strip Chart Line`. Commit the docs with `docs: slice 10c checklist, manual checks and backlog`.

## Rulings made while planning

1. **`automation.write` is not in this slice.** In Swift it is `CreativeCommands.writeAutomation` (`M/CreativeCommands.swift:4-20`): plan 8e's Co-Producer creative command, one `runCreative` call. It is not a Strip Chart command, and its checklist row sits under Mixer. Cost if wrong: one control-only command, a small task in any slice.
2. **The right pane's Controllers view is in.** In Swift it is `StripChartModel.controllers` in the same `StripChartEditor` (`S/PaneViews.swift:223-235`, plan 7a Ruling 7). Here it is a second `ChartSlot` on the same model and view. Without it the pane keeps saying "comes in a later version". Cost if wrong: one slot and one test file section.
3. **The laws come from `core_model::automation`, not a Swift port.** Swift copied them because it could not call the core. `vizdaw-app` already depends on `core-model`, and one copy cannot drift. Only the text formatting is ported.
4. **One automation-target cache per document.** Swift kept three: the pane's chart, the controllers chart and the panel. Here both charts, the panel's pickers and `learn::target_title` share `automation_targets()`, keyed on the snapshot and slots versions. Cost if wrong: a stale name until the next version.
5. **`stripChart`, `stripRows`, `stripRowsLane` and `controllers` keep Swift's JSON shapes.** That is the convention `layout.rs` already follows for every other key. It needs no compatibility work, and a `ui.json` stays readable by eye against the Swift sources.
6. **A row's title is a `menu` "Row ‹n›" in the tree.** Swift exposed no element for it; a press on the drawn title opened the menu. Without one, ui.click and the GPUI tests could not open the row menu.
7. **`cascade_menu.rs` is new; the mixer keeps its own one-level menu for now.** The row menu and the panel's target pickers need three levels, and `mixer_view.rs`'s `menu_rows` is one level and tied to `MixerView`. Moving the mixer onto it is a backlog row, not this slice's risk.
8. **The panel now offers Strip Chart Line and Automation Target.** 10b left them out only because the editor was missing. Swift's pickers list every kind (`ConditionKind.allCases`, `S/ModifyPanel.swift:146`).
9. **The e2e does not drag.** The control channel has no `ui.drag` (10b Ruling 9). The e2e opens and configures the chart and applies a Strip Chart Line through the panel. The strokes are covered by app tests and by GPUI tests that drive the mouse in the test context (Task 6), and on screen by M28.
10. **Swift's `previewClock` is not ported.** It existed to stop SwiftUI redrawing on every poll. The GPUI chart redraws every frame only while its sequence plays (as the roll does via `RollState::playing`), and that is when the preview moves.
11. **⌥ at a press is Alt off macOS.** This is the existing modifier mapping. Some X11 window managers take Alt-drag to move windows, and L34 checks it. If one does, backlog an alternative modifier rather than change the mapping now.
12. **`ChartSlot` has no window variant.** Torn-off windows come in a later slice, which adds `ChartSlot::Window(id)` beside `Pane` and `Controllers`.
13. **The sequence target's track menu is `menu` "Track" with the face "Track: ‹name›".** Swift's menu had no a11y label beyond its title. A fixed label keeps the tree stable when the track changes.
14. **The bar's Operation picker offers all nine operations, as Swift's bar did.** `StripOp::choices` is used only where Swift used it (tests and the panel). A Legato stroke on a velocity row is the core's to refuse.

## Controller decisions

- Confirm Ruling 1 (`automation.write` waits for the Co-Producer slice) and Ruling 2 (the Controllers pane is in 10c).
- Ruling 11: if the owner's Linux desktop takes Alt-drag, which modifier should "selected only" use there?

## Outcome (2026-11-27)

Nine tasks, 11 commits (8c8f9325..f14168ef), each task reviewed; final whole-branch review: ready to merge, no Critical or Important findings. Controller rulings:

1. Ruling 1 confirmed: `automation.write` waits for the Co-Producer slice. Ruling 2 confirmed: the Controllers view is in 10c.
2. A bad `controllers` entry in `ui.json` is dropped on its own, not the whole map as Swift did (Review Focus 2). An old non-Swift entry is lost on the next save, which the no-compatibility rule allows.
3. A VCA already named "VCA 1" reads "VCA 1 Fader", not Swift's doubled "VCA VCA 1 Fader".
4. A Busy first read of the automation targets keeps the saved automation rows: `StripNames::knows` answers true while no strips are read. Guarded by `a_busy_first_read_keeps_saved_automation_rows_through_an_edit`.
5. Strokes carry the serial of the gesture they opened and touch only that gesture, so a stroke whose gesture was cancelled under it cannot end another editor's.
6. Duplicated helper code, a stale picker index after a row removal, and a destinations read error that drops saved Velocity Scale rows were graded minor (no events touched) and backlogged as "GPUI slice 10c: review leftovers".
7. Modify strokes edit the selection's tracks when there is a selection, as Plan 4d Ruling 22 and Swift do.

