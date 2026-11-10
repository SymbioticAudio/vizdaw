# MIDI Strips Implementation Plan (5g-1b)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Each external MIDI destination gets a simplified channel strip: the name, MIDI FX, pan (CC 10), a 0–127 fader (CC 7) with a note-activity meter, Mute and Record arm. The strip shows in the mixer, and in the left pane for a MIDI track.

**Architecture:**
- The strip's state lives on the destination (`Destination.midi_mix`), not on a mixer `Strip`, so the audio graph, automation and control surfaces are untouched.
- The schedule compiler sends the set values at frame 0 after the default patch, and chase already covers CC 7 and CC 10.
- A change is sent at once through a general `Engine::send_now` (audition becomes a wrapper of it).
- Swift shows MIDI strips as a second kind of column in the existing strip grid.

**Tech Stack:** Rust (core-model, core-engine, core-ffi, core-control), UniFFI, Swift 6.1 / SwiftUI.

**Spec:** `docs/superpowers/specs/2026-10-18-recording-audio-design.md` §6.8a (owner, 2026-11-10), with §4.5a and §6.8.

## Global Constraints

These are the same as plan 5g-1's Global Constraints (`docs/superpowers/plans/2026-11-02-midi-input-effects.md`, "Global Constraints"), which every task includes, with these differences:

- **Branch** `sp5g-midi-strip` (made from `sp5g-input-fx` at `1e1a5cd`). Never switch branches, never push.
- **UniFFI changes** (Task 3):
  - record `MidiMixRow { volume: Option<u8>, pan: Option<u8>, mute: bool }`;
  - record `MidiLevelRow { dest: u64, velocity: u8 }`;
  - fields `DestRow.midi_mix: MidiMixRow`, `DestRow.channel: Option<u8>` and `MixerLevels.midi: Vec<MidiLevelRow>`, each `#[uniffi(default)]` (give `MidiMixRow` a `Default`);
  - `Session` methods `set_midi_mix(dest: u64, mix: MidiMixRow) -> Result<DocSnapshot, VizError>` and `preview_midi_mix(dest: u64, volume: Option<u8>, pan: Option<u8>) -> Result<bool, VizError>`, both inside `ffi_guard`.
- **rt-check gate** as in 5g-1, plus the new `rt_alloc_midi_strip` test (Task 2).
- **Format:** file `VERSION` 5 → 6 (Task 1).
- **Real-time:** the activity counter is the only new real-time code, and it is marked `// RT`. `send_now` reuses `EngineCmd::Audition`, so the renderer gains no new command.
- **Baseline at `1e1a5cd`:** Rust 2146 passed / 25 ignored, Swift 1302, e2e 21, registry 163, README checks through 244, file `VERSION` 5. Report the measured counts.
- **Gates:**
  - Whole workspace tests in Tasks 1, 2, 3 and 6.
  - Swift build and test in Tasks 3–6.
  - The no-brand build in Task 6.
  - `scripts/make-app.sh` and the e2e in Tasks 4 and 6.

## Review Focus

1. **A value the synth must not get.** Examples: a project from before this plan; a destination never touched; a built-in-synth or plugin destination; volume 128; pan −1.
   - Expected: an unset value sends nothing, ever. Only external destinations accept a mix. Out-of-range values are refused and change nothing; a damaged file is Corrupt, naming the field.
   - Pinned by `an_untouched_destination_sends_no_cc` (Task 2), and `midi_mix_is_checked_and_external_only` and `a_damaged_midi_mix_is_corrupt` (Task 1).
2. **Order at the start.** Example: a destination with a default patch, volume 90 and pan 20, starting at bar 1 or mid-song after a recorded CC 7 of 40.
   - Expected: bank, program, CC 7 and then CC 10 at frame 0. Mid-song, chase sends the recorded 40, not the strip's 90.
   - Pinned by `mix_follows_the_patch_at_frame_0` and `a_recorded_cc7_wins_over_the_mix_mid_song` (Task 2).
3. **Mute with notes sounding.** Examples: mute while a long note plays; unmute.
   - Expected: the sounding note ends at once (a schedule swap releases); later notes don't play while muted, but CCs and pitch bend do; unmuting plays the next note.
   - Pinned by `muting_ends_sounding_notes_and_keeps_controllers` (Task 2).
4. **A fader drag.** Examples: a drag across the throw; a click without dragging; a typed "200"; a typed "L 70".
   - Expected: the synth follows during the drag without an undo step per move; one Undo step ("Set MIDI Mix") per gesture; a click is one step; bad text is refused with no change.
   - Pinned by `aDragPreviewsAndCommitsOnce`, `aClickCommitsOnce` and `typedValuesAreParsedAndRangeChecked` (Task 5).
5. **A hostile control call.** Examples: an unknown dest, a plugin dest, `volume: "128"`, `pan: -1`, `mute: "yes"`, extra keys, a huge string.
   - Expected: `invalid_params` or `failed` as for `destination.set_return_latency`, and nothing changes.
   - Pinned by `midi_mix_params_are_checked` (Task 4, core-control) and `setMidiMixChecksItsArguments` (Task 4, Swift).

## Plan Rulings

1. **The state is on the destination, not a mixer `Strip`.** Lanes are per sequence, while a destination is the device channel all of them share, as instrument strips already are. Making it a real `StripKind` would pull in the audio graph, automation and the control surfaces' banking. *Cost if wrong:* moving three fields onto a strip later, plus a migration.
2. **Unset is the default** (`volume: None, pan: None`), so opening an old project never changes a synth's level. The strip shows an unset fader at 100 and an unset pan at C, both dimmed, and the first move sets the value. *Cost if wrong:* a one-line default change.
3. **Mute drops only note-ons and note-offs** of a muted destination at compile time. The swap's `release_now` ends sounding notes. *Cost if wrong:* a filter change in one place.
4. **No solo** on MIDI strips. Audio solo semantics don't reach an external synth. *Cost if wrong:* a later button.
5. **The fader is linear in CC value** (position = value ÷ 127) with an integer readout. *Cost if wrong:* a curve function.
6. **A drag previews and commits on release.** `preview_midi_mix` sends CCs without touching the document, at most once per 20 ms; release (or a click, or typed text) calls `set_midi_mix`, which commits and sends. *Cost if wrong:* one undo step per drag becomes many, or the synth lags.
7. **Order in the mixer.** MIDI strips come after instrument and audio strips and before buses, in destination-list order. All external destinations are shown, used or not. *Cost if wrong:* a sort.
8. **The left pane's header has a destination menu** (the focused track's `LaneEdit::SetDest`, existing). The mixer's header shows the destination name only. *Cost if wrong:* a menu in the other placement.
9. **Arm and MIDI FX** on a MIDI strip act on the same lane `midiFxLane(for:in:)` chooses: the armed lane first, else the focused one (5g-1's AudioPath precedent). *Cost if wrong:* the same as 5g-1's ruling.

## File map

| File | What changes |
|---|---|
| `crates/core-model/src/document.rs` | `MidiMix`, `Destination.midi_mix` |
| `crates/core-model/src/command/routing.rs` | `RoutingCmd::SetMidiMix` |
| `crates/core-model/src/rules.rs` | file check of `midi_mix` |
| `crates/core-model/src/file.rs` | `VERSION` 6, `migrate_v5_to_v6` |
| `crates/core-engine/src/schedule/compile.rs` | frame-0 CCs, mute filter |
| `crates/core-engine/src/engine.rs`, `render.rs` | `send_now`, activity counter, `take_midi_activity` |
| `crates/core-ffi/src/mixer.rs`, `session.rs`, `host.rs` | rows, `set_midi_mix`, `preview_midi_mix`, levels |
| `crates/core-control/src/registry.rs` | `destination.set_midi_mix` |
| `app/Sources/VizDAWAppModel/SynthCommands.swift` | its handler |
| `app/Sources/VizDAWAppModel/MidiStrip.swift` (new) | `MidiStripMath`, `MixerColumn`, the gesture logic |
| `app/Sources/VizDAWAppModel/MixerModel.swift`, `PaneContent.swift` | columns, `StripFocus.Result.midi` |
| `app/Sources/VizDAW/MidiStripCells.swift` (new), `ChannelStrip.swift`, `MixerView.swift`, `PaneViews.swift` | the views |
| `app/README.md` | checks 245–250 |

---

### Task 1: The model (`core-model`)

**Files:**
- Modify: `crates/core-model/src/document.rs`, `command/routing.rs`, `rules.rs`, `file.rs`
- Test: `crates/core-model/tests/midi_mix.rs` (new), `crates/core-model/tests/file.rs`

**Interfaces:**
- Produces:
  - `pub struct MidiMix { pub volume: Option<u8>, pub pan: Option<u8>, pub mute: bool }`, with `Default`, `Copy`, `PartialEq`, serde, and `is_default()`;
  - `pub const MIDI_CENTRE: u8 = 64`;
  - `Destination.midi_mix: MidiMix`, `#[serde(default, skip_serializing_if = "MidiMix::is_default")]`;
  - `RoutingCmd::SetMidiMix { dest: DestId, mix: MidiMix }`, named "Set MIDI Mix".

- [ ] **Step 1: Write the failing tests** in `tests/midi_mix.rs`:
  - `midi_mix_is_checked_and_external_only`:
    - `SetMidiMix` with `volume: Some(90), pan: Some(20)` on an external destination stores both, and Undo restores the default.
    - `volume: Some(128)` is `ModelError::Range` naming `midi_volume`, and `pan: Some(128)` names `midi_pan`.
    - A plugin-instrument destination, the built-in synth's destination and an unknown id are each refused (`check_external`).
    - Every refusal leaves the document equal to before.
  - `an_untouched_destination_writes_no_midi_mix`: the saved JSON of a new external destination has no `midi_mix` key.
  - `a_damaged_midi_mix_is_corrupt` (in `tests/file.rs`): `"midi_mix":{"volume":200}` and the same mix on a plugin destination each fail to load as Corrupt, naming `midi_volume` and `midi_mix` respectively.
  - Extend the version test so a version-5 file migrates to 6 unchanged, and `VERSION == 6`.
- [ ] **Step 2: Run** `cargo test -p core-model --test midi_mix --test file`. Expected: fail to compile, or FAIL.
- [ ] **Step 3: Implement.**
  - The command follows `SetReturnLatency` (`routing.rs`), using `check_external`, then a range check on each `Some`.
  - In `rules.rs`, next to the `default_patch` check: values ≤ 127, and a non-default mix only on an external destination.
  - `migrate_v5_to_v6` returns `Ok(v)`, with a doc line in the style of `migrate_v4_to_v5`.
- [ ] **Step 4: Run** `cargo test --workspace`. Expected: PASS (about +4 tests).
- [ ] **Step 5: Commit** `feat(model): MIDI mix on external destinations (5g-1b Task 1)` with the files above.

### Task 2: Sending it (`core-engine`)

**Files:**
- Modify: `crates/core-engine/src/schedule/compile.rs`, `engine.rs`, `render.rs` (or the `Shared` struct's file)
- Test: `crates/core-engine/tests/midi_strip.rs` (new), `crates/core-engine/tests/rt_alloc_midi_strip.rs` (new, `rt-check` feature as the other `rt_alloc_*`)

**Interfaces:**
- Consumes: `Destination.midi_mix` (Task 1).
- Produces:
  - `Engine::send_now(&mut self, doc: &Document, dest: DestId, msg: Midi1Msg) -> Result<bool, EngineError>`, which replaces the status byte's channel nibble with the destination's channel (0 for a plugin). `audition` builds its note and calls it.
  - `Engine::take_midi_activity(&self, doc: &Document) -> Vec<(DestId, u8)>`: per external destination with a live target, the highest note-on velocity sent to it since the last call, then reset. Omit destinations at 0.

- [ ] **Step 1: Write the failing tests** with the fake MIDI sinks the existing engine tests use, none of them real devices:
  - `mix_follows_the_patch_at_frame_0`: a destination with a default patch (bank 1/2, program 5) and mix 90/20 on channel 3. The compiled entries at frame 0 for its target are, in order: `B2 00 01`, `B2 20 02`, `C2 05`, `B2 07 5A`, `B2 0A 14`. Only the volume set → just `B2 07 5A` after the program.
  - `an_untouched_destination_sends_no_cc`: no CC 7 or CC 10 entry anywhere for a default mix.
  - `a_recorded_cc7_wins_over_the_mix_mid_song`: mix volume 90 and a recorded CC 7 = 40 at bar 3. Starting at bar 5 chases 40; starting at bar 1 sends 90 at frame 0 and 40 at bar 3.
  - `muting_ends_sounding_notes_and_keeps_controllers`:
    - while a long note sounds, swapping in a schedule compiled with `mute: true` sends its note-off;
    - the muted schedule has no note-on or note-off for that destination but keeps its CC 1 and pitch bend;
    - other destinations are unaffected.
  - `send_now_uses_the_destination_channel`: `send_now(dest on ch 3, B0 07 64)` reaches the target as `B2 07 64`, and an unknown dest returns `Ok(false)`.
  - `activity_reports_the_peak_velocity_once`: playing note-ons of velocity 30 and 100 → `take_midi_activity` gives `[(dest, 100)]`, then `[]`.
  - `rt_alloc_midi_strip`: rendering a block that sends notes, under the rt-check allocator, does not allocate.
- [ ] **Step 2: Run** `cargo test -p core-engine --test midi_strip`. Expected: FAIL.
- [ ] **Step 3: Implement.**
  - In the default-patch loop in `compile.rs`, for external destinations (`used` keys), push the mix's CC 7 and CC 10 as `Class::Patch` entries after the program change, in that order, also for destinations with no default patch.
  - The mute filter skips note-on and note-off events for a muted destination where events are scheduled.
  - Activity uses one `AtomicU8` per target index (`MAX_TARGETS`) in the shared state, updated with `fetch_max` on each note-on the renderer sends to a MIDI sink (`// RT`). `take_midi_activity` `swap`s them to 0.
- [ ] **Step 4: Run** `cargo test --workspace` and the rt-check gate. Expected: PASS (about +7 tests).
- [ ] **Step 5: Commit** `feat(engine): send MIDI mix, mute filter, note activity (5g-1b Task 2)`.

### Task 3: The FFI (`core-ffi`)

**Files:**
- Modify: `crates/core-ffi/src/mixer.rs`, `session.rs`, `host.rs`
- Test: `crates/core-ffi/tests/midi_strip.rs` (new)

**Interfaces:**
- Consumes: Tasks 1–2.
- Produces: the UniFFI items in Global Constraints.
  - `DestRow.channel` is the destination's 0–15 channel for MIDI endpoints, else `None`.
  - `set_midi_mix` executes `SetMidiMix`, then (if this session holds the engine) `send_now`s CC 7 and CC 10 for the values set.
  - `preview_midi_mix` only sends, and returns false when nothing was sent.
  - `levels()` fills `midi` from `take_midi_activity`.

- [ ] **Step 1: Write the failing tests.** Every test calls `host::force_offline()` through the file's `serial` helper.
  - `set_midi_mix_round_trips_and_undoes`: set 90/20/mute → `destinations()` shows it and `channel == Some(2)`; Undo → default.
  - `set_midi_mix_refuses_bad_destinations`: a plugin dest, an unknown dest and volume 128 each give `Err`, and nothing changes.
  - `preview_sends_without_an_edit`: while playing offline, the document version doesn't change and the result is `true`; with no engine it is `false`.
  - `levels_report_midi_activity`: playing a note-on of velocity 100 to an external destination puts `MidiLevelRow { dest, velocity: 100 }` in `levels().midi`.
- [ ] **Step 2: Run** `cargo test -p core-ffi --test midi_strip`. Expected: FAIL.
- [ ] **Step 3: Implement** following `set_return_latency` (session.rs) and `audition` (`host::audition` → `Engine`). Add `host::send_now(owner, doc, dest, msg)` and `host::midi_activity(owner, doc)` in the style of `host::audition`.
- [ ] **Step 4: Run** `cargo test --workspace`, `cargo clippy -p core-ffi --lib -- -D warnings`, `scripts/build-ffi.sh`, `scripts/swift.sh build` and `scripts/swift.sh test`. Expected: PASS (about +4 Rust tests). Swift is unchanged apart from the new defaulted fields.
- [ ] **Step 5: Commit** `feat(ffi): MIDI mix rows, set and preview, MIDI levels (5g-1b Task 3)`.

### Task 4: The control channel

**Files:**
- Modify: `crates/core-control/src/registry.rs`, `app/Sources/VizDAWAppModel/SynthCommands.swift`, the `state.get` destination output (wherever `return_latency_ms` is reported)
- Test: `crates/core-control/tests/` (where `destination.set_return_latency` is tested), `app/Tests/VizDAWAppModelTests/SynthCommandsTests.swift`, `crates/vizctl/tests/e2e.rs`

**Interfaces:**
- Produces: the command `destination.set_midi_mix`, with params:
  - `dest` (a destination id string, as `set_return_latency`);
  - `volume`, `pan`: optional, each a decimal string `"0"`–`"127"` or the string `"none"` (unset);
  - `mute`: optional, `"true"`/`"false"`.

  An absent key keeps the current value, and at least one of the three is required. The registry goes to 164. `state.get` reports `midi_volume`, `midi_pan` and `midi_mute` per destination, next to `return_latency_ms`.

- [ ] **Step 1: Write the failing tests.**
  - `midi_mix_params_are_checked` (core-control): the registry entry exists, and its schema refuses extra keys, as its sibling's does.
  - `setMidiMixChecksItsArguments` (Swift): each of these is `invalid_params` and nothing changes:
    - a missing `dest`; a non-number `dest`; a dest that isn't external;
    - `volume: "128"`, `"-1"` or `"1e2"`; `pan: ""`;
    - `mute: "yes"`; no value keys at all;
    - a 10 000-character string.
  - `setMidiMixSetsAndKeepsAbsentValues` (Swift): `{dest, volume: "90"}` and then `{dest, pan: "20"}` give 90/20, and `{dest, volume: "none"}` gives unset/20.
  - e2e `midi_mix_reaches_state_get`: add a MIDI destination (`destination.add_midi`), set its mix, and `state.get` shows it.
- [ ] **Step 2: Run** the new tests. Expected: FAIL.
- [ ] **Step 3: Implement** beside `setReturnLatency` in `SynthCommands.swift`, with the same error mapping. Nothing opens a dialog.
- [ ] **Step 4: Run** the core-control tests, `scripts/swift.sh test`, `scripts/make-app.sh` and the e2e. Expected: PASS (registry 164, e2e 22).
- [ ] **Step 5: Commit** `feat(control): destination.set_midi_mix (5g-1b Task 4)`.

### Task 5: The strip's model (Swift, `VizDAWAppModel`)

**Files:**
- Create: `app/Sources/VizDAWAppModel/MidiStrip.swift`
- Modify: `MixerModel.swift`, `PaneContent.swift`
- Test: `app/Tests/VizDAWAppModelTests/MidiStripTests.swift` (new), `MixerModelTests.swift`, the `StripFocus` tests

**Interfaces:**
- Consumes: `DestRow.midiMix`, `DestRow.channel`, `MixerLevels.midi`, `setMidiMix` and `previewMidiMix` (Task 3).
- Produces:
  - `public enum MidiStripMath`:
    - `volumeLabel(_ v: UInt8?) -> String`: "100" when unset, else the number;
    - `panLabel(_ p: UInt8?) -> String`: 64 → "C", 0 → "L 64", 127 → "R 63", unset → "C";
    - `parseVolume(_ s: String) -> UInt8?`: 0–127;
    - `parsePan(_ s: String) -> UInt8?`: accepts "C", "L n" (1–64), "R n" (1–63) and a plain 0–127;
    - `position(_ v: UInt8?) -> Double` and `value(at: Double) -> UInt8`, linear.
  - `public enum MixerColumn: Identifiable, Equatable { case audio(StripRow), midi(DestRow) }`, with IDs that never collide (for example a "s"/"d" prefix). `MixerModel.columns: [MixerColumn]` follows Ruling 7, master excluded as `strips` does.
  - `MixerModel` intents:
    - `midiDrag(_ dest: UInt64, volume: UInt8?, pan: UInt8?, now: Double)`: previews, throttled to 20 ms;
    - `midiRelease(_ dest: UInt64)`: commits the last dragged values once;
    - `setMidiVolume`, `setMidiPan`, `toggleMidiMute(_ dest: UInt64)`;
    - `typeMidiVolume` and `typeMidiPan(_ dest: UInt64, _ text: String) -> Bool`;
    - `midiActivity: [UInt64: Double]`, decayed with `MixerMath.meterDecay` in `pollLevels`;
    - `midiLane(for dest: UInt64, in placement: MixerPlacement) -> LaneRow?`, the same rule as `midiFxLane`, for arm and MIDI FX.
  - `StripFocus.Result.midi(dest: UInt64?, lane: UInt64)`: a focused lane of kind `.midi` gives it, with `dest` set when its destination is external and nil when it has none.

- [ ] **Step 1: Write the failing tests** with the fake session the other MixerModel tests use:
  - `labelsAndParsing`: the table above, plus `parseVolume("128") == nil`, `parsePan("L 65") == nil` and `parsePan("R 64") == nil`.
  - `aDragPreviewsAndCommitsOnce`: three drags 5 ms apart give 1 preview, and one 25 ms later gives 2. Release gives exactly 1 `setMidiMix` with the last value; a second release gives none.
  - `aClickCommitsOnce`: a `setMidiVolume` with no drag gives one commit (5g-1's m1).
  - `typedValuesAreParsedAndRangeChecked`: "200" and "L 70" return false with no session call; "90" and "R 10" commit 90 and 74.
  - `columnsPutMidiStripsBeforeBuses`.
  - `aMidiTrackFocusesItsMidiStrip`: an external dest gives `.midi(dest: d, lane: l)`, no dest gives `.midi(dest: nil, lane: l)`, and an instrument lane is unchanged.
  - `midiActivityDecays`.
- [ ] **Step 2: Run** `scripts/swift.sh test --filter MidiStrip`. Expected: FAIL.
- [ ] **Step 3: Implement** to pass the tests.
- [ ] **Step 4: Run** `scripts/swift.sh build` and `scripts/swift.sh test`. Expected: PASS (about +7).
- [ ] **Step 5: Commit** `feat(app): MIDI strip model (5g-1b Task 5)`.

### Task 6: The views, the README checks, the gates

**Files:**
- Create: `app/Sources/VizDAW/MidiStripCells.swift`
- Modify: `ChannelStrip.swift` (`ChannelStrips` takes `[MixerColumn]`, and the MIDI column's cells come from `MidiStripCells`), `MixerView.swift` (uses `model.columns`), `PaneViews.swift` (`LeftStripPane` handles `.midi`), `app/README.md`
- Test: the name scans, and `scripts/swift.sh test`

**Interfaces:**
- Consumes: Task 5.
- The MIDI column fills these rows of the shared grid and leaves the rest empty, as a strip without sends does:
  - `.header`: the name; in the left pane, a destination menu (Ruling 8) or "No destination";
  - `.midiFx`: `StripMidiFx`, on the `midiLane` lane;
  - `.pan`;
  - `.fader`: the throw, a 0–127 scale (127, 100, 64, 32, 0) and the activity meter;
  - `.level`: the typeable volume box;
  - `.buttons`: M and R.
- Unset values are dimmed. With no destination, pan, the fader and M are disabled.
- Theme tokens only; no `@State`; no dialog when the control channel is connected (5g-1's Task 9 rule).

- [ ] **Step 1: Build the views.** Add README checks 245–250 after 244, in the README's style:
  - 245: a MIDI track playing an external synth shows the MIDI strip in the left pane, and the Mixer shows one per destination before the buses.
  - 246: moving the fader or pan changes the synth as you move. Each drag is one Undo ("Set MIDI Mix"), and typed values work ("90", "R 10").
  - 247: Play from bar 1 sends the strip's level and pan after the patch. A part with its own volume moves still plays them.
  - 248: Mute while a note sounds ends it; controllers still play; unmute plays the next note.
  - 249: the meter shows the velocity of notes going out.
  - 250: a MIDI track with no destination shows "No destination" with pan and fader off, while MIDI FX and arm work. Save, close and reopen: the strip's values are there.
- [ ] **Step 2: Run all the gates:**
  - `cargo fmt --all -- --check`;
  - both clippies and the rt-check gate;
  - `cargo test --workspace`;
  - `scripts/build-ffi.sh`, `scripts/swift.sh build` and `scripts/swift.sh test`;
  - the no-brand build;
  - `scripts/make-app.sh` and the e2e.

  Afterwards `pgrep -lf MacOS/VizDAW` prints nothing. Expected: all PASS.
- [ ] **Step 3: Commit** `feat(app): MIDI strip views and README checks 245–250 (5g-1b Task 6)`.

## Self-Review

- **Spec §6.8a coverage:**
  - model → Task 1;
  - sending, chase and mute → Task 2;
  - the strip's rows, left out items and no-destination → Tasks 5–6;
  - undo per gesture → Tasks 1 and 5;
  - control → Task 4.
- **Names are consistent across tasks:** `MidiMix`, `midi_mix`, `SetMidiMix`, `send_now`, `take_midi_activity`, `MidiMixRow`, `MidiLevelRow`, `set_midi_mix`, `preview_midi_mix`, `MixerColumn`, `StripFocus.Result.midi`.
- **Review Focus:** each line names its tests in the owning task.
