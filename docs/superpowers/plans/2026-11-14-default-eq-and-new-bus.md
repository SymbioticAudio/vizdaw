# Chosen Default EQ, EQ on Buses and Master, New Bus from a Channel Strip Implementation Plan (6g-2)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The user picks which EQ new channels get (Multi-Q, or any installed AU or CLAP effect). New buses, new InstrumentOutput strips and the master of new projects get it too. Every strip's send menu offers "New Bus", which makes a bus and a send to it in one undo step.

**Architecture:**
- **`core-model`.** `fill_new_eqs` fills every strip kind a command created except `Master`, so buses and InstrumentOutput strips get the default with no new call site. `History` gains a fallback note. When the fill put at least one slot in, the note is added to that step's `Report.warnings`.
- **`core-ffi::bundled`.**
  - A process-wide choice, `set_default_eq(Option<DefaultEqRow>)`. `None` means Multi-Q.
  - `new_strip_eq()` resolves the choice: the chosen plug-in when it is usable, else `multiq()`, else nothing. `arm` passes the plug-in and the fallback note to the History.
  - `starting_document` gives the master of a new project the same EQ, without a history step.
- **`Session::new_bus_from(strip, name)`.** One undo step, "New Bus": `AddBus` then `AddSend`, through the armed History, so the bus gets the default EQ.
- **Swift.**
  - `PluginSettings` gains the choice, the list of choices and the note. `PluginSettingsView` gains the picker.
  - `MixerModel.newBus(from:)` and `SendKnobRow` add the "New Bus" item and the ＋.
  - Two control commands: `plugins.default_eq`, and `mixer.new_bus` (new `MixerCommands`).

**Tech Stack:** Rust 2024 (`core-model`, `core-ffi` with UniFFI 0.32.2, `core-control`, `vizctl`). Swift 6.1 / SwiftPM with Swift Testing, macOS 14+. No new crate, no new dependency.

**Spec:** the owner's approval of 2026-11-14 below is the binding spec. It amends `docs/superpowers/specs/2026-11-11-import-export-design.md` §8 ("New channels"). Executors read both, plus plan 6g (`docs/superpowers/plans/2026-11-13-multiq.md`) for context.

> **What the owner approved (2026-11-14). These are the binding requirements:**
> 1. **Your own default EQ.** In Settings › Plug-ins, beside the existing "Put an EQ on new channels" switch, add a picker for which EQ it is: Multi-Q, which is the default, or any installed AU or CLAP effect. If the chosen plug-in is missing or unusable later, new channels fall back to Multi-Q and a note says so. Add a control-channel command to set it, plus a hostile-input test; the registry goes from 179 to 180 (check the current count).
> 2. **The default EQ on buses and master too.** New buses get it, and so does the master of new projects. Existing projects are not changed. This overrides the 6g rule (spec docs/superpowers/specs/2026-11-11-import-export-design.md §8, "Buses and master do not, and keep 'EQ ＋'") and 6g pre-flight Ruling 3's exclusions for buses and master. InstrumentOutput strips: decide from the spirit, which is that every new mixing channel gets the default EQ, and state your call.
> 3. **New Bus from a channel strip.** Each track's send menu on its channel strip always offers "New Bus". The strip view is shared by the arrange window and the mixer, so it works in both. Choosing it creates a bus and a send from that track's strip to it, as one undo step. If the strip has no sends yet, the send area shows a ＋ that does the same thing. The bus is named like the existing "Add Bus" names it, and it gets the default EQ (item 2). Add a control-channel command, or extend an existing one, for this too, with hostile-input tests.

**Not in this plan:**
- Changing the EQ of strips that already exist, in any project. "Existing projects are not changed."
- A per-project default EQ. The choice is a preference of this Mac, like the switch.
- Choosing an EQ's preset or starting settings. The chosen plug-in starts at its own default, as Multi-Q does.
- The demo projects (`Session::new_demo`, `new_demo_audio`). They are fixtures, not new projects.

## Plan Rulings

These are this plan's decisions where the approval leaves a choice. Each names its cost if wrong.

1. **InstrumentOutput strips get the default EQ.** The spirit is that every new mixing channel gets it, and an extra output of a multi-output instrument has its own fader, inserts and sends, so it counts. `fill_new_eqs` therefore fills every created strip except `Master`.
   - **Cost if wrong:** a 16-output instrument whose outputs are added one by one gets one EQ per output, and the user removes the ones not wanted. Reverting is a one-line change in `fill_new_eqs`.
2. **The master of new projects.** Only `Session::new_empty` and `Session::new_project` count, which is File › New and the New Project sheet. Both go through `starting_document`, which applies `MixerCmd::SetEq { strip: master, plugin: Some(eq) }` with `core_model::command::apply` when `bundled::new_strip_eq()` is `Some`. There is no history step, so the new project is not dirty and has nothing to undo.
   - **Not new projects:** `Session::open`, recovery, the demos, and `Session::from_document`.
   - **Imports.** An import into a new project starts from that session's document, so its master already has the default. An import that loads its own master EQ replaces it, which is spec §8's "unless the import fills the slot".
3. **Resolving the choice** (`bundled::new_strip_eq`, read at every `arm`):
   - the switch is off: `None`, with no note;
   - no choice is set: `multiq()`, with no note;
   - a choice is set, and the catalog's `preferred(format, id)` entry has `kind == Some(PluginKind::Effect)` and status `Ok` or `NotValidated`: that entry's `PluginRef`, with no note;
   - otherwise `multiq()`. When that is `Some`, the note is "<name> is not available, so new channels get Multi-Q." When it is `None`, there is no EQ and no History note.

   `<name>` is the `DefaultEqRow.name` the app stored, cut to 100 characters with "…". Nothing is loaded to decide.
4. **Where the note shows.**
   - **History note.** Every command whose fill used the fallback gets the History note, as a line in its snapshot's `warnings`. That includes `new_track`, `new_bus_from` and imports.
   - **Settings note.** Settings › Plug-ins shows `default_eq_note()` under the picker. It gives the History note's sentence, or, when Multi-Q is unusable too, "<name> is not available, and neither is Multi-Q, so new channels get no EQ." With the switch off it is nil.
   - **Cost if wrong:** one extra status line per new track while the chosen plug-in is gone. The line is the only sign the user gets.
5. **Storing the choice.**
   - It lives in `UserDefaults` under `VizDAW.newChannelEqPlugin`, as `["format": "au"|"clap", "id": String, "name": String]`. The key absent means Multi-Q.
   - A value of any other shape reads as Multi-Q, and the key is removed.
   - The app pushes the choice to the core at launch and on every change, as it does the switch.
   - Choosing Multi-Q in the picker stores nothing, and the key is removed. Multi-Q's own CLAP ID (`MULTIQ_CLAP_ID`) never appears as a second item.
6. **The picker's items:**
   - "Multi-Q (built in)", when `bundledEqName` is non-nil;
   - then every effect `appPlugins.browser(.effect)` lists that is enabled with status `ok` or `notValidated`, except Multi-Q's CLAP, by name. Each is labelled "<name> (AU)" or "<name> (CLAP)";
   - then, if the stored choice is not among them, "<name> (not available)", so the picker shows what is chosen.

   The list is read again by `PluginSettings.refreshEqChoices()` when the tab appears.
7. **The control commands.** They bring the registry from 179 to **181**. Item 3's option of extending an existing command does not apply, because the registry has no mixer bus or send command to extend.
   - **`plugins.default_eq`** takes `plugin` (Text, required):
     - `multi-q`;
     - `clap:<id>` or `au:<type/subtype/manufacturer>`, which must match an item of `eqChoices` that is not "(not available)", else `unknown_id` "No installed effect is <plugin>.";
     - a NUL, an empty ID or another prefix is `invalid_params`.

     It answers `status` "default EQ <name>". Its registry entry is `key_only`, menu "Settings", `document: false`.
   - **`mixer.new_bus`** takes `strip` (`opt_id_param`) and `name` (Text, optional):
     - with `strip`, it runs `new_bus_from`, whose undo label is "New Bus";
     - without `strip`, it runs `add_bus`, which is "Add Bus";
     - `name` defaults to `MixerModel.nextBusName`;
     - a NUL in `name`, or a blank `name`, is `invalid_params`;
     - master as `strip` is `invalid_params` "Master has no sends.";
     - an unknown strip is `unknown_id`.

     It answers `status` with the bus's name. Its registry entry is `key_only`, menu "Mixer", `document: true`.
8. **Which strips offer New Bus.** Every strip with a send area offers it, which is every kind but master. That includes buses: a send from a bus to a new bus cannot loop. In `StripCells`, the sends row is shown for every non-master strip. Before 6g-2 it was hidden when the strip had no sends and no bus existed.
   - **The send area holds:**
     - the sends;
     - the "Send" menu, always: the buses, a `Divider`, then "New Bus";
     - when `strip.sends` is empty, a "＋" button beside the menu.
   - **Labels and help.** The ＋ has the accessibility label "New bus fed by \(strip.name)" and the help "New Bus: a bus with a send from this strip". The menu item has the label "New Bus".
   - **The name.** One rule: `MixerModel.nextBusName(_ buses: [BusRow]) -> String`, the existing `addBus` rule moved into a static: "Bus N" with the lowest N ≥ 1 that no bus has. `addBus`, `newBus(from:)` and the command all use it.
   - **Cost if wrong:** strips grow by one row. The row is shared across a mixer line, so strips stay aligned.

## Global Constraints

Every task's requirements include these.

- **Shell.** Every command runs after `export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools` and `cd /Users/lattawb/workspace/vizdaw`.
- **Swift.**
  - Build only through `scripts/build-ffi.sh` (after any FFI change) and `scripts/swift.sh build|test`, plus `scripts/make-app.sh` in Task 4. Never run `swift`, `xcodebuild` or `xcrun` directly.
  - Swift 6.1:
    - no `[weak self]`, and no non-Sendable value captured inside `MainActor.assumeIsolated`;
    - no `@State` or `@FocusState`: view state lives in the `@Observable` models.
  - Every new control has an accessibility label, and colours, fonts and spacing come from theme tokens (`theme.text…`, `theme.type…`, `theme.spacing…`).
- **Tests never touch the machine.**
  - They never open real audio or MIDI, and never trigger the microphone or TCC prompt.
  - They never touch the real `~/Library`: Application Support/VizDAW, preferences, the Trash, `Audio/Plug-Ins`.
  - Use temp folders, `MemoryDefaults()`, the e2e's throwaway home, and fakes.
  - Every `core-ffi` test file calls `force_offline()` through its `serial` helper.
  - Swift tests that make a real `Session` set `TestSupportDir.shared`.
  - Tests that load the real Multi-Q run in a child process whose `HOME` and `CFFIXED_USER_HOME` are a temp folder, through `crates/core-host/tests/support/temp_home.rs`, as 6g's do. This plan adds none; its tests use `bundled::debug_set_multiq` and the test-plugins bundle.
  - Linux CI has no AU, so any test that needs one is `#[cfg(target_os = "macos")]`.
- **Tools.**
  - Never run `auval`.
  - No GUI, computer-use or accessibility-automation tools. Launch VizDAW only through the e2e with `VIZDAW_AUDIO=offline`. Afterwards `pgrep -lf MacOS/VizDAW` prints nothing.
  - Never use `dangerouslyDisableSandbox`.
  - Never read `~/workspace/SVP/research`.
- **Programmatic calls open nothing.** Control-channel commands and programmatic calls never open dialogs, sheets, alerts, panels, menus or windows.
- **Hostile input.**
  - Every new FFI export runs inside `ffi_guard` (and so `catch_unwind`). It returns `Result<_, VizError>`, or a safe default for the never-failing ones.
  - Each new control command gets hostile-input tests: wrong types, unknown params, missing required params, a 70 000-character string, a NUL, and IDs out of range.
  - The registry count goes from 179 to **181**.
- **Names.**
  - The repo and UI never name the vendors and DAWs on the project's banned list. That list lives only in the name scans (`app/Tests/VizDAWAppModelTests/NameScanTests.swift`, `crates/core-control/tests/names.rs`); read them, keep all new text clear of it, and never copy the list.
  - Text the app shows says "track", never "lane". Identifiers and JSON keys keep `lane`.
- **Commits.**
  - Stay on branch `sp6-import` (at `809fc0c` when this plan was written). Never switch branches, never push.
  - Commit at the end of every task by explicit paths, never `git add -A`. Never commit `.superpowers/`, `.DS_Store`, `target/`, generated FFI (`app/Sources/VizDAWCore/vizdaw_ffi.swift`, `app/Sources/vizdaw_ffiFFI/include/`, `target/ffi/`) or `dist/`.
  - Every message ends with exactly these two lines:
    ```
    Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
    Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P
    ```
  - Check each commit with `git log -1 --oneline && git show --stat HEAD`.
  - `Cargo.lock` does not change.
- **Brand.** Brand assets are never in the repo. `VIZDAW_NO_BRAND=1` builds without them.
- **Lints.** Every crate keeps `#![deny(missing_docs)]` and the `clippy::{unwrap_used, expect_used, indexing_slicing, panic}` denials outside tests. `core-model` stays free of OS APIs.
- **Format.** `core_model::file::VERSION` stays 9. The EQ slot already exists.
- **TDD.** Every new or changed test is run and seen to fail before the code that makes it pass.
- **Each task ends green.**
  - Always:
    - `cargo fmt --all -- --check`;
    - `cargo clippy --workspace --all-targets -- -D warnings`;
    - the Linux clippy: `cargo clippy --workspace --all-targets --target x86_64-unknown-linux-gnu --target-dir target/linux-check -- -D warnings`;
    - `cargo test` for the crates the task touches.
  - Tasks 1 and 3 also run `cargo clippy -p core-ffi --lib -- -D warnings`.
  - Tasks 1–3 also run `scripts/build-ffi.sh`, `scripts/swift.sh build` and `scripts/swift.sh test`.
  - Known flaky: the `damaged_files` and `dawproject_fuzz` proptests, `platform-macos/tests/midi.rs`, `core-ffi` catalog tests under load, and the latency e2e timing. Re-run once before investigating, and say so in the report.
  - **Baseline:** measure at the start of Task 1: registry 179, README through 317, e2e 30 ignored, `VERSION` 9. Each task reports the counts it adds.

## Review Focus

These are the five failure modes a user is most likely to hit, most likely first. Each is pinned by named tests in its task.

1. **The chosen EQ is gone or unusable when a channel is made.**
   - Examples:
     - it was uninstalled, or disabled in the Plugin Manager;
     - it was blocklisted after a crash;
     - the catalog has not scanned yet at first launch;
     - the stored choice names an instrument;
     - Multi-Q is unusable too.
   - Expected:
     - the new strip gets Multi-Q, and the step's warnings say "<name> is not available, so new channels get Multi-Q.";
     - with Multi-Q unusable too, the strip shows "EQ ＋" and the Settings note gives the "neither" sentence;
     - nothing panics, and no placeholder slot for the missing plug-in is made.
   - Pinned by Task 1's `chosen_eq_falls_back_with_a_note` and Task 2's `aMissingChoiceShowsAndSaysSo`.
2. **Existing projects change.**
   - Examples:
     - opening a project saved before 6g-2 whose buses and master have no EQ;
     - Undo and Redo across the change;
     - adding a track to an old project;
     - a DAWproject merge into an existing project.
   - Expected: only strips the command creates are filled. An opened project's master and buses stay as saved, and opening it leaves it clean.
   - Pinned by Task 1's `existing_projects_keep_their_strips` and `every_new_strip_but_master`.
3. **New Bus is not one step, or half happens.**
   - Examples:
     - Undo right after;
     - Redo;
     - a strip removed between the menu opening and the click (a stale ID);
     - master as the source;
     - a second New Bus from the same strip.
   - Expected:
     - Undo removes the bus and the send together, and Redo brings both back with the same IDs;
     - a refused New Bus leaves no bus behind and no undo step, and shows one alert, "New Bus Failed".
   - Pinned by Task 3's `new_bus_from_a_strip_is_one_step` and `new_bus_refuses_bad_input`.
4. **Bus names collide.**
   - Examples:
     - "Bus 1" renamed;
     - a gap, with "Bus 1" and "Bus 3" present;
     - a track strip that happens to be named "Bus 1".
   - Expected: the same name Add Bus would give, counting bus names only.
   - Pinned by Task 3's `newBusUsesTheAddBusName`.
5. **Hostile control input.**
   - Examples:
     - `plugins.default_eq` with a NUL, `clap:` and nothing after it, `vst:x`, an instrument's ID, or 70 000 characters;
     - `mixer.new_bus` with `strip` as a string, negative, past `i64::MAX`, master or unknown, or a NUL or blank `name`.
   - Expected: each is `invalid_params` or `unknown_id`, nothing changes, and no dialog opens.
   - Pinned by Task 2's `defaultEqCommandRefusesBadInput` and `default_eq_command_checks_its_params`, and Task 3's `mixer_new_bus_checks_its_params` and `newBusCommandRefusesBadInput`.

---

### Task 1: The default EQ on every new strip but master, on the master of new projects, and the chosen EQ with its fallback

**Files:**
- Modify:
  - `crates/core-model/src/command/strips.rs` (`fill_new_eqs`: every kind but `Master`);
  - `crates/core-model/src/history.rs` (the note; the doc lines of `execute` and `set_new_strip_eq`);
  - `crates/core-model/tests/new_strip_eq.rs`;
  - `crates/core-ffi/src/bundled.rs`;
  - `crates/core-ffi/src/session.rs` (`starting_document`; `new_track`'s snapshot carries the step's warnings);
  - `crates/core-ffi/tests/new_channel_eq.rs` (flip any 6g assertion that a bus, master or InstrumentOutput strip stays empty).
- Create: `crates/core-ffi/tests/default_eq.rs`.

**Interfaces:**
- Produces (`core-model`):
  ```rust
  impl History {
      /// A line added to the warnings of each step whose fill put in at least one slot (6g-2 Ruling 4).
      pub fn set_new_strip_eq_note(&mut self, note: Option<String>);
  }
  // fill_new_eqs keeps its signature; it now fills every created strip kind except Master.
  ```
- Produces (`core-ffi::bundled`):
  ```rust
  /// The user's choice of EQ for new channels (Ruling 5); None: Multi-Q.
  #[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Record)]
  pub struct DefaultEqRow { pub format: PluginFormatTag, pub id: String, pub name: String }
  #[uniffi::export] pub fn set_default_eq(choice: Option<DefaultEqRow>);   // never fails
  #[uniffi::export] pub fn default_eq() -> Option<DefaultEqRow>;           // never fails
  /// Ruling 4's Settings sentence; None when the choice is usable, unset, or the switch is off.
  #[uniffi::export] pub fn default_eq_note() -> Option<String>;            // never fails
  pub fn new_strip_eq() -> Option<PluginRef>;      // now Ruling 3
  pub fn fallback_note() -> Option<String>;        // Ruling 3's note, only when the fallback is Some
  pub fn arm(h: &mut History);                     // sets both new_strip_eq and the note
  ```

- [ ] **Step 1: Change and add the `core-model` tests** in `new_strip_eq.rs`.
  - Rename `only_new_audio_instrument_and_synth_strips` to `every_new_strip_but_master`, and flip its assertions:
    - `AddBus` fills the bus;
    - `AddInstrumentOutput` fills the InstrumentOutput strip;
    - master stays `None`;
    - strips that already existed keep their `eq`.
  - Add `the_note_rides_with_a_fill`. With `set_new_strip_eq(Some(mq()))` and `set_new_strip_eq_note(Some("X is not available, so new channels get Multi-Q.".into()))`:
    - an `AddBus` report's `warnings` contain the line once;
    - a `SetGain` (no new strip) has no such line;
    - with the plug-in `None`, an `AddBus` has no line.
- [ ] **Step 2: Run** `cargo test -p core-model --test new_strip_eq`. Expected: FAIL (the bus is empty; no `set_new_strip_eq_note`).
- [ ] **Step 3: Implement** the `fill_new_eqs` change, the note field (kept by `History::clone`), and the push in `History::execute` when `fill_new_eqs` returned more than 0. Update the doc lines, which still say "audio, instrument and built-in-synth".
- [ ] **Step 4: Run** `cargo test -p core-model`. Expected: PASS.
- [ ] **Step 5: Write the failing `core-ffi` tests** in `default_eq.rs`.
  - Use a `serial` helper copied from `new_channel_eq.rs`: offline, the test-plugins bundle added, the switch on, the override cleared. It also calls `set_default_eq(None)`.
  - `gain()` is `DefaultEqRow { Clap, "vizdaw.test.gain", "Test Gain" }`.
  - `chosen_eq_goes_on_new_channels`:
    - with `debug_set_multiq(Some(mq()))` and `set_default_eq(Some(gain()))`, `new_track(Audio)` gives the AudioPath strip an EQ with ID `vizdaw.test.gain`;
    - `add_bus("Bus 1")` gives the bus the same;
    - `default_eq()` answers `gain()`, and `default_eq_note()` is `None`;
    - the snapshots' `warnings` have no "not available" line.
  - `chosen_eq_falls_back_with_a_note`, with the override `Some(mq())`:
    - disable `vizdaw.test.gain` through the catalog, as the Plugin Manager's `set_enabled` does. The next `new_track(Audio)` strip has Multi-Q, and the snapshot's `warnings` contain "Test Gain is not available, so new channels get Multi-Q.";
    - the same holds for a choice of `vizdaw.test.synth4` (an instrument), of `vizdaw.test.unknown`, of an ID holding a NUL, and of a 70 000-character ID. None of them panics;
    - a 300-character `name` appears cut to 100 characters with "…";
    - with the override `None`, the strip is empty, no warning line appears, and `default_eq_note()` is "Test Gain is not available, and neither is Multi-Q, so new channels get no EQ.";
    - with `set_new_channel_eq(false)`, `default_eq_note()` is `None`.
  - `new_projects_master_gets_it`, with the override `Some(mq())`:
    - `Session::new_empty()` and `Session::new_project(<48 kHz/24-bit row>)` each have a master whose EQ names Multi-Q, with `dirty == false` and `undo_label == None`;
    - with the switch off, master is empty;
    - `Session::new_demo()` keeps master empty.
  - `existing_projects_keep_their_strips`:
    - with the override `None`, make a session, add a track and `add_bus("Bus 1")`, then `save_as` into a temp folder;
    - set the override `Some(mq())` and `Session::open` the file. Master and the bus are still empty, and the snapshot is not dirty;
    - `add_bus("Bus 2")`: only Bus 2 is filled;
    - one `undo` empties nothing that was there before.
- [ ] **Step 6: Run** `cargo test -p core-ffi --test default_eq --test new_channel_eq`. Expected: FAIL.
- [ ] **Step 7: Implement** in `bundled.rs` and `session.rs`.
  - The choice is a `static CHOSEN: Mutex<Option<DefaultEqRow>>`, read through `crate::guard::lock`.
  - `new_strip_eq` and `fallback_note` follow Ruling 3, reading the catalog through `catalog::with`, `Catalog::preferred` and `status_of`.
  - `arm` calls `h.set_new_strip_eq(..)` and `h.set_new_strip_eq_note(fallback_note())`.
  - `starting_document` applies Ruling 2.
  - `new_track` returns `inner.snapshot_with(<the step's warnings>)` instead of `inner.snapshot()`, so the note reaches the app.
- [ ] **Step 8: Run** the Step 6 tests, then `cargo test -p core-ffi`. Expected: PASS.
- [ ] **Step 9: End-of-task checks**, including `scripts/build-ffi.sh`, `scripts/swift.sh build` and `scripts/swift.sh test`. **Commit** `feat(model): the default EQ on new buses, extra outputs and new projects' master, and a chosen EQ with a fallback (plan 6g-2 Task 1)`.

### Task 2: Settings › Plug-ins picker and `plugins.default_eq`

**Files:**
- Modify:
  - `app/Sources/VizDAWAppModel/PluginSettings.swift`;
  - `app/Sources/VizDAWAppModel/PluginSettingsCommands.swift`;
  - `app/Sources/VizDAW/PluginSettingsView.swift` (the picker, the note, the new caption);
  - `app/Sources/VizDAW/VizDAWApp.swift` (`LivePluginSettingsSink`);
  - `app/Tests/VizDAWAppModelTests/PluginSettingsTests.swift` (`FakePluginSettingsSink` gains the new members);
  - `crates/core-control/src/registry.rs`;
  - `crates/core-control/tests/commands.rs`.

**Interfaces:**
- Consumes: Task 1's `DefaultEqRow`, `set_default_eq`, `default_eq_note`. In Swift these are `DefaultEqRow(format:id:name:)`, `setDefaultEq(choice:)` and `defaultEqNote()`.
- Produces (Swift):
  ```swift
  // PluginSettingsSink gains:
  func setDefaultEq(_ choice: DefaultEqRow?)
  func eqChoices() -> [DefaultEqRow]        // Ruling 6's installed items, Multi-Q's CLAP excluded
  func defaultEqNote() -> String?
  // PluginSettings gains:
  public static let eqKey = "VizDAW.newChannelEqPlugin"
  public private(set) var defaultEq: DefaultEqRow?      // nil: Multi-Q; pushed to the sink in init
  public private(set) var eqChoices: [DefaultEqRow]
  public private(set) var eqNote: String?
  public var defaultEqName: String                      // defaultEq?.name ?? "Multi-Q"
  public var defaultEqAvailable: Bool                   // nil choice, or the choice is in eqChoices
  public func setDefaultEq(_ choice: DefaultEqRow?)     // stores (Ruling 5), pushes, re-reads the note
  public func refreshEqChoices()                        // re-reads eqChoices and eqNote
  // PluginSettingsCommands gains:
  public static let defaultEq = "plugins.default_eq"
  ```
- **The view.** It adds `Picker("EQ for new channels", …)` after the Toggle, with Ruling 6's items. The picker is disabled while the switch is off, and has the accessibility label "EQ for new channels". `eqNote`, when present, goes under it in `theme.state.warning`.
  - **The caption** becomes "New channels, new buses and the master of new projects start with \(settings.defaultEqName) in their EQ slot. Projects you already have are not changed."
  - **The Toggle** is no longer disabled when `bundledEqName` is nil and `eqChoices` is not empty.
  - **On appear**, `.onAppear { settings.refreshEqChoices() }`.
- **The registry entry.** `PLUGINS_DEFAULT_EQ_PARAMS` is one required Text `plugin`. Its help is "multi-q, or clap:<id> or au:<type/subtype/manufacturer> of an installed effect (see the Plugin Manager)."

- [ ] **Step 1: Write the failing command tests** in `commands.rs`.
  - The registry count becomes **180** for now, with "; 6g-2 adds plugins.default_eq" appended to the message. Task 3 makes it 181.
  - `default_eq_command_checks_its_params`:
    - `multi-q` and `clap:vizdaw.test.gain` parse to `CommandRun` with `("plugin", Text(..))`;
    - each of these is `invalid_params`: `plugin` missing, `plugin: 1`, `plugin: true`, an unknown param, and a 70 000-character `plugin`.
- [ ] **Step 2: Write the failing Swift tests** in `PluginSettingsTests.swift`, with `MemoryDefaults()` and the fake sink.
  - `theChoiceDefaultsToMultiQAndPersists`:
    - a new model has `defaultEq == nil`, and the sink got `setDefaultEq(nil)` once;
    - `setDefaultEq(gain)` stores the dictionary under `eqKey` and pushes it;
    - a new model on the same defaults reads `gain` and pushes it;
    - `setDefaultEq(nil)` removes the key.
  - `aDamagedChoiceReadsAsMultiQ`: each of these under `eqKey` gives `defaultEq == nil`, with the key removed:
    - a String;
    - a dictionary with `format: "vst"`;
    - a dictionary with no `id`;
    - an `id` holding a NUL.
  - `aMissingChoiceShowsAndSaysSo`: the stored `gain` is not in the fake's `eqChoices`, and the fake's note is "Test Gain is not available, so new channels get Multi-Q.". After `refreshEqChoices()`, `defaultEqAvailable == false` and `eqNote` is that line.
  - `defaultEqCommandSetsAndAnswers`:
    - `run("plugins.default_eq", ["plugin": "clap:vizdaw.test.gain"])`, with `gain` in `eqChoices`, answers status "default EQ Test Gain" and sets `defaultEq`;
    - `multi-q` answers "default EQ Multi-Q" and sets nil.
  - `defaultEqCommandRefusesBadInput`:
    - `"clap:"`, `"vst:x"`, `"gain"` and `"clap:a\u{0}b"` are each `.invalidParams`;
    - `"clap:vizdaw.test.synth4"`, which is not in `eqChoices`, is `.unknownId`;
    - `defaultEq` is unchanged after each, and the sink got no extra `setDefaultEq`.
- [ ] **Step 3: Run** `cargo test -p core-control --test commands` and `scripts/swift.sh test --filter PluginSettingsTests`. Expected: FAIL.
- [ ] **Step 4: Implement** the registry entry, the model, the command, the live sink and the view.
  - `eqChoices()` maps `appPlugins.browser(.effect)` per Ruling 6.
  - Dispatch needs no change: `PluginSettingsCommands.ids` already flows into `ControlCommands.handled`.
- [ ] **Step 5: Run** the Step 3 commands. Expected: PASS. `NameScanTests` and `core-control/tests/names.rs` pass with the new copy.
- [ ] **Step 6: End-of-task checks**, including the Swift build and test. **Commit** `feat(app): choose the EQ new channels get in Settings › Plug-ins (plan 6g-2 Task 2)`.

### Task 3: New Bus from a channel strip, and `mixer.new_bus`

**Files:**
- Modify:
  - `crates/core-ffi/src/session.rs` (`new_bus_from`);
  - `crates/core-control/src/registry.rs`;
  - `crates/core-control/tests/commands.rs`;
  - `app/Sources/VizDAWAppModel/MixerModel.swift`;
  - `app/Sources/VizDAWAppModel/ControlCommands.swift` (dispatch and `handled`);
  - `app/Sources/VizDAW/ChannelStrip.swift` (`StripCells`, `SendKnobRow`);
  - `app/Tests/VizDAWAppModelTests/FakeSession.swift`;
  - `app/Tests/VizDAWAppModelTests/MixerModelTests.swift`.
- Create:
  - `crates/core-ffi/tests/new_bus.rs`;
  - `app/Sources/VizDAWAppModel/MixerCommands.swift`;
  - `app/Tests/VizDAWAppModelTests/MixerCommandsTests.swift`.

**Interfaces:**
- Consumes: Task 1's armed History. The bus is filled by `fill_new_eqs`, with no call of its own.
- Produces (FFI):
  ```rust
  /// A bus named `name` (trimmed; blank refused) and a post-fader 0 dB send from `strip` to it,
  /// one undo step "New Bus" (Ruling 7). Refused for master ("Master has no sends.") and an
  /// unknown strip; a refusal leaves no bus and no step.
  pub fn new_bus_from(&self, strip: u64, name: String) -> Result<DocSnapshot, VizError>;
  ```
  It follows `add_lane_monitored`'s pattern under one lock:
  1. `bundled::arm`;
  2. `begin("New Bus")`;
  3. `execute(AddBus { name, index: None })`;
  4. find the new `AnyId::Strip` in the report;
  5. `execute(AddSend { strip, send: AuxSend::new(bus) })`;
  6. `commit`, or `abort` on any error.

  Then the usual `version`, `reload_host` and `mark`, and `snapshot_with(<both steps' warnings>)`.
- Produces (Swift):
  ```swift
  // MixerModel
  public static func nextBusName(_ buses: [BusRow]) -> String   // the rule addBus used
  public func newBus(from strip: UInt64)                         // "New Bus Failed" on error
  // MixerCommands
  @MainActor public enum MixerCommands {
      public static let newBus = "mixer.new_bus"
      public static let ids: Set<String>
      public static func run(_ id: String, args: [ControlArg], docID: UUID, _ m: DocumentModel) -> ControlOutcome
  }
  ```
  `MixerCommands.run` works through `m.session` and `m.apply`, as `AutomationCommands` does. It reads buses from `m.session.mixer().buses`.
- **The registry entry** is `MIXER_NEW_BUS_PARAMS`:
  - `opt_id_param("strip", "The strip whose new send feeds the bus (see state.get mixer); absent: a bus with no send.")`;
  - Text `name`, optional, with the help "The bus's name (default: the next free Bus N)."

- [ ] **Step 1: Write the failing Rust tests** in `new_bus.rs`, through a `serial` like Task 1's.
  - `new_bus_from_a_strip_is_one_step`:
    - `new_empty`, then `new_track(Audio)`; take its AudioPath strip;
    - `new_bus_from(strip, "Bus 1")`. The mixer has bus "Bus 1", and the strip's sends are exactly `[SendRow { target: bus, level_db: 0, pre_fader: false, enabled: true }]`;
    - `undo_label == Some("New Bus")`;
    - one `undo` leaves no bus and no send, and `undo_label` is the track's;
    - `redo` brings back both, with the same bus ID;
    - a second `new_bus_from(strip, "Bus 2")` gives the strip two sends.
  - `new_bus_gets_the_default_eq`:
    - with the override `Some(mq())`, the new bus's EQ names Multi-Q, and the source strip's EQ is unchanged;
    - with the switch off, the bus is empty.
  - `new_bus_refuses_bad_input`. Each of these returns `Err`, leaves the bus count, `undo_label` and the version unchanged, and leaves no open transaction (a following `add_bus` works):
    - master as `strip` (message "Master has no sends.");
    - `u64::MAX`;
    - a strip ID removed by an earlier `undo`;
    - the names `""` and `"   "`.
- [ ] **Step 2: Write the failing command tests** in `commands.rs`.
  - The count becomes **181**, with "and mixer.new_bus" appended to the 6g-2 clause.
  - `mixer_new_bus_checks_its_params`:
    - `{}`, `{strip: 3}` and `{strip: 3, name: "Drums"}` parse;
    - each of these is `invalid_params`: `strip: "3"`, `strip: -1`, `strip: 1e30`, `strip: true`, `name: 5`, a 70 000-character `name`, and an unknown param.
- [ ] **Step 3: Write the failing Swift tests.**
  - In `MixerModelTests.swift`:
    - `newBusUsesTheAddBusName`. `nextBusName` gives:
      - "Bus 1" for no buses;
      - "Bus 2" for `["Bus 1", "Bus 3"]`;
      - "Bus 1" for `["Drums"]`;
      - "Bus 1" when only a non-bus strip is named "Bus 1".

      `m.newBus(from: 1)` on the fake whose buses are `["Bus 1"]` records `"newBusFrom 1 Bus 2"`.
    - `aRefusedNewBusAlertsOnce`: the fake throws. One alert titled "New Bus Failed" is raised, and the model refetches.
  - In `MixerCommandsTests.swift`, with a fake-backed `DocumentModel` as `TrackCommandsTests` builds one:
    - `newBusCommandRuns`:
      - `{strip: 1}` calls `newBusFrom 1 Bus 2` and answers status "Bus 2";
      - `{}` calls `addBus Bus 2`;
      - `{strip: 1, name: "Drums"}` passes "Drums".
    - `newBusCommandRefusesBadInput`:
      - `name: "a\u{0}b"` and `name: "  "` are `.invalidParams`;
      - the master's ID is `.invalidParams` with "Master has no sends.";
      - an ID no strip has is `.unknownId`;
      - the fake records no call for any of them, and no alert is raised.
- [ ] **Step 4: Run** `cargo test -p core-ffi --test new_bus`, `cargo test -p core-control --test commands`, and `scripts/build-ffi.sh && scripts/swift.sh test --filter 'MixerModelTests|MixerCommandsTests'`. Expected: FAIL.
- [ ] **Step 5: Implement** `new_bus_from`, the registry entry, `nextBusName`, with `addBus` now calling it, `newBus(from:)`, and `MixerCommands` with its dispatch case. That case goes in `ControlCommands.run`'s document section, after the `noDocument` guard, plus `.union(MixerCommands.ids)` in `handled`. Then the view per Ruling 8:
  - `StripCells` shows `row(.sends, …)` for every non-master strip;
  - `SendKnobRow` always shows the Send menu, ending in `Divider()` and `Button("New Bus") { model.newBus(from: strip.id) }`;
  - when `strip.sends.isEmpty`, `SendKnobRow` also shows a ＋ (`AppIcon.add`, theme tokens) with the same action, label and help.
- [ ] **Step 6: Run** the Step 4 commands. Expected: PASS.
- [ ] **Step 7: End-of-task checks**, including the Swift build and test. **Commit** `feat(mixer): New Bus from a channel strip's send menu, and mixer.new_bus (plan 6g-2 Task 3)`.

### Task 4: README checks, the e2e, the backlog and the gates

**Files:**
- Modify:
  - `app/README.md` (step 311's last sentence; new steps from 318);
  - `crates/vizctl/tests/e2e.rs`;
  - `docs/feature-backlog.md`.

**Interfaces:**
- Consumes: Tasks 1–3. No production code, unless a step finds a bug. Such a bug is fixed in its owner's module, with a regression test.

- [ ] **Step 1: Add the e2e** `default_eq_and_new_bus_over_the_channel`, `#[ignore]` and in the style of `multiq_install_and_switch_over_the_channel`. The e2e runs with `--no-scan`, so it checks the commands and the undo step, not which EQ lands.
  - `plugins.default_eq` `{plugin: "multi-q"}` answers "default EQ Multi-Q", and `{plugin: "clap:vizdaw.e2e.none"}` is `unknown_id`.
  - `file.new`, then `track.new_audio`. Read the AudioPath strip's ID from `state.get mixer`.
  - `mixer.new_bus {strip}`:
    - it answers status "Bus 1" and undo label "New Bus";
    - `state.get mixer` shows the bus and the strip's send to it;
    - `edit.undo` removes both;
    - `edit.redo` brings them back.
  - `mixer.new_bus {}` answers "Bus 2". `mixer.new_bus {strip: <master>}` is `invalid_params`.
- [ ] **Step 2: Write the README steps** in `app/README.md`.
  - Change step 311's last sentence to "Add Bus: the bus shows "Multi-Q" too (plan 6g-2)."
  - Add a heading "Plan 6g-2, the chosen default EQ and New Bus:". Each step states what to do and what is seen, and says "track":
    - **318.** File › New: the master's EQ slot shows "Multi-Q". Open a project saved before this build: its master and buses are as they were, and the project is not marked edited.
    - **319.** Settings › Plug-ins: "EQ for new channels" lists "Multi-Q (built in)" and your installed effects. Choose another EQ. A new audio track, a new bus and File › New's master show it.
    - **320.** Plugin Manager: disable the EQ you chose. A new track gets Multi-Q, and the status line says "<name> is not available, so new channels get Multi-Q."; Settings › Plug-ins says the same under the picker. Enable it again, and choose Multi-Q again.
    - **321.** In the mixer, on an audio track's strip with no sends, click ＋ in the send area. A bus "Bus N" appears with Multi-Q in its EQ slot, and the strip has a send to it. Edit › Undo once: the bus and the send go together. Redo.
    - **322.** On a strip that already has a send, open its Send menu: the buses, then "New Bus". Choose it: a second send to a new bus.
    - **323.** Show the mixer in the arrange window and repeat step 321 on a strip there: the same.
    - **324.** With Settings › Plug-ins' switch off, New Bus makes a bus with "EQ ＋". Turn it back on.
- [ ] **Step 3: Update the backlog** (`docs/feature-backlog.md`):
  - in the 6g rows, note that 6g-2 overrode "Buses and master do not", and how (Rulings 1–2);
  - **new rows:**
    - "Choose the default EQ's starting preset";
    - "Per-project default EQ";
    - "Apply the default EQ to existing strips on request (a mixer menu item)";
    - any narrow edge cases the task reports listed, at the owner's review pace.
- [ ] **Step 4: Run the gates:**
  - `cargo test --workspace`;
  - both clippies;
  - `cargo clippy -p core-ffi --lib -- -D warnings`;
  - `scripts/build-ffi.sh`;
  - `scripts/swift.sh build`;
  - `scripts/swift.sh test`;
  - the no-brand build, `VIZDAW_NO_BRAND=1 scripts/swift.sh build --scratch-path <fresh temp dir>`;
  - `scripts/make-app.sh`;
  - the e2e, `VIZDAW_E2E_APP=dist/VizDAW.app cargo test -p vizctl --test e2e -- --ignored --nocapture --test-threads 1`: 31 ignored tests run, and all pass.

  Then `pgrep -lf MacOS/VizDAW` prints nothing. Report against the baseline: registry 181, README through 324, e2e 31, `VERSION` 9.
- [ ] **Step 5: Commit** `docs(app): default-EQ and New Bus README checks, e2e and backlog (plan 6g-2 Task 4)`.

## Self-Review

1. **Spec coverage.**
   - **Item 1:**
     - the picker beside the switch, with Multi-Q as the default and installed AU or CLAP effects: Task 2 and Rulings 5–6;
     - the fallback to Multi-Q with a note: Task 1 and Rulings 3–4;
     - the command and its hostile tests: Task 2.
     - The count is 181, not 180, because item 3 adds one too (Ruling 7).
   - **Item 2:**
     - new buses: Task 1 (`fill_new_eqs`);
     - the master of new projects: Task 1 and Ruling 2;
     - existing projects unchanged: `existing_projects_keep_their_strips`;
     - the override of spec §8 and of 6g's exclusions: Task 1 and the backlog note;
     - the InstrumentOutput call: Ruling 1.
   - **Item 3:**
     - "New Bus" always in the send menu: Ruling 8 and Task 3;
     - shared by the arrange window and the mixer: the one `SendKnobRow`, and README 323;
     - one undo step: `new_bus_from_a_strip_is_one_step`;
     - the ＋ when there are no sends: Task 3 Step 5;
     - named like Add Bus: `nextBusName`;
     - the default EQ: `new_bus_gets_the_default_eq`;
     - the command and its hostile tests: Task 3.
2. **Steps.** Every test step names its tests and assertions with the plan's values: IDs, names, sentences and counts. Code steps give signatures and point to the ruling that fixes the behaviour.
3. **Type consistency.** These names are spelled the same in every task:
   - `DefaultEqRow`, `set_default_eq`, `default_eq`, `default_eq_note`, `fallback_note`;
   - `History::set_new_strip_eq_note`;
   - `new_bus_from`, which Swift sees as `newBusFrom(strip:name:)`;
   - `MixerModel.nextBusName`, `newBus(from:)`, `MixerCommands.newBus`;
   - `PluginSettings.eqKey`, `defaultEq`, `eqChoices`, `eqNote`, `refreshEqChoices`;
   - `plugins.default_eq`, `mixer.new_bus`.
4. **Review Focus.** Each of the five lines names the tests that pin it, in Tasks 1–3.
5. **Proportion.** Four tasks at the owner's lighter pace. Code appears only as signatures. The plan reuses 6g's arm and fill, `add_lane_monitored`'s one-step pattern, the catalog browser, and the existing settings and command patterns.
6. **Owner decisions.** None are open. The InstrumentOutput call (Ruling 1) and the count of 181 (Ruling 7) are stated for the owner's review.
