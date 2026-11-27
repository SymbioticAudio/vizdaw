# 27.1.0 Slice 10a: Plug-ins (browser, inserts, EQ, Plugin Manager, launch scan) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The GPUI app can find plug-ins and put them on strips, as the Swift app did.
- The app scans for new or changed plug-ins at launch (unless `--no-scan` or `--smoke`) and applies the results while it runs.
- A strip's "+" and "EQ ＋" open the plug-in browser. "Replace…" works. Clicking an instrument slot or the Built-in Synth slot opens the instrument menu.
- The mixer bar has "Add Track" and "Instruments" menus. The right pane's "Plug-ins" view is a browser.
- The Plugin Manager window (⌥⌘P) lists, filters, enables/disables, rescans and reveals plug-ins.
- `mixer.add_insert`, `remove_insert`, `move_insert`, `set_bypass`, `set_eq`, `set_param`, `track.new_plugin_instrument` and `window.plugin_manager` work over the control channel.

**Architecture:**
- The core is complete: the catalog in `core-ffi/src/catalog.rs:367-515` and the session's slot calls in `core-ffi/src/session.rs:3394-3705`. The slice reaches them through the app's two seams, as 9a–9d did:
  - The process-wide catalog calls are a new `PluginCatalogCalls` trait in `vizdaw-app/src/plugins.rs`, with a live impl over `live_catalog()` (`session.rs:1239`) and a `FakeCatalog` in `testing.rs` (ported from `app/Tests/VizDAWAppModelTests/FakeCatalog.swift`). `CoreCalls::eq_choices` and `disable_plugins` keep working and read the same catalog.
  - The per-document calls `add_insert`, `add_plugin_instrument` and `set_destination_plugin` join `DocSession`, recorded by `FakeSession`.
- `vizdaw-app` gets:
  - `plugins.rs`: `AppPlugins` (ports `M/AppPlugins.swift`): the open, the automatic scan, the poll, and the alert queue.
  - `plugin_browser.rs`: `PluginBrowserModel` (ports `M/PluginBrowser.swift`).
  - `mixer/browser.rs`: `BrowserTarget`, `pick`, `instrument_menu`, `pick_instrument` (ports `M/MixerModel.swift:124-140,314-319,452-568`).
  - `plugin_manager.rs`: `PluginManagerModel` (ports `M/PluginManagerModel.swift`).
- GPUI gets `plugin_browser.rs` (the popover view), `plugin_manager_window.rs`, live "+" / "EQ ＋" / "Replace…" / instrument menu in `mixer_view.rs`, the bar menus, the right-pane browser, a `PluginManager` window kind, and an app-level 4 Hz catalog poll on the existing app timer.

**Tech Stack:** Rust 2024; GPUI at the slice 1 pin; `core-ffi` (`catalog`, `session`), `core-control`, `vizctl`, `test-plugins`.

**Spec:** `docs/superpowers/specs/2026-11-20-cross-platform-27-1-design.md`.
- Parity rows in `docs/superpowers/specs/2026-11-22-parity-checklist.md`: `window.plugin_manager` (:203), `track.new_plugin_instrument` (:227), `mixer.add_insert`…`mixer.set_param` (:245-250), the Mixer window note (:389), "Plugin Manager" (:392), README "Plugins (plan 3c)" (:501), Linux gap "AU inserts in Mac projects" (:599).
- **Recon with every Swift file:line and exact string:** `.superpowers/sdd/plugin-slice-recon.md` (git-ignored; sections 1a–1e Swift, 2 control, 3 core API, 4 GPUI today, 5 e2e, 6 Linux). Implementers read the section their task names. `S/` = `app/Sources/VizDAW/`, `M/` = `app/Sources/VizDAWAppModel/`.

## Global Constraints

- **Rules from slices 7a–9d that still apply:**
  - The Swift app is retired: match its behaviour; don't keep compatibility with it.
  - Use Swift's exact strings, citing file:line (the recon has them).
  - The UI says "track", never "lane".
  - No banned vendor or DAW product names in repo text. Multi-Q and licence notices are fine.
  - Nothing slow on the UI thread: `scan`, `rescan` and `clear_blocklist` only start the background pool, so they may run on it; `progress()` **must** run on the main thread (`core-ffi/src/catalog.rs:482`). `rows()` and `browser()` read memory and may run on it.
  - GPUI tests press `secondary-` keys.
  - Linux CI runs the GPUI tests and the e2e on the offline backend, and every 10a test must pass there. A Mac-only step is skipped on Linux with a line saying why.
- **Before every commit:** `export PATH="$HOME/.cargo/bin:$PATH"`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and the tests the task names. CI stops at Format.
- **Running the e2e on the Mac:** `cargo build -p vizdaw-gpui -p vizdaw-scan`, then `VIZDAW_E2E_APP=target/debug/vizdaw-gpui VIZDAW_AUDIO=offline cargo test -p vizctl --test e2e_gpui -- --ignored --exact <name>`.
- **Seams:** no app or GPUI unit test opens the real catalog, writes `plugins.json` or starts `vizdaw-scan`. They use `FakeCatalog` and `FakeSession`.
- **Formats:** a VST3 plug-in reads "VST3" everywhere (fixes Swift's "CLAP" label, `S/PluginBrowserView.swift:34`, `M/PluginManagerModel.swift:35`). AU reads "AU", CLAP "CLAP". The Manager's format filter offers All/AU/CLAP/VST3 on macOS and All/CLAP/VST3 on Linux.
- **Control replies:** every new verb answers at once. No control verb opens an alert or a popover; a refusal is the reply (`failed`/`busy`/`unavailable` as for the existing strip edits, `A/mixer/commands.rs:368-380`). Catalog alerts raised while the control channel drives the app go to its notes, as Swift's `ControlChannel.note` did.
- **Toggling a plug-in off unloads it at once** (owner rule): `set_enabled(false)` is followed by the existing idle (`A/document.rs:414`) on every open document, so the slot becomes "Disabled in Plugin Manager" within one tick.
- **The look:** theme tokens only (`appearance::current()`, `strip_palette::tokens()`); `no_source_file_keeps_a_colour_literal` stays green. Sizes (Swift): the browser popover is 320 wide with a list at least 240 high and an empty line at least 80 high (`S/PluginBrowserView.swift:11,44,19-22`); the Plugin Manager is at least 760×360 (`S/PluginManagerView.swift:61`) with column widths On 30, Format 50, Version 70, Type 80.
- **Out of scope:** Co-Producer and MCP (`co_producer.plugins`, the `co_producer_mixer_over_mcp` e2e), the Console look, user themes, H3, a Linux bundled EQ.
- **Review pace:** fix crashes, data loss and wrong overwrites; log other edge cases to the backlog.

## Review Focus

1. **A scan finishes while a browser is open or a plug-in is being picked.** The open browser keeps its rows until it is closed and reopened (Swift cached it per presentation, `M/MixerModel.swift:173-186`); a pick of a plug-in the scan just marked Missing gets the core's refusal as the "Add Plugin Failed" alert, and nothing panics. Test: `a_pick_after_the_plugin_went_missing_is_an_alert` (Task 3).
2. **The plug-in in use is turned off in the Plugin Manager.** Its slots in every open document become "Disabled in Plugin Manager" placeholders at the next tick, keeping their settings; turning it back on loads them. Tests: `disabling_reaches_the_catalog_and_idles_every_document` (Task 6) and e2e `gpui_plugin_manager_lists_toggles_and_rescans` (Task 8).
3. **No scanner next to the app** (a bare `cargo run`, or a Linux build without `vizdaw-scan`). The launch scan raises one "Scan Failed" alert with the core's text and the app keeps running; "Rescan All" raises it again; nothing repeats every poll. Test: `a_missing_scanner_is_one_alert` (Task 2).
4. **The progress poll only runs on the main thread, and only while needed.** It runs at 4 Hz while a scan runs or the Manager is open, refreshes rows only when `done`/`running` changed, and stops when the scan ends and the Manager is closed. Test: `the_poll_refreshes_only_on_change` (Task 2).
5. **Picking the instrument already loaded, or an EQ already in the slot, sends nothing**; replacing an instrument renames the strip and the track's destination at once and opens the new plug-in's window. Tests: `picking_the_loaded_instrument_or_on_a_non_instrument_strip_does_nothing`, `picking_the_eq_already_in_the_slot_sends_nothing`, `replacing_the_instrument_renames_the_strip_and_the_track_destination_at_once` (Task 3).

---

### Task 1: The catalog seam and the new session calls

**Files:**
- Create: `crates/vizdaw-app/src/plugins.rs` (the trait and its live impl only; Task 2 adds `AppPlugins`)
- Modify: `crates/vizdaw-app/src/{session.rs,testing.rs,lib.rs}`
- Test: `crates/vizdaw-app/tests/plugin_catalog.rs`

**Interfaces:**
- Produces:
  - `pub trait PluginCatalogCalls: Send + Sync` with, mirroring `core-ffi/src/catalog.rs:392-515` (recon §3):
    `take_problem() -> Result<Option<String>, VizError>`, `rows() -> Result<Vec<CatalogRowFfi>, VizError>`, `browser(kind: PluginKindTag) -> Result<Vec<CatalogRowFfi>, VizError>`, `set_enabled(key: PluginKey, enabled: bool) -> Result<(), VizError>`, `clear_blocklist(keys: Vec<PluginKey>)`, `rescan(keys: Vec<PluginKey>)`, `scan(all: bool)`, `progress() -> Result<ScanProgress, VizError>`, `reveal_path(key: PluginKey) -> Result<Option<String>, VizError>`.
  - `pub fn live_plugin_catalog() -> Arc<dyn PluginCatalogCalls>`: wraps `live_catalog()`; when the catalog could not open, every call returns that open error.
  - `pub fn format_label(PluginFormatTag) -> &'static str`: "AU", "CLAP", "VST3".
  - `pub fn plugin_key(&PluginRefRow) -> PluginKey`.
  - `DocSession` gains `add_insert(strip: u64, index: Option<u32>, plugin: PluginRefRow) -> Result<DocSnapshot, VizError>`, `add_plugin_instrument(name: String, plugin: PluginRefRow)`, `set_destination_plugin(dest: u64, plugin: PluginRefRow)`, each a one-line live impl next to `remove_insert` (`session.rs:667`).
  - `testing.rs`: `FakeCatalog` (recording: `scans: Vec<bool>`, `rescans`, `cleared`, `enabled_calls: Vec<(PluginKey, bool)>`, `revealed`; scripted: `rows`, `progress` queue, `problems` queue, per-call `fail_*: Option<VizError>`) and `catalog_row(name, vendor, format, kind, status)` (ports `FakeCatalog.swift:66`). `FakeSession` records `add_inserts`, `plugin_instruments`, `destination_plugins` and can script a refusal for each.
- [ ] **Step 1: Write the failing tests:** `format_labels_name_vst3_as_vst3`, `fake_catalog_records_every_call`, `fake_session_records_the_new_slot_calls`, `a_catalog_that_cannot_open_refuses_every_call` (live wrapper built over an `Err`; keep the constructor taking the open result so it is testable without touching disk).
- [ ] **Step 2: Run** `cargo test -p vizdaw-app --test plugin_catalog`. Expected: FAIL (missing items).
- [ ] **Step 3: Implement.** `eq_choices` (`session.rs:1087`) and `disable_plugins` (`:1210`) keep their behaviour.
- [ ] **Step 4: Run** the test, then `cargo test -p vizdaw-app`. Expected: PASS.
- [ ] **Step 5: Commit** `plugins: the catalog seam and the add-insert and instrument session calls`.

### Task 2: AppPlugins, the launch scan and the poll

**Files:**
- Modify: `crates/vizdaw-app/src/plugins.rs`, `crates/vizdaw-gpui/src/{lib.rs,control.rs,app_state.rs,alerts.rs}`
- Test: `crates/vizdaw-app/tests/app_plugins.rs`; a GPUI test in `crates/vizdaw-gpui` for the poll wiring

**Interfaces:**
- Consumes: Task 1's `PluginCatalogCalls`.
- Produces: `pub struct AppPlugins` (one per process, in the GPUI app state), porting `M/AppPlugins.swift` (recon §1d):
  - `new(catalog: Arc<dyn PluginCatalogCalls>)`; `start(scan: bool)`: reads `take_problem` (alert "Plugins Unavailable"), loads the rows, then `scan_new()` when `scan`.
  - `scan_new()` = `scan(false)`, silent when the core says a scan is already running; `scan_all()` = `scan(true)`; `rescan(keys)`; `clear_blocklist(keys)`; `set_enabled(key, on) -> bool`.
  - `poll() -> bool`: calls `progress()`, refreshes `rows()` when `running` or `done` changed (`M/AppPlugins.swift:81-87`), drains `take_problem`, returns whether anything changed. `needs_poll() -> bool` = a scan is running.
  - `rows()`, `browser(kind) -> Vec<CatalogRowFfi>`, `progress() -> ScanProgress`.
  - Alerts: `next_alert() -> Option<(String, String)>`, a FIFO dropping duplicates; titles verbatim "Plugins Unavailable", "Plugin Settings", "Scan Failed", "Rescan Failed" (`M/AppPlugins.swift:15-23,124-135`). `VizError::Busy` never alerts.
- GPUI:
  - Launch (`lib.rs:347-350`): build `AppPlugins` over `live_plugin_catalog()` and `start(scan)` where `scan` is false under `--no-scan` or `--smoke`. `LaunchOptions` (`control.rs:1701-1735`) now records `no_scan`.
  - The app timer polls `AppPlugins` at 4 Hz while `needs_poll()` or the Plugin Manager window is open (Task 6 sets that flag), on the main thread. A poll that changed anything notifies the open Manager window.
  - Catalog alerts show as app alerts on the front window (or go to the control channel's notes when it drives the app, as the existing alerts do in `alerts.rs`). Remove the `eprintln!`.
- [ ] **Step 1: Write the failing tests.** Port `PluginBrowserTests.swift` by name: `start_opens_scans_and_shows_a_problem_once` (:43), `a_finished_scan_refreshes_the_rows` (:60), `actions_reach_the_catalog_and_errors_become_alerts` (:72), `set_enabled_reports_whether_the_catalog_took_it` (:90), `a_catalog_that_cannot_open_is_an_alert` (:100), `a_problem_and_a_failed_scan_are_both_shown_in_order` (:108), `a_repeated_problem_is_queued_once` (:121), `busy_never_alerts_and_automatic_running_scan_is_silent` (:131). New: `start_without_scan_scans_nothing`, `a_missing_scanner_is_one_alert` (Review Focus 3: a scripted `NotFound` from `scan` gives one "Scan Failed" alert; ten polls add none), `the_poll_refreshes_only_on_change` (Review Focus 4: `rows()` call count rises only when the scripted progress changes; `needs_poll` is false after `running` goes false). GPUI: `no_scan_is_honoured_at_launch`.
- [ ] **Step 2: Run** `cargo test -p vizdaw-app --test app_plugins`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the tests, `cargo test -p vizdaw-app` and `cargo test -p vizdaw-gpui`. Expected: PASS.
- [ ] **Step 5: Commit** `plugins: scan at launch and apply results while the app runs`.

### Task 3: The browser model, picks and the instrument menu

**Files:**
- Create: `crates/vizdaw-app/src/plugin_browser.rs`, `crates/vizdaw-app/src/mixer/browser.rs`
- Modify: `crates/vizdaw-app/src/mixer/{mod.rs,model.rs,slots.rs}`
- Test: `crates/vizdaw-app/tests/plugin_browser.rs`

**Interfaces:**
- Consumes: Task 1's session calls, Task 2's `AppPlugins::browser`.
- Produces (recon §1a–1b):
  - `PluginBrowserModel { kind, rows }` with `groups(search: &str) -> Vec<(String, Vec<CatalogRowFfi>)>`: vendors case-insensitive, an empty vendor is "Other" and sorts last, plug-ins sort by name, search trimmed and matches name, vendor or id in any case (`M/PluginBrowser.swift:32-53`). Strings: `NO_PLUGINS` = "No plugins found. Scan in Window > Plugin Manager.", `NO_MATCH` = "No match." (`S/PluginBrowserView.swift:19-22`), `SEARCH` = "Search".
  - `pub enum BrowserTarget { Insert { strip: u64 }, Instrument, Destination(u64), Eq { strip: u64 } }` with `wants_instrument()`.
  - `MixerModel`: `open_browser(target, placement)`, `browser() -> Option<&PluginBrowserModel>` (cached while open, reset on close), `close_browser()`, `pick(row) -> Option<u64>` returning the slot whose window should open. Alert titles verbatim: "Add Plugin Failed", "New Instrument Failed", "Instrument Failed", "Load EQ Failed" (`M/MixerModel.swift:452-472`); "Clear EQ Failed" stays as is.
  - `instrument_menu(rows, current: Option<&PluginRefRow>) -> InstrumentMenu` (makers ▸ plug-ins with `checked`, or the disabled "No Plugin Instruments"; enabled `.ok` instruments only), with "Open Editor" first (disabled with no editor) (`M/MixerModel.swift:487-501`, `S/MixerView.swift:356-391`).
  - `pick_instrument(row, on strip) -> Option<u64>`: nothing when it is the loaded plug-in or the strip is not an instrument strip; else `set_destination_plugin` on `instrument_destination(strip)` (`M/MixerModel.swift:510-562`).
  - Each successful pick applies its snapshot as one undo step with the core's label.
- [ ] **Step 1: Write the failing tests.** Port by name: `groups_by_vendor_with_other_last` (PluginBrowserTests :16), `search_matches_name_vendor_or_id_in_any_case` (:22), `the_model_lists_its_kind_only` (:29); from MixerModelTests: `the_browser_pick_goes_where_it_was_opened` (:166), `the_instrument_menu_groups_by_maker_and_marks_the_current_plugin` (:182), `the_instrument_menu_leaves_out_effects_and_disabled_plugins` (:197), `picking_an_instrument_replaces_it_in_that_destination` (:211), `replacing_the_instrument_renames_the_strip_and_the_track_destination_at_once` (:223), `a_pick_returns_the_new_slot_to_open_and_a_failure_returns_none` (:252), `picking_the_loaded_instrument_or_on_a_non_instrument_strip_does_nothing` (:266), `a_destinations_instrument_is_replaced_unless_it_is_the_loaded_one` (:324), `a_failing_instrument_pick_alerts` (:340), `browser_models_are_reused_while_open_and_reset_on_close` (:383); from MixerStripTests: `the_eq_slot_loads_bypasses_and_empties` (:100), `picking_the_eq_already_in_the_slot_sends_nothing` (:160). New: `a_pick_after_the_plugin_went_missing_is_an_alert` (Review Focus 1), `a_vst3_row_reads_vst3`.
- [ ] **Step 2: Run** `cargo test -p vizdaw-app --test plugin_browser`. Expected: FAIL.
- [ ] **Step 3: Implement.** Update the "out of this slice" note at `mixer/slots.rs:11-15`.
- [ ] **Step 4: Run** the test, then `cargo test -p vizdaw-app`. Expected: PASS.
- [ ] **Step 5: Commit** `mixer: the plug-in browser model, picks and the instrument menu`.

### Task 4: The browser popover and the strip's slots

**Files:**
- Create: `crates/vizdaw-gpui/src/plugin_browser.rs`
- Modify: `crates/vizdaw-gpui/src/mixer_view.rs` (`add_face` 2503-2524, `eq_slot` 2526+, `inserts` 2619-2665, `slot_holder` 2414-2468, `builtin_synth_slot` 2470-2501, `slot_entries` 3154-3205, the note at 15-19)
- Test: GPUI tests next to the mixer view's existing ones

**Interfaces:**
- Consumes: Task 3's model.
- Produces:
  - `plugin_browser::view(model, search: Entity<TextInput>, on_pick)`: a 320-wide popover anchored to its button, as `record_popover.rs` anchors: the "Search" field (`text_input.rs`, focused on open), then one section per vendor with name left and `format_label` right, min list height 240; empty lines per Task 3. Esc or a click outside closes it. Enter picks when exactly one row matches.
  - "+" (help "Add an effect", a11y "Add an effect to ‹strip›") opens it for `Insert`; "EQ ＋" (help "Add an EQ", a11y "EQ of ‹strip›" value "empty") for `Eq`, both with the effect browser. Dropping a slot on "+" still moves it to the end.
  - "Replace…" in the EQ menu reopens the browser for `Eq` (`S/ChannelStrip.swift:657-677`).
  - A single click on an instrument strip's own slot, or on the Built-in Synth slot, opens the instrument menu at the pointer (texts and a11y from recon §1b). A pick opens the new slot's plug-in window via `plugin_windows::show_later`.
- [ ] **Step 1: Write the failing GPUI tests:** `the_plus_opens_the_effect_browser_and_a_pick_adds_the_insert` (FakeSession records `add_inserts` with the strip), `the_eq_plus_opens_the_browser_for_the_eq`, `replace_reopens_the_eq_browser`, `clicking_the_instrument_slot_opens_the_instrument_menu`, `escape_closes_the_browser_without_a_pick`, `the_browser_says_no_plugins_found_when_empty`.
- [ ] **Step 2: Run** `cargo test -p vizdaw-gpui mixer`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p vizdaw-gpui`. Expected: PASS.
- [ ] **Step 5: Commit** `mixer: the plug-in browser on the strip's inserts, EQ and instrument slots`.

### Task 5: The mixer bar's menus and the right-pane browser

**Files:**
- Modify: `crates/vizdaw-gpui/src/{mixer_view.rs (bar 1724-1760),panes.rs (770-800)}`, `crates/vizdaw-app/src/{view.rs or panels.rs}`
- Test: GPUI tests; `crates/vizdaw-app/tests/plugin_browser.rs` for the pane target rule

**Interfaces:**
- Consumes: Tasks 3–4.
- Produces (recon §1b "Mixer bar", §1a "Right pane"):
  - The bar's "Add Track" menu: "New Audio Track", "New Instrument Track", "New MIDI Track" (disabled with help "Select a sequence first", else help "Add a track"), running the existing track commands.
  - The bar's "Instruments" menu: "New Plugin Instrument…" opens the instrument browser for `Instrument`; a divider; one item per destination "‹name›: ‹plug-in or "MIDI"› → Plugin…" for `Destination(id)`.
  - The right pane's "Plug-ins" view (replacing "Plug-ins comes in a later version.") is the browser at the pane's width; a pick goes on the end of the target strip per `M/PaneContent.swift:233-253`, else the status line "Select a track or one mixer strip first."; with no mixer, "The mixer of this document could not be read.".
  - `pane_insert_target(doc) -> Result<u64, &'static str>` in vizdaw-app.
- [ ] **Step 1: Write the failing tests:** `the_pane_target_is_the_selected_strip_else_the_left_pane_strip`, `the_pane_pick_without_a_strip_says_select_a_track` (app); GPUI `the_instruments_menu_lists_new_plugin_instrument_and_each_destination`, `add_track_is_disabled_without_a_sequence`.
- [ ] **Step 2: Run** them. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p vizdaw-app` and `cargo test -p vizdaw-gpui`. Expected: PASS.
- [ ] **Step 5: Commit** `mixer: Add Track and Instruments menus; the Plug-ins pane browses`.

### Task 6: The Plugin Manager window

**Files:**
- Create: `crates/vizdaw-app/src/plugin_manager.rs`, `crates/vizdaw-gpui/src/plugin_manager_window.rs`
- Modify: `crates/vizdaw-gpui/src/{menus.rs (706-708),app_state.rs (WindowKind),lib.rs}`
- Test: `crates/vizdaw-app/tests/plugin_manager.rs`; GPUI tests

**Interfaces:**
- Consumes: Task 2's `AppPlugins`.
- Produces (recon §1c):
  - `PluginManagerModel`: `rows(filter: FormatFilter, search) -> Vec<ManagerRow>` (id `"<format>:<id>|<path>"`; status text "OK", "Disabled", "Blocklisted: ‹reason›", "Not validated", "Missing"; type "Instrument"/"Effect"/"—"), `FormatFilter::{All, Au, Clap, Vst3}` with `FormatFilter::available()` per platform, selection, `progress_text()` = "Scanning ‹done› of ‹total›…" while running, `can_rescan_selected()`, `can_rescan_all()`, `can_clear_blocklist()`, `can_reveal()`, and intents `set_enabled`, `rescan_selected`, `rescan_all`, `clear_blocklist`, `reveal` (via `AppPlugins`).
  - `set_enabled` is followed by `idle()` on every open document (owner rule: unload at once).
  - Reveal: macOS "Reveal in Finder" (`open -R <path>`); Linux "Show in Files" (`xdg-open <parent folder>`), run off the UI thread.
  - GPUI window: `WindowKind::PluginManager`, one per app, titled "Plugin Manager", min 760×360; the table (On toggle 30 wide and disabled without a plug-in, Name, Vendor, Format 50, Version 70, Type 80, Status coloured: blocklisted `state.error`, OK `text_primary`, else `state.warning`; the row's tooltip is the path), multi-select with click/shift/⌘-click, the search field, the format picker, the progress text, the four buttons, and the footer "Turning a plugin off unloads it from all open projects; its settings are kept.". Window menu "Plugin Manager" with `window.plugin_manager`'s ⌥⌘P from the registry. Opening sets the poll flag of Task 2; closing clears it.
- [ ] **Step 1: Write the failing tests.** Port `PluginManagerTests.swift` by name: `rows_show_the_spec_statuses_and_filter` (:21), `disabling_and_enabling_reach_the_catalog` (:32), `selection_drives_rescan_clear_and_reveal` (:43), `re_enabling_works_and_a_blocklisted_row_can_be_switched_off_and_stays_off` (:63), `selecting_several_blocklisted_plugins_clears_them_in_one_call_without_an_alert` (:79), `busy_during_a_toggle_raises_no_alert_and_keeps_the_row` (:95). New: `the_format_filter_offers_vst3_and_au_only_on_macos`, `disabling_reaches_the_catalog_and_idles_every_document` (Review Focus 2). GPUI: `the_window_menu_opens_one_plugin_manager`, `the_manager_shows_scanning_progress`.
- [ ] **Step 2: Run** `cargo test -p vizdaw-app --test plugin_manager`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p vizdaw-app` and `cargo test -p vizdaw-gpui`. Expected: PASS.
- [ ] **Step 5: Commit** `plugins: the Plugin Manager window`.

### Task 7: The control verbs

**Files:**
- Modify: `crates/vizdaw-app/src/{mixer/commands.rs (68-75,194-215,368-380),commands.rs (66)}`, `crates/vizdaw-gpui/src/control.rs` (dispatch, `state.get windows`, the unavailable list 1650-1670)
- Test: `crates/vizdaw-app/tests/mixer_commands.rs` (existing file or its equivalent); GPUI control tests

**Interfaces:**
- Consumes: Tasks 1, 6.
- Produces (recon §2):
  - `mixer.add_insert`, `remove_insert`, `move_insert`, `set_bypass`, `set_eq`, `set_param` route through `strip_edit` → `session.run_mixer(id, args)`; reply `status` is the registry title ("Add Insert", "Remove Insert", "Move Insert", "Bypass Insert", "Set EQ", "Set Plug-in Parameter"); the core's refusal text is the `failed` reply verbatim (e.g. "No installed plug-in is clap:vizdaw.e2e.none."); an open gesture is `unavailable`.
  - `track.new_plugin_instrument`: `seq` defaults to the selected sequence, `after` to the selection's first track, else the end; reply status is the new track's name; the new track is selected (`M/MixerCommands.swift:107-138`).
  - `window.plugin_manager` (app-wide, no document) opens or fronts the window; `state.get windows` lists it as kind `"plugin_manager"`.
  - The verbs leave the "There is no command…" list.
- [ ] **Step 1: Write the failing tests:** `add_insert_over_the_channel_runs_the_core_mixer_edit`, `each_plugin_verb_replies_with_its_registry_title`, `an_unknown_plugin_is_the_cores_refusal`, `a_new_plugin_instrument_track_goes_below_the_selection` (ports MixerCommandsTests :164), `window_plugin_manager_opens_one_window_listed_as_plugin_manager`.
- [ ] **Step 2: Run** them. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p vizdaw-app` and `cargo test -p vizdaw-gpui`. Expected: PASS.
- [ ] **Step 5: Commit** `control: the plug-in mixer verbs, new plug-in instrument track, Plugin Manager`.

### Task 8: The e2e and CI

**Files:**
- Modify: `crates/vizctl/tests/e2e_gpui.rs` (reuse `seed_catalog` ~4315, `plugin_ref` 4356, `insert_of` 4418), `.github/workflows/ci.yml` (every `cargo build -p vizdaw-gpui` line also builds `-p vizdaw-scan`)

- [ ] **Step 1: Write the e2e tests:**
  - `gpui_plugin_inserts_and_eq_over_the_channel`: seed the catalog with `test_plugins::bundle()`; `ui.click "Add an effect to ‹strip›"`, pick the test plug-in in the browser by its a11y row, and `state.get mixer` shows the insert; then over the channel `mixer.set_bypass`, `mixer.move_insert`, `mixer.set_param`, `mixer.remove_insert`, `mixer.set_eq plugin=multi-q` (Mac only: Multi-Q is AU; on Linux use the test plug-in's ref and say so), `mixer.add_insert plugin=clap:vizdaw.e2e.none` → the core's refusal text, and `track.new_plugin_instrument plugin=built-in-synth` → the new track's name.
  - `gpui_plugin_manager_lists_toggles_and_rescans`: copy the test CLAP bundle into the throwaway home's CLAP folder (`Library/Audio/Plug-Ins/CLAP` on Mac, `.clap` on Linux), launch (the launch passes `--no-scan`), `window.plugin_manager` → kind `plugin_manager` in `state.get windows`; `ui.click "Rescan All"`, wait until the Manager lists the test plug-in as "OK"; with it on a strip, switch it off with its "On" toggle and see the slot's placeholder "Disabled in Plugin Manager" at the next `state.get mixer`; switch it on and see it load.
- [ ] **Step 2: Run** both on the Mac (Global Constraints command). Expected: PASS. Fix the app, not the test, when a step fails for an app reason.
- [ ] **Step 3: Commit** `e2e: plug-in inserts, EQ, the Plugin Manager and a real scan; CI builds vizdaw-scan`.

### Task 9: Checks, checklist and backlog

**Files:**
- Modify: `docs/superpowers/specs/2026-11-22-parity-checklist.md`, `docs/testing/manual-checks.md`, `docs/feature-backlog.md`, `app/README.md` if it names the GPUI gaps

- [ ] **Step 1:** Tick the Mac cells "S10a" of rows 203, 227, 245-250, 392, 501, and extend row 389's note with "slots with the browser". Leave the Linux cells for the post-CI tick. Note at 599 that AU inserts stay placeholders (the core's behaviour).
- [ ] **Step 2:** Add manual checks: M26 (Mac: launch scan finds the installed AU/CLAP/VST3 plug-ins; browser and Manager on screen; Reveal in Finder), L32 (Linux: the browser and Manager on screen under XWayland; Show in Files).
- [ ] **Step 3:** Update the backlog row "Plug-in browser, adding inserts and EQ, and a plug-in scan at launch in the GPUI app" to done in 10a; add any deferred minors.
- [ ] **Step 4: Commit** `docs: slice 10a checklist, manual checks and backlog`.

## Rulings made while planning

1. **VST3 reads "VST3"** and the Manager's filter has a VST3 choice. Swift's "CLAP" label for VST3 was a bug. Cost if wrong: one label.
2. **Linux reveal is "Show in Files"**, opening the parent folder with `xdg-open`; Swift had no Linux precedent. Cost if wrong: a button title.
3. **The e2e's real scan runs from "Rescan All"**, not at launch: `vizctl launch` always passes `--no-scan`, and the launch path is covered by `no_scan_is_honoured_at_launch` and `start_opens_scans_and_shows_a_problem_once`. CI now builds `vizdaw-scan` so the scanner sits next to `target/debug/vizdaw-gpui`. Cost if wrong: an untested launch-only wiring bug.
4. **The 4 Hz poll runs only while a scan runs or the Manager is open.** Swift polled at 4 Hz always; nothing else changes the catalog. Cost if wrong: a late row refresh after a change from outside the app.
5. **The popover is a GPUI overlay anchored like `record_popover.rs`**, with Enter picking a single match (new, harmless).
6. **"Remove from VCA" on the track header and the VCA offset display stay in the backlog**; this slice is plug-ins only.

## Outcome (2026-11-27)

All nine tasks landed on `next`, each with a clean task review. The final whole-branch review found no Critical or Important issues; its four minors and every task review's minors are in the backlog row "GPUI slice 10a: review leftovers".

### Rulings

1. `MixerModel::open_browser` takes `&AppPlugins` as a third argument, so the browser's rows come from the caller when it opens (Task 3).
2. Picking a destination's loaded plug-in again sends nothing (Swift sent it), as Review Focus 5 asks for the instrument slot (Task 3).
3. A pick from the instrument menu opens the new slot's plug-in window. A browser pick for an insert, an EQ, a new plug-in instrument track or a destination opens none, as in Swift (Tasks 4–5).
4. The e2e found three app bugs, each fixed with a test that failed first (Task 8):
   - every slot verb refused a song's first slot, id 0;
   - `state.get mixer` slots now report `status` and `reason`;
   - `ui.key` reaches the Plugin Manager, and its toolbar keeps Search inside the window at 760 wide.
5. Ruling 4 above (poll only while needed) stands. Crash recovery's Disable also changes the catalog; the stale Manager rows that causes are logged in the backlog.
6. MCP and Co-Producer plug-in paths stay out of scope. MCP's `plugin_params` still refuses slot 0 (backlog).
