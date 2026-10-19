# VizDAW Sub-project 4: Editors — Design

Status: approved in conversation 2026-10-16 (sections 1–2); written spec pending review.
Parent spec: `2026-10-15-vizdaw-design.md` §9 (Native macOS UI) and §11 step 4. That section's editor requirements are binding here and are not repeated in full; this document says how they are built and in what order.

## 1. Scope

In scope: the arrangement timeline, piano roll, list editor, Strip Chart (including automation lanes and their playback), drum grid, the select-and-modify panel with templates and Modify Again, exact mode, nudge, the sequence/subsequence workflow in the UI, key bindings and step entry.

Out of scope:
- Audio recording, disk streaming and region fades editing beyond what the core already models (sub-project 5). Audio regions are drawn and can be moved, trimmed and split with existing commands.
- Import/export (sub-project 6).
- The full visual design pass (sub-project 7). Editors here are functional and clean; colours, icons, animation and typography are provisional.
- MIDI recording (loop record, keep/discard pass, record-arm moves) beyond binding their command identifiers (sub-project 5 owns recording; 4f registers the commands so keys can be bound now).
- EQ automation: the mixer has no EQ yet. Automation targets are open to extension; EQ joins when the mixer gains one.

## 2. Decisions (user, 2026-10-16)

- One spec, executed as six sequential plans (4a–4f), like sub-project 3.
- Drawing: custom AppKit `NSView`s backed by Core Animation layers, drawn with Core Graphics. Metal only later, for a view measured too slow.
- Windows: the document window holds the arrangement on top and a split detail editor below; any editor can be torn off into its own window showing a chosen block, lane or sequence. Several editors may be open at once.

## 3. Architecture

```
Swift  VizDAWEditors   EditorCanvas · Arrangement · PianoRoll · ListEditor · StripChart · DrumGrid · SelectModifyPanel · KeyBindings
Swift  VizDAWAppModel  EditorModel (per document: lane cache by revision, selection, edit dispatch) · TransportClock
FFI    core-ffi        lane_events · edit(EditCommand) · selection · view filters · lane_revs · transport host-time stamp
Rust   core-model      existing Command set (+ Automation event kind, 4d) · History · Selection · queries
Rust   core-engine     automation playback (4d)
```

### 3.1 FFI read model

- `DocSnapshot` gains `lane_revs: Vec<LaneRev { seq, lane, rev: u64 }>`. A lane's revision changes whenever anything in that lane (its events, active take, flags, drum layout) changes. Revisions are computed by `core-ffi` by comparing persistent-collection pointers between the previous and the new document (`im` structural sharing makes unchanged lanes pointer-equal), so `core-model` gains no bookkeeping.
- `Session.lane_events(seq, lane) -> LaneEvents { rev, events: Vec<EventRow> }` returns the active take's events in time order. `EventRow` is a flat record: `id, tick, kind, mute, dest (optional), pitch, velocity, release_velocity, duration, value (i64), value2 (i64), ref_seq (optional), ref_length, ref_loops, ref_transpose, media (optional), source_offset, gain_db, text (optional), per_note (Vec of expression points)`; fields unused by a kind are zero/None.
- `Session.sequence_info(seq)` returns tempo map, meter map, markers, start offset and length for rulers.
- `Session.blocks(seq, lane, rule: BlockRule) -> Vec<Block>` computes arrangement blocks in Rust (`BlockRule`: SilenceGap(ticks) · FixedBars(n) · WholeLane). A reference event is always its own block.
- Sizes: a lane with 100 000 events must return in under 50 ms in a release build (test-enforced with a generous bound in CI).

### 3.2 FFI edit model

- `Session.edit(cmd: EditCommand) -> DocSnapshot`. `EditCommand` is a UniFFI enum mirroring the core `Command` groups editors need: Event, Reference, Drum, Lane, Sequence, Audio, SelectAndModify, ModifyAgain, Automation (4d). One variant per core command; field types are FFI records. Errors map to `VizError` as today.
- Gestures use the existing `begin_edit(label)` / `end_edit()`; one drag is one undo step. A drag sends intermediate edits so the document (and sound) follow the pointer.
- Notes-sound-while-dragged uses a new `Session.audition(dest, pitch, velocity, on: bool)` that sends a note on/off straight to the destination without touching the document.

### 3.3 Selection and filters

- `Session.selection() -> SelectionRow { time_range (optional), lanes, events }` and `set_selection(SelectionRow)`. The selection is the document-wide one in `History`; it is not an undo step. Every editor draws and edits the same selection.
- View filters are per editor and live in Swift; they are passed with commands that act on "the selection" (`EditCommand` variants carry an optional `ViewFilterRow`), matching the core `ViewFilter`.

### 3.4 Playhead

- `TransportState` gains `host_time_ns: u64` (when `tick` was sampled) and `ticks_per_second: f64` (at that tick).
- `TransportClock` (Swift) extrapolates the tick from the last poll on each display-link frame (`NSView.displayLink`, macOS 14), clamped so it never runs more than one poll interval past the last sample and snaps back on a locate or stop. Rust is called no more often than today.

### 3.5 EditorCanvas

A shared `NSView` subclass with `wantsLayer`, providing:
- `TimeAxis`: tick ↔ x with zoom (ticks per point) and scroll origin; a matching `PitchAxis`/`ValueAxis` for vertical mapping. Pure value types, unit-tested.
- Layers: grid (bar/beat lines, cached per zoom level), content (editor-supplied), selection overlay, playhead (moved, not redrawn). Content draws only the dirty rect; layers use `contentsScale` for Retina.
- Ruler view: a drag in any ruler selects a time range across all shown lanes and kinds whatever the tool (parent spec §9).
- Snap: the shared edit grid (`Snap { division, triplet, dotted, enabled }`) toggled by one key.
- Tool state machine: `Tool` (arrow, pencil, eraser, plus editor-specific tools) × gesture phases (down, drag, up, cancel on Escape). Gestures open a transaction on the first real movement (a 3-point dead zone) and close it on up; Escape aborts.
- Auto-scroll when dragging past an edge; follow-playback scrolling (page mode).
- Accessibility: the canvas is an accessibility container; editors expose events as elements with role, label (e.g. "Note C4, bar 3 beat 1, velocity 100") and actions for the selected element.
- Hit-testing and geometry are pure functions of (model rows, axes) so they can be tested without a window.

### 3.6 EditorModel

- One per open document, `@Observable`, owned by `DocumentModel`.
- Caches `LaneEvents` by (seq, lane) and refetches only lanes whose revision changed in the latest snapshot.
- Dispatches `EditCommand`s and gesture transactions; applies the returned snapshot.
- Holds Swift-side editor state that is not document state: zoom, scroll, tool, snap, per-editor view filters, open editors and what they show.
- Tested against `FakeSession` like the existing models.

### 3.7 Windows

- The document window: sidebar | arrangement over a split detail pane. Double-clicking a block opens exactly that block in the detail pane in the last-used detail editor (default piano roll); a menu chooses another.
- "Open in Window" (⌥⌘ with the editor shortcut) tears the current detail editor off into an `NSWindow` showing the same block/lane/sequence. Windows are restored with the project (stored in the existing window-state file).
- All editors follow playback and share selection.

### 3.8 Busy contract

Editors respect the sub-project 3 re-entrancy rules: `edit` may return `VizError::Busy` while a plugin editor spins the run loop. A gesture's intermediate edits that hit Busy are dropped (the next pointer move resends the full target); the final edit on mouse-up is retried each tick until it succeeds or the gesture is cancelled. Polling paths skip Busy silently.

## 4. Phases

### 4a — Foundation and arrangement
- FFI: lane revisions, `lane_events`, `sequence_info`, `blocks`, `edit` with Event/Reference/Lane/Sequence/Audio variants, selection, audition, transport host time.
- Swift: `VizDAWEditors` target, `TimeAxis`, `EditorCanvas`, rulers, snap, `TransportClock`, `EditorModel`.
- Arrangement: lanes as rows with header (name, mute/solo/arm, destination); blocks per `BlockRule` with a user-set size; reference events as their own blocks labelled with the sequence name; drag moves (⌥ copies) or merges blocks into another lane; light drawing mode (block outlines only) switches on automatically above a block count threshold and manually.
- Sequence sidebar: name, keyboard slot, comment, use count; drag reorder, duplicate, new from template.
- Subsequence workflow: make a reference from the selection, open the referenced sequence (double-click a reference block), make a private copy, flatten, re-point, set params.
- Window layout: split detail pane (placeholder until 4b) and tear-off windows.

### 4b — Piano roll
- Note drawing over a pitch axis; several lanes at once with per-lane colour; audio regions in their own strip between notes and the controller pane, with mute/solo per audio output path; optional keyboard beside it (click auditions).
- Cursor zones by position over a note: start moves, middle transposes, end resizes; ⌥ copies. Pencil inserts at the default length or paints a length by dragging; eraser deletes. Notes sound while dragged (audition).
- Per-note expression lanes for the MIDI 2.0 per-note data.
- Nudge: time and pitch steps (pitch can follow a key and scale), steps set on one Settings page.

### 4c — List editor, select-and-modify, exact mode
- List editor per parent spec §9, all of it: bar-number typing moves to that downbeat, sub-field editing, Enter commits and follows, pitch/velocity by playing a note (MIDI input), insert repeats the last kind, optional columns, type/destination filters (apply to all or save as default); reference events and regions are rows; opening a reference opens its sequence.
- Select-and-modify panel: select half and modify half exactly as in parent spec §9, driving the existing `SelectAndModify` command; named templates; Modify Again; the transpose/quantize/modify/move/select-by-rule menu commands open it set to their task.
- Exact mode: a mode toggle; any drag then opens the panel pre-filled with the dragged amounts instead of applying.

### 4d — Strip Chart and automation
- Core: `EventBody::Automation { target: AutomationTarget, value: f64 }` (normalised 0–1) where `AutomationTarget` is `StripFader(StripId) · StripPan(StripId) · StripMute(StripId) · StripSolo(StripId) · SendLevel(StripId, SendIndex) · PluginParam(PluginInstanceId, ParamId)`. Automation events live in ordinary lanes; a lane showing automation is a normal lane whose Strip Chart view selects a target. Values between points ramp linearly; mute/solo/stepped params jump.
- Engine: the render path reads automation events for the playing range and applies them per block (sub-block accuracy: split at automation points, minimum 32 frames) to mixer controls and as plugin parameter changes. Automation overrides the static control value while playing; stopping leaves the control at its last automated value until the user moves it.
- Strip Chart view: lanes for velocity, release velocity, duration, CC (32-bit), pitch bend, pressure, program, tempo, velocity scale and automation targets; draw and modify modes; line shapes straight, free, curved, flat, random and oscillator (sine/square/triangle/saw, period in musical or clock units, optionally following the meter); Compress rectangle; modify operations set, scale, add, limit max/min, legato, thin/thicken; a modifier limits to selected notes; tempo ramps sound linear. Every operation is also reachable numerically in the select-and-modify panel.

### 4e — Drum grid
- Shows drum lanes only (lanes with a `DrumLayout`); rows with note, name and destination.
- Step matrix and timeline views; paint tool toggles on click and paints/erases a run on drag with configurable spacing, duration and velocity; a shown velocity lane sets new hits' velocity from the vertical pointer position.
- Rows: add, reorder, drag to another drum lane or below the last to create one, split out into its own lane (existing `Drum` commands).
- Per drum lane play quantize (grid or groove) and nudge; the engine rule that a drum lane plays a note only if it starts after playback/unmute/solo began.

### 4f — Keys and step entry
- Command registry: every command (menu or not) has a stable identifier; bindings can be saved, loaded, reset; bound keys show in menus.
- Defaults per parent spec §9: unmodified single keys for transport and mode toggles; focus keys for position, edit-in and punch fields with bar·beat·unit entry; number keys jump to locators (modifiers set/clear; a preference reserves the number row); modified Space plays from/only the selection; modified arrows nudge or move record-arm; snap toggle.
- Step entry: note values from number keys, keys for dot, triplet, tie, rest, delete-last-step; pitch from MIDI input.

## 5. Errors and faults

- Every FFI call returns `Result`; editors show a non-modal status message for user-action failures and stay silent for polling Busy.
- A drag whose target becomes invalid mid-gesture (lane deleted by undo in another window) aborts the transaction.
- Editors never hold Rust objects across the run loop; everything they draw is a Swift copy of rows.

## 6. Testing

- Rust: FFI tests for event rows (every `EventBody` kind round-trips), lane revisions (only changed lanes bump), blocks (each rule, reference isolation), edit mapping (each `EditCommand` variant applies and undoes), selection round trip; automation playback golden renders (fader ramp, plugin param ramp with a test plugin) in 4d; the existing proptest undo property extended to `Automation` events.
- Swift: `TimeAxis`/`PitchAxis` maths, hit-testing and cursor zones, block layout, tool state machine transitions, `EditorModel` cache invalidation with `FakeSession`, key binding persistence, step entry sequences.
- Manual checklist in `app/README.md` for each phase (drag feel, playhead smoothness, audition, tear-off windows, accessibility with VoiceOver).

## 7. Execution

Plans 4a–4f are written and executed one after another by subagent-driven development under the standing "keep going" authorization, with rulings logged in each plan's ledger. Push and merge still need the user's say-so.
