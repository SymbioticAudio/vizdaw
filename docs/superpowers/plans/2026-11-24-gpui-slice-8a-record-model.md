# 27.1.0 Slice 8a: Recording, Step Entry and Marks (Model and Control Channel) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every recording, step-entry, click, record-arm, punch, overdub, locator and marker command works from the control channel and the keyboard in the GPUI app, on the Swift model: audio and MIDI loop recording with keep/discard, `record.set`, `record.recover`, `midi.inject`, step entry with its own key context, the punch and locator marks saved in `ui.json`, and the Save/Save As refusals while recording. After 8a, `G/control.rs` answers `not_yet` for none of the slice 8 ids, and the three 8a e2e ports pass on the Mac and on Linux with the offline device. Views (Record button, popover, bar items, Step menu) are 8b; inputs, meters, the growing take and the Markers pane are 8c.

**Architecture:** The core already does it all (inventory §3). 8a puts the missing calls on the `DocSession` seam (and `FakeSession` with scripted passes), ports Swift `TransportMarks`, `RecordModel`/`RecordOptions` (model half), `StepEntryModel`/`StepCommands` and the command rows into `vizdaw-app`, and wires the ids, `state.get record|input`, editor `step`/`marks`, transport `click` and a step-aware key lookup into `vizdaw-gpui/src/control.rs` and the static key bindings. All core calls that can block (record, stop, recover, waveform reads) stay off the UI thread or are the same short session calls the poll already makes.

**Tech Stack:** Rust 2024; GPUI at the slice 1 pin; `core-ffi`, `core-control`, `vizctl`.

**Spec:** `docs/superpowers/specs/2026-11-20-cross-platform-27-1-design.md`. Parity rows: `docs/superpowers/specs/2026-11-22-parity-checklist.md` ("### Record", "### Transport" recording rows, "### Step", "### Locators", Alerts "Recovered Recordings"). Swift reference with exact strings and file:line: the slice 8 inventory (`slice8-inventory.md`, §1.1–1.2, §1.4 model rules, §1.5–1.7, §3, §4, §5, §7, §8.1–8.3, the 8a list in §8.4). Slice 7c plan and Outcome for the house style of rulings and carries.

## Global Constraints

- All slice 7a–7c Global Constraints apply: the Swift app is retired (parity with its behaviour, no compatibility); exact Swift strings with the inventory's file:line; UI says "track", never "lane"; no banned vendor/DAW product names in repo text; nothing slow on the UI thread; GPUI tests press `secondary-` keys; Linux CI runs the GPUI tests and the offline backend (`VIZDAW_AUDIO=offline`), so every 8a e2e port must run on Linux (fake input `VIZDAW_AUDIO_INPUT=fake`, MIDI by `midi.inject`).
- Controller rulings binding 8a:
  - The "### Locators" rows are in slice 8: 8a carries `locator.go_N` / `set_N` / `clear_N` (model, commands, keys 0–9, ⌥0–9, ⌥⇧0–9), including ⌥2 for the step-entry e2e.
  - Rows that pass on Linux with the offline device get the Linux column ticked; add the Linux-gaps rows "live audio/MIDI input → H3" and "Move to Trash for recovered recordings (`record.recover action: trash`) → H4" (on Linux the file stays and the problem text `…: not moved to the Trash (this platform has no Trash support yet)` is the answer; the e2e asserts it there).
  - Field keys follow Swift: P, E and U with the item folded open » at that field (8a's model returns the folded target; 8b draws it). `The window shows no transport fields.` only when no field item is on the bar or in ».
  - `audio.set` steps stay with the Settings slice; the 8a e2e ports skip them (none of the three uses them).
- Step-entry keys win only while step entry is on: `.`, `0`–`9` and Delete keep their global meaning (Next Marker, locators, delete selection) when it is off; with "reserve number row" on, the number row stays with the locators even during step entry (`bindings.rs:402-416`). Only `step.rest`, `step.tie`, `step.delete_last` repeat while held.
- The punch-draft rule: with a punch draft open (popover or bar field) and not recording, every Record path (key, menu, channel, and the model call the 8b button will use) refuses with `Press Return to set the punch, or Esc to discard it.` before the text-field gate, and the refusal neither ends the field's edit nor closes the popover (`close_allowed(press_on_record, draft_open)`).
- The poll reads the record state while playing, recording or counting in (counting in reports `playing: false`), and polls fast while recording, counting in or metering when stopped; every Record, `record.set` and keep/discard reads the state quietly first (a pass can end by itself).
- Save while recording: Save As → `Stop recording to use Save As: the take is being written into this project.`; untitled Save → `Stop recording to save this new project: its take is being written into it.`; titled Save saves and the poll shows `Saved. The take is saved when recording stops.`; closing cancels the owed save.
- `record.recover` never opens the Recovered Recordings alert; the alert never shows while the channel is on (7b rule).
- Slice 8 rulings for 8b/8c (not 8a work): the track input picker and monitor button go in 8c; takes (choose/rename/delete) go to a later slice; the Linux input-opening status is `Opening the audio input…` with the macOS-only microphone sentence omitted (no new string; 8c); the Customize Transport Bar sheet stays with the Settings slice; 8b's » entries open at the field (above).

## Review Focus

1. Key routing: no step chord fires while step entry is off, no global `.`/digit/Delete fires while it is on (except the reserved number row), and a text field still types them; the keypad twins stay a logged gap.
2. The punch draft refuses Record on every path with one message, before the text gate, without closing or ending the edit; a refused draft is dropped and the field shows the old value.
3. Passes: a pass ending by itself, keep/discard outside loop recording, a second Record press stopping, and one undo step `Record` at Stop; `record.set` is all or nothing (validate, then `loop` first).
4. Poll: fast while counting in or recording with `playing: false`; no fast poll when idle; recovery re-offer from the poll never fires under the channel.
5. Saves while recording refuse with Swift's strings, a titled save during a pass is reported once, and a close cancels it.
6. Step entry ends at once (status line) when the shown sequence changes or its track goes, and `state.get editor` hides an ended one.

---

### Task 1: the session seam

**Files:** `crates/vizdaw-app/src/session.rs`, `crates/vizdaw-app/src/testing.rs` (`FakeSession`); tests `crates/vizdaw-app/tests/session_record.rs`.
**Produces:** on `DocSession` and the fake, every call in inventory §8.1-1 (`record`, `keep_pass`, `discard_pass`, `record_state`, `recording_waveform`, `recovered_recordings`, `move_record_arm`, `click`, `run_click`, `step_entry`, `run_chore`, `input_info`, `input_levels`, `inject_midi`, `saves_after_pass`, `cancel_save_after_pass`, `save_as_refusal`) and the free fns (`step_state_new`, `step_parse_pitches`, `audio_settings`). The fake scripts passes (on/off, pass/kept/discarded counters, ended-by-itself, counting in), a click row, input info/levels, arm moves with the core's refusal texts, and Busy.
- [ ] Tests: seam round-trips on the real session (offline, fake input) and the fake's scripted pass ending by itself; implement; commit `app: record, step and click calls on the session seam`.

### Task 2: transport marks, punch fields and locators

**Files:** `crates/vizdaw-app/src/editor/layout.rs` (`TransportMarks` under key `transport`), `crates/vizdaw-app/src/transport_fields.rs`, `crates/vizdaw-app/src/commands.rs` (`FieldFocus::Punch`, `TRANSPORT_COMMANDS`); tests `crates/vizdaw-app/tests/{transport_fields.rs,transport_commands.rs,editor.rs}`.
**Produces:** `TransportMarks` (10 locators, punch in 0 / out `4 * 61_440`, punch off, overdub **on**; tolerant decode, ticks within ±2^40, punch kept only when out > in; no undo step); `TransportField::{PunchIn, PunchOut}` with Swift's commit rules and strings (`Punch Out must be after Punch In.`, `The punch range must end by the last position.`, in-moves-out-by-a-bar), Tab order Position → Edit In → Edit Out → Punch In → Punch Out; `transport.overdub` / `transport.punch` (flip or `on`); `transport.go_punch` with the field target (row, or folded → open » at the field); `locator.go_N` / `set_N` / `clear_N` with `Locator N is not set.`, `Locator N cannot be set at this position.`, `There is no locator N.`, enabled without a sequence.
- [ ] Tests: Swift `TransportFieldsTests` punch rows (`punchFieldsKeepTheirOrder`, `aPunchRangePastTheLimitIsRefused`, Tab with punch, `fieldKeysFocusTheirField` U), `TransportCommandsTests` `modesToggleOrTakeTheirFlag`, `locatorsAreSetJumpedToAndCleared`, `locatorRefusalsMenusAndStopMatchTheirRun`; layout round-trip and tolerant decode; folded-field target for P/E/U; implement; commit `app: transport marks, punch fields and locators`.

### Task 3: the record model

**Files:** `crates/vizdaw-app/src/record.rs` (new: `RecordModel`, readouts), `crates/vizdaw-app/src/{transport.rs,document.rs}` (poll, `SaveAction` refusals), `crates/vizdaw-app/src/document/recovery.rs` (re-offer from the poll); tests `crates/vizdaw-app/tests/record.rs`, `crates/vizdaw-app/tests/document.rs`.
**Produces:** `spec()` from the marks and document loop; `toggle` (quiet refresh, stop finishes the pass, refusals `Select a sequence first.`, `Step entry is on: turn it off to record.`, gesture open, draft open); `press(keep)` with `Keep and discard act on loop passes while recording.`; punch drafts (popover and bar, focus-leaving rules, `close_allowed`); readouts (pass text, time left from `record_state().free_bytes` minus 100 MiB with Swift's four texts, overdub note, mode title/label/help, count-in toggle off ↔ last non-zero bars); defaults from `audio_settings()`; popover host per window (document or mixer; opening one closes the other); poll integration (refresh while playing/recording/counting in, fast interval while recording, counting in or metering); Save/Save As refusals and the saved-during-pass status; `cancel_save_after_pass` on close.
- [ ] Tests: Swift `RecordModelTests` (non-recovery rows) and the model rows of `RecordOptionsTests` (drafts refuse Record on every model path, closing drops drafts, close veto, time left for every depth and mono, loop counters, overdub note, save while recording ×5, text editing only while a punch field has focus); poll fast while counting in with `playing: false` and slow when idle; implement; commit `app: the record model`.

### Task 4: recording, click, arm and marker commands

**Files:** `crates/vizdaw-app/src/commands.rs`, `crates/vizdaw-app/src/record.rs`; tests `crates/vizdaw-app/tests/{transport_commands.rs,record.rs}`.
**Produces:** `transport.record` (draft refusal first, then the text gate), `record.keep_pass` / `discard_pass` (enabled only while loop recording), `transport.click` / `set_click_level` / `set_click_mode` (menu/key refusals `Click Level needs a level, such as -12 dB.`, `Click Mode needs a mode: play_and_record or record_only.`; click not text-gated), `transport.arm_previous` / `arm_next` (undo `Move Record Arm`, `Record arm did not move.` fallback, none armed → ⌥⇧↑ arms the last track; not text-gated in menu/channel), `record.set` (`Stop recording first.`, range checks before applying, loop first), `record.recover` (`There are no recovered recordings.`, `Recovery Failed`), `marker.add/rename/remove` as chores (`refresh_selection`), `midi.inject` (hex refusal, `Play or record first: this document does not hold the audio engine.`, `MIDI Injection Failed`); `enabled` / `is_on` for all.
- [ ] Tests: Swift `TransportCommandsTests` `recordArmMovesAndARefusalShows`, `busyIsBusyAndAnOpenDragBlocksArming` (arm half), `clickTogglesAndShowsItsCheck`, `clickLevelAndModeReachTheSession`; `RecordModelTests` `record.set` all-or-nothing and refused while recording, `record.recover` side-effect free, `midi.inject` rows; marker chores and their undo names; implement; commit `app: recording, click, arm and marker commands`.

### Task 5: step entry and its keys

**Files:** `crates/vizdaw-app/src/step_entry.rs` (new: `StepEntryModel`, `StepCommands`), `crates/vizdaw-app/src/prefs.rs` (reserve-number-row flag, default off; its Keys settings UI is the Settings slice), `crates/vizdaw-gpui/src/{menus.rs,shell.rs}` (a `StepEntry` key context on the document window while step entry is on; step chords bound in `DocumentWindow && StepEntry && !TextInput` after the global ones; number row skipped when reserved); tests `crates/vizdaw-app/tests/step_entry.rs`, `crates/vizdaw-gpui/tests/step_keys.rs`.
**Produces:** `step.entry` (start rules `Stop playing to enter steps.`, `Select a sequence with a track first.`; target = selected track of the shown sequence else its first), the note values, dot, triplet, tie, rest, delete_last (`Step entry is off (Step > Step Entry).`, playing refusal uses the key up, `Step entry ended: the sequence changed.` / `Step entry ended: its track is gone.`), `step.notes` (pitch parse, velocity `100 << 9`), the summary text (`Step Entry · <track or "Track <id>"> · 1/8[ dotted][ triplet]`), the editor-state row; repeats only for rest/tie/delete_last.
- [ ] Tests: Swift `StepEntryTests` (minus keyboard slots), `KeyBindingsTests` step lookup and reserve number row; GPUI: with step entry off `.` is Next Marker, `2` goes to locator 2 and Delete deletes the selection; on, they are Dotted, Half Note and Delete Last Step; with reserve on, `2` stays the locator; a focused text field types all three; implement; commit `app, gpui: step entry and its keys`.

### Task 6: the control channel and e2e

**Files:** `crates/vizdaw-gpui/src/control.rs` (and `control/` as needed), `crates/vizctl/tests/e2e_gpui.rs` (helpers `wait_for_record`, `lane_named` copied from `e2e.rs:967-1000`), `docs/superpowers/specs/2026-11-22-parity-checklist.md`; tests `crates/vizdaw-gpui/tests/control.rs`.
**Produces:** every 8a id answered (no `not_yet`); `state.get record` and `state.get input`; `state.get editor` real `step` and `marks`; `state.get transport` `click`; `ui.key` lookup with `step_entry` and the reserve flag (`G/control.rs:1269`); e2e `gpui_records_audio_loop_takes_and_midi`, `gpui_arpeggiates_an_injected_chord_while_recording`, `gpui_keys_drive_step_entry_and_locators` (keys t, 4, 0, delete, p, ⌥2, return, 2 and `step.notes`; the bindings-file part waits for Keys settings); the controller runs Linux over ssh and ticks Mac and Linux for the rows that pass, and adds the two Linux-gaps rows (H3 live input, H4 Trash) and the step keypad twins to the keypad gap row.
- [ ] Tests (each id's success and refusal kind; hostile params for `record.set`, `record.recover`, `marker.*`, `midi.inject`, `set_click_level` `"6"`; `record.recover` under the channel shows no alert); implement; e2e green on both platforms; commit `gpui: recording and step entry over the control channel`.

## Outcome (2026-11-24)

All six tasks done (commits on `next` from 3652c23b to a5c1b384). Every recording, step-entry, click, record-arm, punch, overdub, locator and marker command works from the keyboard and the control channel in the GPUI app on the Swift model; `G/control.rs` answers `not_yet` for no slice 8 id. Evidence: vizdaw-app 1005 and vizdaw-gpui 547 green on the Mac at the end, app+gpui 1551/1551 on Linux; GPUI e2e 32/32 on the Mac (the three ports plus `gpui_record_recover_trash_over_the_channel`); the Linux e2e runs in CI (the test box has no X server). Final whole-branch review: ready, no Critical/Important.

**Not yet seen on screen or on a real device:** ⌥⇧digit (Clear Locator) on a US Mac (`alt-@` aliases, US layouts only; other layouts have no key), and every view (8b).

### Rulings

- Task 1: Ruling: audio_input_channels / midi_source_ports left off the seam — not in the brief; the input picker is 8c, which adds them — cost: 8c adds two seam calls
- Task 1: Ruling (implementer, accepted): step_state_new/step_parse_pitches/audio_settings are CoreCalls default methods — cost: the fake core can't script audio_settings
- Task 2: Ruling (implementer, accepted): a field key with no item holding its field anywhere answers "The window shows no transport fields." (controller ruling, Swift opens » at a made view) — cost: one variant if Swift's case is wanted
- Task 3: Ruling: reviewer Minor 'close prompt offers Don't Save while a save is owed' re-graded Important (data loss: the take the user saved is dropped) — fix round 1 to the implementer; Swift sheetText(savePending:)
- Task 3: Ruling (implementer, accepted): RecordDefaults (Swift 0/0/false) since AudioSettingsRow lacks count-in/pre-roll/MIDI-passes — Settings slice calls apply_record_defaults — cost: preferences ignored until then
- Task 4: Ruling (implementer, accepted): record.recover with no action refuses (registry wording) instead of Swift's quiet Trash — safer; cost: a string differs from Swift
- Task 5: Rulings (implementer, accepted): repeat inferred without GPUI's repeat flag (lost key-up swallows one press); Delete = Delete Last Step while step entry is on even in Piano Roll/List (Swift parity); keypad twins unbound
- Task 6: Rulings (implementer, accepted): Trash that moved nothing answers failed (core already cleared the offer; file kept); record.recover reads idle news itself (IdleNews.problems added; other lines that tick dropped as before)
- Final: Ruling: Stop finishing a pass can block the UI thread up to 2 s (FINISH_WAIT) — stands: Swift parity and the existing Play/Stop pattern; the engine stops at once, only the take's finish waits — cost: a hitch at Stop after long passes
- Final: Ruling: file.close over the channel keeps a save made during recording — stands: the safer reading (no take lost) — cost: a channel close may save when Swift would not
- Final: Ruling: a step key in the same frame as T uses the old key context — stands: not humanly reachable; the channel reads the model — cost: none in practice
- Final: Ruling: transport.go_punch answers "The window shows no transport fields." until 8b draws the punch item — by plan

### Carried

- Task 2: carry to Task 5: GPUI test must press 2, ⌥2 and ⌥⇧2 (locator go/set/clear) with step entry off — reviewer could not verify GPUI dispatch of locator keys (⌥⇧digit on macOS unproven)
- Task 2: carry to 8b: Overdub/Punch menu check marks; punch label widths placeholders
- Task 3: carry to Task 4: punch-draft refusal ahead of the text gate in transport.record's menu path; replace the set_step_entry_on stand-in in Task 5; metering fast poll in 8c
- Task 4: carry to Task 6: recover_from_channel must match action exactly (import|trash, else refuse) — today any other text means Trash (registry blocks it, but data-loss class); GPUI test that R does not record while a punch field has focus; channel routes RECORD_COMMANDS
- Task 4: carry to 8b: Click/Overdub/Punch menu check marks
- Task 5: carry to Task 6: wire step_state_row into state.get editor
- Final: carry to 8b: bar punch field calls bar_punch_ended on every end path incl. focus loss; set_punch_text ignores writes while the popover is closed; Overdub/Punch/Click menu check marks; punch label widths
- Final: carry to Settings: apply_record_defaults for new documents; Keys toggle for reserve number row

### Deferred minors

- Task 1: minor (deferred): SAFETY comment overclaims; fake record skips some refusals; fake next_pass during count-in; arm_move duplicated; fake click-level text differs; weak input_levels assertion; pre-existing untitled-save text differs
- Task 3: minor (deferred): normal close question text differs from Swift (pre-existing: 'Save changes to “X”?'); quit text adds 'Some documents have unsaved changes.'; no test of quit wording or normal Save/Don't Save mapping
- Task 3: minor (deferred): 4 Minors in task-3-review.md
- Task 4: minor (deferred): other Minors in task-4-review.md
- Task 5: minor (deferred): Linux US variants (intl/dead keys) rejected; Windows KLID ids rejected; other Minors in task-5-review.md
- Task 6: minor (deferred): retry after failed Trash says no recoveries until reopen; Linux e2e text needs a build without test-hooks (CI rebuild ok); GPUI e2e launch doesn't default VIZDAW_TRASH_DIR; channel_delete_is_delete_last_step_over_the_roll passes without its code; other Minor
- Final: minor (deferred): a pass stopping by itself never says why (core lines dropped by idle) — folded into the 8b Faults backlog row; step entry can start during a count-in
