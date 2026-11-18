# 26.0.0 Beta: Surfaces, Small UI and Takes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the control-surface, small-UI and takes rows the owner pulled into the 26.0.0 beta on 2026-11-18. Where a row needs the owner's keyboard or a vendor fact the repo does not hold, build the part that can be tested and leave a README check.

**Architecture:** Most surface work is in `crates/core-surface` (the pure Kontrol and MCU state machines and the learn mapper) and in `crates/core-ffi/src/surfaces.rs`/`session.rs`, which drive them. There is one new core-owned preferences file, `plugin-pages.json`, beside `surfaces.json`. The UI rows are small changes in `VizDAWAppModel`, each with a pure, tested decision, plus thin view wiring in `VizDAW`. The takes rows touch the Takes menu, the record defaults and the recording pass. The project file format does not change; two new optional fields (`LearnMap.feedback` and `ui.json`'s `audioEditor`) are written only when they are set.

**Tech Stack:** Rust 2024 (workspace, uniffi FFI), Swift 6.1 / SwiftUI + AppKit (macOS 14+), Swift Testing.

**Spec:** There is no separate design document. Each task's spec is the Notes of its rows in `docs/feature-backlog.md` (lines 58, 59, 61–71, 73, 80–82, 84, 90, 98, 105 and 293–295), with the owner's instructions of 2026-11-18. "Comping" is left out because it needs its own design. Rulings made here without a spec are listed under Rulings as provisional.

## Global Constraints

- Version: `26.0.0` everywhere a version is shown or written; this build ships as the 26.0.0 beta.
- Co-Producer: in 26.0.0 Co-Producer never writes MIDI notes, on any track, its own included (owner, 2026-11-16). Changing or deleting the composer's work, routing or sound needs an approval-sheet yes. A Co-Producer bus's own send going to −∞ or being retargeted needs no yes (owner, 2026-11-18).
- Programmatic and control-channel calls never open dialogs. The only exceptions are the import's new-project window, Co-Producer's approval sheet (only for a pending request) and the starter project's one new song window. The save prompts are user-only.
- New commands get hostile-input tests. New FFI goes inside `ffi_guard`.
- Swift: build and test only through `scripts/swift.sh`, `scripts/build-ffi.sh` and `scripts/make-app.sh`. Swift 6.1 rules:
  - no `weak self` or non-Sendable values inside `MainActor.assumeIsolated`;
  - no `@State` or `@FocusState`;
  - AppKit main-actor globals (`NSApp` and friends) only from `@MainActor` code; CI's Xcode 16.4 refuses other uses.
- UI: accessibility labels and theme tokens on every control. The UI says "track", never "lane".
- Tests:
  - never open real audio or MIDI devices, and never trigger mic/TCC prompts;
  - never touch the real `~/Library` (Application Support/VizDAW, prefs, Trash, Audio/Plug-Ins, Keychain) or the real UserDefaults;
  - Swift tests that make a real Session set `TestSupportDir.shared`;
  - the e2e uses vizctl's throwaway home and `VIZDAW_TEST_HOOKS=1`;
  - on Linux CI, AU/CF/importer tests are macOS-gated.
- Never run `auval`. No GUI or computer-use tools. Never use dangerouslyDisableSandbox.
- Names:
  - banned vendor and DAW names appear only in the name scans (`app/Tests/VizDAWAppModelTests/NameScanTests.swift`, `crates/core-control/tests/names.rs`) and in licence notices;
  - the reference DAW's repo name stays out of repo text.
- Commits:
  - by explicit paths;
  - never commit `.DS_Store`, `.superpowers/` or generated FFI files;
  - brand assets are never committed;
  - each commit ends with the two trailer lines `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>` and `Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P`.
- Env: `export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools`.
- Linux clippy: `cargo clippy --workspace --all-targets --target x86_64-unknown-linux-gnu --target-dir target/linux-check -- -D warnings`.
- Pace (owner): fix crashes, data loss, wrong overwrites, security holes and rule violations. Narrow edge cases go to the backlog.

The name rule in practice: Rust string literals and Swift string literals are scanned, so problem lines and UI copy say "host-integration keyboard" and "surface", never the product name. Doc comments and docs may use "Kontrol MK3", as the backlog does.

## Review Focus

1. **A newer build's `surfaces.json` must survive a save by this build.** Its unknown commands and unknown fields (on surfaces, on commands and at the top level) must still be there afterwards. A file that is actually broken still goes to `.bad`. (Task 8: `a_newer_files_unknown_commands_and_fields_survive_a_save`.)
2. **The user's own tempo turns must be acted on, and VizDAW's echoes must never become edits.** That holds for a turn back inside the 500 ms window, and for an echo that arrives in the same batch as a turn. (Task 1: `a_turn_back_inside_the_echo_window_is_acted_on`, `an_echo_drops_only_its_entry_and_older`.)
3. **A wheel run over a knob or fader is one undo step, and it always ends.** It ends after the idle time, when a mouse button goes down, and when the view leaves its window. (Task 12: `aWheelRunEndsOnAMousePressOrWhenTheViewGoes`.)
4. **A saved audio-editor zoom never shows an empty or out-of-file view.** That covers a hand-edited or stale `ui.json` (NaN, negative or huge values) and a region trimmed shorter since the save. (Task 13: `aSavedViewIsClampedToTheRegionAsItIsNow`.)
5. **Replace after an undo during recording lands in the take that was playing when Record was pressed.** It never clears another take's notes. (Task 16: `replace_lands_in_the_take_that_was_playing_at_record`.)

## Rulings (provisional unless marked owner-confirmed, for the owner's review of this plan)

- **R1 (rows 61, 62).** The NKS hand-off and the Quantize and Clear buttons are not built:
  - The NKS hand-off needs the keyboard maker's NKS protocol, which is not public and not in the repo.
  - Quantize stays dark: its only key command opens a sheet, and `modify.quantize_notes` needs explicit tracks and a grid.
  - Clear stays dark: it still has no counterpart.

  Metro is built in Task 2 (owner, 2026-11-18): it runs `transport.click`, the metronome that the groups/VCAs/transport plan adds to 26.0.0 before this plan runs. Row 61 stays in the backlog with this note, and so does row 62's Quantize and Clear part.
- **R2 (row 66).** `TEMPO_RUN` (500 ms) and `TAP_RESET` (2 s) stay until the owner runs README 198. The plan found that taps are already stamped where the MIDI arrives (`enqueue` stamps `at` on CoreMIDI's thread, and `route` passes `item.at`), not on the 60 Hz idle. Task 1 goes further and uses the packet's host time when it has one.
- **R3 (rows 64, 67, 68).** These rows need facts only the owner's keyboard can give, so each one stops at the testable part:
  - Task 2 builds the keep-alive and the press-only track buttons as unit options. Both are off by default, so behaviour is unchanged.
  - Task 2 also adds a "heard" log to each surface's feedback row, which lets the owner see what the keyboard sends.
  - High-resolution knob deltas are not built: the format of the 0x7F delta messages is not in the repo, and the plan invents no bytes.
  - README steps tell the owner what to capture. A follow-up turns each option on, or decodes the deltas, from the capture.
- **R4 (row 63).** When the playhead is strictly inside a ramp (the point in effect has `ramp: true`, and a later point exists), the keyboard's first turn adds a point at the playhead. The new point has the new tempo and `ramp: true`, so the ramp still reaches the next point. The rest of that gesture edits the same point. On a constant segment, or exactly on a point's tick, it edits the point in effect, as today. **Confirmed by the owner (2026-11-18):** a tempo turn inside a ramp writes a tempo change as a point at the playhead.
- **R5 (row 73).** Page arrangements are app-wide preferences, one per plug-in (format and id). They live in a core-owned `plugin-pages.json` in the support folder. Keeping them out of `surfaces.json` means older builds never set that file aside.
- **R6 (row 98).** The wheel works on the strip's fader, pan knob and send knobs, and on the MIDI strip's fader and pan knob. A notched wheel moves 0.01 per notch; a trackpad moves 0.002 per point. ⌥ moves a tenth of that. Scrolling up raises the value, whatever the scroll-direction setting. A run ends 0.4 s after the last event. Mostly-horizontal scrolls pass through, so the mixer still scrolls sideways.
- **R7 (row 105).** The zoom is saved per region, and only the 64 most recently shown regions are kept.
- **R8 (row 294).** "New documents" means a session with no saved UI layout: File › New, the demo and the starter project, and also a project from a build that never wrote `ui.json`.
- **R9 (row 90).** The Plug-ins pane is a list, not a popover. Its pick goes straight to the strip and never touches the mixer's shared popover state. Popovers keep one shared state, because only one can be open.
- **R10 (row 81).** A banner warns when a project opens with a keyboard slot that a command shadows. Settings › Keys is left as it is.
- **R11 (row 295, item 9).** The spec bullet stays as it is: it is the last item of its list, so its "." is correct.

---

### Task 1: Kontrol tempo echoes and tap times (rows 80, 66)

**Files:**
- Modify: `crates/core-surface/src/kontrol.rs` (`Kontrol::tempo`, ~line 486)
- Modify: `crates/core-ffi/src/surfaces.rs` (`enqueue`, ~line 2134)
- Test: `crates/core-surface/tests/kontrol_input.rs`, the `surfaces.rs` unit tests, `crates/core-ffi/tests/kontrol.rs`

**Interfaces:**
- Produces: `fn stamp(at: Instant, host_now_ns: u64, host_ns: u64) -> Instant` in `surfaces.rs` (private).

- [ ] **Step 1: Write the failing tests.**
  - `a_turn_back_inside_the_echo_window_is_acted_on` (kontrol_input.rs). Set `v.tempo = 121.0` and bring the keyboard online with `online(&v, 0)`, so VizDAW has sent 121. Then:
    - `msg(121)` at +10 ms gives nothing (the echo);
    - `msg(122)` at +50 gives `Tempo { bpm: heard(122), new_step: true }`;
    - set `v.tempo = heard(122)` and send feedback at +70;
    - `msg(122)` at +80 gives nothing;
    - `msg(121)` at +150 gives `Tempo { bpm: heard(121), new_step: false }`.
  - `an_echo_drops_only_its_entry_and_older`. Three feedbacks at +0, +20 and +40 with tempos 120, 121 and 122, then:
    - `msg(121)` at +50 gives nothing;
    - `msg(120)` at +60 is acted on (that entry is gone);
    - `msg(122)` at +70 gives nothing (that one is still remembered).
  - `stamp_uses_the_packet_time` (surfaces.rs unit test):
    - `stamp(at, 10_000_000_000, 9_900_000_000) == at - 100 ms`;
    - `host_ns == 0` gives `at`;
    - `host_ns > host_now` gives `at`;
    - a gap of 1 s or more gives `at`.
  - `taps_are_timed_where_they_arrive_not_at_idle` (core-ffi kontrol.rs). With `debug_freeze_clock(true)`, inject five taps (`BF 18 01`), with `debug_advance(500)` between them and no idle in between. Then run one `idle()`: the song's tempo at bar 1 reads 120.0.
- [ ] **Step 2: Confirm they fail.** Run `cargo test -p core-surface --test kontrol_input`, then `cargo test -p core-ffi stamp_uses` and `cargo test -p core-ffi --test kontrol taps_are_timed`. The first two fail. The third may already pass (Ruling R2); keep it as a pin.
- [ ] **Step 3: Implement.**
  - In `tempo`, find the newest `sent_tempos` entry that matches within `TEMPO_RUN`. When there is one, drain the queue through it (`drain(..=i)`) and return. Clear the `CMD_TEMPO` cache key only when the matched entry was not the newest, as today.
  - In `enqueue`, set `at = stamp(now(), crate::clock::now_ns(), host_ns)` when `host_ns != 0`.
- [ ] **Step 4: Confirm they pass.** Run `cargo test -p core-surface -p core-ffi --test kontrol`. The existing `a_late_echo_after_the_keyboards_own_tempo_acts_on_nothing` must still pass.
- [ ] **Step 5: Update the README.** Extend step 198 with: "Turn the tempo control up one step and straight back: the screen and VizDAW both end at the first value. Report whether 500 ms between turns and 2 s between tap series feel right." Name the new test in step 194's list.
- [ ] **Step 6: Commit** as `fix(kontrol): an echo drops older sent tempos, so a turn back is kept; taps use the packet time`.

### Task 2: Kontrol Metro, the hardware-dependent options and the heard log (rows 62, 64, 67, 68; Rulings R1, R3)

**Files:**
- Modify: `crates/core-surface/src/kontrol.rs` (fields, `control` ~559, `command` ~670, `answer` ~712, `send_state`, `feedback` ~961, `tick`), `crates/core-surface/src/lib.rs` (`MixerView.click`)
- Modify: `crates/core-ffi/src/session.rs` (`surface_view`: `click`)
- Modify: `crates/core-ffi/src/surfaces.rs` (`Unit` ~391, `feedback_row` ~441, `route` ~1327, `SurfaceFeedbackRow` ~182), `crates/core-ffi/src/control.rs:1854` (`"heard"`)
- Test: `crates/core-surface/tests/kontrol_input.rs`, `crates/core-ffi/tests/kontrol.rs`

**Interfaces:**
- Consumes, from the groups/VCAs/transport plan (`docs/superpowers/plans/2026-11-18-groups-vcas-transport.md`), which runs first: the registry command `transport.click` (a toggle) and the click's on/off state. Read that state wherever that plan keeps it: if it is core state, `surface_view` reads it directly; if it is the app's, the app passes it the way it passes count-in (`Session::set_surface_count_in`). `transport.set_click_level` and `transport.set_click_mode` are not used.
- Produces: `MixerView.click: bool`, which is true while the metronome is on.
- Produces, on `Kontrol`:
  - `pub fn set_press_only(&mut self, on: bool)`;
  - `pub fn set_keep_alive(&mut self, every: Option<Duration>)`;
  - `pub const KEEP_ALIVE_EVERY: Duration = Duration::from_secs(5)`.
- Produces: `SurfaceFeedbackRow.heard: Vec<String>`, the last 64 messages heard from the unit, in hex.

- [ ] **Step 1: Write the failing tests:**
  - `metro_toggles_the_click_and_lights_with_it` (kontrol_input.rs):
    - `BF 17 01` gives `[Command("transport.click")]`;
    - `BF 17 00` (the release) gives nothing;
    - feedback with `v.click = true` holds `BF 17 01`, and with `false`, `BF 17 00`;
    - Quantize (`BF 22`) and Clear (`BF 15`) still do nothing, are not counted, and stay dark.
  - `the_metro_button_reaches_the_app_as_a_command` (core-ffi kontrol.rs): inject `BF 17 01`; `take_surface_events` holds `Command { id: "transport.click", programmatic: true }`.
  - `track_buttons_act_on_every_message_by_default`: a mute SysEx (0x43) with value 1, then one with value 0, toggles twice. This pins today's behaviour.
  - `press_only_track_buttons_ignore_a_release`: after `set_press_only(true)`, the same pair toggles once; select, solo and arm with value 0 do nothing and are not counted as ignored.
  - `keep_alive_is_off_by_default`: online, feedback every 20 ms for 60 s sends no `BF 01 04`.
  - `a_keep_alive_answered_keeps_the_keyboard_online_without_a_refresh`, with `set_keep_alive(Some(KEEP_ALIVE_EVERY))`:
    - the first feedback at or after +5 s holds `BF 01 04`;
    - after the answer `BF 01 04`, the keyboard is still online, the next feedback holds no identity message (0x07) and nothing already sent is resent.
  - `a_keep_alive_unanswered_takes_it_offline_and_releases_its_knobs`: a volume knob is held when the keep-alive goes out. `tick` at hello + `HELLO_EVERY` answers `IdleRelease` for that fader, `online()` is false, and the mode is Mixer.
  - `the_feedback_row_lists_what_the_unit_sent` (core-ffi): inject `BF 10 01`; the focused session's `surface_state()` feedback row for the unit has `heard` ending in `midi::hex(&[0xBF, 0x10, 0x01])`.
- [ ] **Step 2: Confirm they fail.** Run `cargo test -p core-surface --test kontrol_input` and `cargo test -p core-ffi --test kontrol the_feedback_row`.
- [ ] **Step 3: Implement.**
  - In `control`, CC 0x17 joins the press-only command arm and pushes `Command("transport.click")`. Remove 0x17 from the do-nothing list.
  - In `send_state`, `tx.cc(0x17, 0)` becomes `tx.cc(0x17, on(view.click))`.
  - `surface_view` fills `click`, which is false until the other plan's state exists.
  - `command` reads `value`: with `press_only`, a value of 0 on 0x42–0x45 returns early.
  - Keep-alive uses two new fields, `alive_due: Option<Instant>` and `alive_wait: Option<Instant>`:
    - while online, `feedback` sends the hello when `alive_due` has passed and sets `alive_wait = now + HELLO_EVERY`;
    - `answer(v >= PROTOCOL)` with `alive_wait` set only clears the wait and sets the next `alive_due`;
    - `tick` past `alive_wait` takes the goodbye path of `message` (`set_online(false)`, `leave_mode`, `next_hello`) and returns its releases.
  - The registry's `Unit` gets `heard: VecDeque<String>` (`LOG_LIMIT`). `route` pushes the hex of each message it hands to a unit.
  - `Unit::new` sets neither option (Ruling R3).
- [ ] **Step 4: Confirm they pass.** Run `cargo test -p core-surface -p core-ffi --test kontrol`.
- [ ] **Step 5: Update the README.** In step 191, replace "Metro and Quantize stay dark and do nothing" with "Press Metro: the metronome turns on and the button lights; press again: off and dark; turn the click on in VizDAW: the light follows. Quantize stays dark and does nothing." Then add three hardware captures to step 198. Each one runs `target/debug/vizctl state surfaces` and reads the unit's `heard` list:
  - (a) press a track's Mute once: one 0x43 message, or two (with which value bytes);
  - (b) switch the keyboard to MIDI mode: does `heard` show a goodbye (`BF 02`)? If not, remove the surface and add it again, which sends a hello: does `heard` show an answer `BF 01 …`, and does the keyboard come back to DAW mode? That decides whether the keep-alive can be turned on;
  - (c) high-resolution knob deltas are not built (Ruling R3); the step records this as an open hardware question for a later build.
- [ ] **Step 6: Commit** as `feat(kontrol): Metro runs the click and lights with it; press-only track buttons and a keep-alive behind unit options; a heard log per surface`.

### Task 3: Kontrol session fixes: tempo after a ride, ramps, the preset cache (rows 70, 63, 69)

**Files:**
- Modify: `crates/core-ffi/src/session.rs` (`surface_tempo` ~13150, `sync_presets` ~13534, `pump` ~1432)
- Modify: `crates/core-ffi/src/surfaces.rs` (`SessionSurfaces.tempo_step` ~2420; a new `take_idle_holds`)
- Modify: `crates/core-ffi/src/plugins.rs` (`presets` ~404: a read counter)
- Test: `crates/core-ffi/tests/kontrol.rs`

**Interfaces:**
- Produces:
  - `pub(crate) fn take_idle_holds(unit: SurfaceId) -> Vec<AutomationTarget>` in surfaces.rs: the Kontrol unit's idle holds from `release_holds().idle`; empty for any other kind;
  - `SessionSurfaces.tempo_step: Option<(SurfaceId, u64, Tick)>`, where the tick is the point the gesture edits;
  - `#[cfg(feature = "test-hooks")] pub fn debug_preset_reads() -> u64` in plugins.rs.

- [ ] **Step 1: Write the failing tests:**
  - `a_tempo_turn_right_after_a_ride_is_taken`. Replace the last block of `an_edit_splits_a_tempo_gesture_and_a_ride_refuses_the_tempo`: after `BF 50 01`, `debug_advance(100)` and `msg(90)`:
    - the tempo reads 90, and `ignored` is unchanged;
    - one `undo` puts the tempo back to `start`;
    - a second `undo` undoes the fader ride.
  - `a_tempo_turn_during_a_mouse_drag_is_still_refused`: open a fader touch with `Session::gesture` (mouse, `Touch`), then `msg(90)`: the turn is refused and counted, and the tempo is unchanged.
  - `a_tempo_turn_inside_a_ramp_adds_a_point_at_the_playhead` (Ruling R4):
    - set tempo `[0: 100 ramp, 61440: 140]` and locate to 30720;
    - `msg(130)` gives points `[0: 100 ramp, 30720: 130 ramp, 61440: 140]`;
    - `msg(131)` 100 ms later (the same gesture) gives three points, the middle one 131;
    - one `undo` restores the two original points.
  - `a_turn_on_a_ramps_own_tick_edits_that_point`: at tick 0, `msg(110)` gives `[0: 110 ramp, 61440: 140]`.
  - `parameter_notices_do_not_reread_presets`. Use `plugin_mode(&[("vizdaw.test.bank", "Test Bank")])`, record `debug_preset_reads()`, then move the bank's parameter 100 with ten knob turns and ten feeds inside 400 ms: the reads rose by at most 1. A preset step (`BF 36 01`) still re-reads; that is the existing `preset_buttons_step_the_focused_plugins_presets`.
- [ ] **Step 2: Confirm they fail.** Run `cargo test -p core-ffi --test kontrol`.
- [ ] **Step 3: Implement.**
  - In `surface_tempo`, run a ride check before the refusal checks. It applies when all of these hold:
    - `inner.history.in_transaction()`;
    - `inner.static_edit.is_some()`;
    - every entry of `inner.touches` is held only by `TouchHolder::Surface(unit)`.

    Then call `take_idle_holds(unit)`, and for each target call `release_from(inner, t, TouchHolder::Surface(unit), last_ns)`, using the `last_ns` stamp as `IdleRelease` does. After that, the existing checks decide.
  - Ramps: choose the point as Ruling R4 says. With `new_step == false` and the same unit, edit the point at the gesture's stored tick.
  - Presets: drop the `params_changed` term from `fresh` in `sync_presets`. In `pump`, set `inner.surfaces.presets = None` when `r.loaded` or `r.restart` names the cached slot. Count calls in `plugins::presets`.
- [ ] **Step 4: Confirm they pass.** Run `cargo test -p core-ffi --test kontrol` and `cargo test -p core-ffi`.
- [ ] **Step 5: Update the README.** Step 194 loses "wait 1 s after any knob turn" and gains: "Turn knob 1, then the tempo control at once: both change, and two Undos undo the tempo, then the fader." Add to step 194: "On a song with a tempo ramp, play inside the ramp and turn the tempo: a new tempo point appears at the playhead, and the ramp still ends where it did." Mark that line *(Ruling R4, owner-confirmed)*.
- [ ] **Step 6: Commit** as `fix(kontrol): a ride's idle hold yields to a tempo turn; a turn inside a ramp adds a point; presets re-read only on load`.

### Task 4: Plug-in pages the user arranges, in the core (rows 73, 65)

**Files:**
- Create: `crates/core-ffi/src/surfaces/pages.rs` (the file, its checks and the page builder)
- Modify: `crates/core-ffi/src/surfaces.rs` (`mod pages;`, the exports, `debug_reset` clears the cache, and `chain` ~1714 returns the plug-in's key)
- Modify: `crates/core-ffi/src/session.rs` (`surface_page`, `slot_param_count`, `surface_plugin_page`, `PageChain` ~13642)
- Test: unit tests in `pages.rs`, `crates/core-ffi/tests/kontrol.rs`

**Interfaces:**
- Produces, in FFI:

  ```rust
  #[derive(Debug, Clone, PartialEq, uniffi::Record)]
  pub struct SurfacePageRow { pub name: String, pub params: Vec<u64> }
  #[uniffi::export] pub fn plugin_pages(plugin: PluginKey) -> Vec<SurfacePageRow>;
  #[uniffi::export] pub fn set_plugin_pages(plugin: PluginKey, pages: Vec<SurfacePageRow>) -> Result<(), VizError>;
  ```

  An empty `pages` removes the arrangement. `PluginKey` is the one in `catalog.rs`.
- Produces: `pub(crate) fn pages_for(params: &[u64], arranged: Option<&[SurfacePageRow]>) -> Vec<(String, Vec<u64>)>`. `params` are the automatable, non-hidden ids in list order.
- Produces: `chain()` returns `Vec<ChainSlot>`, with `pub(crate) struct ChainSlot { pub id: PluginInstanceId, pub name: String, pub key: (PluginFormat, String) }`.

The file is `support_dir()/plugin-pages.json`:

```json
{"version": 1, "plugins": [{"format": "clap", "id": "…", "pages": [{"name": "…", "params": [100, 103]}]}]}
```

It is read once on first use (at most 1 MiB) and written with `core_model::file::write_atomic`. A file that does not parse goes to `plugin-pages.json.bad` with a problem line, as `surfaces.json` does.

`set_plugin_pages` checks, each a `Model` error naming the rule:
- at most 127 pages;
- each page has 1 to 8 parameter ids;
- no id appears twice in one arrangement;
- each name passes `core_model::lane::valid_name` and is at most 64 characters;
- the file holds at most 512 plug-ins.

- [ ] **Step 1: Write the failing tests:**
  - Unit tests:
    - `default_pages_are_eights_in_list_order_up_to_127`: 20 ids give pages "Page 1" (8 ids), "Page 2" (8) and "Page 3" (4); 2000 ids give 127 pages;
    - `arranged_pages_reach_parameters_past_1016`: 2000 ids and an arrangement `[("Late", [1500, 1999])]` give `[("Late", [1500, 1999])]`;
    - `arranged_pages_skip_parameters_the_plugin_no_longer_has`: an arranged page whose ids are all gone is dropped, and an arrangement with nothing left falls back to the default.
  - `arranged_pages_show_on_the_keyboard` (kontrol.rs). With the bank plug-in (`vizdaw.test.bank`) in Plug-in mode, `set_plugin_pages(bank, [Mix: [123, 100]])`; after a feed:
    - the page name SysEx is `sysex(0x75, 0, 0, b"Mix")`;
    - parameter name 0 is parameter 123's name;
    - the page count is 1;
    - knob 1 moves parameter 123.
  - `set_plugin_pages_refuses_bad_arrangements`: each of these is an `Err`, and `plugin_pages` is unchanged:
    - 128 pages;
    - a page with 9 ids;
    - a page with no ids;
    - an id on two pages;
    - a blank name;
    - a name with a NUL.
  - `plugin_pages_survive_a_reload`: after `debug_reset()`, `plugin_pages(bank)` reads the same pages back from the file.
- [ ] **Step 2: Confirm they fail.** Run `cargo test -p core-ffi pages` and `cargo test -p core-ffi --test kontrol arranged`.
- [ ] **Step 3: Implement.**
  - `surface_page` builds the slot's pages with `pages_for`. The page count is the number of pages, still at most 127; the page name is the page's name.
  - The page's `ParamView`s come from its ids, through a new `slot_param_views_of(inner, slot, ids: &[u64], cached)`, the existing `slot_param_views` taking ids rather than a skip.
  - `surface_plugin_page` counts the same pages.
- [ ] **Step 4: Confirm they pass.** Run `cargo test -p core-ffi` and Linux clippy.
- [ ] **Step 5: Commit** as `feat(surfaces): plug-in pages the user arranges, kept in plugin-pages.json; they reach parameters past 1016`.

### Task 5: The Surface Pages sheet (row 73)

**Files:**
- Create: `app/Sources/VizDAWAppModel/SurfacePagesModel.swift`, `app/Sources/VizDAW/SurfacePagesSheet.swift`
- Modify: `app/Sources/VizDAWAppModel/SurfaceCommands.swift` (`SurfaceCore`, `NoSurfaceCore`), `app/Sources/VizDAW/LiveSurfaceCore.swift`, `app/Sources/VizDAW/PluginWindows.swift` (the header's buttons, ~line 260)
- Test: `app/Tests/VizDAWAppModelTests/SurfacePagesModelTests.swift`, `FakeSurfaceCore` in `SurfaceCommandsTests.swift`

**Interfaces:**
- Consumes: Task 4's `pluginPages(plugin:)` and `setPluginPages(plugin:pages:)`.
- Produces, on `SurfaceCore`:
  - `func pluginPages(_ p: PluginKey) -> [SurfacePageRow]`;
  - `func setPluginPages(_ p: PluginKey, _ pages: [SurfacePageRow]) throws`.
- Produces:

  ```swift
  @MainActor @Observable public final class SurfacePagesModel {
      public static let pageFull = "A page holds at most 8 parameters."
      public init(plugin: PluginKey, params: [ParamRow], core: SurfaceCore)
      public private(set) var pages: [SurfacePageRow]
      /// Automatable, shown parameters on no page, in the plug-in's order.
      public var unplaced: [ParamRow] { get }
      public private(set) var problem: String?
      public func addPage()                          // "Page \(pages.count + 1)", empty
      public func rename(_ page: Int, _ name: String)
      public func removePage(_ page: Int)
      public func movePage(from: Int, to: Int)
      @discardableResult public func place(_ param: UInt64, on page: Int, at index: Int?) -> Bool
      public func unplace(_ param: UInt64)
      public func useListOrder()                     // pages = []
      @discardableResult public func save() -> Bool  // empty pages dropped
  }
  ```

- [ ] **Step 1: Write the failing tests:**
  - `aPageHoldsAtMostEightParameters`: the 9th `place` returns false and sets `problem` to `pageFull`;
  - `placingMovesAParameterOffOtherPages`;
  - `saveDropsEmptyPagesAndSendsTheRest`: the fake records exactly the non-empty pages, in order;
  - `useListOrderClearsTheArrangement`: saving sends `[]`;
  - `onlyAutomatableShownParametersArePlaceable`;
  - `aRefusedSaveShowsTheCoresReason`: the fake throws `VizError.Model`, `problem` is its message, and `save()` returns false.
- [ ] **Step 2: Confirm they fail.** Run `scripts/build-ffi.sh` and `scripts/swift.sh test --filter SurfacePagesModelTests`.
- [ ] **Step 3: Implement the model, then the sheet.**
  - The sheet has two lists: pages with their parameters (drag to reorder, a name field per page), and unplaced parameters. Its buttons are Add Page, Use Plug-in Order, Cancel and Save. Every control is labelled; there are no `@State` or `@FocusState` properties.
  - The plug-in window's header gets `Button("Surface Pages…")`, enabled when the slot is live. Its help is "The pages a host-integration keyboard shows for this plug-in".
- [ ] **Step 4: Confirm they pass.** Run `scripts/swift.sh test --filter SurfacePagesModelTests` and `--filter NameScanTests`, then `scripts/make-app.sh`.
- [ ] **Step 5: Update the README.** Add a step after 196: open a plug-in's window and choose Surface Pages…. Make a page "Tone" with three parameters and Save; on the keyboard in Plug-in mode, "Tone" is page 1 and shows those three. Use Plug-in Order brings back "Page 1…".
- [ ] **Step 6: Commit** as `feat(app): Surface Pages… arranges a plug-in's pages for the keyboard`.

### Task 6: Extenders show plug-in parameters 9–16 (row 58)

**Files:**
- Modify: `crates/core-surface/src/lib.rs` (`Surface`: `set_plugin_first`), `crates/core-surface/src/mcu.rs` (`plugin_mode` ~314, `feedback_rings` ~404, `feedback_rows` ~426, `cc` ~738, `feedback` ~821)
- Modify: `crates/core-ffi/src/surfaces.rs` (`Due.plugin` ~598, `due` ~1419, `rebank` ~915), `crates/core-ffi/src/session.rs` (`surface_params` ~13377, `slot_param_views`)
- Test: `crates/core-surface/tests/mcu_feedback.rs`, `crates/core-ffi/tests/surfaces.rs`

**Interfaces:**
- Produces:
  - `fn set_plugin_first(&mut self, _first: usize) {}` on `Surface`. `Mcu` keeps it; the main unit's is 0, and each extender's is `8 × (1 + its rank among the extenders by position)`;
  - `Due.plugin: usize`, the parameters the due units need (0 for none);
  - `surface_params(inner, cached, count: usize)`.

- [ ] **Step 1: Write the failing tests:**
  - `extenders_show_the_next_eight_parameters` (mcu_feedback.rs). Give `v.plugin` 24 params, then:
    - extenders with `set_plugin_first(8)` and `set_plugin_first(16)`, in Plug-in, show row 1 starting `"Prm8   "` and `"Prm16  "`;
    - V-pot 1 on the first extender touches `ParamId(8)`;
    - an extender at 24 shows blank rows, and its V-pots answer nothing;
    - an extender in Pan is unchanged.
  - `an_extender_in_plugin_mode_shows_parameters_9_to_16` (core-ffi surfaces.rs): an MCU at position 0 and an XT at 1, the bank plug-in on the selected strip, and Plug-in pressed on the main unit; after a feed, the XT's sent row 1 holds the bank's 9th parameter name.
- [ ] **Step 2: Confirm they fail.** Run `cargo test -p core-surface --test mcu_feedback` and `cargo test -p core-ffi --test surfaces extender`.
- [ ] **Step 3: Implement.**
  - `plugin_mode()` becomes `assign == Plugin`. Rows, rings and V-pots read `plugin.get(plugin_first + slot)`.
  - `rebank` sets each unit's first.
  - `due` sets `plugin` to the largest `first + 8` among due MCU-group units in Plug-in, and shows `PluginParam` moves on extenders in Plug-in too.
  - `surface_params` takes `count`.
- [ ] **Step 4: Confirm they pass.** Run `cargo test -p core-surface -p core-ffi`.
- [ ] **Step 5: Update the README.** In the MCU section's Plug-in step, add: "with an extender added, it shows parameters 9–16 and its V-pots move them; a second extender shows 17–24."
- [ ] **Step 6: Commit** as `feat(surfaces): extenders show the next eight plug-in parameters`.

### Task 7: LED feedback for learned note maps, opt-in per map (row 59)

**Files:**
- Modify: `crates/core-model/src/learn.rs` (`LearnMap` ~86: `feedback`, `feeds_back()`), `crates/core-surface/src/learn.rs` (`Mapper::feedback` ~534)
- Modify: `crates/core-ffi/src/surfaces.rs` (`LearnMapRow` ~308), `crates/core-ffi/src/session.rs` (`set_learn_feedback` beside `set_learn_pickup` ~12694)
- Modify: `app/Sources/VizDAWAppModel/LearnModel.swift` (beside `setPickup` ~131), the session protocol and `FakeSession`, `app/Sources/VizDAW/LearnMapsSheet.swift` (~68)
- Update: every `LearnMap { … }` literal (`git grep -n "LearnMap {"`) gains `feedback: false`
- Test: `crates/core-surface/tests/learn.rs`, `crates/core-model` learn tests, `crates/core-ffi/tests/learn.rs`, `app/Tests/VizDAWAppModelTests/LearnModelTests.swift`

**Interfaces:**
- Produces:
  - `LearnMap.feedback: bool`, as `#[serde(default, skip_serializing_if = "std::ops::Not::not")]`, so projects without it stay readable by builds that do not know it;
  - `LearnMap::feeds_back(&self) -> bool`, true for `LearnKind::Note` only;
  - `Session::set_learn_feedback(&self, source: LearnSourceRow, on: bool) -> Result<DocSnapshot, VizError>`, one "MIDI Learn" undo step;
  - `LearnMapRow.feedback` and `LearnMapRow.feeds_back`;
  - Swift: `LearnModel.setFeedback(_ m: LearnMapRow, _ on: Bool)` and `LearnModel.feedbackHelp`.

- [ ] **Step 1: Write the failing tests:**
  - `note_maps_light_their_pad_only_when_asked` (learn.rs), on a note map (ch 9, note 36) to `StripMute`:
    - with `feedback: false`, `feedback(&|_| Some(1.0))` is empty;
    - with `true`, it gives `("Knobs", [0x99, 36, 127])`;
    - then `Some(0.0)` gives `[0x99, 36, 0]`;
    - an unchanged value gives nothing.
  - `feedback_is_written_only_when_on`: a map with `feedback: false` serializes with no `feedback` key; `true` round-trips; `feedback` on a CC map is accepted and has no effect.
  - `set_learn_feedback_is_one_undo_step_and_refuses_an_unknown_source` (core-ffi learn.rs).
  - Swift `theMapsSheetOffersFeedbackOnlyForNoteMaps`: `setFeedback` reaches the fake, and `feedsBack` is false for a CC map.
- [ ] **Step 2: Confirm they fail.** Run `cargo test -p core-surface --test learn`, `cargo test -p core-model learn` and `cargo test -p core-ffi --test learn`.
- [ ] **Step 3: Implement.**
  - In `Mapper::feedback`, a note map with `feedback` sends note-on with velocity 127 at a value of 0.5 or more, and velocity 0 below, on the map's channel. This is MIDI 1.0's documented note-on, not a vendor protocol.
  - The sheet gets a `Toggle("LED")` beside Pick-up, disabled unless `feedsBack`. Its help is "Lights the pad with the target's state; only for pads that do not light themselves".
- [ ] **Step 4: Confirm they pass.** Run `cargo test --workspace`, `scripts/build-ffi.sh` and `scripts/swift.sh test --filter LearnModelTests`.
- [ ] **Step 5: Update the README.** In the MIDI Learn section, add: learn a pad to a track's Mute, tick LED, and press the pad: its light follows the Mute button, including when you click Mute in the mixer. Untick LED: the pad's light is left to the pad.
- [ ] **Step 6: Commit** as `feat(learn): opt-in LED feedback for learned note maps`.

### Task 8: Keep learned commands and fields this build does not know in surfaces.json (row 71)

**Files:**
- Modify: `crates/core-ffi/src/surfaces.rs` (`Entry` ~476, `CommandEntry` ~503, `SurfacesFile` ~519, `Registry.commands`, `file` ~1075, `check_command` ~1870, `parse_file` ~1883, and the learn path that adds a command)
- Test: `crates/core-ffi/src/surfaces.rs` tests (beside `a_file_is_checked_entry_by_entry` ~3121), `crates/core-ffi/tests/kontrol.rs` (`a_surfaces_file_with_an_unknown_kind_keeps_the_rest`)

**Interfaces:**
- Produces:
  - `Entry`, `CommandEntry` and `SurfacesFile` drop `deny_unknown_fields` and gain `#[serde(flatten)] extra: serde_json::Map<String, Value>`, written back as read;
  - `struct KeptCommand { raw: serde_json::Value, problem: String, source: Option<LearnSource> }` in `Registry.kept_commands`, never run, claimed or listed.

A command is kept when its `command` is not a registry ID, or when its `source` does not parse. Its problem line is `learned command “<id>” needs a newer VizDAW; it is kept in surfaces.json and not run`. A known ID that no key can run still refuses the file, as today. When a new learn takes a source that a kept command uses, the kept command goes, with the problem line `… replaced by the new learn`. Otherwise a newer build would find the same source learned twice.

- [ ] **Step 1: Write the failing tests:**
  - `a_newer_files_unknown_commands_and_fields_survive_a_save`. Write a file with:
    - a Kontrol entry with the extra field `"screen": "x"`;
    - a command `"command": "future.thing"`;
    - a known command with the extra field `"hold_ms": 30`;
    - the top-level field `"profiles": []`.

    Load it, add an MCU with `surface_add` (which saves), and read the file back as JSON. All four unknowns are there unchanged, there is no `.bad` file, and the Kontrol is driven.
  - `a_kept_command_is_not_run_or_claimed`: injecting the kept command's message runs nothing, and `would_listen` is false for its port.
  - `a_new_learn_on_a_kept_commands_source_replaces_it`.
  - `a_broken_file_still_goes_bad`: not JSON, version 2, a surface without an ID, and the same source learned twice (both known) each go to `.bad`, as before.
- [ ] **Step 2: Confirm they fail.** Run `cargo test -p core-ffi surfaces` and `cargo test -p core-ffi --test kontrol surfaces_file`.
- [ ] **Step 3: Implement.** Parse `commands` as `Vec<serde_json::Value>`, then each value into a `CommandEntry` or a `KeptCommand`. `file()` writes the driven commands, then the kept ones, as read.
- [ ] **Step 4: Confirm they pass.** Run `cargo test -p core-ffi` and Linux clippy.
- [ ] **Step 5: Update the backlog line.** The "Dropped at 1.0" note on pre-5f-4 builds stays. This row's Notes say what is kept from now on.
- [ ] **Step 6: Commit** as `fix(surfaces): keep unknown learned commands and fields in surfaces.json`.

### Task 9: A too-short window hides only the bottom pane; a dropped pane ends its edits (rows 82, 84)

**Files:**
- Modify: `app/Sources/VizDAWAppModel/PaneGeometry.swift` (`frames(in:setup:)`)
- Modify: `app/Sources/VizDAWAppModel/ViewCommands.swift` (`layoutRow`), `app/Sources/VizDAWAppModel/DocumentModel.swift` (`droppedPanes`), `app/Sources/VizDAW/DocumentShell.swift` (`regions`)
- Modify: `crates/core-ffi/src/control.rs` (`LayoutStateRow` ~211, the JSON ~1826)
- Test: `app/Tests/VizDAWAppModelTests/PaneGeometryTests.swift`, `ViewCommandsTests` (or `LayoutTests.swift`), `crates/core-ffi/tests/control.rs` (~930)

**Interfaces:**
- Produces:
  - `DocumentModel.droppedPanes: [PaneSide]` (`public private(set)`);
  - `DocumentModel.panesDropped(_ hidden: [PaneSide])`, which calls `endPaneEdits()` when `hidden` has a side the last call did not;
  - `LayoutStateRow.dropped: Vec<String>` (JSON `"dropped"`).

The rule: the height is short when the bottom pane is shown and `size.height − divider − bottom's lower bound < centreMinimum.height`. Then the drop order is `[.bottom, .right, .left]`; otherwise it stays `[.right, .left, .bottom]`.

- [ ] **Step 1: Write the failing tests:**
  - `aHeightShortfallHidesOnlyTheBottomPane`: 1000×270 with left, bottom and right shown gives `hidden == [.bottom]` with `left` and `right` not nil.
  - In `aSmallWindowShrinksPanesBeforeTheCentre`, `tiny.hidden` becomes `[.bottom, .right, .left]`; `narrow` is unchanged.
  - `aDroppedPaneEndsItsEditsOnce`: with a sequence rename open, `panesDropped([.left])` commits the rename (the draft text is kept, `renamingSeq == nil`); a second `panesDropped([.left])` and then `panesDropped([])` call nothing more.
  - `theLayoutRowNamesDroppedPanes`: `layoutRow(...).dropped == ["left"]`.
  - Rust: the `state.get layout` JSON has `"dropped": ["left"]`.
- [ ] **Step 2: Confirm they fail.** Run `scripts/swift.sh test --filter PaneGeometryTests` and `cargo test -p core-ffi --test control layout`.
- [ ] **Step 3: Implement.** In `DocumentShell.regions`, add `.onChange(of: f.hidden, initial: true) { _, h in model.panesDropped(h) }`.
- [ ] **Step 4: Confirm they pass.** Run `scripts/build-ffi.sh`, `scripts/swift.sh test`, and the e2e `window_views_and_panes_over_the_channel`.
- [ ] **Step 5: Update the README.** In the plan 7a section, add: at 700×450 with three banners, the left pane stays and only the bottom pane goes. Rename a sequence in the left pane, then narrow the window until the left pane goes: the rename is kept, and Space plays.
- [ ] **Step 6: Commit** as `fix(app): a short window drops only the bottom pane; a dropped pane ends its edits and is reported`.

### Task 10: Warn about keyboard slots that commands shadow (row 81)

**Files:**
- Modify: `app/Sources/VizDAWAppModel/KeyBindings.swift` (`KeyRouter`), `app/Sources/VizDAWAppModel/ProjectMedia.swift` (`ProjectBanner`, `banners` ~246), `app/Sources/VizDAW/ProjectBanners.swift`, `app/Sources/VizDAW/DocumentShell.swift` (check on appear)
- Test: `app/Tests/VizDAWAppModelTests/KeyBindingsTests.swift`

**Interfaces:**
- Produces:

  ```swift
  public struct ShadowedSlot: Equatable, Sendable { public let seq: UInt64; public let name: String; public let chord: String; public let command: String }
  extension KeyRouter { public static func shadowedSlots(sequences: [SeqRow], keys: KeyBindings) -> [ShadowedSlot] }
  ```

  A slot is shadowed when `route(keyCode:modifiers:typing: false, stepEntry: false, menus: false, keys:sequences:)` answers `.command`, not `.slot`. `chord` is `KeySlotFormat.text(slot)`, and `command` is the command's title.
- Produces: `ProjectBanner.shadowedSlots([ShadowedSlot])`, `DocumentModel.checkShadowedSlots(_ keys: KeyBindings)` and `DocumentModel.dismissShadowedSlots()`.

The banner text: `Keyboard slots that run a command instead: ⌃1 (“Verse”, runs Arrange). Change them with Keyboard Slot… in the Sequences list.` It has a Dismiss button.

- [ ] **Step 1: Write the failing tests:**
  - `aSlotOnAViewKeyIsShadowed`: sequences with slots ⌃1 and ⇧F5 give one `ShadowedSlot` for ⌃1, naming the Arrange command;
  - `aSlotThatAYieldingCommandKeepsIsNotShadowed`: a command with `yields` and no customisation;
  - `theBannerListsShadowedSlotsUntilDismissed`.
- [ ] **Step 2: Confirm they fail.** Run `scripts/swift.sh test --filter KeyBindingsTests`.
- [ ] **Step 3: Implement.** `DocumentShell` calls `model.checkShadowedSlots(keyBindings)` once in `.task`.
- [ ] **Step 4: Confirm they pass.** Run the same filter, then `--filter NameScanTests`.
- [ ] **Step 5: Update the README.** Give a sequence the slot ⌃1, save, close and reopen: the banner names it, and Dismiss hides it.
- [ ] **Step 6: Commit** as `feat(app): a banner names keyboard slots that a command shadows`.

### Task 11: A Plug-ins pane pick leaves other placements' browsers open (row 90; Ruling R9)

**Files:**
- Modify: `app/Sources/VizDAWAppModel/MixerModel.swift` (`pick` ~440), `app/Sources/VizDAWAppModel/PaneContent.swift` (`insertPlugin` ~234)
- Test: `app/Tests/VizDAWAppModelTests/RightPaneTests.swift`

**Interfaces:**
- Produces: `MixerModel.add(_ plugin: PluginRefRow, to target: BrowserTarget)`, the body `pick` has today. `pick` becomes "clear `browser`, then `add`".

- [ ] **Step 1: Write the failing test.** `aPanePickLeavesAnOpenPopoverOpen`: `openBrowser(.instrument, in: .window)`, then `PaneContent.insertPlugin(delay, …)`. The insert went in (the fake's edit), and afterwards `browser == .instrument` and `showsBrowser(in: .window)` is true.
- [ ] **Step 2: Confirm it fails.** Run `scripts/swift.sh test --filter RightPaneTests`.
- [ ] **Step 3: Implement.** `insertPlugin` calls `mixer.add(p, to: .insert(strip: strip))`.
- [ ] **Step 4: Confirm it passes.** Run the same filter and `--filter MixerModelTests`.
- [ ] **Step 5: Commit** as `fix(app): a Plug-ins pane pick no longer closes a browser open elsewhere`.

### Task 12: Knob and fader scroll-wheel control (row 98; Ruling R6)

**Files:**
- Create: `app/Sources/VizDAWAppModel/WheelRun.swift` (the pure run), `app/Sources/VizDAW/WheelControl.swift` (the monitor and the modifier)
- Modify: `app/Sources/VizDAW/ChannelStrip.swift` (`SendKnob` ~698, `StripPan` ~784, `StripFaderSection` ~828), `app/Sources/VizDAW/MidiStripCells.swift` (`MidiStripPan` ~127, `MidiStripFader` ~175)
- Test: `app/Tests/VizDAWAppModelTests/WheelRunTests.swift`

**Interfaces:**
- Produces:

  ```swift
  public enum WheelStep: Equatable, Sendable { case begin, change(Double), end }
  public struct WheelRun: Sendable {
      public static let notch = 0.01, point = 0.002, fine = 0.1, idleEnd = 0.4
      public init()
      /// One wheel event over a control showing `value` (0…1) at `now` (seconds).
      public mutating func scroll(dy: Double, dx: Double, precise: Bool, option: Bool, value: Double, now: Double) -> [WheelStep]
      /// `.end` once `idleEnd` has passed since the last event; nil otherwise.
      public mutating func tick(now: Double) -> WheelStep?
      /// Ends an open run at once (a mouse press, the view leaving its window).
      public mutating func cancel() -> WheelStep?
      public var isOpen: Bool { get }
  }
  ```

- Produces: `View.wheelAdjust(value: Double, onBegin: @escaping () -> Void, onChange: @escaping (Double) -> Void, onEnd: @escaping () -> Void) -> some View`. This is a background `NSViewRepresentable` whose view registers with one app-wide local `.scrollWheel` monitor. The monitor consumes an event only over a registered view in the event's window, and only when `|dy| > |dx|`. It passes the event on while `NSEvent.pressedMouseButtons != 0`, and calls `cancel()` when a run is open then.

- [ ] **Step 1: Write the failing tests:**
  - `aRunBeginsOnceAndChangesByNotches`: three notches up from 0.5 give `[.begin, .change(0.51)]`, then `[.change(0.52)]`, then `[.change(0.53)]`;
  - `optionIsFineAndPreciseIsPerPoint`: ⌥ one notch gives +0.001; a precise dy of 10 gives +0.02;
  - `valuesClampToZeroAndOne`;
  - `aMostlyHorizontalScrollIsNotARun`: dx 5 and dy 1 give `[]`;
  - `theRunEndsAfterIdle`: `tick` at +0.39 s gives nil, and at +0.4 s gives `.end` (once);
  - `aWheelRunEndsOnAMousePressOrWhenTheViewGoes`: `cancel()` on an open run gives `.end`, and nil after that.
- [ ] **Step 2: Confirm they fail.** Run `scripts/swift.sh test --filter WheelRunTests`.
- [ ] **Step 3: Implement.**
  - `WheelRun` takes the direction with `isDirectionInvertedFromDevice` already applied by the caller, so up raises the value.
  - The NSView runs a 0.1 s main-actor timer while a run is open, and calls `cancel()` in `viewWillMove(toWindow:)` when the window is nil.
  - Put `.wheelAdjust` on the five controls with their existing `onBegin`/`onChange`/`onEnd`, so each run is one touch and so one undo step.
- [ ] **Step 4: Confirm they pass.** Run `scripts/swift.sh test`, then `scripts/make-app.sh`.
- [ ] **Step 5: Update the README.** In the plan 7b section, add:
  - scroll over a fader: it moves, and one Undo undoes the whole scroll;
  - ⌥-scroll moves finely;
  - a trackpad's sideways swipe over the mixer still scrolls the strips;
  - while you scroll, the fader shows its touch ring, which goes 0.4 s after you stop.
- [ ] **Step 6: Commit** as `feat(app): the scroll wheel turns knobs and moves faders`.

### Task 13: Audio editor zoom saved with the project (row 105; Ruling R7)

**Files:**
- Modify: `app/Sources/VizDAWAppModel/EditorLayout.swift` (an `audioEditor` field and its coding key), `app/Sources/VizDAWAppModel/FrameAxis.swift` (`restored`), `app/Sources/VizDAWAppModel/AudioEditorModel.swift` (`sync`, `set`, `taller`, `shorter`), `app/Sources/VizDAWAppModel/DocumentModel.swift` (`save` ~893, `saveAs` ~920)
- Test: `app/Tests/VizDAWAppModelTests/AudioEditorModelTests.swift`, `LayoutTests.swift`

**Interfaces:**
- Produces:

  ```swift
  public struct AudioEditorView: Codable, Equatable, Sendable { public var region: UInt64; public var framesPerPoint: Double; public var originFrame: Double; public var amplitude: Int }
  // EditorLayout
  public var audioEditor: [AudioEditorView] = []   // most recent last, at most 64
  // FrameAxis
  public static func restored(framesPerPoint: Double, originFrame: Double, width: CGFloat, mediaFrames: UInt64) -> FrameAxis?
  // AudioEditorModel
  public func storeViews()   // writes the remembered views into editor.layout
  ```

  `restored` is nil unless both numbers are finite and `framesPerPoint ≥ 1`; otherwise it clamps as `zoomed` does (make `clamped` internal). Decoding drops any entry with a non-finite field and clamps `amplitude` to 1…16.

- [ ] **Step 1: Write the failing tests:**
  - `aRegionShownAgainKeepsItsZoom`: zoom in twice and scroll on region A, show region B, then show A again: its axis and amplitude are as they were.
  - `zoomSurvivesSaveAndReopen`: `save()` stores the views; a new `DocumentModel` over the same fake `uiLayout` restores A's axis on its first `sync(width:)`.
  - `aSavedViewIsClampedToTheRegionAsItIsNow`. Feed hostile entries: NaN, a `framesPerPoint` of −5 or 1e300, an origin past the file's end, and a region trimmed shorter since. Each gives a finite axis inside `maxFramesPerPoint` for the media, or falls back to fit.
  - `onlyTheLast64RegionsAreKept`.
- [ ] **Step 2: Confirm they fail.** Run `scripts/swift.sh test --filter AudioEditorModelTests`.
- [ ] **Step 3: Implement.** `AudioEditorModel` keeps `@ObservationIgnored var views`, seeded from `layout.audioEditor`. It updates the views on every axis or amplitude change and writes them only in `storeViews()`, which `save()` and `saveAs` call before `editor.storeLayout()`. So scrolling never touches `layout` or `drawVersion`.
- [ ] **Step 4: Confirm they pass.** Run `scripts/swift.sh test --filter AudioEditorModelTests` and `--filter LayoutTests`.
- [ ] **Step 5: Update the README.** In the plan 7c section, add: zoom into a region and scroll, then save, close and reopen; open the same region: the same view. A new region still opens fitted.
- [ ] **Step 6: Commit** as `feat(app): the audio editor's zoom and scroll are saved per region in ui.json`.

### Task 14: Rename or delete any take from the Takes menu (row 293)

**Files:**
- Modify: `app/Sources/VizDAWEditors/TakesMenu.swift`
- Test: `app/Tests/VizDAWAppModelTests/ArrangementViewTests.swift` (`theTakesMenuListsAndActs` ~286)

The menu:

```
Takes ▸   ✓ Take 2              (choose: plays it)
            Take 1
            ───
            Rename Take ▸   Take 1…  ·  Take 2 (playing)…
            Delete Take ▸   Take 1   ·  Take 2 (playing)
```

Delete Take is disabled with one take. Both submenus are disabled under an empty list. Each item runs `TakeCommands.renameAsking` or `TakeCommands.delete` with its own take ID.

- [ ] **Step 1: Write the failing test.** Rewrite `theTakesMenuListsAndActs`:
  - the top titles are `["Take 1", "Take 2", "Rename Take", "Delete Take"]`;
  - Rename Take's submenu is `["Take 1…", "Take 2 (playing)…"]`;
  - choosing Delete Take › Take 1 records `.removeTake(seq: 3, lane: 30, take: 300)`;
  - on Chords (one take), Delete Take is disabled;
  - on a lane with `takes == []`, both submenus are disabled.
- [ ] **Step 2: Confirm it fails.** Run `scripts/swift.sh test --filter ArrangementViewTests`.
- [ ] **Step 3: Implement**, and update the doc comment.
- [ ] **Step 4: Confirm it passes.** Run the same filter and `--filter TakeCommandsTests`.
- [ ] **Step 5: Update the README.** In the takes step: with three takes, rename Take 1 while Take 3 plays: Take 3 keeps playing. Delete Take 1: Take 3 still plays.
- [ ] **Step 6: Commit** as `feat(app): the Takes menu renames or deletes any take`.

### Task 15: A Settings default for Replace/Overdub; the mode waits while recording (row 294, row 295 final review)

**Files:**
- Modify: `app/Sources/VizDAWAppModel/AudioPreferences.swift` (`AudioSettings`, `setRecordDefaults` ~223, `RecordDefaults` ~235), `app/Sources/VizDAWAppModel/EditorModel.swift:111` (the new-document default), `app/Sources/VizDAWAppModel/RecordOptions.swift` (`replaces` ~25), `app/Sources/VizDAWAppModel/TransportCommands.swift` (`enabled` ~62, `run` ~106), `app/Sources/VizDAW/AudioSettingsView.swift` (~156)
- Test: `app/Tests/VizDAWAppModelTests/AudioPreferencesTests.swift`, `RecordOptionsTests.swift`

**Interfaces:**
- Produces:
  - `AudioSettings.newDocumentsReplace = false` (key `newDocumentsReplace`) and `RecordDefaults.replace`;
  - `setRecordDefaults(countIn:preRoll:midiPassesAsTakes:replace:)`, with every caller updated;
  - `RecordModel.modeWhileRecording = "Stop recording to switch between Replace and Overdub."`.

- [ ] **Step 1: Write the failing tests:**
  - `theReplaceDefaultIsKeptAndRead` (in the `settingsAreKeptOnlyOnceTheCoreTookThem` style);
  - `aNewDocumentStartsInTheDefaultMode`: with `RecordModel.defaults.replace = true`, a `DocumentModel` over a fake whose `uiLayout()` is nil has `transport.overdub == false`, while one with a saved layout keeps its own (Ruling R8);
  - `replacesFollowsTheDefaultWithNoDocument`;
  - `theModeKeyWaitsWhileRecording`: while recording, `TransportCommands.enabled("transport.overdub")` is false, and `run` answers `.refused(modeWhileRecording)` and leaves `overdub` unchanged.
- [ ] **Step 2: Confirm they fail.** Run `scripts/swift.sh test --filter AudioPreferencesTests` and `--filter RecordOptionsTests`.
- [ ] **Step 3: Implement.**
  - In `EditorModel`'s init, when `uiLayout()` returns nil, set `layout.transport.overdub = !RecordModel.defaults.replace`.
  - In Settings › Recording › New Documents, add `Picker("Record mode", …)` with Overdub and Replace, beside "MIDI passes as takes".
- [ ] **Step 4: Confirm they pass.** Run `scripts/swift.sh test`.
- [ ] **Step 5: Update the README.** Set Settings › Recording › Record mode to Replace and choose File › New: the bar reads "Replace". An older project opens in its own mode. While recording, the overdub key and the bar's switch both do nothing, and the status line says why. Check that the bar's "Replace"/"Overdub" title reads clearly.
- [ ] **Step 6: Commit** as `feat(app): a Settings default for Replace or Overdub in new documents; the mode waits while recording`.

### Task 16: Takes polish in the core (row 295, items 1–4, 8 and the e2e comment)

**Files:**
- Modify: `crates/core-ffi/src/passes.rs` (`take_name` ~778; `Captured` ~158), `crates/core-ffi/src/record.rs` (`Plan` ~498, `Recording` ~255, `Finished` ~297, `build` ~835)
- Modify: `crates/core-model/src/command/record.rs` (`LaneRecord.take` ~67, `apply` ~213), `crates/core-model/src/document.rs` (`remove_take` ~911: a comment)
- Modify: `crates/core-ffi/src/control.rs:903` (the `status` doc), `crates/vizctl/tests/e2e.rs:4950` (the comment)
- Test: unit tests in `passes.rs`, `crates/core-ffi/tests/record.rs`, `crates/core-ffi/tests/takes.rs`, `crates/core-model/tests/cmd_lane.rs`

**Interfaces:**
- Produces:
  - `fn next_take_number<'a>(names: impl Iterator<Item = &'a str>, count: usize) -> usize` in passes.rs, which answers `max(count, highest N of names exactly "Take N") + 1`;
  - `LaneRecord.take: Option<TakeId>`, the take the pass lands in (`None`: the active one);
  - `Plan.takes`, `Recording.takes` and `Finished.takes: BTreeMap<LaneId, TakeId>`, and `Captured.takes: &BTreeMap<LaneId, TakeId>`, holding each armed lane's active take at Record.

- [ ] **Step 1: Write the failing tests:**
  - `a_new_take_never_repeats_a_name`: `next_take_number(["Take 1", "Take 3"], 2) == 4`, `(["Take 1", "Verse"], 2) == 3`, and `([], 0) == 1`. Then an FFI case: delete Take 2 of three and loop-record one take: it is named "Take 4".
  - `replace_lands_in_the_take_that_was_playing_at_record` (record.rs):
    - give a track takes A and B, each with a note at bar 1, and choose B (one undo step);
    - record in Replace over bar 1 with an injected note;
    - during the pass, `undo` (the active take is now A);
    - at Stop, B holds only the new note, A keeps its note, and the playing take after Stop is B's pass, as before.
  - `undoing_the_delete_of_the_playing_take_restores_it_playing` (takes.rs).
  - `apply` with `take` naming a take that has since gone falls back to the active take (cmd_lane.rs).
- [ ] **Step 2: Confirm they fail.** Run `cargo test -p core-ffi --test record replace_lands`, `cargo test -p core-ffi --test takes` and `cargo test -p core-model --test cmd_lane`.
- [ ] **Step 3: Implement.**
  - `apply` takes the lane's `take` when it still exists, for both the `clear` and `insert_content`.
  - Comment in `remove_take` why its `ok_or` cannot fail: there are at least two takes, so a neighbour exists.
  - The `Ran.status` doc: "The editors' status line afterwards; for `track.choose_take`, `track.rename_take` and `track.delete_take`, the take's name (chosen, new or deleted)."
  - The e2e comment: "30 000 frames in (more than one beat at 120 BPM at 48 kHz): a pass cut short before it is heard makes no take."
- [ ] **Step 4: Confirm they pass.** Run `cargo test -p core-model -p core-ffi` and Linux clippy.
- [ ] **Step 5: Commit** as `fix(takes): new take names never repeat; a pass lands in the take that played at Record`.

### Task 17: Takes polish in the app (row 295, items 5–7 and the VoiceOver check)

**Files:**
- Modify: `app/Sources/VizDAWAppModel/EditorModel.swift` (`change` ~246, `report` ~281), `app/Sources/VizDAWAppModel/TakeCommands.swift` (`validName`, `execute`, `renameAsking`)
- Test: `app/Tests/VizDAWAppModelTests/TakeCommandsTests.swift`, `ArrangementViewTests.swift`

**Interfaces:**
- Produces: `EditorModel.lastFailureWasBusy: Bool` (`@ObservationIgnored public private(set)`), set by `change` from `VizError.Busy` and cleared on success.

- [ ] **Step 1: Write the failing tests:**
  - `busyIsReadFromTheErrorNotTheText`: the fake throws `VizError.Busy(message: "anything")`, and `execute` answers `.busy`. A `.Model` error whose message equals `TrackCommands.recordingBusy` answers `.refused`.
  - `zeroWidthNamesAreRefusedBeforeTheCore`: the names `"\u{200B}"` and `"A\u{FEFF}"` give `invalidParams` on the channel and a status line from the menu, with no edit sent.
  - `theMenuRenameSendsTheNameTrimmed`: an asked `"  Verse  "` sends `"Verse"`.
  - `theRegionMenuFromARightClickHasTakes`: `ArrangementView.menu(for:)` with a synthetic right-mouse-down `NSEvent` at a block's centre answers a menu whose titles contain "Takes". `blockMenu`'s strong hold stays: it is bounded and released when the menu closes. Say so in a comment.
- [ ] **Step 2: Confirm they fail.** Run `scripts/swift.sh test --filter TakeCommandsTests` and `--filter ArrangementViewTests`.
- [ ] **Step 3: Implement.** `validName` also refuses U+200B–U+200D, U+2060 and U+FEFF, matching `core_model::lane::valid_name`. `renameAsking` trims before it renames.
- [ ] **Step 4: Confirm they pass.** Run `scripts/swift.sh test`.
- [ ] **Step 5: Update the README.** In the takes step, add a VoiceOver check: "Add Bus" and the record mode switch are each read once, by name.
- [ ] **Step 6: Commit** as `fix(takes): typed Busy, zero-width names refused in the app, the menu's rename trimmed`.

### Task 18: Backlog bookkeeping and the whole-branch check

**Files:**
- Modify: `docs/feature-backlog.md` (each row's Notes; the 26.0.0 milestone list), `app/README.md` (renumber the new steps after the last one, which is 438 at the time of writing)

- [ ] **Step 1: Update the backlog.**
  - Prefix each built row's Notes with `**Done (26.0.0 beta, plan kontrol-ui-takes Task N):**`.
  - Prefix rows 64, 66 and 68 with `**Partly done (26.0.0 beta, Task N; Ruling R2/R3):** waits for README 198`, adding what is left. Row 63 is done (Task 3; Ruling R4, owner-confirmed).
  - Prefix row 62 with "**Partly done (26.0.0 beta, Task 2):** Metro runs `transport.click`; Quantize and Clear are not built (Ruling R1)."
  - Prefix rows 61 and 67 with `**Not built (26.0.0 beta, Ruling R1/R3):**` and the reason.
  - Add `- Beta (owner, 2026-11-18): plan 2026-11-18-kontrol-ui-takes.md.` to the 26.0.0 list.
- [ ] **Step 2: Verify the whole branch.**
  - Run `cargo test --workspace`, Linux clippy, `scripts/build-ffi.sh`, `scripts/swift.sh test` (the name scans included) and `scripts/make-app.sh`.
  - Run the e2e: `VIZDAW_E2E_APP=dist/VizDAW.app cargo test -p vizctl --test e2e -- --ignored --nocapture --test-threads 1`. It must pass, `drives_the_mixer_from_a_kontrol_mk3` included.
- [ ] **Step 3: Commit** as `docs: the 26.0.0 beta rows marked; README checks for the owner`.
