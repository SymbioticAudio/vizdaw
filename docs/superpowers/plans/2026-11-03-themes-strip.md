# VizDAW 7b — Themes, Icons and the Channel Strip Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** VizDAW looks finished in its default theme.
- It adopts `symbiotic-design` 0.2.0, which this plan finishes:
  - five real palettes, approved by the owner from browser mockups;
  - a theme `name`;
  - the `knob`, `fader` and `relief` token groups;
  - fourteen new icon roles;
  - a new `SymbioticControls` product (knob, fader and number box).
- Classic Light is the default. Settings › Appearance switches the theme live, in every window.
- Every icon comes from `SymbioticIcons`, and the app still builds without the branded glyphs.
- One channel strip serves the left pane, the bottom pane's mixer, the Mix view and the mixer window. From the top it shows:
  - the colour bar and the name, then the input;
  - a real, saved **EQ slot**, which processes before the other inserts, then the inserts;
  - send knobs, each with a typed level;
  - the output picker;
  - a pan knob with a typed L/C/R value;
  - a large, long-throw fader with a dB scale and a meter, then the typed level;
  - M S R I and the automation mode.
- The mixer is coloured like a console in every theme: fader caps by strip kind, knobs by function. The Console theme draws its hardware look in code.

**Architecture:**
- **Part A, `~/workspace/symbiotic-design`** (its own repo, branch `sp7-0.2.0`):
  - `docs/mockups/0.2.0/palettes.json`, the palettes as named tokens. `tools/mockup.py` turns it into `themes.html`, which shows the 7a window and the new strip in all five themes;
  - after the owner's gate, `tokens/tokens.json` 0.2.0 and the generator;
  - the new icon roles and their standard icons;
  - `Sources/SymbioticControls` (geometry and interaction kept apart from drawing, with colours injected through palettes, following the HANDOFF family rule);
  - the local tag `0.2.0`.
- **Part B, `~/workspace/vizdaw`** (branch `sp7-ui`, after 7a):
  - **`core-model`:**
    - `Strip.eq: Option<PluginSlot>`, `SlotOwner::Eq`, `Strip::chain()` and `Mixer::slots()`;
    - `MixerCmd::SetEq`;
    - format `VERSION` 4.
  - **`core-engine`:** `mixer_spec` builds each strip's insert list from `chain()`, so the EQ runs first.
  - **`core-host`:** loads the EQ like any insert.
  - **`core-ffi`:**
    - `StripRow.eq` and `StripRow.colour` (`StripColourRow`);
    - `Session::set_eq`;
    - `state.get mixer`;
    - `state.get windows`' `theme`.
  - **`core-control`:** `ui.theme`, and the `mixer` state target.
  - **`VizDAWAppModel`** (tested):
    - `AppearancePreferences` and `AppearanceCommands`;
    - `AppIcon`;
    - `StripEntry` (the typed values), `StripLayout` and `StripPalette`;
    - `MixerModel`'s EQ, typed-value and arm/monitor actions.
  - **`VizDAWEditors`:** `EditorCanvas.theme`.
  - **`VizDAW`** (views):
    - `Theming.swift` (the themed root modifier and the AppKit windows);
    - `AppearanceSettingsView`;
    - `ChannelStrip.swift`, the new strip;
    - the colour and icon sweep.

**Tech Stack:**
- Rust 2024 (`core-model`, `core-engine`, `core-host`, `core-ffi` with UniFFI 0.32.2, `core-control`, `vizctl`).
- Swift 6 / SwiftPM (Swift Testing; SwiftUI and AppKit), macOS 14+.
- Stdlib Python 3 (the design repo's generator and `tools/mockup.py`).
- HTML and CSS for the mockups (no frameworks, web fonts or images).

**Spec:** `docs/superpowers/specs/2026-11-03-ui-polish-design.md`, §7 row 7b. Read the whole spec: every "(owner)" and "(controller's call)" binds. This plan delivers:
- §2 item 5 (the channel strip, including the EQ slot's core, FFI, format and undo change);
- §2 item 6 (themes: `symbiotic-design` adopted and finished, the five palettes, Settings › Appearance, live switching, `SymbioticControls`, and the Console realism drawn in code);
- §2 item 7 (console colouring in every theme);
- §2 item 8's strip kinds for colour;
- §2 item 9 (icons, with the fallback set);
- §2 item 11's 7b part (the root README's themes);
- §4's anti-goals "a realistic mixer forced on everyone", "controls too small to use" and "hard-coded colours";
- §5's "the theme preference is a user preference".

It builds on 7a's end state. Read `docs/superpowers/plans/2026-11-03-window-views-panes.md`: its Rulings 8 (one mixer, three placements), 9 (the transport bar) and 10 (the control channel) shape this plan.

Also read, in `~/workspace/symbiotic-design`:
- `README.md`;
- `HANDOFF.md` (the family rule (g), the knob decisions (h)–(k), and the "M7" request 1 geometry);
- `docs/superpowers/specs/2026-10-20-design-system-design.md` and its plan;
- `tokens/tokens.json`, `tools/tokenlib.py` and `tools/swiftgen_tokens.py`.

7a (window and panes), 7c (audio editor) and 7d (clean-up) are out of scope.

## Global Constraints

- **Two repos, one plan.**
  - Part A runs in `~/workspace/symbiotic-design`. Part B runs in `/Users/lattawb/workspace/vizdaw`.
  - Every bash block names its repo with `cd` and starts with `export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools`.
  - A `git commit` without `DEVELOPER_DIR` fails silently: check every commit with `git log -1 --oneline` and `git show --stat HEAD`.
- **Part A's repo.**
  - Branch `sp7-0.2.0`, made in Task A1 from `eg13-requests` at `b209d2c` (it holds the EG13 requests that A4 answers). Never commit to `main` or `eg13-requests`.
  - Commits are authored by Westlee Latta <symbioticaudio@proton.me>, the repo's local identity (check `git config user.email`).
  - **Never push and never push a tag.** Pushing needs the owner's say-so (Gate G2).
  - Swift only through `scripts/swift.sh build|test`. Python is stdlib only: `python3 tools/generate.py`, `python3 tools/contact_sheet.py`, `python3 tools/references.py` (headless Chrome, only when an SVG changed), and `python3 -m unittest discover -s tools/tests`.
  - Generated files are committed: CI fails on any difference.
- **Part B's repo.**
  - Branch `sp7-ui`, after 7a's last commit. Never switch branches, never push.
  - Swift only through the scripts, from the repo root (`scripts/build-ffi.sh`, `scripts/swift.sh build`, `scripts/swift.sh test`, `scripts/make-app.sh`). Never call `swift`, `xcodebuild` or `xcrun` directly.
  - Commit at the end of every task, adding only the files the task lists, by path (never `git add -A`: other agents work in this tree).
  - Never commit `.superpowers/`, `.DS_Store` or generated files (`app/Sources/VizDAWCore/vizdaw_ffi.swift`, the FFI headers).
  - Manifests stay as they are: `Cargo.lock` and every `Cargo.toml`, with no new crate and no new dependency. The exception is Task B3, which changes `app/Package.swift` and adds `app/Package.resolved` (Ruling 6).
- **Commit trailers, both repos** (use the `Co-Authored-By` line naming the model you actually are):
  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P
  ```
- **Clean room.**
  - Never read `~/workspace/SVP/research` or any Studio Vision material. Classic Light comes from the plan's direction (Ruling 2) and the owner's remarks at Gate G1, from memory only.
  - No Logic, Apple, Midas, Soundcraft, Steinberg, Cubase, Microsoft, Metro, Zune, Opcode, SVP or Studio Vision names, logos, fonts or artwork in the UI, the mockups, token names or icon names. The themes' user-visible names are exactly "Classic Light", "Studio Dark", "Flat", "Console" and "Symbiotic".
  - No copied textures and no image assets. The Console theme's realism is gradients, shadows and bevels drawn in code from `relief` tokens.
  - The mockups use the system font stack only (`-apple-system, "Helvetica Neue", sans-serif`) and no image files.
- **No GUI tools, no real devices.**
  - Never use GUI, computer-use, browser-automation or accessibility-automation tools. The owner checks the mockups (Gate G1) and the app by eye (README steps 207–216, Task B8).
  - `tools/references.py` runs headless Chrome only to render the reference PNGs of SVGs. It is not a GUI tool.
  - Tests never open real audio or MIDI, and never trigger the microphone prompt:
    - Swift tests use `FakeSession`;
    - every `core-ffi` test uses its `serial()` helper;
    - the e2e runs with `VIZDAW_AUDIO=offline`, as `App::start` sets it.
  - Afterwards `pgrep -lf MacOS/VizDAW` prints nothing and `ls -d /tmp/vze*` prints nothing new.
  - **Never touch the real `~/Library/Application Support/VizDAW`, the real preferences or the real Trash.**
    - Theme preferences in tests go through an injected store: `AppearancePreferences(defaults: MemoryDefaults())`.
    - The app takes `MemoryDefaults.appStore()`, which is in memory under `vizctl launch`, so the e2e's `ui.theme` never reaches the real preferences.
    - Swift tests use `scripts/swift.sh test`'s own `VIZDAW_TRASH_DIR` and `VIZDAW_SCRATCH_DIR`.
- **Programmatic and channel calls never open dialogs.**
  - `ui.theme` changes the preference and the windows, and opens nothing.
  - A typed value that is refused shows its message in the status line, never in an alert.
  - The EQ slot's "EQ ＋" opens the plug-in browser popover only from a click, as the inserts' "+" does today.
- **New commands get hostile-input tests:**
  - `mixer_state_target_parses` and `ui_theme_checks_its_params` (core-control);
  - `uiThemeChecksItsArguments` (app);
  - `hostile_eq_calls_are_refused` (core-ffi);
  - the e2e's hostile block (Task B8).
  - Every typed number box has hostile-input tests: `hostileLevelsAreRefused` and `hostilePansAreRefused` (Task B6).
- **FFI.**
  - The one new export is `Session::set_eq`. It runs inside `ffi_guard` (which is `catch_unwind`) and returns `Result<DocSnapshot, VizError>`.
  - The new UniFFI types are `StripColourRow`, the `StripRow` fields `eq` and `colour`, `StateTargetRow::Mixer`, `ControlResult::Mixer`, and `ControlResult::Windows`' new `theme` field.
- **Real time.**
  - The render path stays allocation-free. Only `mixer_spec`, on the control thread, changes in the engine.
  - Tasks B1 and B8 run the rt-check gates:
    - `cargo clippy -p vizdaw-play --all-targets --features rt-check -- -D warnings`;
    - `cargo test -p core-engine --features rt-check --test rt_alloc --test rt_alloc_graph --test rt_alloc_record --test rt_alloc_gestures --test rt_alloc_bounce`.
- **Swift 6.1 (VizDAW's CI runs macos-15) as well as the local toolchain.** This applies to `SymbioticControls` too, because VizDAW builds it.
  - Pass no weak `self` and no non-Sendable value into `MainActor.assumeIsolated`. Capture a strong `let` made before the closure, and bring a notification or event in through `nonisolated(unsafe) let`, as `KeyMonitor` does.
  - No `@State`, `@GestureState` or `@Entry` in any package code built with the Command Line Tools, which cannot expand them. Spell out `State(initialValue:)` and `GestureState(initialValue:)`, and declare environment values with an `EnvironmentKey`.
  - Use no API newer than macOS 14.
- **Fallback icons.** VizDAW builds and runs with `VIZDAW_NO_BRAND=1`, which leaves the `SymbioticBrand` product out (Ruling 6). Every role then draws its standard icon.
- **Lints**, in Rust as in every plan: `#![deny(missing_docs)]`, and no unwrap, expect, indexing or panic outside tests.
- **TDD.** Every new test is run and seen to fail (RED) before the code that makes it pass.
- **File compatibility.**
  - `core_model::file::VERSION` goes from 3 to 4 (Ruling 8), with `migrate_v3_to_v4`.
  - `fixtures/demo.vizdaw` stays a version-3 file, so CI's offline render keeps proving the migration.
  - `ui.json` (`EditorLayout.version` 1) is unchanged: the theme is a preference, not project state.
- **Each task ends green.**
  - **Part A:** `python3 tools/generate.py`, then `git status --porcelain` shows only the task's files; `python3 -m unittest discover -s tools/tests`; `scripts/swift.sh build && scripts/swift.sh test`.
  - **Part B, always:** `cargo fmt --all -- --check`.
  - **Part B, Rust tasks (B1, B2, B4, B8):**
    - `cargo clippy --workspace --all-targets -- -D warnings`;
    - `cargo clippy --workspace --all-targets --target x86_64-unknown-linux-gnu --target-dir target/linux-check -- -D warnings`;
    - `cargo test --workspace`.
  - **Part B, every task that touches Swift:** `scripts/build-ffi.sh`, `scripts/swift.sh build` and `scripts/swift.sh test`. From B3 on, also `VIZDAW_NO_BRAND=1 scripts/swift.sh build --scratch-path .build-nobrand`.
  - **Task B8:** everything, plus `scripts/make-app.sh` and the e2e.
- **Baselines and expected counts.**
  - **Part A, measured at `b209d2c`:** Python 174, Swift 46.
  - **Part B:** 7a's end, as its `progress.md` records it (`.superpowers/sdd/2026-11-03-window-views-panes/progress.md`). Its plan expects Rust 1996 passed / 19 ignored (macOS), e2e 15, registry 153 and app README last step 206. Swift measured 1040 at 7a Task 7, against the 1034 planned.
  - Read the real 7a-end numbers before Task B1 and write them into this plan's `progress.md`. The deltas below are what binds.

  | Task | Python | Swift (design) | Rust (macOS and Linux) | Swift (VizDAW) | Other |
  |---|---|---|---|---|---|
  | A1 mockups | +3 | — | | | |
  | A2 tokens 0.2.0 | +5 | +4 | | | |
  | A3 icon roles | +2 | +1 | | | |
  | A4 controls, tag | — | +12 | | | tag `0.2.0` (local) |
  | B1 EQ slot core | | | +9 | — | |
  | B2 EQ FFI, colours, `state.get mixer` | | | +6 | +0 (compiles) | |
  | B3 package, icons | | | — | +3 | no-brand build |
  | B4 themes, Settings, `ui.theme` | | | +2 | +7 | registry 154 |
  | B5 colour sweep | | | — | +3 | |
  | B6 strip models | | | — | +15 | |
  | B7 strip view | | | — | +0 | |
  | B8 e2e, READMEs | | | ignored +1 | — | e2e 16; README 207–216 |
  | **After** | **184** | **63** | **7a end + 17 / ignored + 1** | **7a end + 28** | |

  Report the measured counts at every task's end. A difference is explained, never hidden.
- **How code is given.** Signatures, test names and assertions, not bodies.
  - A Find anchor that depends on 7a's result is named by symbol (`MixerView(model:placement:)`, `StripView`, `DocumentShell`, `DocumentTransportBar`, `PaneFrame`), never by exact text.
  - Where the tree differs from a name here, use the tree's name and say so in the report.

## Review Focus

These are the five failure modes a user is most likely to hit that the spec implies but no task's feature tests exercise, most likely first. Each is pinned by the named test in its task.

1. **Careless or hostile typing in a number box.** Examples:
   - an empty box or spaces;
   - `abc`, `4.2.1`, `--3`;
   - `nan`, `inf`, `1e309`, `1e3`;
   - `-4.2 dBFS`;
   - a pasted 300-character string or a control character;
   - the Unicode minus `−4.2`;
   - `+40`, `-200`;
   - for pan: `LR`, `L-5`, `24L`, `C 5`, `L150`, `r 10`.

   Expected:
   - a value the grammar accepts is applied, with out-of-range numbers clamped (+40 → +12.0 dB, −200 → −∞, L150 → L 100), as one undo step;
   - anything else is refused: the box shows the old value, the status line shows the hint, and nothing is sent to the core.

   Pinned by `levelsAcceptDecibelsWithOrWithoutTheUnit`, `levelsClampAndReadMinusInfinity`, `hostileLevelsAreRefused`, `pansAcceptLeftCentreRight`, `hostilePansAreRefused` and `typingALevelIsOneClickAndARefusalChangesNothing` (Task B6), and by the e2e's typed `LR` (Task B8).
2. **The EQ slot under ordinary editing.** The cases:
   - an insert dragged above the EQ;
   - the EQ dragged onto another strip;
   - the EQ replaced, then Undo;
   - the EQ emptied, saved, reopened;
   - an older project opened;
   - a bus that feeds the EQ's sidechain removed.

   Expected:
   - the EQ stays first and processes first;
   - it cannot be moved or removed as an insert;
   - Undo restores the replaced plug-in with its settings;
   - a version-3 project opens with every EQ slot empty;
   - the sidechain is cleared.

   Pinned by `the_eq_slot_stays_first_when_inserts_move`, `the_eq_slot_is_not_an_insert`, `removing_a_bus_clears_an_eq_sidechain_from_it` and `version_3_projects_migrate_to_version_4` (B1); `replacing_the_eq_keeps_the_old_state_for_undo` and `hostile_eq_calls_are_refused` (B2).
3. **A theme switched while things are open**: the mixer window, a torn editor, a plug-in window, the Modify panel, Settings, a popover, and macOS Dark Mode on.

   Expected:
   - every VizDAW window changes at once;
   - AppKit canvases redraw with the new colours;
   - no window keeps the old palette or the system appearance;
   - Classic Light stays light in Dark Mode.

   Pinned by `choosingAThemeSavesItAndTellsTheWindows` (B4), `anEditorRedrawsWhenItsThemeChanges` and `noColourLiteralsOutsideTheThemeBridges` (B5), and by README step 208.
4. **A short pane.** The bottom pane's mixer at its default 260 points, or 120, and the left strip on a 700×450 window.

   Expected: the fader keeps at least `StripLayout.minimumThrow` (160 points) and the strip scrolls vertically, rather than squashing the fader or cutting off M S R I.

   Pinned by `aShortPaneScrollsTheStripAndKeepsTheMinimumThrow` (B6) and README step 213.
5. **A damaged or unknown saved theme**: a preference written by a newer VizDAW (`"theme": "neon"`), garbage bytes, or no preference at all.

   Expected: Classic Light, with the colour bars on; nothing is rewritten until the user chooses.

   Pinned by `anUnknownOrDamagedSavedThemeFallsBack` and `theDefaultThemeIsClassicLight` (B4).

## Gates

The controller stops at each gate and asks the owner. Neither gate is a task. The answer is recorded in `progress.md`.

- **Gate G1, "owner approves palettes"**, after Task A1. The controller gives the owner the absolute path of `docs/mockups/0.2.0/themes.html` (in the design repo) and the commit. The owner opens it in a browser and approves each theme by eye, or asks for changes.
  - Changes go back to A1's implementer as a fix round, editing `palettes.json` only and regenerating. Repeat until all five are approved.
  - Record `G1: approved <date>, palettes.json at <sha>`, with any geometry the owner changed (fader cap, knob sizes, strip width), which then binds Rulings 4 and 14.
  - Tasks B1 and B2 consume no tokens, so they may run while G1 is open. Tasks A2–A4 and B3–B8 wait.
- **Gate G2, "owner approves the 0.2.0 push"**, after Task A4. The controller asks the owner three things:
  1. May `sp7-0.2.0` be fast-forwarded into `main`, and may `main` and the annotated tag `0.2.0` be pushed?
  2. Should `HANDOFF.md` stay in the public repo? It is already public on the pushed `eg13-requests` branch.
  3. After the push, is CI green on GitHub?

  The controller pushes only with that yes, and records the pushed SHA and the CI run. Tasks B3–B8 need `0.2.0` on GitHub (Ruling 6) and wait for G2.

Execution order: A1 → G1 (B1 and B2 meanwhile) → A2 → A3 → A4 → G2 → B3 → B4 → B5 → B6 → B7 → B8.

## Plan Rulings

Each ruling decides something the spec leaves to the builder, as "what — why — cost if wrong". The spec's "(controller's call)" items are restated only where this plan makes them concrete.

1. **Ruling (two repos): Part A's tasks finish `symbiotic-design` 0.2.0 before VizDAW consumes it.**
   - What: Part A runs on branch `sp7-0.2.0` from `eg13-requests` and ends in a local tag. VizDAW pins `0.2.0` only after G2's push.
   - Why: spec §2 item 6 ("releases a new tag (0.2.0) that VizDAW pins"; "pushing the design repo needs the owner's say-so"). The EG13 knob requests live only on `eg13-requests`.
   - Cost if wrong: one rebase of a branch nobody else uses.
2. **Ruling (the mockups): palettes as named tokens, rendered by a script.**
   - `docs/mockups/0.2.0/palettes.json` has `tokens.json`'s `themes` shape, with two additions: a `name` per theme, and values for the three new groups (Ruling 3). Each colour is `#RRGGBB` or `#RRGGBBAA`; no system colours.
   - `tools/mockup.py palettes.json out.html` (stdlib) writes `themes.html`. Each theme is a CSS block of custom properties named `--<group>-<token>` in kebab case (`--surface-window`, `--strip-aux-return`, `--relief-cap-top`). The page uses only those variables for colour, so the approved file moves into `tokens.json` by copy, not retyping.
   - `themes.html` holds, at 1440×900, under five theme tabs:
     1. the 7a window: the transport bar with Arrange | Mix and the pane toggles; the left channel strip running the full height; the arrangement with five lanes and regions and a playhead; the bottom piano roll; the right pane's markers;
     2. the Mix view with six strips:
        - Built-in Synth (instrument);
        - Vocal (audio channel, its EQ slot loaded, shown as "EQ plug-in": the mockup names no EQ);
        - Guitar (channel, "EQ ＋" empty);
        - Reverb (aux return);
        - Drums Bus (bus);
        - Master;
     3. the controls at 100 % and 200 %: knobs small, medium and large, with arc and indicator; the fader with its scale and meter; the number box idle, editing and refused; M S R I off and on; the five automation-mode tints.
   - The mockups follow the spec's strip order (§2 item 5) and 7a's layout A. Use the spec's brainstorm mockups as layout reference: `/Users/lattawb/workspace/vizdaw/.superpowers/brainstorm/11155-1793752858/content/{layout,strip,mixview}.html`. Where they differ from the spec, the spec wins (7a's spec checks).
   - Direction per theme (inspiration only; the owner tunes it at G1):
     - **Classic Light (default):** a late-1990s desktop-platinum feel. Light neutral-grey chrome (around `#DDDDDD`), white editing canvases, crisp one-point dark outlines, small dense type, saturated primary state colours (red record, green play, yellow solo), and a single blue selection. Square-ish corners (`radius.s` 2). Fader caps by strip kind on a light panel.
     - **Studio Dark:** a modern dark studio look. Charcoal panels (around `#2A2C30`), slightly lighter raised controls, a cool blue accent, soft separators, and muted, distinct kind colours.
     - **Flat:** typography-led and flat. Pure white or near-black surfaces, no gradients, no borders except one-point rules, and bold flat accent blocks; fader caps are flat tiles. `radius` all 0.
     - **Console:** a realistic console look, drawn in code. Dark blue-grey metal panels with a vertical gradient (`relief.panelTop`/`panelBottom`), cream legends, fader caps shaded from `capTop` to `capBottom` with a one-point `highlight` bevel and a `shadow`, knob caps likewise, and LED-style meters. This is the only theme with any non-clear `relief` value.
     - **Symbiotic:** today's `symbiotic` values (cyan, gold, ink, paper, black rules), extended to the new groups.
   - Every theme, Console included, colours fader caps by `strip.*` and knob arcs by `control.*` (spec §2 item 7).
   - Why: spec §2 item 6 ("approves each theme by eye from browser mockups before it is coded") and the brief's "palettes as named tokens".
   - Cost if wrong: one fix round per theme at G1.
3. **Ruling (tokens 0.2.0): a theme `name`, three new colour groups, all hex, no placeholders, and source compatibility.**
   - **Theme `name`:** required, one line, at most 32 characters, unique. Swift gets `Theme.name: String`.
   - **New groups**, each with `"since": "0.2.0"` in its group spec:
     - `knob`: `skirt`, `cap`, `track`, `indicator`;
     - `fader`: `well`, `scale`, `capLine`;
     - `relief`: `highlight`, `shadow`, `capTop`, `capBottom`, `panelTop`, `panelBottom`. In every theme but Console these are all `#00000000`, and drawing code skips a fully clear layer.
     - The knob's arc and the fader's cap take `control.*` and `strip.*`, which already exist.
   - **Values:** all five themes take the approved `palettes.json` values, hex only. `placeholder` is false for all five. Increase Contrast therefore no longer re-pins `classic-light` or `studio-dark`: the README's "Checking by eye" bullet is replaced by `contrast.md`'s body-text rule (A2's test).
   - **Source compatibility with 0.1.0's public initialisers** (EG13 builds its own `Theme`): every new `Theme.init` parameter has a default.
     - `name: String? = nil` reads back as the `id`.
     - Each `since` group defaults to `classicLight`'s values, generated as `public static let classicLight` on the group's struct.
   - Why: spec §2 item 6 ("`flat` and `console` are placeholders, and `classic-light` follows the macOS system colours"), and the design spec §2.2 ("user-facing names are chosen in SP7").
   - Cost if wrong: a token renamed before any app other than VizDAW reads it.
4. **Ruling (`SymbioticControls`): geometry, interaction and drawing for the knob, the fader and the number box, with colours injected.**
   - **Product:** a new `SymbioticControls` product and target, depending on `SymbioticTokens` only for the palettes' `init(theme:)` conveniences. The views take palettes, never a `Theme` (HANDOFF (g): "colours injected").
   - **Knob geometry** (HANDOFF "M7" request 1, EG13's):
     - skirt diameters: large 48, medium 38, small 28;
     - cap ratio: 0.676 for large, 0.618 for medium and small;
     - sweep 270°, from 7:30 to 4:30, with a bipolar fill from `origin`;
     - the value arc in the skirt band, stroke 0.7 of the band;
     - the indicator: a capsule `max(2, 0.07 d)` wide, `0.42 cap` long, centred `0.25 cap` from the centre;
     - a 2-point focus ring, and 0.45 opacity when disabled.
     - Options, as HANDOFF (j) lists them: arc `off | skirtBand | outside`; inner ring; ticks; cap `plain | accent | dark`; label `text | chip`. VizDAW uses arc `skirtBand`, no ticks and no inner ring.
   - **Fader:** a cap of 36×52 points with a one-point `capLine` across its middle; a throw given by the caller (VizDAW: 160…480); a scale of `FaderMark`s drawn left of the well; and a meter, which the caller puts beside it.
   - **Number box:** at least 56×22 points, monospaced digits in `type.label`. A click or Return starts editing; Return commits; Escape or losing focus cancels. Text beyond `NumberEntry.maxLength` (32) is not taken.
   - **Hit targets:** every control's hit area is at least `ControlMetrics.minimumHit` (24) points in each direction, padded beyond the drawing where needed (spec §4, "easy to hit with a trackpad").
   - **Interaction:**
     - a vertical drag of 200 points covers the whole range of a knob; a fader covers its throw; ⌥ held divides by 10;
     - a drag is relative to the grab point, so the cap never jumps to the pointer;
     - the scroll wheel steps 1 % of the range per line;
     - a double-click calls `onReset`;
     - VoiceOver's increment and decrement go through `accessibilityAdjustableAction` by the caller's step.
     - Callbacks are `onBegin` (mouse-down), `onChange(Double)` and `onEnd` (mouse-up), so VizDAW maps them to `touch`, `move` and `release` (plan 5e-2's gestures).
   - **Relief:** a `Relief` value (`highlight`, `shadow`, `capTop`, `capBottom`); `Relief.none` draws nothing extra. Console passes its tokens, and every other theme passes clear ones.
   - Why: spec §2 item 6 ("VizDAW's knob, fader and number box as a shared `SymbioticControls` product … geometry is agreed with EG13's knob through the HANDOFF") and HANDOFF (k). The 2026-10-28 owner note "SymbioticControls later, when VizDAW gets closer to building its UI (SP7)" is now.
   - Cost if wrong: EG13 adopts a later version; VizDAW changes palettes, not views.
5. **Ruling (new icon roles): fourteen roles VizDAW's sweep needs, each with a standard icon.**
   - The roles and their standard icons:
     - `view.left-pane`, `view.bottom-pane`, `view.right-pane`;
     - `view.mixer`, `view.split`, `view.empty`;
     - `view.expand`, `view.collapse` (the existing `expand` and `collapse` icons);
     - `settings.keys`, `settings.audio`, `settings.surfaces`, `settings.appearance`;
     - `status.info`;
     - `mixer.eq`.
   - That is twelve new standard SVGs in the house style (24 grid, 1.2 monoline, `currentColor`), named after the role's last part (`left-pane.svg`, `eq.svg`, …). No brand glyph is assigned: the owner assigns glyphs later.
   - Why: spec §2 item 9. Today's SF Symbols (about 45 uses, plus 7a's three pane toggles) have no role for these.
   - Cost if wrong: an icon redrawn later; the roles stay.
6. **Ruling (the pin and the fallback build).**
   - `app/Package.swift` adds `.package(url: "https://github.com/SymbioticAudio/symbiotic-design", exact: "0.2.0")`. The repo is public and answers anonymous `git ls-remote` over https, so CI's `actions/checkout` of vizdaw alone is enough: SwiftPM fetches the tag. `app/Package.resolved` is committed.
   - The products, by target:
     - `VizDAWAppModel` takes `SymbioticTokens` and `SymbioticIcons`;
     - `VizDAWEditors` takes `SymbioticTokens`;
     - `VizDAW` takes `SymbioticTokens`, `SymbioticIcons` and `SymbioticControls`, and `SymbioticBrand` unless `Context.environment["VIZDAW_NO_BRAND"] == "1"`.
     - No test target links `SymbioticBrand`, so every Swift test runs on the fallback set.
   - `VizDAWApp.init` registers the brand first thing, as `#if canImport(SymbioticBrand) Icons.register(SymbioticBrand.provider) #endif`.
   - `.github/workflows/ci.yml`'s `app` job gains a step, "App builds without the branded glyphs". It runs `VIZDAW_NO_BRAND=1 scripts/swift.sh build --scratch-path "$RUNNER_TEMP/nobrand"`, then fails if `find "$RUNNER_TEMP/nobrand" \( -name 'SymbioticBrand*.o' -o -name 'SymbioticBrand*.swiftmodule' \)` prints anything (the design repo's CI pattern).
   - Why: spec §2 item 9 ("still builds with the fallback icon set when the branded assets are absent") and the design spec's fork rule.
   - Cost if wrong: one manifest line.
7. **Ruling (the EQ slot in the model): `Strip.eq: Option<PluginSlot>`, beside the inserts, not in them.**
   - `Strip` gains `eq`, with `#[serde(default, skip_serializing_if = "Option::is_none")]`.
   - `Strip::chain() -> impl Iterator<Item = &PluginSlot>` yields the EQ, then the inserts, and `chain_mut` likewise.
   - `Mixer::slots() -> impl Iterator<Item = (StripId, &PluginSlot)>` yields every strip's chain, by strip ID.
   - `Mixer::inserts()` is unchanged (inserts only, with their insert positions): `find_slot` and `insert_owner` keep their meaning.
   - `SlotOwner::Eq(StripId)` is new. `find_slot` checks it before the inserts; `plugin_slot`, `slot_mut` and `referenced_states` see it.
   - Every "all slots" reader switches to `slots()` or `chain()`:
     - `core-host` `loads_needed` and its sync;
     - `mixer_rules`' four insert loops;
     - `core-ffi`'s `slot_ids`, `plugins.rs` (two), `host.rs`' `has inserts`, `surfaces.rs`' `chain`, and the Kontrol page's first plug-in.
   - **`MixerCmd::SetEq { strip, plugin: Option<PluginRef> }`:**
     - `Some(p)` allocates a new instance ID and replaces any current EQ (the old one in `Report.removed`, the new one in `created`). Its title is "Load EQ".
     - `None` empties the slot, with title "Remove EQ". On an empty slot it is refused with `invalid("the EQ slot is empty")`.
   - `MoveInsert` and `RemoveInsert` of the EQ's instance are refused with `invalid("the EQ slot is not an insert")`. An insert never moves into the EQ slot: insert indices count inserts only, so `AddInsert { index: 0 }` is the first insert after the EQ.
   - `SetBypass`, `SetPluginParams` and `SetSidechain` work on the EQ, through `slot_mut`. A dynamic EQ may take a sidechain.
   - Removing a strip clears an EQ's sidechain from it (`Mixer::remove` walks `chain_mut`).
   - Every strip kind, master included, has an EQ slot.
   - Undo comes from the document history, as for every command. `core-ffi`'s state capture covers the EQ once `slot_ids` includes it (B2).
   - Why: spec §2 item 5 ("a real, saved part of a strip … survives reordering the other inserts. It processes before the other inserts. It cannot be moved among them, and it can be emptied"). A field, not "insert 0 with a flag", makes the rule structural: no insert index can reach it.
   - Cost if wrong: every slot reader is listed here; a missed one fails `every_slot_lookup_sees_the_eq` or `an_eq_slot_is_loaded_as_an_effect`.
8. **Ruling (format `VERSION` 4).**
   - `VERSION` 3 → 4. `MIGRATIONS` gains `migrate_v3_to_v4`, which returns the document unchanged: a strip without `eq` has an empty slot.
   - An older VizDAW then refuses a version-4 project with its clear `NewerFormat` message, instead of `deny_unknown_fields`' "unknown field `eq`" on only the projects that use an EQ.
   - The tests that pin 3 move to `VERSION`: `golden` and `v1_plugin_instruments_migrate_to_slots_and_strips` (`file.rs`), `mixer_document.rs:126`, and `learn.rs:207`, which becomes `assert_eq!(VERSION, 4)` with its comment.
   - Why: spec §2 item 5 ("a small Rust core, FFI and document-format change").
   - Cost if wrong: one constant and one identity migration.
9. **Ruling (the engine): the EQ is the first `InsertSpec` of its strip.**
   - `graph::mixer::mixer_spec` iterates `s.chain()` where it iterated `s.inserts`. Latency, bypass, PDC, adoption and sidechains then treat the EQ as an insert at chain position 0. Nothing on the real-time thread changes.
   - Why: "it processes before the other inserts".
   - Cost if wrong: one loop.
10. **Ruling (strip kinds for colour): `StripColourRow`, computed in `core-ffi`.**
    - The cases are `Channel`, `Instrument`, `AuxReturn`, `Bus` and `Master`.
    - The kinds map as follows:
      - `Master` → `Master`;
      - `Instrument`, `InstrumentOutput` and `BuiltinSynth` → `Instrument`;
      - `AudioPath` → `Channel`;
      - `Bus` → `Bus` when any strip's `output` is `Bus(it)`; else `AuxReturn` when at least one send targets it; else (it has no feed at all) `Bus`.
    - Instrument strips are channels with their own token, `strip.instrument`, which the design spec already has. The spec's four cap kinds become five.
    - Why: spec §2 item 8 ("a bus fed only by sends is coloured as an aux return, and any other bus as a bus"). Routing logic stays in Rust, where `state.get mixer` reads it too.
    - Cost if wrong: one function.
11. **Ruling (the control channel).**
    - **`ui.theme`** (`key_only`, menu "View", title "Theme", no document needed) takes `theme`, a required Choice of `classic-light|studio-dark|flat|console|symbiotic`. Its answer's status is `"theme ‹id›"`. The registry goes from 153 to 154.
    - **`state.get mixer`** answers `ControlResult::Mixer { doc, mixer: MixerSnapshot }`, in this JSON shape:
      ```json
      {"doc": "…", "strips": [{"id": 3, "name": "Built-in Synth", "kind_label": "Built-in Synth",
        "colour": "instrument", "gain_db": 0.0, "pan": 0.0, "mute": false, "solo": false,
        "output": null, "automation": "read",
        "eq": {"slot": 9, "name": "…", "bypass": false} | null,
        "inserts": [{"slot": 10, "name": "…", "bypass": false}],
        "sends": [{"target": 7, "level_db": -6.0, "pre_fader": false, "enabled": true}]}, …]}
      ```
      `colour` is `channel`, `instrument`, `aux_return`, `bus` or `master`.
    - **`state.get windows`** gains `"theme": "‹id›"`, the app's theme, because the theme is app-wide.
    - **No EQ command on the channel.** The registry has no strip or insert command today; the mixer is driven by `automation.gesture` and the UI. The e2e loads no plug-ins (`--no-scan`, offline), so an EQ command could not be exercised end to end. The EQ is covered by `core-ffi`'s test plug-ins (B2) and README step 214. Should a later plan add strip commands, `mixer.eq` joins them.
    - Why: spec §4 ("breaking agents and tests": state the channel can read is updated, not dropped).
    - Cost if wrong: one more registry entry.
12. **Ruling (the theme preference and where it applies).**
    - **The preference:** `AppearancePreferences(defaults:)` (`VizDAWAppModel`) stores `AppearanceSettings { theme: String, colourBars: Bool }` as JSON under the key `"appearance"`.
      - Decoding is tolerant, as `AudioSettings`' is. An unknown id or bad bytes give `classic-light` and `colourBars` true, and nothing is written back until `choose`.
      - `AppearancePreferences.defaultID == "classic-light"`.
    - **The SwiftUI root:** every scene's root view and every `NSHostingView` the app makes (plug-in windows, torn editors, the Modify panel) applies `ThemedRoot` (`Theming.swift`). It sets:
      - `.symbioticTheme(theme)`, which sets `\.symbioticTheme` and `\.colorScheme`;
      - `.foregroundStyle(text.primary, text.secondary, text.disabled)`, so SwiftUI's `.primary`, `.secondary` and `.tertiary` are the theme's;
      - `.tint(accent.accent)`;
      - `.background(surface.window)`.
    - **AppKit windows:** `ThemeWindows.apply(_:)` sets every VizDAW `NSWindow`'s `appearance` to `NSAppearance(named: theme.appearance.nsAppearanceName)` and its `backgroundColor` to `surface.window`.
      - `AppearancePreferences.onChange` calls it on each change.
      - `ThemeWindows.style(_ window:)` styles each window as it is made, through the paths that make them: `PluginWindows.show`, `EditorWindows` and `ModifyPanels`.
    - **Canvases:** `EditorCanvas` and every other `NSView` in `VizDAWEditors` gain `var theme: Theme` (redraw on set). Each host's `updateNSView` sets it from `context.environment.symbioticTheme`.
    - **Colour bars:** the spec's "optional colour bar per channel" shows the strip kind's colour. Settings › Appearance › "Show channel colour bars" (`colourBars`) turns it off. The model has no per-channel colour (§3: no other format change), so "User track colours" stays in the backlog.
    - Why: spec §2 item 6 ("chosen in Settings, applies to the whole app including the mixer, and switches live") and §5 ("a user preference, not a project setting. Tests never touch the real preferences").
    - Cost if wrong: one modifier.
13. **Ruling (the colour and metric sweep, and its scan).**
    - Every colour in `app/Sources/VizDAW` and `app/Sources/VizDAWEditors` comes from the theme. The only files that may name a concrete colour are the two bridges, `app/Sources/VizDAW/Theming.swift` and `app/Sources/VizDAWEditors/EditorTheme.swift`.
    - `noColourLiteralsOutsideTheThemeBridges` (B5) fails on any of the following elsewhere:
      - `NSColor.<name>`, `Color.<name>` (`Color.clear` and `NSColor.clear` excepted);
      - `Color(red:`, `NSColor(red:`, `NSColor(calibrated`;
      - `Color.accentColor`;
      - a bare SwiftUI colour argument such as `.red`, `.green`, `.orange`, `.yellow`, `.purple`, `.gray`, `.black` or `.white` inside `fill(`, `tint(`, `foregroundStyle(`, `stroke(` or `background(`.
    - `noCornerRadiusOrFontSizeLiterals` (B5) fails on `cornerRadius: <number>` and `.system(size: <number>` in those two directories: radii come from `theme.radius` and sizes from `theme.type`.
    - Spacing comes from `theme.spacing` in the views 7b redraws: the strip, `MixerView`, `MixerBar`, `DocumentTransportBar`, `PaneFrame` and Settings › Appearance. Review checks this; the scan does not. The other views' padding literals go to 7d's list (backlog row, B8).
    - The editor colour roles, all in `EditorTheme.swift`:

      | Role | Token |
      |---|---|
      | canvas background | `surface.inset` |
      | ruler and header background | `surface.panel` |
      | bar line / beat line | `graph.grid` / `graph.grid` at 0.35 alpha |
      | separators | `surface.separator` |
      | text / secondary text | `text.primary` / `text.secondary` |
      | selection fill / text on it | `surface.selection` / `text.inverse` |
      | region and note fill / outline / selected | `graph.fill` / `graph.line` / `graph.handleSelected` |
      | velocity and controller bars | `graph.line` |
      | playhead | `state.record` |
      | arm R / mute M / solo S | `state.record` / `state.mute` / `state.solo` |
      | warning / error / offline | `state.warning` / `state.error` / `state.offline` |
      | piano white keys / black keys | `surface.raised` / `text.primary` |
      | lane meters | `meter.*`, by `StripPalette.meter` |

    - Why: spec §4 ("Every colour and metric comes from theme tokens, so switching the theme changes everything").
    - Cost if wrong: one table.
14. **Ruling (the strip's layout and controls).** One `ChannelStrip` view, 120 points wide (`StripLayout.width`), in every placement. Top to bottom:
    1. **Colour bar:** 4 points, the kind's `strip.*` colour, hidden when `colourBars` is off. Then the name: double-click renames; the context menu is today's (Rename…, Set All Strips to, Remove Bus, MIDI Learn Maps…, Add Outputs); the kind label is below it.
    2. **Input:** today's `StripInputView`, for an audio path's input lane. The instrument's own slot stays above it, as today.
    3. **EQ slot:** empty, it reads exactly `EQ ＋` (U+FF0B) and a click opens the effect browser for `.eq(strip:)`. Loaded, it shows the plug-in's name and a bypass toggle; a click opens its plug-in window. Its menu has Open, Replace…, Bypass/Turn On, Sidechain, and Remove EQ.
    4. **Inserts:** today's `SlotView` rows (reorder, drop, sidechain, Load, Remove) restyled, with "+" below.
    5. **Sends:** one row per send.
       - A small knob (28), arc `control.send`, unipolar over `MixerMath.faderPosition`.
       - The target's name, and a number box with the level (`StripEntry.levelText`).
       - Pre-fader shows as a `PRE` chip. An off send dims its knob and strikes its name through.
       - The context menu holds Pre-Fader, On, MIDI Learn and Remove Send: today's Pre and On toggles move there, and stay reachable.
       - Then the "Send" menu to add one.
    6. **Output picker:** a menu "→ ‹Master or bus›" listing Master and the buses (`targets(for:)`). No groups or VCAs (the spec's controller's call; linked fader groups go to the backlog).
    7. **Pan:** a medium knob (38), arc `control.pan`, bipolar from 0.5, and a number box (`StripEntry.panText`: `L 24`, `C`, `R 10`). Double-click resets to C. It is absent on master.
    8. **Fader section:** the dB scale (`StripLayout.faderMarks`), then the fader (cap colour `StripPalette.cap(kind)`), then the meter (14 points, `meter.*`).
       - Double-click on the cap gives 0 dB.
       - The throw is `StripLayout.split(height:upper:)`'s `throwLength`, from 160 to 480.
       - When the placement is too short for the upper part plus the minimum throw, the whole strip scrolls vertically (`scrolls`).
    9. **Level box:** `StripEntry.levelText` (`−4.2 dB`, `0.0 dB`, `−∞ dB`).
    10. **M S R I:** four 24×24 toggles in one row.
        - M is `state.mute` and S is `state.solo`, which master lacks.
        - R arms the strip's input lane and I cycles its monitor, through `MixerModel.toggleArm` and `cycleMonitor` (5d-2 Ruling 4). Both are disabled when `inputLane(for:)` is nil.
        - Each toggle keeps its touch ring and MIDI Learn item, as today.
    11. **Automation mode:** today's menu, tinted with `StripPalette.automation(mode)`.
    - **Accessibility labels** (the e2e finds them):
      - "Level of ‹name›", "Pan of ‹name›" and "Send ‹n› of ‹name›" (n from 1), for the number boxes, both idle and editing;
      - "Fader of ‹name›" and "Pan knob of ‹name›";
      - "EQ of ‹name›", whose value is "empty" or the plug-in's name.
    - Every control keeps its touch ring, `LearnItem` and help text from today's strip.
    - Why: spec §2 item 5 and §4 ("controls too small to use").
    - Cost if wrong: a view rearranged; the models stay.
15. **Ruling (typed values): a typed value is one click gesture; the grammar is strict.**
    - **Levels and sends:** after trimming, an optional sign (`+`, `-` or U+2212 `−`), digits, optionally `.` and digits (or `.5`), then an optional `dB` (any case, with or without one space).
      - `-inf`, `−inf`, `-∞`, `−∞` (with an optional `dB`) mean −∞.
      - The value is rounded to 0.1 dB, half away from zero, and clamped to −144…+12; −144 and below read −∞.
      - Refused: exponents, a comma, `nan`, `inf` without a minus, a second sign or point, other units, control characters, and more than 32 characters.
    - **Pan:** after trimming and folding case:
      - `C`, `0`, or a signed number;
      - or `L` or `R`, an optional space, and an unsigned number.
      - The number is rounded to an integer and clamped to 0…100; L is negative. Refused: anything else (`LR`, `L-5`, `24L`, `C 5`).
    - **Applying:** an accepted value goes through `MixerModel.click(target, norm)`, so it is one undo step, as today's 0 dB double-click is.
      - The fader and sends use `AutomationLaw.faderNorm(db)`; pan uses `(pan + 1) / 2`.
      - A refusal sets `document.editor.status` and sends nothing. The messages are "Type a level in dB, like −4.2 or −inf." (`StripEntry.levelHint`) and "Type a pan like L 24, C or R 10." (`StripEntry.panHint`).
    - Why: spec §2 item 5 ("every number box can be clicked and typed into … hostile-input tested").
    - Cost if wrong: one grammar, pinned by six tests.

## Spec checks (for the owner, no plan change needed)

1. **Theme name.** §7's 7b row says "SVP Light the default". §2 item 6's controller's call names it "Classic Light" in the UI, for the clean room. The plan follows §2.
2. **No strip commands.** The control registry has no strip or insert commands today, so the EQ slot gets no channel command (Ruling 11). The channel reads it through the new `state.get mixer`.
3. **The colour bar.** "An optional colour bar per channel" (§2 item 7) has no per-channel colour to show without a format change, which §3 rules out. It shows the strip kind's colour, with a switch in Settings (Ruling 12). "User track colours" stays in the backlog.
4. **Five cap kinds.** §2 item 7 lists four fader-cap kinds; the token set already has five, adding instrument. Instrument strips get `strip.instrument` (Ruling 10).
5. **Licence (owner question).** VizDAW is GPL-3.0, and the `SymbioticBrand` product it links by default is all rights reserved. As the copyright holder the owner may wish to add a GPL §7 additional permission to VizDAW's `LICENSE` for linking `SymbioticBrand`. This plan changes no licence file. The no-brand build (Ruling 6) is what a fork ships.
6. **Metrics.** §4 says every colour *and metric* comes from tokens. This plan scans for colours, radii and font sizes everywhere, and keeps spacing on tokens in the views it redraws. The remaining views' padding literals go to 7d (Ruling 13).
7. **Hardware colours.** The Kontrol MK3's colours per track kind (`core_surface::kontrol::color`) are hardware LED values, not UI. They stay.

## File map

| File | Task | Role |
|---|---|---|
| **symbiotic-design:** `docs/mockups/0.2.0/{palettes.json,themes.html}` (new), `tools/mockup.py` (new), `tools/tests/test_mockup.py` (new) | A1 | mockups |
| `tokens/tokens.json`, `tokens/contrast.md`, `tools/{tokenlib.py,swiftgen_tokens.py}`, `tools/tests/{test_tokenlib.py,test_swiftgen_tokens.py}`, `Sources/SymbioticTokens/Generated/*`, `Tests/SymbioticTokensTests/ThemeTests.swift`, `README.md` | A2 | tokens 0.2.0 |
| `icons/roles.json`, `icons/standard.json`, `icons/standard/*.svg` (12 new), generated icons, references, `icons/contact-sheet.html`, `icons/coverage.md`, `tools/tests/test_rolelib.py`, `Tests/SymbioticIconsTests/RoleTests.swift` | A3 | icon roles |
| `Package.swift`, `Sources/SymbioticControls/*` (new), `Tests/SymbioticControlsTests/*` (new), `.github/workflows/ci.yml`, `README.md`, `HANDOFF.md`, `docs/superpowers/specs/2026-10-20-design-system-design.md` (§7 addendum) | A4 | controls, release |
| **vizdaw:** `crates/core-model/src/{mixer.rs,document.rs,mixer_rules.rs,file.rs,command/mixer.rs}`, `crates/core-engine/src/graph/mixer.rs`, `crates/core-host/src/host.rs`; tests `core-model/tests/{mixer_commands.rs,file.rs,mixer_document.rs,learn.rs}`, `core-engine/tests/graph_mixer.rs`, `core-host/tests/host_loads.rs` | B1 | EQ slot core |
| `crates/core-ffi/src/{mixer.rs,session.rs,plugins.rs,host.rs,surfaces.rs,control.rs}`, `crates/core-control/src/request.rs`, tests `core-ffi/tests/{mixer.rs,control.rs}`, `core-control/tests/commands.rs`; `app/Tests/VizDAWAppModelTests/FakeSession.swift`, `app/Sources/VizDAWAppModel/ControlCommands.swift` | B2 | EQ FFI, colours, `state.get mixer` |
| `app/Package.swift`, `app/Package.resolved` (new), `.github/workflows/ci.yml`, `app/Sources/VizDAWAppModel/AppIcon.swift` (new), `app/Sources/VizDAW/*` (icon sites, `VizDAWApp.swift`), `app/Tests/VizDAWAppModelTests/AppIconTests.swift` (new) | B3 | pin, icons |
| `crates/core-control/src/registry.rs`, `crates/core-ffi/src/control.rs`, `core-control/tests/commands.rs`; `app/Sources/VizDAWAppModel/{AppearancePreferences.swift,AppearanceCommands.swift}` (new), `ControlCommands.swift`; `app/Sources/VizDAW/{Theming.swift,AppearanceSettingsView.swift}` (new), `VizDAWApp.swift`, `ControlChannel.swift`, `PluginWindows.swift`, `EditorWindows.swift`, `ModifyPanel.swift`; `app/Tests/VizDAWAppModelTests/AppearanceTests.swift` (new) | B4 | themes, Settings, `ui.theme` |
| `app/Sources/VizDAWEditors/EditorTheme.swift` (new) and every file the scan names; `app/Sources/VizDAW/*` colour sites; `app/Tests/VizDAWEditorsTests/ThemeScanTests.swift` (new) | B5 | colour sweep |
| `app/Sources/VizDAWAppModel/{StripEntry.swift,StripLayout.swift,StripPalette.swift}` (new), `MixerModel.swift`; `app/Tests/VizDAWAppModelTests/{StripEntryTests.swift,StripLayoutTests.swift,MixerStripTests.swift}` (new) | B6 | strip models |
| `app/Sources/VizDAW/ChannelStrip.swift` (new), `MixerView.swift`, `PaneViews.swift` (7a's `LeftStripPane`) | B7 | strip view |
| `crates/vizctl/tests/e2e.rs`, `app/README.md`, `README.md`, `docs/feature-backlog.md` | B8 | e2e, READMEs, backlog |

---

## Part A — `symbiotic-design` 0.2.0 (repo `~/workspace/symbiotic-design`)

### Task A1: The theme mockups and palettes

**Files:**
- Create: `docs/mockups/0.2.0/palettes.json`, `docs/mockups/0.2.0/themes.html` (generated), `tools/mockup.py`, `tools/tests/test_mockup.py`

**Interfaces:**
- Consumes: `tools/tokenlib.py`'s `load` and `flat_values`; `tokens/tokens.json`'s groups.
- Produces (for A2 and Gate G1):
  - `palettes.json`: `{"themes": [ {id, name, appearance, values: {<every 0.1.0 group>, knob, fader, relief}} ×5 ]}`, in the id order `classic-light`, `studio-dark`, `flat`, `console`, `symbiotic`;
  - `tools/mockup.py`: `def css_name(group: str, token: str) -> str` (`("strip", "auxReturn")` → `"--strip-aux-return"`); `def render(palettes: dict) -> str`; and `main(argv)`, as `mockup.py IN OUT`;
  - `NEW_GROUPS = {"knob": [...], "fader": [...], "relief": [...]}`, the token names of Ruling 3, which A2's schema must match.

- [ ] **Step 1: Make the branch.** `git switch -c sp7-0.2.0 b209d2c`, then check `git log -1 --oneline` reads `b209d2c`.
- [ ] **Step 2: Write the failing tests** in `tools/tests/test_mockup.py` (`unittest`):
  - `test_every_theme_has_every_token`: for each of the five themes in `palettes.json`, the set of `group.token` keys equals `tokens.json`'s groups plus `NEW_GROUPS`. Every value is `#RRGGBB` or `#RRGGBBAA` (no `system`). `name` is one of the five exact names.
  - `test_css_names_are_kebab_case_and_unique`: `css_name("strip", "auxReturn") == "--strip-aux-return"`, `css_name("relief", "capTop") == "--relief-cap-top"`, and no two tokens map to one name.
  - `test_the_page_uses_only_theme_variables`: `render(palettes)` contains one `[data-theme="<id>"]` block per theme. Outside those blocks the page has no `#hex`, `rgb(`, `hsl(`, `url(` or `<img`, and its only `font-family` is the system stack. Every `var(--…)` it uses is defined in each block.
- [ ] **Step 3: Run them and see them fail.** `python3 -m unittest tools.tests.test_mockup -v`. Expected: an import error, because `tools/mockup.py` does not exist.
- [ ] **Step 4: Write `tools/mockup.py`, `palettes.json` and the page.**
  - Follow Ruling 2's content and per-theme direction.
  - `symbiotic` copies today's values and adds the new groups.
  - Every theme but Console has all six `relief` tokens `#00000000`. Console's are real and drawn with CSS `linear-gradient` and `box-shadow`, and nothing else.
  - Text on every surface meets 4.5:1 (A2 will test it).
  - Then run `python3 tools/mockup.py docs/mockups/0.2.0/palettes.json docs/mockups/0.2.0/themes.html`.
- [ ] **Step 5: Run the tests and see them pass**, then the whole Python suite: 177 (174 + 3). `python3 tools/generate.py` leaves no diff, because the generator does not read the mockups.
- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd ~/workspace/symbiotic-design
git add docs/mockups/0.2.0/palettes.json docs/mockups/0.2.0/themes.html tools/mockup.py tools/tests/test_mockup.py
git commit -m "Mockups: the five 0.2.0 themes on VizDAW's window and strip, palettes as named tokens

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --format='%h %an <%ae> %s' && git show --stat HEAD | tail -6
```

- [ ] **Step 7: Report** the absolute path of `themes.html` and the SHA for Gate G1. **Stop: Gate G1.**

---

### Task A2: Tokens 0.2.0 from the approved palettes

**Files:**
- Modify: `tokens/tokens.json`, `tools/tokenlib.py` (`validate`: `name`, `since`), `tools/swiftgen_tokens.py` (name, defaulted init parameters, the `classicLight` statics), `tools/tests/test_tokenlib.py`, `tools/tests/test_swiftgen_tokens.py`, `Tests/SymbioticTokensTests/ThemeTests.swift`, `README.md` (Themes, Checking by eye)
- Regenerated: `Sources/SymbioticTokens/Generated/*`, `tokens/contrast.md`

**Interfaces:**
- Consumes: the G1-approved `palettes.json` (its SHA in `progress.md`), and `NEW_GROUPS` (A1).
- Produces (Swift, `SymbioticTokens`, all public):
  - `Theme.name: String`;
  - `Theme.knob: KnobColors { skirt, cap, track, indicator }`;
  - `Theme.fader: FaderColors { well, scale, capLine }`;
  - `Theme.relief: ReliefColors { highlight, shadow, capTop, capBottom, panelTop, panelBottom }`;
  - the statics `KnobColors.classicLight`, `FaderColors.classicLight`, `ReliefColors.classicLight`;
  - `Theme.init(id:name:appearance:isPlaceholder:…:knob:fader:relief:)`, where `name`, `knob`, `fader` and `relief` have defaults (Ruling 3).

- [ ] **Step 1: Write the failing Python tests.**
  - `test_tokenlib.py`:
    - `test_a_theme_needs_one_unique_name`: missing, empty, 33 characters, a newline, or two themes with the same name are each one problem.
    - `test_since_marks_a_group`: `"since": "0.2.0"` is accepted, `"since": 2` is a problem, and an unknown group key is still refused.
    - `test_no_theme_ships_as_a_placeholder`: in the real `tokens.json` every theme has `placeholder` false and no `system` value.
    - `test_body_text_reads_on_every_surface`: in each theme, `text.primary` on `surface.window`, `panel`, `raised` and `inset` has `contrast_ratio ≥ 4.5`.
  - `test_swiftgen_tokens.py`:
    - `test_since_groups_and_the_name_have_defaults`: the generated `init` reads `name: String? = nil`, `knob: KnobColors = .classicLight`, `fader: FaderColors = .classicLight` and `relief: ReliefColors = .classicLight`, and `Theme.name` falls back to `id`.
- [ ] **Step 2: Write the failing Swift tests** in `ThemeTests.swift`:

```swift
@Test func themesHaveTheirNamesInOrder() {
    #expect(Theme.all.map(\.name) == ["Classic Light", "Studio Dark", "Flat", "Console", "Symbiotic"])
    #expect(Theme.all.allSatisfy { !$0.isPlaceholder })
}
@Test func theNewGroupsReadAsTyped() {
    #expect(Theme.console.relief.capTop != Theme.console.relief.capBottom)
    #expect(Theme.studioDark.knob.skirt.fallbackComponents.alpha == 1)
}
@Test func flatThemesHaveClearRelief() {
    for t in Theme.all where t.id != "console" {
        let r = t.relief
        #expect([r.highlight, r.shadow, r.capTop, r.capBottom, r.panelTop, r.panelBottom]
            .allSatisfy { $0.fallbackComponents.alpha == 0 })
    }
}
@Test func aZeroOneThemeStillBuilds() {
    // EG13's call shape at 0.1.0: no name, knob, fader or relief.
    let t = Theme(id: "eg13", appearance: .dark, surface: Theme.studioDark.surface, text: Theme.studioDark.text,
        accent: Theme.studioDark.accent, state: Theme.studioDark.state, meter: Theme.studioDark.meter,
        graph: Theme.studioDark.graph, control: Theme.studioDark.control, strip: Theme.studioDark.strip,
        brand: Theme.studioDark.brand, spacing: Theme.studioDark.spacing, radius: Theme.studioDark.radius,
        iconStroke: Theme.studioDark.iconStroke, type: Theme.studioDark.type)
    #expect(t.name == "eg13")
    #expect(t.relief == ReliefColors.classicLight)
}
```

- [ ] **Step 3: Run them and see them fail.** Run `python3 -m unittest discover -s tools/tests` (the five new tests fail) and `scripts/swift.sh test` (the new tests fail to compile, because `Theme.name` is not defined).
- [ ] **Step 4: Implement.**
  - Add `name` to each theme. Add the three groups to `tokens` with `"since": "0.2.0"` and their one-line descriptions (Ruling 3).
  - Copy the five approved themes' `values` from `palettes.json` into `themes`, set `placeholder: false`, and replace each `note` with one line naming its direction.
  - `tokenlib.validate` checks `name` and accepts `since`. `swiftgen_tokens` emits the defaults and the statics.
  - In `README.md`, the Themes section names the five themes by name with their ids. The "Checking by eye" bullet on Increase Contrast becomes: "Read `tokens/contrast.md`: body text passes AA on every surface in every theme (tested)."
  - Then run `python3 tools/generate.py`.
- [ ] **Step 5: Run everything and see it pass.** Python 182 (177 + 5), Swift 50 (46 + 4); `git status --porcelain` lists only this task's files.
- [ ] **Step 6: Commit** the files above, plus the regenerated `Sources/SymbioticTokens/Generated/*` and `tokens/contrast.md`. Message: "Tokens 0.2.0: the approved palettes, theme names, knob, fader and relief groups, no placeholders", with the trailers. Check the author line as in A1.

---

### Task A3: The icon roles VizDAW needs

**Files:**
- Create: `icons/standard/{left-pane,bottom-pane,right-pane,mixer,split,empty,keys,audio,surfaces,appearance,info,eq}.svg`
- Modify: `icons/roles.json`, `icons/standard.json`, `tools/tests/test_rolelib.py`, `Tests/SymbioticIconsTests/RoleTests.swift`
- Regenerated: `Sources/SymbioticIcons/Generated/*`, the reference PNGs (`python3 tools/references.py`), `icons/contact-sheet.html`, `icons/coverage.md`

**Interfaces:**
- Produces: the `IconRole` cases `.viewLeftPane`, `.viewBottomPane`, `.viewRightPane`, `.viewMixer`, `.viewSplit`, `.viewEmpty`, `.viewExpand`, `.viewCollapse`, `.settingsKeys`, `.settingsAudio`, `.settingsSurfaces`, `.settingsAppearance`, `.statusInfo` and `.mixerEq`. Use the generator's own spelling of each case and report it if it differs.

- [ ] **Step 1: Write the failing tests.**
  - `test_rolelib.py`'s `test_vizdaw_sp7_roles_exist`: the fourteen ids of Ruling 5 are in `roles.json`, each with its `standard` icon present in `standard.json`, a one-line `label`, and no `brand`.
  - `test_rolelib.py`'s `test_coverage_briefs_the_new_roles`: the generated `coverage.md` lists all fourteen under "Brief for the designer" (roles with no brand glyph).
  - `RoleTests.swift`'s `sp7RolesDrawTheirStandardIcons`: for each of the fourteen roles, `NSImage.icon(role, size: 16, theme: .classicLight)` is a template image of size 16×16 that is not blank (some pixel's alpha is above 0).
- [ ] **Step 2: Run them and see them fail.**
- [ ] **Step 3: Draw the twelve SVGs** in the SVG subset (design spec §3.1).
  - Pane icons are a window outline with the named side filled. `mixer` is three fader slots with caps; `split` is two stacked rectangles; `empty` is a dashed rectangle; `keys` is a keyboard; `audio` is a speaker; `surfaces` is a fader-and-knob control surface; `appearance` is a half-filled circle; `info` is an "i" in a circle; `eq` is a curve over three band dots.
  - Add the roles. Then regenerate everything (`generate.py`, `references.py`, `contact_sheet.py`).
- [ ] **Step 4: Run everything and see it pass.** Python 184, Swift 51. The fidelity tests cover the new icons through their existing arguments.
- [ ] **Step 5: Commit** the SVGs, JSON, generated Swift, reference PNGs, contact sheet, coverage and the two test files. Message: "Icons: fourteen roles for VizDAW SP7 (panes, mixer, EQ, settings, info) with standard icons", with the trailers.

---

### Task A4: `SymbioticControls`, the HANDOFF, and the 0.2.0 tag

**Files:**
- Modify: `Package.swift` (the product, the target `SymbioticControls` depending on `SymbioticTokens`, and the test target `SymbioticControlsTests`), `.github/workflows/ci.yml` (the fork step also runs `swift build --target SymbioticControls`), `README.md` (the products table, "Controls"), `HANDOFF.md`, `docs/superpowers/specs/2026-10-20-design-system-design.md` (a short "§7 Controls (0.2.0)" addendum)
- Create: `Sources/SymbioticControls/{ControlMetrics.swift,KnobGeometry.swift,FaderGeometry.swift,ControlDrag.swift,NumberEntry.swift,Palettes.swift,Knob.swift,Fader.swift,NumberBox.swift}`, `Tests/SymbioticControlsTests/{GeometryTests.swift,EntryAndPaletteTests.swift}`

**Interfaces:**
- Consumes: `Theme`'s `knob`, `fader`, `relief`, `control`, `strip`, `surface`, `text`, `accent` and `type` (A2).
- Produces (`SymbioticControls`, all public and `Sendable` where they are values):
  - **Metrics:** `enum KnobSize { case small, medium, large }`, and `enum ControlMetrics` with:
    - `static func knobDiameter(_ s: KnobSize) -> CGFloat` (28, 38, 48);
    - `static func capRatio(_ s: KnobSize) -> CGFloat`;
    - `static let sweep = 270.0`;
    - `static let faderCap = CGSize(width: 36, height: 52)`;
    - `static let numberBox = CGSize(width: 56, height: 22)`;
    - `static let minimumHit: CGFloat = 24`;
    - `static let dragTravel: CGFloat = 200`;
    - `static let fineDivisor = 10.0`.
  - **Knob geometry:** `enum KnobGeometry` with:
    - `static func angle(_ value: Double) -> Angle` (0 → 225°, 1 → −45°, measured as SwiftUI's degrees from 3 o'clock, counter-clockwise positive);
    - `static func arc(origin: Double, value: Double) -> (from: Double, to: Double)` (values in 0…1, ordered);
    - `static func indicator(diameter: CGFloat, size: KnobSize, value: Double) -> CGRect` (its unrotated frame) and `static func band(diameter: CGFloat, size: KnobSize) -> CGFloat`.
  - **Fader geometry:** `enum FaderGeometry` with:
    - `static func capCentre(value: Double, throwLength: CGFloat) -> CGFloat`, where 0 is the bottom of the throw;
    - `static func value(capCentre: CGFloat, throwLength: CGFloat) -> Double`, clamped to 0…1.
  - **Drag:** `enum ControlDrag` with `static func value(start: Double, translation: CGFloat, travel: CGFloat, fine: Bool) -> Double`. Upward is positive (`translation` is SwiftUI's height, so it is negated inside); the result is clamped to 0…1.
  - **Entry:** `enum NumberEntry` with `static let maxLength = 32` and `static func clean(_ s: String) -> String?`. `clean` trims whitespace, maps U+2212 to `-`, and gives nil for more than `maxLength` characters or any control character.
  - **Palettes:**
    - `struct KnobPalette { skirt, plainCap, darkCap, track, tick, indicator, indicatorOnDark, label, chipText, focus: Color; init(theme: Theme) }`;
    - `struct FaderPalette { well, scale, capLine: Color; init(theme: Theme) }`;
    - `struct NumberBoxPalette { background, editingBackground, text, refused, focus: Color; init(theme: Theme) }`;
    - `struct Relief: Equatable { highlight, shadow, capTop, capBottom: Color; static let none; init(theme: Theme); var isNone: Bool }`.
  - **Knob style:** `struct KnobStyle { var size: KnobSize; var arc: ArcPlacement = .skirtBand; var innerRing = false; var ticks = false; var cap: CapEmphasis = .plain; var label: LabelStyle = .text }`, with `enum ArcPlacement { case off, skirtBand, outside }`, `enum CapEmphasis { case plain, accent, dark }` and `enum LabelStyle { case text, chip }`.
  - **Views:**
    - `struct Knob: View { init(value: Double, origin: Double = 0, arcColour: Color, style: KnobStyle, palette: KnobPalette, relief: Relief = .none, label: String? = nil, accessibilityLabel: String, accessibilityValue: String, step: Double = 0.01, onBegin: @escaping () -> Void, onChange: @escaping (Double) -> Void, onEnd: @escaping () -> Void, onReset: (() -> Void)? = nil) }`;
    - `struct FaderMark: Equatable { var position: Double; var label: String }`;
    - `struct Fader: View { init(value: Double, throwLength: CGFloat, cap: Color, palette: FaderPalette, relief: Relief = .none, marks: [FaderMark], accessibilityLabel: String, accessibilityValue: String, step: Double, onBegin:, onChange:, onEnd:, onReset:) }`;
    - `struct NumberBox: View { init(text: String, accessibilityLabel: String, palette: NumberBoxPalette, commit: @escaping (String) -> Bool) }`. `commit` returning false keeps the old text shown.

- [ ] **Step 1: Write the failing tests** (Swift Testing, in `SymbioticControlsTests`).
  - `GeometryTests.swift`:
    - `knobSizesAndCapsFollowTheHandoff`: diameters 28/38/48, and cap ratios 0.618/0.618/0.676 within 0.001.
    - `theSweepRunsFromSevenThirtyToFourThirty`: `angle(0)` is 225°, `angle(1)` is −45° and `angle(0.5)` is 90°, each within 1e-9.
    - `aBipolarArcFillsFromItsOrigin`: `arc(origin: 0.5, value: 0.2) == (0.2, 0.5)` and `arc(origin: 0, value: 0.7) == (0, 0.7)`.
    - `theIndicatorIsSizedFromTheCap`: for large at 48, the width is `max(2, 0.07 × 48)` and the height is `0.42 × 48 × 0.676`, within 0.01.
    - `aDragIsRelativeClampedAndFine`: `value(start: 0.5, translation: -100, travel: 200, fine: false)` is 1.0 (100 up is +0.5); with `fine: true` it is 0.55; `translation: +1000` gives 0.
    - `theFaderCapRoundTrips`: for every value in 0, 0.1 … 1, `value(capCentre: capCentre(v, 300), 300)` equals `v` within 1e-9; values outside clamp.
    - `everyControlMeetsTheMinimumHit`: `knobDiameter(.small) + padding ≥ minimumHit`, `faderCap.width ≥ minimumHit`, and `numberBox.height + padding ≥ minimumHit`, where `padding` is the views' own `hitPadding` constant.
  - `EntryAndPaletteTests.swift`:
    - `numberEntryCleansAndRefuses`: `clean("  −4.2 dB ") == "-4.2 dB"`; `clean(String(repeating: "1", count: 33)) == nil`; `clean("4\u{0}2") == nil`; `clean("") == ""`.
    - `palettesComeFromTheTheme`: `KnobPalette(theme: .console).skirt == Theme.console.knob.skirt.color` and `FaderPalette(theme: .flat).well == Theme.flat.fader.well.color`.
    - `reliefIsNoneOutsideConsole`: `Relief(theme: t).isNone` for every theme but Console, and false for Console.
    - `aNumberBoxRefusalKeepsTheOldText`: through `NumberBoxModel`, the view's internal editing state, exposed `internal` for tests. Begin, type "abc", commit with a `commit` returning false: `shown == "0.0 dB"` and `editing == false`. A commit returning true shows the caller's next `text`.
    - `escapeCancelsEditing`: `NumberBoxModel` begin, type "1", cancel: `shown` is unchanged and `commit` was not called.
- [ ] **Step 2: Run them and see them fail** (no such module).
- [ ] **Step 3: Implement** the files above to Ruling 4.
  - Drawing uses SwiftUI `Canvas` or shapes only, with no images. The relief gradients and bevels are drawn only when `!relief.isNone`.
  - Interaction (`DragGesture(minimumDistance: 0)` with `NSEvent.modifierFlags` for ⌥, `onTapGesture(count: 2)`, `accessibilityAdjustableAction`) stays separate from drawing (HANDOFF (k)).
  - No `@State`: spell out `State(initialValue:)` (Global Constraints).
- [ ] **Step 4: Run everything and see it pass.** Swift 63 (51 + 12), Python 184. Also build the fork targets locally: `swift build --package-path . --scratch-path "$(mktemp -d)/b" --target SymbioticControls`, which is what CI runs, with `DEVELOPER_DIR` set. Expected: built, and no `SymbioticBrand` objects.
- [ ] **Step 5: Update the docs.**
  - `README.md`: the products table gains `SymbioticControls` (MIT: knob, fader, number box, geometry and interaction; colours injected through palettes), with a four-line usage example. The "Themes" text is A2's.
  - `HANDOFF.md`: a new "Status: 0.2.0 (VizDAW SP7)" section. It lists:
    - the five final themes and their names;
    - the new groups;
    - the fourteen roles;
    - `SymbioticControls`, with EG13's request 1 geometry adopted, and its palette slots matched to EG13's names (`skirt`, `plainCap`, `darkCap`, `track`, `tick`, `indicator`, `indicatorOnDark`, `label`, `chipText`, `focus`; the accent passed per knob as `arcColour`);
    - what is not in 0.2.0: EG13 requests 2–5 (the mod wave mark, the LCD readout, the mini trace, the card columns) and the category and `goldInk` tokens, which (f) dropped;
    - an invitation to EG13 to adopt `SymbioticControls` at 0.2.0 or later.
  - The design spec's §7 addendum (one paragraph): the 0.2.0 schema additions and the product.
- [ ] **Step 6: Commit** (message: "SymbioticControls 0.2.0: knob, fader and number box shared by the family; HANDOFF and README for 0.2.0", with the trailers).
- [ ] **Step 7: Tag, locally only.** `git tag -a 0.2.0 -m "symbiotic-design 0.2.0: final palettes, theme names, knob/fader/relief tokens, SP7 icon roles, SymbioticControls"`. Check with `git cat-file -t 0.2.0` (expected: `tag`). **Do not push. Stop: Gate G2.**

---

## Part B — VizDAW (repo `/Users/lattawb/workspace/vizdaw`)

### Task B1: The EQ slot in the model, the format and the engine

**Files:**
- Modify:
  - `crates/core-model/src/mixer.rs` (`Strip.eq`, `chain`, `chain_mut`, `Mixer::slots`, `Mixer::remove`), `document.rs` (`SlotOwner::Eq`, `find_slot`, `plugin_slot`, `slot_mut`, `referenced_states`), `mixer_rules.rs` (four loops → `chain()`), `command/mixer.rs` (`SetEq`, titles, refusals), `file.rs` (`VERSION`, `migrate_v3_to_v4`);
  - `crates/core-engine/src/graph/mixer.rs` (`mixer_spec`);
  - `crates/core-host/src/host.rs` (`loads_needed` and the sync's `wanted` → `slots()`).
- Test: `crates/core-model/tests/{mixer_commands.rs,file.rs,mixer_document.rs,learn.rs}`, `crates/core-engine/tests/graph_mixer.rs`, `crates/core-host/tests/host_loads.rs`

**Interfaces:**
- Produces (Ruling 7):
  - `pub eq: Option<PluginSlot>` on `Strip`, where `Strip::new` sets `eq: None`;
  - `pub fn chain(&self) -> impl Iterator<Item = &PluginSlot> + '_` and `pub(crate) fn chain_mut(&mut self) -> impl Iterator<Item = &mut PluginSlot> + '_` on `Strip`;
  - `pub fn slots(&self) -> impl Iterator<Item = (StripId, &PluginSlot)> + '_` on `Mixer`;
  - `SlotOwner::Eq(StripId)`;
  - `MixerCmd::SetEq { strip: StripId, plugin: Option<PluginRef> }`, with titles "Load EQ" and "Remove EQ";
  - `file::VERSION == 4` and `pub fn migrate_v3_to_v4(v: Value) -> Result<Value>`.

- [ ] **Step 1: Write the failing tests.**
  - `mixer_commands.rs`:
    - `the_eq_slot_loads_replaces_and_empties_as_undo_steps`:
      - on the demo's Built-in Synth strip, `SetEq { plugin: Some(r("eq-a")) }` makes `strip.eq` hold "eq-a", with `report.created == [Plugin(id1)]` and title "Load EQ";
      - `Some(r("eq-b"))` gives a new `id2 != id1`, with `removed == [Plugin(id1)]`;
      - `None` gives `eq == None`, with title "Remove EQ";
      - `None` again is `Err(ModelError::Invalid(_))`, and the document is unchanged;
      - master takes an EQ too.
    - `the_eq_slot_stays_first_when_inserts_move`: an EQ and inserts a, b, c. `MoveInsert { slot: c, index: 0 }` makes the chain ids `[eq, c, a, b]` and leaves `strip.eq` unchanged. `AddInsert { index: Some(0) }` puts the new insert after the EQ.
    - `the_eq_slot_is_not_an_insert`: `MoveInsert` and `RemoveInsert` of the EQ's id are each `Err(Invalid)` with the message "the EQ slot is not an insert". `SetBypass`, `SetPluginParams` (one finite value) and `SetSidechain` on it succeed and change it.
    - `every_slot_lookup_sees_the_eq`: `find_slot(eq) == Some(SlotOwner::Eq(strip))`, `plugin_slot(eq)` is the EQ, `mixer.slots()` yields it before that strip's inserts, and `referenced_states()` holds its state once `state` is set.
    - `removing_a_bus_clears_an_eq_sidechain_from_it`.
  - `file.rs`:
    - `an_eq_slot_round_trips_and_an_empty_one_is_not_written`: save and load equal; the JSON of a strip without an EQ has no `"eq"` key; with one it has.
    - `version_3_projects_migrate_to_version_4`: `small_doc()`'s JSON with `"version": 3` loads, every strip's `eq` is `None`, and `to_json` says `"version": 4`.
    - Move the four version pins to `VERSION` (Ruling 8).
  - `graph_mixer.rs`: `the_eq_slot_runs_before_the_inserts`. A strip with an EQ (latency 64 via the latency closure) and two inserts: `spec.inserts`' instances are `[eq, i1, i2]`, and the PDC total includes 64.
  - `host_loads.rs`: `an_eq_slot_is_loaded_as_an_effect`. `loads_needed` on a document whose only plug-in is an EQ lists its ref once, as an effect (not an instrument).
- [ ] **Step 2: Run them and see them fail.** `cargo test -p core-model -p core-engine -p core-host`. Expected: compile errors (`eq`, `SetEq` and `slots` are not defined).
- [ ] **Step 3: Implement** Rulings 7–9. Check that `mixer_rules::check_strip_values` checks the EQ's values (it walks `chain()`).
- [ ] **Step 4: Run them and see them pass,** then the workspace, both clippies, fmt, and the rt-check gates (Global Constraints). Expected: Rust +9 (7a end + 9), all five rt_alloc tests pass, and the rt-check clippy is clean.
- [ ] **Step 5: Commit** the files above. Message: "core: the EQ slot, a saved part of each strip that processes first; format version 4 (plan 7b Task B1)", with the trailers.

---

### Task B2: The EQ slot over the FFI, strip colours and `state.get mixer`

**Files:**
- Modify:
  - `crates/core-ffi/src/mixer.rs` (`StripRow.eq`, `StripRow.colour`, `StripColourRow`, `strip_colour`, `slot_ids` → `slots()`), `session.rs` (`set_eq`, the Kontrol page's first plug-in → `chain().next()`), `plugins.rs` (two loops → `slots()`), `host.rs` (`has inserts` → `slots()`), `surfaces.rs` (`chain`: the instrument, then `s.chain()`), `control.rs` (`StateTargetRow::Mixer`, `ControlResult::Mixer` and its JSON);
  - `crates/core-control/src/request.rs` (`StateTarget::Mixer`, `"mixer"`);
  - `app/Tests/VizDAWAppModelTests/FakeSession.swift` (`setEq`), `app/Sources/VizDAWAppModel/ControlCommands.swift` (`state(.mixer)`).
- Test: `crates/core-ffi/tests/{mixer.rs,control.rs}`, `crates/core-control/tests/commands.rs`

**Interfaces:**
- Consumes: B1.
- Produces:
  - `#[derive(uniffi::Enum)] pub enum StripColourRow { Channel, Instrument, AuxReturn, Bus, Master }`;
  - `StripRow { …, eq: Option<SlotRow>, colour: StripColourRow }`;
  - `pub fn strip_colour(m: &Mixer, s: &Strip) -> StripColourRow` (Ruling 10);
  - `Session::set_eq(&self, strip: u64, plugin: Option<PluginRefRow>) -> Result<DocSnapshot, VizError>`, in `ffi_guard`;
  - `ControlResult::Mixer { doc: String, mixer: MixerSnapshot }`, with Ruling 11's JSON;
  - Swift: `SessionProtocol.setEq(strip:plugin:)` (generated), `ControlCommands.state(.mixer, …)` answering `.mixer(doc:mixer:)` from `m.session.mixer()` (nil on a throw, as `.automation` does).

- [ ] **Step 1: Write the failing tests.**
  - `core-ffi/tests/mixer.rs`, with the file's `serial()` and `test_plugin`:
    - `the_eq_slot_loads_and_survives_reordering_saving_and_undo`:
      - `set_eq(synth, Some(test_plugin(...)))`: `strip.eq?.plugin.name` matches;
      - add two inserts and `move_insert` the second to 0: `strip.eq` is the same slot id, and `inserts` reorder;
      - save to the temp dir, close, open: `eq` is kept;
      - `undo` until the EQ goes, then `redo`: it is back, with `saved_state` equal to before.
    - `replacing_the_eq_keeps_the_old_state_for_undo`: load A and let it capture state, replace it with B, undo: A is back with its state (`capture_removed` sees the EQ through `slot_ids`).
    - `hostile_eq_calls_are_refused`: each of these fails and leaves `snapshot().version` unchanged:
      - `set_eq(999_999, Some(..))`;
      - `set_eq(synth, None)` on an empty slot;
      - `remove_insert(eq_id)`;
      - `move_insert(eq_id, synth, 0)`.
    - `strips_are_coloured_by_kind`:
      - the demo: Built-in Synth is `.instrument`, master is `.master` and an audio path is `.channel`;
      - `add_bus` gives `.bus` (no feed);
      - a send from the synth to it gives `.auxReturn`;
      - then setting another strip's output to it gives `.bus`.
  - `core-ffi/tests/control.rs`'s `state_get_mixer_answers_json`: `ControlResult::Mixer`'s JSON for the demo has a `strips` array whose entries hold exactly Ruling 11's keys, with `eq` null and `colour` `"instrument"` for the synth.
  - `core-control/tests/commands.rs`'s `mixer_state_target_parses`: `state.get {target: "mixer"}` parses to `StateTarget::Mixer`, and `{target: "mixers"}` is `invalid_params`.
- [ ] **Step 2: Run them and see them fail.**
- [ ] **Step 3: Implement.**
  - `strip_row` fills `eq` with `slot_row(eq, false, host)` and `colour` with `strip_colour`.
  - The JSON goes in `control.rs`, beside `Layout`'s. Its keys match Ruling 11.
  - Then the Swift side: `scripts/build-ffi.sh`, `FakeSession.setEq` (records the call, replaces or empties a fake EQ, and throws `VizError.Invalid` on an empty clear), and `ControlCommands.state`'s new `.mixer` case.
- [ ] **Step 4: Run the gates.** Rust: +6 (7a end + 15), Linux clippy clean. Swift: builds and runs the 7a-end count unchanged (`StripRow`'s new fields are in every literal `FakeSession` makes; fix those literals).
- [ ] **Step 5: Commit** the files above. Message: "ffi: the EQ slot (set_eq, StripRow.eq), strip colours by kind, state.get mixer (plan 7b Task B2)", with the trailers.

---

### Task B3: Pin `symbiotic-design` 0.2.0, the icons and the no-brand build

**Files:**
- Modify: `app/Package.swift` (Ruling 6), `.github/workflows/ci.yml` (the no-brand step), `app/Sources/VizDAW/VizDAWApp.swift` (the brand registration, the Settings tab icons), and every `VizDAW` file with `systemImage:`, `Image(systemName:)` or `NSImage(systemSymbolName:)` (`DetailPane`, `StripInputView`, `SequenceSidebar`, `ArrangementPane`, `DrumGridPane`, `RelinkSheet`, `MediaPoolWindow`, `DocumentView`, `ProjectBanners`, `ModifyPanel`, `MixerView`, `LearnMapsSheet`, `RecordControls`, `BounceSheet`, `PianoRollPane`, `DocumentShell` (7a's pane toggles), `EditorWindows`)
- Create: `app/Package.resolved`, `app/Sources/VizDAWAppModel/AppIcon.swift`, `app/Tests/VizDAWAppModelTests/AppIconTests.swift`, `app/Tests/VizDAWAppModelTests/SourceScan.swift`

**Interfaces:**
- Consumes: the tag `0.2.0` on GitHub (Gate G2); `IconRole`, `Icon(_:size:)` and `NSImage.icon(_:size:theme:)`.
- Produces: `public enum AppIcon: CaseIterable, Sendable { … ; public var role: IconRole }`, one case per meaning the app shows. These follow today's SF Symbols:
  - `warning` → `.statusWarning`, `problem` → `.statusProblem`, `fault` → `.statusFault`, `info` → `.statusInfo`;
  - `mixer` → `.viewMixer`;
  - `add` → `.editAdd`, `remove` → `.editRemove`, `confirm` → `.editConfirm`, `close` → `.editClose`;
  - `zoomIn` → `.viewZoomIn`, `zoomOut` → `.viewZoomOut`;
  - `audio` → `.inputAudio`, `drumGrid` → `.editorDrumGrid`, `pianoRoll` → `.editorPianoRoll`, `list` → `.editorList`;
  - `muted` → `.statusAudioMuted`, `loop` → `.modeLoop`, `split` → `.viewSplit`, `empty` → `.viewEmpty`;
  - `record` → `.transportRecord`, `armed` → `.recordArm`, `pencil` → `.toolPencil`, `diskSlow` → `.statusDiskSlow`;
  - `expand` → `.viewExpand`, `collapse` → `.viewCollapse`, `menuDown` → `.navDown`;
  - `leftPane`, `bottomPane`, `rightPane` → the pane roles;
  - `settingsEditing` → `.toolPencil`, `settingsKeys`, `settingsAudio`, `settingsRecording` → `.transportRecord`, `settingsAutomation` → `.editorEnvelope`, `settingsSurfaces`, `settingsAppearance`;
  - `eq` → `.mixerEq`.

- [ ] **Step 1: Pin the package.** Edit `app/Package.swift` per Ruling 6, then run `scripts/swift.sh build`: SwiftPM resolves `0.2.0` and writes `app/Package.resolved`. Check that its pin's revision equals the pushed tag's commit (`git ls-remote https://github.com/SymbioticAudio/symbiotic-design refs/tags/0.2.0^{}`).
- [ ] **Step 2: Write the failing tests** in `AppIconTests.swift`:

```swift
@MainActor @Suite struct AppIconTests {
    @Test func everyAppIconDrawsWithoutTheBrand() {
        for i in AppIcon.allCases {
            let img = NSImage.icon(i.role, size: 16, theme: .classicLight)
            #expect(img.isTemplate && img.size == NSSize(width: 16, height: 16), "\(i)")
        }
    }
    @Test func thePaneTogglesUseThePaneRoles() {
        #expect([AppIcon.leftPane, .bottomPane, .rightPane].map(\.role) == [.viewLeftPane, .viewBottomPane, .viewRightPane])
        #expect(AppIcon.eq.role == .mixerEq)
    }
    @Test func theAppUsesNoSystemSymbols() throws {
        // app/Sources, from this file's path.
        let hits = try SourceScan.lines(in: ["VizDAW", "VizDAWEditors"],
            matching: #"systemImage:|systemName:|systemSymbolName:"#)
        #expect(hits.isEmpty, "\(hits)")
    }
}
```

  `SourceScan` is a file-private helper in the test target (`SourceScan.swift`, made here and reused by B5). It walks `app/Sources/<dir>/**/*.swift` from `URL(fileURLWithPath: #filePath)` and returns `"file:line: text"` for each match of a regular expression.
- [ ] **Step 3: Run them and see them fail** (`AppIcon` is not defined).
- [ ] **Step 4: Replace every SF Symbol** with its `AppIcon`:
  - `Label(t, systemImage: s)` becomes `Label { Text(t) } icon: { Icon(AppIcon.x.role, size: 16) }`;
  - `Image(systemName:)` becomes `Icon(…, size:)`;
  - `ContentUnavailableView(title, systemImage:, description:)` becomes its label and description form with `Icon(…, size: 32)`;
  - `NSImage(systemSymbolName:)` becomes `NSImage.icon(…, size:, theme:)`.
  - Register the brand in `VizDAWApp.init`, first. Add the CI step.
- [ ] **Step 5: Run the gates:** Swift 7a end + 3; then `VIZDAW_NO_BRAND=1 scripts/swift.sh build --scratch-path .build-nobrand`, after which `find .build-nobrand -name 'SymbioticBrand*.o'` prints nothing; then `scripts/make-app.sh`; then the e2e, all passing. `.build-nobrand` is ignored: add it to `.gitignore` only if `git status` shows it.
- [ ] **Step 6: Commit** `app/Package.swift`, `app/Package.resolved`, `.github/workflows/ci.yml`, `AppIcon.swift`, `AppIconTests.swift`, `SourceScan.swift`, and the `VizDAW` files changed. Message: "app: adopt symbiotic-design 0.2.0: icons from SymbioticIcons, the brand registered, a build without it (plan 7b Task B3)", with the trailers.

---

### Task B4: Themes, Settings › Appearance, live switching and `ui.theme`

**Files:**
- Modify:
  - `crates/core-control/src/registry.rs` (`ui.theme`, Ruling 11), `crates/core-control/tests/commands.rs`;
  - `crates/core-ffi/src/control.rs` (`ControlResult::Windows.theme` and its JSON), `crates/core-ffi/tests/control.rs` (the windows JSON);
  - `app/Sources/VizDAWAppModel/ControlCommands.swift` (`handled` adds `AppearanceCommands.ids`, the dispatch, and `ControlHost.appearance`);
  - `app/Sources/VizDAW/VizDAWApp.swift` (`appearancePreferences`, `ThemedRoot` on every scene, the Appearance tab), `ControlChannel.swift` (`.windows(… theme: appearancePreferences.theme.id)`, and `AppControlHost.appearance`), `PluginWindows.swift`, `EditorWindows.swift`, `ModifyPanel.swift` (`ThemedRoot` on their hosting views, `ThemeWindows.style`);
  - every Editors host (`ArrangementHost`, `PianoRollHost`, `ListHost`, `DrumGridHost`, `StripChartHost`, `TransportFieldsHost`): `updateNSView` passes `context.environment.symbioticTheme` to its views' `theme`;
  - the test target's fake `ControlHost` (gains `appearance`).
- Create: `app/Sources/VizDAWAppModel/{AppearancePreferences.swift,AppearanceCommands.swift}`, `app/Sources/VizDAW/{Theming.swift,AppearanceSettingsView.swift}`, `app/Tests/VizDAWAppModelTests/AppearanceTests.swift`

**Interfaces:**
- Consumes: B3 (`SymbioticTokens` in `VizDAWAppModel`); `MemoryDefaults`; and `AudioSettings`' tolerant-decoding pattern (`KeyedDecodingContainer.field`).
- Produces:
  - `public struct AppearanceSettings: Codable, Equatable, Sendable { public var theme = "classic-light"; public var colourBars = true }`;
  - `@MainActor @Observable public final class AppearancePreferences` with:
    - `public init(defaults: UserDefaults)`;
    - `public private(set) var theme: Theme` and `public private(set) var colourBars: Bool`;
    - `public func choose(_ id: String) -> Bool` (false for an unknown id, with nothing changed);
    - `public func setColourBars(_ on: Bool)`;
    - `public var onChange: (@MainActor (Theme) -> Void)?`;
    - `public static let defaultID = "classic-light"` and `public static let key = "appearance"`;
  - `public enum AppearanceCommands` with `public static let ids: Set<String> = ["ui.theme"]` and `public static func run(_ args: [ControlArg], prefs: AppearancePreferences) -> ControlOutcome`. Its status is `"theme ‹id›"`; an unknown theme is `invalid_params` "‹id› is not a theme.";
  - `ControlHost.appearance: AppearancePreferences`;
  - the `VizDAW` target:
    - `struct ThemedRoot: ViewModifier { let prefs: AppearancePreferences }`;
    - `@MainActor enum ThemeWindows { static func apply(_ t: Theme); static func style(_ w: NSWindow) }`;
    - `struct AppearanceSettingsView: View { @Bindable var prefs: AppearancePreferences }`.

- [ ] **Step 1: Write the failing Rust tests.**
  - `commands.rs`'s `ui_theme_checks_its_params`. Each of the five ids is accepted. Each of these is `invalid_params`:
    - no `theme`;
    - `"svp-light"`;
    - `7`;
    - `{theme: "console", extra: 1}`.
  - The registry count test says 154.
  - `control.rs`'s `windows_json_names_the_theme`: `ControlResult::Windows { …, theme: "flat" }` has `"theme": "flat"`.
- [ ] **Step 2: Write the failing Swift tests** in `AppearanceTests.swift`:

```swift
@MainActor @Suite struct AppearanceTests {
    @Test func theDefaultThemeIsClassicLight() {
        let p = AppearancePreferences(defaults: MemoryDefaults())
        #expect(p.theme.id == "classic-light" && p.theme.name == "Classic Light" && p.colourBars)
    }
    @Test func choosingAThemeSavesItAndTellsTheWindows() {
        let d = MemoryDefaults()
        let p = AppearancePreferences(defaults: d)
        var told: [String] = []
        p.onChange = { told.append($0.id) }
        #expect(p.choose("console"))
        #expect(told == ["console"])
        #expect(AppearancePreferences(defaults: d).theme.id == "console")
    }
    @Test func anUnknownOrDamagedSavedThemeFallsBack() {
        for bytes in [Data(#"{"theme":"neon","colourBars":false}"#.utf8), Data([0xFF, 0x00])] {
            let d = MemoryDefaults()
            d.set(bytes, forKey: AppearancePreferences.key)
            let p = AppearancePreferences(defaults: d)
            #expect(p.theme.id == "classic-light")
            #expect(d.data(forKey: AppearancePreferences.key) == bytes)  // not rewritten
        }
    }
    @Test func chooseRefusesAnUnknownTheme() {
        let p = AppearancePreferences(defaults: MemoryDefaults())
        var told = 0
        p.onChange = { _ in told += 1 }
        #expect(!p.choose("svp-light") && !p.choose("") && p.theme.id == "classic-light" && told == 0)
    }
    @Test func themeNamesKeepTheCleanRoom() {
        let banned = ["svp", "studio vision", "opcode", "logic", "apple", "midas", "soundcraft",
                      "steinberg", "cubase", "metro", "zune", "microsoft"]
        for t in Theme.all { #expect(!banned.contains { t.name.lowercased().contains($0) }, "\(t.name)") }
    }
    @Test func uiThemeChecksItsArguments() {
        let p = AppearancePreferences(defaults: MemoryDefaults())
        for bad: [ControlArg] in [[], [.init(name: "theme", value: .text(value: "neon"))]] {
            guard case .failed(let f) = AppearanceCommands.run(bad, prefs: p) else { Issue.record("ran"); continue }
            #expect(f.code == .invalidParams)
        }
        #expect(p.theme.id == "classic-light")
    }
    @Test func uiThemeAnswersItsStatusAndNeedsNoDocument() {
        let p = AppearancePreferences(defaults: MemoryDefaults())
        let r = AppearanceCommands.run([.init(name: "theme", value: .text(value: "flat"))], prefs: p)
        guard case .done(.ran(_, let status)) = r else { Issue.record("\(r)"); return }
        #expect(status == "theme flat" && p.theme.id == "flat")
    }
}
```

  Use the tree's real spellings of `ControlArg`, `ControlOutcome` and the failure code, and say so in the report if they differ.
- [ ] **Step 3: Run them and see them fail.**
- [ ] **Step 4: Implement** Rulings 11 and 12.
  - `AppearanceSettingsView` is a radio `Picker("Theme", …)` over `Theme.all` by `name`. Each row has a swatch strip: `surface.window`, `surface.panel`, `accent.accent` and the five `strip.*` colours, drawn from tokens. Below it are `Toggle("Show channel colour bars", …)` and the caption "The theme applies to every window. It is a preference of this Mac, not saved in projects."
  - The tab is "Appearance", with `AppIcon.settingsAppearance`, placed first.
  - `ThemedRoot` goes on `DocumentWindow`, `MixerWindow`, `MediaPoolWindow`, `PluginManagerView`, the Settings `TabView` and each `NSHostingView` root. `appearancePreferences.onChange = { ThemeWindows.apply($0) }` is set in `VizDAWApp.init`.
  - `VizDAWEditors`' views gain `public var theme: Theme = .classicLight { didSet { needsDisplay = true } }`, used by B5.
- [ ] **Step 5: Run the gates:** Rust both clippies and the workspace (7a end + 17, registry 154); Swift 7a end + 10; the no-brand build; `scripts/make-app.sh`; the e2e (all pass, since `state.get windows` has only gained a key).
- [ ] **Step 6: Commit** the files above. Message: "app: themes: Settings › Appearance, Classic Light by default, live in every window; ui.theme and state.get windows' theme (plan 7b Task B4)", with the trailers.

---

### Task B5: Every colour from the theme

**Files:**
- Create: `app/Sources/VizDAWEditors/EditorTheme.swift`, `app/Tests/VizDAWEditorsTests/ThemeScanTests.swift` (with its own copy of `SourceScan`, because the two test targets share no files)
- Modify: every file the scan names in `app/Sources/VizDAWEditors` (`LaneHeaderView`, `ArrangementView`, `DrumGridView`, `DrumRowHeaderView`, `RegionDrawing`, `RegionStripView`, `TransportFieldsView`, `ExpressionPaneView`, `EditorCanvas`, `StripChartView`, `PianoKeyboardView`, `PianoRollView`, `ControlUI` if named) and `app/Sources/VizDAW` (`LearnMapsSheet`, `ArrangementPane`, `KeysSettingsView`, `ModifyPanel`, `DocumentShell`, `SurfacesSettingsView`, `PluginManagerView`, `MixerView`, `RecordControls`, and any other the scan names); `app/Sources/VizDAW/Theming.swift` (the SwiftUI-side bridge for the mixer-independent views)

**Interfaces:**
- Consumes: B4's `theme` on the Editors views, and `ThemedRoot`.
- Produces: `public extension Theme` in `EditorTheme.swift`, with one `NSColor` property per role of Ruling 13's table:
  - `canvas`, `rulerBackground`, `barLine`, `beatLine`, `separatorLine`, `label`, `secondaryLabel`;
  - `selectionFill`, `selectionText`;
  - `regionFill`, `regionLine`, `regionSelected`, `valueBar`;
  - `playhead`, `armColour`, `muteColour`, `soloColour`, `warningColour`, `errorColour`, `offlineColour`;
  - `whiteKey`, `blackKey`.

- [ ] **Step 1: Write the failing tests** in `ThemeScanTests.swift`:
  - `noColourLiteralsOutsideTheThemeBridges`: Ruling 13's pattern over `VizDAW` and `VizDAWEditors`, minus the two bridge files. Expected: no hits. The message lists `file:line`.
  - `noCornerRadiusOrFontSizeLiterals`: `cornerRadius:\s*[0-9]` and `\.system\(size:\s*[0-9]`. Expected: no hits.
  - `anEditorRedrawsWhenItsThemeChanges`: an `EditorCanvas` 100×100 with `needsDisplay = false`, then `theme = .console`. Then `needsDisplay` is true and `layer?.backgroundColor` equals `Theme.console.canvas.cgColor`.
- [ ] **Step 2: Run them and see them fail.** The scan lists today's sites: about 250, which is this task's worklist.
- [ ] **Step 3: Sweep.**
  - Each AppKit site uses `theme.<role>`; each layer colour is set in `theme`'s `didSet` as well as at set-up.
  - Each SwiftUI site uses `@Environment(\.symbioticTheme)` and a token's `.color`, or a hierarchical `.primary`/`.secondary`/`.tertiary`, which `ThemedRoot` maps.
  - `automationColor(_:)` (in `MixerView.swift`) is deleted; B6's `StripPalette.automation` replaces it, and B7 uses that. Until then it reads `theme.state.*` through the same mapping.
  - `cornerRadius` uses `theme.radius.s`/`.m`/`.l`, and fonts use `theme.type.*.font`.
- [ ] **Step 4: Run the gates:** Swift 7a end + 13, no-brand build, `scripts/make-app.sh`, e2e.
- [ ] **Step 5: Commit** the new files and every swept file, by path. Message: "app: every colour, radius and font size from the theme; editor colour roles (plan 7b Task B5)", with the trailers.

---

### Task B6: The strip's models: typed values, layout, palette, EQ and arm actions

**Files:**
- Create: `app/Sources/VizDAWAppModel/{StripEntry.swift,StripLayout.swift,StripPalette.swift}`, `app/Tests/VizDAWAppModelTests/{StripEntryTests.swift,StripLayoutTests.swift,MixerStripTests.swift}`
- Modify: `app/Sources/VizDAWAppModel/MixerModel.swift` (`BrowserTarget.eq`, `pick`, `clearEq`, `typeLevel`, `typePan`, `typeSend`, `canArm`, `toggleArm`, `cycleMonitor`)

**Interfaces:**
- Consumes: B2 (`StripRow.eq`, `.colour`, `SessionProtocol.setEq`), B4 (`Theme` in `VizDAWAppModel`), and `MixerModel.click`, `inputLane(for:)`, `LaneInputs.cycleMonitor` and `EditorModel.edit(.lane(edit: .setFlags(…)))` (as `SurfaceRelay.arm` uses it).
- Produces (all `public`):
  - `enum StripEntry` with:
    - `static func level(_ text: String) -> Double?` (dB, where −144 is −∞) and `static func pan(_ text: String) -> Double?` (−1…1), per Ruling 15;
    - `static func levelText(_ db: Double) -> String` (`"−4.2 dB"`, `"+3.0 dB"`, `"0.0 dB"`, `"−∞ dB"`, using U+2212) and `static func panText(_ pan: Double) -> String` (`"L 24"`, `"C"`, `"R 10"`);
    - `static let levelHint = "Type a level in dB, like −4.2 or −inf."` and `static let panHint = "Type a pan like L 24, C or R 10."`.
  - `struct ScaleMark: Equatable, Sendable { public var position: Double; public var label: String }` and `struct StripSplit: Equatable { public var throwLength: CGFloat; public var scrolls: Bool }`.
  - `enum StripLayout` with:
    - `static let width: CGFloat = 120`, `minimumThrow: CGFloat = 160` and `maximumThrow: CGFloat = 480`;
    - `static let lowerFixed: CGFloat` (the level box, M S R I, the automation menu and the padding; the implementer measures it once and states it);
    - `static func split(height: CGFloat, upper: CGFloat) -> StripSplit`. Its `throwLength` is `clamp(height − upper − lowerFixed, 160, 480)`, and `scrolls` is `height < upper + lowerFixed + 160`;
    - `static let faderMarks: [ScaleMark]`, for +12, +6, 0, −6, −12, −24, −36, −48 and −60 at `MixerMath.faderPosition(db)`, plus "−∞" at 0. Labels are `"+12"`, `"0"`, `"−6"`.
  - `enum KnobFunction { case gain, pan, send }` and `enum StripPalette` with:
    - `static func cap(_ k: StripColourRow, _ t: Theme) -> ThemeColor` (`t.strip.channel/instrument/auxReturn/bus/master`);
    - `static func knob(_ f: KnobFunction, _ t: Theme) -> ThemeColor` (`t.control.gain/pan/send`);
    - `static func automation(_ m: AutomationModeRow, _ t: Theme) -> ThemeColor`. It maps `AutomationModes.colorName`: green → `state.play`, yellow → `state.solo`, orange → `state.warning`, red → `state.record`, purple → `control.send`, and anything else → `state.offline`;
    - `static func meter(_ peak: Float, _ t: Theme) -> ThemeColor`: `meter.low` below −18 dBFS, `mid` to −6, `high` below 0, `clip` at 0 and above.
  - On `MixerModel`:
    - `BrowserTarget.eq(strip: UInt64)` (`wantsInstrument` false);
    - `func clearEq(_ s: StripRow)`;
    - `func typeLevel(_ s: StripRow, _ text: String) -> Bool`, `func typePan(_ s: StripRow, _ text: String) -> Bool` and `func typeSend(_ s: StripRow, _ index: Int, _ text: String) -> Bool`;
    - `func canArm(_ s: StripRow) -> Bool`, `func toggleArm(_ s: StripRow)` and `func cycleMonitor(_ s: StripRow)`.

- [ ] **Step 1: Write the failing tests.**
  - `StripEntryTests.swift`:
    - `levelsAcceptDecibelsWithOrWithoutTheUnit`: `"-4.2"`, `"−4.2 dB"`, `"-4.2db"` and `" -4.2 DB "` all give −4.2; `"+3"` gives 3; `"0"` gives 0; `".5"` gives 0.5; `"-4.25"` gives −4.3.
    - `levelsClampAndReadMinusInfinity`: `"40"` gives 12; `"-200"`, `"-144"`, `"-inf"`, `"−∞"` and `"-∞ dB"` each give −144; `levelText(-144) == "−∞ dB"`.
    - `hostileLevelsAreRefused`: each of these gives nil:
      - `""`, `"   "`, `"abc"`, `"4.2.1"`, `"--3"`, `"+-3"`;
      - `"nan"`, `"NaN"`, `"inf"`, `"+inf"`, `"1e3"`, `"1e309"`, `"1,5"`;
      - `"-4.2 dBFS"`, `"-4.2 dB dB"`, `"4\u{0}"`;
      - `String(repeating: "1", count: 33)`, and 300 × `"9"`.
    - `pansAcceptLeftCentreRight`:
      - `"C"`, `"c"`, `"0"` and `"L0"` give 0;
      - `"L24"`, `"L 24"`, `"l 24"` and `"-24"` give −0.24;
      - `"R10"`, `"r 10"`, `"10"` and `"+10"` give 0.10;
      - `"L150"` gives −1; `"L 24.6"` gives −0.25; `"R 0.4"` gives 0.
    - `hostilePansAreRefused`: each of these gives nil: `""`, `"LR"`, `"L-5"`, `"L R"`, `"LL24"`, `"24L"`, `"C 5"`, `"nan"`, `"L nan"`, `"∞"`, `"L1e2"`, `"R\u{7}"` and 33 characters.
    - `typedValuesReadBackAsTheyShow`: for −60, −4.2, 0 and 12, `level(levelText(db)) == db`; for −1, −0.24, 0 and 0.1, `pan(panText(p)) == p`.
  - `StripLayoutTests.swift`:
    - `theFaderGetsALongThrowWhenThereIsRoom`: `split(height: 900, upper: 300).throwLength == min(480, 900 - 300 - lowerFixed)` and `!scrolls`.
    - `aShortPaneScrollsTheStripAndKeepsTheMinimumThrow`: `split(height: 260, upper: 300)` is `StripSplit(throwLength: 160, scrolls: true)`, and so is `split(height: 120, upper: 0)`.
    - `theDbScaleMarksSitWhereTheFaderPutsThem`: the positions are strictly decreasing from +12 down to −∞; the 0 dB mark is at `60.0 / 72`; the −∞ mark is at 0.
  - `MixerStripTests.swift` (`FakeSession`, as `MixerModelTests` builds a model):
    - `faderCapsFollowTheStripKindInEveryTheme`: for each `Theme.all` and each `StripColourRow`, `cap` is the matching `strip.*` token, and the five caps are distinct in every theme.
    - `knobsAndAutomationUseTheirTokens`: `knob(.pan, t) == t.control.pan`; `automation(.write, t) == t.state.record`; `meter(1.0, t) == t.meter.clip`; `meter(0.01, t) == t.meter.low`.
    - `typingALevelIsOneClickAndARefusalChangesNothing`:
      - `typeLevel(synth, "-4.2")` is true, and the fake records exactly touch, value and release of `.stripFader(strip:)` at `AutomationLaw.faderNorm(-4.2)`;
      - `typeLevel(synth, "abc")` is false, the fake records nothing new, and `document.editor.status == StripEntry.levelHint`.
    - `typingPanAndSendLevelsUsesTheirTargets`: `typePan(s, "L 24")` sends `.stripPan` at `(−0.24 + 1) / 2`. `typeSend(s, 0, "-6")` sends `.sendLevel(strip:send: 0)` at `faderNorm(-6)`. `typeSend(s, 5, "-6")` on a strip with one send is false and sends nothing.
    - `theEqSlotLoadsBypassesAndEmpties`: `openBrowser(.eq(strip: s.id), in: .window)` then `pick(ref)` calls `setEq(strip: s.id, plugin: ref)` once; `toggleBypass(eqSlot)` calls `setBypass`; `clearEq(s)` calls `setEq(…, plugin: nil)`. A thrown `setEq` raises the "Load EQ Failed" alert.
    - `recordArmAndMonitorNeedAnInputLane`: an audio-path strip with an input lane can arm (`toggleArm` sends `setFlags(arm: true)`), and its monitor cycles. An instrument strip and master give `canArm == false`, and `toggleArm` sends nothing.
- [ ] **Step 2: Run them and see them fail.**
- [ ] **Step 3: Implement** to Rulings 14 and 15, using the signatures above. `StripEntry` uses `NumberEntry`'s rules for length and control characters, but has its own copy here, because `VizDAWAppModel` does not import `SymbioticControls`.
- [ ] **Step 4: Run the gates:** Swift 7a end + 28, plus the no-brand build.
- [ ] **Step 5: Commit** the six files and `MixerModel.swift`. Message: "app model: the strip's typed values, layout and colours; the EQ slot and arm actions (plan 7b Task B6)", with the trailers.

---

### Task B7: The channel strip

**Files:**
- Create: `app/Sources/VizDAW/ChannelStrip.swift`
- Modify:
  - `app/Sources/VizDAW/MixerView.swift`: `MixerView(model:placement:)`'s strips row uses `ChannelStrip`, and its scroll view is two-axis when `StripLayout.split` scrolls. `StripView`, `SendView`, `FaderView` and `MeterView` are removed. `SlotView` and `LearnItem` stay, restyled, and `touchRing` reads `theme.surface.focusRing`;
  - `app/Sources/VizDAW/PaneViews.swift`: 7a's `LeftStripPane` shows `ChannelStrip`.

**Interfaces:**
- Consumes: B3 (icons), B4 (`appearancePreferences.colourBars`, `\.symbioticTheme`), B6 (everything it produces), A4 (`Knob`, `Fader`, `NumberBox`, the palettes, `Relief`), and 7a's `\.mixerPlacement`, `openBrowser(_:in:)`, `showsBrowser(in:)`, `beginRename(_:in:)`, `isRenaming(_:in:)` and `\.openPluginWindow`.
- Produces: `struct ChannelStrip: View { @Bindable var model: MixerModel; let strip: StripRow; let height: CGFloat }`, with `EqSlotView`, `SendKnobRow`, `StripFaderSection` and `StripButtons` in the same file.

- [ ] **Step 1: Build `ChannelStrip`** to Ruling 14, in that order.
  - Each knob, fader and number box maps `onBegin`, `onChange` and `onEnd` to `model.touch`, `move` and `release`, with today's targets. Each `onReset` maps to `model.click`: 0 dB, or C.
  - Each number box's `commit` calls `typeLevel`, `typePan` or `typeSend`.
  - Use `Relief(theme:)` for the cap and knob relief, and the theme's `relief.panelTop`/`panelBottom` gradient behind the strip only when it is not clear.
  - The help text and accessibility labels are Ruling 14's. The EQ slot's popover is `PluginBrowserView` for `.effect`, bound to `model.showsBrowser(in: placement) && model.browser == .eq(strip:)`.
- [ ] **Step 2: Check that nothing was lost.** Every item of today's `StripView` is present, with its menu text:
  - rename;
  - Set All Strips to;
  - Remove Bus;
  - MIDI Learn Maps…;
  - Add Outputs;
  - Load All;
  - the instrument slot;
  - insert reorder and drop;
  - sidechain;
  - Remove Send;
  - MIDI Learn on fader, pan, sends, mute and solo;
  - the touch rings;
  - the solo-muted dimming and help;
  - selection by click and ⌘-click.

  List them in the report, each with where it now lives.
- [ ] **Step 3: Run the gates:** Swift build and test (7a end + 28, no new warnings), the no-brand build, `scripts/make-app.sh`, and the existing e2e. `touch_writes_a_fader_over_the_channel`, `drives_a_fader_and_a_button_from_an_mcu` and `drives_the_mixer_from_a_kontrol_mk3` still pass: they drive the mixer through the channel, not the old sliders. A failure is a strip fault to fix here.
- [ ] **Step 4: Commit** the three files. Message: "app: the channel strip: EQ slot, send knobs, typed pan, level and sends, the long-throw fader, M S R I, console colouring (plan 7b Task B7)", with the trailers.

---

### Task B8: The e2e, the by-eye steps, the root README and the full gates

**Files:**
- Modify: `crates/vizctl/tests/e2e.rs`, `app/README.md`, `README.md`, `docs/feature-backlog.md`

**Interfaces:**
- Consumes: everything above, and from the e2e: `App::start`, `app.call`, `app.call_err`, `app.wait`, `wait_for_state`, `wait_for_undo`, `labelled`, `labelled_like`, `play_button`, and `ui.key` as `keys_drive_step_entry_locators_and_bindings` passes it.

- [ ] **Step 1: Write the e2e** `themes_and_the_strip_over_the_channel`, marked `#[ignore = "needs a window server and VIZDAW_E2E_APP"]`.
  1. `app.new_demo`. `state.get windows`' `theme == "classic-light"`. Record its windows' numbers and kinds.
  2. The hostile calls, each `invalid_params`:
     - `command.run ui.theme {}`;
     - `{theme: "svp-light"}`;
     - `{theme: "console", extra: 1}`;
     - `state.get {target: "mixers"}`.

     After them the theme is unchanged, the windows and kinds are the same, `alerts` is empty, and `state.get document`'s `alert` is null.
  3. `command.run ui.theme {theme: "console"}` answers status `"theme console"`, and `state.get windows`' `theme == "console"`. No window opened or closed.
  4. `state.get mixer`:
     - every strip has `colour`; the Built-in Synth's is `"instrument"` and master's is `"master"`;
     - every `eq` is null;
     - the document window's `ui.tree` has a node labelled "EQ of Built-in Synth" whose value is "empty".
  5. `ui.click` the node labelled "Level of Built-in Synth". Then `ui.key` each character of `-4.2`, and `return`. Wait until `state.get mixer` shows that strip's `gain_db == -4.2`, then `wait_for_undo(…, "Set Fader")`.
  6. `ui.click` "Pan of Built-in Synth", type `LR` and press `return`. Then `state.get editor`'s `status == "Type a pan like L 24, C or R 10."` and the pan is still 0.0. Type `L 24` and press `return`: wait for `pan == -0.24`.
  7. `command.run window.mixer`: a `kind == "mixer"` window appears, and its `ui.tree` has "Fader of Built-in Synth".
  8. `file.save_as` to `<home>/strip.vizdaw`, `file.close`, then `app.open` it. `state.get mixer` shows `gain_db == -4.2` and `pan == -0.24`, and `state.get windows`' theme is still `"console"` (a preference, not project state).
  9. `app.quit()`.
- [ ] **Step 2: Run it and see it pass** (it was RED until B7; record the run against B7's build in the report), then all 16 e2e tests.
- [ ] **Step 3: Write README steps 207–216** in `app/README.md`, under a new section after 7a's "Window, main views and panes (plan 7a)":

```markdown
### Themes and the channel strip (plan 7b)

Checked by hand; the rules are tested (`AppearanceTests`, `AppIconTests`,
`ThemeScanTests`, `StripEntryTests`, `StripLayoutTests`, `MixerStripTests`,
the core's EQ slot tests) and the e2e `themes_and_the_strip_over_the_channel`.

207. Open Settings › Appearance. If another theme is chosen, choose
     Classic Light. File > Create Demo Project: light grey panels, white
     editing areas, dark text and one blue selection colour. Settings ›
     Appearance lists Classic Light, Studio Dark, Flat, Console and
     Symbiotic, with Classic Light chosen.
208. With the demo window, the mixer window (⌥⌘M), a torn-off piano roll
     (⌥⌘1), Modify › Transpose and Settings open, choose each theme in
     turn: every window changes at once, the arrangement and piano roll
     canvases included; no window keeps the old colours. Turn macOS Dark
     Mode on: Classic Light stays light and Studio Dark stays dark. Quit
     and start VizDAW again: the last theme is still chosen.
209. Press ⌃2. Each strip shows, from the top: a colour bar and its name;
     the input (audio strips); "EQ ＋"; the inserts and "+"; its sends as
     knobs, each with its level; "→ Master"; a pan knob reading "C"; a
     large fader cap beside a dB scale and a meter; "0.0 dB"; M S R I;
     the automation mode. The Built-in Synth's cap, an audio strip's cap
     and master's cap have three different colours. Add a bus and give
     the synth a send to it: the bus's cap takes the aux-return colour.
     Route an audio strip's output to the bus: its cap changes to the bus
     colour.
210. Settings › Appearance › Show channel colour bars off: the bars go in
     the Mix view, the bottom pane's mixer, the left strip and the mixer
     window. Turn it back on.
211. Click the Built-in Synth's level box, type `-4.2` and press Return:
     the fader moves down and the box reads "−4.2 dB"; Edit shows "Undo
     Set Fader", and one Undo puts it back to "0.0 dB". Type `abc` and
     press Return: the box still reads "0.0 dB" and the status line says
     "Type a level in dB, like −4.2 or −inf.". Type `-inf`: "−∞ dB" and
     the cap at the bottom. Type `40`: "+12.0 dB". Press Escape while
     typing: nothing changes.
212. In its pan box type `L 24`: the knob turns left and the box reads
     "L 24". Then `r10`: "R 10". Then `c`: "C". Then `LR`: refused, with
     "Type a pan like L 24, C or R 10." in the status line. Drag the pan
     knob up and down on the trackpad: it follows without jumping; with ⌥
     held it moves a tenth as far; a double-click returns it to C. Do the
     same on a send knob and its level box.
213. Drag the fader cap from the bottom to the top on the trackpad: the
     cap is easy to grab, the throw is long, and the "0" mark lines up
     with 0.0 dB. Drag the bottom pane's divider down to its smallest
     size with the bottom pane on Mixer: the strips scroll up and down,
     and the fader keeps a usable throw; M S R I stay reachable.
214. On an audio strip click "EQ ＋" and choose any effect plug-in: the
     slot shows its name. Click it: the plug-in's window opens. Turn its
     bypass on and off from the slot. Add two inserts and drag the second
     above the first: the EQ stays at the top. Try to drag the EQ among
     the inserts: it does not move. Save, close and reopen: the EQ is
     still there. Remove EQ from its menu: "EQ ＋" again; Undo brings it
     back with its settings. Open the project in a VizDAW build from
     before plan 7b: it says the project was made by a newer VizDAW.
215. Choose Console: fader caps, knobs and panels look shaded and
     bevelled, like hardware, with no photographs or textures and no
     brand names; the other four themes are flat. In every theme the pan
     knob's ring and the send knobs' rings have their own colours.
216. The transport bar, the pane buttons, the editors' bars and the
     Settings tabs show SymbioticAudio's icons, with no system symbols.
     Build with `VIZDAW_NO_BRAND=1 scripts/make-app.sh` and open
     `dist/VizDAW.app`: every icon still shows, in the standard set.
```

- [ ] **Step 4: Update the root `README.md`** (spec §2 item 11, 7b's part). Keep 7a's sections.
  - In the status paragraph, "The interface's look (themes) and an audio editor are next." becomes "An audio editor is next."
  - "What works today" gains a bullet: "five themes, Classic Light by default, and a channel strip with typed levels, pan and sends, send knobs and an EQ slot".
  - Add this section after "Getting around":

```markdown
## Themes

VizDAW has five themes: Classic Light (the default), Studio Dark, Flat,
Console and Symbiotic. Choose one in Settings › Appearance; it changes
every window at once, the mixer included, and it stays chosen on this Mac
rather than in your projects. In every theme the mixer colours its fader
caps by strip type (channel, instrument, aux return, bus, master) and its
knobs by what they do, like a mixing console. Only Console draws a
hardware look, in code, with no photographs. The icons come from
SymbioticAudio's shared design library; a build without its branded
glyphs (`VIZDAW_NO_BRAND=1`) uses the standard icon set.
```

- [ ] **Step 5: Add backlog rows** to `docs/feature-backlog.md`:
  - "Linked fader groups and VCAs" (spec §2 item 5's controller's call: the output picker serves buses only);
  - "A built-in VizDAW EQ after 1.0" (spec §3);
  - "Padding and spacing literals outside the views 7b redrew" (Ruling 13, for 7d).

  The "User track colours" row gains "(the strip's colour bar shows the strip kind until then, plan 7b Ruling 12)".
- [ ] **Step 6: Run the full gates.**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --workspace --all-targets --target x86_64-unknown-linux-gnu --target-dir target/linux-check -- -D warnings
cargo test --workspace
cargo clippy -p vizdaw-play --all-targets --features rt-check -- -D warnings
cargo test -p core-engine --features rt-check --test rt_alloc --test rt_alloc_graph --test rt_alloc_record --test rt_alloc_gestures --test rt_alloc_bounce
scripts/build-ffi.sh && scripts/swift.sh build && scripts/swift.sh test
VIZDAW_NO_BRAND=1 scripts/swift.sh build --scratch-path .build-nobrand
find .build-nobrand \( -name 'SymbioticBrand*.o' -o -name 'SymbioticBrand*.swiftmodule' \)
scripts/make-app.sh
VIZDAW_E2E_APP=dist/VizDAW.app cargo test -p vizctl --test e2e -- --ignored --test-threads 1
pgrep -lf MacOS/VizDAW; ls -d /tmp/vze* 2>/dev/null
```

Expected:
- Rust: 7a end + 17 passed, 7a end + 1 ignored (macOS); Linux clippy clean; the rt-check gates pass;
- Swift: 7a end + 28;
- the `find` prints nothing;
- e2e 16 passed;
- registry 154;
- `pgrep` and `ls` print nothing.

- [ ] **Step 7: Commit** the four files. Message: "e2e: themes and the strip over the channel; README 207-216; the root README's themes; backlog rows (plan 7b Task B8)", with the trailers. Counts: ignored +1, e2e 16.

---

## Self-Review

1. **Spec coverage.**
   - §2 item 5, the strip and every part of it in order: B6 and B7 (Ruling 14). The EQ slot as a saved part with undo, processing first, not movable, emptiable: B1 and B2 (Rulings 7–9). Typed and hostile-tested number boxes: B6 (Ruling 15).
   - §2 item 6:
     - the package adopted at a released tag: B3;
     - the five themes with Classic Light the default: A1, G1 and A2, then B4;
     - Settings and live switching: B4;
     - finishing the themes in `symbiotic-design` with tag 0.2.0, and the owner approving by eye before coding: A1–A4, Gates G1 and G2;
     - `SymbioticControls` with EG13's geometry: A4 (Ruling 4);
     - the Console realism in code: A1, A2 and A4 (`relief`), and B7.
   - §2 item 7, console colouring in every theme: B6's `StripPalette` and B7. The optional colour bar: Ruling 12.
   - §2 item 8's strip kinds for colour: B2 (Ruling 10).
   - §2 item 9, the icons and their fallback: A3 and B3 (Ruling 6).
   - §2 item 11, 7b's README part: B8.
   - §4's anti-goals:
     - skeuomorphism in Console only: `reliefIsNoneOutsideConsole` and `flatThemesHaveClearRelief`;
     - controls too small: `everyControlMeetsTheMinimumHit` and `aShortPaneScrollsTheStripAndKeepsTheMinimumThrow`;
     - hard-coded colours: B5's scan;
     - the clean room: `themeNamesKeepTheCleanRoom` and A1's page test.
   - §5, the theme as a preference with tests off the real preferences: B4 (`MemoryDefaults`).
   - Out of scope, as asked: the window and panes (7a), the audio editor (7c), and clean-up (7d, given one backlog row).
2. **Step scan.** Every test step gives names and assertions with the spec's values. Every code step gives signatures, files and the pinned values. The view tasks (B7, and B5's sweep) are checked by the scan, the e2e and README 207–216, and B7's step 2 lists what must survive. No step says "handle edge cases" without naming them.
3. **Type consistency.**
   - `StripColourRow` (B2) is used by `StripPalette.cap` (B6) and `ChannelStrip` (B7).
   - `SetEq` (B1) → `Session::set_eq` (B2) → `SessionProtocol.setEq` → `MixerModel.pick(.eq)` and `clearEq` (B6).
   - `AppearancePreferences` (B4) is used by `ThemedRoot` and `ChannelStrip`'s colour bars (B7).
   - `KnobPalette`, `FaderPalette`, `NumberBoxPalette` and `Relief` (A4) take `Theme`'s `knob`, `fader` and `relief` (A2).
   - `AppIcon` (B3) uses A3's roles.
   - `StripEntry.levelHint` and `panHint` are the e2e's and README's exact strings.
   - `ScaleMark` (B6) maps to `FaderMark` (A4) in B7.
4. **Review Focus.** Five items, each with its pinning tests in the owning task (B1, B2, B4, B5, B6 and B8).
5. **Proportion.** About 1300 short lines (7a's plan is 1680), against a 145-line spec whose 7b row covers §2 items 5–9 and 11, across two repos and twelve tasks. The code blocks are tests and the exact README text; bodies are left to the signatures.

## Execution handoff

Plan complete and saved to `docs/superpowers/plans/2026-11-03-themes-strip.md`. The controller runs it subagent-driven, as in SP5 and 7a, with one reviewer per task, and stops at Gates G1 and G2 for the owner.

- Part A runs in `~/workspace/symbiotic-design`: brief its implementers with that repo's README, HANDOFF and design spec, and with this plan's Global Constraints for Part A.
- B1 and B2 may run while G1 is open.
- B5 and B7 are view work that only the scans, the e2e and the owner's by-eye steps check. Their reviews should read each changed site against Rulings 13 and 14 line by line, and check B7's "nothing lost" list item by item.
