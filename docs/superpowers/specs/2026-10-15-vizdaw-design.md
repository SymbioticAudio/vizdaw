# VizDAW — Design Spec

- **Date:** 2026-10-15
- **Revised:** 2026-10-15 (research-informed revision)
- **Status:** Draft, awaiting review
- **License:** GPL-3.0-only

## 1. Intent

VizDAW is a new, open-source macOS DAW inspired by Opcode Studio Vision Pro 4.5.
It is not a port and not a clone. It keeps the ideas that made Studio Vision
fast to work in and rebuilds them on a modern stack:

- MIDI-first editing: list editor, graphic (piano-roll) editor, Strip Chart controller editing, keyboard-driven workflow.
- Sequences and subsequences as the arrangement model, with alternate takes.
- Integrated audio edited alongside MIDI on the same timeline lanes, with one shared selection.
- Modern plugin hosting (AU v2/v3, CLAP), MIDI 2.0 throughout, and a native modern Mac UI.

**Success for v1:** a musician can build a song from nested sequences with
MIDI and audio, play AU/CLAP instruments and effects plus external
MIDI gear with low latency on any CoreAudio interface, import SMF/OMF/AAF
material, and edit it in list, piano-roll, Strip Chart and arrangement views
in an app that feels native on macOS.

### Non-goals (v1)

- Faithful reproduction of Studio Vision's UI, code or file format.
- Importing original Studio Vision song files (may be revisited later).
- Native Logic Pro (`.logicx`) import. The format is undocumented and changes between versions. Users export OMF/AAF + SMF from Logic instead.
- Audio-to-MIDI conversion, notation, video, MIDI-CI Property Exchange/Profiles, external clock/MTC sync (including tape lock and machine control/MMC), multi-core rendering. All are deferred; the architecture must not preclude them.
- Live sequence triggering: simultaneous sequence players, queues, trigger/gated/transpose keyboard modes, and mapping incoming MIDI to keys. The sequence keyboard slot (Section 5) is limited to select, audition and inserting a reference.
- A nested sequence keeping its own tempo while it plays inside a parent (see the timing rule in Section 5).
- An automatic song-track layer that places sequences end to end and copies their meter and markers upward. Songs are built by placing reference events.
- Fader events and a separate fader bank. All continuous control is automation or controller data.
- Inter-application audio (ReWire-style). Audio enters only from hardware inputs and hosted plugins.
- Offline or destructive audio DSP (time/pitch scaling and similar render-to-file processing). Audio processing is non-destructive: plugins, region gain, and fades stored as region parameters. Normalize is a non-destructive gain adjustment on the region.
- Intel Macs, macOS < 14, Windows/Linux.

Limitations VizDAW deliberately does not carry over: single-level undo; fixed caps on references or nesting depth; one record-armed track at a time; replace-recording that clears material outside the recorded range; fades baked into audio files; routing and instrument changes outside undo.

## 2. Clean-room rules

Studio Vision is still the intellectual property of its rights holder.

- Reverse engineering (REA) is used only to understand **behaviour, workflow and concepts**. Findings are written as prose descriptions of behaviour, not as code.
- No decompiled code, translated algorithms, resources (icons, PICTs, patterns, fonts, sounds), or Opcode trademarks/branding go into the VizDAW repo.
- The original binary and the research dossier live in `~/workspace/SVP/research/`, **outside** the VizDAW repository.
- Before public release, search USPTO classes 9 and 42 for "VizDAW" (nearest known marks: Vizrt "Viz …" broadcast software, VIZ Media). The README must state: "Not affiliated with or endorsed by Opcode Systems or Gibson."

## 3. Platform

- Apple Silicon only, macOS 14 Sonoma or later.
- Rust (stable) for the core; Swift 5.10+/SwiftUI + AppKit for the app shell.

### Naming and versioning

- Product name **VizDAW**. Repository: `github.com/wblatta/vizdaw`.
- Year-based versions: the first release line is **VizDAW 26**, tagged `26.0.0`, `26.1.0`, … and next year's is `27.x.y`.
- The project package extension `.vizdaw` stays the same across versions; the document inside carries its own format version.

## 4. Architecture

```
┌──────────────────────── Swift app (SwiftUI + AppKit) ────────────────────────┐
│  Window/Toolbar/Sidebar/Inspector · Settings · Menus/Shortcuts               │
│  Custom editors (Metal/CoreAnimation): Arrange · Piano Roll · List · Strip   │
│  AU/CLAP plugin windows hosted in NSViews                                    │
└───────────────▲──────────────────────────────────┬───────────────────────────┘
                │ change notifications (snapshots) │ commands (edit, transport)
┌───────────────┴──── core-ffi (UniFFI) ───────────▼───────────────────────────┐
│ core-model   project · tracks · sequences/subsequences · events · undo       │
│ core-engine  real-time graph · transport · CoreAudio/CoreMIDI · lock-free Qs │
│ core-host    CLAP host · AU v2/v3 host (objc2/AudioToolbox) · plugin scanner │
│ core-io      project file · SMF · OMF 2.0 · AAF · audio file decode/stream   │
└──────────────────────────────────────────────────────────────────────────────┘
```

### Invariants

1. **Rust owns the model.** Swift never owns project state. It sends commands and receives immutable snapshots or diffs to draw.
2. **The real-time thread never blocks.** No locks, allocation, I/O or syscalls other than CoreAudio/CoreMIDI real-time calls. A test allocator enforces this.
3. **All edits are commands.** Every mutation is a reversible command, which gives undo/redo, enables testing, and later makes scripting possible.
4. **Editors are views.** Arrange, piano roll, list and Strip Chart all show and edit the same events through the same commands; a change in one appears in all immediately.
   - There is **one document-wide selection**: a time range (edit-in to edit-out) plus a set of selected items (notes, controllers, reference events, whole or partial audio regions). Every editor reads and writes it, and every view updates together.
   - Commands state their scope in their names (e.g. "Quantize Selection" vs "Quantize Track"). View filters hide items without deselecting them.

### Repository layout

```
vizdaw/
  crates/
    core-model/   core-engine/   core-host/   core-io/   core-ffi/
    scan-helper/            # out-of-process plugin scanner binary
  app/                      # Xcode project / SwiftPM packages
  docs/superpowers/{specs,plans}/
  LICENSE                   # GPL-3.0
```

## 5. Domain model

- **Project:** global settings, an **instrument list** (named destinations: a CoreMIDI endpoint + group/channel, or a hosted plugin instrument), a **media pool** (audio files, referenced relatively within the project package), and a set of **sequences**.
- **Media pool:** each file shows which regions use it and how much of it is unused. Commands: select unused, compact (drop unused audio), consolidate into the package, rename, and relink (Section 10). Deleting a file in use needs confirmation listing the affected regions.
- **Sequence:** the main unit of work. Each sequence has its own tracks, tempo map, meter map, loop/length, a start offset (for pickups before bar 1), a free-text comment, and an optional **keyboard slot**.
  - Two kinds: **recorded/edited** sequences hold ordinary event data; **generated** sequences produce their events from rules or patterns and their parameters. Both can be referenced, nested and auditioned the same way, and a generated sequence can be rendered into an ordinary one.
  - **Nested timing rule:** the playing (top-level) sequence's tempo and meter govern everything nested inside it. A sequence's own tempo and meter maps apply when it is played or edited on its own; editors offer a local-time view that shows a referenced sequence from its own bar 1 with its own maps.
  - **Keyboard slot:** an optional key chord (any key plus modifiers), unique in the project and refused at assignment if it clashes with a command binding or another slot. Pressing it makes the sequence current and auditions it. While recording, it inserts a reference to the sequence at the record position on the record track; in step recording the position then advances by the sequence's length, so a song can be typed in. The slot is a bindable command so later performance features can reuse it.
- **Subsequence:** a track event that **references another sequence**. A song is simply a sequence made of subsequences.
  - References can nest; cycles are rejected when a command is validated.
  - There are no fixed limits on how many sequences a track may reference or on nesting depth. If the engine cannot start a nested reference for lack of resources, it reports the failure instead of silently skipping it.
  - **Shared by default:** edits to a sequence apply to every place it is used. Pasting a sequence from the sequence list into a track creates a live reference, not a copy.
  - **Make Private Copy** gives one use its own new sequence (auto-named, still nestable and editable as a sequence).
  - **Flatten** turns a reference into an editable copy of its events.
  - Per-use fields on the reference event: start, transpose, mute. Its length is derived from the referenced sequence (a short looped track inside it repeats to fill that length).
  - **Make Reference from Selection** creates either one multi-track reference or one reference per track, with an optional repeat count.
  - **Referential integrity:** the sequence list shows each sequence's use count and its users, separates top-level sequences from those used only as references, asks for confirmation before deleting a sequence that is in use (naming what will change), and offers **Remove Unused Sequences**.
- **Track:** an untyped timeline lane that may hold MIDI events, audio regions, or both.
  - Each lane has a **MIDI destination** (an instrument-list entry) and an **audio output path** for its regions. Audio regions always follow the lane's audio output path.
  - Mute/solo/record-arm per lane; optional own length, independent loop flag (a short looped track repeats against the sequence, allowing polymetric loops) and a length lock that recording cannot change. The sequence length follows the longest track unless locked.
  - Non-destructive per-track play quantize and time shift.
  - **Alternate takes:** several versions of a lane, one of which plays. Each loop-record pass can become a take; a take can hold notes, regions and reference events; switching the playing take is undoable; per-event mute still works inside a take.
- **Channel strips** belong to things that produce audio: each hosted plugin instrument in the instrument list, and each lane's audio output path. A MIDI lane driving a plugin instrument has no strip of its own; its sound appears on that instrument's strip, shared with every lane or event targeting it. A lane holding both kinds feeds two strips.
- **Mixed content:** one selection, one clipboard and one bounce/capture can span MIDI events and audio regions together.
- **Time:**
  - Musical time is `i64` ticks at **15,360 ticks per quarter note**. This divides evenly for 2, 3, 5 and 7 subdivisions and exceeds any import/export resolution.
  - The tempo map converts ticks to samples, with a cached piecewise mapping.
  - Positions are shown and typed as **bar·beat·unit** with a user-selectable display unit; typed SMPTE positions round to that unit.
  - Tempo, meter, key signatures and markers are event data, editable in the list editor and Strip Chart. Meter changes fall on barlines.
- **Events (MIDI 2.0 native):**
  - Note on/off with 16-bit velocity, release velocity, and an optional attribute (e.g. pitch 7.9 for microtonal tuning).
  - 32-bit CC, pitch bend, channel pressure and poly pressure.
  - Per-note controllers and per-note pitch bend.
  - Program change with bank select.
  - SysEx (7-bit and 8-bit).
  - Text-type events (text, lyric, cue, marker, copyright), convertible to and from markers.
  - Reference events (see Subsequence). The model reserves room for further reference-event kinds later.
  - A per-event mute flag on notes, reference events and audio regions.
- **Event routing (hybrid):**
  - A lane's MIDI destination applies to its events by default.
  - An event may override the destination, and may carry an optional group/channel override that is used only when set.
  - At output every MIDI event resolves to a concrete UMP group and channel (from its destination unless overridden), as MIDI 2.0 per-note data and CLAP note expressions require.
  - SMF and MIDI Clip File import map each used channel (and group) to a destination, creating instrument-list entries as needed, while keeping the original group/channel values.
- **Audio region:** media reference, start tick, length, source offset (in samples), gain, fade in/out (with curve shapes). Fades and gain are re-editable parameters applied at playback. When two regions overlap on one lane, the later start takes over and the earlier resumes if it is longer.
- **Undo:** a command stack per project with multiple levels. Commands are grouped into transactions so each user gesture is one undo step.
  - Menu items name the action ("Undo Quantize").
  - A whole recording pass, including edits made while it ran, becomes one step after Stop; before Stop, those edits are undoable individually.
  - Instrument-list, destination and routing changes are undoable.
  - Recorded media files are kept while any undo step refers to them and cleaned up afterwards.

## 6. Engine

- **Thread model:** one real-time render thread driven by the CoreAudio IOProc. It joins the device's `os_workgroup`. Processing is block-based, with sample-accurate event offsets inside each block.
- **Render graph compilation:**
  - On every committed edit, a non-real-time thread compiles the model into a flat, time-sorted render graph (subsequences resolved under the nested timing rule of Section 5; nodes for lanes → instruments/plugins → strips → buses → master).
  - The new graph is published with a lock-free swap (an `arc-swap`-style pattern or a triple buffer).
  - Retired graphs are freed on a separate thread.
- **Plugin delay compensation (PDC)** from day one.
- **Multi-core rendering** is deferred. The graph is designed so independent subgraphs can run in parallel within the same workgroup later.
- **Transport:** play/stop/locate/loop and record with pre-roll. Position is shared with the UI through atomics. Editing continues while the transport runs, including during loop recording.
- **Recording:** audio input goes into a lock-free ring buffer and a separate thread writes it to disk (CAF, 32-bit float). MIDI input is timestamped with host time and mapped onto the timeline.
  - Record modes are independent choices: real-time or step; replace or overdub; punch on/off; loop on/off. Replace affects only the recorded range.
  - More than one lane can be record-armed.
  - Loop recording: one key keeps the passes so far, another discards everything since the last keep; kept passes can become alternate takes.
  - **MIDI input effects:** an optional stage (e.g. arpeggiator, groove) processes incoming MIDI while recording; what is recorded is the stage's output.

### CoreAudio I/O

- Talks directly to the CoreAudio HAL (`AudioDeviceCreateIOProcID`) and works with any CoreAudio device: class-compliant USB/Thunderbolt interfaces, interfaces with their own drivers, built-in audio, and aggregate devices.
- Buffer sizes from 32 to 2048 frames; any sample rate the device supports.
- Reads device and stream latency plus safety offsets to compensate recording alignment automatically.
- Multi-channel input/output routing using the channel names the device reports.
- Separate input and output devices are supported through a private aggregate device with drift correction.
- Live handling of device removal and of sample-rate or buffer-size changes, with a clean engine restart.
- Optional exclusive device access (hog mode).

### CoreMIDI I/O

- A UMP-native client using `MIDIEventList` on input and output.
- Output is scheduled using host time.
- MIDI 1.0 endpoints are converted (up on input, down on output) using the MIDI 2.0 specification's translation rules.

## 7. Plugin hosting

- **AU v2 and v3** through AudioToolbox/AVFoundation via `objc2`.
  - AUv3 plugins run out of process where the plugin supports it.
  - MIDI 2.0 events are delivered through `scheduleMIDIEventListBlock`.
- **CLAP** through the `clack` host crate (or our own bindings if it falls short).
  - Native MIDI 2.0 events and note expressions.
- **Scanner:** the `scan-helper` subprocess validates and catalogues plugins. A plugin that crashes or hangs during scanning is blocklisted with its reason. Results are cached and keyed by bundle path and version.
- **Plugin UI:** the core hands Swift an `NSView`/`NSViewController` handle (AU) or a parent view to attach to (CLAP), which Swift places in a window.
- **State:** plugin state blobs are stored in the project. Parameter IDs are kept stable so automation can refer to them.

## 8. Files and import

- **Project format:** a `.vizdaw` package directory containing a versioned project document (Rust `serde`; format chosen in the core-model plan), `Media/` and `Autosave/`. Saves write to a temporary file and atomically rename it over the old one. Autosave keeps rolling snapshots.
- **Standard MIDI Files** (types 0/1) import/export, plus the MIDI 2.0 Clip File for import/export. Channel/group mapping on import follows the event routing rules in Section 5.
- **OMF 2.0 and AAF** import: audio clips, positions, source offsets, fades, clip gain and basic volume/pan automation; embedded or linked media.
  - AAF uses the open AAF SDK, wrapped through the C ABI, or a Rust reader if one is suitable — to be decided in that sub-project's spec.
  - OMF needs a dedicated reader, based on the published OMF Interchange Specification 2.1.
- **Audio files:** WAV, AIFF, CAF and FLAC decoded via `symphonia` or ExtAudioFile; playback streams from disk.
- **External sample editor:** a region's audio file can be opened in a user-chosen external editor (set in Settings). VizDAW watches the file, detects changes and reloads it, updating every region that uses it.

## 9. Native macOS UI

- A SwiftUI app shell: `NavigationSplitView` (sidebar with sequences, instruments and media), a unified toolbar with transport, inspectors, Settings scene, full menu bar and keyboard shortcuts, dark mode, accessibility labels on all controls.
- **Sequence sidebar** is a working surface: per sequence it shows name, keyboard slot, comment and use count; supports drag reordering, duplicate, and new-from-template.
- **High-performance editors** are custom AppKit views drawn with Metal/Core Animation:
  - Arrangement timeline: groups each lane's events into blocks automatically (by silence gaps, fixed bar blocks, or whole lane, with a user-set size); a reference event is always its own block. Double-clicking a block opens exactly that block in a detail editor; a drag moves (with a modifier, copies) or merges blocks into another lane. A lightweight drawing mode keeps large projects fast.
  - Piano roll, with per-note expression lanes. The cursor's action depends on its position over a note (start moves, middle transposes, end resizes), with a modifier to copy; the pencil inserts at a default size or paints a length; notes sound while dragged. It can show several lanes, shows the lanes' audio regions in their own strip, and has an optional keyboard beside it.
  - List editor (fast, keyboard-first, inline editing). Typing a bar number moves an event to that downbeat; a single sub-field can be edited alone; Enter commits and the list follows the moved event; pitch and velocity can be entered by playing a note; insert repeats the last inserted kind; optional columns (end, duration, release velocity, SMPTE); type/destination filters can be applied to all lists or saved as default. Reference events and audio regions are rows like any other (opening a reference opens its sequence).
  - Strip Chart with smooth 32-bit controller curves. Besides drawing new values, a drawn line over a range can modify existing data (set, scale, add, limit max/min, legato for durations, thin/thicken, compress) with a line shape (straight, free, curved, flat, random). A modifier limits the edit to selected notes. Inserting writes to the shown lane; modifying acts on every selected lane in the range. It covers velocity, release velocity, duration, controllers, pitch bend, pressure, program changes, tempo and plugin automation; drawn tempo ramps sound linear.
- **Shared editor behaviours:**
  - A drag in any ruler always selects a time range across all shown lanes and event kinds, whatever the current tool.
  - **Exact mode:** any drag (move, transpose, resize, scale time, Strip Chart line) can instead open a dialog pre-filled with the dragged amounts.
  - **Nudge** moves the selection by a set time or pitch step; pitch nudge can follow a key and scale.
  - **Select by rule** (time, type, pitch, velocity, duration, destination, metric position, every Nth) with select, add and refine modes; rules can be saved as named presets followed by a modify step; "Repeat Last Modify".
- **Keyboard:**
  - Every command, including those without a menu item (transport, locate, mode toggles, keep/discard loop pass, next/previous record lane), has a stable identifier and can be bound to a key. Shipped defaults cover a small core; users can save, load and reset bindings, and bound keys appear in menus.
  - By default, unmodified single keys drive the transport (play/pause, stop, record, play from position, scrub, previous/next marker) and mode toggles (overdub, punch, loop, step, snap).
  - A key focuses the position, edit-in or punch field; digits with separators enter bar·beat·unit and Enter commits. Number keys jump to numbered locators, with modifiers to set and clear them (a preference can reserve the number row for locators).
  - Step entry chooses note values from number keys, with keys for dot, triplet, tie, rest and delete-last-step.
  - Modified Space plays from or only the selection; modified arrows nudge in time and pitch or move record-arm between lanes; one key toggles the shared edit-grid snap.
- Plugin windows are standard macOS windows hosting the plugin views.
- The detailed UX design is its own sub-project (Section 11, step 7). Earlier sub-projects ship only functional UI.

## 10. Faults and errors

- **Real-time thread:** written to be panic-free (no indexing that can go out of bounds, no `unwrap`; enforced by clippy lints on the engine crate). Faults (overload, plugin errors, xruns, nested references that could not start) set atomic flags that the UI polls and reports.
- **Plugins:** a plugin that crashes during scanning is blocklisted. Out-of-process AUv3 plugins are isolated. A missing plugin becomes an offline placeholder that keeps its saved state.
- **Media:** missing files become offline regions with a relink dialog. Files changed outside VizDAW (e.g. by an external sample editor) are detected and reloaded.
- **Devices:** handled as in Section 6.
- **FFI:** every core API returns a `Result` mapped to a Swift `throws`. No panics cross the FFI boundary (`catch_unwind` at the boundary).

## 11. Sub-projects (each gets its own spec, plan and build cycle)

1. **Research dossier (private):** an REA study of Studio Vision Pro 4.5.3 covering menus, windows, dialogs, the editing model, sequence/subsequence behaviour, alternate takes, the list editor and Strip Chart. Output is a behaviour description in `~/workspace/SVP/research/`.
2. **Core model, engine and MIDI:** the core-model crate with commands and undo, the core-engine crate with CoreAudio/CoreMIDI, the FFI layer, and a minimal Swift window that plays a sequence to an external MIDI device.
3. **Plugin hosting:** AU first, then CLAP; the scanner; plugin windows.
4. **Editors:** arrangement, piano roll, list, Strip Chart, and the sequence/subsequence workflow in the UI.
5. **Audio tracks:** recording, regions, fades, disk streaming.
6. **Import:** SMF and MIDI Clip File, then OMF, then AAF.
7. **UI polish:** the full modern Mac UX pass.

**Open decision:** a dedicated drum-grid editor is under consideration pending further research; it is neither in v1 scope nor a non-goal yet.

## 12. Testing

- **core-model:** property tests (`proptest`) checking that for any command sequence, applying then undoing returns exactly the original state; checks that nested subsequences never form cycles; a track can reference a large number of distinct sequences with no fixed cap; private copy, flatten and delete-in-use keep use counts correct.
- **core-engine:** offline rendering compared against golden audio files; tests of the tick↔sample conversion, including nested sequences following the playing sequence's tempo and meter; destination → UMP group/channel resolution stays stable when a destination's channel changes; a real-time allocation guard on the render path.
- **core-io:** a test set of sample SMF, OMF and AAF files, made only from material that may be redistributed under GPL-3.0; SMF/Clip File round trips preserve group/channel.
- **core-host:** loading and processing tests using Apple's built-in AUs and the open-source CLAP example plugins.
- **App:** XCUITest smoke tests; manual latency checks on real interfaces.
- **CI:** GitHub Actions on macOS 14 (Apple Silicon) — `cargo test`, `clippy -D warnings`, `xcodebuild test`.
