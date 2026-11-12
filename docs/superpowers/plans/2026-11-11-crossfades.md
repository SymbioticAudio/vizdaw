# Crossfades Implementation Plan (6c)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Real crossfades on one track. Where a later region starts inside an earlier one on the same take, the user (or, later, an import) can turn the later region's fade-in into a crossfade, so the earlier region keeps sounding under it on the complementary curve. The feature covers the model, file format 9, the engine, undo, the X handle in the arrangement and the audio editor, typed values, the Region menu and two control-channel commands.

**Architecture:**
- **`core-model`** gains `AudioRegion.crossfade: bool` (format 9) and a module `crossfade`. The module finds each region's *partner* (the region it starts inside) with the engine's own overlap rule. It also provides the `Crossfade` and `RemoveCrossfade` commands and validation.
- A `settle` pass runs at the end of every command. It keeps, shortens or drops each crossfade that the command disturbed, inside the same undo step.
- **`core-engine`** changes the schedule's region segments in one place. Under a crossfading region's fade-in span, it adds a segment of the partner whose gain carries the fade's complement (`Segment.xfade`). The render path stays allocation-free.
- **`core-ffi`** gains the two `AudioEdit` variants, three `EventRow` fields (the flag, the partner and the overlap) and the crossfade in `state.get audio_editor`.
- **Swift** gains the X handle and its drag, shape menu, typed box, inspector row and VoiceOver wording. It also gains Region › Crossfade and Remove Crossfade, a "Default crossfade" preference, and `region.crossfade` and `region.remove_crossfade`.

**Tech Stack:** Rust 2024 (`core-model`, `core-engine`, `core-ffi` with UniFFI 0.32.2, `core-control`, `vizctl`), Swift 6.1 / SwiftPM (Swift Testing), macOS 14+.

**Spec:** `docs/superpowers/specs/2026-11-11-import-export-design.md`. Plan 6c's row of §13 covers:
- §2 goal 5b and §7.0 in full;
- §6.4's version 9;
- §9's Region menu items;
- §10's `region.crossfade` and `region.remove_crossfade` rows and their hostile-input line;
- §12's crossfade bullet, the 6c README checks and the 6c owner check.

Also read:
- `docs/superpowers/specs/2026-10-18-recording-audio-design.md`: §1's overlap rule ("the later start takes over"), §4.2's fades in media frames and §5.3's segments;
- `docs/superpowers/specs/2026-11-03-ui-polish-design.md` and plan `docs/superpowers/plans/2026-11-04-audio-editor.md` (7c): the audio editor's handles, boxes and `state.get audio_editor`.

DAWproject, AAF and OMF use crossfades later (6d, 6f, 6h); this plan adds none of their mapping.

## Global Constraints

Every task's requirements include these.

- **Shell.** Every command runs after `export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools` and `cd /Users/lattawb/workspace/vizdaw`. Run Swift only through `scripts/build-ffi.sh` (after any FFI change), `scripts/swift.sh build|test` and `scripts/make-app.sh`. Never run `swift`, `xcodebuild` or `xcrun` directly.
- **Branch and commits.**
  - Branch `sp6-import` (at `13a84e0` when this plan was written; plans 6a and 6b are finished on it). Never switch branches, never push.
  - Commit at the end of every task by explicit paths (never `git add -A`). Never commit `.superpowers/`, `.DS_Store` or generated FFI (`app/Sources/VizDAWCore/vizdaw_ffi.swift`, `app/Sources/vizdaw_ffiFFI/include/`, `target/ffi/`, `dist/`).
  - `Cargo.lock` does not change: no new crate and no new dependency.
  - Every message ends with the executing session's own attribution lines: its harness's `Co-Authored-By` line naming the model actually running, then the session line, e.g.
    ```
    Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
    Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P
    ```
    Then check with `git log -1 --oneline && git show --stat HEAD` (a commit without `DEVELOPER_DIR` fails silently).
- **Clean room and names.**
  - Never read `~/workspace/SVP/research`.
  - No Opcode, Logic, Apple (except existing platform facts), Midas, Soundcraft, NI, Microsoft, Ableton, Steinberg/Cubase or ROLI names in UI or repo text; say "other DAWs". The MIDI Association, General MIDI and MPE are specification names and are allowed.
  - **Ardour** may be named only in docs, the backlog and third-party licence notices, whose notices are kept. Never name it in code, comments, tests, UI text or README steps.
  - The scans enforce this: `NameScanTests.noThirdPartyNamesInWhatTheAppShows` and `core-control/tests/names.rs`.
- **Track, not lane.** Text the app shows (labels, menus, model refusals, `Report` warnings, the status line, README steps) says "track". Identifiers, command IDs, JSON keys and the file format keep `lane`. `core-ffi/tests/wording.rs` scans the core's messages.
- **Never run `auval`**, in any form.
- **No GUI tools, no real devices.**
  - Never use GUI, computer-use or accessibility-automation tools. The owner checks the views by eye from README steps 274–281.
  - Launch VizDAW only through the e2e (Task 7) with `VIZDAW_AUDIO=offline`. Afterwards `pgrep -lf MacOS/VizDAW` prints nothing and `ls -d /tmp/vze*` prints nothing new.
  - Tests never open real audio or MIDI, never trigger the microphone prompt or TCC, and never touch the real `~/Library/Application Support/VizDAW`, preferences, Trash or devices. Use temp folders (`tempfile`), `MemoryDefaults()`, the e2e's throwaway home and fakes only. Every `core-ffi` test file calls `vizdaw_ffi::host::force_offline()` through its `serial` helper.
- **Programmatic calls open nothing.** Control-channel commands and programmatic calls never open dialogs, sheets, menus, panels or windows. The shape menu and the Settings field open only from the user's own clicks.
- **Hostile input.** Every new command gets hostile-input tests at every layer (Tasks 1, 4 and 5). Every computed tick, frame and length uses `checked_*`/`saturating_*` or `i128`; nothing wraps. A float from text is finite before it becomes frames.
- **FFI safety.** No panic crosses the FFI: every export returns `Result<_, VizError>` inside `ffi_guard` (`catch_unwind`). This plan adds no export; the new `AudioEdit` variants go through the existing `Session::edit`.
- **Swift 6.1.**
  - Generated bindings compile in Swift 5 mode; app modules and tests compile in Swift 6 mode with no new warnings.
  - No `@State` or `@FocusState` anywhere in this plan's code (view state lives in the `@Observable` models).
  - Never capture `[weak self]` or a non-Sendable value inside `MainActor.assumeIsolated`; bind a strong `let` first, as `DocumentWindow.swift` does.
  - Colours, spacing, radii and fonts come from the theme; every new control is accessibility-labelled.
- **Lints.** Every crate keeps `#![deny(missing_docs)]` and `#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic))]`: use `.get()`, iterators and `checked_*`/`saturating_*`. Rust stable, edition 2024, GPL-3.0-only. `core-model`, `core-engine` and `core-control` stay free of OS APIs; everything Rust here runs on Linux.
- **TDD.** Every new test is run and seen to fail before the code that makes it pass.
- **Real-time.** This plan adds real-time code (`Segment::gain_at`'s complement) and changes `EventBody::AudioRegion`. Tasks 1 and 3 run the **rt-check gate**: `cargo clippy -p vizdaw-play --all-targets --features rt-check -- -D warnings` and `cargo test -p core-engine --features rt-check --test rt_alloc --test rt_alloc_graph --test rt_alloc_record --test rt_alloc_gestures --test rt_alloc_input_fx --test rt_alloc_midi_strip --test rt_alloc_bounce --test rt_alloc_mpe`. From Task 3 on, the gate also runs the new `--test rt_alloc_crossfade`. CI's rt-check step (`.github/workflows/ci.yml`) gains `--test rt_alloc_mpe` (missing today) and `--test rt_alloc_crossfade` in Task 3 (F4). Nothing on the render path allocates, locks or does I/O. Segments are built on the control thread.
- **Format.** `core_model::file::VERSION` goes 8 → 9 in Task 1 (spec §6.4), with the no-op `migrate_v8_to_v9` added to `MIGRATIONS` (F5).
- **Lessons from plans 6a and 6b (binding):**
  - **No file I/O or parsing on the main thread.** Every new call is a model edit or a model read. The arrangement and audio editor read `overlap_with` and `overlap_frames` from the rows they already fetch, and never compute an overlap in Swift.
  - **Every dropped item gets a report line.** When an edit drops or shortens a crossfade, `settle` pushes one `Report.warnings` line per region. Today `Session::execute_with` drops the `Report`, so Task 4 carries every edit's warnings in `DocSnapshot.warnings`, and Task 6 shows them in the editor's status line (F1).
  - **Goldens are hand-written before the code and can fail.** The render goldens are literal curve tables typed into `core-engine/tests/crossfades.rs` (Task 3 Step 1). They are never computed with `fade_curve` and never blessed.
  - **Cancel is tested.** This plan adds no job. Its cancel cases are Escape during an X-handle drag (no edit, no undo step) and a refused drag end (the gesture is cancelled) (Task 6).
  - **One job at a time.** Neither command is a job. Both answer `unavailable` while an editor gesture is open, as `region.*` does today.
- **UniFFI changes** (Task 4 only; every new field on an existing record is `#[uniffi(default)]` or `#[uniffi(default = None)]`):
  - `AudioEdit::Crossfade { event, length: CrossfadeLengthRow, shape: FadeShapeRow }` and `AudioEdit::RemoveCrossfade { event }`;
  - the enum `CrossfadeLengthRow { Frames { frames: u64 }, UpTo { frames: u64 } }`;
  - `EventRow` gains `crossfade: bool`, `overlap_with: Option<u64>` and `overlap_frames: u64`;
  - `AudioEditorRegionRow` gains `crossfade: bool` and `crossfade_with: Option<u64>`;
  - `DocSnapshot` gains `#[uniffi(default)] warnings: Vec<String>` (F1).

  Every Swift `switch` over `AudioEdit` gains the two arms in Task 4.
- **Control channel.** The registry goes 171 → 173 in Task 5. Every command is in the registry and handled (`everyRegisteredMenuCommandHasAHandler`).
- **Each task ends green.**
  - Always:
    - `cargo fmt --all` then `cargo fmt --all -- --check`;
    - `cargo clippy --workspace --all-targets -- -D warnings`;
    - the Linux clippy: `cargo clippy --workspace --all-targets --target x86_64-unknown-linux-gnu --target-dir target/linux-check -- -D warnings`;
    - `cargo test` for the crates the task touches. Run the whole workspace in Tasks 2, 4 and 7.
  - Tasks 4–7 also run `cargo clippy -p core-ffi --lib -- -D warnings`, `scripts/build-ffi.sh`, `scripts/swift.sh build` and `scripts/swift.sh test`.
  - Task 7 also runs:
    - the no-brand build: `VIZDAW_NO_BRAND=1 scripts/swift.sh build --scratch-path <a fresh temp dir>`;
    - `scripts/make-app.sh`;
    - the e2e: `VIZDAW_E2E_APP=dist/VizDAW.app cargo test -p vizctl --test e2e -- --ignored --nocapture --test-threads 1`.
  - Known flaky:
    - the `damaged_files` proptest;
    - the MIDI listener tests (`platform-macos/tests/midi.rs`);
    - `core-ffi` catalog tests under parallel load;
    - the latency e2e timing.

    Re-run once before investigating, and say so in the report.
  - **Baseline:** measure at the start of Task 1: Rust passed/ignored, Swift, e2e 25, registry 171, README through 273, `VERSION` 8. Each task's last step gives the counts it adds. Report the measured counts; a slip in the writer's arithmetic is not a failure.
- **How code is given.**
  - New test files are written whole.
  - New implementation files get their public items with doc lines, plus the algorithm where the signature and tests leave a choice.
  - Changes to existing files follow the existing pattern named in the step. If a named anchor no longer matches, make the same change to the current text and say so in the report.

## Review Focus

These are the five failure modes a user is most likely to hit, most likely first. The spec implies them, but no ordinary test would exercise them. Each is pinned by named tests in its task.

1. **An edit that quietly breaks a crossfade's pair.**
   - Examples:
     - the later region moved to another track and dropped inside a different region;
     - a third region dropped so that it starts inside the earlier one, just before the later one starts;
     - only the later region copied onto another region;
     - the earlier region deleted, or trimmed to end inside the span;
     - a tempo change that pulls the two apart.
   - Expected: the crossfade either stays with its own partner (shortened when the overlap shrank) or becomes a plain fade-in. It never joins a different region silently. The status line says what happened, and one Undo restores everything.
   - Pinned by `a_different_partner_drops_it`, `moving_keeps_shrinks_or_drops`, `copies_keep_it_only_with_their_partner` and `tempo_changes_shrink_or_drop` (Task 2), and `every_state_validates` (Task 2 property).
2. **Splitting inside a crossfade.**
   - Examples: Split at the playhead inside the span, on the later region or on the earlier one.
   - Expected: refused with "Split outside the crossfade, or remove the crossfade first."; outside the span, the split works and the second part has no crossfade.
   - Pinned by `split_inside_a_crossfade_is_refused` (Task 2).
3. **A third region inside the span, a muted partner, or a partner cut away by a reference.**
   - Expected: the engine plays the earlier region's complement only where the crossfading region is the one heard, and plays a plain fade-in where the partner is muted or missing. It never plays three regions at once and never clicks at a segment edge.
   - Pinned by `a_third_region_in_the_span_cuts_the_complement`, `a_muted_partner_leaves_a_plain_fade_in` and `crossfades_repeat_in_each_pass_of_a_looped_track` (Task 3).
4. **Hostile lengths from the channel and the typed box.**
   - Examples: `length_ms` "0", "-5", "NaN", "inf", "1e400", "", or longer than the overlap; "1.5 frames" and "-3 frames" in the box; `event` that is a note, unknown, on another track's take, or a region that starts inside nothing.
   - Expected: `invalid_params` for bad text (never reaching the session) and `failed` with the core's one sentence for a value the model refuses. Nothing changes, and no undo step is added.
   - Pinned by `crossfadeChannelRefusesHostileArguments` (Task 5), `fadeFramesReadsMsSecondsAndFrames` (Task 6) and `hostile_crossfade_edits_are_refused` (Task 4).
5. **Old projects with overlapping regions.**
   - Expected: a version-8 project opens with every overlap still a hard takeover, with no implicit crossfade, sounding exactly as before. It saves as version 9 with no `crossfade` key on any region.
   - Pinned by `version_8_projects_migrate_to_version_9` (Task 1) and `a_region_without_a_crossfade_renders_as_before` (Task 3).

## Plan Rulings

These are this plan's decisions where the spec leaves a choice. Each names its cost if wrong.

1. **What a crossfade is (spec §7.0, kept).** It is a flag on the *later* region: `crossfade: true` makes its `fade_in` the crossfade. There is one shape, the fade-in's; the earlier region follows the complement of that shape.
   - There is no region-pair object and no partner ID in the file. The partner is found by rule, the same one the engine uses to decide what plays. On the later region's take, the partner is the region with the latest `(tick, id)` that starts *strictly* before it in ticks and ends after its start in frames (Ruling 4).
   - *Cost if wrong:* storing a partner ID would be a format-10 field and a migration. Per-side shapes would need a second `Fade` field.
2. **Old projects (spec §6.4).** `migrate_v8_to_v9` is a no-op. `crossfade` is `#[serde(default, skip_serializing_if = "std::ops::Not::not")]`, so a version-8 project's overlaps load with no crossfade and the later region still takes over. A region without a crossfade writes the same JSON as in version 8.
   - *Cost if wrong:* none to the file; an "add crossfades to every overlap" command would be new work.
3. **What creates, resizes and removes one.**
   - **Creates:**
     - `AudioCmd::Crossfade`, from Region › Crossfade, its key, `region.crossfade`, and later from imports;
     - an inserted pair that carries the flag (paste, Duplicate, an import batch; Ruling 6). Both regions must arrive in **one** command: `AudioCmd::Insert` has no flag, so 6d/6f insert the pair as two specs in one `EventCmd::Insert` (F22).
   - **Never creates:** dragging regions to overlap, recording, trimming or splitting. There is no auto-crossfade setting (spec §7.0, SP5 "no automatic crossfades").
   - **Resizes:**
     - dragging the X handle;
     - typing in the audio editor's Crossfade box;
     - `region.crossfade` with `length_ms`;
     - an edit that shrinks the overlap (Ruling 6).
   - **Removes:**
     - Region › Remove Crossfade, the shape menu's Remove Crossfade, and `region.remove_crossfade`. The flag clears and the fade-in stays as a plain fade of the same length and shape;
     - an edit that ends the overlap or changes the partner (Ruling 6).
   - The spec's `AudioCmd::Crossfade { event, length_frames, shape }` becomes `length: CrossfadeLength`:
     - `Frames(n)` is exact, and refused past the room;
     - `UpTo(n)` is `min(n, room)`, for the menu's "whole overlap, or the default when it is longer".
   - *Cost if wrong:* one enum.
4. **Room, minimum, maximum and default.**
   - **Overlap.** At the later region's media rate `r`, `frame(t) = round(tempo.tick_to_seconds(t) × r)`. The overlap is `frame(P.tick) + P.length × r / P.rate − frame(R.tick)`, rounded, in `i128`, and positive only. The engine pairs by the model's partner `EventId` (Task 3, F9) and clamps the span to the partner's real end, so model and engine agree on *who* pairs. At the project rate (media is imported at it) they agree on the frames too; with a start offset or another media rate they may differ by one frame, which only shortens the audible blend by that frame (F12).
   - **Room** is `min(overlap, R.length − R.fade_out.length_frames)`, so the region's fades still fit it.
   - **Minimum** 1 frame (spec). **Maximum** is the room; there is no absolute cap.
   - **Default length:** an app preference, Settings › Editing › "Default crossfade", 10 ms, from 1 to 1 000 ms (spec: "Settings, 10 ms"). It is converted at the region's media rate and sent as `UpTo`.
   - **Default shape** (`crossfade::DEFAULT_SHAPE`): Equal Power for Region › Crossfade, its key and `region.crossfade` without `shape`. Equal power keeps the level of two unrelated sounds steady, and Linear is one menu click away. Imports keep the file's shape.
   - *Cost if wrong:* a 3 dB bump on crossfades between correlated takes, until the user picks Linear; one constant.
5. **Which regions the menu acts on.** It acts on one selected region whose `overlap_with` is set, or on two selected regions on one track where one's `overlap_with` is the other (the later one is crossfaded). Anything else is unavailable: "Select two overlapping audio regions on one track, or the later one."
   - `region.crossfade` and `region.remove_crossfade` take `seq`, `lane` and `event` all together or not at all. Without them they act on the selection, like the menu. This is how keys and surfaces call them, with no arguments.
   - With them, `event` must be a region on the active take of `lane` in `seq` (else `invalid_params`).
   - *Cost if wrong:* spec §10 lists the three as plain parameters; making them required would break keys.
6. **Edits that disturb a crossfade (spec §7.0 "keeps or drops… inside the same undo step").** `crossfade::settle(old, &mut new, &mut report)` runs at the end of `apply_in` for every command.
   - It visits:
     - each take whose `events` map is not `ptr_eq` to the same take in `old`;
     - every take of a sequence whose `tempo` changed;
     - every take of a new sequence.

     For each region `R` with `crossfade` in a visited take:
     1. No partner, or `R.fade_in.length_frames == 0`: clear the flag. Warning: `Crossfade removed: region '<name>' no longer starts inside another region on its track.`
     2. `R` was in `old` with `crossfade: true`, and its partner then had a different `EventId` from now (a move to another track, a new region in between, or a deleted partner): clear the flag. Warning: `Crossfade removed: region '<name>' now starts inside a different region.`
        **Exception, a split partner (F2):** the new partner is new in this command, the old partner still exists in the take and ends exactly where the new one starts (in frames), with the same media and `source_offset` continuing (`old.source_offset + old.length == new.source_offset`). That is the second part of a Split of the old partner, the sound is unchanged, and the crossfade stays with no warning.
     3. `R` is new (not in `old`) and its partner is not new: clear the flag, without a warning (a lone copy never joins an old region).
     4. Otherwise, if `R.fade_in.length_frames > room`, shorten it to the room. Warning: `Crossfade on region '<name>' shortened to fit the overlap.`

     `<name>` is the region's name, else its media's name.
   - Moving both regions together keeps the crossfade unchanged.
   - Shortening, rather than dropping, follows a trim of the earlier region's end, as other DAWs do.
   - **Cost (F11):** a visited take is first scanned for a flagged region, in `old` or `new`, with no allocation. A take without one is skipped before any sweep, so note edits in a 100 000-note take pay one linear scan.
   - *Cost if wrong:* dropping instead of shortening is one branch; a slower settle shows in Task 2's bench and Task 4's big-lane test.
7. **Split** (`AudioCmd::Split`) refuses a point strictly inside a crossfade span `(R.start, R.start + fade_in)`, on `R` or on its partner, with "Split outside the crossfade, or remove the crossfade first."
   - Otherwise the second part gets `crossfade: false`.
   - *Reason:* a split inside the span would end the complement with a click, and splitting the partner there starts a region that takes over.
   - *Cost if wrong:* splitting into two shorter crossfades is a design of its own.
8. **The other interactions.**
   - **Region fades:** the partner's own fade-out applies on top of the complement (spec). `SetFades`, `FadeTo` and `RemoveFades` on `R` act on its fade-in, and settle then clears or shortens the crossfade. Remove Fades on a crossfading region clears both, so the crossfade goes, with a warning.
   - **Trim** is Ruling 6.
   - **Mute:** a muted partner keeps the flag, and the engine plays a plain fade-in, because flatten drops muted events. The engine never pairs with an older region instead, because it pairs only with the model's partner by `EventId` (F9).
   - **Nested sequences:** a crossfade joins two regions on one take of one sequence, and every use of that sequence plays it. The engine pairs within each lane instance. A partner a reference window cuts away gives a plain fade-in, and a region in a referenced sequence never pairs with one in its parent (different takes).
   - **Takes:** each take's flags are its own; switching the active take changes nothing.
   - **The 6a backlog row "Zero-crossing snapping and edge fades for audio edits"** is not done here. When it lands, a crossfaded start counts as the region's own fade (no edge fade), and the X drag snaps nothing. Task 7 adds that note to the row.
   - *Cost if wrong:* rule text and a test each.
9. **What the user sees.**
   - **Arrangement and audio editor:** the X is two lines across the full height from the region's start to the end of the fade-in, drawn in `fadeLine`. Its handle replaces the fade-in corner at the same place.
   - **Drag:** the handle drags between 1 and the room, with the readout "<n> ms".
   - **Shape menu** of the crossfade area: "Crossfade: Linear", "Crossfade: Equal Power", "Crossfade: Exponential", "Crossfade: S-Curve" (current one ticked), then "Remove Crossfade".
   - **Audio editor:** the Crossfade box and shape picker replace Fade In while the region has a crossfade.
   - **Inspector:** a "Crossfade" row, "<ms> ms, <Shape>", replaces "Fade In".
   - **VoiceOver:** the region's label says ", crossfade".
   - **Undo labels:** "Crossfade" (making, resizing or reshaping one) and "Remove Crossfade".

## Pre-flight amendments (2026-11-11)

These come from `.superpowers/sdd/2026-11-11-crossfades/preflight-report.md`, and the controller accepted every proposed ruling. Each is applied in place in the task it touches; where a step and an older line disagree, the amendment wins.

- **F1** Edit warnings reach the user: `DocSnapshot.warnings` from `execute_with`, `edit_all` and the gesture end (Task 4), shown in the status line by `DocumentModel.apply` (Task 6); tests at both layers.
- **F2** Splitting the earlier region before the later one starts keeps the crossfade (Ruling 6 rule 2's split-partner exception); test `splitting_the_partner_before_the_span_keeps_it` (Task 2).
- **F3** The two IDs join `RegionCommands.ids`, and `run` and `enabled` handle them first, so they never fall into Fade Out from Cursor; test `crossfadeIdsNeverRunFadeTo` (Task 5).
- **F4** `ci.yml`'s rt-check step gains `rt_alloc_mpe` and `rt_alloc_crossfade` (Task 3).
- **F5** Task 1 lists every `VERSION == 8` assert (`file.rs`, `learn.rs`, `input_fx.rs`, `mpe.rs`), the `file.rs` doc line and the `MIGRATIONS` entry.
- **F6** C in `overlaps_follow_the_engine_rule` is 10 000 frames long, so it ends before E starts (Task 1).
- **F7** A model test drives extreme ticks, trims, lengths and tempos beside a flagged pair: no panic, valid, keep or warn (Task 2).
- **F8** The shrinking move uses +13 056 ticks (exactly 20 400 frames, overlap 3 600) (Task 2).
- **F9** The engine pairs only with the model's partner `EventId`, so a muted partner never lets an older region pair; test `a_muted_partner_never_pairs_with_an_older_region` (Ruling 8, Task 3).
- **F10** Split edge cases: 1 frame inside the span is refused, exactly at its end works (Task 2).
- **F11** Settle skips a take with no flagged region before any sweep; `lane_events` sweeps only takes with two or more regions; the existing 100 000-event lane test runs in Task 4 (Ruling 6).
- **F12** Ruling 4 says where model and engine agree; tests with a start offset and with regions one tick apart (Task 3).
- **F13** No change to `gain_at`'s cost (a `powf` per sample for Exponential, as fades already do); the no-crossfade golden is a flat 0.25 after frame 800; plus a version-8 pair loaded from JSON renders with no extra segment (Task 3).
- **F14** The 2 048-case random test runs on documents at the model level (`apply`); the session test keeps 64 cases, reset by undo (Task 4).
- **F15** The registry help says "all three or none", and `{seq}` alone validates at the registry (Task 5).
- **F16** The existing `fadeHint` assertions (`AudioEditorEntryTests.swift` ~99 and ~104, `AudioEditorModelTests.swift` ~150) are updated first and seen to fail (Task 6).
- **F17** Task 6 adds `AudioEditorHandle.regionHandle`'s `.crossfade` arm, `RegionHandles.target`'s `.crossfade` arm and `Int64(clamping:)` in `crossfadeRoom`.
- **F18** The e2e bounce passes `dither: false` (Task 7).
- **F19** The e2e gains `audio_file_level(path, frames, level, channels)`; `audio_file` and its callers are unchanged (Task 7).
- **F20** The e2e uses mono CAFs (one track per import), keeps Linear for the midpoint check, and also checks an Equal Power midpoint (Task 7).
- **F21** Ruling 6's rules 2 and 3 (dropping on a different partner or a lone copy) join the owner questions (Self-Review).
- **F22** Ruling 3 and a backlog row record that an import inserts a crossfaded pair in one command (Task 7).

---

### Task 1: `core-model`: format 9, the flag, partners, the two commands and validation

**Files:**
- Create: `crates/core-model/src/crossfade.rs`
- Modify:
  - `crates/core-model/src/lib.rs` (the module);
  - `src/event.rs` (`AudioRegion.crossfade`);
  - `src/command/audio.rs` (`Crossfade`, `RemoveCrossfade`, `CrossfadeLength`, names; `Split` sets `b`'s flag false);
  - `src/command/mod.rs` (re-export `CrossfadeLength`);
  - `src/rules.rs` (`validate` extends with `crossfade::problems`);
  - `src/file.rs` (`VERSION = 9`, the module doc's `"version": 8` → 9, `migrate_v8_to_v9` appended to `MIGRATIONS`; F5).

  Every other `EventBody::AudioRegion { … }` literal gains `crossfade: false`; the compiler lists them in `core-model`, `core-engine`, `core-io`, `core-ffi` and the tests.
- Test:
  - `crates/core-model/tests/crossfades.rs` (new);
  - `crates/core-model/tests/file.rs`: every `assert_eq!(VERSION, 8)` (about lines 658, 676, 695) becomes 9, plus the new tests;
  - `crates/core-model/tests/learn.rs` (~211), `input_fx.rs` (~262) and `mpe.rs` (~267): their `VERSION == 8` asserts become 9 (F5).

**Interfaces:**
- Produces:
  - ```rust
    // event.rs, EventBody::AudioRegion
    /// The fade in is a crossfade with the region this one starts inside (plan 6c).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    crossfade: bool,
    ```
  - ```rust
    // crossfade.rs
    pub const DEFAULT_SHAPE: FadeShape = FadeShape::EqualPower;            // Ruling 4
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Overlap { pub with: EventId, pub frames: u64 }              // Ruling 4's overlap
    /// Each region of `take` that starts inside another, by Ruling 1's rule.
    pub fn overlaps(doc: &Document, seq: SeqId, take: &Take) -> BTreeMap<EventId, Overlap>;
    /// The overlap of region `event` (Err for an unknown event or a non-region).
    pub fn overlap(doc: &Document, event: EventId) -> Result<Option<Overlap>>;
    /// `min(overlap, length − fade_out)`; 0 without an overlap.
    pub fn room(doc: &Document, event: EventId) -> Result<u64>;
    /// One line per crossfade that breaks the rules (for `validate`).
    pub(crate) fn problems(doc: &Document) -> Vec<String>;
    ```
    `overlaps` is a sweep in take order over the take's regions. It keeps the regions still sounding at the current start (dropping those that end at or before it, in frames). The partner is the last one kept whose tick is strictly earlier. `problems` reports, naming `crossfade` and the event:
    - a flagged region with no partner;
    - a flagged region whose `fade_in` is 0 or longer than the room.
  - ```rust
    // command/audio.rs
    pub enum CrossfadeLength { Frames(u64), UpTo(u64) }
    AudioCmd::Crossfade { event: EventId, length: CrossfadeLength, shape: FadeShape }   // "Crossfade"
    AudioCmd::RemoveCrossfade { event: EventId }                                        // "Remove Crossfade"
    ```
    The refusals, each `ModelError::invalid` with this text:
    - not a region: the existing `region()` error;
    - no overlap: "This region does not start inside another region on its track.";
    - `Frames(0)` or `UpTo(0)`: "A crossfade is at least 1 frame long.";
    - `Frames(n)` over the overlap: "A crossfade of {n} frames runs past the end of the region it crosses ({overlap} frames).";
    - `Frames(n)` over `length − fade_out`: "A crossfade of {n} frames does not fit the region with its fade out ({m} frames).";
    - `RemoveCrossfade` on a region without one: "This region has no crossfade.".

    `Crossfade` sets `fade_in = Fade { length, shape }` and `crossfade = true`; `RemoveCrossfade` clears the flag only.
  - `file::migrate_v8_to_v9(v: Value) -> Result<Value>`, the identity, documented like `migrate_v7_to_v8`.

- [ ] **Step 1: Measure the baseline** (Global Constraints) and write it in the task report.
- [ ] **Step 2: Write the failing tests.** The fixture `doc()` in `tests/crossfades.rs`:
  - sequence 0 at 120 BPM with lane 0;
  - media 0, `Media/a.caf`, 48 kHz, 192 000 frames;
  - region A at tick 0, offset 0, 48 000 frames;
  - region B at tick 15 360 (frame 24 000), offset 48 000, 48 000 frames, so the overlap is 24 000;
  - a note at tick 0.

  The tests:
  - `crossfade_sets_the_fade_in_and_the_flag`: `Crossfade(B, Frames(4800), EqualPower)` gives B `fade_in {4800, EqualPower}` and `crossfade: true`. Through a `History`, `undo_label` is "Crossfade", and one undo gives the fixture back.
  - `up_to_takes_the_overlap_or_less`:
    - `UpTo(480)` gives 480, and `UpTo(100_000)` gives 24 000;
    - with B's `fade_out` at 30 000, `UpTo(100_000)` gives 18 000;
    - `room(B)` equals the last value.
  - `crossfade_refusals`. Each is `Err` with the sentence above, and the document is unchanged:
    - `Frames(0)`, `UpTo(0)`, `Frames(24_001)`;
    - `Frames(20_000)` with B's fade-out at 30 000;
    - A itself (starts inside nothing), the note, `EventId(999)`;
    - a region C at tick 0 like A (equal ticks: not inside);
    - a region D on a second take (`LaneCmd::AddTake`) overlapping where A lies on take 1.
  - `remove_crossfade_leaves_a_plain_fade_in`: after a crossfade, `RemoveCrossfade(B)` gives `crossfade: false` and the same `fade_in`, with undo label "Remove Crossfade". A second `RemoveCrossfade` is refused.
  - `overlaps_follow_the_engine_rule`. Add C inside both A and B (tick 20 000, 10 000 frames, so it ends at frame 41 250; F6), and E starting exactly where A ends (tick 30 720):
    - `overlaps` gives B→A (24 000), C→B and E→B (E starts where A ends, so not inside A), and nothing for A;
    - with a tempo of 60 BPM, B→A is 48 000 − 48 000 = 0, which is not an overlap (B starts at A's end).
  - `validate_names_a_damaged_crossfade`: the fixture's JSON with `"crossfade": true` on A, and then on B with `fade_in` 0, each `from_json` as `Corrupt`, with a problem containing `crossfade`.
  - `file.rs`:
    - `version_8_projects_migrate_to_version_9`: an overlapping pair saved with `"version": 8` loads equal, with no region's `crossfade` set. `to_json` writes `"version": 9` and no `"crossfade"` key, and `migrate_v8_to_v9(v) == v`.
    - `crossfade_survives_save_and_load`: `"crossfade": true` appears once and the document loads equal.
- [ ] **Step 3: Run** `cargo test -p core-model --test crossfades --test file`. Expected: fail to compile.
- [ ] **Step 4: Implement** the items above. `Split` copies the flag to its first part (same ID) and sets the second part's to false. Ruling 7's refusal comes in Task 2.
- [ ] **Step 5: Run** `cargo test --workspace`, both clippies and the rt-check gate. Expected: PASS (about +9 Rust tests; existing goldens unchanged, since the field is skipped when false).
- [ ] **Step 6: Commit** `feat(model): crossfade flag, partners, Crossfade and Remove Crossfade, format 9 (plan 6c Task 1)` with every file above.

### Task 2: `core-model`: edits keep, shorten or drop crossfades (settle), split, and the undo property

**Files:**
- Modify:
  - `crates/core-model/src/crossfade.rs` (`settle`);
  - `src/command/mod.rs` (`apply_in` calls `settle` before building `Applied`);
  - `src/command/audio.rs` (Ruling 7's refusal in `Split`).
- Test:
  - `crates/core-model/tests/crossfades.rs` (new cases);
  - `crates/core-model/tests/properties.rs` (new ops, a media item in `seed`, a new property).

**Interfaces:**
- Consumes: Task 1.
- Produces: `pub(crate) fn settle(old: &Document, new: &mut Document, r: &mut Report) -> Result<()>`, exactly Ruling 6. The warnings use "track", never "lane". It changes events only through `replace_event`.

- [ ] **Step 1: Write the failing tests** in `tests/crossfades.rs`, on Task 1's fixture with `Crossfade(B, Frames(4800), Linear)` applied first:
  - `moving_keeps_shrinks_or_drops`. With `EventCmd::Move` of B:
    - by −1 920 ticks: the length is unchanged and there is no warning;
    - by +12 288 ticks (frame 43 200, overlap 4 800): unchanged;
    - by +13 056 ticks (exactly 20 400 frames, overlap 3 600; F8): `fade_in` 3 600, the flag kept, one warning containing "shortened";
    - by +16 000 ticks (past A's end): the flag false, `fade_in` 4 800 kept, one warning containing "removed".

    Each case is one undo step back to the start.
  - `moving_both_together_keeps_it`: moving A and B by 2 bars, or to lane 1 together (`to_lane`), keeps the flag and the length, with no warning.
  - `trims_shrink_it`:
    - `Trim(A, back_frames: 22_000)` (overlap 2 000) gives `fade_in` 2 000;
    - `Trim(B, front_frames: 23_000)` (overlap 1 000) gives 1 000;
    - `Trim(A, back_frames: 24_000)` (A ends at B's start) clears the flag.
  - `a_different_partner_drops_it`:
    - deleting A clears B's flag;
    - inserting C at tick 14 000 for 10 000 frames (inside A, before B, covering B's start) clears it with the "different region" warning;
    - moving B alone to lane 1, where a long region covers its new start, clears it.
  - `copies_keep_it_only_with_their_partner`:
    - `EventCmd::Duplicate` of A and B by 4 bars gives a copied B with the flag;
    - duplicating B alone onto A's later copy gives a copy without it, and no warning;
    - `EventCmd::Insert` of the two specs (B's flagged) into an empty lane keeps it.
  - `fade_edits_settle_it`:
    - `SetFades(B, fade_in 0)` and `RemoveFades([B])` each clear it with a warning;
    - `SetFades(B, fade_in 30_000)` shortens it to 24 000.
  - `tempo_changes_shrink_or_drop`: `SequenceCmd::SetTempo` to 240 BPM (B's start moves to frame 12 000, A still ends at 48 000) keeps 4 800. A map that makes B start after A's end clears it.
  - `split_inside_a_crossfade_is_refused`:
    - `Split(B, at: Tick(16_000))` (frame 25 000) and `Split(A, at: Tick(16_640))` (frame 26 000) are refused with Ruling 7's sentence;
    - `Split(B, at: Tick(25_600))` (frame 40 000) works, and the new part's flag is false;
    - edges (F10): a split 1 frame inside the span (frame 24 001, at its tick) is refused; one exactly at the span's end (frame 28 800, tick 18 432) works, and B keeps its crossfade.
  - `splitting_the_partner_before_the_span_keeps_it` (F2): `Split(A, at: Tick(7_680))` (frame 12 000) leaves B's flag and length, with no warning; A's second part is B's partner. One undo restores the fixture.
  - `extreme_edits_beside_a_crossfade_never_panic` (F7), beside the flagged pair: `Move` of B and of A by ±2⁴⁰ ticks, `Trim` by ±`i64::MAX / 2`, an `EventCmd::Insert` of a region of `u64::MAX / 2` frames (refused by its media, not by a panic), and `SetTempo` at the minimum and maximum BPM the tempo map allows. Each is `Ok` or `Err` without a panic; after each `Ok`, `validate` is empty and B either keeps its crossfade or the report has its warning.
  - `settle_leaves_a_valid_document_untouched`: an `EventCmd::SetMute` on the note returns the crossfade unchanged, with no warning.
  - `properties.rs`:
    - `seed` adds one media item (48 kHz, 480 000 frames);
    - new ops (weights 3, 2, 1 and 2):
      - `Region { seq, lane, beat 0..16, frames 1_000..60_000 }` (`AudioCmd::Insert`);
      - `Crossfade { ev, frames 0..30_000, up_to: bool, shape }`;
      - `RemoveCrossfade { ev }`;
      - `Trim { ev, front -5_000..5_000, back -5_000..5_000 }`;
    - a new property `every_state_validates(ops)`: after every applied op, `rules::validate(h.doc())` is empty.

    The existing undo/redo and save/load properties now cover the two commands (spec §12).
- [ ] **Step 2: Run** `cargo test -p core-model --test crossfades --test properties`. Expected: FAIL.
- [ ] **Step 3: Implement** `settle` and the Split refusal. In `apply_in`, call `crossfade::settle(doc, &mut d, &mut report)?` after the command's `apply`, for every command variant.
- [ ] **Step 4: Run** `cargo test --workspace` and both clippies. Then measure `cargo bench -p core-model` against the baseline. Expected: PASS, with no bench over 10 % slower (re-run once before investigating, since laptop noise can exceed that); give the numbers in the report.
- [ ] **Step 5: Commit** `feat(model): edits keep, shorten or drop crossfades in the same undo step (plan 6c Task 2)`.

### Task 3: `core-engine`: the complement under the span, golden renders, the null test, rt-check

**Files:**
- Modify:
  - `crates/core-engine/src/schedule/audio.rs` (`Segment.xfade`, `Segment.xfade_from`, `gain_at`, the pairing in `build`);
  - `crates/core-engine/tests/schedule_audio.rs` (the `Segment` literal at `segment_gain_combines_fades` gains the two fields; new cases);
  - `.github/workflows/ci.yml`: the rt-check step gains `--test rt_alloc_mpe --test rt_alloc_crossfade` (F4).
- Create:
  - `crates/core-engine/tests/crossfades.rs`;
  - `crates/core-engine/tests/rt_alloc_crossfade.rs`.

**Interfaces:**
- Consumes: Task 1 (`AudioCmd::Crossfade` through `Builder::run`).
- Produces:
  - ```rust
    /// While a later region crossfades over this one: the complement of its
    /// fade in, from media frame `xfade_from` (length 0: none; plan 6c).
    pub xfade: Fade,
    /// Media frame where the complement starts.
    pub xfade_from: u64,
    ```
    `gain_at(f)` multiplies by `fade_curve(xfade.shape, 1 − x)` when `xfade.length_frames > 0` and `f ≥ xfade_from`, where `x = min((f − xfade_from) / len, 1)`. That is 1 − x for Linear, cos against sin for Equal Power, and the mirror for the other two (spec §7.0). It stays `// RT`: no allocation and no branch on anything but the segment.
  - **Pairing in `build`** (control thread). `Use` gains `raw_start: i64` (the signed frame before the cut at 0), `crossfade: bool` and `fade_in`. After a group's winner pieces are flushed, for each piece won by a crossfading use `u`:
    1. Intersect the piece with the span `[u.raw_start, u.raw_start + fade_in)`.
    2. Find the partner `p` among the group's other uses: the use whose region event (`r.path.last().event`) is the model's partner, `core_model::crossfade::overlap(doc, u's event)`'s `with`, memoised per take. It must also satisfy `p.raw_start < u.raw_start < p.raw_start + p.frames`; if several uses of that event qualify (loops), take the max `(start, order)`. No other region is ever a partner (F9).
    3. Clip the intersection to `p`'s end, and push a copy of `p.seg` over it, with `xfade = u.fade_in`, `xfade_from = p.region_offset + (u.raw_start − p.raw_start)` and `file_frame` moved to the piece's start.

    No partner means no extra segment. `needs()` then includes the extra segments without change.

- [ ] **Step 1: Write the failing tests** (hand-written goldens first). In `tests/crossfades.rs`, copy `graph()`, `schedule()` and `streamer()` from `tests/graph_regions.rs`, and add `dc(path, frames, level)`, which writes a mono 48 kHz float CAF of one level.
  - The layout at 120 BPM (1 tick = 1.5625 frames):
    - A, DC 0.5, at tick 0 for 1 200 frames;
    - B, DC 0.25, at tick 512 (frame 800) for 1 000 frames, with `Crossfade(B, Frames(400), shape)`.
  - The literal tables, at x = 0, ¼, ½, ¾:
    ```rust
    const IN:  [(FadeShape, [f32; 4]); 4] = [
        (Linear,      [0.0, 0.25,      0.5,       0.75]),
        (EqualPower,  [0.0, 0.3826834, 0.7071068, 0.9238795]),
        (Exponential, [0.0, 0.004628,  0.0306534, 0.1770049]),
        (SCurve,      [0.0, 0.2429802, 0.7071068, 0.9700313]),
    ];
    const OUT: [(FadeShape, [f32; 4]); 4] = [
        (Linear,      [1.0, 0.75,      0.5,       0.25]),
        (EqualPower,  [1.0, 0.9238795, 0.7071068, 0.3826834]),
        (Exponential, [1.0, 0.1770049, 0.0306534, 0.004628]),
        (SCurve,      [1.0, 0.9700313, 0.7071068, 0.2429802]),
    ];
    ```
  - The tests:
    - `golden_renders_for_each_shape`: for each shape, the left output at frames 800, 900, 1 000 and 1 100 is `0.5·OUT[k] + 0.25·IN[k]` (±1e-5). It is 0.5 at frames 0 and 799, and 0.25 at frames 1 200 and 1 799.
    - `crossfade_beside_a_plain_fade_out`: A also has a 200-frame linear fade-out. With Linear, frame 1 000 is 0.375, frame 1 100 is 0.25 and frame 1 150 is 0.234375 (±1e-6).
    - `null_test_matches_a_hand_mix`: A and B are the ramp media of `graph_regions.rs` (`value(f)` and its negative), with Equal Power over 400 frames. For every frame 0…1 800, the engine equals `A(f)·cos(π/2·x) + B(f)·sin(π/2·x)` inside the span, and the single region outside it. `x` and the hand mix are computed in `f64` with no call to `fade_curve`; the largest difference is under 1e-6.
    - `a_region_without_a_crossfade_renders_as_before`: the same layout without the command gives 0.5 up to frame 799, then a flat 0.25 (B has no fade-in of its own; F13).
    - `a_version_8_pair_loads_and_renders_as_before` (F13): the overlapping pair saved as JSON with `"version": 8` loads through `file::from_json` and compiles with no segment whose `xfade.length_frames` is non-zero.
  - `schedule_audio.rs`:
    - `a_crossfade_plays_the_earlier_region_under_the_span`: the segments are A `[0, 800)`, A `[800, 1200)` with `xfade {400, Linear}` and `xfade_from` 800, and B `[800, 1800)`. The plain segments have `xfade.length_frames == 0`.
    - `a_third_region_in_the_span_cuts_the_complement`: with C at frame 1 000 for 100 frames, A's extra segments are exactly `[800, 1000)` and `[1100, 1200)`.
    - `a_muted_partner_leaves_a_plain_fade_in`: A muted gives no extra segment.
    - `a_muted_partner_never_pairs_with_an_older_region` (F9): with Z at frame 0 for 2 000 frames under A, and A muted, there is still no extra segment (Z is not B's partner in the model).
    - `pairs_with_a_start_offset_and_one_tick_apart` (F12): with a sequence start offset of one beat, the extra segment's span is 400 frames ±1 and ends at A's end; with B one tick after A's start (inside A), B pairs with A.
    - `crossfades_repeat_in_each_pass_of_a_looped_track`: in a lane looped at 2 000 frames, every pass has its own A extra segment at its own `xfade_from`.
    - `segment_gain_applies_the_complement`: a literal segment with `xfade {400, Linear}` from 800 gives `gain_at` 1.0, 0.75 and 0.5 at 800, 900 and 1 000; with a fade-out, the two multiply.
  - `rt_alloc_crossfade.rs`, as `rt_alloc.rs`'s regions phase:
    - two CAF files and two regions on one track with `Crossfade(Frames(4800), EqualPower)` across the overlap;
    - a loop range around the span and locates every 500 blocks;
    - no allocations while armed, and output in more than half the blocks.
- [ ] **Step 2: Run** `cargo test -p core-engine --test crossfades --test schedule_audio` and `cargo test -p core-engine --features rt-check --test rt_alloc_crossfade`. Expected: FAIL.
- [ ] **Step 3: Implement** the items above, and the `ci.yml` line (F4). Say in the report that `gain_at`'s cost is unchanged in kind: a `powf` per sample for an Exponential crossfade, as for fades (F13).
- [ ] **Step 4: Run** `cargo test -p core-engine`, both clippies and the rt-check gate with `rt_alloc_crossfade`. Expected: PASS (about +10 tests). Every existing render test passes unchanged.
- [ ] **Step 5: Commit** `feat(engine): crossfades play the earlier region under the span (plan 6c Task 3)`.

### Task 4: `core-ffi`: the edits, the rows and the audio editor's state

**Files:**
- Modify:
  - `crates/core-ffi/src/edit.rs` (`AudioEdit` variants, `CrossfadeLengthRow`, `audio_cmd`);
  - `src/events.rs` (`EventRow` fields; `lane_events` fills them from `crossfade::overlaps` of the active take; `spec` carries `crossfade`);
  - `src/control.rs` (`AudioEditorRegionRow` fields; `audio_editor_json` adds `"crossfade"`);
  - `src/snapshot.rs` and `src/session.rs` (F1): `DocSnapshot.warnings`. `execute_with` (session.rs ~1064), `edit_all` and the gesture's final step keep `history.execute`'s `Report.warnings` in `Inner.last_warnings`. The snapshot those calls return carries them; every other snapshot (`snapshot()`, undo, redo, `idle`) has `[]`. Normalize keeps its own `news.problems` path;
  - `app/Tests/VizDAWAppModelTests/FakeSession.swift` (records the new edits like the others); every Swift `switch` over `AudioEdit`.
- Test: `crates/core-ffi/tests/crossfades.rs` (new; `serial` with `force_offline`), `crates/core-ffi/tests/control.rs`

**Interfaces:**
- Consumes: Tasks 1–2.
- Produces: the UniFFI items in Global Constraints. In `audio_editor_json`, the region's `"crossfade"` is `null`, or `{"frames": fade_in.length, "shape": <fade_shape_text>, "with": crossfade_with}`. `EventRow.overlap_with` and `overlap_frames` come from `Overlap` for every region of the active take; they are `None` and 0 otherwise.

- [ ] **Step 1: Write the failing tests** in `tests/crossfades.rs`. The fixture is Task 1's layout built through `Session::edit` on a saved temp package with generated CAF media.
  - `lane_events_show_overlaps_and_crossfades`: B's row has `overlap_with == Some(A)`, `overlap_frames == 24_000` and `crossfade == false`. A's row has `None` and 0. After the edit, B's row has `crossfade == true` and `fade_in.length == 4_800`.
  - `crossfade_edit_is_one_undo_step`: `snapshot().undo_label == Some("Crossfade")`. One `undo` makes `debug_document_json` equal to before. `RemoveCrossfade` gives "Remove Crossfade".
  - `hostile_crossfade_edits_are_refused`. Each is `Err(VizError::Model)` (never `Internal`) and leaves `debug_document_json` unchanged:
    - `Frames { u64::MAX }`, `Frames { 0 }`, `UpTo { 0 }`;
    - event `u64::MAX`, the note, A;
    - a region overlapping only on another take.
  - `random_crossfade_edits_never_panic`: proptest, 2 048 cases at the model level (`audio_cmd` of a random `AudioEdit` applied with `core_model::command::apply` to the fixture document): `(event 0..40 or any u64, Frames|UpTo with any u64, any shape)` and `RemoveCrossfade(any)`. Each result is `Ok` or an error, never a panic, and `rules::validate` of an `Ok` result is empty. A session version runs 64 cases on one session, resetting with `undo` after each `Ok`, and no result is `Internal` (F14).
  - `edit_warnings_reach_the_snapshot` (F1): with the crossfade made, an `EventEdit::Move` of B past A's end returns a `DocSnapshot` whose `warnings` has one line starting "Crossfade removed" and saying "track". A following `snapshot()` has none. An `edit_all` and a gesture end with the same move carry it too.
  - `pasted_rows_carry_the_flag`: `spec(row)` of B's flagged row is `crossfade: true`. Inserting A's and B's specs into another lane keeps it (Ruling 6).
  - `tests/control.rs` `audio_editor_state_shows_the_crossfade`: a row with `crossfade: true` and `crossfade_with: Some(7)` gives `"crossfade": {"frames": 4800, "shape": "equal_power", "with": 7}`. Without it, `"crossfade": null`.
- [ ] **Step 2: Run** `cargo test -p core-ffi --test crossfades --test control`. Expected: fail to compile.
- [ ] **Step 3: Implement** the items above, then `scripts/build-ffi.sh` and the Swift switch arms (`FakeSession` records the edit).
- [ ] **Step 4: Run** `cargo test --workspace` (including the existing `lane_events_of_100_000_events_is_fast`, which now also runs `overlaps`: `lane_events` sweeps only a take with two or more regions; F11), `cargo clippy -p core-ffi --lib -- -D warnings`, `scripts/swift.sh build` and `scripts/swift.sh test`. Expected: PASS (about +7 Rust; Swift unchanged).
- [ ] **Step 5: Commit** `feat(ffi): crossfade edits, overlap rows and the audio editor's crossfade state (plan 6c Task 4)`.

### Task 5: The Region menu, the preference and the control channel

**Files:**
- Modify:
  - `crates/core-control/src/registry.rs`:
    - `CROSSFADE_PARAMS` and `REMOVE_CROSSFADE_PARAMS`;
    - the two entries after `region.remove_fades`, in menu "Region" with `document: true`: "Crossfade" and "Remove Crossfade";
  - `crates/core-control/tests/commands.rs` (count 173; the sentence gains "6c adds region.crossfade and region.remove_crossfade");
  - `app/Sources/VizDAWAppModel/RegionCommands.swift`;
  - `app/Sources/VizDAWAppModel/Nudge.swift` (`EditingPreferences.crossfadeMs`);
  - `ControlCommands.swift`:
    - `ControlHost.crossfadeMs`;
    - the two IDs are dispatched before `_ where RegionCommands.ids.contains(id)`;
  - `app/Sources/VizDAW/ControlChannel.swift` (`AppControlHost.crossfadeMs` reads `editingPreferences`);
  - `app/Sources/VizDAW/AppCommands.swift` (the two items after Remove Fades);
  - `app/Sources/VizDAW/EditingSettingsView.swift` ("Default crossfade" in ms, theme-styled, labelled);
  - `app/Tests/VizDAWAppModelTests/ControlCommandsTests.swift` (the fake host's `crossfadeMs = 10`).
- Test: `crates/core-control/tests/commands.rs`, `app/Tests/VizDAWAppModelTests/RegionCommandsTests.swift`, `NudgeTests.swift`

**Interfaces:**
- Consumes: Task 4.
- Produces:
  - **Registry parameters** for `region.crossfade`:
    - `opt_id_param("seq")`, `opt_id_param("lane")`, `opt_id_param("event")`, each with help ending "(give seq, lane and event together, or none: the selection)" (F15);
    - `length_ms` (`Text`, "Milliseconds, more than 0, as text (\"12.5\"); default: the whole overlap, at most Settings › Editing's default crossfade");
    - `shape` (`Choice ["linear","equal_power","exponential","s_curve"]`, default `equal_power`).

    `region.remove_crossfade` takes the three IDs only. Neither has a dialog.
  - `EditingPreferences`:
    - `static let crossfadeKey = "VizDAW.crossfadeMs"`;
    - `static let crossfadeRange: ClosedRange<Double> = 1...1000`;
    - `var crossfadeMs: Double`: default 10, clamped on read and set, stored in the defaults it was given.
  - `RegionCommands`:
    - `static let crossfade = "region.crossfade"` and `static let removeCrossfade = "region.remove_crossfade"`;
    - `static let noCrossfadeTarget = "Select two overlapping audio regions on one track, or the later one."`;
    - `static func crossfadeTarget(_ m: DocumentModel) -> (row: EventRow, lane: LaneKey)?`: Ruling 5, from `editor.laneEvents` of the selection's lanes.
    - `static func parseLengthMs(_ text: String) -> Double?`: finite, more than 0 and at most 3 600 000, after `StripEntry.clean`.
    - `static func shape(_ text: String) -> FadeShapeRow?`.
    - `@discardableResult static func crossfade(_ m: DocumentModel, defaultMs: Double) -> KeyOutcome`: the menu and keys. It sends `UpTo(frames(defaultMs))` at the region's media rate (the project rate when the media is unknown) with `.equalPower`, as one `editor.edit`.
    - `@discardableResult static func removeCrossfade(_ m: DocumentModel) -> KeyOutcome`: on the target with `crossfade == true`, else unavailable: "The selected region has no crossfade."
    - `static func runChannel(_ id: String, args: [ControlArg], doc: UUID, _ m: DocumentModel, defaultMs: Double) -> ControlOutcome`, in `MpeCommands.run`'s style. It refuses with `invalid_params`, before any session call, naming the command and the parameter:
      - some but not all of the three IDs;
      - IDs that are not a region on that track's active take;
      - `length_ms` that `parseLengthMs` refuses;
      - an unknown `shape`.

      The core's refusals are `failed` with its sentence ("Crossfade Failed"). An open gesture is `unavailable`. With `length_ms`, it sends `Frames`.
    - `enabled` is true for `crossfade` when `crossfadeTarget` is non-nil, and for `removeCrossfade` when that target has `crossfade`.
    - **Routing (F3):** both IDs join `RegionCommands.ids` (so `ControlCommands.handled` has them). `run(_:on:_:)` and `enabled` test for them **first**: `run` calls `crossfade(m, defaultMs: RegionCommands.defaultCrossfadeMs)` (10, used only when no host is at hand) or `removeCrossfade(m)`, and never reaches the `.fadeTo` line. `ControlCommands.run` dispatches them to `runChannel` with `host.crossfadeMs` before `_ where RegionCommands.ids.contains(id)`.

- [ ] **Step 1: Write the failing tests.**
  - core-control `crossfade_commands_check_their_params`:
    - `{}`, `{seq, lane, event}`, `{seq}` alone (the Swift layer refuses it, not the registry; F15) and `{length_ms: "12.5", shape: "linear"}` validate;
    - `shape: "cubic"`, `event: -1`, `length_ms: 5` (not text) and extra keys are `InvalidParams`.

    `every_command_survives_random_params` (2 048 cases) covers both.
  - `RegionCommandsTests`. The fake session holds Task 1's layout as rows (B with `overlapWith: A`, `overlapFrames: 24_000`); the fake host has `crossfadeMs` 10:
    - `crossfadeTargetsTheLaterRegion`: B alone, A and B together, and B and A in either order each give B. A alone, the note, or two regions on different tracks give nil.
    - `menuCrossfadeSendsTheDefaultUpTo`: one edit `.crossfade(event: B, length: .upTo(frames: 480), shape: .equalPower)` at 48 kHz.
    - `crossfadeChannelBuildsTheEdit`: `{seq, lane, event: B, length_ms: "100", shape: "linear"}` gives `.frames(frames: 4_800)` with `.linear`; without arguments it is the menu's edit.
    - `crossfadeChannelRefusesHostileArguments`. Each is `invalid_params` naming `region.crossfade` and the parameter, with no session call:
      - `length_ms` "0", "-5", "NaN", "inf", "1e400", "", "abc";
      - only `seq` and `lane`;
      - `event` that is the note, an unknown ID, or a region on another track;
      - `event` A, which starts inside nothing, reaches the session and is `failed` with the core's sentence.
    - `crossfadeIsUnavailableDuringAGesture` and `removeCrossfadeNeedsACrossfade`.
    - `crossfadeIdsNeverRunFadeTo` (F3): `RegionCommands.run("region.crossfade", m)`, `run("region.remove_crossfade", m)` and `ControlCommands.run` of each with no arguments (a key's path) send only `.crossfade` / `.removeCrossfade` edits, never `.fadeTo`; with no target they send nothing.
    - `everyRegisteredMenuCommandHasAHandler` passes with the new IDs.
  - `NudgeTests.crossfadePreferenceIsKeptAndClamped`: the default is 10; 0 and 5 000 clamp to 1 and 1 000; a new `EditingPreferences` on the same `MemoryDefaults` reads the stored value.
- [ ] **Step 2: Run** `cargo test -p core-control` and `scripts/swift.sh test --filter RegionCommands`. Expected: FAIL.
- [ ] **Step 3: Implement** the items above. The menu items call `RegionCommands.crossfade(doc, defaultMs: AppControlHost.shared.crossfadeMs)` and `removeCrossfade`, with shortcuts from `KeyMenu.shortcut` and `.disabled` from `enabled`.
- [ ] **Step 4: Run** `cargo test -p core-control`, both clippies, `scripts/build-ffi.sh`, `scripts/swift.sh build` and `scripts/swift.sh test`. Expected: PASS (registry 173; about +1 Rust, +7 Swift).
- [ ] **Step 5: Commit** `feat(app): Region › Crossfade and Remove Crossfade, the default length, region.crossfade and region.remove_crossfade (plan 6c Task 5)`.

### Task 6: The editors: the X handle, its drag and menu, the typed box, the inspector and VoiceOver

**Files:**
- Modify:
  - `app/Sources/VizDAWEditors/RegionEditing.swift`:
    - `RegionHandle.crossfade`;
    - `RegionHandles`: `label`, `rects`, `crossfadeRoom`, `fades`, `fadeCommand`, `shapeCommand` and `fadeArea`;
    - `RegionA11y.label`;
  - `RegionDrawing.swift` (`crossfadeLines`, `draw`, `fadeShapeMenu`);
  - `AudioEditorGeometry.swift` (`AudioEditorHandle.crossfade`, `drawn`, `hit`'s tie rule, labels);
  - `AudioEditorCanvas.swift` (draws the X; a11y "Crossfade Handle"; the readout);
  - `ArrangementView.swift` (the drag readout "<n> ms" for `.crossfade`);
  - `app/Sources/VizDAWAppModel/AudioEditorModel.swift` (`AudioEditorBox.crossfade`, `text`, `type`, `setShape`, `stateRow`);
  - `DocumentModel.swift` (`apply` shows `warnings`; F1);
  - `AudioEditorEntry.swift` (`fadeFrames`, `fadeHint`, `crossfadeInHint`, `noCrossfadeHint`);
  - `PaneContent.swift` (`Inspector.rows`);
  - `app/Sources/VizDAW/AudioEditorPane.swift` (the Crossfade field and picker in place of Fade In).
- Test:
  - `app/Tests/VizDAWEditorsTests/RegionEditingTests.swift`, `AudioEditorGeometryTests.swift`;
  - `app/Tests/VizDAWAppModelTests/AudioEditorModelTests.swift`, `AudioEditorEntryTests.swift`, `RegionHandleViewTests.swift`, `LeftPaneTests.swift`, `EditorModelTests.swift`.

**Interfaces:**
- Consumes: Task 4 (Task 5 only for the menu IDs the shape menu's Remove runs through `editor.edit`).
- Produces:
  - **`RegionHandles`:**
    - `label(.crossfade)` is "Crossfade";
    - `rects` gives `.crossfade` in place of `.fadeIn` when `r.crossfade`, at `fadeInX`;
    - `static func crossfadeRoom(_ r: EventRow) -> Int64` is `max(min(Int64(clamping: overlapFrames), Int64(clamping: frames) − fadeOut), 0)` (F17);
    - `fades(.crossfade)` clamps the fade-in to `1...max(room, 1)`;
    - `fadeCommand(.crossfade)` gives `.audio(edit: .crossfade(event:, length: .frames(frames:), shape: r.fadeIn.shape))`;
    - `shapeCommand(.crossfade, s)` keeps the length;
    - `fadeArea` gives `.crossfade` over a crossfade's fade-in area;
    - `target`'s switch gains `.crossfade` with the fades (F17).
  - `AudioEditorHandle.regionHandle` gains `.crossfade: .crossfade` (F17).
  - **Edit warnings (F1):** `DocumentModel.apply(_:)` sets `editor.status` to `s.warnings.joined(separator: " ")` when it is non-empty.
  - `RegionDrawing.crossfadeLines(_ r: EventRow, in rect: CGRect) -> [(CGPoint, CGPoint)]`: `[(minX, maxY)→(fadeInX, minY), (minX, minY)→(fadeInX, maxY)]`, or `[]` without a crossfade. `draw` strokes them in place of the fade-in line.
  - `fadeShapeMenu` for `.crossfade` titles its items "Crossfade: <shape>" and adds "Remove Crossfade" (`.audio(edit: .removeCrossfade(event:))`).
  - `RegionA11y.label` adds ", crossfade" when `r.crossfade`. ", fades" then counts the fade-out only.
  - **`AudioEditorBox.crossfade`:** raw value `"crossfade"`, label "Crossfade Length", hint `fadeHint`.
    - Its `text` is `fadeText` of the fade-in when `crossfade`, else "–". The `fadeIn` box shows "–" while there is a crossfade.
    - `type(.crossfade, t)` sends `Frames(min(max(fadeFrames(t), 1), room))`. Typing it on a region without a crossfade is refused with `noCrossfadeHint` ("This region has no crossfade. Use Region › Crossfade."). `type(.fadeIn, t)` on a crossfade is refused with `crossfadeInHint` ("This region's fade in is a crossfade: type its length in the Crossfade box.").
    - `setShape(.crossfade, s)` sends the crossfade with the same length.
    - `stateRow` fills `crossfade` and `crossfadeWith` (`overlapWith`).
  - **`AudioEditorEntry`:**
    - `static func fadeFrames(_ text: String, rate: UInt32) -> Int64?`: "N frames" or "N frame" (a whole number ≥ 0, at most `Int64.max / 2`), else `duration(text, bare: .ms)` through `frames`;
    - `fadeHint` becomes "Type a fade like 250 ms, 1.5 s or 480 frames.";
    - the fade boxes use `fadeFrames`.
  - **`Inspector.rows`:** with `region.crossfade`, the "Fade In" row becomes `InspectorRow(label: "Crossfade", value: "<ms> ms, <shape title>")`, with the shape titles of `RegionHandles.shapes`.

- [ ] **Step 1: Write the failing tests.**
  - `RegionEditingTests`:
    - `crossfadeHandleReplacesTheFadeIn`: a flagged row has `.crossfade` and no `.fadeIn` in `rects`; an unflagged row is unchanged.
    - `crossfadeDragClampsToTheRoom`: a drag far left gives frames 1; far right gives `min(overlap, frames − fadeOut)`; the command is `.crossfade` with `.frames` and the row's shape.
    - `crossfadeShapeKeepsTheLength`.
    - `crossfadeLinesMakeAnX`: the two exact segments in a 100×40 rect with the fade-in ending at x 30.
    - `crossfadeMenuOffersShapesAndRemove`: five titles in order, the current one checked, and the last runs `.removeCrossfade`.
    - `a11yLabelSaysCrossfade`.
  - `AudioEditorGeometryTests.crossfadeCornerInTheAudioEditor`: `drawn` has `.crossfade` at the fade-in end; `hit` there gives `.crossfade`, and when both corners reach the press, the nearer one wins as for `.fadeIn`.
  - Update first (F16): the existing `fadeHint` assertions in `AudioEditorEntryTests.swift` (~99, ~104) and `AudioEditorModelTests.swift` (~150) expect the new text; run them and see them fail.
  - `EditorModelTests.editWarningsShowInTheStatus` (F1): a fake session whose edit returns a snapshot with `warnings ["Crossfade removed: …"]` leaves that text in `editor.status`; one with none leaves the status alone.
  - `AudioEditorEntryTests.fadeFramesReadsMsSecondsAndFrames` at 48 kHz:
    - "250 ms" gives 12 000, "1.5 s" gives 72 000, "480 frames" gives 480, "1 frame" gives 1 and a bare "10" gives 480;
    - refused (nil): "1.5 frames", "-3 frames", "NaN", "inf ms", "1e30 frames", "" and "frames".
  - `AudioEditorModelTests`:
    - `crossfadeBoxTypesMsAndFrames`: "25 ms" sends `Frames(1200)`; "999999 ms" sends `Frames(room)`; its own text back sends nothing.
    - `fadeInBoxRefusesOnACrossfade` and `crossfadeBoxWithoutACrossfade`: each sets its hint, sends nothing and answers false.
    - `crossfadeShapePickerKeepsTheLength`.
    - `stateRowCarriesTheCrossfade`: `boxes` has `crossfade`, and `region.crossfade` and `crossfadeWith` are set.
  - `RegionHandleViewTests`:
    - `draggingTheXIsOneCrossfadeStep`: a press on the X handle in the arrangement view, a drag and a release make one gesture labelled "Crossfade", ending in one `.crossfade` edit.
    - `escapeDuringTheXDragEditsNothing`: Escape mid-drag cancels the gesture, with no edit and no undo step.
    - `aRefusedXDragEndCancelsTheGesture`: the fake session refuses the edit; the gesture is cancelled and the status shows the core's sentence.
  - `LeftPaneTests.inspectorShowsTheCrossfade`: "Crossfade" "10 ms, Equal Power" in place of "Fade In".
- [ ] **Step 2: Run** `scripts/swift.sh test --filter RegionEditing`, `--filter AudioEditor`, `--filter RegionHandleView` and `--filter LeftPane`. Expected: FAIL.
- [ ] **Step 3: Implement** the items above. Say in the task report that the region strip (`RegionStripView`) gets the X through the shared `RegionHandles` with no code of its own.
- [ ] **Step 4: Run** `scripts/swift.sh build` and `scripts/swift.sh test` (the name scans included). Expected: PASS (about +18 Swift).
- [ ] **Step 5: Commit** `feat(app): the crossfade X handle, shape menu, typed box, inspector row and VoiceOver label (plan 6c Task 6)`.

### Task 7: README checks, e2e, backlog, gates

**Files:**
- Modify: `app/README.md`, `docs/feature-backlog.md`, `crates/vizctl/tests/e2e.rs`
- Test: the e2e

**Interfaces:**
- Consumes: Tasks 1–6.

- [ ] **Step 1: Write the e2e** `crossfades_over_the_channel` (`#[ignore = "needs a window server and VIZDAW_E2E_APP"]`, throwaway home, offline):
  1. Write `a.caf` (2 s at 0.25) and `b.caf` (2 s at 0.5), both **mono**, in the home with a new helper `audio_file_level(path, frames, level, channels)` beside `audio_file` (F19; `audio_file` and its callers are unchanged). Mono makes one track per import (F20). Run `file.new` (an empty project, so nothing else sounds in the bounce; read `selected_seq` as the e2e at `file.new` does), then `media.import` of `a` at tick 0, which makes a new track. Read that lane from `state.get document`. Run `media.import` of `b` with that `lane` at tick 15 360 (0.5 s), so the overlap is 1.5 s.
  2. Select `b`'s region by its element (`wait_for_label_like(…, "Region b on ", …)` and `ui.click`), and read `event` and the lane from `state.get selection`.
  3. Hostile calls:
     - `region.crossfade` with `length_ms` "0", with `shape` "cubic", and with only `seq` is `InvalidParams`;
     - with `event` set to `a`'s region (found the same way) it is `Failed`;
     - `state.get document`'s `undo` is unchanged, and `window_numbers` is unchanged.
  4. `region.crossfade {seq, lane, event: b, length_ms: "100", shape: "linear"}` gives `undo` "Crossfade". With `b` selected, `state.get audio_editor`'s `region.crossfade` is `{"frames": 4800, "shape": "linear", "with": <a>}`, and `boxes.crossfade` is "100 ms".
  5. `file.bounce` to `home/x.wav` (16-bit, 48 kHz, `dither: false` (F18), range `selection` from tick 0 to tick 46 080) and `read_wav16`. With `s(f)` the left sample at frame `f`:
     - before = `s(12_000)`, mid = `s(26_400)` (the span's middle), after = `s(40_000)`;
     - `|mid − (before + after) / 2| ≤ 3` LSB, and `after > before`.

     Ratios, not levels, so the strip's gain does not matter.
  6. `region.remove_crossfade` with no arguments (the selection) gives `undo` "Remove Crossfade". `region.crossfade` with no arguments gives a crossfade of 480 frames (Settings' 10 ms) and `shape` "equal_power". A second bounce (dither off) checks Equal Power's midpoint (F20): `s(24_240)` is within 3 LSB of `0.7071 × (before + after)`. `edit.undo` twice brings back the linear one.
  7. With the crossfade back and `b` selected, `region.remove_fades` (the channel has no move or delete) sets the fade-in to 0, so settle drops the crossfade: the result's `status`, which `ControlCommands.ran` takes from `editor.status`, contains "Crossfade removed" (F1).
- [ ] **Step 2: Add README checks 274–281** after 273, under "### Crossfades (plan 6c)". Start with a line naming the tests that cover the rest (`core-model/tests/crossfades.rs`, `core-engine/tests/crossfades.rs`, `core-ffi/tests/crossfades.rs`, `RegionCommandsTests`, `RegionEditingTests`, `AudioEditorModelTests` and the e2e `crossfades_over_the_channel`).
  - 274: Put two regions on one track so that the second starts inside the first, select both and choose Region › Crossfade. An X covers the start of the overlap: 10 ms, or the whole overlap when it is shorter. Region › Remove Crossfade turns it back into a plain fade-in; Undo brings the crossfade back.
  - 275: Drag the X's handle. The length follows, with an "<n> ms" readout. It stops at the end of the first region, and at 1 frame. One Undo ("Crossfade") puts it back; Escape mid-drag changes nothing.
  - 276: Right-click the X. There are four "Crossfade: …" shapes (the current one ticked) and Remove Crossfade. Each shape changes the drawing's name in VoiceOver ("crossfade") and the sound, not the length.
  - 277: In the audio editor with the later region selected, the Crossfade box and shape picker replace Fade In. Type "25 ms", then "1200 frames": both land as one Undo step each. Type "abc": the status says how to type a fade.
  - 278: The inspector shows "Crossfade 10 ms, Equal Power" for the region.
  - 279: Move the later region a little right: the crossfade shortens and the status says so. Move it past the first region's end: the crossfade goes, with a status line, and moving it back does not bring it back (Undo does). Split inside the X: refused with its sentence. Split later in the region: the new part has no X.
  - 280: Listen across the overlap with Linear and with Equal Power: no click, no gap. Open a project saved before this build that has overlapping regions: they still cut hard from one to the next.
  - 281: Settings › Editing › Default crossfade: set 50 ms. Region › Crossfade on a long overlap makes 50 ms; the value is kept after a relaunch.
  - **Owner check (manual, only the owner can do):** make, drag, type and remove a crossfade, and listen through it (spec §12).
- [ ] **Step 3: Add backlog rows** to `docs/feature-backlog.md`, in its style:
  - "Crossfades with a partner ID or per-side shapes" (Ruling 1);
  - "Split inside a crossfade" (Ruling 7: two shorter crossfades instead of a refusal);
  - "Add crossfades to every overlap" (Ruling 2: one command for old projects);
  - "Imports insert a crossfaded pair in one command" (F22): `AudioCmd::Insert` has no flag, so 6d, 6f and 6h insert both regions as specs in one `EventCmd::Insert`; a later region inserted alone loses its flag (Ruling 6 rule 3).
  - "Crossfade the waveform drawing": the earlier region's tail under the X is not drawn, and the overview's fade shape (`core-ffi/src/overviews.rs`) ignores the complement.

  Also append to the existing "Zero-crossing snapping and edge fades for audio edits" row: "Plan 6c Ruling 8: a crossfaded start counts as the region's own fade, so it gets no edge fade, and the X drag snaps nothing." Add to the "A control-channel path to set a fade shape" row: "`region.crossfade`'s `shape` (plan 6c) sets a crossfade's; plain fades still have none."
- [ ] **Step 4: Run all the gates:**
  - `cargo fmt --all -- --check`;
  - both clippies and `cargo clippy -p core-ffi --lib -- -D warnings`;
  - the rt-check gate with `rt_alloc_crossfade`;
  - `cargo test --workspace`;
  - `scripts/build-ffi.sh`, `scripts/swift.sh build` and `scripts/swift.sh test`;
  - the no-brand build;
  - `scripts/make-app.sh` and the e2e.

  Afterwards `pgrep -lf MacOS/VizDAW` prints nothing. Expected: all PASS (e2e 26, registry 173, README through 281, `VERSION` 9).
- [ ] **Step 5: Commit** `docs: README checks 274–281, crossfade e2e and backlog rows (plan 6c Task 7)` with the files above.

## Self-Review

- **Spec coverage (6c's row of §13, §7.0):**
  - **Model and version 9:** the flag on the later region, the three validation rules, and dropping or shortening in the same command → Tasks 1–2 and Rulings 1, 2, 4 and 6.
  - **Engine:** the complement per shape, the partner's fade-out on top, no allocation and `rt-check` → Task 3.
  - **Commands:** `Crossfade` and `RemoveCrossfade`, undoable, with move, trim and delete inside the same step → Tasks 1–2. The spec's `length_frames` becomes `CrossfadeLength` (Ruling 3).
  - **Editors:** the X handle in the arrangement and the audio editor, its drag, typed values in ms or frames, the shape menu, the Region menu items with the default length, and no automatic crossfades → Tasks 5–6 and Rulings 3–5 and 9.
  - **Control channel:** both commands, the hostile list (no overlap, another take, length 0 or past the end, NaN or negative, unknown shapes, non-regions) and the state in `state.get` region rows → Tasks 4–5 and 7.
  - **§12:** golden renders per shape and beside a fade-out, the null test, the undo property, move/trim/delete, `rt-check`, the v8 → v9 migration, README checks and the owner check → Tasks 1–3 and 7.
- **Deliberately not here:** the DAWproject, AAF and OMF mappings (6d, 6f, 6h); the "Add crossfades to every overlap" command (backlog).
- **Names are consistent across tasks:**
  - `crossfade::{DEFAULT_SHAPE, Overlap, overlaps, overlap, room, problems, settle}`, `CrossfadeLength::{Frames, UpTo}`, `AudioCmd::{Crossfade, RemoveCrossfade}`, `migrate_v8_to_v9`;
  - `Segment.{xfade, xfade_from}`;
  - `AudioEdit::{Crossfade, RemoveCrossfade}`, `CrossfadeLengthRow`, `EventRow.{crossfade, overlap_with, overlap_frames}`, `AudioEditorRegionRow.{crossfade, crossfade_with}`;
  - `RegionCommands.{crossfade, removeCrossfade, crossfadeTarget, parseLengthMs, shape, runChannel, noCrossfadeTarget}`, `EditingPreferences.crossfadeMs`, `ControlHost.crossfadeMs`;
  - `RegionHandle.crossfade`, `RegionHandles.crossfadeRoom`, `RegionDrawing.crossfadeLines`, `AudioEditorHandle.crossfade`, `AudioEditorBox.crossfade`, `AudioEditorEntry.{fadeFrames, crossfadeInHint, noCrossfadeHint}`.
- **Review Focus:** each line names its tests in the owning task.
- **Spec questions for the owner:**
  1. Ruling 4: Equal Power as the default shape.
  2. Ruling 6: shortening rather than dropping when the overlap shrinks.
  3. Ruling 7: refusing a split inside a crossfade.
  4. Ruling 5: `seq`, `lane` and `event` are optional, which differs from the plain parameters in §10.
  5. Ruling 6 rules 2 and 3 drop a crossfade in cases the spec does not list: when the region it crossed is replaced by a different one, and when the later region is copied alone (F21).
