# 27.1.0 Slice 8b: The Transport Bar and Menus for Recording Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The GPUI transport bar draws every recording item on 8a's models, as the Swift bar did: the Record part of the cluster with its readouts, Replace/Overdub, Count-in, Record Options and its popover, Punch In and Out, Click with its Level/Mode menu, DSP Load, Audio Device, Faults with its popover, the Step Entry summary and command buttons, each with its » entry. The Transport menu shows the Overdub/Punch/Click check marks and the Step menu exists. `Session.idle()` problems reach the user through Faults instead of being dropped. After 8b the standard bar drops no item, and the bar steps of `transport_jumps…` (step 7) and `groups_vcas_and_transport_bar…` pass on the Mac and on Linux with the offline device. Inputs, monitor, meters, the growing take and the Markers pane are 8c; takes come later.

**Architecture:** Views only, on 8a's `RecordModel`, `TransportMarks`, `StepEntryModel` and commands (`crates/vizdaw-app/src/{record.rs,step_entry.rs,commands.rs,transport_fields.rs}`). The app crate gets the few texts and faces the views need (fault lines, DSP and device text, click menu entries, `CommandButtonFace`). `vizdaw-gpui/src/transport_bar.rs` gains a `Piece` for each item. The two new overlays (record popover, Faults popover) and the » field popovers follow the » menu's `deferred(anchored())` pattern, and they show in `ui.tree`. The idle tick keeps `IdleNews.problems` on the document (Swift `pluginProblems`) instead of dropping them.

**Tech Stack:** Rust 2024; GPUI at the slice 1 pin; `core-ffi`, `core-control`, `vizctl`.

**Spec:** `docs/superpowers/specs/2026-11-20-cross-platform-27-1-design.md`. Parity rows: `docs/superpowers/specs/2026-11-22-parity-checklist.md` ("### Transport" rows marked "8b", "### Step" "the Step menu is 8b", e2e `groups_vcas_and_transport_bar_over_the_channel`, Manual checks). Swift reference with exact strings and file:line: the slice 8 inventory (`slice8-inventory.md` §1.1, §1.3, §1.4, §1.5, §7 rows "8b", §8.3, the 8b list in §8.4). Plan 8a's Outcome (Rulings, Carried "to 8b", Deferred minors). Backlog row "Idle problems never reach the user" (`docs/feature-backlog.md`).

## Global Constraints

- All slice 7a–8a Global Constraints apply: the Swift app is retired (parity with its behaviour, no compatibility); exact Swift strings with the inventory's file:line; UI says "track", never "lane"; no banned vendor/DAW product names in repo text; nothing slow on the UI thread; GPUI tests press `secondary-` keys; Linux CI runs the GPUI tests and the e2e on the offline backend (`VIZDAW_AUDIO=offline`, `VIZDAW_AUDIO_INPUT=fake`, MIDI by `midi.inject`).
- Controller rulings binding 8b:
  - » opens at the field (Swift): P, E and U with their item folded open » with that item's live fields and the field focused. `The window shows no transport fields.` shows only when no item holding the field is on the bar or in » (8a Task 2 ruling stands; Swift's made view for an absent item is not ported).
  - The Customize Transport Bar sheet and `audio.set` stay with the Settings slice. Inputs, the monitor button, meters, the growing take and the Markers pane are 8c. Takes come later.
  - Each item's » entry lands with its view task, in Swift's form (`M/TransportItemEntries.swift:96-143`): Replace/Overdub and Count-in are check actions, disabled while recording; Record Options, Position, Edit and Punch fields are popovers; Summary, DSP Load and Audio Device are `<title>: <text>` texts; Click is a submenu; Faults is an action; a command button is an action with its face's label.
- The Record button calls `DocumentModel::press_record` (Swift `pressRecord` → `RecordModel.toggle()`), never `commands::run`. Its order is Swift's: quiet refresh, stop if recording, sequence, step entry, open gesture, punch draft. It is not text-gated. A press on it with a punch draft open neither ends the bar field's edit nor closes the popover (`RecordModel::close_allowed(true, draft_open)`).
- Popovers are in-window overlays, one per window. They close on Esc, on a click outside, or on their opener, apart from a press on Record with a draft open. They are reachable in `ui.tree` by Swift's labels; no Swift string changes.
- Idle problems are kept, never dropped: newest last, at most 50 (Swift `DocumentModel.swift:852-855`). `record.recover` reads the lines its own call added and leaves them in the list.
- Look: the record colour is red while recording (count-in too), and Faults uses the error tint with faults, else the warning tint. These are theme tokens, not literals. Show-finished-UI rule: screenshots go to the owner only once the bar resembles the approved look. The controller runs Linux over ssh and makes the on-screen macOS checks; every look item gets a manual-check row.
- Review pace: fix crash, data loss and wrong overwrite; log other edge cases to the backlog.

## Review Focus

1. The punch draft. No end path of the bar's Punch In/Out may leave a stale `bar_punch_draft`, which would refuse Record for good. The end paths are Return, Esc, Tab and ⇧Tab, a click elsewhere, the field folding away, the keyboard leaving the window, and the window closing. A press on Record with a draft open is the one press that keeps the edit. `set_punch_text` writes nothing while the popover is closed.
2. The Record button follows Swift's order (`press_record`), stays enabled while a field types, and shows its refusal in the status line. The R key stays text-gated.
3. Idle problems: none are dropped (including on the `record.recover` path), the cap is 50, and the idle tick redraws when lines arrive. DSP Load polls at most 4 times a second, and only while it shows.
4. The Step menu: the step keys it shows must not fire through the native menu while step entry is off (`2` stays locator 2, `.` stays Next Marker), and locators must not fire while it is on.
5. Popover hosts: opening in the Mixer window closes the document window's popover and drops its drafts; Faults is document window only.
6. » at the field: a folded field opens live and focused, and committing it there acts as it does on the bar.

---

### Task 1: idle problems and the Faults item

**Files:** `crates/vizdaw-app/src/document.rs` (problem list, `take_idle_news`, `idle`), `crates/vizdaw-app/src/record.rs` (`recover_from_channel`), `crates/vizdaw-app/src/close_prompts.rs`, `crates/vizdaw-gpui/src/transport_bar.rs` (`Piece::Faults`, popover), `crates/vizdaw-gpui/src/shell.rs`; tests `crates/vizdaw-app/tests/{document.rs,record.rs,close_prompts.rs}`, `crates/vizdaw-gpui/tests/transport.rs`, `crates/vizctl/tests/e2e_gpui.rs`.
**Produces:**
- `take_idle_news` appends `IdleNews.problems` to the document's list: newest last, oldest dropped past 50. `idle()` answers true when lines arrive.
- `fault_lines()` is `transport.faults + transport.report + problems` (Swift `messages`). `has_faults()` is true with transport faults. `has_report()` is true with a report or problems.
- `shows_faults` is per document (Swift `DocumentModel.showsFaults`).
- `record.recover`'s trash answer uses the not-moved lines its call added, and keeps them in the list.
- The Faults item (`V/TransportItemViews.swift:221-243`) shows only with faults or a report: button `Faults`, help `Faults and compile messages`, error tint with faults, else warning. A click toggles a popover (min width 240) with one line per message. It is document window only (`document_only`). Its » entry is an action, enabled with faults or a report.
- This is how plug-in load failures, `autosave failed: …`, media read errors and why a pass stopped by itself (`Recording stopped: …`, `The recording could not be placed: … Its files are kept.`, the input-gap line) reach the user.
- [ ] Tests:
  - App: lines are kept in order and capped at 50, and `idle()` is true on arrival (`FakeSession::set_idle_problems`).
  - App: a scripted pass that ends by itself leaves its `Recording stopped: …` line in `fault_lines()`.
  - App: `record.recover action: trash` with a not-moved line answers it and keeps it.
  - GPUI: there is no Faults button with no lines; a problem line gives the warning tint, a transport fault gives the error tint, and the popover lists all three kinds; the Mixer window has no Faults.
  - e2e: on Linux `gpui_record_recover_trash_over_the_channel` also finds the `…: not moved to the Trash (this platform has no Trash support yet)` line in the Faults popover (`ui.tree`).
  - Then implement and commit `app, gpui: idle problems reach the Faults popover`.
- [ ] The close question (`M/ClosePrompts.swift:23-26`):
  - `close_prompt` reads `Do you want to save the changes you made to “X”?` / `Your changes will be lost if you don't save them.`; the save-owed case keeps `SAVE_PENDING_INFORMATIVE` and has no Don't Save.
  - Update `tests/close_prompts.rs`.
  - The quit alert differs from Swift in more than a string (Swift asks one song's sheet or Review Changes…/Discard Changes, with an empty informative text when nothing waits). Log it as a backlog row, not a fix.
  - Commit `app: the close question in Swift's words`.

### Task 2: the Record part of the cluster, Replace/Overdub, Count-in and the Step Entry summary

**Files:** `crates/vizdaw-gpui/src/transport_bar.rs` (`BarState` record fields; `Piece::{ReplaceOverdub, CountIn, Summary, Separator}`; the cluster's width counts Record), `crates/vizdaw-gpui/src/shell.rs` (`BarState` from the document); tests `crates/vizdaw-gpui/tests/transport.rs`.
**Produces:**
- The Record button sits after Play/Stop (`V/RecordControls.swift:12-50`). It reads `Record` / `Stop Recording`, with help `Record the armed tracks (R)` / `Stop recording (R)`, and is red while recording.
  - It is enabled by `record_button_enabled()` and is not off while a field types.
  - Its mouse-down does not take the keyboard: with a punch draft open, the field keeps it.
  - A press calls `press_record()`.
- Cluster readouts (`passText`, `remainingText`):
  - `Count-in`, in the record colour, while counting in.
  - The pass text while loop recording (`Pass N · K kept · D discarded`).
  - The time left, with help `Recording time left on the project's disk for the armed audio tracks`.
- Replace/Overdub (`V/RecordControls.swift:54-64`): the title is `record_mode_title()` (a new document reads `Overdub`), on means overdub, and a11y is `Record mode: Replace|Overdub`. Help: `Replace removes the notes in the recorded range; Overdub keeps them (transport.overdub)`. It is disabled while recording, and a press calls `press_record_mode()`.
- Count-in: a toggle, on when bars > 0, help `Count in before recording (the bars are in Record Options)`, disabled while recording; a press calls `toggle_count_in()`.
- The Step Entry summary: an accent pill with `step_summary()` while step entry is on, its a11y the text.
- Separators are drawn.
- » entries for these items, as in Global Constraints.
- [ ] Tests:
  - The tree names the cluster controls as the Swift bar does.
  - Record is enabled while a field types, and refuses into the status line (`Step entry is on: turn it off to record.`).
  - `Stop Recording` shows and stops a scripted pass; the pass text and the time left come from a scripted `RecordStateRow`; `Count-in` shows while counting in, with `playing: false`.
  - Replace/Overdub flips `transport.overdub` and is disabled while recording.
  - Count-in goes off and back to its last bars.
  - The summary follows the value, dot and triplet, and the » entries match Swift.
  - Then implement and commit `gpui: Record, Replace/Overdub, Count-in and the step summary on the bar`.

### Task 3: Record Options and its popover

**Files:** `crates/vizdaw-gpui/src/record_popover.rs` (new), `crates/vizdaw-gpui/src/transport_bar.rs` (`Piece::RecordOptions`), `crates/vizdaw-gpui/src/{shell.rs,mixer_view.rs}` (a host per window), `crates/vizdaw-app/src/record.rs` (`set_punch_text` guard); tests `crates/vizdaw-gpui/tests/record_popover.rs`, `crates/vizdaw-app/tests/record.rs`.
**Produces:**
- The chevron `Record Options`, with help `Replace or overdub, punch, loop, count-in, pre-roll and MIDI takes`, calls `toggle_record_popover(host)`. The host is `PopoverHost::Document` or the Mixer window's; opening in one closes the other.
- The popover is anchored under the chevron, 340 wide, has no title and is disabled while recording. It holds the inventory §1.4 rows in order:
  1. Mode segments `Replace` / `Overdub`.
  2. The overdub note (`record_overdub_note()`).
  3. Toggle `Punch`, with fields `Punch In` / `Punch Out`, disabled unless Punch is on. Each keystroke calls `set_punch_text`; Return calls `commit_punch`; Esc calls `drop_punch`; focus changes call `punch_focus_changed`.
  4. Toggle `Loop`.
  5. `Count-in: off|1 bar|N bars` (0–4) and `Pre-roll: …` (0–8).
  6. Toggle `MIDI passes as takes`, with the `MIDI_TAKES_HELP` help.
  7. `Disk` with the time left.
- It closes through `set_record_popover(false)`, which drops the drafts, except for a press on a Record button with a draft open.
- The 8a carry: `set_punch_text` ignores writes while the popover is closed.
- The » entry is a popover titled `Record Options`.
- [ ] Tests (Swift `RecordOptionsTests` view rows):
  - The popover sets the modes and agrees with the bar switch, and the steppers clamp and read Swift's texts.
  - Loop toggles the document's loop.
  - Opening in the Mixer window closes the document's popover and drops its drafts.
  - Closing (Esc, a click outside, the chevron) drops the drafts.
  - A draft refuses Record from the bar button, the R key, the menu and the channel with `Press Return to set the punch, or Esc to discard it.`; after the refusal the popover is still open and the field still has the keyboard.
  - Moving from Punch In to Punch Out commits; leaving the pair drops.
  - Everything is disabled while recording; the overdub note shows only with overdub on and an armed audio track.
  - App: `set_punch_text` while the popover is closed leaves no draft.
  - Then implement and commit `gpui: the record popover`.

### Task 4: Punch In and Out on the bar, the field keys and » at the field

**Files:** `crates/vizdaw-gpui/src/transport_bar.rs` (`Piece::PunchFields`; measured label widths; » field popovers), `crates/vizdaw-gpui/src/transport_fields.rs` (bar draft reporting, end paths, P/E/U through `field_key`), `crates/vizdaw-gpui/src/control.rs` (`transport.go_punch`), `docs/superpowers/specs/2026-11-22-parity-checklist.md`; tests `crates/vizdaw-gpui/tests/transport.rs`, `crates/vizctl/tests/e2e_gpui.rs`.
**Produces:**
- The Punch In and Out item, with labels `Punch In` / `Punch Out`. The 56/64 placeholder widths become widths measured from the labels at the caption size (the 8a carry).
- Tab goes Position → Edit In → Edit Out → Punch In → Punch Out.
- Each change in a bar punch field calls `bar_punch_typed`. Every end path calls `bar_punch_ended`: Return, Esc, Tab/⇧Tab, `on_mouse_down_out`, the fold in `set_shown_fields`, the keyboard leaving the window (blur or window deactivation) and the window closing.
- The press on Record with a draft open is exempt from `on_mouse_down_out` (`close_allowed`).
- P/E/U go through `transport_fields::field_key`:
  - `OnRow` focuses the field.
  - `Folded(item)` opens » with that item's fields live and the field focused.
  - `None` sets the status `The window shows no transport fields.`
- In », Position, Edit In and Out and Punch In and Out are popovers with live fields (Swift `.popover`). Main View's » popover stays text: log it as a backlog row.
- On the channel, `transport.go_punch` answers as P and E do.
- [ ] Tests:
  - The tab order with punch.
  - A typed bar Punch In makes the Record click refuse with the draft message; the field keeps the keyboard and the draft, and Return then sets `punch_in`.
  - For each end path (including focus leaving for another window), the draft is cleared and Record is no longer refused.
  - U focuses Punch In; U with punch folded opens » at Punch In, and typing plus Return sets it; with no punch item, U gives the status.
  - The labels do not clip.
  - The channel's `go_punch`.
  - e2e: step 7 of Swift `transport_jumps_readouts_and_fields_over_the_channel` (`X/e2e.rs:3164-3200`) added to `gpui_transport_jumps_readouts_and_fields_over_the_channel`, including the channel's `transport.record` refusal.
  - Then implement and commit `gpui: punch fields on the bar and » at the field`.

### Task 5: Click, DSP Load, Audio Device and command buttons

**Files:** `crates/vizdaw-app/src/bar_items.rs` (new: `dsp_text`, `device_text`, click menu entries, `CommandButtonFace`, command availability), `crates/vizdaw-gpui/src/transport_bar.rs` (`Piece::{Click, DspLoad, Device, Command}`), `crates/vizdaw-gpui/src/shell.rs`; tests `crates/vizdaw-app/tests/bar_items.rs`, `crates/vizdaw-gpui/tests/transport.rs`, `crates/vizctl/tests/e2e_gpui.rs`.
**Produces:**
- Click (`V/TransportItemViews.swift:295-406`): a toggle `Click` (tooltip `The metronome on or off`) plus ⌄ (a11y `Click Options`, tooltip `Click level and mode`). A right-click on either, or the ⌄, opens `Level ›` and `Mode ›`:
  - Levels `0 dB` through `−60 dB`, titles with U+2212 and `-12` on the wire, ✓ within 0.05 dB.
  - Modes `Play and Record` / `Record Only`.
  - Both parts are disabled while `click` is `None`.
  - From the channel the menu refuses with `Click's menu does not open for the control channel; use transport.set_click_level or transport.set_click_mode.`
  - The » entry is a submenu `Click`.
- DSP Load: `DSP n%` (`dspText`, clamped 0…99.99 and non-finite → 0), a11y `DSP Load`, fixed width for `DSP 100%`. It is polled through the mixer model's `poll_levels` every 0.25 s, only while shown.
- Audio Device: `No output`, or `<device> · <rate>` (`M/Formatting.swift:42-44`, with the rate text shared with `banners.rs`).
- DSP Load and Audio Device » entries are `<title>: <text>`.
- Command buttons (`M/CommandButtons.swift:58-110`):
  - Label: the given one, else the registry title. Tooltip and a11y: the title. Enabled as its menu item.
  - A stale id is greyed, with `This command is no longer in VizDAW`.
  - A click runs what its menu item runs (a dialog command opens its dialog); when nothing can run, the status says `<title> can't run from a button right now.`
- [ ] Tests:
  - Swift `theClickItemSetsLevelAndMode`, the `dspText` rows, the device text.
  - `TransportCustomizeTests` `staleCommandsStayGreyed`, `commandAvailabilityFollowsTheMenus` and `aDialogCommandFallsThroughToItsMenuItem`.
  - GPUI: the click menu's checks follow `state.get transport`; it is disabled while unknown; a channel click on ⌄ gets the refusal; a `command:window.mixer` click opens the Mixer.
  - e2e: the bar part of `groups_vcas_and_transport_bar_over_the_channel` in a new `gpui_transport_bar_click_and_command_buttons`: the command button opens the Mixer, `ui.reset_transport_bar`, `transport.click {on}`, `set_click_level "-12"`, `set_click_mode record_only`, `state.get transport` `click`, and the hostile level `"6"` is refused.
  - Then implement and commit `app, gpui: click, DSP load, device and command buttons on the bar`.

### Task 6: the Transport menu's checks and the Step menu

**Files:** `crates/vizdaw-gpui/src/menus.rs`; tests `crates/vizdaw-gpui/tests/{menus.rs,step_keys.rs}`.
**Produces:**
- `transport.overdub`, `transport.punch` and `transport.click` are checked by `commands::is_on` (the 8a carry).
- The Step menu goes after Transport (Swift `AppCommands.swift:354-365`): `Step Entry` ✓ | the six note values ✓ | `Dotted` ✓, `Triplet` ✓, `Tie`, `Rest`, `Delete Last Step`. Titles and keys come from the registry; checks from `step_is_on`.
- Enablement (`M/StepEntry.swift:187-191`): nothing while a text field has the keyboard. `step.entry` is enabled when on, or with a track and stopped. The rest are enabled when on and stopped.
- The step keys the menu shows do not fire through the native menu outside the `StepEntry` context.
- [ ] Tests:
  - The menu model follows Swift's order and dividers.
  - The checks follow `layout.transport` and the click row, and the step value, dot and triplet.
  - The Step menu's enablement in each state (off, on, playing, text field).
  - GPUI: with the Step menu built and step entry off, `2` still goes to locator 2 and `.` is still Next Marker; with it on, they are Half Note and Dotted.
  - Then implement and commit `gpui: the Step menu and the Transport check marks`.

### Task 7: manual checks, checklist and backlog

**Files:** `docs/testing/manual-checks.md`, `docs/superpowers/specs/2026-11-22-parity-checklist.md`, `docs/feature-backlog.md`.
**Produces:**
- A manual-checks section "Transport bar and recording views (GPUI slice 8b)", with Mac and Linux rows for the look:
  - The Record button's colours and Stop Recording.
  - The popover's anchoring, width and disabled state while recording.
  - The Click menu.
  - The Faults tints and popover.
  - DSP Load digits that do not move the row.
  - Device text and command button faces.
  - The README steps 130–142, 167, 228, 262 and 326–331, and the stop-reason line in Faults (README 1218).
- Checklist ticks for the rows that pass: the Record button, the check marks, `go_punch`, the Step menu cells, the `transport_jumps` and `groups_vcas_and_transport_bar` e2e rows.
- Backlog: close "Idle problems never reach the user", and add the quit-wording row (Task 1) and the Main View » popover row (Task 4).
- The controller runs the Linux GPUI tests and e2e over ssh and makes the macOS on-screen checks. Screenshots go to the owner only when the bar resembles the approved look.
- [ ] Commit `docs: slice 8b manual checks, checklist and backlog`.
