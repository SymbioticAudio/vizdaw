# VizDAW 7b — Themes, Icons and the Channel Strip Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** VizDAW looks finished in its default theme.
- It adopts `symbiotic-design` 0.2.0, which this plan finishes:
  - five real palettes, approved by the owner from browser mockups;
  - a theme `name`;
  - the `knob`, `fader`, `relief` and `swatch` token groups;
  - fourteen new icon roles;
  - a new `SymbioticControls` product (knob, fader and number box).
- Classic Light is the default. Settings › Appearance switches the theme live, in every window.
- Every icon comes from `SymbioticIcons`, and the app still builds without the branded glyphs.
- One channel strip serves the left pane, the bottom pane's mixer, the Mix view and the mixer window. From the top it shows:
  - the colour bar (the strip's own colour when one is set, else its kind's) and the name, then the input;
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
    - `Strip.colour: Option<Swatch>` (controller ruling C-1);
    - `MixerCmd::SetEq` and `MixerCmd::SetColour`;
    - format `VERSION` 4.
  - **`core-engine`:** `mixer_spec` builds each strip's insert list from `chain()`, so the EQ runs first.
  - **`core-host`:** loads the EQ like any insert.
  - **`core-ffi`:**
    - `StripRow.eq`, `StripRow.kind_colour` (`StripColourRow`) and `StripRow.colour` (`SwatchRow?`);
    - `Session::set_eq` and `Session::set_strip_colour`;
    - `state.get mixer`;
    - `state.get windows`' `theme`.
  - **`core-control`:** `ui.theme`, and the `mixer` state target.
  - **`VizDAWAppModel`** (tested):
    - `AppearancePreferences` and `AppearanceCommands`;
    - `AppIcon`;
    - `StripEntry` (the typed values), `StripLayout` and `StripPalette`;
    - `MixerModel`'s EQ, strip-colour, typed-value and arm/monitor actions.
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
- `HANDOFF.md`, the local file (the family rule (g), the knob decisions (h)–(k), and the "M7" request 1 geometry). It stops being tracked in A4 and is never committed again;
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
  - **`HANDOFF.md` is local only (owner ruling, 2026-11-03).** Task A4 stops tracking it (`git rm --cached HANDOFF.md`, the file kept on disk) and adds it to `.gitignore`. After that no commit adds it again. Tasks that read HANDOFF for the family rule read the local file.
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
  - `hostile_eq_calls_are_refused` and `hostile_strip_colour_calls_are_refused` (core-ffi);
  - the e2e's hostile block (Task B8).
  - Every typed number box has hostile-input tests: `hostileLevelsAreRefused` and `hostilePansAreRefused` (Task B6), and `aLongPasteIsRefusedNotCut` (Task A4).
- **FFI.**
  - The two new exports are `Session::set_eq` and `Session::set_strip_colour` (C-1). Each runs inside `ffi_guard` (which is `catch_unwind`) and returns `Result<DocSnapshot, VizError>`.
  - The new UniFFI types are `StripColourRow`, `SwatchRow`, the `StripRow` fields `eq`, `kind_colour` and `colour`, `StateTargetRow::Mixer`, `ControlResult::Mixer`, and `ControlResult::Windows`' new `theme` field.
- **Real time.**
  - The render path stays allocation-free. Only `mixer_spec`, on the control thread, changes in the engine.
  - Tasks B1 and B8 run the rt-check gates:
    - `cargo clippy -p vizdaw-play --all-targets --features rt-check -- -D warnings`;
    - `cargo test -p core-engine --features rt-check --test rt_alloc --test rt_alloc_graph --test rt_alloc_record --test rt_alloc_gestures --test rt_alloc_bounce`.
- **Swift 6.1 (VizDAW's CI runs macos-15) as well as the local toolchain.** This applies to `SymbioticControls` too, because VizDAW builds it. The design repo's CI pins Xcode 26 (Swift ≥ 6.2), so Task A4 adds a `swift61` job on `macos-15`, and Gate G2 checks it green before the tag is pushed.
  - Pass no weak `self` and no non-Sendable value into `MainActor.assumeIsolated`. Capture a strong `let` made before the closure, and bring a notification or event in through `nonisolated(unsafe) let`, as `KeyMonitor` does.
  - No `@State`, `@GestureState` or `@Entry` in any package code built with the Command Line Tools, which cannot expand them. Spell out `State(initialValue:)` and `GestureState(initialValue:)`, and declare environment values with an `EnvironmentKey`.
  - Use no API newer than macOS 14.
- **Fallback icons.** VizDAW builds and runs with `VIZDAW_NO_BRAND=1`, which leaves the `SymbioticBrand` product out (Ruling 6). Every role then draws its standard icon.
- **Lints**, in Rust as in every plan: `#![deny(missing_docs)]`, and no unwrap, expect, indexing or panic outside tests.
- **TDD.** Every new test is run and seen to fail (RED) before the code that makes it pass.
- **File compatibility.**
  - `core_model::file::VERSION` goes from 3 to 4 (Ruling 8), with `migrate_v3_to_v4`.
  - Task B1 regenerates `fixtures/demo.vizdaw` as a version-4 file (`cargo run -p vizdaw-play -- --write-demo fixtures/demo.vizdaw`), because `checked_in_demo_matches_a_fresh_build` (`crates/vizdaw-play/tests/cli.rs`) compares it with a fresh build. `diff fixtures/demo.offline.txt` must still pass: rendering is unchanged. The migration stays proven by the core-model goldens (the v1 and v2 fixtures) and by `version_3_projects_migrate_to_version_4`.
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
  | A2 tokens 0.2.0 | +6 | +4 | | | |
  | A3 icon roles | +2 | +1 | | | |
  | A4 controls, tag | — | +15 | | | tag `0.2.0` (local); `swift61` CI job |
  | B1 EQ slot, strip colour, core | | | +11 | — | demo fixture regenerated (v4) |
  | B2 EQ and colour FFI, kind colours, `state.get mixer` | | | +7 | +0 (compiles) | |
  | B3 package, icons | | | — | +3 | no-brand build |
  | B4 themes, Settings, `ui.theme` | | | +2 | +7 | registry 154 |
  | B5 colour sweep | | | — | +4 | |
  | B6 strip models | | | — | +17 | |
  | B7 strip view | | | — | +0 | |
  | B8 e2e, READMEs | | | ignored +1 | — | e2e 16; README 207–216 |
  | **After** | **185** | **66** | **7a end + 20 / ignored + 1** | **7a end + 31** | |

  Running totals: Python 177 (A1), 183 (A2), 185 (A3); design Swift 50 (A2), 51 (A3), 66 (A4); Rust 7a end + 11 (B1), + 18 (B2), + 20 (B4); VizDAW Swift 7a end + 3 (B3), + 10 (B4), + 14 (B5), + 31 (B6, B7, B8).

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
   - the Unicode minus `−4.2`, and `-0.04` (which must not read "−0.0 dB");
   - `+40`, `-200`;
   - for pan: `LR`, `L-5`, `24L`, `C 5`, `L150`, `r 10`, `−24` (U+2212), `L ２４` (full-width digits), `R ٣` (Arabic-Indic), `L` alone;
   - a level typed while the same fader is held by a surface or a drag.

   Expected:
   - a value the grammar accepts is applied, with out-of-range numbers clamped (+40 → +12.0 dB, −200 → −∞, L150 → L 100), as one undo step;
   - anything else is refused: the box shows the old value, the status line shows the hint, and nothing is sent to the core;
   - a paste longer than 32 characters is refused whole, never cut to 32 and applied;
   - a value typed at a touched control is refused with "Release the control first.".

   Pinned by `aLongPasteIsRefusedNotCut` (Task A4); `levelsAcceptDecibelsWithOrWithoutTheUnit`, `levelsClampAndReadMinusInfinity`, `hostileLevelsAreRefused`, `pansAcceptLeftCentreRight`, `hostilePansAreRefused` and `typingALevelIsOneClickAndARefusalChangesNothing` (Task B6); and the e2e's typed `LR` (Task B8).
2. **The EQ slot under ordinary editing.** The cases:
   - an insert dragged above the EQ;
   - the EQ dragged onto another strip;
   - the EQ replaced, then Undo;
   - the EQ emptied, saved, reopened;
   - an older project opened;
   - a bus that feeds the EQ's sidechain removed;
   - a sidechain set on the EQ, and EQ parameters automated with the strip in Touch.

   Expected:
   - the EQ stays first and processes first;
   - it cannot be moved or removed as an insert;
   - Undo restores the replaced plug-in with its settings;
   - a version-3 project opens with every EQ slot empty;
   - the sidechain is cleared;
   - the EQ takes a sidechain, and its parameters follow the strip's automation mode.

   Pinned by `the_eq_slot_stays_first_when_inserts_move`, `the_eq_slot_is_not_an_insert` (with `strip_of` and `SetSidechain`), `removing_a_bus_clears_an_eq_sidechain_from_it` and `version_3_projects_migrate_to_version_4` (B1); `replacing_the_eq_keeps_the_old_state_for_undo` and `hostile_eq_calls_are_refused` (B2); and `theEqSlotIsSeenByEveryStripSlotReader` (B6: the EQ's plug-in window opens, Load All loads it, and its parameters can be automated).
3. **A theme switched while things are open**: the mixer window, a torn editor, a plug-in window, the Modify panel, Settings, a popover, and macOS Dark Mode on; then a window, menu or alert opened after the switch.

   Expected:
   - every VizDAW window changes at once;
   - AppKit canvases redraw with the new colours;
   - no window keeps the old palette or the system appearance, including windows, menus, alerts and open panels made after the switch (`NSApp.appearance`);
   - Classic Light stays light in Dark Mode.

   Pinned by `choosingAThemeSavesItAndTellsTheWindows` (B4), `anEditorRedrawsWhenItsThemeChanges` and `noColourLiteralsOutsideTheThemeBridges` (B5), and by README step 208.
4. **A short pane.** The bottom pane's mixer at its default 260 points, or 120, and the left strip on a 700×450 window.

   Expected: the fader keeps at least `StripLayout.minimumThrow` (160 points) and the strip scrolls vertically, rather than squashing the fader or cutting off M S R I.

   Pinned by `aShortPaneScrollsTheStripAndKeepsTheMinimumThrow` (B6) and README step 213.
5. **A damaged or unknown saved theme**: a preference written by a newer VizDAW (`"theme": "neon"`), garbage bytes, or no preference at all.

   Expected: Classic Light; nothing is rewritten until the user chooses.

   Decoding is per field: `{"theme":"neon","colourBars":false}` gives Classic Light with the colour bars **off** (the field that reads is kept), and garbage bytes give Classic Light with the bars on.

   Pinned by `anUnknownOrDamagedSavedThemeFallsBack` (both cases) and `theDefaultThemeIsClassicLight` (B4).

## Gates

The controller stops at each gate and asks the owner. Neither gate is a task. The answer is recorded in `progress.md`.

- **Gate G1, "owner approves palettes"**, after Task A1. The controller gives the owner the absolute path of `docs/mockups/0.2.0/themes.html` (in the design repo) and the commit. The owner opens it in a browser and approves each theme by eye, or asks for changes. The controller also shows the owner this item and records the answer:
  - **Q2 (C-1).** Controller ruling C-1 adds an optional per-strip colour (`Strip.colour`, in the same v3→v4 format bump). This overrides spec §3's "no change to the document format beyond the EQ slot". Confirm C-1, and approve the eight `swatch` colours in each theme (the page shows one strip with a user colour bar and the piano roll's lane colours).
  - **Knob finish (owner, 2026-11-03: Q4 answered, binding; not a question).** The page shows each theme's knob with its `knobFinish` (Ruling 3): by default Console `shaded`, the other four `flat`. The owner may change any theme's finish in the G1 fix rounds; a theme the owner makes `shaded` at G1 also gets non-clear `relief` values (it then shades its fader caps too), and Console stays the only theme with panel gradients unless the owner says otherwise.
  - Changes go back to A1's implementer as a fix round, editing `palettes.json` only and regenerating. Repeat until all five are approved.
  - At G1 the owner may change the strip width and the fader cap only. Knob sizes and cap ratios are family geometry (HANDOFF "M7" request 1) and change only through the HANDOFF.
  - Record `G1: approved <date>, palettes.json at <sha>; Q2 <answer>; knob finishes <list>`, with any strip width or fader cap the owner changed, which then binds Rulings 4 and 14. If the owner overturns C-1, the controller rules on removing it from B1, B2, B6, B7 and B8 before they run.
  - Tasks B1 and B2 consume no tokens, so they may run while G1 is open (the eight swatch names are fixed now; G1 tunes only their values). Tasks A2–A4 and B3–B8 wait.
- **Gate G2, "owner approves the 0.2.0 push"**, after Task A4.
  1. The controller asks the owner: may `sp7-0.2.0` be fast-forwarded into `main`, and may `main` and the annotated tag `0.2.0` be pushed? With the question it notes that `HANDOFF.md` stops being tracked in A4 (owner ruling, 2026-11-03) but is still in pushed history on `main` and on `eg13-requests`. Removing it from history would need a force-push rewrite, which only the owner can decide; this plan does not plan or do that rewrite. Once `main` drops `HANDOFF.md`, any other clone that pulls it deletes its working copy, so back up any other clone's `HANDOFF.md` before pulling `main`.
  2. With the yes, the controller pushes `main` only.
  3. The controller checks with `gh run list --branch main --json headSha,conclusion` that the run's `headSha` is the pushed SHA, and with `gh run view <id>` that both CI jobs (the existing one and A4's `swift61`) are green. It does not ask the owner to check.
  4. Only then does it push the tag `0.2.0`.

  If `swift61` fails, the fix is made on `sp7-0.2.0`, the local tag (not yet pushed) is moved to the fixed commit, and steps 2–4 repeat. The controller records the pushed SHA and the CI run. Tasks B3–B8 need `0.2.0` on GitHub (Ruling 6) and wait for G2.
  - The licence question (Q1) is settled: the owner chose the GPL-3.0 §7 additional permission (2026-11-03). Task B3 writes it (Ruling 6).

Execution order: A1 → G1 (B1 and B2 meanwhile) → A2 → A3 → A4 → G2 → B3 → B4 → B5 → B6 → B7 → B8.

## Plan Rulings

Each ruling decides something the spec leaves to the builder, as "what — why — cost if wrong". The spec's "(controller's call)" items are restated only where this plan makes them concrete.

1. **Ruling (two repos): Part A's tasks finish `symbiotic-design` 0.2.0 before VizDAW consumes it.**
   - What: Part A runs on branch `sp7-0.2.0` from `eg13-requests` and ends in a local tag. VizDAW pins `0.2.0` only after G2's push.
   - Why: spec §2 item 6 ("releases a new tag (0.2.0) that VizDAW pins"; "pushing the design repo needs the owner's say-so"). The EG13 knob requests live only on `eg13-requests`.
   - Cost if wrong: one rebase of a branch nobody else uses.
2. **Ruling (the mockups): palettes as named tokens, rendered by a script.**
   - `docs/mockups/0.2.0/palettes.json` has `tokens.json`'s `themes` shape, with three additions: a `name` and a `knobFinish` per theme, and values for the four new groups (Ruling 3). Each value in a group of kind `color` is `#RRGGBB` or `#RRGGBBAA`, with no system colours. The metric groups (`spacing`, `radius`, `iconStroke`, `type`) take `tokens.json`'s kinds and today's values, and `brand` (kind `color`, `constant: true`) equals today's values in every theme.
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
        - Vocal's strip also carries a user colour bar (`swatch.blue`, C-1), beside the kind-coloured bars of the others;
     3. the controls at 100 % and 200 %: knobs small, medium and large, with arc and indicator; the fader with its scale and meter; the number box idle, editing and refused; M S R I off and on; the six automation-mode tints (off, read, touch, latch, write, trim);
     4. the eight `swatch` colours as strip colour bars and as the piano roll's lane colours;
     5. each theme's knob in its `knobFinish` (`flat` or `shaded`), at the three sizes.
   - The mockups follow the spec's strip order (§2 item 5) and 7a's layout A. Use the spec's brainstorm mockups as layout reference: `/Users/lattawb/workspace/vizdaw/.superpowers/brainstorm/11155-1793752858/content/{layout,strip,mixview}.html`. Where they differ from the spec, the spec wins (7a's spec checks).
   - Direction per theme (inspiration only; the owner tunes it at G1):
     - **Classic Light (default):** a late-1990s desktop look: light neutral-grey chrome (around `#DDDDDD`), white editing canvases, crisp one-point dark outlines, small dense type, saturated primary state colours (red record, green play, yellow solo), and a single blue selection. Square-ish corners (`radius.s` 2). Fader caps by strip kind on a light panel.
     - **Studio Dark:** a modern dark studio look. Charcoal panels (around `#2A2C30`), slightly lighter raised controls, a cool blue accent, soft separators, and muted, distinct kind colours.
     - **Flat:** typography-led and flat. Pure white or near-black surfaces, no gradients, no borders except one-point rules, and bold flat accent blocks; fader caps are flat tiles. `radius` all 0.
     - **Console:** a realistic console look, drawn in code. Dark blue-grey metal panels with a vertical gradient (`relief.panelTop`/`panelBottom`), cream legends, fader caps shaded from `capTop` to `capBottom` with a one-point `highlight` bevel and a `shadow`, knob caps likewise (`knobFinish: shaded`), and LED-style meters. By default it is the only theme with non-clear `relief` values and the only `shaded` knob finish. A theme the owner makes `shaded` at G1 also gets non-clear `relief` values (it then shades its fader caps too); Console stays the only theme with panel gradients unless the owner says otherwise.
     - **Symbiotic:** today's `symbiotic` values (cyan, gold, ink, paper, black rules), extended to the new groups.
   - Every theme, Console included, colours fader caps by `strip.*` and knob arcs by `control.*` (spec §2 item 7).
   - Why: spec §2 item 6 ("approves each theme by eye from browser mockups before it is coded") and the brief's "palettes as named tokens".
   - Cost if wrong: one fix round per theme at G1.
3. **Ruling (tokens 0.2.0): a theme `name`, four new colour groups, all hex, no placeholders, and source compatibility.**
   - **Theme `name`:** required, one line, at most 32 characters, unique. Swift gets `Theme.name: String`.
   - **New groups**, each with `"since": "0.2.0"` in its group spec:
     - `knob`: `skirt`, `cap`, `track`, `indicator`;
     - `fader`: `well`, `scale`, `capLine`;
     - `relief`: `highlight`, `shadow`, `capTop`, `capBottom`, `panelTop`, `panelBottom`. Console's are real. A theme the owner makes `shaded` at G1 also gets non-clear `relief` values (it then shades its fader caps too); Console stays the only theme with panel gradients unless the owner says otherwise. In every other theme these are all `#00000000`, and drawing code skips a fully clear layer;
     - `swatch`: `red`, `orange`, `yellow`, `green`, `teal`, `blue`, `purple`, `pink` (controller ruling C-1). These are the user strip colours and the piano roll's lane colours. The eight names are fixed now; G1 tunes the values, which are distinct within each theme.
   - **Knob finish (owner, 2026-11-03, binding):** each theme carries `knobFinish`, `"flat"` or `"shaded"`, beside `appearance` (`"since": "0.2.0"`). `knobFinish` is `flat` or `shaded`, as recorded at G1 (default: Console `shaded`; Classic Light, Studio Dark, Flat and Symbiotic `flat`). Swift gets `public enum KnobFinish: String, Sendable { case flat, shaded }` and `Theme.knobFinish`, whose `init` parameter defaults to `.flat`. The finish is a drawing layer only: knob geometry stays the family's (HANDOFF "M7" request 1), and a shaded finish is drawn in code from `relief` tokens, with no textures.
     - Swift uses the generator's nesting: `Theme.KnobColors`, `Theme.FaderColors`, `Theme.ReliefColors` and `Theme.SwatchColors` (as `Theme.StripColors` is today).
     - The knob's arc and the fader's cap take `control.*` and `strip.*`, which already exist.
   - **Values:** all five themes take the approved `palettes.json` values, hex only. `placeholder` is false for all five. Increase Contrast therefore no longer re-pins `classic-light` or `studio-dark`: the README's "Checking by eye" bullet is replaced by `contrast.md`'s body-text rule (A2's test).
   - **Source compatibility with 0.1.0's public initialisers** (EG13 builds its own `Theme`): every new `Theme.init` parameter has a default.
     - `name: String? = nil` reads back as the `id`; `knobFinish: KnobFinish = .flat`.
     - Each `since` group defaults to `classicLight`'s values, generated as `public static let classicLight` on the group's struct (`Theme.ReliefColors.classicLight`, …).
   - **Notes:** each theme's `note` is one line naming its direction, with no Metro, Soundcraft, SVP, Apple or other banned name (the clean-room list in the Global Constraints).
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
   - **Number box:** at least 56×22 points, monospaced digits in `type.label`. A click or Return starts editing, and the box takes keyboard focus (`FocusState`, set in `begin`); Return commits; Escape or losing focus cancels. The box **never cuts** text: on commit it passes the whole text, `NumberEntry.clean` gives nil past `NumberEntry.maxLength` (32) characters, and the box refuses it (old text shown, `commit` not called, the caller's `onRefuse` called so it can show its hint).
   - **Hit targets:** every control's hit area is at least `ControlMetrics.minimumHit` (24) points in each direction, padded beyond the drawing where needed (spec §4, "easy to hit with a trackpad"). The views take their hit frame from `ControlMetrics.hitSize(_ c: ControlKind) -> CGSize`, so the test checks what the views use.
   - **Interaction:**
     - a vertical drag of 200 points covers the whole range of a knob; a fader covers its throw; ⌥ held divides by 10;
     - a drag is relative to the grab point, so the cap never jumps to the pointer; `ControlDrag.Tracker` re-anchors whenever ⌥ changes mid-drag, so the value never jumps;
     - `DragGesture(minimumDistance: 2)`: `onBegin` fires on the first `onChanged`, and `onEnd` only after a begin, so a bare click sends no gesture (no touch in Touch or Latch);
     - a double-click comes through `.simultaneousGesture(TapGesture(count: 2))` and calls `onReset` only when no drag began;
     - arrow keys step by `step` (×10 with ⇧) through `.focusable()` and `.onKeyPress` (macOS 14), as HANDOFF's shared behaviours list keyboard control;
     - no scroll wheel in 0.2.0: macOS 14 SwiftUI has no scroll-wheel gesture, and an `NSEvent` path is not worth its Swift 6.1 risk here (backlog row, B8);
     - VoiceOver's increment and decrement go through `accessibilityAdjustableAction` by the caller's step.
     - Callbacks are `onBegin` (first movement), `onChange(Double)` and `onEnd` (mouse-up after a begin), so VizDAW maps them to `touch`, `move` and `release` (plan 5e-2's gestures).
   - **Relief:** a `Relief` value (`highlight`, `shadow`, `capTop`, `capBottom`); `Relief.none` draws nothing extra. Console and any theme G1 made `shaded` pass their tokens; every other theme passes clear ones.
   - **Knob finish:** `KnobPalette(theme:)` carries `finish: KnobFinish` from `theme.knobFinish`, and `Knob` draws its shading layer (from `Relief`) only when `finish == .shaded`. The geometry is the same in both finishes. EG13 stays flat simply by using a flat theme. HANDOFF's 0.2.0 section records: "finish is a theme token; geometry is family".
   - **Chip label:** `Knob` takes `chipFill: Color? = nil` (HANDOFF's `accentInk`) for the `chip` label style.
   - Why: spec §2 item 6 ("VizDAW's knob, fader and number box as a shared `SymbioticControls` product … geometry is agreed with EG13's knob through the HANDOFF") and HANDOFF (k). The 2026-10-28 owner note "SymbioticControls later, when VizDAW gets closer to building its UI (SP7)" is now.
   - Cost if wrong: EG13 adopts a later version; VizDAW changes palettes, not views.
5. **Ruling (new icon roles): fourteen roles VizDAW's sweep needs, each with a standard icon, and two existing MIT icons left untouched.**
   - The roles and their standard icons:
     - `view.left-pane`, `view.bottom-pane`, `view.right-pane`;
     - `view.mixer`, `view.split`, `view.empty`;
     - `view.expand`, `view.collapse` (the existing `expand` and `collapse` icons);
     - `settings.keys`, `settings.audio`, `settings.surfaces`, `settings.appearance`;
     - `status.info` (the existing `info` icon, which `app.about` uses);
     - `mixer.eq`.
   - `view.split` gets a new `split-stacked.svg` (two stacked rectangles), because `split.svg` already exists (two side-by-side rectangles, for `part.split`).
   - That is eleven new standard SVGs in the house style (24 grid, 1.2 monoline, `currentColor`): `left-pane`, `bottom-pane`, `right-pane`, `mixer`, `split-stacked`, `empty`, `keys`, `audio`, `surfaces`, `appearance` and `eq`. `info.svg` and `split.svg` stay byte-identical to `b209d2c` (A3 checks their SHA-256 digests). No brand glyph is assigned: the owner assigns glyphs later.
   - Why: spec §2 item 9. Today's SF Symbols (about 45 uses, plus 7a's three pane toggles) have no role for these.
   - Cost if wrong: an icon redrawn later; the roles stay.
6. **Ruling (the pin and the fallback build).**
   - `app/Package.swift` adds `.package(url: "https://github.com/SymbioticAudio/symbiotic-design", exact: "0.2.0")`. The repo is public and answers anonymous `git ls-remote` over https, so CI's `actions/checkout` of vizdaw alone is enough: SwiftPM fetches the tag. `app/Package.resolved` is committed.
   - The products, by target:
     - `VizDAWAppModel` takes `SymbioticTokens` and `SymbioticIcons`;
     - `VizDAWEditors` takes `SymbioticTokens`;
     - `VizDAW` takes `SymbioticTokens`, `SymbioticIcons` and `SymbioticControls`, and `SymbioticBrand` unless `Context.environment["VIZDAW_NO_BRAND"] == "1"`.
     - No test target links `SymbioticBrand`, so every Swift test runs on the fallback set.
   - `VizDAWApp.init` registers the brand first thing, as `#if canImport(SymbioticBrand) Icons.register(SymbioticBrand.provider) #endif`, and the import is guarded the same way: `#if canImport(SymbioticBrand) import SymbioticBrand #endif`.
   - `.github/workflows/ci.yml`'s `app` job gains a step, "App builds without the branded glyphs". It runs `VIZDAW_NO_BRAND=1 scripts/swift.sh build --scratch-path "$RUNNER_TEMP/nobrand"`, then fails if `find "$RUNNER_TEMP/nobrand" \( -name 'SymbioticBrand*.o' -o -name 'SymbioticBrand*.swiftmodule' \)` prints anything (the design repo's CI pattern).
   - `.gitignore` gains `.build-nobrand/` (it has only `.build/`).
   - **Licence (owner, 2026-11-03: Q1 answered, binding).** VizDAW's official build links the all-rights-reserved `SymbioticBrand`, so B3 appends this GPL-3.0 §7 additional permission to the end of `LICENSE`, after the GPL text and not inside it:
     > Copyright (C) 2025–2026 Westlee Latta
     >
     > Additional permission under GNU GPL version 3 section 7: if you modify this Program, or any covered work, by linking or combining it with SymbioticAudio's SymbioticBrand library, containing parts covered by the terms of its ASSETS-LICENSE, the licensors of this Program grant you additional permission to convey the resulting work. This permission does not license the SymbioticBrand artwork itself.
     >
     > The SymbioticBrand library is not part of this Program and is not licensed under the GNU GPL; it remains under its ASSETS-LICENSE, which alone decides whether it may be copied or conveyed. Corresponding Source for such a combination need not include it.

     The root README's "License" section gains one line: "VizDAW's own build links SymbioticAudio's branded icons under a GPL section 7 additional permission (see LICENSE). The artwork stays all rights reserved and is not covered by the GPL; forks build without it with `VIZDAW_NO_BRAND=1`."

     The copyright line comes first so that "the licensors of this Program" names someone: the repo has no copyright notice today. `scripts/make-app.sh` copies `LICENSE` into `dist/VizDAW.app/Contents/Resources/`, so the conveyed binary carries the permission. After G2, once this `LICENSE` is on GitHub, the controller checks that GitHub still detects GPL-3.0; if it does not, the text stays and "Other" is accepted.
   - Why: spec §2 item 9 ("still builds with the fallback icon set when the branded assets are absent"), the design spec's fork rule, and `ASSETS-LICENSE` (the artwork is not licensed under any application's open-source licence).
   - Cost if wrong: one manifest line; one licence text.
7. **Ruling (the EQ slot in the model): `Strip.eq: Option<PluginSlot>`, beside the inserts, not in them.**
   - `Strip` gains `eq`, with `#[serde(default, skip_serializing_if = "Option::is_none")]`.
   - `Strip::chain() -> impl Iterator<Item = &PluginSlot>` yields the EQ, then the inserts, and `chain_mut` likewise.
   - `Mixer::slots() -> impl Iterator<Item = (StripId, &PluginSlot)>` yields every strip's chain, by strip ID.
   - `Mixer::inserts()` is unchanged (inserts only, with their insert positions): `find_slot` and `insert_owner` keep their meaning.
   - `SlotOwner::Eq(StripId)` is new. `find_slot` checks it before the inserts; `plugin_slot`, `slot_mut` and `referenced_states` see it.
   - `insert_owner` keeps its meaning and refuses an EQ with `invalid("the EQ slot is not an insert")`. A new `fn chain_owner(d: &Document, slot: PluginInstanceId) -> Result<StripId>` (insert or EQ) serves the commands that work on either.
   - Every "all slots" reader switches to `slots()` or `chain()`:
     - `core-host` `loads_needed` and its sync;
     - `mixer_rules`' four insert loops;
     - `core-ffi`'s `slot_ids`, `plugins.rs` (two), `host.rs`' `has inserts`, `surfaces.rs`' `chain`, and the Kontrol page's first plug-in.
     - Swift readers: `StripRow.slots` (B6).
   - The `SlotOwner` matches gain an `Eq` arm:
     - `core_model::automation::strip_of`: `SlotOwner::Eq(s) => Some(s)`, so EQ automation follows the strip's mode;
     - `core-ffi`'s `slot_name` (`session.rs`): `SlotOwner::Eq(s)` → "‹strip› — EQ ‹plugin›".
   - **`MixerCmd::SetEq { strip, plugin: Option<PluginRef> }`:**
     - `Some(p)` allocates a new instance ID and replaces any current EQ (the old one in `Report.removed`, the new one in `created`). Its title is "Load EQ".
     - `None` empties the slot, with title "Remove EQ". On an empty slot it is refused with `invalid("the EQ slot is empty")`.
   - `MoveInsert` and `RemoveInsert` of the EQ's instance are refused with `invalid("the EQ slot is not an insert")`. An insert never moves into the EQ slot: insert indices count inserts only, so `AddInsert { index: 0 }` is the first insert after the EQ.
   - `SetBypass` and `SetPluginParams` work on the EQ through `slot_mut`. `SetSidechain` calls `insert_owner` today (`command/mixer.rs`, its `SetSidechain` arm), so it switches to `chain_owner`: a dynamic EQ may take a sidechain.
   - Removing a strip clears an EQ's sidechain from it (`Mixer::remove` walks `chain_mut`).
   - Every strip kind, master included, has an EQ slot.
   - Undo comes from the document history, as for every command. `core-ffi`'s state capture covers the EQ once `slot_ids` includes it (B2).
   - Why: spec §2 item 5 ("a real, saved part of a strip … survives reordering the other inserts. It processes before the other inserts. It cannot be moved among them, and it can be emptied"). A field, not "insert 0 with a flag", makes the rule structural: no insert index can reach it.
   - Cost if wrong: every slot reader is listed here; a missed one fails `every_slot_lookup_sees_the_eq` or `an_eq_slot_is_loaded_as_an_effect`.
8. **Ruling (format `VERSION` 4).**
   - `VERSION` 3 → 4. `MIGRATIONS` gains `migrate_v3_to_v4`, which returns the document unchanged: a strip without `eq` has an empty slot.
   - An older VizDAW then refuses a version-4 project with its clear `NewerFormat` message, instead of `deny_unknown_fields`' "unknown field `eq`" on only the projects that use an EQ.
   - The five tests that pin 3 move to `VERSION`: `golden` (`tests/file.rs:243`), `v1_plugin_instruments_migrate_to_slots_and_strips` (`tests/file.rs:417`), `version_2_projects_migrate_to_version_3` (`tests/file.rs:469`, `text.contains("\"version\": 3")`), `mixer_document.rs:126`, and `learn.rs:207`, which becomes `assert_eq!(VERSION, 4)` with its comment.
   - The doc comment that says version 3 follows: `file.rs`'s module comment (line 3) moves to 4. `document.rs:169` ("`None` in projects older than version 3 (5a Ruling 4)") is about `content_hash`, which arrived in v3, so it stays.
   - `fixtures/demo.vizdaw/project.json` is regenerated at version 4 (Global Constraints, File compatibility).
   - The same bump carries C-1's `Strip.colour` (Ruling 12), so neither field needs a second bump.
   - Why: spec §2 item 5 ("a small Rust core, FFI and document-format change").
   - Cost if wrong: one constant and one identity migration.
9. **Ruling (the engine): the EQ is the first `InsertSpec` of its strip.**
   - `graph::mixer::mixer_spec` iterates `s.chain()` where it iterated `s.inserts`. Latency, bypass, PDC, adoption and sidechains then treat the EQ as an insert at chain position 0. Nothing on the real-time thread changes.
   - Why: "it processes before the other inserts".
   - Cost if wrong: one loop.
10. **Ruling (strip kinds for colour): `StripColourRow`, computed in `core-ffi`, as `StripRow.kind_colour`.**
    - The cases are `Channel`, `Instrument`, `AuxReturn`, `Bus` and `Master`.
    - The kinds map as follows:
      - `Master` → `Master`;
      - `Instrument`, `InstrumentOutput` and `BuiltinSynth` → `Instrument`;
      - `AudioPath` → `Channel`;
      - `Bus` → `Bus` when any strip's `output` is `Bus(it)`; else `AuxReturn` when at least one send targets it; else (it has no feed at all) `Bus`.
    - Instrument strips are channels with their own token, `strip.instrument`, which the design spec already has. The spec's four cap kinds become five.
    - The field is `StripRow.kind_colour: StripColourRow`. `StripRow.colour` is the user's swatch (C-1, Ruling 12). Fader caps always follow `kind_colour`; the colour bar follows `colour` when it is set.
    - Why: spec §2 item 8 ("a bus fed only by sends is coloured as an aux return, and any other bus as a bus"). Routing logic stays in Rust, where `state.get mixer` reads it too.
    - Cost if wrong: one function.
11. **Ruling (the control channel).**
    - **`ui.theme`** (`key_only`, menu "View", title "Theme", no document needed) takes `theme`, a required Choice of `classic-light|studio-dark|flat|console|symbiotic`. Its answer's status is `"theme ‹id›"`. The registry goes from 153 to 154.
    - **`state.get mixer`** answers `ControlResult::Mixer { doc, mixer: MixerSnapshot }`, in this JSON shape:
      ```json
      {"doc": "…", "strips": [{"id": 3, "name": "Built-in Synth", "kind_label": "Built-in Synth",
        "kind_colour": "instrument", "colour": "red" | null, "gain_db": 0.0, "pan": 0.0, "mute": false, "solo": false,
        "output": null, "automation": "read",
        "eq": {"slot": 9, "name": "…", "bypass": false} | null,
        "inserts": [{"slot": 10, "name": "…", "bypass": false}],
        "sends": [{"target": 7, "level_db": -6.0, "pre_fader": false, "enabled": true}]}, …]}
      ```
      `kind_colour` is `channel`, `instrument`, `aux_return`, `bus` or `master`. `colour` is one of the eight swatch names (`red` … `pink`) or null.
    - **`state.get windows`** gains `"theme": "‹id›"`, the app's theme, because the theme is app-wide.
    - **No EQ or strip-colour command on the channel.** The registry has no strip or insert command today; the mixer is driven by `automation.gesture` and the UI. The e2e loads no plug-ins (`--no-scan`, offline), so an EQ command could not be exercised end to end. The EQ is covered by `core-ffi`'s test plug-ins (B2) and README step 214. Should a later plan add strip commands, `mixer.eq` joins them.
    - Why: spec §4 ("breaking agents and tests": state the channel can read is updated, not dropped).
    - Cost if wrong: one more registry entry.
12. **Ruling (the theme preference and where it applies).**
    - **The preference:** `AppearancePreferences(defaults:)` (`VizDAWAppModel`) stores `AppearanceSettings { theme: String, colourBars: Bool }` as JSON under the key `"appearance"`.
      - Decoding is tolerant per field, as `AudioSettings`' is (`KeyedDecodingContainer.field`). An unknown id gives `classic-light` and keeps a `colourBars` that reads: `{"theme":"neon","colourBars":false}` gives Classic Light with `colourBars == false`. Bytes that are not JSON give `classic-light` and `colourBars` true. Nothing is written back until `choose` or `setColourBars`.
      - `AppearancePreferences.defaultID == "classic-light"`.
    - **The SwiftUI root:** every scene's root view and every `NSHostingView` the app makes (plug-in windows, torn editors, the Modify panel) applies `ThemedRoot` (`Theming.swift`). It sets:
      - `.symbioticTheme(theme)`, which sets `\.symbioticTheme` and `\.colorScheme`;
      - `.foregroundStyle(text.primary, text.secondary, text.disabled)`, so SwiftUI's `.primary`, `.secondary` and `.tertiary` are the theme's;
      - `.tint(accent.accent)`;
      - `.background(surface.window)`.
    - **AppKit windows:** `ThemeWindows.apply(_:)` first sets `NSApp.appearance = NSAppearance(named: t.appearance.nsAppearanceName)`, so windows, menus, `NSAlert`s and open panels made later follow the theme. It then sets every VizDAW `NSWindow`'s `appearance` to the same and its `backgroundColor` to `surface.window`.
      - `AppDelegate.applicationDidFinishLaunching` calls `ThemeWindows.apply(appearancePreferences.theme)` first thing, once at launch (`NSApp` may still be nil in a SwiftUI `App.init`), and `AppearancePreferences.onChange`, set in `VizDAWApp.init`, calls it on each change.
      - `ThemeWindows.style(_ window:)` styles each window as it is made, through the paths that make them: `PluginWindows.show`, `EditorWindows` and `ModifyPanels`.
    - **Canvases:** `EditorCanvas` and every other `NSView` in `VizDAWEditors` gain `var theme: Theme` (redraw on set). Each host's `updateNSView` sets it from `context.environment.symbioticTheme`.
    - **Colour bars (controller ruling C-1: a real optional per-strip colour).** §3's "no change to the document format beyond the EQ slot" is overridden by controller ruling C-1 (owner to confirm at G1, Q2).
      - `core_model::mixer::Swatch { Red, Orange, Yellow, Green, Teal, Blue, Purple, Pink }` (serde `snake_case`), and `Strip.colour: Option<Swatch>`, with `#[serde(default, skip_serializing_if = "Option::is_none")]`; `Strip::new` sets `None`. It rides the same v3→v4 bump (Ruling 8), and `migrate_v3_to_v4` stays the identity.
      - `MixerCmd::SetColour { strip: StripId, colour: Option<Swatch> }`, title "Set Strip Colour"; an unknown strip is `UnknownId`.
      - Over the FFI: `SwatchRow` (the same eight cases), `StripRow.colour: Option<SwatchRow>`, and `Session::set_strip_colour(strip: u64, colour: Option<SwatchRow>) -> Result<DocSnapshot, VizError>` in `ffi_guard`.
      - The bar shows `StripPalette.bar(strip, theme)`: the `swatch.*` token when `colour` is set, otherwise the kind's `strip.*`. Fader caps stay by kind.
      - The strip name's context menu gains "Colour ▸ None, Red, Orange, Yellow, Green, Teal, Blue, Purple, Pink", each item with its token swatch. A choice is one undo step.
      - Settings › Appearance › "Show channel colour bars" (`colourBars`) still hides every bar, user colour or not.
      - Lanes and the Kontrol's LEDs still show the kind (backlog row, B8).
    - Why: spec §2 item 6 ("chosen in Settings, applies to the whole app including the mixer, and switches live") and §5 ("a user preference, not a project setting. Tests never touch the real preferences").
    - Cost if wrong: one modifier.
13. **Ruling (the colour and metric sweep, and its scan).**
    - Every colour in `app/Sources/VizDAW` and `app/Sources/VizDAWEditors` comes from the theme. The only files that may name a concrete colour are the two bridges, `app/Sources/VizDAW/Theming.swift` and `app/Sources/VizDAWEditors/EditorTheme.swift`.
    - `noColourLiteralsOutsideTheThemeBridges` (B5) fails on any of the following elsewhere:
      - `NSColor.<name>`, `Color.<name>` (`Color.clear` and `NSColor.clear` excepted);
      - `Color(red:`, `NSColor(red:`, `NSColor(calibrated`;
      - `Color.accentColor`;
      - a bare member colour anywhere, context-typed or not (`return .red`, `case "green": .green`, `[.systemBlue, …]`), by this pattern: `(?<![A-Za-z0-9_])\.(system(Red|Orange|Yellow|Green|Mint|Teal|Cyan|Blue|Indigo|Purple|Pink|Brown|Gray)|labelColor|secondaryLabelColor|tertiaryLabelColor|separatorColor|controlAccentColor|windowBackgroundColor|textColor|black|white|red|orange|yellow|green|mint|teal|cyan|blue|indigo|purple|pink|brown|gray)\b`.
    - `noCornerRadiusOrFontSizeLiterals` (B5) fails on `cornerRadius: <number>`, `.system(size: <number>`, `systemFont(ofSize:` and `SystemFontSize` in those two directories: radii come from `theme.radius` and sizes from `theme.type`.
    - `theScanSeesEachForbiddenForm` (B5) runs both patterns over a fixed list of 12 known-bad lines, one per form: each of the 12 lines matches at least one pattern, and a clean line (`theme.state.error.color`) matches none, so the scan cannot pass vacuously.
    - Spacing comes from `theme.spacing` in the views 7b redraws: the strip, `MixerView`, `MixerBar`, `DocumentTransportBar`, `PaneFrame` and Settings › Appearance. Review checks this; the scan does not. The other views' padding literals go to 7d's list (backlog row, B8).
    - The colour roles: the editor roles, all in `EditorTheme.swift`, then the mixer's context-typed sites (`MixerView.swift`, swept in B5):

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
      | lane colours (piano roll, `PianoRollPalette`) | `swatch.*`, in order (`red` … `pink`) |
      | slot dot: faulted | `state.error` |
      | slot dot: placeholder | `state.warning` |
      | slot dot: loading | `state.offline` |
      | slot dot: live | `state.play` |
      | slot dot: live and bypassed | `text.disabled` |
      | mixer meter well | `meter.background` |
      | mixer meter peak | `meter.clip` at or above 1.0, else `meter.low` (until B6's `StripPalette.meter`) |
      | M tint | `state.mute` |
      | S tint | `state.solo` |
      | automation "off" | `state.offline` |

    - Why: spec §4 ("Every colour and metric comes from theme tokens, so switching the theme changes everything").
    - Cost if wrong: one table.
14. **Ruling (the strip's layout and controls).** One `ChannelStrip` view, 120 points wide (`StripLayout.width`), in every placement. Top to bottom:
    1. **Colour bar:** 4 points, `StripPalette.bar(strip, theme)` (the strip's swatch when set, else the kind's `strip.*`; C-1), hidden when `colourBars` is off. Then the name: double-click renames; the context menu is today's (Rename…, Set All Strips to, Remove Bus, MIDI Learn Maps…, Add Outputs) plus "Colour ▸ None, Red … Pink" (Ruling 12); the kind label is below it.
    2. **Input:** today's `StripInputView`, for an audio path's input lane. The instrument's own slot stays above it, as today.
    3. **EQ slot:** empty, it reads exactly `EQ ＋` (U+FF0B) and a click opens the effect browser for `.eq(strip:)`. Loaded, it shows the plug-in's name and a bypass toggle; a click opens its plug-in window. Its menu has Open, Load (when held), Replace…, Bypass/Turn On, Sidechain, and Remove EQ.
    4. **Inserts:** today's `SlotView` rows (reorder, drop, sidechain, Load, Remove) restyled, with "+" below.
    5. **Sends:** one row per send.
       - The target's name on its own line, then below it a small knob (28), arc `control.send`, unipolar over `MixerMath.faderPosition`, beside a number box with the level (`StripEntry.levelText`). A knob's 28 plus a box's 56 and the spacing already fill 120 less the padding, so the name cannot share their row.
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
        - R arms the strip's input lane and I cycles its monitor, through `MixerModel.toggleArm` and `cycleMonitor` (5d-2 Ruling 4: audio paths only). Both are disabled when `inputLane(for:)` is nil, so they stay disabled on instrument strips and master (controller's ruling on Q3, binding); MIDI arm stays on the lane headers.
        - Each toggle keeps its touch ring and MIDI Learn item, as today.
    11. **Automation mode:** today's menu over the six modes (off, read, touch, latch, write, trim), tinted with `StripPalette.automation(mode)`.
    - **Hit frames:** every knob, fader cap, number box and toggle takes its frame from `ControlMetrics.hitSize(_:)` (Ruling 4).
    - **In the left pane:** 7a's `LeftStripPane` wraps the strip in `ScrollView(.vertical)` (`PaneViews.swift`), which gives `ChannelStrip(height:)` no height. B7 replaces that with `GeometryReader { ChannelStrip(model:strip:height: $0.size.height) }`, and `ChannelStrip` scrolls itself by `StripLayout.split(...).scrolls`, as in `MixerView`.
    - **Accessibility labels** (the e2e finds them):
      - "Level of ‹name›", "Pan of ‹name›" and "Send ‹n› of ‹name›" (n from 1), for the number boxes, both idle and editing;
      - "Fader of ‹name›" and "Pan knob of ‹name›";
      - "EQ of ‹name›", whose value is "empty" or the plug-in's name.
    - Every control keeps its touch ring, `LearnItem` and help text from today's strip.
    - Why: spec §2 item 5 and §4 ("controls too small to use").
    - Cost if wrong: a view rearranged; the models stay.
15. **Ruling (typed values): a typed value is one click gesture; the grammar is strict.**
    - **Digits** are ASCII `0`–`9` only, in both grammars: full-width, Arabic-Indic and other Unicode digits are refused.
    - **Levels and sends:** after trimming, an optional sign (`+`, `-` or U+2212 `−`), digits, optionally `.` and digits (or `.5`), then an optional `dB` (any case, with or without one space).
      - `-inf`, `−inf`, `-∞`, `−∞` (with an optional `dB`) mean −∞.
      - The value is rounded to 0.1 dB, half away from zero, and clamped to −144…+12; −144 and below read −∞. A result of −0 is normalised to 0: `"-0.04"` gives 0, and `levelText(-0.0) == "0.0 dB"`.
      - Refused: exponents, a comma, `nan`, `inf` without a minus, a second sign or point, other units, control characters, and more than 32 characters.
    - **Pan:** after trimming and folding case:
      - `C`, `0`, or a signed number (`-`, `+` or U+2212 `−`: `"−24"` gives −0.24);
      - or `L` or `R`, an optional space, and an unsigned number.
      - The number is rounded to an integer and clamped to 0…100; L is negative. Refused: anything else (`LR`, `L-5`, `24L`, `C 5`, `L` alone, `L ２４`, `R ٣`).
    - **Applying:** an accepted value goes through `MixerModel.click(target, norm)`, so it is one undo step, as today's 0 dB double-click is.
      - The fader and sends use `AutomationLaw.faderNorm(db)`; pan uses `(pan + 1) / 2`.
      - A refusal sets `document.editor.status` and sends nothing. The messages are "Type a level in dB, like −4.2 or −inf." (`StripEntry.levelHint`) and "Type a pan like L 24, C or R 10." (`StripEntry.panHint`).
      - A value typed at a control that is touched (held by a drag or a surface, where `MixerModel.click` would drop it) is refused: the typed setter returns false and sets the status "Release the control first." (`StripEntry.touchedHint`).
    - Why: spec §2 item 5 ("every number box can be clicked and typed into … hostile-input tested").
    - Cost if wrong: one grammar, pinned by six tests.

## Spec checks (for the owner, no plan change needed)

1. **Theme name.** §7's 7b row says "SVP Light the default". §2 item 6's controller's call names it "Classic Light" in the UI, for the clean room. The plan follows §2.
2. **No strip commands.** The control registry has no strip or insert commands today, so the EQ slot gets no channel command (Ruling 11). The channel reads it through the new `state.get mixer`.
3. **The colour bar (C-1).** C-1: a real optional per-strip colour; §3's "no other format change" is overridden by controller ruling C-1 (owner to confirm at G1). The bar shows the strip's swatch, or its kind's colour when none is set, with a switch in Settings (Ruling 12). It rides the EQ slot's v3→v4 bump.
4. **Five cap kinds.** §2 item 7 lists four fader-cap kinds; the token set already has five, adding instrument. Instrument strips get `strip.instrument` (Ruling 10).
5. **Licence (settled by the owner, 2026-11-03).** VizDAW is GPL-3.0, and the `SymbioticBrand` product it links by default is all rights reserved. The owner chose a GPL-3.0 §7 additional permission. B3 appends it to `LICENSE` and adds one line to the root README's licence section (Ruling 6). The artwork stays all rights reserved; the no-brand build is what a fork ships.
6. **Metrics.** §4 says every colour *and metric* comes from tokens. This plan scans for colours, radii and font sizes everywhere, and keeps spacing on tokens in the views it redraws. The remaining views' padding literals go to 7d (Ruling 13).
7. **Hardware colours.** The Kontrol MK3's colours per track kind (`core_surface::kontrol::color`) are hardware LED values, not UI. They stay, and keep showing the kind even when a strip has a user colour.
8. **R and I on instrument strips.** R/I follow 5d-2 Ruling 4, audio paths only; MIDI arm stays on the lane headers. They are disabled on instrument strips (controller's ruling on Q3).
9. **`HANDOFF.md` is local only (owner, 2026-11-03).** A4 stops tracking it. It stays in the pushed history of `main` and `eg13-requests`; removing it from there needs a force-push rewrite, which is the owner's decision and is not in this plan.
10. **Knob finish (owner, 2026-11-03).** HANDOFF (j)'s "flat, no shading" stays the family default: `knobFinish` is a theme token, by default flat in four themes and shaded in Console; the owner may change any theme's finish at G1, and a theme made `shaded` also gets non-clear `relief` values. Geometry is family.

## File map

| File | Task | Role |
|---|---|---|
| **symbiotic-design:** `docs/mockups/0.2.0/{palettes.json,themes.html}` (new), `tools/mockup.py` (new), `tools/tests/test_mockup.py` (new) | A1 | mockups |
| `tokens/tokens.json`, `tokens/contrast.md`, `tools/{tokenlib.py,swiftgen_tokens.py}`, `tools/tests/{test_tokenlib.py,test_swiftgen_tokens.py}`, `Sources/SymbioticTokens/Generated/*`, `Tests/SymbioticTokensTests/ThemeTests.swift`, `README.md` | A2 | tokens 0.2.0 |
| `icons/roles.json`, `icons/standard.json`, `icons/standard/*.svg` (11 new; `info.svg` and `split.svg` unchanged), generated icons, references, `icons/contact-sheet.html`, `icons/coverage.md`, `tools/tests/test_rolelib.py`, `Tests/SymbioticIconsTests/RoleTests.swift` | A3 | icon roles |
| `Package.swift`, `Sources/SymbioticControls/*` (new), `Tests/SymbioticControlsTests/*` (new), `.github/workflows/ci.yml` (fork step, `swift61` job), `.gitignore` (`HANDOFF.md`), `README.md`, `docs/superpowers/specs/2026-10-20-design-system-design.md` (§7 addendum, HANDOFF wording); `HANDOFF.md` untracked (`git rm --cached`) and edited locally only | A4 | controls, release |
| **vizdaw:** `crates/core-model/src/{mixer.rs,document.rs,mixer_rules.rs,file.rs,automation.rs,command/mixer.rs}`, `crates/core-engine/src/graph/mixer.rs`, `crates/core-host/src/host.rs`; tests `core-model/tests/{mixer_commands.rs,file.rs,mixer_document.rs,learn.rs}`, `core-engine/tests/graph_mixer.rs`, `core-host/tests/host_loads.rs`; `fixtures/demo.vizdaw/project.json` (regenerated) | B1 | EQ slot and strip colour core |
| `crates/core-ffi/src/{mixer.rs,session.rs,plugins.rs,host.rs,surfaces.rs,control.rs}`, `crates/core-control/src/request.rs`, tests `core-ffi/tests/{mixer.rs,control.rs}`, `core-control/tests/commands.rs`; `app/Tests/VizDAWAppModelTests/FakeSession.swift`, `app/Sources/VizDAWAppModel/ControlCommands.swift` | B2 | EQ and colour FFI, kind colours, `state.get mixer` |
| `app/Package.swift`, `app/Package.resolved` (new), `.github/workflows/ci.yml`, `.gitignore`, `LICENSE` (copyright line, §7 permission), `scripts/make-app.sh` (`LICENSE` into the bundle), `README.md` (licence line, build sentence), `app/Sources/VizDAWAppModel/AppIcon.swift` (new), `app/Sources/VizDAW/*` (icon sites incl. `PaneViews.swift`, `VizDAWApp.swift`), `app/Tests/VizDAWAppModelTests/{AppIconTests.swift,SourceScan.swift}` (new) | B3 | pin, icons, licence |
| `crates/core-control/src/registry.rs`, `crates/core-ffi/src/control.rs`, `core-control/tests/commands.rs`; `app/Sources/VizDAWAppModel/{AppearancePreferences.swift,AppearanceCommands.swift}` (new), `ControlCommands.swift`; `app/Sources/VizDAW/{Theming.swift,AppearanceSettingsView.swift}` (new), `VizDAWApp.swift`, `ControlChannel.swift`, `PluginWindows.swift`, `EditorWindows.swift`, `ModifyPanel.swift`, `{ArrangementPane,PianoRollPane,ListEditorPane,DrumGridPane,StripChartPane}.swift`; `app/Tests/VizDAWAppModelTests/{AppearanceTests.swift (new),ControlCommandsTests.swift}` | B4 | themes, Settings, `ui.theme` |
| `app/Sources/VizDAWEditors/EditorTheme.swift` (new) and every file the scan names (`PianoRollView.swift`'s `PianoRollPalette` → `swatch.*`); `app/Sources/VizDAW/*` colour sites; `app/Tests/VizDAWEditorsTests/ThemeScanTests.swift` (new) | B5 | colour sweep |
| `app/Sources/VizDAWAppModel/{StripEntry.swift,StripLayout.swift,StripPalette.swift}` (new), `MixerModel.swift` (EQ, `setColour`, typed values, arm, `heldSlots` → `slots`), `PluginWindowModel.swift` and `StripChartModel.swift` (→ `StripRow.slots`); `app/Tests/VizDAWAppModelTests/{StripEntryTests.swift,StripLayoutTests.swift,MixerStripTests.swift}` (new) | B6 | strip models |
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
  - `palettes.json`: `{"themes": [ {id, name, appearance, knobFinish, values: {<every 0.1.0 group>, knob, fader, relief, swatch}} ×5 ]}`, in the id order `classic-light`, `studio-dark`, `flat`, `console`, `symbiotic`; `knobFinish` is `"flat"` or `"shaded"`, as recorded at G1 (default: Console `shaded`, the rest `flat`);
  - `tools/mockup.py`: `def css_name(group: str, token: str) -> str` (`("strip", "auxReturn")` → `"--strip-aux-return"`); `def render(palettes: dict) -> str`; and `main(argv)`, as `mockup.py IN OUT`;
  - `NEW_GROUPS = {"knob": [...], "fader": [...], "relief": [...], "swatch": ["red", "orange", "yellow", "green", "teal", "blue", "purple", "pink"]}`, the token names of Ruling 3, which A2's schema must match.

- [ ] **Step 1: Make the branch.** `git switch -c sp7-0.2.0 b209d2c`, then check `git log -1 --oneline` reads `b209d2c`.
- [ ] **Step 2: Write the failing tests** in `tools/tests/test_mockup.py` (`unittest`):
  - `test_every_theme_has_every_token`: for each of the five themes in `palettes.json`, the set of `group.token` keys equals `tokens.json`'s groups plus `NEW_GROUPS`. In groups of kind `color` (the new groups included) every value is `#RRGGBB` or `#RRGGBBAA` (no `system`). The metric groups (`spacing`, `radius`, `iconStroke`, `type`) match `tokens.json`'s kinds, and `brand` (kind `color`, `constant: true`) equals today's values in every theme. `name` is one of the five exact names, and `knobFinish` is `flat` or `shaded`, as recorded at G1 (default: Console shaded); every `shaded` theme has non-clear `relief.capTop`/`capBottom`.
  - `test_css_names_are_kebab_case_and_unique`: `css_name("strip", "auxReturn") == "--strip-aux-return"`, `css_name("relief", "capTop") == "--relief-cap-top"`, and no two tokens map to one name.
  - `test_the_page_uses_only_theme_variables`: `render(palettes)` contains one `[data-theme="<id>"]` block per theme. Outside those blocks the page has no `#hex`, `rgb(`, `hsl(`, `url(` or `<img`, and its only `font-family` is the system stack. Every `var(--…)` it uses is defined in each block. Each block also sets `data-knob-finish` to its theme's finish, and the page's swatch row uses all eight `--swatch-*` variables.
- [ ] **Step 3: Run them and see them fail.** `python3 -m unittest tools.tests.test_mockup -v`. Expected: an import error, because `tools/mockup.py` does not exist.
- [ ] **Step 4: Write `tools/mockup.py`, `palettes.json` and the page.**
  - Follow Ruling 2's content and per-theme direction.
  - `symbiotic` copies today's values and adds the new groups.
  - Every `flat` theme but Console has all six `relief` tokens `#00000000`. Console's are real, as are a `shaded` theme's (Ruling 3), drawn with CSS `linear-gradient` and `box-shadow`, and nothing else. A knob is shaded on the page only where its theme's `knobFinish` is `shaded`.
  - The eight `swatch` values are distinct within each theme, and readable as a 4-point bar on `surface.panel`.
  - Theme `note`s name no banned brand (Ruling 3).
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
- Modify: `tokens/tokens.json`, `tools/tokenlib.py` (`validate`: `name`, `since`, `knobFinish`), `tools/swiftgen_tokens.py` (name, `KnobFinish`, defaulted init parameters, the `classicLight` statics), `tools/tests/test_tokenlib.py`, `tools/tests/test_swiftgen_tokens.py`, `Tests/SymbioticTokensTests/ThemeTests.swift`, `README.md` (Themes, Checking by eye)
- Regenerated: `Sources/SymbioticTokens/Generated/*`, `tokens/contrast.md`

**Interfaces:**
- Consumes: the G1-approved `palettes.json` (its SHA in `progress.md`), and `NEW_GROUPS` (A1).
- Produces (Swift, `SymbioticTokens`, all public):
  - `Theme.name: String`;
  - `public enum KnobFinish: String, Sendable { case flat, shaded }` and `Theme.knobFinish: KnobFinish`;
  - `Theme.knob: Theme.KnobColors { skirt, cap, track, indicator }`;
  - `Theme.fader: Theme.FaderColors { well, scale, capLine }`;
  - `Theme.relief: Theme.ReliefColors { highlight, shadow, capTop, capBottom, panelTop, panelBottom }`;
  - `Theme.swatch: Theme.SwatchColors { red, orange, yellow, green, teal, blue, purple, pink }` (C-1);
  - the statics `Theme.KnobColors.classicLight`, `Theme.FaderColors.classicLight`, `Theme.ReliefColors.classicLight`, `Theme.SwatchColors.classicLight`;
  - `Theme.init(id:name:appearance:knobFinish:isPlaceholder:…:knob:fader:relief:swatch:)`, where `name`, `knobFinish`, `knob`, `fader`, `relief` and `swatch` have defaults (Ruling 3). Use the generator's real parameter order and say so if it differs.

- [ ] **Step 1: Write the failing Python tests.**
  - `test_tokenlib.py`:
    - `test_a_theme_needs_one_unique_name`: missing, empty, 33 characters, a newline, or two themes with the same name are each one problem.
    - `test_since_marks_a_group`: `"since": "0.2.0"` is accepted, `"since": 2` is a problem, and an unknown group key is still refused.
    - `test_no_theme_ships_as_a_placeholder`: in the real `tokens.json` every theme has `placeholder` false and no `system` value, and no theme's `name` or `note` contains, case-insensitively, any of these 12 banned names: svp, studio vision, opcode, logic, apple, midas, soundcraft, steinberg, cubase, metro, zune, microsoft.
    - `test_knob_finish_is_flat_or_shaded`: a missing `knobFinish`, or `"matte"`, is one problem; in the real `tokens.json` only the themes G1 recorded as shaded are `shaded` (default: `console` only).
    - `test_body_text_reads_on_every_surface`: in each theme, `text.primary` on `surface.window`, `panel`, `raised` and `inset` has `contrast_ratio ≥ 4.5`.
  - `test_swiftgen_tokens.py`:
    - `test_since_groups_and_the_name_have_defaults`: the generated `init` reads `name: String? = nil`, `knobFinish: KnobFinish = .flat`, `knob: KnobColors = .classicLight`, `fader: FaderColors = .classicLight`, `relief: ReliefColors = .classicLight` and `swatch: SwatchColors = .classicLight`, and `Theme.name` falls back to `id`.
  - Rewrite (do not delete) the existing tests that the 0.2.0 values break:
    - `test_tokenlib.test_tokens_json_is_valid_and_has_the_five_themes` (it expects placeholders and system colours);
    - `test_tokenlib.test_spec_groups_are_present` (it asserts the exact sorted list of 13 groups): the group list gains `knob`, `fader`, `relief` and `swatch`;
    - `test_tokenlib.test_symbiotic_theme_is_the_site_look` (it asserts `"symbioticaudio.org"` in the symbiotic theme's `note`): the symbiotic note keeps "from the symbioticaudio.org site palette".
- [ ] **Step 2: Write the failing Swift tests** in `ThemeTests.swift`:

```swift
@Test func themesHaveTheirNamesInOrder() {
    #expect(Theme.all.map(\.name) == ["Classic Light", "Studio Dark", "Flat", "Console", "Symbiotic"])
    #expect(Theme.all.allSatisfy { !$0.isPlaceholder })
}
@Test func theNewGroupsReadAsTyped() {
    #expect(Theme.console.relief.capTop != Theme.console.relief.capBottom)
    #expect(Theme.studioDark.knob.skirt.fallbackComponents.alpha == 1)
    for t in Theme.all {  // C-1: eight distinct swatches in every theme
        let w = t.swatch
        let all = [w.red, w.orange, w.yellow, w.green, w.teal, w.blue, w.purple, w.pink]
        #expect(Set(all.map { "\($0.fallbackComponents)" }).count == 8, "\(t.id)")
    }
}
@Test func flatThemesHaveClearRelief() {
    // Keyed on the finish G1 recorded, not on the id (Ruling 3).
    for t in Theme.all where t.knobFinish == .flat && t.id != "console" {
        let r = t.relief
        #expect([r.highlight, r.shadow, r.capTop, r.capBottom, r.panelTop, r.panelBottom]
            .allSatisfy { $0.fallbackComponents.alpha == 0 }, "\(t.id)")
    }
    for t in Theme.all where t.knobFinish == .shaded {
        #expect(t.relief.capTop != t.relief.capBottom, "\(t.id)")
    }
}
@Test func aZeroOneThemeStillBuilds() {
    // EG13's call shape at 0.1.0: no name, knob, fader or relief.
    let t = Theme(id: "eg13", appearance: .dark, surface: Theme.studioDark.surface, text: Theme.studioDark.text,
        accent: Theme.studioDark.accent, state: Theme.studioDark.state, meter: Theme.studioDark.meter,
        graph: Theme.studioDark.graph, control: Theme.studioDark.control, strip: Theme.studioDark.strip,
        brand: Theme.studioDark.brand, spacing: Theme.studioDark.spacing, radius: Theme.studioDark.radius,
        iconStroke: Theme.studioDark.iconStroke, type: Theme.studioDark.type)
    #expect(t.name == "eg13" && t.knobFinish == .flat)
    #expect(t.relief == Theme.ReliefColors.classicLight && t.swatch == Theme.SwatchColors.classicLight)
}
```

  Rewrite (do not delete) the four existing Swift tests that the 0.2.0 values break, so the counts hold:
  - `ThemeTests.themesInTokensOrder` (`isPlaceholder == [f,f,t,t,f]`): the list becomes all false;
  - `increaseContrastIsKeptWhileTheAppearanceStaysPinned` (reads `classicLight.text.secondary` as `secondaryLabelColor`), `customDarkThemeResolvesCopiedSystemColoursDark` and `customThemeIsBuiltFromScratchAndReadThroughTheEnvironment` (`surface != classicLight.surface` after re-pinning): each builds a local `Theme` whose `surface` and `text` use `ThemeColor(system:fallback:appearance:)`, so re-pinning and Increase Contrast stay tested for EG13-style custom themes.
- [ ] **Step 3: Run them and see them fail.** Run `python3 -m unittest discover -s tools/tests` (the six new tests fail) and `scripts/swift.sh test` (the new tests fail to compile, because `Theme.name` is not defined).
- [ ] **Step 4: Implement.**
  - Add `name` and `knobFinish` to each theme. Add the four groups (`knob`, `fader`, `relief`, `swatch`) to `tokens` with `"since": "0.2.0"` and their one-line descriptions (Ruling 3).
  - Copy the five approved themes' `values` and `knobFinish` from `palettes.json` into `themes`, set `placeholder: false`, and replace each `note` with one line naming its direction (no banned names: "light neutral-grey chrome", not a vendor's appearance name; the symbiotic note keeps its symbioticaudio.org reference).
  - `tokenlib.validate` checks `name` and accepts `since`. `swiftgen_tokens` emits the defaults and the statics.
  - In `README.md`, the Themes section names the five themes by name with their ids. The "Checking by eye" bullet on Increase Contrast becomes: "Read `tokens/contrast.md`: body text passes AA on every surface in every theme (tested)."
  - Then run `python3 tools/generate.py`.
- [ ] **Step 5: Run everything and see it pass.** Python 183 (177 + 6), Swift 50 (46 + 4); `git status --porcelain` lists only this task's files.
- [ ] **Step 6: Commit** the files above, plus the regenerated `Sources/SymbioticTokens/Generated/*` and `tokens/contrast.md`. Message: "Tokens 0.2.0: the approved palettes, theme names, knob finish, knob, fader, relief and swatch groups, no placeholders", with the trailers. Check the author line as in A1.

---

### Task A3: The icon roles VizDAW needs

**Files:**
- Create: `icons/standard/{left-pane,bottom-pane,right-pane,mixer,split-stacked,empty,keys,audio,surfaces,appearance,eq}.svg` (eleven; `info.svg` and `split.svg` already exist and are not touched)
- Modify: `icons/roles.json`, `icons/standard.json`, `tools/tests/test_rolelib.py`, `Tests/SymbioticIconsTests/RoleTests.swift`
- Regenerated: `Sources/SymbioticIcons/Generated/*`, the reference PNGs (`python3 tools/references.py`), `icons/contact-sheet.html`, `icons/coverage.md`

**Interfaces:**
- Produces: the `IconRole` cases `.viewLeftPane`, `.viewBottomPane`, `.viewRightPane`, `.viewMixer`, `.viewSplit`, `.viewEmpty`, `.viewExpand`, `.viewCollapse`, `.settingsKeys`, `.settingsAudio`, `.settingsSurfaces`, `.settingsAppearance`, `.statusInfo` and `.mixerEq`. Use the generator's own spelling of each case and report it if it differs.

- [ ] **Step 1: Write the failing tests.**
  - `test_rolelib.py`'s `test_vizdaw_sp7_roles_exist`: the fourteen ids of Ruling 5 are in `roles.json`, each with its `standard` icon present in `standard.json`, a one-line `label`, and no `brand`. `status.info` names `info` and `view.split` names `split-stacked`. `icons/standard/split.svg` and `info.svg` are unchanged, checked against fixed SHA-256 digests with `hashlib.sha256(path.read_bytes())` (not `git show b209d2c:…`, which CI's shallow `actions/checkout@v4` clone cannot read): `split.svg` `ff981d5dccfb96a96736a4ee20b34713339b75d9345d050ca5e962a6f828d070`, `info.svg` `6cd55d98958a237fe54889b38a13bebaa0972a7884659061b71bc9b0dedec676`.
  - `test_rolelib.py`'s `test_coverage_briefs_the_new_roles`: the generated `coverage.md` lists all fourteen under "Brief for the designer" (roles with no brand glyph).
  - `RoleTests.swift`'s `sp7RolesDrawTheirStandardIcons`: for each of the fourteen roles, `NSImage.icon(role, size: 16, theme: .classicLight)` is a template image of size 16×16 that is not blank (some pixel's alpha is above 0).
- [ ] **Step 2: Run them and see them fail.**
- [ ] **Step 3: Draw the eleven SVGs** in the SVG subset (design spec §3.1).
  - Pane icons are a window outline with the named side filled. `mixer` is three fader slots with caps; `split-stacked` is two stacked rectangles; `empty` is a dashed rectangle; `keys` is a keyboard; `audio` is a speaker; `surfaces` is a fader-and-knob control surface; `appearance` is a half-filled circle; `eq` is a curve over three band dots.
  - `status.info` reuses the existing `info.svg`; do not edit `info.svg` or `split.svg`.
  - Add the roles. Then regenerate everything (`generate.py`, `references.py`, `contact_sheet.py`).
- [ ] **Step 4: Run everything and see it pass.** Python 185, Swift 51. The fidelity tests cover the new icons through their existing arguments.
- [ ] **Step 5: Commit** the eleven new SVGs, JSON, generated Swift, reference PNGs, contact sheet, coverage and the two test files (`git status` shows no change to `split.svg` or `info.svg`). Message: "Icons: fourteen roles for VizDAW SP7 (panes, mixer, EQ, settings, info) with standard icons", with the trailers.

---

### Task A4: `SymbioticControls`, the HANDOFF, and the 0.2.0 tag

**Files:**
- Modify: `Package.swift` (the product, the target `SymbioticControls` depending on `SymbioticTokens`, and the test target `SymbioticControlsTests`), `.github/workflows/ci.yml` (the fork step also runs `swift build --target SymbioticControls`; a new `swift61` job), `.gitignore` (`HANDOFF.md`), `README.md` (the products table, "Controls", the HANDOFF line), `docs/superpowers/specs/2026-10-20-design-system-design.md` (a short "§7 Controls (0.2.0)" addendum, and its HANDOFF mention)
- Untrack: `HANDOFF.md` (`git rm --cached HANDOFF.md`; the file stays on disk and is edited locally, never committed again)
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
    - `enum ControlKind { case knob(KnobSize), faderCap, numberBox, toggle }` and `static func hitSize(_ c: ControlKind) -> CGSize`, which every view uses for its hit frame;
    - `static let dragTravel: CGFloat = 200`;
    - `static let fineDivisor = 10.0`.
  - **Knob geometry:** `enum KnobGeometry` with:
    - `static func angle(_ value: Double) -> Angle` (0 → 225°, 1 → −45°, measured as SwiftUI's degrees from 3 o'clock, counter-clockwise positive);
    - `static func arc(origin: Double, value: Double) -> (from: Double, to: Double)` (values in 0…1, ordered);
    - `static func indicator(diameter: CGFloat, size: KnobSize, value: Double) -> CGRect` (its unrotated frame) and `static func band(diameter: CGFloat, size: KnobSize) -> CGFloat`.
  - **Fader geometry:** `enum FaderGeometry` with:
    - `static func capCentre(value: Double, throwLength: CGFloat) -> CGFloat`, where 0 is the bottom of the throw;
    - `static func value(capCentre: CGFloat, throwLength: CGFloat) -> Double`, clamped to 0…1.
  - **Drag:** `enum ControlDrag` with `static func value(start: Double, translation: CGFloat, travel: CGFloat, fine: Bool) -> Double`. Upward is positive (`translation` is SwiftUI's height, so it is negated inside); the result is clamped to 0…1. And `struct ControlDrag.Tracker: Sendable { init(start: Double, travel: CGFloat); mutating func value(translation: CGFloat, fine: Bool) -> Double }`, which re-anchors (start = current value, origin = current translation) whenever `fine` changes.
  - **Gestures:** `internal struct ControlGestureModel` (what the views use, `internal` for tests), with `changed(translation:fine:)`, `ended()` and `doubleTapped()`. It calls `onBegin` on the first change, `onEnd` only after a begin, and `onReset` on a double tap only when no drag began.
  - **Entry:** `enum NumberEntry` with `static let maxLength = 32` and `static func clean(_ s: String) -> String?`. `clean` trims whitespace, maps U+2212 to `-`, and gives nil for more than `maxLength` characters or any control character. Nothing ever cuts text to `maxLength`.
  - **Palettes:**
    - `struct KnobPalette { skirt, plainCap, darkCap, track, tick, indicator, indicatorOnDark, label, chipText, focus: Color; finish: KnobFinish; init(theme: Theme) }` (`finish` from `theme.knobFinish`). The slots map: `skirt` = `knob.skirt`, `plainCap` = `knob.cap`, `darkCap` = `surface.inset`, `track` = `knob.track`, `tick` = `knob.track`, `indicator` = `knob.indicator`, `indicatorOnDark` = `text.inverse`, `label` = `text.secondary`, `chipText` = `accent.onAccent`, `focus` = `surface.focusRing`;
    - `struct FaderPalette { well, scale, capLine: Color; init(theme: Theme) }`;
    - `struct NumberBoxPalette { background, editingBackground, text, refused, focus: Color; init(theme: Theme) }`;
    - `struct Relief: Equatable { highlight, shadow, capTop, capBottom: Color; static let none; init(theme: Theme); var isNone: Bool }`.
  - **Knob style:** `struct KnobStyle { var size: KnobSize; var arc: ArcPlacement = .skirtBand; var innerRing = false; var ticks = false; var cap: CapEmphasis = .plain; var label: LabelStyle = .text }`, with `enum ArcPlacement { case off, skirtBand, outside }`, `enum CapEmphasis { case plain, accent, dark }` and `enum LabelStyle { case text, chip }`.
  - **Views:**
    - `struct Knob: View { init(value: Double, origin: Double = 0, arcColour: Color, style: KnobStyle, palette: KnobPalette, relief: Relief = .none, label: String? = nil, chipFill: Color? = nil, accessibilityLabel: String, accessibilityValue: String, step: Double = 0.01, onBegin: @escaping () -> Void, onChange: @escaping (Double) -> Void, onEnd: @escaping () -> Void, onReset: (() -> Void)? = nil) }`. It draws `relief` only when `palette.finish == .shaded`;
    - `struct FaderMark: Equatable { var position: Double; var label: String }`;
    - `struct Fader: View { init(value: Double, throwLength: CGFloat, cap: Color, palette: FaderPalette, relief: Relief = .none, marks: [FaderMark], accessibilityLabel: String, accessibilityValue: String, step: Double, onBegin:, onChange:, onEnd:, onReset:) }`;
    - `struct NumberBox: View { init(text: String, accessibilityLabel: String, palette: NumberBoxPalette, commit: @escaping (String) -> Bool, onRefuse: (() -> Void)? = nil) }`. `commit` returning false keeps the old text shown. Text that `NumberEntry.clean` refuses is not passed to `commit`; `onRefuse` is called instead. Editing takes keyboard focus.

- [ ] **Step 1: Write the failing tests** (Swift Testing, in `SymbioticControlsTests`).
  - `GeometryTests.swift`:
    - `knobSizesAndCapsFollowTheHandoff`: diameters 28/38/48, and cap ratios 0.618/0.618/0.676 within 0.001.
    - `theSweepRunsFromSevenThirtyToFourThirty`: `angle(0)` is 225°, `angle(1)` is −45° and `angle(0.5)` is 90°, each within 1e-9.
    - `aBipolarArcFillsFromItsOrigin`: `arc(origin: 0.5, value: 0.2) == (0.2, 0.5)` and `arc(origin: 0, value: 0.7) == (0, 0.7)`.
    - `theIndicatorIsSizedFromTheCap`: for large at 48, the width is `max(2, 0.07 × 48)` and the height is `0.42 × 48 × 0.676`, within 0.01.
    - `aDragIsRelativeClampedAndFine`: `value(start: 0.5, translation: -100, travel: 200, fine: false)` is 1.0 (100 up is +0.5); with `fine: true` it is 0.55; `translation: +1000` gives 0.
    - `theFaderCapRoundTrips`: for every value in 0, 0.1 … 1, `value(capCentre: capCentre(v, 300), 300)` equals `v` within 1e-9; values outside clamp.
    - `everyControlMeetsTheMinimumHit`: for every `ControlKind` (the three knob sizes, `faderCap`, `numberBox`, `toggle`), `hitSize(kind)` is at least 24×24, and at least the drawn size.
    - `aFineToggleMidDragDoesNotJump`: a `ControlDrag.Tracker` from 0.5, dragged half-way, then ⌥ on: the value just after the toggle equals the value just before within 1e-9, and further movement is a tenth as fast.
    - `aClickWithoutMovementSendsNoGesture`: through `ControlGestureModel`, a press and release with translation 0 calls neither `onBegin` nor `onEnd`; a double tap calls `onReset` once; a drag then a double tap calls `onReset` not at all.
  - `EntryAndPaletteTests.swift`:
    - `numberEntryCleansAndRefuses`: `clean("  −4.2 dB ") == "-4.2 dB"`; `clean(String(repeating: "1", count: 33)) == nil`; `clean("4\u{0}2") == nil`; `clean("") == ""`.
    - `palettesComeFromTheTheme`: `KnobPalette(theme: .console).skirt == Theme.console.knob.skirt.color` and `FaderPalette(theme: .flat).well == Theme.flat.fader.well.color`.
    - `reliefIsNoneOutsideConsole`: for every theme, `Relief(theme: t).isNone == (t.knobFinish == .flat && t.id != "console")`.
    - `aNumberBoxRefusalKeepsTheOldText`: through `NumberBoxModel`, the view's internal editing state, exposed `internal` for tests. Begin, type "abc", commit with a `commit` returning false: `shown == "0.0 dB"` and `editing == false`. A commit returning true shows the caller's next `text`.
    - `escapeCancelsEditing`: `NumberBoxModel` begin, type "1", cancel: `shown` is unchanged and `commit` was not called.
    - `aLongPasteIsRefusedNotCut`: `NumberBoxModel` begin, paste 300 × "9", commit: `commit` was not called, `onRefuse` was called once, and `shown` is the old text.
    - `reliefIsNoneOutsideConsole` also checks `KnobPalette(theme: t).finish == t.knobFinish`.
- [ ] **Step 2: Run them and see them fail** (no such module).
- [ ] **Step 3: Implement** the files above to Ruling 4.
  - Drawing uses SwiftUI `Canvas` or shapes only, with no images. The relief gradients and bevels are drawn only when `!relief.isNone`.
  - Interaction stays separate from drawing (HANDOFF (k)): `DragGesture(minimumDistance: 2)` feeding `ControlGestureModel` and `ControlDrag.Tracker`, with `NSEvent.modifierFlags` read for ⌥ on each change; `.simultaneousGesture(TapGesture(count: 2))` for reset; `.focusable()` and `.onKeyPress` for the arrow keys; `accessibilityAdjustableAction`. No scroll wheel (Ruling 4) and no `NSEvent` monitor.
  - Add CI job `swift61` to `.github/workflows/ci.yml`: `runs-on: macos-15`, the runner's default Xcode (no `xcode-select`), steps `actions/checkout@v4` first, then `swift --version` and `swift build --package-path .` (all products).
  - No `@State`: spell out `State(initialValue:)` (Global Constraints).
- [ ] **Step 4: Run everything and see it pass.** Swift 66 (51 + 15), Python 185. Also build the fork target locally through the script (Part A allows Swift only through `scripts/swift.sh`): `scripts/swift.sh build --scratch-path "$(mktemp -d)/b" --target SymbioticControls`, the target CI's fork step builds, with `DEVELOPER_DIR` set. Expected: built, and no `SymbioticBrand` objects.
- [ ] **Step 5: Update the docs.**
  - `README.md`: the products table gains `SymbioticControls` (MIT: knob, fader, number box, geometry and interaction; colours injected through palettes), with a four-line usage example. The "Themes" text is A2's.
  - **`HANDOFF.md` becomes local only (owner ruling, 2026-11-03).** Run `git rm --cached HANDOFF.md` (the file stays on disk), add `HANDOFF.md` to `.gitignore`, and check `git ls-files HANDOFF.md` prints nothing. In `README.md`, the Contents line "`HANDOFF.md` — status and requests between the app projects." becomes "Requests between the app projects are kept in a local, unpublished HANDOFF file." The design spec's mention of "EG13's request list in `HANDOFF.md`" becomes "EG13's request list (kept locally)". Older plans under `docs/superpowers/plans/` are history and stay as written.
  - `HANDOFF.md` (the local file, not committed): replace the session name "svp-8b" with "the VizDAW session", and add a new "Status: 0.2.0 (VizDAW SP7)" section. It lists:
    - the five final themes and their names;
    - the new groups (`knob`, `fader`, `relief`, `swatch`) and `knobFinish`: "finish is a theme token; geometry is family" (Console shaded, the rest flat; EG13 stays flat by using a flat theme);
    - `relief`: an optional layer, off by default, drawn only where a theme's tokens are not clear (VizDAW's Console);
    - the fourteen roles;
    - `SymbioticControls`, with EG13's request 1 geometry adopted, and its palette slots matched to EG13's names (`skirt`, `plainCap`, `darkCap`, `track`, `tick`, `indicator`, `indicatorOnDark`, `label`, `chipText`, `focus`; the accent passed per knob as `arcColour`);
    - what is not in 0.2.0: EG13 requests 2–5 (the mod wave mark, the LCD readout, the mini trace, the card columns), the category and `goldInk` tokens, which (f) dropped, and scroll-wheel control (macOS 14 SwiftUI has none; a later SymbioticControls may add an `NSViewRepresentable` overlay);
    - an invitation to EG13 to adopt `SymbioticControls` at 0.2.0 or later.
  - The design spec's §7 addendum (one paragraph): the 0.2.0 schema additions and the product.
- [ ] **Step 6: Commit** by path: `Package.swift`, `Sources/SymbioticControls/`, `Tests/SymbioticControlsTests/`, `.github/workflows/ci.yml`, `.gitignore`, `README.md`, the design spec, and the `HANDOFF.md` removal from the index (staged by `git rm --cached`). Message: "SymbioticControls 0.2.0: knob, fader and number box shared by the family; README for 0.2.0; HANDOFF kept local", with the trailers. Check `git show --stat HEAD` lists `HANDOFF.md` as deleted and that the file is still on disk.
- [ ] **Step 7: Tag, locally only.** `git tag -a 0.2.0 -m "symbiotic-design 0.2.0: final palettes, theme names, knob/fader/relief tokens, SP7 icon roles, SymbioticControls"`. Check with `git cat-file -t 0.2.0` (expected: `tag`). **Do not push. Stop: Gate G2.**

---

## Part B — VizDAW (repo `/Users/lattawb/workspace/vizdaw`)

### Task B1: The EQ slot and the strip colour in the model, the format and the engine

**Files:**
- Modify:
  - `crates/core-model/src/mixer.rs` (`Strip.eq`, `Strip.colour`, `Swatch`, `chain`, `chain_mut`, `Mixer::slots`, `Mixer::remove`), `document.rs` (`SlotOwner::Eq`, `find_slot`, `plugin_slot`, `slot_mut`, `referenced_states`), `mixer_rules.rs` (four loops → `chain()`), `automation.rs` (`strip_of`: `SlotOwner::Eq`), `command/mixer.rs` (`SetEq`, `SetColour`, titles, refusals, `chain_owner` for `SetSidechain`, `insert_owner`'s EQ refusal), `file.rs` (`VERSION`, `migrate_v3_to_v4`, the module comment);
  - `crates/core-engine/src/graph/mixer.rs` (`mixer_spec`);
  - `crates/core-host/src/host.rs` (`loads_needed` and the sync's `wanted` → `slots()`).
- Regenerate: `fixtures/demo.vizdaw/project.json` (`cargo run -p vizdaw-play -- --write-demo fixtures/demo.vizdaw`), now version 4.
- Test: `crates/core-model/tests/{mixer_commands.rs,file.rs,mixer_document.rs,learn.rs}`, `crates/core-engine/tests/graph_mixer.rs`, `crates/core-host/tests/host_loads.rs`

**Interfaces:**
- Produces (Ruling 7):
  - `pub eq: Option<PluginSlot>` on `Strip`, where `Strip::new` sets `eq: None`;
  - `pub fn chain(&self) -> impl Iterator<Item = &PluginSlot> + '_` and `pub(crate) fn chain_mut(&mut self) -> impl Iterator<Item = &mut PluginSlot> + '_` on `Strip`;
  - `pub fn slots(&self) -> impl Iterator<Item = (StripId, &PluginSlot)> + '_` on `Mixer`;
  - `SlotOwner::Eq(StripId)`, and `fn chain_owner(d: &Document, slot: PluginInstanceId) -> Result<StripId>` in `command/mixer.rs`;
  - `MixerCmd::SetEq { strip: StripId, plugin: Option<PluginRef> }`, with titles "Load EQ" and "Remove EQ";
  - (C-1, Ruling 12) `pub enum Swatch { Red, Orange, Yellow, Green, Teal, Blue, Purple, Pink }` (serde `snake_case`), `pub colour: Option<Swatch>` on `Strip` (`Strip::new` sets `None`), and `MixerCmd::SetColour { strip: StripId, colour: Option<Swatch> }`, titled "Set Strip Colour";
  - `file::VERSION == 4` and `pub fn migrate_v3_to_v4(v: Value) -> Result<Value>`.

- [ ] **Step 1: Write the failing tests.**
  - `mixer_commands.rs`:
    - `the_eq_slot_loads_replaces_and_empties_as_undo_steps`:
      - on `base()`'s Built-in Synth strip (`StripId(1)`), `SetEq { plugin: Some(plug("eq-a")) }` makes `strip.eq` hold "eq-a", with `report.created == [Plugin(id1)]` and title "Load EQ";
      - `Some(plug("eq-b"))` gives a new `id2 != id1`, with `removed == [Plugin(id1)]`;
      - `None` gives `eq == None`, with title "Remove EQ";
      - `None` again is `Err(ModelError::Invalid(_))`, and the document is unchanged;
      - master takes an EQ too.
    - `the_eq_slot_stays_first_when_inserts_move`: an EQ and inserts a, b, c. `MoveInsert { slot: c, index: 0 }` makes the chain ids `[eq, c, a, b]` and leaves `strip.eq` unchanged. `AddInsert { index: Some(0) }` puts the new insert after the EQ.
    - `the_eq_slot_is_not_an_insert`: `MoveInsert` and `RemoveInsert` of the EQ's id are each `Err(Invalid)` with the message "the EQ slot is not an insert". `SetBypass`, `SetPluginParams` (one finite value) and `SetSidechain` (through `chain_owner`) on it succeed and change it. `automation::strip_of(doc, AutomationTarget::PluginParam(eq, p)) == Some(strip)`.
    - `every_slot_lookup_sees_the_eq`: `find_slot(eq) == Some(SlotOwner::Eq(strip))`, `plugin_slot(eq)` is the EQ, `mixer.slots()` yields it before that strip's inserts, and `referenced_states()` holds its state once `state` is set.
    - `removing_a_bus_clears_an_eq_sidechain_from_it`.
    - `a_strip_colour_is_set_saved_and_undone`: `SetColour { colour: Some(Swatch::Red) }` sets it, with title "Set Strip Colour"; undo gives `None`; `SetColour { colour: None }` clears it; an unknown strip is `Err(UnknownId)` and the document is unchanged.
    - `arb_cmd` gains `SetEq` and `SetColour`, so the existing `mixer_commands_undo_to_the_original` checks undo and `validate` with an EQ (no count change).
  - `file.rs`:
    - `an_eq_slot_round_trips_and_an_empty_one_is_not_written`: save and load equal; the JSON of a strip without an EQ has no `"eq"` key; with one it has.
    - `version_3_projects_migrate_to_version_4`: `small_doc()`'s JSON with `"version": 3` loads, every strip's `eq` and `colour` are `None`, and `to_json` says `"version": 4`.
    - `a_strip_colour_round_trips_and_none_is_not_written`: a strip with no colour has no `"colour"` key; `"colour": "red"` round-trips.
    - `unknown_fields_inside_persisted_enums_are_rejected` (existing) gains a case: a strip with `"colour": "neon"` is `Corrupt`.
    - Move the five version pins to `VERSION` (Ruling 8).
  - `graph_mixer.rs`: `the_eq_slot_runs_before_the_inserts`. A strip with an EQ (latency 64 via the latency closure) and two inserts: `spec.inserts`' instances are `[eq, i1, i2]`, and the PDC total includes 64.
  - `host_loads.rs`: `an_eq_slot_is_loaded_as_an_effect`. `loads_needed` on a document whose only plug-in is an EQ lists its ref once, as an effect (not an instrument).
- [ ] **Step 2: Run them and see them fail.** `cargo test -p core-model -p core-engine -p core-host`. Expected: compile errors (`eq`, `colour`, `Swatch`, `SetEq`, `SetColour` and `slots` are not defined).
- [ ] **Step 3: Implement** Rulings 7–9 and Ruling 12's model part. Check that `mixer_rules::check_strip_values` checks the EQ's values (it walks `chain()`). Move `file.rs`'s module comment (line 3) to version 4; leave `document.rs:169`, which is about `content_hash` (Ruling 8).
- [ ] **Step 4: Regenerate the demo fixture.** `cargo run -p vizdaw-play -- --write-demo fixtures/demo.vizdaw`; `git diff --stat fixtures/` shows only `project.json` (its `"version": 4`). Then `cargo test -p vizdaw-play` passes (`checked_in_demo_matches_a_fresh_build`), and `diff fixtures/demo.offline.txt` in CI's offline render step is unchanged.
- [ ] **Step 5: Run them and see them pass,** then the workspace, both clippies, fmt, and the rt-check gates (Global Constraints). Expected: Rust +11 (7a end + 11), all five rt_alloc tests pass, and the rt-check clippy is clean.
- [ ] **Step 6: Commit** the files above and `fixtures/demo.vizdaw/project.json`, by path. Message: "core: the EQ slot, a saved part of each strip that processes first, and an optional strip colour; format version 4 (plan 7b Task B1)", with the trailers.

---

### Task B2: The EQ slot and the strip colour over the FFI, kind colours and `state.get mixer`

**Files:**
- Modify:
  - `crates/core-ffi/src/mixer.rs` (`StripRow.eq`, `StripRow.kind_colour`, `StripRow.colour`, `StripColourRow`, `SwatchRow`, `strip_colour`, `slot_ids` → `slots()`), `session.rs` (`set_eq`, `set_strip_colour`, `slot_name`'s `SlotOwner::Eq` arm, the Kontrol page's first plug-in → `chain().next()`), `plugins.rs` (two loops → `slots()`), `host.rs` (`has inserts` → `slots()`), `surfaces.rs` (`chain`: the instrument, then `s.chain()`), `control.rs` (`StateTargetRow::Mixer`, `ControlResult::Mixer` and its JSON);
  - `crates/core-control/src/request.rs` (`StateTarget::Mixer`, `"mixer"`);
  - `app/Tests/VizDAWAppModelTests/FakeSession.swift` (`setEq`, `setStripColour`), `app/Sources/VizDAWAppModel/ControlCommands.swift` (`state(.mixer)`).
- Test: `crates/core-ffi/tests/{mixer.rs,control.rs}`, `crates/core-control/tests/commands.rs`

**Interfaces:**
- Consumes: B1.
- Produces:
  - `#[derive(uniffi::Enum)] pub enum StripColourRow { Channel, Instrument, AuxReturn, Bus, Master }`;
  - `#[derive(uniffi::Enum)] pub enum SwatchRow { Red, Orange, Yellow, Green, Teal, Blue, Purple, Pink }`, mapped one to one from `core_model::mixer::Swatch`;
  - `StripRow { …, eq: Option<SlotRow>, kind_colour: StripColourRow, colour: Option<SwatchRow> }`;
  - `pub fn strip_colour(m: &Mixer, s: &Strip) -> StripColourRow` (Ruling 10), which fills `kind_colour`;
  - `Session::set_eq(&self, strip: u64, plugin: Option<PluginRefRow>) -> Result<DocSnapshot, VizError>`, in `ffi_guard`;
  - `Session::set_strip_colour(&self, strip: u64, colour: Option<SwatchRow>) -> Result<DocSnapshot, VizError>`, in `ffi_guard` (C-1);
  - `ControlResult::Mixer { doc: String, mixer: MixerSnapshot }`, with Ruling 11's JSON;
  - Swift: `SessionProtocol.setEq(strip:plugin:)` and `setStripColour(strip:colour:)` (generated), `ControlCommands.state(.mixer, …)` answering `.mixer(doc:mixer:)` from `m.session.mixer()` (nil on a throw, as `.automation` does).

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
    - `hostile_strip_colour_calls_are_refused`: `set_strip_colour(999_999, Some(.red))` fails and leaves `snapshot().version` unchanged; `set_strip_colour(synth, Some(.blue))` then `undo` gives `colour == None`.
    - `strips_are_coloured_by_kind` (on `kind_colour`):
      - the demo: Built-in Synth is `.instrument`, master is `.master` and an audio path is `.channel`;
      - `add_bus` gives `.bus` (no feed);
      - a send from the synth to it gives `.auxReturn`;
      - then setting another strip's output to it gives `.bus`.
  - `core-ffi/tests/control.rs`'s `state_get_mixer_answers_json`: `ControlResult::Mixer`'s JSON for the demo has a `strips` array whose entries hold exactly Ruling 11's keys, with `eq` null, `kind_colour` `"instrument"` and `colour` null for the synth.
  - `core-control/tests/commands.rs`'s `mixer_state_target_parses`: `state.get {target: "mixer"}` parses to `StateTarget::Mixer`, and `{target: "mixers"}` is `invalid_params`.
- [ ] **Step 2: Run them and see them fail.**
- [ ] **Step 3: Implement.**
  - `strip_row` fills `eq` with `slot_row(eq, false, host)`, `kind_colour` with `strip_colour`, and `colour` from `Strip.colour`.
  - The JSON goes in `control.rs`, beside `Layout`'s. Its keys match Ruling 11.
  - Then the Swift side: `scripts/build-ffi.sh`, `FakeSession.setEq` (records the call, replaces or empties a fake EQ, and throws `VizError.Invalid` on an empty clear), `FakeSession.setStripColour` (records the call and sets the fake strip's `colour`), and `ControlCommands.state`'s new `.mixer` case.
- [ ] **Step 4: Run the gates.** Rust: +7 (7a end + 18), Linux clippy clean. Swift: builds and runs the 7a-end count unchanged (`StripRow`'s new fields are in every literal `FakeSession` makes; fix those literals).
- [ ] **Step 5: Commit** the files above. Message: "ffi: the EQ slot (set_eq, StripRow.eq), the strip colour (set_strip_colour), kind colours, state.get mixer (plan 7b Task B2)", with the trailers.

---

### Task B3: Pin `symbiotic-design` 0.2.0, the icons, the no-brand build and the licence permission

**Files:**
- Modify: `app/Package.swift` (Ruling 6), `.github/workflows/ci.yml` (the no-brand step), `.gitignore` (`.build-nobrand/`), `LICENSE` (the copyright line and the §7 additional permission, appended), `scripts/make-app.sh` (copies `LICENSE` into `Contents/Resources/`), `README.md` (one line in "License", one sentence in "Building from source"), `app/Sources/VizDAW/VizDAWApp.swift` (the brand registration, the Settings tab icons), and every `VizDAW` file with `systemImage:`, `Image(systemName:)` or `NSImage(systemSymbolName:)` (`DetailPane`, `StripInputView`, `SequenceSidebar`, `ArrangementPane`, `DrumGridPane`, `RelinkSheet`, `MediaPoolWindow`, `DocumentView` (7a's pane toggles: `paneToggle(_:_:symbol:key:)` takes an `AppIcon` parameter in place of `symbol`), `ProjectBanners`, `ModifyPanel`, `MixerView`, `LearnMapsSheet`, `RecordControls`, `BounceSheet`, `PianoRollPane`, `DocumentShell` (its two "Mixer Unavailable" views), `PaneViews` (7a Task 8's `ContentUnavailableView`), `EditorWindows`, and any other the scan names)
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

- [ ] **Step 1: Pin the package.** Edit `app/Package.swift` per Ruling 6, then run `scripts/swift.sh build`: SwiftPM resolves `0.2.0` and writes `app/Package.resolved`. Check that its pin's revision equals the pushed tag's commit (`git ls-remote https://github.com/SymbioticAudio/symbiotic-design refs/tags/0.2.0^{}`), and that `app/Package.resolved` has `"version" : 3` (the schema Swift 6.1 reads). If the local toolchain wrote a newer schema, stop and report.
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

  `SourceScan` is an `internal` helper in the test target (`SourceScan.swift`, made here and copied by B5), so `AppIconTests.swift` can use it. It walks `app/Sources/<dir>/**/*.swift` from `URL(fileURLWithPath: #filePath)` and returns `"file:line: text"` for each match of a regular expression.
- [ ] **Step 3: Run them and see them fail** (`AppIcon` is not defined).
- [ ] **Step 4: Replace every SF Symbol** with its `AppIcon`:
  - `Label(t, systemImage: s)` becomes `Label { Text(t) } icon: { Icon(AppIcon.x.role, size: 16) }`;
  - `Image(systemName:)` becomes `Icon(…, size:)`;
  - `ContentUnavailableView(title, systemImage:, description:)` becomes its label and description form with `Icon(…, size: 32)`;
  - `NSImage(systemSymbolName:)` becomes `NSImage.icon(…, size:, theme:)`.
  - Register the brand in `VizDAWApp.init`, first. Add the CI step. Add `.build-nobrand/` to `.gitignore`.
- [ ] **Step 4b: The licence permission** (owner, 2026-11-03, binding; Ruling 6).
  - Append to the end of `LICENSE`, after the GPL text and separated from it by a blank line and a rule, Ruling 6's copyright line and its two permission paragraphs, exactly.
  - In `scripts/make-app.sh`, copy `LICENSE` to `$app/Contents/Resources/LICENSE` (the script already makes `Contents/Resources`). Check after `scripts/make-app.sh` that `dist/VizDAW.app/Contents/Resources/LICENSE` equals `LICENSE`.
  - In `README.md`'s "License" section, after "GPL-3.0 — see [LICENSE](LICENSE).", add Ruling 6's one line: the artwork stays all rights reserved and is not covered by the GPL, and forks build without it (`VIZDAW_NO_BRAND=1`).
  - In `README.md`'s "Building from source", after the code block, add (M-10): "The first build fetches symbiotic-design 0.2.0 from GitHub; `VIZDAW_NO_BRAND=1` builds without SymbioticAudio's branded icons."
  - Check: `git diff LICENSE` adds lines only at the end and changes no GPL line.
  - After G2, once this `LICENSE` is on GitHub (the owner pushes VizDAW), the controller checks that GitHub still detects GPL-3.0. If not, keep the text and accept "Other".
- [ ] **Step 5: Run the gates:** Swift 7a end + 3; then `VIZDAW_NO_BRAND=1 scripts/swift.sh build --scratch-path .build-nobrand`, after which `find .build-nobrand -name 'SymbioticBrand*.o'` prints nothing; then `scripts/make-app.sh`; then the e2e, all passing. `git status` does not show `.build-nobrand`.
- [ ] **Step 6: Commit** `app/Package.swift`, `app/Package.resolved`, `.github/workflows/ci.yml`, `.gitignore`, `LICENSE`, `scripts/make-app.sh`, `README.md`, `AppIcon.swift`, `AppIconTests.swift`, `SourceScan.swift`, and the `VizDAW` files changed, by path. Message: "app: adopt symbiotic-design 0.2.0: icons from SymbioticIcons, the brand registered under a GPL section 7 permission, a build without it (plan 7b Task B3)", with the trailers.

---

### Task B4: Themes, Settings › Appearance, live switching and `ui.theme`

**Files:**
- Modify:
  - `crates/core-control/src/registry.rs` (`ui.theme`, Ruling 11), `crates/core-control/tests/commands.rs`;
  - `crates/core-ffi/src/control.rs` (`ControlResult::Windows.theme` and its JSON), `crates/core-ffi/tests/control.rs` (the windows JSON);
  - `app/Sources/VizDAWAppModel/ControlCommands.swift` (`handled` adds `AppearanceCommands.ids`, the dispatch, and `ControlHost.appearance`);
  - `app/Sources/VizDAW/VizDAWApp.swift` (`appearancePreferences`, `ThemedRoot` on every scene, the Appearance tab), `ControlChannel.swift` (`.windows(… theme: appearancePreferences.theme.id)`, and `AppControlHost.appearance`), `PluginWindows.swift`, `EditorWindows.swift`, `ModifyPanel.swift` (`ThemedRoot` on their hosting views, `ThemeWindows.style`);
  - every Editors host (`ArrangementHost`, `PianoRollHost`, `ListHost`, `DrumGridHost`, `StripChartHost`, `TransportFieldsHost`), in their files `app/Sources/VizDAW/{ArrangementPane,PianoRollPane,ListEditorPane,DrumGridPane,StripChartPane}.swift` (`TransportFieldsHost` is in `ArrangementPane.swift`): `updateNSView` passes `context.environment.symbioticTheme` to its views' `theme`. Use the tree's real host and file names and say so if they differ;
  - the test target's fake `ControlHost`, `FakeControlHost` in `app/Tests/VizDAWAppModelTests/ControlCommandsTests.swift` (gains `appearance`).
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
    - `public var onChange: ((Theme) -> Void)?`, typed as `KeyRecorderView.onKey` is, so a test closure may mutate a captured `var` under Swift 6 (a `@MainActor` closure type may be inferred `@Sendable`, which rejects that);
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
        // Per field: a readable colourBars is kept; garbage gives the default.
        let kept = MemoryDefaults()
        kept.set(Data(#"{"theme":"neon","colourBars":false}"#.utf8), forKey: AppearancePreferences.key)
        #expect(!AppearancePreferences(defaults: kept).colourBars)
        let junk = MemoryDefaults()
        junk.set(Data([0xFF, 0x00]), forKey: AppearancePreferences.key)
        #expect(AppearancePreferences(defaults: junk).colourBars)
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
  - `ThemedRoot` goes on `DocumentWindow`, `MixerWindow`, `MediaPoolWindow`, `PluginManagerView`, the Settings `TabView` and each `NSHostingView` root. Call `ThemeWindows.apply(appearancePreferences.theme)` first thing in `AppDelegate.applicationDidFinishLaunching` (`NSApp` may still be nil in `VizDAWApp.init`; today's code touches it only in closures and the `AppDelegate`). In `VizDAWApp.init`, set `appearancePreferences.onChange = { ThemeWindows.apply($0) }`. `apply` sets `NSApp.appearance` first (Ruling 12), so windows, menus, alerts and panels made later follow the theme.
  - `VizDAWEditors`' views gain `public var theme: Theme = .classicLight { didSet { needsDisplay = true } }`, used by B5.
- [ ] **Step 5: Run the gates:** Rust both clippies and the workspace (7a end + 20, registry 154); Swift 7a end + 10; the no-brand build; `scripts/make-app.sh`; the e2e (all pass, since `state.get windows` has only gained a key).
- [ ] **Step 6: Commit** the files above. Message: "app: themes: Settings › Appearance, Classic Light by default, live in every window; ui.theme and state.get windows' theme (plan 7b Task B4)", with the trailers.

---

### Task B5: Every colour from the theme

**Files:**
- Create: `app/Sources/VizDAWEditors/EditorTheme.swift`, `app/Tests/VizDAWEditorsTests/ThemeScanTests.swift` (with its own copy of `SourceScan`, because the two test targets share no files)
- Modify: every file the scan names in `app/Sources/VizDAWEditors` (`LaneHeaderView`, `ArrangementView`, `DrumGridView`, `DrumRowHeaderView`, `RegionDrawing`, `RegionStripView`, `TransportFieldsView`, `ExpressionPaneView`, `EditorCanvas`, `StripChartView`, `PianoKeyboardView`, `PianoRollView` (its `PianoRollPalette` takes `swatch.*`, in order), `ControlUI` if named) and `app/Sources/VizDAW` (`LearnMapsSheet`, `ArrangementPane`, `KeysSettingsView`, `ModifyPanel`, `DocumentShell`, `SurfacesSettingsView`, `PluginManagerView`, `MixerView`, `RecordControls`, and any other the scan names); `app/Sources/VizDAW/Theming.swift` (the SwiftUI-side bridge for the mixer-independent views)

**Interfaces:**
- Consumes: B4's `theme` on the Editors views, and `ThemedRoot`.
- Produces: `public extension Theme` in `EditorTheme.swift`, with one `NSColor` property per role of Ruling 13's table:
  - `canvas`, `rulerBackground`, `barLine`, `beatLine`, `separatorLine`, `label`, `secondaryLabel`;
  - `selectionFill`, `selectionText`;
  - `regionFill`, `regionLine`, `regionSelected`, `valueBar`;
  - `playhead`, `armColour`, `muteColour`, `soloColour`, `warningColour`, `errorColour`, `offlineColour`;
  - `whiteKey`, `blackKey`;
  - `func laneColour(_ index: Int) -> NSColor`: `swatch.*` in the order `red` … `pink`, by `index % 8` (replaces `PianoRollPalette`'s system colours).

- [ ] **Step 1: Write the failing tests** in `ThemeScanTests.swift`:
  - `noColourLiteralsOutsideTheThemeBridges`: Ruling 13's pattern over `VizDAW` and `VizDAWEditors`, minus the two bridge files. Expected: no hits. The message lists `file:line`.
  - `noCornerRadiusOrFontSizeLiterals`: `cornerRadius:\s*[0-9]`, `\.system\(size:\s*[0-9]`, `systemFont\(ofSize:` and `SystemFontSize`. Expected: no hits.
  - `theScanSeesEachForbiddenForm`: both patterns over a fixed list of 12 known-bad lines, one per form (`NSColor.red`, `Color.blue`, `Color(red:`, `NSColor(calibrated`, `Color.accentColor`, `return .red`, `case "green": .green`, `[.systemBlue, .systemOrange]`, `.labelColor`, `cornerRadius: 4`, `.system(size: 11`, `.systemFont(ofSize: NSFont.smallSystemFontSize)`). Expected: each of the 12 lines matches at least one pattern (two lines match twice), and a clean line (`theme.state.error.color`) matches none.
  - `anEditorRedrawsWhenItsThemeChanges`: an `EditorCanvas` 100×100 (`wantsLayer = true`) with `needsDisplay = false`, then `theme = .console`. Then `needsDisplay` is true, and `NSColor(cgColor: canvas.layer!.backgroundColor!)!.usingColorSpace(.sRGB) == Theme.console.canvas.usingColorSpace(.sRGB)` (compared in sRGB, because bridged colour spaces make a raw `CGColor` comparison fragile).
- [ ] **Step 2: Run them and see them fail.** The scan lists today's sites: about 250, which is this task's worklist.
- [ ] **Step 3: Sweep.**
  - Each AppKit site uses `theme.<role>`; each layer colour is set in `theme`'s `didSet` as well as at set-up. `EditorCanvas` sets `wantsLayer = true`.
  - Context-typed colours are swept too, by Ruling 13's table:
    - `SlotView.color` (`MixerView.swift`), all five cases: faulted `state.error`, placeholder `state.warning`, loading `state.offline`, live `state.play`, live and bypassed `text.disabled`;
    - `MeterView`'s well (`.black.opacity`) `meter.background`, and its peak (`.red`/`.green`) `meter.clip` at or above 1.0, else `meter.low`, until B6's `StripPalette.meter`;
    - the M and S toggles' tints (`.orange`, `.yellow`) `state.mute` and `state.solo`;
    - `automationColor`'s `default: .gray` `state.offline`;
    - the lane colours take `laneColour(_:)`.
  - Each SwiftUI site uses `@Environment(\.symbioticTheme)` and a token's `.color`, or a hierarchical `.primary`/`.secondary`/`.tertiary`, which `ThemedRoot` maps.
  - `automationColor(_:)` (in `MixerView.swift`) is deleted; B6's `StripPalette.automation` replaces it, and B7 uses that. Until then it reads `theme.state.*` through the same mapping.
  - `cornerRadius` uses `theme.radius.s`/`.m`/`.l`, and fonts use `theme.type.*.font`.
- [ ] **Step 4: Run the gates:** Swift 7a end + 14, no-brand build, `scripts/make-app.sh`, e2e.
- [ ] **Step 5: Commit** the new files and every swept file, by path. Message: "app: every colour, radius and font size from the theme; editor colour roles (plan 7b Task B5)", with the trailers.

---

### Task B6: The strip's models: typed values, layout, palette, EQ, colour and arm actions

**Files:**
- Create: `app/Sources/VizDAWAppModel/{StripEntry.swift,StripLayout.swift,StripPalette.swift}`, `app/Tests/VizDAWAppModelTests/{StripEntryTests.swift,StripLayoutTests.swift,MixerStripTests.swift}`
- Modify:
  - `app/Sources/VizDAWAppModel/MixerModel.swift` (`BrowserTarget.eq`, `pick`, `clearEq`, `setColour`, `typeLevel`, `typePan`, `typeSend`, `canArm`, `toggleArm`, `cycleMonitor`; `heldSlots(of:)` reads `strip.slots`);
  - `PluginWindowModel.swift` (`refresh` finds its slot in `s.slots`, line 78 today) and `StripChartModel.swift` (the slot loop, line 35 today, walks `s.slots`). Today all three read `[instrument] + inserts`, so they would not see the EQ.

**Interfaces:**
- Consumes: B2 (`StripRow.eq`, `.kindColour`, `.colour`, `SwatchRow`, `SessionProtocol.setEq` and `setStripColour`), B4 (`Theme` in `VizDAWAppModel`), and `MixerModel.click`, `inputLane(for:)`, `LaneInputs.cycleMonitor` and `EditorModel.edit(.lane(edit: .setFlags(…)))` (as `SurfaceRelay.arm` uses it).
- Produces (all `public`):
  - `extension StripRow { public var slots: [SlotRow] }`: the instrument (when there is one), then `eq` (when set), then the inserts, in that order. Every Swift reader of a strip's slots uses it;
  - `enum StripEntry` with:
    - `static func level(_ text: String) -> Double?` (dB, where −144 is −∞) and `static func pan(_ text: String) -> Double?` (−1…1), per Ruling 15;
    - `static func levelText(_ db: Double) -> String` (`"−4.2 dB"`, `"+3.0 dB"`, `"0.0 dB"`, `"−∞ dB"`, using U+2212) and `static func panText(_ pan: Double) -> String` (`"L 24"`, `"C"`, `"R 10"`);
    - `static let levelHint = "Type a level in dB, like −4.2 or −inf."`, `static let panHint = "Type a pan like L 24, C or R 10."` and `static let touchedHint = "Release the control first."`.
  - `struct ScaleMark: Equatable, Sendable { public var position: Double; public var label: String }` and `struct StripSplit: Equatable { public var throwLength: CGFloat; public var scrolls: Bool }`.
  - `enum StripLayout` with:
    - `static let width: CGFloat = 120`, `minimumThrow: CGFloat = 160` and `maximumThrow: CGFloat = 480`;
    - `static let lowerFixed: CGFloat` (the level box, M S R I, the automation menu and the padding; the implementer measures it once and states it);
    - `static func split(height: CGFloat, upper: CGFloat) -> StripSplit`. Its `throwLength` is `clamp(height − upper − lowerFixed, 160, 480)`, and `scrolls` is `height < upper + lowerFixed + 160`;
    - `static let faderMarks: [ScaleMark]`, for +12, +6, 0, −6, −12, −24, −36 and −48 at `MixerMath.faderPosition(db)`, then "−∞" at 0, which stands for −60 and below (`faderPosition(-60) == 0`, so a separate −60 mark would sit on −∞). Labels are `"+12"`, `"0"`, `"−6"`, `"−∞"`.
  - `enum KnobFunction { case gain, pan, send }` and `enum StripPalette` with:
    - `static func cap(_ k: StripColourRow, _ t: Theme) -> ThemeColor` (`t.strip.channel/instrument/auxReturn/bus/master`);
    - `static func swatch(_ c: SwatchRow, _ t: Theme) -> ThemeColor` (`t.swatch.red` … `pink`);
    - `static func bar(_ s: StripRow, _ t: Theme) -> ThemeColor`: `swatch(s.colour, t)` when `s.colour` is set, otherwise `cap(s.kindColour, t)` (C-1);
    - `static func knob(_ f: KnobFunction, _ t: Theme) -> ThemeColor` (`t.control.gain/pan/send`);
    - `static func automation(_ m: AutomationModeRow, _ t: Theme) -> ThemeColor`. It maps `AutomationModes.colorName`: green → `state.play`, yellow → `state.solo`, orange → `state.warning`, red → `state.record`, purple → `control.send`, and anything else → `state.offline`;
    - `static func meter(_ peak: Float, _ t: Theme) -> ThemeColor`: `meter.low` below −18 dBFS, `mid` to −6, `high` below 0, `clip` at 0 and above.
  - On `MixerModel`:
    - `BrowserTarget.eq(strip: UInt64)` (`wantsInstrument` false);
    - `func clearEq(_ s: StripRow)`;
    - `func setColour(_ s: StripRow, _ c: SwatchRow?)`: one undo step; a throw raises the "Strip Colour Failed" alert;
    - `func typeLevel(_ s: StripRow, _ text: String) -> Bool`, `func typePan(_ s: StripRow, _ text: String) -> Bool` and `func typeSend(_ s: StripRow, _ index: Int, _ text: String) -> Bool`. Each returns false and sets `StripEntry.touchedHint` as the status when its target is touched;
    - `func canArm(_ s: StripRow) -> Bool`, `func toggleArm(_ s: StripRow)` and `func cycleMonitor(_ s: StripRow)`.

- [ ] **Step 1: Write the failing tests.**
  - `StripEntryTests.swift`:
    - `levelsAcceptDecibelsWithOrWithoutTheUnit`: `"-4.2"`, `"−4.2 dB"`, `"-4.2db"` and `" -4.2 DB "` all give −4.2; `"+3"` gives 3; `"0"` gives 0; `".5"` gives 0.5; `"-4.25"` gives −4.3.
    - `levelsClampAndReadMinusInfinity`: `"40"` gives 12; `"-200"`, `"-144"`, `"-inf"`, `"−∞"` and `"-∞ dB"` each give −144; `levelText(-144) == "−∞ dB"`. Negative zero: `"-0.04"` and `"-0"` give 0 (not −0), and `levelText(-0.0) == "0.0 dB"`.
    - `hostileLevelsAreRefused`: each of these gives nil:
      - `""`, `"   "`, `"abc"`, `"4.2.1"`, `"--3"`, `"+-3"`;
      - `"nan"`, `"NaN"`, `"inf"`, `"+inf"`, `"1e3"`, `"1e309"`, `"1,5"`;
      - `"-4.2 dBFS"`, `"-4.2 dB dB"`, `"4\u{0}"`;
      - `String(repeating: "1", count: 33)`, and 300 × `"9"`.
    - `pansAcceptLeftCentreRight`:
      - `"C"`, `"c"`, `"0"` and `"L0"` give 0;
      - `"L24"`, `"L 24"`, `"l 24"`, `"-24"` and `"−24"` (U+2212) give −0.24;
      - `"R10"`, `"r 10"`, `"10"` and `"+10"` give 0.10;
      - `"L150"` gives −1; `"L 24.6"` gives −0.25; `"R 0.4"` gives 0.
    - `hostilePansAreRefused`: each of these gives nil: `""`, `"LR"`, `"L"`, `"L-5"`, `"L R"`, `"LL24"`, `"24L"`, `"C 5"`, `"nan"`, `"L nan"`, `"∞"`, `"L1e2"`, `"R\u{7}"`, `"L ２４"` (full-width digits), `"R ٣"` (Arabic-Indic) and 33 characters.
    - `typedValuesReadBackAsTheyShow`: for −60, −4.2, 0 and 12, `level(levelText(db)) == db`; for −1, −0.24, 0 and 0.1, `pan(panText(p)) == p`.
  - `StripLayoutTests.swift`:
    - `theFaderGetsALongThrowWhenThereIsRoom`: `split(height: 900, upper: 300).throwLength == min(480, 900 - 300 - lowerFixed)` and `!scrolls`.
    - `aShortPaneScrollsTheStripAndKeepsTheMinimumThrow`: `split(height: 260, upper: 300)` is `StripSplit(throwLength: 160, scrolls: true)`, and so is `split(height: 120, upper: 0)`.
    - `theDbScaleMarksSitWhereTheFaderPutsThem`: the labels are `+12, +6, 0, −6, −12, −24, −36, −48, −∞`; the positions are strictly decreasing from +12 down to −∞; the 0 dB mark is at `60.0 / 72`; the −∞ mark is at 0, and no other mark is.
  - `MixerStripTests.swift` (`FakeSession`, as `MixerModelTests` builds a model):
    - `faderCapsFollowTheStripKindInEveryTheme`: for each `Theme.all` and each `StripColourRow`, `cap` is the matching `strip.*` token, and the five caps are distinct in every theme. `bar(s, t)` is `t.swatch.blue` for a strip with `colour == .blue` (its cap still by kind), and `cap(s.kindColour, t)` for a strip with no colour.
    - `knobsAndAutomationUseTheirTokens`: `knob(.pan, t) == t.control.pan`; `automation(.write, t) == t.state.record`; `meter(1.0, t) == t.meter.clip`; `meter(0.01, t) == t.meter.low`.
    - `typingALevelIsOneClickAndARefusalChangesNothing`:
      - `typeLevel(synth, "-4.2")` is true, and the fake records exactly touch, value and release of `.stripFader(strip:)` at `AutomationLaw.faderNorm(-4.2)`;
      - `typeLevel(synth, "abc")` is false, the fake records nothing new, and `document.editor.status == StripEntry.levelHint`;
      - with the fader touched (`touch(.stripFader(strip:))`), `typeLevel(synth, "-6")` is false, nothing new is sent, and the status is `StripEntry.touchedHint`.
    - `typingPanAndSendLevelsUsesTheirTargets`: `typePan(s, "L 24")` sends `.stripPan` at `(−0.24 + 1) / 2`. `typeSend(s, 0, "-6")` sends `.sendLevel(strip:send: 0)` at `faderNorm(-6)`. `typeSend(s, 5, "-6")` on a strip with one send is false and sends nothing.
    - `theEqSlotLoadsBypassesAndEmpties`: `openBrowser(.eq(strip: s.id), in: .window)` then `pick(ref)` calls `setEq(strip: s.id, plugin: ref)` once; `toggleBypass(eqSlot)` calls `setBypass`; `clearEq(s)` calls `setEq(…, plugin: nil)`. A thrown `setEq` raises the "Load EQ Failed" alert.
    - `aStripColourIsOneStep`: `setColour(synth, .red)` calls `setStripColour(strip: synth.id, colour: .red)` once; `setColour(synth, nil)` calls it with nil; a thrown call raises the "Strip Colour Failed" alert.
    - `theEqSlotIsSeenByEveryStripSlotReader`: on a strip with an EQ (and an instrument and one insert), `slots` is `[instrument, eq, insert]`;
      - `PluginWindowModel(document: d, slot: eq.id).refresh()` gives a non-nil `row` and the strip's name as `stripName`;
      - with the EQ held, `heldSlots(of:)` includes it;
      - the Strip Chart's menus list the EQ's automatable parameters.
    - `recordArmAndMonitorNeedAnInputLane`: an audio-path strip with an input lane can arm (`toggleArm` sends `setFlags(arm: true)`), and its monitor cycles. An instrument strip and master give `canArm == false`, and `toggleArm` sends nothing (Q3, ruled: R/I stay disabled on instrument strips).
- [ ] **Step 2: Run them and see them fail.**
- [ ] **Step 3: Implement** to Rulings 14 and 15, using the signatures above. `StripEntry` uses `NumberEntry`'s rules for length and control characters, but has its own copy here, because `VizDAWAppModel` does not import `SymbioticControls`.
- [ ] **Step 4: Run the gates:** Swift 7a end + 31, plus the no-brand build.
- [ ] **Step 5: Commit** the six files, `MixerModel.swift`, `PluginWindowModel.swift` and `StripChartModel.swift`. Message: "app model: the strip's typed values, layout and colours; the EQ slot, strip colour and arm actions (plan 7b Task B6)", with the trailers.

---

### Task B7: The channel strip

**Files:**
- Create: `app/Sources/VizDAW/ChannelStrip.swift`
- Modify:
  - `app/Sources/VizDAW/MixerView.swift`: `MixerView(model:placement:)`'s strips row uses `ChannelStrip`, and its scroll view is two-axis when `StripLayout.split` scrolls. `StripView`, `SendView`, `FaderView` and `MeterView` are removed. `SlotView` and `LearnItem` stay, restyled, and `touchRing` reads `theme.surface.focusRing`;
  - `app/Sources/VizDAW/PaneViews.swift`: 7a's `LeftStripPane` shows `ChannelStrip`. Its `ScrollView(.vertical)` around the strip becomes `GeometryReader { ChannelStrip(model:strip:height: $0.size.height) }`, and `ChannelStrip` scrolls itself by `StripLayout.split(...).scrolls` (Ruling 14). Anchor on `LeftStripPane` and its `ScrollView`, not on line numbers: 7a Task 9 may still touch this file.

**Interfaces:**
- Consumes: B3 (icons), B4 (`appearancePreferences.colourBars`, `\.symbioticTheme`), B6 (everything it produces, including `StripPalette.bar` and `MixerModel.setColour`), A4 (`Knob`, `Fader`, `NumberBox` with `onRefuse`, the palettes with `KnobPalette.finish`, `Relief`, `ControlMetrics.hitSize`), and 7a's `\.mixerPlacement`, `openBrowser(_:in:)`, `showsBrowser(in:)`, `beginRename(_:in:)`, `isRenaming(_:in:)` and `\.openPluginWindow`.
- Produces: `struct ChannelStrip: View { @Bindable var model: MixerModel; let strip: StripRow; let height: CGFloat }`, with `EqSlotView`, `SendKnobRow`, `StripFaderSection` and `StripButtons` in the same file.

- [ ] **Step 1: Build `ChannelStrip`** to Ruling 14, in that order.
  - Each knob, fader and number box maps `onBegin`, `onChange` and `onEnd` to `model.touch`, `move` and `release`, with today's targets. Each `onReset` maps to `model.click`: 0 dB, or C. Because a bare click sends no gesture (Ruling 4), the double-click's `click` is never dropped by a touch of the same target.
  - Each number box's `commit` calls `typeLevel`, `typePan` or `typeSend`; its `onRefuse` sets the matching hint (`levelHint` or `panHint`) as the status.
  - The colour bar draws `StripPalette.bar(strip, theme)` when `colourBars` is on. The name's context menu gains "Colour ▸" with None and the eight swatches, each item showing its token colour, calling `model.setColour`.
  - The send row puts the target's name on its own line above the knob and its box (Ruling 14). Every control's hit frame is `ControlMetrics.hitSize(_:)`.
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
  - selection by click and ⌘-click;
  - the new Colour ▸ menu (C-1).

  List them in the report, each with where it now lives.
- [ ] **Step 3: Run the gates:** Swift build and test (7a end + 31, no new warnings), the no-brand build, `scripts/make-app.sh`, and the existing e2e. `touch_writes_a_fader_over_the_channel`, `drives_a_fader_and_a_button_from_an_mcu` and `drives_the_mixer_from_a_kontrol_mk3` still pass: they drive the mixer through the channel, not the old sliders. A failure is a strip fault to fix here.
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
  3b. A new demo opens in the Arrange view with nothing selected, so the left pane shows `StripFocus.nothingMessage` and no strip. `command.run ui.view_mix`, then `wait_for_state(&mut app, &window, "layout", |l| l["main"] == "mix")`.
  4. `state.get mixer`:
     - every strip has `kind_colour` and `colour`; the Built-in Synth's `kind_colour` is `"instrument"`, master's is `"master"`, and every `colour` is null;
     - every `eq` is null;
     - the document window's `ui.tree` has a node labelled "EQ of Built-in Synth" whose value is "empty".
  5. Send one `ui.key {window, element: <the "Level of Built-in Synth" node's element>, text: "-4.2\n"}`, as step 6 does: the app clicks the element first, so the box starts editing and takes keyboard focus before the keys arrive (a separate `ui.click` then `ui.key` can race the focus, and unfocused digits go to the key bindings). Wait until `state.get mixer` shows that strip's `(gain_db - (-4.2)).abs() < 1e-6`, then `wait_for_undo(…, "Set Fader")`.
  6. `ui.click` "Pan of Built-in Synth" and `ui.key {window, text: "LR\n"}`. Then `state.get editor`'s `status == "Type a pan like L 24, C or R 10."` and the pan is still 0.0. The refusal ended editing, so refocus: `ui.key {window, element: <the "Pan of Built-in Synth" node's element>, text: "L 24\n"}` (the tree's `ui.key` takes `element`, not `focus`). Wait until `(pan - (-0.24)).abs() < 1e-6`.
  7. `command.run window.mixer`: a `kind == "mixer"` window appears, and its `ui.tree` has "Fader of Built-in Synth".
  8. `file.save_as` to `<home>/strip.vizdaw`, `file.close`, then `app.open` it. `state.get mixer` shows `(gain_db + 4.2).abs() < 1e-6` and `(pan + 0.24).abs() < 1e-6`, and `state.get windows`' theme is still `"console"` (a preference, not project state).
  9. `app.quit()`.
- [ ] **Step 2: Run it and see it pass** (it was RED until B7; record the run against B7's build in the report), then all 16 e2e tests.
- [ ] **Step 3: Write README steps 207–216** in `app/README.md`, under a new section after 7a's "Window, main views and panes (plan 7a)". In step 214, `<7a-end-sha>` is the SHA recorded in `progress.md` at 7a's end. Each step is an action, then a colon, then the expected result, as the README's earlier steps are.

```markdown
### Themes and the channel strip (plan 7b)

Checked by hand; the rules are tested (`AppearanceTests`, `AppIconTests`,
`ThemeScanTests`, `StripEntryTests`, `StripLayoutTests`, `MixerStripTests`,
the core's EQ slot and strip colour tests) and the e2e
`themes_and_the_strip_over_the_channel`.

207. Open Settings › Appearance and, if another theme is chosen, choose
     Classic Light; then File > Create Demo Project: light grey panels,
     white editing areas, dark text and one blue selection colour.
     Settings › Appearance lists Classic Light, Studio Dark, Flat, Console
     and Symbiotic, with Classic Light chosen.
208. With the demo window, the mixer window (⌥⌘M), a torn-off piano roll
     (⌥⌘1), Modify › Transpose and Settings open, choose each theme in
     turn: every window changes at once, the arrangement and piano roll
     canvases included; no window keeps the old colours. Then open
     Window > Mixer and File > New: both come up in the chosen theme, and
     a strip's context menu and the File menu follow it too. Turn macOS
     Dark Mode on: Classic Light stays light and Studio Dark stays dark.
     Quit and start VizDAW again: the last theme is still chosen.
209. Press ⌃2: each strip shows, from the top, a colour bar and its name;
     the input (audio strips); "EQ ＋"; the inserts and "+"; its sends,
     each a name over a knob and its level; "→ Master"; a pan knob
     reading "C"; a large fader cap beside a dB scale and a meter;
     "0.0 dB"; M S R I; the automation mode. The Built-in Synth's cap, an
     audio strip's cap and master's cap have three different colours. Add
     a bus and give the synth a send to it: the bus's cap takes the
     aux-return colour. Route an audio strip's output to the bus: the
     bus's cap changes to the bus colour.
210. Right-click the Built-in Synth's name and choose Colour ▸ Blue: its
     colour bar turns blue in the Mix view, the bottom pane's mixer, the
     mixer window and (select a lane that plays through the Built-in
     Synth) the left strip, and its fader cap keeps the
     instrument colour. Undo: the bar shows the instrument colour again.
     Redo, save, close and reopen: the bar is still blue. Then Settings ›
     Appearance › Show channel colour bars off: every bar goes, in all
     four places. Turn it back on: they return.
211. Click the Built-in Synth's level box, type `-4.2` and press Return:
     the fader moves down and the box reads "−4.2 dB"; Edit shows "Undo
     Set Fader", and one Undo puts it back to "0.0 dB". Type `abc` and
     press Return: the box still reads "0.0 dB" and the status line says
     "Type a level in dB, like −4.2 or −inf.". Type `-inf`: "−∞ dB" and
     the cap at the bottom. Type `40`: "+12.0 dB". Type `-0.04`: "0.0 dB".
     Paste a long line of 9s and press Return: refused, the box unchanged.
     Press Escape while typing: nothing changes.
212. In its pan box type `L 24`: the knob turns left and the box reads
     "L 24". Then `r10`: "R 10". Then `c`: "C". Then `LR`: refused, with
     "Type a pan like L 24, C or R 10." in the status line. Drag the pan
     knob up and down on the trackpad: it follows without jumping; press
     ⌥ half-way through a drag: the knob does not jump, then moves a
     tenth as far. Click the knob once without moving: nothing changes.
     Double-click it: it returns to C. Focus it and press the arrow keys:
     it steps. Do the same on a send knob and its level box.
213. Drag the fader cap from the bottom to the top on the trackpad: the
     cap is easy to grab, the throw is long, and the "0" mark lines up
     with 0.0 dB; the scale reads +12 down to −48, then "−∞" at the
     bottom (−60 and below). Drag the bottom pane's divider down to its
     smallest size with the bottom pane on Mixer: the strips scroll up and
     down, the fader keeps a usable throw, and M S R I stay reachable.
     Show the left strip on a 700×450 window: it scrolls the same way.
214. On an audio strip click "EQ ＋" and choose any effect plug-in: the
     slot shows its name. Click it: the plug-in's window opens. Turn its
     bypass on and off from the slot: the slot shows each state. Add two
     inserts and drag the second above the first: the EQ stays at the
     top. Try to drag the EQ among the inserts: it does not move. Save,
     close and reopen: the EQ is still there. Remove EQ from its menu:
     "EQ ＋" again; Undo brings it back with its settings. Open the
     project in a VizDAW build from before plan 7b (`git worktree add
     /tmp/vz7a <7a-end-sha> && (cd /tmp/vz7a && scripts/build-ffi.sh &&
     scripts/make-app.sh)`, then open `/tmp/vz7a/dist/VizDAW.app`): it
     refuses with "project format version 4 is newer than this build
     supports". Quit it, then `git worktree remove /tmp/vz7a`.
215. Choose Console: fader caps, knobs and panels look shaded and
     bevelled, like hardware, with no photographs or textures and no
     brand names. Choose each of the other four: their knobs and caps are
     flat (unless G1 made a theme shaded). In every theme the pan knob's ring and the send knobs' rings
     have their own colours.
216. Look at the transport bar, the pane buttons, the editors' bars and
     the Settings tabs: they show SymbioticAudio's icons, with no system
     symbols. Then `rm -rf app/.build`, build with
     `VIZDAW_NO_BRAND=1 scripts/make-app.sh` and open `dist/VizDAW.app`:
     every icon still shows, in the standard set. Rebuild without the
     variable afterwards (`rm -rf app/.build && scripts/make-app.sh`):
     the branded icons are back.
```

- [ ] **Step 4: Update the root `README.md`** (spec §2 item 11, 7b's part). Keep 7a's sections.
  - In the status paragraph, the sentence that names the themes and an audio editor as next becomes "An audio editor is next." The sentence reads "The look of the interface (themes) and an audio editor are next." (7a end, ff5b97b; wrapped over two lines).
  - "What works today" gains a bullet: "five themes, Classic Light by default, and a channel strip with typed levels, pan and sends, send knobs, an EQ slot and an optional colour per strip".
  - The "License" line and the "Building from source" sentence are B3's; leave them.
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
  - "Padding and spacing literals outside the views 7b redrew" (Ruling 13, for 7d);
  - "Knob and fader scroll-wheel control" (Ruling 4: macOS 14 SwiftUI has no scroll-wheel gesture; needs an `NSViewRepresentable` overlay).

  The "User track colours" row becomes: "User track colours: strips have one since plan 7b (C-1); lanes and the Kontrol's LEDs still show the kind."
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
- Rust: 7a end + 20 passed, 7a end + 1 ignored (macOS); Linux clippy clean; the rt-check gates pass;
- Swift: 7a end + 31;
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
   - §2 item 7, console colouring in every theme: B6's `StripPalette` and B7. The optional colour bar: a real per-strip colour (C-1, Ruling 12) in B1, B2, B6 and B7, its swatches approved at G1.
   - §2 item 8's strip kinds for colour: B2 (Ruling 10).
   - §2 item 9, the icons and their fallback: A3 and B3 (Ruling 6).
   - §2 item 11, 7b's README part: B8.
   - §4's anti-goals:
     - skeuomorphism in Console only: `reliefIsNoneOutsideConsole` and `flatThemesHaveClearRelief` (relief and `knobFinish`);
     - controls too small: `everyControlMeetsTheMinimumHit` and `aShortPaneScrollsTheStripAndKeepsTheMinimumThrow`;
     - hard-coded colours: B5's scan;
     - the clean room: `themeNamesKeepTheCleanRoom` and A1's page test.
   - §5, the theme as a preference with tests off the real preferences: B4 (`MemoryDefaults`).
   - Out of scope, as asked: the window and panes (7a), the audio editor (7c), and clean-up (7d, given one backlog row).
2. **Step scan.** Every test step gives names and assertions with the spec's values. Every code step gives signatures, files and the pinned values. The view tasks (B7, and B5's sweep) are checked by the scan, the e2e and README 207–216, and B7's step 2 lists what must survive. No step says "handle edge cases" without naming them.
3. **Type consistency.**
   - `StripColourRow` (B2, `StripRow.kind_colour`) is used by `StripPalette.cap` (B6) and `ChannelStrip` (B7).
   - `Swatch` (B1) → `SwatchRow` and `Session::set_strip_colour` (B2) → `MixerModel.setColour` and `StripPalette.bar` (B6) → the Colour ▸ menu and the bar (B7); `Theme.swatch` (A2) supplies the colours.
   - `Theme.knobFinish` (A2) → `KnobPalette.finish` (A4) → `Knob`'s shading (A4, B7).
   - `SetEq` (B1) → `Session::set_eq` (B2) → `SessionProtocol.setEq` → `MixerModel.pick(.eq)` and `clearEq` (B6).
   - `StripRow.eq` (B2) → `StripRow.slots` (B6) → `PluginWindowModel.refresh`, `MixerModel.heldSlots(of:)` and `StripChartModel` (B6), and the EQ slot's Open and Load (B7).
   - `AppearancePreferences` (B4) is used by `ThemedRoot` and `ChannelStrip`'s colour bars (B7).
   - `KnobPalette`, `FaderPalette`, `NumberBoxPalette` and `Relief` (A4) take `Theme`'s `knob`, `fader`, `relief` and `knobFinish` (A2).
   - `AppIcon` (B3) uses A3's roles.
   - `StripEntry.levelHint`, `panHint` and `touchedHint` are the e2e's and README's exact strings.
   - `ScaleMark` (B6) maps to `FaderMark` (A4) in B7.
4. **Review Focus.** Five items, each with its pinning tests in the owning task (B1, B2, B4, B5, B6 and B8).
5. **Proportion.** About 1490 short lines after the second revision (7a's plan is 1680), against a 145-line spec whose 7b row covers §2 items 5–9 and 11, across two repos and twelve tasks. The code blocks are tests and the exact README text; bodies are left to the signatures.

## Execution handoff

Plan complete and saved to `docs/superpowers/plans/2026-11-03-themes-strip.md`. The controller runs it subagent-driven, as in SP5 and 7a, with one reviewer per task, and stops at Gates G1 and G2 for the owner.

- Part A runs in `~/workspace/symbiotic-design`: brief its implementers with that repo's README, HANDOFF and design spec, and with this plan's Global Constraints for Part A.
- B1 and B2 may run while G1 is open.
- B5 and B7 are view work that only the scans, the e2e and the owner's by-eye steps check. Their reviews should read each changed site against Rulings 13 and 14 line by line, and check B7's "nothing lost" list item by item.
