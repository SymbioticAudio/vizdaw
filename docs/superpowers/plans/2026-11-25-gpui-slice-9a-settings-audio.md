# 27.1.0 Slice 9a: The Settings Window, with Audio, Recording and Automation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The GPUI app gets its Settings window (VizDAW › Settings…, the nine tabs in Swift's order) with the Audio, Recording and Automation tabs working as in the Swift app. Their settings are saved in the preferences, handed to the core at launch, and changed through the core first (saved only once the core took them). New documents start from the Recording tab's record defaults. `audio.set` works over the control channel, so the `audio.set` steps of `gpui_arms_meters_counts_in_and_draws_the_take` are restored. The other six tabs show a placeholder line until their slices (9b Appearance; 9c Keys, Editing, Plug-ins and the Customize Transport Bar sheet; later Surfaces and Co-Producer).

**Architecture:**
- `vizdaw-app` gets the settings models:
  - `audio_prefs.rs` ports Swift `M/AudioPreferences.swift` and `M/AudioReadouts.swift`.
  - `automation_prefs.rs` ports Swift `M/AutomationPreferences.swift`.
  - Both sit over `Prefs` under the keys `audio` and `automation`. Swift used `VizDAW.audio` and `VizDAW.automation`; the Rust keys drop the prefix, as `list` and `reserveNumberRow` already do.
  - They reach the core through new `CoreCalls` methods, so `FakeCore` can record and refuse them.
- `audio.set` runs in `vizdaw-app` (`audio_prefs::run_audio_set`) and is dispatched as an app-wide command in GPUI.
- In GPUI, `settings_window.rs` holds the one Settings window: the tab strip, the tab bodies, and a 1 Hz round-trip readout read off the UI thread. `AppState` holds the window handle outside `windows` (it belongs to no document; Swift's `state.get windows` never listed Settings either).

**Tech Stack:** Rust 2024; GPUI at the slice 1 pin; `core-ffi`, `core-control`, `vizctl`.

**Spec:** `docs/superpowers/specs/2026-11-20-cross-platform-27-1-design.md`.
- Parity rows in `docs/superpowers/specs/2026-11-22-parity-checklist.md`: "Settings: Audio", "Settings: Recording" and "Settings: Automation" under Windows; `audio.set` under Audio; the e2e row `arms_meters_counts_in_and_draws_the_take`.
- Swift reference with exact strings and file:line: the settings inventory at `.superpowers/sdd/settings-inventory.md`, §0, §1.4–1.6, §3 (`audio.set`), §4 and §5 (`AudioPreferencesTests`, `AudioReadoutsTests`, `AudioCommandsTests`, `AutomationPreferencesTests`).
- Swift sources, still in the tree for reading:
  - `app/Sources/VizDAW/{AudioSettingsView,AutomationSettingsView,AudioSetup,AutomationSetup,VizDAWApp}.swift`
  - `app/Sources/VizDAWAppModel/{AudioPreferences,AudioReadouts,AudioCommands,AutomationPreferences,AutomationModes,RecordOptions,DocumentStore}.swift`
  - The tests in `app/Tests/VizDAWAppModelTests/`.

## Global Constraints

- **Slice 7a–8c rules that still apply:**
  - The Swift app is retired: match its behaviour; don't keep compatibility with it.
  - Use Swift's exact strings, citing the inventory's file:line.
  - The UI says "track", never "lane".
  - No banned vendor or DAW product names in repo text.
  - Nothing slow on the UI thread.
  - GPUI tests press `secondary-` keys.
  - Linux CI runs the GPUI tests and the e2e on the offline backend (`VIZDAW_AUDIO=offline`, `VIZDAW_AUDIO_INPUT=fake`), and every 9a test must pass there.
- **Running the e2e on the Mac:** `cargo build -p vizdaw-gpui`, then `VIZDAW_E2E_APP=target/debug/vizdaw-gpui VIZDAW_AUDIO=offline cargo test -p vizctl --test e2e_gpui -- --ignored <name>`.
- **Core first, save after (Swift `AudioPreferences`).** Every change goes to the core first. It is saved only if the core took it. On a refusal the old value stays, and the tab shows the refusal in red at its bottom (the tab's `problem` line).
- **Tolerant decode.** A damaged or missing value keeps that field's default; one bad field does not drop the others. The JSON field names are Swift's: `softwareMonitoring, bufferFrames, inputDevice, inputDeviceUID, latencyTrim, cacheMb, readAheadSeconds, defaultMonitor, countInBars, preRollBars, midiPassesAsTakes, bounceTailSeconds`, and `afterWrite, returnMs, thinning`.
  - An old name-only `inputDevice` selects the device by name (Swift `AudioPreferencesTests:93`).
  - The old `passesAsTakes` key is ignored (`:150`).
- **Defaults (Swift):**
  - Software Monitoring on; buffer 128 frames; trim 0; bounce tail limit 10 s; RAM cache 512 MB; read-ahead 3 s; input device "Same as output".
  - Count-in off, pre-roll off, MIDI passes as takes off, monitoring for new tracks Auto.
  - After Write Latch, return time 250 ms, thinning Medium.
- **Launch.** `apply_all` hands every saved audio and automation setting to the core before the first document opens (Swift `VizDAWApp.swift:217`, `AudioPreferencesTests:32`, `AutomationPreferencesTests:22`). Tests and the e2e never write the real preferences: `vizctl launch` already uses a throwaway home, and unit tests use `Prefs::in_memory()`.
- **Linux.**
  - The input-device picker lists only "Same as output" off macOS, because `audio_input_device_list()` answers empty. This is expected until host track H3.
  - The Settings window opens from the in-window menu bar's VizDAW menu with Ctrl-, (`secondary-,`).
  - No microphone text appears off macOS.
- **The Settings window.**
  - One per app. Settings… while it is open brings it to the front on the tab it was on.
  - Title "Settings", the tab's label in the strip, 500 pt wide (Swift's Form width), height to fit the tab.
  - It never takes a document's keys: Space in it does not play.
  - Closing it with ⌘W or the close button closes only it.
  - A tab not built yet shows `<Tab> comes in a later version.`, the wording `panes.rs:776` already uses.
- **Look.** Use the existing controls (`controls/{toggle,menu_face,number_box}.rs`) and the slice's current palette (`strip_palette::tokens()`). No new colour literals: 9b replaces the palette with the theme table. Screenshots go to the owner only when a tab resembles Swift's Form.
- **Review pace:** fix crashes, data loss and wrong overwrites; log other edge cases to the backlog.

## Review Focus

1. **Core first.** A refused core call, such as a buffer size while recording (`Busy`) or a bad automation return time, leaves the saved preference and the shown value as they were, and shows the refusal. A write failure in `Prefs` after the core took the value shows the write problem and does not roll back the core. This is Swift's behaviour: the core runs on the new value for this run.
2. **Launch order.** `apply_all` runs before the first document's session is made, so the first engine opens with the saved buffer size, stream sizes and monitoring. The first new document gets the record defaults.
3. **`audio.set` arguments.**
   - No known parameter, an unknown one, or a buffer size off the list (100) answers `invalid_params` with Swift's message, unchanged.
   - Two devices sharing a name answer `failed` with Swift's message.
   - `""` is "Same as output".
   - Several parameters in one call apply in Swift's order. The first refusal stops the call, and the ones already applied stay applied and saved, as in Swift `AudioCommands.swift:57-114`; check that file and match it.
4. **The round-trip readout** reads `input_info` off the UI thread, only while the Audio tab is shown, from the first document with an open input. With no document, or with none open, it shows the "About … at 48 kHz" text. Closing the window stops the timer.
5. **An absent device.** A saved device that is not connected shows `<name> (not connected)` and stays chosen. Two devices with one name show `<name> (2)` and stay apart by UID.

---

### Task 1: The core seam and the audio settings model

**Files:**
- Create: `crates/vizdaw-app/src/audio_prefs.rs`
- Modify: `crates/vizdaw-app/src/{session.rs,lib.rs,testing.rs}`
- Test: `crates/vizdaw-app/tests/audio_prefs.rs`

**Interfaces:**
- **Produces, on `CoreCalls`.** Add these default methods calling `vizdaw_ffi`:
  - `set_software_monitoring(on: bool)`
  - `set_io_buffer_frames(frames: u32) -> Result<(), VizError>`
  - `set_stream_settings(cache_mb: u32, read_ahead_seconds: u32) -> Result<(), VizError>`
  - `set_default_monitor_mode(mode: MonitorModeRow)`
  - `set_audio_input_device(name: Option<String>)`
  - `set_record_latency_trim(frames: i32)`
  - `set_bounce_settings(row: BounceSettingsRow)`
  - `audio_input_device_list() -> Vec<InputDeviceRow>`
  - `set_automation_settings(row: AutomationSettingsRow) -> Result<(), VizError>`, used by Task 2.
- **Produces, on `FakeCore`.** It records each call in `calls()` as `name(arg)`, and gets `refuse_next(name, VizError)` and `set_input_devices(Vec<InputDeviceRow>)`.
- **Produces, in `audio_prefs`:**
  - `pub struct AudioSettings`, with Swift's fields and defaults, serde using Swift's names.
  - `pub struct AudioPrefs`, which holds an `AudioSettings` and the last `problem: Option<String>`, with:
    - `AudioPrefs::load(&Prefs) -> AudioPrefs` (tolerant).
    - `apply_all(&self, &dyn CoreCalls)`.
    - One setter per control: `set_software_monitoring`, `set_buffer_frames`, `set_input_device(Option<InputDeviceRow>)`, `set_latency_trim`, `set_bounce_tail_seconds`, `set_cache_mb`, `set_read_ahead_seconds`, `set_default_monitor`, `set_count_in_bars`, `set_pre_roll_bars`, `set_midi_passes_as_takes`. Each is `(&mut self, value, &mut Prefs, &dyn CoreCalls) -> Result<(), String>`, follows "core first, save after", and sets or clears `problem`.
    - `record_defaults(&self) -> RecordDefaults`.
  - The readouts as pure functions, with Swift's strings from inventory §1.4:
    - `device_choices(&AudioSettings, &[InputDeviceRow]) -> Vec<DeviceChoice>`, covering "Same as output", the `(2)` duplicates, and `(not connected)`.
    - `round_trip_text(Option<&InputInfoRow>, buffer_frames) -> String`.
    - `trim_text(frames, rate) -> String`.
    - `cache_choices(current) -> Vec<u32>`, which keeps an off-list current value.
    - `count_text(n) -> String`, giving "off", "1 bar" or "n bars".
  - `BUFFER_SIZES` comes from the core (`[32, 64, 128, 256, 512, 1024]`).
  - `DISK_TEXT` is "Recording warns below 1 GB of free disk space and stops below 100 MB."

- [ ] **Step 1: Write the failing tests** in `tests/audio_prefs.rs`. Port `AudioPreferencesTests` (:32, :45, :93, :106, :121, :134, :150) and `AudioReadoutsTests` (:9, :32, :40, :55, :74):
  - `launch_hands_every_setting_to_the_core`: `FakeCore` `calls()` lists all eight setters with the loaded values.
  - `a_setting_is_kept_only_once_the_core_took_it`: `refuse_next("set_io_buffer_frames", Busy)`, then `set_buffer_frames(256)` is `Err`. Both `settings().buffer_frames` and the stored JSON stay 128, and `problem` is set.
  - `an_old_name_only_device_selects_by_name`
  - `the_uid_is_kept`
  - `a_damaged_field_keeps_its_default_and_the_others`
  - `record_defaults_come_from_the_settings`
  - `midi_passes_as_takes_defaults_off_and_the_old_key_is_ignored`
  - `round_trip_before_and_after_the_device_opens`, with both Swift texts verbatim.
  - `the_cache_picker_keeps_an_off_list_value`
  - `a_disconnected_choice_is_kept`
  - `two_same_named_devices_stay_apart`
  - `trim_and_count_texts`, covering `"+12 frames (0.25 ms)"`, "off", "1 bar" and "3 bars".
- [ ] **Step 2:** Run `cargo test -p vizdaw-app --test audio_prefs`. Expected: it fails to compile (no `audio_prefs`).
- [ ] **Step 3:** Implement the `CoreCalls` methods, the `FakeCore` recording, and `audio_prefs.rs`.
- [ ] **Step 4:** Run `cargo test -p vizdaw-app` and `cargo clippy -p vizdaw-app --all-targets -- -D warnings`. Expected: all pass, no warnings.
- [ ] **Step 5:** Commit `app: audio settings model over the preferences, core first`.

### Task 2: The automation settings model

**Files:**
- Create: `crates/vizdaw-app/src/automation_prefs.rs`
- Test: `crates/vizdaw-app/tests/automation_prefs.rs`

**Interfaces:**
- **Consumes:** `CoreCalls::set_automation_settings` (Task 1).
- **Produces:**
  - `pub struct AutomationPrefs { settings: AutomationSettingsRow, problem: Option<String> }`, with:
    - `load(&Prefs)`.
    - `apply_all(&dyn CoreCalls)`.
    - `set_after_write`, `set_return_ms` and `set_thinning`. These follow the same core-first rule as Task 1; the key is `automation` and the JSON is `{afterWrite, returnMs, thinning}`, in Swift's encodings.
  - `AFTER_WRITE_CHOICES`: ("Off", "Read", "Touch", "Latch", "Trim"), never Write.
  - The texts:
    - The return-time label `"Return time: <ms> ms"`.
    - `THINNING_HELP`, `PARAMS_ONLY_LABEL` and `PARAMS_ONLY_HELP`.
    - `APPLIES_TEXT`: "Changes apply from the next Play."
  - The return time ranges over 0…2000 in steps of 50.

- [ ] **Step 1: Write the failing tests:**
  - `launch_hands_the_settings_to_the_core` (Swift `AutomationPreferencesTests:22`).
  - `kept_only_once_the_core_took_them` (:30), with a refused return time.
  - `damaged_settings_keep_defaults` (:54).
  - `after_write_never_offers_write`.
- [ ] **Step 2:** Run `cargo test -p vizdaw-app --test automation_prefs`. Expected: it fails to compile.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run `cargo test -p vizdaw-app` and clippy. Expected: all pass.
- [ ] **Step 5:** Commit `app: automation settings model`.

### Task 3: Launch wiring, record defaults, and `audio.set`

**Files:**
- Modify: `crates/vizdaw-app/src/{audio_prefs.rs,store.rs,commands.rs}`, `crates/vizdaw-gpui/src/{app_state.rs,lib.rs,control.rs}`, `crates/vizctl/tests/e2e_gpui.rs`
- Test: `crates/vizdaw-app/tests/audio_prefs.rs`, `crates/vizdaw-gpui/tests/control.rs`

**Interfaces:**
- **Consumes:** Tasks 1–2.
- **Produces:**
  - `AppState.audio: AudioPrefs` and `AppState.automation: AutomationPrefs`.
  - `AppState::set_prefs` loads both, runs both `apply_all` against `store.core()`, and gives the store `set_record_defaults(RecordDefaults)`.
  - Every document the store makes from then on (new, demo, opened) gets `apply_record_defaults` before its first window. Read Swift `DocumentStore.swift` to see whether opened documents get them as well, and match it.
  - `audio_prefs::run_audio_set(args: &[(String, ArgValue)], audio: &mut AudioPrefs, prefs: &mut Prefs, core: &dyn CoreCalls) -> Result<Ran, ControlError>`, with:
    - Swift's checks, order and messages (inventory §3, `AudioCommands.swift:57-114`).
    - `""` for "Same as output".
    - Matching a device by uid, or by a unique name.
    - The two-name `failed` message, verbatim.
    - A core refusal answered as `Busy` → `ErrorCode::Busy`, anything else → `failed`, with the message `alerts::error_message` gives.
  - `"audio.set"` joins `APP_COMMANDS`, and GPUI dispatches it beside `run_app`, with no document needed.

- [ ] **Step 1: Write the failing tests:**
  - In vizdaw-app:
    - `audio_set_value_checks`, which ports `AudioCommandsTests:32`: 256 is ok, while 100, `{junk: 1}`, `{}` and a trim beyond ±192000 are each `invalid_params` with Swift's message.
    - `an_ambiguous_device_name_is_refused` (:71).
    - `a_busy_core_answers_busy_and_keeps_the_value`.
    - `new_documents_start_from_the_record_defaults`: the store with count-in 2 makes a document whose record model has count-in 2.
  - In `vizdaw-gpui/tests/control.rs`: `audio_set_runs_without_a_document`, and `audio_set_saves_the_buffer_size`, checked through `AppState.prefs`.
- [ ] **Step 2:** Run `cargo test -p vizdaw-app --test audio_prefs` and `cargo test -p vizdaw-gpui --test control audio_set`. Expected: both fail (no `run_audio_set`, and the command answers `unavailable`).
- [ ] **Step 3:** Implement. Call `set_prefs` in `lib.rs::run` (`:332`) before any document opens, as now. Remove `audio.set` from the `not_yet` list in `control.rs`.
- [ ] **Step 4: Restore the e2e steps** in `gpui_arms_meters_counts_in_and_draws_the_take`, replacing the comment at `e2e_gpui.rs:3664-3665` with Swift's steps (`e2e.rs:1483-1507` and `:1536-1544`):
  - `buffer_frames 256` gives `state.get input` `buffer_frames 256` and `device_buffer_frames 512`.
  - 100 and `{junk: 1}` are `invalid_params`.
  - 512 while recording is `busy`, and the buffer size stays 256.
- [ ] **Step 5:** Run `cargo test -p vizdaw-app -p vizdaw-gpui`, clippy, and the e2e test on the Mac (env in Global Constraints). Expected: all green.
- [ ] **Step 6:** Commit `settings: hand audio and automation settings to the core at launch; audio.set`.

### Task 4: The Settings window and its tab strip

**Files:**
- Create: `crates/vizdaw-gpui/src/settings_window.rs`
- Modify: `crates/vizdaw-gpui/src/{app_state.rs,menus.rs,lib.rs,menu_bar.rs}`, `crates/core-control/src/registry.rs` (only if `app.settings` is not already a registry id; check first)
- Test: `crates/vizdaw-gpui/tests/settings_window.rs`

**Interfaces:**
- **Produces:**
  - `pub enum SettingsTab { Appearance, Editing, Keys, Audio, Recording, Automation, Surfaces, Plugins, CoProducer }`, with `label()` giving Swift's labels (`VizDAWApp.swift:269-285`): "Appearance", "Editing", "Keys", "Audio", "Recording", "Automation", "Surfaces", "Plug-ins", "Co-Producer".
  - `pub fn open_settings(tab: Option<SettingsTab>, cx: &mut App)`, which opens the window or brings it to the front.
  - `AppState.settings_window: Option<WindowHandle<SettingsWindow>>`, cleared when the window closes.
  - The VizDAW menu reads "Settings…" (`secondary-,`), then a separator, then Quit.
  - The tab body is one function per tab. Tasks 5–6 fill Audio, Recording and Automation; the rest show the placeholder line.
  - The window keeps its own registry, so `ui.tree` and `ui.click` reach it once a later slice lists it. In this slice, tests drive it through `VisualTestContext`.
- Before adding a registry id, check Swift's app menu item for its exact title and whether the registry already has an id for it.

- [ ] **Step 1: Write the failing tests:**
  - `settings_opens_once_and_comes_to_the_front`
  - `the_tabs_are_swifts_nine_in_order`
  - `an_unbuilt_tab_says_it_comes_later`, with the exact line for Keys.
  - `space_in_settings_does_not_play`
  - `closing_settings_leaves_the_documents`
  - `the_vizdaw_menu_lists_settings_with_its_key`
- [ ] **Step 2:** Run `cargo test -p vizdaw-gpui --test settings_window`. Expected: it fails to compile.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run `cargo test -p vizdaw-gpui` and clippy. Expected: green.
- [ ] **Step 5:** Commit `gpui: the Settings window and its tabs`.

### Task 5: The Audio tab

**Files:**
- Modify: `crates/vizdaw-gpui/src/settings_window.rs`; create `controls/stepper.rs` if no stepper exists
- Test: `crates/vizdaw-gpui/tests/settings_window.rs`

**Interfaces:**
- **Consumes:** `AudioPrefs` and the readouts (Task 1); `AppState.audio` (Task 3).
- **Produces:** The Audio tab, laid out as in inventory §1.4:
  - **Input:** the device picker; the caption "A new device takes effect at once; while recording or measuring, once that stops."; the Software Monitoring toggle and its caption. Changing the toggle refreshes every document's input monitoring, as Swift `:35` does; find the GPUI equivalent in `LaneInputs`.
  - **Latency:** the I/O Buffer Size picker (`"<n> frames"`); the round-trip caption; Latency trim, a stepper with its value text; Reset Trim, disabled at 0.
  - **Bounce:** the tail limit stepper (0…30) and its caption.
  - **Streaming:** the RAM cache picker (`"N MB"`/`"N GB"`), the read-ahead stepper (2…8) and the caption.
  - The red `problem` line at the bottom.
- **Reads off the UI thread:**
  - The device list is read when the tab opens and when the picker opens.
  - `input_info` is read at 1 Hz while the tab shows, from the first document whose input is open (Swift `inputHolder`). Use the 7a generation-counter pattern, and render reads only held values.
- **The stepper**, if it is new: a label, then − and + buttons. It is clamped to its range, keeps its step, and the button at each end is disabled.

- [ ] **Step 1: Write the failing tests:**
  - `the_audio_tab_shows_swifts_rows_and_captions`, checking the strings.
  - `choosing_a_buffer_size_goes_to_the_core_then_the_prefs`
  - `a_refused_buffer_size_shows_the_problem_and_keeps_the_choice`
  - `an_absent_device_shows_not_connected`
  - `reset_trim_is_off_at_zero`
  - `the_round_trip_is_read_off_the_ui_thread_only_while_shown`, using a `FakeSession` whose `input_info` blocks: render never waits, and closing the window stops the reads.
- [ ] **Step 2:** Run them. Expected: they fail.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run `cargo test -p vizdaw-gpui` and clippy. Expected: green.
- [ ] **Step 5:** Commit `gpui: Settings › Audio`.

### Task 6: The Recording and Automation tabs

**Files:**
- Modify: `crates/vizdaw-gpui/src/settings_window.rs`
- Test: `crates/vizdaw-gpui/tests/settings_window.rs`

**Interfaces:**
- **Consumes:** Tasks 1–3, `AppState::remember_default_format`, `format_panel.rs` (the rate and depth choices), and `CoreCalls::project_rates`.
- **Produces:**
  - **The Recording tab** (inventory §1.5):
    - **New Projects:** the Sample rate picker (`"48 kHz"`, `"44.1 kHz"` and the current one) and the Bit depth picker ("16-bit", "24-bit", "32-bit float"), both writing `defaultProjectFormat`.
    - **New Documents:** the Count-in and Pre-roll steppers with `count_text`; the "MIDI passes as takes" toggle and its help; the "Monitoring for new tracks" picker (Auto / Input / Off) and its caption.
    - **Disk:** `DISK_TEXT`.
  - **The Automation tab** (inventory §1.6):
    - The After Write picker and the Return time stepper.
    - The Thinning picker and its caption.
    - The disabled, always-on toggle and its caption.
    - "Changes apply from the next Play."
    - The problem line.

- [ ] **Step 1: Write the failing tests:**
  - `the_recording_tab_shows_swifts_rows`
  - `changing_the_format_sets_the_next_new_project`, checked with File › New afterwards.
  - `count_in_applies_to_the_next_new_document_not_open_ones`, which also checks Swift's caption.
  - `the_automation_tab_shows_swifts_rows`
  - `after_write_never_offers_write`
  - `a_refused_automation_change_shows_the_problem`
- [ ] **Step 2:** Run them. Expected: they fail.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run `cargo test -p vizdaw-app -p vizdaw-gpui` and clippy. Expected: green.
- [ ] **Step 5:** Commit `gpui: Settings › Recording and › Automation`.

### Task 7: Checks, checklist and backlog

**Files:** `docs/testing/controller-checks.md`, `docs/testing/manual-checks.md`, `docs/superpowers/specs/2026-11-22-parity-checklist.md`, `docs/feature-backlog.md`

**Produces:**
- **`controller-checks.md`:** a "Settings window (9a)" section covering:
  - Opening from the menu and with ⌘,.
  - The tab strip.
  - Each tab's layout against Swift's Form.
  - Steppers at their ends.
  - The red problem line while recording.
  - The round trip changing once the device opens.
- **`manual-checks.md`:**
  - **macOS:** a real interface in the Audio tab's picker (its name, then `(not connected)` after unplugging), and a buffer-size change heard without a glitch.
  - **Linux:** the Settings window from the in-window menu with Ctrl-,, and its tabs drawn with the system font.
- **Parity checklist ticks:**
  - The Settings: Audio, Recording and Automation rows (S9a, on Mac and Linux).
  - `audio.set` (S9a).
  - The `arms_meters_counts_in_and_draws_the_take` row, whose `audio.set` note goes.
- **Backlog:** "GPUI slice 9a: review leftovers"; and that Linux live device choice waits for H3.
- [ ] Commit `docs: slice 9a checks, checklist and backlog`.

## Outcome (2026-11-25)

All seven tasks done (commits on `next` from 80064666 to the Outcome commit). The GPUI app has its Settings window (VizDAW › Settings…, ⌘,/Ctrl-,; Swift's nine tabs, six of them "… comes in a later version."). The Audio, Recording and Automation tabs work as in Swift. Saved audio and automation settings reach the core at launch, every change goes to the core before it is saved, and documents start from the record defaults. `audio.set` works over the control channel, and `gpui_arms_meters_counts_in_and_draws_the_take` runs its `audio.set` steps again. Evidence: vizdaw-app and vizdaw-gpui suites green on the Mac at each task, including `audio_prefs` 19, `automation_prefs` 5 and `settings_window` 24; the e2e green on the Mac; the controller opened the window on screen and checked each built tab's layout. Final whole-branch review: ready to merge, no Critical or Important findings. It found the README's Rust floor wrong (let-chains need 1.88), which is now fixed in the README. `Cargo.toml` keeps `rust-version = "1.86"` for now: raising it turns on clippy's let-chain lint across the workspace (backlog row "Declared Rust floor").

### Rulings

- Pre-flight: commit on `next` directly, as earlier slices did. Cost: none.
- Task 1 (implementer, accepted): `apply_all` makes Swift's seven core calls (automation is Task 2); `apply_at_launch` stores the refusal; setters clear the problem on success; an unknown default monitor is a no-op (Swift's guard).
- Task 2 (implementer, accepted, Swift parity): the return time clamps to 2000; an unknown or Write after-write and an unknown thinning are no-ops; a saved "write" or a value over 2000 decodes to that field's default.
- Task 3 (implementer, accepted, Swift parity): opened documents get the record defaults too; wrong-typed `audio.set` values are ignored, and the call is `invalid_params` only if no known value remains; it saves after each step (Swift saved once at the end), with the same result.
- Task 4: a menu-local `app.settings` id with no registry id (Swift's item was the system's own); the Settings window is kept outside `windows` and is not in `state.get windows`. Cost if wrong: the channel cannot see or click the window, so a later slice adds it.
- Task 5 (implementer, accepted): reads run on a plain thread, and a 20 ms timer takes the answer while a read is out. GPUI's test scheduler would block on a held fake read on the background executor. Also added: `DocumentStore::refresh_input_monitoring`/`sessions` and `controls/stepper.rs`.
- Task 6 (implementer, accepted): `cache_text` shows "N GB" only for multiples of 1024. Swift's `mb / 1024` showed 1536 MB as "1 GB".

### Carried

- The Linux cells (Settings tabs, `audio.set`, the arms-meters e2e) are ticked once Linux CI passes on this slice.
- On screen, still open in `controller-checks.md`: the red problem line while recording (check that it is not clipped), the round trip once an input opens, and the steppers at their ends.

### Deferred minors

Everything else is in the backlog row "GPUI slice 9a: review leftovers".
