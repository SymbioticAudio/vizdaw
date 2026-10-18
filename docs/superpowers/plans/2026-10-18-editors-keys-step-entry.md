# VizDAW 4f — Keys and Step Entry Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The last plan of sub-project 4: every command of VizDAW (menu item or not) has a stable ID in one registry and can be bound to keys; bindings are saved, loaded and reset, and the menus show them; the parent spec's default keys work (unmodified single keys for the transport and the modes, P/E/U focusing the Position, Edit-In and Punch fields with bar·beat·unit entry, number keys for ten locators with Option to set and Option-Shift to clear, a preference that keeps the number row for locators, Option-Space and Shift-Space playing only or from the selection, Option-Shift arrows moving record arm, G for snap); and step entry (T) enters notes from MIDI input (a chord is every key down until the last is released) or typed pitches, with note values on the number keys and keys for dot, triplet, tie, rest and delete-last-step, keyboard slots entering references. Parked items assigned to 4f are picked up (the `thirtySecond` wire spelling, the stale channel status, the lane-name Escape gap, the drum row header's Escape).

**Architecture:** Rust first, so a later port only redraws (the user's guideline). `core-control` (no UniFFI) gains `bindings`: a `Chord` (a physical key code plus the slot bitmask), a `Keymap` over the registry (defaults, contexts, binding with conflict refusal or replacement, reset, a tolerant JSON document) — the registry itself becomes the table of every command (`menu_item`, `context`, `extra_keys`; 44 → 108 IDs). `core-engine` learns to stop playing at a frame (Play Selection). `core-editor` gains `transport` (what Play Selection plays, marker steps, record-arm moves) and `step` (the step entry state machine: `press` an input into an `Effect`, `commit` it once the document applied it; typed pitches). `core-ffi` exposes a `Keymap` object, `Session.play_from / selection_span / marker_step / move_record_arm / step_entry`, step records, `chord_text`, `key_code_for`, and note-offs from MIDI input. Swift keeps state and draws: `KeyBindings`, `KeyRouter`, `TransportMarks`, `TransportCommands`, `StepEntryModel` and `StepCommands` (`VizDAWAppModel`); `TransportFieldsView` and `KeyRecorderView` (`VizDAWEditors`); the key monitor, the menus from the bindings, the transport bar and Settings > Keys (`VizDAW`). One router decides what a key does for the keyboard and for the control channel's `ui.key`; one handler (`ControlCommands.run`) runs every command for keys, menus and the channel.

**Tech Stack:** Rust 2024 (`core-control`, `core-editor`, `core-engine`, `core-model`, `core-ffi` with UniFFI 0.32.2, `vizctl`, `serde_json`, `proptest`), Swift 6 / SwiftPM (SwiftUI shell, AppKit views, Swift Testing), macOS 14+.

**Spec:** `docs/superpowers/specs/2026-10-16-editors-design.md` (§4f — binding: "Command registry: every command (menu or not) has a stable identifier; bindings can be saved, loaded, reset; bound keys show in menus", "Defaults per parent spec §9 …", "Step entry: note values from number keys, keys for dot, triplet, tie, rest, delete-last-step; pitch from MIDI input"; §1 "4f registers the commands so keys can be bound now"; §3.5, §5, §6 "key binding persistence, step entry sequences"), parent `docs/superpowers/specs/2026-10-15-vizdaw-design.md` §9 "Keyboard" and §5 (keyboard slots), `docs/superpowers/specs/2026-10-18-control-channel-design.md` §6 ("Plan 4f extends it to non-menu commands and drives key bindings and menu titles from it"), §5.5, §7. Previous plans for conventions: `docs/superpowers/plans/2026-10-18-editors-drum-grid.md` (closest model), `2026-10-18-control-channel.md`.

## Global Constraints

- Every shell command runs after `export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools` (the Xcode license is not accepted, so `git`, `cc`, `swift` and the linker work only with the Command Line Tools; a `git commit` without `DEVELOPER_DIR` fails silently — check every commit with `git log -1`). Every bash block below starts with that line and `cd /Users/lattawb/workspace/vizdaw`.
- Work on branch `sp4-editors` (code head `2940bc5` before Task 1, plus this plan's commit). Commit at the end of every task, adding only the files the task lists (and `Cargo.lock` when a manifest changed; none does here); never push. End every commit message with the attribution lines your harness requires (the blocks below show `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>` and the `Claude-Session:` line).
- Clean room: never read or copy anything from `~/workspace/SVP/research` or the Studio Vision binary. The default keys below are VizDAW's own choices (Rulings 4, 12).
- Never use GUI or computer-use tools. Launch VizDAW only through `vizctl launch` or the e2e test (a throwaway home under `/tmp`), quit it when done (`vizctl quit`; `pkill -f "VizDAW --control"` if a test left one), check `pgrep -lf MacOS/VizDAW` prints nothing at the end of a task that launched it, and remove the `/tmp/vze*` homes your e2e runs left (the test keeps them for post-mortems).
- Rust stable, edition 2024; no new crate and no new dependency (`core-ffi` already depends on `core-control` and `core-editor`; `core-editor` on `core-model`). License GPL-3.0-only. `core-control`, `core-editor`, `core-ffi` keep `#![deny(missing_docs)]` and `#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic))]`; `core-control` and `core-editor` keep `#![forbid(unsafe_code)]`. Tests may `unwrap` and index. No new `unsafe`.
- File compatibility: `core_model::file::VERSION` stays 2; no project data changes. Locators, punch range and switches live in the package's `ui.json` (`EditorLayout.transport`, tolerant decoding: a project without them, or with damaged ones, reads with the defaults). The snap division writes `thirty_second` and still reads the `thirtySecond` that builds before 4f wrote (Ruling 1). Key bindings live in the preferences (`VizDAW.keys`, in memory under `VIZDAW_SUPPORT_DIR`) and in `.vizkeys` files.
- No panic crosses the FFI: every new exported function or method returns `Result<_, VizError>` and runs inside `ffi_guard`, or cannot fail (`Keymap.rows`, `lookup`, `slot_clash`, `reset*`, `to_json`, `step_state_new`, `step_length`, `chord_text`, `key_code_for`). No session `inner` lock is held across plugin code. The engine change stays real-time safe (one field and one branch; no allocation).
- UniFFI: new records, enums, functions and the `Keymap` object are new. Changed records: `NoteInRow` gains `on: bool` last with `#[uniffi(default = true)]` (Swift's memberwise initialiser keeps compiling); `EditorStateRow` gains `step` and `marks` (Swift builds it in one place, `ControlStates.state`); `KeyStrokeRow` loses `command` (the key bindings decide, Ruling 5; Swift never built one outside a test). `FakeSession` implements the whole generated `SessionProtocol`: it gains `playFrom`, `selectionSpan`, `markerStep`, `moveRecordArm` (Task 3) and `stepEntry` (Task 5) in the same task as the Rust methods.
- Swift: the generated bindings compile in Swift 5 mode; `VizDAWAppModel`, `VizDAWEditors`, `VizDAW` and the tests in Swift 6 mode with no new warnings (the `MemoryDefaults` Sendable warning is older). Never use SwiftUI's `@State` or `@FocusState` in new views; keep view state in `@Observable` models. Closures stored by views capture `[weak self]`. Models and pure logic are tested with Swift Testing; AppKit views get behaviour tests through internal entry points; the app target (`VizDAW`) has no tests — logic it needs lives in `VizDAWAppModel` or `VizDAWEditors`.
- Control channel (control spec §6, §7): programmatic calls never open dialogs; every command is in `core_control::registry::COMMANDS` (44 today; Task 7 makes it 92, Task 8 105, Task 10 108; the count test moves with them) and handled by `ControlCommands.run` (the test `everyRegisteredMenuCommandHasAHandler` compares the two).
- Generated files are never committed: `app/Sources/VizDAWCore/vizdaw_ffi.swift`, `app/Sources/vizdaw_ffiFFI/include/`, `target/ffi/`, `dist/`. Run `scripts/build-ffi.sh` before any `scripts/swift.sh` command after a Rust change.
- Each task ends green: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `scripts/build-ffi.sh`, `scripts/swift.sh build`, `scripts/swift.sh test`. Run `cargo fmt --all` first. Task 3 also runs `cargo clippy -p vizdaw-play --all-targets --features rt-check -- -D warnings` (it touches the engine). Tasks 9–12 also run `scripts/make-app.sh` and the e2e (`VIZDAW_E2E_APP=dist/VizDAW.app cargo test -p vizctl --test e2e -- --ignored --test-threads 1`). Baseline before Task 1: 864 Rust tests listed by `cargo test --workspace -- --list` (859 pass, 5 ignored), 609 Swift `@Test` functions, 3 e2e tests.
- Code in the steps is complete for new files (written whole with `cat > … <<'VIZDAW_EOF'`). Changes to existing files are exact **Find / Replace** excerpts of the tree the previous task left, in order; apply them with an editor (the Edit tool). Every excerpt was checked to be unique in that tree and to give exactly the tested file. If one does not match because a fix landed meanwhile, make the same change by hand and keep going.
- Spec values used verbatim: "every command (menu or not) has a stable identifier"; "bindings can be saved, loaded, reset"; "bound keys show in menus"; "unmodified single keys for transport and mode toggles"; "focus keys for position, edit-in and punch fields with bar·beat·unit entry"; "number keys jump to locators (modifiers set/clear; a preference reserves the number row)"; "modified Space plays from/only the selection"; "modified arrows nudge or move record-arm"; "snap toggle"; "note values from number keys, keys for dot, triplet, tie, rest, delete-last-step; pitch from MIDI input"; parent §5: a keyboard slot is "refused at assignment if it clashes with a command binding or another slot"; "Pressing it makes the sequence current and auditions it"; "in step recording the position then advances by the sequence's length"; 15 360 ticks per quarter note.

## Review Focus

The five failure modes a user is most likely to hit that the spec implies but no requirement spells out, each pinned by a named test in its owning task:

1. **Typing that turns into commands** — a letter typed into any text field (a sheet, the sidebar rename, a list cell, a lane or drum-row name, a transport field, a Settings field) while single unmodified keys are bound to transport and mode commands. Expected: the field always gets its plain keys, past the menus' single-key equivalents; ⌘ and ⌃ keys still run commands; a transport field never takes the keyboard on its own when its window opens. Pinned: Task 6 `textFieldsKeepTheirPlainKeys`; Task 10 `theFieldsViewCommitsRevertsAndNamesItsFields` ("not as the window's first text field", "Return gives the keyboard back"); Task 11 e2e `keys_drive_step_entry_locators_and_bindings` (T works at once after opening; ⌥2 after typing in the Position field sets the locator).
2. **Damaged or hostile bindings** — a hand-edited preference, a `.vizkeys` file from another version, with unknown commands, unreadable keys, five keys for one command, a key two commands want, a 300 KB file, not JSON. Expected: a bindings document loads what it can and lists what it skipped; anything else is refused whole and nothing changes; never two commands on one key in one context. Pinned: Task 2 `a_damaged_document_loads_what_it_can`, `hostile_documents_never_collide`, `binds_and_resets_never_collide`; Task 5 `bindings_change_save_load_and_reset`; Task 6 `bindingsPersistInThePreferences`, `filesSaveLoadAndRefuseDamage`.
3. **A step entry state that no longer fits the document** — the app's copy of the state names notes since deleted, notes in another lane, a position past the time limit, a history of 300 steps, 30 held keys; a reference to the sequence itself. Expected: refused with a `Model` error or applied to what is still there in this lane; never a panic, `Internal`, or a change to another lane; a refused step leaves the state and the transport where they were. Pinned: Task 4 `hostile_inputs_keep_the_bounds`; Task 5 `hostile_states_apply_or_are_refused`, `a_stale_state_never_touches_another_lanes_notes`, `a_step_whose_notes_were_deleted_still_goes_back`, `a_keyboard_slot_enters_a_reference_and_moves_by_its_length`, `refusals_leave_everything_as_it_was`; Task 8 `aRefusedStepShowsInTheStatusLine`.
4. **Keys that mean two things** — the number row (locators) during step entry, with and without the reserved-row preference; the keypad; "." (next marker or dot); Delete (the editors' delete, or delete-last-step); ⌘↓ (the list editor opens a reference). Expected: step entry's keys win only while it is on, the reserved row stays the locators', the keypad always enters values in step entry, keys outside it reach the editors. Pinned: Task 2 `step_entry_keys_win_while_it_is_on_unless_the_number_row_is_reserved`; Task 8 `step_entry_keys_are_live_only_in_step_entry`, `stepKeysWinOnlyDuringStepEntry`.
5. **Transport keys at their edges** — Play Selection with nothing selected, an empty range, the loop on, a device change while it plays; Stop while stopped; Next Marker past the last; arm moves past the first or last lane; Go to a locator never set; typed positions that are not positions or out of range. Expected: unavailable or refused with a reason in the status line (or the channel's error), nothing moves; a selection plays once and stops. Pinned: Task 3 `an_end_stops_playing_there_and_ignores_the_loop`, `play_from_stops_at_the_end_of_its_range`, `a_device_change_keeps_play_selections_end`, `record_arm_moves_one_lane_and_leaves_one_armed`, `arm_moves_stay_in_range`; Task 7 `stopGoesBackToWherePlayingStarted`, `playSelectionPlaysItsSpanOnceOrOnwards`, `markerKeysLocateToTheNearestMarker`, `locatorsAreSetJumpedToAndCleared`; Task 10 `fieldsShowAndTakeBarBeatUnit`, `editInAndOutSetTheSelectionsRange`, `punchFieldsKeepTheirOrder`.

## Plan Rulings

Decisions the spec leaves open, each implemented and tested as stated (format: Ruling: what — why — cost if wrong):

1. **Ruling:** The snap division is snake_case everywhere it is written: `Snap.Division.thirtySecond` has the raw value `thirty_second`, so `state.get editor` (`snap.division`), `ui.json` and the nudge preference write it; decoding also accepts the old `thirtySecond`. No other enum changes: the division is the one Swift raw value on the wire (control ruling F18); `ui.json`'s other camel-case values are Swift's own file — cost if wrong: none (old projects and preferences still read).
2. **Ruling:** A `command.run` result's `status` is the command's own: `ControlCommands.run` clears `editor.status` before it runs a document command, so a command that succeeds or is unavailable without a message reports none (control final review, Task 6 #3) — cost if wrong: an earlier message disappears from the status line one command sooner.
3. **Ruling:** The registry is every command. `CommandSpec` gains `extra_keys` (more default keys, never shown: the keypad twins), `menu_item` (false: keys, the Keys settings and the channel only) and `context`; a command without a menu item names its group in `menu` ("Locators", "Record", "Key Bindings"); IDs are snake_case words and may hold digits (`locator.go_3`). Bindings are Rust's (`core_control::bindings`): a `Chord` is a physical key (`NSEvent.keyCode`, US-ANSI names) plus the keyboard slot's modifier bitmask (1 ⇧, 2 ⌃, 4 ⌥, 8 ⌘), written `⌃⌥⇧⌘K` (`Space`, `←`, `F5`, `Keypad1`); the defaults are the registry's shortcut and extra keys; the user's changes are overrides; a command has at most four keys; the document `{"vizdaw_keys": 1, "bindings": {id: [chord, …]}}` holds only the changes, at most 256 KiB, and loads tolerantly (unknown IDs, unreadable keys, fifth keys and keys another command keeps are skipped and listed), anything else is refused whole — why: "every command … has a stable identifier", keys bound by physical position survive keyboard layouts — cost if wrong: a layout user sees US names for their keys.
4. **Ruling:** Two key contexts: global, and step entry. While step entry is on its keys win over global ones, except keys on the number row when the preference "Number keys always go to locators" is on (the keypad then enters values). Defaults (the parent's "small core"): Space Play/Stop, S Stop, ⌥Space Play Selection, ⇧Space Play from Selection, Y Play from Edit-In, R Record, K Scrub, Return Return to Start, `,` `.` Previous/Next Marker, L Loop, O Overdub, I Punch, T Step Entry, G Snap to Grid, P/E/U the Position, Edit-In and Punch fields, ⌥⇧↑/⌥⇧↓ Arm Previous/Next Lane, 0–9 Go to Locator, ⌥0–9 Set, ⌥⇧0–9 Clear; in step entry 1–6 1/1…1/32 (and Keypad1–6), `.` Dotted (Keypad.), `/` Triplet (Keypad/), `=` Tie (Keypad+), `0` Rest (Keypad0), Delete Delete Last Step. The menus' existing ⌘ shortcuts stay. Record arm uses ⌥⇧ arrows, not ⌘ arrows: the list editor's ⌘↓ opens a reference — cost if wrong: users rebind.
5. **Ruling:** One router decides what a key press does, for the keyboard (an `NSEvent` local monitor, before the menus) and for `ui.key` alike (`KeyRouter.route`): in any window, a text field's plain keys (no ⌘, no ⌃) go straight to the field, past the menus' single-key equivalents; a bound key runs its command on the window's document through `ControlCommands.run` (the channel's handler, so keys never open dialogs; in a window without a document only the commands that need none run; a menu item's ⌘ or ⌃ key is left to AppKit's menu on the keyboard, which may open its dialog; the channel has no menus and answers `dialog_required`); a sequence's keyboard slot comes after the bindings; an unavailable command (or one needing its dialog) passes the key on to the menus and the views; a refused one shows in the status line. The Keys page's recorder takes every key it is shown. The canvas's own G handler goes (Snap to Grid is a binding); `KeyStrokeRow.command` goes (the registry's defaults no longer decide) — why: one place to change keys; text fields must never lose letters to single-key shortcuts — cost if wrong: a view that wanted a bound plain key (none today) gets it only when the command is unavailable.
6. **Ruling:** Menus show the bindings: every menu item's `.keyboardShortcut` comes from its command's first key (`KeyMenu.shortcut(id)`), so a rebound key shows at once; the Transport menu gains Stop, Play Selection, Play from Selection, Play from Edit-In, Record and Scrub (disabled), Previous/Next Marker, Overdub, Punch, the three field items and the arm moves; a Step menu holds Step Entry and the step keys; Editors gains Snap to Grid — cost if wrong: none.
7. **Ruling:** Ten locators (0–9) per document, set at the transport's position, stored in `ui.json` (`EditorLayout.transport`); Go to an unset locator is unavailable — why: the model has markers but no locators, and a file-format change is not needed for positions — cost if wrong: locators per sequence later (the layout field can grow).
8. **Ruling:** Stop (S) stops and goes back to where playing last started (Play/Stop keeps pausing in place); Overdub (O) and Punch (I) are switches, and the punch range is two fields, stored with the locators; recording (sub-project 5) reads them; Record, Scrub, Keep Loop Pass and Discard Since Last Keep are registered and unavailable ("Recording and scrubbing arrive with sub-project 5.") — why: editors spec §1 ("4f registers the commands so keys can be bound now") — cost if wrong: none.
9. **Ruling:** Play Selection plays the selection's range, else the span of the selected events in the shown sequence (first start to last end; a reference ends after its uses, a region after its audio), once: the engine's transport stops at that frame (`Transport.end`, `EngineCmd::Play { until }`), ignoring the loop; a device rebuild while it plays keeps the end. Play from Selection plays on from the span's start; Play from Edit-In from the range's start (the selection's range is the core's "edit-in/edit-out", `SelectionRow.time_range`) — cost if wrong: none.
10. **Ruling:** Record arm moves (⌥⇧↑/↓, `Session.move_record_arm`): forward from the last armed lane, backward from the first; with none armed, the first or last lane; every other armed lane is disarmed, so one lane is armed after; one undo step "Move Record Arm"; refused at the ends — cost if wrong: none (arming is sub-project 5's to use).
11. **Ruling:** Step entry's position is the transport's: it starts where the transport stands (stopped), every input starts from where the transport is now (a click in a ruler or a typed Position moves the next step), and every step moves the transport on (or back). It enters into the selection's first lane of the shown sequence, else its first lane; it is refused while playing. The state machine is Rust's (`core_editor::step`); the app keeps a copy and hands it to `Session.step_entry` with each input, which applies the effect as one undo step ("Step Entry", or "Delete Last Step") and makes the entered events the selection; a tie or a step taken back acts only on its events still in that lane; history keeps the newest 256 steps — why: the spec's "pitch from MIDI input" needs a position, and the transport is the one the user sees — cost if wrong: a separate step cursor later.
12. **Ruling:** Step keys (Ruling 4): a value key sets the value; Dotted and Triplet are switches that stay until pressed again (both: the plain value); a note lasts the whole step (legato); Tie lengthens the last step's notes (again and again) by a step and moves on — after a rest or with nothing to tie it is a rest; Rest moves on; Delete Last Step takes back the last notes, tie, rest or reference and moves back — cost if wrong: a different feel for ties.
13. **Ruling:** MIDI input now queues note-offs too (`NoteInRow.on`); a chord is every key pressed until the last one is released (at most 16 notes, by pitch, each with its own velocity). While step entry is on, played notes go to it and not to a list editor; the note hub listens while step entry is on — cost if wrong: none.
14. **Ruling:** `step.notes` (no menu item, parameter `pitches`: note numbers or names, C4 = 60, at most 16) enters a chord at velocity 100 — the control channel's and a script's stand-in for a MIDI keyboard — cost if wrong: none.
15. **Ruling:** Keyboard slots: a pressed slot (a key no command has) makes its sequence the shown one and plays it from its start; during step entry it enters a reference to the sequence at the step position and moves on by the sequence's length. The slot sheet refuses a key a global command has ("⌘S is the key of “Save”.") and stays open; the core already refuses a slot another sequence has — cost if wrong: none.
16. **Ruling:** The transport bar (AppKit, above the arrangement) holds Position, Edit In, Edit Out, Punch In and Punch Out. P, E and U give Position, Edit In and Punch In the keyboard (Tab moves on); a field takes the keyboard only from its key or a click, never as the window's first text field. Entry is bar·beat·unit as in the list editor (`5`, `5.3`, `5.3.480`, an empty part keeps that part) with `:`, `,` or a space separating like a dot; Enter commits and gives the keyboard back to the keys, Escape reverts. Edit In sets the selection's range start (no range: one bar; past the end: the length moves along), Edit Out its end (after Edit In, else refused); Punch In past Punch Out moves Punch Out a bar after it; Punch Out must be after Punch In — cost if wrong: none.
17. **Ruling:** Settings gets a Keys tab beside Editing: every command by menu with its keys (changed ones tinted, step entry's marked), Record (an AppKit recorder: the next key, Escape cancels; the recorded key becomes the command's only key; a key another command has waits for Replace or Cancel), Reset, Reset All, Save… and Load… (`.vizkeys`), and the reserved-row preference. `keys.save`, `keys.load` (parameter `path`, answering the Save and Open dialogs) and `keys.reset` are registry commands that need no document — cost if wrong: none.
18. **Ruling:** `state.get editor` gains `step` (`seq`, `lane`, `tick`, `value` snake_case, `dotted`, `triplet`, `steps`; null while off) and `transport` (`locators`, `punch_in`, `punch_out`, `punch`, `overdub`); the e2e builds its own project ("Tune", one lane on the built-in synth) — cost if wrong: none.
19. **Ruling:** Not in 4f: recording, loop passes and scrubbing (sub-project 5; their commands only), what punch and overdub do, locators per sequence, the visual design of the transport bar (sub-project 7) — cost if wrong: small follow-ups.
20. **Ruling:** New Swift tests go to `VizDAWAppModelTests` (they need `FakeSession`), as in 4c Ruling 26, 4d Ruling 27 and 4e Ruling 18; one view test edit is in `VizDAWEditorsTests` (`ControlUITests`).

## Carried and parked items

Picked up by this plan:
- Control ledger Ruling F18 / control preflight F18: `thirtySecond` on the wire → Task 1, Ruling 1 (snake_case, old spelling still read in `ui.json` and preferences).
- Control final review, stay-parked Task 6 #3: stale `status` after a silent refusal ("plan 4f's status contract") → Task 1, Ruling 2.
- 4e final review / fix report, Item 4: `LaneHeaderView`'s rename field had the same Escape gap as the drum row header ("parked for 4f") → Task 1 (`escapeInTheLaneRenameFieldCancelsTheRename`).
- 4e final review T2 M1: swing parity on odd-length loops ("document it with the 4f quantize work") → Task 1, the `QuantizeSpec::swing` doc comment (4f has no quantize work; the behaviour stays as 4e Ruling 10 has it).
- 4e final review, "New, parked": `DrumRowHeaderView` keeps the keyboard after a click, so Escape there did nothing for a grid stroke waiting on Busy ("revisit with 4f key routing") → Task 9 (Escape on the header cancels the grid's stroke).
- 4a ledger Task 13 minor "slot key unvalidated/duplicates": the core already refuses a duplicate slot; the clash with a binding is Task 8 (Ruling 15).
- Snap's G key, which only a focused canvas heard → a binding (Task 9, Ruling 5).

Reviewed and left parked (with the reason):
- Control final review, Task 11 M1: an element click under the toolbar ("revisit with plan 4f's hit-testing"). 4f adds no hit-testing: the spec's 4f is keys and step entry. It stays as the control review triaged it (narrow widening; raw points were never restricted).

Left untouched — every other parked item, so none is lost (as their ledgers, final reviews and fix reports record them; a fix report is the authority on whether one closed since):
- **4a ledger:** T1 `revs.rs` bump duplication, no lane add/remove revision test; T2 `seq_length` hides a dangling reference as 0, round-trip variants, `kind_of` untested, the session lock held while building 100k rows; T3 FixedBars block end vs note tails, `bar_span` recomputed per event; T4 abort/gesture error path skips `mark()`, undo-label assertions; T5 `SameLanesAs` redundant renames, zero-lane source keeps the default lane; T6 Busy note-off leaves an audition note held, misindented `FormattingTests`, Locate/wrap release and engine host time untested; T7 bar snap ignores triplet/dotted, `Grid.lines` unclamped, cap tests; T8 poll literals, init feed untested; T9 `storeLayout` swallows a `setUiLayout` error, `infoCache` never pruned, test gaps; T10 rewind flags, read error re-shown per snapshot, `rewind()` race, `settle()` yields, `infoFailures` not pruned; T11 non-precise wheel deltas, ruler/G/Escape/autoscroll tests, a11y refresh contract, display link on window moves; T12 view tests in the model target, rename field vs scroll, header controls ≥46 pt, `MenuAction` static array, header a11y only toggles mute, note sketch by time; T13 no-op comment/slot undo steps, template menu duplicates; T14 `refLength` nil, no-op reference commit, re-point lists cycle targets; T15 tear-off close inside SwiftUI, window title not following kind/rename, frame on zoom, ⌥⌘N inconsistency, `DetailPane.title()` FFI per render; T16 teardown test, shared `textEditing` Bool.
- **4b ledger:** T1 `engine.stop()` result ignored in handover, lock probe starvation; T2 edge tests, negative ticks from Duplicate, per-note ranges; T3 shadowed `BAR`, FFI negative tests; T5 doc vs docID for torn targets, duplicated tear-off; T7 unclamped pencil/snap ticks, zone tests; T8 `laneRowCache` not pruned, toggle leaves the lane selected; T9 silent refusal after a sounded note, raw==0 guard, eraser skips between samples; T10 `pianoRoll()` get-or-create in a body; T11 unknown strip mute silent, region names overprinted, region drag doesn't select; T12 thinning drops the last point, O(n²) rebuild per drag, pane a11y one element; T13 cold cache anchor; the 300 ms sleep in `handle_events` on device rebuild.
- **4c ledger:** T1 `Bracketed` index-0 `by`, non-saturating `b.tick + duration`; T2 edge tests, compress overflow with negative durations; T3 `TrimStart` before 0; T4 doc comments, `DeleteTemplate` trim; T5 poisoned `try_lock`, `ticks_at` NaN, a CoreMIDI client per start; T6 failed modify reads not cached; T7 "5." resets the unit, beat ≥5 rolls over, integer frame rates; T8 `ListDelete` skips the first tempo/meter silently; T9 receiver/tie-break tests; T10 `syncCursor` mid-edit; T11 `abs(Int64.min)`; T12 the panel's "acting on the selection" hint, `requestIsStale` side effect; T13 panel title after rename, floating panel above other documents; T14 cancel during a Busy exact revert, multi-lane test.
- **4d final review / fix report:** scale time can still act on selected notes of lanes the piano roll does not show (needs a lane condition or modify-by-IDs; Minor 13); whether bridged AU v3 units echo host writes (I2, README step 76); in-block ramp resolution collapses under queue overload; out-of-tree `automate` implementors now return `bool`; `catalog.rs` load sensitivity on CI.
- **4e final review / fix report, stay-parked:** T1 M1–M3; T2 M2–M6; T3 M1, M4, M5; T4 M1–M4 (tick-window `drum_lanes`); T5 M3, M5; T6 M1–M5, M7; T7 M1–M5 (the bar width stays a by-eye check: SwiftUI bars cannot be screenshotted); T8 M1–M4; T9 M1–M5; e2e temp homes kept (`tempdir().keep()`).
- **Control final review, stay-parked:** Task 1 M1–M3; Task 2 M1–M8; Task 3 items 1–4, 6–8; Task 4 M1, M2, M4, M6–M9; Task 5 M1–M5; Task 6 items 1, 2, 4–6; Task 7 items 1–6; Task 8 M1–M7; Task 9 M1, M3, M6, M7c; Task 10 M1, M3, M7, M8, M10, M11, M13–M15; Task 11 M3, M5, M6; new observations (`eventsPending` counts the user's own events; `app.quit` while a channel-opened menu tracks: one manual check; `.DS_Store` stays out of the merge).
- **Linux CI:** deferred by the user (ubuntu jobs non-blocking).

## Spec gaps and contradictions found while planning

- "Bindings can be saved, loaded, reset" names no format or place (Rulings 3, 17); "bound keys show in menus" says nothing of commands without menu items (Ruling 3: the Keys page lists them).
- "Number keys … jump to locators" and "Step entry chooses note values from number keys" share one row of keys (Ruling 4); "a preference reserves the number row" says where locators win, not where note values go then (the keypad, Ruling 4).
- "Play/pause, stop" as two keys (Ruling 8); "play from position" (Ruling 9: from the edit-in point; Play/Stop already plays from the position).
- The model has markers but no locators, no punch range and no record modes (Rulings 7, 8); "edit-in field" — the core names the selection's range "edit-in/edit-out" (`SelectionRow.time_range`), so Edit In and Out edit it (Ruling 16).
- Step entry needs a position and a lane the spec does not name (Ruling 11), and a duration rule for its notes (Ruling 12); "pitch from MIDI input" with chords needs note-offs, which MIDI input dropped (Ruling 13).
- Recording keys (record, overdub, punch, keep/discard pass, record-arm moves) are bindable now but their behaviour is sub-project 5's (Ruling 8); record arm itself exists in the model, so its moves work now (Ruling 10).
- A keyboard slot "auditions" its sequence: there is no audition for a whole sequence other than playing it (Ruling 15).
- Modified arrows "nudge or move record-arm": ⌘ arrows belong to the list editor (Ruling 4).

## File Structure

```
crates/core-control/
  src/bindings.rs              Chord, KeyContext lookups, Keymap, the bindings document (Task 2)
  src/registry.rs              extra_keys, menu_item, context; every command (2, 7, 8, 10)
  src/keys.rs, src/lib.rs      keypad and F12 names for ui.key; the module (2)
  tests/bindings.rs            chords, defaults, contexts, binding, documents, proptests (2)
  tests/commands.rs            the new commands, their keys, the count 44 → 108 (2, 7, 8, 10)
crates/core-engine/src/{transport,render,engine}.rs, tests/transport.rs   play until a frame (3)
crates/core-model/src/query/{length,mod}.rs   event_end (3)
crates/core-editor/
  src/transport.rs, tests/transport.rs   selection span, marker steps, record-arm moves (3)
  src/step.rs, tests/step.rs             the step entry state machine, typed pitches (4)
crates/core-ffi/
  src/host.rs, src/session.rs  play_from, selection_span, marker_step, move_record_arm (3); step_entry (5)
  src/keys.rs                  the Keymap object, BindingRow (5); key_code_for (6); chord_text (10)
  src/step.rs                  step records and functions (5)
  src/input.rs                 note-offs (5)
  src/control.rs               snake_case doc (1); StepEntryStateRow, TransportMarksRow, no KeyStrokeRow.command (11)
  tests/{keys,step,input,transport,control}.rs (3, 5, 6, 10, 11)
crates/core-model/src/lane.rs  swing parity doc (1)
crates/vizctl/tests/e2e.rs     the keys end to end (11)
app/
  Sources/VizDAWAppModel/      Snap, ControlCommands (1); KeyBindings, TransportMarks, EditorLayout (6);
                               TransportCommands, DocumentModel, EditorModel (7); StepEntry, DocumentStore,
                               DocumentModel+Sequences (8); TransportFields (10); ListEditorModel (5)
  Sources/VizDAWEditors/       LaneHeaderView (1); EditorCanvas, DrumRowHeaderView and the canvases (9);
                               TransportFieldsView (10)
  Sources/VizDAW/              VizDAWApp, ControlChannel (7, 9, 10); SequenceSidebar (8); KeyMonitor,
                               AppCommands (9, 10); ArrangementPane, KeysSettingsView (10)
  Tests/VizDAWAppModelTests/   SnapTests, ControlCommandsTests, ArrangementViewTests (1); FakeSession (3, 5, 11);
                               ListEditorModelTests (5); KeyBindingsTests (6); TransportCommandsTests (7);
                               StepEntryTests (8, 9, 11); DrumGridViewTests (9); TransportFieldsTests (10)
  Tests/VizDAWEditorsTests/ControlUITests (11)
  README.md                    keys and step entry, checklist steps 99–108 (12)
```

---

### Task 1: Carried items — the snake_case snap division, a command's own status, Escape in the lane-name field

**Files:**
- Modify: `app/Sources/VizDAWAppModel/ControlCommands.swift`, `app/Sources/VizDAWAppModel/Snap.swift`, `app/Sources/VizDAWEditors/LaneHeaderView.swift`, `crates/core-ffi/src/control.rs`, `crates/core-model/src/lane.rs`
- Test: `app/Tests/VizDAWAppModelTests/ArrangementViewTests.swift`, `app/Tests/VizDAWAppModelTests/ControlCommandsTests.swift`, `app/Tests/VizDAWAppModelTests/SnapTests.swift`

**Interfaces:**
- Consumes: `Snap.Division`, `NudgeSettings`, `EditorLayout.decode`, `ControlCommands.run`, `ControlStates.state`, `LaneHeaderView.{beginRename, controlTextDidEndEditing}`, `EditorModel.{status, updateLayout, endTextEditing}`.
- Produces: `Snap.Division.thirtySecond.rawValue == "thirty_second"` and `Snap.Division.init(from:)` reading `thirtySecond` too; `ControlCommands.run` clears `editor.status` before a document command; `LaneHeaderView.control(_:textView:doCommandBy:) -> Bool` and `LaneHeaderView.cancelRename()`; a cancel text movement never renames.

Rulings 1 and 2, and two parked items (see "Carried and parked items"). The Rust changes are doc comments only: `QuantizeSpec::swing` records the swing-parity behaviour 4e ruled, and `EditorStateRow.snap_division` names its snake_case values.

- [ ] **Step 1: Write the failing tests**

In `app/Tests/VizDAWAppModelTests/ArrangementViewTests.swift`:

Find:

```swift
        #expect(h.renaming == nil)
    }

    @Test func aHeaderTornDownMidRenameReEnablesTransportKeys() throws {
        let m = try DocumentModel(session: editorFake())
        let h = LaneHeaderView(document: m)
```

Replace with:

```swift
        #expect(h.renaming == nil)
    }

    @Test func escapeInTheLaneRenameFieldCancelsTheRename() throws {
        let f = editorFake()
        let m = try DocumentModel(session: f)
        let h = LaneHeaderView(document: m)
        let n = f.edits.count
        h.beginRename(m.lanes[1], seq: 3, row: 1)
        let field = try #require(h.renaming?.field)
        field.stringValue = "Keys"
        #expect(h.control(field, textView: NSTextView(), doCommandBy: #selector(NSResponder.cancelOperation(_:))))
        #expect(h.renaming == nil && field.superview == nil && !m.editor.textEditing)
        h.controlTextDidEndEditing(Notification(name: NSControl.textDidEndEditingNotification))
        #expect(f.edits.count == n, "no rename sent")
        // Other commands are left to the field; the field editor's own
        // cancel movement does not rename either.
        h.beginRename(m.lanes[1], seq: 3, row: 1)
        let again = try #require(h.renaming?.field)
        #expect(!h.control(again, textView: NSTextView(), doCommandBy: #selector(NSResponder.insertNewline(_:))))
        again.stringValue = "Keys"
        h.controlTextDidEndEditing(
            Notification(
                name: NSControl.textDidEndEditingNotification, object: again,
                userInfo: ["NSTextMovement": NSTextMovement.cancel.rawValue]))
        #expect(f.edits.count == n && h.renaming == nil && !m.editor.textEditing)
    }

    @Test func aHeaderTornDownMidRenameReEnablesTransportKeys() throws {
        let m = try DocumentModel(session: editorFake())
        let h = LaneHeaderView(document: m)
```

In `app/Tests/VizDAWAppModelTests/ControlCommandsTests.swift`:

Find:

```swift
        #expect(ControlStates.facts(m).detailKind == "none")
    }

    @Test func storeCommandsAnswerWithTheNewDocument() {
        let s = DocumentStore(
            makeEmpty: { throw VizError.Io(message: "disk full") }, makeDemo: { demoFake() },
```

Replace with:

```swift
        #expect(ControlStates.facts(m).detailKind == "none")
    }

    @Test func theEditorStateSpellsTheDivisionSnakeCase() throws {
        let (_, id, m, _) = try demo()
        m.editor.updateLayout { $0.snap.division = .thirtySecond }
        guard case .editor(_, let e)? = ControlStates.state(.editor, id, m) else {
            Issue.record("no editor state")
            return
        }
        #expect(e.snapDivision == "thirty_second")
    }

    @Test func aCommandReportsOnlyItsOwnStatus() throws {
        let (s, id, m, _) = try demo()
        m.editor.status = "An earlier refusal."
        guard case .done(.ran(_, _, _, _, let status)) = run("modify.exact_mode", s, id) else {
            Issue.record("not done")
            return
        }
        #expect(status == nil, "a command that says nothing reports no status")
        #expect(m.editor.status == nil)
        m.editor.status = "An earlier refusal."
        let e = try #require(failure(run("sequence.back", s, id)))
        #expect(e.code == .unavailable && m.editor.status == nil, "an unavailable command clears it too")
    }

    @Test func storeCommandsAnswerWithTheNewDocument() {
        let s = DocumentStore(
            makeEmpty: { throw VizError.Io(message: "disk full") }, makeDemo: { demoFake() },
```

In `app/Tests/VizDAWAppModelTests/SnapTests.swift`:

Find:

```swift
        #expect(try JSONDecoder().decode(Snap.self, from: data) == s)
    }

    @Test func floorTakesTheGridLineAtOrBefore() {
        let meter = [MeterEntryRow(tick: 0, num: 4, den: 4, sharps: 0, minor: false)]
        var s = Snap()
```

Replace with:

```swift
        #expect(try JSONDecoder().decode(Snap.self, from: data) == s)
    }

    @Test func theThirtySecondIsSnakeCaseAndTheOldSpellingStillReads() throws {
        var s = Snap()
        s.division = .thirtySecond
        let json = String(decoding: try JSONEncoder().encode(s), as: UTF8.self)
        #expect(json.contains(#""thirty_second""#) && !json.contains("thirtySecond"))
        let old = Data(#"{"division":"thirtySecond","triplet":false,"dotted":false,"enabled":true}"#.utf8)
        #expect(try JSONDecoder().decode(Snap.self, from: old).division == .thirtySecond)
        // A project saved before plan 4f keeps its grid.
        let layout = EditorLayout.decode(
            #"{"snap":{"division":"thirtySecond","triplet":true,"dotted":false,"enabled":true}}"#)
        #expect(layout.snap.division == .thirtySecond && layout.snap.triplet)
        #expect(EditorLayout.decode(#"{"snap":{"division":"thirtysecond","triplet":true,"dotted":false,"enabled":true}}"#).snap == Snap(), "unknown: the default")
        // Nudge preferences written before plan 4f still read.
        var n = NudgeSettings()
        n.time = .division(.thirtySecond, triplet: true)
        let written = String(decoding: try JSONEncoder().encode(n), as: UTF8.self)
        #expect(written.contains("thirty_second"))
        let before = Data(written.replacingOccurrences(of: "thirty_second", with: "thirtySecond").utf8)
        #expect(try JSONDecoder().decode(NudgeSettings.self, from: before).time == .division(.thirtySecond, triplet: true))
    }

    @Test func floorTakesTheGridLineAtOrBefore() {
        let meter = [MeterEntryRow(tick: 0, num: 4, den: 4, sharps: 0, minor: false)]
        var s = Snap()
```

- [ ] **Step 2: Run them to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/build-ffi.sh && scripts/swift.sh test --filter 'SnapTests|ControlCommandsTests|ArrangementViewTests' 2>&1 | grep -E "✘|Test run" | head
```

Expected: FAIL: `theThirtySecondIsSnakeCaseAndTheOldSpellingStillReads` (the JSON holds `thirtySecond`), `theEditorStateSpellsTheDivisionSnakeCase` (`thirtySecond`), `aCommandReportsOnlyItsOwnStatus` (the old status comes back), `escapeInTheLaneRenameFieldCancelsTheRename` (no `control(_:textView:doCommandBy:)` yet: a compile error, so the whole run fails until Step 3).

- [ ] **Step 3: Implement**

In `app/Sources/VizDAWAppModel/ControlCommands.swift`:

Find:

```swift
        default: break
        }
        guard let (docID, m) = doc else { return .failed(.of(.noDocument, "No document is open.")) }
        let before = m.alert?.id
        switch id {
        case "file.close":
```

Replace with:

```swift
        default: break
        }
        guard let (docID, m) = doc else { return .failed(.of(.noDocument, "No document is open.")) }
        // A result's `status` is this command's own (plan 4f Ruling 2): a
        // message an earlier action left is cleared first.
        m.editor.status = nil
        let before = m.alert?.id
        switch id {
        case "file.close":
```

In `app/Sources/VizDAWAppModel/Snap.swift`:

Find:

```swift
/// toggled by one key (Plan Ruling 16). The grid is counted from the start
/// of the bar a tick is in, so a bar line always snaps.
public struct Snap: Codable, Equatable, Sendable {
    public enum Division: String, Codable, CaseIterable, Sendable {
        case bar, half, quarter, eighth, sixteenth, thirtySecond

        public var title: String {
            switch self {
```

Replace with:

```swift
/// toggled by one key (Plan Ruling 16). The grid is counted from the start
/// of the bar a tick is in, so a bar line always snaps.
public struct Snap: Codable, Equatable, Sendable {
    /// Written snake_case in `ui.json`, preferences and on the control
    /// channel (plan 4f Ruling 1); the camel-case `thirtySecond` that
    /// builds before 4f wrote still reads.
    public enum Division: String, Codable, CaseIterable, Sendable {
        case bar, half, quarter, eighth, sixteenth
        case thirtySecond = "thirty_second"

        public init(from decoder: Decoder) throws {
            let raw = try decoder.singleValueContainer().decode(String.self)
            guard let d = Division(rawValue: raw == "thirtySecond" ? Division.thirtySecond.rawValue : raw) else {
                throw DecodingError.dataCorrupted(
                    DecodingError.Context(codingPath: decoder.codingPath, debugDescription: "no division \(raw)"))
            }
            self = d
        }

        public var title: String {
            switch self {
```

In `app/Sources/VizDAWEditors/LaneHeaderView.swift`:

Find:

```swift
        editor.endTextEditing("laneName")
        let name = r.field.stringValue.trimmingCharacters(in: .whitespacesAndNewlines)
        r.field.removeFromSuperview()
        if !name.isEmpty, name != r.name {
            editor.edit(.lane(edit: .rename(seq: r.seq, lane: r.lane, name: name)))
        }
    }

    /// A header torn down mid-rename never gets its end-editing callback;
    /// give the keyboard back to the transport keys.
    public override func viewDidMoveToWindow() {
```

Replace with:

```swift
        editor.endTextEditing("laneName")
        let name = r.field.stringValue.trimmingCharacters(in: .whitespacesAndNewlines)
        r.field.removeFromSuperview()
        let movement = (obj.userInfo?["NSTextMovement"] as? Int).flatMap(NSTextMovement.init(rawValue:))
        guard movement != .cancel else { return }
        if !name.isEmpty, name != r.name {
            editor.edit(.lane(edit: .rename(seq: r.seq, lane: r.lane, name: name)))
        }
    }

    /// Escape cancels a rename: the name stays and no edit is sent (the
    /// gap parked from plan 4e, closed like the drum row header's).
    public func control(_ control: NSControl, textView: NSTextView, doCommandBy selector: Selector) -> Bool {
        guard selector == #selector(NSResponder.cancelOperation(_:)) else { return false }
        cancelRename()
        return true
    }

    /// Ends a rename without renaming.
    func cancelRename() {
        guard let r = renaming else { return }
        renaming = nil
        editor.endTextEditing("laneName")
        r.field.abortEditing()
        r.field.removeFromSuperview()
        window?.makeFirstResponder(nil)
    }

    /// A header torn down mid-rename never gets its end-editing callback;
    /// give the keyboard back to the transport keys.
    public override func viewDidMoveToWindow() {
```

In `crates/core-ffi/src/control.rs`:

Find:

```rust
    pub tool: String,
    /// Snap on.
    pub snap_enabled: bool,
    /// Snap division (`quarter`, ...).
    pub snap_division: String,
    /// Exact mode on.
    pub exact_mode: bool,
```

Replace with:

```rust
    pub tool: String,
    /// Snap on.
    pub snap_enabled: bool,
    /// Snap division, snake_case: `bar`, `half`, `quarter`, `eighth`,
    /// `sixteenth`, `thirty_second` (plan 4f Ruling 1).
    pub snap_division: String,
    /// Exact mode on.
    pub exact_mode: bool,
```

In `crates/core-model/src/lane.rs`:

Find:

```rust

    /// Swing: every second line of `grid` moves late so that the pair
    /// splits `percent : 100 − percent` (50 is straight, 66 a triplet
    /// feel); `percent` is clamped to 50–75; full strength.
    pub fn swing(grid: Tick, percent: u8) -> QuantizeSpec {
        let p = i64::from(percent.clamp(50, 75));
        let late = grid.0.saturating_mul(2).saturating_mul(p - 50) / 100;
```

Replace with:

```rust

    /// Swing: every second line of `grid` moves late so that the pair
    /// splits `percent : 100 − percent` (50 is straight, 66 a triplet
    /// feel); `percent` is clamped to 50–75; full strength. Lines are
    /// counted from tick 0 of the lane, not from a loop start: a lane that
    /// loops over an odd number of grid lines swings the other lines on
    /// its next pass (4e Ruling 10, kept; parked T2 M1).
    pub fn swing(grid: Tick, percent: u8) -> QuantizeSpec {
        let p = i64::from(percent.clamp(50, 75));
        let late = grid.0.saturating_mul(2).saturating_mul(p - 50) / 100;
```

- [ ] **Step 4: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh test --filter 'SnapTests|ControlCommandsTests|ArrangementViewTests' 2>&1 | tail -1
```

Expected: the three suites pass.

- [ ] **Step 5: Gate and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace 2>&1 | grep -E "FAILED|panicked" ; cargo test --workspace -- --list 2>/dev/null | grep -c ': test$'
scripts/build-ffi.sh && scripts/swift.sh build 2>&1 | tail -1 && scripts/swift.sh test 2>&1 | tail -1
git add app/Sources/VizDAWAppModel/ControlCommands.swift \
  app/Sources/VizDAWAppModel/Snap.swift \
  app/Sources/VizDAWEditors/LaneHeaderView.swift \
  app/Tests/VizDAWAppModelTests/ArrangementViewTests.swift \
  app/Tests/VizDAWAppModelTests/ControlCommandsTests.swift \
  app/Tests/VizDAWAppModelTests/SnapTests.swift \
  crates/core-ffi/src/control.rs \
  crates/core-model/src/lane.rs
git commit -m "fix(editors): snake_case snap division, a command's own status, Escape in lane names

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
git log -1 --format='%h %s'
```

Expected: 864 Rust tests listed (859 + 5 ignored, unchanged); Swift 613 (609 + 4). No `FAILED` or `panicked` line (rerun once if the known-flaky MIDI listener test trips, and say so).

---

### Task 2: Key bindings over the registry — chords, contexts, a keymap and its document

**Files:**
- Create: `crates/core-control/src/bindings.rs`
- Modify: `crates/core-control/src/keys.rs`, `crates/core-control/src/lib.rs`, `crates/core-control/src/registry.rs`
- Test: `crates/core-control/tests/bindings.rs`, `crates/core-control/tests/commands.rs`

**Interfaces:**
- Consumes: `core_control::registry::{COMMANDS, CommandSpec, cmd, find}`, `keys::{NAMED, named_key}`, `protocol::truncate`.
- Produces: `registry::KeyContext {Global, StepEntry}` (`name()` → `global`/`step_entry`); `CommandSpec.{extra_keys: &'static [&'static str], menu_item: bool, context: KeyContext}`; `commands.list` entries gain `keys` (default chord texts), `menu_item`, `context`; `command_for_stroke` considers global commands only. `bindings::{SHIFT=1, CONTROL=2, OPTION=4, COMMAND=8, MAX_CHORDS=4, MAX_JSON=262144, VERSION=1, NUMBER_ROW, key_name(u16) -> Option<&str>, Chord {code: u16, mods: u8}, Chord::{new(code, mods) -> Option<Chord>, parse(&str) -> Result<Chord, String>, text(&self) -> String, of_slot(&str, u8) -> Option<Chord>, number_row(&self) -> bool, menu_key(&self) -> String}, BindError {UnknownCommand(String), TooMany, Taken {chord, other}} (Display: "⌘K is the key of “Save”"), default_chords(&CommandSpec) -> Vec<Chord>, Keymap::{new(table), default(), table(), chords(id) -> Vec<Chord>, holder(chord, context) -> Option<&'static str>, lookup(chord, step_entry, reserve_numbers) -> Option<&'static str>, bind(id, &[Chord], replace) -> Result<(), BindError>, reset_one(id), reset(), changed() -> Vec<&'static str>, to_json() -> String, from_json(table, &str) -> Result<(Keymap, Vec<String>), String>}`. `ui.key` names the keypad (`keypad_0` … `keypad_9`, `keypad_decimal`, `keypad_divide`, `keypad_multiply`, `keypad_minus`, `keypad_plus`, `keypad_equals`, `keypad_clear`) and `f12`.

Rulings 3 and 4. No command is added here (each arrives with its handler, Tasks 7, 8 and 10); the step-entry context is exercised with a test table of its own. The keymap is a pure value: the FFI wraps it in Task 5.

- [ ] **Step 1: Write the failing tests**

Create `crates/core-control/tests/bindings.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p crates/core-control/tests
cat > crates/core-control/tests/bindings.rs <<'VIZDAW_EOF'
//! Key bindings (plan 4f Task 2): chords, defaults from the registry,
//! contexts, binding and resetting, and the bindings document.

use core_control::bindings::{
    BindError, COMMAND, Chord, Keymap, MAX_CHORDS, MAX_JSON, SHIFT, default_chords,
};
use core_control::registry::{COMMANDS, CommandSpec, KeyContext};
use proptest::prelude::*;
use serde_json::json;

fn c(text: &str) -> Chord {
    Chord::parse(text).unwrap_or_else(|e| panic!("{text}: {e}"))
}

const fn spec(
    id: &'static str,
    shortcut: Option<&'static str>,
    extra_keys: &'static [&'static str],
    context: KeyContext,
) -> CommandSpec {
    CommandSpec {
        id,
        menu: "Test",
        title: id,
        shortcut,
        extra_keys,
        menu_item: false,
        context,
        document: true,
        params: &[],
        dialogs: &[],
    }
}

/// A global locator and marker key, and step entry keys on the same keys.
static TABLE: &[CommandSpec] = &[
    spec("locator.go_1", Some("1"), &[], KeyContext::Global),
    spec("transport.next", Some("."), &[], KeyContext::Global),
    spec("transport.stop", Some("S"), &[], KeyContext::Global),
    spec("step.whole", Some("1"), &["Keypad1"], KeyContext::StepEntry),
    spec("step.dot", Some("."), &["Keypad."], KeyContext::StepEntry),
    spec("step.rest", None, &[], KeyContext::StepEntry),
];

/// Every chord is held by at most one command per context.
fn no_collisions(k: &Keymap) -> bool {
    let mut seen = std::collections::BTreeSet::new();
    k.table().iter().all(|s| {
        k.chords(s.id)
            .into_iter()
            .all(|ch| seen.insert((s.context, ch)))
    })
}

#[test]
fn chords_read_and_write_as_menus_show_them() {
    let save_as = c("⇧⌘S");
    assert_eq!(
        save_as,
        Chord {
            code: 1,
            mods: SHIFT | COMMAND
        }
    );
    assert_eq!(save_as.text(), "⇧⌘S");
    assert_eq!(c("⌘⇧s"), save_as, "any order, any case");
    assert_eq!(c("⌥⇧1").text(), "⌥⇧1");
    assert_eq!(c("left"), c("←"));
    assert_eq!(c("esc"), c("Escape"));
    assert_eq!(c("Keypad1").code, 83);
    assert_eq!(c("keypad.").code, 65);
    assert_eq!(c("F5").code, 96);
    assert_eq!(c("space").text(), "Space");
    for bad in ["", "⌘", "⌘⌘S", "Hyper", "SS", "⌘ S S", "Keypad", "F13"] {
        assert!(Chord::parse(bad).is_err(), "{bad:?}");
    }
    // Every key VizDAW binds, with every modifier set, round-trips.
    for code in 0..=200u16 {
        for mods in 0..16u8 {
            if let Some(ch) = Chord::new(code, mods) {
                assert_eq!(Chord::parse(&ch.text()), Ok(ch), "{}", ch.text());
            }
        }
    }
    assert_eq!(Chord::new(0xFFFF, 0), None, "a key no US key types");
    assert_eq!(Chord::new(0, 0xFF).map(|x| x.mods), Some(15));
    assert_eq!(c("⌘S").menu_key(), "s");
    assert_eq!(c("Space").menu_key(), " ");
    assert_eq!(c("←").menu_key(), "\u{f702}");
    assert_eq!(c("F5").menu_key(), "\u{f708}");
    assert_eq!(c("F").menu_key(), "f");
    assert_eq!(c("Keypad1").menu_key(), "1");
    assert_eq!(c("Delete").menu_key(), "\u{8}");
    assert_eq!(Chord::of_slot("a", 8), Some(c("⌘A")));
    assert_eq!(Chord::of_slot("F5", 4 | 16), Some(c("⌥F5")));
    assert_eq!(Chord::of_slot("nope", 0), None);
    assert!(c("1").number_row() && c("0").number_row());
    assert!(!c("Keypad1").number_row() && !c("Q").number_row());
}

#[test]
fn the_registrys_defaults_parse_and_never_collide() {
    let k = Keymap::default();
    for s in COMMANDS {
        let n = usize::from(s.shortcut.is_some()) + s.extra_keys.len();
        assert_eq!(
            default_chords(s).len(),
            n,
            "{}: a default does not parse",
            s.id
        );
        for ch in default_chords(s) {
            assert_eq!(
                k.holder(ch, s.context),
                Some(s.id),
                "{} {}",
                s.id,
                ch.text()
            );
        }
    }
    assert!(no_collisions(&k));
    assert_eq!(
        k.lookup(c("Space"), false, false),
        Some("transport.play_stop")
    );
    assert_eq!(k.lookup(c("⌘S"), true, true), Some("file.save"));
    assert_eq!(
        k.lookup(c("⌥⌘1"), false, false),
        Some("editors.piano_roll_window")
    );
    assert_eq!(k.lookup(c("X"), false, false), None);
}

#[test]
fn step_entry_keys_win_while_it_is_on_unless_the_number_row_is_reserved() {
    let k = Keymap::new(TABLE);
    assert!(no_collisions(&k));
    assert_eq!(k.lookup(c("1"), false, false), Some("locator.go_1"));
    assert_eq!(k.lookup(c("1"), true, false), Some("step.whole"));
    assert_eq!(
        k.lookup(c("1"), true, true),
        Some("locator.go_1"),
        "the row is the locators'"
    );
    assert_eq!(
        k.lookup(c("Keypad1"), true, true),
        Some("step.whole"),
        "the keypad still enters"
    );
    assert_eq!(
        k.lookup(c("Keypad1"), false, false),
        None,
        "step keys are off outside step entry"
    );
    assert_eq!(
        k.lookup(c("."), true, true),
        Some("step.dot"),
        "only the number row is reserved"
    );
    assert_eq!(
        k.lookup(c("S"), true, false),
        Some("transport.stop"),
        "global keys still work"
    );
}

#[test]
fn binding_refuses_a_taken_key_unless_it_replaces() {
    let mut k = Keymap::default();
    k.bind("file.save", &[c("⌘K"), c("⌘K")], false).unwrap();
    assert_eq!(k.chords("file.save"), vec![c("⌘K")], "deduplicated");
    assert_eq!(k.lookup(c("⌘S"), false, false), None, "⌘S is free now");
    let e = k.bind("edit.undo", &[c("⌘K")], false).unwrap_err();
    assert_eq!(
        e,
        BindError::Taken {
            chord: c("⌘K"),
            other: "file.save"
        }
    );
    assert_eq!(e.to_string(), "⌘K is the key of “Save”");
    k.bind("edit.undo", &[c("⌘K"), c("⌘Z")], true).unwrap();
    assert_eq!(k.chords("file.save"), Vec::<Chord>::new(), "taken away");
    assert_eq!(k.lookup(c("⌘K"), false, false), Some("edit.undo"));
    assert_eq!(
        k.bind("nope.nope", &[], false),
        Err(BindError::UnknownCommand("nope.nope".into()))
    );
    let five: Vec<Chord> = ["F1", "F2", "F3", "F4", "F5"]
        .iter()
        .map(|t| c(t))
        .collect();
    assert_eq!(k.bind("file.save", &five, false), Err(BindError::TooMany));
    assert_eq!(five.len(), MAX_CHORDS + 1);
    // Contexts are separate: a step key may share a global key, not
    // another step key.
    let mut t = Keymap::new(TABLE);
    t.bind("step.rest", &[c("S")], false).unwrap();
    assert_eq!(
        t.bind("step.rest", &[c("1")], false),
        Err(BindError::Taken {
            chord: c("1"),
            other: "step.whole"
        })
    );
    assert!(no_collisions(&k) && no_collisions(&t));
}

#[test]
fn resetting_gives_defaults_back_but_not_ones_taken_since() {
    let mut k = Keymap::default();
    k.bind("edit.undo", &[c("⌘S")], true).unwrap();
    k.reset_one("file.save");
    assert_eq!(
        k.chords("file.save"),
        Vec::<Chord>::new(),
        "⌘S stays with Undo"
    );
    k.reset_one("edit.undo");
    assert_eq!(k.chords("edit.undo"), vec![c("⌘Z")]);
    k.reset_one("file.save");
    assert_eq!(k.chords("file.save"), vec![c("⌘S")]);
    assert!(k.changed().is_empty(), "back to the defaults");
    k.bind("file.save", &[], false).unwrap();
    assert_eq!(k.changed(), vec!["file.save"], "unbound is a change");
    k.reset();
    assert_eq!(k, Keymap::default());
}

#[test]
fn the_document_holds_only_changes_and_reads_back() {
    let mut k = Keymap::default();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&k.to_json()).unwrap(),
        json!({"vizdaw_keys": 1, "bindings": {}})
    );
    k.bind("file.save", &[c("⌘K"), c("F5")], false).unwrap();
    k.bind("edit.redo", &[], false).unwrap();
    let text = k.to_json();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&text).unwrap(),
        json!({"vizdaw_keys": 1, "bindings": {"edit.redo": [], "file.save": ["⌘K", "F5"]}})
    );
    let (back, skipped) = Keymap::from_json(COMMANDS, &text).unwrap();
    assert_eq!(back, k);
    assert!(skipped.is_empty(), "{skipped:?}");
}

#[test]
fn a_damaged_document_loads_what_it_can() {
    let doc = json!({"vizdaw_keys": 1, "bindings": {
        "nope.nope": ["⌘K"],
        "edit.undo": "⌘Z",
        "edit.redo": ["⌘⌘Z", 7, "⇧⌘Z"],
        "file.new": ["F1", "F2", "F3", "F4", "F5", "F6"],
        "transport.play_stop": ["⌘O"],
        "file.save": ["⌘Y"],
        "file.save_as": ["⌘S"],
    }});
    let (k, skipped) = Keymap::from_json(COMMANDS, &doc.to_string()).unwrap();
    assert!(no_collisions(&k));
    assert_eq!(k.chords("edit.redo"), vec![c("⇧⌘Z")]);
    assert_eq!(k.chords("edit.undo"), vec![c("⌘Z")], "unreadable: defaults");
    assert_eq!(k.chords("file.new").len(), MAX_CHORDS);
    assert_eq!(
        k.chords("transport.play_stop"),
        Vec::<Chord>::new(),
        "⌘O is still Open's"
    );
    assert_eq!(k.chords("file.save"), vec![c("⌘Y")]);
    assert_eq!(k.chords("file.save_as"), vec![c("⌘S")], "keys may move");
    let joined = skipped.join("\n");
    for part in [
        "no command \"nope.nope\"",
        "edit.undo: the keys are not a list",
        "edit.redo: \"⌘⌘Z\" repeats a modifier",
        "edit.redo: a key is not text",
        "file.new: more than 4 keys",
        "transport.play_stop: ⌘O is already the key of file.open",
    ] {
        assert!(joined.contains(part), "{part:?} in\n{joined}");
    }
    for bad in [
        "garbage".to_string(),
        json!({"vizdaw_keys": 2, "bindings": {}}).to_string(),
        json!({"vizdaw_keys": 1}).to_string(),
        json!([1, 2]).to_string(),
        format!(
            "{{\"vizdaw_keys\":1,\"bindings\":{{}},\"pad\":\"{}\"}}",
            "x".repeat(MAX_JSON)
        ),
    ] {
        assert!(
            Keymap::from_json(COMMANDS, &bad).is_err(),
            "{}",
            &bad[..bad.len().min(40)]
        );
    }
}

fn any_key_text() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("⌘S".to_string()),
        Just("1".to_string()),
        Just("Space".to_string()),
        Just("Keypad1".to_string()),
        Just("⌥⇧1".to_string()),
        Just("⌘⌘".to_string()),
        "[⌘⌥⇧⌃]{0,3}[A-Za-z0-9.←]{0,2}",
        ".{0,6}",
    ]
}

fn any_id() -> impl Strategy<Value = String> {
    prop_oneof![
        (0..COMMANDS.len()).prop_map(|i| COMMANDS[i].id.to_string()),
        "[a-z._]{0,12}",
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// A hostile document loads or is refused; a loaded keymap never has
    /// a key twice in a context, and writes back to itself.
    #[test]
    fn hostile_documents_never_collide(
        entries in prop::collection::vec((any_id(), prop::collection::vec(any_key_text(), 0..6)), 0..12),
        version in prop_oneof![Just(json!(1)), Just(json!(2)), Just(json!("1")), Just(json!(null))],
    ) {
        let mut b = serde_json::Map::new();
        for (id, keys) in entries {
            b.insert(id, json!(keys));
        }
        let doc = json!({"vizdaw_keys": version, "bindings": b}).to_string();
        if let Ok((k, _)) = Keymap::from_json(COMMANDS, &doc) {
            prop_assert!(no_collisions(&k));
            let (again, skipped) = Keymap::from_json(COMMANDS, &k.to_json()).unwrap();
            prop_assert_eq!(&again, &k);
            prop_assert!(skipped.is_empty());
        }
    }

    /// Any sequence of binds and resets keeps every key in one command
    /// per context.
    #[test]
    fn binds_and_resets_never_collide(
        ops in prop::collection::vec((0..COMMANDS.len(), prop::collection::vec(any_key_text(), 0..5), any::<bool>(), 0..8u8), 0..24),
    ) {
        let mut k = Keymap::default();
        for (i, keys, replace, op) in ops {
            let id = COMMANDS[i].id;
            let chords: Vec<Chord> = keys.iter().filter_map(|t| Chord::parse(t).ok()).collect();
            match op {
                0 => k.reset_one(id),
                1 => k.reset(),
                _ => {
                    let before = k.clone();
                    if k.bind(id, &chords, replace).is_err() {
                        prop_assert_eq!(&k, &before, "a refused bind changes nothing");
                    }
                }
            }
            prop_assert!(no_collisions(&k));
        }
    }
}
VIZDAW_EOF
```

In `crates/core-control/tests/commands.rs`:

Find:

```rust
    let list = registry::list_json();
    let first = &list["commands"][0];
    assert_eq!(first["id"], "file.new");
    assert_eq!(list["commands"].as_array().unwrap().len(), COMMANDS.len());
    let open = list["commands"]
        .as_array()
```

Replace with:

```rust
    let list = registry::list_json();
    let first = &list["commands"][0];
    assert_eq!(first["id"], "file.new");
    assert_eq!(
        first["keys"],
        json!(["⌘N"]),
        "default keys as menus show them"
    );
    assert_eq!(first["menu_item"], true);
    assert_eq!(first["context"], "global");
    assert_eq!(list["commands"].as_array().unwrap().len(), COMMANDS.len());
    let open = list["commands"]
        .as_array()
```

- [ ] **Step 2: Run them to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-control --test bindings 2>&1 | grep -E "^error|test result" | head -3
```

Expected: FAIL to compile: `unresolved import core_control::bindings` and no field `extra_keys` on `CommandSpec`.

- [ ] **Step 3: Implement**

Create `crates/core-control/src/bindings.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p crates/core-control/src
cat > crates/core-control/src/bindings.rs <<'VIZDAW_EOF'
//! Key bindings (editors spec §4f, plan 4f Rulings 3–5): every registry
//! command can be bound to keys. A binding is a [`Chord`] (a physical key
//! by its virtual key code, plus modifiers); the registry's shortcuts are
//! the defaults; the user's changes are kept as overrides and saved as a
//! small JSON document. Lookups know two contexts: step entry's keys win
//! over the global ones while step entry is on, unless a preference
//! reserves the number row for locators.

use crate::protocol::truncate;
use crate::registry::{COMMANDS, CommandSpec, KeyContext};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

/// ⇧ in [`Chord::mods`] (the keyboard slot's bitmask too).
pub const SHIFT: u8 = 1;
/// ⌃
pub const CONTROL: u8 = 2;
/// ⌥
pub const OPTION: u8 = 4;
/// ⌘
pub const COMMAND: u8 = 8;

/// The most keys one command may have.
pub const MAX_CHORDS: usize = 4;
/// The largest bindings document read (bytes).
pub const MAX_JSON: usize = 256 * 1024;
/// The bindings document's format version.
pub const VERSION: u64 = 1;

/// Display names by virtual key code (US ANSI positions). Letters are
/// upper case; the names are what menus, files and the Keys page show.
const KEYS: [(u16, &str); 79] = [
    (0, "A"),
    (1, "S"),
    (2, "D"),
    (3, "F"),
    (4, "H"),
    (5, "G"),
    (6, "Z"),
    (7, "X"),
    (8, "C"),
    (9, "V"),
    (11, "B"),
    (12, "Q"),
    (13, "W"),
    (14, "E"),
    (15, "R"),
    (16, "Y"),
    (17, "T"),
    (18, "1"),
    (19, "2"),
    (20, "3"),
    (21, "4"),
    (22, "6"),
    (23, "5"),
    (24, "="),
    (25, "9"),
    (26, "7"),
    (27, "-"),
    (28, "8"),
    (29, "0"),
    (30, "]"),
    (31, "O"),
    (32, "U"),
    (33, "["),
    (34, "I"),
    (35, "P"),
    (37, "L"),
    (38, "J"),
    (39, "'"),
    (40, "K"),
    (41, ";"),
    (42, "\\"),
    (43, ","),
    (44, "/"),
    (45, "N"),
    (46, "M"),
    (47, "."),
    (50, "`"),
    (36, "Return"),
    (76, "Enter"),
    (48, "Tab"),
    (49, "Space"),
    (51, "Delete"),
    (53, "Escape"),
    (117, "ForwardDelete"),
    (115, "Home"),
    (119, "End"),
    (116, "PageUp"),
    (121, "PageDown"),
    (123, "←"),
    (124, "→"),
    (125, "↓"),
    (126, "↑"),
    (122, "F1"),
    (120, "F2"),
    (99, "F3"),
    (118, "F4"),
    (96, "F5"),
    (97, "F6"),
    (98, "F7"),
    (100, "F8"),
    (101, "F9"),
    (109, "F10"),
    (103, "F11"),
    (111, "F12"),
    (82, "Keypad0"),
    (83, "Keypad1"),
    (84, "Keypad2"),
    (85, "Keypad3"),
    (86, "Keypad4"),
];

/// The rest of the keypad, and Help.
const KEYPAD: [(u16, &str); 13] = [
    (87, "Keypad5"),
    (88, "Keypad6"),
    (89, "Keypad7"),
    (91, "Keypad8"),
    (92, "Keypad9"),
    (65, "Keypad."),
    (75, "Keypad/"),
    (67, "Keypad*"),
    (78, "Keypad-"),
    (69, "Keypad+"),
    (81, "Keypad="),
    (71, "KeypadClear"),
    (114, "Help"),
];

/// Other spellings a chord's key may be written in.
const ALIASES: [(&str, u16); 7] = [
    ("left", 123),
    ("right", 124),
    ("down", 125),
    ("up", 126),
    ("esc", 53),
    ("backspace", 51),
    ("del", 117),
];

/// The number row's key codes, `1` … `9`, `0`.
pub const NUMBER_ROW: [u16; 10] = [18, 19, 20, 21, 23, 22, 26, 28, 25, 29];

fn keys() -> impl Iterator<Item = &'static (u16, &'static str)> {
    KEYS.iter().chain(KEYPAD.iter())
}

/// The display name of key `code`, if it is one VizDAW binds.
pub fn key_name(code: u16) -> Option<&'static str> {
    keys().find(|(c, _)| *c == code).map(|(_, n)| *n)
}

fn key_code(name: &str) -> Option<u16> {
    keys()
        .find(|(_, n)| n.eq_ignore_ascii_case(name))
        .map(|(c, _)| *c)
        .or_else(|| {
            ALIASES
                .iter()
                .find(|(a, _)| a.eq_ignore_ascii_case(name))
                .map(|(_, c)| *c)
        })
}

/// A key and the modifiers held with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Chord {
    /// The virtual key code (`NSEvent.keyCode`): a physical key, so a
    /// binding stays on its key whatever the keyboard layout types.
    pub code: u16,
    /// [`SHIFT`] | [`CONTROL`] | [`OPTION`] | [`COMMAND`].
    pub mods: u8,
}

impl Chord {
    /// `code` with `mods` (bits beyond the four modifiers dropped), or
    /// None for a key VizDAW does not bind.
    pub fn new(code: u16, mods: u8) -> Option<Chord> {
        key_name(code).map(|_| Chord {
            code,
            mods: mods & (SHIFT | CONTROL | OPTION | COMMAND),
        })
    }

    /// Reads `⌃⌥⇧⌘K` style text: modifier glyphs (each at most once, any
    /// order), then one key name (`A`, `1`, `Space`, `←`, `F5`,
    /// `Keypad1`; letters and names in any case).
    pub fn parse(text: &str) -> Result<Chord, String> {
        let shown = || truncate(text, 32);
        let mut mods = 0u8;
        let mut rest = text.trim();
        loop {
            let mut chars = rest.chars();
            let bit = match chars.next() {
                Some('⇧') => SHIFT,
                Some('⌃') => CONTROL,
                Some('⌥') => OPTION,
                Some('⌘') => COMMAND,
                _ => break,
            };
            if mods & bit != 0 {
                return Err(format!("{:?} repeats a modifier", shown()));
            }
            mods |= bit;
            rest = chars.as_str();
        }
        match key_code(rest) {
            Some(code) => Ok(Chord { code, mods }),
            None if rest.is_empty() => Err(format!("{:?} has no key", shown())),
            None => Err(format!("{:?} is not a key VizDAW binds", shown())),
        }
    }

    /// The chord as menus show it: `⌃⌥⇧⌘` then the key's name.
    pub fn text(&self) -> String {
        let mut s = String::new();
        for (bit, glyph) in [(CONTROL, '⌃'), (OPTION, '⌥'), (SHIFT, '⇧'), (COMMAND, '⌘')] {
            if self.mods & bit != 0 {
                s.push(glyph);
            }
        }
        s.push_str(key_name(self.code).unwrap_or("?"));
        s
    }

    /// A keyboard slot's key (`a`, `F5`) and bitmask as a chord.
    pub fn of_slot(key: &str, modifiers: u8) -> Option<Chord> {
        let code = key_code(key.trim())?;
        Chord::new(code, modifiers)
    }

    /// The key is on the number row (not the keypad).
    pub fn number_row(&self) -> bool {
        NUMBER_ROW.contains(&self.code)
    }

    /// The character AppKit's menus use as this key's key equivalent
    /// (letters lower case; function keys as their private-use
    /// characters).
    pub fn menu_key(&self) -> String {
        let name = key_name(self.code).unwrap_or("");
        let c = match name {
            "Return" => '\r',
            "Enter" => '\u{3}',
            "Tab" => '\t',
            "Space" => ' ',
            "Delete" => '\u{8}',
            "Escape" => '\u{1b}',
            "ForwardDelete" => '\u{7f}',
            "Home" => '\u{f729}',
            "End" => '\u{f72b}',
            "PageUp" => '\u{f72c}',
            "PageDown" => '\u{f72d}',
            "←" => '\u{f702}',
            "→" => '\u{f703}',
            "↓" => '\u{f701}',
            "↑" => '\u{f700}',
            "KeypadClear" => '\u{f739}',
            "Help" => '\u{f746}',
            n => match (n.strip_prefix('F'), n.strip_prefix("Keypad")) {
                (Some(k), _) if !k.is_empty() => {
                    let k: u32 = k.parse().unwrap_or(1);
                    char::from_u32(0xf703 + k).unwrap_or(' ')
                }
                (_, Some(k)) => k.chars().next().unwrap_or(' '),
                _ => n.chars().next().map_or(' ', |c| c.to_ascii_lowercase()),
            },
        };
        c.to_string()
    }
}

/// Why a binding was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BindError {
    /// No registry command has this ID.
    UnknownCommand(String),
    /// More than [`MAX_CHORDS`] keys.
    TooMany,
    /// The chord is another command's in the same context.
    Taken {
        /// The chord.
        chord: Chord,
        /// The command that has it.
        other: &'static str,
    },
}

impl std::fmt::Display for BindError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BindError::UnknownCommand(id) => write!(f, "no command {:?}", truncate(id, 64)),
            BindError::TooMany => write!(f, "a command has at most {MAX_CHORDS} keys"),
            BindError::Taken { chord, other } => {
                let title = crate::registry::find(other).map_or(*other, |c| c.title);
                write!(f, "{} is the key of “{title}”", chord.text())
            }
        }
    }
}

/// The keys of every command: the registry's defaults plus the user's
/// overrides.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keymap {
    table: &'static [CommandSpec],
    /// Commands whose keys differ from their defaults (an empty list
    /// unbinds).
    overrides: BTreeMap<&'static str, Vec<Chord>>,
}

impl Default for Keymap {
    fn default() -> Keymap {
        Keymap::new(COMMANDS)
    }
}

/// A command's default chords: its shortcut, then its extra keys.
pub fn default_chords(spec: &CommandSpec) -> Vec<Chord> {
    spec.shortcut
        .into_iter()
        .chain(spec.extra_keys.iter().copied())
        .filter_map(|s| Chord::parse(s).ok())
        .collect()
}

impl Keymap {
    /// The defaults of `table`.
    pub fn new(table: &'static [CommandSpec]) -> Keymap {
        Keymap {
            table,
            overrides: BTreeMap::new(),
        }
    }

    fn spec(&self, id: &str) -> Option<&'static CommandSpec> {
        self.table.iter().find(|c| c.id == id)
    }

    /// The command table this keymap binds.
    pub fn table(&self) -> &'static [CommandSpec] {
        self.table
    }

    /// The keys of command `id` now (empty for an unknown ID).
    pub fn chords(&self, id: &str) -> Vec<Chord> {
        match self.spec(id) {
            Some(c) => self
                .overrides
                .get(c.id)
                .cloned()
                .unwrap_or_else(|| default_chords(c)),
            None => Vec::new(),
        }
    }

    /// The command that has `chord` in `context`, if any.
    pub fn holder(&self, chord: Chord, context: KeyContext) -> Option<&'static str> {
        self.table
            .iter()
            .filter(|c| c.context == context)
            .find(|c| self.chords(c.id).contains(&chord))
            .map(|c| c.id)
    }

    /// The command a key press runs: with step entry on, its own keys
    /// first (except the number row's when `reserve_numbers` keeps the
    /// row for locators), then the global ones.
    pub fn lookup(
        &self,
        chord: Chord,
        step_entry: bool,
        reserve_numbers: bool,
    ) -> Option<&'static str> {
        if step_entry && !(reserve_numbers && chord.number_row()) {
            if let Some(id) = self.holder(chord, KeyContext::StepEntry) {
                return Some(id);
            }
        }
        self.holder(chord, KeyContext::Global)
    }

    /// Gives command `id` exactly `chords` (deduplicated, in order). A
    /// chord another command of the same context has is refused, unless
    /// `replace`, which takes it from that command.
    pub fn bind(&mut self, id: &str, chords: &[Chord], replace: bool) -> Result<(), BindError> {
        let spec = self
            .spec(id)
            .ok_or_else(|| BindError::UnknownCommand(id.to_string()))?;
        let mut want: Vec<Chord> = Vec::new();
        for c in chords {
            if !want.contains(c) {
                want.push(*c);
            }
        }
        if want.len() > MAX_CHORDS {
            return Err(BindError::TooMany);
        }
        let mut taken = Vec::new();
        for c in &want {
            if let Some(other) = self.holder(*c, spec.context).filter(|o| *o != spec.id) {
                if !replace {
                    return Err(BindError::Taken { chord: *c, other });
                }
                taken.push((other, *c));
            }
        }
        for (other, c) in taken {
            let left: Vec<Chord> = self.chords(other).into_iter().filter(|x| *x != c).collect();
            self.set(other, left);
        }
        self.set(spec.id, want);
        Ok(())
    }

    fn set(&mut self, id: &'static str, chords: Vec<Chord>) {
        let Some(spec) = self.spec(id) else { return };
        if chords == default_chords(spec) {
            self.overrides.remove(spec.id);
        } else {
            self.overrides.insert(spec.id, chords);
        }
    }

    /// Command `id` back to its default keys; a default another command
    /// took meanwhile stays with that command.
    pub fn reset_one(&mut self, id: &str) {
        let Some(spec) = self.spec(id) else { return };
        self.overrides.remove(spec.id);
        let mine: Vec<Chord> = default_chords(spec)
            .into_iter()
            .filter(|c| {
                self.table
                    .iter()
                    .filter(|o| o.id != spec.id && o.context == spec.context)
                    .all(|o| !self.chords(o.id).contains(c))
            })
            .collect();
        self.set(spec.id, mine);
    }

    /// Every command back to its defaults.
    pub fn reset(&mut self) {
        self.overrides.clear();
    }

    /// The commands whose keys differ from their defaults.
    pub fn changed(&self) -> Vec<&'static str> {
        self.overrides.keys().copied().collect()
    }

    /// The bindings document: `{"vizdaw_keys": 1, "bindings": {id:
    /// [chord text, ...]}}`, holding only the changed commands.
    pub fn to_json(&self) -> String {
        let mut b = Map::new();
        for (id, chords) in &self.overrides {
            b.insert(
                (*id).to_string(),
                Value::from(chords.iter().map(Chord::text).collect::<Vec<_>>()),
            );
        }
        json!({"vizdaw_keys": VERSION, "bindings": Value::Object(b)}).to_string()
    }

    /// Reads a bindings document over the defaults of `table`. Not a
    /// bindings document (not JSON, too large, another version) is an
    /// error; inside one, an unknown command, an unreadable key, a fifth
    /// key or a key another command already has is skipped and named in
    /// the returned list, so a file from another version still loads.
    pub fn from_json(
        table: &'static [CommandSpec],
        text: &str,
    ) -> Result<(Keymap, Vec<String>), String> {
        if text.len() > MAX_JSON {
            return Err(format!("a key bindings file is at most {MAX_JSON} bytes"));
        }
        let v: Value =
            serde_json::from_str(text).map_err(|e| format!("not a key bindings file: {e}"))?;
        if v.get("vizdaw_keys").and_then(Value::as_u64) != Some(VERSION) {
            return Err("not a VizDAW key bindings file (version 1)".into());
        }
        let Some(map) = v.get("bindings").and_then(Value::as_object) else {
            return Err("a key bindings file needs a `bindings` object".into());
        };
        let mut k = Keymap::new(table);
        let mut skipped = Vec::new();
        // First every listed command loses its default keys, so keys that
        // move between commands do not collide with the old owner.
        let mut wanted: Vec<(&'static str, Vec<Chord>)> = Vec::new();
        for (id, keys) in map {
            let Some(spec) = k.spec(id) else {
                skipped.push(format!("no command {:?}", truncate(id, 64)));
                continue;
            };
            let Some(list) = keys.as_array() else {
                skipped.push(format!("{}: the keys are not a list", spec.id));
                continue;
            };
            let mut chords = Vec::new();
            for key in list {
                let parsed = key
                    .as_str()
                    .ok_or_else(|| "a key is not text".to_string())
                    .and_then(Chord::parse);
                match parsed {
                    Ok(c) if chords.contains(&c) => {}
                    Ok(_) if chords.len() == MAX_CHORDS => {
                        skipped.push(format!("{}: more than {MAX_CHORDS} keys", spec.id));
                        break;
                    }
                    Ok(c) => chords.push(c),
                    Err(e) => skipped.push(format!("{}: {e}", spec.id)),
                }
            }
            k.set(spec.id, Vec::new());
            wanted.push((spec.id, chords));
        }
        for (id, chords) in wanted {
            let mut kept = Vec::new();
            for c in chords {
                let context = k.spec(id).map_or(KeyContext::Global, |s| s.context);
                match k.holder(c, context) {
                    Some(other) if other != id => {
                        skipped.push(format!("{id}: {} is already the key of {other}", c.text()))
                    }
                    _ => kept.push(c),
                }
            }
            k.set(id, kept);
        }
        Ok((k, skipped))
    }
}
VIZDAW_EOF
```

In `crates/core-control/src/keys.rs`:

Find:

```rust
    pub modifiers: Modifiers,
}

/// Named keys: name, key code, the character AppKit reports.
const NAMED: [(&str, u16, char); 26] = [
    ("return", 36, '\r'),
    ("enter", 76, '\u{3}'),
    ("tab", 48, '\t'),
```

Replace with:

```rust
    pub modifiers: Modifiers,
}

/// Named keys: name, key code, the character AppKit reports (the keypad's
/// keys since plan 4f, which binds step entry to them).
const NAMED: [(&str, u16, char); 44] = [
    ("return", 36, '\r'),
    ("enter", 76, '\u{3}'),
    ("tab", 48, '\t'),
```

Find:

```rust
    ("f9", 101, '\u{f70c}'),
    ("f10", 109, '\u{f70d}'),
    ("f11", 103, '\u{f70e}'),
];

/// Unshifted characters of the US ANSI layout and their key codes.
```

Replace with:

```rust
    ("f9", 101, '\u{f70c}'),
    ("f10", 109, '\u{f70d}'),
    ("f11", 103, '\u{f70e}'),
    ("f12", 111, '\u{f70f}'),
    ("keypad_0", 82, '0'),
    ("keypad_1", 83, '1'),
    ("keypad_2", 84, '2'),
    ("keypad_3", 85, '3'),
    ("keypad_4", 86, '4'),
    ("keypad_5", 87, '5'),
    ("keypad_6", 88, '6'),
    ("keypad_7", 89, '7'),
    ("keypad_8", 91, '8'),
    ("keypad_9", 92, '9'),
    ("keypad_decimal", 65, '.'),
    ("keypad_divide", 75, '/'),
    ("keypad_multiply", 67, '*'),
    ("keypad_minus", 78, '-'),
    ("keypad_plus", 69, '+'),
    ("keypad_equals", 81, '='),
    ("keypad_clear", 71, '\u{f739}'),
];

/// Unshifted characters of the US ANSI layout and their key codes.
```

In `crates/core-control/src/lib.rs`:

Find:

```rust
//! JSON-lines requests and replies, their limits and errors, element IDs
//! and key strokes (this task); command validation, the menu-command
//! registry and wait conditions (Task 3); the Unix-socket server and
//! client (Task 4). Platform-neutral Unix code: the app executes what
//! needs its windows.
#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![cfg_attr(
```

Replace with:

```rust
//! JSON-lines requests and replies, their limits and errors, element IDs
//! and key strokes (this task); command validation, the menu-command
//! registry and wait conditions (Task 3); the Unix-socket server and
//! client (Task 4); key bindings over the registry (plan 4f).
//! Platform-neutral Unix code: the app executes what needs its windows.
#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![cfg_attr(
```

Find:

```rust
    )
)]

pub mod client;
pub mod element;
pub mod error;
```

Replace with:

```rust
    )
)]

pub mod bindings;
pub mod client;
pub mod element;
pub mod error;
```

In `crates/core-control/src/registry.rs`:

Find:

```rust
//! The menu-command registry (spec §6): every menu command of the app by
//! a stable ID, with its menu, title, shortcut, parameters and the dialogs
//! its parameters answer. `command.run` runs these; plan 4f extends the
//! registry to every command (menu or not) and drives key bindings from it.

use crate::error::{ControlError, ErrorCode};
use crate::keys::{KeyStroke, Modifiers, named_key};
```

Replace with:

```rust
//! The command registry (spec §6, plan 4f): every command of the app,
//! menu item or not, by a stable ID, with its menu (or group), title,
//! default keys, where its keys are live, parameters and the dialogs its
//! parameters answer. `command.run` runs these; [`crate::bindings`] binds
//! keys to them.

use crate::error::{ControlError, ErrorCode};
use crate::keys::{KeyStroke, Modifiers, named_key};
```

Find:

```rust
    pub answered_by: &'static [&'static str],
}

/// A menu command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandSpec {
    /// Stable ID (`transport.play_stop`).
    pub id: &'static str,
    /// The menu it is in.
    pub menu: &'static str,
    /// Its menu title.
    pub title: &'static str,
    /// Its default shortcut, if any.
    pub shortcut: Option<&'static str>,
    /// It acts on a document (the target window's, else the front one's).
    pub document: bool,
    /// Its parameters.
```

Replace with:

```rust
    pub answered_by: &'static [&'static str],
}

/// Where a command's keys are live (plan 4f Ruling 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum KeyContext {
    /// In every VizDAW window that shows a document.
    Global,
    /// Only while step entry is on; there it wins over a global key.
    StepEntry,
}

impl KeyContext {
    /// Its name in `commands.list`.
    pub fn name(self) -> &'static str {
        match self {
            KeyContext::Global => "global",
            KeyContext::StepEntry => "step_entry",
        }
    }
}

/// A command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandSpec {
    /// Stable ID (`transport.play_stop`).
    pub id: &'static str,
    /// The menu it is in (for a command without a menu item, the group the
    /// Keys settings list it under).
    pub menu: &'static str,
    /// Its menu title.
    pub title: &'static str,
    /// Its default shortcut, if any (shown in its menu item).
    pub shortcut: Option<&'static str>,
    /// More default keys (a keypad twin); never shown in menus.
    pub extra_keys: &'static [&'static str],
    /// It has a menu item (false: keys, the Keys settings and the channel
    /// only).
    pub menu_item: bool,
    /// Where its keys are live.
    pub context: KeyContext,
    /// It acts on a document (the target window's, else the front one's).
    pub document: bool,
    /// Its parameters.
```

Find:

```rust
        menu,
        title,
        shortcut,
        document,
        params: &[],
        dialogs: &[],
```

Replace with:

```rust
        menu,
        title,
        shortcut,
        extra_keys: &[],
        menu_item: true,
        context: KeyContext::Global,
        document,
        params: &[],
        dialogs: &[],
```

Find:

```rust
    named_key(&key, m).ok()
}

/// The menu command whose shortcut `stroke` is, if any (control spec
/// §5.5: `ui.key` runs it on the target window's document, as the menu
/// would for the key window).
pub fn command_for_stroke(stroke: &KeyStroke) -> Option<&'static str> {
    COMMANDS.iter().find_map(|c| {
        let s = shortcut_stroke(c.shortcut?)?;
        (s.key_code == stroke.key_code && s.modifiers == stroke.modifiers).then_some(c.id)
    })
}

fn kind_json(k: ParamKind) -> Value {
```

Replace with:

```rust
    named_key(&key, m).ok()
}

/// The global command whose default shortcut `stroke` is, if any
/// (control spec §5.5). Step entry's keys are not global.
pub fn command_for_stroke(stroke: &KeyStroke) -> Option<&'static str> {
    COMMANDS
        .iter()
        .filter(|c| c.context == KeyContext::Global)
        .find_map(|c| {
            let s = shortcut_stroke(c.shortcut?)?;
            (s.key_code == stroke.key_code && s.modifiers == stroke.modifiers).then_some(c.id)
        })
}

fn kind_json(k: ParamKind) -> Value {
```

Find:

```rust
                .iter()
                .map(|d| json!({"name": d.name, "when": d.when, "answered_by": d.answered_by}))
                .collect();
            json!({
                "id": c.id, "menu": c.menu, "title": c.title, "shortcut": c.shortcut,
                "document": c.document, "params": params, "dialogs": dialogs,
            })
        })
```

Replace with:

```rust
                .iter()
                .map(|d| json!({"name": d.name, "when": d.when, "answered_by": d.answered_by}))
                .collect();
            let keys: Vec<String> = crate::bindings::default_chords(c)
                .iter()
                .map(crate::bindings::Chord::text)
                .collect();
            json!({
                "id": c.id, "menu": c.menu, "title": c.title, "shortcut": c.shortcut,
                "keys": keys, "menu_item": c.menu_item, "context": c.context.name(),
                "document": c.document, "params": params, "dialogs": dialogs,
            })
        })
```

- [ ] **Step 4: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-control 2>&1 | grep -E "^test result|FAILED|panicked"
```

Expected: `bindings` 9 passed; `commands` 13 passed (the `commands.list` test reads `keys`, `menu_item` and `context`).

- [ ] **Step 5: Gate and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace 2>&1 | grep -E "FAILED|panicked" ; cargo test --workspace -- --list 2>/dev/null | grep -c ': test$'
scripts/build-ffi.sh && scripts/swift.sh build 2>&1 | tail -1 && scripts/swift.sh test 2>&1 | tail -1
git add crates/core-control/src/bindings.rs \
  crates/core-control/src/keys.rs \
  crates/core-control/src/lib.rs \
  crates/core-control/src/registry.rs \
  crates/core-control/tests/bindings.rs \
  crates/core-control/tests/commands.rs
git commit -m "feat(control): key bindings over the command registry

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
git log -1 --format='%h %s'
```

Expected: 873 Rust tests listed (864 + 9); Swift 613. No `FAILED` or `panicked` line (rerun once if the known-flaky MIDI listener test trips, and say so).

---

### Task 3: Transport in Rust — play until a frame, what Play Selection plays, marker steps, record-arm moves

**Files:**
- Create: `crates/core-editor/src/transport.rs`
- Modify: `crates/core-editor/src/lib.rs`, `crates/core-engine/src/engine.rs`, `crates/core-engine/src/render.rs`, `crates/core-engine/src/transport.rs`, `crates/core-ffi/src/host.rs`, `crates/core-ffi/src/session.rs`, `crates/core-model/src/query/length.rs`, `crates/core-model/src/query/mod.rs`
- Test: `app/Tests/VizDAWAppModelTests/FakeSession.swift`, `crates/core-editor/tests/transport.rs`, `crates/core-engine/tests/transport.rs`, `crates/core-ffi/tests/keys.rs`, `crates/core-ffi/tests/transport.rs`

**Interfaces:**
- Consumes: `core_model::query::length::Lengths`, `Selection`, `Sequence.markers`, `LaneEdit::SetFlags`, `Session.edit_all`, `host::play`, `Engine::{play, locate_tick}`, `Transport::{play_pos, boundary, next}`.
- Produces: `core_engine::transport::Transport.end: Option<u64>` (the loop ignored while set); `EngineCmd::Play { until: Option<u64> }`; `Engine::play_until(Tick)`; `host::play(owner, doc, seq, start, until: Option<Tick>, looped, plugins)` (a rebuild resumes with `until`); `core_model::query::event_end(doc, seq, &Event) -> Result<Tick>`; `core_editor::transport::{selection_span(&Document, &Selection, SeqId) -> Option<(Tick, Tick)>, marker_step(&Sequence, Tick, forward) -> Option<Tick>, ArmMove {disarm: Vec<usize>, arm: usize}, arm_move(&[bool], forward) -> Option<ArmMove>}`; `Session.{play_from(seq, from: i64, to: Option<i64>), selection_span(seq) -> Option<TickRange>, marker_step(seq, tick, forward) -> Option<i64>, move_record_arm(seq, forward) -> DocSnapshot}`; Swift `FakeSession.{playFrom, playsFrom, span, selectionSpan, markerTicks, markerStep, armMoves, armError, moveRecordArm}`.

Rulings 9 and 10. `Session.play` and `play_from` share one private `start_playing`. `FakeSession` gains the four new protocol methods in this task, so the Swift tests keep compiling after `build-ffi.sh` (it is test support, so it goes with the implementation step).

- [ ] **Step 1: Write the failing tests**

Create `crates/core-editor/tests/transport.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p crates/core-editor/tests
cat > crates/core-editor/tests/transport.rs <<'VIZDAW_EOF'
//! The transport keys' logic (plan 4f Task 3): markers and record arm.

use core_editor::transport::{ArmMove, arm_move, marker_step};
use core_model::ids::SeqId;
use core_model::sequence::{Marker, Sequence};
use core_model::time::Tick;
use proptest::prelude::*;

#[test]
fn markers_step_to_the_nearest_one_either_way() {
    let mut s = Sequence::new(SeqId(1), "Song", Default::default());
    assert_eq!(marker_step(&s, Tick(0), true), None, "no markers");
    // Stored out of order: the nearest counts, not the next in the list.
    for t in [61_440, 0, 15_360] {
        s.markers.push_back(Marker {
            tick: Tick(t),
            name: format!("m{t}"),
        });
    }
    assert_eq!(
        marker_step(&s, Tick(0), true),
        Some(Tick(15_360)),
        "strictly after"
    );
    assert_eq!(marker_step(&s, Tick(15_360), true), Some(Tick(61_440)));
    assert_eq!(marker_step(&s, Tick(61_440), true), None);
    assert_eq!(marker_step(&s, Tick(20_000), false), Some(Tick(15_360)));
    assert_eq!(
        marker_step(&s, Tick(15_360), false),
        Some(Tick(0)),
        "strictly before"
    );
    assert_eq!(marker_step(&s, Tick(0), false), None);
    assert_eq!(marker_step(&s, Tick(i64::MIN), true), Some(Tick(0)));
    assert_eq!(marker_step(&s, Tick(i64::MAX), false), Some(Tick(61_440)));
}

#[test]
fn record_arm_moves_one_lane_and_leaves_one_armed() {
    let m = |disarm: Vec<usize>, arm| Some(ArmMove { disarm, arm });
    assert_eq!(
        arm_move(&[false, false, false], true),
        m(vec![], 0),
        "none armed: the first"
    );
    assert_eq!(
        arm_move(&[false, false, false], false),
        m(vec![], 2),
        "or the last"
    );
    assert_eq!(arm_move(&[false, true, false], true), m(vec![1], 2));
    assert_eq!(arm_move(&[false, true, false], false), m(vec![1], 0));
    assert_eq!(arm_move(&[false, false, true], true), None, "at the end");
    assert_eq!(arm_move(&[true, false, false], false), None, "at the start");
    assert_eq!(
        arm_move(&[true, false, true, false], true),
        m(vec![0, 2], 3),
        "from the last armed, all others disarmed"
    );
    assert_eq!(
        arm_move(&[true, false, true, false], false),
        None,
        "from the first"
    );
    assert_eq!(arm_move(&[], true), None);
    assert_eq!(arm_move(&[], false), None);
}

proptest! {
    /// A move arms one lane in range next to the armed ones and disarms
    /// every armed lane; None only at an end or without lanes.
    #[test]
    fn arm_moves_stay_in_range(armed in prop::collection::vec(any::<bool>(), 0..12), forward in any::<bool>()) {
        match arm_move(&armed, forward) {
            Some(m) => {
                prop_assert!(m.arm < armed.len());
                let on: Vec<usize> = (0..armed.len()).filter(|i| armed[*i]).collect();
                prop_assert_eq!(&m.disarm, &on);
                match (forward, on.first(), on.last()) {
                    (true, _, Some(l)) => prop_assert_eq!(m.arm, l + 1),
                    (false, Some(f), _) => prop_assert_eq!(m.arm + 1, *f),
                    (true, _, None) => prop_assert_eq!(m.arm, 0),
                    (false, None, _) => prop_assert_eq!(m.arm, armed.len() - 1),
                }
            }
            None => {
                let edge = if forward { armed.last() } else { armed.first() };
                prop_assert!(armed.is_empty() || edge == Some(&true));
            }
        }
    }
}
VIZDAW_EOF
```

In `crates/core-engine/tests/transport.rs`:

Find:

```rust
                to: 15,
                offset: 0
            }
        ]
    );
}
```

Replace with:

```rust
                to: 15,
                offset: 0
            }
        ]
    );
}

/// Play Selection (plan 4f Ruling 9): an end before the sequence end
/// stops there, the loop ignored; the next Play without one goes on.
#[test]
fn an_end_stops_playing_there_and_ignores_the_loop() {
    let mut t = Transport::new(0);
    t.loop_range = Some((0, 50));
    t.end = Some(40);
    t.locate(10);
    t.play(1000);
    assert_eq!(t.play_pos(1000), Some(10));
    assert_eq!(t.boundary(1000), Some(40), "the end, not the loop end");
    assert_eq!(
        block(&mut t, 1000, 64),
        vec![
            Step::Play {
                from: 10,
                to: 40,
                offset: 0
            },
            Step::End { offset: 30 },
            Step::Idle { len: 34 },
        ]
    );
    assert_eq!(t.state, PlayState::Stopped);
    assert!(t.take_ended());
    assert_eq!(t.pos, 40, "it stays at the end");
    // An end past the sequence end is the sequence end.
    t.end = Some(5000);
    t.locate(990);
    t.play(1000);
    assert_eq!(t.boundary(1000), Some(1000));
    // Without an end the loop is back.
    t.stop();
    t.end = None;
    t.locate(45);
    t.play(1000);
    assert_eq!(t.boundary(1000), Some(50));
}
```

Create `crates/core-ffi/tests/keys.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p crates/core-ffi/tests
cat > crates/core-ffi/tests/keys.rs <<'VIZDAW_EOF'
//! The transport keys' reads and edits (plan 4f Task 3): what Play
//! Selection plays, marker steps, record-arm moves.

use vizdaw_ffi::{
    DocSnapshot, EditCommand, LaneEdit, MarkerRow, SelectionRow, SequenceEdit, Session, TickRange,
    VizError,
};

const Q: i64 = 15_360;
const BAR: i64 = 4 * Q;

fn id_of(s: &DocSnapshot, name: &str) -> u64 {
    s.sequences.iter().find(|r| r.name == name).unwrap().id
}

fn song() -> (std::sync::Arc<Session>, u64) {
    let s = Session::new_demo().unwrap();
    let song = id_of(&s.snapshot().unwrap(), "Song");
    (s, song)
}

fn select(s: &Session, range: Option<TickRange>, events: Vec<u64>) {
    s.set_selection(SelectionRow {
        time_range: range,
        lanes: vec![],
        events,
    })
    .unwrap();
}

#[test]
fn play_selection_plays_the_range_else_the_selected_events() {
    let (s, song) = song();
    assert_eq!(s.selection_span(song).unwrap(), None, "nothing selected");
    let range = TickRange {
        start: BAR,
        end: 2 * BAR,
    };
    select(&s, Some(range), vec![]);
    assert_eq!(s.selection_span(song).unwrap(), Some(range));
    let chords = s.lanes(song).unwrap()[1].id;
    let notes = s.lane_events(song, chords).unwrap().events;
    let (a, b) = (&notes[0], &notes[notes.len() - 1]);
    select(&s, None, vec![b.id, a.id]);
    let end = notes
        .iter()
        .filter(|e| e.id == a.id || e.id == b.id)
        .map(|e| e.tick + e.duration.max(1))
        .max()
        .unwrap();
    assert_eq!(
        s.selection_span(song).unwrap(),
        Some(TickRange { start: a.tick, end }),
        "from the first start to the last end"
    );
    // Events of another sequence do not count.
    let beat = id_of(&s.snapshot().unwrap(), "Beat");
    assert_eq!(s.selection_span(beat).unwrap(), None);
    assert!(matches!(
        s.selection_span(9_999),
        Err(VizError::Model { .. })
    ));
}

#[test]
fn markers_step_from_a_position() {
    let (s, song) = song();
    s.edit(
        EditCommand::Sequence {
            edit: SequenceEdit::SetMarkers {
                seq: song,
                markers: vec![
                    MarkerRow {
                        tick: 4 * BAR,
                        name: "Verse".into(),
                    },
                    MarkerRow {
                        tick: BAR,
                        name: "Intro".into(),
                    },
                ],
            },
        },
        None,
    )
    .unwrap();
    assert_eq!(s.marker_step(song, 0, true).unwrap(), Some(BAR));
    assert_eq!(s.marker_step(song, BAR, true).unwrap(), Some(4 * BAR));
    assert_eq!(s.marker_step(song, 4 * BAR, true).unwrap(), None);
    assert_eq!(s.marker_step(song, 4 * BAR, false).unwrap(), Some(BAR));
    assert_eq!(s.marker_step(song, BAR, false).unwrap(), None);
    assert!(s.marker_step(9_999, 0, true).is_err());
}

#[test]
fn record_arm_moves_between_lanes_as_one_undo_step() {
    let (s, song) = song();
    let lanes =
        |s: &Session| -> Vec<bool> { s.lanes(song).unwrap().iter().map(|l| l.arm).collect() };
    assert!(lanes(&s).len() >= 2, "the demo song has lanes");
    assert!(lanes(&s).iter().all(|a| !a));
    let snap = s.move_record_arm(song, true).unwrap();
    assert_eq!(snap.undo_label.as_deref(), Some("Move Record Arm"));
    assert!(lanes(&s)[0], "none armed: the first lane");
    s.move_record_arm(song, true).unwrap();
    let after = lanes(&s);
    assert!(!after[0] && after[1], "one lane on, the old one off");
    s.move_record_arm(song, false).unwrap();
    assert!(lanes(&s)[0]);
    let e = s.move_record_arm(song, false).unwrap_err();
    assert!(matches!(e, VizError::Model { .. }), "{e:?}");
    s.undo().unwrap();
    let back = lanes(&s);
    assert!(!back[0] && back[1], "one undo step per move");
    // Two armed lanes, a third after them: moving arms the third alone.
    s.edit(
        EditCommand::Lane {
            edit: LaneEdit::Add {
                seq: song,
                name: "Third".into(),
                index: Some(2),
            },
        },
        None,
    )
    .unwrap();
    let first = s.lanes(song).unwrap()[0].id;
    s.edit(
        EditCommand::Lane {
            edit: LaneEdit::SetFlags {
                seq: song,
                lane: first,
                mute: None,
                solo: None,
                arm: Some(true),
            },
        },
        None,
    )
    .unwrap();
    assert_eq!(&lanes(&s)[..3], &[true, true, false]);
    s.move_record_arm(song, true).unwrap();
    assert_eq!(&lanes(&s)[..3], &[false, false, true]);
}
VIZDAW_EOF
```

In `crates/core-ffi/tests/transport.rs`:

Find:

```rust
        "the rebuilt engine dropped the override: {after:?}"
    );
}
```

Replace with:

```rust
        "the rebuilt engine dropped the override: {after:?}"
    );
}

/// Play Selection (plan 4f Ruling 9): playing from a tick up to another
/// stops there by itself, the loop on or not; a plain Play then goes on.
#[test]
fn play_from_stops_at_the_end_of_its_range() {
    let _g = serial();
    let (s, song) = demo();
    s.set_loop(true).unwrap();
    s.play_from(song, BAR, Some(BAR + PPQ / 2)).unwrap();
    assert!(wait_for(&s, |st| st.playing && st.tick >= BAR));
    assert!(wait_for(&s, |st| !st.playing), "it stops by itself");
    let st = t(&s);
    assert!(st.tick >= BAR && st.tick <= BAR + PPQ, "{}", st.tick);
    s.play(song).unwrap();
    assert!(
        wait_for(&s, |st| st.tick > BAR + PPQ),
        "a plain Play goes on"
    );
    s.stop().unwrap();
    for (from, to) in [
        (BAR, Some(BAR)),
        (BAR, Some(0)),
        (1 << 41, None),
        (0, Some(1 << 41)),
    ] {
        assert!(
            matches!(
                s.play_from(song, from, to),
                Err(vizdaw_ffi::VizError::Model { .. })
            ),
            "{from} {to:?}"
        );
    }
    assert!(s.play_from(9_999, 0, None).is_err(), "no such sequence");
}

/// A device change while a selection plays resumes with its end (plan 4f
/// Ruling 9): it still stops there, not at the sequence end.
#[test]
fn a_device_change_keeps_play_selections_end() {
    let _g = serial();
    let (s, song) = demo();
    s.play_from(song, BAR, Some(BAR + 2 * PPQ)).unwrap();
    assert!(wait_for(&s, |st| st.playing && st.tick >= BAR));
    let created = hosts_created();
    simulate_event(BackendEvent::DeviceLost);
    t(&s);
    assert_eq!(hosts_created(), created + 1, "rebuilt");
    // The new engine applies Play on its first block.
    assert!(wait_for(&s, |st| st.playing), "resumed");
    assert!(wait_for(&s, |st| !st.playing), "it still stops at its end");
    let st = t(&s);
    assert!(st.tick >= BAR && st.tick <= BAR + 3 * PPQ, "{}", st.tick);
}
```

- [ ] **Step 2: Run them to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-editor --test transport 2>&1 | grep -E "^error" | head -3
cargo test -p core-engine --test transport 2>&1 | grep -E "^error" | head -3
cargo test -p core-ffi --test keys 2>&1 | grep -E "^error" | head -3
```

Expected: FAIL to compile: `could not find transport in core_editor`; no field `end` on `Transport`; no method `selection_span`, `marker_step`, `move_record_arm`, `play_from` on `Session`.

- [ ] **Step 3: Implement**

In `crates/core-editor/src/lib.rs`:

Find:

```rust
//! drawing — what a point is over, cells, strokes, where a dragged row
//! lands — as pure functions of plain data, so a front end only draws
//! (editors spec §3.5: hit-testing and geometry are pure functions; Plan
//! 4e Ruling 1). Platform-neutral; it depends only on `core-model`.
#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![cfg_attr(
```

Replace with:

```rust
//! drawing — what a point is over, cells, strokes, where a dragged row
//! lands — as pure functions of plain data, so a front end only draws
//! (editors spec §3.5: hit-testing and geometry are pure functions; Plan
//! 4e Ruling 1); the transport keys' logic (plan 4f).
//! Platform-neutral; it depends only on `core-model`.
#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![cfg_attr(
```

Find:

```rust
)]

pub mod drum;
```

Replace with:

```rust
)]

pub mod drum;
pub mod transport;
```

Create `crates/core-editor/src/transport.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p crates/core-editor/src
cat > crates/core-editor/src/transport.rs <<'VIZDAW_EOF'
//! Transport keys' logic (plan 4f): what Play Selection plays, the marker
//! before or after a position, and where record arm moves.

use core_model::document::Document;
use core_model::ids::SeqId;
use core_model::query::event_end;
use core_model::selection::Selection;
use core_model::sequence::Sequence;
use core_model::time::Tick;

/// What Play Selection plays in sequence `seq` (plan 4f Ruling 9): the
/// selection's range when it has one, else from the earliest selected
/// event of `seq` to the latest end among them (an event ends at least a
/// tick after it starts). None when nothing of `seq` is selected.
pub fn selection_span(doc: &Document, sel: &Selection, seq: SeqId) -> Option<(Tick, Tick)> {
    if let Some((a, b)) = sel.time_range.filter(|(a, b)| b > a) {
        return Some((a, b));
    }
    let mut span: Option<(Tick, Tick)> = None;
    for id in &sel.events {
        let Ok((e, loc)) = doc.event(*id) else {
            continue;
        };
        if loc.seq != seq {
            continue;
        }
        let end = event_end(doc, seq, e)
            .unwrap_or(e.tick)
            .max(e.tick + Tick(1));
        span = Some(match span {
            Some((a, b)) => (a.min(e.tick), b.max(end)),
            None => (e.tick, end),
        });
    }
    span
}

/// The nearest marker of `seq` after `tick` (`forward`) or before it.
pub fn marker_step(seq: &Sequence, tick: Tick, forward: bool) -> Option<Tick> {
    let ticks = seq.markers.iter().map(|m| m.tick);
    if forward {
        ticks.filter(|t| *t > tick).min()
    } else {
        ticks.filter(|t| *t < tick).max()
    }
}

/// A record-arm move: the lanes (by index) to disarm and the one to arm.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArmMove {
    /// Armed lanes that lose their arm.
    pub disarm: Vec<usize>,
    /// The lane armed instead.
    pub arm: usize,
}

/// Moves record arm to the next (`forward`) or previous lane (plan 4f
/// Ruling 10): from the last armed lane forward or the first backward;
/// with none armed, the first or last lane. Every other armed lane is
/// disarmed, so one lane is armed after. None at either end (or without
/// lanes).
pub fn arm_move(armed: &[bool], forward: bool) -> Option<ArmMove> {
    let n = armed.len();
    let arm = if forward {
        match armed.iter().rposition(|a| *a) {
            Some(l) => l.checked_add(1).filter(|i| *i < n)?,
            None if n > 0 => 0,
            None => return None,
        }
    } else {
        match armed.iter().position(|a| *a) {
            Some(f) => f.checked_sub(1)?,
            None => n.checked_sub(1)?,
        }
    };
    let disarm = armed
        .iter()
        .enumerate()
        .filter(|(_, a)| **a)
        .map(|(i, _)| i)
        .collect();
    Some(ArmMove { disarm, arm })
}
VIZDAW_EOF
```

In `crates/core-engine/src/engine.rs`:

Find:

```rust

    /// Starts playback (from the start when at or past the end).
    pub fn play(&mut self) -> Result<(), EngineError> {
        self.push(EngineCmd::Play)
    }

    /// Stops playback; sounding notes get note-offs.
```

Replace with:

```rust

    /// Starts playback (from the start when at or past the end).
    pub fn play(&mut self) -> Result<(), EngineError> {
        self.push(EngineCmd::Play { until: None })
    }

    /// Starts playback that stops at `tick` of the loaded sequence, the
    /// loop ignored (Play Selection, plan 4f Ruling 9).
    pub fn play_until(&mut self, tick: Tick) -> Result<(), EngineError> {
        let frame = self.tempo.as_ref().map_or(0, |t| t.tick_to_frame(tick));
        self.push(EngineCmd::Play { until: Some(frame) })
    }

    /// Stops playback; sounding notes get note-offs.
```

In `crates/core-engine/src/render.rs`:

Find:

```rust
    /// Replace the render graph; it adopts the running graph's inserts and,
    /// with the same shape, its node state.
    SetGraph(Box<Graph>),
    /// Start playing.
    Play,
    /// Stop (note-offs are sent).
    Stop,
    /// Move to a schedule frame.
```

Replace with:

```rust
    /// Replace the render graph; it adopts the running graph's inserts and,
    /// with the same shape, its node state.
    SetGraph(Box<Graph>),
    /// Start playing; `until` (a schedule frame) stops it there instead
    /// of at the end, ignoring the loop (Play Selection, plan 4f).
    Play {
        /// The frame playing stops at.
        until: Option<u64>,
    },
    /// Stop (note-offs are sent).
    Stop,
    /// Move to a schedule frame.
```

Find:

```rust
                            new.tempo.tick_to_frame(old.tempo.frame_to_tick(e)),
                        )
                    });
                    if let Err(rtrb::PushError::Full(old)) =
                        self.retire.push(Retired::Schedule(old))
                    {
```

Replace with:

```rust
                            new.tempo.tick_to_frame(old.tempo.frame_to_tick(e)),
                        )
                    });
                    self.transport.end = self
                        .transport
                        .end
                        .map(|e| new.tempo.tick_to_frame(old.tempo.frame_to_tick(e)));
                    if let Err(rtrb::PushError::Full(old)) =
                        self.retire.push(Retired::Schedule(old))
                    {
```

Find:

```rust
                // New inserts need their automated values.
                self.auto_last.fill(f64::NAN);
            }
            EngineCmd::Play => {
                let length = self.schedule.as_ref().map_or(0, |s| s.length_frames);
                self.transport.play(length);
                self.seek();
                self.forget_unautomated();
```

Replace with:

```rust
                // New inserts need their automated values.
                self.auto_last.fill(f64::NAN);
            }
            EngineCmd::Play { until } => {
                let length = self.schedule.as_ref().map_or(0, |s| s.length_frames);
                self.transport.end = until;
                self.transport.play(length);
                self.seek();
                self.forget_unautomated();
```

In `crates/core-engine/src/transport.rs`:

Find:

```rust
    pub pos: u64,
    /// Loop range `[start, end)`; ignored unless `end > start`.
    pub loop_range: Option<(u64, u64)>,
    /// Tail length in frames.
    pub tail_frames: u64,
    /// Graph latency `L`: added to the tail, so `ended` fires only after
```

Replace with:

```rust
    pub pos: u64,
    /// Loop range `[start, end)`; ignored unless `end > start`.
    pub loop_range: Option<(u64, u64)>,
    /// Where playing stops before the sequence end (Play Selection, plan
    /// 4f Ruling 9); while it is set the loop is ignored. Every Play sets
    /// or clears it.
    pub end: Option<u64>,
    /// Tail length in frames.
    pub tail_frames: u64,
    /// Graph latency `L`: added to the tail, so `ended` fires only after
```

Find:

```rust
            state: PlayState::Stopped,
            pos: 0,
            loop_range: None,
            tail_frames,
            latency: 0,
            tail_left: 0,
```

Replace with:

```rust
            state: PlayState::Stopped,
            pos: 0,
            loop_range: None,
            end: None,
            tail_frames,
            latency: 0,
            tail_left: 0,
```

Find:

```rust
    }

    fn active_loop(&self) -> Option<(u64, u64)> {
        self.loop_range.filter(|(s, e)| e > s)
    }

    /// Starts playing from `pos` (from 0 when at or past `length`).
    pub fn play(&mut self, length: u64) {
        if self.state == PlayState::Playing {
```

Replace with:

```rust
    }

    fn active_loop(&self) -> Option<(u64, u64)> {
        if self.end.is_some() {
            return None;
        }
        self.loop_range.filter(|(s, e)| e > s)
    }

    /// Where playing ends without a loop: `end` when set (within the
    /// sequence), else the sequence end. // RT
    fn last(&self, length: u64) -> u64 {
        self.end.map_or(length, |e| e.min(length))
    }

    /// Starts playing from `pos` (from 0 when at or past `length`).
    pub fn play(&mut self, length: u64) {
        if self.state == PlayState::Playing {
```

Find:

```rust
        };
        let end = match lp {
            Some((_, e)) if pos < e => e,
            _ => length,
        };
        (pos < end).then_some(pos)
    }
```

Replace with:

```rust
        };
        let end = match lp {
            Some((_, e)) if pos < e => e,
            _ => self.last(length),
        };
        (pos < end).then_some(pos)
    }
```

Find:

```rust
        let pos = self.play_pos(length)?;
        Some(match self.active_loop() {
            Some((_, e)) if pos < e => e,
            _ => length,
        })
    }

```

Replace with:

```rust
        let pos = self.play_pos(length)?;
        Some(match self.active_loop() {
            Some((_, e)) if pos < e => e,
            _ => self.last(length),
        })
    }

```

Find:

```rust
                }
                let end = match lp {
                    Some((_, e)) if self.pos < e => e,
                    _ => length,
                };
                if self.pos >= end {
                    self.state = PlayState::Tail;
```

Replace with:

```rust
                }
                let end = match lp {
                    Some((_, e)) if self.pos < e => e,
                    _ => self.last(length),
                };
                if self.pos >= end {
                    self.state = PlayState::Tail;
```

In `crates/core-ffi/src/host.rs`:

Find:

```rust
    /// its next audio block, so `engine.state()` can still say Stopped
    /// right after `play`; a rebuild must resume from this too.
    want_play: bool,
    device: String,
    sample_rate: f64,
    report: Vec<String>,
```

Replace with:

```rust
    /// its next audio block, so `engine.state()` can still say Stopped
    /// right after `play`; a rebuild must resume from this too.
    want_play: bool,
    /// Where the asked Play stops (Play Selection, plan 4f): a rebuild
    /// resumes with it.
    until: Option<Tick>,
    device: String,
    sample_rate: f64,
    report: Vec<String>,
```

Find:

```rust
            seq: SeqId(0),
            looped: false,
            want_play: false,
            device: device.to_string(),
            sample_rate,
            report: Vec::new(),
```

Replace with:

```rust
            seq: SeqId(0),
            looped: false,
            want_play: false,
            until: None,
            device: device.to_string(),
            sample_rate,
            report: Vec::new(),
```

Find:

```rust
        playing,
        drain,
    } = r;
    let (owner, seq, looped) = (old.owner, old.seq, old.looped);
    if drain {
        std::thread::sleep(DRAIN);
    }
```

Replace with:

```rust
        playing,
        drain,
    } = r;
    let (owner, seq, looped, until) = (old.owner, old.seq, old.looped, old.until);
    if drain {
        std::thread::sleep(DRAIN);
    }
```

Find:

```rust
    new.load(doc, seq, looped)?;
    new.engine.locate_tick(at)?;
    if playing {
        new.engine.play()?;
        new.want_play = true;
    }
    new.owner = owner;
    new.seq = seq;
```

Replace with:

```rust
    new.load(doc, seq, looped)?;
    new.engine.locate_tick(at)?;
    if playing {
        match until {
            Some(t) => new.engine.play_until(t)?,
            None => new.engine.play()?,
        }
        new.want_play = true;
        new.until = until;
    }
    new.owner = owner;
    new.seq = seq;
```

Find:

```rust

/// Plays `seq` of `doc` for session `owner`. A different owner, sequence
/// or an explicit `start` reloads (stopping whoever played); otherwise the
/// engine resumes where it stopped.
pub fn play(
    owner: u64,
    doc: &Document,
    seq: SeqId,
    start: Option<Tick>,
    looped: bool,
    mut plugins: Option<&mut PluginHost>,
) -> Result<(), VizError> {
```

Replace with:

```rust

/// Plays `seq` of `doc` for session `owner`. A different owner, sequence
/// or an explicit `start` reloads (stopping whoever played); otherwise the
/// engine resumes where it stopped. `until` stops it there (the loop
/// ignored; plan 4f Ruling 9).
pub fn play(
    owner: u64,
    doc: &Document,
    seq: SeqId,
    start: Option<Tick>,
    until: Option<Tick>,
    looped: bool,
    mut plugins: Option<&mut PluginHost>,
) -> Result<(), VizError> {
```

Find:

```rust
        host.load(doc, seq, looped)?;
    }
    host.faults.clear();
    host.engine.play()?;
    host.want_play = true;
    Ok(())
}

```

Replace with:

```rust
        host.load(doc, seq, looped)?;
    }
    host.faults.clear();
    match until {
        Some(t) => host.engine.play_until(t)?,
        None => host.engine.play()?,
    }
    host.want_play = true;
    host.until = until;
    Ok(())
}

```

Find:

```rust
    match slot.as_mut() {
        Some(h) if h.owner == owner => {
            h.want_play = false;
            Ok(h.engine.stop()?)
        }
        _ => Ok(()),
```

Replace with:

```rust
    match slot.as_mut() {
        Some(h) if h.owner == owner => {
            h.want_play = false;
            h.until = None;
            Ok(h.engine.stop()?)
        }
        _ => Ok(()),
```

In `crates/core-ffi/src/session.rs`:

Find:

```rust
        inner.snapshot()
    }

    /// Runs a transport call; an `Internal` error drops the engine host so
    /// the next Play starts clean.
    fn transport_call<T>(&self, f: impl FnOnce() -> Result<T, VizError>) -> Result<T, VizError> {
```

Replace with:

```rust
        inner.snapshot()
    }

    /// Plays `seq` from `start`, else from where this session's position
    /// is (a position set while another session held the engine), up to
    /// `until` or the end.
    fn start_playing(
        &self,
        seq: u64,
        start: Option<Tick>,
        until: Option<Tick>,
    ) -> Result<(), VizError> {
        let mut inner = lock(&self.inner);
        let id = seq_id(inner.doc(), seq)?;
        let start = start.or(match (inner.cursor, inner.cursor_seq) {
            (Some(t), None) => Some(t),
            (Some(t), Some(s)) if s == id => Some(t),
            _ => None,
        });
        let looped = inner.looped;
        inner.mark(self.id, true);
        let played = plugins::with(self.id, |p| {
            host::play(self.id, inner.doc(), id, start, until, looped, p)
        })
        .and_then(|r| r);
        if let Err(e) = played {
            // Retried by `idle` (the engine may hold stale plugins).
            inner.reload_owed = true;
            return Err(e);
        }
        inner.reload_owed = false;
        inner.reload_fault = None;
        inner.cursor = None;
        inner.cursor_seq = Some(id);
        Ok(())
    }

    /// Runs a transport call; an `Internal` error drops the engine host so
    /// the next Play starts clean.
    fn transport_call<T>(&self, f: impl FnOnce() -> Result<T, VizError>) -> Result<T, VizError> {
```

Find:

```rust
    /// Plays sequence `seq`, stopping any other session. Resumes where
    /// this session stopped when the same sequence is still loaded.
    pub fn play(&self, seq: u64) -> Result<(), VizError> {
        self.transport_call(|| {
            let mut inner = lock(&self.inner);
            let id = seq_id(inner.doc(), seq)?;
            // A position set while another session held the engine.
            let start = match (inner.cursor, inner.cursor_seq) {
                (Some(t), None) => Some(t),
                (Some(t), Some(s)) if s == id => Some(t),
                _ => None,
            };
            let looped = inner.looped;
            inner.mark(self.id, true);
            let played = plugins::with(self.id, |p| {
                host::play(self.id, inner.doc(), id, start, looped, p)
            })
            .and_then(|r| r);
            if let Err(e) = played {
                // Retried by `idle` (the engine may hold stale plugins).
                inner.reload_owed = true;
                return Err(e);
            }
            inner.reload_owed = false;
            inner.reload_fault = None;
            inner.cursor = None;
            inner.cursor_seq = Some(id);
            Ok(())
        })
    }

    /// Stops; the position is kept.
    pub fn stop(&self) -> Result<(), VizError> {
        self.transport_call(|| {
```

Replace with:

```rust
    /// Plays sequence `seq`, stopping any other session. Resumes where
    /// this session stopped when the same sequence is still loaded.
    pub fn play(&self, seq: u64) -> Result<(), VizError> {
        self.transport_call(|| self.start_playing(seq, None, None))
    }

    /// Plays sequence `seq` from `from` (plan 4f Ruling 9): on to its end,
    /// or up to `to`, where it stops (the loop ignored). `Model` error
    /// unless `from < to` and both are within ±2^40 ticks.
    pub fn play_from(&self, seq: u64, from: i64, to: Option<i64>) -> Result<(), VizError> {
        self.transport_call(|| {
            core_model::rules::check_ticks("from", from)?;
            if let Some(to) = to {
                core_model::rules::check_ticks("to", to)?;
                if to <= from {
                    return Err(crate::events::bad("the end must be after the start"));
                }
            }
            self.start_playing(seq, Some(Tick(from)), to.map(Tick))
        })
    }

    /// Where Play Selection plays in `seq`: the selection's range, else
    /// the span of its selected events in `seq` (plan 4f Ruling 9); None
    /// when nothing of `seq` is selected.
    pub fn selection_span(
        &self,
        seq: u64,
    ) -> Result<Option<crate::selection::TickRange>, VizError> {
        ffi_guard(|| {
            let inner = lock(&self.inner);
            let id = seq_id(inner.doc(), seq)?;
            let span =
                core_editor::transport::selection_span(inner.doc(), inner.history.selection(), id);
            Ok(span.map(|(a, b)| crate::selection::TickRange {
                start: a.0,
                end: b.0,
            }))
        })
    }

    /// The nearest marker of `seq` after (`forward`) or before `tick`.
    pub fn marker_step(&self, seq: u64, tick: i64, forward: bool) -> Result<Option<i64>, VizError> {
        ffi_guard(|| {
            let inner = lock(&self.inner);
            let s = inner.doc().sequence(seq_id(inner.doc(), seq)?)?;
            Ok(core_editor::transport::marker_step(s, Tick(tick), forward).map(|t| t.0))
        })
    }

    /// Moves record arm to the next (`forward`) or previous lane of `seq`
    /// as one undo step "Move Record Arm" (plan 4f Ruling 10). `Model`
    /// error at the first or last lane.
    pub fn move_record_arm(&self, seq: u64, forward: bool) -> Result<DocSnapshot, VizError> {
        let (lanes, m) = ffi_guard(|| {
            let inner = lock(&self.inner);
            let s = inner.doc().sequence(seq_id(inner.doc(), seq)?)?;
            let armed: Vec<bool> = s.lanes.iter().map(|l| l.arm).collect();
            let ids: Vec<u64> = s.lanes.iter().map(|l| l.id.0).collect();
            let m = core_editor::transport::arm_move(&armed, forward).ok_or_else(|| {
                crate::events::bad(if forward {
                    "there is no lane after the armed one"
                } else {
                    "there is no lane before the armed one"
                })
            })?;
            Ok((ids, m))
        })?;
        let lane = |i: usize| lanes.get(i).copied().unwrap_or(0);
        let flags = |i: usize, arm: bool| crate::edit::EditCommand::Lane {
            edit: crate::edit::LaneEdit::SetFlags {
                seq,
                lane: lane(i),
                mute: None,
                solo: None,
                arm: Some(arm),
            },
        };
        let mut cmds: Vec<_> = m
            .disarm
            .iter()
            .filter(|i| **i != m.arm)
            .map(|i| flags(*i, false))
            .collect();
        cmds.push(flags(m.arm, true));
        self.edit_all("Move Record Arm".into(), cmds, None)
    }

    /// Stops; the position is kept.
    pub fn stop(&self) -> Result<(), VizError> {
        self.transport_call(|| {
```

In `crates/core-model/src/query/length.rs`:

Find:

```rust
    }
}

/// The playing length of a sequence (derived lengths are computed, never
/// stored).
pub fn seq_length(doc: &Document, seq: SeqId) -> Result<Tick> {
```

Replace with:

```rust
    }
}

/// Where event `e` of sequence `seq` ends: a note after its duration, a
/// reference after its uses, a region after its audio, any other event at
/// its tick.
pub fn event_end(doc: &Document, seq: SeqId, e: &Event) -> Result<Tick> {
    let owner = doc.sequence(seq)?;
    Ok(Lengths::default().event_end(doc, owner, e))
}

/// The playing length of a sequence (derived lengths are computed, never
/// stored).
pub fn seq_length(doc: &Document, seq: SeqId) -> Result<Tick> {
```

In `crates/core-model/src/query/mod.rs`:

Find:

```rust
pub mod usage;

pub use flatten::{PathStep, ResolvedEvent, flatten};
pub use length::{reference_span, seq_length};
pub use routing::{Routing, resolve, resolve_event};
pub use usage::{
    drum_view, media_in_use, unresolved_events, unused_sequences, use_count, use_counts, users_of,
```

Replace with:

```rust
pub mod usage;

pub use flatten::{PathStep, ResolvedEvent, flatten};
pub use length::{event_end, reference_span, seq_length};
pub use routing::{Routing, resolve, resolve_event};
pub use usage::{
    drum_view, media_in_use, unresolved_events, unused_sequences, use_count, use_counts, users_of,
```

In `app/Tests/VizDAWAppModelTests/FakeSession.swift`:

Find:

```swift
        state.seq = seq
    }

    var undoCalls = 0
    var redoCalls = 0
    func redo() throws -> DocSnapshot {
```

Replace with:

```swift
        state.seq = seq
    }

    /// `playFrom` calls: sequence, start and end (plan 4f).
    var playsFrom: [(seq: UInt64, from: Int64, to: Int64?)] = []
    func playFrom(seq: UInt64, from: Int64, to: Int64?) throws {
        if let e = playError { throw e }
        playsFrom.append((seq, from, to))
        state.playing = true
        state.seq = seq
        state.tick = from
    }

    /// What `selectionSpan` answers.
    var span: TickRange?
    func selectionSpan(seq: UInt64) throws -> TickRange? { span }

    /// Marker ticks `markerStep` steps between.
    var markerTicks: [Int64] = []
    func markerStep(seq: UInt64, tick: Int64, forward: Bool) throws -> Int64? {
        forward ? markerTicks.filter { $0 > tick }.min() : markerTicks.filter { $0 < tick }.max()
    }

    var armMoves: [(seq: UInt64, forward: Bool)] = []
    var armError: VizError?
    func moveRecordArm(seq: UInt64, forward: Bool) throws -> DocSnapshot {
        if let e = armError { throw e }
        armMoves.append((seq, forward))
        return commandResult()
    }

    var undoCalls = 0
    var redoCalls = 0
    func redo() throws -> DocSnapshot {
```

- [ ] **Step 4: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-editor --test transport 2>&1 | grep -E "^test result"
cargo test -p core-engine --test transport 2>&1 | grep -E "^test result"
cargo test -p core-ffi --test keys --test transport 2>&1 | grep -E "^test result|FAILED|panicked"
```

Expected: core-editor `transport` 3 passed; core-engine `transport` 9 passed; core-ffi `keys` 3 passed, `transport` 24 passed (the offline engine runs in real time: about 15 s).

- [ ] **Step 5: Gate and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p vizdaw-play --all-targets --features rt-check -- -D warnings
cargo test --workspace 2>&1 | grep -E "FAILED|panicked" ; cargo test --workspace -- --list 2>/dev/null | grep -c ': test$'
scripts/build-ffi.sh && scripts/swift.sh build 2>&1 | tail -1 && scripts/swift.sh test 2>&1 | tail -1
git add app/Tests/VizDAWAppModelTests/FakeSession.swift \
  crates/core-editor/src/lib.rs \
  crates/core-editor/src/transport.rs \
  crates/core-editor/tests/transport.rs \
  crates/core-engine/src/engine.rs \
  crates/core-engine/src/render.rs \
  crates/core-engine/src/transport.rs \
  crates/core-engine/tests/transport.rs \
  crates/core-ffi/src/host.rs \
  crates/core-ffi/src/session.rs \
  crates/core-ffi/tests/keys.rs \
  crates/core-ffi/tests/transport.rs \
  crates/core-model/src/query/length.rs \
  crates/core-model/src/query/mod.rs
git commit -m "feat(transport): play until a frame, selection spans, marker steps and record-arm moves

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
git log -1 --format='%h %s'
```

Expected: 882 Rust tests listed (873 + 9); Swift 613. No `FAILED` or `panicked` line (rerun once if the known-flaky MIDI listener test trips, and say so).

---

### Task 4: `core_editor::step` — the step entry state machine and typed pitches

**Files:**
- Create: `crates/core-editor/src/step.rs`
- Modify: `crates/core-editor/src/lib.rs`
- Test: `crates/core-editor/tests/step.rs`

**Interfaces:**
- Consumes: `core_model::{ids::{EventId, SeqId}, time::{PPQ, Tick}}`.
- Produces: `core_editor::step::{MAX_CHORD=16, MAX_HISTORY=256, MAX_PITCH_TEXT=256, NoteValue {Whole, Half, Quarter, Eighth, Sixteenth, ThirtySecond} (ticks()), step_length(NoteValue, dotted, triplet) -> Tick, StepRecord {tick, length, kind}, StepKind {Notes(Vec<EventId>), Rest, Tie {notes, by}, Reference(EventId)}, StepState {tick, value, dotted, triplet, held: Vec<u8>, chord: Vec<(u8, u16)>, history: Vec<StepRecord>} (new(Tick), length(), bounded()), StepInput {Value(NoteValue), Dot, Triplet, Tie, Rest, DeleteLast, NoteOn {pitch, velocity}, NoteOff {pitch}, Notes(Vec<(u8, u16)>), Reference {seq, length}}, Effect {Nothing, Rest {length}, Insert {tick, length, notes}, InsertReference {tick, seq, length}, Extend {notes, by}, TakeBack {delete, shorten, by, to}}, press(&StepState, StepInput) -> (StepState, Effect), commit(&StepState, &Effect, created: &[EventId]) -> StepState, parse_pitches(&str) -> Result<Vec<u8>, String>}`.

Rulings 11–14 as pure data: `press` turns an input into the effect the document must take (the state it returns has its settings changed but has not moved on); `commit` records the step and moves the position once the document took it. A refused effect is never committed. Pitches above 127 are masked; chords hold at most 16 notes, each pitch once, sorted.

- [ ] **Step 1: Write the failing tests**

Create `crates/core-editor/tests/step.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p crates/core-editor/tests
cat > crates/core-editor/tests/step.rs <<'VIZDAW_EOF'
//! Step entry's state machine (plan 4f Task 4).

use core_editor::step::{
    Effect, MAX_CHORD, MAX_HISTORY, NoteValue, StepInput, StepKind, StepRecord, StepState, commit,
    parse_pitches, press, step_length,
};
use core_model::ids::{EventId, SeqId};
use core_model::time::Tick;
use proptest::prelude::*;

const Q: i64 = 15_360;

fn ids(n: &[u64]) -> Vec<EventId> {
    n.iter().copied().map(EventId).collect()
}

/// Runs `inputs`, committing each effect with the next IDs from `next`.
fn run(mut s: StepState, inputs: Vec<StepInput>, next: &mut u64) -> (StepState, Vec<Effect>) {
    let mut effects = Vec::new();
    for i in inputs {
        let (t, e) = press(&s, i);
        let created: Vec<EventId> = match &e {
            Effect::Insert { notes, .. } => notes
                .iter()
                .map(|_| {
                    *next += 1;
                    EventId(*next)
                })
                .collect(),
            Effect::InsertReference { .. } => {
                *next += 1;
                vec![EventId(*next)]
            }
            _ => vec![],
        };
        s = commit(&t, &e, &created);
        effects.push(e);
    }
    (s, effects)
}

#[test]
fn values_dots_and_triplets_give_the_length() {
    use NoteValue::*;
    let table = [
        (Whole, 4 * Q),
        (Half, 2 * Q),
        (Quarter, Q),
        (Eighth, Q / 2),
        (Sixteenth, Q / 4),
        (ThirtySecond, Q / 8),
    ];
    for (v, t) in table {
        assert_eq!(step_length(v, false, false), Tick(t), "{v:?}");
        assert_eq!(step_length(v, true, false), Tick(t * 3 / 2), "dotted {v:?}");
        assert_eq!(
            step_length(v, false, true),
            Tick(t * 2 / 3),
            "triplet {v:?}"
        );
        assert_eq!(step_length(v, true, true), Tick(t), "both {v:?}");
    }
    let mut s = StepState::new(Tick(0));
    assert_eq!(s.length(), Tick(Q), "quarter notes to start");
    s = press(&s, StepInput::Value(NoteValue::Eighth)).0;
    s = press(&s, StepInput::Dot).0;
    assert_eq!(s.length(), Tick(Q * 3 / 4));
    s = press(&s, StepInput::Value(NoteValue::Half)).0;
    assert!(s.dotted, "dot and triplet stay until toggled");
    s = press(&s, StepInput::Dot).0;
    s = press(&s, StepInput::Triplet).0;
    assert_eq!(s.length(), Tick(2 * Q * 2 / 3));
}

#[test]
fn a_chord_is_every_key_down_until_the_last_is_released() {
    let s = StepState::new(Tick(Q));
    let on = |p: u8| StepInput::NoteOn {
        pitch: p,
        velocity: u16::from(p) << 8,
    };
    let off = |p: u8| StepInput::NoteOff { pitch: p };
    let mut next = 0;
    let (s, effects) = run(
        s,
        vec![
            off(50),
            on(64),
            on(60),
            on(60),
            off(64),
            on(67),
            off(60),
            off(67),
        ],
        &mut next,
    );
    assert!(
        effects[..7].iter().all(|e| *e == Effect::Nothing),
        "{effects:?}"
    );
    assert_eq!(
        effects[7],
        Effect::Insert {
            tick: Tick(Q),
            length: Tick(Q),
            notes: vec![(60, 60 << 8), (64, 64 << 8), (67, 67 << 8)],
        },
        "by pitch, each once, with its own velocity"
    );
    assert_eq!(s.tick, Tick(2 * Q), "moved on by the step");
    assert!(s.held.is_empty() && s.chord.is_empty());
    assert_eq!(
        s.history,
        vec![StepRecord {
            tick: Tick(Q),
            length: Tick(Q),
            kind: StepKind::Notes(ids(&[1, 2, 3])),
        }]
    );
    // Pitches above 127 are masked; a chord never holds more than 16.
    let mut many = StepState::new(Tick(0));
    for p in 0..40u8 {
        many = press(&many, on(p.wrapping_add(200))).0;
    }
    assert_eq!(many.chord.len(), MAX_CHORD);
    assert!(many.chord.iter().all(|(p, _)| *p < 128));
}

#[test]
fn rest_and_tie_move_on_and_a_tie_lengthens_the_last_notes() {
    let mut next = 0;
    let s = StepState::new(Tick(0));
    let (s, effects) = run(
        s,
        vec![
            StepInput::Tie,
            StepInput::Notes(vec![(62, 100), (62, 90)]),
            StepInput::Value(NoteValue::Eighth),
            StepInput::Tie,
            StepInput::Tie,
            StepInput::Rest,
            StepInput::Tie,
        ],
        &mut next,
    );
    assert_eq!(
        effects[0],
        Effect::Rest { length: Tick(Q) },
        "a tie with nothing to tie rests"
    );
    assert_eq!(
        effects[1],
        Effect::Insert {
            tick: Tick(Q),
            length: Tick(Q),
            notes: vec![(62, 100)]
        }
    );
    assert_eq!(
        effects[3],
        Effect::Extend {
            notes: ids(&[1]),
            by: Tick(Q / 2)
        }
    );
    assert_eq!(
        effects[4], effects[3],
        "a second tie lengthens the same notes"
    );
    assert_eq!(
        effects[6],
        Effect::Rest {
            length: Tick(Q / 2)
        },
        "after a rest, a tie rests"
    );
    assert_eq!(s.tick, Tick(Q + Q + Q / 2 * 4));
}

#[test]
fn delete_last_takes_each_kind_of_step_back() {
    let mut next = 0;
    let start = StepState::new(Tick(2 * Q));
    let (s, _) = run(
        start.clone(),
        vec![
            StepInput::Notes(vec![(60, 1), (64, 1)]),
            StepInput::Tie,
            StepInput::Rest,
            StepInput::Reference {
                seq: SeqId(7),
                length: Tick(8 * Q),
            },
        ],
        &mut next,
    );
    assert_eq!(s.tick, Tick(2 * Q + Q + Q + Q + 8 * Q));
    let back = |s: &StepState| press(s, StepInput::DeleteLast).1;
    assert_eq!(
        back(&s),
        Effect::TakeBack {
            delete: ids(&[3]),
            shorten: vec![],
            by: Tick(0),
            to: Tick(5 * Q)
        },
        "the reference"
    );
    let s = commit(&s, &back(&s), &[]);
    assert_eq!(s.tick, Tick(5 * Q));
    assert_eq!(
        back(&s),
        Effect::TakeBack {
            delete: vec![],
            shorten: vec![],
            by: Tick(0),
            to: Tick(4 * Q)
        },
        "the rest"
    );
    let s = commit(&s, &back(&s), &[]);
    assert_eq!(
        back(&s),
        Effect::TakeBack {
            delete: vec![],
            shorten: ids(&[1, 2]),
            by: Tick(Q),
            to: Tick(3 * Q)
        },
        "the tie"
    );
    let s = commit(&s, &back(&s), &[]);
    assert_eq!(
        back(&s),
        Effect::TakeBack {
            delete: ids(&[1, 2]),
            shorten: vec![],
            by: Tick(0),
            to: Tick(2 * Q)
        },
        "the notes"
    );
    let s = commit(&s, &back(&s), &[]);
    assert_eq!(s.tick, start.tick);
    assert!(s.history.is_empty());
    assert_eq!(back(&s), Effect::Nothing, "nothing left to take back");
}

#[test]
fn a_keyboard_slot_enters_a_reference_and_moves_by_its_length() {
    let s = StepState::new(Tick(Q));
    let (t, e) = press(
        &s,
        StepInput::Reference {
            seq: SeqId(4),
            length: Tick(8 * Q),
        },
    );
    assert_eq!(
        e,
        Effect::InsertReference {
            tick: Tick(Q),
            seq: SeqId(4),
            length: Tick(8 * Q)
        }
    );
    let after = commit(&t, &e, &ids(&[9]));
    assert_eq!(after.tick, Tick(9 * Q));
    assert_eq!(after.history[0].kind, StepKind::Reference(EventId(9)));
    let (_, neg) = press(
        &s,
        StepInput::Reference {
            seq: SeqId(4),
            length: Tick(-5),
        },
    );
    assert!(matches!(
        neg,
        Effect::InsertReference {
            length: Tick(0),
            ..
        }
    ));
}

#[test]
fn history_keeps_the_newest_steps() {
    let mut s = StepState::new(Tick(0));
    for _ in 0..MAX_HISTORY + 10 {
        let (t, e) = press(&s, StepInput::Rest);
        s = commit(&t, &e, &[]);
    }
    assert_eq!(s.history.len(), MAX_HISTORY);
    assert_eq!(s.history[0].tick, Tick(10 * Q), "the oldest went");
}

#[test]
fn typed_pitches_are_numbers_or_names() {
    assert_eq!(parse_pitches("60 64 67"), Ok(vec![60, 64, 67]));
    assert_eq!(parse_pitches(" C4, E4,G4 "), Ok(vec![60, 64, 67]));
    assert_eq!(
        parse_pitches("c#4 Eb3 B-1 G9 C-1"),
        Ok(vec![61, 51, 11, 127, 0])
    );
    for bad in [
        "", " , ", "128", "G#9", "Cb-1", "H4", "C", "C#", "60x", "4C", "C4.5",
    ] {
        assert!(parse_pitches(bad).is_err(), "{bad:?}");
    }
    assert!(parse_pitches(&"60 ".repeat(17)).is_err(), "more than 16");
    assert_eq!(parse_pitches(&"60 ".repeat(16)).map(|v| v.len()), Ok(16));
    assert!(parse_pitches(&"6".repeat(300)).is_err(), "too long");
}

fn any_input() -> impl Strategy<Value = StepInput> {
    use NoteValue::*;
    prop_oneof![
        prop_oneof![
            Just(Whole),
            Just(Half),
            Just(Quarter),
            Just(Eighth),
            Just(Sixteenth),
            Just(ThirtySecond)
        ]
        .prop_map(StepInput::Value),
        Just(StepInput::Dot),
        Just(StepInput::Triplet),
        Just(StepInput::Tie),
        Just(StepInput::Rest),
        Just(StepInput::DeleteLast),
        (any::<u8>(), any::<u16>())
            .prop_map(|(pitch, velocity)| StepInput::NoteOn { pitch, velocity }),
        any::<u8>().prop_map(|pitch| StepInput::NoteOff { pitch }),
        prop::collection::vec((any::<u8>(), any::<u16>()), 0..40).prop_map(StepInput::Notes),
        (any::<u64>(), any::<i64>()).prop_map(|(s, l)| StepInput::Reference {
            seq: SeqId(s),
            length: Tick(l)
        }),
    ]
}

proptest! {
    /// Any input sequence, from any position, keeps the bounds; an
    /// entered step taken back returns to where it began.
    #[test]
    fn hostile_inputs_keep_the_bounds(
        tick in any::<i64>(),
        inputs in prop::collection::vec(any_input(), 0..60),
        notes in prop::collection::vec((any::<u8>(), any::<u16>()), 1..20),
    ) {
        let mut next = 0;
        let (s, effects) = run(StepState::new(Tick(tick)), inputs, &mut next);
        prop_assert!(s.history.len() <= MAX_HISTORY);
        prop_assert!(s.held.len() <= MAX_CHORD && s.chord.len() <= MAX_CHORD);
        for e in effects {
            if let Effect::Insert { notes, .. } = e {
                prop_assert!(!notes.is_empty() && notes.len() <= MAX_CHORD);
                prop_assert!(notes.windows(2).all(|w| w[0].0 < w[1].0));
            }
        }
        let before = s.clone();
        let (s2, _) = run(s, vec![StepInput::Notes(notes), StepInput::DeleteLast], &mut next);
        prop_assert_eq!(s2.tick, before.tick);
        if before.history.len() < MAX_HISTORY {
            prop_assert_eq!(s2.history, before.history);
        }
    }

    /// Typed pitches never panic; every note's name reads back as it.
    #[test]
    fn typed_pitches_never_panic(text in ".{0,40}", n in 0u8..128) {
        let _ = parse_pitches(&text);
        let names = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
        let name = format!("{}{}", names[usize::from(n % 12)], i32::from(n / 12) - 1);
        prop_assert_eq!(parse_pitches(&name), Ok(vec![n]));
    }
}
VIZDAW_EOF
```

- [ ] **Step 2: Run them to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-editor --test step 2>&1 | grep -E "^error" | head -3
```

Expected: FAIL to compile: `could not find step in core_editor`.

- [ ] **Step 3: Implement**

In `crates/core-editor/src/lib.rs`:

Find:

```rust
//! drawing — what a point is over, cells, strokes, where a dragged row
//! lands — as pure functions of plain data, so a front end only draws
//! (editors spec §3.5: hit-testing and geometry are pure functions; Plan
//! 4e Ruling 1); the transport keys' logic (plan 4f).
//! Platform-neutral; it depends only on `core-model`.
#![forbid(unsafe_code)]
#![deny(missing_docs)]
```

Replace with:

```rust
//! drawing — what a point is over, cells, strokes, where a dragged row
//! lands — as pure functions of plain data, so a front end only draws
//! (editors spec §3.5: hit-testing and geometry are pure functions; Plan
//! 4e Ruling 1); the transport keys' and step entry's logic (plan 4f).
//! Platform-neutral; it depends only on `core-model`.
#![forbid(unsafe_code)]
#![deny(missing_docs)]
```

Find:

```rust
)]

pub mod drum;
pub mod transport;
```

Replace with:

```rust
)]

pub mod drum;
pub mod step;
pub mod transport;
```

Create `crates/core-editor/src/step.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p crates/core-editor/src
cat > crates/core-editor/src/step.rs <<'VIZDAW_EOF'
//! Step entry (editors spec §4f, plan 4f Rulings 11–15): note values
//! from keys, dot and triplet, tie, rest, delete-last-step, pitches from
//! MIDI input (a chord is every key pressed before the last one is
//! released) and keyboard slots. A pure state machine: [`press`] turns an
//! input into an [`Effect`] on the document; once the document applied
//! it, [`commit`] records the step and moves on. A refused effect is
//! never committed, so the state stays where it was.

use core_model::ids::{EventId, SeqId};
use core_model::time::{PPQ, Tick};

/// The most notes one step enters.
pub const MAX_CHORD: usize = 16;
/// The most past steps Delete Last Step can take back.
pub const MAX_HISTORY: usize = 256;
/// The most characters of a typed pitch list.
pub const MAX_PITCH_TEXT: usize = 256;

/// A note value chosen by a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NoteValue {
    /// 1/1
    Whole,
    /// 1/2
    Half,
    /// 1/4
    Quarter,
    /// 1/8
    Eighth,
    /// 1/16
    Sixteenth,
    /// 1/32
    ThirtySecond,
}

impl NoteValue {
    /// Its plain length.
    pub fn ticks(self) -> Tick {
        let whole = 4 * PPQ;
        Tick(match self {
            NoteValue::Whole => whole,
            NoteValue::Half => whole / 2,
            NoteValue::Quarter => whole / 4,
            NoteValue::Eighth => whole / 8,
            NoteValue::Sixteenth => whole / 16,
            NoteValue::ThirtySecond => whole / 32,
        })
    }
}

/// A step's length: the value, times 3/2 when dotted, times 2/3 as a
/// triplet (both: the value).
pub fn step_length(value: NoteValue, dotted: bool, triplet: bool) -> Tick {
    let dot = if dotted { 3 } else { 2 };
    let three = if triplet { 2 } else { 3 };
    Tick(value.ticks().0 * dot * three / 6)
}

/// One past step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepRecord {
    /// Where it began.
    pub tick: Tick,
    /// How far it moved the position.
    pub length: Tick,
    /// What it did.
    pub kind: StepKind,
}

/// What a past step did to the document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepKind {
    /// Entered these notes.
    Notes(Vec<EventId>),
    /// Entered nothing.
    Rest,
    /// Lengthened these notes by `by`.
    Tie {
        /// The notes.
        notes: Vec<EventId>,
        /// How much.
        by: Tick,
    },
    /// Entered this reference (a keyboard slot).
    Reference(EventId),
}

/// Step entry's state; the app keeps a copy and hands it back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepState {
    /// Where the next step goes.
    pub tick: Tick,
    /// The chosen value.
    pub value: NoteValue,
    /// Dotted (sticky until toggled).
    pub dotted: bool,
    /// Triplet (sticky until toggled).
    pub triplet: bool,
    /// MIDI keys down now.
    pub held: Vec<u8>,
    /// The chord being played: every key pressed since the first went
    /// down, with its velocity.
    pub chord: Vec<(u8, u16)>,
    /// Past steps, oldest first (at most [`MAX_HISTORY`]).
    pub history: Vec<StepRecord>,
}

impl StepState {
    /// Step entry starting at `tick` with quarter notes.
    pub fn new(tick: Tick) -> StepState {
        StepState {
            tick,
            value: NoteValue::Quarter,
            dotted: false,
            triplet: false,
            held: Vec::new(),
            chord: Vec::new(),
            history: Vec::new(),
        }
    }

    /// The length a step has now.
    pub fn length(&self) -> Tick {
        step_length(self.value, self.dotted, self.triplet)
    }

    /// The state with every list within its bound (a copy the app built
    /// from stale or hostile data).
    pub fn bounded(mut self) -> StepState {
        self.held.truncate(MAX_CHORD);
        self.chord.truncate(MAX_CHORD);
        let n = self.history.len();
        if n > MAX_HISTORY {
            self.history.drain(..n - MAX_HISTORY);
        }
        self
    }
}

/// An input to step entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepInput {
    /// A note value key.
    Value(NoteValue),
    /// The dot key: toggles dotted.
    Dot,
    /// The triplet key: toggles triplet.
    Triplet,
    /// Lengthens the last step's notes by a step.
    Tie,
    /// Moves on by a step.
    Rest,
    /// Takes the last step back.
    DeleteLast,
    /// A MIDI key went down.
    NoteOn {
        /// Note number (masked to 0–127).
        pitch: u8,
        /// 16-bit velocity.
        velocity: u16,
    },
    /// A MIDI key went up.
    NoteOff {
        /// Note number.
        pitch: u8,
    },
    /// A whole chord at once (typed pitches, the control channel).
    Notes(Vec<(u8, u16)>),
    /// A keyboard slot: a reference to `seq`, `length` long.
    Reference {
        /// The referenced sequence.
        seq: SeqId,
        /// Its length.
        length: Tick,
    },
}

/// What an input asks of the document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Nothing (a setting changed, or a chord is still being played).
    Nothing,
    /// Move on without touching the document.
    Rest {
        /// How far.
        length: Tick,
    },
    /// Enter notes at `tick`, each `length` long.
    Insert {
        /// Where.
        tick: Tick,
        /// How long (also how far the position moves).
        length: Tick,
        /// Pitch and velocity of each, by pitch.
        notes: Vec<(u8, u16)>,
    },
    /// Enter a reference to `seq` at `tick`.
    InsertReference {
        /// Where.
        tick: Tick,
        /// The sequence.
        seq: SeqId,
        /// How far the position moves.
        length: Tick,
    },
    /// Lengthen `notes` by `by`.
    Extend {
        /// The notes.
        notes: Vec<EventId>,
        /// How much (also how far the position moves).
        by: Tick,
    },
    /// Take the last step back: delete `delete`, shorten `shorten` by `by`,
    /// and go back to `to`.
    TakeBack {
        /// Events to delete (a step's notes or reference).
        delete: Vec<EventId>,
        /// Notes a tie lengthened.
        shorten: Vec<EventId>,
        /// How much they were lengthened.
        by: Tick,
        /// Where the step began.
        to: Tick,
    },
}

fn sorted_chord(mut notes: Vec<(u8, u16)>) -> Vec<(u8, u16)> {
    let mut out: Vec<(u8, u16)> = Vec::new();
    for (p, v) in notes.drain(..) {
        let p = p & 0x7F;
        if !out.iter().any(|(q, _)| *q == p) && out.len() < MAX_CHORD {
            out.push((p, v));
        }
    }
    out.sort_by_key(|(p, _)| *p);
    out
}

/// The notes the last step left sounding (its own or the ones it tied).
fn last_notes(state: &StepState) -> Option<Vec<EventId>> {
    match &state.history.last()?.kind {
        StepKind::Notes(ids) if !ids.is_empty() => Some(ids.clone()),
        StepKind::Tie { notes, .. } if !notes.is_empty() => Some(notes.clone()),
        _ => None,
    }
}

/// What `input` does in `state`: the state with any setting changed (not
/// yet moved on) and the effect to apply before [`commit`].
pub fn press(state: &StepState, input: StepInput) -> (StepState, Effect) {
    let mut s = state.clone().bounded();
    let length = s.length();
    let effect = match input {
        StepInput::Value(v) => {
            s.value = v;
            Effect::Nothing
        }
        StepInput::Dot => {
            s.dotted = !s.dotted;
            Effect::Nothing
        }
        StepInput::Triplet => {
            s.triplet = !s.triplet;
            Effect::Nothing
        }
        StepInput::Rest => Effect::Rest { length },
        StepInput::Tie => match last_notes(&s) {
            Some(notes) => Effect::Extend { notes, by: length },
            None => Effect::Rest { length },
        },
        StepInput::DeleteLast => match s.history.last() {
            None => Effect::Nothing,
            Some(r) => {
                let (delete, shorten, by) = match &r.kind {
                    StepKind::Notes(ids) => (ids.clone(), Vec::new(), Tick::ZERO),
                    StepKind::Rest => (Vec::new(), Vec::new(), Tick::ZERO),
                    StepKind::Tie { notes, by } => (Vec::new(), notes.clone(), *by),
                    StepKind::Reference(id) => (vec![*id], Vec::new(), Tick::ZERO),
                };
                Effect::TakeBack {
                    delete,
                    shorten,
                    by,
                    to: r.tick,
                }
            }
        },
        StepInput::NoteOn { pitch, velocity } => {
            let p = pitch & 0x7F;
            if !s.held.contains(&p) && s.held.len() < MAX_CHORD {
                s.held.push(p);
            }
            if !s.chord.iter().any(|(q, _)| *q == p) && s.chord.len() < MAX_CHORD {
                s.chord.push((p, velocity));
            }
            Effect::Nothing
        }
        StepInput::NoteOff { pitch } => {
            let p = pitch & 0x7F;
            let was_held = s.held.contains(&p);
            s.held.retain(|q| *q != p);
            if was_held && s.held.is_empty() && !s.chord.is_empty() {
                let notes = sorted_chord(std::mem::take(&mut s.chord));
                Effect::Insert {
                    tick: s.tick,
                    length,
                    notes,
                }
            } else {
                Effect::Nothing
            }
        }
        StepInput::Notes(notes) => {
            let notes = sorted_chord(notes);
            if notes.is_empty() {
                Effect::Nothing
            } else {
                Effect::Insert {
                    tick: s.tick,
                    length,
                    notes,
                }
            }
        }
        StepInput::Reference { seq, length } => Effect::InsertReference {
            tick: s.tick,
            seq,
            length: length.max(Tick::ZERO),
        },
    };
    (s, effect)
}

/// After the document applied `effect` (`created`: the events it made, in
/// order): records the step and moves the position on (or back).
pub fn commit(state: &StepState, effect: &Effect, created: &[EventId]) -> StepState {
    let mut s = state.clone().bounded();
    let record = |tick: Tick, length: Tick, kind: StepKind| StepRecord { tick, length, kind };
    match effect {
        Effect::Nothing => return s,
        Effect::Rest { length } => {
            let r = record(s.tick, *length, StepKind::Rest);
            s.history.push(r);
            s.tick = s.tick + *length;
        }
        Effect::Insert { tick, length, .. } => {
            s.history
                .push(record(*tick, *length, StepKind::Notes(created.to_vec())));
            s.tick = *tick + *length;
        }
        Effect::InsertReference { tick, length, .. } => {
            let kind = match created.first() {
                Some(id) => StepKind::Reference(*id),
                None => StepKind::Rest,
            };
            s.history.push(record(*tick, *length, kind));
            s.tick = *tick + *length;
        }
        Effect::Extend { notes, by } => {
            let r = record(
                s.tick,
                *by,
                StepKind::Tie {
                    notes: notes.clone(),
                    by: *by,
                },
            );
            s.history.push(r);
            s.tick = s.tick + *by;
        }
        Effect::TakeBack { to, .. } => {
            s.history.pop();
            s.tick = *to;
        }
    }
    s.bounded()
}

/// Reads typed pitches (plan 4f Ruling 14): note numbers (`60`) or names
/// (`C4`, `c#4`, `Eb3`, `B-1`; middle C is C4), separated by spaces or
/// commas; at most [`MAX_CHORD`] of them.
pub fn parse_pitches(text: &str) -> Result<Vec<u8>, String> {
    if text.chars().count() > MAX_PITCH_TEXT {
        return Err(format!("at most {MAX_PITCH_TEXT} characters of pitches"));
    }
    let mut out = Vec::new();
    for token in text.split([' ', ',', '\t', '\n']).filter(|t| !t.is_empty()) {
        if out.len() == MAX_CHORD {
            return Err(format!("at most {MAX_CHORD} pitches"));
        }
        out.push(parse_pitch(token)?);
    }
    if out.is_empty() {
        return Err("no pitches".into());
    }
    Ok(out)
}

fn parse_pitch(token: &str) -> Result<u8, String> {
    let shown = || token.chars().take(16).collect::<String>();
    if let Ok(n) = token.parse::<u8>() {
        return if n <= 127 {
            Ok(n)
        } else {
            Err(format!("{:?} is above 127", shown()))
        };
    }
    let mut chars = token.chars();
    let class: i32 = match chars.next().map(|c| c.to_ascii_uppercase()) {
        Some('C') => 0,
        Some('D') => 2,
        Some('E') => 4,
        Some('F') => 5,
        Some('G') => 7,
        Some('A') => 9,
        Some('B') => 11,
        _ => return Err(format!("{:?} is not a pitch", shown())),
    };
    let rest = chars.as_str();
    let (shift, octave) = match rest.chars().next() {
        Some('#') => (1, rest.get(1..).unwrap_or("")),
        Some('b') => (-1, rest.get(1..).unwrap_or("")),
        _ => (0, rest),
    };
    let octave: i32 = octave
        .parse()
        .map_err(|_| format!("{:?} is not a pitch", shown()))?;
    let n = (octave + 1)
        .checked_mul(12)
        .and_then(|o| o.checked_add(class + shift))
        .filter(|n| (0..=127).contains(n))
        .ok_or_else(|| format!("{:?} is out of range", shown()))?;
    u8::try_from(n).map_err(|_| format!("{:?} is out of range", shown()))
}
VIZDAW_EOF
```

- [ ] **Step 4: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-editor 2>&1 | grep -E "^test result|FAILED|panicked"
```

Expected: `step` 9 passed (the two property tests included); `drum` and `transport` as before.

- [ ] **Step 5: Gate and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace 2>&1 | grep -E "FAILED|panicked" ; cargo test --workspace -- --list 2>/dev/null | grep -c ': test$'
scripts/build-ffi.sh && scripts/swift.sh build 2>&1 | tail -1 && scripts/swift.sh test 2>&1 | tail -1
git add crates/core-editor/src/lib.rs \
  crates/core-editor/src/step.rs \
  crates/core-editor/tests/step.rs
git commit -m "feat(editor): the step entry state machine

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
git log -1 --format='%h %s'
```

Expected: 891 Rust tests listed (882 + 9); Swift 613. No `FAILED` or `panicked` line (rerun once if the known-flaky MIDI listener test trips, and say so).

---

### Task 5: FFI — the keymap object, step entry through the session, note-offs from MIDI input

**Files:**
- Create: `crates/core-ffi/src/keys.rs`, `crates/core-ffi/src/step.rs`
- Modify: `app/Sources/VizDAWAppModel/ListEditorModel.swift`, `crates/core-ffi/src/input.rs`, `crates/core-ffi/src/lib.rs`, `crates/core-ffi/src/session.rs`
- Test: `app/Tests/VizDAWAppModelTests/FakeSession.swift`, `app/Tests/VizDAWAppModelTests/ListEditorModelTests.swift`, `crates/core-ffi/tests/input.rs`, `crates/core-ffi/tests/keys.rs`, `crates/core-ffi/tests/step.rs`

**Interfaces:**
- Consumes: Task 2's `bindings::{Keymap, Chord, default_chords, MAX_JSON}`, `registry::{COMMANDS, KeyContext}`; Task 4's `step::{press, commit, StepState, StepInput, Effect, parse_pitches}`; `History::{begin, execute, commit, abort, set_selection}`, `Report.created`, `EventCmd::{Insert, Delete, Resize}`, `query::seq_length`.
- Produces: `vizdaw_ffi::{Keymap (object): new(), rows() -> Vec<BindingRow>, lookup(key_code, modifiers: ModifiersRow, step_entry, reserve_numbers) -> Option<String>, slot_clash(key, modifiers: u8) -> Option<String>, bind(id, keys: Vec<String>, replace) -> Result<()>, reset_one(id), reset(), to_json() -> String, load_json(json) -> Result<Vec<String>>, save_file(path) -> Result<()>, load_file(path) -> Result<Vec<String>>; BindingRow {id, menu, title, menu_item, step_entry, keys, defaults, menu_key: Option<String>, menu_modifiers: ModifiersRow}; NoteValueRow; StepNoteRow {pitch, velocity}; StepKindRow {Notes{events}, Rest, Tie{events, by}, Reference{event}}; StepRecordRow {tick, length, kind}; StepStateRow {tick, value, dotted, triplet, held, chord, history}; StepInputRow {Value{value}, Dot, Triplet, Tie, Rest, DeleteLast, NoteOn{note}, NoteOff{pitch}, Notes{notes}, Reference{seq}}; StepResultRow {state, snapshot: Option<DocSnapshot>, moved}; step_state_new(tick), step_length(value, dotted, triplet) -> i64, step_parse_pitches(text) -> Result<Vec<u8>>; Session.step_entry(seq, lane, state, input) -> Result<StepResultRow>}`; `NoteInRow.on: bool` (default true); `input::note_event(&[u8]) -> Option<NoteInRow>` (replaces `note_on`). Swift: `FakeSession.{stepCalls, stepEntryError, stepAnswer, stepEntry}`; `ListEditorModel.receive` takes the last note-on.

The session method is `step_entry` (`Session` already has a private `step` for undo and redo). It applies an effect as one transaction (labelled "Step Entry", or "Delete Last Step" for a take-back), keeps the IDs the core created, makes them the selection, and returns the committed state; a refused command aborts the transaction and is the error, and the state is not committed. A tie or a take-back acts only on its events still in the step's lane: the state is the app's copy and may be old. MIDI input now queues note-offs (`on: false`) for chords (Ruling 13); the list editor ignores them.

- [ ] **Step 1: Write the failing tests**

In `app/Tests/VizDAWAppModelTests/ListEditorModelTests.swift`:

Find:

```swift
            return
        }
        #expect(row.pitch == 62 && row.velocity == 0xC924)
    }

    @Test func openingAReferenceOpensItsSequence() throws {
```

Replace with:

```swift
            return
        }
        #expect(row.pitch == 62 && row.velocity == 0xC924)
        // A note-off (plan 4f) is not a note to enter.
        let n = f.edits.count
        l.receive([NoteInRow(pitch: 64, velocity: 0, channel: 0, on: false)])
        #expect(f.edits.count == n)
        l.receive([d4Loud, NoteInRow(pitch: 62, velocity: 0, channel: 0, on: false)])
        guard case .event(.replace(1, let again))? = f.edits.last else {
            Issue.record("no replace")
            return
        }
        #expect(again.velocity == 0xC924, "the note-on counts, not the off after it")
    }

    @Test func openingAReferenceOpensItsSequence() throws {
```

In `crates/core-ffi/tests/input.rs`:

Find:

```rust
//! MIDI note input (plan 4c, Plan Ruling 11): note-ons queue with MIDI 2.0
//! velocities; note-offs and other messages do not.

use vizdaw_ffi::input::{CAPACITY, upscale_velocity};
use vizdaw_ffi::{NoteInRow, NoteInput};

#[test]
fn note_ons_queue_in_order_and_nothing_else_does() {
    let n = NoteInput::new();
    n.inject(vec![0x90, 60, 127]).unwrap();
    n.inject(vec![0x80, 60, 0]).unwrap();
    n.inject(vec![0x91, 62, 0]).unwrap();
    n.inject(vec![0xB0, 7, 100]).unwrap();
    n.inject(vec![0x9F, 64, 1]).unwrap();
    assert_eq!(
        n.take().unwrap(),
        vec![
            NoteInRow {
                pitch: 60,
                velocity: 0xFFFF,
                channel: 0
            },
            NoteInRow {
                pitch: 64,
                velocity: 1 << 9,
                channel: 15
            },
        ]
    );
    assert!(n.take().unwrap().is_empty());
```

Replace with:

```rust
//! MIDI note input (plan 4c, Plan Ruling 11): note-ons queue with MIDI 2.0
//! velocities, note-offs (plan 4f) without; other messages do not.

use vizdaw_ffi::input::{CAPACITY, upscale_velocity};
use vizdaw_ffi::{NoteInRow, NoteInput};

#[test]
fn notes_queue_in_order_and_nothing_else_does() {
    let n = NoteInput::new();
    n.inject(vec![0x90, 60, 127]).unwrap();
    n.inject(vec![0x80, 60, 40]).unwrap();
    n.inject(vec![0x91, 62, 0]).unwrap();
    n.inject(vec![0xB0, 7, 100]).unwrap();
    n.inject(vec![0x9F, 64, 1]).unwrap();
    n.inject(vec![0x90, 60]).unwrap();
    let note = |pitch, velocity, channel, on| NoteInRow {
        pitch,
        velocity,
        channel,
        on,
    };
    assert_eq!(
        n.take().unwrap(),
        vec![
            note(60, 0xFFFF, 0, true),
            note(60, 0, 0, false),
            note(62, 0, 1, false),
            note(64, 1 << 9, 15, true),
        ]
    );
    assert!(n.take().unwrap().is_empty());
```

In `crates/core-ffi/tests/keys.rs`:

Find:

```rust
    s.move_record_arm(song, true).unwrap();
    assert_eq!(&lanes(&s)[..3], &[false, false, true]);
}
```

Replace with:

```rust
    s.move_record_arm(song, true).unwrap();
    assert_eq!(&lanes(&s)[..3], &[false, false, true]);
}

fn cmd() -> vizdaw_ffi::ModifiersRow {
    vizdaw_ffi::ModifiersRow {
        command: true,
        ..Default::default()
    }
}

#[test]
fn the_keymap_lists_every_command_with_its_keys() {
    let k = vizdaw_ffi::Keymap::new();
    let rows = k.rows();
    assert_eq!(rows.len(), core_control::registry::COMMANDS.len());
    let save = rows.iter().find(|r| r.id == "file.save").unwrap();
    assert_eq!(save.keys, vec!["⌘S"]);
    assert_eq!(save.defaults, save.keys);
    assert_eq!(save.menu_key.as_deref(), Some("s"));
    assert_eq!(save.menu_modifiers, cmd());
    assert!(save.menu_item && !save.step_entry);
    let close = rows
        .iter()
        .find(|r| r.id == "editors.close_detail")
        .unwrap();
    assert!(close.keys.is_empty() && close.menu_key.is_none());
    assert_eq!(
        k.lookup(49, Default::default(), false, false).as_deref(),
        Some("transport.play_stop"),
        "Space"
    );
    assert_eq!(k.lookup(1, cmd(), true, true).as_deref(), Some("file.save"));
    assert_eq!(k.lookup(0xFFFF, cmd(), false, false), None, "no such key");
    assert_eq!(k.slot_clash("s".into(), 8).as_deref(), Some("file.save"));
    assert_eq!(k.slot_clash("q".into(), 2), None);
    assert_eq!(
        k.slot_clash("nope".into(), 0),
        None,
        "not a key: never pressed"
    );
}

#[test]
fn bindings_change_save_load_and_reset() {
    let k = vizdaw_ffi::Keymap::new();
    k.bind("file.save".into(), vec!["⌘K".into()], false)
        .unwrap();
    let e = k
        .bind("edit.undo".into(), vec!["⌘K".into()], false)
        .unwrap_err();
    assert_eq!(
        e,
        VizError::Model {
            message: "⌘K is the key of “Save”".into()
        }
    );
    assert!(matches!(
        k.bind("file.save".into(), vec!["⌘⌘".into()], false),
        Err(VizError::Model { .. })
    ));
    assert_eq!(
        k.lookup(40, cmd(), false, false).as_deref(),
        Some("file.save")
    );
    let dir = tempfile::tempdir().unwrap();
    let path = dir
        .path()
        .join("mine.vizkeys")
        .to_string_lossy()
        .into_owned();
    k.save_file(path.clone()).unwrap();
    k.reset();
    assert_eq!(k.lookup(40, cmd(), false, false), None);
    assert!(k.load_file(path).unwrap().is_empty());
    assert_eq!(
        k.lookup(40, cmd(), false, false).as_deref(),
        Some("file.save")
    );
    k.reset_one("file.save".into());
    assert_eq!(
        k.lookup(1, cmd(), false, false).as_deref(),
        Some("file.save")
    );
    // Damaged and hostile files: refused whole, the bindings unchanged.
    let before = k.to_json();
    let big = dir.path().join("big.vizkeys");
    std::fs::write(&big, vec![b' '; 300 * 1024]).unwrap();
    assert!(matches!(
        k.load_file(big.to_string_lossy().into_owned()),
        Err(VizError::Model { .. })
    ));
    assert!(matches!(
        k.load_file(dir.path().join("none").to_string_lossy().into_owned()),
        Err(VizError::Io { .. })
    ));
    assert!(matches!(
        k.load_json("garbage".into()),
        Err(VizError::Model { .. })
    ));
    assert!(matches!(
        k.save_file("/nonexistent/dir/k.vizkeys".into()),
        Err(VizError::Io { .. })
    ));
    assert_eq!(k.to_json(), before);
    let skipped = k
        .load_json(r#"{"vizdaw_keys": 1, "bindings": {"no.such": ["F1"]}}"#.into())
        .unwrap();
    assert_eq!(skipped, vec!["no command \"no.such\"".to_string()]);
}
```

Create `crates/core-ffi/tests/step.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p crates/core-ffi/tests
cat > crates/core-ffi/tests/step.rs <<'VIZDAW_EOF'
//! Step entry through the session (plan 4f Task 5): chords, rests, ties,
//! taking steps back, keyboard slots, refusals and hostile states.

use proptest::prelude::*;
use vizdaw_ffi::{
    DocSnapshot, EventKindRow, NoteValueRow, Session, StepInputRow, StepKindRow, StepNoteRow,
    StepRecordRow, StepStateRow, VizError, step_length, step_parse_pitches, step_state_new,
};

const Q: i64 = 15_360;
const BAR: i64 = 4 * Q;

fn id_of(s: &DocSnapshot, name: &str) -> u64 {
    s.sequences.iter().find(|r| r.name == name).unwrap().id
}

/// The demo's Song and its Chords lane, and an empty lane added after.
fn song() -> (std::sync::Arc<Session>, u64, u64) {
    let s = Session::new_demo().unwrap();
    let song = id_of(&s.snapshot().unwrap(), "Song");
    s.edit(
        vizdaw_ffi::EditCommand::Lane {
            edit: vizdaw_ffi::LaneEdit::Add {
                seq: song,
                name: "Steps".into(),
                index: None,
            },
        },
        None,
    )
    .unwrap();
    let lane = s.lanes(song).unwrap().last().unwrap().id;
    (s, song, lane)
}

fn note(pitch: u8) -> StepNoteRow {
    StepNoteRow {
        pitch,
        velocity: u16::from(pitch) << 9,
    }
}

/// Notes in the lane: (tick, pitch, duration).
fn notes(s: &Session, seq: u64, lane: u64) -> Vec<(i64, u8, i64)> {
    s.lane_events(seq, lane)
        .unwrap()
        .events
        .iter()
        .filter(|e| e.kind == EventKindRow::Note)
        .map(|e| (e.tick, e.pitch, e.duration))
        .collect()
}

fn step(s: &Session, seq: u64, lane: u64, st: &mut StepStateRow, input: StepInputRow) -> bool {
    let r = s.step_entry(seq, lane, st.clone(), input).unwrap();
    *st = r.state;
    r.snapshot.is_some()
}

#[test]
fn a_chord_is_one_undo_step_and_becomes_the_selection() {
    let (s, song, lane) = song();
    let mut st = step_state_new(2 * BAR);
    assert_eq!(st.value, NoteValueRow::Quarter);
    for input in [
        StepInputRow::NoteOn { note: note(64) },
        StepInputRow::NoteOn { note: note(60) },
        StepInputRow::NoteOff { pitch: 60 },
    ] {
        assert!(
            !step(&s, song, lane, &mut st, input),
            "nothing until all keys are up"
        );
    }
    assert!(step(
        &s,
        song,
        lane,
        &mut st,
        StepInputRow::NoteOff { pitch: 64 }
    ));
    assert_eq!(
        notes(&s, song, lane),
        vec![(2 * BAR, 60, Q), (2 * BAR, 64, Q)]
    );
    assert_eq!(st.tick, 2 * BAR + Q);
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Step Entry")
    );
    let StepKindRow::Notes { events } = &st.history[0].kind else {
        panic!("{:?}", st.history)
    };
    let mut sel = s.selection().unwrap().events;
    sel.sort_unstable();
    let mut mine = events.clone();
    mine.sort_unstable();
    assert_eq!(sel, mine, "the entered notes are selected");
    s.undo().unwrap();
    assert!(notes(&s, song, lane).is_empty(), "one undo takes the chord");
}

#[test]
fn rests_ties_and_taking_steps_back() {
    let (s, song, lane) = song();
    let mut st = step_state_new(0);
    let eighth = StepInputRow::Value {
        value: NoteValueRow::Eighth,
    };
    assert!(!step(&s, song, lane, &mut st, eighth));
    assert!(
        !step(&s, song, lane, &mut st, StepInputRow::Rest),
        "a rest touches nothing"
    );
    assert_eq!(st.tick, Q / 2);
    let chord = StepInputRow::Notes {
        notes: vec![note(67)],
    };
    assert!(step(&s, song, lane, &mut st, chord));
    assert!(step(&s, song, lane, &mut st, StepInputRow::Tie));
    assert_eq!(
        notes(&s, song, lane),
        vec![(Q / 2, 67, Q)],
        "tied: two eighths"
    );
    assert_eq!(st.tick, Q / 2 + Q);
    assert!(step(&s, song, lane, &mut st, StepInputRow::DeleteLast));
    assert_eq!(
        notes(&s, song, lane),
        vec![(Q / 2, 67, Q / 2)],
        "the tie undone"
    );
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Delete Last Step")
    );
    assert!(step(&s, song, lane, &mut st, StepInputRow::DeleteLast));
    assert!(notes(&s, song, lane).is_empty());
    assert_eq!(st.tick, Q / 2);
    let r = s
        .step_entry(song, lane, st.clone(), StepInputRow::DeleteLast)
        .unwrap();
    assert!(
        r.moved && r.snapshot.is_none(),
        "the rest goes back without an edit"
    );
    assert_eq!(r.state.tick, 0);
    let r = s
        .step_entry(song, lane, r.state, StepInputRow::DeleteLast)
        .unwrap();
    assert!(!r.moved, "nothing left");
}

#[test]
fn a_step_whose_notes_were_deleted_still_goes_back() {
    let (s, song, lane) = song();
    let mut st = step_state_new(0);
    step(
        &s,
        song,
        lane,
        &mut st,
        StepInputRow::Notes {
            notes: vec![note(60)],
        },
    );
    let StepKindRow::Notes { events } = st.history[0].kind.clone() else {
        panic!()
    };
    s.edit(
        vizdaw_ffi::EditCommand::Event {
            edit: vizdaw_ffi::EventEdit::Delete { events },
        },
        None,
    )
    .unwrap();
    let r = s
        .step_entry(song, lane, st.clone(), StepInputRow::DeleteLast)
        .unwrap();
    assert!(r.moved && r.snapshot.is_none() && r.state.tick == 0);
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Delete Events")
    );
}

#[test]
fn a_keyboard_slot_enters_a_reference_and_moves_by_its_length() {
    let (s, song, lane) = song();
    let beat = id_of(&s.snapshot().unwrap(), "Beat");
    let beat_len = s
        .snapshot()
        .unwrap()
        .sequences
        .iter()
        .find(|r| r.id == beat)
        .unwrap()
        .length_ticks;
    let mut st = step_state_new(BAR);
    assert!(step(
        &s,
        song,
        lane,
        &mut st,
        StepInputRow::Reference { seq: beat }
    ));
    let ev = s.lane_events(song, lane).unwrap().events;
    assert_eq!(ev.len(), 1);
    assert_eq!(
        (ev[0].kind, ev[0].tick, ev[0].ref_seq),
        (EventKindRow::Reference, BAR, Some(beat))
    );
    assert_eq!(st.tick, BAR + beat_len);
    // A reference to the sequence itself is a cycle: refused, nothing moves.
    let before = st.clone();
    let e = s
        .step_entry(
            song,
            lane,
            st.clone(),
            StepInputRow::Reference { seq: song },
        )
        .unwrap_err();
    assert!(matches!(e, VizError::Model { .. }), "{e:?}");
    assert_eq!(st, before);
    assert_eq!(s.lane_events(song, lane).unwrap().events.len(), 1);
}

#[test]
fn refusals_leave_everything_as_it_was() {
    let (s, song, lane) = song();
    let st = step_state_new(0);
    let input = StepInputRow::Notes {
        notes: vec![note(60)],
    };
    for (seq, l) in [(9_999, lane), (song, 9_999)] {
        assert!(matches!(
            s.step_entry(seq, l, st.clone(), input.clone()),
            Err(VizError::Model { .. })
        ));
    }
    s.begin_edit("Drag".into()).unwrap();
    assert!(matches!(
        s.step_entry(song, lane, st.clone(), input.clone()),
        Err(VizError::Model { .. })
    ));
    s.abort_edit().unwrap();
    // A step past the model's time limit is refused by the core.
    let far = step_state_new(1 << 41);
    assert!(s.step_entry(song, lane, far, input).is_err());
    assert!(notes(&s, song, lane).is_empty());
}

#[test]
fn lengths_pitches_and_a_fresh_state() {
    assert_eq!(step_length(NoteValueRow::Eighth, true, false), Q * 3 / 4);
    assert_eq!(step_length(NoteValueRow::Whole, false, true), 4 * Q * 2 / 3);
    assert_eq!(
        step_parse_pitches("C4 e4 67".into()).unwrap(),
        vec![60, 64, 67]
    );
    assert!(matches!(
        step_parse_pitches("X9".into()),
        Err(VizError::Model { .. })
    ));
    let st = step_state_new(-BAR);
    assert_eq!(st.tick, -BAR);
    assert!(st.history.is_empty() && !st.dotted && !st.triplet);
}

fn any_kind() -> impl Strategy<Value = StepKindRow> {
    prop_oneof![
        prop::collection::vec(0u64..400, 0..4).prop_map(|events| StepKindRow::Notes { events }),
        Just(StepKindRow::Rest),
        (prop::collection::vec(0u64..400, 0..4), any::<i64>())
            .prop_map(|(events, by)| StepKindRow::Tie { events, by }),
        (0u64..400).prop_map(|event| StepKindRow::Reference { event }),
    ]
}

fn any_state() -> impl Strategy<Value = StepStateRow> {
    (
        -(1i64 << 42)..(1i64 << 42),
        prop::collection::vec(any::<u8>(), 0..30),
        prop::collection::vec((any::<i64>(), any::<i64>(), any_kind()), 0..300),
        any::<bool>(),
    )
        .prop_map(|(tick, held, history, dotted)| StepStateRow {
            tick,
            value: NoteValueRow::Sixteenth,
            dotted,
            triplet: !dotted,
            chord: held.iter().map(|p| note(*p & 0x7F)).collect(),
            held,
            history: history
                .into_iter()
                .map(|(tick, length, kind)| StepRecordRow { tick, length, kind })
                .collect(),
        })
}

fn any_input() -> impl Strategy<Value = StepInputRow> {
    prop_oneof![
        Just(StepInputRow::Tie),
        Just(StepInputRow::Rest),
        Just(StepInputRow::DeleteLast),
        Just(StepInputRow::Dot),
        any::<u8>().prop_map(|pitch| StepInputRow::NoteOff { pitch }),
        prop::collection::vec(any::<u8>(), 0..20).prop_map(|p| StepInputRow::Notes {
            notes: p.into_iter().map(note).collect()
        }),
        (0u64..8).prop_map(|seq| StepInputRow::Reference { seq }),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// A stale or hostile state with any input is applied or refused
    /// with a `Model` error; never a panic or `Internal`, and the state
    /// that comes back is bounded.
    #[test]
    fn hostile_states_apply_or_are_refused(st in any_state(), input in any_input()) {
        let (s, song, lane) = song();
        match s.step_entry(song, lane, st, input) {
            Ok(r) => {
                prop_assert!(r.state.history.len() <= 256);
                prop_assert!(r.state.held.len() <= 16 && r.state.chord.len() <= 16);
            }
            Err(e) => prop_assert!(matches!(e, VizError::Model { .. }), "{:?}", e),
        }
        prop_assert!(s.snapshot().is_ok());
    }
}

#[test]
fn a_stale_state_never_touches_another_lanes_notes() {
    let (s, song, lane) = song();
    let chords = s.lanes(song).unwrap()[1].id;
    let before = s.lane_events(song, chords).unwrap().events;
    let theirs: Vec<u64> = before.iter().map(|e| e.id).collect();
    let mut st = step_state_new(0);
    st.history = vec![
        StepRecordRow {
            tick: 0,
            length: Q,
            kind: StepKindRow::Notes {
                events: theirs.clone(),
            },
        },
        StepRecordRow {
            tick: Q,
            length: Q,
            kind: StepKindRow::Tie {
                events: theirs,
                by: Q,
            },
        },
    ];
    let r = s.step_entry(song, lane, st, StepInputRow::Tie).unwrap();
    assert!(r.snapshot.is_none(), "nothing of this lane to tie");
    let r = s
        .step_entry(song, lane, r.state, StepInputRow::DeleteLast)
        .unwrap();
    let r = s
        .step_entry(song, lane, r.state, StepInputRow::DeleteLast)
        .unwrap();
    s.step_entry(song, lane, r.state, StepInputRow::DeleteLast)
        .unwrap();
    assert_eq!(s.lane_events(song, chords).unwrap().events, before);
}
VIZDAW_EOF
```

- [ ] **Step 2: Run them to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-ffi --test step --test keys --test input 2>&1 | grep -E "^error" | head -3
```

Expected: FAIL to compile: unresolved imports `vizdaw_ffi::step_state_new`, `vizdaw_ffi::Keymap`; no field `on` in `NoteInRow`.

- [ ] **Step 3: Implement**

In `app/Sources/VizDAWAppModel/ListEditorModel.swift`:

Find:

```swift
        return true
    }

    /// Notes played on a MIDI source (Plan 4c Ruling 12).
    public func receive(_ notes: [NoteInRow]) {
        guard listens, let n = notes.last else { return }
        if let cell = editing {
            guard let r = row(cell.row) else { return }
            if cell.column == .data1, r.kind == .note || r.kind == .polyPressure {
```

Replace with:

```swift
        return true
    }

    /// Notes played on a MIDI source (Plan 4c Ruling 12); the last
    /// note-on counts (note-offs are step entry's, plan 4f).
    public func receive(_ notes: [NoteInRow]) {
        guard listens, let n = notes.last(where: \.on) else { return }
        if let cell = editing {
            guard let r = row(cell.row) else { return }
            if cell.column == .data1, r.kind == .note || r.kind == .polyPressure {
```

In `crates/core-ffi/src/input.rs`:

Find:

```rust
//! MIDI note input (plan 4c, Plan Ruling 11): notes played on any MIDI
//! source, for the list editor's "play a note to enter it". Recording
//! proper is sub-project 5.

use crate::error::VizError;
```

Replace with:

```rust
//! MIDI note input (plan 4c, Plan Ruling 11): notes played on any MIDI
//! source, for the list editor's "play a note to enter it" and step entry
//! (plan 4f, which also needs the note-offs to tell a chord). Recording
//! proper is sub-project 5.

use crate::error::VizError;
```

Find:

```rust
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

/// A note-on played on a MIDI source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Record)]
pub struct NoteInRow {
    /// Note number 0–127.
    pub pitch: u8,
    /// 16-bit velocity (MIDI 2.0 upscale of the 7-bit one).
    pub velocity: u16,
    /// MIDI channel 0–15.
    pub channel: u8,
}

/// How many note-ons are kept until taken (the newest win).
pub const CAPACITY: usize = 256;

type Queue = Arc<Mutex<VecDeque<NoteInRow>>>;
```

Replace with:

```rust
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

/// A note-on or note-off played on a MIDI source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Record)]
pub struct NoteInRow {
    /// Note number 0–127.
    pub pitch: u8,
    /// 16-bit velocity (MIDI 2.0 upscale of the 7-bit one); 0 for an off.
    pub velocity: u16,
    /// MIDI channel 0–15.
    pub channel: u8,
    /// A note-on (false: a note-off, plan 4f).
    #[uniffi(default = true)]
    pub on: bool,
}

/// How many notes are kept until taken (the newest win).
pub const CAPACITY: usize = 256;

type Queue = Arc<Mutex<VecDeque<NoteInRow>>>;
```

Find:

```rust
    shifted | (rep << 3) | (rep >> 3)
}

/// The note-on in a MIDI 1.0 message (velocity 0 is a note-off).
pub fn note_on(bytes: &[u8]) -> Option<NoteInRow> {
    match bytes {
        [s, p, v] if s & 0xF0 == 0x90 && *v > 0 => Some(NoteInRow {
            pitch: p & 0x7F,
            velocity: upscale_velocity(*v),
            channel: s & 0x0F,
        }),
        _ => None,
    }
}

fn push(q: &Queue, bytes: &[u8]) {
    if let Some(n) = note_on(bytes) {
        let mut q = lock(q);
        if q.len() >= CAPACITY {
            q.pop_front();
```

Replace with:

```rust
    shifted | (rep << 3) | (rep >> 3)
}

/// The note-on or note-off in a MIDI 1.0 message (a note-on with velocity
/// 0 is a note-off).
pub fn note_event(bytes: &[u8]) -> Option<NoteInRow> {
    match bytes {
        [s, p, v] if s & 0xF0 == 0x90 && *v > 0 => Some(NoteInRow {
            pitch: p & 0x7F,
            velocity: upscale_velocity(*v),
            channel: s & 0x0F,
            on: true,
        }),
        [s, p, _] if s & 0xF0 == 0x80 || s & 0xF0 == 0x90 => Some(NoteInRow {
            pitch: p & 0x7F,
            velocity: 0,
            channel: s & 0x0F,
            on: false,
        }),
        _ => None,
    }
}

fn push(q: &Queue, bytes: &[u8]) {
    if let Some(n) = note_event(bytes) {
        let mut q = lock(q);
        if q.len() >= CAPACITY {
            q.pop_front();
```

Find:

```rust
        })
    }

    /// The note-ons since the last call, oldest first.
    pub fn take(&self) -> Result<Vec<NoteInRow>, VizError> {
        ffi_guard(|| Ok(lock(&self.queue).drain(..).collect()))
    }
```

Replace with:

```rust
        })
    }

    /// The notes played since the last call, oldest first.
    pub fn take(&self) -> Result<Vec<NoteInRow>, VizError> {
        ffi_guard(|| Ok(lock(&self.queue).drain(..).collect()))
    }
```

Create `crates/core-ffi/src/keys.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p crates/core-ffi/src
cat > crates/core-ffi/src/keys.rs <<'VIZDAW_EOF'
//! Key bindings for the app (plan 4f Rulings 3–6): the registry's
//! commands as rows for menus and the Keys settings, and a [`Keymap`] the
//! app keeps for its whole run (the bindings, saved, loaded and reset;
//! what a key press runs; whether a keyboard slot clashes).

use crate::control::ModifiersRow;
use crate::error::VizError;
use crate::guard::{ffi_guard, lock};
use core_control::bindings::{
    self, COMMAND, CONTROL, Chord, MAX_JSON, OPTION, SHIFT, default_chords,
};
use core_control::registry::{COMMANDS, KeyContext};
use std::sync::{Arc, Mutex};

/// One command with its keys, for menus and the Keys settings.
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Record)]
pub struct BindingRow {
    /// Stable ID.
    pub id: String,
    /// Its menu (or, without a menu item, its group).
    pub menu: String,
    /// Its title.
    pub title: String,
    /// It has a menu item.
    pub menu_item: bool,
    /// Its keys are live only during step entry.
    pub step_entry: bool,
    /// Its keys now, as menus show them (`⇧⌘S`).
    pub keys: Vec<String>,
    /// Its default keys.
    pub defaults: Vec<String>,
    /// The first key's key equivalent for a menu item (AppKit's
    /// character), if it has a key.
    pub menu_key: Option<String>,
    /// The first key's modifiers.
    pub menu_modifiers: ModifiersRow,
}

fn mods_row(m: u8) -> ModifiersRow {
    ModifiersRow {
        shift: m & SHIFT != 0,
        control: m & CONTROL != 0,
        option: m & OPTION != 0,
        command: m & COMMAND != 0,
    }
}

fn mods_bits(m: &ModifiersRow) -> u8 {
    (if m.shift { SHIFT } else { 0 })
        | (if m.control { CONTROL } else { 0 })
        | (if m.option { OPTION } else { 0 })
        | (if m.command { COMMAND } else { 0 })
}

fn model(message: impl Into<String>) -> VizError {
    VizError::Model {
        message: message.into(),
    }
}

fn parse(keys: &[String]) -> Result<Vec<Chord>, VizError> {
    keys.iter()
        .map(|k| Chord::parse(k).map_err(model))
        .collect()
}

/// The app's key bindings (one for the whole app; Swift's `KeyBindings`
/// owns it). Thread-safe.
#[derive(uniffi::Object)]
pub struct Keymap {
    inner: Mutex<bindings::Keymap>,
}

#[uniffi::export]
impl Keymap {
    /// The registry's defaults.
    #[uniffi::constructor]
    pub fn new() -> Arc<Keymap> {
        Arc::new(Keymap {
            inner: Mutex::new(bindings::Keymap::default()),
        })
    }

    /// Every command with its keys, in registry order.
    pub fn rows(&self) -> Vec<BindingRow> {
        let k = lock(&self.inner);
        COMMANDS
            .iter()
            .map(|c| {
                let now = k.chords(c.id);
                BindingRow {
                    id: c.id.to_string(),
                    menu: c.menu.to_string(),
                    title: c.title.to_string(),
                    menu_item: c.menu_item,
                    step_entry: c.context == KeyContext::StepEntry,
                    keys: now.iter().map(Chord::text).collect(),
                    defaults: default_chords(c).iter().map(Chord::text).collect(),
                    menu_key: now.first().map(Chord::menu_key),
                    menu_modifiers: mods_row(now.first().map_or(0, |ch| ch.mods)),
                }
            })
            .collect()
    }

    /// The command a key press runs (plan 4f Ruling 4): during step entry
    /// its keys first, except the number row's when `reserve_numbers`;
    /// then the global keys. None for a key VizDAW does not bind.
    pub fn lookup(
        &self,
        key_code: u16,
        modifiers: ModifiersRow,
        step_entry: bool,
        reserve_numbers: bool,
    ) -> Option<String> {
        let chord = Chord::new(key_code, mods_bits(&modifiers))?;
        lock(&self.inner)
            .lookup(chord, step_entry, reserve_numbers)
            .map(str::to_string)
    }

    /// The global command a keyboard slot's key (`a`, `F5`) with
    /// `modifiers` (the slot's bitmask) would clash with (parent spec §5:
    /// such a slot is refused).
    pub fn slot_clash(&self, key: String, modifiers: u8) -> Option<String> {
        let chord = Chord::of_slot(&key, modifiers)?;
        lock(&self.inner)
            .holder(chord, KeyContext::Global)
            .map(str::to_string)
    }

    /// Gives command `id` exactly `keys`; a key another command (of the
    /// same context) has is refused naming it, unless `replace` takes it.
    pub fn bind(&self, id: String, keys: Vec<String>, replace: bool) -> Result<(), VizError> {
        ffi_guard(|| {
            let chords = parse(&keys)?;
            lock(&self.inner)
                .bind(&id, &chords, replace)
                .map_err(|e| model(e.to_string()))
        })
    }

    /// Command `id` back to its default keys (those still free).
    pub fn reset_one(&self, id: String) {
        lock(&self.inner).reset_one(&id);
    }

    /// Every command back to its defaults.
    pub fn reset(&self) {
        lock(&self.inner).reset();
    }

    /// The bindings document (only the changes).
    pub fn to_json(&self) -> String {
        lock(&self.inner).to_json()
    }

    /// Replaces the bindings with a document's (over the defaults); what
    /// it skipped. Not a bindings document: `Model` error, nothing
    /// changed.
    pub fn load_json(&self, json: String) -> Result<Vec<String>, VizError> {
        ffi_guard(|| {
            let (k, skipped) = bindings::Keymap::from_json(COMMANDS, &json).map_err(model)?;
            *lock(&self.inner) = k;
            Ok(skipped)
        })
    }

    /// Writes the bindings document to `path`.
    pub fn save_file(&self, path: String) -> Result<(), VizError> {
        ffi_guard(|| {
            let text = lock(&self.inner).to_json();
            std::fs::write(&path, text).map_err(|e| VizError::Io {
                message: format!("{path}: {e}"),
            })
        })
    }

    /// Reads a bindings document from `path` (at most 256 KiB); what it
    /// skipped.
    pub fn load_file(&self, path: String) -> Result<Vec<String>, VizError> {
        ffi_guard(|| {
            let io = |e: std::io::Error| VizError::Io {
                message: format!("{path}: {e}"),
            };
            let len = std::fs::metadata(&path).map_err(io)?.len();
            if len > MAX_JSON as u64 {
                return Err(model(format!(
                    "a key bindings file is at most {MAX_JSON} bytes"
                )));
            }
            let text = std::fs::read_to_string(&path).map_err(io)?;
            self.load_json(text)
        })
    }
}
VIZDAW_EOF
```

In `crates/core-ffi/src/lib.rs`:

Find:

```rust
pub mod guard;
pub mod host;
pub mod input;
pub mod mixer;
pub mod modify;
pub mod plugins;
```

Replace with:

```rust
pub mod guard;
pub mod host;
pub mod input;
pub mod keys;
pub mod mixer;
pub mod modify;
pub mod plugins;
```

Find:

```rust
pub mod selection;
pub mod session;
pub mod snapshot;
pub mod strip;
pub mod templates;

```

Replace with:

```rust
pub mod selection;
pub mod session;
pub mod snapshot;
pub mod step;
pub mod strip;
pub mod templates;

```

Find:

```rust
};
pub use guard::ffi_guard;
pub use input::{NoteInRow, NoteInput};
pub use mixer::{
    BusRow, DestRow, MeterRow, MixerLevels, MixerSnapshot, SendRow, StripKindRow, StripRow,
};
```

Replace with:

```rust
};
pub use guard::ffi_guard;
pub use input::{NoteInRow, NoteInput};
pub use keys::{BindingRow, Keymap};
pub use mixer::{
    BusRow, DestRow, MeterRow, MixerLevels, MixerSnapshot, SendRow, StripKindRow, StripRow,
};
```

Find:

```rust
    DocSnapshot, IdleUpdate, KeySlotRow, LaneRow, ParamRow, ParamValue, PluginFormatTag,
    PluginRefRow, SeqRow, SlotRow, SlotStatus, TransportState,
};
pub use strip::{
    LineOpRow, LinePointRow, LineRow, LineShapeRow, LineTargetRow, PeriodRow, SeriesRow,
};
```

Replace with:

```rust
    DocSnapshot, IdleUpdate, KeySlotRow, LaneRow, ParamRow, ParamValue, PluginFormatTag,
    PluginRefRow, SeqRow, SlotRow, SlotStatus, TransportState,
};
pub use step::{
    NoteValueRow, StepInputRow, StepKindRow, StepNoteRow, StepRecordRow, StepResultRow,
    StepStateRow, step_length, step_parse_pitches, step_state_new,
};
pub use strip::{
    LineOpRow, LinePointRow, LineRow, LineShapeRow, LineTargetRow, PeriodRow, SeriesRow,
};
```

In `crates/core-ffi/src/session.rs`:

Find:

```rust
    }
}

/// The selection (spec §3.3), sequence templates and the editor layout.
#[uniffi::export]
impl Session {
```

Replace with:

```rust
    }
}

/// Step entry (plan 4f Rulings 11–15).
#[uniffi::export]
impl Session {
    /// One step entry `input` on lane `lane` of `seq`, from `state` (the
    /// app's copy). A step that enters notes or a reference, or ties, is
    /// one undo step "Step Entry"; taking one back is "Delete Last Step".
    /// Entered events become the selection. A refused edit leaves the
    /// document and `state` as they were and is the error. `Model` error
    /// while an edit is open, or for an unknown sequence or lane.
    pub fn step_entry(
        &self,
        seq: u64,
        lane: u64,
        state: crate::step::StepStateRow,
        input: crate::step::StepInputRow,
    ) -> Result<crate::step::StepResultRow, VizError> {
        use core_editor::step::{Effect, commit, press};
        use core_model::command::EventCmd;
        use core_model::event::{EventBody, EventSpec};
        ffi_guard(|| {
            let (seq_id, lane_id, next, effect, live) = {
                let inner = lock(&self.inner);
                if inner.edit_label.is_some() {
                    return Err(crate::events::bad("finish the open edit first"));
                }
                let doc = inner.doc();
                let seq_id = seq_id(doc, seq)?;
                let lane_id = LaneId(lane);
                doc.lane(seq_id, lane_id)?;
                let input = crate::step::input(&input, |r| {
                    core_model::query::seq_length(doc, r).unwrap_or(Tick::ZERO)
                });
                let (next, effect) = press(&crate::step::state(&state), input);
                // A tie or a step taken back acts only on its events still
                // in this lane (the state is the app's copy; it may be old).
                let live = |ids: &[core_model::ids::EventId]| -> Vec<core_model::ids::EventId> {
                    ids.iter()
                        .copied()
                        .filter(|e| {
                            doc.locate(*e)
                                .is_ok_and(|l| l.seq == seq_id && l.lane == lane_id)
                        })
                        .collect()
                };
                let live = match &effect {
                    Effect::Extend { notes, .. } => live(notes),
                    Effect::TakeBack {
                        delete, shorten, ..
                    } => [live(delete), live(shorten)].concat(),
                    _ => Vec::new(),
                };
                (seq_id, lane_id, next, effect, live)
            };
            let cmds: Vec<Command> = match &effect {
                Effect::Nothing | Effect::Rest { .. } => Vec::new(),
                Effect::Insert {
                    tick,
                    length,
                    notes,
                } => vec![Command::Event(EventCmd::Insert {
                    seq: seq_id,
                    lane: lane_id,
                    events: notes
                        .iter()
                        .map(|(p, v)| EventSpec::new(*tick, EventBody::note(*p, *v, *length)))
                        .collect(),
                })],
                Effect::InsertReference { tick, seq: r, .. } => {
                    vec![Command::Event(EventCmd::Insert {
                        seq: seq_id,
                        lane: lane_id,
                        events: vec![EventSpec::new(*tick, EventBody::reference(*r))],
                    })]
                }
                Effect::Extend { by, .. } if !live.is_empty() => {
                    vec![Command::Event(EventCmd::Resize {
                        events: live.clone(),
                        delta: *by,
                    })]
                }
                Effect::Extend { .. } => Vec::new(),
                Effect::TakeBack {
                    delete,
                    shorten,
                    by,
                    ..
                } => {
                    let mut c = Vec::new();
                    let gone: Vec<_> = delete
                        .iter()
                        .filter(|e| live.contains(e))
                        .copied()
                        .collect();
                    if !gone.is_empty() {
                        c.push(Command::Event(EventCmd::Delete { events: gone }));
                    }
                    let short: Vec<_> = shorten
                        .iter()
                        .filter(|e| live.contains(e))
                        .copied()
                        .collect();
                    if !short.is_empty() {
                        c.push(Command::Event(EventCmd::Resize {
                            events: short,
                            delta: -*by,
                        }));
                    }
                    c
                }
            };
            let moved = effect != Effect::Nothing;
            if cmds.is_empty() {
                return Ok(crate::step::StepResultRow {
                    state: crate::step::state_row(&commit(&next, &effect, &[])),
                    snapshot: None,
                    moved,
                });
            }
            let label = match effect {
                Effect::TakeBack { .. } => "Delete Last Step",
                _ => "Step Entry",
            };
            let mut inner = lock(&self.inner);
            inner.history.begin(label.to_string())?;
            let mut created = Vec::new();
            for c in &cmds {
                match inner.history.execute(c, &ViewFilter::default()) {
                    Ok(r) => created.extend(r.created.iter().filter_map(|a| match a {
                        core_model::ids::AnyId::Event(e) => Some(*e),
                        _ => None,
                    })),
                    Err(e) => {
                        inner.history.abort()?;
                        return Err(e.into());
                    }
                }
            }
            inner.history.commit()?;
            if !created.is_empty() {
                inner
                    .history
                    .set_selection(core_model::selection::Selection {
                        time_range: None,
                        lanes: std::iter::once((seq_id, lane_id)).collect(),
                        events: created.iter().copied().collect(),
                    });
            }
            inner.version += 1;
            inner.reload_host(self.id);
            self.mark(&mut inner);
            Ok(crate::step::StepResultRow {
                state: crate::step::state_row(&commit(&next, &effect, &created)),
                snapshot: Some(inner.snapshot()?),
                moved,
            })
        })
    }
}

/// The selection (spec §3.3), sequence templates and the editor layout.
#[uniffi::export]
impl Session {
```

Create `crates/core-ffi/src/step.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p crates/core-ffi/src
cat > crates/core-ffi/src/step.rs <<'VIZDAW_EOF'
//! Step entry over the FFI (plan 4f Rulings 11–15): `core_editor::step`'s
//! state and inputs as records. Swift keeps a copy of the state and hands
//! it to `Session.step` with each input; the session applies the effect as
//! one undo step and returns the next state.

use crate::error::VizError;
use crate::guard::ffi_guard;
use crate::snapshot::DocSnapshot;
use core_editor::step::{NoteValue, StepInput, StepKind, StepRecord, StepState};
use core_model::ids::{EventId, SeqId};
use core_model::time::Tick;

/// A note value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum NoteValueRow {
    /// 1/1
    Whole,
    /// 1/2
    Half,
    /// 1/4
    Quarter,
    /// 1/8
    Eighth,
    /// 1/16
    Sixteenth,
    /// 1/32
    ThirtySecond,
}

/// A note of a step: pitch and 16-bit velocity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Record)]
pub struct StepNoteRow {
    /// 0–127.
    pub pitch: u8,
    /// 16-bit velocity.
    pub velocity: u16,
}

/// What a past step did.
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum StepKindRow {
    /// Entered these notes.
    Notes {
        /// Their IDs.
        events: Vec<u64>,
    },
    /// Moved on.
    Rest,
    /// Lengthened notes.
    Tie {
        /// Their IDs.
        events: Vec<u64>,
        /// By how much.
        by: i64,
    },
    /// Entered a reference.
    Reference {
        /// Its ID.
        event: u64,
    },
}

/// One past step.
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Record)]
pub struct StepRecordRow {
    /// Where it began.
    pub tick: i64,
    /// How far it moved.
    pub length: i64,
    /// What it did.
    pub kind: StepKindRow,
}

/// Step entry's state.
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Record)]
pub struct StepStateRow {
    /// Where the next step goes.
    pub tick: i64,
    /// The chosen value.
    pub value: NoteValueRow,
    /// Dotted.
    pub dotted: bool,
    /// Triplet.
    pub triplet: bool,
    /// MIDI keys down.
    pub held: Vec<u8>,
    /// The chord being played.
    pub chord: Vec<StepNoteRow>,
    /// Past steps, oldest first.
    pub history: Vec<StepRecordRow>,
}

/// An input to step entry.
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum StepInputRow {
    /// A note value key.
    Value {
        /// The value.
        value: NoteValueRow,
    },
    /// Toggles dotted.
    Dot,
    /// Toggles triplet.
    Triplet,
    /// Lengthens the last step's notes.
    Tie,
    /// Moves on.
    Rest,
    /// Takes the last step back.
    DeleteLast,
    /// A MIDI key went down.
    NoteOn {
        /// Its note.
        note: StepNoteRow,
    },
    /// A MIDI key went up.
    NoteOff {
        /// Its pitch.
        pitch: u8,
    },
    /// A whole chord at once.
    Notes {
        /// The notes.
        notes: Vec<StepNoteRow>,
    },
    /// A keyboard slot: a reference to `seq`.
    Reference {
        /// The sequence.
        seq: u64,
    },
}

/// What a step input came to.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct StepResultRow {
    /// The next state.
    pub state: StepStateRow,
    /// The document after the step, when it changed.
    pub snapshot: Option<DocSnapshot>,
    /// The position moved (a step was taken or taken back).
    pub moved: bool,
}

/// The core's value.
pub fn value(v: NoteValueRow) -> NoteValue {
    match v {
        NoteValueRow::Whole => NoteValue::Whole,
        NoteValueRow::Half => NoteValue::Half,
        NoteValueRow::Quarter => NoteValue::Quarter,
        NoteValueRow::Eighth => NoteValue::Eighth,
        NoteValueRow::Sixteenth => NoteValue::Sixteenth,
        NoteValueRow::ThirtySecond => NoteValue::ThirtySecond,
    }
}

fn value_row(v: NoteValue) -> NoteValueRow {
    match v {
        NoteValue::Whole => NoteValueRow::Whole,
        NoteValue::Half => NoteValueRow::Half,
        NoteValue::Quarter => NoteValueRow::Quarter,
        NoteValue::Eighth => NoteValueRow::Eighth,
        NoteValue::Sixteenth => NoteValueRow::Sixteenth,
        NoteValue::ThirtySecond => NoteValueRow::ThirtySecond,
    }
}

fn ids(v: &[u64]) -> Vec<EventId> {
    v.iter().copied().map(EventId).collect()
}

fn id_rows(v: &[EventId]) -> Vec<u64> {
    v.iter().map(|e| e.0).collect()
}

/// The core's state (bounded: a stale or hostile copy is cut to size).
pub fn state(r: &StepStateRow) -> StepState {
    StepState {
        tick: Tick(r.tick),
        value: value(r.value),
        dotted: r.dotted,
        triplet: r.triplet,
        held: r.held.iter().map(|p| p & 0x7F).collect(),
        chord: r
            .chord
            .iter()
            .map(|n| (n.pitch & 0x7F, n.velocity))
            .collect(),
        history: r
            .history
            .iter()
            .map(|h| StepRecord {
                tick: Tick(h.tick),
                length: Tick(h.length),
                kind: match &h.kind {
                    StepKindRow::Notes { events } => StepKind::Notes(ids(events)),
                    StepKindRow::Rest => StepKind::Rest,
                    StepKindRow::Tie { events, by } => StepKind::Tie {
                        notes: ids(events),
                        by: Tick(*by),
                    },
                    StepKindRow::Reference { event } => StepKind::Reference(EventId(*event)),
                },
            })
            .collect(),
    }
    .bounded()
}

/// The state as a row.
pub fn state_row(s: &StepState) -> StepStateRow {
    StepStateRow {
        tick: s.tick.0,
        value: value_row(s.value),
        dotted: s.dotted,
        triplet: s.triplet,
        held: s.held.clone(),
        chord: s
            .chord
            .iter()
            .map(|(pitch, velocity)| StepNoteRow {
                pitch: *pitch,
                velocity: *velocity,
            })
            .collect(),
        history: s
            .history
            .iter()
            .map(|h| StepRecordRow {
                tick: h.tick.0,
                length: h.length.0,
                kind: match &h.kind {
                    StepKind::Notes(e) => StepKindRow::Notes { events: id_rows(e) },
                    StepKind::Rest => StepKindRow::Rest,
                    StepKind::Tie { notes, by } => StepKindRow::Tie {
                        events: id_rows(notes),
                        by: by.0,
                    },
                    StepKind::Reference(e) => StepKindRow::Reference { event: e.0 },
                },
            })
            .collect(),
    }
}

/// The core's input; a reference's length is the session's to fill in.
pub fn input(r: &StepInputRow, reference_length: impl FnOnce(SeqId) -> Tick) -> StepInput {
    let notes = |v: &[StepNoteRow]| v.iter().map(|n| (n.pitch, n.velocity)).collect();
    match r {
        StepInputRow::Value { value: v } => StepInput::Value(value(*v)),
        StepInputRow::Dot => StepInput::Dot,
        StepInputRow::Triplet => StepInput::Triplet,
        StepInputRow::Tie => StepInput::Tie,
        StepInputRow::Rest => StepInput::Rest,
        StepInputRow::DeleteLast => StepInput::DeleteLast,
        StepInputRow::NoteOn { note } => StepInput::NoteOn {
            pitch: note.pitch,
            velocity: note.velocity,
        },
        StepInputRow::NoteOff { pitch } => StepInput::NoteOff { pitch: *pitch },
        StepInputRow::Notes { notes: n } => StepInput::Notes(notes(n)),
        StepInputRow::Reference { seq } => StepInput::Reference {
            seq: SeqId(*seq),
            length: reference_length(SeqId(*seq)),
        },
    }
}

/// Step entry starting at `tick` with quarter notes.
#[uniffi::export]
pub fn step_state_new(tick: i64) -> StepStateRow {
    state_row(&StepState::new(Tick(tick)))
}

/// A step's length in ticks.
#[uniffi::export]
pub fn step_length(value: NoteValueRow, dotted: bool, triplet: bool) -> i64 {
    core_editor::step::step_length(self::value(value), dotted, triplet).0
}

/// Typed pitches (`60 64 67`, `C4 E4 G4`; plan 4f Ruling 14).
#[uniffi::export]
pub fn step_parse_pitches(text: String) -> Result<Vec<u8>, VizError> {
    ffi_guard(|| {
        core_editor::step::parse_pitches(&text).map_err(|message| VizError::Model { message })
    })
}
VIZDAW_EOF
```

In `app/Tests/VizDAWAppModelTests/FakeSession.swift`:

Find:

```swift
        return commandResult()
    }

    var undoCalls = 0
    var redoCalls = 0
    func redo() throws -> DocSnapshot {
```

Replace with:

```swift
        return commandResult()
    }

    /// `stepEntry` calls (plan 4f) and how they are answered: by
    /// `stepAnswer`, else a step's quarter note forward for notes, rests,
    /// ties and references (with a new snapshot for notes and references).
    var stepCalls: [(seq: UInt64, lane: UInt64, state: StepStateRow, input: StepInputRow)] = []
    var stepEntryError: VizError?
    var stepAnswer: ((StepStateRow, StepInputRow) -> StepResultRow)?
    func stepEntry(seq: UInt64, lane: UInt64, state: StepStateRow, input: StepInputRow) throws -> StepResultRow {
        if let e = stepEntryError { throw e }
        stepCalls.append((seq, lane, state, input))
        if let a = stepAnswer { return a(state, input) }
        var s = state
        switch input {
        case .notes, .reference:
            s.tick += 15_360
            return StepResultRow(state: s, snapshot: commandResult(), moved: true)
        case .rest, .tie:
            s.tick += 15_360
            return StepResultRow(state: s, snapshot: nil, moved: true)
        default:
            return StepResultRow(state: s, snapshot: nil, moved: false)
        }
    }

    var undoCalls = 0
    var redoCalls = 0
    func redo() throws -> DocSnapshot {
```

- [ ] **Step 4: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-ffi --test step --test keys --test input 2>&1 | grep -E "^test result|FAILED|panicked"
scripts/build-ffi.sh && scripts/swift.sh test --filter ListEditorModelTests 2>&1 | tail -1
```

Expected: `step` 8 passed, `keys` 5 passed, `input` 4 passed; the Swift suite passes (`aPlayedNoteSetsTheCursorNote` ignores note-offs).

- [ ] **Step 5: Gate and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace 2>&1 | grep -E "FAILED|panicked" ; cargo test --workspace -- --list 2>/dev/null | grep -c ': test$'
scripts/build-ffi.sh && scripts/swift.sh build 2>&1 | tail -1 && scripts/swift.sh test 2>&1 | tail -1
git add app/Sources/VizDAWAppModel/ListEditorModel.swift \
  app/Tests/VizDAWAppModelTests/FakeSession.swift \
  app/Tests/VizDAWAppModelTests/ListEditorModelTests.swift \
  crates/core-ffi/src/input.rs \
  crates/core-ffi/src/keys.rs \
  crates/core-ffi/src/lib.rs \
  crates/core-ffi/src/session.rs \
  crates/core-ffi/src/step.rs \
  crates/core-ffi/tests/input.rs \
  crates/core-ffi/tests/keys.rs \
  crates/core-ffi/tests/step.rs
git commit -m "feat(ffi): the keymap, step entry through the session, note-offs from MIDI input

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
git log -1 --format='%h %s'
```

Expected: 901 Rust tests listed (891 + 10); Swift 613 (one test extended). No `FAILED` or `panicked` line (rerun once if the known-flaky MIDI listener test trips, and say so).

---

### Task 6: Swift — `KeyBindings`, `KeyRouter` and the transport's marks

**Files:**
- Create: `app/Sources/VizDAWAppModel/KeyBindings.swift`, `app/Sources/VizDAWAppModel/TransportMarks.swift`
- Modify: `app/Sources/VizDAWAppModel/EditorLayout.swift`, `crates/core-ffi/src/keys.rs`, `crates/core-ffi/src/lib.rs`
- Test: `app/Tests/VizDAWAppModelTests/KeyBindingsTests.swift`, `crates/core-ffi/tests/keys.rs`

**Interfaces:**
- Consumes: Task 5's `Keymap`, `BindingRow`; `KeyStrokeRow`-free routing; `MemoryDefaults`, `AlertItem`, `ControlOutcome`, `SeqRow.keyboardSlot`, `EditorLayout`.
- Produces: `vizdaw_ffi::key_code_for(name) -> Option<u16>`; Swift `KeyBindings` (`@Observable @MainActor`): `key = "VizDAW.keys"`, `reserveKey = "VizDAW.reserveNumberRow"`, `keymap`, `rows`, `reserveNumberRow`, `message`, `init(defaults:)`, `row(_:)`, `command(keyCode:modifiers:stepEntry:)`, `bind(_:keys:replace:) throws`, `reset(_:)`, `resetAll()`, `save(to:) throws`, `load(from:) throws`, `slotClash(_ slot: KeySlotRow) -> String?` (the command's title); `KeyRoute {typing, command(String), slot(UInt64), pass}`; `KeyRouter.{plain(_:), route(keyCode:modifiers:typing:stepEntry:menus:keys:sequences:)}`; `KeyAfter {handled, pass, opened(UUID)}`; `KeyDispatch.after(_:doc:)`; `TransportMarks` (`locatorCount = 10`, `tickLimit = 1 << 40`, `locators: [Int64?]`, `punchIn`, `punchOut`, `punch`, `overdub`, `locator(_:)`, `setLocator(_:_:) -> Bool`, `setPunch(in:out:) -> Bool`, tolerant decoding); `EditorLayout.transport`.

Rulings 3–5 and 7 on the Swift side, without any key handling yet (Task 9 wires it). `KeyRouter.route` is the one decision the keyboard and the channel share: `menus: true` for the keyboard (a menu item's ⌘ or ⌃ key is left to AppKit's menu, with its dialog), `menus: false` for the channel. `KeyDispatch.after` turns a command's outcome into the key's fate: done → used up; unavailable, needs a dialog, no document → passed on; anything else → used up with the reason in the status line.

- [ ] **Step 1: Write the failing tests**

Create `app/Tests/VizDAWAppModelTests/KeyBindingsTests.swift`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p app/Tests/VizDAWAppModelTests
cat > app/Tests/VizDAWAppModelTests/KeyBindingsTests.swift <<'VIZDAW_EOF'
import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

let noMods = ModifiersRow(shift: false, control: false, option: false, command: false)
let cmdMod = ModifiersRow(shift: false, control: false, option: false, command: true)
let optMod = ModifiersRow(shift: false, control: false, option: true, command: false)

@MainActor
@Suite struct KeyBindingsTests {
    @Test func bindingsPersistInThePreferences() throws {
        let d = MemoryDefaults()
        let k = KeyBindings(defaults: d)
        #expect(k.row("file.save")?.keys == ["⌘S"])
        #expect(k.command(keyCode: 49, modifiers: noMods, stepEntry: false) == "transport.play_stop")
        try k.bind("file.save", keys: ["⌘K"])
        #expect(k.row("file.save")?.keys == ["⌘K"] && k.row("file.save")?.menuKey == "k")
        k.reserveNumberRow = true
        let again = KeyBindings(defaults: d)
        #expect(again.row("file.save")?.keys == ["⌘K"])
        #expect(again.reserveNumberRow)
        // A damaged preference leaves the defaults.
        d.set("garbage", forKey: KeyBindings.key)
        #expect(KeyBindings(defaults: d).row("file.save")?.keys == ["⌘S"])
    }

    @Test func aTakenKeyIsRefusedUnlessReplaced() throws {
        let k = KeyBindings(defaults: MemoryDefaults())
        do {
            try k.bind("edit.undo", keys: ["⌘S"])
            Issue.record("bound a taken key")
        } catch {
            #expect(error as? VizError == .Model(message: "⌘S is the key of “Save”"))
        }
        try k.bind("edit.undo", keys: ["⌘S"], replace: true)
        #expect(k.row("file.save")?.keys == [])
        k.reset("file.save")
        #expect(k.row("file.save")?.keys == [], "⌘S stays with Undo")
        k.resetAll()
        #expect(k.row("file.save")?.keys == ["⌘S"] && k.row("edit.undo")?.keys == ["⌘Z"])
    }

    @Test func filesSaveLoadAndRefuseDamage() throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("vzk-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: dir) }
        let k = KeyBindings(defaults: MemoryDefaults())
        try k.bind("file.save", keys: ["F5"])
        let path = dir.appendingPathComponent("mine.vizkeys").path
        try k.save(to: path)
        k.resetAll()
        try k.load(from: path)
        #expect(k.row("file.save")?.keys == ["F5"] && k.message == nil)
        let bad = dir.appendingPathComponent("bad.vizkeys")
        try Data("garbage".utf8).write(to: bad)
        #expect(throws: (any Error).self) { try k.load(from: bad.path) }
        #expect(k.row("file.save")?.keys == ["F5"], "unchanged")
        #expect(k.message?.isEmpty == false)
        try Data(#"{"vizdaw_keys": 1, "bindings": {"no.such": ["F1"]}}"#.utf8).write(to: bad)
        try k.load(from: bad.path)
        #expect(k.message == "Skipped: no command \"no.such\"")
        #expect(k.row("file.save")?.keys == ["⌘S"], "a loaded file replaces the bindings")
    }

    @Test func aSlotThatClashesNamesTheCommand() {
        let k = KeyBindings(defaults: MemoryDefaults())
        #expect(k.slotClash(KeySlotRow(key: "s", modifiers: 8)) == "Save")
        #expect(k.slotClash(KeySlotRow(key: "q", modifiers: 2)) == nil)
    }
}

@MainActor
@Suite struct KeyRouterTests {
    let keys = KeyBindings(defaults: MemoryDefaults())

    func route(
        _ code: UInt16, _ m: ModifiersRow = noMods, typing: Bool = false, step: Bool = false, menus: Bool = true,
        seqs: [SeqRow] = []
    ) -> KeyRoute {
        KeyRouter.route(
            keyCode: code, modifiers: m, typing: typing, stepEntry: step, menus: menus, keys: keys, sequences: seqs)
    }

    @Test func textFieldsKeepTheirPlainKeys() {
        #expect(route(49, typing: true) == .typing, "Space types a space")
        #expect(route(49) == .command("transport.play_stop"))
        #expect(route(1, cmdMod, typing: true, menus: false) == .command("file.save"), "⌘ keys still run")
    }

    @Test func menuKeysAreTheMenusOnTheKeyboardAndRunOnTheChannel() {
        #expect(route(1, cmdMod) == .pass, "the File menu saves, with its dialog")
        #expect(route(1, cmdMod, menus: false) == .command("file.save"))
        #expect(route(7) == .pass, "X is nothing")
    }

    @Test func keyboardSlotsNameTheirSequence() {
        var beat = seq(1, "Beat", topLevel: false)
        beat.keyboardSlot = KeySlotRow(key: "b", modifiers: 4)
        #expect(route(11, optMod, seqs: [beat]) == .slot(1))
        #expect(route(11, seqs: [beat]) == .pass, "without ⌥")
        #expect(route(11, optMod, typing: true, seqs: [beat]) == .typing)
    }

    @Test func aRefusedOrDialogCommandPassesTheKeyOn() throws {
        let m = try DocumentModel(session: demoFake())
        let ran = ControlResult.ran(command: "x", doc: nil, window: nil, undoLabel: nil, status: nil)
        #expect(KeyDispatch.after(.done(ran), doc: m) == .handled)
        #expect(KeyDispatch.after(.failed(.of(.unavailable, "no")), doc: m) == .pass)
        #expect(KeyDispatch.after(.failed(.dialog("Save", "path")), doc: m) == .pass)
        #expect(m.editor.status == nil)
        #expect(KeyDispatch.after(.failed(.of(.failed, "Refused.")), doc: m) == .handled)
        #expect(m.editor.status == "Refused.")
        let id = UUID()
        #expect(KeyDispatch.after(.opened(command: nil, doc: id), doc: m) == .opened(id))
    }
}

@Suite struct TransportMarksTests {
    @Test func marksSetWithinTheirLimits() {
        var t = TransportMarks()
        #expect(t.locators.count == 10 && t.locators.allSatisfy { $0 == nil })
        let set = t.setLocator(3, 61_440)
        #expect(set && t.locator(3) == 61_440)
        let refused = [t.setLocator(10, 0), t.setLocator(-1, 0), t.setLocator(1, 1 << 41)]
        #expect(refused == [false, false, false] && t.locator(10) == nil && t.locator(1) == nil)
        let cleared = t.setLocator(3, nil)
        #expect(cleared && t.locator(3) == nil)
        let empty = t.setPunch(in: 100, out: 100)
        #expect(!empty && t.punchOut == 4 * 61_440)
        let punched = t.setPunch(in: 0, out: 15_360)
        #expect(punched && t.punchOut == 15_360)
        var l = EditorLayout()
        l.transport = t
        #expect(EditorLayout.decode(l.encoded()).transport == t)
    }

    @Test func marksDecodeTolerantly() {
        let damaged = EditorLayout.decode(
            #"{"transport":{"locators":[1,null,"x"],"punchIn":50,"punchOut":10,"punch":"yes","overdub":true}}"#
        ).transport
        #expect(damaged.locators == Array(repeating: nil, count: 10), "an unreadable list: none set")
        #expect(damaged.punchIn == 0 && damaged.punchOut == 4 * 61_440, "out before in: the defaults")
        #expect(!damaged.punch && damaged.overdub)
        let odd = EditorLayout.decode(
            #"{"transport":{"locators":[1,null,9999999999999999,4,5,6,7,8,9,10,11,12],"punchIn":-5,"punchOut":7}}"#
        ).transport
        #expect(odd.locators == [1, nil, nil, 4, 5, 6, 7, 8, 9, 10], "past the limit: unset; ten kept")
        #expect(odd.punchIn == -5 && odd.punchOut == 7)
        #expect(EditorLayout.decode(#"{"snap":{"division":"half","triplet":false,"dotted":false,"enabled":true}}"#).transport == TransportMarks())
    }
}
VIZDAW_EOF
```

In `crates/core-ffi/tests/keys.rs`:

Find:

```rust
        .unwrap();
    assert_eq!(skipped, vec!["no command \"no.such\"".to_string()]);
}
```

Replace with:

```rust
        .unwrap();
    assert_eq!(skipped, vec!["no command \"no.such\"".to_string()]);
}

#[test]
fn a_slots_key_name_gives_its_key_code() {
    assert_eq!(vizdaw_ffi::key_code_for("a".into()), Some(0));
    assert_eq!(vizdaw_ffi::key_code_for("F5".into()), Some(96));
    assert_eq!(vizdaw_ffi::key_code_for("space".into()), Some(49));
    assert_eq!(
        vizdaw_ffi::key_code_for("⌘A".into()),
        None,
        "a slot's modifiers are apart"
    );
    assert_eq!(vizdaw_ffi::key_code_for("nope".into()), None);
}
```

- [ ] **Step 2: Run them to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-ffi --test keys 2>&1 | grep -E "^error" | head -2
scripts/swift.sh test --filter 'KeyBindingsTests|KeyRouterTests|TransportMarksTests' 2>&1 | grep -E "error:" | head -3
```

Expected: FAIL to compile: no `key_code_for` in `vizdaw_ffi`; `cannot find 'KeyBindings' in scope`.

- [ ] **Step 3: Implement**

In `app/Sources/VizDAWAppModel/EditorLayout.swift`:

Find:

```swift
    public var stripChart = StripChartSettings()
    /// Every drum grid's bar (Plan 4e Ruling 11).
    public var drumGrid = DrumGridSettings()

    public init() {}

    enum CodingKeys: String, CodingKey {
        case version, detail, lastKind, windows, sequenceOrder, blockRule
        case lightMode, snap, laneHeight, ticksPerPoint, follow, pianoRoll, stripChart, drumGrid
    }

    public init(from decoder: Decoder) throws {
```

Replace with:

```swift
    public var stripChart = StripChartSettings()
    /// Every drum grid's bar (Plan 4e Ruling 11).
    public var drumGrid = DrumGridSettings()
    /// Locators, punch range, punch and overdub (plan 4f Ruling 7).
    public var transport = TransportMarks()

    public init() {}

    enum CodingKeys: String, CodingKey {
        case version, detail, lastKind, windows, sequenceOrder, blockRule
        case lightMode, snap, laneHeight, ticksPerPoint, follow, pianoRoll, stripChart, drumGrid, transport
    }

    public init(from decoder: Decoder) throws {
```

Find:

```swift
        pianoRoll = c.field(.pianoRoll, d.pianoRoll)
        stripChart = c.field(.stripChart, d.stripChart)
        drumGrid = c.field(.drumGrid, d.drumGrid)
    }

    /// Lane heights a hand-edited or corrupt `ui.json` is clamped to.
```

Replace with:

```swift
        pianoRoll = c.field(.pianoRoll, d.pianoRoll)
        stripChart = c.field(.stripChart, d.stripChart)
        drumGrid = c.field(.drumGrid, d.drumGrid)
        transport = c.field(.transport, d.transport)
    }

    /// Lane heights a hand-edited or corrupt `ui.json` is clamped to.
```

Create `app/Sources/VizDAWAppModel/KeyBindings.swift`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p app/Sources/VizDAWAppModel
cat > app/Sources/VizDAWAppModel/KeyBindings.swift <<'VIZDAW_EOF'
import Foundation
import Observation
import VizDAWCore

/// The app's key bindings (editors spec §4f, plan 4f Rulings 3–6): every
/// registry command with its keys, kept in the preferences, saved to and
/// loaded from `.vizkeys` files, reset; menus show the keys from here.
/// The bindings themselves (what a key runs, clashes, the file) are Rust's
/// `Keymap`.
@Observable
@MainActor
public final class KeyBindings {
    /// The preferences key of the bindings document.
    public static let key = "VizDAW.keys"
    /// The preferences key of "the number row is for locators".
    public static let reserveKey = "VizDAW.reserveNumberRow"

    @ObservationIgnored public let keymap: Keymap
    @ObservationIgnored private let defaults: UserDefaults
    /// Every command with its keys, in registry order.
    public private(set) var rows: [BindingRow]
    /// The number row jumps to locators even during step entry (whose
    /// note values then come from the keypad; plan 4f Ruling 4).
    public var reserveNumberRow: Bool {
        didSet { defaults.set(reserveNumberRow, forKey: Self.reserveKey) }
    }
    /// What the last load skipped or why it failed, for the Keys page.
    public var message: String?

    public init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
        keymap = Keymap()
        if let json = defaults.string(forKey: Self.key) {
            // A damaged preference leaves the defaults.
            _ = try? keymap.loadJson(json: json)
        }
        rows = keymap.rows()
        reserveNumberRow = defaults.bool(forKey: Self.reserveKey)
    }

    /// Command `id`'s row.
    public func row(_ id: String) -> BindingRow? { rows.first { $0.id == id } }

    /// The command a key press runs.
    public func command(keyCode: UInt16, modifiers: ModifiersRow, stepEntry: Bool) -> String? {
        keymap.lookup(keyCode: keyCode, modifiers: modifiers, stepEntry: stepEntry, reserveNumbers: reserveNumberRow)
    }

    /// Gives command `id` exactly `keys` (chord texts); a key another
    /// command has is refused naming it, unless `replace` takes it.
    public func bind(_ id: String, keys: [String], replace: Bool = false) throws {
        try keymap.bind(id: id, keys: keys, replace: replace)
        changed()
    }

    public func reset(_ id: String) {
        keymap.resetOne(id: id)
        changed()
    }

    public func resetAll() {
        keymap.reset()
        changed()
    }

    /// Writes the bindings to `path`.
    public func save(to path: String) throws { try keymap.saveFile(path: path) }

    /// Replaces the bindings with the file's; `message` lists what it
    /// skipped. A file that is no bindings file changes nothing.
    public func load(from path: String) throws {
        do {
            let skipped = try keymap.loadFile(path: path)
            message = skipped.isEmpty ? nil : "Skipped: " + skipped.joined(separator: "; ")
        } catch {
            message = AlertItem(error, title: "").message
            throw error
        }
        changed()
    }

    /// The title of the command a keyboard slot would clash with (parent
    /// spec §5: such a slot is refused).
    public func slotClash(_ slot: KeySlotRow) -> String? {
        keymap.slotClash(key: slot.key, modifiers: slot.modifiers).map { row($0)?.title ?? $0 }
    }

    private func changed() {
        rows = keymap.rows()
        defaults.set(keymap.toJson(), forKey: Self.key)
    }
}

/// What a key press does (plan 4f Ruling 5).
public enum KeyRoute: Equatable, Sendable {
    /// A text field has the keyboard: the key types.
    case typing
    /// Runs this command on the window's document.
    case command(String)
    /// A sequence's keyboard slot.
    case slot(UInt64)
    /// Not VizDAW's: AppKit's menus and the views have it.
    case pass
}

/// Decides what a key press does, the same for the keyboard and the
/// control channel's `ui.key` (plan 4f Ruling 5).
@MainActor
public enum KeyRouter {
    /// No ⌘ and no ⌃: a key that types in a text field.
    public static func plain(_ m: ModifiersRow) -> Bool { !m.command && !m.control }

    /// The route of a key press in a window of a document. `typing`: a
    /// text field has the keyboard. `menus`: AppKit's menus will see the
    /// key next (the keyboard), so a menu item's ⌘ or ⌃ key is left to
    /// its menu, which may open its dialog; the channel has no menus.
    public static func route(
        keyCode: UInt16, modifiers m: ModifiersRow, typing: Bool, stepEntry: Bool, menus: Bool,
        keys: KeyBindings, sequences: [SeqRow]
    ) -> KeyRoute {
        if typing && plain(m) { return .typing }
        if let id = keys.command(keyCode: keyCode, modifiers: m, stepEntry: stepEntry) {
            if menus, !plain(m), keys.row(id)?.menuItem == true { return .pass }
            return .command(id)
        }
        let bits = UInt8(m.shift ? 1 : 0) | (m.control ? 2 : 0) | (m.option ? 4 : 0) | (m.command ? 8 : 0)
        let slot = sequences.first { s in
            guard let k = s.keyboardSlot else { return false }
            return k.modifiers & 15 == bits && keyCodeFor(name: k.key) == keyCode
        }
        return slot.map { .slot($0.id) } ?? .pass
    }
}

/// What the key press comes to after its command ran.
public enum KeyAfter: Equatable, Sendable {
    /// Done: the key is used up.
    case handled
    /// The command was unavailable (or needs its dialog): the key goes on
    /// to the menus and the views, as with a disabled menu item.
    case pass
    /// The command made a document; its window should open.
    case opened(UUID)
}

@MainActor
public enum KeyDispatch {
    /// The key's fate after `outcome`; a refusal shows in `doc`'s status
    /// line.
    public static func after(_ outcome: ControlOutcome, doc: DocumentModel?) -> KeyAfter {
        switch outcome {
        case .done: return .handled
        case .opened(_, let d): return .opened(d)
        case .failed(let f):
            switch f.code {
            case .unavailable, .dialogRequired, .unknownId, .noDocument: return .pass
            default:
                doc?.editor.status = f.message
                return .handled
            }
        }
    }
}
VIZDAW_EOF
```

Create `app/Sources/VizDAWAppModel/TransportMarks.swift`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p app/Sources/VizDAWAppModel
cat > app/Sources/VizDAWAppModel/TransportMarks.swift <<'VIZDAW_EOF'
import Foundation

/// The transport's marks and record modes, saved with the project in
/// `ui.json` (plan 4f Rulings 7–8): ten numbered locators, the punch range
/// and the punch and overdub switches (recording, sub-project 5, will
/// read them). Decoding is tolerant: a missing or malformed field keeps
/// its default; ticks are kept within ±2^40.
public struct TransportMarks: Codable, Equatable, Sendable {
    /// How many locators (keys 0–9).
    public static let locatorCount = 10
    /// The model's time limit.
    public static let tickLimit: Int64 = 1 << 40

    /// Locator `n`'s tick, or nil when it is not set.
    public var locators: [Int64?] = Array(repeating: nil, count: locatorCount)
    public var punchIn: Int64 = 0
    /// After `punchIn`.
    public var punchOut: Int64 = 4 * 61_440
    public var punch = false
    public var overdub = false

    public init() {}

    enum CodingKeys: String, CodingKey {
        case locators, punchIn, punchOut, punch, overdub
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        let d = TransportMarks()
        let raw: [Int64?] = c.field(.locators, d.locators)
        locators = (0..<Self.locatorCount).map { i in
            i < raw.count ? raw[i].flatMap(Self.sane) : nil
        }
        let a = c.field(.punchIn, d.punchIn)
        let b = c.field(.punchOut, d.punchOut)
        if let a = Self.sane(a), let b = Self.sane(b), b > a {
            punchIn = a
            punchOut = b
        }
        punch = c.field(.punch, d.punch)
        overdub = c.field(.overdub, d.overdub)
    }

    /// `t` when it is within the model's limit.
    static func sane(_ t: Int64) -> Int64? { abs(t) <= tickLimit ? t : nil }

    /// Locator `n` (0–9), if set.
    public func locator(_ n: Int) -> Int64? {
        locators.indices.contains(n) ? locators[n] : nil
    }

    /// Sets (a tick) or clears (nil) locator `n`; false for a number
    /// outside 0–9 or a tick past the limit.
    @discardableResult
    public mutating func setLocator(_ n: Int, _ tick: Int64?) -> Bool {
        guard locators.indices.contains(n) else { return false }
        if let t = tick, Self.sane(t) == nil { return false }
        locators[n] = tick
        return true
    }

    /// Sets the punch range; false (nothing changed) unless `out > in`
    /// and both are within the limit.
    @discardableResult
    public mutating func setPunch(in a: Int64, out b: Int64) -> Bool {
        guard Self.sane(a) != nil, Self.sane(b) != nil, b > a else { return false }
        punchIn = a
        punchOut = b
        return true
    }
}
VIZDAW_EOF
```

In `crates/core-ffi/src/keys.rs`:

Find:

```rust
        })
    }
}
```

Replace with:

```rust
        })
    }
}

/// The key code of a key's name (`a`, `F5`, `Space`; the names chords
/// use, any case), for matching a keyboard slot to a key press.
#[uniffi::export]
pub fn key_code_for(name: String) -> Option<u16> {
    Chord::of_slot(&name, 0).map(|c| c.code)
}
```

In `crates/core-ffi/src/lib.rs`:

Find:

```rust
};
pub use guard::ffi_guard;
pub use input::{NoteInRow, NoteInput};
pub use keys::{BindingRow, Keymap};
pub use mixer::{
    BusRow, DestRow, MeterRow, MixerLevels, MixerSnapshot, SendRow, StripKindRow, StripRow,
};
```

Replace with:

```rust
};
pub use guard::ffi_guard;
pub use input::{NoteInRow, NoteInput};
pub use keys::{BindingRow, Keymap, key_code_for};
pub use mixer::{
    BusRow, DestRow, MeterRow, MixerLevels, MixerSnapshot, SendRow, StripKindRow, StripRow,
};
```

- [ ] **Step 4: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-ffi --test keys 2>&1 | grep -E "^test result"
scripts/build-ffi.sh && scripts/swift.sh test --filter 'KeyBindingsTests|KeyRouterTests|TransportMarksTests' 2>&1 | tail -1
```

Expected: `keys` 6 passed; 10 Swift tests in 3 suites pass.

- [ ] **Step 5: Gate and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace 2>&1 | grep -E "FAILED|panicked" ; cargo test --workspace -- --list 2>/dev/null | grep -c ': test$'
scripts/build-ffi.sh && scripts/swift.sh build 2>&1 | tail -1 && scripts/swift.sh test 2>&1 | tail -1
git add app/Sources/VizDAWAppModel/EditorLayout.swift \
  app/Sources/VizDAWAppModel/KeyBindings.swift \
  app/Sources/VizDAWAppModel/TransportMarks.swift \
  app/Tests/VizDAWAppModelTests/KeyBindingsTests.swift \
  crates/core-ffi/src/keys.rs \
  crates/core-ffi/src/lib.rs \
  crates/core-ffi/tests/keys.rs
git commit -m "feat(keys): key bindings, routing and the transport's marks in the app model

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
git log -1 --format='%h %s'
```

Expected: 902 Rust tests listed (901 + 1); Swift 623 (613 + 10). No `FAILED` or `panicked` line (rerun once if the known-flaky MIDI listener test trips, and say so).

---

### Task 7: Transport, locator, mode and key-binding commands — 48 registry entries, one handler

**Files:**
- Create: `app/Sources/VizDAWAppModel/TransportCommands.swift`
- Modify: `app/Sources/VizDAW/ControlChannel.swift`, `app/Sources/VizDAW/VizDAWApp.swift`, `app/Sources/VizDAWAppModel/ControlCommands.swift`, `app/Sources/VizDAWAppModel/DocumentModel.swift`, `app/Sources/VizDAWAppModel/EditorModel.swift`, `crates/core-control/src/registry.rs`
- Test: `app/Tests/VizDAWAppModelTests/ControlCommandsTests.swift`, `app/Tests/VizDAWAppModelTests/TransportCommandsTests.swift`, `crates/core-control/tests/commands.rs`

**Interfaces:**
- Consumes: Task 3's `Session.{playFrom, selectionSpan, markerStep, moveRecordArm}`; Task 6's `KeyBindings`, `TransportMarks`; `ControlCommands.{run, handled, unavailable}`, `registry::{cmd, CommandSpec, ON}`.
- Produces: registry IDs `editors.snap` (G, `on`), `transport.{stop (S), play_selection (⌥Space), play_from_selection (⇧Space), play_from_edit_in (Y), record (R), scrub (K), previous_marker (,), next_marker (.), overdub (O, on), punch (I, on), arm_previous (⌥⇧↑), arm_next (⌥⇧↓)}`, `transport.loop` gains L, `record.{keep_pass, discard_pass}` (no menu item), `locator.{go,set,clear}_{0…9}` (no menu item; N, ⌥N, ⌥⇧N), `keys.{save, load}` (`path` `.vizkeys`, answering Save/Open; no document) and `keys.reset`; count 44 → 92. Swift: `KeyOutcome` (= `DrumOutcome`); `TransportCommands.{notYet, ids, locator(_:) -> (op, n)?, enabled(_:_:), isOn(_:_:), run(_:on:_:)}`; `DocumentModel.{playStart, startPlaying(from:to:) throws, stopAndReturn() throws, moveTo(_:) throws}`; `EditorModel.change(_ body: () throws -> DocSnapshot) -> Bool` (what `edit` now uses); `ControlHost.keys: KeyBindings`; `ControlCommands.failure(_:) -> ControlOutcome?`; the app's `keyBindings` and `AppControlHost.keys`.

Rulings 7–10 and the key-binding files of Ruling 17. `ControlCommands.run` handles `keys.*` before it needs a document (a store-level command, like `file.new`) and hands the rest to `TransportCommands`. Play/Stop now starts through `DocumentModel.startPlaying`, so Stop knows where playing started. The menus come in Task 9; the commands already run from the channel.

- [ ] **Step 1: Write the failing tests**

In `app/Tests/VizDAWAppModelTests/ControlCommandsTests.swift`:

Find:

```swift
final class FakeControlHost: ControlHost {
    var calls: [String] = []
    var nudge = NudgeSettings()
    func showMixer(_ doc: UUID) { calls.append("mixer") }
    func showPluginManager() { calls.append("plugins") }
    func tearOff(_ doc: UUID, kind: DetailEditorKind) { calls.append("tear \(kind.rawValue)") }
```

Replace with:

```swift
final class FakeControlHost: ControlHost {
    var calls: [String] = []
    var nudge = NudgeSettings()
    let keys = KeyBindings(defaults: MemoryDefaults())
    func showMixer(_ doc: UUID) { calls.append("mixer") }
    func showPluginManager() { calls.append("plugins") }
    func tearOff(_ doc: UUID, kind: DetailEditorKind) { calls.append("tear \(kind.rawValue)") }
```

Create `app/Tests/VizDAWAppModelTests/TransportCommandsTests.swift`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p app/Tests/VizDAWAppModelTests
cat > app/Tests/VizDAWAppModelTests/TransportCommandsTests.swift <<'VIZDAW_EOF'
import Foundation
import Testing
@testable import VizDAWAppModel
import VizDAWCore

/// The transport keys, modes, locators and key-binding files (plan 4f
/// Task 7), through `ControlCommands.run` as keys and the channel run
/// them.
@MainActor
@Suite struct TransportCommandsTests {
    let host = FakeControlHost()

    func demo() throws -> (DocumentStore, UUID, DocumentModel, FakeSession) {
        let f = demoFake()
        let s = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { f }, open: { _ in demoFake() })
        let id = try #require(s.newDemo())
        return (s, id, try #require(s.model(id)), f)
    }

    func run(_ id: String, _ args: [ControlArg], _ s: DocumentStore, _ doc: UUID?) -> ControlOutcome {
        ControlCommands.run(id, args: args, doc: doc.flatMap { d in s.model(d).map { (d, $0) } }, store: s, host: host)
    }

    func run(_ id: String, _ s: DocumentStore, _ doc: UUID?) -> ControlOutcome { run(id, [], s, doc) }

    func failure(_ o: ControlOutcome) -> ControlFailure? {
        if case .failed(let f) = o { return f }
        return nil
    }

    let on = [ControlArg(name: "on", value: .flag(value: true))]

    @Test func stopGoesBackToWherePlayingStarted() throws {
        let (s, id, m, f) = try demo()
        #expect(failure(run("transport.stop", s, id))?.message == "Nothing is playing.")
        f.state.tick = 30_000
        m.poll()
        _ = run("transport.play_stop", s, id)
        #expect(m.playStart == 30_000 && f.state.playing)
        f.state.tick = 90_000
        #expect(failure(run("transport.stop", s, id)) == nil)
        #expect(!f.state.playing && f.state.tick == 30_000)
    }

    @Test func playSelectionPlaysItsSpanOnceOrOnwards() throws {
        let (s, id, m, f) = try demo()
        #expect(failure(run("transport.play_selection", s, id))?.code == .unavailable, "nothing selected")
        f.span = TickRange(start: 1_000, end: 5_000)
        _ = run("transport.play_selection", s, id)
        #expect(f.playsFrom.last.map { [$0.seq, UInt64($0.from), UInt64($0.to ?? -1)] } == [3, 1_000, 5_000])
        #expect(m.playStart == 1_000)
        _ = run("transport.play_from_selection", s, id)
        #expect(f.playsFrom.last?.to == nil && f.playsFrom.last?.from == 1_000)
        f.playError = .Model(message: "the end must be after the start")
        #expect(failure(run("transport.play_selection", s, id)) == .of(.failed, "the end must be after the start"))
    }

    @Test func playFromEditInNeedsARange() throws {
        let (s, id, m, f) = try demo()
        #expect(failure(run("transport.play_from_edit_in", s, id))?.code == .unavailable)
        #expect(!TransportCommands.enabled("transport.play_from_edit_in", m))
        m.editor.setSelection(SelectionRow(timeRange: TickRange(start: 7_680, end: 61_440), lanes: [], events: []))
        #expect(TransportCommands.enabled("transport.play_from_edit_in", m))
        _ = run("transport.play_from_edit_in", s, id)
        #expect(f.playsFrom.last?.from == 7_680 && f.playsFrom.last?.to == nil)
    }

    @Test func markerKeysLocateToTheNearestMarker() throws {
        let (s, id, _, f) = try demo()
        f.markerTicks = [61_440, 122_880]
        _ = run("transport.next_marker", s, id)
        #expect(f.state.tick == 61_440)
        _ = run("transport.next_marker", s, id)
        #expect(f.state.tick == 122_880)
        #expect(failure(run("transport.next_marker", s, id))?.message == "No marker after the position.")
        _ = run("transport.previous_marker", s, id)
        #expect(f.state.tick == 61_440)
        #expect(failure(run("transport.previous_marker", s, id))?.code == .unavailable)
    }

    @Test func locatorsAreSetJumpedToAndCleared() throws {
        let (s, id, m, f) = try demo()
        #expect(failure(run("locator.go_3", s, id))?.message == "Locator 3 is not set.")
        f.state.tick = 12_345
        _ = run("locator.set_3", s, id)
        #expect(m.editor.layout.transport.locator(3) == 12_345)
        f.state.tick = 0
        _ = run("locator.go_3", s, id)
        #expect(f.state.tick == 12_345)
        _ = run("locator.clear_3", s, id)
        #expect(m.editor.layout.transport.locator(3) == nil)
        #expect(TransportCommands.locator("locator.clear_9")! == ("clear", 9))
        #expect(TransportCommands.locator("transport.stop") == nil)
    }

    @Test func modesToggleOrTakeTheirFlag() throws {
        let (s, id, m, _) = try demo()
        for (cmd, value) in [
            ("transport.overdub", { m.editor.layout.transport.overdub }),
            ("transport.punch", { m.editor.layout.transport.punch }),
        ] {
            _ = run(cmd, on, s, id)
            #expect(value() && TransportCommands.isOn(cmd, m), "\(cmd) on")
            _ = run(cmd, s, id)
            #expect(!value(), "\(cmd) toggled off")
        }
        #expect(m.editor.layout.snap.enabled)
        _ = run("editors.snap", s, id)
        #expect(!m.editor.layout.snap.enabled && !TransportCommands.isOn("editors.snap", m))
    }

    @Test func recordingCommandsWaitForSubProjectFive() throws {
        let (s, id, m, _) = try demo()
        for cmd in ["transport.record", "transport.scrub", "record.keep_pass", "record.discard_pass"] {
            #expect(failure(run(cmd, s, id)) == .of(.unavailable, TransportCommands.notYet), "\(cmd)")
            #expect(!TransportCommands.enabled(cmd, m))
        }
    }

    @Test func recordArmMovesAndARefusalShows() throws {
        let (s, id, m, f) = try demo()
        _ = run("transport.arm_next", s, id)
        #expect(f.armMoves.map(\.forward) == [true])
        f.armError = .Model(message: "there is no lane after the armed one")
        #expect(failure(run("transport.arm_next", s, id)) == .of(.failed, "there is no lane after the armed one"))
        #expect(m.editor.status == "there is no lane after the armed one")
    }

    @Test func keyBindingFilesNeedAPathAndNeedNoDocument() throws {
        let (s, _, _, _) = try demo()
        #expect(failure(run("keys.save", s, nil))?.dialog == "Save")
        #expect(failure(run("keys.load", s, nil))?.dialog == "Open")
        let path = FileManager.default.temporaryDirectory.appendingPathComponent("k-\(UUID().uuidString).vizkeys").path
        defer { try? FileManager.default.removeItem(atPath: path) }
        try host.keys.bind("transport.stop", keys: ["X"])
        let p = [ControlArg(name: "path", value: .text(value: path))]
        guard case .done = run("keys.save", p, s, nil) else {
            Issue.record("not saved")
            return
        }
        _ = run("keys.reset", s, nil)
        #expect(host.keys.row("transport.stop")?.keys == ["S"])
        _ = run("keys.load", p, s, nil)
        #expect(host.keys.row("transport.stop")?.keys == ["X"])
        let bad = [ControlArg(name: "path", value: .text(value: "/nonexistent/k.vizkeys"))]
        #expect(failure(run("keys.load", bad, s, nil))?.message.hasPrefix("Load Key Bindings Failed: ") == true)
    }
}
VIZDAW_EOF
```

In `crates/core-control/tests/commands.rs`:

Find:

```rust
    );
}

#[test]
fn the_registry_has_unique_ids_and_answers_dialogs_by_parameter() {
    let ids: std::collections::BTreeSet<_> = COMMANDS.iter().map(|c| c.id).collect();
    assert_eq!(ids.len(), COMMANDS.len(), "IDs are unique");
    assert_eq!(
        COMMANDS.len(),
        44,
        "38 before plan 4e, plus the Drum menu's six"
    );
    for c in COMMANDS {
        assert!(
            c.id.contains('.')
                && c.id
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b == b'.' || b == b'_'),
            "{}",
            c.id
        );
```

Replace with:

```rust
    );
}

#[test]
fn every_transport_key_has_a_command() {
    use core_control::bindings::{Chord, Keymap};
    let k = Keymap::default();
    let key = |t: &str| k.lookup(Chord::parse(t).unwrap(), false, false);
    for (text, id) in [
        ("Space", "transport.play_stop"),
        ("S", "transport.stop"),
        ("⌥Space", "transport.play_selection"),
        ("⇧Space", "transport.play_from_selection"),
        ("Y", "transport.play_from_edit_in"),
        ("R", "transport.record"),
        ("K", "transport.scrub"),
        ("Return", "transport.return_to_start"),
        (",", "transport.previous_marker"),
        (".", "transport.next_marker"),
        ("L", "transport.loop"),
        ("O", "transport.overdub"),
        ("I", "transport.punch"),
        ("G", "editors.snap"),
        ("⌥⇧↑", "transport.arm_previous"),
        ("⌥⇧↓", "transport.arm_next"),
        ("1", "locator.go_1"),
        ("0", "locator.go_0"),
        ("⌥7", "locator.set_7"),
        ("⌥⇧9", "locator.clear_9"),
    ] {
        assert_eq!(key(text), Some(id), "{text}");
    }
    let locators: Vec<&str> = COMMANDS
        .iter()
        .filter(|c| c.menu == "Locators")
        .map(|c| c.id)
        .collect();
    assert_eq!(locators.len(), 30);
    assert!(
        COMMANDS
            .iter()
            .filter(|c| ["Locators", "Record", "Key Bindings"].contains(&c.menu))
            .all(|c| !c.menu_item),
        "no menu items: keys, the Keys settings and the channel"
    );
    let keys: Vec<&str> = COMMANDS
        .iter()
        .filter(|c| c.id.starts_with("keys."))
        .map(|c| c.id)
        .collect();
    assert_eq!(keys, vec!["keys.save", "keys.load", "keys.reset"]);
    assert!(
        COMMANDS
            .iter()
            .filter(|c| c.id.starts_with("keys."))
            .all(|c| !c.document)
    );
    let e = err("command.run", json!({"id": "keys.save"}));
    assert_eq!(e.0, ErrorCode::DialogRequired);
    assert_eq!(
        err(
            "command.run",
            json!({"id": "keys.load", "params": {"path": "/tmp/k.json"}})
        )
        .0,
        ErrorCode::InvalidParams,
        "a .vizkeys file"
    );
    assert_eq!(
        ok(
            "command.run",
            json!({"id": "transport.punch", "params": {"on": true}})
        ),
        Command::CommandRun {
            id: "transport.punch".into(),
            args: vec![("on".into(), ArgValue::Flag(true))],
            window: None,
        }
    );
}

#[test]
fn the_registry_has_unique_ids_and_answers_dialogs_by_parameter() {
    let ids: std::collections::BTreeSet<_> = COMMANDS.iter().map(|c| c.id).collect();
    assert_eq!(ids.len(), COMMANDS.len(), "IDs are unique");
    assert_eq!(
        COMMANDS.len(),
        92,
        "44 before plan 4f, plus 48 transport, locator, record and key-binding commands"
    );
    for c in COMMANDS {
        // snake_case words (a locator's number too), a dot after the group.
        assert!(
            c.id.contains('.')
                && c.id.bytes().all(|b| b.is_ascii_lowercase()
                    || b.is_ascii_digit()
                    || b == b'.'
                    || b == b'_'),
            "{}",
            c.id
        );
```

- [ ] **Step 2: Run them to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-control --test commands 2>&1 | grep -E "panicked|test result" | head -3
scripts/build-ffi.sh && scripts/swift.sh test --filter 'TransportCommandsTests' 2>&1 | grep -E "error:" | head -3
```

Expected: FAIL: `every_transport_key_has_a_command` (no `transport.stop`) and the count (44, expected 92); Swift: `cannot find 'TransportCommands' in scope`.

- [ ] **Step 3: Implement**

In `app/Sources/VizDAW/ControlChannel.swift`:

Find:

```swift
    }

    var nudge: NudgeSettings { editingPreferences.nudge }
}
```

Replace with:

```swift
    }

    var nudge: NudgeSettings { editingPreferences.nudge }
    var keys: KeyBindings { keyBindings }
}
```

In `app/Sources/VizDAW/VizDAWApp.swift`:

Find:

```swift
/// Editing preferences: the nudge steps (Plan 4b Ruling 16).
@MainActor let editingPreferences = EditingPreferences(defaults: MemoryDefaults.appStore())

/// Notes played on MIDI sources, for list editors with MIDI In on (Plan 4c
/// Ruling 11).
@MainActor let noteInputHub = NoteInputHub(input: NoteInput())
```

Replace with:

```swift
/// Editing preferences: the nudge steps (Plan 4b Ruling 16).
@MainActor let editingPreferences = EditingPreferences(defaults: MemoryDefaults.appStore())

/// The key bindings (plan 4f): menus show them, keys and the channel run
/// by them.
@MainActor let keyBindings = KeyBindings(defaults: MemoryDefaults.appStore())

/// Notes played on MIDI sources, for list editors with MIDI In on (Plan 4c
/// Ruling 11).
@MainActor let noteInputHub = NoteInputHub(input: NoteInput())
```

In `app/Sources/VizDAWAppModel/ControlCommands.swift`:

Find:

```swift
    func tearOff(_ doc: UUID, kind: DetailEditorKind)
    func closeDocument(_ doc: UUID)
    var nudge: NudgeSettings { get }
}

/// Runs registered menu commands (control spec §6) on a document model
```

Replace with:

```swift
    func tearOff(_ doc: UUID, kind: DetailEditorKind)
    func closeDocument(_ doc: UUID)
    var nudge: NudgeSettings { get }
    /// The app's key bindings (plan 4f).
    var keys: KeyBindings { get }
}

/// Runs registered menu commands (control spec §6) on a document model
```

Find:

```swift
        "window.plugin_manager", "editors.close_detail", "editors.insert_event", "modify.again",
        "modify.template", "modify.exact_mode", "sequence.make_reference", "sequence.back",
        "transport.play_stop", "transport.loop", "transport.return_to_start",
    ]

    /// Every registry ID `run` handles (a test checks it is all of them).
    public static var handled: Set<String> {
        other.union(detail.keys).union(torn.keys).union(nudges.keys).union(panels.keys).union(DrumCommands.ids)
    }

    /// Runs command `id` with `args` on `doc` (the target window's document,
```

Replace with:

```swift
        "window.plugin_manager", "editors.close_detail", "editors.insert_event", "modify.again",
        "modify.template", "modify.exact_mode", "sequence.make_reference", "sequence.back",
        "transport.play_stop", "transport.loop", "transport.return_to_start",
        "keys.save", "keys.load", "keys.reset",
    ]

    /// Every registry ID `run` handles (a test checks it is all of them).
    public static var handled: Set<String> {
        other.union(detail.keys).union(torn.keys).union(nudges.keys).union(panels.keys).union(DrumCommands.ids)
            .union(TransportCommands.ids)
    }

    /// Runs command `id` with `args` on `doc` (the target window's document,
```

Find:

```swift
        case "window.plugin_manager":
            host.showPluginManager()
            return .done(ran(id, nil))
        default: break
        }
        guard let (docID, m) = doc else { return .failed(.of(.noDocument, "No document is open.")) }
```

Replace with:

```swift
        case "window.plugin_manager":
            host.showPluginManager()
            return .done(ran(id, nil))
        case "keys.save", "keys.load":
            guard let path = args.text("path") else {
                return .failed(.dialog(id == "keys.save" ? "Save" : "Open", "path"))
            }
            do {
                if id == "keys.save" { try host.keys.save(to: path) } else { try host.keys.load(from: path) }
            } catch {
                return .failed(.thrown(error, id == "keys.save" ? "Save Key Bindings Failed" : "Load Key Bindings Failed"))
            }
            return .done(ran(id, nil))
        case "keys.reset":
            host.keys.resetAll()
            return .done(ran(id, nil))
        default: break
        }
        guard let (docID, m) = doc else { return .failed(.of(.noDocument, "No document is open.")) }
```

Find:

```swift
            do {
                if playing {
                    try m.session.stop()
                } else if let seq = m.selectedSeq {
                    try m.session.play(seq: seq)
                }
            } catch {
                return .failed(.thrown(error, playing ? "Stop Failed" : "Play Failed"))
```

Replace with:

```swift
            do {
                if playing {
                    try m.session.stop()
                } else {
                    try m.startPlaying()
                }
            } catch {
                return .failed(.thrown(error, playing ? "Stop Failed" : "Play Failed"))
```

Find:

```swift
            guard m.returnKeyEnabled else { return unavailable("A text field or list has the keyboard.") }
            m.returnToStart()
        case _ where DrumCommands.ids.contains(id):
            switch DrumCommands.run(id, on: args.flag("on"), m) {
            case .done: break
            case .unavailable(let why): return unavailable(why)
            case .refused(let why): return .failed(.of(.failed, why))
            }
        default:
            return .failed(.of(.unknownId, "VizDAW has no handler for \(id)."))
        }
```

Replace with:

```swift
            guard m.returnKeyEnabled else { return unavailable("A text field or list has the keyboard.") }
            m.returnToStart()
        case _ where DrumCommands.ids.contains(id):
            if let f = failure(DrumCommands.run(id, on: args.flag("on"), m)) { return f }
        case _ where TransportCommands.ids.contains(id):
            if let f = failure(TransportCommands.run(id, on: args.flag("on"), m)) { return f }
        default:
            return .failed(.of(.unknownId, "VizDAW has no handler for \(id)."))
        }
```

Find:

```swift

    static func unavailable(_ message: String) -> ControlOutcome { .failed(.of(.unavailable, message)) }

    static func ran(_ id: String, _ doc: UUID?, _ m: DocumentModel? = nil) -> ControlResult {
        .ran(
            command: id, doc: doc?.uuidString, window: nil, undoLabel: m?.snapshot.undoLabel,
```

Replace with:

```swift

    static func unavailable(_ message: String) -> ControlOutcome { .failed(.of(.unavailable, message)) }

    /// A command's outcome as the channel's failure; nil when it ran.
    static func failure(_ o: KeyOutcome) -> ControlOutcome? {
        switch o {
        case .done: nil
        case .unavailable(let why): unavailable(why)
        case .refused(let why): .failed(.of(.failed, why))
        }
    }

    static func ran(_ id: String, _ doc: UUID?, _ m: DocumentModel? = nil) -> ControlResult {
        .ran(
            command: id, doc: doc?.uuidString, window: nil, undoLabel: m?.snapshot.undoLabel,
```

In `app/Sources/VizDAWAppModel/DocumentModel.swift`:

Find:

```swift
    /// this, not `transport`.
    public private(set) var playKey = PlayKey(playing: false, seq: nil, stoppedTick: 0)
    public private(set) var looping = false
    /// The sequence whose name is being edited in the sidebar.
    public private(set) var renamingSeq: UInt64?
    /// The edited name (view state lives here: the Command Line Tools
```

Replace with:

```swift
    /// this, not `transport`.
    public private(set) var playKey = PlayKey(playing: false, seq: nil, stoppedTick: 0)
    public private(set) var looping = false
    /// Where playing last started, for Stop (plan 4f Ruling 8).
    public private(set) var playStart: Int64?
    /// The sequence whose name is being edited in the sidebar.
    public private(set) var renamingSeq: UInt64?
    /// The edited name (view state lives here: the Command Line Tools
```

Find:

```swift
        do {
            if isPlaying {
                try session.stop()
            } else if let seq = selectedSeq {
                try session.play(seq: seq)
            }
        } catch {
            alert = AlertItem(error, title: isPlaying ? "Stop Failed" : "Play Failed")
```

Replace with:

```swift
        do {
            if isPlaying {
                try session.stop()
            } else if selectedSeq != nil {
                try startPlaying()
            }
        } catch {
            alert = AlertItem(error, title: isPlaying ? "Stop Failed" : "Play Failed")
```

Find:

```swift
        poll()
    }

    public func toggleLoop() {
        do {
            try session.setLoop(on: !looping)
```

Replace with:

```swift
        poll()
    }

    /// Plays the shown sequence from `from` (nil: where the transport
    /// is), up to `to` where it stops (plan 4f Ruling 9), and remembers
    /// where it started.
    public func startPlaying(from: Int64? = nil, to: Int64? = nil) throws {
        guard let seq = selectedSeq else { throw VizError.Model(message: "Select a sequence to play.") }
        let start = from ?? transport.tick
        if let from {
            try session.playFrom(seq: seq, from: from, to: to)
        } else {
            try session.play(seq: seq)
        }
        playStart = start
        poll()
    }

    /// Stops and goes back to where playing started (the Stop key, plan
    /// 4f Ruling 8).
    public func stopAndReturn() throws {
        try session.stop()
        try session.locate(tick: playStart ?? 0)
        poll()
    }

    /// Moves the transport to `tick`, throwing (keys report a refusal in
    /// the status line instead of an alert).
    public func moveTo(_ tick: Int64) throws {
        try session.locate(tick: tick)
        poll()
    }

    public func toggleLoop() {
        do {
            try session.setLoop(on: !looping)
```

In `app/Sources/VizDAWAppModel/EditorModel.swift`:

Find:

```swift
    /// `status`; returns whether it applied. Refused while a gesture is open.
    @discardableResult
    public func edit(_ cmd: EditCommand, filter: ViewFilterRow? = nil) -> Bool {
        guard gesture == nil else {
            status = Self.gestureOpenMessage
            return false
        }
        do {
            applySnapshot(try session.edit(cmd: cmd, filter: filter))
            refreshSelection()
            return true
        } catch {
```

Replace with:

```swift
    /// `status`; returns whether it applied. Refused while a gesture is open.
    @discardableResult
    public func edit(_ cmd: EditCommand, filter: ViewFilterRow? = nil) -> Bool {
        change { try session.edit(cmd: cmd, filter: filter) }
    }

    /// Applies a session call that edits and returns the document, as
    /// `edit` does: refused while a drag is open; a failure goes to the
    /// status line.
    @discardableResult
    public func change(_ body: () throws -> DocSnapshot) -> Bool {
        guard gesture == nil else {
            status = Self.gestureOpenMessage
            return false
        }
        do {
            applySnapshot(try body())
            refreshSelection()
            return true
        } catch {
```

Create `app/Sources/VizDAWAppModel/TransportCommands.swift`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p app/Sources/VizDAWAppModel
cat > app/Sources/VizDAWAppModel/TransportCommands.swift <<'VIZDAW_EOF'
import Foundation
import VizDAWCore

/// What a command came to: the Drum menu's three outcomes, shared by
/// every command that runs from keys, menus and the channel alike.
public typealias KeyOutcome = DrumOutcome

/// The transport keys, modes and locators (editors spec §4f, plan 4f
/// Rulings 7–10): one implementation for the menu items, the keys and
/// the control channel's `command.run`. None opens a dialog.
@MainActor
public enum TransportCommands {
    /// Recording, scrubbing and loop passes are sub-project 5's; their
    /// commands exist so keys can be bound now (editors spec §1).
    public static let notYet = "Recording and scrubbing arrive with sub-project 5."
    static let noSequence = "Select a sequence first."

    public static let ids: Set<String> = Set(
        [
            "transport.stop", "transport.play_selection", "transport.play_from_selection",
            "transport.play_from_edit_in", "transport.record", "transport.scrub",
            "transport.previous_marker", "transport.next_marker", "transport.overdub", "transport.punch",
            "transport.arm_previous", "transport.arm_next", "record.keep_pass", "record.discard_pass",
            "editors.snap",
        ]
            + (0..<TransportMarks.locatorCount).flatMap { n in
                ["go", "set", "clear"].map { "locator.\($0)_\(n)" }
            })

    /// A locator command's operation and number.
    static func locator(_ id: String) -> (op: String, n: Int)? {
        guard id.hasPrefix("locator."), let us = id.lastIndex(of: "_"),
            let n = Int(id[id.index(after: us)...])
        else { return nil }
        let op = id[id.index(id.startIndex, offsetBy: 8)..<us]
        return (String(op), n)
    }

    /// Whether `id` can run on `m` now (its menu item is enabled).
    public static func enabled(_ id: String, _ m: DocumentModel) -> Bool {
        switch id {
        case "transport.record", "transport.scrub", "record.keep_pass", "record.discard_pass": false
        case "transport.stop": m.isPlaying
        case "transport.overdub", "transport.punch", "editors.snap": true
        case "transport.play_from_edit_in": m.selectedSeq != nil && m.editor.selection.timeRange != nil
        default: m.selectedSeq != nil
        }
    }

    /// A toggle's state (its menu item's check mark).
    public static func isOn(_ id: String, _ m: DocumentModel) -> Bool {
        switch id {
        case "transport.overdub": m.editor.layout.transport.overdub
        case "transport.punch": m.editor.layout.transport.punch
        case "editors.snap": m.editor.layout.snap.enabled
        default: false
        }
    }

    /// Runs `id` on `m`; `on` sets a toggle (nil flips it).
    @discardableResult
    public static func run(_ id: String, on: Bool? = nil, _ m: DocumentModel) -> KeyOutcome {
        switch id {
        case "transport.record", "transport.scrub", "record.keep_pass", "record.discard_pass":
            return .unavailable(notYet)
        case "transport.overdub":
            m.editor.updateLayout { $0.transport.overdub = on ?? !$0.transport.overdub }
            return .done
        case "transport.punch":
            m.editor.updateLayout { $0.transport.punch = on ?? !$0.transport.punch }
            return .done
        case "editors.snap":
            m.editor.updateLayout { $0.snap.enabled = on ?? !$0.snap.enabled }
            return .done
        case "transport.stop":
            guard m.isPlaying else { return .unavailable("Nothing is playing.") }
            return attempt { try m.stopAndReturn() }
        default: break
        }
        if let (op, n) = locator(id) { return runLocator(op, n, m) }
        guard let seq = m.selectedSeq else { return .unavailable(noSequence) }
        switch id {
        case "transport.play_selection", "transport.play_from_selection":
            let span: TickRange?
            do {
                span = try m.session.selectionSpan(seq: seq)
            } catch {
                return refused(error)
            }
            guard let span else { return .unavailable("Select something in the shown sequence first.") }
            let only = id == "transport.play_selection"
            return attempt { try m.startPlaying(from: span.start, to: only ? span.end : nil) }
        case "transport.play_from_edit_in":
            guard let r = m.editor.selection.timeRange else {
                return .unavailable("Set an edit-in point (select a time range) first.")
            }
            return attempt { try m.startPlaying(from: r.start) }
        case "transport.previous_marker", "transport.next_marker":
            let forward = id == "transport.next_marker"
            m.poll()
            let target: Int64?
            do {
                target = try m.session.markerStep(seq: seq, tick: m.transport.tick, forward: forward)
            } catch {
                return refused(error)
            }
            guard let t = target else {
                return .unavailable(forward ? "No marker after the position." : "No marker before the position.")
            }
            return attempt { try m.moveTo(t) }
        case "transport.arm_previous", "transport.arm_next":
            let forward = id == "transport.arm_next"
            return m.editor.change { try m.session.moveRecordArm(seq: seq, forward: forward) }
                ? .done : .refused(m.editor.status ?? "Record arm did not move.")
        default:
            return .unavailable("VizDAW has no handler for \(id).")
        }
    }

    private static func runLocator(_ op: String, _ n: Int, _ m: DocumentModel) -> KeyOutcome {
        switch op {
        case "go":
            guard let t = m.editor.layout.transport.locator(n) else {
                return .unavailable("Locator \(n) is not set.")
            }
            return attempt { try m.moveTo(t) }
        case "set":
            m.poll()
            let t = m.transport.tick
            m.editor.updateLayout { $0.transport.setLocator(n, t) }
            return .done
        default:
            m.editor.updateLayout { $0.transport.setLocator(n, nil) }
            return .done
        }
    }

    private static func attempt(_ body: () throws -> Void) -> KeyOutcome {
        do {
            try body()
            return .done
        } catch {
            return refused(error)
        }
    }

    private static func refused(_ error: any Error) -> KeyOutcome {
        .refused(AlertItem(error, title: "").message)
    }
}
VIZDAW_EOF
```

In `crates/core-control/src/registry.rs`:

Find:

```rust
    help: "Where to save (answers the Save dialog).",
}];

const ON: &[ParamSpec] = &[ParamSpec {
    name: "on",
    kind: ParamKind::Flag,
```

Replace with:

```rust
    help: "Where to save (answers the Save dialog).",
}];

const KEYS_PATH: &[ParamSpec] = &[ParamSpec {
    name: "path",
    kind: ParamKind::Path("vizkeys"),
    required: false,
    help: "The key bindings file (answers the dialog).",
}];

/// `c` without a menu item: keys, the Keys settings and the channel only.
const fn key_only(c: CommandSpec) -> CommandSpec {
    CommandSpec {
        menu_item: false,
        ..c
    }
}

/// A numbered locator's command (plan 4f Ruling 7).
const fn locator(id: &'static str, title: &'static str, key: &'static str) -> CommandSpec {
    key_only(cmd(id, "Locators", title, Some(key), true))
}

const ON: &[ParamSpec] = &[ParamSpec {
    name: "on",
    kind: ParamKind::Flag,
```

Find:

```rust
        Some("⌥↓"),
        true,
    ),
    cmd(
        "modify.select_by_rule",
        "Modify",
```

Replace with:

```rust
        Some("⌥↓"),
        true,
    ),
    CommandSpec {
        params: ON,
        ..cmd("editors.snap", "Editors", "Snap to Grid", Some("G"), true)
    },
    cmd(
        "modify.select_by_rule",
        "Modify",
```

Find:

```rust
        Some("Space"),
        true,
    ),
    CommandSpec {
        params: ON,
        ..cmd("transport.loop", "Transport", "Loop", None, true)
    },
    cmd(
        "transport.return_to_start",
        "Transport",
```

Replace with:

```rust
        Some("Space"),
        true,
    ),
    cmd("transport.stop", "Transport", "Stop", Some("S"), true),
    cmd(
        "transport.play_selection",
        "Transport",
        "Play Selection",
        Some("⌥Space"),
        true,
    ),
    cmd(
        "transport.play_from_selection",
        "Transport",
        "Play from Selection",
        Some("⇧Space"),
        true,
    ),
    cmd(
        "transport.play_from_edit_in",
        "Transport",
        "Play from Edit-In",
        Some("Y"),
        true,
    ),
    cmd("transport.record", "Transport", "Record", Some("R"), true),
    cmd("transport.scrub", "Transport", "Scrub", Some("K"), true),
    cmd(
        "transport.return_to_start",
        "Transport",
```

Find:

```rust
        Some("Return"),
        true,
    ),
];

/// The command with `id`.
```

Replace with:

```rust
        Some("Return"),
        true,
    ),
    cmd(
        "transport.previous_marker",
        "Transport",
        "Previous Marker",
        Some(","),
        true,
    ),
    cmd(
        "transport.next_marker",
        "Transport",
        "Next Marker",
        Some("."),
        true,
    ),
    CommandSpec {
        params: ON,
        ..cmd("transport.loop", "Transport", "Loop", Some("L"), true)
    },
    CommandSpec {
        params: ON,
        ..cmd("transport.overdub", "Transport", "Overdub", Some("O"), true)
    },
    CommandSpec {
        params: ON,
        ..cmd("transport.punch", "Transport", "Punch", Some("I"), true)
    },
    cmd(
        "transport.arm_previous",
        "Transport",
        "Arm Previous Lane",
        Some("⌥⇧↑"),
        true,
    ),
    cmd(
        "transport.arm_next",
        "Transport",
        "Arm Next Lane",
        Some("⌥⇧↓"),
        true,
    ),
    key_only(cmd(
        "record.keep_pass",
        "Record",
        "Keep Loop Pass",
        None,
        true,
    )),
    key_only(cmd(
        "record.discard_pass",
        "Record",
        "Discard Since Last Keep",
        None,
        true,
    )),
    locator("locator.go_0", "Go to Locator 0", "0"),
    locator("locator.go_1", "Go to Locator 1", "1"),
    locator("locator.go_2", "Go to Locator 2", "2"),
    locator("locator.go_3", "Go to Locator 3", "3"),
    locator("locator.go_4", "Go to Locator 4", "4"),
    locator("locator.go_5", "Go to Locator 5", "5"),
    locator("locator.go_6", "Go to Locator 6", "6"),
    locator("locator.go_7", "Go to Locator 7", "7"),
    locator("locator.go_8", "Go to Locator 8", "8"),
    locator("locator.go_9", "Go to Locator 9", "9"),
    locator("locator.set_0", "Set Locator 0", "⌥0"),
    locator("locator.set_1", "Set Locator 1", "⌥1"),
    locator("locator.set_2", "Set Locator 2", "⌥2"),
    locator("locator.set_3", "Set Locator 3", "⌥3"),
    locator("locator.set_4", "Set Locator 4", "⌥4"),
    locator("locator.set_5", "Set Locator 5", "⌥5"),
    locator("locator.set_6", "Set Locator 6", "⌥6"),
    locator("locator.set_7", "Set Locator 7", "⌥7"),
    locator("locator.set_8", "Set Locator 8", "⌥8"),
    locator("locator.set_9", "Set Locator 9", "⌥9"),
    locator("locator.clear_0", "Clear Locator 0", "⌥⇧0"),
    locator("locator.clear_1", "Clear Locator 1", "⌥⇧1"),
    locator("locator.clear_2", "Clear Locator 2", "⌥⇧2"),
    locator("locator.clear_3", "Clear Locator 3", "⌥⇧3"),
    locator("locator.clear_4", "Clear Locator 4", "⌥⇧4"),
    locator("locator.clear_5", "Clear Locator 5", "⌥⇧5"),
    locator("locator.clear_6", "Clear Locator 6", "⌥⇧6"),
    locator("locator.clear_7", "Clear Locator 7", "⌥⇧7"),
    locator("locator.clear_8", "Clear Locator 8", "⌥⇧8"),
    locator("locator.clear_9", "Clear Locator 9", "⌥⇧9"),
    CommandSpec {
        params: KEYS_PATH,
        dialogs: &[DialogSpec {
            name: "Save",
            when: "always",
            always: true,
            answered_by: &["path"],
        }],
        ..key_only(cmd(
            "keys.save",
            "Key Bindings",
            "Save Key Bindings…",
            None,
            false,
        ))
    },
    CommandSpec {
        params: KEYS_PATH,
        dialogs: &[DialogSpec {
            name: "Open",
            when: "always",
            always: true,
            answered_by: &["path"],
        }],
        ..key_only(cmd(
            "keys.load",
            "Key Bindings",
            "Load Key Bindings…",
            None,
            false,
        ))
    },
    key_only(cmd(
        "keys.reset",
        "Key Bindings",
        "Reset Key Bindings",
        None,
        false,
    )),
];

/// The command with `id`.
```

- [ ] **Step 4: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-control --test commands 2>&1 | grep -E "^test result|FAILED|panicked"
scripts/build-ffi.sh && scripts/swift.sh test --filter 'TransportCommandsTests|ControlCommandsTests' 2>&1 | tail -1
```

Expected: `commands` 14 passed; 25 Swift tests in 2 suites pass, including `everyRegisteredMenuCommandHasAHandler` (Rust's 92 IDs equal `handled`).

- [ ] **Step 5: Gate and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace 2>&1 | grep -E "FAILED|panicked" ; cargo test --workspace -- --list 2>/dev/null | grep -c ': test$'
scripts/build-ffi.sh && scripts/swift.sh build 2>&1 | tail -1 && scripts/swift.sh test 2>&1 | tail -1
git add app/Sources/VizDAW/ControlChannel.swift \
  app/Sources/VizDAW/VizDAWApp.swift \
  app/Sources/VizDAWAppModel/ControlCommands.swift \
  app/Sources/VizDAWAppModel/DocumentModel.swift \
  app/Sources/VizDAWAppModel/EditorModel.swift \
  app/Sources/VizDAWAppModel/TransportCommands.swift \
  app/Tests/VizDAWAppModelTests/ControlCommandsTests.swift \
  app/Tests/VizDAWAppModelTests/TransportCommandsTests.swift \
  crates/core-control/src/registry.rs \
  crates/core-control/tests/commands.rs
git commit -m "feat(keys): transport, locator, mode and key-binding commands

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
git log -1 --format='%h %s'
```

Expected: 903 Rust tests listed (902 + 1); Swift 632 (623 + 9). No `FAILED` or `panicked` line (rerun once if the known-flaky MIDI listener test trips, and say so).

---

### Task 8: Step entry in the app — its model, the Step menu's commands, MIDI notes and keyboard slots

**Files:**
- Create: `app/Sources/VizDAWAppModel/StepEntry.swift`
- Modify: `app/Sources/VizDAW/SequenceSidebar.swift`, `app/Sources/VizDAW/VizDAWApp.swift`, `app/Sources/VizDAWAppModel/ControlCommands.swift`, `app/Sources/VizDAWAppModel/DocumentModel+Sequences.swift`, `app/Sources/VizDAWAppModel/DocumentModel.swift`, `app/Sources/VizDAWAppModel/DocumentStore.swift`, `crates/core-control/src/registry.rs`
- Test: `app/Tests/VizDAWAppModelTests/StepEntryTests.swift`, `crates/core-control/tests/commands.rs`

**Interfaces:**
- Consumes: Task 5's `Session.stepEntry`, `stepStateNew`, `stepParsePitches`, `StepInputRow`, `NoteInRow.on`; Task 6's `KeyBindings.slotClash`; Task 7's `DocumentModel.{startPlaying, moveTo}`, `KeyOutcome`; `NoteInputHub.poll`, `DocumentModel.{receiveNotes, listensForNotes, selectedLane, choose, commitSlot}`.
- Produces: registry IDs `step.entry` (T, `on`; global), `step.{whole 1, half 2, quarter 3, eighth 4, sixteenth 5, thirty_second 6, dot ., triplet /, tie =, rest 0, delete_last Delete}` (step-entry context, keypad twins), `step.notes` (`pitches`, required; no menu item); count 92 → 105. Swift: `StepEntryModel` (`typedVelocity`, `state: StepStateRow?`, `target: LaneRef?`, `isOn`, `lane(_:)`, `start() -> KeyOutcome`, `stop()`, `input(_:) -> KeyOutcome`, `receive(_:)`, `enter(_:)`, `summary`, `title(_:)`, `playingMessage`, `offMessage`); `StepCommands.{entry, notes, ids, enabled(_:_:), isOn(_:_:), run(_:on:pitches:_:)}`; `DocumentModel.{stepEntry, keys, pressSlot(_:) -> KeyOutcome}`; `KeySlotDraft.problem`; `DocumentStore.keyBindings`; `FakeSession` answers a value key by setting the value.

Rulings 11–15. Played notes go to step entry first while it is on (the hub keeps listening); a chord comes from Rust when the last key is released. Keyboard slots: a slot press during step entry enters a reference; otherwise the sequence becomes the shown one and plays from its start. The slot sheet shows why a key is refused.

- [ ] **Step 1: Write the failing tests**

Create `app/Tests/VizDAWAppModelTests/StepEntryTests.swift`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p app/Tests/VizDAWAppModelTests
cat > app/Tests/VizDAWAppModelTests/StepEntryTests.swift <<'VIZDAW_EOF'
import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

/// Step entry in the app (plan 4f Task 8): turning it on, its keys, MIDI
/// notes, keyboard slots and their clashes.
@MainActor
@Suite struct StepEntryTests {
    let host = FakeControlHost()

    func demo() throws -> (DocumentStore, UUID, DocumentModel, FakeSession) {
        let f = demoFake()
        let s = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { f }, open: { _ in demoFake() })
        let id = try #require(s.newDemo())
        return (s, id, try #require(s.model(id)), f)
    }

    func run(_ id: String, _ args: [ControlArg], _ m: DocumentModel, _ s: DocumentStore, _ doc: UUID)
        -> ControlFailure?
    {
        if case .failed(let f) = ControlCommands.run(id, args: args, doc: (doc, m), store: s, host: host) { return f }
        return nil
    }

    func run(_ id: String, _ m: DocumentModel, _ s: DocumentStore, _ doc: UUID) -> ControlFailure? {
        run(id, [], m, s, doc)
    }

    @Test func stepEntryStartsOnTheSelectedLaneAtThePosition() throws {
        let (s, id, m, f) = try demo()
        f.state.tick = 61_440
        #expect(run("step.entry", m, s, id) == nil)
        #expect(m.stepEntry.isOn && m.stepEntry.target == LaneRef(seq: 3, lane: 30), "the first lane")
        #expect(m.stepEntry.state?.tick == 61_440 && m.stepEntry.state?.value == .quarter)
        #expect(m.stepEntry.summary == "Step Entry · Groove · 1/4")
        _ = run("step.entry", m, s, id)
        #expect(!m.stepEntry.isOn, "toggled off")
        m.editor.setSelection(SelectionRow(timeRange: nil, lanes: [LaneRef(seq: 3, lane: 31)], events: []))
        _ = run("step.entry", [ControlArg(name: "on", value: .flag(value: true))], m, s, id)
        #expect(m.stepEntry.target == LaneRef(seq: 3, lane: 31), "the selected lane")
        m.stepEntry.stop()
        m.playStop()
        #expect(run("step.entry", m, s, id)?.message == StepEntryModel.playingMessage)
    }

    @Test func stepKeysSendInputsAndStepsMoveTheTransport() throws {
        let (s, id, m, f) = try demo()
        #expect(run("step.rest", m, s, id) == .of(.unavailable, StepEntryModel.offMessage))
        _ = run("step.entry", m, s, id)
        _ = run("step.eighth", m, s, id)
        #expect(f.stepCalls.last?.input == .value(value: .eighth))
        #expect(f.state.tick == 0, "a value is no step")
        _ = run("step.rest", m, s, id)
        #expect(f.stepCalls.last?.input == .rest && f.state.tick == 15_360, "the transport follows")
        let version = m.snapshot.version
        _ = run("step.notes", [ControlArg(name: "pitches", value: .text(value: "C4 E4"))], m, s, id)
        let typed = StepEntryModel.typedVelocity
        #expect(
            f.stepCalls.last?.input
                == .notes(notes: [StepNoteRow(pitch: 60, velocity: typed), StepNoteRow(pitch: 64, velocity: typed)]))
        #expect(m.snapshot.version > version, "the new document is applied")
        #expect(f.stepCalls.last?.state.tick == 15_360, "each input starts where the transport is")
        f.state.tick = 0
        _ = run("step.tie", m, s, id)
        #expect(f.stepCalls.last?.state.tick == 0, "moving the transport moves the next step")
        let e = run("step.notes", [ControlArg(name: "pitches", value: .text(value: "H9"))], m, s, id)
        #expect(e?.code == .failed && e?.message.contains("not a pitch") == true)
        for (cmd, input) in [
            ("step.dot", StepInputRow.dot), ("step.triplet", .triplet), ("step.delete_last", .deleteLast),
            ("step.whole", .value(value: .whole)), ("step.thirty_second", .value(value: .thirtySecond)),
        ] {
            _ = run(cmd, m, s, id)
            #expect(f.stepCalls.last?.input == input, "\(cmd)")
        }
    }

    @Test func aRefusedStepShowsInTheStatusLine() throws {
        let (s, id, m, f) = try demo()
        _ = run("step.entry", m, s, id)
        f.stepEntryError = .Model(message: "a reference cannot contain itself")
        #expect(run("step.rest", m, s, id) == .of(.failed, "a reference cannot contain itself"))
        #expect(m.editor.status == "a reference cannot contain itself")
        #expect(m.stepEntry.state?.tick == 0, "the state stays")
    }

    @Test func playedNotesGoToStepEntryFirst() throws {
        let (s, id, m, f) = try demo()
        #expect(!m.listensForNotes)
        _ = run("step.entry", m, s, id)
        #expect(m.listensForNotes)
        let input = FakeNoteInput()
        let hub = NoteInputHub(input: input)
        hub.poll([m], focused: m)
        #expect(input.starts == 1)
        input.queued = [NoteInRow(pitch: 60, velocity: 0x8000, channel: 0), NoteInRow(pitch: 60, velocity: 0, channel: 0, on: false)]
        hub.poll([m], focused: m)
        #expect(f.stepCalls.suffix(2).map(\.input) == [.noteOn(note: StepNoteRow(pitch: 60, velocity: 0x8000)), .noteOff(pitch: 60)])
        _ = run("step.entry", m, s, id)
        hub.poll([m], focused: m)
        #expect(input.stops == 1, "nobody listens any more")
    }

    @Test func keyboardSlotsPlayTheirSequenceOrEnterAReference() throws {
        let (s, id, m, f) = try demo()
        #expect(m.pressSlot(1) == .done)
        #expect(m.selectedSeq == 1 && f.playsFrom.last?.seq == 1 && f.playsFrom.last?.from == 0)
        m.playStop()
        m.choose(3)
        _ = run("step.entry", m, s, id)
        #expect(m.pressSlot(1) == .done)
        #expect(f.stepCalls.last?.input == .reference(seq: 1) && m.selectedSeq == 3, "entered, not played")
    }

    @Test func aSlotThatIsACommandsKeyIsRefused() throws {
        let (_, _, m, f) = try demo()
        m.keys = KeyBindings(defaults: MemoryDefaults())
        m.beginSlot(3)
        m.slotDraft?.key = "s"
        m.slotDraft?.command = true
        m.commitSlot()
        #expect(m.slotDraft?.problem == "⌘S is the key of “Save”.")
        #expect(f.edits.isEmpty, "nothing set")
        m.slotDraft?.command = false
        m.slotDraft?.control = true
        m.commitSlot()
        #expect(m.slotDraft == nil)
        #expect(f.edits.last == .sequence(edit: .setKeyboardSlot(seq: 3, slot: KeySlotRow(key: "s", modifiers: 2))))
    }

    @Test func checkMarksFollowTheState() throws {
        let (s, id, m, _) = try demo()
        #expect(!StepCommands.isOn("step.entry", m) && !StepCommands.enabled("step.rest", m))
        #expect(StepCommands.enabled("step.entry", m))
        _ = run("step.entry", m, s, id)
        #expect(StepCommands.isOn("step.quarter", m) && !StepCommands.isOn("step.eighth", m))
        #expect(StepCommands.enabled("step.rest", m) && !StepCommands.isOn("step.dot", m))
    }

    @Test func stepKeysWinOnlyDuringStepEntry() {
        let keys = KeyBindings(defaults: MemoryDefaults())
        func route(_ code: UInt16, step: Bool) -> KeyRoute {
            KeyRouter.route(
                keyCode: code, modifiers: noMods, typing: false, stepEntry: step, menus: true, keys: keys, sequences: [])
        }
        #expect(route(18, step: false) == .command("locator.go_1"))
        #expect(route(18, step: true) == .command("step.whole"))
        #expect(route(83, step: true) == .command("step.whole"), "the keypad")
        keys.reserveNumberRow = true
        #expect(route(18, step: true) == .command("locator.go_1"))
        #expect(route(83, step: true) == .command("step.whole"))
    }
}
VIZDAW_EOF
```

In `crates/core-control/tests/commands.rs`:

Find:

```rust
    );
}

#[test]
fn the_registry_has_unique_ids_and_answers_dialogs_by_parameter() {
    let ids: std::collections::BTreeSet<_> = COMMANDS.iter().map(|c| c.id).collect();
    assert_eq!(ids.len(), COMMANDS.len(), "IDs are unique");
    assert_eq!(
        COMMANDS.len(),
        92,
        "44 before plan 4f, plus 48 transport, locator, record and key-binding commands"
    );
    for c in COMMANDS {
        // snake_case words (a locator's number too), a dot after the group.
```

Replace with:

```rust
    );
}

#[test]
fn step_entry_keys_are_live_only_in_step_entry() {
    use core_control::bindings::{Chord, Keymap};
    use core_control::registry::KeyContext;
    let k = Keymap::default();
    let key =
        |t: &str, step: bool, reserve: bool| k.lookup(Chord::parse(t).unwrap(), step, reserve);
    assert_eq!(key("T", false, false), Some("step.entry"));
    for (text, id) in [
        ("1", "step.whole"),
        ("Keypad1", "step.whole"),
        ("2", "step.half"),
        ("3", "step.quarter"),
        ("4", "step.eighth"),
        ("5", "step.sixteenth"),
        ("6", "step.thirty_second"),
        (".", "step.dot"),
        ("Keypad.", "step.dot"),
        ("/", "step.triplet"),
        ("=", "step.tie"),
        ("Keypad+", "step.tie"),
        ("0", "step.rest"),
        ("Delete", "step.delete_last"),
    ] {
        assert_eq!(key(text, true, false), Some(id), "{text}");
    }
    assert_eq!(
        key("1", false, false),
        Some("locator.go_1"),
        "outside step entry"
    );
    assert_eq!(key(".", false, false), Some("transport.next_marker"));
    assert_eq!(key("Delete", false, false), None, "the editors' own key");
    assert_eq!(
        key("1", true, true),
        Some("locator.go_1"),
        "the row reserved"
    );
    assert_eq!(key("Keypad1", true, true), Some("step.whole"));
    let step: Vec<&str> = COMMANDS
        .iter()
        .filter(|c| c.context == KeyContext::StepEntry)
        .map(|c| c.id)
        .collect();
    assert_eq!(step.len(), 11);
    assert_eq!(
        err("command.run", json!({"id": "step.notes"})).0,
        ErrorCode::InvalidParams,
        "the pitches are needed"
    );
    assert_eq!(
        ok(
            "command.run",
            json!({"id": "step.notes", "params": {"pitches": "C4 E4"}})
        ),
        Command::CommandRun {
            id: "step.notes".into(),
            args: vec![("pitches".into(), ArgValue::Text("C4 E4".into()))],
            window: None,
        }
    );
}

#[test]
fn the_registry_has_unique_ids_and_answers_dialogs_by_parameter() {
    let ids: std::collections::BTreeSet<_> = COMMANDS.iter().map(|c| c.id).collect();
    assert_eq!(ids.len(), COMMANDS.len(), "IDs are unique");
    assert_eq!(
        COMMANDS.len(),
        105,
        "44 before plan 4f, plus 48 transport, locator, record and key-binding commands, 13 step entry"
    );
    for c in COMMANDS {
        // snake_case words (a locator's number too), a dot after the group.
```

Find:

```rust
    assert_eq!(
        stroke("x", Modifiers::default()),
        None,
        "a plain letter is typing"
    );
    for c in COMMANDS {
        if let Some(s) = c.shortcut {
            let k = registry::shortcut_stroke(s).unwrap_or_else(|| panic!("{}: {s}", c.id));
            assert_eq!(
```

Replace with:

```rust
    assert_eq!(
        stroke("x", Modifiers::default()),
        None,
        "an unbound letter is nothing"
    );
    // Global shortcuts name their commands; step entry's keys are not
    // global (plan 4f: they share keys with the locators).
    for c in COMMANDS
        .iter()
        .filter(|c| c.context == core_control::registry::KeyContext::Global)
    {
        if let Some(s) = c.shortcut {
            let k = registry::shortcut_stroke(s).unwrap_or_else(|| panic!("{}: {s}", c.id));
            assert_eq!(
```

Find:

```rust
            Just("max_depth"),
            Just("on"),
            Just("name"),
        ]
        .prop_map(str::to_string)
    }
```

Replace with:

```rust
            Just("max_depth"),
            Just("on"),
            Just("name"),
            Just("pitches"),
        ]
        .prop_map(str::to_string)
    }
```

- [ ] **Step 2: Run them to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-control --test commands 2>&1 | grep -E "panicked|test result" | head -3
scripts/swift.sh test --filter StepEntryTests 2>&1 | grep -E "error:" | head -3
```

Expected: FAIL: `step_entry_keys_are_live_only_in_step_entry` (T is nothing) and the count (92, expected 105); Swift: `value of type 'DocumentModel' has no member 'stepEntry'`.

- [ ] **Step 3: Implement**

In `app/Sources/VizDAW/SequenceSidebar.swift`:

Find:

```swift
                flag("⇧", \.shift)
                flag("⌘", \.command)
            }
            HStack {
                Spacer()
                Button("Cancel") { model.slotDraft = nil }.keyboardShortcut(.cancelAction)
```

Replace with:

```swift
                flag("⇧", \.shift)
                flag("⌘", \.command)
            }
            if let problem = model.slotDraft?.problem {
                Text(problem).font(.caption).foregroundStyle(.red)
            }
            HStack {
                Spacer()
                Button("Cancel") { model.slotDraft = nil }.keyboardShortcut(.cancelAction)
```

In `app/Sources/VizDAW/VizDAWApp.swift`:

Find:

```swift
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var delegate

    init() {
        appStore.willClose = {
            PluginWindows.closeDocument($0)
            EditorWindows.closeDocument($0)
```

Replace with:

```swift
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var delegate

    init() {
        appStore.keyBindings = keyBindings
        appStore.willClose = {
            PluginWindows.closeDocument($0)
            EditorWindows.closeDocument($0)
```

In `app/Sources/VizDAWAppModel/ControlCommands.swift`:

Find:

```swift
    /// Every registry ID `run` handles (a test checks it is all of them).
    public static var handled: Set<String> {
        other.union(detail.keys).union(torn.keys).union(nudges.keys).union(panels.keys).union(DrumCommands.ids)
            .union(TransportCommands.ids)
    }

    /// Runs command `id` with `args` on `doc` (the target window's document,
```

Replace with:

```swift
    /// Every registry ID `run` handles (a test checks it is all of them).
    public static var handled: Set<String> {
        other.union(detail.keys).union(torn.keys).union(nudges.keys).union(panels.keys).union(DrumCommands.ids)
            .union(TransportCommands.ids).union(StepCommands.ids)
    }

    /// Runs command `id` with `args` on `doc` (the target window's document,
```

Find:

```swift
            if let f = failure(DrumCommands.run(id, on: args.flag("on"), m)) { return f }
        case _ where TransportCommands.ids.contains(id):
            if let f = failure(TransportCommands.run(id, on: args.flag("on"), m)) { return f }
        default:
            return .failed(.of(.unknownId, "VizDAW has no handler for \(id)."))
        }
```

Replace with:

```swift
            if let f = failure(DrumCommands.run(id, on: args.flag("on"), m)) { return f }
        case _ where TransportCommands.ids.contains(id):
            if let f = failure(TransportCommands.run(id, on: args.flag("on"), m)) { return f }
        case _ where StepCommands.ids.contains(id):
            if let f = failure(StepCommands.run(id, on: args.flag("on"), pitches: args.text("pitches"), m)) {
                return f
            }
        default:
            return .failed(.of(.unknownId, "VizDAW has no handler for \(id)."))
        }
```

In `app/Sources/VizDAWAppModel/DocumentModel+Sequences.swift`:

Find:

```swift
    public var control = false
    public var option = false
    public var command = false
    public var id: UInt64 { seq }

    var slot: KeySlotRow? {
```

Replace with:

```swift
    public var control = false
    public var option = false
    public var command = false
    /// Why the last Set was refused (the key is a command's, plan 4f
    /// Ruling 15).
    public var problem: String?
    public var id: UInt64 { seq }

    var slot: KeySlotRow? {
```

Find:

```swift
        slotDraft = d
    }

    public func commitSlot() {
        guard let d = slotDraft else { return }
        slotDraft = nil
        editor.edit(.sequence(edit: .setKeyboardSlot(seq: d.seq, slot: d.slot)))
    }

    /// Deletes an unused sequence at once; a used one waits for
    /// `confirmDelete` (its references go too).
    public func requestDelete(_ seq: UInt64) {
```

Replace with:

```swift
        slotDraft = d
    }

    /// Sets the edited slot; a key that is a command's is refused and the
    /// draft stays open saying why (parent spec §5).
    public func commitSlot() {
        guard var d = slotDraft else { return }
        if let slot = d.slot, let clash = keys?.slotClash(slot) {
            d.problem = "\(KeySlotFormat.text(slot)) is the key of “\(clash)”."
            slotDraft = d
            return
        }
        slotDraft = nil
        editor.edit(.sequence(edit: .setKeyboardSlot(seq: d.seq, slot: d.slot)))
    }

    /// A sequence's keyboard slot was pressed (parent spec §5): during
    /// step entry it enters a reference to the sequence and moves on by
    /// its length; otherwise the sequence becomes the shown one and plays
    /// from its start.
    @discardableResult
    public func pressSlot(_ seq: UInt64) -> KeyOutcome {
        if stepEntry.isOn { return stepEntry.input(.reference(seq: seq)) }
        choose(seq)
        do {
            try startPlaying(from: 0)
            return .done
        } catch {
            return .refused(AlertItem(error, title: "").message)
        }
    }

    /// Deletes an unused sequence at once; a used one waits for
    /// `confirmDelete` (its references go too).
    public func requestDelete(_ seq: UInt64) {
```

In `app/Sources/VizDAWAppModel/DocumentModel.swift`:

Find:

```swift
    public let editor: EditorModel
    /// The select-and-modify panel (plan 4c).
    public let selectModify = SelectModifyModel()
    public private(set) var selectedSeq: UInt64?
    public private(set) var lanes: [LaneRow] = []
    public private(set) var transport: TransportState
```

Replace with:

```swift
    public let editor: EditorModel
    /// The select-and-modify panel (plan 4c).
    public let selectModify = SelectModifyModel()
    /// Step entry (plan 4f).
    public let stepEntry = StepEntryModel()
    /// The app's key bindings (set by `DocumentStore`; keyboard slots are
    /// checked against them, plan 4f Ruling 15).
    @ObservationIgnored public var keys: KeyBindings?
    public private(set) var selectedSeq: UInt64?
    public private(set) var lanes: [LaneRow] = []
    public private(set) var transport: TransportState
```

Find:

```swift
        return l
    }

    /// A list listens while MIDI In is on and its slot still shows a list.
    public var listensForNotes: Bool { lists.values.contains { $0.listens } }

    /// Played notes go to the active list when it listens, else the pane's,
    /// else the first listening window's (by ID, so the choice is stable).
    public func receiveNotes(_ notes: [NoteInRow]) {
        var order: [DetailSlot] = []
        if let a = activeList { order.append(a) }
        order.append(.pane)
```

Replace with:

```swift
        return l
    }

    /// Step entry is on, or a list listens (MIDI In on, its slot still
    /// showing a list).
    public var listensForNotes: Bool { stepEntry.isOn || lists.values.contains { $0.listens } }

    /// Played notes go to step entry while it is on (plan 4f Ruling 13);
    /// else to the active list when it listens, else the pane's, else the
    /// first listening window's (by ID, so the choice is stable).
    public func receiveNotes(_ notes: [NoteInRow]) {
        if stepEntry.isOn {
            stepEntry.receive(notes)
            return
        }
        var order: [DetailSlot] = []
        if let a = activeList { order.append(a) }
        order.append(.pane)
```

Find:

```swift
        editor.selectSequence = { [weak self] in self?.select($0) }
        editor.currentSequence = { [weak self] in self?.selectedSeq }
        selectModify.document = self
        editor.onExact = { [weak self] r in self?.selectModify.open(r) }
        editor.layoutChanged = { [weak self] in
            self?.pruneLists()
```

Replace with:

```swift
        editor.selectSequence = { [weak self] in self?.select($0) }
        editor.currentSequence = { [weak self] in self?.selectedSeq }
        selectModify.document = self
        stepEntry.document = self
        editor.onExact = { [weak self] r in self?.selectModify.open(r) }
        editor.layoutChanged = { [weak self] in
            self?.pruneLists()
```

In `app/Sources/VizDAWAppModel/DocumentStore.swift`:

Find:

```swift

    /// List defaults and columns given to every document (Plan 4c Ruling 19).
    public var listPreferences = ListPreferences()

    public func newDemoAudio() -> UUID? {
        guard let make = makeDemoAudio else { return nil }
```

Replace with:

```swift

    /// List defaults and columns given to every document (Plan 4c Ruling 19).
    public var listPreferences = ListPreferences()
    /// The app's key bindings, handed to every document (plan 4f).
    public var keyBindings: KeyBindings?

    public func newDemoAudio() -> UUID? {
        guard let make = makeDemoAudio else { return nil }
```

Find:

```swift
        do {
            let model = try DocumentModel(session: make())
            model.listPreferences = listPreferences
            let id = UUID()
            documents[id] = model
            return id
```

Replace with:

```swift
        do {
            let model = try DocumentModel(session: make())
            model.listPreferences = listPreferences
            model.keys = keyBindings
            let id = UUID()
            documents[id] = model
            return id
```

Create `app/Sources/VizDAWAppModel/StepEntry.swift`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p app/Sources/VizDAWAppModel
cat > app/Sources/VizDAWAppModel/StepEntry.swift <<'VIZDAW_EOF'
import Foundation
import Observation
import VizDAWCore

/// Step entry for one document (editors spec §4f, plan 4f Rulings 11–15):
/// on or off, the lane it enters into and the state Rust's state machine
/// hands back. The step position is the transport's: steps locate it as
/// they go, and moving the transport moves where the next step goes.
@Observable
@MainActor
public final class StepEntryModel {
    /// The velocity of typed notes (a played note keeps its own): 100 as
    /// 16 bits.
    public static let typedVelocity: UInt16 = 100 << 9

    @ObservationIgnored weak var document: DocumentModel?
    /// The state; nil while step entry is off.
    public private(set) var state: StepStateRow?
    /// The sequence and lane steps go into.
    public private(set) var target: LaneRef?

    public var isOn: Bool { state != nil }

    /// The lane step entry starts on: the selection's first lane of the
    /// shown sequence, else its first lane.
    public static func lane(_ m: DocumentModel) -> LaneRef? {
        guard let seq = m.selectedSeq else { return nil }
        if let l = m.selectedLane { return LaneRef(seq: seq, lane: l.id) }
        return m.lanes.first.map { LaneRef(seq: seq, lane: $0.id) }
    }

    /// Turns step entry on at the transport's position.
    public func start() -> KeyOutcome {
        guard let m = document else { return .unavailable("No document.") }
        guard !m.isPlaying else { return .unavailable(Self.playingMessage) }
        guard let t = Self.lane(m) else { return .unavailable("Select a sequence with a lane first.") }
        m.poll()
        target = t
        state = stepStateNew(tick: m.transport.tick)
        return .done
    }

    public func stop() {
        state = nil
        target = nil
    }

    static let playingMessage = "Stop playing to enter steps."
    static let offMessage = "Step entry is off (Step > Step Entry)."

    /// One input; a step moves the transport to the next position.
    @discardableResult
    public func input(_ i: StepInputRow) -> KeyOutcome {
        guard let m = document, var s = state, let t = target else { return .unavailable(Self.offMessage) }
        guard !m.isPlaying else { return .unavailable(Self.playingMessage) }
        m.poll()
        s.tick = m.transport.tick
        do {
            let r = try m.session.stepEntry(seq: t.seq, lane: t.lane, state: s, input: i)
            state = r.state
            if let snap = r.snapshot {
                m.apply(snap)
                m.editor.refreshSelection()
            }
            if r.moved { try m.moveTo(r.state.tick) }
            return .done
        } catch {
            let message = AlertItem(error, title: "").message
            m.editor.status = message
            return .refused(message)
        }
    }

    /// Notes played on a MIDI source: each key down and up in order.
    public func receive(_ notes: [NoteInRow]) {
        for n in notes {
            input(n.on ? .noteOn(note: StepNoteRow(pitch: n.pitch, velocity: n.velocity)) : .noteOff(pitch: n.pitch))
        }
    }

    /// Typed pitches as one chord at the typed velocity.
    @discardableResult
    public func enter(_ pitches: [UInt8]) -> KeyOutcome {
        input(.notes(notes: pitches.map { StepNoteRow(pitch: $0, velocity: Self.typedVelocity) }))
    }

    /// The step bar's line: "Step Entry · Chords · 1/8 dotted".
    public var summary: String? {
        guard let s = state, let t = target, let m = document else { return nil }
        let lane = m.lanes.first { $0.id == t.lane }?.name ?? "Lane \(t.lane)"
        var v = Self.title(s.value)
        if s.dotted { v += " dotted" }
        if s.triplet { v += " triplet" }
        return "Step Entry · \(lane) · \(v)"
    }

    public static func title(_ v: NoteValueRow) -> String {
        switch v {
        case .whole: "1/1"
        case .half: "1/2"
        case .quarter: "1/4"
        case .eighth: "1/8"
        case .sixteenth: "1/16"
        case .thirtySecond: "1/32"
        }
    }
}

/// The Step menu and step entry's keys (plan 4f Ruling 12): one
/// implementation for the menu items, the keys and `command.run`.
@MainActor
public enum StepCommands {
    public static let entry = "step.entry"
    public static let notes = "step.notes"
    static let values: [String: NoteValueRow] = [
        "step.whole": .whole, "step.half": .half, "step.quarter": .quarter, "step.eighth": .eighth,
        "step.sixteenth": .sixteenth, "step.thirty_second": .thirtySecond,
    ]
    static let inputs: [String: StepInputRow] = [
        "step.dot": .dot, "step.triplet": .triplet, "step.tie": .tie, "step.rest": .rest,
        "step.delete_last": .deleteLast,
    ]

    public static var ids: Set<String> { Set(values.keys).union(inputs.keys).union([entry, notes]) }

    public static func enabled(_ id: String, _ m: DocumentModel) -> Bool {
        id == entry ? m.stepEntry.isOn || (m.selectedSeq != nil && !m.isPlaying) : m.stepEntry.isOn
    }

    /// A menu item's check mark: step entry, the chosen value, dot, triplet.
    public static func isOn(_ id: String, _ m: DocumentModel) -> Bool {
        guard let s = m.stepEntry.state else { return false }
        switch id {
        case entry: return true
        case "step.dot": return s.dotted
        case "step.triplet": return s.triplet
        default: return values[id] == s.value
        }
    }

    /// Runs `id` on `m`; `on` sets step entry (nil flips it); `pitches`
    /// is step.notes' text.
    @discardableResult
    public static func run(_ id: String, on: Bool? = nil, pitches: String? = nil, _ m: DocumentModel) -> KeyOutcome {
        let step = m.stepEntry
        if id == entry {
            let want = on ?? !step.isOn
            guard want != step.isOn else { return .done }
            if want { return step.start() }
            step.stop()
            return .done
        }
        guard step.isOn else { return .unavailable(StepEntryModel.offMessage) }
        if id == notes {
            do {
                return step.enter(Array(try stepParsePitches(text: pitches ?? "")))
            } catch {
                return .refused(AlertItem(error, title: "").message)
            }
        }
        if let v = values[id] { return step.input(.value(value: v)) }
        if let i = inputs[id] { return step.input(i) }
        return .unavailable("VizDAW has no handler for \(id).")
    }
}
VIZDAW_EOF
```

In `crates/core-control/src/registry.rs`:

Find:

```rust
    }
}

/// A numbered locator's command (plan 4f Ruling 7).
const fn locator(id: &'static str, title: &'static str, key: &'static str) -> CommandSpec {
    key_only(cmd(id, "Locators", title, Some(key), true))
```

Replace with:

```rust
    }
}

/// A step entry key's command (plan 4f Ruling 12): live only during step
/// entry, with a keypad twin.
const fn step(
    id: &'static str,
    title: &'static str,
    key: &'static str,
    keypad: &'static [&'static str],
) -> CommandSpec {
    CommandSpec {
        extra_keys: keypad,
        context: KeyContext::StepEntry,
        ..cmd(id, "Step", title, Some(key), true)
    }
}

/// A numbered locator's command (plan 4f Ruling 7).
const fn locator(id: &'static str, title: &'static str, key: &'static str) -> CommandSpec {
    key_only(cmd(id, "Locators", title, Some(key), true))
```

Find:

```rust
        Some("⌥⇧↓"),
        true,
    ),
    key_only(cmd(
        "record.keep_pass",
        "Record",
```

Replace with:

```rust
        Some("⌥⇧↓"),
        true,
    ),
    CommandSpec {
        params: ON,
        ..cmd("step.entry", "Step", "Step Entry", Some("T"), true)
    },
    step("step.whole", "Whole Note", "1", &["Keypad1"]),
    step("step.half", "Half Note", "2", &["Keypad2"]),
    step("step.quarter", "Quarter Note", "3", &["Keypad3"]),
    step("step.eighth", "Eighth Note", "4", &["Keypad4"]),
    step("step.sixteenth", "Sixteenth Note", "5", &["Keypad5"]),
    step(
        "step.thirty_second",
        "Thirty-Second Note",
        "6",
        &["Keypad6"],
    ),
    step("step.dot", "Dotted", ".", &["Keypad."]),
    step("step.triplet", "Triplet", "/", &["Keypad/"]),
    step("step.tie", "Tie", "=", &["Keypad+"]),
    step("step.rest", "Rest", "0", &["Keypad0"]),
    step("step.delete_last", "Delete Last Step", "Delete", &[]),
    CommandSpec {
        params: &[ParamSpec {
            name: "pitches",
            kind: ParamKind::Text,
            required: true,
            help: "The chord to enter: note numbers or names (`60 64 67`, `C4 E4 G4`).",
        }],
        ..key_only(cmd("step.notes", "Step", "Enter Notes", None, true))
    },
    key_only(cmd(
        "record.keep_pass",
        "Record",
```

- [ ] **Step 4: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-control --test commands 2>&1 | grep -E "^test result|FAILED|panicked"
scripts/build-ffi.sh && scripts/swift.sh test --filter 'StepEntryTests|ControlCommandsTests' 2>&1 | tail -1
```

Expected: `commands` 15 passed; 24 Swift tests in 2 suites pass.

- [ ] **Step 5: Gate and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace 2>&1 | grep -E "FAILED|panicked" ; cargo test --workspace -- --list 2>/dev/null | grep -c ': test$'
scripts/build-ffi.sh && scripts/swift.sh build 2>&1 | tail -1 && scripts/swift.sh test 2>&1 | tail -1
git add app/Sources/VizDAW/SequenceSidebar.swift \
  app/Sources/VizDAW/VizDAWApp.swift \
  app/Sources/VizDAWAppModel/ControlCommands.swift \
  app/Sources/VizDAWAppModel/DocumentModel+Sequences.swift \
  app/Sources/VizDAWAppModel/DocumentModel.swift \
  app/Sources/VizDAWAppModel/DocumentStore.swift \
  app/Sources/VizDAWAppModel/StepEntry.swift \
  app/Tests/VizDAWAppModelTests/StepEntryTests.swift \
  crates/core-control/src/registry.rs \
  crates/core-control/tests/commands.rs
git commit -m "feat(keys): step entry, the Step menu's commands and keyboard slots

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
git log -1 --format='%h %s'
```

Expected: 904 Rust tests listed (903 + 1); Swift 640 (632 + 8). No `FAILED` or `panicked` line (rerun once if the known-flaky MIDI listener test trips, and say so).

---

### Task 9: Keys by the bindings — the key monitor, menus that show the bindings, the channel's keys

**Files:**
- Create: `app/Sources/VizDAW/KeyMonitor.swift`
- Modify: `app/Sources/VizDAW/AppCommands.swift`, `app/Sources/VizDAW/ControlChannel.swift`, `app/Sources/VizDAW/VizDAWApp.swift`, `app/Sources/VizDAWAppModel/StepEntry.swift`, `app/Sources/VizDAWAppModel/TransportCommands.swift`, `app/Sources/VizDAWEditors/ArrangementView.swift`, `app/Sources/VizDAWEditors/DrumGridView.swift`, `app/Sources/VizDAWEditors/DrumRowHeaderView.swift`, `app/Sources/VizDAWEditors/EditorCanvas.swift`, `app/Sources/VizDAWEditors/PianoRollView.swift`, `app/Sources/VizDAWEditors/StripChartView.swift`
- Test: `app/Tests/VizDAWAppModelTests/DrumGridViewTests.swift`, `app/Tests/VizDAWAppModelTests/StepEntryTests.swift`

**Interfaces:**
- Consumes: Task 6's `KeyRouter`, `KeyDispatch`, `KeyBindings.row`; Tasks 7–8's `TransportCommands`, `StepCommands`, `DocumentModel.pressSlot`; `ControlCommands.run`, `ControlWindows.{tagOf, openDocument}`, `AppControlHost.shared`, `DrumCommands`.
- Produces: `MenuCommands` protocol (`enabled`, `isOn`, `run(_:on:_:)`; `TransportCommands`, `StepCommands`, `DrumCommands` conform; `StepCommands.run(_:on:_:)` and `run(_:on:pitches:_:)`); app `KeyMenu.shortcut(_ id:) -> KeyboardShortcut?`; app `KeyMonitor.{start(), modifiers(_:), route(_:in:menus:), perform(_:_:), consume(_:)}`; `ControlChannel.keys` routes through `KeyMonitor.route(menus: false)`; every `AppCommands` shortcut from the bindings; new Transport items, a Step menu and Editors > Snap to Grid; `EditorCanvas.onToggleSnap` removed; `DrumRowHeaderView.cancelOperation` cancels the grid's stroke when no row drag is open.

Rulings 5 and 6, and the parked drum-row-header Escape. The monitor runs before AppKit's menus: a text field's plain keys go straight to it (`w.sendEvent(e)`, used up), a routed command runs through `ControlCommands.run` with the app's real host, an unavailable one passes the key on. The channel's `ui.key` uses the same route without the menus (a menu command runs and answers `dialog_required` when it would open a dialog, as before). The app target has no tests: what can be tested lives in `VizDAWAppModel` (Tasks 6–8); this task adds the protocol test and the header's Escape test.

- [ ] **Step 1: Write the failing tests**

In `app/Tests/VizDAWAppModelTests/DrumGridViewTests.swift`:

Find:

```swift
        h.mouseUp(at: CGPoint(x: 20, y: top + 95))
        #expect(f.edits.count == n, "the release after Escape drops nothing")
        #expect(h.acceptsFirstResponder)
    }
}
```

Replace with:

```swift
        h.mouseUp(at: CGPoint(x: 20, y: top + 95))
        #expect(f.edits.count == n, "the release after Escape drops nothing")
        #expect(h.acceptsFirstResponder)
        // With the keyboard still on the header, Escape cancels the grid's
        // open stroke.
        c.canvas.contentMouseDown(CGPoint(x: 5, y: 50), clicks: 1, modifiers: [])
        c.canvas.contentMouseDragged(CGPoint(x: 125, y: 70), modifiers: [])
        #expect(m.editor.gesture != nil)
        h.keyDown(with: esc)
        #expect(m.editor.gesture == nil && f.calls.last == "abort")
    }
}
```

In `app/Tests/VizDAWAppModelTests/StepEntryTests.swift`:

Find:

```swift
        #expect(StepCommands.enabled("step.rest", m) && !StepCommands.isOn("step.dot", m))
    }

    @Test func stepKeysWinOnlyDuringStepEntry() {
        let keys = KeyBindings(defaults: MemoryDefaults())
        func route(_ code: UInt16, step: Bool) -> KeyRoute {
```

Replace with:

```swift
        #expect(StepCommands.enabled("step.rest", m) && !StepCommands.isOn("step.dot", m))
    }

    @Test func menuItemsRunThroughOneProtocol() throws {
        let (_, _, m, _) = try demo()
        let kinds: [(any MenuCommands.Type, String)] = [
            (TransportCommands.self, "transport.overdub"), (StepCommands.self, StepCommands.entry),
            (DrumCommands.self, DrumCommands.timeline),
        ]
        for (kind, id) in kinds {
            let before = (kind.enabled(id, m), kind.isOn(id, m))
            #expect(before == (true, false), "\(id)")
            let ran = kind.run(id, on: nil, m)
            let after = kind.isOn(id, m)
            #expect(ran == .done && after, "\(id) toggled on")
        }
    }

    @Test func stepKeysWinOnlyDuringStepEntry() {
        let keys = KeyBindings(defaults: MemoryDefaults())
        func route(_ code: UInt16, step: Bool) -> KeyRoute {
```

- [ ] **Step 2: Run them to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh test --filter 'StepEntryTests|DrumGridViewTests' 2>&1 | grep -E "error:|✘" | head -3
```

Expected: FAIL: `cannot find type 'MenuCommands' in scope`; once it compiles, `escapeDuringARowHeaderDragDropsNothing` (the header's Escape leaves the grid's gesture open).

- [ ] **Step 3: Implement**

In `app/Sources/VizDAW/AppCommands.swift`:

Find:

```swift
    var body: some Commands {
        CommandGroup(replacing: .newItem) {
            Button("New") { show(store.newEmpty()) }
                .keyboardShortcut("n")
            Button("Open…") { if let path = Panels.chooseProject() { show(store.open(path: path)) } }
                .keyboardShortcut("o")
            Button("Create Demo Project") { show(store.newDemo()) }
            Button("Create Demo Project with Audio") { show(store.newDemoAudio()) }
        }
        CommandGroup(replacing: .saveItem) {
            Button("Close") { NSApp.keyWindow?.performClose(nil) }
                .keyboardShortcut("w")
            Button("Save") { save(as: false) }
                .keyboardShortcut("s")
                .disabled(doc == nil)
            Button("Save As…") { save(as: true) }
                .keyboardShortcut("s", modifiers: [.command, .shift])
                .disabled(doc == nil)
        }
        CommandGroup(replacing: .undoRedo) {
            Button(doc?.undoTitle ?? "Undo") { doc?.undo() }
                .keyboardShortcut("z")
                .disabled(!(doc?.canUndo ?? false) || doc?.editor.gesture != nil)
            Button(doc?.redoTitle ?? "Redo") { doc?.redo() }
                .keyboardShortcut("z", modifiers: [.command, .shift])
                .disabled(!(doc?.canRedo ?? false) || doc?.editor.gesture != nil)
        }
        CommandGroup(before: .windowList) {
            Button("Mixer") { if let docID { openWindow(id: "mixer", value: docID) } }
                .keyboardShortcut("m", modifiers: [.command, .option])
                .disabled(docID == nil)
            Button("Plugin Manager") { openWindow(id: "plugins") }
                .keyboardShortcut("p", modifiers: [.command, .option])
            Divider()
        }
        CommandMenu("Editors") {
```

Replace with:

```swift
    var body: some Commands {
        CommandGroup(replacing: .newItem) {
            Button("New") { show(store.newEmpty()) }
                .keyboardShortcut(KeyMenu.shortcut("file.new"))
            Button("Open…") { if let path = Panels.chooseProject() { show(store.open(path: path)) } }
                .keyboardShortcut(KeyMenu.shortcut("file.open"))
            Button("Create Demo Project") { show(store.newDemo()) }
            Button("Create Demo Project with Audio") { show(store.newDemoAudio()) }
        }
        CommandGroup(replacing: .saveItem) {
            Button("Close") { NSApp.keyWindow?.performClose(nil) }
                .keyboardShortcut(KeyMenu.shortcut("file.close"))
            Button("Save") { save(as: false) }
                .keyboardShortcut(KeyMenu.shortcut("file.save"))
                .disabled(doc == nil)
            Button("Save As…") { save(as: true) }
                .keyboardShortcut(KeyMenu.shortcut("file.save_as"))
                .disabled(doc == nil)
        }
        CommandGroup(replacing: .undoRedo) {
            Button(doc?.undoTitle ?? "Undo") { doc?.undo() }
                .keyboardShortcut(KeyMenu.shortcut("edit.undo"))
                .disabled(!(doc?.canUndo ?? false) || doc?.editor.gesture != nil)
            Button(doc?.redoTitle ?? "Redo") { doc?.redo() }
                .keyboardShortcut(KeyMenu.shortcut("edit.redo"))
                .disabled(!(doc?.canRedo ?? false) || doc?.editor.gesture != nil)
        }
        CommandGroup(before: .windowList) {
            Button("Mixer") { if let docID { openWindow(id: "mixer", value: docID) } }
                .keyboardShortcut(KeyMenu.shortcut("window.mixer"))
                .disabled(docID == nil)
            Button("Plugin Manager") { openWindow(id: "plugins") }
                .keyboardShortcut(KeyMenu.shortcut("window.plugin_manager"))
            Divider()
        }
        CommandMenu("Editors") {
```

Find:

```swift
                        doc?.editor.showDetail(kind, shown: doc?.selectedSeq)
                    }
                }
                .keyboardShortcut(KeyEquivalent(Character(kind.key)), modifiers: .command)
                .disabled(doc == nil)
            }
            Divider()
```

Replace with:

```swift
                        doc?.editor.showDetail(kind, shown: doc?.selectedSeq)
                    }
                }
                .keyboardShortcut(KeyMenu.shortcut("editors.\(ControlStates.kindName(kind))"))
                .disabled(doc == nil)
            }
            Divider()
```

Find:

```swift
                Button("Open \(kind.title) in Window") {
                    if let docID { EditorWindows.tearOff(store, doc: docID, kind: kind, from: tornWindow) }
                }
                .keyboardShortcut(KeyEquivalent(Character(kind.key)), modifiers: [.command, .option])
                .disabled(docID == nil)
            }
            Divider()
            Button("Close Detail Editor") { doc?.editor.closeDetail() }
                .disabled(doc?.editor.layout.detail == nil)
            Button("Insert Event") { doc?.listForCommands()?.insert() }
                .keyboardShortcut("i")
                .disabled(doc?.listForCommands() == nil || doc?.editor.textEditing == true)
            Divider()
            nudge("Nudge Earlier", .earlier, .leftArrow)
            nudge("Nudge Later", .later, .rightArrow)
            nudge("Nudge Up", .up, .upArrow)
            nudge("Nudge Down", .down, .downArrow)
        }
        CommandMenu("Modify") {
            panelItem(.selectByRule, "s")
            panelItem(.transpose, "t")
            panelItem(.quantize, "u")
            panelItem(.change, "m")
            panelItem(.move, "v")
            Divider()
            Button(doc?.selectModify.modifyAgainTitle ?? "Modify Again") {
                doc?.selectModify.modifyAgain()
            }
            .keyboardShortcut("r", modifiers: [.command, .control])
            .help(SelectModifyModel.modifyAgainHelp)
            .disabled(!(doc?.selectModify.canModifyAgain ?? false))
            Menu("Templates") {
```

Replace with:

```swift
                Button("Open \(kind.title) in Window") {
                    if let docID { EditorWindows.tearOff(store, doc: docID, kind: kind, from: tornWindow) }
                }
                .keyboardShortcut(KeyMenu.shortcut("editors.\(ControlStates.kindName(kind))_window"))
                .disabled(docID == nil)
            }
            Divider()
            Button("Close Detail Editor") { doc?.editor.closeDetail() }
                .disabled(doc?.editor.layout.detail == nil)
            Button("Insert Event") { doc?.listForCommands()?.insert() }
                .keyboardShortcut(KeyMenu.shortcut("editors.insert_event"))
                .disabled(doc?.listForCommands() == nil || doc?.editor.textEditing == true)
            Divider()
            nudge("Nudge Earlier", .earlier, "editors.nudge_earlier")
            nudge("Nudge Later", .later, "editors.nudge_later")
            nudge("Nudge Up", .up, "editors.nudge_up")
            nudge("Nudge Down", .down, "editors.nudge_down")
            Divider()
            commandToggle("editors.snap", TransportCommands.self)
        }
        CommandMenu("Modify") {
            panelItem(.selectByRule, "modify.select_by_rule")
            panelItem(.transpose, "modify.transpose")
            panelItem(.quantize, "modify.quantize")
            panelItem(.change, "modify.change")
            panelItem(.move, "modify.move")
            Divider()
            Button(doc?.selectModify.modifyAgainTitle ?? "Modify Again") {
                doc?.selectModify.modifyAgain()
            }
            .keyboardShortcut(KeyMenu.shortcut("modify.again"))
            .help(SelectModifyModel.modifyAgainHelp)
            .disabled(!(doc?.selectModify.canModifyAgain ?? false))
            Menu("Templates") {
```

Find:

```swift
                "Exact Mode",
                isOn: Binding(get: { doc?.editor.exactMode ?? false }, set: { doc?.editor.exactMode = $0 })
            )
            .keyboardShortcut("e", modifiers: [.command, .control])
            .disabled(doc == nil)
        }
        CommandMenu("Sequence") {
            Button("Make Reference from Selection") {
                if let doc, let seq = doc.selectedSeq { doc.editor.makeReferenceFromSelection(seq: seq) }
            }
            .keyboardShortcut("r", modifiers: [.command, .option])
            .disabled(doc?.selectedSeq == nil || doc?.editor.gesture != nil)
            Button("Back to Parent Sequence") { doc?.editor.back() }
                .keyboardShortcut("[", modifiers: [.command])
                .disabled(doc?.editor.navigation.isEmpty ?? true)
        }
        CommandMenu("Drum") {
```

Replace with:

```swift
                "Exact Mode",
                isOn: Binding(get: { doc?.editor.exactMode ?? false }, set: { doc?.editor.exactMode = $0 })
            )
            .keyboardShortcut(KeyMenu.shortcut("modify.exact_mode"))
            .disabled(doc == nil)
        }
        CommandMenu("Sequence") {
            Button("Make Reference from Selection") {
                if let doc, let seq = doc.selectedSeq { doc.editor.makeReferenceFromSelection(seq: seq) }
            }
            .keyboardShortcut(KeyMenu.shortcut("sequence.make_reference"))
            .disabled(doc?.selectedSeq == nil || doc?.editor.gesture != nil)
            Button("Back to Parent Sequence") { doc?.editor.back() }
                .keyboardShortcut(KeyMenu.shortcut("sequence.back"))
                .disabled(doc?.editor.navigation.isEmpty ?? true)
        }
        CommandMenu("Drum") {
```

Find:

```swift
        }
        CommandMenu("Transport") {
            Button(doc?.isPlaying == true ? "Stop" : "Play") { doc?.playStop() }
                .keyboardShortcut(.space, modifiers: [])
                .disabled(!(doc?.canPlay ?? false) || !(doc?.transportKeysEnabled ?? false))
            Toggle("Loop", isOn: Binding(get: { doc?.looping ?? false }, set: { _ in doc?.toggleLoop() }))
                .disabled(doc == nil)
            Button("Return to Start") { doc?.returnToStart() }
                .keyboardShortcut(.return, modifiers: [])
                .disabled(!(doc?.returnKeyEnabled ?? false))
        }
    }

    /// ⌥-arrows nudge the selection (Plan 4b Ruling 16); off while a text
    /// field has the keyboard, so ⌥-arrows there still move the cursor.
    private func nudge(_ title: String, _ dir: NudgeDirection, _ key: KeyEquivalent) -> some View {
        Button(title) { doc?.editor.nudge(dir, settings: editingPreferences.nudge) }
            .keyboardShortcut(key, modifiers: .option)
            .disabled(
                doc == nil || !(doc?.transportKeysEnabled ?? false) || (doc?.editor.selection.events.isEmpty ?? true))
    }
```

Replace with:

```swift
        }
        CommandMenu("Transport") {
            Button(doc?.isPlaying == true ? "Stop" : "Play") { doc?.playStop() }
                .keyboardShortcut(KeyMenu.shortcut("transport.play_stop"))
                .disabled(!(doc?.canPlay ?? false) || !(doc?.transportKeysEnabled ?? false))
            commandItem("transport.stop", TransportCommands.self)
            commandItem("transport.play_selection", TransportCommands.self)
            commandItem("transport.play_from_selection", TransportCommands.self)
            commandItem("transport.play_from_edit_in", TransportCommands.self)
            commandItem("transport.record", TransportCommands.self)
            commandItem("transport.scrub", TransportCommands.self)
            Button("Return to Start") { doc?.returnToStart() }
                .keyboardShortcut(KeyMenu.shortcut("transport.return_to_start"))
                .disabled(!(doc?.returnKeyEnabled ?? false))
            commandItem("transport.previous_marker", TransportCommands.self)
            commandItem("transport.next_marker", TransportCommands.self)
            Divider()
            Toggle("Loop", isOn: Binding(get: { doc?.looping ?? false }, set: { _ in doc?.toggleLoop() }))
                .keyboardShortcut(KeyMenu.shortcut("transport.loop"))
                .disabled(doc == nil)
            commandToggle("transport.overdub", TransportCommands.self)
            commandToggle("transport.punch", TransportCommands.self)
            Divider()
            commandItem("transport.arm_previous", TransportCommands.self)
            commandItem("transport.arm_next", TransportCommands.self)
        }
        CommandMenu("Step") {
            commandToggle(StepCommands.entry, StepCommands.self)
            Divider()
            ForEach(["step.whole", "step.half", "step.quarter", "step.eighth", "step.sixteenth", "step.thirty_second"],
                    id: \.self) { commandToggle($0, StepCommands.self) }
            Divider()
            commandToggle("step.dot", StepCommands.self)
            commandToggle("step.triplet", StepCommands.self)
            commandItem("step.tie", StepCommands.self)
            commandItem("step.rest", StepCommands.self)
            commandItem("step.delete_last", StepCommands.self)
        }
    }

    /// A menu item for a command of `kind` (plan 4f): its title and key
    /// from the bindings; a refusal shows in the status line.
    private func commandItem(_ id: String, _ kind: any MenuCommands.Type) -> some View {
        Button(keyBindings.row(id)?.title ?? id) { if let doc { report(kind.run(id, on: nil, doc), doc) } }
            .keyboardShortcut(KeyMenu.shortcut(id))
            .disabled(doc.map { !kind.enabled(id, $0) } ?? true)
    }

    /// A toggle menu item (its check mark from the command's state).
    private func commandToggle(_ id: String, _ kind: any MenuCommands.Type) -> some View {
        Toggle(
            keyBindings.row(id)?.title ?? id,
            isOn: Binding(
                get: { doc.map { kind.isOn(id, $0) } ?? false },
                set: { on in if let doc { report(kind.run(id, on: on, doc), doc) } })
        )
        .keyboardShortcut(KeyMenu.shortcut(id))
        .disabled(doc.map { !kind.enabled(id, $0) } ?? true)
    }

    private func report(_ o: KeyOutcome, _ doc: DocumentModel) {
        if case .refused(let why) = o { doc.editor.status = why }
    }

    /// ⌥-arrows nudge the selection (Plan 4b Ruling 16); off while a text
    /// field has the keyboard, so ⌥-arrows there still move the cursor.
    private func nudge(_ title: String, _ dir: NudgeDirection, _ id: String) -> some View {
        Button(title) { doc?.editor.nudge(dir, settings: editingPreferences.nudge) }
            .keyboardShortcut(KeyMenu.shortcut(id))
            .disabled(
                doc == nil || !(doc?.transportKeysEnabled ?? false) || (doc?.editor.selection.events.isEmpty ?? true))
    }
```

Find:

```swift
    /// status line like any editor command.
    private func drumItem(_ title: String, _ id: String) -> some View {
        Button(title) { if let doc { DrumCommands.run(id, doc) } }
            .disabled(doc.map { !DrumCommands.enabled(id, $0) } ?? true)
    }

```

Replace with:

```swift
    /// status line like any editor command.
    private func drumItem(_ title: String, _ id: String) -> some View {
        Button(title) { if let doc { DrumCommands.run(id, doc) } }
            .keyboardShortcut(KeyMenu.shortcut(id))
            .disabled(doc.map { !DrumCommands.enabled(id, $0) } ?? true)
    }

```

Find:

```swift
                get: { doc.map { DrumCommands.isOn(id, $0) } ?? false },
                set: { on in if let doc { DrumCommands.run(id, on: on, doc) } })
        )
        .disabled(doc.map { !DrumCommands.enabled(id, $0) } ?? true)
    }

    /// A Modify menu command: the panel set to its task (Plan 4c Ruling 10).
    private func panelItem(_ task: PanelTask, _ key: Character) -> some View {
        Button("\(task.title)…") { doc?.selectModify.open(task) }
            .keyboardShortcut(KeyEquivalent(key), modifiers: [.command, .control])
            .disabled(doc == nil)
    }

```

Replace with:

```swift
                get: { doc.map { DrumCommands.isOn(id, $0) } ?? false },
                set: { on in if let doc { DrumCommands.run(id, on: on, doc) } })
        )
        .keyboardShortcut(KeyMenu.shortcut(id))
        .disabled(doc.map { !DrumCommands.enabled(id, $0) } ?? true)
    }

    /// A Modify menu command: the panel set to its task (Plan 4c Ruling 10).
    private func panelItem(_ task: PanelTask, _ id: String) -> some View {
        Button("\(task.title)…") { doc?.selectModify.open(task) }
            .keyboardShortcut(KeyMenu.shortcut(id))
            .disabled(doc == nil)
    }

```

In `app/Sources/VizDAW/ControlChannel.swift`:

Find:

```swift
        }
    }

    /// Key strokes into `w` (control spec §5.5): a menu shortcut runs its
    /// command on the window's document, as the menu would for the key
    /// window; an unavailable command, or a stroke that is no shortcut,
    /// goes to the window's first responder (typing, canvas keys).
    static func keys(_ server: ControlServer, _ token: UInt64, _ strokes: [KeyStrokeRow], _ w: NSWindow) {
        var sent: UInt32 = 0
        for s in strokes {
            if let id = s.command {
                let doc = ControlWindows.document(.number(number: Int64(w.windowNumber)))
                let outcome = ControlCommands.run(id, args: [], doc: doc, store: appStore, host: AppControlHost.shared)
                switch outcome {
                case .failed(let f) where f.code == .unavailable: break
```

Replace with:

```swift
        }
    }

    /// Key strokes into `w` (control spec §5.5, plan 4f Ruling 5): routed
    /// as the keyboard's are, by the current key bindings. A bound key runs
    /// its command on the window's document (a menu item's too: the
    /// channel has no menus, so a dialog it would open is
    /// `dialog_required`); a keyboard slot plays or enters its sequence; a
    /// text field's plain keys, an unavailable command and an unbound key
    /// go to the window's first responder (typing, canvas keys).
    static func keys(_ server: ControlServer, _ token: UInt64, _ strokes: [KeyStrokeRow], _ w: NSWindow) {
        var sent: UInt32 = 0
        for s in strokes {
            let events = ControlUI.keyEvents(w, [s])
            guard let down = events.first else { continue }
            let (route, doc) = KeyMonitor.route(down, in: w, menus: false)
            switch route {
            case .command(let id):
                let outcome = ControlCommands.run(id, args: [], doc: doc, store: appStore, host: AppControlHost.shared)
                switch outcome {
                case .failed(let f) where f.code == .unavailable: break
```

Find:

```swift
                    sent += 1
                    continue
                }
            }
            let events = ControlUI.keyEvents(w, [s])
            ControlUI.deliver(events, to: w)
            sent += UInt32(events.count)
        }
```

Replace with:

```swift
                    sent += 1
                    continue
                }
            case .slot:
                if KeyMonitor.perform(route, doc) {
                    sent += 1
                    continue
                }
            case .typing, .pass: break
            }
            ControlUI.deliver(events, to: w)
            sent += UInt32(events.count)
        }
```

Create `app/Sources/VizDAW/KeyMonitor.swift`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p app/Sources/VizDAW
cat > app/Sources/VizDAW/KeyMonitor.swift <<'VIZDAW_EOF'
import AppKit
import SwiftUI
import VizDAWAppModel
import VizDAWCore

/// A command's first key as a menu shortcut (plan 4f Ruling 6): menus
/// show the bindings, so a rebound key shows at once.
@MainActor
enum KeyMenu {
    static func shortcut(_ id: String) -> KeyboardShortcut? {
        guard let r = keyBindings.row(id), let c = r.menuKey?.first else { return nil }
        var m: EventModifiers = []
        if r.menuModifiers.shift { m.insert(.shift) }
        if r.menuModifiers.control { m.insert(.control) }
        if r.menuModifiers.option { m.insert(.option) }
        if r.menuModifiers.command { m.insert(.command) }
        return KeyboardShortcut(KeyEquivalent(c), modifiers: m)
    }
}

/// Keys by the bindings (plan 4f Ruling 5): every key press in VizDAW
/// passes here before AppKit's menus see it. A text field keeps its
/// plain keys (they go straight to it, past the menus' single-key
/// shortcuts); in a window showing a document, a bound key runs its
/// command (or a sequence's keyboard slot) through the same handlers as
/// the control channel; anything else, and a command that is
/// unavailable or needs its dialog, goes on to the menus and the views.
@MainActor
enum KeyMonitor {
    private static var monitor: Any?

    static func start() {
        guard monitor == nil else { return }
        monitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { e in
            // Key events arrive on the main thread.
            nonisolated(unsafe) let event = e
            let used = MainActor.assumeIsolated { consume(event) }
            return used ? nil : e
        }
    }

    static func modifiers(_ f: NSEvent.ModifierFlags) -> ModifiersRow {
        ModifiersRow(
            shift: f.contains(.shift), control: f.contains(.control), option: f.contains(.option),
            command: f.contains(.command))
    }

    /// What a key press in `w` does: the route, decided as for the
    /// channel's keys (`menus`: AppKit's menus see the key next).
    static func route(_ e: NSEvent, in w: NSWindow, menus: Bool) -> (KeyRoute, (id: UUID, model: DocumentModel)?) {
        let typing = w.firstResponder is NSText
        let doc: (id: UUID, model: DocumentModel)? = ControlWindows.tagOf(w).doc.flatMap { d in
            appStore.model(d).map { (d, $0) }
        }
        let m = modifiers(e.modifierFlags)
        let r = KeyRouter.route(
            keyCode: e.keyCode, modifiers: m, typing: typing, stepEntry: doc?.model.stepEntry.isOn ?? false,
            menus: menus, keys: keyBindings, sequences: doc?.model.snapshot.sequences ?? [])
        return (r, doc)
    }

    /// Runs the route's command (a window without a document runs only
    /// commands that need none); whether the key was used up.
    static func perform(_ r: KeyRoute, _ doc: (id: UUID, model: DocumentModel)?) -> Bool {
        let outcome: ControlOutcome
        switch r {
        case .typing, .pass: return false
        case .slot(let seq):
            guard let doc else { return false }
            switch doc.model.pressSlot(seq) {
            case .done: return true
            case .unavailable: return false
            case .refused(let why):
                doc.model.editor.status = why
                return true
            }
        case .command(let id):
            outcome = ControlCommands.run(id, args: [], doc: doc, store: appStore, host: AppControlHost.shared)
        }
        switch KeyDispatch.after(outcome, doc: doc?.model) {
        case .handled: return true
        case .pass: return false
        case .opened(let d):
            ControlWindows.openDocument(d)
            return true
        }
    }

    /// Whether VizDAW used the key press up (AppKit never sees it).
    static func consume(_ e: NSEvent) -> Bool {
        guard let w = e.window ?? NSApp.keyWindow else { return false }
        let (r, doc) = route(e, in: w, menus: true)
        if r == .typing {
            // Straight to the text: the menus' single-key shortcuts must
            // not take a letter typed into a field.
            w.sendEvent(e)
            return true
        }
        return perform(r, doc)
    }
}
VIZDAW_EOF
```

In `app/Sources/VizDAW/VizDAWApp.swift`:

Find:

```swift
        NSApp.setActivationPolicy(.regular)
        NSApp.activate()
        StoreAlerts.watch(appStore)
        AppSetup.startScan()
        PluginTicker.start(appStore)
    }
```

Replace with:

```swift
        NSApp.setActivationPolicy(.regular)
        NSApp.activate()
        StoreAlerts.watch(appStore)
        KeyMonitor.start()
        AppSetup.startScan()
        PluginTicker.start(appStore)
    }
```

In `app/Sources/VizDAWAppModel/StepEntry.swift`:

Find:

```swift
/// The Step menu and step entry's keys (plan 4f Ruling 12): one
/// implementation for the menu items, the keys and `command.run`.
@MainActor
public enum StepCommands {
    public static let entry = "step.entry"
    public static let notes = "step.notes"
    static let values: [String: NoteValueRow] = [
```

Replace with:

```swift
/// The Step menu and step entry's keys (plan 4f Ruling 12): one
/// implementation for the menu items, the keys and `command.run`.
@MainActor
public enum StepCommands: MenuCommands {
    public static let entry = "step.entry"
    public static let notes = "step.notes"
    static let values: [String: NoteValueRow] = [
```

Find:

```swift
        }
    }

    /// Runs `id` on `m`; `on` sets step entry (nil flips it); `pitches`
    /// is step.notes' text.
    @discardableResult
    public static func run(_ id: String, on: Bool? = nil, pitches: String? = nil, _ m: DocumentModel) -> KeyOutcome {
        let step = m.stepEntry
        if id == entry {
            let want = on ?? !step.isOn
```

Replace with:

```swift
        }
    }

    /// Runs `id` on `m` (a menu item, a key): `on` sets step entry (nil
    /// flips it).
    @discardableResult
    public static func run(_ id: String, on: Bool?, _ m: DocumentModel) -> KeyOutcome {
        run(id, on: on, pitches: nil, m)
    }

    /// Runs `id` on `m`; `pitches` is step.notes' text.
    @discardableResult
    public static func run(_ id: String, on: Bool?, pitches: String?, _ m: DocumentModel) -> KeyOutcome {
        let step = m.stepEntry
        if id == entry {
            let want = on ?? !step.isOn
```

In `app/Sources/VizDAWAppModel/TransportCommands.swift`:

Find:

```swift
/// every command that runs from keys, menus and the channel alike.
public typealias KeyOutcome = DrumOutcome

/// The transport keys, modes and locators (editors spec §4f, plan 4f
/// Rulings 7–10): one implementation for the menu items, the keys and
/// the control channel's `command.run`. None opens a dialog.
```

Replace with:

```swift
/// every command that runs from keys, menus and the channel alike.
public typealias KeyOutcome = DrumOutcome

/// A group of commands with menu items (plan 4f): whether one is enabled,
/// its check mark, and running it (`on` sets a toggle; nil flips it).
@MainActor
public protocol MenuCommands {
    static func enabled(_ id: String, _ m: DocumentModel) -> Bool
    static func isOn(_ id: String, _ m: DocumentModel) -> Bool
    static func run(_ id: String, on: Bool?, _ m: DocumentModel) -> KeyOutcome
}

extension TransportCommands: MenuCommands {}
extension DrumCommands: MenuCommands {}

/// The transport keys, modes and locators (editors spec §4f, plan 4f
/// Rulings 7–10): one implementation for the menu items, the keys and
/// the control channel's `command.run`. None opens a dialog.
```

In `app/Sources/VizDAWEditors/ArrangementView.swift`:

Find:

```swift
        }
        onRulerRange = { [unowned self] r in self.editor.selectRange(r, lanes: self.lanes) }
        onRulerClick = { [unowned self] t in self.document.locate(t) }
        onToggleSnap = { [unowned self] in self.editor.updateLayout { $0.snap.enabled.toggle() } }
        onAxisChange = { [unowned self] a in
            if a.ticksPerPoint != self.editor.layout.ticksPerPoint {
                self.editor.updateLayout { $0.ticksPerPoint = a.ticksPerPoint }
```

Replace with:

```swift
        }
        onRulerRange = { [unowned self] r in self.editor.selectRange(r, lanes: self.lanes) }
        onRulerClick = { [unowned self] t in self.document.locate(t) }
        onAxisChange = { [unowned self] a in
            if a.ticksPerPoint != self.editor.layout.ticksPerPoint {
                self.editor.updateLayout { $0.ticksPerPoint = a.ticksPerPoint }
```

In `app/Sources/VizDAWEditors/DrumGridView.swift`:

Find:

```swift
            self.editor.selectRange(r, lanes: self.model.lanes.map { LaneKey(seq, $0.lane) })
        }
        onRulerClick = { [weak self] t in self?.document?.locate(t) }
        onToggleSnap = { [weak self] in self?.editor.updateLayout { $0.snap.enabled.toggle() } }
        onAxisChange = { [weak self] a in
            guard let self, a.ticksPerPoint != self.editor.layout.drumGrid.ticksPerPoint else { return }
            self.editor.updateLayout { $0.drumGrid.ticksPerPoint = a.ticksPerPoint }
```

Replace with:

```swift
            self.editor.selectRange(r, lanes: self.model.lanes.map { LaneKey(seq, $0.lane) })
        }
        onRulerClick = { [weak self] t in self?.document?.locate(t) }
        onAxisChange = { [weak self] a in
            guard let self, a.ticksPerPoint != self.editor.layout.drumGrid.ticksPerPoint else { return }
            self.editor.updateLayout { $0.drumGrid.ticksPerPoint = a.ticksPerPoint }
```

In `app/Sources/VizDAWEditors/DrumRowHeaderView.swift`:

Find:

```swift
    }

    public override func keyDown(with e: NSEvent) {
        if e.keyCode == 53, dragging != nil {  // Escape
            cancelOperation(nil)
        } else {
            super.keyDown(with: e)
        }
    }

    /// Escape: the drag ends and its release drops nothing.
    public override func cancelOperation(_ sender: Any?) {
        guard dragging != nil else { return }
        dragging = nil
        needsDisplay = true
    }
```

Replace with:

```swift
    }

    public override func keyDown(with e: NSEvent) {
        if e.keyCode == 53 {  // Escape
            cancelOperation(nil)
        } else {
            super.keyDown(with: e)
        }
    }

    /// Escape: a row drag ends and its release drops nothing; without one,
    /// the grid's stroke (one still waiting on a busy session) is cancelled,
    /// as Escape in the grid would (parked from plan 4e: the header keeps
    /// the keyboard after a click).
    public override func cancelOperation(_ sender: Any?) {
        guard dragging != nil else {
            model.strokeCancelled()
            return
        }
        dragging = nil
        needsDisplay = true
    }
```

In `app/Sources/VizDAWEditors/EditorCanvas.swift`:

Find:

```swift
    public var playheadSource: (@MainActor (UInt64) -> Double?)?
    public var onRulerRange: ((TickRange) -> Void)?
    public var onRulerClick: ((Int64) -> Void)?
    public var onToggleSnap: (() -> Void)?
    public var onAxisChange: ((TimeAxis) -> Void)?
    public var onVerticalScroll: ((CGFloat) -> Void)?

```

Replace with:

```swift
    public var playheadSource: (@MainActor (UInt64) -> Double?)?
    public var onRulerRange: ((TickRange) -> Void)?
    public var onRulerClick: ((Int64) -> Void)?
    public var onAxisChange: ((TimeAxis) -> Void)?
    public var onVerticalScroll: ((CGFloat) -> Void)?

```

Find:

```swift
        dragStep()
    }

    open override func keyDown(with e: NSEvent) {
        let plain = e.modifierFlags.intersection(.deviceIndependentFlagsMask)
            .subtracting([.capsLock, .numericPad, .function]).isEmpty
        if e.keyCode == 53 {  // Escape
            cancelGestures()
        } else if plain, e.charactersIgnoringModifiers?.lowercased() == "g" {
            onToggleSnap?()
        } else {
            super.keyDown(with: e)
        }
```

Replace with:

```swift
        dragStep()
    }

    /// Escape cancels a gesture. Snap (G) and every other bound key are
    /// the key bindings' (Editors > Snap to Grid, plan 4f).
    open override func keyDown(with e: NSEvent) {
        if e.keyCode == 53 {  // Escape
            cancelGestures()
        } else {
            super.keyDown(with: e)
        }
```

In `app/Sources/VizDAWEditors/PianoRollView.swift`:

Find:

```swift
            self.editor.selectRange(r, lanes: self.model.shownLanes)
        }
        onRulerClick = { [weak self] t in self?.document?.locate(t) }
        onToggleSnap = { [weak self] in self?.editor.updateLayout { $0.snap.enabled.toggle() } }
        onAxisChange = { [weak self] a in
            guard let self else { return }
            self.onTimeAxis?(a)
```

Replace with:

```swift
            self.editor.selectRange(r, lanes: self.model.shownLanes)
        }
        onRulerClick = { [weak self] t in self?.document?.locate(t) }
        onAxisChange = { [weak self] a in
            guard let self else { return }
            self.onTimeAxis?(a)
```

In `app/Sources/VizDAWEditors/StripChartView.swift`:

Find:

```swift
            self.editor.selectRange(r, lanes: [lane])
        }
        onRulerClick = { [weak self] t in self?.document?.locate(t) }
        onToggleSnap = { [weak self] in self?.editor.updateLayout { $0.snap.enabled.toggle() } }
        onAxisChange = { [weak self] a in
            guard let self, a.ticksPerPoint != self.model.settings.ticksPerPoint else { return }
            self.model.updateSettings { $0.ticksPerPoint = a.ticksPerPoint }
```

Replace with:

```swift
            self.editor.selectRange(r, lanes: [lane])
        }
        onRulerClick = { [weak self] t in self?.document?.locate(t) }
        onAxisChange = { [weak self] a in
            guard let self, a.ticksPerPoint != self.model.settings.ticksPerPoint else { return }
            self.model.updateSettings { $0.ticksPerPoint = a.ticksPerPoint }
```

- [ ] **Step 4: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh build 2>&1 | grep -E "error|warning: " | grep -v "build-system|MemoryDefaults" ; scripts/swift.sh test 2>&1 | tail -1
```

Expected: the app builds with no new warning; 641 Swift tests pass.

- [ ] **Step 5: Gate and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace 2>&1 | grep -E "FAILED|panicked" ; cargo test --workspace -- --list 2>/dev/null | grep -c ': test$'
scripts/build-ffi.sh && scripts/swift.sh build 2>&1 | tail -1 && scripts/swift.sh test 2>&1 | tail -1
touch /tmp/vizdaw-4f-e2e-marker
scripts/make-app.sh && VIZDAW_E2E_APP=dist/VizDAW.app cargo test -p vizctl --test e2e -- --ignored --test-threads 1 2>&1 | grep -E "test result|panicked"
pgrep -lf MacOS/VizDAW ; find /private/tmp -maxdepth 1 -name 'vze*' -newer /tmp/vizdaw-4f-e2e-marker -exec rm -rf {} +
git add app/Sources/VizDAW/AppCommands.swift \
  app/Sources/VizDAW/ControlChannel.swift \
  app/Sources/VizDAW/KeyMonitor.swift \
  app/Sources/VizDAW/VizDAWApp.swift \
  app/Sources/VizDAWAppModel/StepEntry.swift \
  app/Sources/VizDAWAppModel/TransportCommands.swift \
  app/Sources/VizDAWEditors/ArrangementView.swift \
  app/Sources/VizDAWEditors/DrumGridView.swift \
  app/Sources/VizDAWEditors/DrumRowHeaderView.swift \
  app/Sources/VizDAWEditors/EditorCanvas.swift \
  app/Sources/VizDAWEditors/PianoRollView.swift \
  app/Sources/VizDAWEditors/StripChartView.swift \
  app/Tests/VizDAWAppModelTests/DrumGridViewTests.swift \
  app/Tests/VizDAWAppModelTests/StepEntryTests.swift
git commit -m "feat(keys): keys by the bindings; menus show them

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
git log -1 --format='%h %s'
```

Expected: 904 Rust tests listed; Swift 641 (640 + 1); e2e 3 passed (Space through the router, the toolbar click, the drum grid). No `FAILED` or `panicked` line (rerun once if the known-flaky MIDI listener test trips, and say so); `pgrep` prints nothing.

---

### Task 10: Transport fields and Settings > Keys — P/E/U, bar·beat·unit entry, the key recorder

**Files:**
- Create: `app/Sources/VizDAW/KeysSettingsView.swift`, `app/Sources/VizDAWAppModel/TransportFields.swift`, `app/Sources/VizDAWEditors/TransportFieldsView.swift`
- Modify: `app/Sources/VizDAW/AppCommands.swift`, `app/Sources/VizDAW/ArrangementPane.swift`, `app/Sources/VizDAW/ControlChannel.swift`, `app/Sources/VizDAW/KeyMonitor.swift`, `app/Sources/VizDAW/VizDAWApp.swift`, `app/Sources/VizDAWAppModel/ControlCommands.swift`, `app/Sources/VizDAWAppModel/KeyBindings.swift`, `crates/core-control/src/registry.rs`, `crates/core-ffi/src/keys.rs`, `crates/core-ffi/src/lib.rs`
- Test: `app/Tests/VizDAWAppModelTests/ControlCommandsTests.swift`, `app/Tests/VizDAWAppModelTests/TransportFieldsTests.swift`, `crates/core-control/tests/commands.rs`, `crates/core-ffi/tests/keys.rs`

**Interfaces:**
- Consumes: `Bbu`, `PositionInput`, `MeterMath`, `EditorModel.{info, setSelection, updateLayout, beginTextEditing, endTextEditing}`, `ControlUI.isChannel`, Task 7's `DocumentModel.moveTo`, Task 6's `KeyBindings`.
- Produces: `vizdaw_ffi::chord_text(key_code, modifiers) -> Option<String>`; registry `transport.{go_position (P), go_edit_in (E), go_punch (U)}` (count 105 → 108); `TransportField {position, editIn, editOut, punchIn, punchOut}` (`title`); `DocumentModel.{fieldTick(_:), fieldText(_:), fieldInput(_:_:), commitField(_:_:) -> Bool}`; `ControlHost.focus(_ field: TransportField, _ doc: UUID) -> Bool`; `ControlCommands.fieldKeys`; `TransportFieldsView` (`textOwner = "transport"`, `fields`, `focus(_:)`, `refresh()`, `giveKeyboardBack(_:)`, `find(in:)`, the text-field delegate); `TransportTextField` (`armed`, `title`); `KeyCapturing` protocol; `KeyRecorderView` (`title`, `onKey`, `recording`, `start()`, `take(_:)`); `KeyBindings.{pending: PendingKey?, record(_:key:), confirmReplace(), cancelReplace()}`; app `TransportBar`, `TransportFieldsHost`, `KeysSettingsView`, `KeyRecorder`, the Settings tabs, the Transport menu's field items; the key monitor leaves a `KeyCapturing` first responder alone.

Rulings 16 and 17. The fields are AppKit text fields in a bar above the arrangement (beside step entry's line); a field takes the keyboard only from its key or a click (`TransportTextField.armed`), never as the window's first text field when the window opens — otherwise T, G and the other single keys would type into Position from the start. Return commits and, a run-loop turn later (the field selects its text again after the notification), gives the keyboard back to the keys. Each field is an accessibility element named by its title ("Position"). The Keys page's recorder captures keys in `performKeyEquivalent`, before the menus.

- [ ] **Step 1: Write the failing tests**

In `app/Tests/VizDAWAppModelTests/ControlCommandsTests.swift`:

Find:

```swift
    var calls: [String] = []
    var nudge = NudgeSettings()
    let keys = KeyBindings(defaults: MemoryDefaults())
    func showMixer(_ doc: UUID) { calls.append("mixer") }
    func showPluginManager() { calls.append("plugins") }
    func tearOff(_ doc: UUID, kind: DetailEditorKind) { calls.append("tear \(kind.rawValue)") }
```

Replace with:

```swift
    var calls: [String] = []
    var nudge = NudgeSettings()
    let keys = KeyBindings(defaults: MemoryDefaults())
    /// Whether `focus` finds the fields.
    var fieldsShown = true
    func focus(_ field: TransportField, _ doc: UUID) -> Bool {
        calls.append("focus \(field.rawValue)")
        return fieldsShown
    }
    func showMixer(_ doc: UUID) { calls.append("mixer") }
    func showPluginManager() { calls.append("plugins") }
    func tearOff(_ doc: UUID, kind: DetailEditorKind) { calls.append("tear \(kind.rawValue)") }
```

Create `app/Tests/VizDAWAppModelTests/TransportFieldsTests.swift`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p app/Tests/VizDAWAppModelTests
cat > app/Tests/VizDAWAppModelTests/TransportFieldsTests.swift <<'VIZDAW_EOF'
import AppKit
import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel
@testable import VizDAWEditors

private let bar: Int64 = 61_440
private let beat: Int64 = 15_360

/// The transport fields, their keys and the Keys settings' recorder
/// (plan 4f Task 10).
@MainActor
@Suite struct TransportFieldsTests {
    func model() throws -> (FakeSession, DocumentModel) {
        let f = demoFake()
        return (f, try DocumentModel(session: f))
    }

    @Test func fieldsShowAndTakeBarBeatUnit() throws {
        let (f, m) = try model()
        f.state.tick = bar + beat
        m.poll()
        #expect(m.fieldText(.position) == "2.2.0")
        #expect(m.fieldText(.editIn).isEmpty && m.fieldText(.punchOut) == "5.1.0")
        #expect(m.commitField(.position, "3"))
        #expect(f.state.tick == 2 * bar, "a bar alone is its downbeat")
        #expect(m.commitField(.position, "4:2:480"), "colons separate like dots")
        #expect(f.state.tick == 3 * bar + beat + beat / 2)
        #expect(m.commitField(.position, ".3"), "only the beat: the bar and unit stay")
        #expect(f.state.tick == 3 * bar + 2 * beat + beat / 2)
        #expect(!m.commitField(.position, "x.y"))
        #expect(m.editor.status == "“x.y” is not a position (bar.beat.unit).")
    }

    @Test func editInAndOutSetTheSelectionsRange() throws {
        let (_, m) = try model()
        #expect(!m.commitField(.editOut, "3"))
        #expect(m.editor.status == "Set Edit In first.")
        #expect(m.commitField(.editIn, "2"))
        #expect(m.editor.selection.timeRange == TickRange(start: bar, end: 2 * bar), "a bar long")
        #expect(m.commitField(.editOut, "4"))
        #expect(m.editor.selection.timeRange == TickRange(start: bar, end: 3 * bar))
        #expect(!m.commitField(.editOut, "1"))
        #expect(m.editor.status == "Edit Out must be after Edit In.")
        #expect(m.commitField(.editIn, "5"), "past the end: the length moves along")
        #expect(m.editor.selection.timeRange == TickRange(start: 4 * bar, end: 6 * bar))
        #expect(m.fieldText(.editIn) == "5.1.0" && m.fieldText(.editOut) == "7.1.0")
    }

    @Test func punchFieldsKeepTheirOrder() throws {
        let (_, m) = try model()
        #expect(m.commitField(.punchIn, "2"))
        #expect(m.editor.layout.transport.punchIn == bar && m.editor.layout.transport.punchOut == 4 * bar)
        #expect(m.commitField(.punchIn, "9"), "past Punch Out: it moves a bar after")
        #expect(m.editor.layout.transport.punchOut == 9 * bar)
        #expect(!m.commitField(.punchOut, "8"))
        #expect(m.editor.status == "Punch Out must be after Punch In.")
    }

    @Test func theFieldsViewCommitsRevertsAndNamesItsFields() throws {
        let (f, m) = try model()
        let v = TransportFieldsView(document: m)
        let w = NSWindow(contentRect: CGRect(x: 0, y: 0, width: 700, height: 40), styleMask: [.titled],
                         backing: .buffered, defer: true)
        w.contentView = v
        #expect(TransportFieldsView.find(in: w.contentView?.superview) === v)
        let pos = try #require(v.fields[.position])
        #expect(pos.accessibilityLabel() == "Position" && pos.stringValue == "1.1.0")
        w.makeFirstResponder(pos)
        #expect(pos.currentEditor() == nil, "not as the window's first text field")
        #expect(v.focus(.position) && w.firstResponder === pos.currentEditor())
        v.controlTextDidBeginEditing(Notification(name: NSControl.textDidBeginEditingNotification, object: pos))
        #expect(m.editor.textEditors == [TransportFieldsView.textOwner])
        pos.stringValue = "2"
        v.controlTextDidEndEditing(
            Notification(
                name: NSControl.textDidEndEditingNotification, object: pos,
                userInfo: ["NSTextMovement": NSTextMovement.return.rawValue]))
        #expect(f.state.tick == bar && !m.editor.textEditing)
        v.focus(.position)
        v.giveKeyboardBack(.position)
        #expect(w.firstResponder === w, "Return gives the keyboard back to the keys")
        // Escape reverts: nothing moves.
        v.controlTextDidBeginEditing(Notification(name: NSControl.textDidBeginEditingNotification, object: pos))
        pos.stringValue = "9"
        #expect(v.control(pos, textView: NSTextView(), doCommandBy: #selector(NSResponder.cancelOperation(_:))))
        #expect(f.state.tick == bar && !m.editor.textEditing)
        m.poll()
        v.refresh()
        #expect(pos.stringValue == "2.1.0")
        #expect(!v.control(pos, textView: NSTextView(), doCommandBy: #selector(NSResponder.insertNewline(_:))))
    }

    @Test func fieldKeysFocusTheirField() throws {
        let host = FakeControlHost()
        let f = demoFake()
        let s = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { f }, open: { _ in demoFake() })
        let id = try #require(s.newDemo())
        let m = try #require(s.model(id))
        for (cmd, field) in [("transport.go_position", "position"), ("transport.go_edit_in", "editIn"),
                             ("transport.go_punch", "punchIn")] {
            guard case .done = ControlCommands.run(cmd, args: [], doc: (id, m), store: s, host: host) else {
                Issue.record("\(cmd) not done")
                continue
            }
            #expect(host.calls.last == "focus \(field)")
        }
        host.fieldsShown = false
        guard case .failed(let e) = ControlCommands.run("transport.go_punch", args: [], doc: (id, m), store: s, host: host)
        else {
            Issue.record("no fields, no focus")
            return
        }
        #expect(e.code == .unavailable)
    }

    @Test func theRecorderTakesOneKeyAndEscapeCancels() throws {
        let r = KeyRecorderView()
        r.title = "Stop"
        var got: [String] = []
        r.onKey = { got.append($0) }
        #expect(r.accessibilityLabel() == "Record Key for Stop" && r.accessibilityRole() == .button)
        func key(_ code: UInt16, _ flags: NSEvent.ModifierFlags) throws -> NSEvent {
            try #require(
                NSEvent.keyEvent(
                    with: .keyDown, location: .zero, modifierFlags: flags, timestamp: 0, windowNumber: 0, context: nil,
                    characters: "k", charactersIgnoringModifiers: "k", isARepeat: false, keyCode: code))
        }
        #expect(!r.performKeyEquivalent(with: try key(40, [.command])), "not recording: the menus' key")
        #expect(r.accessibilityPerformPress() && r.recording)
        #expect(r.performKeyEquivalent(with: try key(40, [.command, .shift])))
        #expect(got == ["⇧⌘K"] && !r.recording)
        r.start()
        r.keyDown(with: try key(0xFFFF, []))
        #expect(r.recording && got.count == 1, "a key VizDAW cannot bind: still recording")
        r.keyDown(with: try key(53, []))
        #expect(!r.recording && got.count == 1, "Escape cancels")
        #expect((r as AnyObject) is KeyCapturing)
    }

    @Test func aRecordedKeyThatIsTakenWaitsForReplace() throws {
        let k = KeyBindings(defaults: MemoryDefaults())
        k.record("transport.stop", key: "X")
        #expect(k.row("transport.stop")?.keys == ["X"] && k.pending == nil)
        k.record("transport.stop", key: "Space")
        #expect(k.pending == .init(id: "transport.stop", key: "Space"))
        #expect(k.message == "Space is the key of “Play / Stop”. Replace it?")
        k.confirmReplace()
        #expect(k.row("transport.stop")?.keys == ["Space"] && k.row("transport.play_stop")?.keys == [])
        k.record("transport.play_stop", key: "Space")
        k.cancelReplace()
        #expect(k.pending == nil && k.message == nil && k.row("transport.play_stop")?.keys == [])
    }
}
VIZDAW_EOF
```

In `crates/core-control/tests/commands.rs`:

Find:

```rust
        ("O", "transport.overdub"),
        ("I", "transport.punch"),
        ("G", "editors.snap"),
        ("⌥⇧↑", "transport.arm_previous"),
        ("⌥⇧↓", "transport.arm_next"),
        ("1", "locator.go_1"),
```

Replace with:

```rust
        ("O", "transport.overdub"),
        ("I", "transport.punch"),
        ("G", "editors.snap"),
        ("P", "transport.go_position"),
        ("E", "transport.go_edit_in"),
        ("U", "transport.go_punch"),
        ("⌥⇧↑", "transport.arm_previous"),
        ("⌥⇧↓", "transport.arm_next"),
        ("1", "locator.go_1"),
```

Find:

```rust
    assert_eq!(ids.len(), COMMANDS.len(), "IDs are unique");
    assert_eq!(
        COMMANDS.len(),
        105,
        "44 before plan 4f, plus 48 transport, locator, record and key-binding commands, 13 step entry"
    );
    for c in COMMANDS {
        // snake_case words (a locator's number too), a dot after the group.
```

Replace with:

```rust
    assert_eq!(ids.len(), COMMANDS.len(), "IDs are unique");
    assert_eq!(
        COMMANDS.len(),
        108,
        "44 before plan 4f; 4f adds 51 transport, locator, record and key-binding commands and 13 step entry"
    );
    for c in COMMANDS {
        // snake_case words (a locator's number too), a dot after the group.
```

In `crates/core-ffi/tests/keys.rs`:

Find:

```rust
    );
    assert_eq!(vizdaw_ffi::key_code_for("nope".into()), None);
}
```

Replace with:

```rust
    );
    assert_eq!(vizdaw_ffi::key_code_for("nope".into()), None);
}

#[test]
fn a_key_press_reads_as_its_chord() {
    let m = |shift, option, command| vizdaw_ffi::ModifiersRow {
        shift,
        control: false,
        option,
        command,
    };
    assert_eq!(
        vizdaw_ffi::chord_text(40, m(true, false, true)).as_deref(),
        Some("⇧⌘K")
    );
    assert_eq!(
        vizdaw_ffi::chord_text(83, m(false, true, false)).as_deref(),
        Some("⌥Keypad1")
    );
    assert_eq!(vizdaw_ffi::chord_text(0xFFFF, m(false, false, false)), None);
}
```

- [ ] **Step 2: Run them to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-ffi --test keys 2>&1 | grep -E "^error" | head -2
scripts/swift.sh test --filter TransportFieldsTests 2>&1 | grep -E "error:" | head -3
```

Expected: FAIL to compile: no `chord_text` in `vizdaw_ffi`; `cannot find 'TransportFieldsView' in scope`.

- [ ] **Step 3: Implement**

In `app/Sources/VizDAW/AppCommands.swift`:

Find:

```swift
            commandToggle("transport.overdub", TransportCommands.self)
            commandToggle("transport.punch", TransportCommands.self)
            Divider()
            commandItem("transport.arm_previous", TransportCommands.self)
            commandItem("transport.arm_next", TransportCommands.self)
        }
```

Replace with:

```swift
            commandToggle("transport.overdub", TransportCommands.self)
            commandToggle("transport.punch", TransportCommands.self)
            Divider()
            fieldItem("transport.go_position")
            fieldItem("transport.go_edit_in")
            fieldItem("transport.go_punch")
            Divider()
            commandItem("transport.arm_previous", TransportCommands.self)
            commandItem("transport.arm_next", TransportCommands.self)
        }
```

Find:

```swift
        .disabled(doc.map { !kind.enabled(id, $0) } ?? true)
    }

    private func report(_ o: KeyOutcome, _ doc: DocumentModel) {
        if case .refused(let why) = o { doc.editor.status = why }
    }
```

Replace with:

```swift
        .disabled(doc.map { !kind.enabled(id, $0) } ?? true)
    }

    /// A key that focuses a transport field (plan 4f Ruling 16).
    private func fieldItem(_ id: String) -> some View {
        Button(keyBindings.row(id)?.title ?? id) {
            if let docID, let f = ControlCommands.fieldKeys[id] { _ = AppControlHost.shared.focus(f, docID) }
        }
        .keyboardShortcut(KeyMenu.shortcut(id))
        .disabled(docID == nil)
    }

    private func report(_ o: KeyOutcome, _ doc: DocumentModel) {
        if case .refused(let why) = o { doc.editor.status = why }
    }
```

In `app/Sources/VizDAW/ArrangementPane.swift`:

Find:

```swift
    var body: some View {
        VStack(spacing: 0) {
            ArrangementBar(model: model, editor: model.editor)
            Divider()
            if model.selectedSeq == nil {
                ContentUnavailableView(
```

Replace with:

```swift
    var body: some View {
        VStack(spacing: 0) {
            ArrangementBar(model: model, editor: model.editor)
            TransportBar(model: model)
            Divider()
            if model.selectedSeq == nil {
                ContentUnavailableView(
```

Find:

```swift
    }
}

/// The last failure of an editor action (spec §5: non-modal).
struct StatusLine: View {
    let editor: EditorModel
```

Replace with:

```swift
    }
}

/// The transport fields and step entry's line (plan 4f Rulings 11, 16).
struct TransportBar: View {
    let model: DocumentModel

    var body: some View {
        HStack(spacing: 12) {
            TransportFieldsHost(document: model)
                .frame(height: 22)
            if let s = model.stepEntry.summary {
                Text(s)
                    .font(.caption)
                    .padding(.horizontal, 6)
                    .padding(.vertical, 2)
                    .background(Color.accentColor.opacity(0.18), in: RoundedRectangle(cornerRadius: 4))
                    .accessibilityLabel(s)
            }
            Spacer()
        }
        .padding(.horizontal, 8)
        .padding(.bottom, 4)
    }
}

struct TransportFieldsHost: NSViewRepresentable {
    let document: DocumentModel

    func makeNSView(context: Context) -> TransportFieldsView { TransportFieldsView(document: document) }

    func updateNSView(_ view: TransportFieldsView, context: Context) {}
}

/// The last failure of an editor action (spec §5: non-modal).
struct StatusLine: View {
    let editor: EditorModel
```

In `app/Sources/VizDAW/ControlChannel.swift`:

Find:

```swift

    var nudge: NudgeSettings { editingPreferences.nudge }
    var keys: KeyBindings { keyBindings }
}
```

Replace with:

```swift

    var nudge: NudgeSettings { editingPreferences.nudge }
    var keys: KeyBindings { keyBindings }

    func focus(_ field: TransportField, _ doc: UUID) -> Bool {
        guard let w = ControlWindows.window(of: doc),
            let v = TransportFieldsView.find(in: w.contentView?.superview ?? w.contentView)
        else { return false }
        return v.focus(field)
    }
}
```

In `app/Sources/VizDAW/KeyMonitor.swift`:

Find:

```swift
import SwiftUI
import VizDAWAppModel
import VizDAWCore

/// A command's first key as a menu shortcut (plan 4f Ruling 6): menus
/// show the bindings, so a rebound key shows at once.
```

Replace with:

```swift
import SwiftUI
import VizDAWAppModel
import VizDAWCore
import VizDAWEditors

/// A command's first key as a menu shortcut (plan 4f Ruling 6): menus
/// show the bindings, so a rebound key shows at once.
```

Find:

```swift
    /// What a key press in `w` does: the route, decided as for the
    /// channel's keys (`menus`: AppKit's menus see the key next).
    static func route(_ e: NSEvent, in w: NSWindow, menus: Bool) -> (KeyRoute, (id: UUID, model: DocumentModel)?) {
        let typing = w.firstResponder is NSText
        let doc: (id: UUID, model: DocumentModel)? = ControlWindows.tagOf(w).doc.flatMap { d in
            appStore.model(d).map { (d, $0) }
```

Replace with:

```swift
    /// What a key press in `w` does: the route, decided as for the
    /// channel's keys (`menus`: AppKit's menus see the key next).
    static func route(_ e: NSEvent, in w: NSWindow, menus: Bool) -> (KeyRoute, (id: UUID, model: DocumentModel)?) {
        // The Keys settings' recorder takes every key it is shown.
        if w.firstResponder is KeyCapturing { return (.pass, nil) }
        let typing = w.firstResponder is NSText
        let doc: (id: UUID, model: DocumentModel)? = ControlWindows.tagOf(w).doc.flatMap { d in
            appStore.model(d).map { (d, $0) }
```

Create `app/Sources/VizDAW/KeysSettingsView.swift`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p app/Sources/VizDAW
cat > app/Sources/VizDAW/KeysSettingsView.swift <<'VIZDAW_EOF'
import AppKit
import SwiftUI
import VizDAWAppModel
import VizDAWCore
import VizDAWEditors

/// Settings > Keys (plan 4f Ruling 17): every command by menu with its
/// keys; record a new key, reset one or all, save and load a bindings
/// file, and keep the number row for locators.
struct KeysSettingsView: View {
    @Bindable var keys: KeyBindings

    private var groups: [(String, [BindingRow])] {
        var order: [String] = []
        var by: [String: [BindingRow]] = [:]
        for r in keys.rows {
            if by[r.menu] == nil { order.append(r.menu) }
            by[r.menu, default: []].append(r)
        }
        return order.map { ($0, by[$0] ?? []) }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Toggle("Number keys always go to locators (step entry uses the keypad)", isOn: $keys.reserveNumberRow)
            if let m = keys.message {
                HStack {
                    Text(m).font(.caption).foregroundStyle(.secondary).lineLimit(3)
                    if keys.pending != nil {
                        Button("Replace") { keys.confirmReplace() }
                        Button("Cancel") { keys.cancelReplace() }
                    }
                }
            }
            List {
                ForEach(groups, id: \.0) { group in
                    Section(group.0) {
                        ForEach(group.1, id: \.id) { row in
                            HStack {
                                Text(row.title)
                                if row.stepEntry {
                                    Text("step entry").font(.caption2).foregroundStyle(.secondary)
                                }
                                Spacer()
                                Text(row.keys.joined(separator: "  "))
                                    .font(.system(.body, design: .monospaced))
                                    .foregroundStyle(row.keys == row.defaults ? .primary : Color.accentColor)
                                KeyRecorder(title: row.title) { keys.record(row.id, key: $0) }
                                    .frame(width: 96, height: 20)
                                Button("Reset") { keys.reset(row.id) }
                                    .disabled(row.keys == row.defaults)
                            }
                        }
                    }
                }
            }
            HStack {
                Button("Save…") { save() }
                Button("Load…") { load() }
                Spacer()
                Button("Reset All") { keys.resetAll() }
            }
        }
        .padding()
        .frame(width: 620, height: 520)
    }

    private func save() {
        let panel = NSSavePanel()
        panel.nameFieldStringValue = "Key Bindings.vizkeys"
        guard panel.runModal() == .OK, let path = panel.url?.path else { return }
        do {
            try keys.save(to: path)
        } catch {
            keys.message = AlertItem(error, title: "").message
        }
    }

    private func load() {
        let panel = NSOpenPanel()
        panel.allowsMultipleSelection = false
        panel.canChooseDirectories = false
        guard panel.runModal() == .OK, let path = panel.url?.path else { return }
        try? keys.load(from: path)
    }
}

/// The AppKit key recorder in a row.
struct KeyRecorder: NSViewRepresentable {
    let title: String
    let onKey: (String) -> Void

    func makeNSView(context: Context) -> KeyRecorderView {
        let v = KeyRecorderView()
        v.title = title
        v.onKey = onKey
        return v
    }

    func updateNSView(_ v: KeyRecorderView, context: Context) {
        v.title = title
        v.onKey = onKey
    }
}
VIZDAW_EOF
```

In `app/Sources/VizDAW/VizDAWApp.swift`:

Find:

```swift
            PluginManagerView(model: pluginManager)
        }
        Settings {
            EditingSettingsView(prefs: editingPreferences)
        }
    }
}
```

Replace with:

```swift
            PluginManagerView(model: pluginManager)
        }
        Settings {
            TabView {
                EditingSettingsView(prefs: editingPreferences)
                    .tabItem { Label("Editing", systemImage: "pencil") }
                KeysSettingsView(keys: keyBindings)
                    .tabItem { Label("Keys", systemImage: "keyboard") }
            }
        }
    }
}
```

In `app/Sources/VizDAWAppModel/ControlCommands.swift`:

Find:

```swift
    var nudge: NudgeSettings { get }
    /// The app's key bindings (plan 4f).
    var keys: KeyBindings { get }
}

/// Runs registered menu commands (control spec §6) on a document model
```

Replace with:

```swift
    var nudge: NudgeSettings { get }
    /// The app's key bindings (plan 4f).
    var keys: KeyBindings { get }
    /// Gives a transport field of document `doc`'s window the keyboard
    /// (plan 4f Ruling 16); false when the window shows none.
    func focus(_ field: TransportField, _ doc: UUID) -> Bool
}

/// Runs registered menu commands (control spec §6) on a document model
```

Find:

```swift
        "transport.play_stop", "transport.loop", "transport.return_to_start",
        "keys.save", "keys.load", "keys.reset",
    ]

    /// Every registry ID `run` handles (a test checks it is all of them).
    public static var handled: Set<String> {
        other.union(detail.keys).union(torn.keys).union(nudges.keys).union(panels.keys).union(DrumCommands.ids)
            .union(TransportCommands.ids).union(StepCommands.ids)
    }

```

Replace with:

```swift
        "transport.play_stop", "transport.loop", "transport.return_to_start",
        "keys.save", "keys.load", "keys.reset",
    ]
    /// The keys that focus a transport field.
    public static let fieldKeys: [String: TransportField] = [
        "transport.go_position": .position, "transport.go_edit_in": .editIn, "transport.go_punch": .punchIn,
    ]

    /// Every registry ID `run` handles (a test checks it is all of them).
    public static var handled: Set<String> {
        other.union(detail.keys).union(torn.keys).union(nudges.keys).union(panels.keys).union(DrumCommands.ids)
            .union(fieldKeys.keys)
            .union(TransportCommands.ids).union(StepCommands.ids)
    }

```

Find:

```swift
            m.returnToStart()
        case _ where DrumCommands.ids.contains(id):
            if let f = failure(DrumCommands.run(id, on: args.flag("on"), m)) { return f }
        case _ where TransportCommands.ids.contains(id):
            if let f = failure(TransportCommands.run(id, on: args.flag("on"), m)) { return f }
        case _ where StepCommands.ids.contains(id):
```

Replace with:

```swift
            m.returnToStart()
        case _ where DrumCommands.ids.contains(id):
            if let f = failure(DrumCommands.run(id, on: args.flag("on"), m)) { return f }
        case _ where fieldKeys[id] != nil:
            guard host.focus(fieldKeys[id] ?? .position, docID) else {
                return unavailable("The window shows no transport fields.")
            }
        case _ where TransportCommands.ids.contains(id):
            if let f = failure(TransportCommands.run(id, on: args.flag("on"), m)) { return f }
        case _ where StepCommands.ids.contains(id):
```

In `app/Sources/VizDAWAppModel/KeyBindings.swift`:

Find:

```swift
    }
    /// What the last load skipped or why it failed, for the Keys page.
    public var message: String?

    public init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
```

Replace with:

```swift
    }
    /// What the last load skipped or why it failed, for the Keys page.
    public var message: String?
    /// A recorded key another command has, waiting for Replace.
    public private(set) var pending: PendingKey?

    /// A key the Keys page recorded that another command has.
    public struct PendingKey: Equatable, Sendable {
        public let id: String
        public let key: String
    }

    public init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
```

Find:

```swift
        changed()
    }

    public func reset(_ id: String) {
        keymap.resetOne(id: id)
        changed()
```

Replace with:

```swift
        changed()
    }

    /// The Keys page recorded `key` for `id` (plan 4f Ruling 17): it
    /// becomes the command's only key; one another command has waits in
    /// `pending` for `confirmReplace`.
    public func record(_ id: String, key: String) {
        do {
            try bind(id, keys: [key])
            pending = nil
            message = nil
        } catch {
            pending = PendingKey(id: id, key: key)
            message = AlertItem(error, title: "").message + ". Replace it?"
        }
    }

    /// Takes the pending key from the command that has it.
    public func confirmReplace() {
        guard let p = pending else { return }
        pending = nil
        message = nil
        try? bind(p.id, keys: [p.key], replace: true)
    }

    public func cancelReplace() {
        pending = nil
        message = nil
    }

    public func reset(_ id: String) {
        keymap.resetOne(id: id)
        changed()
```

Create `app/Sources/VizDAWAppModel/TransportFields.swift`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p app/Sources/VizDAWAppModel
cat > app/Sources/VizDAWAppModel/TransportFields.swift <<'VIZDAW_EOF'
import Foundation
import VizDAWCore

/// The transport bar's fields (parent spec §9, plan 4f Ruling 16): a key
/// focuses one; digits with separators enter bar·beat·unit; Enter
/// commits, Escape reverts.
public enum TransportField: String, CaseIterable, Sendable {
    case position, editIn, editOut, punchIn, punchOut

    public var title: String {
        switch self {
        case .position: "Position"
        case .editIn: "Edit In"
        case .editOut: "Edit Out"
        case .punchIn: "Punch In"
        case .punchOut: "Punch Out"
        }
    }
}

extension DocumentModel {
    /// The shown sequence's meter (empty: 4/4 from bar 1).
    var shownMeter: [MeterEntryRow] { selectedSeq.flatMap { editor.info($0)?.meter } ?? [] }

    /// The tick a field holds; nil for an edit range that is not set.
    public func fieldTick(_ f: TransportField) -> Int64? {
        switch f {
        case .position: transport.tick
        case .editIn: editor.selection.timeRange?.start
        case .editOut: editor.selection.timeRange?.end
        case .punchIn: editor.layout.transport.punchIn
        case .punchOut: editor.layout.transport.punchOut
        }
    }

    /// A field's text: bar.beat.unit, empty when it holds nothing.
    public func fieldText(_ f: TransportField) -> String {
        fieldTick(f).map { Bbu.of($0, meter: shownMeter).text } ?? ""
    }

    /// The tick typed text names, read against the field's position (an
    /// empty part keeps that part); `:`, `,` and spaces separate like dots.
    public func fieldInput(_ f: TransportField, _ text: String) -> Int64? {
        let meter = shownMeter
        let typed = text.trimmingCharacters(in: .whitespaces)
            .map { ":, ".contains($0) ? "." : String($0) }.joined()
        let current = Bbu.of(fieldTick(f) ?? 0, meter: meter)
        return PositionInput.parse(typed, current: current)?.tick(meter: meter)
    }

    /// Commits typed text to a field (plan 4f Ruling 16). False, with the
    /// status line saying why, for unreadable text or an impossible range.
    @discardableResult
    public func commitField(_ f: TransportField, _ text: String) -> Bool {
        guard let t = fieldInput(f, text), abs(t) <= TransportMarks.tickLimit else {
            editor.status = "“\(text)” is not a position (bar.beat.unit)."
            return false
        }
        let bar = MeterMath.barTicks(MeterMath.bar(at: t, shownMeter).meter)
        let sel = editor.selection
        switch f {
        case .position:
            do {
                try moveTo(t)
            } catch {
                editor.report(error)
                return false
            }
            return true
        case .editIn:
            let r = sel.timeRange
            let length = r.map { $0.end - $0.start } ?? bar
            let end = r.map { $0.end > t ? $0.end : t + length } ?? t + bar
            editor.setSelection(
                SelectionRow(timeRange: TickRange(start: t, end: end), lanes: sel.lanes, events: sel.events))
        case .editOut:
            guard let r = sel.timeRange else {
                editor.status = "Set Edit In first."
                return false
            }
            guard t > r.start else {
                editor.status = "Edit Out must be after Edit In."
                return false
            }
            editor.setSelection(
                SelectionRow(timeRange: TickRange(start: r.start, end: t), lanes: sel.lanes, events: sel.events))
        case .punchIn:
            let out = editor.layout.transport.punchOut
            editor.updateLayout { $0.transport.setPunch(in: t, out: out > t ? out : t + bar) }
        case .punchOut:
            guard t > editor.layout.transport.punchIn else {
                editor.status = "Punch Out must be after Punch In."
                return false
            }
            editor.updateLayout { $0.transport.setPunch(in: $0.transport.punchIn, out: t) }
        }
        return true
    }
}
VIZDAW_EOF
```

Create `app/Sources/VizDAWEditors/TransportFieldsView.swift`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p app/Sources/VizDAWEditors
cat > app/Sources/VizDAWEditors/TransportFieldsView.swift <<'VIZDAW_EOF'
import AppKit
import Observation
import VizDAWAppModel
import VizDAWCore

/// The transport bar's fields (parent spec §9, plan 4f Ruling 16):
/// Position, Edit In and Out, Punch In and Out, each a text field showing
/// bar.beat.unit. A key (P, E, U) focuses one; Enter commits the typed
/// position, Escape reverts. Each field is an accessibility element named
/// by its title, so `vizctl find Position` reaches it.
@MainActor
public final class TransportFieldsView: NSView, NSTextFieldDelegate {
    /// The text-editing owner while a field has the keyboard.
    public static let textOwner = "transport"
    public let document: DocumentModel
    public private(set) var fields: [TransportField: NSTextField] = [:]
    /// The field being typed in.
    private(set) var editing: TransportField?

    public init(document: DocumentModel) {
        self.document = document
        super.init(frame: .zero)
        let stack = NSStackView()
        stack.orientation = .horizontal
        stack.spacing = 6
        stack.translatesAutoresizingMaskIntoConstraints = false
        for f in TransportField.allCases {
            let label = NSTextField(labelWithString: f.title)
            label.font = .systemFont(ofSize: NSFont.smallSystemFontSize)
            let field = TransportTextField(string: "")
            field.font = .monospacedDigitSystemFont(ofSize: NSFont.systemFontSize, weight: .regular)
            field.alignment = .right
            field.delegate = self
            field.title = f.title
            field.identifier = NSUserInterfaceItemIdentifier("transport.\(f.rawValue)")
            field.widthAnchor.constraint(equalToConstant: 86).isActive = true
            fields[f] = field
            stack.addArrangedSubview(label)
            stack.addArrangedSubview(field)
            if f == .position || f == .editOut { stack.setCustomSpacing(16, after: field) }
        }
        addSubview(stack)
        NSLayoutConstraint.activate([
            stack.leadingAnchor.constraint(equalTo: leadingAnchor),
            stack.trailingAnchor.constraint(lessThanOrEqualTo: trailingAnchor),
            stack.topAnchor.constraint(equalTo: topAnchor),
            stack.bottomAnchor.constraint(equalTo: bottomAnchor),
        ])
        observe()
    }

    public required init?(coder: NSCoder) { nil }

    private func observe() {
        withObservationTracking {
            refresh()
        } onChange: {
            DispatchQueue.main.async { [weak self] in self?.observe() }
        }
    }

    /// Every field not being typed in shows its position.
    func refresh() {
        for (f, field) in fields where f != editing {
            field.stringValue = document.fieldText(f)
        }
    }

    /// Gives `f` the keyboard with its text selected (the P, E and U keys).
    @discardableResult
    public func focus(_ f: TransportField) -> Bool {
        guard let field = fields[f] as? TransportTextField, let w = window else { return false }
        field.armed = true
        guard w.makeFirstResponder(field) else { return false }
        field.currentEditor()?.selectAll(nil)
        return true
    }

    func field(of obj: Notification) -> TransportField? {
        fields.first { $0.value === obj.object as? NSTextField }?.key
    }

    public func controlTextDidBeginEditing(_ obj: Notification) {
        editing = field(of: obj)
        document.editor.beginTextEditing(Self.textOwner)
    }

    public func controlTextDidEndEditing(_ obj: Notification) {
        guard let f = field(of: obj) ?? editing, let text = fields[f]?.stringValue else { return }
        editing = nil
        document.editor.endTextEditing(Self.textOwner)
        let movement = (obj.userInfo?["NSTextMovement"] as? Int).flatMap(NSTextMovement.init(rawValue:))
        if movement != .cancel, text != document.fieldText(f) { document.commitField(f, text) }
        (fields[f] as? TransportTextField)?.armed = false
        refresh()
        // Enter commits and gives the keyboard back to the keys (Tab moves
        // on to the next field). Next turn: the field selects its text
        // again after this notification.
        if movement == .return {
            DispatchQueue.main.async { [weak self] in self?.giveKeyboardBack(f) }
        }
    }

    /// The window's keyboard goes back to the window if `f` still has it.
    func giveKeyboardBack(_ f: TransportField) {
        guard let w = window, let field = fields[f], let editor = field.currentEditor(),
            w.firstResponder === editor
        else { return }
        w.makeFirstResponder(nil)
    }

    /// Escape reverts the field and gives the keyboard back.
    public func control(_ control: NSControl, textView: NSTextView, doCommandBy selector: Selector) -> Bool {
        guard selector == #selector(NSResponder.cancelOperation(_:)),
            let f = fields.first(where: { $0.value === control })?.key
        else { return false }
        editing = nil
        document.editor.endTextEditing(Self.textOwner)
        fields[f]?.abortEditing()
        (fields[f] as? TransportTextField)?.armed = false
        refresh()
        window?.makeFirstResponder(nil)
        return true
    }

    /// The fields view inside `view`, if any (the P, E and U keys find it).
    public static func find(in view: NSView?) -> TransportFieldsView? {
        guard let view else { return nil }
        if let v = view as? TransportFieldsView { return v }
        for s in view.subviews {
            if let v = find(in: s) { return v }
        }
        return nil
    }

    /// Torn down mid-edit: give the keyboard back to the transport keys.
    public override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        if window == nil, editing != nil {
            editing = nil
            document.editor.endTextEditing(Self.textOwner)
        }
    }
}

/// A transport field takes the keyboard only when clicked or focused by
/// its key, never as the window's first text field when it opens, so the
/// transport and step keys work from the start (plan 4f Ruling 16).
final class TransportTextField: NSTextField {
    var armed = false
    /// Its name for VoiceOver and the control channel ("Position").
    var title = "" {
        didSet {
            setAccessibilityLabel(title)
            cell?.setAccessibilityLabel(title)
        }
    }

    override func accessibilityLabel() -> String? { title }

    /// A click the control channel sends is never a window-activating
    /// first click (control spec §5.5).
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool {
        ControlUI.isChannel(event) || super.acceptsFirstMouse(for: event)
    }

    override func mouseDown(with event: NSEvent) {
        armed = true
        super.mouseDown(with: event)
    }

    override func becomeFirstResponder() -> Bool {
        guard armed else { return false }
        return super.becomeFirstResponder()
    }

    override func textDidEndEditing(_ notification: Notification) {
        super.textDidEndEditing(notification)
        armed = false
    }
}

/// A view that takes every key while it records one (the Keys settings'
/// recorder): the key bindings leave its keys alone (plan 4f Ruling 17).
@MainActor
public protocol KeyCapturing: AnyObject {}

/// Records one key for a binding (plan 4f Ruling 17): press it (or press
/// its button with VoiceOver), then press the key; Escape cancels.
@MainActor
public final class KeyRecorderView: NSView, KeyCapturing {
    /// The command whose key is recorded (its title names the button).
    public var title = ""
    /// Called with the recorded key's chord text (`⇧⌘K`).
    public var onKey: ((String) -> Void)?
    public private(set) var recording = false {
        didSet { needsDisplay = true }
    }

    public override var acceptsFirstResponder: Bool { true }

    public override func mouseDown(with event: NSEvent) { start() }

    public func start() {
        recording = true
        window?.makeFirstResponder(self)
    }

    public override func resignFirstResponder() -> Bool {
        recording = false
        return super.resignFirstResponder()
    }

    /// Takes the key before the menus do (⌘ keys included).
    public override func performKeyEquivalent(with event: NSEvent) -> Bool {
        guard recording else { return super.performKeyEquivalent(with: event) }
        take(event)
        return true
    }

    public override func keyDown(with event: NSEvent) {
        if recording { take(event) } else { super.keyDown(with: event) }
    }

    /// Records `event`'s key (Escape alone cancels); a key VizDAW cannot
    /// bind is ignored and recording goes on.
    func take(_ event: NSEvent) {
        let f = event.modifierFlags
        let m = ModifiersRow(
            shift: f.contains(.shift), control: f.contains(.control), option: f.contains(.option),
            command: f.contains(.command))
        if event.keyCode == 53, !(m.shift || m.control || m.option || m.command) {
            recording = false
            return
        }
        guard let text = chordText(keyCode: event.keyCode, modifiers: m) else { return }
        recording = false
        onKey?(text)
    }

    public override func draw(_ dirtyRect: NSRect) {
        let r = bounds.insetBy(dx: 1, dy: 1)
        (recording ? NSColor.controlAccentColor : NSColor.separatorColor).setStroke()
        let p = NSBezierPath(roundedRect: r, xRadius: 4, yRadius: 4)
        p.stroke()
        let s = recording ? "Press a key…" : "Record"
        let a: [NSAttributedString.Key: Any] = [
            .font: NSFont.systemFont(ofSize: NSFont.smallSystemFontSize),
            .foregroundColor: NSColor.labelColor,
        ]
        let size = s.size(withAttributes: a)
        s.draw(at: CGPoint(x: r.midX - size.width / 2, y: r.midY - size.height / 2), withAttributes: a)
    }

    public override func isAccessibilityElement() -> Bool { true }
    public override func accessibilityRole() -> NSAccessibility.Role? { .button }
    public override func accessibilityLabel() -> String? { "Record Key for \(title)" }
    public override func accessibilityPerformPress() -> Bool {
        start()
        return true
    }
}
VIZDAW_EOF
```

In `crates/core-control/src/registry.rs`:

Find:

```rust
        params: ON,
        ..cmd("transport.punch", "Transport", "Punch", Some("I"), true)
    },
    cmd(
        "transport.arm_previous",
        "Transport",
```

Replace with:

```rust
        params: ON,
        ..cmd("transport.punch", "Transport", "Punch", Some("I"), true)
    },
    cmd(
        "transport.go_position",
        "Transport",
        "Go to Position Field",
        Some("P"),
        true,
    ),
    cmd(
        "transport.go_edit_in",
        "Transport",
        "Go to Edit-In Field",
        Some("E"),
        true,
    ),
    cmd(
        "transport.go_punch",
        "Transport",
        "Go to Punch Fields",
        Some("U"),
        true,
    ),
    cmd(
        "transport.arm_previous",
        "Transport",
```

In `crates/core-ffi/src/keys.rs`:

Find:

```rust
pub fn key_code_for(name: String) -> Option<u16> {
    Chord::of_slot(&name, 0).map(|c| c.code)
}
```

Replace with:

```rust
pub fn key_code_for(name: String) -> Option<u16> {
    Chord::of_slot(&name, 0).map(|c| c.code)
}

/// The chord text of a key press (`⇧⌘K`), or None for a key VizDAW does
/// not bind (the Keys settings' recorder).
#[uniffi::export]
pub fn chord_text(key_code: u16, modifiers: ModifiersRow) -> Option<String> {
    Chord::new(key_code, mods_bits(&modifiers)).map(|c| c.text())
}
```

In `crates/core-ffi/src/lib.rs`:

Find:

```rust
};
pub use guard::ffi_guard;
pub use input::{NoteInRow, NoteInput};
pub use keys::{BindingRow, Keymap, key_code_for};
pub use mixer::{
    BusRow, DestRow, MeterRow, MixerLevels, MixerSnapshot, SendRow, StripKindRow, StripRow,
};
```

Replace with:

```rust
};
pub use guard::ffi_guard;
pub use input::{NoteInRow, NoteInput};
pub use keys::{BindingRow, Keymap, chord_text, key_code_for};
pub use mixer::{
    BusRow, DestRow, MeterRow, MixerLevels, MixerSnapshot, SendRow, StripKindRow, StripRow,
};
```

- [ ] **Step 4: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-ffi --test keys 2>&1 | grep -E "^test result"
scripts/build-ffi.sh && scripts/swift.sh test --filter 'TransportFieldsTests|ControlCommandsTests' 2>&1 | tail -1
```

Expected: `keys` 7 passed; 23 Swift tests in 2 suites pass.

- [ ] **Step 5: Gate and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace 2>&1 | grep -E "FAILED|panicked" ; cargo test --workspace -- --list 2>/dev/null | grep -c ': test$'
scripts/build-ffi.sh && scripts/swift.sh build 2>&1 | tail -1 && scripts/swift.sh test 2>&1 | tail -1
touch /tmp/vizdaw-4f-e2e-marker
scripts/make-app.sh && VIZDAW_E2E_APP=dist/VizDAW.app cargo test -p vizctl --test e2e -- --ignored --test-threads 1 2>&1 | grep -E "test result|panicked"
pgrep -lf MacOS/VizDAW ; find /private/tmp -maxdepth 1 -name 'vze*' -newer /tmp/vizdaw-4f-e2e-marker -exec rm -rf {} +
git add app/Sources/VizDAW/AppCommands.swift \
  app/Sources/VizDAW/ArrangementPane.swift \
  app/Sources/VizDAW/ControlChannel.swift \
  app/Sources/VizDAW/KeyMonitor.swift \
  app/Sources/VizDAW/KeysSettingsView.swift \
  app/Sources/VizDAW/VizDAWApp.swift \
  app/Sources/VizDAWAppModel/ControlCommands.swift \
  app/Sources/VizDAWAppModel/KeyBindings.swift \
  app/Sources/VizDAWAppModel/TransportFields.swift \
  app/Sources/VizDAWEditors/TransportFieldsView.swift \
  app/Tests/VizDAWAppModelTests/ControlCommandsTests.swift \
  app/Tests/VizDAWAppModelTests/TransportFieldsTests.swift \
  crates/core-control/src/registry.rs \
  crates/core-control/tests/commands.rs \
  crates/core-ffi/src/keys.rs \
  crates/core-ffi/src/lib.rs \
  crates/core-ffi/tests/keys.rs
git commit -m "feat(keys): transport fields, focus keys and Settings > Keys

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
git log -1 --format='%h %s'
```

Expected: 905 Rust tests listed (904 + 1); Swift 648 (641 + 7); e2e 3 passed. No `FAILED` or `panicked` line (rerun once if the known-flaky MIDI listener test trips, and say so); `pgrep` prints nothing.

---

### Task 11: The channel's state for keys and step entry; the keys end to end

**Files:**
- Modify: `app/Sources/VizDAWAppModel/ControlCommands.swift`, `app/Sources/VizDAWAppModel/StepEntry.swift`, `crates/core-ffi/src/control.rs`, `crates/core-ffi/src/lib.rs`
- Test: `app/Tests/VizDAWAppModelTests/FakeSession.swift`, `app/Tests/VizDAWAppModelTests/StepEntryTests.swift`, `app/Tests/VizDAWEditorsTests/ControlUITests.swift`, `crates/core-ffi/tests/control.rs`, `crates/vizctl/tests/e2e.rs`

**Interfaces:**
- Consumes: everything above; `vizctl::launch`, `core_engine::fixtures::Builder`, the e2e's `App`, `wait_for_label`, `wait_for_undo`.
- Produces: `EditorStateRow.{step: Option<StepEntryStateRow>, marks: TransportMarksRow}`; `StepEntryStateRow {seq, lane, tick, value, dotted, triplet, steps}`; `TransportMarksRow {locators, punch_in, punch_out, punch, overdub}`; JSON `state.get editor` → `step` (null while off) and `transport`; `KeyStrokeRow.command` removed; `StepEntryModel.wireName(_:)`; `ControlStates.{stepRow(_:), marksRow(_:)}`; e2e `keys_drive_step_entry_locators_and_bindings` with helpers `wait_for_editor`, `wait_for_tick`.

Ruling 18. The e2e drives README steps 99–108's main flow with keys only (plus `step.notes` for the MIDI keyboard and `keys.load` for a bindings file): T, 4, a chord, 0, Delete, Delete, T; P, typed `3` and Return, ⌥2, Return, 2; a bindings file that moves Go to Locator 2 to X; G. Every wait is a condition with the e2e's generous timeout.

- [ ] **Step 1: Write the failing tests**

In `app/Tests/VizDAWAppModelTests/StepEntryTests.swift`:

Find:

```swift
        }
    }

    @Test func aRefusedStepShowsInTheStatusLine() throws {
        let (s, id, m, f) = try demo()
        _ = run("step.entry", m, s, id)
```

Replace with:

```swift
        }
    }

    @Test func theEditorStateShowsStepEntryAndTheMarks() throws {
        let (s, id, m, _) = try demo()
        func state() -> EditorStateRow? {
            if case .editor(_, let e)? = ControlStates.state(.editor, id, m) { return e }
            return nil
        }
        #expect(state()?.step == nil)
        #expect(state()?.marks.locators == Array(repeating: nil, count: 10))
        _ = run("step.entry", m, s, id)
        _ = run("step.thirty_second", m, s, id)
        _ = run("step.rest", m, s, id)
        #expect(
            state()?.step
                == StepEntryStateRow(seq: 3, lane: 30, tick: 15_360, value: "thirty_second", dotted: false,
                                     triplet: false, steps: 0),
            "the fake moves without history")
        _ = run("transport.punch", m, s, id)
        #expect(state()?.marks.punch == true)
    }

    @Test func aRefusedStepShowsInTheStatusLine() throws {
        let (s, id, m, f) = try demo()
        _ = run("step.entry", m, s, id)
```

In `app/Tests/VizDAWEditorsTests/ControlUITests.swift`:

Find:

```swift
        w.makeFirstResponder(r)
        let cmdS = KeyStrokeRow(
            keyCode: 1, characters: "s", unmodified: "s",
            modifiers: ModifiersRow(shift: false, control: false, option: false, command: true), command: "file.save")
        let space = KeyStrokeRow(keyCode: 49, characters: " ", unmodified: " ", modifiers: none, command: nil)
        let events = ControlUI.keyEvents(w, [space, cmdS])
        #expect(events.map(\.type) == [NSEvent.EventType.keyDown, .keyUp, .keyDown, .keyUp])
        ControlUI.deliver(events, to: w)
```

Replace with:

```swift
        w.makeFirstResponder(r)
        let cmdS = KeyStrokeRow(
            keyCode: 1, characters: "s", unmodified: "s",
            modifiers: ModifiersRow(shift: false, control: false, option: false, command: true))
        let space = KeyStrokeRow(keyCode: 49, characters: " ", unmodified: " ", modifiers: none)
        let events = ControlUI.keyEvents(w, [space, cmdS])
        #expect(events.map(\.type) == [NSEvent.EventType.keyDown, .keyUp, .keyDown, .keyUp])
        ControlUI.deliver(events, to: w)
```

In `crates/core-ffi/tests/control.rs`:

Find:

```rust
        control_element_check("AXButton".into(), Some(String::new()), Some("Play".into()))
    );
}
```

Replace with:

```rust
        control_element_check("AXButton".into(), Some(String::new()), Some("Play".into()))
    );
}

#[test]
fn the_editor_state_shows_step_entry_and_the_transport_marks() {
    let editor = |step| vizdaw_ffi::EditorStateRow {
        detail: None,
        windows: vec![],
        tool: "arrow".into(),
        snap_enabled: true,
        snap_division: "thirty_second".into(),
        exact_mode: false,
        gesture: false,
        status: None,
        step,
        marks: vizdaw_ffi::TransportMarksRow {
            locators: vec![None, Some(61_440)],
            punch_in: 0,
            punch_out: 245_760,
            punch: true,
            overdub: false,
        },
    };
    let off = result_json(&ControlResult::Editor {
        doc: "D".into(),
        editor: editor(None),
    });
    assert_eq!(off["step"], Value::Null);
    assert_eq!(off["snap"]["division"], "thirty_second");
    assert_eq!(
        off["transport"],
        json!({"locators": [null, 61_440], "punch_in": 0, "punch_out": 245_760, "punch": true, "overdub": false})
    );
    let on = result_json(&ControlResult::Editor {
        doc: "D".into(),
        editor: editor(Some(vizdaw_ffi::StepEntryStateRow {
            seq: 3,
            lane: 31,
            tick: 7_680,
            value: "eighth".into(),
            dotted: true,
            triplet: false,
            steps: 2,
        })),
    });
    assert_eq!(
        on["step"],
        json!({"seq": 3, "lane": 31, "tick": 7_680, "value": "eighth", "dotted": true, "triplet": false, "steps": 2})
    );
}
```

In `crates/vizctl/tests/e2e.rs`:

Find:

```rust
    app.quit();
}

/// The ID of the tree's button labelled (or titled) exactly "Play".
fn play_button(nodes: &Value) -> Option<Value> {
    nodes.as_array().into_iter().flatten().find_map(|n| {
```

Replace with:

```rust
    app.quit();
}

/// Polls `state.get editor` until `ok` holds; returns it.
fn wait_for_editor(
    app: &mut App,
    window: &Value,
    what: &str,
    ok: impl Fn(&Value) -> bool,
) -> Value {
    let deadline = Instant::now() + Duration::from_millis(WAIT_MS);
    loop {
        let e = app.call("state.get", json!({"target": "editor", "window": window}));
        if ok(&e) {
            return e;
        }
        assert!(Instant::now() < deadline, "{what} within {WAIT_MS} ms: {e}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// The transport's tick, read until it is `tick`.
fn wait_for_tick(app: &mut App, window: &Value, tick: i64) {
    let deadline = Instant::now() + Duration::from_millis(WAIT_MS);
    loop {
        let t = app.call(
            "state.get",
            json!({"target": "transport", "window": window}),
        );
        if t["tick"].as_i64() == Some(tick) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "tick {tick} within {WAIT_MS} ms: {t}"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// README steps 99–104 through the channel (plan 4f): keys by the
/// bindings drive step entry (T on, 4 an eighth, a typed chord, 0 a
/// rest, Delete takes steps back), P focuses the Position field and a
/// typed bar moves there, ⌥2 sets a locator and 2 jumps to it, a loaded
/// bindings file moves that key to X, G toggles snap.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn keys_drive_step_entry_locators_and_bindings() {
    use core_engine::fixtures::Builder;
    const Q: i64 = 15_360;
    let mut app = App::start();
    let mut b = Builder::new();
    let synth = b.dest("Synth", "builtin:synth", 0).unwrap();
    b.sequence("Tune", "Keys", Some(synth)).unwrap();
    let path = app.launched.home.join("Tune.vizdaw");
    core_model::file::save(&b.finish(), &path).unwrap();
    let window = app.call("app.open", json!({"path": path}))["window"].clone();
    app.wait(
        &window,
        json!({"condition": "documents_at_least", "count": 1}),
    );
    let key = |app: &mut App, k: &str, mods: Value| {
        app.call(
            "ui.key",
            json!({"window": window, "key": k, "modifiers": mods}),
        );
    };

    // Step entry: T, then 4 for eighths.
    key(&mut app, "t", json!([]));
    wait_for_editor(&mut app, &window, "step entry on", |e| {
        e["step"]["value"] == "quarter"
    });
    key(&mut app, "4", json!([]));
    wait_for_editor(&mut app, &window, "eighths", |e| {
        e["step"]["value"] == "eighth"
    });
    // A chord (MIDI input's stand-in), then a rest: the position moves on.
    app.call(
        "command.run",
        json!({"id": "step.notes", "params": {"pitches": "C4 E4 G4"}, "window": window}),
    );
    wait_for_undo(&mut app, &window, "Step Entry");
    let sel = app.call(
        "state.get",
        json!({"target": "selection", "window": window}),
    );
    assert_eq!(
        sel["events"].as_array().unwrap().len(),
        3,
        "the chord is selected: {sel}"
    );
    wait_for_tick(&mut app, &window, Q / 2);
    key(&mut app, "0", json!([]));
    wait_for_editor(&mut app, &window, "a rest", |e| e["step"]["tick"] == Q);
    // Delete takes the rest back, then the chord.
    key(&mut app, "delete", json!([]));
    wait_for_editor(&mut app, &window, "the rest back", |e| {
        e["step"]["tick"] == Q / 2
    });
    key(&mut app, "delete", json!([]));
    wait_for_undo(&mut app, &window, "Delete Last Step");
    wait_for_tick(&mut app, &window, 0);
    key(&mut app, "t", json!([]));
    wait_for_editor(&mut app, &window, "step entry off", |e| e["step"].is_null());

    // P gives the Position field the keyboard, a typed bar and Return
    // move there; ⌥2 sets locator 2 there.
    wait_for_label(&mut app, &window, "Position");
    key(&mut app, "p", json!([]));
    app.call("ui.key", json!({"window": window, "text": "3\n"}));
    wait_for_tick(&mut app, &window, 8 * Q);
    key(&mut app, "2", json!(["option"]));
    wait_for_editor(&mut app, &window, "locator 2", |e| {
        e["transport"]["locators"][2] == 8 * Q
    });
    key(&mut app, "return", json!([]));
    wait_for_tick(&mut app, &window, 0);
    key(&mut app, "2", json!([]));
    wait_for_tick(&mut app, &window, 8 * Q);

    // A bindings file moves Go to Locator 2 to X.
    let keys = app.launched.home.join("mine.vizkeys");
    std::fs::write(
        &keys,
        r#"{"vizdaw_keys": 1, "bindings": {"locator.go_2": ["X"]}}"#,
    )
    .unwrap();
    app.call(
        "command.run",
        json!({"id": "keys.load", "params": {"path": keys}}),
    );
    key(&mut app, "return", json!([]));
    wait_for_tick(&mut app, &window, 0);
    key(&mut app, "2", json!([]));
    let t = app.call(
        "state.get",
        json!({"target": "transport", "window": window}),
    );
    assert_eq!(t["tick"], 0, "2 no longer goes to the locator");
    key(&mut app, "x", json!([]));
    wait_for_tick(&mut app, &window, 8 * Q);
    app.call("command.run", json!({"id": "keys.reset"}));

    // G toggles snap; the division reads snake_case.
    let e = wait_for_editor(&mut app, &window, "snap on", |e| {
        e["snap"]["enabled"] == true
    });
    assert_eq!(e["snap"]["division"], "quarter");
    key(&mut app, "g", json!([]));
    wait_for_editor(&mut app, &window, "snap off", |e| {
        e["snap"]["enabled"] == false
    });
    app.quit();
}

/// The ID of the tree's button labelled (or titled) exactly "Play".
fn play_button(nodes: &Value) -> Option<Value> {
    nodes.as_array().into_iter().flatten().find_map(|n| {
```

- [ ] **Step 2: Run them to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-ffi --test control 2>&1 | grep -E "^error" | head -3
```

Expected: FAIL to compile: `cannot find struct StepEntryStateRow in crate vizdaw_ffi`, missing fields `step` and `marks`.

- [ ] **Step 3: Implement**

In `app/Sources/VizDAWAppModel/ControlCommands.swift`:

Find:

```swift
                    windows: e.layout.windows.map { TornRow(id: $0.id.uuidString, detail: detailRow($0.detail)) },
                    tool: e.tool.rawValue, snapEnabled: e.layout.snap.enabled,
                    snapDivision: e.layout.snap.division.rawValue, exactMode: e.exactMode,
                    gesture: e.gesture != nil, status: e.status))
        case .selection:
            return .selection(doc: doc, selection: m.editor.selection)
        case .windows:
```

Replace with:

```swift
                    windows: e.layout.windows.map { TornRow(id: $0.id.uuidString, detail: detailRow($0.detail)) },
                    tool: e.tool.rawValue, snapEnabled: e.layout.snap.enabled,
                    snapDivision: e.layout.snap.division.rawValue, exactMode: e.exactMode,
                    gesture: e.gesture != nil, status: e.status, step: stepRow(m.stepEntry),
                    marks: marksRow(e.layout.transport)))
        case .selection:
            return .selection(doc: doc, selection: m.editor.selection)
        case .windows:
```

Find:

```swift
        }
    }

    /// What a wait reads of document `m` (its transport read fresh: a
    /// window in the background does not poll).
    public static func facts(_ m: DocumentModel) -> DocFactsRow {
```

Replace with:

```swift
        }
    }

    /// Step entry for `state.get editor` (plan 4f).
    static func stepRow(_ s: StepEntryModel) -> StepEntryStateRow? {
        guard let st = s.state, let t = s.target else { return nil }
        return StepEntryStateRow(
            seq: t.seq, lane: t.lane, tick: st.tick, value: StepEntryModel.wireName(st.value), dotted: st.dotted,
            triplet: st.triplet, steps: UInt32(st.history.count))
    }

    static func marksRow(_ t: TransportMarks) -> TransportMarksRow {
        TransportMarksRow(
            locators: t.locators, punchIn: t.punchIn, punchOut: t.punchOut, punch: t.punch, overdub: t.overdub)
    }

    /// What a wait reads of document `m` (its transport read fresh: a
    /// window in the background does not poll).
    public static func facts(_ m: DocumentModel) -> DocFactsRow {
```

In `app/Sources/VizDAWAppModel/StepEntry.swift`:

Find:

```swift
        return "Step Entry · \(lane) · \(v)"
    }

    public static func title(_ v: NoteValueRow) -> String {
        switch v {
        case .whole: "1/1"
```

Replace with:

```swift
        return "Step Entry · \(lane) · \(v)"
    }

    /// The value's snake_case name on the control channel.
    public static func wireName(_ v: NoteValueRow) -> String {
        switch v {
        case .whole: "whole"
        case .half: "half"
        case .quarter: "quarter"
        case .eighth: "eighth"
        case .sixteenth: "sixteenth"
        case .thirtySecond: "thirty_second"
        }
    }

    public static func title(_ v: NoteValueRow) -> String {
        switch v {
        case .whole: "1/1"
```

In `crates/core-ffi/src/control.rs`:

Find:

```rust
    pub characters: String,
    /// `NSEvent.charactersIgnoringModifiers`.
    pub unmodified: String,
    /// Held modifiers.
    pub modifiers: ModifiersRow,
    /// The menu command this stroke is the shortcut of: the app runs it
    /// on the target window's document and gives the key to the window
    /// only when the command is unavailable (as AppKit does with a
    /// disabled menu item).
    pub command: Option<String>,
}

/// A mouse button.
```

Replace with:

```rust
    pub characters: String,
    /// `NSEvent.charactersIgnoringModifiers`.
    pub unmodified: String,
    /// Held modifiers. (What the stroke runs is the app's: its key
    /// bindings decide, plan 4f Ruling 5.)
    pub modifiers: ModifiersRow,
}

/// A mouse button.
```

Find:

```rust
    pub gesture: bool,
    /// The editors' status line.
    pub status: Option<String>,
}

/// Kinds of VizDAW window.
```

Replace with:

```rust
    pub gesture: bool,
    /// The editors' status line.
    pub status: Option<String>,
    /// Step entry while it is on (plan 4f).
    pub step: Option<StepEntryStateRow>,
    /// The transport's marks (plan 4f Ruling 7).
    pub marks: TransportMarksRow,
}

/// Step entry as `state.get editor` shows it (plan 4f).
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Record)]
pub struct StepEntryStateRow {
    /// The sequence steps go into.
    pub seq: u64,
    /// The lane.
    pub lane: u64,
    /// Where the next step goes.
    pub tick: i64,
    /// The note value, snake_case (`quarter`, `thirty_second`).
    pub value: String,
    /// Dotted.
    pub dotted: bool,
    /// Triplet.
    pub triplet: bool,
    /// Steps Delete Last Step can take back.
    pub steps: u32,
}

/// The transport's locators, punch range and record modes.
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Record)]
pub struct TransportMarksRow {
    /// Locators 0–9 (None: not set).
    pub locators: Vec<Option<i64>>,
    /// Punch in.
    pub punch_in: i64,
    /// Punch out.
    pub punch_out: i64,
    /// Punch on.
    pub punch: bool,
    /// Overdub on.
    pub overdub: bool,
}

/// Kinds of VizDAW window.
```

Find:

```rust

fn stroke_row(k: KeyStroke) -> KeyStrokeRow {
    KeyStrokeRow {
        command: registry::command_for_stroke(&k).map(str::to_string),
        key_code: k.key_code,
        characters: k.characters,
        unmodified: k.unmodified,
```

Replace with:

```rust

fn stroke_row(k: KeyStroke) -> KeyStrokeRow {
    KeyStrokeRow {
        key_code: k.key_code,
        characters: k.characters,
        unmodified: k.unmodified,
```

Find:

```rust
            "tool": e.tool,
            "snap": { "enabled": e.snap_enabled, "division": e.snap_division },
            "exact_mode": e.exact_mode, "gesture": e.gesture, "status": e.status,
        }),
        ControlResult::Selection { doc, selection: s } => json!({
            "doc": doc,
```

Replace with:

```rust
            "tool": e.tool,
            "snap": { "enabled": e.snap_enabled, "division": e.snap_division },
            "exact_mode": e.exact_mode, "gesture": e.gesture, "status": e.status,
            "step": e.step.as_ref().map(|s| json!({
                "seq": s.seq, "lane": s.lane, "tick": s.tick, "value": s.value,
                "dotted": s.dotted, "triplet": s.triplet, "steps": s.steps,
            })),
            "transport": {
                "locators": e.marks.locators, "punch_in": e.marks.punch_in,
                "punch_out": e.marks.punch_out, "punch": e.marks.punch, "overdub": e.marks.overdub,
            },
        }),
        ControlResult::Selection { doc, selection: s } => json!({
            "doc": doc,
```

In `crates/core-ffi/src/lib.rs`:

Find:

```rust
    AlertRow, ControlArg, ControlCall, ControlErrorCode, ControlFailure, ControlRequest,
    ControlResult, ControlServer, ControlValue, CrashAnswerRow, DetailRow, DocFactsRow,
    EditorStateRow, ElementRefRow, KeyStrokeRow, ModifiersRow, MouseButtonRow, PointerRow,
    RecoveryAnswersRow, StateTargetRow, TornRow, UiNodeRow, WaitConditionRow, WaitFactsRow,
    WindowKindRow, WindowRefRow, WindowRow, control_command_ids, control_element_check,
    control_host_busy, control_wait_met,
};
pub use drum::{
    DrumBandKindRow, DrumBandRow, DrumCellRow, DrumEdit, DrumGridLayoutRow, DrumGridSettingsRow,
```

Replace with:

```rust
    AlertRow, ControlArg, ControlCall, ControlErrorCode, ControlFailure, ControlRequest,
    ControlResult, ControlServer, ControlValue, CrashAnswerRow, DetailRow, DocFactsRow,
    EditorStateRow, ElementRefRow, KeyStrokeRow, ModifiersRow, MouseButtonRow, PointerRow,
    RecoveryAnswersRow, StateTargetRow, StepEntryStateRow, TornRow, TransportMarksRow, UiNodeRow,
    WaitConditionRow, WaitFactsRow, WindowKindRow, WindowRefRow, WindowRow, control_command_ids,
    control_element_check, control_host_busy, control_wait_met,
};
pub use drum::{
    DrumBandKindRow, DrumBandRow, DrumCellRow, DrumEdit, DrumGridLayoutRow, DrumGridSettingsRow,
```

In `app/Tests/VizDAWAppModelTests/FakeSession.swift`:

Find:

```swift
    }

    /// `stepEntry` calls (plan 4f) and how they are answered: by
    /// `stepAnswer`, else a step's quarter note forward for notes, rests,
    /// ties and references (with a new snapshot for notes and references).
    var stepCalls: [(seq: UInt64, lane: UInt64, state: StepStateRow, input: StepInputRow)] = []
    var stepEntryError: VizError?
    var stepAnswer: ((StepStateRow, StepInputRow) -> StepResultRow)?
```

Replace with:

```swift
    }

    /// `stepEntry` calls (plan 4f) and how they are answered: by
    /// `stepAnswer`, else a quarter note forward for notes, rests, ties
    /// and references (with a new snapshot for notes and references), a
    /// value key sets the value.
    var stepCalls: [(seq: UInt64, lane: UInt64, state: StepStateRow, input: StepInputRow)] = []
    var stepEntryError: VizError?
    var stepAnswer: ((StepStateRow, StepInputRow) -> StepResultRow)?
```

Find:

```swift
        case .rest, .tie:
            s.tick += 15_360
            return StepResultRow(state: s, snapshot: nil, moved: true)
        default:
            return StepResultRow(state: s, snapshot: nil, moved: false)
        }
```

Replace with:

```swift
        case .rest, .tie:
            s.tick += 15_360
            return StepResultRow(state: s, snapshot: nil, moved: true)
        case .value(let v):
            s.value = v
            return StepResultRow(state: s, snapshot: nil, moved: false)
        default:
            return StepResultRow(state: s, snapshot: nil, moved: false)
        }
```

- [ ] **Step 4: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-ffi --test control 2>&1 | grep -E "^test result"
scripts/build-ffi.sh && scripts/swift.sh test --filter 'StepEntryTests|ControlUITests' 2>&1 | tail -1
scripts/make-app.sh && VIZDAW_E2E_APP=dist/VizDAW.app cargo test -p vizctl --test e2e -- --ignored --test-threads 1 keys_drive 2>&1 | grep -E "test result|panicked"
```

Expected: `control` 8 passed; the Swift suites pass; the new e2e passes in a few seconds.

- [ ] **Step 5: Gate and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace 2>&1 | grep -E "FAILED|panicked" ; cargo test --workspace -- --list 2>/dev/null | grep -c ': test$'
scripts/build-ffi.sh && scripts/swift.sh build 2>&1 | tail -1 && scripts/swift.sh test 2>&1 | tail -1
touch /tmp/vizdaw-4f-e2e-marker
scripts/make-app.sh && VIZDAW_E2E_APP=dist/VizDAW.app cargo test -p vizctl --test e2e -- --ignored --test-threads 1 2>&1 | grep -E "test result|panicked"
pgrep -lf MacOS/VizDAW ; find /private/tmp -maxdepth 1 -name 'vze*' -newer /tmp/vizdaw-4f-e2e-marker -exec rm -rf {} +
git add app/Sources/VizDAWAppModel/ControlCommands.swift \
  app/Sources/VizDAWAppModel/StepEntry.swift \
  app/Tests/VizDAWAppModelTests/FakeSession.swift \
  app/Tests/VizDAWAppModelTests/StepEntryTests.swift \
  app/Tests/VizDAWEditorsTests/ControlUITests.swift \
  crates/core-ffi/src/control.rs \
  crates/core-ffi/src/lib.rs \
  crates/core-ffi/tests/control.rs \
  crates/vizctl/tests/e2e.rs
git commit -m "feat(control): step entry and the transport's marks in state.get; the keys end to end

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
git log -1 --format='%h %s'
```

Expected: 907 Rust tests listed (905 + 2: 901 pass, 6 ignored); Swift 649 (648 + 1); e2e 4 passed. No `FAILED` or `panicked` line (rerun once if the known-flaky MIDI listener test trips, and say so); `pgrep` prints nothing.

---

### Task 12: Manual checklist for keys and step entry; the whole-branch gate

**Files:**
- Modify: `app/README.md`

**Interfaces:**
- Consumes: everything above.
- Produces: `app/README.md` — the keys paragraph in "Editors" and the channel paragraph follow the bindings; checklist section "Keys and step entry (plan 4f)" with steps 99–108.

Documentation only, then the whole branch's gate. The steps are what an agent with `vizctl` or a person at the keyboard checks; steps 101 and 104 need a person (typing into fields by hand, a MIDI keyboard).

- [ ] **Step 1: Write the checklist**

In `app/README.md`:

Find:

```markdown
  Drag moves, Option-drag copies, dragging onto another lane merges; G
  toggles snap; Escape cancels a drag; a drag in the ruler selects a time
  range, a click there locates.
- Space and Return are Transport menu shortcuts, so every text entry the
  editors add (lane name, sequence name, comment, keyboard slot, reference
  parameters) switches them off while it is open.
- The editor layout (detail pane, editor windows and their frames, the
  sidebar order, block rule, light mode, snap, zoom) is saved in the
  project's `ui.json` when you save; it is not an undo step.
```

Replace with:

```markdown
  Drag moves, Option-drag copies, dragging onto another lane merges; G
  toggles snap; Escape cancels a drag; a drag in the ruler selects a time
  range, a click there locates.
- Keys follow the key bindings (plan 4f): every command, menu item or
  not, has a stable ID and can be bound in VizDAW > Settings > Keys;
  menus show the bound keys. Single unmodified keys drive the transport
  and the modes (Space, S, R, Y, K, Return, comma and period, L, O, I, T,
  G); P, E and U focus the Position, Edit In and Punch In fields; 0-9 go
  to locators, Option-digit sets one and Option-Shift-digit clears it.
  A text field always keeps the keys it types: a letter typed into any
  field is never a shortcut.
- The editor layout (detail pane, editor windows and their frames, the
  sidebar order, block rule, light mode, snap, zoom) is saved in the
  project's `ui.json` when you save; it is not an undo step.
```

Find:

```markdown
not move. A `ui.click` on an element is refused (`no_element`) when the
element's frame is non-finite or empty or its centre lies outside the
window; only mouse events posted by the channel are accepted as first clicks.
A key that is a menu shortcut runs that menu command on the window's
document.

Agents implementing or reviewing VizDAW may run checklist steps this way
against a throwaway home, read state and attach screenshots; computer-use
```

Replace with:

```markdown
not move. A `ui.click` on an element is refused (`no_element`) when the
element's frame is non-finite or empty or its centre lies outside the
window; only mouse events posted by the channel are accepted as first clicks.
A key runs what the current key bindings give it (plan 4f): its command
on the window's document (a menu command's too, with `dialog_required`
for one that would open a dialog), a sequence's keyboard slot, or, typed
into a text field or unbound, the window's own handling.

Agents implementing or reviewing VizDAW may run checklist steps this way
against a throwaway home, read state and attach screenshots; computer-use
```

Find:

```markdown
    `$V run drum.timeline '{"on": true}'` switches the view; in a fresh
    launch, `$V run drum.split_row` before any row was clicked answers
    `unavailable`.
```

Replace with:

```markdown
    `$V run drum.timeline '{"on": true}'` switches the view; in a fresh
    launch, `$V run drum.split_row` before any row was clicked answers
    `unavailable`.

### Keys and step entry (plan 4f)

Use the demo project in a throwaway home (VizDAW > Settings changes
preferences). Keys are the defaults unless a step changes them.

99. Menus and defaults: the Transport menu lists Play / Stop (Space),
    Stop (S), Play Selection (Option-Space), Play from Selection
    (Shift-Space), Play from Edit-In (Y), Record (R) and Scrub (K), both
    disabled until sub-project 5, Return to Start (Return), Previous and
    Next Marker (comma, period), Loop (L), Overdub (O), Punch (I), the Go
    to ... Field items (P, E, U) and Arm Previous/Next Lane
    (Option-Shift-Up/Down). The Step menu lists Step Entry (T) and the
    note values 1/1 to 1/32 (1-6), Dotted (.), Triplet (/), Tie (=), Rest
    (0) and Delete Last Step (Delete); Editors > Snap to Grid is G.
100. Transport keys: S while Song plays stops and goes back to where it
    started; select a time range in the ruler and press Option-Space: only
    the range plays, loop on or off; Shift-Space plays on from its start;
    with markers in Song (list editor: insert two Marker rows), comma and
    period jump between them. O and I tick Overdub and Punch (they record
    nothing yet); R and K do nothing (`$V run transport.record` answers
    `unavailable` with the reason).
101. Fields: P selects the Position field: type `5` and Return: the
    playhead jumps to bar 5 and the keys work again; `9:3:480` (colons,
    commas or spaces separate like dots) lands on bar 9 beat 3 and a half.
    E focuses Edit In: typing `3` selects bar 3 in the ruler; Tab to Edit
    Out and type `7` and Return: the range runs to bar 7; an Edit Out
    before Edit In is refused in the status line. U focuses Punch In; Punch Out
    before Punch In is refused. Escape in a field puts its text back.
    Typing letters into any text field (a field, the sidebar rename, the
    list editor's cells) types them and never runs a key.
102. Locators: at bar 5 press Option-2; Return goes to bar 1; 2 jumps to
    bar 5; Option-Shift-2 clears it, and 2 then does nothing (`$V run
    locator.go_2` answers `unavailable`: "Locator 2 is not set."). Locators
    are saved with the project.
103. Record arm: Option-Shift-Down arms Song's first lane (the header's
    arm button lights), again moves the arm to the next lane (one lane
    armed), Option-Shift-Up moves it back; past the last lane the status
    line says there is none. Each move is one undo step "Move Record Arm".
104. Step entry: select Chords' block, stop, put the playhead on bar 2
    and press T: the transport bar above the arrangement shows "Step Entry
    · Chords · 1/4". Press 4 (1/8) and play a chord on a MIDI keyboard:
    when the last key is released, the chord is entered at bar 2 as one
    undo step "Step Entry", selected, and the playhead moves on an
    eighth. Press = (tie: the chord grows by an eighth), 0 (a rest: the
    playhead moves on, nothing is entered), . (dotted: the bar shows "1/8
    dotted"), then Delete twice (the rest, then the tie taken back; the
    playhead goes back with them) and once more (the chord goes).
    Clicking in the ruler moves where the next step goes. Number keys
    choose values only while step entry is on; T turns it off and 1-9
    are locators again. With no MIDI keyboard, `vizctl run step.notes
    '{"pitches": "C4 E4 G4"}'` enters a chord.
105. Reserved number row: Settings > Keys > "Number keys always go to
    locators": during step entry 1-6 go to locators and the keypad's 1-6
    choose values.
106. Keyboard slots: give Groove the slot Option-G (sidebar > Keyboard
    Slot...). Option-G selects Groove and plays it from its start; during
    step entry in Song it enters a reference to Groove at the playhead and
    moves on by Groove's length ("Step Entry"). A slot Cmd-S is refused in
    the sheet: "⌘S is the key of “Save”."
107. Keys settings: Settings > Keys lists every command by menu with its
    keys (step entry's marked). Record on Stop, press X: X stops; record
    Space for Stop: "Space is the key of “Play / Stop”. Replace it?" -
    Replace moves it, Cancel keeps both as they were. Reset puts a
    command's keys back; Save... writes a `.vizkeys` file; Reset All, then
    Load... brings the saved keys back; a damaged file is refused with its
    reason and changes nothing. Menus show the new keys at once.
108. Channel: `$V key t`, `$V key 4`, `$V run step.notes '{"pitches":
    "60 64"}'`, `$V state editor` shows `"step": {"value": "eighth", ...}`
    and the transport's locators; `$V run keys.load '{"path":
    "/tmp/x.vizkeys"}'` loads a bindings file and `$V key` follows it;
    `$V state editor` reads the snap division snake_case
    (`thirty_second`).
```

- [ ] **Step 2: The whole-branch gate**

Run every gate once more on the finished branch, with the engine's real-time check, the app and the e2e; check that no VizDAW is left running and that the e2e's homes are gone.

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p vizdaw-play --all-targets --features rt-check -- -D warnings
cargo test --workspace 2>&1 | grep -E "FAILED|panicked|test result: FAILED" ; cargo test --workspace -- --list 2>/dev/null | grep -c ': test$'
scripts/build-ffi.sh && scripts/swift.sh build 2>&1 | tail -1 && scripts/swift.sh test 2>&1 | tail -1
touch /tmp/vizdaw-4f-e2e-marker
scripts/make-app.sh && VIZDAW_E2E_APP=dist/VizDAW.app cargo test -p vizctl --test e2e -- --ignored --test-threads 1 2>&1 | grep -E "test result|panicked"
pgrep -lf MacOS/VizDAW ; find /private/tmp -maxdepth 1 -name 'vze*' -newer /tmp/vizdaw-4f-e2e-marker -exec rm -rf {} +
```

Expected: 907 Rust tests listed (901 pass, 6 ignored); Swift 649; e2e 4 passed — unchanged. No `FAILED` or `panicked` line (rerun once if the known-flaky MIDI listener test trips, and say so); `pgrep` prints nothing.

- [ ] **Step 3: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add app/README.md
git commit -m "docs: keys and step entry checklist steps 99-108

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
git log -1 --format='%h %s'
```

---

## Self-Review

**Spec coverage (editors spec §4f, parent §9 "Keyboard", §5 keyboard slots):**

| Requirement | Where |
|---|---|
| Every command (menu or not) has a stable identifier | Task 2 (registry fields), Tasks 7, 8, 10 (64 new IDs: 108 in all) |
| Bindings can be saved, loaded, reset | Task 2 (`Keymap`, document), Task 5 (files), Task 6 (preferences), Task 7 (`keys.*`), Task 10 (Settings > Keys) |
| Bound keys show in menus | Task 9 (`KeyMenu`, every `AppCommands` shortcut) |
| Unmodified single keys for transport (play/pause, stop, record, play from position, scrub, previous/next marker) | Task 7 (Space, S, R, Y, K, `,` `.`), Task 9 (routing) |
| … and mode toggles (overdub, punch, loop, step, snap) | Task 7 (O, I, L, G), Task 8 (T) |
| Focus keys for position, edit-in and punch fields; bar·beat·unit; Enter commits | Task 10 (P, E, U, `TransportFieldsView`) |
| Number keys jump to locators; modifiers set/clear; a preference reserves the number row | Task 7 (`locator.*`), Task 2 and 6 (reserve), Task 10 (the toggle) |
| Modified Space plays from/only the selection | Task 3 (engine, `play_from`, `selection_span`), Task 7 (⇧Space, ⌥Space) |
| Modified arrows nudge or move record-arm | existing ⌥ arrows (nudge); Task 3 and 7 (⌥⇧↑/↓) |
| One key toggles snap | Task 7 (`editors.snap`), Task 9 (G no longer canvas-only) |
| Step entry: note values from number keys; dot, triplet, tie, rest, delete-last-step | Task 4 (state machine), Task 5 (session), Task 8 (keys) |
| Pitch from MIDI input | Task 5 (note-offs), Task 8 (to step entry first) |
| Recording commands registered so keys can be bound now (editors spec §1) | Task 7 (`transport.record`, `record.*`, `transport.scrub`) |
| Keyboard slot refused if it clashes with a binding; pressing it makes the sequence current and auditions it; in step entry the position advances by its length | Task 8 (`commitSlot`, `pressSlot`, reference steps) |
| §6 Swift tests: key binding persistence, step entry sequences | Tasks 6, 8, 10 |
| Control spec §6: the registry drives key bindings and menu titles | Tasks 2, 9 (titles from `BindingRow.title` for the new items) |
| README checklist from step 99 | Task 12 (99–108) |

**Placeholder scan:** no "TBD", no "similar to Task N", no step without its code; every changed file is given as exact Find/Replace excerpts checked against the tree the previous task leaves, every new file whole.

**Type consistency:** `KeyOutcome` is `DrumOutcome` everywhere; `StepInputRow`/`StepStateRow`/`StepResultRow` match between `core-ffi`, `FakeSession`, `StepEntryModel`; `Session.step_entry` is `stepEntry(seq:lane:state:input:)` in Swift; `ControlHost` gains `keys` (Task 7) and `focus(_:_:)` (Task 10) with `FakeControlHost` and `AppControlHost` in the same tasks; registry counts 44 → 92 → 105 → 108 match the tests.

**Review Focus:** each of the five lines names its tests, all in the tasks that own the code.

**Known limits (accepted):** the app target's key monitor and menus are not unit-tested (the router, dispatch and commands they call are; the e2e drives the channel's same route); the transport bar's AppKit fields sit in a SwiftUI pane, and whether `ui.screenshot` renders them was not checked (SwiftUI bars do not render, 4e final review T7 M1); `state.get` and `ui.tree` read them.
