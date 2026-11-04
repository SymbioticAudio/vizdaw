# VizDAW 7c — The Audio Editor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The bottom pane gains an Audio Editor for the selected audio region:
- the waveform drawn large, with zoom and a level scale;
- gain, fade in and fade out with their shapes, trim of the start and the length, and normalize as gain;
- each operation has a handle on the waveform and a typed value;
- everything goes through the existing model commands, gestures and undo.

There is no pan per region (pan stays on the channel strip), no destructive edit (those stay with the external sample editor) and no format change.

**Architecture:**
- **`core-ffi`:** one read-only export, `Session::region_envelope`, so the editor draws the gain and fade curves exactly as the engine plays them (Ruling 4). `state.get audio_editor` and its rows.
- **`core-control`:** the `audio_editor` state target, `ui.pane`'s `audio_editor` view, and `editors.audio_editor` (⌘5).
- **`VizDAWAppModel`** (tested):
  - `AudioEditorEntry`, the typed grammar;
  - `FrameAxis`, the editor's time axis in media frames;
  - `AudioEditorModel`, which knows the region, zoom, typed commits and normalize, and builds the state row;
  - `PaneView.audioEditor` on both main views' bottom lists.
- **`VizDAWEditors`:**
  - `AudioEditorGeometry`: handles, hits, drag targets, the level scale, the ruler, and waveform scaling (pure, tested);
  - `AudioEditorCanvas`, an AppKit canvas;
  - new `EditorTheme` roles.
- **`VizDAW`:** `AudioEditorPane`, a SwiftUI bar of `SymbioticControls.NumberBox`es, shape pickers, Normalize and zoom buttons over the canvas.

**Tech Stack:** Rust 2024 (`core-ffi` with UniFFI 0.32.2, `core-control`); Swift 6 / SwiftPM (Swift Testing; SwiftUI and AppKit), macOS 14+; `symbiotic-design` 0.2.1 (pinned, unchanged).

**Spec:** `docs/superpowers/specs/2026-11-03-ui-polish-design.md`. Read §2 to §7. §6 item 2 and §7's row 7c define the scope, and every "(owner)" there binds. This plan delivers:
- §6 item 2: the audio editor;
- §2 item 3's bottom-pane list entry "audio editor";
- §2 item 11's 7c part: the root README adds the audio editor;
- §4's "breaking agents and tests": the control channel reads the editor's state;
- §5's hostile-input rule.

It builds on 7a and 7b. Read `docs/superpowers/plans/2026-11-03-window-views-panes.md` Rulings 3 (the bottom pane's list: "7c adds one picker entry and one `PaneView` case") and 10 (the control channel). Read `docs/superpowers/plans/2026-11-03-themes-strip.md` Rulings 13 (theme roles, scans) and 15 (typed values).

7d (clean-up) is out of scope.

## Global Constraints

- **Repo and branch.** `/Users/lattawb/workspace/vizdaw`, branch `sp7-ui`, after `5224ca1`. Never switch branches, never push.
  - Every bash block starts with `cd /Users/lattawb/workspace/vizdaw` and `export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools`.
  - A `git commit` without `DEVELOPER_DIR` fails silently: check every commit with `git log -1 --oneline` and `git show --stat HEAD`.
- **Commits.** Commit at the end of every task, adding only the files the task lists, by path. Never use `git add -A` or `git add .`: other agents work in this tree. Never commit `.superpowers/`, `.DS_Store` or generated files (`app/Sources/VizDAWCore/vizdaw_ffi.swift`, the FFI headers). Trailers, using the `Co-Authored-By` line of the model you actually are:
  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P
  ```
- **Manifests.** `Cargo.lock`, every `Cargo.toml`, `app/Package.swift` and `app/Package.resolved` stay as they are: no new crate, no new dependency, and the `symbiotic-design` pin stays at 0.2.1.
- **Swift only through the scripts**, from the repo root: `scripts/build-ffi.sh`, `scripts/swift.sh build`, `scripts/swift.sh test`, `scripts/make-app.sh`. Never call `swift`, `xcodebuild` or `xcrun` directly.
- **Clean room.**
  - Never read `~/workspace/SVP/research` or any Studio Vision material.
  - No Opcode, Studio Vision, SVP, Logic, Apple, Midas, Soundcraft, Steinberg, Cubase, NI, Native Instruments or Microsoft names, logos or artwork in the UI, accessibility labels, test names, comments, the READMEs or the commit messages.
  - The fade shapes keep their generic names: Linear, Equal Power, Exponential, S-Curve.
- **No GUI tools, no real devices.**
  - Never use GUI, computer-use, browser-automation or accessibility-automation tools. The owner checks the editor by eye (README steps 217–224, Task 7).
  - Tests never open real audio or MIDI and never trigger the microphone prompt:
    - Swift tests use `FakeSession`;
    - `core-ffi` tests that start the transport use their file's `serial()`; the files this plan writes to (`overviews.rs`, `regions.rs`, `control.rs`) open no device and keep their convention of none;
    - the e2e runs with `VIZDAW_AUDIO=offline`, as `App::start` sets it.
  - Afterwards `pgrep -lf MacOS/VizDAW` prints nothing and `ls -d /tmp/vze*` prints nothing new.
  - **Never touch the real `~/Library/Application Support/VizDAW`, the real preferences or the real Trash.** Swift tests use `scripts/swift.sh test`'s own `VIZDAW_TRASH_DIR` and `VIZDAW_SCRATCH_DIR`, and preferences go through `MemoryDefaults`. The e2e's homes are under `/tmp/vze*`.
- **Programmatic and channel calls never open dialogs.**
  - Normalize in the editor takes its peak from the editor's own Peak box: no panel, from a click, a key or the channel.
  - A refused typed value shows its hint in the status line, never in an alert. So does a refused or busy Normalize, as `RegionCommands.normalize(peakDb:_:)` already does.
  - `editors.audio_editor` and `ui.pane … audio_editor` show the pane and open nothing.
- **Real time.** No allocation, lock or IO on the render thread. The engine is not changed. `region_envelope` runs on the caller's thread under the session lock, reads no file, and computes from the document. Tasks 1 and 7 run the rt-check gates:
  - `cargo clippy -p vizdaw-play --all-targets --features rt-check -- -D warnings`;
  - `cargo test -p core-engine --features rt-check --test rt_alloc --test rt_alloc_graph --test rt_alloc_record --test rt_alloc_gestures --test rt_alloc_bounce`.
- **FFI.**
  - The new export is `Session::region_envelope`. It runs inside `ffi_guard` (which is `catch_unwind`) and returns `Result<Vec<f32>, VizError>`.
  - The new UniFFI types are `StateTargetRow::AudioEditor`, `ControlResult::AudioEditor`, `AudioEditorStateRow`, `AudioEditorRegionRow` and `AudioEditorBoxRow`.
  - `FakeSession` gains `regionEnvelope` in Task 1, so the Swift build stays green.
- **New commands get hostile-input tests:**
  - `hostile_region_envelope_calls_are_refused` and `hostile_trims_are_refused_and_change_nothing` (core-ffi, Task 1). Trim is not new, but this plan is its first UI caller;
  - `audio_editor_state_target_parses` and `ui_pane_names_the_audio_editor` (core-control, Task 5);
  - `uiPaneTakesTheAudioEditorOnlyAtTheBottom` (app, Task 5);
  - `hostileGainsAreRefused`, `hostileTimesAreRefused` and `peaksClampAndRefuseMinusInfinity` (Task 2);
  - the e2e's hostile block (Task 7).
- **Swift 6.1 (CI runs macos-15) as well as the local toolchain.**
  - Pass no weak `self` and no non-Sendable value into `MainActor.assumeIsolated`. Capture a strong `let` made before the closure, and bring a notification or event in through `nonisolated(unsafe) let`, as `KeyMonitor` does.
  - No `@State`, `@GestureState` or `@Entry` in package code. Spell out `State(initialValue:)` and declare environment values with an `EnvironmentKey`.
  - Use no API newer than macOS 14.
- **Theme.** Every colour, metric and spacing value comes from the theme.
  - The canvas's colours are `EditorTheme` roles (Task 3).
  - The SwiftUI bar's gaps and padding are `theme.spacing`: `AudioEditorPane.swift` joins `ThemeScanTests.spacingFiles`.
  - Fonts are `theme.font` / `theme.digitsFont`.
  - The colour and metric scans (`noColourLiteralsOutsideTheThemeBridges`, `noCornerRadiusOrFontSizeLiterals`) cover the new files with no new exception.
  - Hit and handle sizes are geometry constants in `AudioEditorGeometry`, as `RegionHandles.size` is.
- **Icons only through `AppIcon`:**
  - `.audio` for the empty state;
  - `.zoomIn` and `.zoomOut` for the time zoom;
  - `.expand` and `.collapse` for the waveform height.

  No new `AppIcon` case and no SF Symbol literal (`theAppUsesNoSystemSymbols` holds).
- **Format.**
  - `core_model::file::VERSION` stays 4, and no model type changes.
  - `EditorLayout.version` stays 1. The only change to `ui.json`'s contents is that a bottom pane's `view` may now read `"audio_editor"`, which an older build decodes tolerantly as its default (7a Ruling 1).
  - The editor's zoom is not saved (Ruling 3).
  - `fixtures/demo.vizdaw` is unchanged.
- **Lints**, in Rust as in every plan: `#![deny(missing_docs)]`, and no unwrap, expect, indexing or panic outside tests.
- **TDD.** Every new test is run and seen to fail (RED) before the code that makes it pass, except characterisation tests named as such (Task 1's trim test).
- **Each task ends green.**
  - Always: `cargo fmt --all -- --check`.
  - Rust tasks (1, 5, 7):
    - `cargo clippy --workspace --all-targets -- -D warnings`;
    - `cargo clippy --workspace --all-targets --target x86_64-unknown-linux-gnu --target-dir target/linux-check -- -D warnings`;
    - `cargo test --workspace`.
  - Every task that touches Swift (all of them): `scripts/build-ffi.sh`, `scripts/swift.sh build` and `scripts/swift.sh test`, then the no-brand build on a fresh scratch path: `rm -rf .build-nobrand && VIZDAW_NO_BRAND=1 scripts/swift.sh build --scratch-path .build-nobrand`.
  - Task 7: everything, plus `scripts/make-app.sh` and the e2e.
- **Baselines and expected counts.** The baselines are 7b's end, as `.superpowers/sdd/2026-11-03-themes-strip/progress.md` records it at the final wave:
  - Rust 2023 passed / 20 ignored (macOS);
  - Swift 1148;
  - e2e 16;
  - registry 154;
  - app README last step 216.

  Tasks 1, 2 and 3 run in that order (Task 1 regenerates the FFI, which every later Swift build needs), so the running Swift totals are +6 (2), +19 (3), +28 (4), +31 (5) and +36 (6). Re-measure at `5224ca1` before Task 1 and write the numbers into this plan's `progress.md` (`.superpowers/sdd/2026-11-04-audio-editor/progress.md`). The deltas below are what binds.

  | Task | Rust (macOS and Linux) | Swift | Other |
  |---|---|---|---|
  | 1 envelope export, trim hostility | +4 | +0 (compiles) | |
  | 2 typed grammar | — | +6 | |
  | 3 axis, geometry, span, theme roles | — | +13 | |
  | 4 editor model, pane view | — | +9 | |
  | 5 control channel, ⌘5 | +3 | +3 | registry 155 |
  | 6 canvas and pane | — | +5 | |
  | 7 e2e, READMEs | ignored +1 | — | e2e 17; README 217–224 |
  | **After** | **+7 / ignored +1** | **+36** | |

  Report the measured counts at every task's end. A difference is explained, never hidden.
- **How code is given.** Signatures, test names and assertions, not bodies. Where the tree differs from a name here, use the tree's name and say so in the report.

## Review Focus

These are the five failure modes a user is most likely to hit that the spec implies but no feature test exercises, most likely first. Each is pinned by the named tests in its task.

1. **Careless or hostile typing in the editor's boxes.** The cases:
   - empty or spaces;
   - `abc`, `1.2.3`, `1e3`, `nan`, `inf`;
   - a sign on a time (`-5`, `+5`, `−5`);
   - `5 sec`, `5ms5`, `1:75`, `1:5`, `:30`;
   - full-width or Arabic-Indic digits;
   - a control character;
   - a 300-character paste;
   - for a gain: `30` or `-200`;
   - for a fade: longer than the region;
   - for the start: past the end;
   - for the length: `0` or past the file's end;
   - for the peak: `-inf`.

   Expected:
   - a value the grammar accepts is applied, clamped to what the region and file allow, as one undo step;
   - a value equal to the current one sends nothing;
   - anything else is refused: the box shows the old value, the status line shows the hint, and nothing reaches the core.

   Pinned by Task 2's `hostileGainsAreRefused`, `hostileTimesAreRefused` and `peaksClampAndRefuseMinusInfinity`; Task 4's `typedGainIsOneUndoStepAndARefusalSendsNothing`, `typedFadesClampToWhatTheOtherLeaves` and `typedStartAndLengthClamp`; and the e2e's `abc` (Task 7).
2. **Trims at the file's edges.** The cases:
   - the start handle dragged past the file's start, or past the sequence's start (a region at bar 1);
   - the end handle dragged past the file's end;
   - the start dragged past the end;
   - a drag of `.infinity` or `.nan` points;
   - a document whose media is shorter than its region claims;
   - extreme frame counts over the FFI.

   Expected:
   - drags clamp at the file's edges or the sequence start, whichever is nearer, and at one frame of length;
   - nothing alerts;
   - the core refuses anything outside the file and changes nothing.

   Pinned by `trimDragsClampToTheFileAndTheRegion` (Task 3), `typedStartAndLengthClamp` (Task 4, its bar-1 case), `aTrimDragPastTheFileStartClamps` (Task 6) and `hostile_trims_are_refused_and_change_nothing` (Task 1).
3. **The region changes under the editor.** The cases:
   - another region selected;
   - a note, or two regions, selected;
   - the region deleted;
   - its import undone;
   - its media going offline.

   Expected:
   - the editor follows the selection and fits a newly shown region;
   - otherwise it shows "Select one audio region to edit it." and typing sends nothing;
   - an offline region still shows its bounds, hatching and envelope, and Normalize answers in the status line.

   Pinned by `theEditorFollowsTheSelectedRegion` and `aVanishedRegionShowsTheMessage` (Task 4), and `region_envelope_is_zero_outside_and_answers_offline` (Task 1).
4. **Fades meeting each other and the trim.** The cases:
   - a fade handle dragged past the other fade;
   - a typed fade longer than the region minus the other fade;
   - a shape change;
   - a trim shorter than the fades.

   Expected:
   - fades stop at what the other leaves;
   - a shape change keeps the length;
   - a trim clamps the fades, as the model already does (`clamp_fades`).

   Pinned by `fadeDragsStopAtTheOtherFade` (Task 3), and `typedFadesClampToWhatTheOtherLeaves` and `shapesKeepTheirLength` (Task 4).
5. **Extreme sizes and zooms.** The cases:
   - a region a few points wide;
   - zooming in or out without end;
   - a 120-point bottom pane;
   - waveform height ×16;
   - gain +24 dB on a loud file;
   - a stereo file;
   - a pane wider than the core's column limit;
   - the file's start scrolled to mid-pane, or its end short of the pane's right edge.

   Expected:
   - every handle of a tiny region stays reachable;
   - the zoom stops at its limits;
   - level marks never overlap;
   - samples pushed over 0 dBFS by the gain show clip marks;
   - no request exceeds 8192 columns, and the waveform and envelope requests cover the same in-file span, so their columns line up; nothing traps on a negative or non-finite frame.

   Pinned by `handlesOfATinyRegionStayReachable`, `scaleMarksNeverOverlap`, `waveformScalesByEnvelopeAndAmplitude`, `columnsNeverExceedTheCoreLimit`, `spanStaysInsideTheFile` and `zoomStaysInRange` (Task 3).

## Plan Rulings

Each ruling decides something the spec leaves to the builder, as "what — why — cost if wrong".

1. **Ruling (where it lives): a bottom-pane view, `PaneView.audioEditor`, not a fifth detail editor.**
   - The raw value is `"audio_editor"` and the title is "Audio Editor".
   - The bottom lists become:
     - Arrange: `[.editor, .audioEditor, .mixer]`;
     - Mix: `[.editor, .audioEditor]`.
   - The picker therefore reads Piano Roll, List, Strip Chart, Drum Grid, Audio Editor, then Mixer (Arrange only). `state.get layout`'s bottom `views` gains `"audio_editor"` after `"drum_grid"`.
   - **⌘5** runs the new registry command `editors.audio_editor` ("Audio Editor", menu Editors). It shows the current main view's bottom pane on the audio editor, through `ViewCommands.choose(.bottom, .audioEditor, kind: nil, shown: true, size: nil, in: nil, m)`. No ⌥⌘5: there is no torn audio editor.
   - Not a `DetailEditorKind`: those have targets (block, lane, sequence), torn windows and `DetailSlot` maps. The audio editor's target is the selection.
   - Why: spec §2 item 3 (the bottom list), §6 item 2 ("a bottom-pane view"), and 7a Ruling 3 ("7c adds one picker entry and one `PaneView` case"). It is in the Mix view's list too, as the editor kinds are, because the selection is shared by both views (spec §2 item 2).
   - Cost if wrong: one list entry, or a later torn window.
2. **Ruling (what it edits): the single selected audio region.**
   - `DocumentModel.editedRegion: EditedRegion?` is today's `inspectorRegion` rule, which becomes `editedRegion?.row`: one selected event of the shown sequence that is an audio region, with its lane and media row.
   - Anything else (nothing, a note, two events, a region that vanished) shows `AudioEditorModel.emptyMessage = "Select one audio region to edit it."`, with `AppIcon.audio`.
   - Why: spec §6 item 2 ("the selected audio region"). One rule for the inspector and the editor.
   - Cost if wrong: one property.
3. **Ruling (the time axis): media frames, showing the whole file.**
   - `FrameAxis` maps the media item's frames to points. The canvas draws:
     - the file's audio outside the region, dimmed (`audioWaveTrimmed`), so a trim can extend into it;
     - the region's part, shaped by its gain and fades.
   - The ruler reads the file's time in `Formatting.time` (`m:ss.fff`), so the Start box and the ruler agree.
   - When a different region is shown, the axis fits it (`FrameAxis.fit`: the region plus 5% of its length each side). While the same region stays shown, the zoom and scroll stay.
   - The zoom lives in memory on `AudioEditorModel` and is not saved. Saving it would add `ui.json` state for no owner request, so it is a backlog row.
   - Limits:
     - 1 frame per point at the closest;
     - the whole file in half the width at the farthest (`maxFramesPerPoint`);
     - the origin can scroll the file's start to the middle of the pane and its end to the middle, no further.
   - Why: spec §6 item 2 ("waveform drawn large, with zoom"), and "trim of the start and length" needs the audio beyond the region in view.
   - Cost if wrong: a bars-and-beats ruler is one more marks function.
4. **Ruling (model gap: the heard envelope is not exported): add `Session::region_envelope`.**
   - **The gap.** The editor must draw the gain and fade curves as they will sound. The curve function, `core_engine::schedule::fade_curve`, is used by the engine and by `region_waveform` (through `overviews::fade_gain`), but nothing exports it to Swift. Re-implementing four curves in Swift would drift.
   - **The design, within "no format change":**
     ```
     pub fn region_envelope(&self, event: u64, from_frame: u64, to_frame: u64, columns: u32) -> Result<Vec<f32>, VizError>
     ```
     - It is read-only: no document, file or format change.
     - The frames are media frames, as `overview`'s are. Column `i`'s centre is `from.saturating_add(floor((i + 0.5) × (to − from) / columns) as u64)`, computed in f64 with the saturating `as` cast.
     - Each value is `core_model::mixer::db_to_gain(f64::from(gain_db)) as f32 × overviews::fade_gain(c − source_offset, length, fade_in, fade_out)` when `source_offset ≤ c < source_offset + length`, and 0 elsewhere. That is the engine's own gain conversion (0 at −144), so the envelope matches playback. A non-finite `gain_db` (which `store` already refuses) gives 0, so every value is finite by construction.
     - The envelope needs no file, so an offline region answers too.
   - **Errors:**
     - `check_columns` (1…8192);
     - `from_frame >= to_frame` gives "from_frame must be before to_frame";
     - an unknown event, a non-region ("‹id› is not an audio region") and unknown media are refused, as in `region_waveform`.
   - The pure part lives in `crates/core-ffi/src/overviews.rs`, next to `fade_gain`. To stay under clippy's seven-argument limit it takes a small struct:
     - `pub struct RegionShape { pub source_offset: u64, pub length: u64, pub gain_db: f32, pub fade_in: Fade, pub fade_out: Fade }`;
     - `pub fn envelope(region: &RegionShape, from: u64, to: u64, columns: u32) -> Vec<f32>`.
   - Why: spec §6 item 2 ("fade-in and fade-out (with shapes)") drawn truthfully, using "only operations the model already has" (spec §3): this export adds no operation.
   - Cost if wrong: the canvas would draw a straight-line approximation instead.
5. **Ruling (model check: every editing operation already exists).** No new model command and no `core-model` change.

   | Operation | Path | Undo label |
   |---|---|---|
   | gain (drag) | `beginGesture("Region Gain")`, then `.audio(edit: .setGain)` | "Region Gain" |
   | gain (typed) | `editor.edit(.audio(edit: .setGain(event:gainDb:)))` | "Set Region Gain" |
   | fade length (drag) | `RegionHandles.fades`, then `.setFades` | "Fade In" / "Fade Out" |
   | fade length, shape (typed, picked) | `.setFades(event:fadeIn:fadeOut:)`, keeping the other fade | "Set Region Fades" |
   | trim start or end (drag) | `.trim(event:frontFrames:backFrames:)` | "Trim Start" / "Trim End" |
   | start, length (typed) | `.trim`: Start keeps the end fixed (`front = new − offset`); Length keeps the start fixed (`back = old length − new`) | "Trim Region" |
   | normalize | `RegionCommands.normalize(peakDb:_:)`, then `Session::normalize_regions` | "Normalize" |

   - `AudioEdit::Trim` reaches the core today but no UI sends it. Its front trim moves the region's tick so the audio stays in place in time (`AudioCmd::Trim`), and it clamps the fades (`clamp_fades`). Task 1 adds its FFI hostile test.
   - Why: spec §3 ("uses only operations the model already has") and §6 item 2 ("using the existing model, commands and undo").
   - Cost if wrong: none expected.
6. **Ruling (handles).** All of them are in `AudioEditorGeometry`, in the waveform rect `wave` (the canvas minus the ruler and the level-scale gutter).
   - **Gain:** a band `gainBand` = 6 points high along the top of the region's rect, between the trim edges, with 4 points of vertical slack below it (`gainSlack`); it comes after the corners and edges in hit order. A vertical drag uses `RegionHandles.gain` (0.25 dB per point, ⇧ 0.05, clamped −144…+24, in tenths). The drag readout shows `AudioEditorEntry.gainText` (U+2212 minus, as the boxes), drawn with `RegionDrawing.readout`. ⌥-click gives `RegionHandles.resetGain`.
   - **Fades:** corner squares `cornerSize` = 10 at the fade-in end and the fade-out start, along the top, with 4 points of slack. A horizontal drag uses `RegionHandles.fades(_:_:dx:in:)` on the region's rect. They are hidden below `RegionHandles.minWidth` (24 points), as in the arrangement; the typed boxes still set them. A right-click in a fade's area opens `RegionDrawing.fadeShapeMenu`.
   - **Trim edges:** the region's left (`.start`) and right (`.end`) edges, the full wave height. The hit is `min(edgeWidth, width / 2)` inside (`edgeWidth` = 8) plus `slack` = 4 outside. Targets:
     - start: `front = round(dx × fpp)` clamped to `[−min(source_offset, startRoom), frames − 1]`. `front` is also capped so the region's start stays at or after tick 0 (Ruling 13.8): `front ≥ −startRoom`, where `startRoom = floor(secondsAt(seq, [tick]) × rate)` frames, floored at 0 (the core moves the tick by `front / rate` seconds, `command/audio.rs`). The model is unchanged: it still allows a negative tick;
     - end: `back = −round(dx × fpp)` clamped to `[min(0, −(mediaFrames − source_offset − frames)), frames − 1]`.

     A non-finite `dx` or a zero change gives nil.
   - **Mapping to the arrangement's handle type:** `AudioEditorHandle.regionHandle: RegionHandle?` gives `.gain → .gain`, `.fadeIn → .fadeIn`, `.fadeOut → .fadeOut`, and nil for `.start` and `.end`. `RegionHandles.fadeCommand`, `fadeArea` and `RegionDrawing.fadeShapeMenu` are called through it.
   - **Hit order:** fade corners, then trim edges, then the gain band. The region's body grabs nothing: moving a region stays in the arrangement.
   - Every drag is one gesture (`beginGesture(label, lanes: [lane], origin: .other)` after a 3-point dead zone, `updateGesture` with the full target computed from the row **at the press**, then `endGesture`). Escape cancels it (`cancelGesture`). Labels come from `AudioEditorGeometry.label(_:)`: "Region Gain", "Fade In", "Fade Out", "Trim Start", "Trim End".
   - Why: spec §6 item 2 ("each with handles"), the arrangement's handle conventions so users learn them once, and §4's "controls too small to use".
   - Cost if wrong: constants in one enum.
7. **Ruling (typed values).** `AudioEditorBox` lists the boxes.

   | Box (`rawValue`) | Accessibility label | Shows | Bare number | Clamp |
   |---|---|---|---|---|
   | `gain` | "Region Gain" | `gainText`: "−4.2 dB", "+6.0 dB", "0.0 dB", "−∞ dB" | dB | −144…+24 (`-inf` gives −144) |
   | `fade_in` | "Fade In Length" | `fadeText`: whole ms, "250 ms", "1500 ms" | ms | 0…`frames − fadeOut` |
   | `fade_out` | "Fade Out Length" | as above | ms | 0…`frames − fadeIn` |
   | `start` | "Region Start" | `Formatting.time(offset / rate)` | s | `max(0, offset − startRoom)`…`offset + frames − 1` (Ruling 13.8) |
   | `length` | "Region Length" | `Formatting.time(frames / rate)` | s | 1…`mediaFrames − offset` |
   | `peak` | "Normalize Peak" | `peakText`: "−0.3 dB", "0.0 dB" | dB | −96…0 (`-inf` refused) |

   - **Gain and peak grammar:** 7b's level grammar, via the new `StripEntry.level(_ text: String, max: Double) -> Double?`. Today's `level(_:)` becomes `level(text, max: MixerMath.maxDb)`, with the same results.
   - **Time grammar (`AudioEditorEntry.duration(_:bare:)`):**
     - forms: `N`, `N ms`, `N s` and `M:SS` or `M:SS.fff`;
     - `N` is 7b's strict ASCII number (digits, optional `.digits`), with no sign, exponent, `nan` or `inf`;
     - the units are case-insensitive, with at most one space before them;
     - `SS` is exactly two digits below 60, optionally followed by `.` and 1–3 digits, and `M` is 1–4 digits;
     - at most 32 Unicode scalars, and no control character.
   - Seconds become frames by `(seconds × rate).rounded()`, at the **media's** sample rate.
   - The fade shape is a menu `Picker` beside each fade box ("Fade In Shape", "Fade Out Shape") listing `RegionHandles.shapes`. A shape change keeps the length.
   - A commit is one `editor.edit(cmd)`, so one undo step. A result equal to the current value sends nothing. A refusal returns false, sets the box's hint as `editor.status` and sends nothing.
   - Without a media row (`editedRegion.media == nil`) the four time boxes (fade in, fade out, start, length) need a rate they do not have: they show "–" and refuse with `AudioEditorEntry.noMediaHint`.
   - With no region shown, every box refuses and sets `editor.status = AudioEditorModel.emptyMessage`.
   - Hints, exact (U+2212 minus):
     - gain: "Type a gain in dB, like −4.2 or +6."
     - fade: "Type a fade like 250 ms or 1.5 s."
     - time: "Type a time like 0:01.250, 1.25 s or 1250 ms."
     - peak: "Type a peak from −96 to 0 dB, like −0.3."
     - no media: "This region's audio file is unknown."
   - The boxes are `SymbioticControls.NumberBox`: Return commits; Escape or clicking away cancels (0.2.1).
   - Why: spec §6 item 2 ("typed values"), spec §2 item 5 ("hostile-input tested, like every other entry"), and 7b Ruling 15's grammar and clamp rule.
   - Cost if wrong: units are one table.
8. **Ruling (the level scale and waveform height).**
   - Each channel has its own band (`wave.height / channels`, stacked as `WaveformGeometry.bars` stacks them), with the centre line at −∞.
   - Waveform height `amplitude` ∈ {1, 2, 4, 8, 16}. The "Taller Waveform" and "Shorter Waveform" buttons double or halve it.
   - **Marks**, per band, top half only:
     - candidates `0, −3, −6, −9, −12, −18, −24, −30, −36, −48, −60` dB, at the linear position `10^(dB / 20) × amplitude` (1 = the band's top);
     - a candidate is shown only if its position ≤ 1, it is at least `minMarkGap` = 12 points below the last kept mark, and it is at least 12 points above the centre;
     - "−∞" is always shown at the centre.
     - Labels use U+2212 ("0", "−3", "−∞").
   - **Drawing:**
     - the waveform is scaled by `amplitude`, and the region's part also by the envelope. `WaveformGeometry.bars` clips at the band;
     - the envelope line is drawn at `envelope × amplitude` (clipped to the band);
     - columns where `|sample| × envelope > 1` (over 0 dBFS after the gain, whatever the zoom) get a clip mark along the band's top in `audioClip`.
   - Why: spec §6 item 2 ("a level scale"), and the owner's ask that normalize and gain read on the waveform.
   - Cost if wrong: one marks list.
9. **Ruling (normalize as gain): the Peak box and a Normalize button, never a panel.**
   - The Peak box holds `AudioEditorModel.peakDb` (in memory, default `RegionCommands.defaultPeakDb` = −0.3).
   - The button calls `RegionCommands.normalize(peakDb:_:)` on the selection, which is the one region. Its Busy ("still being built") and offline refusals go to the status line, as today.
   - Why: spec §6 item 2 ("normalize as gain") and the rule that programmatic calls never open dialogs.
   - Cost if wrong: none.
10. **Ruling (the control channel).**
    - `state.get {target: "audio_editor"}` answers `ControlResult::AudioEditor { doc, editor: AudioEditorStateRow }` in this JSON shape:
      ```json
      {"doc": "…", "shown": true, "message": null,
       "frames_per_point": 88.0, "origin_frame": 21600.0, "amplitude": 1,
       "boxes": {"gain": "−4.2 dB", "fade_in": "250 ms", "fade_out": "0 ms",
                 "start": "0:00.500", "length": "0:01.000", "peak": "−0.3 dB"},
       "region": {"event": 71, "media": 5, "name": "tone", "gain_db": -4.2,
                  "fade_in": {"frames": 12000, "shape": "linear"},
                  "fade_out": {"frames": 0, "shape": "linear"},
                  "source_offset": 24000, "length": 48000,
                  "media_frames": 96000, "sample_rate": 48000, "offline": false}}
      ```
      - The example is Task 4's fixture with a 600-point **wave rect** (the canvas less `scaleWidth`), which is the width `sync(width:)` always takes.
      - `gain_db` is rounded to thousandths, `(f64::from(g) * 1000.0).round() / 1000.0`, as `control.rs` already rounds its f32 fields, so it reads `-4.2`, not `-4.199999809265137`.
      - `boxes` is a JSON object whose keys are the `AudioEditorBox` raw values. Key order is not part of the contract: `serde_json` here sorts keys, and the tests compare `Value`s.
      - `shown` means the current main view's bottom pane is shown on `.audioEditor`.
      - With no region, `region` is null, `message` is `AudioEditorModel.emptyMessage` and `boxes` is `{}`.
      - With a region whose media row is unknown, `media` is the event's media ID (or 0), `media_frames` and `sample_rate` are 0, and `offline` is true.
      - Shape names are `linear`, `equal_power`, `exponential` and `s_curve`.
    - `ui.pane`'s `view` Choice gains `"audio_editor"`. `ViewCommands.runPane` already refuses it on the left or right pane, with "audio_editor is not on the left pane's list in the arrange view.".
    - The registry gains `editors.audio_editor` (⌘5), so the count goes 154 → 155.
    - There is no other new `ui.*` command. The zoom and height buttons and the boxes are driven through `ui.click`, `ui.key` and `ui.drag` by their accessibility labels (Ruling 7). The labels the channel finds:
      - the pane's root, an accessibility container labelled and identified "Audio Editor Pane";
      - the canvas, "Audio Editor";
      - the canvas's handle items "Start Handle", "End Handle", "Gain Handle", "Fade In Handle" and "Fade Out Handle";
      - the bar's buttons "Normalize", "Zoom In", "Zoom Out", "Fit Region", "Taller Waveform" and "Shorter Waveform".
    - Why: spec §4 ("Window and pane state the channel can read or set … is updated with the layout") and §7's row 7c ("control-channel state").
    - Cost if wrong: names in one table.
11. **Ruling (what the editor does not have).** Owner rules restated, so a reviewer can reject a task that adds them:
    - no pan control or pan state anywhere in the editor;
    - no command that writes a media file: no reverse, silence, cut, gain baked in, or sample-rate or format conversion;
    - no playhead, audition or scrub in 7c (Ruling 13.1, with a backlog row).

    The e2e finds the "Audio Editor Pane" container and asserts that no node in its subtree has a label matching the whole word `\bPan\b` (so "Bottom Pane" outside it, and "Pane" inside it, do not count).
12. **Ruling (theme roles).** New `EditorTheme` roles in `EditorTheme.swift`, the bridge file:

    | Role | Token |
    |---|---|
    | `audioWave` | `graph.line` |
    | `audioWaveTrimmed` | `text.disabled` |
    | `audioEnvelope` | `graph.handle` |
    | `audioHandle` | `graph.handle` |
    | `audioHandleSelected` | `graph.handleSelected` |
    | `audioClip` | `meter.clip` |
    | `audioScaleText` | `text.secondary` |
    | `audioScaleLine` | `graph.grid` |
    | `audioOutside` (beyond the file) | `surface.panel` |

    - The region's body uses `regionFill(tint:)`, with the lane's strip colour as in the arrangement. Offline hatching and the "Offline" pill reuse `RegionDrawing`.
    - The scale's and ruler's labels use `theme.digitsFont(theme.type.caption)`.
    - Why: 7b Ruling 13 and spec §4 ("hard-coded colours").
    - Cost if wrong: one table.

13. **Ruling (controller, provisional; owner may override): the owner questions the first draft and the pre-flight raised.** Each is binding for the implementers until the owner says otherwise.
    1. **(controller, provisional; owner may override)** No playhead, audition or scrub in 7c. A backlog row records them (Task 7).
    2. **(controller, provisional; owner may override)** The editor's zoom is kept in memory only and is not saved with the project (Ruling 3). A backlog row records it.
    3. **(controller, provisional; owner may override)** The audio editor is on the Mix view's bottom list too (Ruling 1).
    4. **(controller, provisional; owner may override)** Fades are typed and shown in ms ("250 ms", "1500 ms"); start and length in the file's time, `m:ss.fff` (Ruling 7).
    5. **(controller, provisional; owner may override)** The shortcut is ⌘5 (`editors.audio_editor`), with no ⌥⌘5 (Ruling 1).
    6. **(controller, provisional; owner may override)** The gain handle is a band along the top of the region, as in the arrangement, not a line on the waveform (Ruling 6).
    7. **(controller, provisional; owner may override)** A typed fade that is too long clamps to what the other fade leaves; the other fade is never changed silently (Ruling 7). Only a trim shorter than both fades shortens them, as the model's `clamp_fades` already does.
    8. **(controller, provisional; owner may override)** The editor's Start extension (drag or typed) clamps at the sequence start: the region's start stays at or after tick 0. The room is `floor(secondsAt(seq, [tick]) × rate)` frames (`AudioEditorModel.startRoom`, Rulings 6 and 7). The model is unchanged and still allows a negative tick from other callers; the arrangement is unchanged.

## Spec checks (for the owner, no plan change needed)

- §3 says the audio editor "uses only operations the model already has". Ruling 4's export adds no operation; it reads what the engine already computes.
- Pre-flight M17 (extending a bar-1 region's start would give it a negative tick, which the model allows) is ruled in Ruling 13.8: the editor clamps at the sequence start.
- §6 item 2 says "no format change". The project format is unchanged, and `ui.json`'s schema is too. A saved bottom pane may now name `"audio_editor"`, which a 7b build reads as its default view (README step 223 checks it).

## File map

| File | Task | What |
|---|---|---|
| `crates/core-ffi/src/overviews.rs`, `session.rs` | 1 | `envelope`, `Session::region_envelope` |
| `crates/core-ffi/tests/overviews.rs`, `regions.rs` | 1 | envelope and trim tests |
| `app/Tests/VizDAWAppModelTests/FakeSession.swift` | 1 | `regionEnvelope` |
| `app/Sources/VizDAWAppModel/AudioEditorEntry.swift`, `StripEntry.swift` | 2 | grammar |
| `app/Sources/VizDAWAppModel/FrameAxis.swift` | 3 | time axis |
| `app/Sources/VizDAWEditors/AudioEditorGeometry.swift`, `EditorTheme.swift` | 3 | geometry, roles |
| `app/Sources/VizDAWAppModel/AudioEditorModel.swift`, `PaneContent.swift`, `WindowLayout.swift`, `DocumentModel.swift` | 4 | model, `PaneView.audioEditor` |
| `app/Sources/VizDAW/DocumentShell.swift` | 4, 6 | the pane host case |
| `crates/core-control/src/{request.rs,registry.rs}`, `crates/core-ffi/src/control.rs` | 5 | state target, rows, ⌘5 |
| `app/Sources/VizDAWAppModel/{ControlCommands.swift,ViewCommands.swift}`, `app/Sources/VizDAW/AppCommands.swift` | 5 | state, handler, menu item |
| `app/Sources/VizDAWEditors/AudioEditorCanvas.swift`, `app/Sources/VizDAW/AudioEditorPane.swift` | 6 | views |
| `crates/vizctl/tests/e2e.rs`, `app/README.md`, `README.md`, `docs/feature-backlog.md` | 7 | e2e, docs |

---

### Task 1: The envelope export and the trim's hostile test

**Files:**
- Modify: `crates/core-ffi/src/overviews.rs` (`pub fn envelope`), `crates/core-ffi/src/session.rs` (`region_envelope`, in the "Waveform overviews" `#[uniffi::export] impl Session` block after `region_waveform`), `app/Tests/VizDAWAppModelTests/FakeSession.swift`
- Test: `crates/core-ffi/tests/overviews.rs`, `crates/core-ffi/tests/regions.rs`

**Interfaces:**
- Consumes: `overviews::fade_gain`, `overviews::check_columns`, `core_model::mixer::db_to_gain`, `core_engine::schedule::fade_curve` (tests), and the overview tests' `import`, `settle` and `last_region` helpers (`regions.rs`'s `demo_with` and `region`).
- Produces:
  - `pub struct RegionShape` and `pub fn envelope(region: &RegionShape, from: u64, to: u64, columns: u32) -> Vec<f32>` (Ruling 4);
  - `Session::region_envelope(&self, event: u64, from_frame: u64, to_frame: u64, columns: u32) -> Result<Vec<f32>, VizError>`;
  - Swift `SessionProtocol.regionEnvelope(event:fromFrame:toFrame:columns:) throws -> [Float]`;
  - `FakeSession.envelope: [Float]?` (nil answers `columns` ones) and `FakeSession.envelopeCalls`.

- [ ] **Step 1: Write the failing tests.** Follow each file's convention: `overviews.rs` and `regions.rs` sessions open no device and use no `serial()` (copy `transport.rs:19–27` only if one is wanted).
  - `overviews.rs`, `region_envelope_matches_the_heard_gain_and_fades`:
    - import a mono ramp of 48 000 frames (its own samples through `caf(…, 1, …)`: the file's `ramp()` is stereo), into a session made by `Session::new_demo` so a note exists for the hostile test (or use `regions.rs`'s `a_note`);
    - through `Session::edit`, trim the region to `source_offset` 1000 and length 10 000, set its gain to −6.0 dB, its fade in to linear 2000 frames and its fade out to S-curve 2000 frames;
    - call `region_envelope(event, 0, 12_000, 12)`. With `g = core_model::mixer::db_to_gain(-6.0) as f32`, within 1e-6:
      - `v.len() == 12`;
      - `v[0] == 0` and `v[11] == 0` (centres 500 and 11 500 lie outside);
      - `v[1] == g × fade_curve(Linear, 0.25)` (centre 1500, r = 500);
      - `v[2] == g × fade_curve(Linear, 0.75)`;
      - `v[3]` through `v[8] == g`;
      - `v[9] == g × fade_curve(SCurve, 0.75)` (centre 9500, r = 8500, 1500 frames left);
      - `v[10] == g × fade_curve(SCurve, 0.25)` (centre 10 500, r = 9500, 500 frames left).
  - `region_envelope_is_zero_outside_and_answers_offline`:
    - with the region from above, `region_envelope(event, 20_000, 30_000, 4)` is four zeros;
    - make the media offline, as `a_same_length_replacement_goes_offline_changed_and_back` does: `region_envelope(event, 0, 12_000, 12)` gives the same values as before (no file is read);
    - `region_waveform` still answers `Offline`.
  - `hostile_region_envelope_calls_are_refused`:
    - each of these is `Err`, and the message is non-empty:
      - `event = u64::MAX`;
      - a note's ID (the message contains "is not an audio region");
      - `from == to`, and `from > to` (the message is "from_frame must be before to_frame");
      - `columns` 0 and 8193;
    - `region_envelope(event, 0, u64::MAX, 8192)` is `Ok`, with 8192 values, each finite and in `0.0..=16.0`.
  - `regions.rs`, `hostile_trims_are_refused_and_change_nothing`, on a region with offset 1000, length 10 000 and media 48 000 frames, through `Session::edit(EditCommand::Audio { edit: AudioEdit::Trim { … } })`:
    - each of these is `Err`, and afterwards the region's `tick`, `source_offset`, `frames`, `fade_in` and `fade_out` and `snapshot().undo_label` are unchanged:
      - front −1001 (before the file);
      - back −37 001 (past the file's end);
      - front 10 000, and front 6000 with back 4000 (length 0);
      - front `i64::MIN`, front `i64::MAX`, back `i64::MIN`, back `i64::MAX`;
      - `event: u64::MAX`, and a note's ID;
    - then front −1000 is `Ok`: offset 0, frames 11 000, the region's tick lower than before, and undo label "Trim Region".
- [ ] **Step 2: Run them and see them fail:** `cargo test -p core-ffi --test overviews region_envelope` and `cargo test -p core-ffi --test regions hostile_trims`. The envelope tests fail to compile (no `region_envelope`). The trim test is a **characterisation test**, named as such in its doc comment: it pins today's behaviour, so a pass at RED is acceptable and is reported (the Global TDD exception).
- [ ] **Step 3: Implement** `envelope` and `region_envelope` per Ruling 4. Read the region under the lock as `region_waveform` does, and compute the vector after the lock is released. Add `regionEnvelope` to `FakeSession`.
- [ ] **Step 4: Run the gates:** the Rust gates, the rt-check gates, `scripts/build-ffi.sh`, `scripts/swift.sh build`, `scripts/swift.sh test` and the no-brand build. Rust is baseline + 4 (macOS and Linux clippy clean); Swift is unchanged.
- [ ] **Step 5: Commit** `crates/core-ffi/src/overviews.rs`, `crates/core-ffi/src/session.rs`, `crates/core-ffi/tests/overviews.rs`, `crates/core-ffi/tests/regions.rs` and `app/Tests/VizDAWAppModelTests/FakeSession.swift`. Message: "core-ffi: region_envelope, the heard gain and fades for the audio editor; trim hostile test (plan 7c Task 1)", with the trailers.

---

### Task 2: The typed grammar

**Files:**
- Create: `app/Sources/VizDAWAppModel/AudioEditorEntry.swift`, `app/Tests/VizDAWAppModelTests/AudioEditorEntryTests.swift`
- Modify: `app/Sources/VizDAWAppModel/StripEntry.swift` (`level(_:max:)`, `levelText(_:max:)`)

**Interfaces:**
- Consumes: `StripEntry`'s private `clean` and `number` (made `static` internal for this module), `MixerMath.floorDb` (−144), and `Formatting.time`.
- Produces (all `public`):
  - `StripEntry.level(_ text: String, max: Double) -> Double?`, where `level(_:)` equals `level(_:max: MixerMath.maxDb)`, and `StripEntry.levelText(_ db: Double, max: Double) -> String` beside it (today's `levelText(_:)` clamps at `MixerMath.maxDb` = 12, so `gainText` uses `levelText(db, max: 24)`);
  - `enum AudioEditorEntry` with:
    - `enum Unit { case ms, s }`;
    - `static func gain(_ text: String) -> Double?` (`level(text, max: 24)`);
    - `static func peak(_ text: String) -> Double?`;
    - `static func duration(_ text: String, bare: Unit) -> Double?` (seconds);
    - `static func frames(_ seconds: Double, rate: UInt32) -> Int64`;
    - `static func gainText(_ db: Double) -> String`, `peakText(_ db: Double) -> String` and `fadeText(frames: Int64, rate: UInt32) -> String`;
    - `static let gainHint`, `fadeHint`, `timeHint`, `peakHint` and `noMediaHint`, with Ruling 7's exact strings.

- [ ] **Step 1: Write the failing tests** in `AudioEditorEntryTests.swift`.
  - `gainsAcceptDecibelsAndClamp`:
    - `"-4.2"` gives −4.2, `"+6 dB"` gives 6, `"30"` gives 24, and `"-200"`, `"-inf"` and `"−∞"` each give −144;
    - `"-0.04"` gives 0 (not −0);
    - `gainText(-144) == "−∞ dB"`, `gainText(24) == "+24.0 dB"`, `gainText(-4.2) == "−4.2 dB"` and `gainText(-0.0) == "0.0 dB"`.
  - `hostileGainsAreRefused`: each of these gives nil:
    - `""`, `"   "`, `"abc"`, `"4.2.1"`, `"--3"`;
    - `"nan"`, `"inf"`, `"1e3"`, `"1,5"`;
    - `"-4.2 dBFS"`, `"4\u{0}"`;
    - 33 × `"1"` and 300 × `"9"`.
  - `timesAcceptTheirForms`, with the result in seconds:
    - `duration("250", bare: .ms) == 0.25`;
    - `("250 ms", .s) == 0.25`;
    - `("1.5 s", .ms) == 1.5`;
    - `("1.5", .s) == 1.5`;
    - `("1.5S", .ms) == 1.5`;
    - `("1:02.5", .s) == 62.5`;
    - `("0:00.250", .ms) == 0.25`;
    - `(" 2 ms ", .ms) == 0.002`;
    - `("0", .ms) == 0`;
    - `(".5", .s) == 0.5`.

    And `frames(0.25, rate: 48_000) == 12_000`, `frames(0.0000104, rate: 48_000) == 0` and `frames(1, rate: 0) == 0`.
  - `hostileTimesAreRefused`: each of these gives nil, for both units:
    - `""`, `" "`, `"abc"`;
    - `"-5"`, `"+5"`, `"−5"`;
    - `"1.2.3"`, `"1e3"`, `"nan"`, `"inf"`;
    - `"5 sec"`, `"5 mss"`, `"5ms5"`, `"ms"`, `"5  ms"` (two spaces);
    - `"1:75"`, `"1:5"`, `":30"`, `"1:"`, `"1::00"`, `"99999:00"`;
    - `"0x10"`, `"２５０"`, `"٣"`;
    - `"5\nms"`, `"\u{7}5"`;
    - 33 × `"1"` and 300 × `"9"`.
  - `peaksClampAndRefuseMinusInfinity`:
    - `"-0.3"` gives −0.3, `"−1 dB"` gives −1, `"+3"` gives 0, and `"-200"` gives −96;
    - `"-inf"`, `"−∞"`, `"abc"` and `""` give nil;
    - `peakText(-0.3) == "−0.3 dB"` and `peakText(0) == "0.0 dB"`.
  - `theTextsReadBack`:
    - `fadeText(frames: 12_000, rate: 48_000) == "250 ms"`, `fadeText(frames: 72_000, rate: 48_000) == "1500 ms"`, `fadeText(frames: 0, rate: 48_000) == "0 ms"` and `fadeText(frames: 5, rate: 0) == "–"`;
    - `frames(duration(fadeText(frames: 12_000, rate: 48_000), bare: .ms)!, rate: 48_000) == 12_000`;
    - `duration(Formatting.time(1.25), bare: .s) == 1.25`;
    - `gain(gainText(-4.2)) == -4.2`.
  - `StripEntryTests` stays green unchanged: the refactor is pinned by its `levelsClampAndReadMinusInfinity` (`"40"` gives 12).
- [ ] **Step 2: Run them and see them fail:** `scripts/swift.sh test --filter AudioEditorEntryTests`.
- [ ] **Step 3: Implement** per Ruling 7.
  - `peak` refuses any text whose number part is `inf` or `∞` before clamping.
  - `fadeText` always shows whole ms (`.toNearestOrAwayFromZero`), as Ruling 13's "fades in ms" says: "250 ms", "1500 ms".
  - `frames` returns 0 for a non-finite product or `rate == 0`, and otherwise clamps to `0…Int64.max / 2`.
- [ ] **Step 4: Run the gates.** Swift is baseline + 6, and the no-brand build is clean.
- [ ] **Step 5: Commit** `AudioEditorEntry.swift`, `StripEntry.swift` and `AudioEditorEntryTests.swift`. Message: "app model: the audio editor's typed gain, fade, time and peak grammar (plan 7c Task 2)", with the trailers.

---

### Task 3: The time axis, the editor's geometry and its theme roles

**Files:**
- Create: `app/Sources/VizDAWAppModel/FrameAxis.swift`, `app/Sources/VizDAWEditors/AudioEditorGeometry.swift`, `app/Tests/VizDAWAppModelTests/FrameAxisTests.swift`, `app/Tests/VizDAWEditorsTests/AudioEditorGeometryTests.swift`
- Modify: `app/Sources/VizDAWEditors/EditorTheme.swift` (Ruling 12's roles), `app/Tests/VizDAWEditorsTests/ThemeScanTests.swift` (`theEditorRolesAreTheTablesTokens` loops `Theme.all` for the nine roles)

**Interfaces:**
- Consumes: `RegionHandles.fades`, `gain`, `gainCommand`, `size`, `minWidth`, `label`; `WaveformGeometry`; `OverviewRows`; `EventRow`; `Formatting.time`.
- Produces (all `public`):
  - `struct FrameAxis: Equatable, Sendable` (in `VizDAWAppModel`) with:
    - `framesPerPoint: Double` and `originFrame: Double` (`private(set)`), and `static let minFramesPerPoint: Double = 1`;
    - `init(framesPerPoint: Double = 512, originFrame: Double = 0)`;
    - `func x(_ frame: Double) -> CGFloat` and `func frame(atX x: CGFloat) -> Double`;
    - `static func maxFramesPerPoint(mediaFrames: UInt64, width: CGFloat) -> Double`, which is `max(1, 2 × max(mediaFrames, 1) / max(width, 1))`;
    - `static func fit(offset: UInt64, length: UInt64, width: CGFloat, mediaFrames: UInt64) -> FrameAxis`, with margin `max(length / 20, 1)`;
    - `func zoomed(by factor: Double, aroundX x: CGFloat, width: CGFloat, mediaFrames: UInt64) -> FrameAxis`: `framesPerPoint / factor` (a factor above 1 zooms in), clamped to `1…maxFramesPerPoint`, with the origin then clamped as `scrolled` clamps it; a factor that is not finite or ≤ 0 gives `self`;
    - `func scrolled(byPoints dx: CGFloat, width: CGFloat, mediaFrames: UInt64) -> FrameAxis`, with the origin clamped to `−w·fpp/2 … mediaFrames − w·fpp/2`.
  - `enum AudioEditorHandle: Equatable, Sendable { case gain, fadeIn, fadeOut, start, end }`, with `var regionHandle: RegionHandle?` (Ruling 6's mapping).
  - `struct LevelMark: Equatable { y: CGFloat; label: String }` and `struct RulerMark: Equatable { x: CGFloat; label: String }`.
  - `enum AudioEditorGeometry` with:
    - `static let scaleWidth: CGFloat = 44`, `rulerHeight: CGFloat = 18`, `gainBand: CGFloat = 6`, `gainSlack: CGFloat = 4`, `cornerSize: CGFloat = 10`, `edgeWidth: CGFloat = 8`, `slack: CGFloat = 4`, `minMarkGap: CGFloat = 12`, `amplitudes: [Int] = [1, 2, 4, 8, 16]` and `maxColumns: UInt32 = 8192`;
    - `static func waveRect(in bounds: CGRect) -> CGRect` (minus the ruler at the top and the gutter at the left);
    - `static func regionRect(_ r: EventRow, axis: FrameAxis, in wave: CGRect) -> CGRect`;
    - `static func handles(_ r: EventRow, axis: FrameAxis, in wave: CGRect) -> [(AudioEditorHandle, CGRect)]`, in hit order;
    - `static func hit(_ p: CGPoint, row: EventRow, axis: FrameAxis, in wave: CGRect) -> AudioEditorHandle?`;
    - `static func label(_ h: AudioEditorHandle) -> String`;
    - `static func trimCommand(_ r: EventRow, _ h: AudioEditorHandle, dx: CGFloat, framesPerPoint: Double, mediaFrames: UInt64, startRoom: UInt64) -> EditCommand?`;
    - `static func target(_ r: EventRow, _ h: AudioEditorHandle, dx: CGFloat, dy: CGFloat, fine: Bool, axis: FrameAxis, mediaFrames: UInt64, startRoom: UInt64, in wave: CGRect) -> EditCommand?`;
    - `static func scaleMarks(amplitude: Int, channels: Int, in wave: CGRect) -> [LevelMark]`;
    - `static func rulerMarks(axis: FrameAxis, width: CGFloat, sampleRate: UInt32) -> [RulerMark]`, with steps `[0.001, 0.002, 0.005, 0.01, 0.02, 0.05, 0.1, 0.2, 0.5, 1, 2, 5, 10, 15, 30, 60, 120, 300, 600]` s, the smallest at least 80 points apart, labelled with `Formatting.time`;
    - `static func columns(width: CGFloat) -> UInt32`;
    - `struct FrameSpan: Equatable { from: UInt64; to: UInt64; x: CGFloat; width: CGFloat; columns: UInt32 }` and `static func span(axis: FrameAxis, width: CGFloat, mediaFrames: UInt64) -> FrameSpan?`: the in-file part of the visible frames, clamped to `[0, mediaFrames]`, with its x offset and width in the wave rect and `columns(width:)` of that width. Nil when the part is empty or any input is not finite. It never converts a negative, NaN or huge `Double` to `UInt64` (Swift traps on those). The canvas asks **both** `overview` and `regionEnvelope` for exactly this span and column count, so their columns line up (`overview` spreads its columns over the range cut to the file);
    - `static func shaped(_ rows: OverviewRows, envelope: [Float]?, amplitude: Int) -> OverviewRows`, where a nil envelope scales by `amplitude` only, and an envelope whose count is not `rows.columns` gives silent rows of the same shape;
    - `static func clipColumns(_ rows: OverviewRows, envelope: [Float]) -> [Int]`;
    - `static func envelopeLine(_ envelope: [Float], amplitude: Int, channel: Int, channels: Int, in wave: CGRect) -> [CGPoint]`.
  - `EditorTheme` roles: Ruling 12's nine names, as `NSColor` properties on `Theme`.

- [ ] **Step 1: Write the failing tests.**
  - `FrameAxisTests.swift`, with `mediaFrames` 96 000 and width 600:
    - `zoomStaysInRange`:
      - zooming in by 1000 ten times gives `framesPerPoint == 1`;
      - zooming out by 1000 ten times gives `framesPerPoint == maxFramesPerPoint(mediaFrames: 96_000, width: 600) == 320`;
      - factors `.nan`, 0, −2 and `.infinity` return the axis unchanged.
    - `fitShowsTheRegionWithMargins`:
      - `fit(offset: 24_000, length: 48_000, width: 600, mediaFrames: 96_000)` has `framesPerPoint == 88` and `originFrame == 21_600`, with `x(24_000) ≈ 27.27` and `x(72_000) ≈ 572.73` (±0.01);
      - `fit(offset: 10, length: 1, …)` has `framesPerPoint == 1` and `originFrame == 9`.
    - `zoomKeepsTheFrameUnderThePointer`: from `fit(offset: 24_000, length: 48_000, width: 600, mediaFrames: 96_000)`, `frame(atX: 200)` is equal (±1e-9) before and after `zoomed(by: 2, aroundX: 200, width: 600, mediaFrames: 96_000)`, and the result's `framesPerPoint == 44`.
    - `scrollStopsAtTheFileEdges`: from the fit above, `scrolled(byPoints: -1e9, …)` has origin `−300 × 88`, and `scrolled(byPoints: 1e9, …)` has origin `96_000 − 300 × 88`.
  - `AudioEditorGeometryTests.swift`, with an `EventRow` at offset 24 000, 48 000 frames and media 96 000:
    - `trimDragsClampToTheFileAndTheRegion`, at `framesPerPoint: 100` and `startRoom: 100_000` unless a case says otherwise:
      - start dx −1000 gives `.trim(frontFrames: -24_000, backFrames: 0)`; start dx +1000 gives front 47 999;
      - the sequence start (Ruling 13.8): start dx −1000 with `startRoom: 10_000` gives front −10 000; with `startRoom: 0`, start dx −1000 gives nil and start dx +10 still gives front 1000;
      - end dx +1000 gives back −24 000; end dx −1000 gives back 47 999;
      - start dx 0.004, dx `.nan` and dx `.infinity` give nil;
      - with `mediaFrames: 50_000` (shorter than the region's end at 72 000), end dx +10 gives nil and end dx −10 gives back 1000;
      - `label(.start) == "Trim Start"` and `label(.end) == "Trim End"`.
    - `fadeDragsStopAtTheOtherFade`: fades 12 000 and 12 000, on an axis with `framesPerPoint` 100:
      - fadeIn dx +500 gives `.setFades(fadeIn: 36_000, fadeOut: 12_000)`, with the shapes kept;
      - fadeOut dx −500 gives `fadeOut: 36_000`;
      - fadeIn dx −1000 gives `fadeIn: 0`.
    - `gainDragFollowsTheArrangementsRate`: from 0 dB:
      - dy −20 gives `.setGain(gainDb: 5.0)`;
      - with `fine`, dy −20 gives 1.0;
      - dy 10 000 gives −144;
      - dx alone gives nil.
    - `handlesOfATinyRegionStayReachable`:
      - a region 6 points wide: a press at its left third, mid-height, hits `.start`; at its right third, `.end`; 3 points outside its left edge, `.start`; there is no fade handle;
      - a region 400 points wide: a press on the fade-in corner hits `.fadeIn` (not `.gain`); on the top band between the corners, `.gain`; at the left edge mid-height, `.start`; in the lower middle, nil.
    - `scaleMarksNeverOverlap`:
      - mono, 40 points, amplitude 1: the labels are `["0", "−∞"]`;
      - mono, 600 points, amplitude 1: the labels are `["0", "−3", "−6", "−9", "−12", "−18", "−24", "−∞"]`;
      - at amplitude 2 no mark is labelled "0";
      - for every amplitude in `amplitudes`, height 40, 120 or 600, and channels 1 or 2, consecutive marks within a band are at least 12 points apart, and every mark lies inside its band.
    - `rulerMarksAreEvenAndLabelled`:
      - at rate 48 000, `framesPerPoint` 100, origin 0 and width 600: seven marks, at x = 0, 96, …, 576, labelled `"0:00.000"`, `"0:00.200"` … `"0:01.200"`;
      - rate 0 gives `[]`.
    - `waveformScalesByEnvelopeAndAmplitude`:
      - mono, four columns, min `[-0.5, -0.25, 0, -1]` and max `[0.5, 0.25, 0, 1]`, envelope `[0, 0.5, 1, 2]` and amplitude 2: `shaped` gives max `[0, 0.25, 0, 4]` and min `[0, -0.25, 0, -4]`, and `clipColumns == [3]`;
      - an envelope of three values gives all-zero rows of four columns;
      - stereo rows are scaled per column in both channels.
    - `columnsNeverExceedTheCoreLimit`: `columns(width:)` of 0, −5 and `.nan` is 1; of 600.2 is 601; of 1e9 is 8192.
    - `spanStaysInsideTheFile`, media 96 000, width 600:
      - (axes built with `FrameAxis(framesPerPoint:originFrame:)`, whose `init` stores its values as given; only the operations clamp) origin −26 400 at `framesPerPoint` 88: `from == 0`, `x == 300`, `width == 300`, `to == 26_400`;
      - origin 90 000 at 88: `from == 90_000`, `to == 96_000`, `x == 0`, `width ≈ 68.18`;
      - origin 100 000 (wholly past the end): nil;
      - `mediaFrames: 0`: nil;
      - an axis whose origin is not finite (built through `scrolled(byPoints: .nan, …)` or `FrameAxis(framesPerPoint: .nan)`): nil, never a trap.
  - `theEditorRolesAreTheTablesTokens` checks only `Theme.studioDark` today: extend it to loop `Theme.all` for Ruling 12's nine new roles (the existing roles keep their check).
- [ ] **Step 2: Run them and see them fail:** `scripts/swift.sh test --filter 'FrameAxisTests|AudioEditorGeometryTests|ThemeScanTests'` (`spanStaysInsideTheFile` among them).
- [ ] **Step 3: Implement** per Rulings 3, 6, 8 and 12.
  - The trim clamp is Ruling 6's.
  - A tiny region's edge hit is `min(edgeWidth, width / 2)`.
  - Fade corners appear only when the region's rect is at least `RegionHandles.minWidth` wide.
  - `target` dispatches by handle: gain to `RegionHandles.gainCommand`; the fades to `RegionHandles.fadeCommand` on `regionRect`; start and end to `trimCommand` with `axis.framesPerPoint`.
- [ ] **Step 4: Run the gates.** Swift is baseline + 19, and the no-brand build is clean.
- [ ] **Step 5: Commit** the four new files, `EditorTheme.swift` and `ThemeScanTests.swift`. Message: "editors: the audio editor's frame axis, handles, level scale, ruler and theme roles (plan 7c Task 3)", with the trailers.

---

### Task 4: The editor's model and its place in the bottom pane

**Files:**
- Create: `app/Sources/VizDAWAppModel/AudioEditorModel.swift`, `app/Tests/VizDAWAppModelTests/AudioEditorModelTests.swift`
- Modify:
  - `app/Sources/VizDAWAppModel/WindowLayout.swift` (`PaneView.audioEditor`, `views(.bottom, in:)`);
  - `PaneContent.swift` (`EditedRegion`, `editedRegion`, with `inspectorRegion` as `editedRegion?.row`);
  - `DocumentModel.swift` (`public var audioEditor: AudioEditorModel`, made once on first use from an `@ObservationIgnored private var audioEditorModel`, as `controllersChart` is);
  - `app/Sources/VizDAW/DocumentShell.swift` (`PaneHost`: `case .audioEditor` shows `ContentUnavailableView("Audio Editor", icon: .audio, description: Text(AudioEditorModel.emptyMessage))` until Task 6);
  - `app/Tests/VizDAWAppModelTests/WindowLayoutTests.swift`, and every existing test that pins the bottom lists (`ViewCommandsTests`' `views == ["piano_roll", "list", "strip_chart", "drum_grid"]` becomes `[…, "drum_grid", "audio_editor"]` for Mix, with `"audio_editor"` before `"mixer"` for Arrange).

**Interfaces:**
- Consumes:
  - Task 2's `AudioEditorEntry`;
  - Task 3's `FrameAxis`;
  - `EditorModel.edit(_:)`, `status` and `selection`;
  - `RegionCommands.normalize(peakDb:_:)` and `defaultPeakDb`;
  - `MediaModel.row(_:)`;
  - `LaneKey`.
- Produces (all `public`):
  - `PaneView.audioEditor = "audio_editor"`, titled "Audio Editor";
  - `struct EditedRegion: Equatable { let row: EventRow; let lane: LaneKey; let media: MediaRow? }` and `DocumentModel.editedRegion: EditedRegion?` (Ruling 2);
  - `enum AudioEditorBox: String, CaseIterable, Sendable { case gain, fadeIn = "fade_in", fadeOut = "fade_out", start, length, peak }`, with `var label: String` and `var hint: String` (Ruling 7's table);
  - `@MainActor @Observable final class AudioEditorModel` with:
    - `init(document: DocumentModel)`, keeping it `unowned`;
    - `static let emptyMessage = "Select one audio region to edit it."`;
    - `private(set) var axis: FrameAxis`, `amplitude: Int` (1), `shownRegion: UInt64?`, `width: CGFloat` and `peakDb: Double` (−0.3);
    - `var startRoom: UInt64`: the shown region's room before tick 0, `floor(secondsAt(seq, [row.tick]) × rate)` frames through `document.editor.session.secondsAt(seq:ticks:)`, floored at 0, and 0 when the call throws or there is no media row (Ruling 13.8). The canvas passes it to `AudioEditorGeometry.target`, and `type(.start, …)` clamps with it;
    - `func sync(width: CGFloat)` (assigning each property only when its value changed), where `width` is always the **wave rect's** width (`AudioEditorGeometry.waveRect(in: bounds).width`). It stores `width`, reads `document.editedRegion`, and fits the axis when the region's ID differs from `shownRegion`;
    - `func zoomIn()`, `zoomOut()` (factor 2 about the centre), `fit()`, `zoom(by: Double, aroundX: CGFloat)`, `scroll(byPoints: CGFloat)`, `taller()` and `shorter()`;
    - `func text(_ box: AudioEditorBox) -> String`;
    - `@discardableResult func type(_ box: AudioEditorBox, _ text: String) -> Bool`;
    - `func setShape(_ box: AudioEditorBox, _ shape: FadeShapeRow)`, for `.fadeIn` and `.fadeOut` only;
    - `@discardableResult func normalize() -> KeyOutcome`.

- [ ] **Step 1: Write the failing tests.**
  - `AudioEditorModelTests.swift` uses a `FakeSession` with audio region 71 on lane `(3, 30)`: media 5 ("tone", 48 kHz, 96 000 frames, stereo), offset 24 000, 48 000 frames, gain 0 and fades 0 linear, at tick 48 000 (the fake's `secondsAt` is tick / 30 720, so 1.5625 s and a `startRoom` of 75 000 frames, more than its offset); a second region 72 on the same lane at tick 0, offset 0, 24 000 frames; and a third region 73 at tick 0 (bar 1), offset 24 000, 24 000 frames. The IDs avoid `editorFake()`'s reference block 7 on lane `(3, 30)`; build the fake without that block or keep these IDs. `FakeSession.regionRow` has no `sourceOffset` parameter: set it after.
    - `theEditorFollowsTheSelectedRegion`:
      - with event 71 selected: `editedRegion?.row.id == 71`, `.lane == LaneKey(3, 30)` and `.media?.id == 5`;
      - `sync(width: 600)` sets `axis == FrameAxis.fit(offset: 24_000, length: 48_000, width: 600, mediaFrames: 96_000)` and `shownRegion == 71`;
      - `zoomIn()` then `sync(width: 600)` keeps the zoomed axis;
      - selecting a note, or two regions, gives `editedRegion == nil`, and `sync` sets `shownRegion = nil`;
      - selecting region 71 again fits again;
      - `document.inspectorRegion?.id == 71` still holds.
    - `aVanishedRegionShowsTheMessage`: after the fake's snapshot drops event 71 and the model refreshes, `editedRegion == nil`, and `type(.gain, "-3")` is false, records no edit and leaves `editor.status == AudioEditorModel.emptyMessage`.
    - `typedGainIsOneUndoStepAndARefusalSendsNothing`:
      - `type(.gain, "-4.2")` is true and records exactly one edit, `.audio(edit: .setGain(event: 71, gainDb: -4.2))`;
      - `type(.gain, "abc")` is false, records nothing new, and sets `editor.status == AudioEditorEntry.gainHint`;
      - `type(.gain, "0")` while the row's gain is 0 records nothing.
    - `typedFadesClampToWhatTheOtherLeaves`, with the row's fade out at 12 000:
      - `type(.fadeIn, "10 s")` records `.setFades(event: 71, fadeIn: FadeRow(length: 36_000, shape: .linear), fadeOut: FadeRow(length: 12_000, shape: .linear))`;
      - `type(.fadeOut, "250")` records the fade out at 12 000, so nothing (an equal value);
      - `type(.fadeIn, "-5")` is false with `fadeHint`.
    - `typedStartAndLengthClamp`, from offset 24 000 and 48 000 frames:
      - `type(.start, "0.25")` records `.trim(event: 71, frontFrames: -12_000, backFrames: 0)`;
      - `"0"` records front −24 000;
      - `"10"` records front 47 999;
      - `type(.length, "1.25")` records back −12 000;
      - `"10"` records back −24 000;
      - `"0"` records back 47 999;
      - with `media` nil, `type(.start, "1")` is false with `noMediaHint`, and `text(.start) == "–"`;
      - the sequence start (Ruling 13.8): with region 73 (tick 0) selected, `startRoom == 0`, and `type(.start, "0")` and `type(.start, "0.25")` each record nothing (both clamp to its own offset); region 71's `startRoom == 75_000`.
    - `shapesKeepTheirLength`, with the fade in at 12 000 linear:
      - `setShape(.fadeIn, .sCurve)` records `.setFades(event: 71, fadeIn: FadeRow(length: 12_000, shape: .sCurve), fadeOut: <unchanged>)`;
      - `setShape(.fadeIn, .linear)` records nothing.
    - `normalizeUsesTheTypedPeakAndNeverADialog`:
      - `type(.peak, "-1")` is true, records no edit, and `text(.peak) == "−1.0 dB"`;
      - `normalize()` calls the fake's `normalizeRegions(events: [71], peakDb: -1)`;
      - with the fake throwing `.Busy("The waveform of tone is still being built; try again in a moment.")`, the status is that message and `document.alert == nil`.
    - `theZoomButtonsStayInRange`, after `sync(width: 600)`:
      - 20 × `zoomIn()` gives `axis.framesPerPoint == 1`;
      - 20 × `zoomOut()` gives `maxFramesPerPoint(mediaFrames: 96_000, width: 600)`;
      - 10 × `taller()` gives `amplitude == 16`, and 10 × `shorter()` gives 1.
  - `WindowLayoutTests.swift`, `theBottomPaneListsTheAudioEditorInBothViews`:
    - `ViewLayouts.views(.bottom, in: .arrange) == [.editor, .audioEditor, .mixer]` and `views(.bottom, in: .mix) == [.editor, .audioEditor]`;
    - `PaneView.audioEditor.rawValue == "audio_editor"` and its title is "Audio Editor";
    - decoding `{"arrange":{"bottom":{"view":"audio_editor"}}}` gives `.audioEditor`, and `{"arrange":{"left":{"view":"audio_editor"}}}` gives the left default `.strip`.
- [ ] **Step 2: Run them and see them fail:** `scripts/swift.sh test --filter 'AudioEditorModelTests|WindowLayoutTests'`.
- [ ] **Step 3: Implement** per Rulings 1, 2, 3, 5, 7 and 9.
  - `type` parses, clamps, builds the command from the current row and sends it through `document.editor.edit`. A refusal sets `editor.status = box.hint` and returns false. With no region it sets `editor.status = AudioEditorModel.emptyMessage` and returns false.
  - `.peak` only stores `peakDb`.
  - `normalize` calls `RegionCommands.normalize(peakDb: peakDb, document)`.
  - `PaneFrame`'s choices and `layoutRow`'s lists follow `ViewLayouts.views` with no other change.
- [ ] **Step 4: Run the gates.** Swift is baseline + 28, with the edited list tests passing and the no-brand build clean.
- [ ] **Step 5: Commit** `AudioEditorModel.swift`, `WindowLayout.swift`, `PaneContent.swift`, `DocumentModel.swift`, `DocumentShell.swift`, `AudioEditorModelTests.swift`, `WindowLayoutTests.swift` and each edited test file, by path. Message: "app model: the audio editor's region, zoom, typed edits and normalize; Audio Editor on the bottom pane's lists (plan 7c Task 4)", with the trailers.

---

### Task 5: The control channel: `state.get audio_editor`, `ui.pane` and ⌘5

**Files:**
- Modify:
  - `crates/core-control/src/request.rs` (`StateTarget::AudioEditor`, `"audio_editor"`; the error text ends "…, surfaces, layout, mixer or audio_editor");
  - `crates/core-control/src/registry.rs` (`"audio_editor"` in `UI_PANE_PARAMS`' view Choice, after `"drum_grid"`; `cmd("editors.audio_editor", "Editors", "Audio Editor", Some("⌘5"), true)` after `editors.drum_grid`);
  - `crates/core-ffi/src/control.rs` (the target row, the three records, `ControlResult::AudioEditor`, `fade_shape_text`, the JSON);
  - `app/Sources/VizDAWAppModel/ControlCommands.swift` (`ControlStates.state` `.audioEditor`);
  - `ViewCommands.swift` (`audioEditor = "editors.audio_editor"` in `ids`, and its `run`);
  - `AudioEditorModel.swift` (`stateRow()`);
  - `app/Sources/VizDAW/AppCommands.swift` (the Editors menu's "Audio Editor" button with `KeyMenu.shortcut("editors.audio_editor")`, after the four editors, inside the existing builders, wrapped in a `Group` with its neighbours if a builder's ten-item limit bites, 7a Ruling 4).
- Test: `crates/core-control/tests/commands.rs` (new tests; the registry count assertion becomes 155 with "; plan 7c: editors.audio_editor" appended; `mixer_state_target_parses`' message check becomes `"mixer or audio_editor"`), `crates/core-ffi/tests/control.rs`, `app/Tests/VizDAWAppModelTests/ViewCommandsTests.swift`, `ControlCommandsTests.swift`

**Interfaces:**
- Consumes: Task 4's model, `ViewCommands.choose`, `ViewCommands.runPane`, `FadeRow` and `FadeShapeRow`.
- Produces:
  - Rust (`uniffi::Record` / `Enum`, documented):
    - `StateTargetRow::AudioEditor`;
    - `pub struct AudioEditorBoxRow { pub name: String, pub text: String }`;
    - `pub struct AudioEditorRegionRow { pub event: u64, pub media: u64, pub name: String, pub gain_db: f32, pub fade_in: FadeRow, pub fade_out: FadeRow, pub source_offset: u64, pub length: u64, pub media_frames: u64, pub sample_rate: u32, pub offline: bool }`;
    - `pub struct AudioEditorStateRow { pub shown: bool, pub region: Option<AudioEditorRegionRow>, pub message: Option<String>, pub frames_per_point: f64, pub origin_frame: f64, pub amplitude: u32, pub boxes: Vec<AudioEditorBoxRow> }`;
    - `ControlResult::AudioEditor { doc: String, editor: AudioEditorStateRow }`, serialised in Ruling 10's shape: `boxes` is a JSON object keyed by the `AudioEditorBox` raw values, and `gain_db` is rounded to thousandths.
  - Swift: `AudioEditorModel.stateRow() -> AudioEditorStateRow`.

- [ ] **Step 1: Write the failing tests.**
  - `commands.rs`, `audio_editor_state_target_parses`:
    - `state.get {target: "audio_editor"}` gives `StateTarget::AudioEditor`;
    - `"audio-editor"`, `"audioEditor"`, `"AUDIO_EDITOR"`, `"audio_editor "`, `""` and `7` give `InvalidParams`, each with a non-empty message;
    - the message for `"audio-editor"` contains "mixer or audio_editor".
  - `ui_pane_names_the_audio_editor`:
    - `command.run {id: "ui.pane", params: {pane: "bottom", view: "audio_editor"}}` parses;
    - `view: "audio"` and `view: "Audio_Editor"` give `InvalidParams`;
    - `registry::find("editors.audio_editor")` has shortcut `"⌘5"`, menu "Editors" and `document: true`.

    The existing `the_registrys_defaults_parse_and_never_collide` (`crates/core-control/tests/bindings.rs`) and `shortcuts_name_their_menu_commands` (`commands.rs`) cover ⌘5.
  - `crates/core-ffi/tests/control.rs`, `audio_editor_result_json`:
    - the row in Ruling 10's example (with `gain_db: -4.2_f32`) serialises to a `Value` equal to that JSON, so `gain_db` reads `-4.2`;
    - a row with `region: None`, `message: Some("Select one audio region to edit it.")` and no boxes serialises with `"region": null` and `"boxes": {}`;
    - `FadeShapeRow::SCurve` gives `"s_curve"` and `EqualPower` gives `"equal_power"`.
  - `ControlCommandsTests.swift`, `audioEditorStateNamesTheRegionAndItsBoxes`. Use Task 4's fake region with its gain set to −4.2 and its fade in to 12 000 linear (offset 24 000, 48 000 frames), and the bottom pane shown on the audio editor:
    - `ControlStates.state(.audioEditor, id, m)` is `.audioEditor`, with `shown == true` and `region?.event == 71`;
    - its boxes are `[gain "−4.2 dB", fade_in "250 ms", fade_out "0 ms", start "0:00.500", length "0:01.000", peak "−0.3 dB"]`;
    - with nothing selected, `region == nil`, `message == AudioEditorModel.emptyMessage` and `boxes == []`;
    - after `sync(width: 600)` on region 71, selecting region 72 and calling `stateRow()` (with no canvas) gives `region?.event == 72` and the axis of `FrameAxis.fit(offset: 0, length: 24_000, width: 600, mediaFrames: 96_000)` (the pre-flight asked for this in Task 4; it lives here because `stateRow` is Task 5's);
    - with region 71's media row removed from the fake, `region?.mediaFrames == 0`, `sampleRate == 0` and `offline == true`.
  - `ViewCommandsTests.swift`, `uiPaneTakesTheAudioEditorOnlyAtTheBottom`:
    - `ui.pane {pane: bottom, view: audio_editor}` sets the current bottom view to `.audioEditor` and answers status `"bottom audio_editor 260 shown"`;
    - `ui.pane {pane: left, view: audio_editor}` fails `invalid_params` with "audio_editor is not on the left pane's list in the arrange view.", and the layout is unchanged.
  - `theAudioEditorKeyShowsTheBottomPane`:
    - with the Arrange bottom pane hidden on Mixer, `run("editors.audio_editor")` gives bottom shown on `.audioEditor`;
    - after `ui.view_mix` the same command sets the Mix setup's bottom pane;
    - no alert, and `layout.detail` is unchanged.
- [ ] **Step 2: Run them and see them fail:** `cargo test -p core-control --test commands audio_editor`, `cargo test -p core-ffi --test control audio_editor` and `scripts/swift.sh test --filter 'audioEditorState|uiPaneTakesTheAudioEditor|theAudioEditorKey'`.
- [ ] **Step 3: Implement** per Ruling 10.
  - `stateRow()` always calls `sync(width:)` with the last width it was given (0 keeps the default axis), so after a selection change the state names the new region and its fit even before the canvas redraws.
  - `everyRegisteredMenuCommandHasAHandler` stays green through `ViewCommands.ids`.
- [ ] **Step 4: Run the gates:** the Rust gates, then the Swift ones. Rust is baseline + 7 and registry 155. Swift is baseline + 31. The no-brand build is clean.
- [ ] **Step 5: Commit** the listed Rust and Swift files and tests, by path. Message: "control: state.get audio_editor, ui.pane audio_editor and Editors > Audio Editor on ⌘5 (plan 7c Task 5)", with the trailers.

---

### Task 6: The canvas and the pane

**Files:**
- Create: `app/Sources/VizDAWEditors/AudioEditorCanvas.swift`, `app/Sources/VizDAW/AudioEditorPane.swift`, `app/Tests/VizDAWAppModelTests/AudioEditorViewTests.swift`
- Modify: `app/Sources/VizDAW/DocumentShell.swift` (`case .audioEditor: AudioEditorPane(model: model)`), `app/Tests/VizDAWEditorsTests/ThemeScanTests.swift` (`spacingFiles` gains `"VizDAW/AudioEditorPane.swift"`)

**Interfaces:**
- Consumes:
  - Tasks 1–5, including `AudioEditorGeometry.span` and `waveRect`;
  - `ThemedView` and `ThemedViews.current`;
  - `CanvasAccessibilityItem` and `CanvasElement`;
  - `RegionDrawing.fadeShapeMenu`, `readout`, `hatch` and `pill` (through `AudioEditorHandle.regionHandle`);
  - `WaveformGeometry.bars`;
  - `EditorModel.beginGesture`, `updateGesture`, `endGesture` and `cancelGesture`;
  - `WheelScroll`;
  - `SymbioticControls.NumberBox` and the palette helper `ChannelStrip` uses;
  - `AppIcon`;
  - `ArrangementView`'s observation set (`ArrangementView.swift:73–89`).
- Produces:
  - `public final class AudioEditorCanvas: NSView, ThemedView`, with `init(document: DocumentModel)`, `public var theme: Theme`, `acceptsFirstResponder == true`, and these test seams, which `mouseDown`/`mouseDragged`/`mouseUp`/`keyDown` call:
    - `public func press(_ p: CGPoint, clicks: Int, modifiers: NSEvent.ModifierFlags)`;
    - `public func drag(_ p: CGPoint, modifiers: NSEvent.ModifierFlags)`;
    - `public func release(_ p: CGPoint, modifiers: NSEvent.ModifierFlags)`;
    - `public func cancelKey()` (Escape);
  - `struct AudioEditorPane: View { let model: DocumentModel }`, whose root is an accessibility container labelled and identified "Audio Editor Pane" (Ruling 10).

- [ ] **Step 1: Write the failing tests** in `AudioEditorViewTests.swift`. They use Task 4's fake (regions 71 and 72; region 71 at tick 48 000 has 75 000 frames of room before the sequence start, more than its 24 000-frame offset) and a canvas framed 644 × 218, so the wave rect is 600 × 200, with `sync(width: 600)` applied. Drive it through the seams above, as `RegionHandleViewTests` drives `ArrangementView` through `contentMouseDown(_:clicks:modifiers:)`. The fake's records are its `calls` strings ("begin …", "gesture", "end", "abort") and `gestureEdits`: use the tree's names.
  - `theCanvasExposesItsHandles`:
    - the canvas's accessibility label is "Audio Editor";
    - its children's labels are exactly `["Fade In Handle", "Fade Out Handle", "Start Handle", "End Handle", "Gain Handle"]`, each frame inside the bounds;
    - with nothing selected the children are `[]`;
    - the same span: after a draw (`display()` in an unshown window, as the theme test does), the fake's last `overview` call and last `envelopeCalls` entry have equal `fromFrame`, `toFrame` and `columns` (give `FakeSession` an `overviewCalls` record beside `envelopeCalls` if it has none);
    - refit with no layout: select region 72 and let the observation refresh run (one main-queue turn), with no `layout()` call: `audioEditor.shownRegion == 72`.
  - `aHandleDragIsOneGesture`:
    - a press on the Gain Handle, a drag 20 points up in four steps, then release;
    - the fake records one begin with label "Region Gain", a last gesture edit of `.audio(edit: .setGain(event: 71, gainDb: 5.0))`, and one end.
  - `aTrimDragPastTheFileStartClamps`: a press on the Start Handle and a drag 10 000 points left give the label "Trim Start" and a last target `.trim(event: 71, frontFrames: -24_000, backFrames: 0)`.
  - `anEscapeCancelsTheDrag`: during a fade-in drag, `cancelKey()` records one abort and no end, and `editor.gesture == nil`.
  - `theCanvasRedrawsWhenItsThemeChanges`: as `anEditorRedrawsWhenItsThemeChanges` does, setting `theme` marks the canvas for display.
- [ ] **Step 2: Run them and see them fail:** `scripts/swift.sh test --filter AudioEditorViewTests`.
- [ ] **Step 3a: Accessibility, hits and the gesture** (Ruling 6).
  - Accessibility items per handle from `AudioEditorGeometry.handles`, as `CanvasElement`s.
  - `press` hits through `AudioEditorGeometry.hit`, keeps the row at the press, and calls `window?.makeFirstResponder(self)` so Escape reaches the canvas. Past the 3-point dead zone `drag` opens `beginGesture(label, lanes: [lane], origin: .other)` and sends `updateGesture(target(pressRow, …, startRoom: audioEditor.startRoom, …))`. `release` calls `endGesture`, and `cancelKey` calls `cancelGesture`.
  - Run `scripts/swift.sh test --filter AudioEditorViewTests`: tests 1–4 pass.
- [ ] **Step 3b: The theme and the drawing** (Rulings 3, 8, 12).
  - **Drawing order:**
    1. the background (`canvas`), with `audioOutside` beyond the file;
    2. the ruler;
    3. the scale gutter, its marks and lines;
    4. the waveform over `AudioEditorGeometry.span(axis:width:mediaFrames:)` from `session.overview(media:fromFrame:toFrame:columns:)` with the span's `from`, `to` and `columns`, `shaped(…, envelope: nil, amplitude:)`, drawn at the span's `x` in `audioWaveTrimmed`;
    5. the region's fill, `regionFill(tint:)` with the lane's strip colour (`document.laneColour`);
    6. the same span and columns shaped with `session.regionEnvelope(event:fromFrame:toFrame:columns:)`, in `audioWave`, clipped to the region's rect;
    7. the envelope line;
    8. clip marks;
    9. the handles (`audioHandle`, the dragged one `audioHandleSelected`; corners use `theme.radius`);
    10. the gain readout while dragging (`AudioEditorEntry.gainText`).
  - When `span` is nil, steps 4–8 draw nothing.
  - Offline media is hatched with the "Offline" pill, with the envelope still drawn.
  - Run the filter again: test 5 passes.
- [ ] **Step 3c: Wheel, pinch, ⌥-click and the shape menu.**
  - ⌘-scroll and pinch call `zoom(by:aroundX:)`; plain scroll calls `scroll(byPoints:)`.
  - ⌥-click on the gain band resets the gain.
  - A right-click in a fade area (`RegionHandles.fadeArea` through `regionHandle`) opens `fadeShapeMenu`.
  - These four inputs have no unit test: they are checked by eye (README 218–220). Run the filter again: all five still pass.
- [ ] **Step 3d: Observation and sync** (pre-flight I4).
  - The canvas observes `ArrangementView`'s set, copied: the document's snapshot and `lanes`, `editor.selection` and `drawVersion`, `mediaVersion` (offline hatching), `mixerVersion` and `storeMixer?.snapshot` (the lane tint), and `overviewVersion` (Pending and Coarse redraw). It also observes `audioEditor`'s `axis`, `amplitude` and `shownRegion`.
  - Observation uses `ArrangementView`'s `withObservationTracking` plus `Task { @MainActor [weak self] }` pattern, not `MainActor.assumeIsolated`.
  - Each observation refresh, and `layout()`, call `audioEditor.sync(width: AudioEditorGeometry.waveRect(in: bounds).width)`, so selecting another region refits it with no frame change. `sync` assigns each property only when its value changed, so a refresh that observes `axis`, `amplitude` and `shownRegion` cannot retrigger itself.
  - Run the filter again: all five pass (test 1's refit and same-span checks pin this sub-step).
- [ ] **Step 4: Build the pane.**
  - The root is `.accessibilityElement(children: .contain)` with label and identifier "Audio Editor Pane".
  - Without a region, it shows `ContentUnavailableView("Audio Editor", icon: .audio, description: Text(AudioEditorModel.emptyMessage))`.
  - With one, a bar over the canvas (an `NSViewRepresentable` host). In a horizontal `ScrollView`, so a narrow pane scrolls rather than squashes, the bar holds:
    - "Gain" with its NumberBox;
    - "Fade In" with its box and shape Picker;
    - "Fade Out" with its box and shape Picker;
    - "Start" and "Length" with their boxes;
    - a spacer;
    - "Peak" with its box, and a "Normalize" button;
    - Zoom Out and Zoom In (`.zoomOut`, `.zoomIn`) and a "Fit Region" text button;
    - Shorter Waveform and Taller Waveform (`.collapse`, `.expand`).
  - The boxes' accessibility labels are Ruling 7's. Each box's `commit` is `audioEditor.type(box, text)`; `onRefuse` sets the box's hint as the status.
  - The pickers use `Binding(get:set:)`, with no `@State`.
  - Gaps and padding come from `theme.spacing`.
  - There is no pan control (Ruling 11).
- [ ] **Step 5: Run the gates.** Swift is baseline + 36, the spacing scan includes the new file, and the no-brand build is clean.
- [ ] **Step 6: Take one screenshot for the controller**, offline, in a throwaway home, then quit and clean up. Only finished UI is shown to the owner.

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/make-app.sh
rm -rf /tmp/vze7c-shot && mkdir -p /tmp/vze7c-shot
# 2 s of stereo 16-bit at 0.25 (−12.04 dBFS), 48 kHz, stdlib only.
python3 -I -c 'import wave,struct,sys; w=wave.open(sys.argv[1],"wb"); w.setnchannels(2); w.setsampwidth(2); w.setframerate(48000); s=struct.pack("<hh",8192,8192); w.writeframes(s*96000); w.close()' /tmp/vze7c-shot/tone.wav
vizctl launch --env VIZDAW_AUDIO=offline --home /tmp/vze7c-shot --demo
```

  Then, through `vizctl call` (see `vizctl help`):
  1. `state.get document` for `selected_seq`;
  2. `command.run media.import {path: "/tmp/vze7c-shot/tone.wav", seq, tick: 0}`;
  3. `ui.click` the region by its element (the "Region tone on …" label);
  4. `command.run editors.audio_editor`;
  5. `ui.screenshot {path: "/Users/lattawb/workspace/vizdaw/.superpowers/sdd/2026-11-04-audio-editor/t6.png"}`;
  6. `app.quit`.

  Afterwards `rm -rf /tmp/vze7c-shot`. `pgrep -lf MacOS/VizDAW` prints nothing. The screenshot is not committed (`.superpowers/` never is). Report its path.
- [ ] **Step 7: Commit** `AudioEditorCanvas.swift`, `AudioEditorPane.swift`, `DocumentShell.swift`, `ThemeScanTests.swift` and `AudioEditorViewTests.swift`. Message: "app: the audio editor's canvas and pane (plan 7c Task 6)", with the trailers.

---

### Task 7: The e2e, the by-eye steps, the root README and the full gates

**Files:**
- Modify: `crates/vizctl/tests/e2e.rs`, `app/README.md`, `README.md`, `docs/feature-backlog.md`

**Interfaces:**
- Consumes: everything above. From the e2e: `App::start`, `app.call`, `app.call_err`, `audio_file`, `wait_for_label_like`, `labelled`, `find_node`, `wait_for_state`, `wait_for_undo`, `layout_pane`, `wait_for_layout`, and `ui.key {window, element, text}` as `themes_and_the_strip_over_the_channel` uses it.

- [ ] **Step 1: Write the e2e** `edits_a_region_in_the_audio_editor`, marked `#[ignore = "needs a window server and VIZDAW_E2E_APP"]`.
  1. Start the app. Make a 2 s stereo tone at 0.25 (`audio_file(&tone, 96_000)`, −12.04 dBFS). Run `file.create_demo`, then `media.import` at tick 0, and click the region by its element, as `regions_normalize_relink_and_trash` does.
  2. The hostile calls:
     - `state.get {target: "audio-editor"}` gives `invalid_params`;
     - `command.run ui.pane {pane: "left", view: "audio_editor"}` gives `invalid_params`, with the message from Ruling 10;
     - `ui.pane {pane: "bottom", view: "audio"}` gives `invalid_params`.

     After them `state.get layout` is unchanged, `alerts` is empty, and `state.get document`'s `alert` is null.
  3. `command.run editors.audio_editor`. Then:
     - `wait_for_layout` until the Arrange bottom pane's `view == "audio_editor"` and it is shown;
     - `state.get audio_editor` has `shown == true`, `region.event` equal to the region, `boxes.gain == "0.0 dB"`, `boxes.start == "0:00.000"` and `boxes.length == "0:02.000"`;
     - in the window's `ui.tree`, find the node labelled "Audio Editor Pane" and walk only its subtree: no node there has a label containing the whole word `\bPan\b` (implemented without a regex crate: split each label on non-letters and look for the word "Pan"). "Bottom Pane" and the left strip's "Pan of …" lie outside it.
  4. `ui.key` on "Region Gain" with `"-4.2\n"`: wait until `(region.gain_db + 4.2).abs() < 1e-6`, then `wait_for_undo(…, "Set Region Gain")`. Then `"abc\n"`: `state.get editor`'s `status == "Type a gain in dB, like −4.2 or +6."` and the gain is still −4.2.
  5. `ui.key` on "Fade In Length" with `"250\n"`: wait until `region.fade_in.frames == 12_000`.
  6. `ui.key` on "Region Length" with `"1.5\n"`: `length == 72_000`. Then on "Region Start" with `"0.5\n"`: `source_offset == 24_000` and `length == 48_000` (the end stayed).
  7. `ui.drag` from the "Gain Handle" element to the point 20 points above its centre (from its `ui.tree` frame), with 8 steps: wait until `(gain_db − 0.8).abs() < 0.3`, then `wait_for_undo(…, "Region Gain")`.
  8. `ui.key` on "Normalize Peak" with `"-1\n"`, then `ui.click` "Normalize": wait until `(gain_db − 11.04).abs() < 0.05`, then `wait_for_undo(…, "Normalize")`. Run `edit.undo`: the gain returns to step 7's value.
  9. `ui.click` "Zoom In": `frames_per_point` halves (±1).
  10. `file.save_as` to `<home>/edited.vizdaw`, `file.close`, then `app.open` it. Select the region again: `state.get audio_editor` shows step 7's gain, `fade_in.frames == 12_000`, `source_offset == 24_000` and `length == 48_000`, and the layout's bottom view is still `"audio_editor"`.
  11. `app.quit()`.
- [ ] **Step 2: Run it and see it pass** (it was RED until Task 6; record the run against Task 6's build in the report), then all 17 e2e tests.
- [ ] **Step 3: Write README steps 217–224** in `app/README.md`, under a new section after 7b's "Themes and the channel strip (plan 7b)". Each step is an action, then a colon, then the expected result, as the earlier steps are.

```markdown
### Audio editor (plan 7c)

Checked by hand; the rules are tested (`AudioEditorEntryTests`,
`FrameAxisTests`, `AudioEditorGeometryTests`, `AudioEditorModelTests`,
`AudioEditorViewTests`, the core's envelope and trim tests) and the e2e
`edits_a_region_in_the_audio_editor`.

217. File > Create Demo Project, import a stereo audio file of a few
     seconds (File > Import Audio…), click its region and press ⌘5: the
     bottom pane's picker reads "Audio Editor" and shows the region's
     waveform across the pane, each channel in its own band; the file's
     audio outside the region is dimmed; a ruler along the top reads the
     file's time (0:00.000 …); the level scale at the left reads 0 at the
     top of each band and −∞ at its centre line. Click an empty part of the
     arrangement: "Select one audio region to edit it."; select two regions:
     the same; select the region again: it comes back.
218. Click Zoom In, Zoom Out and Fit Region, then ⌘-scroll and pinch over
     the waveform: it zooms around the pointer and stops at its limits;
     scroll sideways: it stops when the file's start or end reaches the
     middle of the pane. Click Taller Waveform: quiet parts grow and the
     level marks relabel, and after the first click the "0" mark is gone;
     Shorter Waveform undoes it. Select another region: the editor fits it;
     select the first again: it fits again.
219. Drag the band along the top of the region up: a readout follows
     ("+3.0 dB") and the waveform grows; hold ⇧ for finer steps; release:
     Edit shows "Undo Region Gain". ⌥-click the band: 0.0 dB. In the Gain
     box type `-4.2` and press Return: "−4.2 dB", the waveform shrinks, and
     Edit shows "Undo Set Region Gain". Type `abc`: the box is unchanged and
     the status line says "Type a gain in dB, like −4.2 or +6.". Type `30`:
     "+24.0 dB", and red clip marks appear along the band tops where the
     audio now goes over 0 dBFS. Type `-inf`: "−∞ dB" and a flat line.
     Type `-9` and click away without Return: nothing changes.
220. Drag the square at the region's top-left corner right: a fade-in curve
     is drawn over the waveform and the Fade In box follows in ms; the
     square stops at the fade-out's square; release: "Undo Fade In". Choose
     each Fade In Shape (Linear, Equal Power, Exponential, S-Curve): the
     curve changes and its length stays. Right-click inside the fade: the
     same four shapes, the current one checked. Type `250` in Fade In:
     "250 ms"; `1.5 s`: "1500 ms"; a length longer than the region: it stops
     where the fade out begins; `-5`: refused with "Type a fade like 250 ms
     or 1.5 s.". Do the same with the fade out at the top-right corner.
221. Drag the region's left edge right: the region starts later in the file
     and in the arrangement by the same time, its audio stays where it was,
     and the Start box follows; drag it left past the file's start: it
     stops at 0:00.000, or where the region would begin before the
     sequence's start, whichever is nearer (a region at bar 1 cannot be
     extended earlier); release: "Undo Trim Start". Drag the right edge past
     the file's end: it stops there; "Undo Trim End". Type `0.5` in Start:
     the region's end stays put; type `1` in Length: its start stays put;
     each is "Undo Trim Region". Type `10:00` in Length: it stops at the
     file's end. Trim the region shorter than its fades: the fades shorten
     to fit.
222. Type `-1` in Peak and click Normalize: the gain becomes what brings the
     loudest sample to −1 dBFS (a file peaking at −12 dBFS reads "+11.0 dB")
     and Edit shows "Undo Normalize"; no dialog opens. Type `-inf` in Peak:
     refused with "Type a peak from −96 to 0 dB, like −0.3."; `+3`: "0.0 dB".
     Right after an import Normalize may say the waveform is still being
     built: try again a moment later. Move the region's file away in Finder
     so it goes offline: the region is hatched with "Offline" and its fades
     and gain line still show; Normalize says to relink it, in the status
     line, with no alert.
223. Play while dragging a handle and while typing: each change is heard at
     once. Undo and Redo step through the edits one at a time. Save, close
     and reopen: the gain, fades, shapes and trim are as you left them, and
     the bottom pane still shows the Audio Editor. Open the project in a
     build from before plan 7c (`git worktree add /tmp/vz7b ba0224b && (cd
     /tmp/vz7b && scripts/build-ffi.sh && scripts/make-app.sh)`, then open
     `/tmp/vz7b/dist/VizDAW.app`): it opens with the same gain, fades and
     trim, and its bottom pane shows its default editor. Quit it, then
     `git worktree remove /tmp/vz7b`.
224. Press ⌃2: the Mix view's bottom pane picker lists Audio Editor too, and
     shows the same region. Choose each theme in Settings › Appearance: the
     editor's waveform, scale, handles and boxes change with it. The editor
     has no pan control and nothing that changes the audio file: after
     every step above, the file in the project's Media folder has its
     original modification date.
```

- [ ] **Step 4: Update the root `README.md`** (spec §2 item 11, 7c's part). Keep 7a's and 7b's sections and the disclaimer.
  - Status: delete the sentence "An audio editor is next." (wrapped as "An audio editor\nis next."), so the paragraph reads "…records, edits and mixes MIDI and audio in one window. The design spec describes where the project is going."
  - "What works today" gains a bullet after the themes bullet: "An audio editor in the bottom pane: a region's waveform drawn large with zoom and a level scale, and its gain, fades and fade shapes, trim and normalize, each by dragging or by typing a value".
  - "Getting around": after the "⌘1 to ⌘4 open the piano roll…" bullet, add "⌘5 opens the audio editor for the selected audio region."
- [ ] **Step 5: Add backlog rows** to `docs/feature-backlog.md`:
  - "Audio editor: playhead, audition and scrub" (Rulings 11 and 13.1);
  - "Audio editor in a window of its own" (Ruling 1);
  - "Audio editor zoom saved with the project" (Ruling 3);
  - "Audio editor handles moved with the keyboard" (the handles are reachable by VoiceOver but not movable from the keyboard).
- [ ] **Step 6: Run the full gates.**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --workspace --all-targets --target x86_64-unknown-linux-gnu --target-dir target/linux-check -- -D warnings
cargo test --workspace
cargo clippy -p vizdaw-play --all-targets --features rt-check -- -D warnings
cargo test -p core-engine --features rt-check --test rt_alloc --test rt_alloc_graph --test rt_alloc_record --test rt_alloc_gestures --test rt_alloc_bounce
scripts/build-ffi.sh && scripts/swift.sh build && scripts/swift.sh test
rm -rf .build-nobrand && VIZDAW_NO_BRAND=1 scripts/swift.sh build --scratch-path .build-nobrand
find .build-nobrand \( -name 'SymbioticBrand*.o' -o -name 'SymbioticBrand*.swiftmodule' \)
scripts/make-app.sh
VIZDAW_E2E_APP=dist/VizDAW.app cargo test -p vizctl --test e2e -- --ignored --test-threads 1
pgrep -lf MacOS/VizDAW; ls -d /tmp/vze* 2>/dev/null
```

Expected:
- Rust: baseline + 7 passed and baseline + 1 ignored (macOS); Linux clippy clean; the rt-check gates pass;
- Swift: baseline + 36;
- the `find` prints nothing;
- e2e 17 passed;
- registry 155;
- `pgrep` and `ls` print nothing;
- `git diff 5224ca1 -- crates/core-model fixtures` is empty (no format change).

- [ ] **Step 7: Commit** the four files. Message: "e2e: edits a region in the audio editor; README 217-224; the root README's audio editor; backlog rows (plan 7c Task 7)", with the trailers.

---

## Self-Review

1. **Spec coverage.**
   - §6 item 2:
     - a bottom-pane view of the selected audio region: Tasks 4 and 6 (Rulings 1, 2);
     - the waveform drawn large with zoom and a level scale: Tasks 3 and 6 (Rulings 3, 8);
     - gain, fades with shapes, trim of the start and length, normalize as gain: Tasks 4 and 6 (Ruling 5's table);
     - handles and typed values for each: Tasks 3, 4 and 6 (Rulings 6, 7);
     - the existing model, commands and undo: Ruling 5, with no `core-model` change;
     - no pan, no destructive edits: Ruling 11, the e2e's "Pan" check, README 224;
     - no format change: Global Constraints "Format", Task 7's `git diff` check, README 223.
   - §7 row 7c:
     - control-channel state: Task 5 (Ruling 10);
     - hostile-input tests: Tasks 1, 2, 5 and 7;
     - README steps: Task 7.
   - §2 item 3's bottom list: Task 4. §2 item 11, 7c's README part: Task 7.
   - §4: themes (Ruling 12, the scans), controls big enough (Ruling 6's hit sizes), agents keep working (Task 5, e2e). §5: hostile input; no dialogs (Ruling 9).
   - Model gaps: one, ruled in Ruling 4 (`region_envelope`). Ruling 5 checks the rest.
   - The pre-flight (`.superpowers/sdd/2026-11-04-audio-editor/preflight.md`) is folded: C1, C2, I1–I6 and the Minors, as `fold-log.md` lists. The controller's provisional calls are Ruling 13.
2. **Step scan.** Every test step names its tests and assertions with exact values. Every code step names files, signatures and the ruled values. The view task (6) is pinned by its five tests, the scans, the e2e and README 217–224. No step says "handle edge cases" without naming them.
3. **Type consistency.**
   - `region_envelope` (1) → `SessionProtocol.regionEnvelope` → `AudioEditorGeometry.shaped`/`clipColumns`/`envelopeLine` (3) → the canvas (6).
   - `FrameAxis` (3) → `AudioEditorModel.axis` (4) → `stateRow`'s `frames_per_point`/`origin_frame` (5) → the canvas (6).
   - `AudioEditorGeometry.span` (3) → both of the canvas's requests, `overview` and `regionEnvelope` (6).
   - `sync(width:)` always takes the wave rect's width (4, 5, 6).
   - `AudioEditorBox` raw values (4) are the JSON `boxes` keys (5), and its labels are the e2e's element labels (7).
   - `AudioEditorEntry` hints (2) are the status texts in Tasks 4, 7 and the README.
   - `AudioEditorGeometry.label` (3) gives the undo labels in Tasks 6, 7 and the README; the core's command names give the typed ones.
   - `PaneView.audioEditor` (4) has the raw value `"audio_editor"` used by `ui.pane`'s Choice (5) and `layoutRow`.
4. **Review Focus.** Five items, each with its pinning tests in the owning task.
5. **Proportion.** About 1100 short lines after the pre-flight fold (7b's is 1490, 7a's 1680) for seven tasks against §6 item 2 and §7's row. Most of the length is exact test values, Ruling 7's table and the README copy; bodies are left to the signatures.

## Execution handoff

Plan complete and saved to `docs/superpowers/plans/2026-11-04-audio-editor.md`. The controller runs it subagent-driven, with one reviewer per task, as in 7a and 7b.

- Run the tasks in order, 1 to 7. Tasks 2 and 3 do not use each other's code, but they share the tree with Task 1's FFI regeneration, and the counts above assume that order.
- Task 6 is view work that only its five tests, the scans, the e2e and the owner's by-eye steps check. Its review should read the drawing order and the input list against Rulings 6, 8, 11 and 12 line by line, and the controller compares Task 6's screenshot with the mockup direction before Task 7.
