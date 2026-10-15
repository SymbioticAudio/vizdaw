# VizDAW — Design Spec

- **Date:** 2026-10-15
- **Status:** Draft, awaiting review
- **License:** GPL-3.0-only

## 1. Intent

VizDAW is a new, open-source macOS DAW inspired by Opcode Studio Vision Pro 4.5.
It is not a port and not a clone. It keeps the ideas that made Studio Vision
fast to work in and rebuilds them on a modern stack:

- MIDI-first editing: list editor, graphic (piano-roll) editor, Strip Chart controller editing, keyboard-driven workflow.
- Sequences and subsequences as the arrangement model, with alternate takes.
- Integrated audio tracks edited alongside MIDI.
- Modern plugin hosting (AU v2/v3, CLAP), MIDI 2.0 throughout, and a native modern Mac UI.

**Success for v1:** a musician can build a song from nested sequences with
MIDI and audio tracks, play AU/CLAP instruments and effects plus external
MIDI gear with low latency on any CoreAudio interface, import SMF/OMF/AAF
material, and edit it in list, piano-roll, Strip Chart and arrangement views
in an app that feels native on macOS.

### Non-goals (v1)

- Faithful reproduction of Studio Vision's UI, code or file format.
- Importing original Studio Vision song files (may be revisited later).
- Native Logic Pro (`.logicx`) import. The format is undocumented and changes between versions. Users export OMF/AAF + SMF from Logic instead.
- Audio-to-MIDI conversion, notation, video, MIDI-CI Property Exchange/Profiles, external clock/MTC sync, multi-core rendering. All are deferred; the architecture must not preclude them.
- Intel Macs, macOS < 14, Windows/Linux.

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
- **Sequence:** the main unit of work. Each sequence has its own tracks, tempo map, meter map, loop/length, and an optional **keyboard slot** for instant selection or triggering.
- **Subsequence:** a track event that **references another sequence**. A song is simply a sequence made of subsequences.
  - References can nest; cycles are rejected when a command is validated.
  - Edits to a sequence apply to every place it is used.
  - **Flatten** turns a reference into an editable copy of its events.
- **Track:** MIDI or audio; destination; mute/solo/record-arm; **alternate takes** (several versions of the track, one of which plays).
- **Time:**
  - Musical time is `i64` ticks at **15,360 ticks per quarter note**. This divides evenly for 2, 3, 5 and 7 subdivisions and exceeds any import/export resolution.
  - The tempo map converts ticks to samples, with a cached piecewise mapping.
- **Events (MIDI 2.0 native):**
  - Note on/off with 16-bit velocity and an optional attribute (e.g. pitch 7.9 for microtonal tuning).
  - 32-bit CC, pitch bend, channel pressure and poly pressure.
  - Per-note controllers and per-note pitch bend.
  - Program change with bank select.
  - SysEx (7-bit and 8-bit).
  - Each event is addressed by group (0–15) and channel (0–15).
- **Audio region:** media reference, start tick, length, source offset (in samples), gain, fade in/out (with curve shapes).
- **Undo:** a command stack per project. Commands are grouped into transactions so each user gesture is one undo step.

## 6. Engine

- **Thread model:** one real-time render thread driven by the CoreAudio IOProc. It joins the device's `os_workgroup`. Processing is block-based, with sample-accurate event offsets inside each block.
- **Render graph compilation:**
  - On every committed edit, a non-real-time thread compiles the model into a flat, time-sorted render graph (subsequences resolved; nodes for tracks → instruments/plugins → buses → master).
  - The new graph is published with a lock-free swap (an `arc-swap`-style pattern or a triple buffer).
  - Retired graphs are freed on a separate thread.
- **Plugin delay compensation (PDC)** from day one.
- **Multi-core rendering** is deferred. The graph is designed so independent subgraphs can run in parallel within the same workgroup later.
- **Transport:** play/stop/locate/loop and record with pre-roll. Position is shared with the UI through atomics.
- **Recording:** audio input goes into a lock-free ring buffer and a separate thread writes it to disk (CAF, 32-bit float). MIDI input is timestamped with host time and mapped onto the timeline.

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
- **Standard MIDI Files** (types 0/1) import/export, plus the MIDI 2.0 Clip File for import/export.
- **OMF 2.0 and AAF** import: audio clips, positions, source offsets, fades, clip gain and basic volume/pan automation; embedded or linked media.
  - AAF uses the open AAF SDK, wrapped through the C ABI, or a Rust reader if one is suitable — to be decided in that sub-project's spec.
  - OMF needs a dedicated reader, based on the published OMF Interchange Specification 2.1.
- **Audio files:** WAV, AIFF, CAF and FLAC decoded via `symphonia` or ExtAudioFile; playback streams from disk.

## 9. Native macOS UI

- A SwiftUI app shell: `NavigationSplitView` (sidebar with sequences, instruments and media), a unified toolbar with transport, inspectors, Settings scene, full menu bar and keyboard shortcuts, dark mode, accessibility labels on all controls.
- **High-performance editors** are custom AppKit views drawn with Metal/Core Animation:
  - Arrangement timeline.
  - Piano roll, with per-note expression lanes.
  - List editor (fast, keyboard-first, inline editing).
  - Strip Chart with smooth 32-bit controller curves.
- Plugin windows are standard macOS windows hosting the plugin views.
- The detailed UX design is its own sub-project (Section 11, step 7). Earlier sub-projects ship only functional UI.

## 10. Faults and errors

- **Real-time thread:** written to be panic-free (no indexing that can go out of bounds, no `unwrap`; enforced by clippy lints on the engine crate). Faults (overload, plugin errors, xruns) set atomic flags that the UI polls and reports.
- **Plugins:** a plugin that crashes during scanning is blocklisted. Out-of-process AUv3 plugins are isolated. A missing plugin becomes an offline placeholder that keeps its saved state.
- **Media:** missing files become offline regions with a relink dialog.
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

## 12. Testing

- **core-model:** property tests (`proptest`) checking that for any command sequence, applying then undoing returns exactly the original state; checks that nested subsequences never form cycles.
- **core-engine:** offline rendering compared against golden audio files; tests of the tick↔sample conversion; a real-time allocation guard on the render path.
- **core-io:** a test set of sample SMF, OMF and AAF files, made only from material that may be redistributed under GPL-3.0.
- **core-host:** loading and processing tests using Apple's built-in AUs and the open-source CLAP example plugins.
- **App:** XCUITest smoke tests; manual latency checks on real interfaces.
- **CI:** GitHub Actions on macOS 14 (Apple Silicon) — `cargo test`, `clippy -D warnings`, `xcodebuild test`.
