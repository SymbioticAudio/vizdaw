# VizDAW Sub-project 2a — Core Model Design

- **Date:** 2026-10-15
- **Status:** Draft, awaiting review
- **Parent spec:** `2026-10-15-vizdaw-design.md` (Sections 4, 5, 8, 12)

## 1. Goal

A pure-Rust crate, `core-model`, that holds a VizDAW project and everything you can do to it. That covers sequences with live references, mixed MIDI/audio lanes, MIDI 2.0 events, hybrid routing, drum lanes, takes, a single shared selection, select-and-modify, multi-level undo, and a versioned project file.

The crate has no audio, MIDI I/O, UI or OS dependencies. The engine (2b), the FFI and Swift app (2c), and later importers all build on it.

**Success:** every model behaviour in the parent spec §5 is expressible as a command. Property tests show that undo, save/load, reference integrity and time conversion are correct. Benchmarks meet the targets in §8.

## 2. Constraints

- Rust stable, edition 2024; the crate compiles on macOS and Linux (no platform-specific code). OS-specific code elsewhere in VizDAW stays behind narrow interfaces so a later Linux port can replace it.
- Runtime dependencies are limited to: `serde`, `serde_json`, `im` (persistent collections), `thiserror`. Dev-only: `proptest`, `criterion`, `insta` (snapshot tests).
- No panics on any input: `clippy::unwrap_used`, `expect_used`, `indexing_slicing` and `panic` are denied in the crate.
- Everything public is documented; `#![deny(missing_docs)]`.

## 3. Core types

All types are immutable values. An edit produces a new `Document` that shares unchanged parts with the old one.

### Identity and time
- Typed ID newtypes: `SeqId`, `LaneId`, `EventId`, `DestId`, `OutputPathId`, `MediaId`, `TakeId`, `DrumRowId`. IDs are allocated from per-document counters and never reused within a document.
- `Tick(i64)`: musical time at **15,360 ticks per quarter note**.
- `BarBeatUnit { bar: i32, beat: u16, unit: u32 }`: display and entry position. Bar 0 exists for pickups; `unit` is ticks within the beat at a display resolution chosen by the caller.

### Document
- `Document { settings, instruments: OrdMap<DestId, Destination>, outputs: OrdMap<OutputPathId, OutputPath>, media: OrdMap<MediaId, MediaItem>, sequences: OrdMap<SeqId, Sequence>, counters }`
- `Destination`: name, kind (`MidiEndpoint { endpoint_ref, group, channel }` or `PluginInstrument { plugin_ref }`), default patch. These are the instrument-list entries; changing one re-routes every lane and event that uses it.
- `OutputPath`: name and an opaque reference to an engine bus or hardware output. Its meaning is defined by 2b; the model only stores it.
- `MediaItem`: relative path inside the package, sample rate, channel count, length in frames, and an optional content hash.

### Sequence
- `Sequence { name, kind, keyboard_slot: Option<KeySlot>, tempo: TempoMap, meter: MeterMap, markers, comment, start_offset: Tick, length: SeqLength, lanes: Vector<Lane> }`
- `kind`: `Normal`, or `Generated { generator: GeneratorParams }`. In 2a, generator parameters are stored and round-tripped; producing their notes is out of scope.
- `TempoMap`: ordered tempo points (BPM as fixed-point, plus a flag for a linear ramp to the next point). `MeterMap`: meter changes, which must fall on barlines. Key signatures are kept with the meter changes.
- `SeqLength`: derived from the longest lane, or locked to a fixed length.

### Lane
- `Lane { id, name, midi_dest: Option<DestId>, audio_out: Option<OutputPathId>, mute, solo, arm, length: Option<Tick>, looped: bool, length_locked: bool, play_quantize: Option<QuantizeSpec>, play_shift: Tick, drum: Option<DrumLayout>, takes: Vector<Take>, active_take: TakeId }`
- A lane holds MIDI events, audio regions or both, with no type flag.
- `Take { id, name, events: OrdMap<(Tick, EventId), Event> }`. Only the active take plays. A new lane has one take.
- `DrumLayout { rows: Vector<DrumRow> }`, where `DrumRow { id, name, note: u8, dest: Option<DestId>, show_velocity: bool }`. Rows are a view over the lane's note events, grouped by note and destination. A note without a matching row creates one automatically when inserted from another editor.

### Event
`Event { id, tick, dest: Option<DestId>, group_channel: Option<(u8, u8)>, mute: bool, body: EventBody }`

`EventBody` is one of:
- `Note { pitch: u8, velocity: u16, release_velocity: u16, duration: Tick, attribute: Option<NoteAttribute>, per_note: Vector<PerNoteCtl> }`
- `Cc { index: u8, value: u32 }`
- `PitchBend { value: u32 }`
- `ChannelPressure { value: u32 }`
- `PolyPressure { pitch: u8, value: u32 }`
- `Program { program: u8, bank: Option<(u8, u8)> }`
- `SysEx { data: Arc<[u8]>, eight_bit: bool }`
- `Text { kind: TextKind, text: String }`, where `TextKind` is Text, Lyric, Cue, Marker or Copyright
- `Reference { seq: SeqId, length: Option<Tick>, loop_count: LoopCount, transpose: i8 }`. The event's `mute` doubles as the per-use mute; a player slot is reserved for later.
- `AudioRegion { media: MediaId, source_offset: u64, length: u64, gain_db: f32, fade_in: Fade, fade_out: Fade }`. Lengths are in sample frames, and fades are re-editable parameters (`Fade { length: Tick, shape: FadeShape }`).

**Value ranges:** velocities are 16-bit; controllers, pitch bend and pressure are 32-bit; the group and channel override must each be 0–15. MIDI 1.0 values are scaled on the way in and out by the engine and importers, not by the model.

## 4. Rules the model enforces

1. **No cycles:** a `Reference` may not make a sequence reach itself through any chain. This is checked on every command that inserts or re-points a reference.
2. **Referential integrity:**
   - Removing a sequence that is referenced needs `delete_references: true`, and the command reports which events it removed.
   - Removing a destination, output or media item that is in use is refused unless a replacement is given.
3. **Derived, never stored:**
   - use counts and the list of users of each sequence;
   - sequence length when not locked;
   - drum rows' membership;
   - the resolved group and channel.
4. **Routing resolution:** for each MIDI event, the destination is the event's own `dest`, else the lane's `midi_dest`. The group and channel are the event's override, else the destination's (only `MidiEndpoint` destinations have them; plugin destinations default to group 0, channel 0). Events with no resolvable destination are reported by a query, not silently dropped.
5. **Nested timing:** when a sequence plays inside another, the playing (top-level) sequence's tempo and meter govern. A referenced sequence's own maps are used only in its local-time view.
6. **Meter changes fall on barlines;** tempo values are within 5–999 BPM.

## 5. Commands, selection and history

### Commands
- `Command` is an enum. Each variant has a stable string name, used for key bindings and undo labels.
- The core is pure: `fn apply(doc: &Document, cmd: &Command) -> Result<Applied, ModelError>`, where `Applied { doc: Document, report: Report }`. The report lists created and removed IDs and any warnings.

**Command set for 2a:**
- **Sequences:** new, duplicate, rename, delete (with the confirmation flag), set kind, set keyboard slot, set tempo/meter/markers, set start offset, set length/lock, set comment.
- **Lanes:** add, remove, reorder, set destination/output, mute/solo/arm, set length/loop/lock, set play quantize/shift, add take, set active take, mark as drum lane, add/rename/reorder/remove drum rows, move a drum row to another lane, split a drum row to its own lane.
- **Events:**
  - insert, delete, move (time and lane), change pitch/velocity/duration/value;
  - set the dest or group/channel override, mute;
  - nudge in time or pitch.
- **References:**
  - insert a reference to a sequence;
  - make a reference from a selection, as one multi-lane reference or one per lane, with options for bar rounding and loop count;
  - re-point;
  - make a private copy (clone the sequence and re-point this use);
  - flatten (inline the events in place, applying length, loop and transpose);
  - set per-use length, loop and transpose.
- **Audio regions:** insert, trim, split, set gain and fades.
- **Destinations and outputs:** add, rename, edit, remove (with a replacement).
- **Select-and-modify:** see below.

### Transactions
`History::begin(label)` … `commit()` groups commands into one undo step, used for a drag gesture or a recording pass. `abort()` restores the state from before the transaction.

### Selection
- `Selection { time_range: Option<(Tick, Tick)>, lanes: OrdSet<(SeqId, LaneId)>, events: OrdSet<EventId> }`.
- There is one selection per open document. It is kept beside the history, and changing it is not an undo step.
- View filters are passed to queries and to select-and-modify, so hidden events can't be selected or changed.

### Select-and-modify
- `Rule` is a composable predicate over events: time range, event kind, pitch range, velocity range, duration range, destination, metric position, every Nth, and combinations of these.
- `SelectMode` is select, add or refine.
- `Modify` covers transpose (by interval or to a scale), quantize (grid or groove), set/scale/add velocity, set/scale duration, legato, reassign destination, scale time, move, and change controller values.
- `Command::SelectAndModify { rule: Option<Rule>, mode, modify: Option<Modify>, to_copy: bool }`.
- Named templates are stored in document settings.
- `History` remembers the last `Modify`, and "Modify Again" replays it on the current selection.

### History
- `History { past: Vec<Version>, present: Version, future: Vec<Version>, last_modify: Option<Modify> }`.
- `Version { doc: Document, label: String, at: Timestamp }`.
- Undo and redo move between versions. A new command clears `future`.
- The depth limit is a setting (default 200 steps). The oldest versions are dropped first.
- History is not saved with the project.

## 6. Queries

Read-only functions over a `Document`, for 2b and 2c:
- **`flatten(seq, range, filters) -> impl Iterator<Item = ResolvedEvent>`:** walks lanes and references recursively. It applies per-use length, loop, transpose and mute, the active take, lane and event mute/solo, and the nested-timing rule. Each `ResolvedEvent` carries an absolute tick, its resolved destination and group/channel, and its path back to the source event.
- **Time conversion:** tick ↔ bar·beat·unit (meter map) and tick ↔ seconds (tempo map, including linear ramps).
- **Usage:**
  - `users_of(seq)` and `use_count(seq)`;
  - unused sequences;
  - unresolved-destination events;
  - media in use.
- **Selection:** `events_matching(rule, filters)`.
- **Drum view:** rows of a drum lane with their events.

## 7. Project file

- **Package layout:** a `.vizdaw` directory containing `project.json`, `Media/` and `Autosave/`.
- **`project.json`:**
  - top-level fields: `{ "format": "vizdaw-project", "version": 1, "document": { … } }`;
  - serialized with serde, with stable field names;
  - IDs are serialized as integers, and events within a take are an array ordered by `(tick, id)`.
- **Loading:**
  - Older versions are migrated step by step through `migrate_vN_to_vN+1` functions.
  - A newer version is refused with `ModelError::NewerFormat`.
  - Unknown fields are rejected, so data is never silently dropped.
- **Saving:**
  - Write `project.json.tmp`, `fsync`, then rename it over `project.json` and `fsync` the directory.
  - Autosave writes timestamped copies to `Autosave/` and keeps the newest N (default 10).
- **Validation:** loading runs the same integrity checks as commands (no cycles, IDs exist, ranges valid) and reports every problem found.

## 8. Errors, testing and performance

- **`ModelError` (one enum):**
  - `Cycle { path }`, `UnknownId(..)`, `InUse { users }`, `OutOfRange { field, value }`;
  - `NotOnBarline`, `NewerFormat { found }`, `Corrupt { problems }`, `Io(..)`.
- **Property tests (proptest), on generated valid documents and command sequences:**
  - applying N commands then undoing N returns an identical document;
  - redo after undo returns the same document;
  - save then load gives an identical document;
  - no command sequence can create a reference cycle;
  - derived use counts equal a brute-force recount;
  - tick ↔ bar·beat·unit ↔ seconds round-trips across tempo ramps and meter changes;
  - flattening matches a naive reference implementation.
- **Unit tests** cover each command's rules and edge cases:
  - bar-0 pickups, references longer or shorter than their sequence, transpose clamping at 0/127;
  - private copy versus flatten;
  - routing resolution with and without overrides;
  - drum-row auto-creation.
- **Golden files:** hand-written `project.json` fixtures (minimal, nested references, mixed lanes, drum lane) checked with `insta` snapshot tests.
- **Benchmarks (`criterion`):**
  - one edit on a 100k-event document: < 1 ms;
  - flattening a 64-bar song with three levels of references: < 5 ms;
  - loading a 100k-event project: < 200 ms.
- **CI:** `cargo test`, `cargo clippy -D warnings` and `cargo fmt --check` on macOS and Linux runners.

## 9. Out of scope (later sub-projects)

- Audio and MIDI I/O, the render graph and real-time playback (2b).
- The FFI and the Swift UI (2c).
- Plugin hosting and plugin state (3).
- Running generators to produce notes.
- Input effects (these are applied while recording, which is engine work).
- Importing and exporting SMF, MIDI Clip File, OMF and AAF (6).
- Persisting undo history.
