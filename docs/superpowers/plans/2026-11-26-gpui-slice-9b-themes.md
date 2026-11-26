# 27.1.0 Slice 9b: Appearance and the Six Themes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every window of the GPUI app draws from one current theme, and the theme can be chosen in Settings › Appearance or with `ui.theme`. The theme is one of the six built-ins (owner, 2026-11-25): Classic Light (the default), Studio Dark, Flat, Console, Symbiotic and EG13. The choice applies at once to every window. It is a preference of the machine, never saved in projects, as in Swift. "Show channel colour bars" works. `state.get windows` reports the real theme, and `themes_and_the_strip_over_the_channel` is ported.

**Not in 9b, logged in the backlog for a later slice:**
- The theme *styles* beyond corners: label case, legend and number font families, panel bands, knurled skirts, cap styles and the strip tile. These are mostly the mixer's Console look.
- User themes: the Themes folder, its parser and watcher, Duplicate as File… and Show Themes Folder. A saved `user.*` theme id is kept and shows Classic Light until then.

**Architecture:**
- **The theme table.** `vizdaw-app` gets `theme.rs`, which builds the table from a checked-in copy of symbiotic-design 0.2.3 `tokens/tokens.json`. That file is the owner's MIT-licensed design repo, the same source Swift's generated `Theme` used. It is read with `include_str!` and parsed once into `Theme` values: id, name, appearance, styles, and every colour group as 0xRRGGBB, plus radius.
- **The preference** `AppearancePrefs` sits over `Prefs` under the key `appearance`, as Swift did.
- **The current theme in GPUI.**
  - GPUI holds the current `Theme` as a `Global`.
  - `strip_palette::tokens()` reads it.
  - A new `theme_colors()` (or the same accessor, extended) gives the chrome and editor tokens.
  - Every hard-coded colour literal in `crates/vizdaw-gpui/src` becomes a token. There are about 76 hex sites in 17 files, plus `rgba`/`hsla`/`black()`/`white()` sites.
  - Changing the theme refreshes every window.
- **A scan test**, like Swift's `ThemeScanTests`, keeps literals from coming back.

**Tech Stack:** Rust 2024; GPUI at the slice 1 pin; `core-ffi`, `core-control`, `vizctl`.

**Spec:** `docs/superpowers/specs/2026-11-20-cross-platform-27-1-design.md`. The design says "five themes"; the owner ruled six on 2026-11-25.
- **Parity rows:** "Settings: Appearance"; `ui.theme`; the e2e `themes_and_the_strip_over_the_channel`.
- **Swift reference:**
  - The settings inventory `.superpowers/sdd/settings-inventory.md`: §1.1, §3 (`ui.theme`), §4 ("Theme machinery") and §5 (`AppearanceTests`, `ThemeScanTests`).
  - Swift sources: `app/Sources/VizDAW/AppearanceSettingsView.swift`, `app/Sources/VizDAWAppModel/{AppearancePreferences,AppearanceCommands,ThemedViews}.swift`, `app/Tests/VizDAWAppModelTests/AppearanceTests.swift`, `app/Tests/VizDAWEditorsTests/ThemeScanTests.swift`.
  - Each GPUI view's Swift twin, for which token it used (named per task).
  - The generated Swift `Theme`: `app/.build/checkouts/symbiotic-design/` if present, or `/Users/lattawb/workspace/symbiotic-design`.

## Global Constraints

- **Rules from slices 7a–9a that still hold:**
  - The Swift app is retired: parity with its behaviour, no compatibility.
  - Use exact Swift strings, with file:line.
  - The UI says "track", never "lane".
  - No banned vendor or DAW product names in repo text. Theme names are the six above, as symbiotic-design writes them.
  - Nothing slow on the UI thread.
  - GPUI tests press `secondary-` keys.
  - Linux CI runs every test. The e2e runs on the offline backend.
- **Before every commit:**
  - `cargo fmt --all -- --check`, and `cargo fmt --all` to fix it. CI stops at this step.
  - `cargo clippy -p <touched crates> --all-targets -- -D warnings`.
  - The touched crates' tests.
- **Theme ids:**
  - They are `core_control::registry::THEME_IDS`, in that order. The table's ids must equal them (test).
  - The default is `classic-light`.
  - An unknown saved id is kept in the preference, and the app draws Classic Light (Swift `AppearancePreferences.swift:28-32`).
- **The preference.** Key `appearance`, JSON `{"theme": id, "colourBars": bool}`, decoded tolerantly; colour bars default on.
- **Tokens, not literals.** After Task 5, no `0xRRGGBB`, `rgb(0x…)`, `rgba(0x…)`, `hsla(…)`, `black()` or `white()` colour appears in `crates/vizdaw-gpui/src`. The exceptions are `strip_palette.rs`, the theme accessor, and a short allow-list in the scan test, each entry with its reason (for example the offline hatching stand-in). Alpha applied to a token is fine.
- **Mapping a literal to a token.** Use the token Swift's twin view used. Where GPUI has no Swift twin (the Linux in-window menu bar, for instance), use the nearest semantic token (`surface.*`, `text.*`, `accent.*`, `state.*`) and note the choice in the report.
- **Classic Light is the default.** The app's look changes from today's hard-coded dark chrome to Classic Light. That is intended: Swift parity.
- **Screenshots** go to the owner only when every window resembles Swift's look in that theme. The controller makes the on-screen checks on the Mac, one theme at a time.
- **Review pace:** fix crashes, data loss and wrong overwrites; edge cases go to the backlog.

## Review Focus

1. **The theme switch is live and complete.** After `ui.theme`, every open window redraws in the new theme: document, Mixer, plug-in and Settings windows, and open popovers and sheets. Nothing keeps a cached old colour, for example a colour captured in a struct when it was built.
2. **Hostile `ui.theme` calls change nothing and open no dialog:** `{}`, an unknown id such as `svp-light`, an extra parameter, a non-text value, and a `user.*` id that is not loaded.
3. **The preference round-trips.** A damaged value falls back to Classic Light without losing `colourBars`. An unknown id is kept and Classic Light is drawn. A failed preference write still switches the theme for this run and says so.
4. **Legibility.** No token pairs text on a background that the theme's `contrast` section in `tokens.json` does not allow. Check the pairs the chrome uses: text.primary on surface.panel, text.inverse on accent, state colours on their fills.
5. **Linux.** The in-window menu bar, the sheet cards and the alert window take the theme too.

---

### Task 1: The theme table

**Files:**
- Create:
  - `crates/vizdaw-app/assets/symbiotic-tokens.json`: a copy of symbiotic-design 0.2.3 `tokens/tokens.json`, with a header line in `crates/vizdaw-app/assets/README.md` naming its source, version and MIT licence.
  - `crates/vizdaw-app/src/theme.rs`
- Modify: `crates/vizdaw-app/src/lib.rs`
- Test: `crates/vizdaw-app/tests/theme.rs`

**Interfaces:**
- **Produces:**
  - `pub struct Theme`, with:
    - `id: &'static str` or `String`, `name`, `appearance` (`Light`/`Dark`), and `styles: ThemeStyles` (all eight style fields parsed; 9b uses only `corners`).
    - One field per colour group, as named in `tokens.json`: `surface {window, panel, raised, inset, separator, selection, …}`, `text`, `accent`, `state`, `meter`, `graph`, `control`, `strip`, `brand`, `knob`, `fader`, `relief`, `swatch`, `keys`, `readout` and `band`, each as `u32` 0xRRGGBB. Alpha is kept where the JSON gives it (`0xRRGGBBAA`, or a separate alpha field; match the JSON).
    - `radius {none, s, m, l}: f32`.
  - `pub fn themes() -> &'static [Theme]`, the six in `THEME_IDS` order, parsed once.
  - `pub fn theme(id: &str) -> Option<&'static Theme>`.
  - `pub fn theme_or_default(id: &str) -> &'static Theme`.

- [ ] **Step 1: Write the failing tests:**
  - `the_table_is_the_channels_six_in_order`, which checks against `THEME_IDS`.
  - `every_theme_defines_every_token`: no zero or default value where the JSON has one, so check a few per group against the JSON.
  - `classic_light_matches_the_strip_palette`: every `strip_palette::CLASSIC_LIGHT` value equals the table's. If this test would need `vizdaw-gpui`, put it in Task 2 instead.
  - `theme_names_keep_the_clean_room` (Swift `AppearanceTests:47`).
  - `an_unknown_id_falls_back_to_classic_light`.
- [ ] **Step 2:** Run `cargo test -p vizdaw-app --test theme`. Expected: it fails to compile.
- [ ] **Step 3:** Implement with serde over the JSON shape. Parse once, in a `OnceLock`. A parse failure is a bug in the checked-in file, so `expect` is acceptable there with a clear message, and a test covers it.
- [ ] **Step 4:** Run `cargo test -p vizdaw-app`, then clippy and fmt. Expected: green.
- [ ] **Step 5:** Commit `app: the six themes from symbiotic-design 0.2.3 tokens`.

### Task 2: The preference, the current theme, `ui.theme` and `state.get windows`

**Files:**
- Create: `crates/vizdaw-app/src/appearance_prefs.rs`
- Modify:
  - `crates/vizdaw-app/src/{commands.rs,lib.rs}`
  - `crates/vizdaw-gpui/src/{strip_palette.rs,app_state.rs,lib.rs,control.rs}`
  - `crates/vizdaw-gpui/tests/control.rs`: `state_get_windows_has_theme` now expects `classic-light`.
- Test: `crates/vizdaw-app/tests/appearance_prefs.rs`, `crates/vizdaw-gpui/tests/control.rs`

**Interfaces:**
- **Produces:**
  - `AppearancePrefs { theme: String, colour_bars: bool, problem: Option<String> }`, with `load(&Prefs)`, `set_theme(&mut self, id, &mut Prefs) -> Result<(), String>` (refuses an unknown id) and `set_colour_bars`.
  - `run_ui_theme(args, &mut AppearancePrefs, &mut Prefs) -> Result<Ran, ControlError>`, with Swift's status `"theme <id>"` and its `invalid_params` messages (inventory §3, `AppearanceCommands.swift:11-17`). It needs no document. Add it to `APP_COMMANDS`.
  - In GPUI:
    - `AppState.appearance`.
    - `pub fn current_theme(cx: &App) -> &'static Theme`.
    - `strip_palette::tokens()` becomes `tokens(cx)` (or reads a global set on change), returning `StripTokens` built from the current theme. `StripTokens::from_theme(&Theme)` replaces the `CLASSIC_LIGHT` const; keep the const only if a test needs it.
    - `set_theme` refreshes every window (`cx.refresh_windows()`).
  - `state.get windows.theme` reports the preference's id, or `classic-light` for an unknown id. Swift reported the saved id; check `AppearanceCommands`/`ControlCommands` and match it.

- [ ] **Step 1: Write the failing tests**, porting `AppearanceTests`:
  - `default_is_classic_light` (:12)
  - `choose_saves` (:16)
  - `unknown_saved_theme_falls_back_and_is_kept` (:25)
  - `choose_refuses_unknown` (:41)
  - `ui_theme_arg_checks` (:52), covering `{}`, `svp-light`, an extra parameter and a non-text value
  - `ui_theme_status_needs_no_document` (:60)
  - `colour_bars_saved` (:88)
  - `every_built_in_choosable` (:99)
  - In GPUI: `ui_theme_switches_every_window_without_opening_one`, and `state_get_windows_reports_the_theme`.
- [ ] **Step 2:** Run them. Expected: they fail.
- [ ] **Step 3:** Implement. Load the preference in `AppState::set_prefs` (launch). Remove `ui.theme` from the `not_yet` list.
- [ ] **Step 4:** Run `cargo test -p vizdaw-app -p vizdaw-gpui`, then clippy and fmt. Expected: green. The colours on screen are still mostly literals until Tasks 3–4.
- [ ] **Step 5:** Commit `themes: the appearance preference, ui.theme and the current theme`.

### Task 3: Chrome draws from the theme

**Files:**
- Modify, removing every colour literal and the `transport_bar::colors` consts:
  - `crates/vizdaw-gpui/src/{transport_bar.rs,transport_fields.rs,record_popover.rs,menu_bar.rs,banners.rs,sheet_host.rs,text_input.rs,format_panel.rs,shell.rs,settings_window.rs}`
  - `crates/vizdaw-gpui/src/sheets/*.rs`
  - `crates/vizdaw-gpui/src/controls/*.rs` (where they don't already use `tokens()`)
- Test: `crates/vizdaw-gpui/tests/theme_chrome.rs`

**Interfaces:**
- **Consumes:** Task 2's `current_theme(cx)` / `tokens`.
- **Produces:** chrome colours from the theme. Map each one by its Swift twin:
  - The transport bar: Swift `TransportBarView`/`TransportFieldsView`.
  - Sheets: Swift sheets use system backgrounds, so map them to `surface.window`/`surface.panel`.
  - Banners: Swift `ProjectBanners`.
  - Alerts and the sheet card.
  - The Settings window.
- If `colors::*` helpers are kept, they become functions of the theme.

- [ ] **Step 1: Write the failing tests:**
  - `the_transport_bar_takes_the_theme`: render under Classic Light and under Studio Dark; the bar's background and its text colour differ and equal the theme's tokens. Use the registry or element-colour hook the existing GPUI tests use for colours. If there is none, add a test-only accessor that reports the colour an element drew.
  - `a_sheet_card_takes_the_theme`
  - `the_settings_window_takes_the_theme`
- [ ] **Step 2:** Run them. Expected: they fail.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run `cargo test -p vizdaw-gpui`, then clippy and fmt. Expected: green.
- [ ] **Step 5:** Commit `themes: the chrome draws from the theme`.

### Task 4: Editors, panes, headers and plug-in windows draw from the theme

**Files:**
- Modify: `crates/vizdaw-gpui/src/{arrangement.rs,track_headers.rs,region_drawing.rs,piano_roll.rs,list_editor.rs,panes.rs,detail_pane.rs,channel_strip_pane.rs,mixer_view.rs,midi_strip.rs,vca_strip.rs,input_picker.rs}` and `plugin_windows/*.rs`
- Test: `crates/vizdaw-gpui/tests/theme_editors.rs`

**Interfaces:**
- **Consumes:** Task 2.
- **Produces:** each site mapped by its Swift twin. The twins:
  - `ArrangementView`, `RegionDrawing`, `LaneHeaderView`, `PianoRollView` (`keys.white`/`keys.black`, `graph.*`, `band.*`), `ListEditorView`, `PaneViews`, `PluginWindows` header.
  - The offline hatching (`0xf0883e` at 0.55, an 8c stand-in) becomes `state.offline` or `state.warning`, whichever Swift's offline colour was.
  - The waveform takes Swift's region-line colour at 0.65.
- **Colour bars:** when `colour_bars` is off, the track headers and strips draw no colour bar, as Swift did; find its use of `colourBars`.

- [ ] **Step 1: Write the failing tests:**
  - `the_arrangement_takes_the_theme`, covering the background, the grid and a region's fill.
  - `the_piano_roll_keys_take_the_theme`
  - `colour_bars_off_hides_the_bars`
  - `a_plugin_window_header_takes_the_theme`
- [ ] **Step 2:** Run them. Expected: they fail.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run `cargo test -p vizdaw-gpui`, then clippy and fmt. Expected: green.
- [ ] **Step 5:** Commit `themes: editors, panes, headers and plug-in windows draw from the theme`.

### Task 5: The Appearance tab and the scan test

**Files:**
- Modify: `crates/vizdaw-gpui/src/settings_window.rs`
- Test: `crates/vizdaw-gpui/tests/settings_window.rs`, `crates/vizdaw-gpui/tests/theme_scan.rs`

**Interfaces:**
- **Consumes:** Tasks 2–4.
- **Produces:** the Appearance tab (inventory §1.1):
  - The "Theme" radio list. Each row has the theme's name and eight 12×12 swatches: surface.window, surface.panel, accent.accent, then strip.channel, instrument, auxReturn, bus and master (Swift `AppearanceSettingsView.swift:75-94`).
  - The "Show channel colour bars" toggle.
  - The caption "The theme applies to every window. It is a preference of this Mac, not saved in projects." Off macOS, check Swift for any platform wording. If there is none, keep "this Mac" on macOS and write "this computer" elsewhere, and note it as a ruling.
  - No "Your Themes" section (user themes come later).
  - **Corners:** `styles.corners == square` draws controls with radius 0, as Flat and Symbiotic do. This applies through `tokens().radius` and the chrome radii.
  - **`theme_scan.rs`:** walks `src/` and fails on any colour literal outside the allow-list (Global Constraints).

- [ ] **Step 1: Write the failing tests:**
  - `the_appearance_tab_lists_the_six_with_swatches`
  - `choosing_a_theme_switches_every_window_and_saves`
  - `the_colour_bars_toggle_saves_and_hides_the_bars`
  - `square_corners_in_flat`
  - `no_colour_literals_outside_the_theme` (the scan)
- [ ] **Step 2:** Run them. Expected: they fail. The scan fails on any literal Tasks 3–4 missed; fix those here.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run `cargo test -p vizdaw-app -p vizdaw-gpui`, then clippy and fmt. Expected: green.
- [ ] **Step 5:** Commit `gpui: Settings › Appearance; the colour scan`.

### Task 6: The e2e

**Files:** `crates/vizctl/tests/e2e_gpui.rs`

**Produces:** `gpui_themes_and_the_strip_over_the_channel`, a port of Swift `e2e.rs:2445` onwards:
- Steps 1–3: the default `classic-light`; hostile calls change nothing, open no window and post no alert; `ui.theme console` gives the status `theme console`, and `windows.theme` reads `console` with the same windows.
- The later steps (mixer kind colours, the level box, the values saved with the project while the theme stays a preference) as far as the GPUI app supports them. A step that needs an unported feature is skipped with a comment naming the slice.
- It runs on the Mac (env as in slice 9a) and in Linux CI.

- [ ] Write it, run it green on the Mac, and commit `e2e: gpui themes and the strip`.

### Task 7: Checks, checklist and backlog

**Files:** `docs/testing/controller-checks.md`, `docs/testing/manual-checks.md`, `docs/superpowers/specs/2026-11-22-parity-checklist.md`, `docs/feature-backlog.md`

**Produces:**
- **`controller-checks.md`:** the per-theme rows (the "After the Settings slice" section) become this slice's look checks. The controller runs them on the Mac, all six themes over the document, Mixer, Settings and a plug-in window, and fills them in.
- **`manual-checks.md`:** a Linux row for the six themes on the owner's desktop.
- **Parity checklist ticks:** Settings: Appearance; `ui.theme`; `themes_and_the_strip_over_the_channel`. The Linux cells are ticked after CI.
- **Backlog:**
  - "GPUI slice 9b: review leftovers".
  - "Theme styles in the mixer (label case, fonts, panel bands, knurled skirts, cap styles, tile)".
  - "User themes (folder, parser, watcher, Duplicate as File…, Show Themes Folder)".
- [ ] Commit `docs: slice 9b checks, checklist and backlog`.

## Outcome (2026-11-26)

All seven tasks are done (commits on `next` from cc478b67 to the Outcome commit). Every window of the GPUI app draws from the current theme, one of the six built-ins. The theme is chosen in Settings › Appearance (six rows with swatches, the colour-bars switch, the caption, the problem line) or with `ui.theme`, and the change applies to every window at once. `state.get windows` reports it. Flat and Symbiotic draw square corners. A scan test keeps colour literals out of `crates/vizdaw-gpui/src`. `gpui_themes_and_the_strip_over_the_channel` ports every Swift step.

Evidence:
- vizdaw-app and vizdaw-gpui suites green on the Mac at each task, and the e2e green on the Mac in 9.5 s.
- The controller checked the document window in all six themes and the Mix view in Console and Flat, using the app's offscreen screenshots.
- Final review: ready after two contrast fixes, both made with six-theme tests (bcc3dd1d).

### Rulings

- Pre-flight: tokens.json is checked in as a copy of symbiotic-design 0.2.3, rather than taken as a git dependency. Cost: it must be re-copied by hand on each design bump.
- Task 1 (implementer): colours are `Color { rgb, alpha }`, because the JSON mixes #RRGGBB and #RRGGBBAA.
- Task 2 (implementer): the current theme lives in a thread-local slot that `tokens()` reads. The reviewer checked that every colour is read on the main thread. `windows.theme` reports the resolved id, as Swift did. `user.*` ids are refused until user themes exist.
- Task 3 (implementer): the Settings and shell roots use `surface.window`, as Swift's `ThemedRoot` did. The sheet scrim is `meter.background` at 0.6; Swift had no equivalent. Menu hover uses `surface.raised`.
- Task 3 fix round: the readouts take `readout.*`. Play, Record and destructive buttons show a tinted glyph or text on a normal face, as Swift did, instead of text on a state fill.
- Task 4 (implementer, reviewer confirmed): a track-header colour bar was added, with Swift's geometry. The pane and mixer background is `surface.window`.
- Task 4 fix round: the lit monitor button takes the key ink (`legend_on`). The controller checked this fix diff directly instead of sending it for a re-review; the final review then confirmed it.
- Task 5 (implementer): the whole-`src` scan stands in for a separate `theme_scan.rs`. The caption reads "this Mac" on macOS and "this computer" elsewhere. A `ThemedCorners` trait applies square corners across 19 files.
- Task 6: the controller restored the `Set Fader` undo check that the implementer had relaxed.
- Final: `StripTokens::legend_on` now uses Swift's key-ink rule (the mixer toggles were 1.3–2.8:1 in the dark themes), and the selected Settings tab uses `accent.onAccent`.

### Carried

- The Linux cells (`ui.theme`, Settings: Appearance, the e2e) get ticked after Linux CI.
- `controller-checks.md` "Themes (9b)" still needs an unlocked-screen pass: the Settings window, plug-in windows, menus and popovers in each theme.
- The theme styles beyond corners, and user themes, are backlog rows for a later slice.

### Deferred minors

All are in the backlog row "GPUI slice 9b: review leftovers".
