# 27.1.0 Slice 9d: Surfaces and MIDI Learn Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Control surfaces and MIDI learn work in the GPUI app as in the Swift app.
- `surfaces.json` loads at launch.
- Surface events (commands, select, arm, jog, count-in) reach the document whose session took them.
- `surface.*` and `state.get surfaces` work over the control channel.
- Settings › Surfaces lists the units and the learned commands, with the Add Surface sheet and its alerts.
- The mixer's controls and the plug-in window can learn a MIDI control. The learn banner, Esc to cancel, "MIDI Learn Failed" and the MIDI Learn Maps sheet are all there.
- Settings › Keys gets its Learn MIDI button.
- The e2e `drives_a_fader_and_a_button_from_an_mcu` and `drives_the_mixer_from_a_kontrol_mk3` are ported to `e2e_gpui.rs`.

**Architecture:**
- The core is complete (`core-ffi/src/surfaces.rs`, `core-ffi/src/session.rs:12860-13030`, `crates/core-surface`). This slice reaches it through the app's two seams, as 9a–9c did:
  - The process-wide calls (the unit list, ports, command maps, the waiting learn) are new `CoreCalls` methods, recorded by `FakeCore`.
  - The per-document calls (focus, inject, event drain, count-in, the project's learn maps) are new `DocSession` methods, recorded by `FakeSession`.
- `vizdaw-app` gets four modules:
  - `surface_settings.rs` ports `M/SurfaceSettings.swift`.
  - `surface_commands.rs` ports `M/SurfaceCommands.swift`.
  - `surface_relay.rs` ports `M/SurfaceRelay.swift`. It turns drained events into ordered steps. The document applies select, arm, count-in and locate itself; the GPUI window runs the command steps.
  - `learn.rs` ports `M/LearnModel.swift`. It is one per document, as `DocumentModel.learn`.
- `DocumentModel::idle` drains the events and tells the core the count-in, as Swift's `DocumentModel.idle` did (`:856-870`). `DocumentStore` gives the surfaces' focus to the front document whenever that changes.
- GPUI gets:
  - `settings_window/surfaces.rs`. The Settings window gains a `SheetHost` for `sheets/add_surface.rs`.
  - `sheets/learn_maps.rs` on the document and mixer windows' `SheetHost`.
  - `learn_banner.rs`, shared by the mixer view, the plug-in window and the Keys tab.
  - "MIDI Learn" menus on the mixer's fader, pan, mute, solo and sends, plus "MIDI Learn Maps…" in the strip name's menu.
  - Learn MIDI in `settings_window/keys.rs`.

**Tech Stack:** Rust 2024; GPUI at the slice 1 pin; `core-ffi` (`surfaces`, `session`), `core-control`, `vizctl`.

**Spec:** `docs/superpowers/specs/2026-11-20-cross-platform-27-1-design.md`.
- Parity rows in `docs/superpowers/specs/2026-11-22-parity-checklist.md`:
  - "### Surfaces": `surface.add`, `surface.remove`, `surface.inject`, `surface.learn` and `surface.unlearn` (:342-350).
  - "Settings: Surfaces" (:401), and the Learn MIDI note on "Settings: Keys" (:397).
  - The "MIDI Learn Maps" sheet (:433) and the "Add Surface" sheet (:437).
  - "MIDI Learn alert" (:446) and "Surfaces alert" (:447).
  - The README rows "Control surfaces and MIDI learn (plan 5f-1)" (:516) and "Host-integration keyboards (plan 5f-4)" (:519).
  - The e2e rows `drives_a_fader_and_a_button_from_an_mcu` (:553) and `drives_the_mixer_from_a_kontrol_mk3` (:554).
- Swift reference with exact strings and file:line: `.superpowers/sdd/settings-inventory.md`:
  - §1.7 (:132-149) and §2, the Add Surface and Surfaces alert rows (:209-210).
  - §3, the `surface.*` and `state.get surfaces` rows (:238-243), and the e2e rows (:256-257).
  - §4, the Surfaces gap (:282), §5 (`SurfaceSettingsTests`, :303) and §6 8x-4 (:333).
- Swift sources, still in the tree for reading. `S/` = `app/Sources/VizDAW/`, `M/` = `app/Sources/VizDAWAppModel/`.
  - `S/{SurfacesSettingsView,LearnMapsSheet,LiveSurfaceCore,KeysSettingsView,KeyMonitor,MixerView,ChannelStrip,PluginWindows,VizDAWApp,ControlChannel}.swift`
  - `M/{SurfaceSettings,SurfaceRelay,SurfaceCommands,LearnModel,DocumentModel,StripLane,PluginWindowModel}.swift`
  - `app/Tests/VizDAWAppModelTests/{SurfaceSettingsTests,SurfaceCommandsTests,LearnModelTests}.swift`
  - Swift e2e: `crates/vizctl/tests/e2e.rs:1653-1795` (MCU) and `:1797-1900` (Kontrol MK3).

## Global Constraints

- **Rules from slices 7a–9c that still apply:**
  - The Swift app is retired: match its behaviour; don't keep compatibility with it.
  - Use Swift's exact strings, citing file:line.
  - The UI says "track", never "lane".
  - No banned vendor or DAW product names in repo text. The keyboard is "Host-integration keyboard (DAW mode)" in the UI (`M/SurfaceSettings.swift:173`). The existing ids `kontrol_mk3` and `kontrol_ports` stay as they are.
  - Nothing slow on the UI thread:
    - The Surfaces tab's 2 Hz reads run off it, using `read_off_thread` (`settings_window/mod.rs:174`).
    - The port lists read CoreMIDI on a live Mac.
    - `surfaces_load` at launch and `surface.add` stay on the UI thread, as in Swift (`S/VizDAWApp.swift:515-522`). See Ruling 3.
  - GPUI tests press `secondary-` keys.
  - Linux CI runs the GPUI tests and the e2e on the offline backend, and every 9d test must pass there.
    - Offline, the core uses its fake port transport on both platforms (`core-ffi/src/surfaces.rs:1668-1687`), so `surface.add` and `surface.inject` behave the same on Linux and macOS.
    - A live Linux build has no MIDI ports until host track H3 (`NoPorts`). Its pickers say "No MIDI sources" and "No MIDI destinations".
    - A Mac-only step is skipped on Linux with a line saying why.
- **Before every commit:** `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and the tests the task names. CI stops at Format.
- **Running the e2e on the Mac:** `cargo build -p vizdaw-gpui`, then `VIZDAW_E2E_APP=target/debug/vizdaw-gpui VIZDAW_AUDIO=offline cargo test -p vizctl --test e2e_gpui -- --ignored <name>`.
- **Preferences:** surfaces and learned commands live in the core's `<support>/surfaces.json` (`core-ffi/src/surfaces.rs:41`), not in `Prefs`. The support folder is `vizdaw_ffi::recovery::support_dir()`, which is under the launch home, so the e2e's throwaway home keeps its own file. Learn maps live in the project.
- **Seams:** no GPUI or app test makes a CoreMIDI client or writes `surfaces.json`. Tests use `FakeCore` and `FakeSession`, as Swift's tests used a fake `SurfaceCore` (`S/LiveSurfaceCore.swift:4-6`).
- **Control replies:** every `surface.*` call answers at once (`Handled::Reply`). No `surface.*` call opens a dialog or an alert (`M/SurfaceCommands.swift:53-58`).
- **The look:**
  - Theme tokens only (`appearance::current()` and `strip_palette::tokens()`). `no_source_file_keeps_a_colour_literal` must stay green.
  - Problem lines use `state.warning`, and the sheet's refusal uses `state.error`.
  - The banner is `accent` at 15% opacity behind `text_primary` (`S/LearnMapsSheet.swift:98`).
  - Sizes (Swift):
    - The Surfaces tab is 560 wide (`S/SurfacesSettingsView.swift:69`).
    - The Add Surface sheet is 420 wide (`:154`).
    - The MIDI Learn Maps sheet is 640 wide (`S/LearnMapsSheet.swift:36`), with a 190-wide mode picker (`:63`). Its list is at least 160 high (`:27`); the empty line, at least 60 (`:20`).
- **Out of scope:** Co-Producer, user themes, the Console look, H3's Linux MIDI ports, and the Strip Chart's "MIDI Learn" (no GPUI Strip Chart yet; backlog).
- **Review pace:** fix crashes, data loss and wrong overwrites; log other edge cases to the backlog.

## Review Focus

1. **A port vanishes while the Surfaces tab is open.** The row reads "Disconnected" at the next poll, nothing panics, and the open Add sheet keeps its picked name. Add then shows the core's refusal in the sheet, and the sheet stays open. Test: `a_port_that_vanishes_reads_disconnected_at_the_next_poll` (Task 7).
2. **A learn starts, then its window closes.** Closing the document whose learn waits calls `cancel_learn`, so no learn lands in a closed session, and the other windows' banners go. Test: `closing_the_learning_document_cancels_the_learn` (Task 4).
3. **A damaged `surfaces.json`.** Launch keeps it aside as `surfaces.json.bad` and starts with no surfaces. `state.get surfaces` and the tab's other-problems line show the core's line, "surfaces.json could not be read and was kept aside as surfaces.json.bad; VizDAW starts with no surfaces: …" (`core-ffi/src/surfaces.rs:1002-1004`). Test: e2e `gpui_a_damaged_surfaces_file_is_kept_aside` (Task 2).
4. **The 2 Hz poll stays off the UI thread.** One read is in flight at a time, a held read starts no second thread, the poll stops when the tab is left or the window closes, and an unchanged read does not redraw. Test: `the_surfaces_poll_reads_off_thread_one_at_a_time` with `ReadHold` (Task 7).
5. **Two documents are open when a surface moves a fader.** The events apply to the document whose session took them, and their commands run in that document's window even when the other is in front. Focus moves only when the front document changes, and is kept only once the core took it. Tests: `events_apply_to_the_session_that_took_them` and `focus_follows_the_front_document_and_retries_after_a_refusal` (Task 3).

---

### Task 1: The surface seams and the Surfaces settings model

**Files:**
- Create: `crates/vizdaw-app/src/surface_settings.rs`
- Modify:
  - `crates/vizdaw-app/src/{session.rs,testing.rs,lib.rs}`
  - `crates/core-ffi/src/surfaces.rs`, to add one `pub fn` only
- Test: `crates/vizdaw-app/tests/surface_settings.rs`

**Interfaces:**
- Produces:
  - In `core-ffi/src/surfaces.rs`, `pub fn learnable(id: &str) -> bool`. It is `bindable(id).is_ok()` (`:1853`): the Keys tab's test of a row, as `BindingRow.learnable` (`core-ffi/src/keys.rs:113`).
  - New `CoreCalls` methods. Each `LiveCore` body is the free function of the same name. `FakeCore` records each call and can script it.
    - `surfaces_load() -> Result<(), VizError>`
    - `surface_list() -> Vec<SurfaceRow>`
    - `surface_ports() -> SurfacePortsRow`
    - `kontrol_ports() -> SurfacePortsRow`
    - `surface_add(kind: SurfaceKindRow, input: &str, output: &str, position: Option<u8>) -> Result<u32, VizError>`
    - `surface_remove(id: u32) -> Result<(), VizError>`
    - `surfaces_state() -> SurfaceStateRow`
    - `command_maps() -> Vec<CommandMapRow>`
    - `remove_command_map(command: &str) -> Result<(), VizError>`
    - `learn_command(id: &str) -> Result<(), VizError>`
    - `cancel_learn()`
    - `learn_state() -> LearnStateRow`
  - What `FakeCore` does by default, as Swift's `FakeSurfaceCore`:
    - The ports are `In A` / `Out A`, and the Kontrol ports are empty.
    - `surface_add` takes the lowest free id. Its row is `connected: true, online: false`, with `position` 0 when `None`.
  - `FakeCore`'s scripts: `set_surface_rows`, `set_surface_ports`, `set_kontrol_ports`, `set_surfaces_state`, `set_learn_state`, `fail_surface_add(VizError)`, `fail_remove_command_map(VizError)`, `fail_learn_command(VizError)` and `hold_surface_reads(Arc<ReadHold>)`.
  - `FakeCore`'s records: `surfaces_added()`, `surfaces_removed()`, `port_reads()`, `learned_commands()`, `learn_cancels()` and `removed_command_maps()`.
  - New `DocSession` methods. Each `Session` body is the core's (`core-ffi/src/session.rs:12865-13026`). `FakeSession` records each and can script errors.
    - `focus_surfaces() -> Result<(), VizError>`
    - `set_surface_count_in(on: bool) -> Result<(), VizError>`
    - `inject_surface(surface: u32, bytes: Vec<u8>) -> Result<(), VizError>`
    - `surface_state() -> Result<SurfaceStateRow, VizError>`
    - `take_surface_events() -> Result<Vec<SurfaceEventRow>, VizError>`
    - `learn_target(target: AutomationTargetRow) -> Result<(), VizError>`
    - `learn_maps() -> Result<Vec<LearnMapRow>, VizError>`
    - `set_learn_mode(source: LearnSourceRow, mode: LearnModeRow) -> Result<DocSnapshot, VizError>`
    - `set_learn_pickup(source: LearnSourceRow, on: bool) -> Result<DocSnapshot, VizError>`
    - `remove_learn(source: LearnSourceRow) -> Result<DocSnapshot, VizError>`
    - `unlearn_target(target: AutomationTargetRow) -> Result<DocSnapshot, VizError>`
  - `FakeSession`'s scripts: `push_surface_events`, `set_learn_maps` and `set_surface_state`.
  - `FakeSession`'s records: `focus_calls()`, `surface_count_ins()`, `surface_injections()`, `learn_targets()`, `unlearn_targets()`, `learn_modes()`, `learn_pickups()` and `removed_learns()`.
  - `pub struct SurfaceDraft { pub kind: SurfaceKindRow, pub input: String, pub output: String, pub position: u8 }`. It defaults to `Mcu`, `""`, `""` and `1` (`M/SurfaceSettings.swift:13`).
  - `pub struct SurfaceRead { rows, ports, command_maps, problems }`, with `pub fn read(core: &dyn CoreCalls) -> SurfaceRead`. These are the four reads of Swift's `refresh` (`:52-61`), taken together so the poll can run them off the UI thread.
  - `pub struct SurfaceSettings`, with:
    - `new()`, which reads nothing (`:43-47`)
    - `refresh(&mut self, core) -> bool` and `apply_read(&mut self, SurfaceRead) -> bool`. Each is true when something changed; unchanged values are not replaced.
    - `rows()`, `ports()`, `command_maps()` and `problems()`
    - `problem_for(id: u32) -> Option<&str>` (`:66-70`) and `other_problems() -> Vec<&str>` (`:73-76`)
    - `draft()`, `draft_problem()` and `draft_note()`
    - `begin_add(core)`, `set_draft_kind(kind, core)`, `set_draft_input(String)`, `set_draft_output(String)`, `set_draft_position(u8)`, `commit_add(core)` and `cancel_add()`
    - `add(kind, input, output, position, core)`, `remove(id, core)` and `remove_command_map(command, core)`
    - `take_alert() -> Option<AlertItem>`
  - Free functions: `kind_title(SurfaceKindRow) -> &'static str` (`:166-175`), `status(&SurfaceRow) -> &'static str` (`:178-181`) and `row_title(&SurfaceRow) -> String`. An XT row's title gains `" · position N"` (`S/SurfacesSettingsView.swift:93-96`).
  - Constants, verbatim:
    - `NO_SURFACES` = "No surfaces. Add an MCU-compatible, MCU XT-compatible or HUI-compatible unit, or a host-integration keyboard." (`S/SurfacesSettingsView.swift:20`)
    - `NO_COMMAND_MAPS` = "None. Learn MIDI for a command in Settings › Keys." (`:54`)
    - `NO_SOURCES` = "No MIDI sources" (`:116`)
    - `NO_DESTINATIONS` = "No MIDI destinations" (`:120`)
    - `XT_HELP` = "Units are banked left to right by position; the MCU-compatible main unit takes a free one." (`:136`)
    - `NO_KONTROL` and `KONTROL_HELP`, from `M/SurfaceSettings.swift:40-41`
    - The alert titles "Add Surface Failed", "Remove Surface Failed" and "Remove MIDI Learn Failed" (`:103,115,123,133`)

- [ ] **Step 1: Write the failing tests.** Port `SurfaceSettingsTests.swift` by name:
  - `add_list_and_remove` (:14): `status` is "Connected · not answering", and `kind_title(McuXt)` is "MCU XT-compatible".
  - `a_refused_add_is_an_alert` (:25): the title is "Add Surface Failed".
  - `a_refused_add_keeps_the_sheet` (:270): the draft stays, `draft_problem` is the core's message, and there is no alert.
  - `a_kontrol_draft_picks_the_keyboards_daw_ports` (:285): the unit is added with position 0.
  - `a_kontrol_draft_without_a_keyboard_says_so` (:301): the note is `NO_KONTROL`. Choosing HUI clears it. With no sheet open, `set_draft_kind` does nothing.
  - `a_kontrol_with_old_firmware_says_why` (:318): this pins the last-line-per-id and other-lines rules.
  - `an_answering_row_shows_no_problem_line` (:335)
  - `a_half_found_kontrol_fills_the_side_found` (:351)
  - `choosing_the_kontrol_refreshes_the_ports` (:368): `port_reads()` rises.
  - `no_ports_at_all_and_the_kontrol_give_the_note` (:383)
  - `the_kontrol_sheet_speaks_descriptively` (:394)
  - `a_command_map_is_removed` (`LearnModelTests.swift:125`): a refusal is titled "Remove MIDI Learn Failed".
  - `a_hui_and_a_kontrol_are_added_without_a_position` (`:112`): `add(Hui, …, Some(2))` passes `None`.
  - `refresh_reports_only_a_change`: a second `refresh` with nothing new is `false`.
  - `learnable_matches_the_keys_rows`: `learnable("edit.undo")` is true; `learnable("file.open")` and `learnable("surface.add")` are false.
- [ ] **Step 2: Run** `cargo test -p vizdaw-app --test surface_settings`. Expected: FAIL, because nothing is defined yet.
- [ ] **Step 3: Implement** the seams, then `surface_settings.rs` from `M/SurfaceSettings.swift:52-164`. `begin_add` refreshes, then picks the first source and destination (`:139-144`). `commit_add` passes a position only for an XT (`:150`).
- [ ] **Step 4: Run** the test, then `cargo test -p vizdaw-app`. Expected: PASS.
- [ ] **Step 5: Commit** `surfaces: the core seams and the Surfaces settings model`.

### Task 2: `surface.*`, `state.get surfaces`, and the launch load

**Files:**
- Create: `crates/vizdaw-app/src/surface_commands.rs`
- Modify:
  - `crates/vizdaw-app/src/lib.rs`
  - `crates/vizdaw-gpui/src/{control.rs,lib.rs,app_state.rs}`
  - `crates/vizdaw-app/src/store.rs`, which fills the claimed ports
- Test:
  - `crates/vizdaw-app/tests/surface_commands.rs`
  - `crates/vizdaw-gpui/tests/surfaces_control.rs` (new)
  - `crates/vizctl/tests/e2e_gpui.rs`

**Interfaces:**
- Consumes: Task 1's `CoreCalls` and `DocSession` methods.
- Produces:
  - `pub const SURFACE_COMMANDS: &[&str]`: `surface.add`, `surface.remove`, `surface.inject`, `surface.learn` and `surface.unlearn`.
  - `pub fn parse_kind(&str) -> Option<SurfaceKindRow>`, for `mcu`, `mcu_xt`, `hui` and `kontrol_mk3` (`M/SurfaceCommands.swift:70-78`).
  - `pub fn run_surface(id: &str, args: &[(String, ArgValue)], doc: Option<&mut DocumentModel>, core: &dyn CoreCalls) -> Result<Ran, ControlError>`.
    - `surface.add` and `surface.remove` need no document.
    - The other three check every argument first, and only then need a document: `no_document` "No document is open." (`:108`).
    - A core refusal is `commands::from_viz` (`busy` for `Busy`, else `failed`).
  - Its messages, verbatim, through `ControlError::params` (the `id: ` prefix comes from `params`):
    - "`type` is mcu, mcu_xt, hui or kontrol_mk3" (`:184`)
    - "`input` and `output` name the MIDI ports" (`:189`)
    - "`position` is 0 to 3" (`:194`)
    - "`surface` is a surface ID" (`:92,157`)
    - "`bytes` are 1 to 1024 hex bytes such as \"90 68 7f\"" (`:159`), parsed with `record::hex_bytes`
    - "give exactly one of `target` and `command`" (`:167`)
    - "`target` is fader:<strip>, pan:<strip>, mute:<strip>, solo:<strip>, send:<strip>:<n> (n from 1), param:<slot>:<param> or vca:<vca>" (`:171-173`), parsed with `vizdaw_ffi::automation::parse_automation_target`
    - "`command` is a registry command ID" (`:177`)
  - What `surface.*` does:
    - `surface.add`'s status is "surface <id>" (`:204`). A HUI unit and a Kontrol are added with no position.
    - `surface.learn` with `command` calls `focus_surfaces` first, then `core.learn_command` (`:118-120`).
    - `surface.unlearn` with `target` is refused `unavailable` `GESTURE_OPEN` while a gesture is open (`:122`). Otherwise it applies the snapshot.
  - GPUI dispatch:
    - `surface.add` and `surface.remove` go through `app_wide`. They answer `doc: null, window: null`.
    - The other three go through `target(window)`.
  - `state.get surfaces`:
    - With a document (the named window's, or the front one when no window is named), it answers `ControlResult::Surfaces { doc: Some(id), state: session.surface_state() }`.
    - Unnamed, with no document, it answers `{ doc: None, state: core.surfaces_state() }`.
    - With a named window that has no document, it answers `no_document` (`SurfaceCommandsTests.swift:168`).
  - Launch (`lib.rs::run`):
    - `core.surfaces_load()` runs after `set_prefs` and before the first document, as Swift ran it after `setSupportDir` (`S/VizDAWApp.swift:515-522`).
    - The input picker's claimed ports follow `surface_list()`'s inputs. `LaneInputs::set_claimed_ports` is called at launch and after every add or remove.

- [ ] **Step 1: Write the failing tests:**
  - App, ported from `SurfaceCommandsTests.swift`:
    - `surface_commands_check_their_arguments` (:60): every row of its table is `invalid_params`, and nothing reaches the core or the session.
    - `inject_and_learn_reach_the_session` (:105): the bytes are `[0x90,0x68,0x7f,0xe0,0x00,0x40]`. A cut message still goes to the core. A command learn focuses once. A core refusal is `failed`.
    - `add_and_remove_need_no_document` (:134): the status is "surface 0", and inject without a document is `no_document`.
    - `a_kontrol_is_added_without_a_position` (:156)
    - `unlearn_waits_for_an_open_gesture`
  - GPUI:
    - `state_get_surfaces_answers_app_wide_without_a_document`: `doc` is null and `feedback` is `[]`.
    - `state_get_surfaces_with_a_document_names_it`
    - `surface_add_runs_without_a_window`
    - `launch_loads_the_surfaces_before_the_first_document`: in `FakeCore`'s call log, `surfaces_load` comes before the first `new_empty`.
    - `the_picker_marks_a_surface_input_as_claimed`: after `surface.add` with input "In A", the picker lists "In A (used by a surface)" (`lane_inputs.rs:318`).
  - e2e:
    - `gpui_a_damaged_surfaces_file_is_kept_aside` (Review Focus 3). Before launch it writes `{` to the throwaway home's support file. `vizctl` launches with `HOME` set to that home (`vizctl/src/launch.rs:311`), so the file is `Library/Application Support/VizDAW/surfaces.json` under it on macOS and `.local/share/vizdaw/surfaces.json` on Linux (`core-ffi/src/recovery.rs:53-63`). Then:
      - `state.get surfaces` has `surfaces: []` and one `problems` line starting "surfaces.json could not be read and was kept aside as surfaces.json.bad".
      - `surfaces.json.bad` exists.
      - A `surface.add` then saves a fresh `surfaces.json`.
    - `gpui_midi_learn_over_the_channel`, on a new demo:
      - `surface.learn {}` is `invalid_params`.
      - `{command: "file.open"}` is `failed`, because the core refuses it.
      - `{command: "edit.undo"}` runs, and `state.get surfaces` `learning` is `"edit.undo"` (the core names a command learn by its id, `core-ffi/src/surfaces.rs:2629`).
      - `{target: "fader:<synth>"}` runs, and `learning` changes.
      - `surface.unlearn {command: "edit.undo"}` is `failed` (nothing learned).
- [ ] **Step 2: Run** `cargo test -p vizdaw-app --test surface_commands` and `cargo test -p vizdaw-gpui --test surfaces_control`. Expected: FAIL.
- [ ] **Step 3: Implement** from `M/SurfaceCommands.swift:86-205` and `S/ControlChannel.swift:438-449`. Remove the `not_yet` path for `StateTarget::Surfaces` (`control.rs:541`).
- [ ] **Step 4: Run** the tests, then both e2e on the Mac. Expected: PASS. Note in the commit body how long `surfaces_load` takes at launch on the Mac with one MCU in the file. Over 50 ms gets a backlog row (Ruling 3).
- [ ] **Step 5: Commit** `surfaces: surface.* and state.get surfaces; surfaces.json loads at launch`.

### Task 3: The surface relay, focus and count-in, and the two surface e2e

**Files:**
- Create: `crates/vizdaw-app/src/surface_relay.rs`
- Modify:
  - `crates/vizdaw-app/src/{document.rs,store.rs,lib.rs}`
  - `crates/vizdaw-gpui/src/{shell.rs,app_state.rs,control.rs}`
- Test:
  - `crates/vizdaw-app/tests/surface_relay.rs`
  - `crates/vizdaw-gpui/tests/surface_relay.rs` (new)
  - `crates/vizctl/tests/e2e_gpui.rs`

**Interfaces:**
- Consumes: Task 1's `take_surface_events`, `set_surface_count_in` and `focus_surfaces`.
- Produces:
  - `pub const JOG_WINDOW: Duration = 150 ms` (`M/SurfaceRelay.swift:8`).
  - `pub const DIALOG_COMMANDS: &[&str]`, from `:23-27`: `file.open`, `file.close`, `file.save`, `file.save_as`, `file.project_settings`, `keys.save`, `keys.load`, `media.import`, `media.relink`, `media.rename`, `record.recover`, `region.normalize` and `file.bounce`.
  - `pub enum RelayStep`:
    - `Command { id: String, programmatic: bool }`
    - `Select(u64)`
    - `Arm(Option<u64>)`, holding the strip's path
    - `CountIn`
    - `Locate(i64)`, holding detents
  - `pub struct SurfaceRelay`, with:
    - `apply(&mut self, events: Vec<SurfaceEventRow>, now: Instant) -> Vec<RelayStep>`. Pending detents become a `Locate` before any non-jog event (`:61-64`).
    - `flush(&mut self, now: Instant) -> Option<RelayStep>`, which is a `Locate` once `JOG_WINDOW` has passed since the first unlocated detent (`:100-103`).
  - `pub struct SurfaceCommand { pub id: String, pub programmatic: bool, pub bypass: bool }`, where `bypass` is `!programmatic && !DIALOG_COMMANDS.contains(id)`.
  - On `DocumentModel`:
    - `idle` drains the events into `surface_steps` and tells the core the count-in. The count-in is sent once per change, and again after a refusal (`M/DocumentModel.swift:856-862`).
    - `take_surface_steps(&mut self) -> Vec<RelayStep>`
    - `apply_surface_step(&mut self, step: RelayStep) -> Option<SurfaceCommand>`:
      - `Select` sets the selected strips.
      - `Arm` toggles the input track's arm, as `MixerModel::toggle_arm` does.
      - `CountIn` is `record.toggle_count_in()`.
      - `Locate` reads `session.transport()` fresh, then `move_to(max(0, tick + detents × PPQ / 4))` (`:108-121`). A failure is the status line `"Locate Failed"`'s message.
      - `Command` is returned for the window to run.
    - `set_surface_acting(bool)`. While it is set, `transport_keys_enabled` and `return_key_enabled` are true (ruling T9-1, `M/DocumentModel.swift:582,604`).
  - `DocumentStore::follow_surface_focus(&mut self)`. When `front()` differs from the last focused document, it calls that document's `focus_surfaces`, and keeps it only on `Ok` (`S/VizDAWApp.swift:590-603`). With no document open, it forgets the focus.
  - In GPUI:
    - `DocumentWindow::run_surface_steps(&mut self, steps, window, cx)` runs from the document window's poll loop (`shell.rs:470-487`), after `AppState::poll`. Only a document's own window takes its steps; a mixer window leaves them.
    - A device command (`bypass` or not) is `window.dispatch_action(menus::action_for(id))`. With `bypass`, `set_surface_acting(true)` is set before the dispatch and cleared in a `cx.defer` after it.
    - A programmatic command runs through `control::run_quietly(id: &str, doc: DocId, cx: &mut App)`. That is `command.run`'s path with no arguments. A failure goes to the document's status line, and a `Later` or `Job` is not waited on (`S/VizDAWApp.swift:178-186`).

- [ ] **Step 1: Write the failing tests:**
  - App, ported from `SurfaceSettingsTests.swift`:
    - `events_select_jog_and_run_commands` (:33)
    - `jogs_within_150_ms_are_one_locate` (:53)
    - `arm_toggles_the_strips_input_track` (:70)
    - `dialog_commands_do_not_bypass` (:89): `file.save`'s step has `bypass: false`, and steps keep their order across two batches.
    - `a_surface_button_acts_while_a_text_field_has_the_keyboard` (:110): with acting set, `transport.play` runs; once it is cleared, the keys still wait.
    - `a_jog_is_located_before_a_later_command_in_the_batch` (:124) and `…_in_the_next_batch` (:136)
    - `a_jog_counts_from_the_current_tick_and_stops_at_zero` (:158): one detent is 3840 ticks, and −10 from tick 0 locates to 0.
    - `idle_drains_surface_events` (:206)
    - `a_count_in_press_toggles_the_record_count_in` (:220)
    - `idle_tells_the_surfaces_the_count_in` (:236)
    - `a_refused_count_in_is_said_again` (:255): `[true]`, then `[true, true]`, then still `[true, true]`.
    - `an_injected_save_opens_no_dialog` (:405)
  - App, new:
    - `focus_follows_the_front_document_and_retries_after_a_refusal` (Review Focus 5): two documents, the second is made front, `focus_calls` rises once. A scripted refusal is retried on the next call.
  - GPUI:
    - `events_apply_to_the_session_that_took_them` (Review Focus 5): two document windows, A in front. Events pushed into B's `FakeSession` (`Select`, then `Command transport.play`) select in B and play B; A is unchanged.
    - `a_surface_play_runs_while_the_name_field_has_the_keyboard`
    - `a_programmatic_save_answers_no_dialog`: `Command { id: "file.save", programmatic: true }` on an untitled document shows no sheet. The status line has the refusal.
  - e2e:
    - `gpui_drives_a_fader_and_a_button_from_an_mcu`, ported from Swift `e2e.rs:1653-1795`. Every step and assertion is kept: closing the launch window first, the app-wide answer, the two refused adds (`failed` for an empty input, `invalid_params` for `nope`), touch, "Set Fader", the mute LED `90 10 7f`, `e0 00 20` sent once, no `d0` lines, the cut message counted, `zz` refused and the remove.
    - `gpui_drives_the_mixer_from_a_kontrol_mk3`, ported from `e2e.rs:1797-1900` with every step: the handshake, the knob ride, "Set Fader", the 500 ms end, play and stop through the relay, "Set Tempo", the echoed tempo and the cut SysEx counted. The `ni` variable becomes `identity`.
- [ ] **Step 2: Run** `cargo test -p vizdaw-app --test surface_relay` and `cargo test -p vizdaw-gpui --test surface_relay`. Expected: FAIL.
- [ ] **Step 3: Implement** from `M/SurfaceRelay.swift:40-128`. GPUI has no nested run loop, so Swift's deferred-while-applying queue is not ported: one batch runs per tick, in order.
- [ ] **Step 4: Run** the tests, then both e2e on the Mac. Expected: PASS.
- [ ] **Step 5: Commit** `surfaces: events reach their document; focus and count-in; the MCU and keyboard e2e`.

### Task 4: The learn model

**Files:**
- Create: `crates/vizdaw-app/src/learn.rs`
- Modify: `crates/vizdaw-app/src/{document.rs,store.rs,lib.rs}`
- Test: `crates/vizdaw-app/tests/learn.rs`

**Interfaces:**
- Consumes: Task 1's session and core learn calls.
- Produces:
  - Constants, verbatim:
    - `WAITING_TEXT` = "Waiting for MIDI… move a control (Esc cancels)" (`M/LearnModel.swift:16`)
    - `FAILED_TITLE` = "MIDI Learn Failed" (`:36`)
    - `PICKUP_HELP` = "The control takes over only once it reaches the current value" (`:164`)
    - `NO_MAPS` = "No MIDI controls are learned in this project. Choose MIDI Learn in a control's menu." (`S/LearnMapsSheet.swift:18`)
    - `MISSING` = "(missing)" (`:169`)
    - `MODES` = `[Absolute, Relative2sComplement, RelativeBinOffset]` (`:161`)
  - `pub fn waiting(project: Option<&str>, what: Option<&str>) -> String` (`:20-25`).
  - `pub fn mode_title(LearnModeRow) -> &'static str`: "Absolute", "Relative (2's complement)" and "Relative (binary offset)" (`:152-158`).
  - `pub fn target_title(t: &AutomationTargetRow, doc: &DocumentModel) -> String`, ported from `M/StripLane.swift:155-173`:
    - "Fader · <strip>", "Pan · <strip>", "Mute · <strip>", "Solo · <strip>"
    - "Send <n> to <bus> · <strip>"
    - "<param> · <plug-in>", falling back to "Parameter <p>" and "Plugin <slot>"
    - The VCA strip's title + " Fader"
    - A strip not found is "Strip <id>".
  - `pub struct LearnModel` (at `DocumentModel.learn`), with:
    - `is_learning()` and `is_active()`. Active means learning or the maps sheet is open (`:76`).
    - `banner_text(project: Option<&str>) -> Option<String>`. It uses the `waiting` text the last poll read from `session.surface_state().learning`.
    - `maps() -> &[LearnMapRow]`
    - `maps_open() -> bool`, `open_maps()` and `close_maps()`. Opening rereads the maps (`:50-52`).
    - `problem() -> Option<&str>`, the refusal shown in the open maps sheet (Ruling 5)
  - `DocumentModel` wrappers:
    - `learn_target(t)` (`:80-82`)
    - `learn_command(id, core: &dyn CoreCalls)`, which focuses first (`:87-92`)
    - `cancel_learn(core)` (`:105-108`)
    - `set_learn_mode(&LearnMapRow, LearnModeRow)`
    - `set_learn_pickup(&LearnMapRow, bool)`, which does nothing unless `picks_up` (`:131-134`)
    - `remove_learn(&LearnMapRow)`
    - Each edit applies its snapshot as one undo step.
    - A refusal sets the document alert, titled `FAILED_TITLE` (`:94-102,141-149`). While the maps sheet is open, it sets `problem()` instead.
  - `idle` polls the model while it is active: the banner goes once `learning` is `None`, and the maps follow the project (`:113-124`).
  - `DocumentStore::close(id)` calls `core.cancel_learn()` when that document is learning (Review Focus 2).
  - `DocumentStore::learning_document() -> Option<DocId>`, the first learning document, for Esc (`S/KeyMonitor.swift:161`).

- [ ] **Step 1: Write the failing tests.** Port `LearnModelTests.swift` by name:
  - `learning_shows_a_banner_until_the_map_lands` (:22)
  - `cancel_ends_the_learn` (:35): `focus_calls` is 1, then `learn_cancels` is 1.
  - `the_banner_names_what_it_waits_for_and_the_project` (:48): the three strings, verbatim.
  - `modes_and_removal_reach_the_session` (:65)
  - `pick_up_is_shown_and_set` (:79)
  - `mode_titles` (:88)
  - `a_refused_learn_is_an_alert` (:95): titled "MIDI Learn Failed", with no banner.
  - New:
    - `closing_the_learning_document_cancels_the_learn` (Review Focus 2). Closing a document that is not learning cancels nothing.
    - `an_edit_is_one_undo_step`: the undo label after `set_learn_mode` is "MIDI Learn", and after `remove_learn` it is "Remove MIDI Learn".
    - `a_refusal_with_the_sheet_open_is_its_line`
    - `target_titles_name_strips_sends_and_params`
- [ ] **Step 2: Run** `cargo test -p vizdaw-app --test learn`. Expected: FAIL.
- [ ] **Step 3: Implement** from `M/LearnModel.swift`.
- [ ] **Step 4: Run** the test, then `cargo test -p vizdaw-app`. Expected: PASS.
- [ ] **Step 5: Commit** `learn: the per-document MIDI learn model`.

### Task 5: MIDI Learn in the mixer and the plug-in window, the banner, and Esc

**Files:**
- Create: `crates/vizdaw-gpui/src/learn_banner.rs`
- Modify:
  - `crates/vizdaw-gpui/src/{mixer_view.rs,shell.rs,lib.rs}`
  - `crates/vizdaw-gpui/src/plugin_windows/view.rs`
  - `crates/vizdaw-app/src/plugin_window.rs`, where the "disabled until the surfaces slice" notes go
- Test: `crates/vizdaw-gpui/tests/mixer_learn.rs` (new); `crates/vizdaw-gpui/tests/plugin_window_view.rs`

**Interfaces:**
- Consumes: Task 4's `DocumentModel` learn wrappers, `LearnModel::banner_text` and `DocumentStore::learning_document`.
- Produces:
  - `pub fn learn_banner(doc: DocId, scope: &mut RegistryScope, cx: &App) -> Option<AnyElement>`. It shows the text and a small "Cancel" button (`S/LearnMapsSheet.swift:81-101`). It names the project ("in “<title>”") only when two or more documents are open (`:105-108`).
  - New `StripMenu` variants `Fader`, `Pan`, `Mute` and `Solo`, each a one-item menu "MIDI Learn". Right-clicking the fader or its number box, the pan knob or its number box, M or S opens them (`S/ChannelStrip.swift:882,929,1007,1013`).
  - `send_entries` gains "MIDI Learn" after On (`:834`).
  - The master strip has no Solo.
  - Escape in a document, mixer or plug-in window cancels the first learning document's learn. It does nothing when a sheet is up, a text field has the keyboard, or the key has modifiers (`S/KeyMonitor.swift:155-164`). Otherwise Escape goes on as today.
  - The plug-in window header:
    - Learn is enabled once a parameter was touched. Clicking it learns `param:<slot>:<param>` (`M/PluginWindowModel.swift:255-258`).
    - While a learn waits, the banner text and "Cancel" take the place of the placeholder's reason (`S/PluginWindows.swift:267-275`).
    - The help strings are as today (`plugin_window.rs:44-46,301-310`).

- [ ] **Step 1: Write the failing tests:**
  - `right_click_on_a_fader_offers_midi_learn`: choosing it records `learn_targets == [StripFader { strip }]`, and the banner reads `WAITING_TEXT` at the top of the mixer, below its bar (`S/MixerView.swift:78`).
  - `pan_mute_solo_and_a_send_learn_their_targets`: the master's menu has no Solo.
  - `the_banner_names_the_project_with_two_open`
  - `cancel_and_escape_end_the_learn`: `learn_cancels` rises. Escape with a text field focused cancels nothing.
  - `a_refused_learn_shows_midi_learn_failed`: the document window's alert has that title and the core's message.
  - `the_banner_goes_when_the_map_lands`: the scripted `learning` is cleared, and after a poll there is no banner.
  - In `plugin_window_view.rs`:
    - `learn_is_enabled_after_a_touch_and_learns_the_param`
    - `the_header_shows_the_banner_while_learning`
- [ ] **Step 2: Run** `cargo test -p vizdaw-gpui --test mixer_learn`. Expected: FAIL.
- [ ] **Step 3: Implement.** The menus go on the existing `open_menu` / `popup` path. The banner is in theme tokens only. Remove the "MIDI Learn comes with the surfaces" doc lines (`mixer_view.rs:2906,3062`).
- [ ] **Step 4: Run** the tests and `theme_editors`. Expected: PASS.
- [ ] **Step 5: Commit** `learn: MIDI Learn in the mixer and plug-in window, the banner and Esc`.

### Task 6: The MIDI Learn Maps sheet

**Files:**
- Create: `crates/vizdaw-gpui/src/sheets/learn_maps.rs`
- Modify: `crates/vizdaw-gpui/src/{mixer_view.rs,sheets/mod.rs}`
- Test: `crates/vizdaw-gpui/tests/learn_maps.rs` (new)

**Interfaces:**
- Consumes: Task 4's `LearnModel` (`maps`, `open_maps`, `close_maps`, `problem`), `target_title`, `mode_title`, `MODES`, `PICKUP_HELP`, `NO_MAPS`, and the `Sheet` trait.
- Produces:
  - `pub struct LearnMapsSheet { doc: DocId }`, implementing `Sheet`:
    - The label is "MIDI Learn Maps" (`S/LearnMapsSheet.swift:16`), and the width is 640.
    - It has one button, "Done" (the default; Return or Esc), which calls `close_maps`.
    - A row shows `map.text`, and a caption with `target_title`, or `MISSING` when `missing` (`:47-54`).
    - Each row has a mode picker 190 wide, then "Pick-up": checked when `picks_up && pickup`, disabled unless `picks_up`, with `PICKUP_HELP` (`:56-71`). Then "Remove" (`:72`).
    - With no maps it shows the `NO_MAPS` line. A refusal shows as a `state.error` line under the list.
  - The strip name's menu gains a divider and "MIDI Learn Maps…" after the group and VCA items (`S/ChannelStrip.swift:401-402`). It opens the sheet in the window that holds this mixer: the document window (Mix view or pane) or the mixer window. One sheet is open at a time.

- [ ] **Step 1: Write the failing tests:**
  - `the_strip_menu_opens_the_maps_sheet`: the label is "MIDI Learn Maps".
  - `an_empty_project_says_how_to_learn`
  - `a_row_shows_source_target_and_missing`
  - `the_mode_picker_sets_the_mode_as_one_undo_step`: `learn_modes` records it, and the undo is "MIDI Learn".
  - `pick_up_is_disabled_for_a_relative_map`
  - `remove_removes_and_the_list_follows`
  - `a_refusal_shows_in_the_sheet_and_it_stays_open`
  - `done_closes_and_polling_stops`: `is_active()` is false afterwards.
- [ ] **Step 2: Run** `cargo test -p vizdaw-gpui --test learn_maps`. Expected: FAIL.
- [ ] **Step 3: Implement** from `S/LearnMapsSheet.swift:10-75`. The mode picker uses `menu_face`.
- [ ] **Step 4: Run** the tests and `theme_editors`. Expected: PASS.
- [ ] **Step 5: Commit** `learn: the MIDI Learn Maps sheet`.

### Task 7: The Surfaces tab and the Add Surface sheet

**Files:**
- Create: `crates/vizdaw-gpui/src/settings_window/surfaces.rs`; `crates/vizdaw-gpui/src/sheets/add_surface.rs`
- Modify: `crates/vizdaw-gpui/src/settings_window/mod.rs`, which gets a `SheetHost` field, the tab's width and the poll; `crates/vizdaw-gpui/src/app_state.rs` (`AppState.surfaces: SurfaceSettings`)
- Test: `crates/vizdaw-gpui/tests/settings_surfaces.rs` (new)

**Interfaces:**
- Consumes: Task 1's `SurfaceSettings`, `SurfaceRead`, `read`, the constants and titles; `read_off_thread`; `alerts::ask` with `Ask::alert`.
- Produces:
  - `SURFACES_WIDTH = 560.`
  - `SettingsWindow` gains:
    - `sheet: SheetHost`
    - `surfaces_poll: Option<Task<()>>`, which every 500 ms (`S/SurfacesSettingsView.swift:71-77`) starts one `read_off_thread(read(core))` unless one is out
    - `surfaces_reading: bool`
    - `surfaces_gen: u64`
  - The tab, in Swift's order (`:17-66`):
    - The "Surfaces" section shows `NO_SURFACES`, or one row per unit: `row_title`, then a caption "<input> → <output>". On the right, `status` in `text_primary` when connected and online, else `text_secondary`; under it, `problem_for` in `state.warning`. Then "Remove".
    - Then `other_problems`, then "Add Surface…".
    - The "Commands Learned to MIDI" section shows `NO_COMMAND_MAPS`, or rows: `map.text`, the command's registry title (or its id) and "Remove".
    - A refusal is an `Ask::alert` on the Settings window, with the model's alert title and "OK" (`:83-90`).
  - `pub struct AddSurfaceSheet`, implementing `Sheet`:
    - The label is "Add Surface", and the width is 420.
    - It shows the pickers "Type" (the four `kind_title`s), "Input" and "Output". An empty list shows `NO_SOURCES` or `NO_DESTINATIONS`.
    - For a Kontrol it shows `draft_note` in `state.warning`, then `KONTROL_HELP` as a caption.
    - For an XT it shows a stepper "Position: N" (0…3) and `XT_HELP`.
    - `draft_problem` shows in `state.error`.
    - Its buttons are "Cancel" (Esc) and "Add" (Return), and Add is disabled while the input or output is empty (`:105-155`).
    - A refused Add keeps the sheet open.

- [ ] **Step 1: Write the failing tests:**
  - `the_tab_lists_units_and_learned_commands`: the empty lines, verbatim. A unit row shows "MCU XT-compatible · position 2", "In A → Out A" and "Connected · not answering".
  - `a_problem_line_shows_under_a_silent_unit_only`
  - `remove_removes_and_a_refusal_is_an_alert`: the alert is titled "Remove Surface Failed".
  - `add_surface_opens_the_sheet_with_the_first_ports`
  - `the_kontrol_type_fills_its_daw_ports_and_shows_the_help`
  - `the_xt_type_shows_the_position_stepper`
  - `a_refused_add_shows_in_the_sheet_and_it_stays`
  - `no_ports_disable_add`: the Linux live case. The pickers read "No MIDI sources" and "No MIDI destinations".
  - `a_port_that_vanishes_reads_disconnected_at_the_next_poll` (Review Focus 1): `set_surface_rows` flips `connected` to false. After one poll the row reads "Disconnected", the open sheet keeps "In A", and Add shows the scripted refusal.
  - `the_surfaces_poll_reads_off_thread_one_at_a_time` (Review Focus 4):
    - With `hold_surface_reads`, two poll periods start one read only.
    - After the release, the next read starts.
    - Leaving the tab stops the poll: no read starts in the next second.
    - An unchanged read does not notify (`apply_read` is false).
  - `removing_a_learned_command_reaches_the_core`
- [ ] **Step 2: Run** `cargo test -p vizdaw-gpui --test settings_surfaces`. Expected: FAIL.
- [ ] **Step 3: Implement** from `S/SurfacesSettingsView.swift`. Remove the Surfaces placeholder line from the tab strip.
- [ ] **Step 4: Run** the tests, `settings_window` and `theme_editors`. Expected: PASS.
- [ ] **Step 5: Commit** `settings: the Surfaces tab and the Add Surface sheet`.

### Task 8: Learn MIDI in Settings › Keys

**Files:**
- Modify: `crates/vizdaw-gpui/src/settings_window/{keys.rs,mod.rs}`
- Test: `crates/vizdaw-gpui/tests/settings_keys.rs`

**Interfaces:**
- Consumes:
  - `vizdaw_ffi::surfaces::learnable`
  - Task 4's `DocumentModel::learn_command` and `cancel_learn`
  - Task 5's `learn_banner`
- Produces:
  - `Row` gains `learnable: bool`.
  - Each row gets a "Learn MIDI" button after its recorder, before Reset (`S/KeysSettingsView.swift:63-67`). It learns into `store.front()`, the front-most document still open (`:20-22`). The button:
    - is disabled with no document, or when not `learnable`.
    - has the help "Open a project to learn MIDI", or "Learn a MIDI control (a pad or button) for <title>", or "Only commands a key can run can be learned" (`:88-98`).
  - The banner for the front document sits under the reserve toggle (`:36-39`).
  - A refusal is an `Ask::alert` on the Settings window, titled "MIDI Learn Failed" (`:121-131`).
  - Escape in the Settings window cancels the learn, except while the recorder records, where Escape cancels recording, as in 9c.

- [ ] **Step 1: Write the failing tests:**
  - `learn_midi_is_disabled_without_a_document`: with the help text above.
  - `learn_midi_is_disabled_for_a_command_with_a_dialog`: `file.open`.
  - `learn_midi_learns_into_the_front_document`: in that document's `FakeSession`, `focus_calls` is 1, and `learned_commands` is `["edit.undo"]`.
  - `the_keys_tab_shows_the_banner_and_cancel`
  - `escape_cancels_the_learn_but_not_while_recording`
  - `a_refused_learn_is_midi_learn_failed`
- [ ] **Step 2: Run** `cargo test -p vizdaw-gpui --test settings_keys`. Expected: FAIL.
- [ ] **Step 3: Implement** from `S/KeysSettingsView.swift:37-39,85-98,121-131`.
- [ ] **Step 4: Run** the tests and `theme_editors`. Expected: PASS.
- [ ] **Step 5: Commit** `settings: Learn MIDI in the Keys tab`.

### Task 9: Checks, checklist and backlog

**Files:**
- Modify:
  - `docs/superpowers/specs/2026-11-22-parity-checklist.md`
  - `docs/feature-backlog.md`
  - `docs/testing/manual-checks.md`
  - `docs/testing/controller-checks.md`
  - `README.md`

- [ ] **Step 1:** Run the whole suite: `cargo test --workspace`, clippy, fmt, and every e2e named in this plan on the Mac. Expected: all green.
- [ ] **Step 2:** Fill the 9d Mac cells. The Linux cells are filled after CI, as in 9b and 9c.
  - The five `surface.*` rows.
  - "Settings: Surfaces".
  - "Settings: Keys": add "S9d (Learn MIDI)".
  - The MIDI Learn Maps and Add Surface sheets, and the MIDI Learn and Surfaces alerts.
  - The two e2e rows, with their `gpui_` names.
  - The README rows: 5f-1 complete, 5f-4 with "S9d (the keyboard over the channel; on screen: M24)", and "Strip Chart and automation" unchanged.
  - A new Linux gap row: "Control-surface MIDI ports on Linux (Settings › Surfaces lists no ports; a unit added stays Disconnected)". The answer: "Host track H3 (ALSA sequencer ports); until then `surface.inject` and the offline fake transport work, and the e2e run on them."
- [ ] **Step 3:** Update the backlog:
  - Mark "Learn MIDI in Settings › Keys" and "GPUI claimed MIDI ports wait for surfaces" **Done (slice 9d)**.
  - Add "MIDI Learn in the Strip Chart rows" (Swift `StripChartView.swift:374-381`; waits for the GPUI Strip Chart).
  - Add "The MIDI learn glyph in the banner" (Ruling 6).
  - Add "Maps sheet stays open when its mixer pane hides" (Ruling 7).
  - Add each task's review leftovers.
- [ ] **Step 4:** Add the owner rows to manual-checks, after M22 and L29:
  - M23: add a real MCU-compatible unit in Settings › Surfaces, move fader 1 and press Mute 1. Expected: the strip follows, the LEDs light, and unplugging reads "Disconnected" within a second.
  - M24: plug in a host-integration keyboard and choose it in Add Surface. Expected: both ports fill with its "DAW" port; its transport, knob and tempo work.
  - M25: learn a knob to a fader from the mixer's menu, and a pad to Undo from Settings › Keys. Expected: the banner shows, then goes; the knob rides the fader; the pad undoes; the MIDI Learn Maps sheet lists the knob, changes its mode, and Undo reverts it.
  - L30: Settings › Surfaces on Linux. Expected: the empty lines draw; Add Surface… shows "No MIDI sources" and "No MIDI destinations", and Add is disabled.
  - L31: the MIDI Learn banner and Maps sheet on Linux. Expected: they draw with the system font; Esc cancels.
- [ ] **Step 5:** Add a controller-checks section "Surfaces and MIDI Learn (9d)":
  - The tab at 560
  - The Add sheet at 420, for each type
  - The banner in the mixer, Keys tab and plug-in window
  - The Maps sheet at 640, in a dark and a light theme
- [ ] **Step 6:** In the README:
  - The Settings line gains Surfaces: "…Automation, Surfaces and Plug-ins tabs. Co-Producer comes in a later slice."
  - The "Not yet" line becomes "Settings › Co-Producer and user themes."
  - The line "Changeable key bindings, control surfaces and MIDI learn." is removed: key bindings came in 9c.
  - Add one sentence to the Linux paragraph: control surfaces need H3's MIDI ports.
- [ ] **Step 7: Commit** `Plan 9d: checks, checklist, backlog`.

## Rulings made while planning

1. **The surfaces' focus follows `store.front()`**, the document key last. Swift used `noteTarget` (`S/VizDAWApp.swift:595`), and `front()` is the GPUI app's command target for menus and keys. Cost if wrong: a surface drives the last-key document while a mixer window is key. That is the same document either way.
2. **A learn started over the channel shows no banner**, as in Swift. Only `LearnModel`'s own learns set it (`M/LearnModel.swift:94-102`; `M/SurfaceCommands.swift:115-120` calls the session directly). So the e2e checks `state.get surfaces` `learning`, not the banner, and Esc does not cancel a channel learn. Cost: an agent's learn is invisible on screen until it lands.
3. **`surfaces_load` at launch and `surface.add` stay on the UI thread**, as Swift ran them. They read a small file, and create the MIDI client once on a live Mac. Task 2 measures the launch call; over 50 ms gets a backlog row. Cost if wrong: a short launch stall with a surface configured.
4. **No nested-run-loop queue in the relay.** GPUI's panels and sheets do not spin a nested loop, so Swift's `applying`/`deferred` guard (`M/SurfaceRelay.swift:14-17,44-55`) has nothing to guard. One batch of steps runs per poll tick, in order. Cost if wrong: a reentrant step would run early. The order test would catch it.
5. **A refusal while the Maps sheet is open is a red line in the sheet**, not an alert over it. Swift stacked an alert on the sheet (`S/LearnMapsSheet.swift:37-44`), and the GPUI `SheetHost` holds one card at a time. Cost: a line instead of an alert.
6. **The banner has no glyph.** Swift drew `AppIcon.midiLearn`, but the glyph sources are the owner's PSDs and the GPUI icon set has no MIDI learn glyph yet. Backlog row. Cost: a plainer banner.
7. **The Maps sheet belongs to its window** and closes with it, or with Done. Swift also closed it when its mixer placement disappeared (`S/MixerView.swift:83-87`), for example when the Mix pane was hidden. Backlog row. Cost: a sheet left open after hiding the pane under it.
8. **Closing a learning document cancels its learn** (Review Focus 2). Swift left the core's waiting learn for a closed session. Nothing landed in it, but the core kept waiting. Cost: none known.
9. **A surface's dialog command runs as its key does** (`menus::action_for`), so `file.save` on an untitled song opens the Save panel from a real button. A programmatic one runs quietly (Swift pre-flight I9).

## Outcome (2026-11-26)

All nine tasks are done, each with a task review. Tasks 3 and 4 needed one fix round each. The final whole-branch review found one Important issue, which is fixed.

- Commits `21617121..e9717337` on `next`. The Mac run is green: `cargo fmt --check`, clippy `-D warnings`, `cargo test --workspace`, and the e2e `gpui_a_damaged_surfaces_file_is_kept_aside`, `gpui_midi_learn_over_the_channel`, `gpui_drives_a_fader_and_a_button_from_an_mcu` and `gpui_drives_the_mixer_from_a_kontrol_mk3`.
- **Task fixes:** surface events at Swift's pace, from a 16 ms surface tick (Task 3, `29129e6d`); a newer learn replaces an older one and closing a document cancels only its own learn (Task 4, `37f92415`); an alert or an open menu takes Esc before a waiting learn (Task 5, `f2992e51`).
- **Final review fix:** the core routes live surface input only in the session idle, which ran at the transport poll's 250 ms. The surface tick now runs the idle every 16 ms while a surface, command map, learn or learn map exists (`e9717337`). About 0.7 µs per idle in release.
- **Outside the plan:** `scripts/make-gpui-app.sh` now seals the bundle last (`bac9d619`) and ships `vizdaw-scan` and the microphone usage text (`bf015a51`), after the owner's testing. A pre-existing DAWproject round-trip proptest failure (same-tick MIDI of different kinds) is fixed in `c70a04c8`.
- **Not seen on screen or with hardware yet:** M23–M25, L30–L31, controller-checks "9d".

### Rulings

1. Task 5's two Esc minors (an alert, an open menu) were fixed in the task, because a second Esc to close an alert is something a user meets at once. Cost: a little extra scope.
2. The DAWproject failure was outside 9d and did not hold up Task 9; it was fixed separately so CI stays green.
3. DAWproject keeps each MIDI kind in its own lane, so the order of different kinds on one tick is not kept; the round-trip projection now compares them in lane order. Cost if wrong: same-tick CC or pressure order changes after a DAWproject round trip, with no musical effect.
4. Surface commands while a sheet is up run as their keys would; there is no modal guard, as Swift's timer had none. Cost if wrong: a surface button acts behind a sheet.
5. `Session::idle` runs at about 60 Hz on the UI thread while surfaces are set up, as Swift's 60 Hz timer did. Cost if wrong: UI-thread load with a surface configured.
