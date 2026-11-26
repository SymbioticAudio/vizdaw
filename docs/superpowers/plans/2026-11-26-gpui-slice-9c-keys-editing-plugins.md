# 27.1.0 Slice 9c: Keys, Editing, Plug-ins and the Customize Transport Bar Sheet Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Three more Settings tabs work as in the Swift app: Keys, Editing and Plug-ins. Customize Transport Bar… (View menu) opens its sheet. Saved keys load at launch and drive the menus and the key routing. `keys.save/load/reset` and `plugins.new_channel_eq/install_multiq/default_eq` work over the control channel. `ui.customize_transport` answers `dialog_required`. The e2e parts these unblock are restored: the bindings half of the keys test, Multi-Q install, and the default-EQ half of the new-bus test.

**Architecture:**
- `vizdaw-app` gets three settings models, built like 9a's: they sit over `Prefs` and reach the core through `CoreCalls` so `FakeCore` can record and refuse.
  - `key_prefs.rs` ports `M/KeyBindings.swift` (key `keys`).
  - `editing_prefs.rs` ports the preference half of `M/Nudge.swift` (keys `nudge`, `crossfadeMs`, `zeroCrossings`, `externalEditor`).
  - `plugin_prefs.rs` ports `M/PluginSettings.swift` (keys `newChannelEq`, `newChannelEqPlugin`).
- `vizdaw-app` also gets `transport_customize.rs`, which ports `M/TransportCustomize.swift` over the existing `TransportLayout`.
- The new commands run in `vizdaw-app` and are dispatched as app-wide commands, like `audio.set`.
- In GPUI, `settings_window.rs` (1364 lines) becomes a folder, `settings_window/`. The existing code goes to `mod.rs` and each new tab gets a file. The sheet is `sheets/customize_transport.rs` on the existing `SheetHost`.
- After any change to the bindings, GPUI binds its keys and its menus again.

**Tech Stack:** Rust 2024; GPUI at the slice 1 pin; `core-ffi`, `core-control`, `vizctl`.

**Spec:** `docs/superpowers/specs/2026-11-20-cross-platform-27-1-design.md`.
- Parity rows in `docs/superpowers/specs/2026-11-22-parity-checklist.md`:
  - "Settings: Keys", "Settings: Editing" and "Settings: Plug-ins".
  - "Customize Transport Bar", with its sheet and `ui.customize_transport`.
  - "Replace the (EQ name)…" and the Keys save and open panels.
  - `keys.*` and `plugins.*`.
  - The e2e rows `keys_drive_step_entry_locators_and_bindings`, `multiq_install_and_switch_over_the_channel` and `default_eq_and_new_bus_over_the_channel`.
- Swift reference with exact strings and file:line: `.superpowers/sdd/settings-inventory.md`, §1.2, §1.3, §1.8, §1.10, §2, §3 (`keys.*`, `plugins.*`, `ui.customize_transport`) and §5 (`KeyBindingsTests`, `NudgeTests` :141-217, `PluginSettingsTests`, `TransportCustomizeTests`).
- Swift sources, still in the tree for reading:
  - `app/Sources/VizDAW/{KeysSettingsView,EditingSettingsView,PluginSettingsView,TransportCustomizeSheet,ExternalEditing}.swift`
  - `app/Sources/VizDAWAppModel/{KeyBindings,Nudge,PluginSettings,PluginSettingsCommands,TransportCustomize,TransportItems,CommandButtons,ControlCommands}.swift`
  - `app/Sources/VizDAWEditors/TransportFieldsView.swift:428-520` (`KeyRecorderView`)
  - The tests in `app/Tests/VizDAWAppModelTests/`.

## Global Constraints

- **Rules from slices 7a–9b that still apply:**
  - The Swift app is retired: match its behaviour; don't keep compatibility with it.
  - Use Swift's exact strings, citing the inventory's file:line.
  - The UI says "track", never "lane".
  - No banned vendor or DAW product names in repo text. Multi-Q's own name and its licence notices are allowed.
  - Nothing slow on the UI thread. File panels, the install copy and the catalog read happen off it, using the `read_off_thread` pattern from 9a.
  - GPUI tests press `secondary-` keys.
  - Linux CI runs the GPUI tests and the e2e on the offline backend, and every 9c test must pass there. A Mac-only step is skipped on Linux with a line saying why.
- **Before every commit:** `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and the tests the task names. CI stops at Format.
- **Running the e2e on the Mac:** `cargo build -p vizdaw-gpui`, then `VIZDAW_E2E_APP=target/debug/vizdaw-gpui VIZDAW_AUDIO=offline cargo test -p vizctl --test e2e_gpui -- --ignored <name>`.
- **Preferences:**
  - Keys drop Swift's `VizDAW.` prefix.
  - Decoding is tolerant per field. A damaged value keeps that field's default, and a damaged `keys` keeps the default bindings.
  - Tests use `Prefs::in_memory()`, and the e2e uses `vizctl`'s throwaway home.
  - A write failure after a change shows on the tab's problem line, as in 9a.
- **Core first, save after** (9a's rule) for the two plug-in settings: the core takes the value first, then it is saved.
- **Defaults (Swift):**
  - Nudge: step Edit Grid; pitch Semitones, amount 1, follow the key signature on, root C, scale Major.
  - Crossfade 10 ms, clamped to 1…1000. Snap to zero crossings on (damaged also means on). External editor none.
  - Number keys always go to locators: off.
  - Put an EQ on new channels: on. EQ for new channels: Multi-Q (nothing stored).
- **The look:**
  - Theme tokens only (`appearance::current()` and `strip_palette::tokens()`). `no_source_file_keeps_a_colour_literal` must stay green.
  - Text on a fill uses a contrast pair: `legend_on`, `readout.*` or `accent.onAccent`.
  - Tab widths are Swift's: Keys 720×520, Editing 460, Plug-ins 500. The sheet is 760 wide with a minimum height of 480.
- **Learn MIDI** waits for the Surfaces slice. The Keys tab has no Learn MIDI button and no learn banner yet. A backlog row says so.
- **Review pace:** fix crashes, data loss and wrong overwrites; log other edge cases to the backlog.

## Review Focus

1. **The recorder takes every key while recording**, ⌘ keys included, before the menus and the window's bindings, as Swift's `performKeyEquivalent` did.
   - Esc alone cancels.
   - A key VizDAW cannot bind is ignored, and recording goes on.
   - Clicking elsewhere stops recording.
   - Test: recording ⌘W binds it to the row and closes no window.
2. **A rebind takes effect everywhere at once.** After record, Replace, Reset, Reset All, Load or `keys.reset`:
   - the menus show the new key;
   - the old key no longer runs the command, and the new one does;
   - the step-entry and locator routing (`reserve_number_row`) follows.
   - Test: GPUI `keys_rebind_moves_the_menu_key_and_the_binding`.
3. **A Load that fails changes nothing.** A file that is not a bindings file, a file that is too big or a missing file leaves the bindings and the saved pref unchanged and shows the refusal. A partly known file loads and lists what it skipped ("Skipped: a; b").
4. **The default EQ choice.**
   - A stored choice that is no longer installed shows `<name> (not available)` and stays chosen.
   - A damaged stored choice becomes Multi-Q and the key is removed.
   - `plugins.default_eq` matches only installed, usable, enabled effects. The Multi-Q CLAP is left out of the choices.
   - Ruling: a stored `vst3` choice is kept. Swift's `readEq` dropped it, but its own command can set one.
5. **Customize sheet Cancel writes nothing.** Done writes one valid layout, through the same path as `ui.set_transport_bar`, and every open bar redraws.
   - The cluster cannot be removed.
   - A label over 32 characters is refused with Swift's message, and the sheet stays open.
   - A command button whose command is gone shows the stale line and keeps its place.

---

### Task 1: The key bindings model and `keys.*`

**Files:**
- Create: `crates/vizdaw-app/src/key_prefs.rs`
- Modify: `crates/vizdaw-app/src/{lib.rs,prefs.rs,commands.rs}`
- Test: `crates/vizdaw-app/tests/key_prefs.rs`

**Interfaces:**
- Produces:
  - `pub struct KeyBindings`, with:
    - `fn load(prefs: &Prefs) -> KeyBindings`
    - `fn keymap(&self) -> &Keymap`
    - `fn message(&self) -> Option<&str>`
    - `fn pending(&self) -> Option<&PendingKey>`
    - `fn record(&mut self, id: &str, chord: &str, prefs: &mut Prefs)`
    - `fn confirm_replace(&mut self, prefs)` and `fn cancel_replace(&mut self)`
    - `fn reset(&mut self, id, prefs)` and `fn reset_all(&mut self, prefs)`
    - `fn save(&self, path: &Path) -> Result<(), String>`
    - `fn load_file(&mut self, path: &Path, prefs) -> Result<(), String>`
  - `pub struct PendingKey { pub id: String, pub key: String }`
  - `pub fn run_keys(id: &str, args: &[(String, ArgValue)], keys: &mut KeyBindings, prefs: &mut Prefs) -> Result<Ran, ControlError>` for `keys.save`, `keys.load` and `keys.reset`.
  - `KEYS_KEY = "keys"`. The pref holds the keymap's JSON object (`Keymap::to_json`), written on every change.

- [ ] **Step 1: Write the failing tests.** Port `KeyBindingsTests.swift` :12-145 by name:
  - `bindings_persist_in_the_prefs`
  - `a_taken_key_is_refused_unless_replaced`: the message is `"<why>. Replace it?"`, with `<why>` from `BindError`'s Display.
  - `files_save_load_and_refuse_damage`
  - `replace_is_offered_only_for_a_taken_key_and_resets_forget_it`
  - `a_successful_bind_clears_the_message`
  - `a_file_with_new_ids_loads_and_lists_what_it_skipped`: `"Skipped: a; b"`.
  - `a_damaged_pref_keeps_the_defaults`
  - The `keys.*` commands:
    - without `path`, `dialog_required` with `"Save"` / `"Open"`;
    - a failure titled `"Save Key Bindings Failed"` / `"Load Key Bindings Failed"` (`M/ControlCommands.swift:372-381`);
    - `keys.reset` answers `ran`.
- [ ] **Step 2: Run** `cargo test -p vizdaw-app --test key_prefs`. Expected: FAIL, because nothing is defined yet.
- [ ] **Step 3: Implement `key_prefs.rs`** on `core_control::bindings::Keymap`, following `M/KeyBindings.swift:40-146`. File IO goes through `core-ffi` `keys.rs:196-208`, or through the same size-checked read, called directly. Route the three ids from `commands::run_app`'s caller, as `audio.set` is routed.
- [ ] **Step 4: Run** the test, then `cargo test -p vizdaw-app`. Expected: PASS.
- [ ] **Step 5: Commit** `keys: the bindings model, saved in the prefs, and keys.save/load/reset`.

### Task 2: GPUI keys from the model, rebinding, and the keys e2e

**Files:**
- Modify: `crates/vizdaw-gpui/src/{app_state.rs,lib.rs,menus.rs,control.rs,shell.rs}`
- Test: `crates/vizdaw-gpui/tests/keys.rs` (new); `crates/vizctl/tests/e2e_gpui.rs`

**Interfaces:**
- Consumes: Task 1's `KeyBindings` and `run_keys`.
- Produces:
  - `AppState.keys: KeyBindings`, replacing `AppState.keymap`. Every reader moves to `keys.keymap()`.
  - `pub fn rebind(cx: &mut App)` in `menus.rs`: `cx.clear_key_bindings()`, then `install` with the current keymap, then the front document's menus.

- [ ] **Step 1: Write the failing tests:**
  - GPUI `keys_rebind_moves_the_menu_key_and_the_binding` (Review Focus 2).
  - `saved_keys_load_at_launch`.
  - `keys_load_over_the_channel_dispatches`.
  - In the e2e, restore the bindings half of `gpui_keys_drive_step_entry_and_locators` from Swift `e2e.rs:536-556` (write `mine.vizkeys`, then `keys.load`, then 2 no longer jumps and X does, then `keys.reset`), and drop the doc comment's "waits for Keys settings" line.
- [ ] **Step 2: Run** `cargo test -p vizdaw-gpui --test keys`. Expected: FAIL.
- [ ] **Step 3: Implement.**
  - Load at launch next to `apply_all`.
  - Call `rebind` after every change that comes from the tab or the channel.
  - `reserve_number_row` reads through the model.
- [ ] **Step 4: Run** the GPUI tests, then the e2e `gpui_keys_drive_step_entry_and_locators`. Expected: PASS.
- [ ] **Step 5: Commit** `keys: launch load, rebinding, and the bindings half of the keys e2e`.

### Task 3: The Keys tab and the key recorder

**Files:**
- Move: `crates/vizdaw-gpui/src/settings_window.rs` → `settings_window/mod.rs`, unchanged apart from module lines.
- Create: `crates/vizdaw-gpui/src/settings_window/keys.rs`; `crates/vizdaw-gpui/src/controls/key_recorder.rs`
- Test: `crates/vizdaw-gpui/tests/settings_keys.rs`

**Interfaces:**
- Consumes: `AppState.keys`, `menus::rebind`, `file_panels` (save and open).
- Produces: `KeyRecorder`, with `on_key(Fn(String))`, labelled `"Record Key for <title>"`. It shows `"Record"` when idle and `"Press a key…"` while recording.

- [ ] **Step 1: Write the failing tests:**
  - `keys_tab_lists_commands_by_menu_in_registry_order`
  - `a_changed_key_is_drawn_in_accent_and_reset_is_enabled`
  - `recording_cmd_w_binds_it_and_closes_nothing` (Review Focus 1)
  - `escape_cancels_recording`
  - `a_taken_key_offers_replace_and_cancel`
  - `reset_all_restores_the_defaults`
  - `the_reserve_toggle_saves_and_routes`, with the text "Number keys always go to locators (step entry uses the keypad)"
- [ ] **Step 2: Run** `cargo test -p vizdaw-gpui --test settings_keys`. Expected: FAIL.
- [ ] **Step 3: Implement the tab.** Follow `S/KeysSettingsView.swift` and the inventory's §1.3 rows 1 and 3–5, at 720×520.
  - The list scrolls and shows a "step entry" caption on rows that have it.
  - Save… uses the default name "Key Bindings.vizkeys". Load… opens a single file.
  - The recorder converts a GPUI `Keystroke` into a chord text, the inverse of `menus::keystroke`. It takes keys in the capture phase and stops propagation while recording.
- [ ] **Step 4: Run** the tests and `theme_editors`. Expected: PASS.
- [ ] **Step 5: Commit** `settings: the Keys tab and its key recorder`.

### Task 4: The Editing preferences and tab

**Files:**
- Create: `crates/vizdaw-app/src/editing_prefs.rs`; `crates/vizdaw-gpui/src/settings_window/editing.rs`
- Test: `crates/vizdaw-app/tests/editing_prefs.rs`; `crates/vizdaw-gpui/tests/settings_editing.rs`

**Interfaces:**
- Produces:
  - `pub struct EditingPrefs { nudge: NudgeSettings, crossfade_ms: f64, zero_crossings: bool, external_editor: Option<PathBuf> }`, with `load(&Prefs)` and one setter per field that saves.
  - `NudgeSettings` holds `time: NudgeTime { Grid, Division(Division, bool), Ticks(u32) }`, `pitch_scale_steps: bool`, `amount: u8`, `follow_key_signature: bool`, `root: u8` and `scale: ScaleKind`. Field names in JSON are Swift's (`M/Nudge.swift:24-48`).
  - No consumers yet: nudge, crossfade length and zero-crossing snapping arrive with their features. A backlog row lists them.

- [ ] **Step 1: Write the failing tests.**
  - App, ported from `NudgeTests` :141-217:
    - `nudge_prefs_persist_and_decode_tolerantly`
    - `crossfade_is_kept_and_clamped` (1…1000)
    - `zero_crossings_default_on_and_kept`
    - `a_damaged_field_keeps_its_default`
  - GPUI:
    - `editing_tab_disables_root_and_scale_unless_scale_steps_and_not_following`
    - `ticks_stepper_reads_n_ticks_per_quarter`: "240 ticks (960 per quarter note)", or the value of the app's PPQ.
    - `choose_and_clear_the_external_editor`
- [ ] **Step 2: Run** both test files. Expected: FAIL.
- [ ] **Step 3: Implement** the tab from `S/EditingSettingsView.swift`, at width 460.
  - The external editor is chosen through `file_panels`. On the Mac it is an `.app` in `/Applications`, with the title "Choose External Sample Editor" and Swift's message (§2). On Linux any executable file is accepted, and the line shows the file name.
- [ ] **Step 4: Run** the tests. Expected: PASS.
- [ ] **Step 5: Commit** `settings: the Editing preferences and tab`.

### Task 5: The plug-in settings model and `plugins.*`

**Files:**
- Create: `crates/vizdaw-app/src/plugin_prefs.rs`
- Modify: `crates/vizdaw-app/src/{session.rs,testing.rs,commands.rs,lib.rs}`
- Test: `crates/vizdaw-app/tests/plugin_prefs.rs`

**Interfaces:**
- Produces:
  - New `CoreCalls` methods, each with a `FakeCore` record:
    - `set_new_channel_eq(bool)`
    - `set_default_eq(Option<DefaultEqRow>)`
    - `default_eq_note() -> Option<String>`
    - `bundled_eq_name() -> Option<String>`
    - `bundled_au_present() -> bool`
    - `install_multiq(dest: Option<String>, replace: bool) -> Result<String, VizError>`
    - `eq_choices() -> Vec<DefaultEqRow>`
  - `pub struct PluginPrefs`, with:
    - `load(&Prefs, &dyn CoreCalls)`, which pushes both settings to the core
    - `set_new_channel_eq`, `set_default_eq` and `refresh`
    - `install(replace) -> InstallOutcome { Installed(String), AskReplace, Failed(String) }`
    - `note()` and `status()`
  - `pub fn run_plugins(id, args, prefs_model, prefs, core) -> Result<Ran, ControlError>` for the three `plugins.*` ids.

- [ ] **Step 1: Write the failing tests.** Port `PluginSettingsTests.swift` :39-263 by name:
  - `defaults_on_and_pushes`
  - `persists`
  - `exists_asks_to_replace`
  - `other_failures_say_why`
  - `commands_never_ask`: `plugins.install_multiq` answers `dialog_required` "Replace" instead.
  - `switch_command_sets_and_keeps`
  - `no_bundled_eq_disables`
  - `choice_defaults_to_multiq_and_persists`
  - `damaged_choice_is_multiq_and_removed`
  - `missing_choice_shown_and_said`
  - `default_eq_sets_and_answers`
  - `default_eq_refuses_bad_input`
  - `eq_choices_are_usable_effects_minus_the_multiq_clap`
  - `a_stored_vst3_choice_is_kept` (Review Focus 4)

  Statuses and errors are verbatim from inventory §3.
- [ ] **Step 2: Run** `cargo test -p vizdaw-app --test plugin_prefs`. Expected: FAIL.
- [ ] **Step 3: Implement** from `M/PluginSettings.swift` and `M/PluginSettingsCommands.swift`.
  - `eq_choices` reads the catalog's effect rows: usable (ok or not validated), enabled, a non-empty id, and the Multi-Q CLAP `com.dusk-audio.multi-q` left out.
  - Launch pushes both settings before the first document, next to `apply_all`.
- [ ] **Step 4: Run** the test, then `cargo test -p vizdaw-app`. Expected: PASS.
- [ ] **Step 5: Commit** `plugins: the new-channel EQ settings, pushed at launch, and plugins.*`.

### Task 6: The Plug-ins tab, Multi-Q in the GPUI app, and the plug-in e2e

**Files:**
- Create: `crates/vizdaw-gpui/src/settings_window/plugins.rs`
- Modify: `scripts/make-gpui-app.sh`, which copies `target/multiq/out/Multi-Q.{clap,component}` into `Contents/PlugIns` plus the licences into `Resources/Licences/Multi-Q`, as `scripts/make-app.sh:19-45,177-178` does, ad-hoc signed. If the build output is missing, the script runs `scripts/build-multiq.sh` and stops if that fails.
- Modify: `crates/vizdaw-gpui/src/control.rs`, to dispatch `plugins.*`.
- Test: `crates/vizdaw-gpui/tests/settings_plugins.rs`; `crates/vizctl/tests/e2e_gpui.rs`

**Interfaces:**
- Consumes: Task 5's `PluginPrefs`. The Replace question uses the existing `Ask` sheet, with Swift's strings: "Replace the \(eqName) in your Components folder?", Replace (destructive) and Cancel, and the message "Something is already installed under that name."

- [ ] **Step 1: Write the failing tests:**
  - GPUI:
    - `plugins_tab_with_no_bundled_eq_says_so_and_disables`
    - `the_switch_disables_the_picker`
    - `a_missing_choice_reads_not_available`
    - `install_with_something_there_asks_replace`, using `VIZDAW_BUNDLED_PLUGINS` set to a temp folder holding a fake `Multi-Q.component` directory, and a temp destination.
  - e2e:
    - `gpui_multiq_install_and_switch_over_the_channel`, ported from Swift `e2e.rs:4690-4757`. It launches with `VIZDAW_BUNDLED_PLUGINS=<repo>/target/multiq/out` on the Mac. On Linux, or when that folder is missing, it checks that `plugins.install_multiq` answers `unavailable` "Multi-Q is not in this build of VizDAW." and prints why.
    - In `gpui_new_bus_and_sends_over_the_channel`, the default-EQ steps from Swift `:4772-4780`. Drop the "Not ported" comment.
- [ ] **Step 2: Run** the GPUI test file. Expected: FAIL.
- [ ] **Step 3: Implement** the tab from `S/PluginSettingsView.swift` and inventory §1.8, at width 500.
  - The install runs off the UI thread.
  - The choices refresh when the tab is shown, off the UI thread.
- [ ] **Step 4: Run** the GPUI tests and both e2e tests on the Mac, then build with `scripts/make-gpui-app.sh` and confirm `Contents/PlugIns` has both bundles. Expected: PASS.
- [ ] **Step 5: Commit** `settings: the Plug-ins tab; Multi-Q in the GPUI app; plug-in e2e`.

### Task 7: The transport customize model and `ui.customize_transport`

**Files:**
- Create: `crates/vizdaw-app/src/transport_customize.rs`
- Modify: `crates/vizdaw-app/src/commands.rs`; `crates/vizdaw-gpui/src/menus.rs`, which adds View › "Customize Transport Bar…" and removes the `:612` note.
- Test: `crates/vizdaw-app/tests/transport_customize.rs`

**Interfaces:**
- Produces:
  - `pub struct Customizer { items: Vec<TransportItem>, selected: Option<usize> }`, with:
    - `new(&TransportLayout)`
    - `remove(i) -> bool`, which is false for the cluster
    - `move_left(i)`, `move_right(i)`, `insert(at, item)` and `move_to(from, to)`
    - `palette() -> Vec<TransportItem>`: the built-ins not on the bar, then Flexible Space, Separator, and Command Button (a new one runs `window.mixer`)
    - `set_size(i, Size)`
    - `set_command(i, id)`
    - `set_label(i, &str) -> Result<(), String>`: trimmed, at most 32 characters, with the error "A label is at most 32 characters."
    - `commands(query) -> Vec<(id, title, menu)>`, which matches on the title and the menu
    - `stale(i) -> Option<String>`
    - `finish() -> Result<TransportLayout, String>`
  - `ui.customize_transport` always answers `dialog_required` "Customize Transport Bar".

- [ ] **Step 1: Write the failing tests.** Port `TransportCustomizeTests.swift` :37-263 by name:
  - `cannot_remove_the_cluster`
  - `ops_keep_order`
  - `palette_is_what_is_not_on_the_bar`
  - `picker_by_title_and_menu`
  - `labels_trimmed_and_bounded`
  - `stale_commands_greyed`
  - `availability_follows_menus`
  - `customize_transport_needs_a_dialog`
  - `an_invalid_layout_is_never_written`
  - `distinct_command_buttons`
- [ ] **Step 2: Run** `cargo test -p vizdaw-app --test transport_customize`. Expected: FAIL.
- [ ] **Step 3: Implement** from `M/TransportCustomize.swift` and `M/CommandButtons.swift:80-123`.
- [ ] **Step 4: Run** the test. Expected: PASS.
- [ ] **Step 5: Commit** `transport: the customize model and ui.customize_transport`.

### Task 8: The Customize Transport Bar sheet

**Files:**
- Create: `crates/vizdaw-gpui/src/sheets/customize_transport.rs`
- Modify: `crates/vizdaw-gpui/src/{shell.rs,sheets/mod.rs}`, so that View › Customize Transport Bar… opens it in the front window's `SheetHost`, document or mixer, one at a time.
- Test: `crates/vizdaw-gpui/tests/customize_sheet.rs`

**Interfaces:**
- Consumes: Task 7's `Customizer`, `SheetHost`'s `Sheet` trait (width 760), and the `ui.set_transport_bar` write path (`Prefs::set_transport_bar`, then every bar redraws).

- [ ] **Step 1: Write the failing tests:**
  - `sheet_shows_bar_inspector_and_palette`: title "Customize Transport Bar" and the caption from inventory §1.10.
  - `double_click_in_the_palette_adds_to_the_bar`
  - `drag_along_the_bar_reorders`
  - `drag_to_the_palette_removes`
  - `inspector_sets_size_and_command_and_label`
  - `a_long_label_is_refused_and_the_sheet_stays`
  - `cancel_writes_nothing` (Review Focus 5)
  - `done_writes_and_every_bar_redraws`
  - `reset_to_default`
- [ ] **Step 2: Run** `cargo test -p vizdaw-gpui --test customize_sheet`. Expected: FAIL.
- [ ] **Step 3: Implement** from `S/TransportCustomizeSheet.swift`.
  - Chips are drawn in the bar's own faces.
  - The palette is a grid with columns at least 140 wide.
  - Drag and drop uses GPUI `on_drag` and `on_drop` with a small drag-value type.
  - Return is Done and Esc is Cancel.
  - Each chip has the accessible actions "Remove", "Move Left" and "Move Right", recorded in the registry scope.
- [ ] **Step 4: Run** the tests and `theme_editors`. Expected: PASS.
- [ ] **Step 5: Commit** `transport: the Customize Transport Bar sheet`.

### Task 9: Checks, checklist and backlog

**Files:**
- Modify:
  - `docs/superpowers/specs/2026-11-22-parity-checklist.md`
  - `docs/feature-backlog.md`
  - `docs/testing/manual-checks.md`
  - `docs/testing/controller-checks.md`
  - `README.md`, where the Settings line gains Keys, Editing and Plug-ins

- [ ] **Step 1:** Run the whole suite: `cargo test --workspace`, clippy, fmt, and every e2e named in this plan on the Mac. Expected: all green.
- [ ] **Step 2:** Tick the 9c Mac cells in the checklist. The Linux cells are ticked after CI, as in 9b.
- [ ] **Step 3:** Add the backlog rows:
  - Learn MIDI in Keys, waiting for Surfaces.
  - Consumers for nudge, crossfade length, zero-crossing snapping and the external editor.
  - The bundled EQ on Linux.
  - Each task's review leftovers.
- [ ] **Step 4:** Add owner rows to manual-checks: on macOS, rebinding a key and seeing the menu change, and installing Multi-Q; on Linux, the Keys tab and the Customize sheet. Add on-screen rows to controller-checks.
- [ ] **Step 5: Commit** `Plan 9c: checks, checklist, backlog`.
