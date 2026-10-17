# VizDAW 4d — Strip Chart and Automation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Automation events in ordinary lanes drive strip faders, pans, mutes, solos, send levels, plugin parameters and destination velocity scales, played back by the engine with sub-block accuracy (runs split at automation points, never shorter than 32 frames) without allocating, locking or panicking on the audio thread; a Strip Chart detail editor (⌘3) shows and edits velocity, release velocity, duration, controllers, pitch bend, pressure, program, tempo, velocity scale and every automation target in stacked rows, drawing with straight, curved, flat, free, random and oscillator lines (periods in musical, clock or meter-following units) and modifying with set, scale, add, max/min limit, Compress rectangles, legato, thin and thicken, ⌥ limiting a modify to the selected events; every operation is also a modification of the select-and-modify panel, and exact mode opens the panel from Strip Chart lines and from a new scale-time drag.

**Architecture:** Rust first. `core-model` gains `automation.rs` (targets and the normalised-value laws), `EventBody::Automation`, `Rule::Automation`, and `strip.rs` (lines, shapes, series and the one new modification `Modify::Line`, which draws, changes, compresses, thins, thickens and legatos events or tempo over a line's range). `core-engine` compiles automation into per-target lanes of `(frame, value)` points in the schedule (`schedule/automation.rs`; velocity scale is applied to note-ons at compile time), gives the render graph automatable mixer controls with static values and overrides (`graph/controls.rs`, gains that ramp linearly across a run), and splits each block into runs at automation points, loop ends and the sequence end; plugin parameters go to inserts and plugin instruments as sample-accurate parameter changes through new `automate` hooks that `core-host`'s adapters map from normalised to plain values. `core-ffi` adds automation rows, line records, `ModifyRow::Line` and `ConditionTest::Automation`; the Strip Chart sends ordinary `SelectAndModify` edits, so templates, Modify Again and the panel reach everything. Swift adds pure lane, scale, line and command maths (`VizDAWAppModel`), a per-slot `StripChartModel`, the `StripChartView` canvas (`VizDAWEditors`), the Strip Chart pane and the panel's Strip Chart Line editor (`VizDAW`), plus the piano roll's scale-time drag.

**Tech Stack:** Rust 2024 (`core-model`, `core-engine`, `core-host` with `clack-host`, `core-ffi` with UniFFI 0.32.2, `im` 15, `serde`, `rtrb`), the in-repo CLAP test plugins (`test-plugins`), Swift 6 / SwiftPM (SwiftUI shell, AppKit `NSView` canvases with Core Graphics, Swift Testing), macOS 14+.

**Spec:** `docs/superpowers/specs/2026-10-16-editors-design.md` (§3 architecture, §4 phase 4d — binding, §5 errors, §6 testing), parent `docs/superpowers/specs/2026-10-15-vizdaw-design.md` §9 (Strip Chart paragraph, select-and-modify panel, exact mode) and §10 (real-time thread rules), `docs/superpowers/specs/2026-10-16-plugin-hosting-design.md` (parameters, gestures, stable `ParamId`s, the parameter overlay). Previous plans for conventions: `docs/superpowers/plans/2026-10-16-editors-foundation.md`, `2026-10-17-editors-piano-roll.md`, `2026-10-17-editors-list-modify.md`.

## Global Constraints

- Every shell command runs after `export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools` (the Xcode license is not accepted, so `git`, `cc`, `swift` and the linker work only with the Command Line Tools). Every bash block below starts with that line and `cd /Users/lattawb/workspace/vizdaw`.
- Work on branch `sp4-editors` (checked out; code head `a075852` before Task 1, plus this plan's commit). Commit at the end of every task, adding only the files the task lists; never push. End every commit message with the attribution lines your harness requires (the blocks below show `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>` and the `Claude-Session:` line).
- Rust stable, edition 2024; crate manifests keep `edition.workspace = true`, `license.workspace = true`, `rust-version.workspace = true`. License GPL-3.0-only. No new crates.
- `core-model`, `core-engine`, `core-host`, `core-ffi` and `platform-macos` keep their `#![deny(missing_docs)]` and `#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic))]`. Tests may `unwrap` and index. No new `unsafe`.
- Real-time rules (parent spec §10, `core-engine` crate docs): everything reachable from `Renderer::process` and from a worker running a job is marked `// RT` and must not allocate, free, lock, do I/O, log or panic. New RT state is allocated off the audio thread (in `Graph::build`/`set_controls`, `Automation::build`, `Renderer::new`, adapter constructors). The `rt_alloc` tests (`crates/core-engine/tests/rt_alloc.rs`, `crates/core-host/tests/rt_alloc_plugins.rs`) must keep passing with automation playing.
- File compatibility: existing `EventBody`, `EventKind`, `Rule` and `Modify` variants keep their names and serde shapes; new variants are appended; `core_model::file::VERSION` stays 2 (old projects load unchanged; a build before 4d refuses a project that contains automation at the unknown variant, Plan Ruling 6). `core-model` stays free of OS-specific dependencies and builds and tests on Linux.
- No panic crosses the FFI: every exported function and method returns `Result<_, VizError>` and runs inside `ffi_guard`. No session `inner` lock is held across plugin code; no `HOST` lock is held across a sleep (Task 1).
- UniFFI: new enum variants are appended at the END of existing enums (`EventKindRow`, `ModifyRow`, `ConditionTest`); new record and enum types are new. The one existing record that gains a field is `EventRow` (`automation`, last, `#[uniffi(default = None)]`, Plan Ruling 25); no other record gains a field.
- Swift: the generated bindings compile in Swift 5 mode; `VizDAWAppModel`, `VizDAWEditors`, `VizDAW` and the tests in Swift 6 mode with no new warnings. Never use SwiftUI's `@State` or `@FocusState` in new views (the CLT SDK cannot expand them); keep view state in `@Observable` models. Every Swift `switch` over a generated enum that gains a case is updated in the same task as the Rust change. Models and pure logic are tested (Swift Testing, `@Test`, `#expect`); AppKit views get behaviour tests through internal entry points and the manual checklist. Closures stored by views capture `[weak self]`.
- Drawing (spec §2): custom `NSView`s (an `EditorCanvas` subclass) with Core Graphics; no Metal.
- `FakeSession` implements the whole generated `SessionProtocol`; this plan adds no `Session` method, so it gains none.
- Generated files are never committed: `app/Sources/VizDAWCore/vizdaw_ffi.swift`, `app/Sources/vizdaw_ffiFFI/include/`, `target/ffi/`, `dist/`. Run `scripts/build-ffi.sh` before any `scripts/swift.sh` command after a Rust change.
- Each task ends green: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `scripts/build-ffi.sh`, `scripts/swift.sh build`, `scripts/swift.sh test`. Run `cargo fmt --all` first. Baseline before Task 1: 672 Rust tests listed by `cargo test --workspace -- --list`, 484 Swift `@Test` functions.
- Code in the steps is complete for new files (written whole with `cat > … <<'VIZDAW_EOF'`). Changes to existing files are given as exact **Find / Replace** excerpts of the tree the previous task left; apply them with an editor (the Edit tool). If an excerpt does not match because a fix landed meanwhile, make the same change by hand and keep going.
- Spec values used verbatim: 15 360 ticks per quarter note; automation values normalised 0–1; "values between points ramp linearly; mute/solo/stepped params jump"; "split at automation points, minimum 32 frames"; "automation overrides the static control value while playing; stopping leaves the control at its last automated value until the user moves it"; one drag is one undo step; editors never hold Rust objects across the run loop.

## Review Focus

The five failure modes a user is most likely to hit that the spec implies but no requirement spells out, each pinned by a named test in its owning task:

1. **Automation deleted, then Play** — the user removes an automation lane's points (or the lane) after a run left a fader at −12 dB: the next Play must return the static value, not keep an invisible override forever. Pinned: Task 8 `play_after_the_automation_is_gone_restores_the_static_fader`.
2. **Automation whose target no longer exists** (a deleted strip, bus or plugin; a send removed; undo/redo of the deletion) — playback continues, the target is skipped and the compile report says how many points it skipped; nothing panics on the audio thread. Pinned: Task 6 `missing_targets_are_skipped_and_reported`; Task 8 `automation_of_a_deleted_strip_plays_like_none`.
3. **Very dense automation** (points a few frames apart, thousands per second from a free line or an oscillator with a short period) — runs are never shorter than 32 frames, nothing allocates, the output does not stutter. Pinned: Task 8 `dense_points_never_split_runs_below_32_frames`; Task 8's `rt_alloc` automation phase.
4. **The user moves a control that automation drives** — while stopped the move wins and stays; while playing automation wins again at the next run; a moved send level drops only that send's override. Pinned: Task 7 `a_new_graph_keeps_overrides_only_while_the_static_value_is_unchanged`; Task 8 `stopping_keeps_the_last_automated_value_until_the_fader_moves`, `while_playing_automation_wins_over_a_fader_move`.
5. **Odd Strip Chart drags** — right-to-left drags, a click without a drag, a drag that leaves the row above or below, a zero-width Compress rectangle, a line over a range with no events, Escape mid-drag: ranges are put in time order, a click draws one point, values clamp to the row, a no-op adds no undo step, Escape leaves nothing behind. Pinned: Task 4 `a_line_over_no_events_changes_nothing`; Task 10 `scaleClampsOutsideTheRow`; Task 11 `aBackwardDragIsTurnedAround`, `aZeroWidthCompressIsOneTick`; Tasks 12 and 13 `escapeLeavesNoUndoStep`, `aClickInDrawModeDrawsOnePoint`.

## Plan Rulings

Decisions the spec leaves open, each implemented and tested as stated (format: Ruling: what — why — cost if wrong):

1. **Ruling:** Task 1 carries the 4b/4c follow-ups: `Session::edit_all` catches a panic inside a step, aborts the transaction and clears `edit_label` before returning `Internal`; its doc says none of its commands removes a running plugin (mixer edits, which capture a plugin's state first, are not editor commands); `Modify::ScaleTime` keeps a note that had a length at least one tick long (like Resize and relative Change); a device rebuild drains its 300 ms and recreates the host without holding `HOST` (like `retire()`) — why: carried from the 4b/4c ledgers — cost if wrong: none.
2. **Ruling:** `AutomationTarget` has the spec's six variants plus `VelocityScale(DestId)` — why: the spec lists a velocity-scale Strip Chart lane but no target for it; Studio Vision's velocity scale is a per-instrument automation type — cost if wrong: one variant to remove.
3. **Ruling:** An automation event holds a finite `f64` in 0–1 (anything else is refused with `OutOfRange` on insert and on load); its target is not checked (a strip or plugin may go and come back with undo; the engine skips missing targets and reports them) — cost if wrong: a stale target plays nothing, with a warning.
4. **Ruling:** The normalised laws: fader and send level `12 + 60·log10(v)` dB (−∞ once at or below −144 dB; unity at v ≈ 0.631; gain ≈ 3.98·v³), pan `2v − 1`, mute and solo on at v ≥ 0.5, velocity scale 1–100 % over 0–0.5 and 100–250 % over 0.5–1, plugin parameter `min + v·(max − min)` rounded when stepped. They live once in `core_model::automation` and are mirrored in Swift's `AutomationLaw` (tested against the same numbers) — why: "normalised 0–1" has no law in the spec — cost if wrong: lanes drawn under one law sound different under another.
5. **Ruling:** `SendIndex` is the send's position in its strip's send list (as `MixerCmd::EditSend` addresses sends) — cost if wrong: removing an earlier send moves its automation to the next send.
6. **Ruling:** `EventBody::Automation { target, value }` serialises as `{"type": "automation", "target": {"strip_fader": 3}, "value": 0.5}`; `file::VERSION` stays 2 — why: the variant is additive and needs no migration — cost if wrong: a pre-4d build's refusal names an unknown variant instead of "newer version".
7. **Ruling:** Automation events live in ordinary lanes and follow lane mute, lane solo, per-event mute and references like MIDI events (flatten); several lanes or references automating one target merge in time order, the later in play order winning on a shared frame — cost if wrong: none.
8. **Ruling:** A target's value before its first point is the first point's value, after its last point the last; between points linear, except mute, solo and parameters the plugin declares stepped, which hold until the next point. The schedule keeps at most 1 024 lanes (targets); more are dropped with a warning — cost if wrong: a lane before its first point leaves the static value.
9. **Ruling:** Velocity scale is applied when the sequence compiles: every note-on whose resolved destination is the target gets `velocity × percent / 100` (clamped 1–65 535) from the lane's value at the note's start; release velocities are untouched — why: velocities are compiled into the schedule; nothing on the audio thread needs it — cost if wrong: none (a scale change recompiles like any edit).
10. **Ruling:** While a schedule with automation plays, the renderer splits each block into runs at automation points (a cut closer than 32 frames to the run's start moves to 32 frames), at the loop end and at the sequence end; each run sends its MIDI (instrument offsets relative to the run) and renders the graph on its own. Without automation, or while stopped, a block is one run exactly as before. A deadline missed in a run drops the rest of that block (the transport does not advance over it, like a busy block) — why: "split at automation points, minimum 32 frames" — cost if wrong: CPU of extra graph runs with dense automation.
11. **Ruling:** Inside a run, strip gains (fader × pan) and edge gains (output and send levels × mute/solo pass) move linearly from their values at the run's start to their values at its end, so ramps are piecewise linear in gain between run boundaries; jumps (mute, solo, two points on one frame) switch hard at the run boundary, as a static mute does today. An automated mute silences the strip's output, sends and meters, not its sidechain feed (a static mute still does) — cost if wrong: a click on mute automation (fades belong to sub-project 7).
12. **Ruling:** Plugin parameters get their value at the start of each run and, on ramps, every `max(32, run / 16)` frames, as sample-accurate parameter changes merged after the host's queued overlay values (automation wins a tie on frame 0); a value equal to the last one sent is not resent; the adapters map normalised → plain with the plugin's `ParamInfo` range; unknown parameters are dropped. Automated values are not gestures and never become `SetPluginParams` commands — cost if wrong: ramp resolution.
13. **Ruling:** Automation writes overrides; Stop leaves them (the sound stays at the last automated value); a recompiled graph keeps a control's override only while the control's static value is unchanged (the user moving a fader, pan, mute, solo or send level drops that override); Play, and a new schedule while playing, drop the overrides of controls the schedule no longer automates. A plugin keeps an automated value until the user moves it (the plugin holds it); a state capture records that value — why: the spec's stop rule, plus a way back once automation is deleted — cost if wrong: an automated value outlives its lane until the next Play.
14. **Ruling:** The mixer view keeps showing static values; it does not follow automation in 4d — why: the mixer read model has no live control values; the visual pass is sub-project 7 — cost if wrong: the mixer can show 0 dB while automation plays −6 dB.
15. **Ruling:** A Strip Chart edit is one core modification `Modify::Line(LineChange { target, op, line })` run by `SelectAndModify` with the rule `TimeRange [from, to + 1)`: targets are `Value(velocity | release velocity | duration | value)`, `Series(controller n | pitch bend | channel pressure | poly pressure p | program | automation target)` and `Tempo`; operations `Draw`, `Set`, `Scale`, `Add`, `LimitMax`, `LimitMin`, `Compress`, `Thin`, `Thicken`, `Legato`; a line copy (`to_copy`) is refused. No `EditCommand::Automation` or new `EventEdit` is needed — why: one modification means templates, Modify Again and the panel reach every Strip Chart operation numerically (spec) — cost if wrong: one wide enum.
16. **Ruling:** Draw replaces a series in every selected lane: the lane's active-take events of that series in `[from, to]` go and the line's samples (every `step` from `from`, plus `to`) come in; a sample equal to the previous kept one is dropped on holding series (controllers, programs, mute, solo) and a sample within one unit of the straight line between its neighbours is dropped on ramping series (continuous automation, tempo), so a straight line writes two points. Draw on a note value (velocity, release, length) is Set on the notes in range — cost if wrong: denser data.
17. **Ruling:** Line shapes: Straight; Curved (ease in, ease out, S-curve — the panel's curves); Flat (the start value); Free (the pointer's points, ascending, the later winning on one tick; samples also fall on its corners); Random (a value per sample tick, uniform between start and end, fixed by a seed drawn at gesture start); Oscillator (sine, square, triangle, saw from start (low) to end (high); period in ticks (musical), milliseconds through the tempo map (clock) or bars that follow meter changes; phase in thousandths of a period) — cost if wrong: shape tuning.
18. **Ruling:** Value spaces: velocities 16-bit, lengths ticks, controllers/bend/pressure 32-bit, programs `program × 2²⁵` (7 bits on the 32-bit scale), automation `v × (2³² − 1)`, tempo milli-BPM clamped to 5–999 BPM. `ValueTarget::Value` now also changes programs and automation in those spaces. Scale amounts are percent; Add amounts are signed changes. The Strip Chart maps a row's bottom, middle and top to 0, 100 and 200 % for Scale and to −½, 0 and +½ of the row's range for Add — cost if wrong: a re-tuned mapping.
19. **Ruling:** Thin and Thicken use the line's `step` as their spacing; Compress maps the range's lowest value to the line's `start` and its highest to its `end` (a rectangle's bottom and top); Legato applies to lengths only — cost if wrong: none.
20. **Ruling:** Tempo lines act on the tempo maps of the selected lanes' sequences. Draw replaces the points in `[from, to]` with ramping points (`ramp = true`, the last one `false`, so the tempo after `to` holds the last drawn value until the next existing point); the other operations change the points in range; Thicken adds points on the existing ramps. Drawn tempo is always piecewise-linear ramps in BPM per tick, the model's ramp, so it "sounds linear" — cost if wrong: none.
21. **Ruling:** The Strip Chart is its own detail editor (⌘3, the slot 4a reserved), not a pane under the piano roll. It shows one lane (a lane or block target's lane; for a sequence target the lane chosen in its bar, else the first) in one to four stacked rows; each row picks its kind from a menu on its title (note values, controllers 1, 2, 4, 7, 10, 11, 64, 74 and "Other…", pitch bend, pressures, program, tempo, and Automation: fader, pan, mute, solo and each send per strip, each automatable parameter per plugin slot, velocity scale per destination). Rows are saved in `DetailState.stripRows` (optional, so older layouts decode); the bar's settings in `EditorLayout.stripChart` — cost if wrong: a pane to add under the piano roll later.
22. **Ruling:** Strip Chart modes: Draw, Modify (with an operation picker; Compress is drawn as a rectangle) and Erase (deletes the row's events in the dragged range). A press with ⌥ limits a Modify to the selected events (`Refine`). Before a gesture, Draw and Erase make the shown lane the selection's lanes; Modify keeps the selection's lanes when it has some ("modifying acts on every selected lane"), else uses the shown lane. A click without a drag in Draw mode draws one point — cost if wrong: a different modifier.
23. **Ruling:** Exact mode: a Strip Chart line or rectangle opens the panel on Change Values with the Line modification and a Position condition pre-filled; the piano roll gains a scale-time drag (⌘-drag of a selected note's end zone scales the selected notes' times and lengths around the earliest selected start, `num / den` = new span / old span reduced) which applies as Scale Time, or in exact mode opens the panel on Scale Time — why: parent §9 lists both drags; 4c deferred them — cost if wrong: another modifier for scale time.
24. **Ruling:** The panel's modify kinds gain "Strip Chart Line" (target, operation, shape, curve, wave and period pickers; From and To positions, Start and End values, Step, and for oscillators Period and Phase as fields; a free line shows its point count) and the select half gains the condition "Automation Target" — cost if wrong: none.
25. **Ruling:** FFI: `EventRow` gains `automation: Option<AutomationRow>` (UniFFI default `None`, so Swift's memberwise initialiser keeps compiling) and an automation row's `value` carries the 32-bit scaled value; `EventKindRow::Automation`, `ModifyRow::Line` and `ConditionTest::Automation` are appended; the line records live in `core-ffi/src/strip.rs` — cost if wrong: none.
26. **Ruling:** The list editor shows automation rows: Kind "Automation", Data 1 the target ("Fader · Built-in Synth" in the Strip Chart; "Fader · strip 3" in the list, which has no mixer names), Data 2 the value as 0.0–100.0 % (editable); the kind filter lists Automation — cost if wrong: none.
27. **Ruling:** New Swift tests go to `VizDAWAppModelTests` (pure Strip Chart maths, the model, view behaviour through internal entry points with `FakeSession`), as in 4c Ruling 26 — cost if wrong: none.

## Spec gaps and contradictions found while planning

- §4d lists a velocity-scale lane but `AutomationTarget` has no target for it (Ruling 2).
- "Normalised (0–1)" gives no law from a value to dB, pan or a parameter's range (Ruling 4); `SendIndex` is not defined (Ruling 5).
- "Applies them per block (sub-block accuracy: split at automation points, minimum 32 frames)" can mean per-sample gain curves or split graph runs (Rulings 10–11).
- "Stopping leaves the control at its last automated value until the user moves it" says nothing about deleted automation, and a plugin parameter cannot be "moved back" by VizDAW (Ruling 13).
- "Every operation is also reachable numerically in the select-and-modify panel" while tempo is not an event the panel can select (Ruling 20: tempo lines act on the selected lanes' sequences).
- §3.2 lists an `Automation` `EditCommand` group; one core modification run by `SelectAndModify` covers every Strip Chart edit, so none is added (Ruling 15).
- Parent §9 exact mode lists a scale-time drag that no editor has (Ruling 23).
- 4c's constraint "no existing record gains a field" meets automation rows (Ruling 25).
- Parent §9 lists EQ automation; spec §1 puts it out of scope until the mixer has EQ (not planned).

## File Structure

```
crates/core-model/
  src/automation.rs            targets, SendIndex, laws (Task 2)
  src/event.rs                 EventBody::Automation, EventKind::Automation (2)
  src/rule.rs, src/rules.rs    Rule::Automation (2); value check (2)
  src/strip.rs                 Line, LineShape, Period, Series, TimeCtx, reduce (3);
                               LineChange, LineTarget, LineOp, apply_line (4); tempo lines (5)
  src/meter.rs, src/tempo.rs   MeterMap::bars_at (3); TempoMap::bpm_at (5)
  src/modify.rs                ScaleTime floor (1); Curve/Wave helpers (3); Modify::Line,
                               Value covers programs and automation (4)
  src/command/select.rs        Line runs with the selection's lanes (4)
  tests/{select_modify,automation,strip_line,line_change,tempo_line,properties}.rs
crates/core-engine/
  src/schedule/automation.rs   Automation lanes, values, cuts (6)
  src/schedule/{mod,compile}.rs  Schedule.automation, compile, velocity scale, report (6)
  src/engine.rs                bind instruments, stepped params (6); graph controls (7)
  src/graph/controls.rs        StripControlSpec, Controls (7)
  src/graph/{exec,mixer,dsp,node,mod}.rs  ramps, controls, insert lookup (7); automate hooks (8)
  src/backend.rs, src/transport.rs, src/render.rs  automate hook (8); play_pos, boundary (8); runs (8)
  tests/{schedule_automation,graph_controls,automation,rt_alloc}.rs
crates/core-host/
  src/adapt.rs, src/shelf.rs   normalised → plain, merged param changes (9); stepped params (9)
  tests/{engine_plugins,rt_alloc_plugins}.rs
crates/core-ffi/
  src/session.rs, src/host.rs  edit_all panic, drain without HOST (1)
  src/events.rs                AutomationTargetRow, AutomationRow, EventRow.automation (2)
  src/strip.rs                 line records and conversions (4, 5)
  src/modify.rs, src/lib.rs    ConditionTest::Automation (2); ModifyRow::Line (4)
  tests/{edit,transport,events,strip}.rs
app/
  Sources/VizDAWAppModel/      ListRows/ListEdit/AutomationText (2); SelectModify, PanelFields (2, 4, 5);
                               StripLane (10); StripChart, EditorLayout, ExactMode,
                               EditorModel+Gestures (11); StripChartModel, AutomationTargets,
                               EditorTypes, DocumentModel (12); SelectModifyModel, ExactMode (15)
  Sources/VizDAWEditors/       StripChartView (13); ScaleTimeDrag, PianoRollView (15)
  Sources/VizDAW/              StripChartPane, DetailPane, EditorWindows (14); LineModifyEditor,
                               ModifyPanel (15)
  Tests/VizDAWAppModelTests/   AutomationRowsTests (2); SelectModifyTests (4, 5); StripLaneTests (10);
                               StripChartTests (11); StripChartModelTests (12, 14); StripChartViewTests (13);
                               ScaleTimeTests, LinePanelTests (15)
  README.md                    Strip Chart and automation checklist steps 68–80 (16)
```

---

### Task 1: Carried follow-ups — edit_all survives a panic, Scale Time keeps one tick, a device rebuild drains without the host lock

**Files:**
- Modify: `crates/core-model/src/modify.rs`
- Test: `crates/core-model/tests/select_modify.rs`
- Modify: `crates/core-ffi/src/session.rs`
- Test: `crates/core-ffi/tests/edit.rs`
- Modify: `crates/core-ffi/src/host.rs`
- Test: `crates/core-ffi/tests/transport.rs`

**Interfaces:**
- Consumes: `History::{begin, abort, commit, in_transaction}`, `ffi_guard`, `guard::panic_message`, `host::{HOST, DRAIN, Host::create, Host::attach, Host::load}`.
- Produces: `vizdaw_ffi::session::inject_edit_all_panic(label: Option<&str>, step: usize)` (`test-hooks` only: the next `edit_all` named `label` panics before running command `step`, 0-based); `host::rebuild` and `struct Rebuild` (private). Behaviour: `Session::edit_all` returns `VizError::Internal` after a panic with the transaction aborted and `edit_label` cleared; `Modify::ScaleTime` floors a positive length at 1 tick; `host::handle_events(slot) -> Result<Option<Rebuild>, VizError>` no longer sleeps or rebuilds under `HOST`.

Ruling 1.

- [ ] **Step 1: Write the failing tests**

Append to `crates/core-model/tests/select_modify.rs`:

```rust
#[test]
fn scale_time_keeps_a_note_with_a_length_at_least_one_tick_long() {
    // 1/1 000 000 of a half note or a quarter is under one tick.
    let d = notes_after(Modify::ScaleTime {
        num: 1,
        den: 1_000_000,
        anchor: Tick(0),
    });
    assert_eq!(durations(&d), vec![1; 4]);
    // A zero-length note stays zero.
    let zero = notes_after(change(ValueTarget::Duration, ChangeOp::Set, constant(0)));
    let d = try_sm(
        &zero,
        &lane_sel(&NOTES),
        Modify::ScaleTime {
            num: 3,
            den: 1,
            anchor: Tick(0),
        },
    )
    .unwrap()
    .doc;
    assert_eq!(durations(&d), vec![0; 4]);
}
```

Append to `crates/core-ffi/tests/edit.rs`:

```rust
#[test]
fn a_panic_inside_edit_all_aborts_the_step_and_closes_it() {
    let f = fixture();
    let s = Session::from_document(f.doc.clone()).unwrap();
    let start = state(&s);
    let delete = ev(EventEdit::Delete {
        events: vec![f.note.0],
    });
    let markers = EditCommand::Sequence {
        edit: SequenceEdit::SetMarkers {
            seq: f.s.0,
            markers: vec![],
        },
    };
    // Only an edit_all with this label panics (tests run in parallel).
    vizdaw_ffi::session::inject_edit_all_panic(Some("Panics"), 1);
    let r = s.edit_all("Panics".into(), vec![delete, markers], None);
    vizdaw_ffi::session::inject_edit_all_panic(None, 0);
    assert!(matches!(r, Err(VizError::Internal { .. })), "{r:?}");
    assert_eq!(state(&s), start, "the first command was rolled back");
    assert_eq!(s.snapshot().unwrap().undo_label, None, "no undo step");
    // The transaction is closed: plain edits apply again.
    s.edit(move_note(&f, Q, None), None).unwrap();
    assert_eq!(note_tick(&s, &f), Q);
}
```

Append to `crates/core-ffi/tests/transport.rs`:

```rust
#[test]
fn a_device_rebuild_drains_without_holding_the_host_lock() {
    let _g = serial();
    let (s, song) = demo();
    s.play(song).unwrap();
    ms(40);
    simulate_event(BackendEvent::DefaultDeviceChanged);
    // The owner's poll handles the event on another thread: 300 ms of
    // drain, then the rebuild.
    let poller = std::sync::Arc::clone(&s);
    let poll = std::thread::spawn(move || poller.transport().map(|t| t.playing));
    ms(100);
    let t0 = Instant::now();
    let _ = vizdaw_ffi::host::factory_owner();
    let waited = t0.elapsed();
    assert!(
        waited < Duration::from_millis(150),
        "another host call waited {waited:?} for the drain"
    );
    assert!(poll.join().unwrap().is_ok());
    ms(30);
    assert!(t(&s).playing, "playback resumed after the rebuild");
}
```

- [ ] **Step 2: Run them to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test select_modify scale_time_keeps 2>&1 | tail -5
cargo test -p core-ffi --test edit a_panic_inside 2>&1 | tail -5
cargo test -p core-ffi --test transport a_device_rebuild_drains 2>&1 | tail -8
```

Expected: the Scale Time test fails (`[0, 0, 0, 0]` vs `[1, 1, 1, 1]`); the edit test does not compile (`cannot find function inject_edit_all_panic`); the transport test fails ("another host call waited 2xx ms").

- [ ] **Step 3: Floor Scale Time lengths**

In `crates/core-model/src/modify.rs`, Find:

```rust
            e.tick = *anchor + Tick(scale((e.tick - *anchor).0, *num, *den));
            if let EventBody::Note { duration, .. } = &mut e.body {
                *duration = Tick(scale(duration.0, *num, *den));
            }
```

Replace:

```rust
            e.tick = *anchor + Tick(scale((e.tick - *anchor).0, *num, *den));
            if let EventBody::Note { duration, .. } = &mut e.body {
                // A note that had a length keeps at least one tick, as
                // Resize and relative changes do (Plan 4d Ruling 1).
                let scaled = scale(duration.0, *num, *den);
                *duration = Tick(if duration.0 > 0 { scaled.max(1) } else { scaled });
            }
```

- [ ] **Step 4: Abort `edit_all` on a panic**

In `crates/core-ffi/src/session.rs`, Find:

```rust
/// Editor edits (spec §3.2, §3.8).
#[uniffi::export]
impl Session {
```

Replace:

```rust
/// The `edit_all` that panics (test hook): its label and the 0-based
/// command it panics before.
#[cfg(feature = "test-hooks")]
static EDIT_ALL_PANIC: Mutex<Option<(String, usize)>> = Mutex::new(None);

/// Makes the next `edit_all` named `label` panic before command `step`
/// (0-based); `None` clears it (test hook, Plan 4d Ruling 1).
#[cfg(feature = "test-hooks")]
pub fn inject_edit_all_panic(label: Option<&str>, step: usize) {
    *lock(&EDIT_ALL_PANIC) = label.map(|l| (l.to_string(), step));
}

/// Panics when `edit_all` `label` reaches the injected `step`.
#[cfg(feature = "test-hooks")]
#[allow(clippy::panic)]
fn edit_all_hook(label: &str, step: usize) {
    let hit = lock(&EDIT_ALL_PANIC)
        .as_ref()
        .is_some_and(|(l, s)| l == label && *s == step);
    if hit {
        panic!("injected edit_all panic");
    }
}

#[cfg(not(feature = "test-hooks"))]
fn edit_all_hook(_label: &str, _step: usize) {}

/// Editor edits (spec §3.2, §3.8).
#[uniffi::export]
impl Session {
```

Find:

```rust
    /// Runs `cmds` in order as one undo step named `label`: all of them or
    /// none (a refused command restores the document from before the
    /// first, and its error is returned). For a list Delete of mixed rows.
```

Replace:

```rust
    /// Runs `cmds` in order as one undo step named `label`: all of them or
    /// none (a refused command restores the document from before the
    /// first, and its error is returned; a panic inside a command does too
    /// and returns `Internal`). For a list Delete of mixed rows. None of
    /// these commands removes a running plugin: mixer edits, which capture
    /// a plugin's state before it goes, are not editor commands.
```

Find:

```rust
            {
                let mut inner = lock(&self.inner);
                inner.history.begin(label.clone())?;
                inner.edit_label = Some(label);
            }
            for c in core {
                if let Err(e) = self.execute_with(c, &filter) {
                    let mut inner = lock(&self.inner);
                    inner.edit_label = None;
                    inner.history.abort()?;
                    inner.version += 1;
                    inner.reload_host(self.id);
                    self.commit_held(&mut inner);
                    return Err(e);
                }
            }
```

Replace:

```rust
            {
                let mut inner = lock(&self.inner);
                inner.history.begin(label.clone())?;
                inner.edit_label = Some(label.clone());
            }
            // A panic inside a command must not leave the transaction open:
            // `ffi_guard` alone would report it but keep `edit_label` set.
            let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                for (step, c) in core.into_iter().enumerate() {
                    edit_all_hook(&label, step);
                    self.execute_with(c, &filter)?;
                }
                Ok::<(), VizError>(())
            }));
            let failure = match run {
                Ok(Ok(())) => None,
                Ok(Err(e)) => Some(e),
                Err(payload) => Some(VizError::Internal {
                    message: crate::guard::panic_message(payload.as_ref()),
                }),
            };
            if let Some(e) = failure {
                let mut inner = lock(&self.inner);
                inner.edit_label = None;
                if inner.history.in_transaction() {
                    inner.history.abort()?;
                }
                inner.version += 1;
                inner.reload_host(self.id);
                self.commit_held(&mut inner);
                return Err(e);
            }
```

- [ ] **Step 5: Drain and rebuild without the host lock**

In `crates/core-ffi/src/host.rs`, Find:

```rust
/// Handles device events for the owner (mirrors `vizdaw-play`'s live
/// loop): a buffer-size change restarts the output; a lost device, a new
/// default device or a rate change rebuild output and engine, recompile,
/// and resume at the same tick. A failed rebuild leaves no host. The
/// owner's plugins follow the new engine's rate (`attach`).
fn handle_events(
    slot: &mut Option<Host>,
    doc: &Document,
    plugins: Option<&mut PluginHost>,
) -> Result<(), VizError> {
    let Some(h) = slot.as_mut() else {
        return Ok(());
    };
    for ev in h.take_events() {
        match ev {
            BackendEvent::BufferSizeChanged(n) => {
                if let Some(h) = slot.as_mut() {
                    h.restart_output(n)?;
                }
            }
            BackendEvent::DeviceLost
            | BackendEvent::DefaultDeviceChanged
            | BackendEvent::SampleRateChanged(_) => {
                let Some(mut old) = slot.take() else {
                    return Ok(());
                };
                let at = old.engine.transport_tick();
                let playing = old.engine.state() == PlayState::Playing;
                if !matches!(ev, BackendEvent::DeviceLost) {
                    // The device still renders: let the note-offs play.
                    let _ = old.engine.stop();
                    std::thread::sleep(DRAIN);
                }
                let (owner, seq, looped) = (old.owner, old.seq, old.looped);
                // Output first (frees the renderer), then the engine: its
                // processors go back to the plugin host.
                drop(old);
                let mut new = Host::create()?;
                new.attach(owner, plugins);
                new.load(doc, seq, looped)?;
                new.engine.locate_tick(at)?;
                if playing {
                    new.engine.play()?;
                }
                new.owner = owner;
                new.seq = seq;
                *slot = Some(new);
                // The rebuilt host replaces any later queued events.
                return Ok(());
            }
        }
    }
    Ok(())
}
```

Replace:

```rust
/// A rebuild `handle_events` started: the old host is out of the slot;
/// [`rebuild`] finishes it without the host lock.
struct Rebuild {
    old: Host,
    at: Tick,
    playing: bool,
    /// The device still renders: its note-offs get `DRAIN` to play.
    drain: bool,
}

/// Handles device events for the owner (mirrors `vizdaw-play`'s live
/// loop): a buffer-size change restarts the output at once; a lost device,
/// a new default device or a rate change take the host out of `slot` and
/// return the rebuild, which the caller runs with [`rebuild`] after
/// dropping the host lock (Plan 4d Ruling 1: the drain sleeps 300 ms).
fn handle_events(slot: &mut Option<Host>) -> Result<Option<Rebuild>, VizError> {
    let Some(h) = slot.as_mut() else {
        return Ok(None);
    };
    for ev in h.take_events() {
        match ev {
            BackendEvent::BufferSizeChanged(n) => {
                if let Some(h) = slot.as_mut() {
                    h.restart_output(n)?;
                }
            }
            BackendEvent::DeviceLost
            | BackendEvent::DefaultDeviceChanged
            | BackendEvent::SampleRateChanged(_) => {
                let Some(mut old) = slot.take() else {
                    return Ok(None);
                };
                let at = old.engine.transport_tick();
                let playing = old.engine.state() == PlayState::Playing;
                let drain = !matches!(ev, BackendEvent::DeviceLost);
                if drain {
                    let _ = old.engine.stop();
                }
                // The rebuilt host replaces any later queued events.
                return Ok(Some(Rebuild {
                    old,
                    at,
                    playing,
                    drain,
                }));
            }
        }
    }
    Ok(None)
}

/// Finishes a rebuild without the host lock: lets a still-running output
/// play its note-offs, drops it (output first, then the engine, whose
/// processors go back to the plugin host), makes a new host for the
/// owner, recompiles and resumes at the same tick. The owner's plugins
/// follow the new engine's rate (`attach`). A failed rebuild leaves no
/// host; a host another call made meanwhile wins.
fn rebuild(r: Rebuild, doc: &Document, plugins: Option<&mut PluginHost>) -> Result<(), VizError> {
    let Rebuild {
        old,
        at,
        playing,
        drain,
    } = r;
    let (owner, seq, looped) = (old.owner, old.seq, old.looped);
    if drain {
        std::thread::sleep(DRAIN);
    }
    drop(old);
    let mut new = Host::create()?;
    new.attach(owner, plugins);
    new.load(doc, seq, looped)?;
    new.engine.locate_tick(at)?;
    if playing {
        new.engine.play()?;
    }
    new.owner = owner;
    new.seq = seq;
    let mut slot = lock(&HOST);
    let spare = if slot.is_none() {
        *slot = Some(new);
        None
    } else {
        Some(new)
    };
    drop(slot);
    drop(spare);
    Ok(())
}
```

Find:

```rust
        } else {
            handle_events(&mut slot, doc, plugins.as_deref_mut())?;
        }
```

Replace:

```rust
        } else if let Some(r) = handle_events(&mut slot)? {
            // The drain and the rebuild run without the host lock; then
            // start over (another call may have changed the host).
            drop(slot);
            rebuild(r, doc, plugins.as_deref_mut())?;
            continue;
        }
```

Find:

```rust
    handle_events(&mut slot, doc, plugins)?;
    let Some(h) = slot.as_mut() else {
        return Ok(None);
    };
    h.collect_faults();
```

Replace:

```rust
    if let Some(r) = handle_events(&mut slot)? {
        drop(slot);
        rebuild(r, doc, plugins)?;
        slot = lock(&HOST);
    }
    let Some(h) = slot.as_mut().filter(|h| h.owner == owner) else {
        return Ok(None);
    };
    h.collect_faults();
```

- [ ] **Step 6: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test select_modify 2>&1 | tail -3
cargo test -p core-ffi --test edit 2>&1 | tail -3
cargo test -p core-ffi --test transport 2>&1 | tail -3
```

Expected: PASS, including `a_device_change_rebuilds_and_resumes_at_the_same_position` and the other existing rebuild tests.

- [ ] **Step 7: Gate and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace 2>&1 | grep -E "^test result|FAILED|panicked" | tail -60
scripts/build-ffi.sh && scripts/swift.sh build 2>&1 | tail -3 && scripts/swift.sh test 2>&1 | tail -3
git add crates/core-model/src/modify.rs crates/core-model/tests/select_modify.rs crates/core-ffi/src/session.rs crates/core-ffi/tests/edit.rs crates/core-ffi/src/host.rs crates/core-ffi/tests/transport.rs
git commit -m "fix: edit_all aborts on a panic, scale time keeps one tick, rebuild drains without the host lock

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
```

Expected: every suite passes (675 Rust tests listed).

---

### Task 2: Automation events — targets, laws, the event kind, the rule, FFI rows and list rows

**Files:**
- Create: `crates/core-model/src/automation.rs`
- Modify: `crates/core-model/src/lib.rs`, `crates/core-model/src/event.rs`, `crates/core-model/src/rules.rs`, `crates/core-model/src/rule.rs`
- Test: `crates/core-model/tests/automation.rs`, `crates/core-model/tests/properties.rs`
- Modify: `crates/core-engine/src/midi1.rs`
- Modify: `crates/core-ffi/src/events.rs`, `crates/core-ffi/src/modify.rs`, `crates/core-ffi/src/lib.rs`
- Test: `crates/core-ffi/tests/events.rs`
- Create: `app/Sources/VizDAWAppModel/AutomationText.swift`
- Modify: `app/Sources/VizDAWAppModel/ListRows.swift`, `app/Sources/VizDAWAppModel/ListEdit.swift`, `app/Sources/VizDAWAppModel/SelectModify.swift`, `app/Sources/VizDAWAppModel/PanelFields.swift`
- Test: `app/Tests/VizDAWAppModelTests/AutomationRowsTests.swift`

**Interfaces:**
- Consumes: `ids::{StripId, PluginInstanceId, DestId}`, `mixer::{ParamId, GAIN_FLOOR_DB, GAIN_MAX_DB}`, `rules::need`, `events::{row, spec, kind_row, kind_of}`.
- Produces (Rust): `core_model::automation::{SendIndex(pub u16), AutomationTarget::{StripFader(StripId), StripPan(StripId), StripMute(StripId), StripSolo(StripId), SendLevel(StripId, SendIndex), PluginParam(PluginInstanceId, ParamId), VelocityScale(DestId)}, AutomationTarget::is_stepped(self) -> bool, AutomationTarget::describe(self) -> String, FULL_SCALE: f64, unit(f64) -> f64, check_value(f64) -> bool, to_u32(f64) -> u32, from_u32(u32) -> f64, fader_db(f64) -> f64, fader_norm(f64) -> f64, pan_of(f64) -> f64, pan_norm(f64) -> f64, switch_on(f64) -> bool, velocity_scale_percent(f64) -> f64, param_plain(v, min, max, stepped) -> f64}`; `EventBody::Automation { target: AutomationTarget, value: f64 }`; `EventKind::Automation`; `Rule::Automation { target: AutomationTarget }`.
- Produces (FFI): `AutomationTargetRow::{StripFader { strip: u64 }, StripPan { strip }, StripMute { strip }, StripSolo { strip }, SendLevel { strip: u64, send: u16 }, PluginParam { slot: u64, param: u64 }, VelocityScale { dest: u64 }}`, `AutomationRow { target: AutomationTargetRow, value: f64 }`, `EventRow.automation: Option<AutomationRow>` (last field, default `None`), `EventKindRow::Automation`, `pub fn events::automation_target_row(AutomationTarget) -> AutomationTargetRow`, `pub fn events::automation_target_of(AutomationTargetRow) -> AutomationTarget`.
- Produces (FFI): `ConditionTest::Automation { target: AutomationTargetRow }` (appended; `Rule::Automation` both ways).
- Produces (Swift): `ListKind.automation`; `ConditionKind.automation` ("Automation Target", default target `.stripFader(strip: 0)`); `AutomationText.short(_ t: AutomationTargetRow) -> String`, `AutomationText.percent(_ v: Double) -> String`, `AutomationText.parsePercent(_ s: String) -> Double?`.

Rulings 2–7, 25, 26.

- [ ] **Step 1: Write the failing tests**

Create `crates/core-model/tests/automation.rs`:

```rust
//! Automation events (editors spec §4d, Plan 4d Rulings 2–7).

use core_model::automation::{
    AutomationTarget, SendIndex, check_value, fader_db, fader_norm, from_u32, pan_norm, pan_of,
    param_plain, switch_on, to_u32, velocity_scale_percent,
};
use core_model::command::{Command, EventCmd, SequenceCmd, apply};
use core_model::document::Document;
use core_model::event::{EventBody, EventKind, EventSpec};
use core_model::file::{from_json, to_json};
use core_model::ids::{DestId, EventId, LaneId, PluginInstanceId, SeqId, StripId};
use core_model::mixer::{GAIN_FLOOR_DB, ParamId};
use core_model::rule::{Rule, events_matching};
use core_model::selection::ViewFilter;
use core_model::time::Tick;

fn auto(t: i64, target: AutomationTarget, value: f64) -> EventSpec {
    EventSpec::new(Tick(t), EventBody::Automation { target, value })
}

fn doc_with(events: Vec<EventSpec>) -> core_model::Result<Document> {
    let d = apply(
        &Document::new(),
        &Command::Sequence(SequenceCmd::New { name: "S".into() }),
    )?
    .doc;
    Ok(apply(
        &d,
        &Command::Event(EventCmd::Insert {
            seq: SeqId(0),
            lane: LaneId(0),
            events,
        }),
    )?
    .doc)
}

#[test]
fn the_laws_match_the_rulings() {
    assert_eq!(fader_db(1.0), 12.0);
    assert!(fader_db(0.0) == GAIN_FLOOR_DB && fader_db(0.002) == GAIN_FLOOR_DB);
    assert!((fader_db(fader_norm(0.0))).abs() < 1e-9, "unity round-trips");
    assert!((fader_norm(0.0) - 0.630_957).abs() < 1e-6);
    assert!((fader_norm(-6.0) - fader_norm(-6.0 + 1e-12)).abs() < 1e-9);
    assert_eq!(fader_norm(GAIN_FLOOR_DB), 0.0);
    assert_eq!(fader_db(f64::NAN), GAIN_FLOOR_DB);
    assert_eq!((pan_of(0.0), pan_of(0.5), pan_of(1.0)), (-1.0, 0.0, 1.0));
    assert_eq!(pan_norm(-1.0), 0.0);
    assert!(!switch_on(0.49) && switch_on(0.5));
    assert_eq!(velocity_scale_percent(0.0), 1.0);
    assert_eq!(velocity_scale_percent(0.5), 100.0);
    assert_eq!(velocity_scale_percent(1.0), 250.0);
    assert_eq!(param_plain(0.25, 0.0, 2.0, false), 0.5);
    assert_eq!(param_plain(0.3, 0.0, 10.0, true), 3.0);
    assert_eq!((to_u32(0.0), to_u32(1.0)), (0, u32::MAX));
    assert_eq!(from_u32(to_u32(0.25)), to_u32(0.25) as f64 / u32::MAX as f64);
    assert!(check_value(0.0) && check_value(1.0));
    assert!(!check_value(-0.1) && !check_value(1.5) && !check_value(f64::NAN));
    assert!(AutomationTarget::StripMute(StripId(1)).is_stepped());
    assert!(!AutomationTarget::StripFader(StripId(1)).is_stepped());
}

#[test]
fn automation_events_check_their_value_and_are_not_midi() {
    let target = AutomationTarget::SendLevel(StripId(3), SendIndex(1));
    let d = doc_with(vec![auto(0, target, 0.25)]).unwrap();
    let (ev, _) = d.event(EventId(0)).unwrap();
    assert_eq!(ev.body.kind(), EventKind::Automation);
    assert!(!ev.body.is_midi());
    for bad in [1.5, -0.5, f64::NAN, f64::INFINITY] {
        assert!(
            doc_with(vec![auto(0, target, bad)]).is_err(),
            "{bad} was accepted"
        );
    }
    // A target that does not exist is accepted (it may come back by undo).
    assert!(doc_with(vec![auto(0, AutomationTarget::StripFader(StripId(999)), 0.5)]).is_ok());
}

#[test]
fn the_automation_rule_matches_its_target_only() {
    let fader = AutomationTarget::StripFader(StripId(1));
    let param = AutomationTarget::PluginParam(PluginInstanceId(2), ParamId(7));
    let d = doc_with(vec![
        auto(0, fader, 0.1),
        auto(10, param, 0.2),
        auto(20, AutomationTarget::VelocityScale(DestId(0)), 0.5),
    ])
    .unwrap();
    let lanes = [(SeqId(0), LaneId(0))].into_iter().collect();
    let found = |target| {
        events_matching(&d, &lanes, &Rule::Automation { target }, &ViewFilter::default())
            .into_iter()
            .map(|e| e.0)
            .collect::<Vec<_>>()
    };
    assert_eq!(found(fader), vec![0]);
    assert_eq!(found(param), vec![1]);
    let kind = Rule::Kind {
        kinds: vec![EventKind::Automation],
    };
    assert_eq!(
        events_matching(&d, &lanes, &kind, &ViewFilter::default()).len(),
        3
    );
}

#[test]
fn automation_serialises_as_an_appended_variant_and_round_trips() {
    let body = EventBody::Automation {
        target: AutomationTarget::StripFader(StripId(3)),
        value: 0.5,
    };
    assert_eq!(
        serde_json::to_value(&body).unwrap(),
        serde_json::json!({"type": "automation", "target": {"strip_fader": 3}, "value": 0.5})
    );
    let send = serde_json::to_value(EventBody::Automation {
        target: AutomationTarget::SendLevel(StripId(3), SendIndex(2)),
        value: 1.0,
    })
    .unwrap();
    assert_eq!(send["target"], serde_json::json!({"send_level": [3, 2]}));
    let d = doc_with(vec![auto(5, AutomationTarget::StripPan(StripId(1)), 0.75)]).unwrap();
    let text = to_json(&d).unwrap();
    assert_eq!(from_json(&text).unwrap(), d);
    // A project without automation (every saved project so far) still
    // loads: its JSON has no automation variant at all.
    let plain = doc_with(vec![]).unwrap();
    assert!(!to_json(&plain).unwrap().contains("automation"));
    assert_eq!(from_json(&to_json(&plain).unwrap()).unwrap(), plain);
}
```

In `crates/core-model/tests/properties.rs`, Find:

```rust
    Transpose {
        ev: usize,
        semis: i8,
    },
}
```

Replace:

```rust
    Transpose {
        ev: usize,
        semis: i8,
    },
    Automation {
        seq: usize,
        lane: usize,
        beat: i64,
        strip: u64,
        value: f64,
    },
}
```

Find:

```rust
        1 => (i(), -12i8..12).prop_map(|(ev, semis)| Op::Transpose { ev, semis }),
    ]
```

Replace:

```rust
        1 => (i(), -12i8..12).prop_map(|(ev, semis)| Op::Transpose { ev, semis }),
        2 => (i(), i(), 0i64..16, 0u64..4, 0.0f64..=1.0)
            .prop_map(|(seq, lane, beat, strip, value)| Op::Automation { seq, lane, beat, strip, value }),
    ]
```

Find:

```rust
        Op::Ref {
            seq,
            lane,
            target,
            beat,
            len,
            loops,
            fill,
            transpose,
        } => {
```

Replace:

```rust
        Op::Automation {
            seq,
            lane,
            beat,
            strip,
            value,
        } => {
            let seq = pick(&s, seq)?;
            Command::Event(EventCmd::Insert {
                seq,
                lane: lane_of(d, seq, lane)?,
                events: vec![EventSpec::new(
                    Tick(beat * Q),
                    EventBody::Automation {
                        target: core_model::automation::AutomationTarget::StripFader(
                            core_model::ids::StripId(strip),
                        ),
                        value,
                    },
                )],
            })
        }
        Op::Ref {
            seq,
            lane,
            target,
            beat,
            len,
            loops,
            fill,
            transpose,
        } => {
```

In `crates/core-ffi/tests/events.rs` (spec §6: every `EventBody` kind round-trips), Find:

```rust
                fade_out: Fade {
                    length: Tick(Q / 8),
                    shape: FadeShape::SCurve,
                },
            },
        ),
    ];
    b.events(seq, lane, specs).unwrap();
```

Replace:

```rust
                fade_out: Fade {
                    length: Tick(Q / 8),
                    shape: FadeShape::SCurve,
                },
            },
        ),
        EventSpec::new(
            Tick(3 * BAR),
            EventBody::Automation {
                target: core_model::automation::AutomationTarget::SendLevel(
                    core_model::ids::StripId(2),
                    core_model::automation::SendIndex(1),
                ),
                value: 0.375,
            },
        ),
    ];
    b.events(seq, lane, specs).unwrap();
```

Find:

```rust
    assert_eq!(rows.len(), 10);
```

Replace:

```rust
    assert_eq!(rows.len(), 11);
```

Find:

```rust
            EventKindRow::Reference,
            EventKindRow::AudioRegion,
        ]
    );
```

Replace:

```rust
            EventKindRow::Reference,
            EventKindRow::AudioRegion,
            EventKindRow::Automation,
        ]
    );
```

Append to the same file:

```rust
use core_engine::fixtures::Builder as AutoBuilder;
use vizdaw_ffi::{AutomationRow, AutomationTargetRow};

#[test]
fn automation_rows_round_trip_and_bad_ones_are_refused() {
    let mut b = AutoBuilder::new();
    let (seq, lane) = b.sequence("S", "Auto", None).unwrap();
    let s = vizdaw_ffi::Session::from_document(b.finish()).unwrap();
    let insert = |row: vizdaw_ffi::EventRow| vizdaw_ffi::EditCommand::Event {
        edit: vizdaw_ffi::EventEdit::Insert {
            seq: seq.0,
            lane: lane.0,
            events: vec![row],
        },
    };
    let a = AutomationRow {
        target: AutomationTargetRow::SendLevel { strip: 4, send: 1 },
        value: 0.25,
    };
    let mut r = vizdaw_ffi::EventRow::blank(0, 960, vizdaw_ffi::EventKindRow::Automation);
    r.automation = Some(a);
    s.edit(insert(r), None).unwrap();
    let rows = s.lane_events(seq.0, lane.0).unwrap().events;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].kind, vizdaw_ffi::EventKindRow::Automation);
    assert_eq!(rows[0].tick, 960);
    assert_eq!(rows[0].automation, Some(a));
    assert_eq!(rows[0].value, i64::from(core_model::automation::to_u32(0.25)));
    // Every target kind survives the trip.
    for target in [
        AutomationTargetRow::StripFader { strip: 1 },
        AutomationTargetRow::StripPan { strip: 1 },
        AutomationTargetRow::StripMute { strip: 1 },
        AutomationTargetRow::StripSolo { strip: 1 },
        AutomationTargetRow::PluginParam { slot: 9, param: 1 << 40 },
        AutomationTargetRow::VelocityScale { dest: 2 },
    ] {
        let mut r = vizdaw_ffi::EventRow::blank(0, 0, vizdaw_ffi::EventKindRow::Automation);
        r.automation = Some(AutomationRow { target, value: 1.0 });
        let spec = vizdaw_ffi::events::spec(&r).unwrap();
        let back = match spec.body {
            core_model::event::EventBody::Automation { target, value } => {
                (vizdaw_ffi::events::automation_target_row(target), value)
            }
            b => panic!("{b:?}"),
        };
        assert_eq!(back, (target, 1.0));
    }
    // No automation record, or a value outside 0–1: refused, no undo step.
    let before = s.snapshot().unwrap().undo_label;
    let bare = vizdaw_ffi::EventRow::blank(0, 0, vizdaw_ffi::EventKindRow::Automation);
    assert!(matches!(
        s.edit(insert(bare), None),
        Err(vizdaw_ffi::VizError::Model { .. })
    ));
    let mut high = vizdaw_ffi::EventRow::blank(0, 0, vizdaw_ffi::EventKindRow::Automation);
    high.automation = Some(AutomationRow {
        target: AutomationTargetRow::StripFader { strip: 1 },
        value: 1.5,
    });
    assert!(s.edit(insert(high), None).is_err());
    assert_eq!(s.snapshot().unwrap().undo_label, before);
}

#[test]
fn the_automation_condition_selects_one_target() {
    let mut b = AutoBuilder::new();
    let (seq, lane) = b.sequence("S", "Auto", None).unwrap();
    let fader = core_model::automation::AutomationTarget::StripFader(core_model::ids::StripId(1));
    let pan = core_model::automation::AutomationTarget::StripPan(core_model::ids::StripId(1));
    b.events(
        seq,
        lane,
        vec![
            EventSpec::new(Tick(0), EventBody::Automation { target: fader, value: 0.1 }),
            EventSpec::new(Tick(10), EventBody::Automation { target: pan, value: 0.2 }),
        ],
    )
    .unwrap();
    let s = Session::from_document(b.finish()).unwrap();
    s.set_selection(vizdaw_ffi::SelectionRow {
        time_range: None,
        lanes: vec![vizdaw_ffi::LaneRef {
            seq: seq.0,
            lane: lane.0,
        }],
        events: vec![],
    })
    .unwrap();
    let rule = vizdaw_ffi::RuleRow {
        conditions: vec![vizdaw_ffi::ConditionRow {
            test: vizdaw_ffi::ConditionTest::Automation {
                target: AutomationTargetRow::StripPan { strip: 1 },
            },
            negate: false,
        }],
        match_all: true,
    };
    s.edit(
        vizdaw_ffi::EditCommand::SelectAndModify {
            rule: Some(rule.clone()),
            mode: vizdaw_ffi::SelectModeRow::Select,
            modify: None,
            to_copy: false,
        },
        None,
    )
    .unwrap();
    let rows = s.lane_events(seq.0, lane.0).unwrap().events;
    assert_eq!(s.selection().unwrap().events, vec![rows[1].id]);
    // The condition reads back from the core rule unchanged.
    let core = vizdaw_ffi::modify::rule(&rule).unwrap();
    assert_eq!(vizdaw_ffi::modify::rule_row(&core), Some(rule));
}
```

Create `app/Tests/VizDAWAppModelTests/AutomationRowsTests.swift`:

```swift
import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

private func automationEvent(_ id: UInt64, _ target: AutomationTargetRow, _ value: Double) -> EventRow {
    var r = ListInsert.blankRow(kind: .automation, tick: 0)
    r.id = id
    r.automation = AutomationRow(target: target, value: value)
    r.value = Int64((value * Double(UInt32.max)).rounded())
    return r
}

@Suite struct AutomationRowsTests {
    @Test func automationRowsShowTheirTargetAndPercent() {
        let e = automationEvent(4, .sendLevel(strip: 3, send: 1), 0.25)
        let row = ListRow(.event(lane: listLaneA, row: e))
        let c = listContext()
        #expect(row.kind == .automation)
        #expect(ListText.cell(row, .kind, c) == "Automation")
        #expect(ListText.cell(row, .data1, c) == "Send 2 · strip 3")
        #expect(ListText.cell(row, .data2, c) == "25.0%")
        #expect(ListText.editable(row, .data2) && !ListText.editable(row, .data1))
        #expect(ListText.summary(row, c) == "Automation, bar 1 beat 1, Send 2 · strip 3, 25.0%")
        #expect(!ListKind.automation.isMidi)
        #expect(ListKind(.automation) == .automation && ListKind.automation.eventKind == .automation)
        #expect(AutomationText.short(.pluginParam(slot: 2, param: 7)) == "Parameter 7 · plugin 2")
        #expect(AutomationText.short(.velocityScale(dest: 5)) == "Velocity Scale · destination 5")
    }

    @Test func typingAPercentReplacesTheValue() throws {
        let e = automationEvent(4, .stripFader(strip: 1), 0.25)
        let row = ListRow(.event(lane: listLaneA, row: e))
        let change = try ListEdit.commit("50", column: .data2, row: row, context: listContext())
        var expected = e
        expected.automation = AutomationRow(target: .stripFader(strip: 1), value: 0.5)
        expected.value = Int64((0.5 * Double(UInt32.max)).rounded())
        #expect(change == .edit(.event(edit: .replace(event: 4, row: expected)), tick: 0))
        #expect(throws: ListEditError.self) {
            try ListEdit.commit("150", column: .data2, row: row, context: listContext())
        }
    }
}
```

- [ ] **Step 2: Run them to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test automation 2>&1 | tail -5
```

Expected: compile error (`unresolved import core_model::automation`).

- [ ] **Step 3: Write the targets and laws**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-model/src/automation.rs <<'VIZDAW_EOF'
//! Automation targets and how a normalised value drives each (editors
//! spec §4d; Plan 4d Rulings 2–5). Values are `f64` in `0.0..=1.0`.

use crate::ids::{DestId, PluginInstanceId, StripId};
use crate::mixer::{GAIN_FLOOR_DB, GAIN_MAX_DB, ParamId};
use serde::{Deserialize, Serialize};

/// A send's position in its strip's send list (Plan 4d Ruling 5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SendIndex(pub u16);

/// What an automation event drives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationTarget {
    /// A strip's fader.
    StripFader(StripId),
    /// A strip's pan (ignored on master).
    StripPan(StripId),
    /// A strip's mute (on at 0.5 and above).
    StripMute(StripId),
    /// A strip's solo (on at 0.5 and above).
    StripSolo(StripId),
    /// The level of one of a strip's sends.
    SendLevel(StripId, SendIndex),
    /// A plugin parameter, by its stable ID.
    PluginParam(PluginInstanceId, ParamId),
    /// A destination's velocity scale (Plan 4d Ruling 2), applied to note
    /// velocities when the sequence is compiled.
    VelocityScale(DestId),
}

impl AutomationTarget {
    /// Mute and solo hold each value until the next point; every other
    /// target ramps (a parameter the plugin declares stepped is the
    /// engine's to add, Plan 4d Ruling 8).
    pub fn is_stepped(self) -> bool {
        matches!(
            self,
            AutomationTarget::StripMute(_) | AutomationTarget::StripSolo(_)
        )
    }

    /// A short description for messages ("strip3 fader").
    pub fn describe(self) -> String {
        match self {
            AutomationTarget::StripFader(s) => format!("{s} fader"),
            AutomationTarget::StripPan(s) => format!("{s} pan"),
            AutomationTarget::StripMute(s) => format!("{s} mute"),
            AutomationTarget::StripSolo(s) => format!("{s} solo"),
            AutomationTarget::SendLevel(s, i) => {
                format!("{s} send {} level", u32::from(i.0) + 1)
            }
            AutomationTarget::PluginParam(p, id) => format!("{p} parameter {}", id.0),
            AutomationTarget::VelocityScale(d) => format!("{d} velocity scale"),
        }
    }
}

/// The 32-bit scale controllers use, `2³² − 1` (Plan 4d Ruling 18).
pub const FULL_SCALE: f64 = u32::MAX as f64;

/// `v` as a usable normalised value: NaN is 0, the rest clamped to 0–1.
pub fn unit(v: f64) -> f64 {
    if v.is_nan() { 0.0 } else { v.clamp(0.0, 1.0) }
}

/// True for a finite value in 0–1, what an automation event may hold
/// (Plan 4d Ruling 3).
pub fn check_value(v: f64) -> bool {
    v.is_finite() && (0.0..=1.0).contains(&v)
}

/// A normalised value on the 32-bit scale.
pub fn to_u32(v: f64) -> u32 {
    // Float-to-int `as` saturates.
    (unit(v) * FULL_SCALE).round() as u32
}

/// A 32-bit scale value as a normalised value.
pub fn from_u32(x: u32) -> f64 {
    f64::from(x) / FULL_SCALE
}

/// The fader or send level of normalised `v`: `12 + 60·log10(v)` dB, −∞
/// (`GAIN_FLOOR_DB`) once that reaches the floor; unity at v ≈ 0.631
/// (Plan 4d Ruling 4).
pub fn fader_db(v: f64) -> f64 {
    let v = unit(v);
    if v <= 0.0 {
        return GAIN_FLOOR_DB;
    }
    let db = GAIN_MAX_DB + 60.0 * v.log10();
    if db <= GAIN_FLOOR_DB { GAIN_FLOOR_DB } else { db }
}

/// The normalised value of a fader or send level (inverse of
/// [`fader_db`]).
pub fn fader_norm(db: f64) -> f64 {
    if db.is_nan() || db <= GAIN_FLOOR_DB {
        return 0.0;
    }
    10f64.powf((db.min(GAIN_MAX_DB) - GAIN_MAX_DB) / 60.0)
}

/// Pan −1 … +1 of normalised `v` (0.5 is the centre).
pub fn pan_of(v: f64) -> f64 {
    unit(v) * 2.0 - 1.0
}

/// The normalised value of pan `p`.
pub fn pan_norm(p: f64) -> f64 {
    if p.is_nan() {
        0.5
    } else {
        (p.clamp(-1.0, 1.0) + 1.0) / 2.0
    }
}

/// A switch (mute, solo) is on at 0.5 and above.
pub fn switch_on(v: f64) -> bool {
    unit(v) >= 0.5
}

/// The velocity-scale percentage of normalised `v`: 1–100 % over 0–0.5,
/// 100–250 % over 0.5–1.
pub fn velocity_scale_percent(v: f64) -> f64 {
    let v = unit(v);
    if v <= 0.5 {
        1.0 + 198.0 * v
    } else {
        100.0 + 300.0 * (v - 0.5)
    }
}

/// A plugin parameter's plain value of normalised `v` in `min..=max`,
/// rounded when the parameter is stepped.
pub fn param_plain(v: f64, min: f64, max: f64, stepped: bool) -> f64 {
    let p = min + unit(v) * (max - min);
    if stepped { p.round() } else { p }
}
VIZDAW_EOF
```

In `crates/core-model/src/lib.rs`, Find:

```rust
pub mod command;
pub mod document;
```

Replace:

```rust
pub mod automation;
pub mod command;
pub mod document;
```

- [ ] **Step 4: Add the event kind, the value check and the rule**

In `crates/core-model/src/event.rs`, Find:

```rust
use crate::ids::{DestId, EventId, MediaId, SeqId};
```

Replace:

```rust
use crate::automation::AutomationTarget;
use crate::ids::{DestId, EventId, MediaId, SeqId};
```

Find:

```rust
        /// Fade out.
        fade_out: Fade,
    },
}

/// Event kinds, for rules and view filters.
```

Replace:

```rust
        /// Fade out.
        fade_out: Fade,
    },
    /// An automation point: drives `target` to `value` from here (editors
    /// spec §4d); values between points ramp unless the target is stepped.
    Automation {
        /// What it drives.
        target: AutomationTarget,
        /// Normalised value, finite, 0–1.
        value: f64,
    },
}

/// Event kinds, for rules and view filters.
```

Find:

```rust
    /// [`EventBody::AudioRegion`].
    AudioRegion,
}
```

Replace:

```rust
    /// [`EventBody::AudioRegion`].
    AudioRegion,
    /// [`EventBody::Automation`].
    Automation,
}
```

Find:

```rust
            EventBody::AudioRegion { .. } => EventKind::AudioRegion,
        }
```

Replace:

```rust
            EventBody::AudioRegion { .. } => EventKind::AudioRegion,
            EventBody::Automation { .. } => EventKind::Automation,
        }
```

Find:

```rust
    /// Whether this body is routed to a MIDI destination (everything except
    /// references, audio regions and text meta-events, which the engine
    /// ignores for routing).
    pub fn is_midi(&self) -> bool {
        !matches!(
            self,
            EventBody::Reference { .. } | EventBody::AudioRegion { .. } | EventBody::Text { .. }
        )
    }
```

Replace:

```rust
    /// Whether this body is routed to a MIDI destination (everything except
    /// references, audio regions, text meta-events and automation, which the
    /// engine does not route).
    pub fn is_midi(&self) -> bool {
        !matches!(
            self,
            EventBody::Reference { .. }
                | EventBody::AudioRegion { .. }
                | EventBody::Text { .. }
                | EventBody::Automation { .. }
        )
    }
```

In `crates/core-model/src/rules.rs`, Find:

```rust
        EventBody::PitchBend { .. }
        | EventBody::ChannelPressure { .. }
        | EventBody::Text { .. } => {}
```

Replace:

```rust
        EventBody::Automation { value, .. } => need(
            crate::automation::check_value(*value),
            "automation.value",
            value,
        )?,
        EventBody::PitchBend { .. }
        | EventBody::ChannelPressure { .. }
        | EventBody::Text { .. } => {}
```

In `crates/core-model/src/rule.rs`, Find:

```rust
use crate::document::Document;
```

Replace:

```rust
use crate::automation::AutomationTarget;
use crate::document::Document;
```

Find:

```rust
    /// Starts inside a note of the same lane and take (`note.tick <= tick <
    /// note.tick + duration`, the event itself excluded) that matches `by`.
    Bracketed {
        /// What the bracketing note must match.
        by: Box<Rule>,
    },
}
```

Replace:

```rust
    /// Starts inside a note of the same lane and take (`note.tick <= tick <
    /// note.tick + duration`, the event itself excluded) that matches `by`.
    Bracketed {
        /// What the bracketing note must match.
        by: Box<Rule>,
    },
    /// An automation event driving `target`.
    Automation {
        /// The target.
        target: AutomationTarget,
    },
}
```

Find:

```rust
        Rule::Bracketed { by } => bracketed(doc, by, ev),
    }
}
```

Replace:

```rust
        Rule::Bracketed { by } => bracketed(doc, by, ev),
        Rule::Automation { target } => {
            matches!(ev.body, EventBody::Automation { target: t, .. } if t == *target)
        }
    }
}
```

In `crates/core-engine/src/midi1.rs`, Find:

```rust
        EventBody::Text { .. } | EventBody::Reference { .. } | EventBody::AudioRegion { .. } => {}
```

Replace:

```rust
        EventBody::Text { .. }
        | EventBody::Reference { .. }
        | EventBody::AudioRegion { .. }
        | EventBody::Automation { .. } => {}
```

- [ ] **Step 5: Add the FFI rows**

In `crates/core-ffi/src/events.rs`, Find:

```rust
use core_model::document::Document;
```

Replace:

```rust
use core_model::automation::{AutomationTarget, SendIndex};
use core_model::document::Document;
use core_model::ids::{PluginInstanceId, StripId};
use core_model::mixer::ParamId;
```

Find:

```rust
    /// An audio region.
    AudioRegion,
}
```

Replace:

```rust
    /// An audio region.
    AudioRegion,
    /// An automation point (4d).
    Automation,
}

/// What an automation event drives (core `AutomationTarget`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum AutomationTargetRow {
    /// A strip's fader.
    StripFader {
        /// Strip ID.
        strip: u64,
    },
    /// A strip's pan.
    StripPan {
        /// Strip ID.
        strip: u64,
    },
    /// A strip's mute.
    StripMute {
        /// Strip ID.
        strip: u64,
    },
    /// A strip's solo.
    StripSolo {
        /// Strip ID.
        strip: u64,
    },
    /// A send's level.
    SendLevel {
        /// Strip ID.
        strip: u64,
        /// The send's position in the strip's sends.
        send: u16,
    },
    /// A plugin parameter.
    PluginParam {
        /// Plugin instance (slot) ID.
        slot: u64,
        /// Stable parameter ID.
        param: u64,
    },
    /// A destination's velocity scale.
    VelocityScale {
        /// Destination ID.
        dest: u64,
    },
}

/// An automation point's target and normalised value (0–1).
#[derive(Debug, Clone, Copy, PartialEq, uniffi::Record)]
pub struct AutomationRow {
    /// What it drives.
    pub target: AutomationTargetRow,
    /// Normalised value.
    pub value: f64,
}

/// The row of a core target.
pub fn automation_target_row(t: AutomationTarget) -> AutomationTargetRow {
    match t {
        AutomationTarget::StripFader(s) => AutomationTargetRow::StripFader { strip: s.0 },
        AutomationTarget::StripPan(s) => AutomationTargetRow::StripPan { strip: s.0 },
        AutomationTarget::StripMute(s) => AutomationTargetRow::StripMute { strip: s.0 },
        AutomationTarget::StripSolo(s) => AutomationTargetRow::StripSolo { strip: s.0 },
        AutomationTarget::SendLevel(s, i) => AutomationTargetRow::SendLevel {
            strip: s.0,
            send: i.0,
        },
        AutomationTarget::PluginParam(p, id) => AutomationTargetRow::PluginParam {
            slot: p.0,
            param: id.0,
        },
        AutomationTarget::VelocityScale(d) => AutomationTargetRow::VelocityScale { dest: d.0 },
    }
}

/// The core target of a row.
pub fn automation_target_of(t: AutomationTargetRow) -> AutomationTarget {
    match t {
        AutomationTargetRow::StripFader { strip } => AutomationTarget::StripFader(StripId(strip)),
        AutomationTargetRow::StripPan { strip } => AutomationTarget::StripPan(StripId(strip)),
        AutomationTargetRow::StripMute { strip } => AutomationTarget::StripMute(StripId(strip)),
        AutomationTargetRow::StripSolo { strip } => AutomationTarget::StripSolo(StripId(strip)),
        AutomationTargetRow::SendLevel { strip, send } => {
            AutomationTarget::SendLevel(StripId(strip), SendIndex(send))
        }
        AutomationTargetRow::PluginParam { slot, param } => {
            AutomationTarget::PluginParam(PluginInstanceId(slot), ParamId(param))
        }
        AutomationTargetRow::VelocityScale { dest } => AutomationTarget::VelocityScale(DestId(dest)),
    }
}
```

Find:

```rust
    /// SysEx may hold 8-bit data.
    pub eight_bit: bool,
}
```

Replace:

```rust
    /// SysEx may hold 8-bit data.
    pub eight_bit: bool,
    /// An automation point's target and value (4d; `value` then holds the
    /// value on the 32-bit scale, Plan 4d Ruling 25).
    #[uniffi(default = None)]
    pub automation: Option<AutomationRow>,
}
```

Find:

```rust
            data: Vec::new(),
            eight_bit: false,
        }
    }
}
```

Replace:

```rust
            data: Vec::new(),
            eight_bit: false,
            automation: None,
        }
    }
}
```

Find:

```rust
        EventBody::AudioRegion { .. } => EventKindRow::AudioRegion,
    }
}
```

Replace:

```rust
        EventBody::AudioRegion { .. } => EventKindRow::AudioRegion,
        EventBody::Automation { .. } => EventKindRow::Automation,
    }
}
```

Find:

```rust
        EventKindRow::AudioRegion => K::AudioRegion,
    }
}
```

Replace:

```rust
        EventKindRow::AudioRegion => K::AudioRegion,
        EventKindRow::Automation => K::Automation,
    }
}
```

Find:

```rust
            r.fade_out = fade_row(*fade_out);
            r.duration = region_ticks(doc, tempo, ev.tick, *media, *length);
        }
    }
    r
}
```

Replace:

```rust
            r.fade_out = fade_row(*fade_out);
            r.duration = region_ticks(doc, tempo, ev.tick, *media, *length);
        }
        EventBody::Automation { target, value } => {
            r.automation = Some(AutomationRow {
                target: automation_target_row(*target),
                value: *value,
            });
            r.value = i64::from(core_model::automation::to_u32(*value));
        }
    }
    r
}
```

Find:

```rust
            fade_in: fade_of(&row.fade_in),
            fade_out: fade_of(&row.fade_out),
        },
    };
    Ok(EventSpec {
```

Replace:

```rust
            fade_in: fade_of(&row.fade_in),
            fade_out: fade_of(&row.fade_out),
        },
        EventKindRow::Automation => {
            let a = row
                .automation
                .ok_or_else(|| bad("an automation row needs its automation record"))?;
            EventBody::Automation {
                target: automation_target_of(a.target),
                value: a.value,
            }
        }
    };
    Ok(EventSpec {
```

In `crates/core-ffi/src/modify.rs`, Find:

```rust
        EventKind::AudioRegion => EventKindRow::AudioRegion,
    }
}
```

Replace:

```rust
        EventKind::AudioRegion => EventKindRow::AudioRegion,
        EventKind::Automation => EventKindRow::Automation,
    }
}
```

Find:

```rust
    /// Starts inside a note of this pitch range in its lane.
    BracketedBy {
        /// Lowest pitch.
        low: u8,
        /// Highest pitch.
        high: u8,
    },
}
```

Replace:

```rust
    /// Starts inside a note of this pitch range in its lane.
    BracketedBy {
        /// Lowest pitch.
        low: u8,
        /// Highest pitch.
        high: u8,
    },
    /// An automation point driving `target` (4d).
    Automation {
        /// The target.
        target: crate::events::AutomationTargetRow,
    },
}
```

Find:

```rust
        ConditionTest::BracketedBy { low, high } => Rule::Bracketed {
            by: Box::new(Rule::PitchRange {
                low: *low,
                high: *high,
            }),
        },
    })
}
```

Replace:

```rust
        ConditionTest::BracketedBy { low, high } => Rule::Bracketed {
            by: Box::new(Rule::PitchRange {
                low: *low,
                high: *high,
            }),
        },
        ConditionTest::Automation { target } => Rule::Automation {
            target: crate::events::automation_target_of(*target),
        },
    })
}
```

Find:

```rust
        Rule::All(_) | Rule::Any(_) | Rule::Not(_) => return None,
    })
}
```

Replace:

```rust
        Rule::Automation { target } => ConditionTest::Automation {
            target: crate::events::automation_target_row(*target),
        },
        Rule::All(_) | Rule::Any(_) | Rule::Not(_) => return None,
    })
}
```

In `crates/core-ffi/src/lib.rs`, Find:

```rust
pub use events::{
    EventKindRow, EventRow, FadeRow, FadeShapeRow, GroupChannel, LaneEvents, MarkerRow,
```

Replace:

```rust
pub use events::{
    AutomationRow, AutomationTargetRow, EventKindRow, EventRow, FadeRow, FadeShapeRow,
    GroupChannel, LaneEvents, MarkerRow,
```

(`cargo fmt` reflows the rest of that `pub use`.) If UniFFI 0.32 rejects `#[uniffi(default = None)]`, use `#[uniffi(default)]` on the field instead; either gives the Swift initialiser an `automation: AutomationRow? = nil` parameter.

- [ ] **Step 6: Show automation rows in the list (Swift)**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Sources/VizDAWAppModel/AutomationText.swift <<'VIZDAW_EOF'
import Foundation
import VizDAWCore

/// Automation targets and values as text where no mixer names are at hand
/// (the list editor, Plan 4d Ruling 26).
public enum AutomationText {
    public static func short(_ t: AutomationTargetRow) -> String {
        switch t {
        case .stripFader(let s): "Fader · strip \(s)"
        case .stripPan(let s): "Pan · strip \(s)"
        case .stripMute(let s): "Mute · strip \(s)"
        case .stripSolo(let s): "Solo · strip \(s)"
        case .sendLevel(let s, let i): "Send \(Int(i) + 1) · strip \(s)"
        case .pluginParam(let slot, let p): "Parameter \(p) · plugin \(slot)"
        case .velocityScale(let d): "Velocity Scale · destination \(d)"
        }
    }

    /// A normalised value as 0.0–100.0 %.
    public static func percent(_ v: Double) -> String {
        String(format: "%.1f%%", min(max(v.isNaN ? 0 : v, 0), 1) * 100)
    }

    /// Typed "0"…"100" (an optional "%") as a normalised value.
    public static func parsePercent(_ s: String) -> Double? {
        let t = s.replacingOccurrences(of: "%", with: "").trimmingCharacters(in: .whitespaces)
        guard let p = Double(t), p.isFinite, (0...100).contains(p) else { return nil }
        return p / 100
    }
}
VIZDAW_EOF
```

In `app/Sources/VizDAWAppModel/ListRows.swift`, Find:

```swift
    case note, cc, pitchBend, channelPressure, polyPressure, program, sysEx, text, reference, audioRegion
    case tempo, meter, marker
```

Replace:

```swift
    case note, cc, pitchBend, channelPressure, polyPressure, program, sysEx, text, reference, audioRegion
    case tempo, meter, marker
    case automation
```

Find:

```swift
        case .audioRegion: self = .audioRegion
        }
    }
```

Replace:

```swift
        case .audioRegion: self = .audioRegion
        case .automation: self = .automation
        }
    }
```

Find:

```swift
        case .audioRegion: .audioRegion
        case .tempo, .meter, .marker: nil
```

Replace:

```swift
        case .audioRegion: .audioRegion
        case .automation: .automation
        case .tempo, .meter, .marker: nil
```

Find:

```swift
        case .marker: "Marker"
        }
    }
```

Replace:

```swift
        case .marker: "Marker"
        case .automation: "Automation"
        }
    }
```

Find:

```swift
            case .audioRegion: return "Audio"
            case .pitchBend, .channelPressure: return ""
```

Replace:

```swift
            case .audioRegion: return "Audio"
            case .automation: return e.automation.map { AutomationText.short($0.target) } ?? ""
            case .pitchBend, .channelPressure: return ""
```

Find:

```swift
            case .audioRegion: return String(format: "%.1f dB", e.gainDb)
            }
        }
    }
```

Replace:

```swift
            case .audioRegion: return String(format: "%.1f dB", e.gainDb)
            case .automation: return e.automation.map { AutomationText.percent($0.value) } ?? ""
            }
        }
    }
```

Find:

```swift
                return [.note, .cc, .pitchBend, .channelPressure, .polyPressure, .program, .text, .reference, .audioRegion]
                    .contains(e.kind)
```

Replace:

```swift
                return [
                    .note, .cc, .pitchBend, .channelPressure, .polyPressure, .program, .text, .reference, .audioRegion,
                    .automation,
                ]
                .contains(e.kind)
```

In `app/Sources/VizDAWAppModel/ListEdit.swift`, Find:

```swift
            return replace { $0.gainDb = g }
        default:
            return .unchanged
        }
    }
```

Replace:

```swift
            return replace { $0.gainDb = g }
        case .automation:
            guard let a = e.automation else { return .unchanged }
            guard let v = AutomationText.parsePercent(t) else {
                throw ListEditError("Type a value from 0 to 100 %.")
            }
            return v == a.value
                ? .unchanged
                : replace {
                    $0.automation = AutomationRow(target: a.target, value: v)
                    $0.value = Int64((v * Double(UInt32.max)).rounded())
                }
        default:
            return .unchanged
        }
    }
```

In `app/Sources/VizDAWAppModel/SelectModify.swift`, Find:

```swift
    case time, kind, pitch, velocity, releaseVelocity, duration, value, controller, destination, metric, everyNth
    case bracketed
```

Replace:

```swift
    case time, kind, pitch, velocity, releaseVelocity, duration, value, controller, destination, metric, everyNth
    case bracketed, automation
```

Find:

```swift
        case .bracketed: "Inside a Note"
        }
    }
```

Replace:

```swift
        case .bracketed: "Inside a Note"
        case .automation: "Automation Target"
        }
    }
```

Find:

```swift
        case .bracketedBy: self = .bracketed
        }
    }
```

Replace:

```swift
        case .bracketedBy: self = .bracketed
        case .automation: self = .automation
        }
    }
```

Find:

```swift
        case .bracketed: return .bracketedBy(low: 0, high: 127)
        }
    }
}
```

Replace:

```swift
        case .bracketed: return .bracketedBy(low: 0, high: 127)
        case .automation: return .automation(target: .stripFader(strip: 0))
        }
    }
}
```

In `app/Sources/VizDAWAppModel/PanelFields.swift`, Find:

```swift
        case .everyNth: [Field("every", .bounded(1, 1_000_000)), Field("starting at", .count)]
        case .kinds, .dest: []
        }
    }
```

Replace:

```swift
        case .everyNth: [Field("every", .bounded(1, 1_000_000)), Field("starting at", .count)]
        case .kinds, .dest, .automation: []
        }
    }
```

Find:

```swift
        case let .everyNth(n, p): [Int64(n), Int64(p)]
        case .kinds, .dest: []
        }
    }
```

Replace:

```swift
        case let .everyNth(n, p): [Int64(n), Int64(p)]
        case .kinds, .dest, .automation: []
        }
    }
```

Find:

```swift
        case .everyNth: return .everyNth(n: UInt32(clamping: max(at(0), 1)), phase: UInt32(clamping: at(1)))
        case .kinds, .dest: return self
        }
    }
```

Replace:

```swift
        case .everyNth: return .everyNth(n: UInt32(clamping: max(at(0), 1)), phase: UInt32(clamping: at(1)))
        case .kinds, .dest, .automation: return self
        }
    }
```

(The panel's condition row shows no fields for an Automation Target condition until Task 15 adds its target menu; `SelectModifyTests.kindsRoundTripThroughTheirDefaults` and `fieldsAndNumbersAgree` now cover the new kind.)

- [ ] **Step 7: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test automation --test properties 2>&1 | grep -E "^test result|FAILED"
cargo test -p core-ffi --test events 2>&1 | tail -3
scripts/build-ffi.sh && scripts/swift.sh test --filter AutomationRowsTests 2>&1 | tail -5
```

Expected: PASS.

- [ ] **Step 8: Gate and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace 2>&1 | grep -E "^test result|FAILED|panicked" | tail -60
scripts/build-ffi.sh && scripts/swift.sh build 2>&1 | tail -3 && scripts/swift.sh test 2>&1 | tail -3
git add crates/core-model/src/automation.rs crates/core-model/src/lib.rs crates/core-model/src/event.rs crates/core-model/src/rules.rs crates/core-model/src/rule.rs crates/core-model/tests/automation.rs crates/core-model/tests/properties.rs crates/core-engine/src/midi1.rs crates/core-ffi/src/events.rs crates/core-ffi/src/modify.rs crates/core-ffi/src/lib.rs crates/core-ffi/tests/events.rs app/Sources/VizDAWAppModel/AutomationText.swift app/Sources/VizDAWAppModel/ListRows.swift app/Sources/VizDAWAppModel/ListEdit.swift app/Sources/VizDAWAppModel/SelectModify.swift app/Sources/VizDAWAppModel/PanelFields.swift app/Tests/VizDAWAppModelTests/AutomationRowsTests.swift
git commit -m "feat(core): automation events with targets, laws, rule, FFI rows and list rows

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
```

Expected: 681 Rust tests listed (675 + 4 + 2); Swift 486.

---

### Task 3: Strip Chart lines — shapes, periods, samples, series

**Files:**
- Create: `crates/core-model/src/strip.rs`
- Modify: `crates/core-model/src/lib.rs`, `crates/core-model/src/modify.rs`, `crates/core-model/src/meter.rs`
- Test: `crates/core-model/tests/strip_line.rs`

**Interfaces:**
- Consumes: `modify::{Curve, Wave}`, `TempoMap::tick_to_seconds`, `automation::{to_u32, from_u32, AutomationTarget::is_stepped}`.
- Produces: `core_model::strip::{MAX_SAMPLES: usize = 100_000, Period::{Ticks(Tick), Millis(u32), Bars(u32)}, LineShape::{Straight, Curved { curve: Curve }, Flat, Free { points: Vec<(Tick, i64)> }, Random { seed: u64 }, Oscillator { wave: Wave, period: Period, phase: u16 }}, Line { from: Tick, to: Tick, start: i64, end: i64, step: Tick, shape: LineShape }, Line::check(&self) -> Result<()>, Line::covers(&self, Tick) -> bool, Line::at(&self, Tick, TimeCtx) -> i64, Line::samples(&self, TimeCtx) -> Result<Vec<(Tick, i64)>>, TimeCtx<'a> { tempo: &'a TempoMap, meter: &'a MeterMap }, TimeCtx::of(&Sequence), reduce(&[(Tick, i64)], ramps: bool) -> Vec<(Tick, i64)>, Series::{Controller(u8), PitchBend, ChannelPressure, PolyPressure(u8), Program, Automation(AutomationTarget)}, Series::of(&EventBody) -> Option<Series>, Series::value(&EventBody) -> Option<i64>, Series::set_value(&mut EventBody, i64), Series::body(self, i64) -> EventBody, Series::ramps(self) -> bool, Series::check(self) -> Result<()>}`; `Curve::apply(self, f64) -> f64`, `Wave::value(self, f64) -> f64` (`modify.rs`; `Amount::at` uses them, unchanged results); `pub(crate) fn modify::{lerp, splitmix}`; `MeterMap::bars_at(&self, Tick) -> f64`.

Rulings 16–18. `at` and `samples` take a `TimeCtx` (the sequence's tempo and meter maps) for clock and meter-following periods.

- [ ] **Step 1: Write the failing tests**

Create `crates/core-model/tests/strip_line.rs`:

```rust
//! Strip Chart lines (Plan 4d Rulings 16–18).

use core_model::automation::{AutomationTarget, to_u32};
use core_model::event::EventBody;
use core_model::ids::StripId;
use core_model::meter::{KeySig, Meter, MeterEntry, MeterMap};
use core_model::modify::{Curve, Wave};
use core_model::strip::{Line, LineShape, MAX_SAMPLES, Period, Series, TimeCtx, reduce};
use core_model::tempo::TempoMap;
use core_model::time::{PPQ, Tick};

const Q: i64 = PPQ;

fn line(shape: LineShape) -> Line {
    Line {
        from: Tick(0),
        to: Tick(4 * Q),
        start: 0,
        end: 1000,
        step: Tick(Q),
        shape,
    }
}

fn at(l: &Line, t: i64) -> i64 {
    let (tempo, meter) = (TempoMap::default(), MeterMap::default());
    l.at(
        Tick(t),
        TimeCtx {
            tempo: &tempo,
            meter: &meter,
        },
    )
}

#[test]
fn shapes_give_their_values() {
    let s = line(LineShape::Straight);
    assert_eq!([at(&s, -5), at(&s, 0), at(&s, 2 * Q), at(&s, 4 * Q), at(&s, 9 * Q)], [0, 0, 500, 1000, 1000]);
    let c = line(LineShape::Curved {
        curve: Curve::EaseIn,
    });
    assert_eq!(at(&c, 2 * Q), 250, "x² at the middle");
    assert_eq!(at(&line(LineShape::Flat), 3 * Q), 0, "flat holds the start value");
    let f = line(LineShape::Free {
        points: vec![(Tick(Q), 100), (Tick(3 * Q), 900)],
    });
    assert_eq!([at(&f, 0), at(&f, 2 * Q), at(&f, 4 * Q)], [100, 500, 900]);
    let r = line(LineShape::Random { seed: 7 });
    let values: Vec<i64> = (0..64).map(|k| at(&r, k * 100)).collect();
    assert!(values.iter().all(|v| (0..=1000).contains(v)));
    assert_eq!(values, (0..64).map(|k| at(&r, k * 100)).collect::<Vec<_>>(), "fixed by the seed");
    let other: Vec<i64> = (0..64)
        .map(|k| at(&line(LineShape::Random { seed: 8 }), k * 100))
        .collect();
    assert_ne!(values, other);
}

#[test]
fn oscillators_count_their_period_in_ticks_milliseconds_or_bars() {
    let osc = |wave, period| {
        line(LineShape::Oscillator {
            wave,
            period,
            phase: 0,
        })
    };
    let sine = osc(Wave::Sine, Period::Ticks(Tick(Q)));
    assert_eq!(
        [at(&sine, 0), at(&sine, Q / 4), at(&sine, Q / 2), at(&sine, 3 * Q / 4)],
        [500, 1000, 500, 0]
    );
    let square = osc(Wave::Square, Period::Ticks(Tick(Q)));
    assert_eq!([at(&square, Q / 4), at(&square, 3 * Q / 4)], [1000, 0]);
    // At 120 BPM one second is two quarters.
    let clock = osc(Wave::Saw, Period::Millis(1000));
    let musical = osc(Wave::Saw, Period::Ticks(Tick(2 * Q)));
    for t in [0, Q / 3, Q, 3 * Q / 2] {
        assert!((at(&clock, t) - at(&musical, t)).abs() <= 1, "tick {t}");
    }
    // A phase of 250 thousandths starts a quarter period in.
    let shifted = line(LineShape::Oscillator {
        wave: Wave::Sine,
        period: Period::Ticks(Tick(Q)),
        phase: 250,
    });
    assert_eq!(at(&shifted, 0), 1000);
    // Following the meter: 4/4 then 3/4 from bar 2 (tick 4Q): one bar is
    // one period whatever its length.
    let meter = MeterMap::new(vec![
        MeterEntry {
            tick: Tick(0),
            meter: Meter { num: 4, den: 4 },
            key: KeySig::default(),
        },
        MeterEntry {
            tick: Tick(4 * Q),
            meter: Meter { num: 3, den: 4 },
            key: KeySig::default(),
        },
    ])
    .unwrap();
    assert_eq!(meter.bars_at(Tick(2 * Q)), 0.5);
    assert_eq!(meter.bars_at(Tick(4 * Q + 3 * Q)), 2.0);
    assert_eq!(meter.bars_at(Tick(-2 * Q)), -0.5, "pickup bars count back");
    let bars = Line {
        to: Tick(16 * Q),
        ..osc(Wave::Saw, Period::Bars(1))
    };
    let tempo = TempoMap::default();
    let ctx = TimeCtx {
        tempo: &tempo,
        meter: &meter,
    };
    // Saw: low at each bar start, half-way up mid-bar.
    assert_eq!(bars.at(Tick(4 * Q), ctx), 0);
    assert_eq!(bars.at(Tick(4 * Q + 3 * Q / 2), ctx), 500);
    assert_eq!(bars.at(Tick(4 * Q + 3 * Q), ctx), 0);
}

#[test]
fn samples_fall_on_steps_ends_and_free_corners() {
    let tempo = TempoMap::default();
    let meter = MeterMap::default();
    let ctx = TimeCtx {
        tempo: &tempo,
        meter: &meter,
    };
    let s = line(LineShape::Straight).samples(ctx).unwrap();
    assert_eq!(
        s,
        vec![(Tick(0), 0), (Tick(Q), 250), (Tick(2 * Q), 500), (Tick(3 * Q), 750), (Tick(4 * Q), 1000)]
    );
    let click = Line {
        to: Tick(0),
        ..line(LineShape::Straight)
    };
    assert_eq!(click.samples(ctx).unwrap(), vec![(Tick(0), 0)], "a click is one point");
    let free = Line {
        step: Tick(2 * Q),
        ..line(LineShape::Free {
            points: vec![(Tick(0), 0), (Tick(Q), 800), (Tick(4 * Q), 0)],
        })
    };
    let ticks: Vec<i64> = free.samples(ctx).unwrap().iter().map(|p| p.0.0).collect();
    assert_eq!(ticks, vec![0, Q, 2 * Q, 4 * Q], "the corner at Q is kept");
    let dense = Line {
        step: Tick(1),
        to: Tick(MAX_SAMPLES as i64 + 5),
        ..line(LineShape::Straight)
    };
    assert!(dense.samples(ctx).is_err(), "more than MAX_SAMPLES");
}

#[test]
fn bad_lines_are_refused() {
    let ok = line(LineShape::Straight);
    assert!(ok.check().is_ok());
    assert!(Line { to: Tick(-1), ..ok.clone() }.check().is_err(), "to before from");
    assert!(Line { step: Tick(0), ..ok.clone() }.check().is_err());
    assert!(
        line(LineShape::Free {
            points: vec![(Tick(5), 0), (Tick(5), 1)],
        })
        .check()
        .is_err(),
        "points out of order"
    );
    for (period, phase) in [(Period::Ticks(Tick(0)), 0), (Period::Millis(0), 0), (Period::Bars(0), 0), (Period::Bars(1), 1000)] {
        assert!(
            line(LineShape::Oscillator {
                wave: Wave::Sine,
                period,
                phase,
            })
            .check()
            .is_err(),
            "{period:?} {phase}"
        );
    }
}

#[test]
fn reduce_keeps_what_changes_the_curve() {
    let ramp = [(Tick(0), 0), (Tick(10), 10), (Tick(20), 20), (Tick(30), 31), (Tick(40), 40)];
    assert_eq!(reduce(&ramp, true), vec![(Tick(0), 0), (Tick(40), 40)], "within one unit of the chord");
    let bend = [(Tick(0), 0), (Tick(10), 10), (Tick(20), 50), (Tick(30), 50)];
    assert_eq!(reduce(&bend, true), vec![(Tick(0), 0), (Tick(10), 10), (Tick(20), 50), (Tick(30), 50)]);
    let held = [(Tick(0), 5), (Tick(10), 5), (Tick(20), 7), (Tick(30), 7), (Tick(40), 7)];
    assert_eq!(
        reduce(&held, false),
        vec![(Tick(0), 5), (Tick(20), 7), (Tick(40), 7)],
        "a holding series drops repeats but keeps its last sample"
    );
    assert_eq!(reduce(&[(Tick(3), 1)], true), vec![(Tick(3), 1)]);
    assert!(reduce(&[], false).is_empty());
}

#[test]
fn series_read_and_write_the_32_bit_scale() {
    let prog = EventBody::Program {
        program: 5,
        bank: Some((0, 1)),
    };
    assert_eq!(Series::of(&prog), Some(Series::Program));
    assert_eq!(Series::value(&prog), Some(5 << 25));
    let mut p = prog.clone();
    Series::set_value(&mut p, (9 << 25) + 12);
    assert_eq!(
        p,
        EventBody::Program {
            program: 9,
            bank: Some((0, 1)),
        },
        "the bank stays"
    );
    let fader = AutomationTarget::StripFader(StripId(1));
    let a = Series::Automation(fader).body(i64::from(u32::MAX) / 2);
    assert!(matches!(a, EventBody::Automation { value, .. } if (value - 0.5).abs() < 1e-9));
    assert_eq!(Series::value(&a), Some(i64::from(u32::MAX) / 2), "round-trips");
    assert_eq!(
        Series::value(&Series::Automation(fader).body(i64::from(to_u32(1.0)))),
        Some(i64::from(u32::MAX))
    );
    let cc = Series::Controller(7).body(-5);
    assert_eq!(cc, EventBody::Cc { index: 7, value: 0 }, "clamped");
    assert_eq!(
        Series::PolyPressure(60).body(i64::MAX),
        EventBody::PolyPressure {
            pitch: 60,
            value: u32::MAX,
        }
    );
    assert!(Series::Automation(fader).ramps());
    assert!(!Series::Automation(AutomationTarget::StripSolo(StripId(1))).ramps());
    assert!(!Series::Controller(1).ramps());
    assert!(Series::Controller(128).check().is_err());
    assert_eq!(Series::of(&EventBody::note(60, 1, Tick(1))), None);
}
```

- [ ] **Step 2: Run them to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test strip_line 2>&1 | tail -5
```

Expected: compile error (`unresolved import core_model::strip`).

- [ ] **Step 3: Share the curve and wave maths**

In `crates/core-model/src/modify.rs`, Find:

```rust
fn lerp(a: i64, b: i64, y: f64) -> i64 {
    (a as f64 + (b as f64 - a as f64) * y).round() as i64
}

fn splitmix(mut z: u64) -> u64 {
```

Replace:

```rust
pub(crate) fn lerp(a: i64, b: i64, y: f64) -> i64 {
    (a as f64 + (b as f64 - a as f64) * y).round() as i64
}

pub(crate) fn splitmix(mut z: u64) -> u64 {
```

Find:

```rust
impl Amount {
    /// The amount for event `id` at `t` when the changed events start
    /// between `first` and `last`.
```

Replace:

```rust
impl Curve {
    /// The curve's height (0–1) at `x` (0–1).
    pub fn apply(self, x: f64) -> f64 {
        match self {
            Curve::Straight => x,
            Curve::EaseIn => x * x,
            Curve::EaseOut => 1.0 - (1.0 - x) * (1.0 - x),
            Curve::SCurve => x * x * (3.0 - 2.0 * x),
        }
    }
}

impl Wave {
    /// The wave (−1 … 1) at phase `ph` (0 … 1 of a period).
    pub fn value(self, ph: f64) -> f64 {
        match self {
            Wave::Sine => (std::f64::consts::TAU * ph).sin(),
            Wave::Square => {
                if ph < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            Wave::Triangle => 1.0 - 4.0 * (ph - 0.5).abs(),
            Wave::Saw => 2.0 * ph - 1.0,
        }
    }
}

impl Amount {
    /// The amount for event `id` at `t` when the changed events start
    /// between `first` and `last`.
```

Find:

```rust
                let y = match curve {
                    Curve::Straight => x,
                    Curve::EaseIn => x * x,
                    Curve::EaseOut => 1.0 - (1.0 - x) * (1.0 - x),
                    Curve::SCurve => x * x * (3.0 - 2.0 * x),
                };
                lerp(from, to, y)
```

Replace:

```rust
                lerp(from, to, curve.apply(x))
```

Find:

```rust
                let ph = (t - first).0.saturating_add(phase.0).rem_euclid(p) as f64 / p as f64;
                let w = match wave {
                    Wave::Sine => (std::f64::consts::TAU * ph).sin(),
                    Wave::Square => {
                        if ph < 0.5 {
                            1.0
                        } else {
                            -1.0
                        }
                    }
                    Wave::Triangle => 1.0 - 4.0 * (ph - 0.5).abs(),
                    Wave::Saw => 2.0 * ph - 1.0,
                };
                lerp(low, high, (w + 1.0) / 2.0)
```

Replace:

```rust
                let ph = (t - first).0.saturating_add(phase.0).rem_euclid(p) as f64 / p as f64;
                lerp(low, high, (wave.value(ph) + 1.0) / 2.0)
```

- [ ] **Step 4: Count bars as a fraction**

In `crates/core-model/src/meter.rs`, Find:

```rust
    /// The first barline at or after `t`.
    pub fn barline_at_or_after(&self, t: Tick) -> Tick {
```

Replace:

```rust
    /// Bars from tick 0 to `t` as a fraction (negative in the pickup), each
    /// bar as long as its own meter: what an oscillator that follows the
    /// meter counts (Plan 4d Ruling 17).
    pub fn bars_at(&self, t: Tick) -> f64 {
        let bars = self.with_bars();
        let (e, bar0) = bars
            .iter()
            .take_while(|(e, _)| e.tick <= t)
            .last()
            .or_else(|| bars.first())
            .copied()
            .unwrap_or((FOUR_FOUR, 1));
        let bar_t = e.meter.bar_ticks().max(1);
        (bar0 - 1) as f64 + t.0.saturating_sub(e.tick.0) as f64 / bar_t as f64
    }

    /// The first barline at or after `t`.
    pub fn barline_at_or_after(&self, t: Tick) -> Tick {
```

- [ ] **Step 5: Write the lines and series**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-model/src/strip.rs <<'VIZDAW_EOF'
//! Strip Chart lines (editors spec §4d; Plan 4d Rulings 15–20): a shaped
//! line over a time range, sampled to draw new values or read at an
//! event's position as the amount of a change, and the controller series a
//! line draws.

use crate::automation::{self, AutomationTarget};
use crate::error::{ModelError, Result};
use crate::event::EventBody;
use crate::meter::MeterMap;
use crate::modify::{Curve, Wave, lerp, splitmix};
use crate::sequence::Sequence;
use crate::tempo::TempoMap;
use crate::time::Tick;
use serde::{Deserialize, Serialize};

/// The most samples one line may write.
pub const MAX_SAMPLES: usize = 100_000;

/// How an oscillator's period is measured (Plan 4d Ruling 17).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Period {
    /// Musical: this many ticks.
    Ticks(Tick),
    /// Clock: this many milliseconds, through the sequence's tempo map.
    Millis(u32),
    /// Following the meter: this many bars, each as long as its meter.
    Bars(u32),
}

/// A line's shape between its ends.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum LineShape {
    /// From `start` to `end` in a straight line.
    Straight,
    /// From `start` to `end` along a curve (fades).
    Curved {
        /// The curve.
        curve: Curve,
    },
    /// `start` all along.
    Flat,
    /// Through drawn points (ascending ticks), holding the first before it
    /// and the last after it.
    Free {
        /// `(tick, value)` points.
        points: Vec<(Tick, i64)>,
    },
    /// A value per tick between `start` and `end`, fixed by `seed`.
    Random {
        /// Seed.
        seed: u64,
    },
    /// A wave from `start` (its low) to `end` (its high).
    Oscillator {
        /// Wave.
        wave: Wave,
        /// Period.
        period: Period,
        /// Phase shift in thousandths of a period (0–999).
        phase: u16,
    },
}

/// A Strip Chart line over `[from, to]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Line {
    /// First tick.
    pub from: Tick,
    /// Last tick (at or after `from`).
    pub to: Tick,
    /// Value at `from` (an oscillator's low, a random line's one bound).
    pub start: i64,
    /// Value at `to` (an oscillator's high, a random line's other bound).
    pub end: i64,
    /// Distance between drawn samples, and the spacing of Thin and Thicken.
    pub step: Tick,
    /// Shape.
    pub shape: LineShape,
}

/// What clock and meter-following periods are measured against: the
/// sequence's maps.
#[derive(Debug, Clone, Copy)]
pub struct TimeCtx<'a> {
    /// Tempo map.
    pub tempo: &'a TempoMap,
    /// Meter map.
    pub meter: &'a MeterMap,
}

impl<'a> TimeCtx<'a> {
    /// The maps of sequence `s`.
    pub fn of(s: &'a Sequence) -> TimeCtx<'a> {
        TimeCtx {
            tempo: &s.tempo,
            meter: &s.meter,
        }
    }
}

impl Line {
    /// Refuses a line that cannot be drawn: `to` before `from`, a step of 0
    /// or less, free points out of order, an oscillator period of 0 or a
    /// phase above 999.
    pub fn check(&self) -> Result<()> {
        if self.to < self.from {
            return Err(ModelError::range("line.to", self.to.0));
        }
        if self.step.0 <= 0 {
            return Err(ModelError::range("line.step", self.step.0));
        }
        match &self.shape {
            LineShape::Free { points } => {
                if !points.windows(2).all(|w| matches!(w, [a, b] if a.0 < b.0)) {
                    return Err(ModelError::invalid(
                        "a free line's points must be in time order",
                    ));
                }
            }
            LineShape::Oscillator { period, phase, .. } => {
                let ok = match period {
                    Period::Ticks(t) => t.0 > 0,
                    Period::Millis(ms) => *ms > 0,
                    Period::Bars(n) => *n > 0,
                };
                if !ok {
                    return Err(ModelError::invalid(
                        "an oscillator's period must be longer than 0",
                    ));
                }
                if *phase > 999 {
                    return Err(ModelError::range("line.phase", phase));
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Whether tick `t` lies in `[from, to]`.
    pub fn covers(&self, t: Tick) -> bool {
        t >= self.from && t <= self.to
    }

    /// The line's value at `t` (ends held outside the range).
    pub fn at(&self, t: Tick, ctx: TimeCtx<'_>) -> i64 {
        let span = self.to.0.saturating_sub(self.from.0);
        let x = if span > 0 {
            (t.0.saturating_sub(self.from.0) as f64 / span as f64).clamp(0.0, 1.0)
        } else {
            0.0
        };
        match &self.shape {
            LineShape::Straight => lerp(self.start, self.end, x),
            LineShape::Curved { curve } => lerp(self.start, self.end, curve.apply(x)),
            LineShape::Flat => self.start,
            LineShape::Free { points } => free_at(points, t, self.start),
            LineShape::Random { seed } => random_at(self.start, self.end, *seed, t),
            LineShape::Oscillator {
                wave,
                period,
                phase,
            } => {
                let c = cycles(self.from, t, *period, ctx) + f64::from(*phase) / 1000.0;
                lerp(self.start, self.end, (wave.value(c.rem_euclid(1.0)) + 1.0) / 2.0)
            }
        }
    }

    /// The values to draw: every `step` from `from`, at `to`, and at a free
    /// line's own points inside the range.
    pub fn samples(&self, ctx: TimeCtx<'_>) -> Result<Vec<(Tick, i64)>> {
        self.check()?;
        let span = self.to.0.saturating_sub(self.from.0);
        let count = usize::try_from(span / self.step.0).unwrap_or(usize::MAX);
        if count > MAX_SAMPLES {
            return Err(ModelError::range("line.step", self.step.0));
        }
        let mut ticks: Vec<Tick> = Vec::with_capacity(count.saturating_add(2));
        let mut t = self.from;
        while t < self.to {
            ticks.push(t);
            t = t + self.step;
        }
        ticks.push(self.to);
        if let LineShape::Free { points } = &self.shape {
            ticks.extend(points.iter().map(|p| p.0).filter(|t| self.covers(*t)));
            ticks.sort();
            ticks.dedup();
        }
        Ok(ticks.into_iter().map(|t| (t, self.at(t, ctx))).collect())
    }
}

fn free_at(points: &[(Tick, i64)], t: Tick, fallback: i64) -> i64 {
    let n = points.partition_point(|p| p.0 <= t);
    match (n.checked_sub(1).and_then(|i| points.get(i)), points.get(n)) {
        (None, None) => fallback,
        (None, Some(next)) => next.1,
        (Some(prev), None) => prev.1,
        (Some(prev), Some(next)) => {
            let span = next.0.0.saturating_sub(prev.0.0);
            let x = if span > 0 {
                t.0.saturating_sub(prev.0.0) as f64 / span as f64
            } else {
                0.0
            };
            lerp(prev.1, next.1, x)
        }
    }
}

fn random_at(a: i64, b: i64, seed: u64, t: Tick) -> i64 {
    let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
    let span = i128::from(hi) - i128::from(lo) + 1;
    let h = i128::from(splitmix(seed ^ splitmix(t.0 as u64)));
    i64::try_from(i128::from(lo) + h % span).unwrap_or(hi)
}

/// Periods from `from` to `t`.
fn cycles(from: Tick, t: Tick, p: Period, ctx: TimeCtx<'_>) -> f64 {
    match p {
        Period::Ticks(n) => t.0.saturating_sub(from.0) as f64 / n.0.max(1) as f64,
        Period::Millis(ms) => {
            (ctx.tempo.tick_to_seconds(t) - ctx.tempo.tick_to_seconds(from)) * 1000.0
                / f64::from(ms.max(1))
        }
        Period::Bars(n) => (ctx.meter.bars_at(t) - ctx.meter.bars_at(from)) / f64::from(n.max(1)),
    }
}

/// The samples a draw writes (Plan 4d Ruling 16): the first and last
/// always; between them, on a holding series (`ramps` false) a sample equal
/// to the previous kept one goes; on a ramping series a sample within one
/// unit of the straight line from the previous kept sample to the next
/// sample goes, so a straight line writes two points.
pub fn reduce(samples: &[(Tick, i64)], ramps: bool) -> Vec<(Tick, i64)> {
    let mut out: Vec<(Tick, i64)> = Vec::with_capacity(samples.len());
    for (i, p) in samples.iter().enumerate() {
        let (Some(prev), Some(next)) = (out.last().copied(), samples.get(i + 1)) else {
            out.push(*p);
            continue;
        };
        let drop = if ramps {
            on_chord(prev, *p, *next)
        } else {
            prev.1 == p.1
        };
        if !drop {
            out.push(*p);
        }
    }
    out
}

/// `p` lies within one unit of the line from `a` to `b`.
fn on_chord(a: (Tick, i64), p: (Tick, i64), b: (Tick, i64)) -> bool {
    let dt = i128::from(b.0.0) - i128::from(a.0.0);
    if dt <= 0 {
        return false;
    }
    let dev = (i128::from(p.1) - i128::from(a.1)) * dt
        - (i128::from(b.1) - i128::from(a.1)) * (i128::from(p.0.0) - i128::from(a.0.0));
    dev.abs() <= dt
}

/// A controller-like series a line draws (Plan 4d Ruling 15).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Series {
    /// Control change `n`.
    Controller(u8),
    /// Pitch bend.
    PitchBend,
    /// Channel pressure.
    ChannelPressure,
    /// Polyphonic pressure on one pitch.
    PolyPressure(u8),
    /// Program changes.
    Program,
    /// Automation of one target.
    Automation(AutomationTarget),
}

impl Series {
    /// The series `b` belongs to.
    pub fn of(b: &EventBody) -> Option<Series> {
        Some(match b {
            EventBody::Cc { index, .. } => Series::Controller(*index),
            EventBody::PitchBend { .. } => Series::PitchBend,
            EventBody::ChannelPressure { .. } => Series::ChannelPressure,
            EventBody::PolyPressure { pitch, .. } => Series::PolyPressure(*pitch),
            EventBody::Program { .. } => Series::Program,
            EventBody::Automation { target, .. } => Series::Automation(*target),
            _ => return None,
        })
    }

    /// `b`'s value on the 32-bit scale (Plan 4d Ruling 18): controller,
    /// bend and pressure values as they are, programs × 2²⁵, automation
    /// × (2³² − 1).
    pub fn value(b: &EventBody) -> Option<i64> {
        match b {
            EventBody::Cc { value, .. }
            | EventBody::PitchBend { value }
            | EventBody::ChannelPressure { value }
            | EventBody::PolyPressure { value, .. } => Some(i64::from(*value)),
            EventBody::Program { program, .. } => Some(i64::from(*program) << 25),
            EventBody::Automation { value, .. } => Some(i64::from(automation::to_u32(*value))),
            _ => None,
        }
    }

    /// Sets `b`'s value from the 32-bit scale (clamped; a program keeps its
    /// bank).
    pub fn set_value(b: &mut EventBody, v: i64) {
        let v32 = u32::try_from(v.clamp(0, i64::from(u32::MAX))).unwrap_or(u32::MAX);
        match b {
            EventBody::Cc { value, .. }
            | EventBody::PitchBend { value }
            | EventBody::ChannelPressure { value }
            | EventBody::PolyPressure { value, .. } => *value = v32,
            EventBody::Program { program, .. } => {
                *program = u8::try_from(v32 >> 25).unwrap_or(127);
            }
            EventBody::Automation { value, .. } => *value = automation::from_u32(v32),
            _ => {}
        }
    }

    /// A body of this series holding 32-bit-scale value `v`.
    pub fn body(self, v: i64) -> EventBody {
        let mut b = match self {
            Series::Controller(index) => EventBody::Cc { index, value: 0 },
            Series::PitchBend => EventBody::PitchBend { value: 0 },
            Series::ChannelPressure => EventBody::ChannelPressure { value: 0 },
            Series::PolyPressure(pitch) => EventBody::PolyPressure { pitch, value: 0 },
            Series::Program => EventBody::Program {
                program: 0,
                bank: None,
            },
            Series::Automation(target) => EventBody::Automation { target, value: 0.0 },
        };
        Series::set_value(&mut b, v);
        b
    }

    /// Values ramp between points (continuous automation); everything
    /// else holds until the next event.
    pub fn ramps(self) -> bool {
        matches!(self, Series::Automation(t) if !t.is_stepped())
    }

    /// Refuses a controller number or pitch above 127.
    pub fn check(self) -> Result<()> {
        match self {
            Series::Controller(n) | Series::PolyPressure(n) if n > 127 => {
                Err(ModelError::range("series", n))
            }
            _ => Ok(()),
        }
    }
}
VIZDAW_EOF
```

In `crates/core-model/src/lib.rs`, Find:

```rust
pub mod state;
pub mod tempo;
```

Replace:

```rust
pub mod state;
pub mod strip;
pub mod tempo;
```

- [ ] **Step 6: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test strip_line --test select_modify 2>&1 | grep -E "^test result|FAILED|panicked"
```

Expected: PASS (`select_modify`'s shaped-amount tests prove `Amount::at` is unchanged).

- [ ] **Step 7: Gate and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace 2>&1 | grep -E "^test result|FAILED|panicked" | tail -60
git add crates/core-model/src/strip.rs crates/core-model/src/lib.rs crates/core-model/src/modify.rs crates/core-model/src/meter.rs crates/core-model/tests/strip_line.rs
git commit -m "feat(core): Strip Chart lines with shapes, periods, samples and series

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
```

Expected: 687 Rust tests listed (681 + 6).

---

### Task 4: `Modify::Line` on events — draw a series, set, scale, add, limit, compress, thin, thicken, legato — through the FFI

**Files:**
- Modify: `crates/core-model/src/strip.rs`, `crates/core-model/src/modify.rs`, `crates/core-model/src/command/select.rs`
- Test: `crates/core-model/tests/line_change.rs`
- Create: `crates/core-ffi/src/strip.rs`
- Modify: `crates/core-ffi/src/modify.rs`, `crates/core-ffi/src/lib.rs`
- Test: `crates/core-ffi/tests/strip.rs`
- Modify: `app/Sources/VizDAWAppModel/SelectModify.swift`, `app/Sources/VizDAWAppModel/PanelFields.swift`

**Interfaces:**
- Consumes: Task 3's `Line`, `Series`, `TimeCtx`, `reduce`; `modify::{read_value, write_value, apply_change, legato, density, narrow}` (made `pub(crate)`); `command::event::{edit_events, insert_specs}`; `Document::{event, lane, sequence, remove_event}`; Task 2's `events::{automation_target_row, automation_target_of}`.
- Produces (core): `strip::{LineTarget::{Value(ValueTarget), Series(Series)}, LineOp::{Draw, Set, Scale, Add, LimitMax, LimitMin, Compress, Thin, Thicken, Legato}, LineChange { target: LineTarget, op: LineOp, line: Line }, LineChange::name(&self) -> &'static str, LineChange::check(&self) -> Result<()>, pub(crate) apply_line(d, lanes: &OrdSet<(SeqId, LaneId)>, targets: &[EventId], c: &LineChange, r: &mut Report) -> Result<Vec<EventId>>}`; `Modify::Line(LineChange)`; `ValueTarget::Value` reads and writes programs and automation on the 32-bit scale (`Series::value`/`set_value`).
- Produces (FFI, `core-ffi/src/strip.rs`): `SeriesRow::{Controller { index: u8 }, PitchBend, ChannelPressure, PolyPressure { pitch: u8 }, Program, Automation { target: AutomationTargetRow }}`, `LineTargetRow::{Value { target: ValueTargetRow }, Series { series: SeriesRow }}` (Task 5 appends `Tempo`), `LineOpRow::{Draw, Set, Scale, Add, LimitMax, LimitMin, Compress, Thin, Thicken, Legato}`, `PeriodRow::{Ticks { ticks: i64 }, Millis { ms: u32 }, Bars { bars: u32 }}`, `LinePointRow { tick: i64, value: i64 }`, `LineShapeRow::{Straight, Curved { curve: CurveRow }, Flat, Free { points: Vec<LinePointRow> }, Random { seed: u64 }, Oscillator { wave: WaveRow, period: PeriodRow, phase: u16 }}`, `LineRow { from: i64, to: i64, start: i64, end: i64, step: i64, shape: LineShapeRow }`, `pub fn line_change(LineTargetRow, LineOpRow, &LineRow) -> LineChange`, `pub fn line_change_row(&LineChange) -> (LineTargetRow, LineOpRow, LineRow)`; `ModifyRow::Line { target: LineTargetRow, op: LineOpRow, line: LineRow }` (appended).
- Produces (Swift): `ModifyKind.line` ("Strip Chart Line"), `ModifyKind.lineTitle(_ t: LineTargetRow, _ op: LineOpRow) -> String`, `ModifyRow.isLine: Bool`, line `fields`/`numbers`/`with` (From, To, Start, End, Step; for oscillators Period and Phase); `SelectModifyConfig.command()` never sends a line to a copy.

Rulings 15, 16, 18, 19, 24, 25. Names (undo labels): Draw on a series "Draw Controllers", "Draw Program Changes" or "Draw Automation"; Thin "Thin Out"; Thicken "Thicken"; Compress "Compress"; Legato "Legato"; otherwise "Change Velocity", "Change Release Velocity", "Change Duration" or "Change Values" by target. A line acts only on events whose tick lies in `[from, to]`; a series target only on events of that series; `to_copy` with a line is `Invalid`.

- [ ] **Step 1: Write the failing tests**

Create `crates/core-model/tests/line_change.rs`:

```rust
//! Strip Chart lines that change events (Plan 4d Rulings 15–19).

use core_model::automation::AutomationTarget;
use core_model::command::{Command, EventCmd, LaneCmd, SequenceCmd, apply};
use core_model::document::Document;
use core_model::event::{EventBody, EventSpec};
use core_model::history::History;
use core_model::ids::{EventId, LaneId, SeqId, StripId};
use core_model::modify::{Modify, ValueTarget};
use core_model::rule::{Rule, SelectMode};
use core_model::selection::{Selection, ViewFilter};
use core_model::strip::{Line, LineChange, LineOp, LineShape, LineTarget, Series};
use core_model::time::{PPQ, Tick};

const Q: i64 = PPQ;
const S: SeqId = SeqId(0);
const L0: LaneId = LaneId(0);

/// seq0: lane 0 with notes (ids 0–3) at 0, Q, 2Q, 3Q (velocities 10, 20,
/// 30, 40, half a quarter long) and CC7 = 100 (ids 4–5) at 0 and 2Q;
/// lane 1 empty.
fn doc() -> (Document, LaneId) {
    let d = apply(
        &Document::new(),
        &Command::Sequence(SequenceCmd::New { name: "S".into() }),
    )
    .unwrap()
    .doc;
    let mut events: Vec<EventSpec> = (0..4)
        .map(|k| {
            EventSpec::new(
                Tick(k * Q),
                EventBody::note(60, 10 * (k as u16 + 1), Tick(Q / 2)),
            )
        })
        .collect();
    for t in [0, 2 * Q] {
        events.push(EventSpec::new(
            Tick(t),
            EventBody::Cc {
                index: 7,
                value: 100,
            },
        ));
    }
    let d = apply(
        &d,
        &Command::Event(EventCmd::Insert {
            seq: S,
            lane: L0,
            events,
        }),
    )
    .unwrap()
    .doc;
    let a = apply(
        &d,
        &Command::Lane(LaneCmd::Add {
            seq: S,
            name: "Other".into(),
            index: None,
        }),
    )
    .unwrap();
    let l1 = a
        .doc
        .sequence(S)
        .unwrap()
        .lanes
        .iter()
        .map(|l| l.id)
        .find(|id| *id != L0)
        .unwrap();
    (a.doc, l1)
}

fn line(from: i64, to: i64, start: i64, end: i64) -> Line {
    Line {
        from: Tick(from),
        to: Tick(to),
        start,
        end,
        step: Tick(Q),
        shape: LineShape::Straight,
    }
}

/// Runs `change` over its line's range in `lanes` as the Strip Chart does
/// (rule: the range; Select, or Refine on `selected` events).
fn run(
    h: &mut History,
    lanes: &[LaneId],
    selected: &[u64],
    refine: bool,
    change: LineChange,
) -> core_model::Result<()> {
    h.set_selection(Selection {
        time_range: None,
        lanes: lanes.iter().map(|l| (S, *l)).collect(),
        events: selected.iter().map(|e| EventId(*e)).collect(),
    });
    let (from, to) = (change.line.from, change.line.to);
    h.execute(
        &Command::SelectAndModify {
            rule: Some(Rule::TimeRange {
                start: from,
                end: to + Tick(1),
            }),
            mode: if refine {
                SelectMode::Refine
            } else {
                SelectMode::Select
            },
            modify: Some(Modify::Line(change)),
            to_copy: false,
        },
        &ViewFilter::default(),
    )
    .map(|_| ())
}

fn velocities(h: &History) -> Vec<u16> {
    (0..4)
        .map(|i| match h.doc().event(EventId(i)).unwrap().0.body {
            EventBody::Note { velocity, .. } => velocity,
            ref b => panic!("{b:?}"),
        })
        .collect()
}

fn velocity_line(op: LineOp, l: Line) -> LineChange {
    LineChange {
        target: LineTarget::Value(ValueTarget::Velocity),
        op,
        line: l,
    }
}

/// `(tick, body)` of lane `lane`'s events of series `s`, in time order.
fn series(h: &History, lane: LaneId, s: Series) -> Vec<(i64, EventBody)> {
    h.doc()
        .lane(S, lane)
        .unwrap()
        .active()
        .unwrap()
        .events
        .values()
        .filter(|e| Series::of(&e.body) == Some(s))
        .map(|e| (e.tick.0, e.body.clone()))
        .collect()
}

#[test]
fn set_scale_add_and_limits_follow_the_line_at_each_note() {
    let (d, _) = doc();
    let check = |op, l: Line, want: [u16; 4]| {
        let mut h = History::new(d.clone());
        run(&mut h, &[L0], &[], false, velocity_line(op, l)).unwrap();
        assert_eq!(velocities(&h), want, "{op:?}");
    };
    check(LineOp::Set, line(0, 3 * Q, 0, 3000), [0, 1000, 2000, 3000]);
    check(LineOp::Draw, line(0, 3 * Q, 0, 3000), [0, 1000, 2000, 3000]);
    // Scale amounts are percent along the line.
    check(LineOp::Scale, line(0, 3 * Q, 100, 400), [10, 40, 90, 160]);
    check(LineOp::Add, line(0, 3 * Q, -10, 20), [0, 20, 40, 60]);
    check(LineOp::LimitMax, line(0, 3 * Q, 15, 15), [10, 15, 15, 15]);
    check(LineOp::LimitMin, line(0, 3 * Q, 25, 25), [25, 25, 30, 40]);
    // Only the line's range changes.
    check(LineOp::Set, line(Q, 2 * Q, 7, 7), [10, 7, 7, 40]);
    let mut h = History::new(d.clone());
    run(&mut h, &[L0], &[], false, velocity_line(LineOp::Set, line(0, 3 * Q, 0, 3000))).unwrap();
    assert_eq!(h.undo_label(), Some("Change Velocity"));
}

#[test]
fn a_modifier_limits_the_line_to_the_selected_events() {
    let (d, _) = doc();
    let mut h = History::new(d);
    run(&mut h, &[L0], &[1, 3], true, velocity_line(LineOp::Set, line(0, 3 * Q, 5, 5))).unwrap();
    assert_eq!(velocities(&h), [10, 5, 30, 5]);
}

#[test]
fn a_line_over_no_events_changes_nothing() {
    let (d, _) = doc();
    let mut h = History::new(d.clone());
    run(&mut h, &[L0], &[], false, velocity_line(LineOp::Set, line(10 * Q, 12 * Q, 1, 1))).unwrap();
    assert_eq!(h.doc(), &d);
    assert_eq!(h.undo_label(), None, "no undo step");
}

#[test]
fn a_series_line_changes_only_that_series() {
    let (d, _) = doc();
    let mut h = History::new(d);
    let cc7 = Series::Controller(7);
    run(
        &mut h,
        &[L0],
        &[],
        false,
        LineChange {
            target: LineTarget::Series(cc7),
            op: LineOp::Set,
            line: line(0, 2 * Q, 0, i64::from(u32::MAX)),
        },
    )
    .unwrap();
    assert_eq!(
        series(&h, L0, cc7),
        vec![
            (0, EventBody::Cc { index: 7, value: 0 }),
            (
                2 * Q,
                EventBody::Cc {
                    index: 7,
                    value: u32::MAX,
                },
            ),
        ]
    );
    assert_eq!(velocities(&h), [10, 20, 30, 40], "notes untouched");
    assert_eq!(h.undo_label(), Some("Change Values"));
}

#[test]
fn drawing_replaces_the_series_in_every_selected_lane() {
    let (d, l1) = doc();
    let cc7 = Series::Controller(7);
    let mut h = History::new(d.clone());
    let full = i64::from(u32::MAX);
    run(
        &mut h,
        &[L0, l1],
        &[],
        false,
        LineChange {
            target: LineTarget::Series(cc7),
            op: LineOp::Draw,
            line: line(0, 2 * Q, 0, full),
        },
    )
    .unwrap();
    let want: Vec<i64> = vec![0, Q, 2 * Q];
    for lane in [L0, l1] {
        let got = series(&h, lane, cc7);
        assert_eq!(got.iter().map(|g| g.0).collect::<Vec<_>>(), want, "{lane:?}");
        assert_eq!(got[1].1, EventBody::Cc { index: 7, value: u32::MAX / 2 + 1 });
    }
    assert_eq!(velocities(&h), [10, 20, 30, 40]);
    assert_eq!(h.selection().events.len(), 6, "the drawn events are selected");
    assert_eq!(h.undo_label(), Some("Draw Controllers"));
    // Continuous automation ramps: a straight line is two points.
    let fader = Series::Automation(AutomationTarget::StripFader(StripId(1)));
    let mut h = History::new(d.clone());
    run(
        &mut h,
        &[L0],
        &[],
        false,
        LineChange {
            target: LineTarget::Series(fader),
            op: LineOp::Draw,
            line: line(0, 8 * Q, 0, full),
        },
    )
    .unwrap();
    assert_eq!(series(&h, L0, fader).len(), 2);
    assert_eq!(h.undo_label(), Some("Draw Automation"));
    // Programs hold: one event per new program number.
    let mut h = History::new(d.clone());
    run(
        &mut h,
        &[L0],
        &[],
        false,
        LineChange {
            target: LineTarget::Series(Series::Program),
            op: LineOp::Draw,
            line: Line {
                step: Tick(Q / 4),
                ..line(0, 4 * Q, 0, 4 << 25)
            },
        },
    )
    .unwrap();
    let programs: Vec<u8> = series(&h, L0, Series::Program)
        .into_iter()
        .map(|(_, b)| match b {
            EventBody::Program { program, .. } => program,
            b => panic!("{b:?}"),
        })
        .collect();
    assert_eq!(programs, vec![0, 1, 2, 3, 4]);
    // Drawing needs a lane.
    let mut h = History::new(d);
    assert!(run(
        &mut h,
        &[],
        &[],
        false,
        LineChange {
            target: LineTarget::Series(cc7),
            op: LineOp::Draw,
            line: line(0, Q, 0, 1),
        },
    )
    .is_err());
}

#[test]
fn compress_thin_thicken_and_legato() {
    let (d, _) = doc();
    let mut h = History::new(d.clone());
    // Compress maps the lowest (10) to 100 and the highest (40) to 400.
    run(&mut h, &[L0], &[], false, velocity_line(LineOp::Compress, line(0, 3 * Q, 100, 400))).unwrap();
    assert_eq!(velocities(&h), [100, 200, 300, 400]);
    assert_eq!(h.undo_label(), Some("Compress"));
    // Thicken CC7 between 0 and 2Q every half quarter, then thin it again.
    let cc7 = Series::Controller(7);
    let thick = LineChange {
        target: LineTarget::Series(cc7),
        op: LineOp::Thicken,
        line: Line {
            step: Tick(Q / 2),
            ..line(0, 2 * Q, 0, 0)
        },
    };
    let mut h = History::new(d.clone());
    run(&mut h, &[L0], &[], false, thick).unwrap();
    assert_eq!(series(&h, L0, cc7).len(), 5, "0, Q/2, Q, 3Q/2, 2Q");
    let thin = LineChange {
        target: LineTarget::Series(cc7),
        op: LineOp::Thin,
        line: line(0, 2 * Q, 0, 0),
    };
    run(&mut h, &[L0], &[], false, thin).unwrap();
    assert_eq!(
        series(&h, L0, cc7).iter().map(|s| s.0).collect::<Vec<_>>(),
        vec![0, Q, 2 * Q]
    );
    // Legato lengthens notes to the next one; it needs the length target.
    let mut h = History::new(d.clone());
    run(
        &mut h,
        &[L0],
        &[],
        false,
        LineChange {
            target: LineTarget::Value(ValueTarget::Duration),
            op: LineOp::Legato,
            line: line(0, 3 * Q, 0, 0),
        },
    )
    .unwrap();
    let first = match h.doc().event(EventId(0)).unwrap().0.body {
        EventBody::Note { duration, .. } => duration,
        ref b => panic!("{b:?}"),
    };
    assert_eq!(first, Tick(Q));
    let mut h = History::new(d);
    assert!(run(&mut h, &[L0], &[], false, velocity_line(LineOp::Legato, line(0, Q, 0, 0))).is_err());
}

#[test]
fn bad_lines_and_copies_are_refused_and_modify_again_repeats() {
    let (d, _) = doc();
    let mut h = History::new(d.clone());
    let bad = Line {
        from: Tick(5),
        to: Tick(1),
        ..line(0, 0, 0, 0)
    };
    assert!(run(&mut h, &[L0], &[], false, velocity_line(LineOp::Set, bad)).is_err());
    assert!(
        run(
            &mut h,
            &[L0],
            &[],
            false,
            LineChange {
                target: LineTarget::Series(Series::Controller(200)),
                op: LineOp::Draw,
                line: line(0, Q, 0, 0),
            },
        )
        .is_err()
    );
    h.set_selection(Selection {
        time_range: None,
        lanes: [(S, L0)].into_iter().collect(),
        events: [EventId(0)].into_iter().collect(),
    });
    assert!(
        h.execute(
            &Command::SelectAndModify {
                rule: None,
                mode: SelectMode::Select,
                modify: Some(Modify::Line(velocity_line(LineOp::Add, line(0, 3 * Q, 1, 1)))),
                to_copy: true,
            },
            &ViewFilter::default(),
        )
        .is_err(),
        "a line cannot change copies"
    );
    assert_eq!(h.doc(), &d);
    // Modify Again repeats a line on the selection.
    run(&mut h, &[L0], &[], false, velocity_line(LineOp::Add, line(0, 3 * Q, 1, 1))).unwrap();
    h.modify_again(&ViewFilter::default()).unwrap();
    assert_eq!(velocities(&h), [12, 22, 32, 42]);
}
```

- [ ] **Step 2: Run them to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test line_change 2>&1 | tail -5
```

Expected: compile errors (`no variant named Line`, `cannot find LineChange in strip`).

- [ ] **Step 3: Write the line changes**

Append to `crates/core-model/src/strip.rs`:

```rust
/// What a Strip Chart line acts on (Plan 4d Ruling 15).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LineTarget {
    /// A value of the events in range (velocity, release velocity, length,
    /// or any value on the 32-bit scale).
    Value(ValueTarget),
    /// One series of the events in range; drawing writes it into every
    /// selected lane.
    Series(Series),
}

/// What a Strip Chart line does (Plan 4d Rulings 15, 19).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LineOp {
    /// Write the line: a series is replaced in range; a note value is set.
    Draw,
    /// Each value becomes the line's.
    Set,
    /// Each value times the line's, in percent.
    Scale,
    /// Each value plus the line's.
    Add,
    /// Each value at most the line's.
    LimitMax,
    /// Each value at least the line's.
    LimitMin,
    /// The lowest value in range becomes `start`, the highest `end`.
    Compress,
    /// Events closer than `step` to the last kept one go.
    Thin,
    /// Straight-line values every `step` between neighbours.
    Thicken,
    /// Notes end where the next note in range starts (lengths only).
    Legato,
}

/// A Strip Chart edit: `op` with `line` over `target` (the one
/// modification every Strip Chart operation is, Plan 4d Ruling 15).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LineChange {
    /// What it acts on.
    pub target: LineTarget,
    /// What it does.
    pub op: LineOp,
    /// The line.
    pub line: Line,
}

impl LineChange {
    /// The undo label.
    pub fn name(&self) -> &'static str {
        match (self.op, self.target) {
            (LineOp::Draw, LineTarget::Series(Series::Automation(_))) => "Draw Automation",
            (LineOp::Draw, LineTarget::Series(Series::Program)) => "Draw Program Changes",
            (LineOp::Draw, LineTarget::Series(_)) => "Draw Controllers",
            (LineOp::Thin, _) => "Thin Out",
            (LineOp::Thicken, _) => "Thicken",
            (LineOp::Compress, _) => "Compress",
            (LineOp::Legato, _) => "Legato",
            (_, LineTarget::Value(ValueTarget::Velocity)) => "Change Velocity",
            (_, LineTarget::Value(ValueTarget::ReleaseVelocity)) => "Change Release Velocity",
            (_, LineTarget::Value(ValueTarget::Duration)) => "Change Duration",
            _ => "Change Values",
        }
    }

    /// Refuses a bad line, a bad series, and legato on anything but lengths.
    pub fn check(&self) -> Result<()> {
        self.line.check()?;
        if let LineTarget::Series(s) = self.target {
            s.check()?;
        }
        if self.op == LineOp::Legato && self.target != LineTarget::Value(ValueTarget::Duration) {
            return Err(ModelError::invalid("legato changes note lengths only"));
        }
        Ok(())
    }
}

/// Runs `c` for select-and-modify: drawing writes into `lanes`; every other
/// operation changes the events of `targets` its line covers. Returns the
/// events the selection holds afterwards.
pub(crate) fn apply_line(
    d: &mut Document,
    lanes: &OrdSet<(SeqId, LaneId)>,
    targets: &[EventId],
    c: &LineChange,
    r: &mut Report,
) -> Result<Vec<EventId>> {
    c.check()?;
    match (c.target, c.op) {
        (LineTarget::Series(s), LineOp::Draw) => draw_series(d, lanes, s, &c.line, r),
        (target, op) => change_events(d, targets, target, op, &c.line, r),
    }
}

/// Replaces series `s` in `[from, to]` of every lane in `lanes` with the
/// line's reduced samples (Plan 4d Ruling 16).
fn draw_series(
    d: &mut Document,
    lanes: &OrdSet<(SeqId, LaneId)>,
    s: Series,
    line: &Line,
    r: &mut Report,
) -> Result<Vec<EventId>> {
    if lanes.is_empty() {
        return Err(ModelError::invalid("select a lane to draw in"));
    }
    let mut created = Vec::new();
    for (seq, lane) in lanes.iter().copied() {
        // Each sample as the series can hold it (programs are 7-bit), so
        // reduce sees the values that will be written.
        let held: Vec<(Tick, i64)> = line
            .samples(TimeCtx::of(d.sequence(seq)?))?
            .into_iter()
            .map(|(t, v)| (t, Series::value(&s.body(v)).unwrap_or(v)))
            .collect();
        let samples = reduce(&held, s.ramps());
        let old: Vec<EventId> = d
            .lane(seq, lane)?
            .active()
            .map(|t| {
                t.events
                    .values()
                    .filter(|e| line.covers(e.tick) && Series::of(&e.body) == Some(s))
                    .map(|e| e.id)
                    .collect()
            })
            .unwrap_or_default();
        for id in old {
            d.remove_event(id)?;
            r.removed.push(AnyId::Event(id));
        }
        let specs: Vec<EventSpec> = samples
            .iter()
            .map(|(t, v)| EventSpec::new(*t, s.body(*v)))
            .collect();
        created.extend(insert_specs(d, seq, lane, &specs, r)?);
    }
    Ok(created)
}

/// Changes the events of `ids` that `target` covers inside the line's range.
fn change_events(
    d: &mut Document,
    ids: &[EventId],
    target: LineTarget,
    op: LineOp,
    line: &Line,
    r: &mut Report,
) -> Result<Vec<EventId>> {
    let vt = match target {
        LineTarget::Value(vt) => vt,
        LineTarget::Series(_) => ValueTarget::Value,
    };
    let mut hit: Vec<(EventId, Tick)> = Vec::new();
    for id in ids.iter().copied().collect::<BTreeSet<_>>() {
        let (ev, _) = d.event(id)?;
        let fits = match target {
            LineTarget::Value(vt) => read_value(ev, vt).is_some(),
            LineTarget::Series(s) => Series::of(&ev.body) == Some(s),
        };
        if fits && line.covers(ev.tick) {
            hit.push((id, ev.tick));
        }
    }
    let changed: Vec<EventId> = hit.iter().map(|h| h.0).collect();
    match op {
        LineOp::Legato => legato(d, &changed, r)?,
        LineOp::Compress => apply_change(
            d,
            &changed,
            vt,
            ChangeOp::Compress {
                low: line.start,
                high: line.end,
            },
            &Amount::Constant { value: 0 },
            r,
        )?,
        LineOp::Thin => {
            return density(d, &changed, Density::Thin { spacing: line.step }, r);
        }
        LineOp::Thicken => {
            return density(d, &changed, Density::Thicken { spacing: line.step }, r);
        }
        LineOp::Draw
        | LineOp::Set
        | LineOp::Scale
        | LineOp::Add
        | LineOp::LimitMax
        | LineOp::LimitMin => {
            for (id, tick) in &hit {
                let (a, v) = {
                    let (ev, loc) = d.event(*id)?;
                    let s = d.sequence(loc.seq)?;
                    (line.at(*tick, TimeCtx::of(s)), read_value(ev, vt).unwrap_or(0))
                };
                let next = match op {
                    LineOp::Scale => narrow(i128::from(v) * i128::from(a) / 100),
                    LineOp::Add => v.saturating_add(a),
                    LineOp::LimitMax => v.min(a),
                    LineOp::LimitMin => v.max(a),
                    _ => a,
                };
                // A note that had a length keeps at least one tick unless
                // the line sets it (as Change does).
                let next = if vt == ValueTarget::Duration
                    && v > 0
                    && !matches!(op, LineOp::Draw | LineOp::Set)
                {
                    next.max(1)
                } else {
                    next
                };
                edit_events(d, &[*id], r, |e| {
                    write_value(e, vt, next);
                    Ok(())
                })?;
            }
        }
    }
    Ok(changed)
}
```

In `crates/core-model/src/strip.rs`, Find:

```rust
use crate::automation::{self, AutomationTarget};
use crate::error::{ModelError, Result};
use crate::event::EventBody;
use crate::meter::MeterMap;
use crate::modify::{Curve, Wave, lerp, splitmix};
use crate::sequence::Sequence;
use crate::tempo::TempoMap;
use crate::time::Tick;
use serde::{Deserialize, Serialize};
```

Replace:

```rust
use crate::automation::{self, AutomationTarget};
use crate::command::Report;
use crate::command::event::{edit_events, insert_specs};
use crate::document::Document;
use crate::error::{ModelError, Result};
use crate::event::{EventBody, EventSpec};
use crate::ids::{AnyId, EventId, LaneId, SeqId};
use crate::meter::MeterMap;
use crate::modify::{
    Amount, ChangeOp, Curve, Density, ValueTarget, Wave, apply_change, density, legato, lerp,
    narrow, read_value, splitmix, write_value,
};
use crate::sequence::Sequence;
use crate::tempo::TempoMap;
use crate::time::Tick;
use im::OrdSet;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
```

- [ ] **Step 4: Make `Modify::Line` a modification**

In `crates/core-model/src/modify.rs`, Find:

```rust
use crate::rule::{Rule, SelectMode};
use crate::time::Tick;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
```

Replace:

```rust
use crate::rule::{Rule, SelectMode};
use crate::strip::{LineChange, Series};
use crate::time::Tick;
use im::OrdSet;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
```

Find:

```rust
    /// Change one value of each fitting event by `op` with `amount`.
    Change {
        /// Which value.
        target: ValueTarget,
        /// How the amount combines with it.
        op: ChangeOp,
        /// The amount (percent for `Scale`).
        amount: Amount,
    },
}
```

Replace:

```rust
    /// Change one value of each fitting event by `op` with `amount`.
    Change {
        /// Which value.
        target: ValueTarget,
        /// How the amount combines with it.
        op: ChangeOp,
        /// The amount (percent for `Scale`).
        amount: Amount,
    },
    /// A Strip Chart line (Plan 4d Ruling 15).
    Line(LineChange),
}
```

Find:

```rust
    /// CC, pitch-bend and pressure values (32-bit).
    Value,
}
```

Replace:

```rust
    /// CC, pitch-bend and pressure values, programs (× 2²⁵) and automation
    /// (× (2³² − 1)), all on the 32-bit scale (Plan 4d Ruling 18).
    Value,
}
```

Find:

```rust
            Modify::Change { target, .. } => match target {
                ValueTarget::Velocity => "Change Velocity",
                ValueTarget::ReleaseVelocity => "Change Release Velocity",
                ValueTarget::Duration => "Change Duration",
                ValueTarget::Value => "Change Values",
            },
        }
    }
}
```

Replace:

```rust
            Modify::Change { target, .. } => match target {
                ValueTarget::Velocity => "Change Velocity",
                ValueTarget::ReleaseVelocity => "Change Release Velocity",
                ValueTarget::Duration => "Change Duration",
                ValueTarget::Value => "Change Values",
            },
            Modify::Line(c) => c.name(),
        }
    }
}
```

Find:

```rust
        // Applied to the whole set in `apply_modify`.
        Modify::Change { .. } | Modify::Density(_) | Modify::Delete => {}
```

Replace:

```rust
        // Applied to the whole set in `apply_modify` (lines in `strip`).
        Modify::Change { .. } | Modify::Density(_) | Modify::Delete | Modify::Line(_) => {}
```

Find:

```rust
/// Notes end where the next of `ids` in their lane starts.
fn legato(d: &mut Document, ids: &[EventId], r: &mut Report) -> Result<()> {
```

Replace:

```rust
/// Notes end where the next of `ids` in their lane starts.
pub(crate) fn legato(d: &mut Document, ids: &[EventId], r: &mut Report) -> Result<()> {
```

Find:

```rust
fn read_value(e: &Event, t: ValueTarget) -> Option<i64> {
```

Replace:

```rust
pub(crate) fn read_value(e: &Event, t: ValueTarget) -> Option<i64> {
```

Find:

```rust
        (ValueTarget::Duration, EventBody::Note { duration, .. }) => Some(duration.0),
        (
            ValueTarget::Value,
            EventBody::Cc { value, .. }
            | EventBody::PitchBend { value }
            | EventBody::ChannelPressure { value }
            | EventBody::PolyPressure { value, .. },
        ) => Some(i64::from(*value)),
        _ => None,
    }
}

fn write_value(e: &mut Event, t: ValueTarget, v: i64) {
    let v16 = u16::try_from(v.clamp(0, i64::from(u16::MAX))).unwrap_or(u16::MAX);
    let v32 = u32::try_from(v.clamp(0, i64::from(u32::MAX))).unwrap_or(u32::MAX);
    match (t, &mut e.body) {
```

Replace:

```rust
        (ValueTarget::Duration, EventBody::Note { duration, .. }) => Some(duration.0),
        (ValueTarget::Value, b) => Series::value(b),
        _ => None,
    }
}

pub(crate) fn write_value(e: &mut Event, t: ValueTarget, v: i64) {
    let v16 = u16::try_from(v.clamp(0, i64::from(u16::MAX))).unwrap_or(u16::MAX);
    match (t, &mut e.body) {
```

Find:

```rust
        (ValueTarget::Duration, EventBody::Note { duration, .. }) => *duration = Tick(v.max(0)),
        (
            ValueTarget::Value,
            EventBody::Cc { value, .. }
            | EventBody::PitchBend { value }
            | EventBody::ChannelPressure { value }
            | EventBody::PolyPressure { value, .. },
        ) => *value = v32,
        _ => {}
    }
}

fn narrow(v: i128) -> i64 {
```

Replace:

```rust
        (ValueTarget::Duration, EventBody::Note { duration, .. }) => *duration = Tick(v.max(0)),
        (ValueTarget::Value, b) => Series::set_value(b, v),
        _ => {}
    }
}

pub(crate) fn narrow(v: i128) -> i64 {
```

Find:

```rust
fn apply_change(
    d: &mut Document,
```

Replace:

```rust
pub(crate) fn apply_change(
    d: &mut Document,
```

Find:

```rust
fn density(
    d: &mut Document,
```

Replace:

```rust
pub(crate) fn density(
    d: &mut Document,
```

Find:

```rust
        Modify::Substitute(Substitute::Pitch { pitch }) if *pitch > 127 => {
            Err(ModelError::range("substitute.pitch", pitch))
        }
        _ => Ok(()),
```

Replace:

```rust
        Modify::Substitute(Substitute::Pitch { pitch }) if *pitch > 127 => {
            Err(ModelError::range("substitute.pitch", pitch))
        }
        Modify::Line(c) => c.check(),
        _ => Ok(()),
```

Find:

```rust
        Modify::Change { target, op, amount } => apply_change(d, ids, *target, *op, amount, r)?,
        _ => edit_events(d, ids, r, |e| modify_one(e, m))?,
```

Replace:

```rust
        Modify::Change { target, op, amount } => apply_change(d, ids, *target, *op, amount, r)?,
        // Without the selection's lanes (select-and-modify passes them, see
        // `command::select`), a draw is refused.
        Modify::Line(c) => return crate::strip::apply_line(d, &OrdSet::new(), ids, c, r),
        _ => edit_events(d, ids, r, |e| modify_one(e, m))?,
```

In `crates/core-model/src/command/select.rs`, Find:

```rust
    if let Some(m) = modify {
        // Deleting copies would leave the document as it was.
```

Replace:

```rust
    if let Some(m) = modify {
        if to_copy && matches!(m, Modify::Line(_)) {
            return Err(crate::error::ModelError::invalid(
                "a Strip Chart line cannot change copies",
            ));
        }
        // Deleting copies would leave the document as it was.
```

Find:

```rust
        targets = apply_modify(d, &targets, m, r)?;
```

Replace:

```rust
        targets = match m {
            // A line draws into the selection's lanes (Plan 4d Ruling 16).
            Modify::Line(c) => crate::strip::apply_line(d, &base.lanes, &targets, c, r)?,
            _ => apply_modify(d, &targets, m, r)?,
        };
```


- [ ] **Step 5: Run the core tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model 2>&1 | grep -E "^test result|FAILED|panicked"
```

Expected: PASS (all `core-model` suites; `select_modify` shows value changes still work on controllers). `core-ffi` does not build yet: its `modify_row` matches `Modify` exhaustively — the next steps add the rows.

- [ ] **Step 6: Write the failing FFI test**

Create `crates/core-ffi/tests/strip.rs`:

```rust
//! Strip Chart lines through the FFI (Plan 4d Rulings 15, 25).

use core_engine::fixtures::{Builder, Q};
use core_model::event::{EventBody, EventSpec};
use core_model::time::Tick;
use vizdaw_ffi::strip::{
    LineOpRow, LinePointRow, LineRow, LineShapeRow, LineTargetRow, PeriodRow, SeriesRow,
};
use vizdaw_ffi::{
    AutomationTargetRow, ConditionRow, ConditionTest, CurveRow, EditCommand, LaneRef, ModifyRow,
    RuleRow, SelectModeRow, SelectionRow, Session, ValueTargetRow, VizError, WaveRow,
};

fn line(from: i64, to: i64, start: i64, end: i64, shape: LineShapeRow) -> LineRow {
    LineRow {
        from,
        to,
        start,
        end,
        step: Q,
        shape,
    }
}

fn strip_edit(target: LineTargetRow, op: LineOpRow, l: LineRow) -> EditCommand {
    EditCommand::SelectAndModify {
        rule: Some(RuleRow {
            conditions: vec![ConditionRow {
                test: ConditionTest::TimeRange {
                    start: l.from,
                    end: l.to + 1,
                },
                negate: false,
            }],
            match_all: true,
        }),
        mode: SelectModeRow::Select,
        modify: Some(ModifyRow::Line {
            target,
            op,
            line: l,
        }),
        to_copy: false,
    }
}

#[test]
fn a_drawn_controller_line_is_one_undo_step() {
    let mut b = Builder::new();
    let (seq, lane) = b.sequence("S", "L", None).unwrap();
    b.events(
        seq,
        lane,
        vec![EventSpec::new(
            Tick(Q),
            EventBody::Cc {
                index: 7,
                value: 5,
            },
        )],
    )
    .unwrap();
    let s = Session::from_document(b.finish()).unwrap();
    s.set_selection(SelectionRow {
        time_range: None,
        lanes: vec![LaneRef {
            seq: seq.0,
            lane: lane.0,
        }],
        events: vec![],
    })
    .unwrap();
    let draw = strip_edit(
        LineTargetRow::Series {
            series: SeriesRow::Controller { index: 7 },
        },
        LineOpRow::Draw,
        line(0, 2 * Q, 0, i64::from(u32::MAX), LineShapeRow::Straight),
    );
    let snap = s.edit(draw, None).unwrap();
    assert_eq!(snap.undo_label.as_deref(), Some("Draw Controllers"));
    let rows = s.lane_events(seq.0, lane.0).unwrap().events;
    assert_eq!(
        rows.iter().map(|r| (r.tick, r.value2)).collect::<Vec<_>>(),
        vec![(0, 7), (Q, 7), (2 * Q, 7)],
        "the old CC at Q was replaced"
    );
    assert_eq!(rows[2].value, i64::from(u32::MAX));
    s.undo().unwrap();
    assert_eq!(s.lane_events(seq.0, lane.0).unwrap().events.len(), 1);
}

#[test]
fn line_rows_round_trip_and_bad_lines_are_refused() {
    let shapes = vec![
        LineShapeRow::Straight,
        LineShapeRow::Curved {
            curve: CurveRow::SCurve,
        },
        LineShapeRow::Flat,
        LineShapeRow::Free {
            points: vec![
                LinePointRow { tick: 0, value: 1 },
                LinePointRow { tick: 9, value: 2 },
            ],
        },
        LineShapeRow::Random { seed: 42 },
        LineShapeRow::Oscillator {
            wave: WaveRow::Triangle,
            period: PeriodRow::Bars { bars: 2 },
            phase: 500,
        },
        LineShapeRow::Oscillator {
            wave: WaveRow::Saw,
            period: PeriodRow::Millis { ms: 250 },
            phase: 0,
        },
        LineShapeRow::Oscillator {
            wave: WaveRow::Sine,
            period: PeriodRow::Ticks { ticks: Q },
            phase: 0,
        },
    ];
    let targets = [
        LineTargetRow::Value {
            target: ValueTargetRow::Duration,
        },
        LineTargetRow::Series {
            series: SeriesRow::PolyPressure { pitch: 64 },
        },
        LineTargetRow::Series {
            series: SeriesRow::Automation {
                target: AutomationTargetRow::PluginParam { slot: 3, param: 9 },
            },
        },
    ];
    for shape in shapes {
        for target in targets {
            let row = ModifyRow::Line {
                target,
                op: LineOpRow::LimitMin,
                line: line(0, Q, 3, 4, shape.clone()),
            };
            let core = vizdaw_ffi::modify::modify(&row).unwrap();
            assert_eq!(vizdaw_ffi::modify::modify_row(&core), row);
        }
    }
    let s = Session::new_demo().unwrap();
    let backwards = strip_edit(
        LineTargetRow::Value {
            target: ValueTargetRow::Velocity,
        },
        LineOpRow::Set,
        line(Q, 0, 0, 0, LineShapeRow::Straight),
    );
    // The rule's empty range is refused first; a line whose `to` is before
    // `from` would be refused by the core.
    assert!(matches!(s.edit(backwards, None), Err(VizError::Model { .. })));
    let legato = EditCommand::SelectAndModify {
        rule: None,
        mode: SelectModeRow::Select,
        modify: Some(ModifyRow::Line {
            target: LineTargetRow::Value {
                target: ValueTargetRow::Velocity,
            },
            op: LineOpRow::Legato,
            line: line(0, Q, 0, 0, LineShapeRow::Flat),
        }),
        to_copy: false,
    };
    assert!(matches!(s.edit(legato, None), Err(VizError::Model { .. })));
}
```

- [ ] **Step 7: Add the line rows**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-ffi/src/strip.rs <<'VIZDAW_EOF'
//! Strip Chart lines as FFI records (Plan 4d Rulings 15–20, 25): the parts
//! of `ModifyRow::Line`. Values are on the target's scale (velocities
//! 16-bit, lengths in ticks, everything else on the 32-bit scale).

use crate::events::{AutomationTargetRow, automation_target_of, automation_target_row};
use crate::modify::{
    CurveRow, ValueTargetRow, WaveRow, curve_of, curve_row, target_of, target_row, wave_of,
    wave_row,
};
use core_model::strip::{Line, LineChange, LineOp, LineShape, LineTarget, Period, Series};
use core_model::time::Tick;

/// A controller-like series (core `Series`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum SeriesRow {
    /// Control change `index`.
    Controller {
        /// Controller number 0–127.
        index: u8,
    },
    /// Pitch bend.
    PitchBend,
    /// Channel pressure.
    ChannelPressure,
    /// Polyphonic pressure on one pitch.
    PolyPressure {
        /// Pitch 0–127.
        pitch: u8,
    },
    /// Program changes.
    Program,
    /// Automation of one target.
    Automation {
        /// The target.
        target: AutomationTargetRow,
    },
}

/// What a line acts on (core `LineTarget`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum LineTargetRow {
    /// A value of the events in range.
    Value {
        /// Which value.
        target: ValueTargetRow,
    },
    /// One series of the events in range (drawing writes it).
    Series {
        /// The series.
        series: SeriesRow,
    },
}

/// What a line does (core `LineOp`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum LineOpRow {
    /// Write the line.
    Draw,
    /// Set each value.
    Set,
    /// Scale each value (percent).
    Scale,
    /// Add to each value.
    Add,
    /// Keep each value at most the line's.
    LimitMax,
    /// Keep each value at least the line's.
    LimitMin,
    /// Map the range's lowest and highest onto `start` and `end`.
    Compress,
    /// Thin out to `step`.
    Thin,
    /// Thicken every `step`.
    Thicken,
    /// Lengths to the next note.
    Legato,
}

/// An oscillator's period.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum PeriodRow {
    /// Musical: ticks.
    Ticks {
        /// Ticks (> 0).
        ticks: i64,
    },
    /// Clock: milliseconds.
    Millis {
        /// Milliseconds (> 0).
        ms: u32,
    },
    /// Bars that follow the meter.
    Bars {
        /// Bars (> 0).
        bars: u32,
    },
}

/// One point of a free line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Record)]
pub struct LinePointRow {
    /// Tick.
    pub tick: i64,
    /// Value.
    pub value: i64,
}

/// A line's shape (core `LineShape`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum LineShapeRow {
    /// Straight.
    Straight,
    /// Along a curve.
    Curved {
        /// The curve.
        curve: CurveRow,
    },
    /// The start value all along.
    Flat,
    /// Through drawn points (ascending ticks).
    Free {
        /// The points.
        points: Vec<LinePointRow>,
    },
    /// Random per tick between start and end.
    Random {
        /// Seed.
        seed: u64,
    },
    /// A wave from start (low) to end (high).
    Oscillator {
        /// Wave.
        wave: WaveRow,
        /// Period.
        period: PeriodRow,
        /// Phase in thousandths of a period (0–999).
        phase: u16,
    },
}

/// A Strip Chart line (core `Line`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Record)]
pub struct LineRow {
    /// First tick.
    pub from: i64,
    /// Last tick.
    pub to: i64,
    /// Value at `from`.
    pub start: i64,
    /// Value at `to`.
    pub end: i64,
    /// Sample spacing (and Thin/Thicken spacing), ticks > 0.
    pub step: i64,
    /// Shape.
    pub shape: LineShapeRow,
}

fn series_of(s: SeriesRow) -> Series {
    match s {
        SeriesRow::Controller { index } => Series::Controller(index),
        SeriesRow::PitchBend => Series::PitchBend,
        SeriesRow::ChannelPressure => Series::ChannelPressure,
        SeriesRow::PolyPressure { pitch } => Series::PolyPressure(pitch),
        SeriesRow::Program => Series::Program,
        SeriesRow::Automation { target } => Series::Automation(automation_target_of(target)),
    }
}

fn series_row(s: Series) -> SeriesRow {
    match s {
        Series::Controller(index) => SeriesRow::Controller { index },
        Series::PitchBend => SeriesRow::PitchBend,
        Series::ChannelPressure => SeriesRow::ChannelPressure,
        Series::PolyPressure(pitch) => SeriesRow::PolyPressure { pitch },
        Series::Program => SeriesRow::Program,
        Series::Automation(t) => SeriesRow::Automation {
            target: automation_target_row(t),
        },
    }
}

fn line_target_of(t: LineTargetRow) -> LineTarget {
    match t {
        LineTargetRow::Value { target } => LineTarget::Value(target_of(target)),
        LineTargetRow::Series { series } => LineTarget::Series(series_of(series)),
    }
}

fn line_target_row(t: LineTarget) -> LineTargetRow {
    match t {
        LineTarget::Value(v) => LineTargetRow::Value {
            target: target_row(v),
        },
        LineTarget::Series(s) => LineTargetRow::Series {
            series: series_row(s),
        },
    }
}

fn op_of(o: LineOpRow) -> LineOp {
    match o {
        LineOpRow::Draw => LineOp::Draw,
        LineOpRow::Set => LineOp::Set,
        LineOpRow::Scale => LineOp::Scale,
        LineOpRow::Add => LineOp::Add,
        LineOpRow::LimitMax => LineOp::LimitMax,
        LineOpRow::LimitMin => LineOp::LimitMin,
        LineOpRow::Compress => LineOp::Compress,
        LineOpRow::Thin => LineOp::Thin,
        LineOpRow::Thicken => LineOp::Thicken,
        LineOpRow::Legato => LineOp::Legato,
    }
}

fn op_row(o: LineOp) -> LineOpRow {
    match o {
        LineOp::Draw => LineOpRow::Draw,
        LineOp::Set => LineOpRow::Set,
        LineOp::Scale => LineOpRow::Scale,
        LineOp::Add => LineOpRow::Add,
        LineOp::LimitMax => LineOpRow::LimitMax,
        LineOp::LimitMin => LineOpRow::LimitMin,
        LineOp::Compress => LineOpRow::Compress,
        LineOp::Thin => LineOpRow::Thin,
        LineOp::Thicken => LineOpRow::Thicken,
        LineOp::Legato => LineOpRow::Legato,
    }
}

fn shape_of(s: &LineShapeRow) -> LineShape {
    match s {
        LineShapeRow::Straight => LineShape::Straight,
        LineShapeRow::Curved { curve } => LineShape::Curved {
            curve: curve_of(*curve),
        },
        LineShapeRow::Flat => LineShape::Flat,
        LineShapeRow::Free { points } => LineShape::Free {
            points: points.iter().map(|p| (Tick(p.tick), p.value)).collect(),
        },
        LineShapeRow::Random { seed } => LineShape::Random { seed: *seed },
        LineShapeRow::Oscillator {
            wave,
            period,
            phase,
        } => LineShape::Oscillator {
            wave: wave_of(*wave),
            period: match period {
                PeriodRow::Ticks { ticks } => Period::Ticks(Tick(*ticks)),
                PeriodRow::Millis { ms } => Period::Millis(*ms),
                PeriodRow::Bars { bars } => Period::Bars(*bars),
            },
            phase: *phase,
        },
    }
}

fn shape_row(s: &LineShape) -> LineShapeRow {
    match s {
        LineShape::Straight => LineShapeRow::Straight,
        LineShape::Curved { curve } => LineShapeRow::Curved {
            curve: curve_row(*curve),
        },
        LineShape::Flat => LineShapeRow::Flat,
        LineShape::Free { points } => LineShapeRow::Free {
            points: points
                .iter()
                .map(|(t, v)| LinePointRow {
                    tick: t.0,
                    value: *v,
                })
                .collect(),
        },
        LineShape::Random { seed } => LineShapeRow::Random { seed: *seed },
        LineShape::Oscillator {
            wave,
            period,
            phase,
        } => LineShapeRow::Oscillator {
            wave: wave_row(*wave),
            period: match period {
                Period::Ticks(t) => PeriodRow::Ticks { ticks: t.0 },
                Period::Millis(ms) => PeriodRow::Millis { ms: *ms },
                Period::Bars(b) => PeriodRow::Bars { bars: *b },
            },
            phase: *phase,
        },
    }
}

/// The core change of a `ModifyRow::Line` (checked when it runs).
pub fn line_change(target: LineTargetRow, op: LineOpRow, line: &LineRow) -> LineChange {
    LineChange {
        target: line_target_of(target),
        op: op_of(op),
        line: Line {
            from: Tick(line.from),
            to: Tick(line.to),
            start: line.start,
            end: line.end,
            step: Tick(line.step),
            shape: shape_of(&line.shape),
        },
    }
}

/// The rows of a core change.
pub fn line_change_row(c: &LineChange) -> (LineTargetRow, LineOpRow, LineRow) {
    (
        line_target_row(c.target),
        op_row(c.op),
        LineRow {
            from: c.line.from.0,
            to: c.line.to.0,
            start: c.line.start,
            end: c.line.end,
            step: c.line.step.0,
            shape: shape_row(&c.line.shape),
        },
    )
}
VIZDAW_EOF
```

In `crates/core-ffi/src/modify.rs`, make the six conversion helpers visible to `strip.rs`: Find `fn target_of(t: ValueTargetRow) -> ValueTarget {` and replace it with `pub(crate) fn target_of(t: ValueTargetRow) -> ValueTarget {`; likewise prefix `pub(crate) ` to `fn target_row(t: ValueTarget) -> ValueTargetRow {`, `fn curve_of(c: CurveRow) -> Curve {`, `fn curve_row(c: Curve) -> CurveRow {`, `fn wave_of(w: WaveRow) -> Wave {` and `fn wave_row(w: Wave) -> WaveRow {`.

Find:

```rust
    /// Thicken controllers.
    Thicken {
        /// Distance between added events.
        spacing: i64,
    },
    /// Delete.
    Delete,
}
```

Replace:

```rust
    /// Thicken controllers.
    Thicken {
        /// Distance between added events.
        spacing: i64,
    },
    /// Delete.
    Delete,
    /// A Strip Chart line (4d, Plan 4d Ruling 15).
    Line {
        /// What it acts on.
        target: crate::strip::LineTargetRow,
        /// What it does.
        op: crate::strip::LineOpRow,
        /// The line.
        line: crate::strip::LineRow,
    },
}
```

Find:

```rust
        ModifyRow::Delete => Modify::Delete,
    })
}
```

Replace:

```rust
        ModifyRow::Delete => Modify::Delete,
        ModifyRow::Line { target, op, line } => {
            Modify::Line(crate::strip::line_change(*target, *op, line))
        }
    })
}
```

Find:

```rust
        Modify::Delete => ModifyRow::Delete,
    }
}
```

Replace:

```rust
        Modify::Delete => ModifyRow::Delete,
        Modify::Line(c) => {
            let (target, op, line) = crate::strip::line_change_row(c);
            ModifyRow::Line { target, op, line }
        }
    }
}
```

In `crates/core-ffi/src/lib.rs`, Find:

```rust
pub mod snapshot;
pub mod templates;
```

Replace:

```rust
pub mod snapshot;
pub mod strip;
pub mod templates;
```

Find:

```rust
pub use templates::SequenceTemplate;
```

Replace:

```rust
pub use strip::{
    LineOpRow, LinePointRow, LineRow, LineShapeRow, LineTargetRow, PeriodRow, SeriesRow,
};
pub use templates::SequenceTemplate;
```

- [ ] **Step 8: Teach the panel's pure model the new modification (Swift)**

In `app/Sources/VizDAWAppModel/SelectModify.swift`, Find:

```swift
    case transpose, diatonic, toScale, invert, noteMap, quantize, change, move, scaleTime, trimStart, setDest
    case substitute, thin, thicken, delete
```

Replace:

```swift
    case transpose, diatonic, toScale, invert, noteMap, quantize, change, move, scaleTime, trimStart, setDest
    case substitute, thin, thicken, delete, line
```

Find:

```swift
        case .delete: "Delete"
        }
    }

    public init(_ r: ModifyRow) {
```

Replace:

```swift
        case .delete: "Delete"
        case .line: "Strip Chart Line"
        }
    }

    public init(_ r: ModifyRow) {
```

Find:

```swift
        case .delete: self = .delete
        }
    }
```

Replace:

```swift
        case .delete: self = .delete
        case .line: self = .line
        }
    }
```

Find:

```swift
        case .delete: return .delete
        }
    }
```

Replace:

```swift
        case .delete: return .delete
        case .line:
            return .line(
                target: .value(target: .velocity), op: .set,
                line: LineRow(from: 0, to: 4 * q, start: 0, end: 0xFFFF, step: q / 8, shape: .straight))
        }
    }
```

Find:

```swift
        case .delete: "Delete Events"
        }
    }
}
```

Replace:

```swift
        case .delete: "Delete Events"
        case .line(let t, let op, _): lineTitle(t, op)
        }
    }

    /// A line's undo label, as the core names it (Plan 4d Ruling 15).
    public static func lineTitle(_ t: LineTargetRow, _ op: LineOpRow) -> String {
        switch (op, t) {
        case (.draw, .series(.automation)): "Draw Automation"
        case (.draw, .series(.program)): "Draw Program Changes"
        case (.draw, .series): "Draw Controllers"
        case (.thin, _): "Thin Out"
        case (.thicken, _): "Thicken"
        case (.compress, _): "Compress"
        case (.legato, _): "Legato"
        case (_, .value(.velocity)): "Change Velocity"
        case (_, .value(.releaseVelocity)): "Change Release Velocity"
        case (_, .value(.duration)): "Change Duration"
        default: "Change Values"
        }
    }
}

extension ModifyRow {
    /// A Strip Chart line (never applied to a copy).
    public var isLine: Bool {
        if case .line = self { return true }
        return false
    }
}
```

Find:

```swift
        return .selectAndModify(rule: r, mode: select.mode, modify: m, toCopy: m != nil && modify.toCopy)
```

Replace:

```swift
        return .selectAndModify(
            rule: r, mode: select.mode, modify: m, toCopy: m != nil && modify.toCopy && m?.isLine != true)
```

In `app/Sources/VizDAWAppModel/PanelFields.swift`, Find:

```swift
    public var fields: [Field] {
        switch self {
        case .transposeInterval: [Field("semitones", .bounded(-127, 127))]
```

Replace:

```swift
    /// What a line's values are typed in (Plan 4d Ruling 18).
    static func lineUnit(_ t: LineTargetRow, _ op: LineOpRow) -> FieldKind {
        let add = op == .add
        if op == .scale { return .percent }
        switch t {
        case .value(.velocity), .value(.releaseVelocity): return .velocity(add ? .delta : .exact)
        case .value(.duration): return .length
        default: return .value(add ? .delta : .exact)
        }
    }

    static func lineFields(_ t: LineTargetRow, _ op: LineOpRow, _ l: LineRow) -> [Field] {
        let u = lineUnit(t, op)
        var f = [Field("from", .position), Field("to", .position), Field("start", u), Field("end", u), Field("step", .length)]
        if case .oscillator(_, let p, _) = l.shape {
            switch p {
            case .ticks: f.append(Field("period", .length))
            case .millis: f.append(Field("period (ms)", .bounded(1, 600_000)))
            case .bars: f.append(Field("period (bars)", .bounded(1, 1_000)))
            }
            f.append(Field("phase (‰)", .bounded(0, 999)))
        }
        return f
    }

    static func lineNumbers(_ l: LineRow) -> [Int64] {
        var n = [l.from, l.to, l.start, l.end, l.step]
        if case .oscillator(_, let p, let ph) = l.shape {
            switch p {
            case .ticks(let t): n.append(t)
            case .millis(let ms): n.append(Int64(ms))
            case .bars(let b): n.append(Int64(b))
            }
            n.append(Int64(ph))
        }
        return n
    }

    static func lineWith(_ l: LineRow, _ at: (Int) -> Int64) -> LineRow {
        var o = l
        o.from = at(0)
        o.to = max(at(1), at(0))
        o.start = at(2)
        o.end = at(3)
        o.step = max(at(4), 1)
        if case .oscillator(let w, let p, _) = l.shape {
            let v = max(at(5), 1)
            let period: PeriodRow
            switch p {
            case .ticks: period = .ticks(ticks: v)
            case .millis: period = .millis(ms: UInt32(clamping: v))
            case .bars: period = .bars(bars: UInt32(clamping: v))
            }
            o.shape = .oscillator(wave: w, period: period, phase: UInt16(clamping: min(max(at(6), 0), 999)))
        }
        return o
    }

    public var fields: [Field] {
        switch self {
        case .transposeInterval: [Field("semitones", .bounded(-127, 127))]
```

Find:

```swift
        case .thin, .thicken: [Field("spacing", .length)]
        case .transposeToScale, .noteMap, .setDest, .substitute, .delete: []
        }
    }
```

Replace:

```swift
        case .thin, .thicken: [Field("spacing", .length)]
        case let .line(t, op, l): Self.lineFields(t, op, l)
        case .transposeToScale, .noteMap, .setDest, .substitute, .delete: []
        }
    }
```

Find:

```swift
        case .thin(let s), .thicken(let s): return [s]
        case .transposeToScale, .noteMap, .setDest, .substitute, .delete: return []
        }
    }
```

Replace:

```swift
        case .thin(let s), .thicken(let s): return [s]
        case .line(_, _, let l): return Self.lineNumbers(l)
        case .transposeToScale, .noteMap, .setDest, .substitute, .delete: return []
        }
    }
```

Find:

```swift
        case .thicken: return .thicken(spacing: max(at(0), 1))
        case .transposeToScale, .noteMap, .setDest, .substitute, .delete: return self
        }
    }
```

Replace:

```swift
        case .thicken: return .thicken(spacing: max(at(0), 1))
        case let .line(t, op, l): return .line(target: t, op: op, line: Self.lineWith(l, at))
        case .transposeToScale, .noteMap, .setDest, .substitute, .delete: return self
        }
    }
```

Append to `app/Tests/VizDAWAppModelTests/SelectModifyTests.swift`:

```swift
@Suite struct LineRowFieldTests {
    private let q: Int64 = 15_360

    @Test func lineFieldsRoundTripAndNameTheStep() {
        let osc = ModifyRow.line(
            target: .series(series: .controller(index: 7)), op: .add,
            line: LineRow(
                from: 0, to: 4 * q, start: -5, end: 5, step: q / 8,
                shape: .oscillator(wave: .sine, period: .millis(ms: 250), phase: 125)))
        #expect(osc.fields.map(\.label) == ["from", "to", "start", "end", "step", "period (ms)", "phase (‰)"])
        #expect(osc.fields[2].kind == .value(.delta))
        #expect(osc.numbers == [0, 4 * q, -5, 5, q / 8, 250, 125])
        #expect(osc.with(osc.numbers) == osc)
        // A typed `to` before `from` is held at `from`.
        guard case .line(_, _, let l) = osc.with([4 * q, 0]) else { return }
        #expect(l.from == 4 * q && l.to == 4 * q)
        #expect(ModifyKind.title(osc) == "Change Values")
        #expect(ModifyKind.lineTitle(.series(series: .program), .draw) == "Draw Program Changes")
        #expect(ModifyKind.lineTitle(.value(target: .releaseVelocity), .scale) == "Change Release Velocity")
        var c = SelectModifyConfig()
        c.modify = ModifyHalf(enabled: true, op: osc, toCopy: true)
        #expect(c.command() == .selectAndModify(rule: nil, mode: .select, modify: osc, toCopy: false))
    }
}
```

- [ ] **Step 9: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-ffi --test strip --test modify 2>&1 | grep -E "^test result|FAILED|panicked"
scripts/build-ffi.sh && scripts/swift.sh test --filter "SelectModifyTests|LineRowFieldTests" 2>&1 | tail -5
```

Expected: PASS.

- [ ] **Step 10: Gate and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace 2>&1 | grep -E "^test result|FAILED|panicked" | tail -60
scripts/build-ffi.sh && scripts/swift.sh build 2>&1 | tail -3 && scripts/swift.sh test 2>&1 | tail -3
git add crates/core-model/src/strip.rs crates/core-model/src/modify.rs crates/core-model/src/command/select.rs crates/core-model/tests/line_change.rs crates/core-ffi/src/strip.rs crates/core-ffi/src/modify.rs crates/core-ffi/src/lib.rs crates/core-ffi/tests/strip.rs app/Sources/VizDAWAppModel/SelectModify.swift app/Sources/VizDAWAppModel/PanelFields.swift app/Tests/VizDAWAppModelTests/SelectModifyTests.swift
git commit -m "feat(core): Strip Chart line changes on events, through the FFI and the panel model

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
```

Expected: 696 Rust tests listed (687 + 7 + 2); Swift 487.

---

### Task 5: Tempo lines — draw ramps, change, compress, thin and thicken a sequence's tempo

**Files:**
- Modify: `crates/core-model/src/strip.rs`, `crates/core-model/src/tempo.rs`
- Test: `crates/core-model/tests/tempo_line.rs`
- Modify: `crates/core-ffi/src/strip.rs`
- Test: `crates/core-ffi/tests/strip.rs`
- Modify: `app/Sources/VizDAWAppModel/SelectModify.swift`, `app/Sources/VizDAWAppModel/PanelFields.swift`
- Test: `app/Tests/VizDAWAppModelTests/SelectModifyTests.swift`

**Interfaces:**
- Consumes: Task 4's `LineChange`, `apply_line`, `reduce`, `narrow`; `TempoMap::{new, points}`, `TempoPoint`, `Bpm::{MIN, MAX}`.
- Produces: `LineTarget::Tempo` (appended); `TempoMap::bpm_at(&self, Tick) -> f64`; names "Draw Tempo" and "Change Tempo"; FFI `LineTargetRow::Tempo` (appended); Swift `FieldKind.bpm` (milli-BPM typed and shown as BPM, "120.00"), `ModifyKind.lineTitle` for tempo.

Ruling 20. Tempo values are milli-BPM (`Bpm.0`), clamped to 5 000–999 000. A tempo line acts on the tempo map of every sequence among the selection's lanes (each once); with no lane selected it is `Invalid`; a line starting before tick 0 is `OutOfRange`.

- [ ] **Step 1: Write the failing tests**

Create `crates/core-model/tests/tempo_line.rs`:

```rust
//! Tempo lines (Plan 4d Ruling 20): drawn tempo is ramps, so it "sounds
//! linear".

use core_model::command::{Command, SequenceCmd, apply};
use core_model::document::Document;
use core_model::history::History;
use core_model::ids::{LaneId, SeqId};
use core_model::modify::{Curve, Modify};
use core_model::rule::SelectMode;
use core_model::selection::{Selection, ViewFilter};
use core_model::strip::{Line, LineChange, LineOp, LineShape, LineTarget};
use core_model::tempo::{Bpm, TempoMap, TempoPoint};
use core_model::time::{PPQ, Tick};

const Q: i64 = PPQ;
const S: SeqId = SeqId(0);

fn seq_doc(tempo: TempoMap) -> Document {
    let d = apply(
        &Document::new(),
        &Command::Sequence(SequenceCmd::New { name: "S".into() }),
    )
    .unwrap()
    .doc;
    apply(&d, &Command::Sequence(SequenceCmd::SetTempo { seq: S, tempo }))
        .unwrap()
        .doc
}

fn tempo_line(from: i64, to: i64, start: i64, end: i64, shape: LineShape) -> Line {
    Line {
        from: Tick(from),
        to: Tick(to),
        start,
        end,
        step: Tick(Q),
        shape,
    }
}

fn run(d: &Document, lanes: &[LaneId], op: LineOp, line: Line) -> core_model::Result<History> {
    let mut h = History::new(d.clone());
    h.set_selection(Selection {
        time_range: None,
        lanes: lanes.iter().map(|l| (S, *l)).collect(),
        events: Default::default(),
    });
    h.execute(
        &Command::SelectAndModify {
            rule: None,
            mode: SelectMode::Select,
            modify: Some(Modify::Line(LineChange {
                target: LineTarget::Tempo,
                op,
                line,
            })),
            to_copy: false,
        },
        &ViewFilter::default(),
    )?;
    Ok(h)
}

fn points(h: &History) -> Vec<(i64, u32, bool)> {
    h.doc()
        .sequence(S)
        .unwrap()
        .tempo
        .points()
        .iter()
        .map(|p| (p.tick.0, p.bpm.0, p.ramp))
        .collect()
}

/// Seconds of a ramp linear in BPM per tick from `b0` to `b1` over `len`
/// ticks, at tick `x` of it.
fn ramp_seconds(x: f64, len: f64, b0: f64, b1: f64) -> f64 {
    let k = (b1 - b0) / len;
    60.0 / (PPQ as f64 * k) * (1.0 + k * x / b0).ln()
}

#[test]
fn a_straight_tempo_line_is_one_linear_ramp() {
    let d = seq_doc(TempoMap::default());
    let h = run(&d, &[LaneId(0)], LineOp::Draw, tempo_line(0, 16 * Q, 120_000, 60_000, LineShape::Straight)).unwrap();
    assert_eq!(points(&h), vec![(0, 120_000, true), (16 * Q, 60_000, false)]);
    assert_eq!(h.undo_label(), Some("Draw Tempo"));
    let map = &h.doc().sequence(S).unwrap().tempo;
    let want = ramp_seconds((8 * Q) as f64, (16 * Q) as f64, 120.0, 60.0);
    assert!((map.tick_to_seconds(Tick(8 * Q)) - want).abs() < 1e-9, "sounds linear");
    assert!((map.bpm_at(Tick(8 * Q)) - 90.0).abs() < 1e-9);
    assert_eq!(map.bpm_at(Tick(20 * Q)), 60.0, "holds after the line");
}

#[test]
fn drawn_curves_are_ramps_between_points_and_keep_what_is_outside() {
    let d = seq_doc(TempoMap::default());
    let h = run(
        &d,
        &[LaneId(0)],
        LineOp::Draw,
        tempo_line(2 * Q, 6 * Q, 100_000, 140_000, LineShape::Curved { curve: Curve::EaseIn }),
    )
    .unwrap();
    let p = points(&h);
    assert_eq!(p.first(), Some(&(0, 120_000, false)), "the point at 0 stays");
    assert_eq!(p.last(), Some(&(6 * Q, 140_000, false)));
    assert!(p.len() > 3, "a curve keeps its corners: {p:?}");
    assert!(p[1..p.len() - 1].iter().all(|x| x.2), "ramps between drawn points");
    // A value beyond 5–999 BPM is clamped.
    let h = run(&d, &[LaneId(0)], LineOp::Draw, tempo_line(0, 0, 5_000_000, 0, LineShape::Flat)).unwrap();
    assert_eq!(points(&h), vec![(0, Bpm::MAX.0, false)]);
}

#[test]
fn tempo_changes_compress_thin_and_thicken() {
    let ramp = TempoMap::new(vec![
        TempoPoint {
            tick: Tick(0),
            bpm: Bpm::whole(120),
            ramp: true,
        },
        TempoPoint {
            tick: Tick(16 * Q),
            bpm: Bpm::whole(60),
            ramp: false,
        },
    ])
    .unwrap();
    let d = seq_doc(ramp.clone());
    let scaled = run(&d, &[LaneId(0)], LineOp::Scale, tempo_line(0, 16 * Q, 50, 50, LineShape::Flat)).unwrap();
    assert_eq!(points(&scaled), vec![(0, 60_000, true), (16 * Q, 30_000, false)]);
    assert_eq!(scaled.undo_label(), Some("Change Tempo"));
    let compressed = run(&d, &[LaneId(0)], LineOp::Compress, tempo_line(0, 16 * Q, 100_000, 110_000, LineShape::Flat)).unwrap();
    assert_eq!(points(&compressed), vec![(0, 110_000, true), (16 * Q, 100_000, false)]);
    // Thickening a ramp adds points on it and keeps its timing.
    let thick = run(
        &d,
        &[LaneId(0)],
        LineOp::Thicken,
        Line {
            step: Tick(4 * Q),
            ..tempo_line(0, 16 * Q, 0, 0, LineShape::Flat)
        },
    )
    .unwrap();
    assert_eq!(
        points(&thick),
        vec![
            (0, 120_000, true),
            (4 * Q, 105_000, true),
            (8 * Q, 90_000, true),
            (12 * Q, 75_000, true),
            (16 * Q, 60_000, false)
        ]
    );
    let t = |h: &History| h.doc().sequence(S).unwrap().tempo.tick_to_seconds(Tick(16 * Q));
    assert!((t(&thick) - ramp.tick_to_seconds(Tick(16 * Q))).abs() < 1e-9);
    let thinned = run(
        thick.doc(),
        &[LaneId(0)],
        LineOp::Thin,
        Line {
            step: Tick(6 * Q),
            ..tempo_line(0, 16 * Q, 0, 0, LineShape::Flat)
        },
    )
    .unwrap();
    assert_eq!(
        points(&thinned).iter().map(|p| p.0).collect::<Vec<_>>(),
        vec![0, 8 * Q, 16 * Q]
    );
}

#[test]
fn tempo_lines_need_a_lane_and_a_start_at_or_after_zero() {
    let d = seq_doc(TempoMap::default());
    assert!(run(&d, &[], LineOp::Draw, tempo_line(0, Q, 1, 1, LineShape::Flat)).is_err());
    assert!(run(&d, &[LaneId(0)], LineOp::Draw, tempo_line(-Q, Q, 1, 1, LineShape::Flat)).is_err());
    assert!(run(&d, &[LaneId(0)], LineOp::Legato, tempo_line(0, Q, 1, 1, LineShape::Flat)).is_err());
}
```

Append to `crates/core-ffi/tests/strip.rs`:

```rust
#[test]
fn a_tempo_line_draws_ramps_in_the_selected_lanes_sequence() {
    let s = Session::new_demo().unwrap();
    let song = s
        .snapshot()
        .unwrap()
        .sequences
        .iter()
        .find(|r| r.name == "Song")
        .unwrap()
        .id;
    let lane = s.lanes(song).unwrap()[0].id;
    s.set_selection(SelectionRow {
        time_range: None,
        lanes: vec![LaneRef { seq: song, lane }],
        events: vec![],
    })
    .unwrap();
    let draw = EditCommand::SelectAndModify {
        rule: None,
        mode: SelectModeRow::Select,
        modify: Some(ModifyRow::Line {
            target: LineTargetRow::Tempo,
            op: LineOpRow::Draw,
            line: line(0, 16 * Q, 120_000, 60_000, LineShapeRow::Straight),
        }),
        to_copy: false,
    };
    let snap = s.edit(draw.clone(), None).unwrap();
    assert_eq!(snap.undo_label.as_deref(), Some("Draw Tempo"));
    let tempo = s.sequence_info(song).unwrap().tempo;
    let inside: Vec<(i64, f64, bool)> = tempo
        .iter()
        .filter(|p| p.tick <= 16 * Q)
        .map(|p| (p.tick, p.bpm, p.ramp))
        .collect();
    assert_eq!(inside, vec![(0, 120.0, true), (16 * Q, 60.0, false)]);
    let EditCommand::SelectAndModify {
        modify: Some(row), ..
    } = draw
    else {
        unreachable!()
    };
    let core = vizdaw_ffi::modify::modify(&row).unwrap();
    assert_eq!(vizdaw_ffi::modify::modify_row(&core), row);
}
```

Append to `app/Tests/VizDAWAppModelTests/SelectModifyTests.swift`:

```swift
@Suite struct TempoLineFieldTests {
    @Test func tempoLinesTypeBeatsPerMinute() {
        let row = ModifyRow.line(
            target: .tempo, op: .draw,
            line: LineRow(from: 0, to: 61_440, start: 120_000, end: 90_500, step: 1_920, shape: .straight))
        #expect(row.fields[2].kind == .bpm)
        #expect(FieldText.text(120_000, .bpm, meter: []) == "120.00")
        #expect(FieldText.parse("90.5", .bpm, meter: []) == 90_500)
        #expect(FieldText.parse("-12", .bpm, meter: []) == -12_000)
        #expect(FieldText.parse("1200", .bpm, meter: []) == nil)
        #expect(ModifyKind.title(row) == "Draw Tempo")
        #expect(ModifyKind.lineTitle(.tempo, .limitMax) == "Change Tempo")
    }
}
```

- [ ] **Step 2: Run them to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test tempo_line 2>&1 | tail -5
```

Expected: compile error (`no variant named Tempo`).

- [ ] **Step 3: Read the tempo at a tick**

In `crates/core-model/src/tempo.rs`, Find:

```rust
    /// Seconds from tick 0 to `t` (negative for negative ticks).
    pub fn tick_to_seconds(&self, t: Tick) -> f64 {
```

Replace:

```rust
    /// The tempo at `t` in BPM (a ramp interpolates linearly by tick).
    pub fn bpm_at(&self, t: Tick) -> f64 {
        let mut out = self.first_bpm();
        for (start, end, b0, b1) in self.segments() {
            if t.0 < start {
                break;
            }
            out = match end {
                Some(e) if t.0 < e && e > start => {
                    b0 + (b1 - b0) * (t.0 - start) as f64 / (e - start) as f64
                }
                Some(_) => b1,
                None => b0,
            };
        }
        out
    }

    /// Seconds from tick 0 to `t` (negative for negative ticks).
    pub fn tick_to_seconds(&self, t: Tick) -> f64 {
```

- [ ] **Step 4: Run lines on tempo**

In `crates/core-model/src/strip.rs`, Find:

```rust
    /// One series of the events in range; drawing writes it into every
    /// selected lane.
    Series(Series),
}
```

Replace:

```rust
    /// One series of the events in range; drawing writes it into every
    /// selected lane.
    Series(Series),
    /// The tempo of the selected lanes' sequences, in milli-BPM (Plan 4d
    /// Ruling 20).
    Tempo,
}
```

Find:

```rust
        match (self.op, self.target) {
            (LineOp::Draw, LineTarget::Series(Series::Automation(_))) => "Draw Automation",
```

Replace:

```rust
        match (self.op, self.target) {
            (LineOp::Draw, LineTarget::Tempo) => "Draw Tempo",
            (LineOp::Draw, LineTarget::Series(Series::Automation(_))) => "Draw Automation",
```

Find:

```rust
            (LineOp::Legato, _) => "Legato",
            (_, LineTarget::Value(ValueTarget::Velocity)) => "Change Velocity",
```

Replace:

```rust
            (LineOp::Legato, _) => "Legato",
            (_, LineTarget::Tempo) => "Change Tempo",
            (_, LineTarget::Value(ValueTarget::Velocity)) => "Change Velocity",
```

Find:

```rust
    match (c.target, c.op) {
        (LineTarget::Series(s), LineOp::Draw) => draw_series(d, lanes, s, &c.line, r),
        (target, op) => change_events(d, targets, target, op, &c.line, r),
    }
}
```

Replace:

```rust
    match (c.target, c.op) {
        (LineTarget::Tempo, op) => {
            tempo_line(d, lanes, op, &c.line)?;
            Ok(targets.to_vec())
        }
        (LineTarget::Series(s), LineOp::Draw) => draw_series(d, lanes, s, &c.line, r),
        (target, op) => change_events(d, targets, target, op, &c.line, r),
    }
}

/// The tempo `v` (milli-BPM) inside 5–999 BPM.
fn bpm(v: i64) -> Bpm {
    let v = v.clamp(i64::from(Bpm::MIN.0), i64::from(Bpm::MAX.0));
    Bpm(u32::try_from(v).unwrap_or(Bpm::MAX.0))
}

/// Runs a tempo line on the tempo map of every sequence among `lanes`
/// (Plan 4d Ruling 20).
fn tempo_line(
    d: &mut Document,
    lanes: &OrdSet<(SeqId, LaneId)>,
    op: LineOp,
    line: &Line,
) -> Result<()> {
    let seqs: BTreeSet<SeqId> = lanes.iter().map(|(s, _)| *s).collect();
    if seqs.is_empty() {
        return Err(ModelError::invalid(
            "select a lane of the sequence whose tempo to change",
        ));
    }
    if line.from.0 < 0 {
        return Err(ModelError::range("line.from", line.from.0));
    }
    for seq in seqs {
        let s = d.sequence(seq)?;
        let ctx = TimeCtx::of(s);
        let points: Vec<TempoPoint> = s.tempo.points().iter().copied().collect();
        let next = match op {
            LineOp::Draw => draw_tempo(&points, line, ctx)?,
            LineOp::Thin => thin_tempo(&points, line),
            LineOp::Thicken => thicken_tempo(&s.tempo, &points, line)?,
            LineOp::Compress => compress_tempo(&points, line),
            LineOp::Legato => {
                return Err(ModelError::invalid("legato changes note lengths only"));
            }
            LineOp::Set | LineOp::Scale | LineOp::Add | LineOp::LimitMax | LineOp::LimitMin => {
                change_tempo(&points, op, line, ctx)
            }
        };
        let map = TempoMap::new(next)?;
        d.seq_mut(seq)?.tempo = map;
    }
    Ok(())
}

/// Points outside the line stay; inside, the line's reduced samples become
/// ramping points (the last holds).
fn draw_tempo(points: &[TempoPoint], line: &Line, ctx: TimeCtx<'_>) -> Result<Vec<TempoPoint>> {
    let held: Vec<(Tick, i64)> = line
        .samples(ctx)?
        .into_iter()
        .map(|(t, v)| (t, i64::from(bpm(v).0)))
        .collect();
    let drawn = reduce(&held, true);
    let n = drawn.len();
    let mut out: Vec<TempoPoint> = points
        .iter()
        .copied()
        .filter(|p| !line.covers(p.tick))
        .collect();
    out.extend(drawn.into_iter().enumerate().map(|(i, (tick, v))| TempoPoint {
        tick,
        bpm: bpm(v),
        ramp: i + 1 < n,
    }));
    out.sort_by_key(|p| p.tick);
    Ok(out)
}

fn change_tempo(points: &[TempoPoint], op: LineOp, line: &Line, ctx: TimeCtx<'_>) -> Vec<TempoPoint> {
    points
        .iter()
        .map(|p| {
            if !line.covers(p.tick) {
                return *p;
            }
            let v = i64::from(p.bpm.0);
            let a = line.at(p.tick, ctx);
            let next = match op {
                LineOp::Scale => narrow(i128::from(v) * i128::from(a) / 100),
                LineOp::Add => v.saturating_add(a),
                LineOp::LimitMax => v.min(a),
                LineOp::LimitMin => v.max(a),
                _ => a,
            };
            TempoPoint {
                bpm: bpm(next),
                ..*p
            }
        })
        .collect()
}

fn compress_tempo(points: &[TempoPoint], line: &Line) -> Vec<TempoPoint> {
    let inside: Vec<i64> = points
        .iter()
        .filter(|p| line.covers(p.tick))
        .map(|p| i64::from(p.bpm.0))
        .collect();
    let (Some(lo), Some(hi)) = (inside.iter().min().copied(), inside.iter().max().copied()) else {
        return points.to_vec();
    };
    points
        .iter()
        .map(|p| {
            if !line.covers(p.tick) {
                return *p;
            }
            let v = i64::from(p.bpm.0);
            let next = if hi == lo {
                line.start
            } else {
                narrow(
                    i128::from(line.start)
                        + (i128::from(v) - i128::from(lo)) * (i128::from(line.end) - i128::from(line.start))
                            / (i128::from(hi) - i128::from(lo)),
                )
            };
            TempoPoint {
                bpm: bpm(next),
                ..*p
            }
        })
        .collect()
}

/// Inside the line, a point closer than `step` to the last kept one goes
/// (the point at tick 0 always stays).
fn thin_tempo(points: &[TempoPoint], line: &Line) -> Vec<TempoPoint> {
    let mut out: Vec<TempoPoint> = Vec::with_capacity(points.len());
    let mut last: Option<Tick> = None;
    for p in points {
        if line.covers(p.tick) {
            if p.tick != Tick::ZERO && last.is_some_and(|l| (p.tick - l).0 < line.step.0) {
                continue;
            }
            last = Some(p.tick);
        }
        out.push(*p);
    }
    out
}

/// Points every `step` between neighbouring points inside the line, at the
/// tempo the map has there (ramping where the earlier point ramps), so the
/// timing does not change.
fn thicken_tempo(map: &TempoMap, points: &[TempoPoint], line: &Line) -> Result<Vec<TempoPoint>> {
    let mut out = points.to_vec();
    let mut added = 0usize;
    for pair in points.windows(2) {
        let [a, b] = pair else { continue };
        if !(line.covers(a.tick) && line.covers(b.tick)) {
            continue;
        }
        let mut t = a.tick + line.step;
        while t < b.tick {
            added += 1;
            if added > MAX_SAMPLES {
                return Err(ModelError::range("line.step", line.step.0));
            }
            out.push(TempoPoint {
                tick: t,
                bpm: bpm((map.bpm_at(t) * 1000.0).round() as i64),
                ramp: a.ramp,
            });
            t = t + line.step;
        }
    }
    out.sort_by_key(|p| p.tick);
    Ok(out)
}
```

Find:

```rust
    let vt = match target {
        LineTarget::Value(vt) => vt,
        LineTarget::Series(_) => ValueTarget::Value,
    };
```

Replace:

```rust
    let vt = match target {
        LineTarget::Value(vt) => vt,
        LineTarget::Series(_) => ValueTarget::Value,
        // `apply_line` runs tempo lines itself.
        LineTarget::Tempo => return Ok(Vec::new()),
    };
```

Find:

```rust
        let fits = match target {
            LineTarget::Value(vt) => read_value(ev, vt).is_some(),
            LineTarget::Series(s) => Series::of(&ev.body) == Some(s),
        };
```

Replace:

```rust
        let fits = match target {
            LineTarget::Value(vt) => read_value(ev, vt).is_some(),
            LineTarget::Series(s) => Series::of(&ev.body) == Some(s),
            LineTarget::Tempo => false,
        };
```

Find:

```rust
use crate::sequence::Sequence;
use crate::tempo::TempoMap;
use crate::time::Tick;
```

Replace:

```rust
use crate::sequence::Sequence;
use crate::tempo::{Bpm, TempoMap, TempoPoint};
use crate::time::Tick;
```

- [ ] **Step 5: Add the FFI tempo target**

In `crates/core-ffi/src/strip.rs`, Find:

```rust
    /// One series of the events in range (drawing writes it).
    Series {
        /// The series.
        series: SeriesRow,
    },
}
```

Replace:

```rust
    /// One series of the events in range (drawing writes it).
    Series {
        /// The series.
        series: SeriesRow,
    },
    /// The selected lanes' sequences' tempo (milli-BPM).
    Tempo,
}
```

Find:

```rust
        LineTargetRow::Series { series } => LineTarget::Series(series_of(series)),
    }
}
```

Replace:

```rust
        LineTargetRow::Series { series } => LineTarget::Series(series_of(series)),
        LineTargetRow::Tempo => LineTarget::Tempo,
    }
}
```

Find:

```rust
        LineTarget::Series(s) => LineTargetRow::Series {
            series: series_row(s),
        },
    }
}
```

Replace:

```rust
        LineTarget::Series(s) => LineTargetRow::Series {
            series: series_row(s),
        },
        LineTarget::Tempo => LineTargetRow::Tempo,
    }
}
```

- [ ] **Step 6: Type tempo in beats per minute (Swift)**

In `app/Sources/VizDAWAppModel/PanelFields.swift`, Find:

```swift
    /// A tick as bar.beat.unit.
    case position
}
```

Replace:

```swift
    /// A tick as bar.beat.unit.
    case position
    /// Milli-BPM as beats per minute ("120.00"; a change may be negative).
    case bpm
}
```

Find:

```swift
        case .position: return Bbu.of(n, meter: meter).text
        }
    }
```

Replace:

```swift
        case .position: return Bbu.of(n, meter: meter).text
        case .bpm: return String(format: "%.2f", Double(n) / 1000)
        }
    }
```

Find:

```swift
        case .position:
            return PositionInput.parse(t, current: Bbu(bar: 1, beat: 1, unit: 0)).map { $0.tick(meter: meter) }
        }
    }
```

Replace:

```swift
        case .position:
            return PositionInput.parse(t, current: Bbu(bar: 1, beat: 1, unit: 0)).map { $0.tick(meter: meter) }
        case .bpm:
            let b = t.lowercased().replacingOccurrences(of: "bpm", with: "").trimmingCharacters(in: .whitespaces)
            guard let v = Double(b), v.isFinite, abs(v) <= 999 else { return nil }
            return Int64((v * 1000).rounded())
        }
    }
```

Find:

```swift
        if op == .scale { return .percent }
        switch t {
```

Replace:

```swift
        if op == .scale { return .percent }
        if t == .tempo { return .bpm }
        switch t {
```

In `app/Sources/VizDAWAppModel/SelectModify.swift`, Find:

```swift
        switch (op, t) {
        case (.draw, .series(.automation)): "Draw Automation"
```

Replace:

```swift
        switch (op, t) {
        case (.draw, .tempo): "Draw Tempo"
        case (.draw, .series(.automation)): "Draw Automation"
```

Find:

```swift
        case (.legato, _): "Legato"
        case (_, .value(.velocity)): "Change Velocity"
```

Replace:

```swift
        case (.legato, _): "Legato"
        case (_, .tempo): "Change Tempo"
        case (_, .value(.velocity)): "Change Velocity"
```

- [ ] **Step 7: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test tempo_line --test tempo --test line_change 2>&1 | grep -E "^test result|FAILED|panicked"
cargo test -p core-ffi --test strip 2>&1 | tail -3
scripts/build-ffi.sh && scripts/swift.sh test --filter "TempoLineFieldTests|SelectModifyTests" 2>&1 | tail -5
```

Expected: PASS. (`thicken` puts 105, 90 and 75 BPM on the ramp: `bpm_at` interpolates linearly by tick, the model's ramp.)

- [ ] **Step 8: Gate and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace 2>&1 | grep -E "^test result|FAILED|panicked" | tail -60
scripts/build-ffi.sh && scripts/swift.sh build 2>&1 | tail -3 && scripts/swift.sh test 2>&1 | tail -3
git add crates/core-model/src/strip.rs crates/core-model/src/tempo.rs crates/core-model/tests/tempo_line.rs crates/core-ffi/src/strip.rs crates/core-ffi/tests/strip.rs app/Sources/VizDAWAppModel/SelectModify.swift app/Sources/VizDAWAppModel/PanelFields.swift app/Tests/VizDAWAppModelTests/SelectModifyTests.swift
git commit -m "feat(core): tempo lines draw linear ramps and change, compress, thin and thicken tempo

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
```

Expected: 701 Rust tests listed (696 + 4 + 1); Swift 488.

---

### Task 6: Engine — automation lanes in the schedule, velocity scale at compile time

**Files:**
- Create: `crates/core-engine/src/schedule/automation.rs`
- Modify: `crates/core-engine/src/schedule/mod.rs`, `crates/core-engine/src/schedule/compile.rs`, `crates/core-engine/src/engine.rs`, `crates/core-engine/src/graph/node.rs`
- Test: `crates/core-engine/tests/schedule_automation.rs`

**Interfaces:**
- Consumes: Task 2's `EventBody::Automation`, `AutomationTarget`, `automation::{unit, velocity_scale_percent}`; `query::flatten`; `TargetTable::find`.
- Produces: `core_engine::schedule::{MAX_AUTO_LANES: usize = 1024, MIN_RUN: u32 = 32, StripParam::{Fader, Pan, Mute, Solo}, AutoDest::{Strip(StripId, StripParam), Send(StripId, u16), Plugin(PluginInstanceId, ParamId)}, AutoDest::of(AutomationTarget) -> Option<AutoDest>, AutoPoint { frame: u64, value: f64 }, AutoLane { dest, stepped: bool, instrument: Option<TargetIdx>, .. }, Automation}` with `Automation::build(Vec<(AutoDest, u64, f64)>) -> (Automation, usize)`, `is_empty`, `lanes() -> &[AutoLane]` (sorted by `dest`), `point_count`, `points(&AutoLane) -> &[AutoPoint]`, `value(&AutoLane, frame: u64, left: bool) -> f64` (RT), `next_cut(frame) -> Option<u64>` (RT; the first point frame after `frame`), `drives(AutoDest) -> bool` (RT), `mark_stepped(impl Fn(AutoDest) -> bool)`, `bind_instruments(impl Fn(PluginInstanceId) -> Option<TargetIdx>)`; `Schedule.automation: Automation`; `CompileReport.{automation_lanes, automation_points, automation_missing, automation_dropped_lanes}: usize` and warnings; `InsertFactory::param_stepped(&self, PluginInstanceId, ParamId) -> bool` (default `false`); `Engine::load` binds plugin-instrument targets and stepped parameters.

Rulings 7–9. `value(lane, frame, left)`: with `left`, the limit from the left (a point exactly at `frame` is not yet in effect on a stepped lane; of two points on one frame the earlier counts) — the value at the end of a run; without, the value from `frame` on — the value at a run's start.

- [ ] **Step 1: Write the failing tests**

Create `crates/core-engine/tests/schedule_automation.rs`:

```rust
//! Automation lanes in the schedule (Plan 4d Rulings 7–9).

use core_engine::fixtures::{Builder, Q};
use core_engine::midi1::note_on_velocity;
use core_engine::schedule::{
    AutoDest, Automation, CompileOptions, CompileReport, MAX_AUTO_LANES, Schedule, StripParam,
    TargetIdx, TargetKind, TargetTable, compile,
};
use core_model::automation::{AutomationTarget, SendIndex};
use core_model::document::Document;
use core_model::event::{EventBody, EventSpec};
use core_model::ids::{DestId, PluginInstanceId, SeqId, StripId};
use core_model::mixer::{ParamId, PluginRef, StripKind};
use core_model::time::Tick;

const SR: f64 = 48_000.0;
/// A quarter at 120 BPM is 24 000 frames.
const QF: u64 = 24_000;

fn compile_seq(doc: &Document, seq: SeqId) -> (Schedule, CompileReport) {
    let mut table = TargetTable::new();
    compile(doc, seq, &CompileOptions::new(SR), &mut |k: &TargetKind| {
        table.intern(k.clone()).map(Ok)
    })
    .unwrap()
}

fn auto(t: i64, target: AutomationTarget, value: f64) -> EventSpec {
    EventSpec::new(Tick(t), EventBody::Automation { target, value })
}

#[test]
fn lanes_hold_points_in_frames_and_ramp_or_hold() {
    let mut b = Builder::new();
    let d = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, notes) = b.sequence("S", "Notes", Some(d)).unwrap();
    b.notes(seq, notes, &[(0, 60, 0x8000, 4 * Q)]).unwrap();
    let s = b.strip_of(StripKind::BuiltinSynth).unwrap();
    let lane = b.lane(seq, "Auto", None).unwrap();
    b.events(
        seq,
        lane,
        vec![
            auto(0, AutomationTarget::StripFader(s), 0.0),
            auto(Q, AutomationTarget::StripFader(s), 1.0),
            auto(2 * Q, AutomationTarget::StripFader(s), 0.5),
            auto(Q, AutomationTarget::StripPan(s), 0.25),
            auto(0, AutomationTarget::StripMute(s), 0.0),
            auto(Q, AutomationTarget::StripMute(s), 1.0),
        ],
    )
    .unwrap();
    let (sch, report) = compile_seq(b.doc(), seq);
    let a = &sch.automation;
    assert_eq!(report.automation_lanes, 3);
    assert_eq!(report.automation_points, 6);
    let dests: Vec<AutoDest> = a.lanes().iter().map(|l| l.dest).collect();
    assert_eq!(
        dests,
        vec![
            AutoDest::Strip(s, StripParam::Fader),
            AutoDest::Strip(s, StripParam::Pan),
            AutoDest::Strip(s, StripParam::Mute),
        ]
    );
    let [fader, pan, mute] = [a.lanes()[0], a.lanes()[1], a.lanes()[2]];
    assert!(!fader.stepped && mute.stepped);
    assert_eq!(a.value(&fader, 0, false), 0.0);
    assert_eq!(a.value(&fader, QF / 2, false), 0.5);
    assert_eq!(a.value(&fader, QF, false), 1.0);
    assert_eq!(a.value(&fader, QF + QF / 2, false), 0.75);
    assert_eq!(a.value(&fader, 10 * QF, false), 0.5, "the last value holds");
    assert_eq!(a.value(&pan, 0, false), 0.25, "the first value holds before it");
    assert_eq!(a.value(&mute, QF - 1, false), 0.0);
    assert_eq!(a.value(&mute, QF, false), 1.0);
    assert_eq!(a.value(&mute, QF, true), 0.0, "the left limit at the point");
    assert_eq!(a.next_cut(0), Some(QF));
    assert_eq!(a.next_cut(QF), Some(2 * QF));
    assert_eq!(a.next_cut(2 * QF), None);
    assert!(a.drives(AutoDest::Strip(s, StripParam::Pan)));
    assert!(!a.drives(AutoDest::Strip(s, StripParam::Solo)));
    assert!(sch.entries.iter().all(|e| sch.bytes(&e.msg)[0] & 0xF0 != 0xB0), "no MIDI for automation");
}

#[test]
fn velocity_scale_scales_the_note_ons_of_its_destination() {
    let mut b = Builder::new();
    let a = b.dest("A", "builtin:synth", 0).unwrap();
    let other = b.dest("B", "builtin:synth", 1).unwrap();
    let (seq, la) = b.sequence("S", "A", Some(a)).unwrap();
    let lb = b.lane(seq, "B", Some(other)).unwrap();
    b.notes(seq, la, &[(0, 60, 0x8000, Q), (2 * Q, 62, 0x8000, Q), (5 * Q, 64, 0x8000, Q)])
        .unwrap();
    b.notes(seq, lb, &[(2 * Q, 70, 0x8000, Q)]).unwrap();
    let auto_lane = b.lane(seq, "Auto", None).unwrap();
    b.events(
        seq,
        auto_lane,
        vec![
            auto(0, AutomationTarget::VelocityScale(a), 0.5),
            auto(4 * Q, AutomationTarget::VelocityScale(a), 1.0),
        ],
    )
    .unwrap();
    let (sch, report) = compile_seq(b.doc(), seq);
    assert_eq!(report.automation_lanes, 0, "velocity scale is not a lane");
    let on = |pitch: u8| {
        sch.entries
            .iter()
            .map(|e| sch.bytes(&e.msg))
            .find(|m| m[0] & 0xF0 == 0x90 && m[1] == pitch)
            .unwrap()[2]
    };
    assert_eq!(on(60), 64, "0.5 is 100 %");
    assert_eq!(on(62), note_on_velocity(57_344), "0.75 is 175 %");
    assert_eq!(on(64), 127, "250 % clamps");
    assert_eq!(on(70), 64, "another destination is untouched");
}

#[test]
fn missing_targets_are_skipped_and_reported() {
    let mut b = Builder::new();
    let d = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, notes) = b.sequence("S", "Notes", Some(d)).unwrap();
    b.notes(seq, notes, &[(0, 60, 0x8000, Q)]).unwrap();
    let s = b.strip_of(StripKind::BuiltinSynth).unwrap();
    b.events(
        seq,
        notes,
        vec![
            auto(0, AutomationTarget::StripFader(StripId(999)), 0.5),
            auto(0, AutomationTarget::SendLevel(s, SendIndex(3)), 0.5),
            auto(0, AutomationTarget::PluginParam(PluginInstanceId(77), ParamId(0)), 0.5),
            auto(0, AutomationTarget::VelocityScale(DestId(55)), 0.5),
            auto(0, AutomationTarget::StripFader(s), 0.5),
        ],
    )
    .unwrap();
    let (sch, report) = compile_seq(b.doc(), seq);
    assert_eq!(report.automation_missing, 4);
    assert_eq!(report.automation_lanes, 1);
    assert_eq!(sch.automation.lanes()[0].dest, AutoDest::Strip(s, StripParam::Fader));
    assert!(
        report.warnings.iter().any(|w| w.contains("4 automation point(s) skipped")),
        "{:?}",
        report.warnings
    );
}

#[test]
fn at_most_max_lanes_play_and_plugin_lanes_bind() {
    let mut b = Builder::new();
    let d = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, notes) = b.sequence("S", "Notes", Some(d)).unwrap();
    let s = b.strip_of(StripKind::BuiltinSynth).unwrap();
    let slot = b.insert(s, PluginRef::from_legacy("clap:x")).unwrap();
    let events: Vec<EventSpec> = (0..MAX_AUTO_LANES as u64 + 6)
        .map(|p| auto(0, AutomationTarget::PluginParam(slot, ParamId(p)), 0.5))
        .collect();
    b.events(seq, notes, events).unwrap();
    let (mut sch, report) = compile_seq(b.doc(), seq);
    assert_eq!(report.automation_lanes, MAX_AUTO_LANES);
    assert_eq!(report.automation_dropped_lanes, 6);
    assert!(report.warnings.iter().any(|w| w.contains("6 automation lane(s) dropped")));
    sch.automation.mark_stepped(|d| d == AutoDest::Plugin(slot, ParamId(1)));
    sch.automation.bind_instruments(|id| (id == slot).then_some(TargetIdx(3)));
    let lanes = sch.automation.lanes();
    assert!(lanes[1].stepped && !lanes[0].stepped);
    assert!(lanes.iter().all(|l| l.instrument == Some(TargetIdx(3))));
    // An empty set of lanes plays nothing.
    let (empty, dropped) = Automation::build(Vec::new());
    assert!(empty.is_empty() && dropped == 0 && empty.next_cut(0).is_none());
}
```

- [ ] **Step 2: Run them to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-engine --test schedule_automation 2>&1 | tail -5
```

Expected: compile errors (`unresolved imports AutoDest, Automation …`).

- [ ] **Step 3: Write the lanes**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-engine/src/schedule/automation.rs <<'VIZDAW_EOF'
//! Automation lanes of a compiled schedule (editors spec §4d; Plan 4d
//! Rulings 7, 8, 10): per target, its points `(frame, normalised value)`.
//! Built off the audio thread; read on it by binary search. // RT (reads)

use super::TargetIdx;
use core_model::automation::{AutomationTarget, unit};
use core_model::ids::{PluginInstanceId, StripId};
use core_model::mixer::ParamId;

/// Most lanes (targets) one schedule plays; more are dropped and reported.
pub const MAX_AUTO_LANES: usize = 1024;
/// Shortest run the renderer splits a block into at an automation point
/// (Plan 4d Ruling 10).
pub const MIN_RUN: u32 = 32;

/// Which control of a strip a lane drives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StripParam {
    /// The fader.
    Fader,
    /// The pan.
    Pan,
    /// The mute.
    Mute,
    /// The solo.
    Solo,
}

/// What a lane drives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AutoDest {
    /// A strip control.
    Strip(StripId, StripParam),
    /// The level of a strip's send (by position).
    Send(StripId, u16),
    /// A plugin parameter.
    Plugin(PluginInstanceId, ParamId),
}

impl AutoDest {
    /// The lane destination of a target; `None` for velocity scale, which
    /// is applied when the schedule is compiled (Plan 4d Ruling 9).
    pub fn of(t: AutomationTarget) -> Option<AutoDest> {
        Some(match t {
            AutomationTarget::StripFader(s) => AutoDest::Strip(s, StripParam::Fader),
            AutomationTarget::StripPan(s) => AutoDest::Strip(s, StripParam::Pan),
            AutomationTarget::StripMute(s) => AutoDest::Strip(s, StripParam::Mute),
            AutomationTarget::StripSolo(s) => AutoDest::Strip(s, StripParam::Solo),
            AutomationTarget::SendLevel(s, i) => AutoDest::Send(s, i.0),
            AutomationTarget::PluginParam(p, id) => AutoDest::Plugin(p, id),
            AutomationTarget::VelocityScale(_) => return None,
        })
    }
}

/// One point: from `frame` on, `value` (normalised 0–1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AutoPoint {
    /// Schedule frame.
    pub frame: u64,
    /// Normalised value.
    pub value: f64,
}

/// One target's points.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutoLane {
    /// What it drives.
    pub dest: AutoDest,
    /// Holds each value until the next point (mute, solo, stepped
    /// parameters) instead of ramping.
    pub stepped: bool,
    /// For a plugin instrument's parameter: its target (`None`: an insert,
    /// found in the graph).
    pub instrument: Option<TargetIdx>,
    start: u32,
    end: u32,
}

/// Every lane of a schedule, sorted by destination.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Automation {
    lanes: Box<[AutoLane]>,
    points: Box<[AutoPoint]>,
    /// Every point frame, sorted, without repeats.
    cuts: Box<[u64]>,
}

impl Automation {
    /// Lanes from `(dest, frame, value)` given in play order: each lane
    /// sorted by frame, play order kept on a shared frame; at most
    /// [`MAX_AUTO_LANES`] lanes (returns how many more were dropped); mute
    /// and solo lanes are stepped. Not RT.
    pub fn build(mut raw: Vec<(AutoDest, u64, f64)>) -> (Automation, usize) {
        // Stable: play order stays on a shared frame.
        raw.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
        let mut lanes: Vec<AutoLane> = Vec::new();
        let mut points: Vec<AutoPoint> = Vec::with_capacity(raw.len());
        let mut dropped = 0usize;
        let mut current: Option<AutoDest> = None;
        for (dest, frame, value) in raw {
            if current != Some(dest) {
                current = Some(dest);
                if lanes.len() >= MAX_AUTO_LANES {
                    dropped += 1;
                    continue;
                }
                let start = u32::try_from(points.len()).unwrap_or(u32::MAX);
                lanes.push(AutoLane {
                    dest,
                    stepped: matches!(dest, AutoDest::Strip(_, StripParam::Mute | StripParam::Solo)),
                    instrument: None,
                    start,
                    end: start,
                });
            }
            let Some(lane) = lanes.last_mut().filter(|l| l.dest == dest) else {
                continue;
            };
            points.push(AutoPoint {
                frame,
                value: unit(value),
            });
            lane.end = u32::try_from(points.len()).unwrap_or(u32::MAX);
        }
        let mut cuts: Vec<u64> = points.iter().map(|p| p.frame).collect();
        cuts.sort_unstable();
        cuts.dedup();
        (
            Automation {
                lanes: lanes.into_boxed_slice(),
                points: points.into_boxed_slice(),
                cuts: cuts.into_boxed_slice(),
            },
            dropped,
        )
    }

    /// No lane. // RT
    pub fn is_empty(&self) -> bool {
        self.lanes.is_empty()
    }

    /// The lanes, sorted by destination. // RT
    pub fn lanes(&self) -> &[AutoLane] {
        &self.lanes
    }

    /// Points over every lane.
    pub fn point_count(&self) -> usize {
        self.points.len()
    }

    /// A lane's points, by frame. // RT
    pub fn points(&self, lane: &AutoLane) -> &[AutoPoint] {
        self.points
            .get(lane.start as usize..lane.end as usize)
            .unwrap_or(&[])
    }

    /// `lane`'s value at `frame` (Plan 4d Ruling 8): the first value before
    /// the first point, the last after the last, linear between unless the
    /// lane is stepped. With `left`, the limit from the left: a point
    /// exactly at `frame` is not yet in effect on a stepped lane, and of
    /// two points on one frame the earlier counts. // RT
    pub fn value(&self, lane: &AutoLane, frame: u64, left: bool) -> f64 {
        let pts = self.points(lane);
        let n = if left {
            pts.partition_point(|p| p.frame < frame)
        } else {
            pts.partition_point(|p| p.frame <= frame)
        };
        let Some(prev) = n.checked_sub(1).and_then(|i| pts.get(i)) else {
            return pts.first().map_or(0.0, |p| p.value);
        };
        if lane.stepped {
            return prev.value;
        }
        match pts.get(n) {
            Some(next) if next.frame > prev.frame => {
                let x = (frame - prev.frame) as f64 / (next.frame - prev.frame) as f64;
                prev.value + (next.value - prev.value) * x
            }
            _ => prev.value,
        }
    }

    /// The first point frame after `frame`. // RT
    pub fn next_cut(&self, frame: u64) -> Option<u64> {
        let i = self.cuts.partition_point(|c| *c <= frame);
        self.cuts.get(i).copied()
    }

    /// Whether some lane drives `dest`. // RT
    pub fn drives(&self, dest: AutoDest) -> bool {
        self.lanes.binary_search_by_key(&dest, |l| l.dest).is_ok()
    }

    /// Makes the lanes `stepped` says hold their values (parameters the
    /// plugin declares stepped). Not RT.
    pub fn mark_stepped(&mut self, stepped: impl Fn(AutoDest) -> bool) {
        for l in self.lanes.iter_mut() {
            if stepped(l.dest) {
                l.stepped = true;
            }
        }
    }

    /// Gives plugin-parameter lanes the target of their plugin instrument
    /// (`None`: an insert). Not RT.
    pub fn bind_instruments(&mut self, target: impl Fn(PluginInstanceId) -> Option<TargetIdx>) {
        for l in self.lanes.iter_mut() {
            if let AutoDest::Plugin(id, _) = l.dest {
                l.instrument = target(id);
            }
        }
    }
}
VIZDAW_EOF
```

In `crates/core-engine/src/schedule/mod.rs`, Find:

```rust
pub mod compile;
pub mod targets;
pub mod tempo;

pub use compile::{CompileOptions, CompileReport, Unroutable, compile};
```

Replace:

```rust
pub mod automation;
pub mod compile;
pub mod targets;
pub mod tempo;

pub use automation::{
    AutoDest, AutoLane, AutoPoint, Automation, MAX_AUTO_LANES, MIN_RUN, StripParam,
};
pub use compile::{CompileOptions, CompileReport, Unroutable, compile};
```

Find:

```rust
    /// Controller state to chase after a locate, swap or wrap.
    pub chase: ChaseIndex,
}
```

Replace:

```rust
    /// Controller state to chase after a locate, swap or wrap.
    pub chase: ChaseIndex,
    /// Automation lanes (4d).
    pub automation: Automation,
}
```

- [ ] **Step 4: Compile automation and velocity scale**

In `crates/core-engine/src/schedule/compile.rs`, Find:

```rust
use super::targets::{BuiltinRoute, TargetIdx, TargetKind, kinds_for};
```

Replace:

```rust
use super::automation::{AutoDest, Automation, MAX_AUTO_LANES};
use super::targets::{BuiltinRoute, TargetIdx, TargetKind, kinds_for};
use core_model::automation::{AutomationTarget, velocity_scale_percent};
```

Find:

```rust
    /// The mixer graph compiled with this schedule (`Engine::load`).
    pub graph: crate::graph::GraphReport,
}
```

Replace:

```rust
    /// The mixer graph compiled with this schedule (`Engine::load`).
    pub graph: crate::graph::GraphReport,
    /// Automation lanes (targets) the schedule plays (4d).
    pub automation_lanes: usize,
    /// Automation points in those lanes.
    pub automation_points: usize,
    /// Automation points whose strip, send, plugin or destination is gone
    /// (skipped, Plan 4d Ruling 3).
    pub automation_missing: usize,
    /// Lanes beyond [`MAX_AUTO_LANES`], dropped.
    pub automation_dropped_lanes: usize,
}
```

Find:

```rust
    for r in flatten(doc, seq, None, &ViewFilter::default())? {
        if matches!(r.body, EventBody::AudioRegion { .. }) {
```

Replace:

```rust
    let flat: Vec<_> = flatten(doc, seq, None, &ViewFilter::default())?.collect();
    // Automation (Plan 4d Rulings 7–9): lanes for the renderer; velocity
    // scale is applied to the note-ons below.
    let mut raw: Vec<(AutoDest, u64, f64)> = Vec::new();
    let mut vscale: BTreeMap<DestId, Vec<(Tick, f64)>> = BTreeMap::new();
    for r in &flat {
        let EventBody::Automation { target, value } = r.body else {
            continue;
        };
        if !target_exists(doc, target) {
            report.automation_missing += 1;
            continue;
        }
        match (AutoDest::of(target), target) {
            (Some(d), _) => raw.push((d, frame(r.tick), value)),
            (None, AutomationTarget::VelocityScale(dest)) => {
                vscale.entry(dest).or_default().push((r.tick, value));
            }
            (None, _) => {}
        }
    }
    let (automation, dropped) = Automation::build(raw);
    report.automation_lanes = automation.lanes().len();
    report.automation_points = automation.point_count();
    report.automation_dropped_lanes = dropped;

    for r in flat {
        if matches!(r.body, EventBody::AudioRegion { .. }) {
```

Find:

```rust
        let t = translate(&r.body, channel);
```

Replace:

```rust
        let scaled = vscale
            .get(&dest)
            .and_then(|pts| scale_velocity(&r.body, pts, r.tick));
        let t = translate(scaled.as_ref().unwrap_or(&r.body), channel);
```

Find:

```rust
        used_channels,
        chase,
    };
```

Replace:

```rust
        used_channels,
        chase,
        automation,
    };
```

Find:

```rust
fn mark(used: &mut [u16; MAX_TARGETS], target: TargetIdx, channel: u8) {
```

Replace:

```rust
/// Whether an automation target's strip, send, plugin or destination
/// exists.
fn target_exists(doc: &Document, t: AutomationTarget) -> bool {
    let m = doc.mixer();
    match t {
        AutomationTarget::StripFader(s)
        | AutomationTarget::StripPan(s)
        | AutomationTarget::StripMute(s)
        | AutomationTarget::StripSolo(s) => m.strip(s).is_ok(),
        AutomationTarget::SendLevel(s, i) => m
            .strip(s)
            .is_ok_and(|x| usize::from(i.0) < x.sends.len()),
        AutomationTarget::PluginParam(p, _) => doc.plugin_slot(p).is_some(),
        AutomationTarget::VelocityScale(d) => doc.instruments().contains_key(&d),
    }
}

/// A velocity-scale lane's value at `tick` (Plan 4d Ruling 8).
fn lane_value(points: &[(Tick, f64)], tick: Tick) -> f64 {
    let n = points.partition_point(|p| p.0 <= tick);
    match (n.checked_sub(1).and_then(|i| points.get(i)), points.get(n)) {
        (None, Some(next)) => next.1,
        (Some(prev), Some(next)) if next.0 > prev.0 => {
            prev.1 + (next.1 - prev.1) * (tick - prev.0).0 as f64 / (next.0 - prev.0).0 as f64
        }
        (Some(prev), _) => prev.1,
        (None, None) => 0.5,
    }
}

/// A note `body` with its velocity scaled by the lane at `tick` (clamped
/// 1–65 535, Plan 4d Ruling 9); `None` for anything but a note.
fn scale_velocity(body: &EventBody, points: &[(Tick, f64)], tick: Tick) -> Option<EventBody> {
    let EventBody::Note { velocity, .. } = body else {
        return None;
    };
    let pct = velocity_scale_percent(lane_value(points, tick));
    let v = (f64::from(*velocity) * pct / 100.0).round().clamp(1.0, 65_535.0) as u16;
    let mut b = body.clone();
    if let EventBody::Note { velocity, .. } = &mut b {
        *velocity = v;
    }
    Some(b)
}

fn mark(used: &mut [u16; MAX_TARGETS], target: TargetIdx, channel: u8) {
```

Find:

```rust
    if r.skipped_audio > 0 {
        w.push(format!(
            "{} audio region(s) skipped (audio playback is not available yet)",
            r.skipped_audio
        ));
    }
    w
}
```

Replace:

```rust
    if r.skipped_audio > 0 {
        w.push(format!(
            "{} audio region(s) skipped (audio playback is not available yet)",
            r.skipped_audio
        ));
    }
    if r.automation_missing > 0 {
        w.push(format!(
            "{} automation point(s) skipped: their strip, send, plugin or destination is gone",
            r.automation_missing
        ));
    }
    if r.automation_dropped_lanes > 0 {
        w.push(format!(
            "{} automation lane(s) dropped: at most {MAX_AUTO_LANES} play",
            r.automation_dropped_lanes
        ));
    }
    w
}
```

- [ ] **Step 5: Bind plugin lanes in `Engine::load`**

In `crates/core-engine/src/graph/node.rs`, Find:

```rust
use core_model::mixer::{PluginFormat, PluginRef, PluginSlot};
```

Replace:

```rust
use core_model::ids::PluginInstanceId;
use core_model::mixer::{ParamId, PluginFormat, PluginRef, PluginSlot};
```

Find:

```rust
        Err(format!(
            "{} cannot load until plugin hosting exists",
            slot.plugin.name
        ))
    }
}

/// The factory before plugin hosting: every slot is a placeholder.
```

Replace:

```rust
        Err(format!(
            "{} cannot load until plugin hosting exists",
            slot.plugin.name
        ))
    }

    /// Whether parameter `param` of instance `slot` is stepped: its
    /// automation holds instead of ramping (Plan 4d Ruling 8). Unknown
    /// parameters are not.
    fn param_stepped(&self, slot: PluginInstanceId, param: ParamId) -> bool {
        let _ = (slot, param);
        false
    }
}

/// The factory before plugin hosting: every slot is a placeholder.
```

In `crates/core-engine/src/engine.rs`, Find:

```rust
        let compiled = compile(doc, seq, &opts, &mut resolve);
        let (schedule, mut report) = match compiled {
```

Replace:

```rust
        let compiled = compile(doc, seq, &opts, &mut resolve);
        let (mut schedule, mut report) = match compiled {
```

Find:

```rust
        report.graph = self.compile_graph(doc)?;
```

Replace:

```rust
        // Plugin-parameter lanes: a plugin instrument's target, and the
        // parameters the plugin declares stepped (Plan 4d Rulings 8, 12).
        let targets = &self.targets;
        schedule
            .automation
            .bind_instruments(|id| targets.find(&TargetKind::PluginInstrument(id)));
        let inserts = &self.inserts;
        schedule.automation.mark_stepped(|d| match d {
            crate::schedule::AutoDest::Plugin(id, p) => inserts.param_stepped(id, p),
            _ => false,
        });
        report.graph = self.compile_graph(doc)?;
```

- [ ] **Step 6: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-engine 2>&1 | grep -E "^test result|FAILED|panicked"
```

Expected: PASS (the demo's golden logs are unchanged: no automation, no velocity scale).

- [ ] **Step 7: Gate and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace 2>&1 | grep -E "^test result|FAILED|panicked" | tail -60
git add crates/core-engine/src/schedule/automation.rs crates/core-engine/src/schedule/mod.rs crates/core-engine/src/schedule/compile.rs crates/core-engine/src/engine.rs crates/core-engine/src/graph/node.rs crates/core-engine/tests/schedule_automation.rs
git commit -m "feat(engine): automation lanes in the schedule; velocity scale at compile time

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
```

Expected: 705 Rust tests listed (701 + 4).

---

### Task 7: Engine — automatable mixer controls in the render graph

**Files:**
- Create: `crates/core-engine/src/graph/controls.rs`
- Modify: `crates/core-engine/src/graph/mod.rs`, `crates/core-engine/src/graph/exec.rs`, `crates/core-engine/src/graph/mixer.rs`, `crates/core-engine/src/graph/dsp.rs`, `crates/core-engine/src/engine.rs`
- Test: `crates/core-engine/tests/graph_controls.rs`

**Interfaces:**
- Consumes: Task 6's `StripParam`, `AutoDest`; `core_model::automation::{fader_db, pan_of, switch_on}`; `mixer_rules::{routing_edges, EdgeKind}`; `dsp::strip_gains`, `db_to_gain`.
- Produces: `graph::controls::{SendSpec { index: u16, level_db: f64, enabled: bool }, FeedSpec { to: usize, input: usize, send: Option<SendSpec> }, StripControlSpec { strip, node, gain_db, pan, apply_pan, mute, solo, feeds: Vec<FeedSpec>, solo_keeps: Vec<StripId> }, Controls}`; `MixerGraph.controls: Vec<StripControlSpec>`; `Graph::{set_controls(&mut self, &[StripControlSpec]), automate_strip(&mut self, StripId, StripParam, f64), automate_send(&mut self, StripId, u16, f64), release_overrides(&mut self, impl Fn(AutoDest) -> bool), commit_controls(&mut self, ramp: bool), settle(&mut self), insert_mut(&mut self, PluginInstanceId) -> Option<&mut (dyn EffectNode + 'static)>, gains(&self, node) -> Option<(f32, f32, bool)>, edge_gains(&self, node) -> Vec<f32>}` (all but `set_controls`, `gains` and `edge_gains` are `// RT`); `Runner::graph_mut(&mut self) -> &mut Graph`; `dsp::{mix_into_ramp(dst, src, from: f32, to: f32, frames), scale_ramp(buf, from: (f32, f32), to: (f32, f32), frames)}`; `Engine::compile_graph` installs the controls.

Rulings 11, 13. A commit without any override (and nothing released since the last commit) writes nothing, so a graph without automation renders bit-for-bit as before. Overrides survive a graph swap (`adopt_from`) only while the control's static value (fader dB, pan, mute, solo, a send's level and on/off) is unchanged.

- [ ] **Step 1: Write the failing tests**

Create `crates/core-engine/tests/graph_controls.rs`:

```rust
//! Automatable mixer controls (Plan 4d Rulings 11, 13).

use core_engine::backend::StereoBuf;
use core_engine::fixtures::Builder;
use core_engine::graph::dsp::{mix_into, mix_into_ramp, scale, scale_ramp, strip_gains};
use core_engine::graph::mixer::mixer_spec;
use core_engine::graph::{Graph, InsertInit, InstrumentFeed, Runner, plan};
use core_engine::schedule::{AutoDest, StripParam, TargetIdx};
use core_model::automation::{fader_db, fader_norm, pan_norm};
use core_model::command::MixerCmd;
use core_model::document::Document;
use core_model::ids::StripId;
use core_model::mixer::{AuxSend, PluginSlot, StripKind, db_to_gain};

fn zero(_: &PluginSlot) -> u32 {
    0
}

/// `doc`'s graph with its controls, and each strip's node.
fn built(doc: &Document) -> (Graph, Vec<(usize, StripId)>) {
    let mg = mixer_spec(doc, &[InstrumentFeed::builtin(TargetIdx(0), 0)], &mut zero);
    let p = plan(&mg.spec, 96_000).unwrap();
    let mut g = Graph::build(&mg.spec, &p, 512, |_, _| InsertInit::Placeholder);
    g.set_controls(&mg.controls);
    (g, mg.strips)
}

fn node(strips: &[(usize, StripId)], s: StripId) -> usize {
    strips.iter().find(|x| x.1 == s).unwrap().0
}

/// The built-in synth to master with a post-fader send to bus A; bus B
/// empty. Returns the builder, synth, A, B and master.
fn routed() -> (Builder, StripId, StripId, StripId, StripId) {
    let mut b = Builder::new();
    b.dest("Synth", "builtin:synth", 0).unwrap();
    let synth = b.strip_of(StripKind::BuiltinSynth).unwrap();
    let a = b.bus("A").unwrap();
    let bus_b = b.bus("B").unwrap();
    b.mixer(MixerCmd::AddSend {
        strip: synth,
        send: AuxSend::new(a),
    })
    .unwrap();
    let master = b.doc().mixer().master();
    (b, synth, a, bus_b, master)
}

#[test]
fn automated_faders_pans_and_sends_set_the_gains() {
    let (b, synth, a, _, master) = routed();
    let (mut g, strips) = built(b.doc());
    let (sn, an, mn) = (node(&strips, synth), node(&strips, a), node(&strips, master));
    let (l0, r0) = strip_gains(0.0, 0.0, true);
    assert_eq!(g.gains(sn), Some((l0, r0, true)));
    // A commit without overrides changes nothing.
    g.commit_controls(false);
    assert_eq!(g.gains(sn), Some((l0, r0, true)));
    g.automate_strip(synth, StripParam::Fader, fader_norm(-6.0));
    g.automate_strip(synth, StripParam::Pan, pan_norm(0.5));
    g.commit_controls(false);
    let (gl, gr) = strip_gains(fader_db(fader_norm(-6.0)), 0.5, true);
    assert_eq!(g.gains(sn), Some((gl, gr, true)));
    g.automate_send(synth, 0, fader_norm(-12.0));
    g.commit_controls(false);
    assert_eq!(g.edge_gains(an), vec![db_to_gain(fader_db(fader_norm(-12.0))) as f32]);
    // Master has no pan.
    g.automate_strip(master, StripParam::Pan, 0.0);
    g.commit_controls(false);
    assert_eq!(g.gains(mn), Some((1.0, 1.0, true)));
}

#[test]
fn mute_and_solo_follow_the_routing() {
    let (b, synth, a, bus_b, master) = routed();
    let (mut g, strips) = built(b.doc());
    let n = |s| node(&strips, s);
    g.automate_strip(synth, StripParam::Mute, 1.0);
    g.commit_controls(false);
    assert_eq!(g.gains(n(synth)).map(|x| x.2), Some(false));
    assert_eq!(g.edge_gains(n(a)), vec![0.0], "a muted strip's send carries nothing");
    assert!(g.edge_gains(n(master)).contains(&0.0), "nor its output");
    g.automate_strip(synth, StripParam::Mute, 0.0);
    // Soloing B keeps B and master; the synth and A go quiet.
    g.automate_strip(bus_b, StripParam::Solo, 1.0);
    g.commit_controls(false);
    let on = |g: &Graph, s| g.gains(n(s)).unwrap().2;
    assert!(on(&g, bus_b) && on(&g, master));
    assert!(!on(&g, synth) && !on(&g, a));
    // Soloing A keeps the synth that feeds it.
    g.automate_strip(bus_b, StripParam::Solo, 0.0);
    g.automate_strip(a, StripParam::Solo, 1.0);
    g.commit_controls(false);
    assert!(on(&g, synth) && on(&g, a) && on(&g, master) && !on(&g, bus_b));
}

#[test]
fn a_new_graph_keeps_overrides_only_while_the_static_value_is_unchanged() {
    let (mut b, synth, a, _, _) = routed();
    let (g, strips) = built(b.doc());
    let (sn, an) = (node(&strips, synth), node(&strips, a));
    let mut r = Runner::serial(g);
    r.graph_mut()
        .automate_strip(synth, StripParam::Fader, fader_norm(-12.0));
    r.graph_mut().automate_send(synth, 0, fader_norm(-20.0));
    r.graph_mut().commit_controls(false);
    let held = r.graph_mut().gains(sn);
    let held_send = r.graph_mut().edge_gains(an);
    r.replace_graph(built(b.doc()).0);
    assert_eq!(r.graph_mut().gains(sn), held, "an unchanged fader keeps its override");
    assert_eq!(r.graph_mut().edge_gains(an), held_send);
    // The user moves the fader: its override goes; the send's stays.
    b.mixer(MixerCmd::SetGain {
        strip: synth,
        gain_db: -3.0,
    })
    .unwrap();
    r.replace_graph(built(b.doc()).0);
    let (gl, gr) = strip_gains(-3.0, 0.0, true);
    assert_eq!(r.graph_mut().gains(sn), Some((gl, gr, true)));
    assert_eq!(r.graph_mut().edge_gains(an), held_send);
    // Dropping the overrides no lane drives brings the static values back.
    r.graph_mut()
        .release_overrides(|d| d == AutoDest::Strip(synth, StripParam::Pan));
    r.graph_mut().commit_controls(false);
    assert_eq!(r.graph_mut().edge_gains(an), vec![1.0]);
}

#[test]
fn ramps_move_linearly_across_a_run() {
    let mut buf = StereoBuf::new(4);
    buf.left.copy_from_slice(&[1.0; 4]);
    buf.right.copy_from_slice(&[1.0; 4]);
    scale_ramp(&mut buf, (0.0, 1.0), (1.0, 1.0), 4);
    assert_eq!(&buf.left[..], &[0.0, 0.25, 0.5, 0.75]);
    assert_eq!(&buf.right[..], &[1.0; 4]);
    let mut src = StereoBuf::new(4);
    src.left.copy_from_slice(&[1.0; 4]);
    src.right.copy_from_slice(&[2.0; 4]);
    let mut dst = StereoBuf::new(4);
    mix_into_ramp(&mut dst, &src, 1.0, 0.0, 4);
    assert_eq!(&dst.left[..], &[1.0, 0.75, 0.5, 0.25]);
    assert_eq!(&dst.right[..], &[2.0, 1.5, 1.0, 0.5]);
    // Equal ends are the constant functions.
    let (mut x, mut y) = (src.clone(), src.clone());
    scale_ramp(&mut x, (0.3, 0.7), (0.3, 0.7), 4);
    scale(&mut y, 0.3, 0.7, 4);
    assert_eq!((&x.left[..], &x.right[..]), (&y.left[..], &y.right[..]));
    let (mut x, mut y) = (StereoBuf::new(4), StereoBuf::new(4));
    mix_into_ramp(&mut x, &src, 0.5, 0.5, 4);
    mix_into(&mut y, &src, 0.5, 4);
    assert_eq!(&x.left[..], &y.left[..]);
}
```

- [ ] **Step 2: Run them to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-engine --test graph_controls 2>&1 | tail -5
```

Expected: compile errors (`no function mix_into_ramp`, `no method set_controls`).

- [ ] **Step 3: Write the ramps**

In `crates/core-engine/src/graph/dsp.rs`, Find:

```rust
/// Multiplies the left channel by `gl` and the right by `gr`. // RT
pub fn scale(buf: &mut StereoBuf, gl: f32, gr: f32, frames: u32) {
```

Replace:

```rust
/// `dst += g(i) × src` with `g` moving linearly from `from` at frame 0
/// toward `to` (reached just after the last frame); equal ends are
/// exactly [`mix_into`] (Plan 4d Ruling 11). // RT
pub fn mix_into_ramp(dst: &mut StereoBuf, src: &StereoBuf, from: f32, to: f32, frames: u32) {
    if from == to {
        return mix_into(dst, src, to, frames);
    }
    let n = len(frames, dst).min(len(frames, src));
    let step = (to - from) / n.max(1) as f32;
    for (d, s) in [(&mut dst.left, &src.left), (&mut dst.right, &src.right)] {
        if let (Some(d), Some(s)) = (d.get_mut(..n), s.get(..n)) {
            for (i, (a, b)) in d.iter_mut().zip(s).enumerate() {
                *a += (from + step * i as f32) * *b;
            }
        }
    }
}

/// The left channel times a gain moving from `from.0` toward `to.0`, the
/// right from `from.1` toward `to.1`; equal ends are exactly [`scale`].
/// // RT
pub fn scale_ramp(buf: &mut StereoBuf, from: (f32, f32), to: (f32, f32), frames: u32) {
    if from == to {
        return scale(buf, to.0, to.1, frames);
    }
    let n = len(frames, buf);
    for (c, (a, b)) in [(&mut buf.left, (from.0, to.0)), (&mut buf.right, (from.1, to.1))] {
        let step = (b - a) / n.max(1) as f32;
        if let Some(c) = c.get_mut(..n) {
            for (i, x) in c.iter_mut().enumerate() {
                *x *= a + step * i as f32;
            }
        }
    }
}

/// Multiplies the left channel by `gl` and the right by `gr`. // RT
pub fn scale(buf: &mut StereoBuf, gl: f32, gr: f32, frames: u32) {
```

Find:

```rust
/// Fader × pan as `(left, right)` gains; without pan (master, Plan
/// Ruling 4) both are the fader gain.
```

Replace:

```rust
/// Fader × pan as `(left, right)` gains; without pan (master, Plan
/// Ruling 4) both are the fader gain. Also used on the audio thread by
/// automation (it neither allocates nor panics). // RT
```

- [ ] **Step 4: Write the controls**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-engine/src/graph/controls.rs <<'VIZDAW_EOF'
//! Automatable mixer controls of a render graph (Plan 4d Rulings 11, 13):
//! every strip's fader, pan, mute and solo and every send's level, the
//! static value the mixer gave each, and the override automation set.
//! Built off the audio thread; set, committed and adopted on it. // RT

use crate::graph::dsp::strip_gains;
use crate::schedule::automation::{AutoDest, StripParam};
use core_model::automation::{fader_db, pan_of, switch_on};
use core_model::ids::StripId;
use core_model::mixer::db_to_gain;
use std::collections::BTreeMap;
use std::ops::Range;

/// A send as the mixer set it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SendSpec {
    /// Position in the strip's sends.
    pub index: u16,
    /// Level in dB.
    pub level_db: f64,
    /// On.
    pub enabled: bool,
}

/// An edge a strip feeds: input `input` (its position in that node's
/// `StripSpec::inputs`) of node `to` — the strip's main output, or a send.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FeedSpec {
    /// Target node.
    pub to: usize,
    /// Input position in the target's spec.
    pub input: usize,
    /// The send, or `None` for the main output.
    pub send: Option<SendSpec>,
}

/// One strip's static controls and what it feeds, from the mixer.
#[derive(Debug, Clone, PartialEq)]
pub struct StripControlSpec {
    /// The strip.
    pub strip: StripId,
    /// Its graph node.
    pub node: usize,
    /// Fader in dB.
    pub gain_db: f64,
    /// Pan −1 … +1.
    pub pan: f64,
    /// Pan applies (not master).
    pub apply_pan: bool,
    /// Muted.
    pub mute: bool,
    /// Soloed.
    pub solo: bool,
    /// Its output and sends.
    pub feeds: Vec<FeedSpec>,
    /// Strips audible while it is soloed: itself, everything it reaches and
    /// everything reaching it (sidechains excluded).
    pub solo_keeps: Vec<StripId>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Vals {
    db: f64,
    pan: f64,
    mute: bool,
    solo: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Over {
    db: Option<f64>,
    pan: Option<f64>,
    mute: Option<bool>,
    solo: Option<bool>,
}

#[derive(Debug)]
struct CtlStrip {
    strip: StripId,
    node: usize,
    apply_pan: bool,
    base: Vals,
    over: Over,
    feeds: Range<usize>,
    keeps: Range<usize>,
}

#[derive(Debug)]
struct CtlFeed {
    /// Index into the graph's inputs.
    input: usize,
    send: Option<SendSpec>,
    over_db: Option<f64>,
}

/// A graph's controls (empty for graphs built without a mixer).
#[derive(Debug, Default)]
pub struct Controls {
    /// Sorted by strip.
    strips: Box<[CtlStrip]>,
    feeds: Box<[CtlFeed]>,
    /// Per strip, bit words of the strips its solo keeps.
    keeps: Box<[u64]>,
    /// Scratch: the audible strips.
    audible: Box<[u64]>,
    any_override: bool,
    /// Overrides were dropped: the next commit writes the static values.
    dirty: bool,
}

impl Controls {
    /// The controls of `specs`; `input(node, k)` is the graph's index of
    /// input `k` of node `node` (`None`: the graph dropped that edge). Not
    /// RT.
    pub(crate) fn build(specs: &[StripControlSpec], input: impl Fn(usize, usize) -> Option<usize>) -> Controls {
        let mut sorted: Vec<&StripControlSpec> = specs.iter().collect();
        sorted.sort_by_key(|s| s.strip);
        sorted.dedup_by_key(|s| s.strip);
        let index: BTreeMap<StripId, usize> = sorted.iter().enumerate().map(|(i, s)| (s.strip, i)).collect();
        let n = sorted.len();
        let words = n.div_ceil(64);
        let mut strips = Vec::with_capacity(n);
        let mut feeds = Vec::new();
        let mut keeps = vec![0u64; n * words];
        for (i, s) in sorted.iter().enumerate() {
            let first = feeds.len();
            for f in &s.feeds {
                if let Some(input) = input(f.to, f.input) {
                    feeds.push(CtlFeed {
                        input,
                        send: f.send,
                        over_db: None,
                    });
                }
            }
            for k in &s.solo_keeps {
                if let Some(j) = index.get(k) {
                    if let Some(w) = keeps.get_mut(i * words + j / 64) {
                        *w |= 1u64 << (j % 64);
                    }
                }
            }
            strips.push(CtlStrip {
                strip: s.strip,
                node: s.node,
                apply_pan: s.apply_pan,
                base: Vals {
                    db: s.gain_db,
                    pan: s.pan,
                    mute: s.mute,
                    solo: s.solo,
                },
                over: Over::default(),
                feeds: first..feeds.len(),
                keeps: i * words..(i + 1) * words,
            });
        }
        Controls {
            strips: strips.into_boxed_slice(),
            feeds: feeds.into_boxed_slice(),
            keeps: keeps.into_boxed_slice(),
            audible: vec![0u64; words].into_boxed_slice(),
            any_override: false,
            dirty: false,
        }
    }

    fn find(&self, strip: StripId) -> Option<usize> {
        self.strips.binary_search_by_key(&strip, |s| s.strip).ok()
    }

    /// Automation's value (normalised) for `strip`'s `p`. // RT
    pub(crate) fn set_strip(&mut self, strip: StripId, p: StripParam, v: f64) {
        let Some(i) = self.find(strip) else {
            return;
        };
        let Some(s) = self.strips.get_mut(i) else {
            return;
        };
        match p {
            StripParam::Fader => s.over.db = Some(fader_db(v)),
            StripParam::Pan => s.over.pan = Some(pan_of(v)),
            StripParam::Mute => s.over.mute = Some(switch_on(v)),
            StripParam::Solo => s.over.solo = Some(switch_on(v)),
        }
        self.any_override = true;
    }

    /// Automation's level (normalised) for send `index` of `strip`. // RT
    pub(crate) fn set_send(&mut self, strip: StripId, index: u16, v: f64) {
        let Some(range) = self.find(strip).and_then(|i| self.strips.get(i)).map(|s| s.feeds.clone()) else {
            return;
        };
        for f in self.feeds.get_mut(range).into_iter().flatten() {
            if f.send.is_some_and(|s| s.index == index) {
                f.over_db = Some(fader_db(v));
                self.any_override = true;
            }
        }
    }

    /// Drops the overrides of the controls `driven` says no lane drives;
    /// the next commit writes their static values. // RT
    pub(crate) fn release(&mut self, driven: impl Fn(AutoDest) -> bool) {
        for s in self.strips.iter_mut() {
            let id = s.strip;
            if !driven(AutoDest::Strip(id, StripParam::Fader)) {
                s.over.db = None;
            }
            if !driven(AutoDest::Strip(id, StripParam::Pan)) {
                s.over.pan = None;
            }
            if !driven(AutoDest::Strip(id, StripParam::Mute)) {
                s.over.mute = None;
            }
            if !driven(AutoDest::Strip(id, StripParam::Solo)) {
                s.over.solo = None;
            }
            for f in self.feeds.get_mut(s.feeds.clone()).into_iter().flatten() {
                if let Some(send) = f.send {
                    if !driven(AutoDest::Send(id, send.index)) {
                        f.over_db = None;
                    }
                }
            }
        }
        self.dirty = true;
        self.any_override = self.has_overrides();
    }

    fn has_overrides(&self) -> bool {
        self.strips.iter().any(|s| s.over != Over::default())
            || self.feeds.iter().any(|f| f.over_db.is_some())
    }

    /// Takes `old`'s overrides for the controls whose static value did not
    /// change (Plan 4d Ruling 13); true when one was taken. // RT
    pub(crate) fn adopt(&mut self, old: &Controls) -> bool {
        for s in self.strips.iter_mut() {
            let Some(o) = old
                .find(s.strip)
                .and_then(|j| old.strips.get(j))
            else {
                continue;
            };
            if o.base.db == s.base.db {
                s.over.db = o.over.db;
            }
            if o.base.pan == s.base.pan {
                s.over.pan = o.over.pan;
            }
            if o.base.mute == s.base.mute {
                s.over.mute = o.over.mute;
            }
            if o.base.solo == s.base.solo {
                s.over.solo = o.over.solo;
            }
            let olds = old.feeds.get(o.feeds.clone()).unwrap_or(&[]);
            for f in self.feeds.get_mut(s.feeds.clone()).into_iter().flatten() {
                let Some(send) = f.send else {
                    continue;
                };
                let same = olds.iter().find(|of| {
                    of.send.is_some_and(|x| {
                        x.index == send.index && x.level_db == send.level_db && x.enabled == send.enabled
                    })
                });
                if let Some(of) = same {
                    f.over_db = of.over_db;
                }
            }
        }
        self.any_override = self.has_overrides();
        self.any_override
    }

    /// Writes the effective gains: `strip(node, gain_l, gain_r, audible)`
    /// per strip and `edge(input, gain)` per fed edge, as the mixer
    /// translation computes them (`graph::mixer`). Writes nothing (false)
    /// when no override exists and none was dropped. // RT
    pub(crate) fn commit(
        &mut self,
        mut strip: impl FnMut(usize, f32, f32, bool),
        mut edge: impl FnMut(usize, f32),
    ) -> bool {
        if !self.any_override && !self.dirty {
            return false;
        }
        self.dirty = false;
        let any_solo = self.strips.iter().any(|s| s.over.solo.unwrap_or(s.base.solo));
        if any_solo {
            self.audible.fill(0);
            for s in self.strips.iter() {
                if s.over.solo.unwrap_or(s.base.solo) {
                    let keeps = self.keeps.get(s.keeps.clone()).unwrap_or(&[]);
                    for (a, k) in self.audible.iter_mut().zip(keeps) {
                        *a |= *k;
                    }
                }
            }
        } else {
            self.audible.fill(u64::MAX);
        }
        for (i, s) in self.strips.iter().enumerate() {
            let kept = self
                .audible
                .get(i / 64)
                .is_some_and(|w| w & (1u64 << (i % 64)) != 0);
            let on = kept && !s.over.mute.unwrap_or(s.base.mute);
            let (gl, gr) = strip_gains(
                s.over.db.unwrap_or(s.base.db),
                s.over.pan.unwrap_or(s.base.pan),
                s.apply_pan,
            );
            strip(s.node, gl, gr, on);
            let pass = if on { 1.0 } else { 0.0 };
            for f in self.feeds.get(s.feeds.clone()).unwrap_or(&[]) {
                let gain = match f.send {
                    None => pass,
                    Some(send) => {
                        let level = if send.enabled {
                            db_to_gain(f.over_db.unwrap_or(send.level_db)) as f32
                        } else {
                            0.0
                        };
                        pass * level
                    }
                };
                edge(f.input, gain);
            }
        }
        true
    }
}
VIZDAW_EOF
```

In `crates/core-engine/src/graph/mod.rs`, Find:

```rust
pub mod delay;
pub mod dsp;
```

Replace:

```rust
pub mod controls;
pub mod delay;
pub mod dsp;
```

Find:

```rust
pub use exec::{Graph, InsertInit, Runner, SinkTable};
```

Replace:

```rust
pub use controls::{FeedSpec, SendSpec, StripControlSpec};
pub use exec::{Graph, InsertInit, Runner, SinkTable};
```

- [ ] **Step 5: Record the controls in the mixer translation**

In `crates/core-engine/src/graph/mixer.rs`, Find:

```rust
use crate::graph::dsp::strip_gains;
use crate::graph::spec::{GraphSpec, InputSpec, InsertSpec, NodeSpec, StripSpec, Tap};
use crate::schedule::TargetIdx;
use core_model::document::Document;
use core_model::ids::{DestId, StripId};
use core_model::mixer::{Output, PluginSlot, Strip, StripKind, db_to_gain};
use core_model::mixer_rules::audible;
use std::collections::BTreeMap;
```

Replace:

```rust
use crate::graph::controls::{FeedSpec, SendSpec, StripControlSpec};
use crate::graph::dsp::strip_gains;
use crate::graph::spec::{GraphSpec, InputSpec, InsertSpec, NodeSpec, StripSpec, Tap};
use crate::schedule::TargetIdx;
use core_model::document::Document;
use core_model::ids::{DestId, StripId};
use core_model::mixer::{Output, PluginSlot, Strip, StripKind, db_to_gain};
use core_model::mixer_rules::{EdgeKind, audible, routing_edges};
use std::collections::{BTreeMap, BTreeSet};
```

Find:

```rust
    /// Problems worth showing (not errors).
    pub warnings: Vec<String>,
}
```

Replace:

```rust
    /// Problems worth showing (not errors).
    pub warnings: Vec<String>,
    /// Every strip's automatable controls (4d, `Graph::set_controls`).
    pub controls: Vec<StripControlSpec>,
}
```

Find:

```rust
    let muted = |s: &Strip| s.mute || !on.contains(&s.id);
    for s in m.strips().values() {
        let Some(from) = node(s.id) else {
            continue;
        };
        let pass = if muted(s) { 0.0 } else { 1.0 };
        if s.id != m.master() {
            let target = match s.output {
                Output::Master => m.master(),
                Output::Bus(b) => b,
            };
            if let Some(t) = node(target)
                .and_then(|t| t.checked_sub(base))
                .and_then(|k| strips.get_mut(k))
            {
                t.inputs.push(InputSpec {
                    from,
                    tap: Tap::Post,
                    gain: pass,
                });
            }
        }
        for x in s.sends.iter() {
            let level = if x.enabled {
                db_to_gain(x.level_db) as f32
            } else {
                0.0
            };
            if let Some(t) = node(x.target)
                .and_then(|t| t.checked_sub(base))
                .and_then(|k| strips.get_mut(k))
            {
                t.inputs.push(InputSpec {
                    from,
                    tap: if x.pre_fader { Tap::Pre } else { Tap::Post },
                    gain: pass * level,
                });
            }
        }
    }
```

Replace:

```rust
    let muted = |s: &Strip| s.mute || !on.contains(&s.id);
    // What each strip feeds, for automation (Plan 4d Ruling 11).
    let mut feeds: BTreeMap<StripId, Vec<FeedSpec>> = BTreeMap::new();
    for s in m.strips().values() {
        let Some(from) = node(s.id) else {
            continue;
        };
        let pass = if muted(s) { 0.0 } else { 1.0 };
        if s.id != m.master() {
            let target = match s.output {
                Output::Master => m.master(),
                Output::Bus(b) => b,
            };
            if let Some(to) = node(target) {
                if let Some(t) = to.checked_sub(base).and_then(|k| strips.get_mut(k)) {
                    feeds.entry(s.id).or_default().push(FeedSpec {
                        to,
                        input: t.inputs.len(),
                        send: None,
                    });
                    t.inputs.push(InputSpec {
                        from,
                        tap: Tap::Post,
                        gain: pass,
                    });
                }
            }
        }
        for (i, x) in s.sends.iter().enumerate() {
            let level = if x.enabled {
                db_to_gain(x.level_db) as f32
            } else {
                0.0
            };
            if let Some(to) = node(x.target) {
                if let Some(t) = to.checked_sub(base).and_then(|k| strips.get_mut(k)) {
                    feeds.entry(s.id).or_default().push(FeedSpec {
                        to,
                        input: t.inputs.len(),
                        send: Some(SendSpec {
                            index: u16::try_from(i).unwrap_or(u16::MAX),
                            level_db: x.level_db,
                            enabled: x.enabled,
                        }),
                    });
                    t.inputs.push(InputSpec {
                        from,
                        tap: if x.pre_fader { Tap::Pre } else { Tap::Post },
                        gain: pass * level,
                    });
                }
            }
        }
    }
    // A soloed strip keeps what it reaches and what reaches it, as
    // `mixer_rules::audible` does for the static solos.
    let mut down: BTreeMap<StripId, Vec<StripId>> = BTreeMap::new();
    let mut up: BTreeMap<StripId, Vec<StripId>> = BTreeMap::new();
    for (a, b, k) in routing_edges(m) {
        if !matches!(k, EdgeKind::Sidechain(_)) {
            down.entry(a).or_default().push(b);
            up.entry(b).or_default().push(a);
        }
    }
    let keeps = |s: StripId| -> Vec<StripId> {
        let mut set = BTreeSet::from([s]);
        for adj in [&down, &up] {
            let mut stack = vec![s];
            while let Some(n) = stack.pop() {
                for x in adj.get(&n).into_iter().flatten() {
                    if set.insert(*x) {
                        stack.push(*x);
                    }
                }
            }
        }
        set.into_iter().collect()
    };
    let mut controls: Vec<StripControlSpec> = Vec::new();
```

Find:

```rust
        let (gl, gr) = strip_gains(s.gain_db, s.pan, s.id != m.master());
        spec.gain_l = gl;
        spec.gain_r = gr;
        spec.audible = !muted(s);
```

Replace:

```rust
        let (gl, gr) = strip_gains(s.gain_db, s.pan, s.id != m.master());
        spec.gain_l = gl;
        spec.gain_r = gr;
        spec.audible = !muted(s);
        controls.push(StripControlSpec {
            strip: s.id,
            node: base + k,
            gain_db: s.gain_db,
            pan: s.pan,
            apply_pan: s.id != m.master(),
            mute: s.mute,
            solo: s.solo,
            feeds: feeds.remove(&s.id).unwrap_or_default(),
            solo_keeps: keeps(s.id),
        });
```

Find:

```rust
    MixerGraph {
        spec: GraphSpec { nodes, master },
        strips: strip_nodes,
        warnings,
    }
}
```

Replace:

```rust
    MixerGraph {
        spec: GraphSpec { nodes, master },
        strips: strip_nodes,
        warnings,
        controls,
    }
}
```

- [ ] **Step 6: Give the graph its controls**

In `crates/core-engine/src/graph/exec.rs`, Find:

```rust
use crate::backend::{BlockInfo, StereoBuf};
use crate::error::EngineError;
```

Replace:

```rust
use crate::backend::{BlockInfo, StereoBuf};
use crate::error::EngineError;
use crate::graph::controls::{Controls, StripControlSpec};
use crate::schedule::automation::{AutoDest, StripParam};
```

Find:

```rust
use core_model::ids::PluginInstanceId;
use crossbeam_queue::ArrayQueue;
use std::cell::UnsafeCell;
```

Replace:

```rust
use core_model::ids::{PluginInstanceId, StripId};
use crossbeam_queue::ArrayQueue;
use std::cell::UnsafeCell;
use std::collections::BTreeMap;
```

Find:

```rust
struct Input {
    from: usize,
    tap: Tap,
    gain: f32,
}
```

Replace:

```rust
struct Input {
    from: usize,
    tap: Tap,
    /// Gain at the end of the run (the static gain without automation).
    gain: f32,
    /// Gain at the start of the run; differs from `gain` only while a
    /// control ramps (Plan 4d Ruling 11).
    start: f32,
}
```

Find:

```rust
    Strip {
        inputs: Range<usize>,
        inserts: Range<usize>,
        gain_l: f32,
        gain_r: f32,
        audible: bool,
        meter: usize,
    },
}
```

Replace:

```rust
    Strip {
        inputs: Range<usize>,
        inserts: Range<usize>,
        gain_l: f32,
        gain_r: f32,
        /// Gains at the start of the run (see `Input::start`).
        start_l: f32,
        start_r: f32,
        audible: bool,
        meter: usize,
    },
}
```

Find:

```rust
pub struct Graph {
    defs: Box<[NodeDef]>,
    inputs: Box<[Input]>,
```

Replace:

```rust
pub struct Graph {
    defs: Box<[NodeDef]>,
    inputs: Box<[Input]>,
    /// `(node, spec input position)` of every input, for the controls.
    input_keys: Box<[(usize, usize)]>,
    /// `(instance, insert index)`, sorted by instance.
    insert_ids: Box<[(PluginInstanceId, usize)]>,
    controls: Controls,
    /// A commit ramps the gains of the next run; `settle` ends it.
    ramping: bool,
```

Find:

```rust
        let mut defs = Vec::with_capacity(n);
        let mut inputs = Vec::new();
```

Replace:

```rust
        let mut defs = Vec::with_capacity(n);
        let mut inputs = Vec::new();
        let mut input_keys: Vec<(usize, usize)> = Vec::new();
        let mut insert_ids: Vec<(PluginInstanceId, usize)> = Vec::new();
```

Find:

```rust
                        inputs.push(Input {
                            from: x.from,
                            tap: x.tap,
                            gain: x.gain,
                        });
                        delays.push(DelayLine::new(delay_at(&plan.input_delays, i, k)));
```

Replace:

```rust
                        inputs.push(Input {
                            from: x.from,
                            tap: x.tap,
                            gain: x.gain,
                            start: x.gain,
                        });
                        input_keys.push((i, k));
                        delays.push(DelayLine::new(delay_at(&plan.input_delays, i, k)));
```

Find:

```rust
                                .map(|x| Input {
                                    from: x.from,
                                    tap: x.tap,
                                    gain: x.gain,
                                });
```

Replace:

```rust
                                .map(|x| Input {
                                    from: x.from,
                                    tap: x.tap,
                                    gain: x.gain,
                                    start: x.gain,
                                });
```

Find:

```rust
                        let idx = inserts.len();
                        let state = match init(idx, ins) {
```

Replace:

```rust
                        let idx = inserts.len();
                        if let Some(id) = ins.instance {
                            insert_ids.push((id, idx));
                        }
                        let state = match init(idx, ins) {
```

Find:

```rust
                    defs.push(NodeDef::Strip {
                        inputs: in_start..inputs.len(),
                        inserts: ins_start..insert_defs.len(),
                        gain_l: s.gain_l,
                        gain_r: s.gain_r,
                        audible: s.audible,
                        meter: s.meter.unwrap_or(i),
                    });
```

Replace:

```rust
                    defs.push(NodeDef::Strip {
                        inputs: in_start..inputs.len(),
                        inserts: ins_start..insert_defs.len(),
                        gain_l: s.gain_l,
                        gain_r: s.gain_r,
                        start_l: s.gain_l,
                        start_r: s.gain_r,
                        audible: s.audible,
                        meter: s.meter.unwrap_or(i),
                    });
```

Find:

```rust
            defs.clear();
            inputs.clear();
```

Replace:

```rust
            defs.clear();
            inputs.clear();
            input_keys.clear();
```

Find:

```rust
        Graph {
            defs: defs.into_boxed_slice(),
            inputs: inputs.into_boxed_slice(),
```

Replace:

```rust
        insert_ids.sort_by_key(|x| x.0);
        Graph {
            defs: defs.into_boxed_slice(),
            inputs: inputs.into_boxed_slice(),
            input_keys: input_keys.into_boxed_slice(),
            insert_ids: insert_ids.into_boxed_slice(),
            controls: Controls::default(),
            ramping: false,
```

Find:

```rust
    /// Bitmask of the targets that have an instrument node.
    pub fn instruments(&self) -> u64 {
        self.instruments
    }
```

Replace:

```rust
    /// Bitmask of the targets that have an instrument node.
    pub fn instruments(&self) -> u64 {
        self.instruments
    }

    /// Installs the mixer's controls (after `build`). Not RT.
    pub fn set_controls(&mut self, specs: &[StripControlSpec]) {
        let keys: BTreeMap<(usize, usize), usize> = self
            .input_keys
            .iter()
            .enumerate()
            .map(|(i, k)| (*k, i))
            .collect();
        self.controls = Controls::build(specs, |node, k| keys.get(&(node, k)).copied());
    }

    /// Automation's normalised value for a strip control; it takes effect
    /// at the next `commit_controls`. // RT
    pub fn automate_strip(&mut self, strip: StripId, p: StripParam, v: f64) {
        self.controls.set_strip(strip, p, v);
    }

    /// Automation's normalised level for send `index` of `strip`. // RT
    pub fn automate_send(&mut self, strip: StripId, index: u16, v: f64) {
        self.controls.set_send(strip, index, v);
    }

    /// Drops the overrides of the controls `driven` says no lane drives;
    /// the next commit writes their static values. // RT
    pub fn release_overrides(&mut self, driven: impl Fn(AutoDest) -> bool) {
        self.controls.release(driven);
    }

    /// Writes the effective control values into the strip and edge gains.
    /// With `ramp` the next run moves from the current gains to them (the
    /// renderer commits a run's start values without, then its end values
    /// with, Plan 4d Ruling 11); without, they apply at once. // RT
    pub fn commit_controls(&mut self, ramp: bool) {
        let defs = &mut self.defs;
        let inputs = &mut self.inputs;
        let wrote = self.controls.commit(
            |node, gl, gr, on| {
                if let Some(NodeDef::Strip {
                    gain_l,
                    gain_r,
                    start_l,
                    start_r,
                    audible,
                    ..
                }) = defs.get_mut(node)
                {
                    if ramp {
                        *start_l = *gain_l;
                        *start_r = *gain_r;
                    } else {
                        *start_l = gl;
                        *start_r = gr;
                    }
                    *gain_l = gl;
                    *gain_r = gr;
                    *audible = on;
                }
            },
            |i, g| {
                if let Some(x) = inputs.get_mut(i) {
                    x.start = if ramp { x.gain } else { g };
                    x.gain = g;
                }
            },
        );
        if wrote && ramp {
            self.ramping = true;
        }
    }

    /// After a run: ramps have arrived. // RT
    pub fn settle(&mut self) {
        if !std::mem::take(&mut self.ramping) {
            return;
        }
        for d in self.defs.iter_mut() {
            if let NodeDef::Strip {
                gain_l,
                gain_r,
                start_l,
                start_r,
                ..
            } = d
            {
                *start_l = *gain_l;
                *start_r = *gain_r;
            }
        }
        for x in self.inputs.iter_mut() {
            x.start = x.gain;
        }
    }

    /// The processor of insert instance `id`, between runs (plugin
    /// automation). // RT
    pub fn insert_mut(&mut self, id: PluginInstanceId) -> Option<&mut (dyn EffectNode + 'static)> {
        let i = self.insert_ids.binary_search_by_key(&id, |x| x.0).ok()?;
        let idx = self.insert_ids.get(i)?.1;
        self.inserts
            .get_mut(idx)?
            .get_mut()
            .as_mut()?
            .node
            .as_deref_mut()
    }

    /// A strip node's gains and audibility at the end of the run
    /// (diagnostics and tests). Not RT.
    pub fn gains(&self, node: usize) -> Option<(f32, f32, bool)> {
        match self.defs.get(node) {
            Some(NodeDef::Strip {
                gain_l,
                gain_r,
                audible,
                ..
            }) => Some((*gain_l, *gain_r, *audible)),
            _ => None,
        }
    }

    /// The gains of node `node`'s input edges, in spec order (dropped edges
    /// left out; diagnostics and tests). Not RT.
    pub fn edge_gains(&self, node: usize) -> Vec<f32> {
        self.input_keys
            .iter()
            .zip(self.inputs.iter())
            .filter(|(k, _)| k.0 == node)
            .map(|(_, x)| x.gain)
            .collect()
    }
```

Find:

```rust
        if self.shape == old.shape
            && self.max_frames == old.max_frames
            && self.nodes.len() == old.nodes.len()
        {
            std::mem::swap(&mut self.nodes, &mut old.nodes);
        }
    }
```

Replace:

```rust
        if self.shape == old.shape
            && self.max_frames == old.max_frames
            && self.nodes.len() == old.nodes.len()
        {
            std::mem::swap(&mut self.nodes, &mut old.nodes);
        }
        // Automation overrides carry over while their static values stay
        // (Plan 4d Ruling 13).
        if self.controls.adopt(&old.controls) {
            self.commit_controls(false);
        }
    }
```

Find:

```rust
        NodeDef::Strip {
            inputs,
            inserts,
            gain_l,
            gain_r,
            audible,
            meter,
        } => {
```

Replace:

```rust
        NodeDef::Strip {
            inputs,
            inserts,
            gain_l,
            gain_r,
            start_l,
            start_r,
            audible,
            meter,
        } => {
```

Find:

```rust
                    Some(d) if d.delay() > 0 => {
                        dsp::copy(&mut st.scratch, buf, frames);
                        d.process_buf(&mut st.scratch, frames);
                        dsp::mix_into(&mut st.pre, &st.scratch, inp.gain, frames);
                    }
                    _ => dsp::mix_into(&mut st.pre, buf, inp.gain, frames),
```

Replace:

```rust
                    Some(d) if d.delay() > 0 => {
                        dsp::copy(&mut st.scratch, buf, frames);
                        d.process_buf(&mut st.scratch, frames);
                        dsp::mix_into_ramp(&mut st.pre, &st.scratch, inp.start, inp.gain, frames);
                    }
                    _ => dsp::mix_into_ramp(&mut st.pre, buf, inp.start, inp.gain, frames),
```

Find:

```rust
            dsp::scale(&mut st.post, *gain_l, *gain_r, frames);
```

Replace:

```rust
            dsp::scale_ramp(&mut st.post, (*start_l, *start_r), (*gain_l, *gain_r), frames);
```

Find:

```rust
    /// Hands the workers a new real-time environment.
    pub fn set_env(&self, env: RtEnv) {
```

Replace:

```rust
    /// The graph, between blocks (tests drive automation through it).
    pub fn graph_mut(&mut self) -> &mut Graph {
        self.exec.wait_idle();
        &mut self.graph
    }

    /// Hands the workers a new real-time environment.
    pub fn set_env(&self, env: RtEnv) {
```

In `crates/core-engine/src/engine.rs`, Find:

```rust
        let mut next_live: BTreeMap<PluginInstanceId, Live> = BTreeMap::new();
        let graph = Graph::build(&mg.spec, &p, max, |idx, ins| {
```

Replace:

```rust
        let mut next_live: BTreeMap<PluginInstanceId, Live> = BTreeMap::new();
        let mut graph = Graph::build(&mg.spec, &p, max, |idx, ins| {
```

Find:

```rust
        report.latency_frames = graph.latency();
        self.push(EngineCmd::SetGraph(Box::new(graph)))?;
```

Replace:

```rust
        graph.set_controls(&mg.controls);
        report.latency_frames = graph.latency();
        self.push(EngineCmd::SetGraph(Box::new(graph)))?;
```

`EffectNode` is already imported in `exec.rs` (`use crate::graph::node::{EffectNode, ProcessStatus};`).

- [ ] **Step 7: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-engine 2>&1 | grep -E "^test result|FAILED|panicked"
```

Expected: PASS — including `graph_mixer`, `graph_exec`, `graph_parallel` and `mixer_engine` unchanged (a graph without overrides renders exactly as before).

- [ ] **Step 8: Gate and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace 2>&1 | grep -E "^test result|FAILED|panicked" | tail -60
git add crates/core-engine/src/graph/controls.rs crates/core-engine/src/graph/mod.rs crates/core-engine/src/graph/exec.rs crates/core-engine/src/graph/mixer.rs crates/core-engine/src/graph/dsp.rs crates/core-engine/src/engine.rs crates/core-engine/tests/graph_controls.rs
git commit -m "feat(engine): automatable strip, send, mute and solo controls with ramping gains

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
```

Expected: 709 Rust tests listed (705 + 4).

---

### Task 8: Engine — runs split at automation points; automation drives controls and plugin parameters

**Files:**
- Modify: `crates/core-engine/src/render.rs`, `crates/core-engine/src/transport.rs`, `crates/core-engine/src/graph/exec.rs`, `crates/core-engine/src/graph/node.rs`, `crates/core-engine/src/backend.rs`
- Test: `crates/core-engine/tests/automation.rs`, `crates/core-engine/tests/rt_alloc.rs`

**Interfaces:**
- Consumes: Task 6's `Automation::{value, next_cut, drives, lanes}`, `AutoDest`, `MIN_RUN`, `MAX_AUTO_LANES`, `AutoLane::{stepped, instrument}`; Task 7's `Graph::{automate_strip, automate_send, commit_controls, settle, release_overrides, insert_mut}`.
- Produces: `EffectNode::automate(&mut self, frame: u32, param: ParamId, normalized: f64)` and `InstrumentNode::automate(…)` (default: nothing; `// RT`); `Transport::{play_pos(&self, length: u64) -> Option<u64>, boundary(&self, length: u64) -> Option<u64>}`; `Counters.runs: u64` (graph runs); renderer behaviour of Rulings 10–13: runs, ramps, plugin parameter changes, overrides dropped on Play and on a new schedule while playing.

Rulings 10–13. `Out` gains `base` (the run's block offset): instrument sinks get offsets relative to their run, MIDI outputs keep block-relative timestamps. The renderer touches the graph and the sinks between runs only after `Exec::try_finish` (a straggler of the previous run may still be leaving the job).

- [ ] **Step 1: Write the failing tests**

Create `crates/core-engine/tests/automation.rs`:

```rust
//! Automation playback (editors spec §4d, §6; Plan 4d Rulings 10–13):
//! renders against the tone sink, compared with the laws.

use core_engine::backend::AudioBackend;
use core_engine::engine::{Engine, EngineConfig};
use core_engine::fixtures::{Builder, Q};
use core_engine::offline::{OfflineBackend, OfflineSinks};
use core_model::automation::{AutomationTarget, fader_db, fader_norm, pan_norm};
use core_model::command::MixerCmd;
use core_model::document::Document;
use core_model::event::{EventBody, EventSpec};
use core_model::ids::{DestId, SeqId, StripId};
use core_model::mixer::{StripKind, db_to_gain};
use core_model::time::Tick;

const SR: f64 = 48_000.0;
const BAR: i64 = 4 * Q;

struct Song {
    doc: Document,
    seq: SeqId,
    synth: StripId,
    dest: DestId,
}

/// The built-in synth holding C4 for 16 bars in "S", plus a lane "Auto"
/// with `points(the synth's strip)`. Every song has the same IDs.
fn song(points: impl Fn(StripId) -> Vec<(i64, AutomationTarget, f64)>) -> Song {
    let mut b = Builder::new();
    let dest = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, notes) = b.sequence("S", "Notes", Some(dest)).unwrap();
    b.notes(seq, notes, &[(0, 60, 0xFFFF, 16 * BAR)]).unwrap();
    let synth = b.strip_of(StripKind::BuiltinSynth).unwrap();
    let auto = b.lane(seq, "Auto", None).unwrap();
    let specs: Vec<EventSpec> = points(synth)
        .into_iter()
        .map(|(t, target, value)| EventSpec::new(Tick(t), EventBody::Automation { target, value }))
        .collect();
    if !specs.is_empty() {
        b.events(seq, auto, specs).unwrap();
    }
    Song {
        doc: b.finish(),
        seq,
        synth,
        dest,
    }
}

struct Rig {
    engine: Engine,
    be: OfflineBackend,
    block: u32,
}

fn rig(doc: &Document, seq: SeqId, block: u32) -> Rig {
    let (mut engine, r) = Engine::new(
        EngineConfig::new(SR),
        Box::new(OfflineSinks::new(1 << 16).with_tone()),
    )
    .unwrap();
    engine.load(doc, seq).unwrap();
    let mut be = OfflineBackend::new(SR, block);
    be.start(r).unwrap();
    be.reserve(block);
    Rig { engine, be, block }
}

impl Rig {
    /// The left channel of the next `frames` frames.
    fn render(&mut self, frames: usize) -> Vec<f32> {
        let mut out = Vec::with_capacity(frames + self.block as usize);
        while out.len() < frames {
            self.be.block(self.block);
            out.extend_from_slice(self.be.output(self.block).0);
        }
        out.truncate(frames);
        out
    }
}

fn played(s: &Song, frames: usize) -> Vec<f32> {
    let mut r = rig(&s.doc, s.seq, 256);
    r.engine.play().unwrap();
    r.render(frames)
}

/// `got` is `want` times `gain`, sample by sample.
fn scaled(got: &[f32], want: &[f32], gain: f32) {
    assert!(want.iter().any(|x| x.abs() > 1e-3), "the tone sounds");
    for (i, (g, w)) in got.iter().zip(want).enumerate() {
        assert!((g - w * gain).abs() <= 1e-6, "frame {i}: {g} vs {}", w * gain);
    }
}

#[test]
fn a_fader_ramp_follows_the_lane_between_points() {
    let unity = fader_norm(0.0);
    let dry = played(&song(|_| vec![]), 120_000);
    let wet = played(
        &song(|s| {
            vec![
                (0, AutomationTarget::StripFader(s), 0.0),
                (BAR, AutomationTarget::StripFader(s), unity),
            ]
        }),
        120_000,
    );
    assert!(dry.iter().any(|x| x.abs() > 1e-3));
    // A bar at 120 BPM is 96 000 frames.
    for (i, (w, d)) in wet.iter().zip(&dry).enumerate() {
        let v = unity * (i as f64 / 96_000.0).min(1.0);
        let g = db_to_gain(fader_db(v)) as f32;
        assert!((w - d * g).abs() <= 1e-6, "frame {i}: {w} vs {}", d * g);
    }
}

#[test]
fn a_mute_point_switches_at_its_frame_inside_a_block() {
    let dry = played(&song(|_| vec![]), 60_000);
    let wet = played(
        &song(|s| {
            vec![
                (0, AutomationTarget::StripMute(s), 0.0),
                (Q, AutomationTarget::StripMute(s), 1.0),
                (2 * Q, AutomationTarget::StripMute(s), 0.0),
            ]
        }),
        60_000,
    );
    // A quarter is 24 000 frames; neither 24 000 nor 48 000 is on a block
    // boundary of 256 for the second, so the block is split there.
    for (i, (w, d)) in wet.iter().zip(&dry).enumerate() {
        if (24_000..48_000).contains(&i) {
            assert_eq!(*w, 0.0, "frame {i} is muted");
        } else {
            assert_eq!(w, d, "frame {i} plays as without automation");
        }
    }
}

#[test]
fn dense_points_never_split_runs_below_32_frames() {
    // A pan point every 8 ticks (12.5 frames) for two beats.
    let dense = song(|s| {
        (0..3840)
            .map(|k| (k * 8, AutomationTarget::StripPan(s), (k % 2) as f64))
            .collect()
    });
    let mut r = rig(&dense.doc, dense.seq, 512);
    r.engine.play().unwrap();
    r.render(100 * 512);
    let runs = r.engine.counters().runs;
    assert!(runs > 100, "the points split blocks ({runs} runs)");
    assert!(runs <= 100 * 512 / 32 + 100, "{runs} runs: one shorter than 32 frames");
}

/// Plays for 10 blocks, stops, runs `then`, and returns one block of an
/// auditioned note (left channel).
fn held_after_stop(s: &Song, then: impl FnOnce(&mut Engine)) -> Vec<f32> {
    let mut r = rig(&s.doc, s.seq, 256);
    r.engine.play().unwrap();
    r.render(2_560);
    r.engine.stop().unwrap();
    r.render(256);
    then(&mut r.engine);
    r.render(256);
    r.engine.audition(&s.doc, s.dest, 64, 0xFFFF, true).unwrap();
    r.render(256)
}

#[test]
fn stopping_keeps_the_last_automated_value_until_the_fader_moves() {
    let v = fader_norm(-12.0);
    let wet = song(|s| vec![(0, AutomationTarget::StripFader(s), v)]);
    let dry = song(|_| vec![]);
    let base = held_after_stop(&dry, |_| {});
    let g = db_to_gain(fader_db(v)) as f32;
    scaled(&held_after_stop(&wet, |_| {}), &base, g);
    // An edit that leaves the fader alone keeps the automated value.
    let same = wet.doc.clone();
    scaled(
        &held_after_stop(&wet, |e| {
            e.load(&same, wet.seq).unwrap();
        }),
        &base,
        g,
    );
    // The user moves the fader: the static value plays.
    let mut b = Builder::from_doc(wet.doc.clone());
    b.mixer(MixerCmd::SetGain {
        strip: wet.synth,
        gain_db: -3.0,
    })
    .unwrap();
    let moved = b.finish();
    scaled(
        &held_after_stop(&wet, |e| {
            e.load(&moved, wet.seq).unwrap();
        }),
        &base,
        db_to_gain(-3.0) as f32,
    );
}

#[test]
fn while_playing_automation_wins_over_a_fader_move() {
    let v = fader_norm(-12.0);
    let wet = song(|s| vec![(0, AutomationTarget::StripFader(s), v)]);
    let mut b = Builder::from_doc(wet.doc.clone());
    b.mixer(MixerCmd::SetGain {
        strip: wet.synth,
        gain_db: -3.0,
    })
    .unwrap();
    let moved = b.finish();
    // A recompile releases the played note (every edit does), so an
    // auditioned note carries the sound after the fader move.
    let steps = |s: &Song, load: &Document| {
        let mut r = rig(&s.doc, s.seq, 256);
        r.engine.play().unwrap();
        r.render(2_560);
        r.engine.load(load, s.seq).unwrap();
        r.render(256);
        r.engine.audition(&s.doc, s.dest, 64, 0xFFFF, true).unwrap();
        r.render(2_560)
    };
    let dry = song(|_| vec![]);
    let base = steps(&dry, &dry.doc);
    scaled(&steps(&wet, &moved), &base, db_to_gain(fader_db(v)) as f32);
}

#[test]
fn play_after_the_automation_is_gone_restores_the_static_fader() {
    let wet = song(|s| vec![(0, AutomationTarget::StripFader(s), fader_norm(-12.0))]);
    let dry = song(|_| vec![]);
    let steps = |s: &Song| {
        let mut r = rig(&s.doc, s.seq, 256);
        r.engine.play().unwrap();
        r.render(2_560);
        r.engine.stop().unwrap();
        r.render(256);
        // The automation is deleted (the dry song is the same document
        // without its points).
        r.engine.load(&dry.doc, dry.seq).unwrap();
        r.render(256);
        r.engine.locate_frame(0).unwrap();
        r.engine.play().unwrap();
        r.render(2_560)
    };
    assert_eq!(steps(&wet), steps(&dry));
}

#[test]
fn automation_of_a_deleted_strip_plays_like_none() {
    let gone = song(|_| {
        vec![
            (0, AutomationTarget::StripFader(StripId(999)), 0.0),
            (0, AutomationTarget::StripMute(StripId(999)), 1.0),
            (0, AutomationTarget::StripPan(StripId(999)), pan_norm(1.0)),
        ]
    });
    let (mut engine, _r) = Engine::new(EngineConfig::new(SR), Box::new(OfflineSinks::new(64))).unwrap();
    let report = engine.load(&gone.doc, gone.seq).unwrap();
    assert_eq!(report.automation_missing, 3);
    assert_eq!(played(&gone, 20_000), played(&song(|_| vec![]), 20_000));
}
```

In `crates/core-engine/tests/rt_alloc.rs`, Find:

```rust
use core_engine::fixtures::demo_document;
```

Replace:

```rust
use core_engine::fixtures::{Builder, demo_document};
use core_model::automation::AutomationTarget;
use core_model::document::Document;
use core_model::event::{EventBody, EventSpec};
use core_model::ids::SeqId;
use core_model::mixer::StripKind;
```

Find:

```rust
/// One test, so no other test thread allocates while the global counter
/// is compared.
#[test]
fn render_path_never_allocates() {
```

Replace:

```rust
/// The demo with a lane "Auto" in `song`: a fader ramp every 64 ticks over
/// eight bars, a mute toggled every beat and a pan point every 8 ticks for
/// a bar (runs of 32 frames).
fn automation_document(doc: &Document, song: SeqId) -> Document {
    let mut b = Builder::from_doc(doc.clone());
    let synth = b.strip_of(StripKind::BuiltinSynth).unwrap();
    let lane = b.lane(song, "Auto", None).unwrap();
    let q = 15_360i64;
    let mut events = Vec::new();
    for k in 0..(32 * q / 64) {
        events.push(EventSpec::new(
            Tick(k * 64),
            EventBody::Automation {
                target: AutomationTarget::StripFader(synth),
                value: (k % 100) as f64 / 100.0,
            },
        ));
    }
    for k in 0..32 {
        events.push(EventSpec::new(
            Tick(k * q),
            EventBody::Automation {
                target: AutomationTarget::StripMute(synth),
                value: (k % 2) as f64,
            },
        ));
    }
    for k in 0..(4 * q / 8) {
        events.push(EventSpec::new(
            Tick(k * 8),
            EventBody::Automation {
                target: AutomationTarget::StripPan(synth),
                value: (k % 3) as f64 / 2.0,
            },
        ));
    }
    b.events(song, lane, events).unwrap();
    b.finish()
}

/// One test, so no other test thread allocates while the global counter
/// is compared.
#[test]
fn render_path_never_allocates() {
```

Find:

```rust
    assert!(engine.counters().midi_dropped > 0);
    assert_eq!(
        rt_check::count() - before,
        0,
        "allocations with a full MIDI queue"
    );
}
```

Replace:

```rust
    assert!(engine.counters().midi_dropped > 0);
    assert_eq!(
        rt_check::count() - before,
        0,
        "allocations with a full MIDI queue"
    );
    drop(b.stop().unwrap());
    drop(engine);

    // Automation: runs split at points, ramps, mutes, a stop, a locate and a
    // recompile while playing (Plan 4d Rulings 10–13).
    let auto = automation_document(&doc, song);
    let (mut engine, renderer) = Engine::new(
        EngineConfig::new(48_000.0),
        Box::new(OfflineSinks::new(1 << 20).with_tone()),
    )
    .unwrap();
    engine.load(&auto, song).unwrap();
    let mut b = OfflineBackend::new(48_000.0, 512);
    b.start(renderer).unwrap();
    b.reserve(4096);
    engine.set_loop(true).unwrap();
    engine.play().unwrap();
    for i in 0..3_000u32 {
        match i {
            1_000 => engine.stop().unwrap(),
            1_010 => engine.play().unwrap(),
            1_500 => {
                engine.load(&auto, song).unwrap();
            }
            2_000 => engine.locate_tick(Tick(15_360 * 8)).unwrap(),
            _ => {}
        }
        armed_block(&mut b, 1 + (i * 37) % 1024);
    }
    assert!(engine.counters().runs > 3_000, "automation split blocks");
    assert_eq!(
        rt_check::count() - before,
        0,
        "allocations with automation"
    );
}
```

- [ ] **Step 2: Run them to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-engine --test automation 2>&1 | tail -8
```

Expected: compile error (`no field runs on Counters`); with that field stubbed, the ramp and mute tests fail (automation is not applied yet).

- [ ] **Step 3: Add the automate hooks and the transport's position**

In `crates/core-engine/src/graph/node.rs`, Find:

```rust
    /// [`EffectNode::process`] with the block's transport facts; plugin
    /// processors override this. // RT
```

Replace:

```rust
    /// Automation's value for parameter `param`, normalised 0–1, at `frame`
    /// of the next block; plugin processors map it to the parameter's range
    /// (Plan 4d Ruling 12). // RT
    fn automate(&mut self, frame: u32, param: ParamId, normalized: f64) {
        let _ = (frame, param, normalized);
    }

    /// [`EffectNode::process`] with the block's transport facts; plugin
    /// processors override this. // RT
```

In `crates/core-engine/src/backend.rs`, Find:

```rust
    /// The plugin instance this node has just bypassed for faults (spec
    /// §7), once; the renderer reports it under that instance. Nodes that
    /// are not plugins never fault. // RT
    fn take_fault(&mut self) -> Option<core_model::ids::PluginInstanceId> {
        None
    }
```

Replace:

```rust
    /// The plugin instance this node has just bypassed for faults (spec
    /// §7), once; the renderer reports it under that instance. Nodes that
    /// are not plugins never fault. // RT
    fn take_fault(&mut self) -> Option<core_model::ids::PluginInstanceId> {
        None
    }

    /// Automation's value for parameter `param`, normalised 0–1, at `frame`
    /// of the next render; plugin instruments map it to the parameter's
    /// range (Plan 4d Ruling 12). // RT
    fn automate(&mut self, frame: u32, param: core_model::mixer::ParamId, normalized: f64) {
        let _ = (frame, param, normalized);
    }
```

In `crates/core-engine/src/transport.rs`, Find:

```rust
    /// True once, after the tail finished. // RT
    pub fn take_ended(&mut self) -> bool {
```

Replace:

```rust
    /// The schedule frame the next played frame comes from (after a loop
    /// wrap that is due), or `None` unless playing before the end. // RT
    pub fn play_pos(&self, length: u64) -> Option<u64> {
        if self.state != PlayState::Playing {
            return None;
        }
        let lp = self.active_loop();
        let pos = match lp {
            Some((s, e)) if self.pos == e => s,
            _ => self.pos,
        };
        let end = match lp {
            Some((_, e)) if pos < e => e,
            _ => length,
        };
        (pos < end).then_some(pos)
    }

    /// Where playing from [`Transport::play_pos`] stops or wraps: the loop
    /// end or the sequence end. // RT
    pub fn boundary(&self, length: u64) -> Option<u64> {
        let pos = self.play_pos(length)?;
        Some(match self.active_loop() {
            Some((_, e)) if pos < e => e,
            _ => length,
        })
    }

    /// True once, after the tail finished. // RT
    pub fn take_ended(&mut self) -> bool {
```

In `crates/core-engine/src/graph/exec.rs`, Find:

```rust
    /// Writes master's output (silence when it is muted) into channels 0–1
    /// of `out`, which the caller zeroed. Call only after a completed run.
    /// // RT
    pub(crate) fn write_master(&self, out: &mut [&mut [f32]], frames: u32) {
```

Replace:

```rust
    /// Writes master's output (silence when it is muted) into frames
    /// `offset..offset + frames` of channels 0–1 of `out`, which the caller
    /// zeroed. Call only after a completed run. // RT
    pub(crate) fn write_master(&self, out: &mut [&mut [f32]], offset: usize, frames: u32) {
```

Find:

```rust
        for (ch, src) in out.iter_mut().zip([&st.post.left, &st.post.right]) {
            if let (Some(d), Some(s)) = (ch.get_mut(..n), src.get(..n)) {
                d.copy_from_slice(s);
            }
        }
    }
}
```

Replace:

```rust
        for (ch, src) in out.iter_mut().zip([&st.post.left, &st.post.right]) {
            if let (Some(d), Some(s)) = (ch.get_mut(offset..offset + n), src.get(..n)) {
                d.copy_from_slice(s);
            }
        }
    }
}
```

Find:

```rust
            RunResult::Done => {
                self.graph.write_master(&mut [left, right], frames);
                true
            }
```

Replace:

```rust
            RunResult::Done => {
                self.graph.write_master(&mut [left, right], 0, frames);
                true
            }
```

- [ ] **Step 4: Split blocks into runs and apply automation**

In `crates/core-engine/src/render.rs`, Find:

```rust
use crate::schedule::{MAX_TARGETS, Schedule, TargetIdx};
use crate::transport::{PlayState, Step, Transport};
use core_model::ids::PluginInstanceId;
```

Replace:

```rust
use crate::schedule::{AutoDest, MAX_AUTO_LANES, MAX_TARGETS, MIN_RUN, Schedule, TargetIdx};
use crate::transport::{PlayState, Step, Transport};
use core_model::ids::PluginInstanceId;
use core_model::mixer::ParamId;
```

Find:

```rust
    /// Inserts bypassed for faults.
    pub plugin_faults: u64,
}
```

Replace:

```rust
    /// Inserts bypassed for faults.
    pub plugin_faults: u64,
    /// Graph runs: one per block unless automation splits it (Plan 4d
    /// Ruling 10).
    pub runs: u64,
}
```

Find:

```rust
    plugin_faults: AtomicU64,
    load_last: AtomicU32,
```

Replace:

```rust
    plugin_faults: AtomicU64,
    runs: AtomicU64,
    load_last: AtomicU32,
```

Find:

```rust
            plugin_faults: AtomicU64::new(0),
            load_last: AtomicU32::new(0),
```

Replace:

```rust
            plugin_faults: AtomicU64::new(0),
            runs: AtomicU64::new(0),
            load_last: AtomicU32::new(0),
```

Find:

```rust
            plugin_faults: self.plugin_faults.load(Ordering::Relaxed),
        }
    }
```

Replace:

```rust
            plugin_faults: self.plugin_faults.load(Ordering::Relaxed),
            runs: self.runs.load(Ordering::Relaxed),
        }
    }
```

Find:

```rust
    /// Frames of busy blocks the sinks have not been told about.
    pending_skip: u64,
}
```

Replace:

```rust
    /// Frames of busy blocks the sinks have not been told about.
    pending_skip: u64,
    /// Per automation lane, the plugin value last sent (NaN: none yet).
    auto_last: Box<[f64; MAX_AUTO_LANES]>,
}
```

Find:

```rust
    /// MIDI offset plus the graph latency, in ns.
    late_ns: i64,
}

impl Out<'_> {
    /// Sends `bytes` to `target` at block offset `offset`. // RT
    fn send(&mut self, target: TargetIdx, offset: u32, bytes: &[u8]) {
        match self.sinks.get_mut(usize::from(target.0)) {
            Some(Some(Sink::Instrument(node))) => node.midi(offset, bytes),
```

Replace:

```rust
    /// MIDI offset plus the graph latency, in ns.
    late_ns: i64,
    /// Block offset of the run being rendered: instrument sinks get offsets
    /// relative to it (Plan 4d Ruling 10).
    base: u32,
}

impl Out<'_> {
    /// Sends `bytes` to `target` at block offset `offset`. // RT
    fn send(&mut self, target: TargetIdx, offset: u32, bytes: &[u8]) {
        match self.sinks.get_mut(usize::from(target.0)) {
            Some(Some(Sink::Instrument(node))) => node.midi(offset.saturating_sub(self.base), bytes),
```

Find:

```rust
/// Hard clamp of channels 0–1 at ±1.0; non-finite samples become 0. Raises
/// `CLIPPED` when anything was changed (engine ruling). // RT
fn clamp_master(out: &mut [&mut [f32]], frames: u32, shared: &Shared) {
    let n = frames as usize;
    let mut clipped = false;
    for ch in out.iter_mut().take(2) {
        if let Some(c) = ch.get_mut(..n) {
```

Replace:

```rust
/// Hard clamp of frames `offset..offset + frames` of channels 0–1 at ±1.0;
/// non-finite samples become 0. Raises `CLIPPED` when anything was changed
/// (engine ruling). // RT
fn clamp_master(out: &mut [&mut [f32]], offset: usize, frames: u32, shared: &Shared) {
    let n = frames as usize;
    let mut clipped = false;
    for ch in out.iter_mut().take(2) {
        if let Some(c) = ch.get_mut(offset..offset + n) {
```

Find:

```rust
            latency: 0,
            pending_skip: 0,
        };
        r.prepare(cfg.max_frames);
```

Replace:

```rust
            latency: 0,
            pending_skip: 0,
            auto_last: Box::new([f64::NAN; MAX_AUTO_LANES]),
        };
        r.prepare(cfg.max_frames);
```

Find:

```rust
        self.block_ns = time.host_time_ns;
        self.drain_commands();
        let info = self.block_info();
        let length = self.schedule.as_ref().map_or(0, |s| s.length_frames);
        let late_ns = self.cfg.midi_offset_ns + self.latency_ns();
        let mut o = Out {
            sinks: &mut self.sinks,
            shared: &self.shared,
            block_ns: time.host_time_ns,
            ns_per_frame: 1e9 / self.cfg.sample_rate.max(1.0),
            late_ns,
        };
        let mut offset = 0u32;
        while offset < frames {
            let step = self.transport.next(length, offset, frames - offset);
            match step {
                Step::Play { from, to, offset } => {
                    if let Some(s) = self.schedule.as_deref() {
                        if std::mem::take(&mut self.chase_pending) {
                            s.chase
                                .state_before(from, |t, m| o.send(t, offset, m.as_slice()));
                        }
                        while let Some(e) = s.entries.get(self.cursor) {
                            if e.frame >= to {
                                break;
                            }
                            let off =
                                offset + u32::try_from(e.frame.saturating_sub(from)).unwrap_or(0);
                            let bytes = s.bytes(&e.msg);
                            o.send(e.target, off, bytes);
                            self.notes.observe(e.target, bytes);
                            self.cursor += 1;
                        }
                    }
                }
                Step::Wrap { offset, to } => {
                    release_all(&mut self.notes, &mut o, offset);
                    if let Some(s) = self.schedule.as_deref() {
                        reset_controllers(s, &mut o, offset);
                    }
                    self.chase_pending = true;
                    self.cursor = self.schedule.as_ref().map_or(0, |s| s.cursor_at(to));
                }
                Step::End { offset } => release_all(&mut self.notes, &mut o, offset),
                Step::Idle { .. } => {}
            }
            offset += step.frames();
        }
        let missed = self.render_graph(out, frames, info, deadline);
        self.publish();
```

Replace:

```rust
        self.block_ns = time.host_time_ns;
        self.drain_commands();
        let length = self.schedule.as_ref().map_or(0, |s| s.length_frames);
        let late_ns = self.cfg.midi_offset_ns + self.latency_ns();
        let ns_per_frame = 1e9 / self.cfg.sample_rate.max(1.0);
        let mut missed = false;
        let mut offset = 0u32;
        while offset < frames {
            // Between runs no worker may still hold the graph or the sinks
            // (the first run was checked above).
            if offset > 0 && !self.exec.try_finish(deadline) {
                self.shared.count_deadline_miss();
                missed = true;
                break;
            }
            let run = self.next_run(length, frames - offset);
            let end = offset + run;
            self.automate(length, run);
            let info = self.block_info();
            let mut o = Out {
                sinks: &mut self.sinks,
                shared: &self.shared,
                block_ns: time.host_time_ns,
                ns_per_frame,
                late_ns,
                base: offset,
            };
            let mut at = offset;
            while at < end {
                let step = self.transport.next(length, at, end - at);
                match step {
                    Step::Play { from, to, offset } => {
                        if let Some(s) = self.schedule.as_deref() {
                            if std::mem::take(&mut self.chase_pending) {
                                s.chase
                                    .state_before(from, |t, m| o.send(t, offset, m.as_slice()));
                            }
                            while let Some(e) = s.entries.get(self.cursor) {
                                if e.frame >= to {
                                    break;
                                }
                                let off = offset
                                    + u32::try_from(e.frame.saturating_sub(from)).unwrap_or(0);
                                let bytes = s.bytes(&e.msg);
                                o.send(e.target, off, bytes);
                                self.notes.observe(e.target, bytes);
                                self.cursor += 1;
                            }
                        }
                    }
                    Step::Wrap { offset, to } => {
                        release_all(&mut self.notes, &mut o, offset);
                        if let Some(s) = self.schedule.as_deref() {
                            reset_controllers(s, &mut o, offset);
                        }
                        self.chase_pending = true;
                        self.cursor = self.schedule.as_ref().map_or(0, |s| s.cursor_at(to));
                    }
                    Step::End { offset } => release_all(&mut self.notes, &mut o, offset),
                    Step::Idle { .. } => {}
                }
                at += step.frames();
            }
            if self.render_graph(out, offset, run, info, deadline) {
                missed = true;
                break;
            }
            if let Some(g) = self.graph.as_deref_mut() {
                g.settle();
            }
            offset = end;
        }
        self.publish();
```

Find:

```rust
    /// Renders the graph (or, before the first graph arrives, sums the
    /// instrument sinks) into channels 0–1 and clamps; true when the graph
    /// missed its deadline. // RT
    fn render_graph(
        &mut self,
        out: &mut [&mut [f32]],
        frames: u32,
        info: BlockInfo,
        deadline: Option<Instant>,
    ) -> bool {
        let Some(g) = self.graph.as_deref() else {
            self.mix_direct(out, frames);
            clamp_master(out, frames, &self.shared);
            return false;
        };
```

Replace:

```rust
    /// The next run's length (1 ..= `left`): the rest of the block, cut at
    /// the next automation point (never under `MIN_RUN` frames after the
    /// run's start) and at the loop end or the sequence end while a
    /// schedule with automation plays (Plan 4d Ruling 10). // RT
    fn next_run(&self, length: u64, left: u32) -> u32 {
        let Some(s) = self.schedule.as_deref() else {
            return left;
        };
        if s.automation.is_empty() {
            return left;
        }
        let Some(pos) = self.transport.play_pos(length) else {
            return left;
        };
        let mut run = u64::from(left);
        if let Some(c) = s.automation.next_cut(pos) {
            run = run.min((c - pos).max(u64::from(MIN_RUN)));
        }
        if let Some(b) = self.transport.boundary(length) {
            if b > pos {
                run = run.min(b - pos);
            }
        }
        u32::try_from(run).unwrap_or(left).clamp(1, left.max(1))
    }

    /// Applies automation to the next `run` frames while playing (Plan 4d
    /// Rulings 11, 12): strip and send controls are committed at the run's
    /// start values, then ramp to its end values; a plugin parameter gets
    /// its start value and, on a ramp, a value every `max(MIN_RUN, run /
    /// 16)` frames, each only when it changed. // RT
    fn automate(&mut self, length: u64, run: u32) {
        let Some(s) = self.schedule.as_deref() else {
            return;
        };
        let auto = &s.automation;
        if auto.is_empty() {
            return;
        }
        let Some(pos) = self.transport.play_pos(length) else {
            return;
        };
        let end = pos.saturating_add(u64::from(run));
        if let Some(g) = self.graph.as_deref_mut() {
            for ramp in [false, true] {
                let mut any = false;
                for lane in auto.lanes() {
                    let v = if ramp {
                        auto.value(lane, end, true)
                    } else {
                        auto.value(lane, pos, false)
                    };
                    match lane.dest {
                        AutoDest::Strip(id, p) => {
                            g.automate_strip(id, p, v);
                            any = true;
                        }
                        AutoDest::Send(id, i) => {
                            g.automate_send(id, i, v);
                            any = true;
                        }
                        AutoDest::Plugin(..) => {}
                    }
                }
                if any {
                    g.commit_controls(ramp);
                }
            }
        }
        let every = (run / 16).max(MIN_RUN);
        for (k, lane) in auto.lanes().iter().enumerate() {
            let AutoDest::Plugin(id, param) = lane.dest else {
                continue;
            };
            let Some(last) = self.auto_last.get_mut(k) else {
                break;
            };
            let mut f = 0u32;
            loop {
                let v = auto.value(lane, pos.saturating_add(u64::from(f)), false);
                if v.to_bits() != last.to_bits() {
                    *last = v;
                    send_param(
                        self.graph.as_deref_mut(),
                        &mut self.sinks,
                        lane.instrument,
                        id,
                        f,
                        param,
                        v,
                    );
                }
                if lane.stepped {
                    break;
                }
                f = f.saturating_add(every);
                if f >= run {
                    break;
                }
            }
        }
    }

    /// Drops the overrides of the controls the schedule no longer
    /// automates (Plan 4d Ruling 13). // RT
    fn forget_unautomated(&mut self) {
        let Some(g) = self.graph.as_deref_mut() else {
            return;
        };
        let auto = self.schedule.as_deref().map(|s| &s.automation);
        g.release_overrides(|d| auto.is_some_and(|a| a.drives(d)));
        g.commit_controls(false);
    }

    /// Renders the graph (or, before the first graph arrives, sums the
    /// instrument sinks) into frames `offset..offset + frames` of channels
    /// 0–1 and clamps; true when the graph missed its deadline. // RT
    fn render_graph(
        &mut self,
        out: &mut [&mut [f32]],
        offset: u32,
        frames: u32,
        info: BlockInfo,
        deadline: Option<Instant>,
    ) -> bool {
        let at = offset as usize;
        let Some(g) = self.graph.as_deref() else {
            self.mix_direct(out, at, frames);
            clamp_master(out, at, frames, &self.shared);
            return false;
        };
```

Find:

```rust
        match self
            .exec
            .run(g, &self.sinks, &self.shared, frames, info, deadline)
        {
            RunResult::Done => {
                g.write_master(out, frames);
                let covered = g.instruments();
                skip_sinks(&mut self.sinks, u64::from(frames), covered);
                clamp_master(out, frames, &self.shared);
                false
            }
```

Replace:

```rust
        self.shared.runs.fetch_add(1, Ordering::Relaxed);
        match self
            .exec
            .run(g, &self.sinks, &self.shared, frames, info, deadline)
        {
            RunResult::Done => {
                g.write_master(out, at, frames);
                let covered = g.instruments();
                skip_sinks(&mut self.sinks, u64::from(frames), covered);
                clamp_master(out, at, frames, &self.shared);
                false
            }
```

Find:

```rust
                self.cursor = new.cursor_at(self.transport.pos);
                self.schedule = Some(new);
            }
```

Replace:

```rust
                self.cursor = new.cursor_at(self.transport.pos);
                self.schedule = Some(new);
                self.auto_last.fill(f64::NAN);
                if self.transport.state == PlayState::Playing {
                    self.forget_unautomated();
                }
            }
```

Find:

```rust
                self.graph = Some(new);
                self.shared.graphs_applied.fetch_add(1, Ordering::Relaxed);
            }
            EngineCmd::Play => {
                let length = self.schedule.as_ref().map_or(0, |s| s.length_frames);
                self.transport.play(length);
                self.seek();
            }
```

Replace:

```rust
                self.graph = Some(new);
                self.shared.graphs_applied.fetch_add(1, Ordering::Relaxed);
                // New inserts need their automated values.
                self.auto_last.fill(f64::NAN);
            }
            EngineCmd::Play => {
                let length = self.schedule.as_ref().map_or(0, |s| s.length_frames);
                self.transport.play(length);
                self.seek();
                self.forget_unautomated();
                self.auto_last.fill(f64::NAN);
            }
```

Find:

```rust
            EngineCmd::Locate(frame) => {
                self.release_now(true);
                self.transport.locate(frame);
                self.seek();
            }
```

Replace:

```rust
            EngineCmd::Locate(frame) => {
                self.release_now(true);
                self.transport.locate(frame);
                self.seek();
                self.auto_last.fill(f64::NAN);
            }
```

In `release_now`, Find:

```rust
            ns_per_frame: 0.0,
            late_ns,
        };
        release_all(&mut self.notes, &mut o, 0);
```

Replace:

```rust
            ns_per_frame: 0.0,
            late_ns,
            base: 0,
        };
        release_all(&mut self.notes, &mut o, 0);
```

In the `EngineCmd::Audition` arm, Find:

```rust
                    ns_per_frame: 0.0,
                    late_ns,
                };
                o.send(target, 0, msg.as_slice());
```

Replace:

```rust
                    ns_per_frame: 0.0,
                    late_ns,
                    base: 0,
                };
                o.send(target, 0, msg.as_slice());
```

Find:

```rust
    /// Before the first graph: renders instrument sinks and sums them into
    /// channels 0–1 (2b behaviour). // RT
    fn mix_direct(&mut self, out: &mut [&mut [f32]], frames: u32) {
        let n = frames as usize;
        for (sink, buf) in self.sinks.iter_mut().zip(self.bufs.iter_mut()) {
            let Some(Sink::Instrument(node)) = sink else {
                continue;
            };
            node.render(frames, buf);
            for (i, src) in [&buf.left, &buf.right].into_iter().enumerate() {
                let (Some(dst), Some(src)) =
                    (out.get_mut(i).and_then(|c| c.get_mut(..n)), src.get(..n))
                else {
```

Replace:

```rust
    /// Before the first graph: renders instrument sinks and sums them into
    /// frames `at..at + frames` of channels 0–1 (2b behaviour). // RT
    fn mix_direct(&mut self, out: &mut [&mut [f32]], at: usize, frames: u32) {
        let n = frames as usize;
        for (sink, buf) in self.sinks.iter_mut().zip(self.bufs.iter_mut()) {
            let Some(Sink::Instrument(node)) = sink else {
                continue;
            };
            node.render(frames, buf);
            for (i, src) in [&buf.left, &buf.right].into_iter().enumerate() {
                let (Some(dst), Some(src)) = (
                    out.get_mut(i).and_then(|c| c.get_mut(at..at + n)),
                    src.get(..n),
                ) else {
```

Find:

```rust
/// Note-offs (and pedal-ups) for every target at block offset `offset`. // RT
fn release_all(notes: &mut ActiveNotes, o: &mut Out<'_>, offset: u32) {
```

Replace:

```rust
/// Sends a plugin parameter's automated value: to its plugin
/// instrument's sink when it has one, else to its insert in the graph.
/// // RT
fn send_param(
    graph: Option<&mut Graph>,
    sinks: &mut SinkTable,
    instrument: Option<TargetIdx>,
    id: PluginInstanceId,
    frame: u32,
    param: ParamId,
    v: f64,
) {
    if let Some(t) = instrument {
        if let Some(Some(Sink::Instrument(node))) = sinks.get_mut(usize::from(t.0)) {
            node.automate(frame, param, v);
        }
        return;
    }
    if let Some(node) = graph.and_then(|g| g.insert_mut(id)) {
        node.automate(frame, param, v);
    }
}

/// Note-offs (and pedal-ups) for every target at block offset `offset`. // RT
fn release_all(notes: &mut ActiveNotes, o: &mut Out<'_>, offset: u32) {
```

- [ ] **Step 5: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-engine 2>&1 | grep -E "^test result|FAILED|panicked"
```

Expected: PASS — the new tests, the `rt_alloc` automation phase, and every existing render, offline and golden-log test (a block without automation is one run, exactly as before).

- [ ] **Step 6: Gate and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace 2>&1 | grep -E "^test result|FAILED|panicked" | tail -60
git add crates/core-engine/src/render.rs crates/core-engine/src/transport.rs crates/core-engine/src/graph/exec.rs crates/core-engine/src/graph/node.rs crates/core-engine/src/backend.rs crates/core-engine/tests/automation.rs crates/core-engine/tests/rt_alloc.rs
git commit -m "feat(engine): play automation in runs split at its points, without allocating

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
```

Expected: 716 Rust tests listed (709 + 7).

---

### Task 9: Host — automated plugin parameters reach CLAP and AU processors

**Files:**
- Modify: `crates/core-host/src/adapt.rs`, `crates/core-host/src/shelf.rs`
- Test: `crates/core-host/tests/engine_plugins.rs`, `crates/core-host/tests/rt_alloc_plugins.rs`

**Interfaces:**
- Consumes: Task 8's `EffectNode::automate`, `InstrumentNode::automate`; Task 6's `InsertFactory::param_stepped`; `core_model::automation::param_plain`; `PluginInfo.params` (`ParamInfo { id, min, max, stepped, .. }`).
- Produces: `PluginEffect::automate`, `PluginSynth::automate` (normalised → plain by the parameter's range; unknown parameters dropped; at most `MAX_PARAM_EVENTS` per block), merged after the host's queued values in each `ProcessCtx::params_in` (sorted by frame); `PluginEffect::stepped_params(&self) -> Vec<ParamId>`, `PluginSynth::stepped_params(&self) -> Vec<ParamId>`, `Ready::stepped_params(&self) -> Vec<ParamId>`; `ShelfFactory::param_stepped` (the shelf remembers each instance's stepped parameters when its processor is put up).

Ruling 12. AU processors already pass `params_in` to `AUParameter` writes and record them as their own writes, so automated values never come back as gestures; CLAP plugins report output parameter events only for their own UI changes.

- [ ] **Step 1: Write the failing tests**

Append to `crates/core-host/tests/engine_plugins.rs`:

```rust
use core_engine::graph::InsertFactory;
use core_model::automation::AutomationTarget;

/// The built-in synth holding C4 for 4 bars through a CLAP gain insert,
/// with automation `points` (tick, normalised value) of its gain (0–2).
fn gain_song(points: &[(i64, f64)]) -> (Document, SeqId, PluginInstanceId) {
    let mut b = Builder::new();
    let d = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, notes) = b.sequence("S", "Notes", Some(d)).unwrap();
    b.notes(seq, notes, &[(0, 60, 0xFFFF, 16 * Q)]).unwrap();
    let synth = b.strip_of(StripKind::BuiltinSynth).unwrap();
    let id = b.insert(synth, plugin("vizdaw.test.gain")).unwrap();
    let lane = b.lane(seq, "Auto", None).unwrap();
    let events: Vec<EventSpec> = points
        .iter()
        .map(|(t, v)| {
            EventSpec::new(
                Tick(*t),
                EventBody::Automation {
                    target: AutomationTarget::PluginParam(id, ParamId(0)),
                    value: *v,
                },
            )
        })
        .collect();
    if !events.is_empty() {
        b.events(seq, lane, events).unwrap();
    }
    (b.finish(), seq, id)
}

#[test]
fn a_parameter_ramp_reaches_a_clap_gain_insert() {
    // Without automation the gain stays at its default, 1.0.
    let (flat_doc, seq, id) = gain_song(&[]);
    let mut flat_rig = rig();
    flat_rig.add(id, "vizdaw.test.gain", false);
    flat_rig.engine.load(&flat_doc, seq).unwrap();
    let flat = flat_rig.play(98_304);
    let (doc, seq, id) = gain_song(&[(0, 0.0), (4 * Q, 0.5)]);
    let mut r = rig();
    r.add(id, "vizdaw.test.gain", false);
    let report = r.engine.load(&doc, seq).unwrap();
    assert!(report.graph.placeholders.is_empty(), "{:?}", report.graph);
    assert_eq!(report.automation_lanes, 1);
    let wet = r.play(98_304);
    assert!(flat.iter().any(|x| x.abs() > 1e-3));
    // The test gain applies the last value of a block to the whole block:
    // the one sent at frame 224 of each 256-frame block (a value every 32
    // frames on a ramp, Plan 4d Ruling 12). Four quarters are 96 000 frames.
    for (k, (w, f)) in wet.chunks(256).zip(flat.chunks(256)).enumerate() {
        let at = (k * 256 + 224) as f64;
        let gain = (2.0 * 0.5 * (at / 96_000.0).min(1.0)) as f32;
        for (x, y) in w.iter().zip(f) {
            assert!((x - y * gain).abs() <= 1e-6, "block {k}: {x} vs {}", y * gain);
        }
    }
    // Automated values are not gestures: nothing comes back.
    assert!(r.live[0].gestures.drain().is_empty());
}

#[test]
fn the_shelf_knows_which_parameters_are_stepped() {
    let mut r = rig();
    let id = PluginInstanceId(5);
    r.add(id, "vizdaw.test.gain", false);
    let f = r.shelf.factory();
    assert!(f.param_stepped(id, ParamId(1)), "the latency parameter is stepped");
    assert!(!f.param_stepped(id, ParamId(0)));
    assert!(!f.param_stepped(PluginInstanceId(6), ParamId(1)));
}
```

In `crates/core-host/tests/rt_alloc_plugins.rs`, Find:

```rust
use core_model::command::MixerCmd;
```

Replace:

```rust
use core_model::automation::AutomationTarget;
use core_model::command::MixerCmd;
```

Find:

```rust
    let doc = b.finish();

    let mut host = PluginHost::new(48_000.0, 4096).unwrap();
```

Replace:

```rust
    // Automation of the gain insert's parameter and the built-in fader:
    // plugin parameter changes and split runs on the armed path.
    let auto = b.lane(song, "Auto", None).unwrap();
    let mut points = Vec::new();
    for k in 0..256i64 {
        points.push(EventSpec::new(
            Tick(k * Q / 8),
            EventBody::Automation {
                target: AutomationTarget::PluginParam(gain, ParamId(0)),
                value: (k % 4) as f64 / 4.0,
            },
        ));
        points.push(EventSpec::new(
            Tick(k * Q / 8 + 1),
            EventBody::Automation {
                target: AutomationTarget::StripFader(builtin),
                value: (k % 5) as f64 / 5.0,
            },
        ));
    }
    b.events(song, auto, points).unwrap();
    let doc = b.finish();

    let mut host = PluginHost::new(48_000.0, 4096).unwrap();
```

Find:

```rust
    let allocs = rt_check::count() - before;
    rt_check::set_arm_workers(false);
    assert_eq!(allocs, 0, "allocations while plugins rendered");
```

Replace:

```rust
    let allocs = rt_check::count() - before;
    rt_check::set_arm_workers(false);
    assert_eq!(allocs, 0, "allocations while plugins rendered");
    assert!(engine.counters().runs > 2_000, "automation split blocks");
```

- [ ] **Step 2: Run them to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-host --test engine_plugins -- a_parameter_ramp the_shelf_knows 2>&1 | tail -8
```

Expected: `a_parameter_ramp_reaches_a_clap_gain_insert` fails (the output stays at gain 1.0: the default `automate` drops the values); `the_shelf_knows_which_parameters_are_stepped` fails on its first assertion.

- [ ] **Step 3: Map normalised values and merge them into the block**

In `crates/core-host/src/adapt.rs`, Find:

```rust
use core_model::ids::PluginInstanceId;
use std::sync::mpsc::Sender;
```

Replace:

```rust
use core_model::automation::param_plain;
use core_model::ids::PluginInstanceId;
use core_model::mixer::ParamId;
use std::sync::mpsc::Sender;

/// A parameter's range, for automation (Plan 4d Ruling 12).
#[derive(Debug, Clone, Copy)]
struct ParamRange {
    id: ParamId,
    min: f64,
    max: f64,
    stepped: bool,
}
```

Find:

```rust
    params_buf: Box<[ParamChange; MAX_PARAM_EVENTS]>,
    out: ParamOut,
    back: Option<Sender<Returned>>,
}
```

Replace:

```rust
    params_buf: Box<[ParamChange; MAX_PARAM_EVENTS]>,
    out: ParamOut,
    back: Option<Sender<Returned>>,
    /// Parameter ranges by ID (sorted).
    ranges: Box<[ParamRange]>,
    /// Automation's changes for the next block, in arrival order.
    auto: Box<[ParamChange; MAX_PARAM_EVENTS]>,
    auto_len: usize,
}
```

Find:

```rust
            params_buf: Box::new([ParamChange::default(); MAX_PARAM_EVENTS]),
            out: ParamOut::default(),
            back: Some(back),
        }
    }
```

Replace:

```rust
            params_buf: Box::new([ParamChange::default(); MAX_PARAM_EVENTS]),
            out: ParamOut::default(),
            back: Some(back),
            ranges: {
                let mut r: Vec<ParamRange> = info
                    .params
                    .iter()
                    .map(|p| ParamRange {
                        id: p.id,
                        min: p.min,
                        max: p.max,
                        stepped: p.stepped,
                    })
                    .collect();
                r.sort_by_key(|p| p.id);
                r.into_boxed_slice()
            },
            auto: Box::new([ParamChange::default(); MAX_PARAM_EVENTS]),
            auto_len: 0,
        }
    }

    /// Queues automation's value for `param` at `frame` of the next block:
    /// normalised → plain by the parameter's range (rounded when stepped);
    /// an unknown parameter, or a full queue, drops it (Plan 4d Ruling 12).
    /// // RT
    fn automate(&mut self, frame: u32, param: ParamId, normalized: f64) {
        let Ok(i) = self.ranges.binary_search_by_key(&param, |r| r.id) else {
            return;
        };
        let Some(r) = self.ranges.get(i) else {
            return;
        };
        let value = param_plain(normalized, r.min, r.max, r.stepped);
        if let Some(slot) = self.auto.get_mut(self.auto_len) {
            *slot = ParamChange {
                frame,
                id: param,
                value,
            };
            self.auto_len += 1;
        }
    }

    /// The parameters the plugin declares stepped. Not RT.
    fn stepped(&self) -> Vec<ParamId> {
        self.ranges
            .iter()
            .filter(|r| r.stepped)
            .map(|r| r.id)
            .collect()
    }
```

Find:

```rust
        let k = self.params.drain_into(&mut self.params_buf[..]);
        self.out.clear();
        let Some(proc) = self.proc.as_mut() else {
            return ProcessStatus::Error;
        };
```

Replace:

```rust
        let k = self.params.drain_into(&mut self.params_buf[..]);
        // Automation follows the host's values, so it wins a tie (Plan 4d
        // Ruling 12); then the list is sorted by frame.
        let last = frames.min(self.max_frames).saturating_sub(1);
        let mut n = k;
        for c in self.auto.get(..self.auto_len).unwrap_or(&[]) {
            if let Some(slot) = self.params_buf.get_mut(n) {
                *slot = ParamChange {
                    frame: c.frame.min(last),
                    ..*c
                };
                n += 1;
            }
        }
        self.auto_len = 0;
        sort_params(self.params_buf.get_mut(..n).unwrap_or(&mut []));
        self.out.clear();
        let Some(proc) = self.proc.as_mut() else {
            return ProcessStatus::Error;
        };
```

Find:

```rust
            params_in: self.params_buf.get(..k).unwrap_or(&[]),
```

Replace:

```rust
            params_in: self.params_buf.get(..n).unwrap_or(&[]),
```

Find:

```rust
/// Stable insertion sort by frame: arrival order is frame order except for
/// releases sent at offset 0 after earlier events, and same-frame order
/// must be kept. No allocation (`slice::sort_by_key` may allocate). // RT
fn sort_by_frame(events: &mut [MidiEvent]) {
```

Replace:

```rust
/// Stable insertion sort of parameter changes by frame (no allocation).
/// // RT
fn sort_params(p: &mut [ParamChange]) {
    for i in 1..p.len() {
        let mut j = i;
        while j > 0 {
            let (Some(a), Some(b)) = (p.get(j - 1), p.get(j)) else {
                break;
            };
            if a.frame <= b.frame {
                break;
            }
            p.swap(j - 1, j);
            j -= 1;
        }
    }
}

/// Stable insertion sort by frame: arrival order is frame order except for
/// releases sent at offset 0 after earlier events, and same-frame order
/// must be kept. No allocation (`slice::sort_by_key` may allocate). // RT
fn sort_by_frame(events: &mut [MidiEvent]) {
```

Find:

```rust
impl PluginEffect {
    /// The instance it serves.
    pub fn id(&self) -> PluginInstanceId {
        self.core.id
    }
```

Replace:

```rust
impl PluginEffect {
    /// The instance it serves.
    pub fn id(&self) -> PluginInstanceId {
        self.core.id
    }

    /// The parameters the plugin declares stepped.
    pub fn stepped_params(&self) -> Vec<ParamId> {
        self.core.stepped()
    }
```

Find:

```rust
    fn reset(&mut self) {
        if let Some(p) = self.core.proc.as_mut() {
            p.reset();
        }
    }
}

/// Stable insertion sort of parameter changes by frame (no allocation).
```

Replace:

```rust
    fn reset(&mut self) {
        if let Some(p) = self.core.proc.as_mut() {
            p.reset();
        }
    }

    fn automate(&mut self, frame: u32, param: ParamId, normalized: f64) {
        self.core.automate(frame, param, normalized);
    }
}

/// Stable insertion sort of parameter changes by frame (no allocation).
```

Find:

```rust
impl PluginSynth {
    /// The instance it serves.
    pub fn id(&self) -> PluginInstanceId {
        self.core.id
    }
```

Replace:

```rust
impl PluginSynth {
    /// The instance it serves.
    pub fn id(&self) -> PluginInstanceId {
        self.core.id
    }

    /// The parameters the plugin declares stepped.
    pub fn stepped_params(&self) -> Vec<ParamId> {
        self.core.stepped()
    }
```

Find:

```rust
    fn render_outputs(&mut self, frames: u32, outs: &mut [StereoBuf], info: &BlockInfo) {
        if self.faulted {
            self.count = 0;
```

Replace:

```rust
    fn automate(&mut self, frame: u32, param: ParamId, normalized: f64) {
        self.core.automate(frame, param, normalized);
    }

    fn render_outputs(&mut self, frames: u32, outs: &mut [StereoBuf], info: &BlockInfo) {
        if self.faulted {
            self.count = 0;
            self.core.auto_len = 0;
```

- [ ] **Step 4: Remember stepped parameters on the shelf**

In `crates/core-host/src/shelf.rs`, Find:

```rust
use core_model::ids::PluginInstanceId;
use core_model::mixer::{PluginFormat, PluginRef, PluginSlot};
use std::collections::BTreeMap;
```

Replace:

```rust
use core_model::ids::PluginInstanceId;
use core_model::mixer::{ParamId, PluginFormat, PluginRef, PluginSlot};
use std::collections::{BTreeMap, BTreeSet};
```

Find:

```rust
    /// Unwraps the processor.
    pub fn into_processor(self) -> Option<Box<dyn crate::instance::AudioProcessor>> {
```

Replace:

```rust
    /// The parameters the plugin declares stepped.
    pub fn stepped_params(&self) -> Vec<ParamId> {
        match self {
            Ready::Effect(e) => e.stepped_params(),
            Ready::Instrument(s) => s.stepped_params(),
        }
    }

    /// Unwraps the processor.
    pub fn into_processor(self) -> Option<Box<dyn crate::instance::AudioProcessor>> {
```

Find:

```rust
    /// Why an instance is not on the shelf (loading, failed, disabled).
    why: BTreeMap<PluginInstanceId, String>,
}
```

Replace:

```rust
    /// Why an instance is not on the shelf (loading, failed, disabled).
    why: BTreeMap<PluginInstanceId, String>,
    /// Each instance's stepped parameters, from its last processor put up
    /// (kept after the engine takes it, Plan 4d Ruling 8).
    stepped: BTreeMap<PluginInstanceId, BTreeSet<ParamId>>,
}
```

Find:

```rust
        let mut g = self.lock();
        g.why.remove(&id);
        g.ready
            .insert(
```

Replace:

```rust
        let stepped: BTreeSet<ParamId> = ready.stepped_params().into_iter().collect();
        let mut g = self.lock();
        g.why.remove(&id);
        g.stepped.insert(id, stepped);
        g.ready
            .insert(
```

Find:

```rust
    fn make_instrument(
        &mut self,
        slot: &PluginSlot,
        _sample_rate: f64,
        max_frames: u32,
    ) -> Result<Box<dyn InstrumentNode>, String> {
        match self.0.take(slot, max_frames, true)? {
            Ready::Instrument(s) => Ok(Box::new(s)),
            Ready::Effect(_) => Err("not an instrument".into()),
        }
    }
}
```

Replace:

```rust
    fn make_instrument(
        &mut self,
        slot: &PluginSlot,
        _sample_rate: f64,
        max_frames: u32,
    ) -> Result<Box<dyn InstrumentNode>, String> {
        match self.0.take(slot, max_frames, true)? {
            Ready::Instrument(s) => Ok(Box::new(s)),
            Ready::Effect(_) => Err("not an instrument".into()),
        }
    }

    fn param_stepped(&self, slot: PluginInstanceId, param: ParamId) -> bool {
        self.0
            .lock()
            .stepped
            .get(&slot)
            .is_some_and(|s| s.contains(&param))
    }
}
```

- [ ] **Step 5: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-host 2>&1 | grep -E "^test result|FAILED|panicked"
```

Expected: PASS, including `rt_alloc_plugins` with automation (no allocation on the callback or worker threads) and the gesture tests (`collect_gestures` sees no automated values).

- [ ] **Step 6: Gate and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace 2>&1 | grep -E "^test result|FAILED|panicked" | tail -60
git add crates/core-host/src/adapt.rs crates/core-host/src/shelf.rs crates/core-host/tests/engine_plugins.rs crates/core-host/tests/rt_alloc_plugins.rs
git commit -m "feat(host): automated plugin parameters, mapped to their ranges, merged into each block

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
```

Expected: 718 Rust tests listed (716 + 2).

---

### Task 10: Swift — Strip Chart lanes, laws, names and value scales (pure)

**Files:**
- Create: `app/Sources/VizDAWAppModel/StripLane.swift`
- Test: `app/Tests/VizDAWAppModelTests/StripLaneTests.swift`

**Interfaces:**
- Consumes: generated `AutomationTargetRow`, `LineTargetRow`, `SeriesRow`, `LineOpRow`, `RuleRow`, `ConditionTest`, `EventRow` (with `automation`), `TempoPointRow`, `ParamRow`; `Velocity7`, `Value7`, `Bend14`, `DurationText`, `PitchName`, `MeterMath.ppq`.
- Produces: `AutoTarget` (Codable mirror: `.fader(strip:)`, `.pan(strip:)`, `.mute(strip:)`, `.solo(strip:)`, `.send(strip:index:)`, `.param(slot:param:)`, `.velocityScale(dest:)`; `init(_ AutomationTargetRow)`, `row`, `isSwitch`); `AutomationLaw` (`fullScale`, `floorDb`, `faderDb`, `faderNorm`, `pan`, `velocityScalePercent`, `unit`, `normalized(_ v32: Int64) -> Double`, `v32(_ v: Double) -> Int64`, `text(_:_:param:)`); `StripNames` (`strips`, `sends`, `slots`, `params`, `dests`, `title(_ AutoTarget)`, `param(_:_:)`); `StripValue { id: UInt64?, tick: Int64, length: Int64, value: Int64, ramp: Bool }`; `StripLane` (`.velocity`, `.releaseVelocity`, `.duration`, `.controller(UInt8)`, `.pitchBend`, `.channelPressure`, `.polyPressure(UInt8)`, `.program`, `.tempo`, `.automation(AutoTarget)`; `lineTarget`, `isNoteValue`, `style: StripLane.Style (.bars, .steps, .ramps)`, `range: ClosedRange<Int64>`, `center: Int64?`, `values(_ events: [EventRow], tempo: [TempoPointRow]) -> [StripValue]`, `eraseRule(from:to:) -> RuleRow?`, `text(_ v: Int64, _ names: StripNames) -> String`, `title(_ names: StripNames) -> String`, `init?(_ t: LineTargetRow)`, `static controllers`); `StripMode` (`.draw`, `.modify`, `.erase`); `StripOp` (`.set`, `.scale`, `.add`, `.limitMax`, `.limitMin`, `.compress`, `.thin`, `.thicken`, `.legato`; `title`, `row: LineOpRow`, `choices(for: StripLane)`); `StripScale { range, top, height, inset }` (`y(_:)`, `value(atY:)`, `fraction(atY:)`, `amount(atY:op:)`).

Rulings 4, 18, 21, 22. Value spaces match the core's (Task 4): velocities 16-bit, lengths in ticks (rows show up to one 4/4 bar), controllers/bend/pressure/programs/automation on the 32-bit scale, tempo in milli-BPM (rows show 30–300 BPM).

- [ ] **Step 1: Write the failing tests**

Create `app/Tests/VizDAWAppModelTests/StripLaneTests.swift`:

```swift
import CoreGraphics
import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

private let q: Int64 = 15_360

func ccRow(_ id: UInt64, tick: Int64, index: UInt8, value: Int64) -> EventRow {
    var r = ListInsert.blankRow(kind: .cc, tick: tick)
    r.id = id
    r.value2 = Int64(index)
    r.value = value
    return r
}

func automationRow(_ id: UInt64, tick: Int64, _ target: AutomationTargetRow, _ v: Double) -> EventRow {
    var r = ListInsert.blankRow(kind: .automation, tick: tick)
    r.id = id
    r.automation = AutomationRow(target: target, value: v)
    r.value = AutomationLaw.v32(v)
    return r
}

@Suite struct StripLaneTests {
    @Test func lawsMatchTheCore() {
        #expect(AutomationLaw.faderDb(1) == 12)
        #expect(AutomationLaw.faderDb(0) == AutomationLaw.floorDb)
        #expect(abs(AutomationLaw.faderNorm(0) - 0.630_957) < 1e-6)
        #expect(abs(AutomationLaw.faderDb(AutomationLaw.faderNorm(-6)) + 6) < 1e-9)
        #expect(AutomationLaw.pan(0.25) == -0.5)
        #expect(AutomationLaw.velocityScalePercent(0.5) == 100 && AutomationLaw.velocityScalePercent(1) == 250)
        #expect(AutomationLaw.v32(1) == Int64(UInt32.max) && AutomationLaw.normalized(0) == 0)
        #expect(AutomationLaw.text(.fader(strip: 1), 0) == "-∞ dB")
        #expect(AutomationLaw.text(.fader(strip: 1), AutomationLaw.faderNorm(-6)) == "-6.0 dB")
        #expect(AutomationLaw.text(.pan(strip: 1), 0.5) == "C" && AutomationLaw.text(.pan(strip: 1), 0.25) == "L50")
        #expect(AutomationLaw.text(.mute(strip: 1), 0.7) == "On")
        let p = ParamRow(
            id: 3, name: "Cutoff", module: "", min: 20, max: 20_020, default: 20, value: 20, stepped: false,
            automatable: true, hidden: false)
        #expect(AutomationLaw.text(.param(slot: 2, param: 3), 0.5, param: p) == "1e+04")
        #expect(AutomationLaw.text(.velocityScale(dest: 1), 0.75) == "175%")
    }

    @Test func targetsRoundTripTheirRows() {
        let all: [AutoTarget] = [
            .fader(strip: 1), .pan(strip: 2), .mute(strip: 3), .solo(strip: 4), .send(strip: 5, index: 2),
            .param(slot: 6, param: 1 << 40), .velocityScale(dest: 7),
        ]
        for t in all { #expect(AutoTarget(t.row) == t) }
        #expect(AutoTarget.mute(strip: 1).isSwitch && !AutoTarget.pan(strip: 1).isSwitch)
        let data = try! JSONEncoder().encode(StripLane.automation(.send(strip: 5, index: 2)))
        #expect(try! JSONDecoder().decode(StripLane.self, from: data) == .automation(.send(strip: 5, index: 2)))
    }

    @Test func lanesReadTheirSeries() {
        let note = noteRow(1, tick: 0, duration: q)
        let events = [
            note, ccRow(2, tick: 0, index: 7, value: 100), ccRow(3, tick: q, index: 1, value: 5),
            automationRow(4, tick: q, .stripFader(strip: 9), 0.5), automationRow(5, tick: 2 * q, .stripPan(strip: 9), 1),
        ]
        #expect(StripLane.velocity.values(events, tempo: []) == [StripValue(id: 1, tick: 0, length: q, value: 0x8000, ramp: false)])
        #expect(StripLane.duration.values(events, tempo: []).map(\.value) == [q])
        #expect(StripLane.controller(7).values(events, tempo: []).map(\.id) == [2])
        let fader = StripLane.automation(.fader(strip: 9)).values(events, tempo: [])
        #expect(fader == [StripValue(id: 4, tick: q, length: 0, value: AutomationLaw.v32(0.5), ramp: true)])
        var prog = ListInsert.blankRow(kind: .program, tick: 0)
        prog.id = 6
        prog.value = 5
        #expect(StripLane.program.values([prog], tempo: []).map(\.value) == [5 << 25])
        let tempo = [TempoPointRow(tick: 0, bpm: 120, ramp: true), TempoPointRow(tick: q, bpm: 90, ramp: false)]
        #expect(
            StripLane.tempo.values([], tempo: tempo)
                == [
                    StripValue(id: nil, tick: 0, length: 0, value: 120_000, ramp: true),
                    StripValue(id: nil, tick: q, length: 0, value: 90_000, ramp: false),
                ])
        #expect(StripLane.automation(.mute(strip: 1)).style == .steps)
        #expect(StripLane.automation(.fader(strip: 1)).style == .ramps)
        #expect(StripLane.releaseVelocity.style == .bars && StripLane.controller(1).style == .steps)
    }

    @Test func lanesNameTargetsTextValuesAndBuildRules() {
        var names = StripNames()
        names.strips = [1: "Synth"]
        names.sends = [1: ["Reverb"]]
        names.slots = [6: "EQ"]
        names.params = [6: [
            ParamRow(
                id: 2, name: "Gain", module: "", min: 0, max: 2, default: 1, value: 1, stepped: false,
                automatable: true, hidden: false),
        ]]
        names.dests = [3: "Piano"]
        #expect(StripLane.automation(.fader(strip: 1)).title(names) == "Fader · Synth")
        #expect(StripLane.automation(.send(strip: 1, index: 0)).title(names) == "Send 1 to Reverb · Synth")
        #expect(StripLane.automation(.param(slot: 6, param: 2)).title(names) == "Gain · EQ")
        #expect(StripLane.automation(.velocityScale(dest: 3)).title(names) == "Velocity Scale · Piano")
        #expect(StripLane.automation(.fader(strip: 8)).title(names) == "Fader · Strip 8")
        #expect(StripLane.controller(7).title(names) == "Controller 7 (Volume)")
        #expect(StripLane.polyPressure(60).title(names) == "Poly Pressure C4")
        #expect(StripLane.velocity.text(Int64(Velocity7.up(100)), names) == "100")
        #expect(StripLane.pitchBend.text(Int64(Bend14.up(-8192)), names) == "-8192")
        #expect(StripLane.program.text(9 << 25, names) == "9")
        #expect(StripLane.tempo.text(120_500, names) == "120.5 BPM")
        #expect(StripLane.automation(.param(slot: 6, param: 2)).text(AutomationLaw.v32(0.25), names) == "0.5")
        let rule = StripLane.controller(7).eraseRule(from: 2 * q, to: q)
        #expect(
            rule?.conditions.map(\.test) == [
                .timeRange(start: q, end: 2 * q + 1), .kinds(kinds: [.cc]), .controller(low: 7, high: 7),
            ])
        #expect(StripLane.automation(.mute(strip: 1)).eraseRule(from: 0, to: 0)?.conditions.last?.test
            == .automation(target: .stripMute(strip: 1)))
        #expect(StripLane.velocity.eraseRule(from: 0, to: 1) == nil && StripLane.tempo.eraseRule(from: 0, to: 1) == nil)
        for lane in [StripLane.velocity, .controller(3), .tempo, .automation(.solo(strip: 2))] {
            #expect(StripLane(lane.lineTarget) == lane)
        }
    }

    @Test func scaleClampsOutsideTheRow() {
        let s = StripScale(range: 0...1000, top: 100, height: 106)
        #expect(s.y(1000) == 103 && s.y(0) == 203 && s.y(500) == 153)
        #expect(s.value(atY: 153) == 500)
        #expect(s.value(atY: -500) == 1000, "above the row is the top")
        #expect(s.value(atY: 900) == 0, "below the row is the bottom")
        #expect(s.value(atY: .nan) == 0)
        #expect(s.amount(atY: 103, op: .scale) == 200 && s.amount(atY: 153, op: .scale) == 100)
        #expect(s.amount(atY: 203, op: .add) == -500 && s.amount(atY: 103, op: .add) == 500)
        #expect(s.amount(atY: 153, op: .limitMax) == 500)
        #expect(StripOp.choices(for: .duration).contains(.legato))
        #expect(!StripOp.choices(for: .velocity).contains(.legato))
        #expect(StripOp.choices(for: .controller(1)).contains(.thin))
        #expect(!StripOp.choices(for: .velocity).contains(.thin))
        #expect(StripOp.compress.row == .compress && StripOp.limitMin.row == .limitMin)
    }
}
```

- [ ] **Step 2: Run them to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh test --filter StripLaneTests 2>&1 | tail -5
```

Expected: compile errors (`cannot find 'AutomationLaw' in scope`).

- [ ] **Step 3: Write the lanes**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Sources/VizDAWAppModel/StripLane.swift <<'VIZDAW_EOF'
import CoreGraphics
import Foundation
import VizDAWCore

/// An automation target as layouts save it (mirrors `AutomationTargetRow`).
public enum AutoTarget: Codable, Hashable, Sendable {
    case fader(strip: UInt64)
    case pan(strip: UInt64)
    case mute(strip: UInt64)
    case solo(strip: UInt64)
    case send(strip: UInt64, index: UInt16)
    case param(slot: UInt64, param: UInt64)
    case velocityScale(dest: UInt64)

    public init(_ r: AutomationTargetRow) {
        switch r {
        case .stripFader(let s): self = .fader(strip: s)
        case .stripPan(let s): self = .pan(strip: s)
        case .stripMute(let s): self = .mute(strip: s)
        case .stripSolo(let s): self = .solo(strip: s)
        case .sendLevel(let s, let i): self = .send(strip: s, index: i)
        case .pluginParam(let slot, let p): self = .param(slot: slot, param: p)
        case .velocityScale(let d): self = .velocityScale(dest: d)
        }
    }

    public var row: AutomationTargetRow {
        switch self {
        case .fader(let s): .stripFader(strip: s)
        case .pan(let s): .stripPan(strip: s)
        case .mute(let s): .stripMute(strip: s)
        case .solo(let s): .stripSolo(strip: s)
        case .send(let s, let i): .sendLevel(strip: s, send: i)
        case .param(let slot, let p): .pluginParam(slot: slot, param: p)
        case .velocityScale(let d): .velocityScale(dest: d)
        }
    }

    /// Mute and solo hold each value until the next point (Plan 4d Ruling 8).
    public var isSwitch: Bool {
        switch self {
        case .mute, .solo: true
        default: false
        }
    }
}

/// The normalised-value laws (Plan 4d Ruling 4), exactly as the core has
/// them (`core_model::automation`).
public enum AutomationLaw {
    public static let fullScale = Double(UInt32.max)
    public static let floorDb = -144.0

    public static func unit(_ v: Double) -> Double { v.isNaN ? 0 : min(max(v, 0), 1) }

    public static func faderDb(_ v: Double) -> Double {
        let v = unit(v)
        guard v > 0 else { return floorDb }
        let db = 12 + 60 * log10(v)
        return db <= floorDb ? floorDb : db
    }

    public static func faderNorm(_ db: Double) -> Double {
        db.isNaN || db <= floorDb ? 0 : pow(10, (min(db, 12) - 12) / 60)
    }

    public static func pan(_ v: Double) -> Double { unit(v) * 2 - 1 }

    public static func velocityScalePercent(_ v: Double) -> Double {
        let v = unit(v)
        return v <= 0.5 ? 1 + 198 * v : 100 + 300 * (v - 0.5)
    }

    /// A 32-bit-scale value as a normalised value.
    public static func normalized(_ v32: Int64) -> Double {
        Double(min(max(v32, 0), Int64(UInt32.max))) / fullScale
    }

    /// A normalised value on the 32-bit scale.
    public static func v32(_ v: Double) -> Int64 { Int64((unit(v) * fullScale).rounded()) }

    /// A value as the Strip Chart shows it.
    public static func text(_ t: AutoTarget, _ v: Double, param: ParamRow? = nil) -> String {
        switch t {
        case .fader, .send:
            let db = faderDb(v)
            return db <= floorDb ? "-∞ dB" : String(format: "%.1f dB", db)
        case .pan:
            let p = Int((pan(v) * 100).rounded())
            return p == 0 ? "C" : (p < 0 ? "L\(-p)" : "R\(p)")
        case .mute, .solo:
            return unit(v) >= 0.5 ? "On" : "Off"
        case .param:
            guard let p = param else { return String(format: "%.1f%%", unit(v) * 100) }
            let plain = p.min + unit(v) * (p.max - p.min)
            return p.stepped ? "\(Int(plain.rounded()))" : String(format: "%.3g", plain)
        case .velocityScale:
            return String(format: "%.0f%%", velocityScalePercent(v))
        }
    }
}

/// Names for automation targets, from the mixer (Plan 4d Ruling 21).
public struct StripNames: Equatable, Sendable {
    public var strips: [UInt64: String] = [:]
    /// Per strip, its sends' target names in send order.
    public var sends: [UInt64: [String]] = [:]
    public var slots: [UInt64: String] = [:]
    public var params: [UInt64: [ParamRow]] = [:]
    public var dests: [UInt64: String] = [:]

    public init() {}

    public func param(_ slot: UInt64, _ id: UInt64) -> ParamRow? { params[slot]?.first { $0.id == id } }

    func strip(_ s: UInt64) -> String { strips[s] ?? "Strip \(s)" }

    public func title(_ t: AutoTarget) -> String {
        switch t {
        case .fader(let s): return "Fader · \(strip(s))"
        case .pan(let s): return "Pan · \(strip(s))"
        case .mute(let s): return "Mute · \(strip(s))"
        case .solo(let s): return "Solo · \(strip(s))"
        case .send(let s, let i):
            let to = sends[s].flatMap { $0.indices.contains(Int(i)) ? " to \($0[Int(i)])" : nil } ?? ""
            return "Send \(Int(i) + 1)\(to) · \(strip(s))"
        case .param(let slot, let p):
            return "\(param(slot, p)?.name ?? "Parameter \(p)") · \(slots[slot] ?? "Plugin \(slot)")"
        case .velocityScale(let d):
            return "Velocity Scale · \(dests[d] ?? "Destination \(d)")"
        }
    }
}

/// One value a Strip Chart row draws.
public struct StripValue: Equatable, Sendable {
    /// The event (`nil` for a tempo point).
    public let id: UInt64?
    public let tick: Int64
    /// A note's length (bars are this wide); 0 for points.
    public let length: Int64
    public let value: Int64
    /// The value ramps to the next point (continuous automation, a
    /// ramping tempo point).
    public let ramp: Bool

    public init(id: UInt64?, tick: Int64, length: Int64, value: Int64, ramp: Bool) {
        self.id = id
        self.tick = tick
        self.length = length
        self.value = value
        self.ramp = ramp
    }
}

/// What a Strip Chart row shows (Plan 4d Ruling 21).
public enum StripLane: Codable, Hashable, Sendable {
    case velocity, releaseVelocity, duration
    case controller(UInt8)
    case pitchBend, channelPressure
    case polyPressure(UInt8)
    case program, tempo
    case automation(AutoTarget)

    /// How values are drawn: bars at note starts, held steps, or ramps.
    public enum Style: Sendable { case bars, steps, ramps }

    /// The controllers the row menu lists first.
    public static let controllers: [UInt8] = [1, 2, 4, 7, 10, 11, 64, 74]
    static let controllerNames: [UInt8: String] = [
        1: "Modulation", 2: "Breath", 4: "Foot", 7: "Volume", 10: "Pan", 11: "Expression", 64: "Sustain",
        74: "Brightness",
    ]

    /// The row of a line target (the panel's target picker).
    public init?(_ t: LineTargetRow) {
        switch t {
        case .value(.velocity): self = .velocity
        case .value(.releaseVelocity): self = .releaseVelocity
        case .value(.duration): self = .duration
        case .value(.value): return nil
        case .series(.controller(let i)): self = .controller(i)
        case .series(.pitchBend): self = .pitchBend
        case .series(.channelPressure): self = .channelPressure
        case .series(.polyPressure(let p)): self = .polyPressure(p)
        case .series(.program): self = .program
        case .series(.automation(let t)): self = .automation(AutoTarget(t))
        case .tempo: self = .tempo
        }
    }

    public var lineTarget: LineTargetRow {
        switch self {
        case .velocity: .value(target: .velocity)
        case .releaseVelocity: .value(target: .releaseVelocity)
        case .duration: .value(target: .duration)
        case .controller(let i): .series(series: .controller(index: i))
        case .pitchBend: .series(series: .pitchBend)
        case .channelPressure: .series(series: .channelPressure)
        case .polyPressure(let p): .series(series: .polyPressure(pitch: p))
        case .program: .series(series: .program)
        case .tempo: .tempo
        case .automation(let t): .series(series: .automation(target: t.row))
        }
    }

    public var isNoteValue: Bool {
        switch self {
        case .velocity, .releaseVelocity, .duration: true
        default: false
        }
    }

    public var style: Style {
        switch self {
        case .velocity, .releaseVelocity, .duration: .bars
        case .tempo: .ramps
        case .automation(let t): t.isSwitch ? .steps : .ramps
        default: .steps
        }
    }

    /// The values a row spans, bottom to top (Plan 4d Ruling 18).
    public var range: ClosedRange<Int64> {
        switch self {
        case .velocity, .releaseVelocity: 0...0xFFFF
        case .duration: 0...(4 * MeterMath.ppq)
        case .tempo: 30_000...300_000
        default: 0...Int64(UInt32.max)
        }
    }

    /// A centre line (pitch bend, pan).
    public var center: Int64? {
        switch self {
        case .pitchBend, .automation(.pan): 0x8000_0000
        default: nil
        }
    }

    /// The row's values among `events` (or the tempo map), in time order.
    public func values(_ events: [EventRow], tempo: [TempoPointRow]) -> [StripValue] {
        switch self {
        case .tempo:
            return tempo.map {
                StripValue(id: nil, tick: $0.tick, length: 0, value: Int64(($0.bpm * 1000).rounded()), ramp: $0.ramp)
            }
        case .velocity, .releaseVelocity, .duration:
            return events.filter { $0.kind == .note }.map { e in
                let v: Int64
                switch self {
                case .velocity: v = Int64(e.velocity)
                case .releaseVelocity: v = Int64(e.releaseVelocity)
                default: v = e.duration
                }
                return StripValue(id: e.id, tick: e.tick, length: max(e.duration, 0), value: v, ramp: false)
            }
        default:
            let ramps = style == .ramps
            return events.compactMap { e in
                seriesValue(e).map { StripValue(id: e.id, tick: e.tick, length: 0, value: $0, ramp: ramps) }
            }
        }
    }

    /// `e`'s value when it belongs to this row's series (32-bit scale).
    func seriesValue(_ e: EventRow) -> Int64? {
        switch self {
        case .controller(let i): e.kind == .cc && e.value2 == Int64(i) ? e.value : nil
        case .pitchBend: e.kind == .pitchBend ? e.value : nil
        case .channelPressure: e.kind == .channelPressure ? e.value : nil
        case .polyPressure(let p): e.kind == .polyPressure && e.pitch == p ? e.value : nil
        case .program: e.kind == .program ? e.value << 25 : nil
        case .automation(let t):
            e.kind == .automation && e.automation?.target == t.row ? AutomationLaw.v32(e.automation?.value ?? 0) : nil
        default: nil
        }
    }

    /// What the eraser deletes between two ticks (either order); nil for
    /// note values and tempo, which are not erased here.
    public func eraseRule(from: Int64, to: Int64) -> RuleRow? {
        let lo = min(from, to)
        let hi = max(from, to)
        func rule(_ tests: [ConditionTest]) -> RuleRow {
            RuleRow(
                conditions: ([.timeRange(start: lo, end: hi + 1)] + tests).map { ConditionRow(test: $0, negate: false) },
                matchAll: true)
        }
        switch self {
        case .velocity, .releaseVelocity, .duration, .tempo: return nil
        case .controller(let i): return rule([.kinds(kinds: [.cc]), .controller(low: i, high: i)])
        case .pitchBend: return rule([.kinds(kinds: [.pitchBend])])
        case .channelPressure: return rule([.kinds(kinds: [.channelPressure])])
        case .polyPressure(let p): return rule([.kinds(kinds: [.polyPressure]), .pitch(low: p, high: p)])
        case .program: return rule([.kinds(kinds: [.program])])
        case .automation(let t): return rule([.automation(target: t.row)])
        }
    }

    public func title(_ names: StripNames) -> String {
        switch self {
        case .velocity: "Velocity"
        case .releaseVelocity: "Release Velocity"
        case .duration: "Length"
        case .controller(let i): "Controller \(i)" + (Self.controllerNames[i].map { " (\($0))" } ?? "")
        case .pitchBend: "Pitch Bend"
        case .channelPressure: "Channel Pressure"
        case .polyPressure(let p): "Poly Pressure \(PitchName.name(p))"
        case .program: "Program"
        case .tempo: "Tempo"
        case .automation(let t): names.title(t)
        }
    }

    /// A value as the row shows it.
    public func text(_ v: Int64, _ names: StripNames) -> String {
        switch self {
        case .velocity, .releaseVelocity: return "\(Velocity7.of(UInt16(clamping: v)))"
        case .duration: return DurationText.text(max(v, 0), beat: MeterMath.ppq)
        case .controller, .channelPressure, .polyPressure: return "\(Value7.of(v))"
        case .pitchBend: return "\(Bend14.of(v))"
        case .program: return "\(min(max(v, 0), Int64(UInt32.max)) >> 25)"
        case .tempo: return String(format: "%.1f BPM", Double(v) / 1000)
        case .automation(let t):
            let p: ParamRow?
            if case .param(let slot, let id) = t { p = names.param(slot, id) } else { p = nil }
            return AutomationLaw.text(t, AutomationLaw.normalized(v), param: p)
        }
    }
}

/// The Strip Chart's modes (Plan 4d Ruling 22).
public enum StripMode: String, Codable, CaseIterable, Sendable {
    case draw, modify, erase

    public var title: String {
        switch self {
        case .draw: "Draw"
        case .modify: "Modify"
        case .erase: "Erase"
        }
    }
}

/// What a Modify line does (Plan 4d Rulings 15, 19).
public enum StripOp: String, Codable, CaseIterable, Sendable {
    case set, scale, add, limitMax, limitMin, compress, thin, thicken, legato

    public var title: String {
        switch self {
        case .set: "Set"
        case .scale: "Scale"
        case .add: "Add"
        case .limitMax: "Max Limit"
        case .limitMin: "Min Limit"
        case .compress: "Compress"
        case .thin: "Thin"
        case .thicken: "Thicken"
        case .legato: "Legato"
        }
    }

    public var row: LineOpRow {
        switch self {
        case .set: .set
        case .scale: .scale
        case .add: .add
        case .limitMax: .limitMax
        case .limitMin: .limitMin
        case .compress: .compress
        case .thin: .thin
        case .thicken: .thicken
        case .legato: .legato
        }
    }

    /// What a row offers: legato for lengths only; thin and thicken for
    /// series and tempo.
    public static func choices(for lane: StripLane) -> [StripOp] {
        let common: [StripOp] = [.set, .scale, .add, .limitMax, .limitMin, .compress]
        if lane == .duration { return common + [.legato] }
        return lane.isNoteValue ? common : common + [.thin, .thicken]
    }
}

/// Value ↔ y inside one row (`top` is the row's top in the view; `inset`
/// points stay free above and below).
public struct StripScale: Equatable, Sendable {
    public var range: ClosedRange<Int64>
    public var top: CGFloat
    public var height: CGFloat
    public var inset: CGFloat

    public init(range: ClosedRange<Int64>, top: CGFloat, height: CGFloat, inset: CGFloat = 3) {
        self.range = range
        self.top = top.isFinite ? top : 0
        self.height = height.isFinite ? max(height, 0) : 0
        self.inset = inset
    }

    var span: CGFloat { max(height - 2 * inset, 1) }

    public func y(_ v: Int64) -> CGFloat {
        let lo = Double(range.lowerBound)
        let hi = Double(range.upperBound)
        let c = Double(min(max(v, range.lowerBound), range.upperBound))
        let f = hi > lo ? (c - lo) / (hi - lo) : 0
        return top + inset + span * CGFloat(1 - f)
    }

    /// 0 at the bottom of the row, 1 at its top; clamped.
    public func fraction(atY y: CGFloat) -> Double {
        guard y.isFinite else { return 0 }
        let f = 1 - Double((y - top - inset) / span)
        return f.isFinite ? min(max(f, 0), 1) : 0
    }

    public func value(atY y: CGFloat) -> Int64 {
        let lo = Double(range.lowerBound)
        let hi = Double(range.upperBound)
        return Int64((lo + fraction(atY: y) * (hi - lo)).rounded())
    }

    /// What a point at `y` stands for under `op` (Plan 4d Ruling 18): a
    /// percentage for Scale (0 at the bottom, 100 in the middle, 200 at the
    /// top), a signed change for Add (± half the range at the edges), else a
    /// value.
    public func amount(atY y: CGFloat, op: StripOp) -> Int64 {
        switch op {
        case .scale:
            return Int64((fraction(atY: y) * 200).rounded())
        case .add:
            let half = Double(range.upperBound - range.lowerBound)
            return Int64(((fraction(atY: y) - 0.5) * half).rounded())
        default:
            return value(atY: y)
        }
    }
}
VIZDAW_EOF
```

- [ ] **Step 4: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh test --filter StripLaneTests 2>&1 | tail -5
```

Expected: PASS.

- [ ] **Step 5: Gate and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/build-ffi.sh && scripts/swift.sh build 2>&1 | tail -3 && scripts/swift.sh test 2>&1 | tail -3
git add app/Sources/VizDAWAppModel/StripLane.swift app/Tests/VizDAWAppModelTests/StripLaneTests.swift
git commit -m "feat(app): Strip Chart lanes, automation laws, target names and value scales

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
```

Expected: Swift 493 (488 + 5).

---

### Task 11: Swift — Strip Chart settings, lines from drags, commands and exact mode (pure)

**Files:**
- Create: `app/Sources/VizDAWAppModel/StripChart.swift`
- Modify: `app/Sources/VizDAWAppModel/EditorLayout.swift`, `app/Sources/VizDAWAppModel/ExactMode.swift`
- Test: `app/Tests/VizDAWAppModelTests/StripChartTests.swift`

**Interfaces:**
- Consumes: Task 10's `StripLane`, `StripMode`, `StripOp`; Task 4's `ModifyKind.lineTitle`; generated `LineRow`, `LineShapeRow`, `PeriodRow`, `LinePointRow`, `CurveRow`, `WaveRow`.
- Produces: `ShapeKind` (`.straight`, `.curved`, `.flat`, `.free`, `.random`, `.oscillator`), `CurveKind` (`.easeIn`, `.easeOut`, `.sCurve`; `row`), `WaveKind` (`.sine`, `.square`, `.triangle`, `.saw`; `row`), `PeriodKind` (`.musical`, `.clock`, `.bars`); `StripChartSettings` (`mode`, `op`, `shape`, `curve`, `wave`, `period`, `periodTicks`, `periodMs`, `periodBars`, `phase`, `step`, `rowHeight`, `ticksPerPoint`; tolerant decoding; `periodRow`, `shapeRow(seed:)`); `EditorLayout.stripChart: StripChartSettings`; `StripPoint { tick: Int64, value: Int64 }`; `StripLineBuilder.{line(from:to:stroke:settings:seed:) -> LineRow, freePoints(_:) -> [LinePointRow]}`; `StripCommands.{op(_:settings:) -> LineOpRow, line(_:op:line:selectedOnly:) -> EditCommand, compress(_:from:to:step:selectedOnly:) -> EditCommand, erase(_:from:to:) -> EditCommand?, label(_:op:) -> String}`; `GestureOrigin.stripChart`; `ExactMode.request` maps a Strip Chart line to the panel.

Rulings 16, 17, 22, 23. A line's ends are put in time order (a right-to-left drag is turned around); a click is a line of one tick; Flat holds the pressed value; a Free line runs through the pointer's points (one per tick, the later winning); a Compress rectangle spans its two corners' ticks and values.

- [ ] **Step 1: Write the failing tests**

Create `app/Tests/VizDAWAppModelTests/StripChartTests.swift`:

```swift
import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

private let q: Int64 = 15_360

private func pt(_ t: Int64, _ v: Int64) -> StripPoint { StripPoint(tick: t, value: v) }

@Suite struct StripChartTests {
    @Test func aBackwardDragIsTurnedAround() {
        let s = StripChartSettings()
        let l = StripLineBuilder.line(from: pt(2 * q, 10), to: pt(0, 50), stroke: [], settings: s, seed: 1)
        #expect(l == LineRow(from: 0, to: 2 * q, start: 50, end: 10, step: q / 8, shape: .straight))
    }

    @Test func aClickDrawsOnePoint() {
        let l = StripLineBuilder.line(from: pt(q, 7), to: pt(q, 7), stroke: [], settings: StripChartSettings(), seed: 1)
        #expect(l.from == q && l.to == q && l.start == 7)
        let cmd = StripCommands.line(.controller(7), op: .draw, line: l, selectedOnly: false)
        #expect(
            cmd
                == .selectAndModify(
                    rule: RuleRow(
                        conditions: [ConditionRow(test: .timeRange(start: q, end: q + 1), negate: false)],
                        matchAll: true),
                    mode: .select, modify: .line(target: .series(series: .controller(index: 7)), op: .draw, line: l),
                    toCopy: false))
    }

    @Test func shapesComeFromTheSettings() {
        var s = StripChartSettings()
        s.shape = .flat
        let flat = StripLineBuilder.line(from: pt(q, 30), to: pt(2 * q, 90), stroke: [], settings: s, seed: 1)
        #expect(flat.start == 30 && flat.end == 30 && flat.shape == .flat, "flat holds the pressed value")
        s.shape = .curved
        s.curve = .sCurve
        #expect(StripLineBuilder.line(from: pt(0, 0), to: pt(q, 1), stroke: [], settings: s, seed: 1).shape == .curved(curve: .sCurve))
        s.shape = .random
        #expect(StripLineBuilder.line(from: pt(0, 0), to: pt(q, 1), stroke: [], settings: s, seed: 42).shape == .random(seed: 42))
        s.shape = .oscillator
        s.wave = .triangle
        s.period = .clock
        s.periodMs = 250
        s.phase = 5000
        #expect(
            StripLineBuilder.line(from: pt(0, 0), to: pt(q, 1), stroke: [], settings: s, seed: 1).shape
                == .oscillator(wave: .triangle, period: .millis(ms: 250), phase: 999))
        s.period = .bars
        s.periodBars = 0
        #expect(s.periodRow == .bars(bars: 1), "a period of 0 becomes 1")
        s.shape = .free
        let free = StripLineBuilder.line(
            from: pt(0, 5), to: pt(3 * q, 9), stroke: [pt(0, 5), pt(2 * q, 7), pt(q, 1), pt(q, 2), pt(3 * q, 9)],
            settings: s, seed: 1)
        #expect(
            free
                == LineRow(
                    from: 0, to: 3 * q, start: 5, end: 9, step: q / 8,
                    shape: .free(points: [
                        LinePointRow(tick: 0, value: 5), LinePointRow(tick: q, value: 2),
                        LinePointRow(tick: 2 * q, value: 7), LinePointRow(tick: 3 * q, value: 9),
                    ])))
    }

    @Test func commandsFollowTheModeOperationAndModifier() {
        var s = StripChartSettings()
        #expect(StripCommands.op(.controller(1), settings: s) == .draw)
        #expect(StripCommands.op(.velocity, settings: s) == .set, "drawing a note value sets it")
        s.mode = .modify
        s.op = .scale
        #expect(StripCommands.op(.velocity, settings: s) == .scale)
        let l = LineRow(from: 0, to: q, start: 0, end: 1, step: 1, shape: .straight)
        guard case let .selectAndModify(_, mode, _, _) = StripCommands.line(.velocity, op: .set, line: l, selectedOnly: true)
        else { return }
        #expect(mode == .refine, "⌥ limits the line to the selected events")
        #expect(StripCommands.label(.controller(1), op: .draw) == "Draw Controllers")
        #expect(StripCommands.label(.tempo, op: .draw) == "Draw Tempo")
        #expect(StripCommands.label(.automation(.fader(strip: 1)), op: .draw) == "Draw Automation")
        #expect(StripCommands.label(.velocity, op: .set) == "Change Velocity")
    }

    @Test func aZeroWidthCompressIsOneTick() {
        let c = StripCommands.compress(.velocity, from: pt(q, 900), to: pt(q, 100), step: 0, selectedOnly: false)
        let line = LineRow(from: q, to: q, start: 100, end: 900, step: 1, shape: .flat)
        #expect(
            c
                == .selectAndModify(
                    rule: RuleRow(
                        conditions: [ConditionRow(test: .timeRange(start: q, end: q + 1), negate: false)],
                        matchAll: true),
                    mode: .select, modify: .line(target: .value(target: .velocity), op: .compress, line: line),
                    toCopy: false))
    }

    @Test func eraseDeletesTheRowsSeriesOnly() {
        #expect(
            StripCommands.erase(.pitchBend, from: q, to: 0)
                == .selectAndModify(
                    rule: StripLane.pitchBend.eraseRule(from: 0, to: q), mode: .select, modify: .delete, toCopy: false))
        #expect(StripCommands.erase(.velocity, from: 0, to: q) == nil)
    }

    @Test func exactModeOpensTheLineInThePanel() {
        let l = LineRow(from: q, to: 2 * q, start: 0, end: 100, step: 1, shape: .straight)
        let cmd = StripCommands.line(.controller(7), op: .add, line: l, selectedOnly: false)
        let r = ExactMode.request(for: cmd, lanes: [LaneKey(3, 31)], origin: .stripChart)
        #expect(r?.task == .change)
        #expect(r?.config.modify.op == .line(target: .series(series: .controller(index: 7)), op: .add, line: l))
        #expect(r?.config.select.enabled == true && r?.config.select.conditions.count == 1)
        #expect(r?.events == nil && r?.lanes == [LaneKey(3, 31)])
        #expect(r?.range == TickRange(start: q, end: 2 * q + 1))
        #expect(ExactMode.request(for: .event(edit: .delete(events: [1])), lanes: [], origin: .stripChart) == nil)
    }

    @Test func settingsDecodeTolerantly() throws {
        let json = #"{"mode":"modify","rowHeight":5000,"shape":"bogus","periodMs":250}"#
        let s = try JSONDecoder().decode(StripChartSettings.self, from: Data(json.utf8))
        #expect(s.mode == .modify && s.rowHeight == 400 && s.shape == .straight && s.periodMs == 250)
        var l = EditorLayout()
        l.stripChart.op = .thin
        #expect(EditorLayout.decode(l.encoded()).stripChart.op == .thin)
        #expect(EditorLayout.decode("{}").stripChart == StripChartSettings())
    }
}
```

- [ ] **Step 2: Run them to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh test --filter StripChartTests 2>&1 | tail -5
```

Expected: compile errors (`cannot find 'StripChartSettings' in scope`).

- [ ] **Step 3: Write the settings, lines and commands**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Sources/VizDAWAppModel/StripChart.swift <<'VIZDAW_EOF'
import Foundation
import VizDAWCore

/// Line shapes (Plan 4d Ruling 17).
public enum ShapeKind: String, Codable, CaseIterable, Sendable {
    case straight, curved, flat, free, random, oscillator

    public var title: String {
        switch self {
        case .straight: "Straight"
        case .curved: "Curved"
        case .flat: "Flat"
        case .free: "Free"
        case .random: "Random"
        case .oscillator: "Oscillator"
        }
    }
}

/// Curves of a curved line (the panel's curves).
public enum CurveKind: String, Codable, CaseIterable, Sendable {
    case easeIn, easeOut, sCurve

    public var title: String {
        switch self {
        case .easeIn: "Ease In"
        case .easeOut: "Ease Out"
        case .sCurve: "S-Curve"
        }
    }

    public var row: CurveRow {
        switch self {
        case .easeIn: .easeIn
        case .easeOut: .easeOut
        case .sCurve: .sCurve
        }
    }
}

/// Oscillator waves.
public enum WaveKind: String, Codable, CaseIterable, Sendable {
    case sine, square, triangle, saw

    public var title: String {
        switch self {
        case .sine: "Sine"
        case .square: "Square"
        case .triangle: "Triangle"
        case .saw: "Saw"
        }
    }

    public var row: WaveRow {
        switch self {
        case .sine: .sine
        case .square: .square
        case .triangle: .triangle
        case .saw: .saw
        }
    }
}

/// How an oscillator's period is measured (Plan 4d Ruling 17).
public enum PeriodKind: String, Codable, CaseIterable, Sendable {
    case musical, clock, bars

    public var title: String {
        switch self {
        case .musical: "Musical"
        case .clock: "Clock"
        case .bars: "Bars (follow meter)"
        }
    }
}

/// The Strip Chart bar's settings, shared by every Strip Chart and saved
/// with the project (`EditorLayout.stripChart`). Decoding is tolerant.
public struct StripChartSettings: Codable, Equatable, Sendable {
    public var mode: StripMode = .draw
    public var op: StripOp = .set
    public var shape: ShapeKind = .straight
    public var curve: CurveKind = .easeOut
    public var wave: WaveKind = .sine
    public var period: PeriodKind = .musical
    public var periodTicks: Int64 = MeterMath.ppq
    public var periodMs: UInt32 = 500
    public var periodBars: UInt32 = 1
    /// Thousandths of a period (0–999).
    public var phase: UInt16 = 0
    /// Sample spacing of drawn lines, and the spacing of Thin and Thicken.
    public var step: Int64 = MeterMath.ppq / 8
    public var rowHeight: Double = 96
    /// The Strip Chart's own zoom (a 4/4 bar is 640 pt at 96).
    public var ticksPerPoint: Double = 96

    public static let rowHeightRange: ClosedRange<Double> = 40...400

    public init() {}

    enum CodingKeys: String, CodingKey {
        case mode, op, shape, curve, wave, period, periodTicks, periodMs, periodBars, phase, step, rowHeight
        case ticksPerPoint
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        let d = StripChartSettings()
        mode = c.field(.mode, d.mode)
        op = c.field(.op, d.op)
        shape = c.field(.shape, d.shape)
        curve = c.field(.curve, d.curve)
        wave = c.field(.wave, d.wave)
        period = c.field(.period, d.period)
        periodTicks = max(c.field(.periodTicks, d.periodTicks), 1)
        periodMs = max(c.field(.periodMs, d.periodMs), 1)
        periodBars = max(c.field(.periodBars, d.periodBars), 1)
        phase = min(c.field(.phase, d.phase), 999)
        step = max(c.field(.step, d.step), 1)
        rowHeight = EditorLayout.sane(c.field(.rowHeight, d.rowHeight), Self.rowHeightRange, d.rowHeight)
        ticksPerPoint = EditorLayout.sane(
            c.field(.ticksPerPoint, d.ticksPerPoint), EditorLayout.ticksPerPointRange, d.ticksPerPoint)
    }

    public var periodRow: PeriodRow {
        switch period {
        case .musical: .ticks(ticks: max(periodTicks, 1))
        case .clock: .millis(ms: max(periodMs, 1))
        case .bars: .bars(bars: max(periodBars, 1))
        }
    }

    /// The shape of a line drawn with these settings (a free line's points
    /// are added by `StripLineBuilder`).
    public func shapeRow(seed: UInt64) -> LineShapeRow {
        switch shape {
        case .straight: .straight
        case .curved: .curved(curve: curve.row)
        case .flat: .flat
        case .free: .free(points: [])
        case .random: .random(seed: seed)
        case .oscillator: .oscillator(wave: wave.row, period: periodRow, phase: min(phase, 999))
        }
    }
}

/// A pointer position in a row: a tick and a value on the row's scale.
public struct StripPoint: Equatable, Sendable {
    public var tick: Int64
    public var value: Int64

    public init(tick: Int64, value: Int64) {
        self.tick = tick
        self.value = value
    }
}

/// Drags to lines (Plan 4d Ruling 17).
public enum StripLineBuilder {
    /// A free line's points: ascending ticks, the later point winning on a
    /// tick.
    public static func freePoints(_ stroke: [StripPoint]) -> [LinePointRow] {
        var byTick: [Int64: Int64] = [:]
        for p in stroke { byTick[p.tick] = p.value }
        return byTick.keys.sorted().compactMap { t in byTick[t].map { LinePointRow(tick: t, value: $0) } }
    }

    /// The line a drag from `a` to `b` draws: its ends in time order (a
    /// right-to-left drag is turned around), Flat holding `a`'s value, a
    /// Free line through `stroke`.
    public static func line(
        from a: StripPoint, to b: StripPoint, stroke: [StripPoint], settings s: StripChartSettings, seed: UInt64
    ) -> LineRow {
        let step = max(s.step, 1)
        if s.shape == .free {
            let pts = freePoints(stroke.isEmpty ? [a, b] : stroke)
            if let f = pts.first, let l = pts.last {
                return LineRow(from: f.tick, to: l.tick, start: f.value, end: l.value, step: step, shape: .free(points: pts))
            }
        }
        let (lo, hi) = a.tick <= b.tick ? (a, b) : (b, a)
        if s.shape == .flat {
            return LineRow(from: lo.tick, to: hi.tick, start: a.value, end: a.value, step: step, shape: .flat)
        }
        return LineRow(from: lo.tick, to: hi.tick, start: lo.value, end: hi.value, step: step, shape: s.shapeRow(seed: seed))
    }
}

/// Strip Chart gestures as editor commands (Plan 4d Rulings 15, 22).
public enum StripCommands {
    /// The operation of a Draw or Modify line: Draw writes a series and
    /// sets a note value; Modify uses the bar's operation.
    public static func op(_ lane: StripLane, settings: StripChartSettings) -> LineOpRow {
        guard settings.mode == .modify else { return lane.isNoteValue ? .set : .draw }
        return settings.op.row
    }

    /// A line over `[from, to]` of the selected lanes; `selectedOnly`
    /// limits it to the selected events.
    public static func line(_ lane: StripLane, op: LineOpRow, line: LineRow, selectedOnly: Bool) -> EditCommand {
        .selectAndModify(
            rule: RuleRow(
                conditions: [ConditionRow(test: .timeRange(start: line.from, end: line.to + 1), negate: false)],
                matchAll: true),
            mode: selectedOnly ? .refine : .select,
            modify: .line(target: lane.lineTarget, op: op, line: line), toCopy: false)
    }

    /// A Compress rectangle: the values in its time range onto its bottom
    /// (`start`) and top (`end`).
    public static func compress(
        _ lane: StripLane, from a: StripPoint, to b: StripPoint, step: Int64, selectedOnly: Bool
    ) -> EditCommand {
        let rect = LineRow(
            from: min(a.tick, b.tick), to: max(a.tick, b.tick), start: min(a.value, b.value),
            end: max(a.value, b.value), step: max(step, 1), shape: .flat)
        return line(lane, op: .compress, line: rect, selectedOnly: selectedOnly)
    }

    /// The eraser over `[from, to]`: the row's series goes; nil for note
    /// values and tempo.
    public static func erase(_ lane: StripLane, from: Int64, to: Int64) -> EditCommand? {
        lane.eraseRule(from: from, to: to).map { .selectAndModify(rule: $0, mode: .select, modify: .delete, toCopy: false) }
    }

    /// The undo step's name (the core's).
    public static func label(_ lane: StripLane, op: LineOpRow) -> String {
        ModifyKind.lineTitle(lane.lineTarget, op)
    }
}
VIZDAW_EOF
```

In `app/Sources/VizDAWAppModel/EditorLayout.swift`, Find:

```swift
    /// Every piano roll's view settings (Plan 4b Ruling 11).
    public var pianoRoll = PianoRollSettings()

    public init() {}

    enum CodingKeys: String, CodingKey {
        case version, detail, lastKind, windows, sequenceOrder, blockRule
        case lightMode, snap, laneHeight, ticksPerPoint, follow, pianoRoll
    }
```

Replace:

```swift
    /// Every piano roll's view settings (Plan 4b Ruling 11).
    public var pianoRoll = PianoRollSettings()
    /// Every Strip Chart's bar (Plan 4d Ruling 21).
    public var stripChart = StripChartSettings()

    public init() {}

    enum CodingKeys: String, CodingKey {
        case version, detail, lastKind, windows, sequenceOrder, blockRule
        case lightMode, snap, laneHeight, ticksPerPoint, follow, pianoRoll, stripChart
    }
```

Find:

```swift
        pianoRoll = c.field(.pianoRoll, d.pianoRoll)
    }
```

Replace:

```swift
        pianoRoll = c.field(.pianoRoll, d.pianoRoll)
        stripChart = c.field(.stripChart, d.stripChart)
    }
```

In `app/Sources/VizDAWAppModel/ExactMode.swift`, Find:

```swift
public enum GestureOrigin: Equatable, Sendable {
    case notes, blocks, regionStrip, expression, other
}
```

Replace:

```swift
public enum GestureOrigin: Equatable, Sendable {
    case notes, blocks, regionStrip, expression, other
    /// A Strip Chart line or rectangle (4d).
    case stripChart
}
```

Find:

```swift
        guard origin == .notes || origin == .blocks, case .event(let e) = cmd else { return nil }
        let sorted = lanes.sorted { ($0.seq, $0.lane) < ($1.seq, $1.lane) }
```

Replace:

```swift
        let sorted = lanes.sorted { ($0.seq, $0.lane) < ($1.seq, $1.lane) }
        if origin == .stripChart { return lineRequest(cmd, lanes: sorted) }
        guard origin == .notes || origin == .blocks, case .event(let e) = cmd else { return nil }
```

Find:

```swift
    /// A rubber band as Select by Rule: notes starting in its range and
    /// pitches, in the shown lanes.
```

Replace:

```swift
    /// A Strip Chart line or rectangle as the panel set to Change Values
    /// with the line and its Position condition (Plan 4d Ruling 23).
    static func lineRequest(_ cmd: EditCommand, lanes: [LaneKey]) -> ModifyRequest? {
        guard case let .selectAndModify(rule, mode, .line(target, op, line)?, _) = cmd else { return nil }
        var c = SelectModifyConfig.preset(.change)
        c.select.enabled = rule != nil
        c.select.mode = mode
        c.select.conditions = rule?.conditions ?? []
        c.modify.enabled = true
        c.modify.op = .line(target: target, op: op, line: line)
        c.modify.toCopy = false
        return ModifyRequest(
            task: .change, config: c, events: nil, lanes: lanes, range: TickRange(start: line.from, end: line.to + 1))
    }

    /// A rubber band as Select by Rule: notes starting in its range and
    /// pitches, in the shown lanes.
```

- [ ] **Step 4: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh test --filter "StripChartTests|ExactModeTests|LayoutTests" 2>&1 | tail -5
```

Expected: PASS.

- [ ] **Step 5: Gate and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh build 2>&1 | tail -3 && scripts/swift.sh test 2>&1 | tail -3
git add app/Sources/VizDAWAppModel/StripChart.swift app/Sources/VizDAWAppModel/EditorLayout.swift app/Sources/VizDAWAppModel/ExactMode.swift app/Tests/VizDAWAppModelTests/StripChartTests.swift
git commit -m "feat(app): Strip Chart settings, lines from drags, commands and exact-mode panel requests

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
```

Expected: Swift 501 (493 + 8).

---

### Task 12: Swift — `StripChartModel`: the shown lane, rows, automation targets and gestures

**Files:**
- Create: `app/Sources/VizDAWAppModel/StripChartModel.swift`
- Modify: `app/Sources/VizDAWAppModel/EditorTypes.swift`, `app/Sources/VizDAWAppModel/EditorModel.swift`, `app/Sources/VizDAWAppModel/EditorModel+Windows.swift`, `app/Sources/VizDAWAppModel/DocumentModel.swift`
- Test: `app/Tests/VizDAWAppModelTests/StripChartModelTests.swift`

**Interfaces:**
- Consumes: Tasks 10–11 (`StripLane`, `StripNames`, `StripValue`, `StripChartSettings`, `StripPoint`, `StripLineBuilder`, `StripCommands`, `GestureOrigin.stripChart`); `EditorModel.{laneEvents, info, laneRows, selection, setSelection, beginGesture, updateGesture, endGesture, cancelGesture, updateLayout, snapshotVersion, status}`; `SessionProtocol.{mixer, params, destinations}`.
- Produces: `DetailState.stripRows: [StripLane]?` (decodes from older layouts); `StripMenu { title: String, lanes: [StripLane] }`; `AutomationTargets.load(_ session: any SessionProtocol) -> (names: StripNames, menus: [StripMenu])`; `StripStroke`; `StripChartModel` (`pane(_:)`, `window(_:of:)`, `state`, `target`, `seq`, `lane: LaneKey?`, `chosenLane`, `rows`, `setRow(_:to:)`, `addRow()`, `removeRow(_:)`, `maxRows = 4`, `settings`, `updateSettings(_:)`, `events()`, `values(_:)`, `meter`, `names`, `menus`, `stroke`, `nextSeed`, `strokeBegan(row:at:selectedOnly:)`, `strokeMoved(to:)`, `strokeEnded(at:)`, `strokeCancelled()`, `eraseNotesMessage`); `DocumentModel.{stripChart(_ slot: DetailSlot) -> StripChartModel, dropStripChart(_:)}`.

Rulings 21–23. A stroke opens its gesture (origin `.stripChart`, label from `StripCommands.label`, or "Erase Events") on its first move past the view's dead zone; Draw and Erase make the shown lane the selection's lanes first; Modify keeps the selection's lanes when it has some. A click (no move) in Draw or Erase mode is a one-tick gesture; in Modify mode it does nothing.

- [ ] **Step 1: Write the failing tests**

Create `app/Tests/VizDAWAppModelTests/StripChartModelTests.swift`:

```swift
import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

private let q: Int64 = 15_360

@MainActor
@Suite struct StripChartModelTests {
    func make(_ target: EditorTarget = .lane(seq: 3, lane: 31)) throws -> (FakeSession, DocumentModel, StripChartModel) {
        let f = rollFake()
        let m = try DocumentModel(session: f)
        m.editor.openDetail(target, kind: .stripChart)
        let s = StripChartModel.pane(m)
        s.nextSeed = { 7 }
        f.calls = []
        return (f, m, s)
    }

    @Test func showsTheTargetLaneAndItsRows() throws {
        let (_, m, s) = try make()
        defer { withExtendedLifetime(m) {} }
        #expect(s.lane == LaneKey(3, 31))
        #expect(s.rows == [.velocity])
        #expect(s.values(.velocity).map(\.id) == [1, 2])
        s.addRow()
        #expect(s.rows == [.velocity, .controller(7)])
        s.setRow(1, to: .pitchBend)
        s.removeRow(0)
        #expect(s.rows == [.pitchBend])
        s.removeRow(0)
        #expect(s.rows == [.pitchBend], "one row stays")
        for _ in 0..<6 { s.addRow() }
        #expect(s.rows.count == StripChartModel.maxRows)
        // The rows are saved with the detail state; another editor keeps them.
        m.editor.openDetail(.lane(seq: 3, lane: 31), kind: .pianoRoll)
        #expect(m.editor.layout.detail?.stripRows?.first == .pitchBend)
    }

    @Test func aSequenceTargetShowsTheChosenLaneElseTheFirst() throws {
        let (_, m, s) = try make(.sequence(3))
        defer { withExtendedLifetime(m) {} }
        #expect(s.lane == LaneKey(3, 30))
        s.chosenLane = LaneKey(3, 32)
        #expect(s.lane == LaneKey(3, 32))
        s.chosenLane = LaneKey(3, 99)
        #expect(s.lane == LaneKey(3, 30), "a lane that is gone falls back")
    }

    @Test func automationTargetsComeFromTheMixer() throws {
        let f = mixerFake()
        f.paramTable[10] = [
            ParamRow(
                id: 4, name: "Gain", module: "", min: 0, max: 2, default: 1, value: 1, stepped: false,
                automatable: true, hidden: false),
            ParamRow(
                id: 5, name: "Secret", module: "", min: 0, max: 1, default: 0, value: 0, stepped: false,
                automatable: true, hidden: true),
        ]
        f.dests = [DestRow(id: 2, name: "Piano", plugin: nil)]
        let t = AutomationTargets.load(f)
        #expect(t.menus.map(\.title) == ["Synth", "Gain · Synth", "Bus 1", "Master", "Velocity Scale"])
        #expect(t.menus[0].lanes == [.automation(.fader(strip: 1)), .automation(.pan(strip: 1)), .automation(.mute(strip: 1)), .automation(.solo(strip: 1))])
        #expect(t.menus[1].lanes == [.automation(.param(slot: 10, param: 4))], "hidden parameters are left out")
        #expect(!t.menus[3].lanes.contains(.automation(.pan(strip: 0))), "master has no pan")
        #expect(t.names.title(.param(slot: 10, param: 4)) == "Gain · Gain")
        #expect(t.names.dests[2] == "Piano")
    }

    @Test func aDrawDragIsOneGestureOnTheShownLane() throws {
        let (f, m, s) = try make()
        defer { withExtendedLifetime(m) {} }
        s.strokeBegan(row: 0, at: StripPoint(tick: 0, value: 100), selectedOnly: false)
        #expect(f.calls.isEmpty, "nothing opens before the pointer moves")
        s.strokeMoved(to: StripPoint(tick: q, value: 200))
        #expect(f.selectionSets.last?.lanes == [LaneRef(seq: 3, lane: 31)])
        #expect(m.editor.gesture?.label == "Change Velocity")
        let line = LineRow(from: 0, to: q, start: 100, end: 200, step: q / 8, shape: .straight)
        #expect(f.gestureEdits.last == StripCommands.line(.velocity, op: .set, line: line, selectedOnly: false))
        s.strokeEnded(at: StripPoint(tick: q, value: 200))
        #expect(m.editor.gesture == nil && f.calls.last == "end")
        #expect(s.stroke == nil)
    }

    @Test func aClickInDrawModeDrawsOnePoint() throws {
        let (f, m, s) = try make()
        defer { withExtendedLifetime(m) {} }
        s.setRow(0, to: .controller(1))
        s.strokeBegan(row: 0, at: StripPoint(tick: q, value: 5), selectedOnly: false)
        s.strokeEnded(at: StripPoint(tick: q, value: 5))
        let line = LineRow(from: q, to: q, start: 5, end: 5, step: q / 8, shape: .straight)
        #expect(f.gestureEdits.last == StripCommands.line(.controller(1), op: .draw, line: line, selectedOnly: false))
        #expect(f.calls.first == "begin Draw Controllers" && f.calls.last == "end")
        // A click in Modify mode does nothing.
        s.updateSettings { $0.mode = .modify }
        f.calls = []
        s.strokeBegan(row: 0, at: StripPoint(tick: q, value: 5), selectedOnly: false)
        s.strokeEnded(at: StripPoint(tick: q, value: 5))
        #expect(f.calls.isEmpty)
    }

    @Test func aModifyLineKeepsTheSelectedLanesAndOptionRefines() throws {
        let (f, m, s) = try make()
        defer { withExtendedLifetime(m) {} }
        m.editor.setSelection(SelectionRow(timeRange: nil, lanes: [LaneRef(seq: 3, lane: 32)], events: [3]))
        let sets = f.selectionSets.count
        s.updateSettings {
            $0.mode = .modify
            $0.op = .add
        }
        s.strokeBegan(row: 0, at: StripPoint(tick: 0, value: -5), selectedOnly: true)
        s.strokeMoved(to: StripPoint(tick: q, value: 5))
        #expect(f.selectionSets.count == sets, "the selection's lanes stay")
        #expect(m.editor.gesture?.lanes == [LaneKey(3, 32)])
        guard case let .selectAndModify(_, mode, _, _)? = f.gestureEdits.last else {
            Issue.record("no line")
            return
        }
        #expect(mode == .refine)
        s.strokeCancelled()
    }

    @Test func escapeLeavesNoUndoStep() throws {
        let (f, m, s) = try make()
        defer { withExtendedLifetime(m) {} }
        s.strokeBegan(row: 0, at: StripPoint(tick: 0, value: 1), selectedOnly: false)
        s.strokeMoved(to: StripPoint(tick: q, value: 2))
        s.strokeCancelled()
        #expect(f.calls.last == "abort" && m.editor.gesture == nil && s.stroke == nil)
    }

    @Test func eraseOnANoteRowSaysWhy() throws {
        let (f, m, s) = try make()
        defer { withExtendedLifetime(m) {} }
        s.updateSettings { $0.mode = .erase }
        s.strokeBegan(row: 0, at: StripPoint(tick: 0, value: 1), selectedOnly: false)
        s.strokeMoved(to: StripPoint(tick: q, value: 2))
        #expect(m.editor.status == StripChartModel.eraseNotesMessage)
        #expect(f.calls.isEmpty && m.editor.gesture == nil)
        s.setRow(0, to: .controller(7))
        s.strokeBegan(row: 0, at: StripPoint(tick: 0, value: 1), selectedOnly: false)
        s.strokeMoved(to: StripPoint(tick: q, value: 2))
        #expect(m.editor.gesture?.label == "Erase Events")
        #expect(f.gestureEdits.last == StripCommands.erase(.controller(7), from: 0, to: q))
        s.strokeEnded(at: StripPoint(tick: q, value: 2))
    }

    @Test func exactModeOpensThePanelInsteadOfApplying() throws {
        let (f, m, s) = try make()
        defer { withExtendedLifetime(m) {} }
        var opened = 0
        m.selectModify.onPresent = { opened += 1 }
        m.editor.exactMode = true
        s.setRow(0, to: .controller(7))
        s.strokeBegan(row: 0, at: StripPoint(tick: 0, value: 0), selectedOnly: false)
        s.strokeMoved(to: StripPoint(tick: q, value: 99))
        s.strokeEnded(at: StripPoint(tick: q, value: 99))
        #expect(f.calls.last == "abort", "previewed, then put back")
        #expect(opened == 1 && m.selectModify.task == .change)
        guard case .line(let target, let op, _)? = m.selectModify.config.modification else {
            Issue.record("no line in the panel")
            return
        }
        #expect(target == .series(series: .controller(index: 7)) && op == .draw)
    }
}
```

- [ ] **Step 2: Run them to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh test --filter StripChartModelTests 2>&1 | tail -5
```

Expected: compile errors (`cannot find 'StripChartModel' in scope`).

- [ ] **Step 3: Save rows with the detail state**

In `app/Sources/VizDAWAppModel/EditorTypes.swift`, Find:

```swift
    /// List editor: its kind and destination filter (nil: the default).
    /// Optional, so earlier layouts decode (Plan 4c Ruling 19).
    public var listFilter: ListFilter?

    public init(kind: DetailEditorKind, target: EditorTarget, lanes: [UInt64]? = nil, listFilter: ListFilter? = nil) {
        self.kind = kind
        self.target = target
        self.lanes = lanes
        self.listFilter = listFilter
    }
```

Replace:

```swift
    /// List editor: its kind and destination filter (nil: the default).
    /// Optional, so earlier layouts decode (Plan 4c Ruling 19).
    public var listFilter: ListFilter?
    /// Strip Chart: its rows (nil: one velocity row). Optional, so earlier
    /// layouts decode (Plan 4d Ruling 21).
    public var stripRows: [StripLane]?

    public init(
        kind: DetailEditorKind, target: EditorTarget, lanes: [UInt64]? = nil, listFilter: ListFilter? = nil,
        stripRows: [StripLane]? = nil
    ) {
        self.kind = kind
        self.target = target
        self.lanes = lanes
        self.listFilter = listFilter
        self.stripRows = stripRows
    }
```

In `app/Sources/VizDAWAppModel/EditorModel.swift`, Find:

```swift
            l.detail = DetailState(kind: k, target: target, lanes: lanes, listFilter: l.detail?.listFilter)
```

Replace:

```swift
            l.detail = DetailState(
                kind: k, target: target, lanes: lanes, listFilter: l.detail?.listFilter,
                stripRows: l.detail?.stripRows)
```

In `app/Sources/VizDAWAppModel/EditorModel+Windows.swift`, Find:

```swift
            id: UUID(), detail: DetailState(kind: kind, target: target, lanes: layout.detail?.lanes), frame: nil)
```

Replace:

```swift
            id: UUID(),
            detail: DetailState(
                kind: kind, target: target, lanes: layout.detail?.lanes, stripRows: layout.detail?.stripRows),
            frame: nil)
```

- [ ] **Step 4: Write the model**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Sources/VizDAWAppModel/StripChartModel.swift <<'VIZDAW_EOF'
import Foundation
import Observation
import VizDAWCore

/// A group of the row menu's automation choices.
public struct StripMenu: Equatable, Sendable {
    public let title: String
    public let lanes: [StripLane]

    public init(title: String, lanes: [StripLane]) {
        self.title = title
        self.lanes = lanes
    }
}

/// A document's automation targets (Plan 4d Ruling 21).
public enum AutomationTargets {
    /// Names and menu groups: per strip its fader, pan (not master), mute,
    /// solo and sends; per plugin slot its automatable, visible parameters;
    /// one velocity scale per destination. Parts the session cannot read
    /// now (Busy) are left out.
    public static func load(_ session: any SessionProtocol) -> (names: StripNames, menus: [StripMenu]) {
        var n = StripNames()
        var menus: [StripMenu] = []
        let strips = (try? session.mixer())?.strips ?? []
        for s in strips { n.strips[s.id] = s.name }
        for s in strips {
            n.sends[s.id] = s.sends.map { n.strips[$0.target] ?? "Bus \($0.target)" }
            var lanes: [StripLane] = [.automation(.fader(strip: s.id))]
            if s.kind != .master { lanes.append(.automation(.pan(strip: s.id))) }
            lanes += [.automation(.mute(strip: s.id)), .automation(.solo(strip: s.id))]
            lanes += s.sends.indices.map { .automation(.send(strip: s.id, index: UInt16(clamping: $0))) }
            menus.append(StripMenu(title: s.name, lanes: lanes))
            for slot in (s.instrument.map { [$0] } ?? []) + s.inserts {
                n.slots[slot.id] = slot.plugin.name
                let params = ((try? session.params(slot: slot.id)) ?? []).filter { $0.automatable && !$0.hidden }
                n.params[slot.id] = params
                if !params.isEmpty {
                    menus.append(
                        StripMenu(
                            title: "\(slot.plugin.name) · \(s.name)",
                            lanes: params.map { .automation(.param(slot: slot.id, param: $0.id)) }))
                }
            }
        }
        let dests = (try? session.destinations()) ?? []
        for d in dests { n.dests[d.id] = d.name }
        if !dests.isEmpty {
            menus.append(StripMenu(title: "Velocity Scale", lanes: dests.map { .automation(.velocityScale(dest: $0.id)) }))
        }
        return (n, menus)
    }
}

/// A Strip Chart drag in progress (lane space).
public struct StripStroke: Equatable, Sendable {
    public let row: Int
    public let lane: StripLane
    public let start: StripPoint
    public var current: StripPoint
    /// Every pointer point so far (a free line's).
    public var points: [StripPoint]
    public let mode: StripMode
    /// A Compress rectangle (Modify with Compress).
    public let compress: Bool
    /// ⌥ at mouse-down: the selected events only.
    public let selectedOnly: Bool
    public let seed: UInt64
    /// Its gesture is open.
    public var open = false
}

/// One Strip Chart (spec §4d, Plan 4d Rulings 21–23): the lane and rows it
/// shows, the stroke in progress and the commands it sends. The detail
/// pane and every torn-off window have their own; the bar's settings are
/// shared (`EditorLayout.stripChart`).
@Observable
@MainActor
public final class StripChartModel {
    /// Weak: closing a document while its chart is on screen must not trap.
    public weak var document: DocumentModel?
    public let editor: EditorModel
    @ObservationIgnored private let read: @MainActor () -> DetailState?
    @ObservationIgnored let write: @MainActor (DetailState) -> Void
    /// For a sequence target: the lane shown.
    public var chosenLane: LaneKey?
    public private(set) var stroke: StripStroke?
    /// Seeds random lines (tests replace it).
    @ObservationIgnored public var nextSeed: () -> UInt64 = { UInt64.random(in: 0...UInt64.max) }
    @ObservationIgnored private var targetsCache: (version: UInt64, slots: Int, names: StripNames, menus: [StripMenu])?

    public static let maxRows = 4
    public static let eraseNotesMessage = "Erase works on controller, program and automation rows."

    public init(
        document: DocumentModel, read: @escaping @MainActor () -> DetailState?,
        write: @escaping @MainActor (DetailState) -> Void
    ) {
        self.document = document
        self.editor = document.editor
        self.read = read
        self.write = write
    }

    /// The Strip Chart of the detail pane.
    public static func pane(_ document: DocumentModel) -> StripChartModel {
        StripChartModel(
            document: document,
            read: { [weak document] in document?.editor.layout.detail },
            write: { [weak document] d in document?.editor.updateLayout { $0.detail = d } })
    }

    /// The Strip Chart of torn-off window `id`.
    public static func window(_ id: UUID, of document: DocumentModel) -> StripChartModel {
        StripChartModel(
            document: document,
            read: { [weak document] in document?.editor.layout.windows.first { $0.id == id }?.detail },
            write: { [weak document] d in
                document?.editor.updateLayout { l in
                    if let i = l.windows.firstIndex(where: { $0.id == id }) { l.windows[i].detail = d }
                }
            })
    }

    // MARK: What is shown

    public var state: DetailState? { read() }
    public var target: EditorTarget? { state?.target }
    public var seq: UInt64? { target?.seq }

    /// The lane shown: the target's, else the chosen one, else the
    /// sequence's first.
    public var lane: LaneKey? {
        guard let t = target else { return nil }
        if let l = t.lane { return LaneKey(t.seq, l) }
        let rows = editor.laneRows(t.seq)
        if let c = chosenLane, c.seq == t.seq, rows.contains(where: { $0.id == c.lane }) { return c }
        return rows.first.map { LaneKey(t.seq, $0.id) }
    }

    public var rows: [StripLane] { state?.stripRows ?? [.velocity] }

    public func setRow(_ i: Int, to lane: StripLane) {
        updateRows { r in
            if r.indices.contains(i) { r[i] = lane }
        }
    }

    public func addRow() {
        updateRows { r in
            if r.count < Self.maxRows { r.append(.controller(7)) }
        }
    }

    public func removeRow(_ i: Int) {
        updateRows { r in
            if r.count > 1, r.indices.contains(i) { r.remove(at: i) }
        }
    }

    private func updateRows(_ change: (inout [StripLane]) -> Void) {
        guard var s = state else { return }
        var r = s.stripRows ?? [.velocity]
        change(&r)
        s.stripRows = r
        write(s)
    }

    public var settings: StripChartSettings { editor.layout.stripChart }

    public func updateSettings(_ change: (inout StripChartSettings) -> Void) {
        editor.updateLayout { change(&$0.stripChart) }
    }

    public var meter: [MeterEntryRow] { seq.flatMap { editor.info($0)?.meter } ?? [] }

    public func events() -> [EventRow] { lane.flatMap { editor.laneEvents($0)?.events } ?? [] }

    public func values(_ row: StripLane) -> [StripValue] {
        row.values(events(), tempo: seq.flatMap { editor.info($0)?.tempo } ?? [])
    }

    public var names: StripNames { targets().names }
    public var menus: [StripMenu] { targets().menus }

    private func targets() -> (names: StripNames, menus: [StripMenu]) {
        let v = editor.snapshotVersion
        let slots = document?.slotsVersion ?? 0
        if let c = targetsCache, c.version == v, c.slots == slots { return (c.names, c.menus) }
        let t = AutomationTargets.load(editor.session)
        targetsCache = (v, slots, t.names, t.menus)
        return t
    }

    // MARK: Strokes

    /// The pointer went down on row `row` at `p` (`selectedOnly`: ⌥).
    public func strokeBegan(row: Int, at p: StripPoint, selectedOnly: Bool) {
        guard rows.indices.contains(row), lane != nil else { return }
        let s = settings
        stroke = StripStroke(
            row: row, lane: rows[row], start: p, current: p, points: [p], mode: s.mode,
            compress: s.mode == .modify && s.op == .compress, selectedOnly: selectedOnly, seed: nextSeed())
    }

    /// The pointer moved past the view's dead zone.
    public func strokeMoved(to p: StripPoint) {
        guard var s = stroke else { return }
        s.current = p
        s.points.append(p)
        if !s.open {
            guard open(&s) else {
                stroke = nil
                return
            }
        }
        stroke = s
        editor.updateGesture(command(s))
    }

    /// The pointer went up: the drag's final target; a click in Draw or
    /// Erase mode is a one-tick gesture, in Modify mode nothing.
    public func strokeEnded(at p: StripPoint) {
        guard var s = stroke else { return }
        stroke = nil
        s.current = p
        if s.points.last != p { s.points.append(p) }
        if !s.open {
            guard s.mode != .modify, open(&s) else { return }
        }
        editor.endGesture(command(s))
    }

    /// Escape, or the view went away: no undo step.
    public func strokeCancelled() {
        if stroke?.open == true { editor.cancelGesture() }
        stroke = nil
    }

    func command(_ s: StripStroke) -> EditCommand? {
        var set = settings
        set.mode = s.mode
        switch s.mode {
        case .erase:
            return StripCommands.erase(s.lane, from: s.start.tick, to: s.current.tick)
        case .modify where s.compress:
            return StripCommands.compress(
                s.lane, from: s.start, to: s.current, step: set.step, selectedOnly: s.selectedOnly)
        default:
            let line = StripLineBuilder.line(from: s.start, to: s.current, stroke: s.points, settings: set, seed: s.seed)
            return StripCommands.line(
                s.lane, op: StripCommands.op(s.lane, settings: set), line: line, selectedOnly: s.selectedOnly)
        }
    }

    /// Opens the stroke's gesture on its lanes (Plan 4d Ruling 22).
    private func open(_ s: inout StripStroke) -> Bool {
        guard let lane else { return false }
        if s.mode == .erase, s.lane.eraseRule(from: 0, to: 0) == nil {
            editor.status = Self.eraseNotesMessage
            return false
        }
        let sel = editor.selection
        let keep = s.mode == .modify && !sel.lanes.isEmpty
        let lanes = keep ? sel.lanes.map { LaneKey($0.seq, $0.lane) } : [lane]
        if !keep, sel.lanes != [lane.ref] {
            editor.setSelection(SelectionRow(timeRange: sel.timeRange, lanes: [lane.ref], events: sel.events))
        }
        var set = settings
        set.mode = s.mode
        let label =
            s.mode == .erase
            ? "Erase Events"
            : StripCommands.label(s.lane, op: s.compress ? .compress : StripCommands.op(s.lane, settings: set))
        let pending = editor.gesture != nil
        guard editor.beginGesture(label, lanes: Set(lanes), origin: .stripChart) else {
            if pending { editor.status = EditorModel.gestureOpenMessage }
            return false
        }
        s.open = true
        return true
    }
}
VIZDAW_EOF
```

In `app/Sources/VizDAWAppModel/DocumentModel.swift`, Find:

```swift
    /// The list default filter and columns (set by `DocumentStore`).
    @ObservationIgnored public var listPreferences = ListPreferences()
```

Replace:

```swift
    /// One Strip Chart per slot (Plan 4d Ruling 21).
    @ObservationIgnored private var stripCharts: [DetailSlot: StripChartModel] = [:]

    /// The Strip Chart of the detail pane or a torn-off window, made once.
    public func stripChart(_ slot: DetailSlot) -> StripChartModel {
        if let m = stripCharts[slot] { return m }
        let m: StripChartModel
        switch slot {
        case .pane: m = .pane(self)
        case .window(let id): m = .window(id, of: self)
        }
        stripCharts[slot] = m
        return m
    }

    /// A torn-off window closed: its Strip Chart goes (and its stroke).
    public func dropStripChart(_ slot: DetailSlot) {
        stripCharts.removeValue(forKey: slot)?.strokeCancelled()
    }

    /// The list default filter and columns (set by `DocumentStore`).
    @ObservationIgnored public var listPreferences = ListPreferences()
```

- [ ] **Step 5: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh test --filter "StripChartModelTests|LayoutTests|EditorModelTests" 2>&1 | tail -5
```

Expected: PASS. (`mixerFake()`'s strips are Synth (1, one insert, slot 10 "Gain"), Bus 1 (2) and Master (0); its insert's title is "Gain · Synth".)

- [ ] **Step 6: Gate and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh build 2>&1 | tail -3 && scripts/swift.sh test 2>&1 | tail -3
git add app/Sources/VizDAWAppModel/StripChartModel.swift app/Sources/VizDAWAppModel/EditorTypes.swift app/Sources/VizDAWAppModel/EditorModel.swift app/Sources/VizDAWAppModel/EditorModel+Windows.swift app/Sources/VizDAWAppModel/DocumentModel.swift app/Tests/VizDAWAppModelTests/StripChartModelTests.swift
git commit -m "feat(app): Strip Chart model with rows, automation targets and line gestures

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
```

Expected: Swift 511 (501 + 10).

---

### Task 13: Swift — `StripChartView`: rows, drawing, strokes, row menus, VoiceOver

**Files:**
- Create: `app/Sources/VizDAWEditors/StripChartView.swift`
- Test: `app/Tests/VizDAWAppModelTests/StripChartViewTests.swift`

**Interfaces:**
- Consumes: Task 12's `StripChartModel` (`rows`, `values`, `names`, `menus`, `settings`, `updateSettings`, `meter`, `seq`, `lane`, `stroke`, `strokeBegan/Moved/Ended/Cancelled`, `setRow`, `addRow`, `removeRow`); Task 10's `StripScale`, `StripLane`; `EditorCanvas` (subclass surface), `TimeAxis`, `ToolMachine<Int>.deadZone`, `Modifiers`, `Bbu`.
- Produces: `StripChartView: EditorCanvas` (`init(model:)`; internal for tests: `point(_:row:) -> StripPoint`, `row(atY:) -> Int?`, `rowTop(_:)`, `scale(_:) -> StripScale`, `rowMenu(_:) -> NSMenu`, `path`); `StripRowMenuTarget`.

Rulings 21–23. Rows are `settings.rowHeight` tall, stacked under the ruler and scrolled by the canvas's vertical offset. A press within a row's title (top 18 pt, left 200 pt) opens its menu; any other press starts a stroke; ⌥ at the press limits a Modify to the selected events. Ticks snap to the shared grid when snap is on; in Modify mode a point's value is the operation's amount (Scale: percent; Add: a change). Bars (note values) are drawn at note starts, steps hold to the next point, ramps slope to it; the selection is accented.

- [ ] **Step 1: Write the failing tests**

Create `app/Tests/VizDAWAppModelTests/StripChartViewTests.swift`:

```swift
import AppKit
import Testing
import VizDAWCore
@testable import VizDAWAppModel
@testable import VizDAWEditors

private let q: Int64 = 15_360

/// Chords (31) of `rollFake()` in a Strip Chart in the detail pane: notes 1
/// (C4 at 0) and 2 (E4 at one quarter), 160 pt per quarter at the default
/// zoom, one velocity row 96 pt tall.
@MainActor
private func chart() throws -> (FakeSession, DocumentModel, StripChartView) {
    let f = rollFake()
    let m = try DocumentModel(session: f)
    m.editor.openDetail(.lane(seq: 3, lane: 31), kind: .stripChart)
    let v = StripChartView(model: m.stripChart(.pane))
    v.frame = NSRect(x: 0, y: 0, width: 800, height: 300)
    v.layout()
    f.calls = []
    return (f, m, v)
}

@MainActor
@Suite struct StripChartViewTests {
    @Test func aDragInDrawModeSetsVelocitiesAlongTheLine() throws {
        let (f, m, v) = try chart()
        defer { withExtendedLifetime(m) {} }
        let s = StripScale(range: StripLane.velocity.range, top: 0, height: 96)
        v.contentMouseDown(CGPoint(x: 0, y: 30), clicks: 1, modifiers: [])
        v.contentMouseDragged(CGPoint(x: 160, y: 48), modifiers: [])
        v.contentMouseUp(CGPoint(x: 160, y: 48), modifiers: [])
        let line = LineRow(
            from: 0, to: q, start: s.value(atY: 30), end: s.value(atY: 48), step: q / 8, shape: .straight)
        #expect(f.gestureEdits.last == StripCommands.line(.velocity, op: .set, line: line, selectedOnly: false))
        #expect(f.calls.first == "begin Change Velocity" && f.calls.last == "end")
        #expect(v.path.isEmpty)
    }

    @Test func aClickInDrawModeDrawsOnePoint() throws {
        let (f, m, v) = try chart()
        defer { withExtendedLifetime(m) {} }
        v.contentMouseDown(CGPoint(x: 165, y: 48), clicks: 1, modifiers: [])
        v.contentMouseUp(CGPoint(x: 165, y: 48), modifiers: [])
        guard case .selectAndModify(_, _, .line(_, _, let line)?, _)? = f.gestureEdits.last else {
            Issue.record("no line")
            return
        }
        #expect(line.from == q && line.to == q, "snapped to the quarter, one tick")
    }

    @Test func escapeLeavesNoUndoStep() throws {
        let (f, m, v) = try chart()
        defer { withExtendedLifetime(m) {} }
        v.contentMouseDown(CGPoint(x: 0, y: 30), clicks: 1, modifiers: [])
        v.contentMouseDragged(CGPoint(x: 160, y: 48), modifiers: [])
        v.contentCancel()
        #expect(f.calls.last == "abort" && m.editor.gesture == nil)
        #expect(m.stripChart(.pane).stroke == nil && v.path.isEmpty)
    }

    @Test func modifyModeReadsTheOperationsAmount() throws {
        let (f, m, v) = try chart()
        defer { withExtendedLifetime(m) {} }
        m.stripChart(.pane).updateSettings {
            $0.mode = .modify
            $0.op = .scale
        }
        // Right of the row's title (a press there opens its menu).
        v.contentMouseDown(CGPoint(x: 220, y: 3), clicks: 1, modifiers: [.option])
        v.contentMouseDragged(CGPoint(x: 330, y: 93), modifiers: [.option])
        v.contentMouseUp(CGPoint(x: 330, y: 93), modifiers: [.option])
        guard case let .selectAndModify(_, mode, .line(_, op, line)?, _)? = f.gestureEdits.last else {
            Issue.record("no line")
            return
        }
        #expect(op == .scale && mode == .refine)
        #expect(line.start == 200 && line.end == 0, "top is 200 %, bottom 0 %")
        #expect(line.from == q && line.to == 2 * q)
    }

    @Test func rowsMapPointsAndMenusListEveryKind() throws {
        let (_, m, v) = try chart()
        defer { withExtendedLifetime(m) {} }
        m.stripChart(.pane).addRow()
        #expect(v.row(atY: 10) == 0 && v.row(atY: 100) == 1 && v.row(atY: 250) == nil)
        #expect(v.point(CGPoint(x: 170, y: 3), row: 0) == StripPoint(tick: q, value: 0xFFFF))
        let titles = v.rowMenu(0).items.map(\.title)
        for t in ["Velocity", "Release Velocity", "Length", "Controller", "Pitch Bend", "Channel Pressure",
                  "Poly Pressure", "Program", "Tempo", "Automation", "Add Row", "Remove Row"] {
            #expect(titles.contains(t), "\(t)")
        }
        let target = StripRowMenuTarget()
        var chosen: StripLane?
        target.lanes = [.tempo]
        target.choose = { chosen = $0 }
        let item = NSMenuItem(title: "Tempo", action: #selector(StripRowMenuTarget.pick(_:)), keyEquivalent: "")
        item.tag = 0
        target.pick(item)
        #expect(chosen == .tempo)
    }

    @Test func voiceOverReadsEachValue() throws {
        let (_, m, v) = try chart()
        defer { withExtendedLifetime(m) {} }
        let labels = v.accessibilityItems().map(\.label)
        #expect(labels == ["Velocity 64, bar 1 beat 1", "Velocity 64, bar 1 beat 2"])
        #expect(v.accessibilityName == "Strip Chart")
    }
}
```

- [ ] **Step 2: Run them to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh test --filter StripChartViewTests 2>&1 | tail -5
```

Expected: compile errors (`cannot find 'StripChartView' in scope`).

- [ ] **Step 3: Write the view**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Sources/VizDAWEditors/StripChartView.swift <<'VIZDAW_EOF'
import AppKit
import Observation
import VizDAWAppModel
import VizDAWCore

/// The target of a row menu's items: one object for the whole menu, each
/// item's tag indexing `lanes` (big menus need no per-item objects).
@MainActor
final class StripRowMenuTarget: NSObject {
    var lanes: [StripLane] = []
    var choose: ((StripLane) -> Void)?

    @objc func pick(_ sender: NSMenuItem) {
        guard lanes.indices.contains(sender.tag) else { return }
        choose?(lanes[sender.tag])
    }
}

/// The Strip Chart (editors spec §4d, Plan 4d Rulings 21–23): the shown
/// lane's values in stacked rows under one ruler; Draw, Modify (lines and
/// Compress rectangles) and Erase strokes; a row's title opens the menu of
/// what it shows.
@MainActor
public final class StripChartView: EditorCanvas {
    public let model: StripChartModel
    var editor: EditorModel { model.editor }
    /// Nil once the document closed (the chart holds it weakly).
    var document: DocumentModel? { model.document }

    static let titleHeight: CGFloat = 18
    static let titleWidth: CGFloat = 200

    /// The press in progress (content coordinates): its row and start.
    private var press: (row: Int, at: CGPoint)?
    private var moved = false
    /// The stroke's pointer path, drawn as its preview.
    private(set) var path: [CGPoint] = []
    /// The target the view last scrolled to.
    private var shownTarget: EditorTarget??
    private let menuTarget = StripRowMenuTarget()

    public init(model: StripChartModel) {
        self.model = model
        super.init(frame: .zero)
        axis = TimeAxis(ticksPerPoint: model.settings.ticksPerPoint)
        playheadSource = { [weak self] now in
            guard let self, let doc = self.document, let seq = self.model.seq, doc.transport.seq == seq else {
                return nil
            }
            return doc.clock.tick(atHostTimeNs: now)
        }
        onRulerRange = { [weak self] r in
            guard let self, let lane = self.model.lane else { return }
            self.editor.selectRange(r, lanes: [lane])
        }
        onRulerClick = { [weak self] t in self?.document?.locate(t) }
        onToggleSnap = { [weak self] in self?.editor.updateLayout { $0.snap.enabled.toggle() } }
        onAxisChange = { [weak self] a in
            guard let self, a.ticksPerPoint != self.model.settings.ticksPerPoint else { return }
            self.model.updateSettings { $0.ticksPerPoint = a.ticksPerPoint }
        }
        menuTarget.choose = nil
        refresh()
        observe()
    }

    public required init?(coder: NSCoder) { nil }

    public override var meter: [MeterEntryRow] { model.meter }
    public override var snap: Snap { editor.layout.snap }
    public override var selectedRange: TickRange? { editor.selection.timeRange }
    public override var contentHeight: CGFloat { CGFloat(model.rows.count) * rowHeight }
    public override var accessibilityName: String { "Strip Chart" }

    var rowHeight: CGFloat { CGFloat(model.settings.rowHeight) }

    private func observe() {
        withObservationTracking {
            _ = editor.drawVersion
            _ = editor.layout
            _ = editor.selection
            _ = document?.playKey
            _ = model.chosenLane
            _ = model.stroke
        } onChange: { [weak self] in
            Task { @MainActor in
                self?.refresh()
                self?.observe()
            }
        }
    }

    func refresh() {
        let s = model.settings
        if s.ticksPerPoint != axis.ticksPerPoint {
            axis = TimeAxis(ticksPerPoint: s.ticksPerPoint, originTick: axis.originTick)
        }
        follow = editor.layout.follow
        let key = document?.playKey
        setPlaying(key?.playing == true && key?.seq == model.seq)
        verticalOffset = clampVertical(verticalOffset)
        reload()
    }

    /// The first layout for a target scrolls to a block's start.
    public override func layout() {
        super.layout()
        let target = model.target
        guard shownTarget != .some(target) else { return }
        shownTarget = .some(target)
        if case .block(_, _, let start, _) = target {
            axis = TimeAxis(ticksPerPoint: axis.ticksPerPoint, originTick: Double(start))
        }
    }

    // MARK: Geometry

    func rowTop(_ i: Int) -> CGFloat { CGFloat(i) * rowHeight - verticalOffset }

    func row(atY y: CGFloat) -> Int? {
        guard y.isFinite, rowHeight > 0 else { return nil }
        let i = Int(((y + verticalOffset) / rowHeight).rounded(.down))
        return model.rows.indices.contains(i) ? i : nil
    }

    func scale(_ i: Int) -> StripScale {
        let lane = model.rows.indices.contains(i) ? model.rows[i] : .velocity
        return StripScale(range: lane.range, top: rowTop(i), height: rowHeight)
    }

    /// A content point as row `i`'s tick and value: snapped to the grid when
    /// snap is on; in Modify mode the operation's amount (Plan 4d Ruling 18).
    func point(_ p: CGPoint, row i: Int) -> StripPoint {
        let raw = axis.tick(atX: p.x)
        let tick = snap.enabled ? snap.apply(raw, meter: meter) : raw
        let s = scale(i)
        let set = model.settings
        let value = set.mode == .modify && set.op != .compress ? s.amount(atY: p.y, op: set.op) : s.value(atY: p.y)
        return StripPoint(tick: tick, value: value)
    }

    // MARK: Drawing

    public override func drawContent(in dirty: CGRect) {
        let names = model.names
        let selected = Set(editor.selection.events)
        for (i, lane) in model.rows.enumerated() {
            let top = rowTop(i)
            let rect = CGRect(x: 0, y: top, width: content.bounds.width, height: rowHeight)
            guard rect.intersects(dirty) else { continue }
            NSColor.separatorColor.setFill()
            CGRect(x: dirty.minX, y: rect.maxY - 1, width: dirty.width, height: 1).fill()
            let s = scale(i)
            if let c = lane.center {
                NSColor.separatorColor.withAlphaComponent(0.6).setFill()
                CGRect(x: dirty.minX, y: s.y(c), width: dirty.width, height: 0.5).fill()
            }
            draw(model.values(lane), lane: lane, scale: s, dirty: dirty, selected: selected)
            NSString(string: lane.title(names) + " ▾").draw(
                at: CGPoint(x: 6, y: top + 3),
                withAttributes: [
                    .font: NSFont.systemFont(ofSize: 11, weight: .medium), .foregroundColor: NSColor.secondaryLabelColor,
                ])
        }
    }

    private func draw(_ values: [StripValue], lane: StripLane, scale s: StripScale, dirty: CGRect, selected: Set<UInt64>) {
        let accent = NSColor.controlAccentColor
        func isSelected(_ v: StripValue) -> Bool { v.id.map(selected.contains) ?? false }
        if lane.style == .bars {
            let bottom = s.y(s.range.lowerBound)
            for v in values {
                let x0 = axis.x(v.tick)
                guard x0 + 6 >= dirty.minX, x0 <= dirty.maxX else { continue }
                let y = s.y(v.value)
                (isSelected(v) ? accent : accent.withAlphaComponent(0.45)).setFill()
                CGRect(x: x0, y: y, width: 5, height: max(bottom - y, 1)).fill()
            }
            return
        }
        let line = NSBezierPath()
        var prev: StripValue?
        for v in values {
            let p = CGPoint(x: axis.x(v.tick), y: s.y(v.value))
            if let q = prev {
                if !q.ramp { line.line(to: CGPoint(x: p.x, y: s.y(q.value))) }
                line.line(to: p)
            } else {
                line.move(to: CGPoint(x: min(p.x, dirty.minX), y: p.y))
                line.line(to: p)
            }
            prev = v
            (isSelected(v) ? accent : accent.withAlphaComponent(0.8)).setFill()
            CGRect(x: p.x - 2, y: p.y - 2, width: 4, height: 4).fill()
        }
        if let q = prev {
            line.line(to: CGPoint(x: max(dirty.maxX, axis.x(q.tick)), y: s.y(q.value)))
        }
        accent.withAlphaComponent(0.8).setStroke()
        line.lineWidth = 1.5
        line.stroke()
    }

    public override func drawOverlay(in dirty: CGRect) {
        guard let stroke = model.stroke, let first = path.first, let last = path.last else { return }
        let orange = NSColor.systemOrange
        if stroke.compress {
            let r = CGRect(
                x: min(first.x, last.x), y: min(first.y, last.y), width: abs(last.x - first.x),
                height: abs(last.y - first.y))
            orange.withAlphaComponent(0.12).setFill()
            r.fill()
            orange.setStroke()
            NSBezierPath(rect: r).stroke()
            return
        }
        let p = NSBezierPath()
        p.move(to: first)
        if model.settings.shape == .free {
            for q in path.dropFirst() { p.line(to: q) }
        } else {
            p.line(to: last)
        }
        orange.setStroke()
        p.lineWidth = 1
        p.stroke()
    }

    // MARK: Strokes (content coordinates)

    public override func contentMouseDown(_ p: CGPoint, clicks: Int, modifiers: Modifiers) {
        window?.makeFirstResponder(self)
        guard let i = row(atY: p.y) else { return }
        if p.y - rowTop(i) < Self.titleHeight, p.x < Self.titleWidth {
            rowMenu(i).popUp(positioning: nil, at: p, in: content)
            return
        }
        press = (i, p)
        moved = false
        path = [p]
        model.strokeBegan(row: i, at: point(p, row: i), selectedOnly: modifiers.contains(.option))
    }

    public override func contentMouseDragged(_ p: CGPoint, modifiers: Modifiers) {
        guard let pr = press else { return }
        if !moved {
            guard hypot(p.x - pr.at.x, p.y - pr.at.y) > ToolMachine<Int>.deadZone else { return }
            moved = true
        }
        path.append(p)
        model.strokeMoved(to: point(p, row: pr.row))
        overlay.needsDisplay = true
    }

    public override func contentMouseUp(_ p: CGPoint, modifiers: Modifiers) {
        guard let pr = press else { return }
        press = nil
        model.strokeEnded(at: point(p, row: pr.row))
        path = []
        overlay.needsDisplay = true
    }

    /// Escape: the stroke goes without an undo step.
    public override func contentCancel() {
        press = nil
        path = []
        model.strokeCancelled()
        if editor.gesture != nil { editor.cancelGesture() }
        overlay.needsDisplay = true
    }

    /// Leaving the window ends the press: its release will not come here.
    public override func viewWillMove(toWindow newWindow: NSWindow?) {
        super.viewWillMove(toWindow: newWindow)
        guard newWindow == nil else { return }
        contentCancel()
    }

    // MARK: Row menu

    /// What row `i` can show, and Add Row / Remove Row.
    func rowMenu(_ i: Int) -> NSMenu {
        let menu = NSMenu()
        let current = model.rows.indices.contains(i) ? model.rows[i] : .velocity
        let names = model.names
        var lanes: [StripLane] = []
        menuTarget.choose = { [weak self] lane in self?.model.setRow(i, to: lane) }
        func item(_ lane: StripLane, _ title: String? = nil) -> NSMenuItem {
            let it = NSMenuItem(title: title ?? lane.title(names), action: #selector(StripRowMenuTarget.pick(_:)), keyEquivalent: "")
            it.target = menuTarget
            it.tag = lanes.count
            it.state = lane == current ? .on : .off
            lanes.append(lane)
            return it
        }
        func submenu(_ title: String, _ items: [NSMenuItem]) -> NSMenuItem {
            let sub = NSMenu(title: title)
            items.forEach { sub.addItem($0) }
            let it = NSMenuItem(title: title, action: nil, keyEquivalent: "")
            it.submenu = sub
            return it
        }
        for lane in [StripLane.velocity, .releaseVelocity, .duration] { menu.addItem(item(lane)) }
        menu.addItem(.separator())
        var controllers = StripLane.controllers.map { item(.controller($0)) }
        controllers.append(.separator())
        for group in 0..<4 {
            let lo = UInt8(group * 32)
            controllers.append(
                submenu("\(lo)–\(lo + 31)", (lo...lo + 31).map { item(.controller($0), "Controller \($0)") }))
        }
        menu.addItem(submenu("Controller", controllers))
        menu.addItem(item(.pitchBend))
        menu.addItem(item(.channelPressure))
        let octaves = (0..<11).map { octave -> NSMenuItem in
            let lo = UInt8(octave * 12)
            let hi = UInt8(min(octave * 12 + 11, 127))
            return submenu(PitchName.name(lo) + "–" + PitchName.name(hi), (lo...hi).map { item(.polyPressure($0)) })
        }
        menu.addItem(submenu("Poly Pressure", octaves))
        menu.addItem(item(.program))
        menu.addItem(item(.tempo))
        menu.addItem(.separator())
        let groups = model.menus.map { g in submenu(g.title, g.lanes.map { item($0) }) }
        let automation = submenu("Automation", groups)
        automation.isEnabled = !groups.isEmpty
        menu.addItem(automation)
        menu.addItem(.separator())
        let add = MenuAction(title: "Add Row") { [weak self] in self?.model.addRow() }.item(checked: false)
        add.isEnabled = model.rows.count < StripChartModel.maxRows
        menu.addItem(add)
        let remove = MenuAction(title: "Remove Row") { [weak self] in self?.model.removeRow(i) }.item(checked: false)
        remove.isEnabled = model.rows.count > 1
        menu.addItem(remove)
        menuTarget.lanes = lanes
        menu.autoenablesItems = false
        return menu
    }

    // MARK: Accessibility

    public override func accessibilityItems() -> [CanvasAccessibilityItem] {
        var out: [CanvasAccessibilityItem] = []
        let names = model.names
        for (i, lane) in model.rows.enumerated() {
            let s = scale(i)
            for v in model.values(lane).prefix(200) {
                let b = Bbu.of(v.tick, meter: meter)
                let frame = CGRect(x: axis.x(v.tick) - 3, y: s.y(v.value) - 3, width: 6, height: 6)
                out.append(
                    CanvasAccessibilityItem(
                        label: "\(lane.title(names)) \(lane.text(v.value, names)), bar \(b.bar) beat \(b.beat)",
                        frame: frame, selected: v.id.map { editor.isSelected(event: $0) } ?? false, press: nil))
            }
        }
        return out
    }
}
VIZDAW_EOF
```

- [ ] **Step 4: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh test --filter StripChartViewTests 2>&1 | tail -5
```

Expected: PASS. (At the default zoom of 96 ticks per point a quarter is 160 pt; x 165 snaps to the quarter. A velocity row 96 pt tall with 3 pt insets reads y 3 as 0xFFFF; in Modify with Scale, y 3 is 200 % and y 93 is 0 %.)

- [ ] **Step 5: Gate and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh build 2>&1 | tail -3 && scripts/swift.sh test 2>&1 | tail -3
git add app/Sources/VizDAWEditors/StripChartView.swift app/Tests/VizDAWAppModelTests/StripChartViewTests.swift
git commit -m "feat(editors): Strip Chart view with rows, strokes, row menus and VoiceOver

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
```

Expected: Swift 517 (511 + 6).

---

### Task 14: App — the Strip Chart detail editor (⌘3), its bar and torn-off windows

**Files:**
- Create: `app/Sources/VizDAW/StripChartPane.swift`
- Modify: `app/Sources/VizDAW/DetailPane.swift`, `app/Sources/VizDAW/EditorWindows.swift`
- Test: `app/Tests/VizDAWAppModelTests/StripChartModelTests.swift`

**Interfaces:**
- Consumes: Task 12's `DocumentModel.{stripChart(_:), dropStripChart(_:)}`, `StripChartModel`; Task 13's `StripChartView`; Task 11's settings enums.
- Produces: `StripChartEditor(model:)` (bar + canvas), `StripChartBar`, `StripChartHost`; the detail pane and torn windows show the Strip Chart for `DetailEditorKind.stripChart`.

Rulings 17, 21, 22. The bar: Mode (Draw | Modify | Erase); in Modify an Operation picker; for Draw and the line operations a Shape picker, and by shape a Curve, or a Wave, Period kind (Musical, Clock, Bars), Period value and Phase; Step; for a sequence target a Lane menu; Rows +/−. No `@State` (settings live in `EditorLayout.stripChart`).

- [ ] **Step 1: Write the failing test**

Append to the `StripChartModelTests` suite in `app/Tests/VizDAWAppModelTests/StripChartModelTests.swift` (inside its closing brace):

```swift
    @Test func aTornWindowHasItsOwnStripChart() throws {
        let (_, m, s) = try make()
        s.setRow(0, to: .tempo)
        let w = try #require(m.editor.tearOff(.stripChart, shown: nil))
        let ws = m.stripChart(.window(w.id))
        #expect(ws.rows == [.tempo], "a window starts with the pane's rows")
        ws.setRow(0, to: .program)
        #expect(s.rows == [.tempo] && ws.rows == [.program])
        #expect(m.stripChart(.window(w.id)) === ws)
        m.dropStripChart(.window(w.id))
        #expect(m.stripChart(.window(w.id)) !== ws, "a closed window's chart goes")
    }
```

- [ ] **Step 2: Run it**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh test --filter StripChartModelTests 2>&1 | tail -5
```

Expected: PASS already (Task 12 built the slots); it pins the behaviour the windows below rely on.

- [ ] **Step 3: Write the pane**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Sources/VizDAW/StripChartPane.swift <<'VIZDAW_EOF'
import SwiftUI
import VizDAWAppModel
import VizDAWCore
import VizDAWEditors

/// The Strip Chart (spec §4d) with its bar.
struct StripChartEditor: View {
    let model: StripChartModel

    var body: some View {
        VStack(spacing: 0) {
            StripChartBar(model: model, editor: model.editor)
            Divider()
            StripChartHost(model: model)
        }
    }
}

struct StripChartHost: NSViewRepresentable {
    let model: StripChartModel

    func makeNSView(context: Context) -> StripChartView { StripChartView(model: model) }

    func updateNSView(_ view: StripChartView, context: Context) {}
}

/// Mode, operation, shape and its parameters, step, the lane shown and the
/// number of rows (Plan 4d Rulings 17, 21, 22).
struct StripChartBar: View {
    let model: StripChartModel
    @Bindable var editor: EditorModel

    private static let q = MeterMath.ppq
    static let steps: [(String, Int64)] = [
        ("1/4", q), ("1/8", q / 2), ("1/16", q / 4), ("1/32", q / 8), ("1/64", q / 16), ("1/128", q / 32),
    ]
    static let musicalPeriods: [(String, Int64)] = [
        ("1 Bar of 4/4", 4 * q), ("1/2", 2 * q), ("1/4", q), ("1/8", q / 2), ("1/16", q / 4), ("1/32", q / 8),
    ]
    static let clockPeriods: [UInt32] = [25, 50, 100, 250, 500, 1000, 2000, 4000]
    static let barPeriods: [UInt32] = [1, 2, 4, 8, 16]
    static let phases: [UInt16] = [0, 125, 250, 500, 750]

    private func binding<T>(_ key: WritableKeyPath<StripChartSettings, T>) -> Binding<T> {
        Binding(
            get: { editor.layout.stripChart[keyPath: key] },
            set: { v in editor.updateLayout { $0.stripChart[keyPath: key] = v } })
    }

    private var laneName: String {
        guard let l = model.lane else { return "—" }
        return editor.laneRows(l.seq).first { $0.id == l.lane }?.name ?? "—"
    }

    /// Operations that draw no line shape.
    private var shapeless: Bool {
        let s = editor.layout.stripChart
        return s.mode == .erase || (s.mode == .modify && [.compress, .thin, .thicken, .legato].contains(s.op))
    }

    var body: some View {
        let s = editor.layout.stripChart
        HStack(spacing: 10) {
            Picker("Mode", selection: binding(\.mode)) {
                ForEach(StripMode.allCases, id: \.self) { Text($0.title).tag($0) }
            }
            .pickerStyle(.segmented)
            .labelsHidden()
            .frame(width: 190)
            .accessibilityLabel("Mode")
            if s.mode == .modify {
                Picker("Operation", selection: binding(\.op)) {
                    ForEach(StripOp.allCases, id: \.self) { Text($0.title).tag($0) }
                }
                .labelsHidden()
                .frame(width: 120)
                .accessibilityLabel("Operation")
            }
            if !shapeless {
                Picker("Shape", selection: binding(\.shape)) {
                    ForEach(ShapeKind.allCases, id: \.self) { Text($0.title).tag($0) }
                }
                .labelsHidden()
                .frame(width: 110)
                .accessibilityLabel("Shape")
                if s.shape == .curved {
                    Picker("Curve", selection: binding(\.curve)) {
                        ForEach(CurveKind.allCases, id: \.self) { Text($0.title).tag($0) }
                    }
                    .labelsHidden()
                    .frame(width: 100)
                    .accessibilityLabel("Curve")
                }
                if s.shape == .oscillator {
                    oscillator(s)
                }
            }
            Picker("Step", selection: binding(\.step)) {
                ForEach(Self.steps, id: \.1) { Text($0.0).tag($0.1) }
            }
            .frame(width: 110)
            .help("Distance between drawn points; the spacing of Thin and Thicken")
            .accessibilityLabel("Step")
            if model.target?.lane == nil, let seq = model.seq {
                Menu("Lane: \(laneName)") {
                    ForEach(editor.laneRows(seq), id: \.id) { l in
                        Button(l.name) { model.chosenLane = LaneKey(seq, l.id) }
                    }
                }
                .frame(width: 150)
            }
            Spacer()
            Stepper(
                "Rows: \(model.rows.count)", onIncrement: { model.addRow() },
                onDecrement: { model.removeRow(model.rows.count - 1) }
            )
            .accessibilityLabel("Rows")
        }
        .padding(.horizontal, 8)
        .padding(.vertical, 4)
    }

    @ViewBuilder
    private func oscillator(_ s: StripChartSettings) -> some View {
        Picker("Wave", selection: binding(\.wave)) {
            ForEach(WaveKind.allCases, id: \.self) { Text($0.title).tag($0) }
        }
        .labelsHidden()
        .frame(width: 90)
        .accessibilityLabel("Wave")
        Picker("Period", selection: binding(\.period)) {
            ForEach(PeriodKind.allCases, id: \.self) { Text($0.title).tag($0) }
        }
        .labelsHidden()
        .frame(width: 150)
        .accessibilityLabel("Period Kind")
        switch s.period {
        case .musical:
            Picker("Every", selection: binding(\.periodTicks)) {
                ForEach(Self.musicalPeriods, id: \.1) { Text($0.0).tag($0.1) }
            }
            .frame(width: 140)
            .accessibilityLabel("Period")
        case .clock:
            Picker("Every", selection: binding(\.periodMs)) {
                ForEach(Self.clockPeriods, id: \.self) { Text("\($0) ms").tag($0) }
            }
            .frame(width: 120)
            .accessibilityLabel("Period")
        case .bars:
            Picker("Every", selection: binding(\.periodBars)) {
                ForEach(Self.barPeriods, id: \.self) { Text("\($0) bar\($0 == 1 ? "" : "s")").tag($0) }
            }
            .frame(width: 120)
            .accessibilityLabel("Period")
        }
        Picker("Phase", selection: binding(\.phase)) {
            ForEach(Self.phases, id: \.self) { Text("\(Int($0) * 360 / 1000)°").tag($0) }
        }
        .frame(width: 100)
        .accessibilityLabel("Phase")
    }
}
VIZDAW_EOF
```

In `app/Sources/VizDAW/DetailPane.swift`, Find:

```swift
/// A detail editor's frame: the editor picker, its target, Open in Window,
/// and the editor: the piano roll (4b), the list (4c), else a placeholder.
```

Replace:

```swift
/// A detail editor's frame: the editor picker, its target, Open in Window,
/// and the editor: the piano roll (4b), the list (4c), the Strip Chart
/// (4d), else a placeholder.
```

Find:

```swift
            } else if state.kind == .list {
                ListEditorPane(model: model.list(slot))
            } else {
```

Replace:

```swift
            } else if state.kind == .list {
                ListEditorPane(model: model.list(slot))
            } else if state.kind == .stripChart {
                StripChartEditor(model: model.stripChart(slot))
            } else {
```

In `app/Sources/VizDAW/EditorWindows.swift`, Find:

```swift
            model.dropPianoRoll(.window(window.id))
```

Replace:

```swift
            model.dropPianoRoll(.window(window.id))
            model.dropStripChart(.window(window.id))
```

- [ ] **Step 4: Build and test**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh build 2>&1 | tail -3 && scripts/swift.sh test 2>&1 | tail -3
```

Expected: the app builds with no new warnings; every test passes. If `ForEach(Self.steps, id: \.1)` does not type-check, give `steps` and `musicalPeriods` a small `struct Choice: Hashable { let title: String; let ticks: Int64 }` element type and use `id: \.self`.

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add app/Sources/VizDAW/StripChartPane.swift app/Sources/VizDAW/DetailPane.swift app/Sources/VizDAW/EditorWindows.swift app/Tests/VizDAWAppModelTests/StripChartModelTests.swift
git commit -m "feat(app): the Strip Chart detail editor with its bar and torn-off windows

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
```

Expected: Swift 518 (517 + 1).

---

### Task 15: The panel's Strip Chart Line and Automation Target; the scale-time drag and its exact mode

**Files:**
- Create: `app/Sources/VizDAW/LineModifyEditor.swift`, `app/Sources/VizDAWEditors/ScaleTimeDrag.swift`
- Modify: `app/Sources/VizDAWAppModel/SelectModifyModel.swift`, `app/Sources/VizDAWAppModel/ExactMode.swift`, `app/Sources/VizDAW/ModifyPanel.swift`, `app/Sources/VizDAWEditors/PianoRollView.swift`
- Test: `app/Tests/VizDAWAppModelTests/LinePanelTests.swift`, `app/Tests/VizDAWAppModelTests/ScaleTimeTests.swift`

**Interfaces:**
- Consumes: Task 4's `ModifyRow.line`, `ModifyKind.line`; Tasks 10–12 (`StripLane`, `AutoTarget`, `ShapeKind`, `StripChartSettings.shapeRow`, `AutomationTargets`, `StripMenu`); `NoteDrag.{events, clampTick}`, `Snap.apply`.
- Produces: `SelectModifyModel.{automationNames, automationMenus, setConditionTarget(_:at:), setLineTarget(_:), setLineOp(_:), setLineShape(_:), setLineCurve(_:), setLineWave(_:), setLinePeriodKind(_:)}`; `ShapeKind.init(_ s: LineShapeRow)`; `ScaleTimeDrag` (`init?(notes:)`, `anchor`, `span`, `command(rawTicks:snap:meter:) -> EditCommand?`, `label = "Scale Time"`); `ExactMode.request` maps a scale-time drag (origin `.notes`) to the panel on Scale Time; `LineModifyEditor`, `AutomationTargetPicker` (SwiftUI).

Rulings 23, 24. The panel's line shows target, operation and shape pickers, and by shape a curve, a wave and a period kind, plus the numeric fields Task 4 defined (From, To, Start, End, Step; Period, Phase); a free line shows "Drawn line (n points)" — choosing Free in the panel makes a two-point line from Start to End. Legato is kept only on the Length target (another target turns it into Set). The scale-time drag needs ⌘ at the press on a note's end zone; it acts on the selection (the pressed note becomes the selection when it was not selected).

- [ ] **Step 1: Write the failing tests**

Create `app/Tests/VizDAWAppModelTests/LinePanelTests.swift`:

```swift
import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

private let q: Int64 = 15_360

@MainActor
@Suite struct LinePanelTests {
    func panel() throws -> (FakeSession, DocumentModel, SelectModifyModel) {
        let f = mixerFake()
        let m = try DocumentModel(session: f)
        let p = m.selectModify
        p.nextSeed = { 9 }
        p.open(.change)
        p.setModifyKind(.line)
        return (f, m, p)
    }

    func line(_ p: SelectModifyModel) -> (LineTargetRow, LineOpRow, LineRow)? {
        guard case let .line(t, op, l) = p.config.modify.op else { return nil }
        return (t, op, l)
    }

    @Test func linePickersEditTheLine() throws {
        let (_, m, p) = try panel()
        defer { withExtendedLifetime(m) {} }
        p.setLineTarget(.tempo)
        #expect(line(p)?.0 == .tempo && line(p)?.1 == .set)
        #expect(p.config.modify.op.fields[2].kind == .bpm)
        p.setLineOp(.draw)
        #expect(line(p)?.1 == .draw)
        p.setLineShape(.oscillator)
        #expect(line(p)?.2.shape == .oscillator(wave: .sine, period: .ticks(ticks: q), phase: 0))
        p.setLineWave(.saw)
        p.setLinePeriodKind(.bars)
        #expect(line(p)?.2.shape == .oscillator(wave: .saw, period: .bars(bars: 1), phase: 0))
        #expect(p.config.modify.op.fields.map(\.label).suffix(2) == ["period (bars)", "phase (‰)"])
        p.setLineShape(.random)
        #expect(line(p)?.2.shape == .random(seed: 9))
        p.setLineShape(.curved)
        p.setLineCurve(.easeIn)
        #expect(line(p)?.2.shape == .curved(curve: .easeIn))
        p.setLineShape(.free)
        let l = try #require(line(p)?.2)
        #expect(l.shape == .free(points: [LinePointRow(tick: l.from, value: l.start), LinePointRow(tick: l.to, value: l.end)]))
        #expect(ShapeKind(l.shape) == .free)
        p.setLineOp(.legato)
        p.setLineTarget(.velocity)
        #expect(line(p)?.1 == .set, "legato is for lengths only")
        p.setLineTarget(.duration)
        p.setLineOp(.legato)
        #expect(line(p)?.1 == .legato)
    }

    @Test func automationConditionsPickTheirTarget() throws {
        let (_, m, p) = try panel()
        defer { withExtendedLifetime(m) {} }
        p.addCondition(.automation)
        let i = p.config.select.conditions.count - 1
        p.setConditionTarget(.mute(strip: 1), at: i)
        #expect(p.config.select.conditions[i].test == .automation(target: .stripMute(strip: 1)))
        #expect(p.automationMenus.first?.title == "Synth")
        #expect(p.automationNames.title(.mute(strip: 1)) == "Mute · Synth")
    }

    @Test func applyingALineSendsOneSelectAndModify() throws {
        let (f, m, p) = try panel()
        defer { withExtendedLifetime(m) {} }
        m.editor.setSelection(SelectionRow(timeRange: nil, lanes: [LaneRef(seq: 3, lane: 31)], events: [1]))
        p.config.modify.toCopy = true
        #expect(p.apply())
        guard case let .selectAndModify(_, _, modify, toCopy)? = f.edits.last else {
            Issue.record("nothing sent")
            return
        }
        #expect(modify?.isLine == true && !toCopy, "a line never goes to a copy")
    }
}
```

Create `app/Tests/VizDAWAppModelTests/ScaleTimeTests.swift`:

```swift
import AppKit
import Testing
import VizDAWCore
@testable import VizDAWAppModel
@testable import VizDAWEditors

private let q: Int64 = 15_360

@MainActor
@Suite struct ScaleTimeTests {
    @Test func theDragScalesAroundTheEarliestStart() throws {
        let d = try #require(ScaleTimeDrag(notes: [noteRow(1, tick: q, duration: q), noteRow(2, tick: 2 * q, duration: q)]))
        #expect(d.anchor == q && d.span == 2 * q)
        var off = Snap()
        off.enabled = false
        #expect(
            d.command(rawTicks: 2 * q, snap: off, meter: [])
                == .selectAndModify(
                    rule: nil, mode: .select, modify: .scaleTime(num: 2, den: 1, anchor: q), toCopy: false))
        #expect(d.command(rawTicks: 0, snap: off, meter: []) == nil, "no change, no step")
        #expect(
            d.command(rawTicks: -100 * q, snap: off, meter: [])
                == .selectAndModify(
                    rule: nil, mode: .select, modify: .scaleTime(num: 1, den: UInt32(2 * q), anchor: q), toCopy: false),
            "never shorter than a tick")
        #expect(ScaleTimeDrag(notes: []) == nil)
    }

    @Test func aCommandDragOfANoteEndScalesTheSelectedNotes() throws {
        let f = rollFake()
        let m = try DocumentModel(session: f)
        m.editor.openDetail(.lane(seq: 3, lane: 31), kind: .pianoRoll)
        let v = PianoRollView(model: PianoRollModel.pane(m))
        v.frame = NSRect(x: 0, y: 0, width: 800, height: 400)
        v.layout()
        m.editor.setSelection(SelectionRow(timeRange: nil, lanes: [LaneRef(seq: 3, lane: 31)], events: [1, 2]))
        let n = v.laneNotes().flatMap(\.notes).first { $0.id == 1 }!
        let r = PianoRollGeometry.rect(n, time: v.axis, pitch: v.pitchAxis)
        let end = CGPoint(x: r.maxX - 2, y: r.midY)
        // Notes 1 (0…q) and 2 (q…2q): +1 quarter of drag makes the span 3q.
        let right = CGPoint(x: end.x + 160, y: end.y)
        v.contentMouseDown(end, clicks: 1, modifiers: [.command])
        v.contentMouseDragged(right, modifiers: [.command])
        v.contentMouseUp(right, modifiers: [.command])
        #expect(
            f.gestureEdits.last
                == .selectAndModify(rule: nil, mode: .select, modify: .scaleTime(num: 3, den: 2, anchor: 0), toCopy: false))
        #expect(f.calls.contains("begin Scale Time"))
    }

    @Test func exactModeOpensScaleTimeInThePanel() throws {
        let cmd: EditCommand = .selectAndModify(
            rule: nil, mode: .select, modify: .scaleTime(num: 3, den: 2, anchor: 0), toCopy: false)
        let r = try #require(ExactMode.request(for: cmd, lanes: [LaneKey(3, 31)], origin: .notes))
        #expect(r.task == .change && r.config.modify.op == .scaleTime(num: 3, den: 2, anchor: 0))
        #expect(r.events == nil, "it acts on the selection (the dragged notes)")
        #expect(ExactMode.request(for: cmd, lanes: [], origin: .blocks) == nil)
    }
}
```

- [ ] **Step 2: Run them to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh test --filter "LinePanelTests|ScaleTimeTests" 2>&1 | tail -5
```

Expected: compile errors (`value of type 'SelectModifyModel' has no member 'setLineTarget'`, `cannot find 'ScaleTimeDrag'`).

- [ ] **Step 3: Teach the panel model lines and automation targets**

In `app/Sources/VizDAWAppModel/SelectModifyModel.swift`, Find:

```swift
        invalid.remove("offsets")
        config.modify.op = .quantize(grid: g, strength: st, offsets: o)
    }
}
```

Replace:

```swift
        invalid.remove("offsets")
        config.modify.op = .quantize(grid: g, strength: st, offsets: o)
    }

    // MARK: Strip Chart lines and automation targets (Plan 4d Ruling 24)

    @ObservationIgnored private var targetsCache: (version: UInt64, names: StripNames, menus: [StripMenu])?

    private func targets() -> (names: StripNames, menus: [StripMenu]) {
        guard let editor else { return (StripNames(), []) }
        if let c = targetsCache, c.version == editor.snapshotVersion { return (c.names, c.menus) }
        let t = AutomationTargets.load(editor.session)
        targetsCache = (editor.snapshotVersion, t.names, t.menus)
        return t
    }

    /// Target names for the pickers.
    public var automationNames: StripNames { targets().names }
    /// The automation targets, grouped as the Strip Chart's row menu groups them.
    public var automationMenus: [StripMenu] { targets().menus }

    public func setConditionTarget(_ t: AutoTarget, at i: Int) {
        guard config.select.conditions.indices.contains(i) else { return }
        config.select.conditions[i].test = .automation(target: t.row)
    }

    public func setLineTarget(_ lane: StripLane) {
        guard case let .line(_, op, l) = config.modify.op else { return }
        let keep = op != .legato || lane == .duration
        config.modify.op = .line(target: lane.lineTarget, op: keep ? op : .set, line: l)
        drafts = [:]
        invalid = []
    }

    public func setLineOp(_ op: LineOpRow) {
        guard case let .line(t, _, l) = config.modify.op else { return }
        let lengths = t == .value(target: .duration)
        config.modify.op = .line(target: t, op: op == .legato && !lengths ? .set : op, line: l)
        drafts = [:]
        invalid = []
    }

    /// A new shape with its defaults; Free becomes a two-point line from
    /// Start to End (a drawn line keeps its points).
    public func setLineShape(_ k: ShapeKind) {
        guard case let .line(t, op, l) = config.modify.op else { return }
        var line = l
        switch k {
        case .free:
            if case .free = l.shape { return }
            line.shape = .free(points: [
                LinePointRow(tick: l.from, value: l.start), LinePointRow(tick: l.to, value: l.end),
            ])
        default:
            var s = StripChartSettings()
            s.shape = k
            line.shape = s.shapeRow(seed: nextSeed())
        }
        config.modify.op = .line(target: t, op: op, line: line)
        drafts = [:]
        invalid = []
    }

    public func setLineCurve(_ c: CurveRow) {
        guard case .line(let t, let op, var l) = config.modify.op, case .curved = l.shape else { return }
        l.shape = .curved(curve: c)
        config.modify.op = .line(target: t, op: op, line: l)
    }

    public func setLineWave(_ w: WaveRow) {
        guard case .line(let t, let op, var l) = config.modify.op, case let .oscillator(_, p, ph) = l.shape else {
            return
        }
        l.shape = .oscillator(wave: w, period: p, phase: ph)
        config.modify.op = .line(target: t, op: op, line: l)
    }

    public func setLinePeriodKind(_ k: PeriodKind) {
        guard case .line(let t, let op, var l) = config.modify.op, case let .oscillator(w, _, ph) = l.shape else {
            return
        }
        var s = StripChartSettings()
        s.period = k
        l.shape = .oscillator(wave: w, period: s.periodRow, phase: ph)
        config.modify.op = .line(target: t, op: op, line: l)
        drafts = [:]
        invalid = []
    }
}

extension ShapeKind {
    /// The kind of a line's shape (the panel's picker).
    public init(_ s: LineShapeRow) {
        switch s {
        case .straight: self = .straight
        case .curved: self = .curved
        case .flat: self = .flat
        case .free: self = .free
        case .random: self = .random
        case .oscillator: self = .oscillator
        }
    }
}
```

In `app/Sources/VizDAWAppModel/ExactMode.swift`, Find:

```swift
        if origin == .stripChart { return lineRequest(cmd, lanes: sorted) }
```

Replace:

```swift
        if origin == .stripChart { return lineRequest(cmd, lanes: sorted) }
        // The piano roll's scale-time drag acts on the selection (Plan 4d
        // Ruling 23).
        if origin == .notes, case let .selectAndModify(nil, _, .scaleTime(num, den, anchor)?, _) = cmd {
            var c = SelectModifyConfig.preset(.change)
            c.modify.op = .scaleTime(num: num, den: den, anchor: anchor)
            return ModifyRequest(task: .change, config: c, events: nil, lanes: sorted, range: nil)
        }
```

- [ ] **Step 4: Write the scale-time drag and wire it into the piano roll**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Sources/VizDAWEditors/ScaleTimeDrag.swift <<'VIZDAW_EOF'
import Foundation
import VizDAWAppModel
import VizDAWCore

/// The piano roll's scale-time drag (parent spec §9 exact mode, Plan 4d
/// Ruling 23): ⌘-dragging a selected note's end scales the selected
/// notes' times and lengths around their earliest start, by new span /
/// old span.
public struct ScaleTimeDrag: Equatable, Sendable {
    public static let label = "Scale Time"

    /// The earliest start.
    public let anchor: Int64
    /// From the earliest start to the latest end (≥ 1).
    public let span: Int64

    public init?(notes: [EventRow]) {
        guard let a = notes.map(\.tick).min(), let e = notes.map({ $0.tick + max($0.duration, 0) }).max(), e > a
        else { return nil }
        anchor = a
        span = e - a
    }

    /// The edit for a drag of `rawTicks`: the latest end moves (snapped
    /// when snap is on), never closer than a tick to the anchor; nil when
    /// nothing changes.
    public func command(rawTicks: Int64, snap: Snap, meter: [MeterEntryRow]) -> EditCommand? {
        let end = NoteDrag.clampTick(anchor + span + NoteDrag.clampTick(rawTicks))
        let moved = snap.enabled ? snap.apply(end, meter: meter) : end
        let newSpan = max(moved - anchor, 1)
        guard newSpan != span else { return nil }
        let g = Self.gcd(newSpan, span)
        return .selectAndModify(
            rule: nil, mode: .select,
            modify: .scaleTime(num: UInt32(clamping: newSpan / g), den: UInt32(clamping: span / g), anchor: anchor),
            toCopy: false)
    }

    static func gcd(_ a: Int64, _ b: Int64) -> Int64 {
        var (x, y) = (abs(a), abs(b))
        while y != 0 { (x, y) = (y, x % y) }
        return max(x, 1)
    }
}
VIZDAW_EOF
```

In `app/Sources/VizDAWEditors/PianoRollView.swift`, Find:

```swift
        case erase([UInt64], last: CGPoint)
        case band(from: CGPoint, adding: Bool)
    }
```

Replace:

```swift
        case erase([UInt64], last: CGPoint)
        case band(from: CGPoint, adding: Bool)
        /// A ⌘-drag of a note's end (Plan 4d Ruling 23).
        case scale(ScaleTimeDrag, startTick: Int64)
    }
```

Find:

```swift
            let touched = Set(lanes.filter { l in l.notes.contains { idSet.contains($0.id) } }.map(\.lane))
            guard openGesture(d.label, lanes: touched, origin: .notes) else { return }
            drag = .notes(d, startTick: axis.tick(atX: from.x), startY: from.y)
```

Replace:

```swift
            let touched = Set(lanes.filter { l in l.notes.contains { idSet.contains($0.id) } }.map(\.lane))
            // ⌘ on the end zone scales the dragged notes' time.
            if modifiers.contains(.command), n.zone == .end,
                let sd = ScaleTimeDrag(notes: lanes.flatMap(\.notes).filter { idSet.contains($0.id) })
            {
                guard openGesture(ScaleTimeDrag.label, lanes: touched, origin: .notes) else { return }
                drag = .scale(sd, startTick: axis.tick(atX: from.x))
                return
            }
            guard openGesture(d.label, lanes: touched, origin: .notes) else { return }
            drag = .notes(d, startTick: axis.tick(atX: from.x), startY: from.y)
```

Find:

```swift
        case .band:
            return nil
        }
    }
```

Replace:

```swift
        case let .scale(sd, startTick):
            return sd.command(rawTicks: axis.tick(atX: p.x) - startTick, snap: snap, meter: meter)
        case .band:
            return nil
        }
    }
```

Find:

```swift
        case .pencil:
            editor.updateGesture(target(d, at: p))
```

Replace:

```swift
        case .pencil, .scale:
            editor.updateGesture(target(d, at: p))
```

- [ ] **Step 5: Show the line and the target in the panel**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Sources/VizDAW/LineModifyEditor.swift <<'VIZDAW_EOF'
import SwiftUI
import VizDAWAppModel
import VizDAWCore

/// The panel's Strip Chart Line (Plan 4d Ruling 24): target, operation and
/// shape pickers; the numbers are the panel's fields.
struct LineModifyEditor: View {
    let model: SelectModifyModel
    let target: LineTargetRow
    let op: LineOpRow
    let line: LineRow

    static let ops: [(String, LineOpRow)] = [
        ("Draw", .draw), ("Set", .set), ("Scale", .scale), ("Add", .add), ("Max Limit", .limitMax),
        ("Min Limit", .limitMin), ("Compress", .compress), ("Thin", .thin), ("Thicken", .thicken), ("Legato", .legato),
    ]
    static let basics: [StripLane] = [.velocity, .releaseVelocity, .duration, .pitchBend, .channelPressure, .program, .tempo]

    private var targetTitle: String {
        StripLane(target).map { $0.title(model.automationNames) } ?? "Any Value"
    }

    var body: some View {
        Menu(targetTitle) {
            ForEach(Self.basics, id: \.self) { lane in
                Button(lane.title(model.automationNames)) { model.setLineTarget(lane) }
            }
            Menu("Controller") {
                ForEach(0..<128, id: \.self) { i in
                    Button(StripLane.controller(UInt8(i)).title(model.automationNames)) {
                        model.setLineTarget(.controller(UInt8(i)))
                    }
                }
            }
            Menu("Automation") {
                ForEach(model.automationMenus, id: \.title) { g in
                    Menu(g.title) {
                        ForEach(g.lanes, id: \.self) { lane in
                            Button(lane.title(model.automationNames)) { model.setLineTarget(lane) }
                        }
                    }
                }
            }
        }
        .frame(width: 210)
        .accessibilityLabel("Line Target")
        Picker("", selection: Binding(get: { op }, set: { model.setLineOp($0) })) {
            ForEach(Self.ops, id: \.0) { Text($0.0).tag($0.1) }
        }
        .labelsHidden()
        .frame(width: 110)
        .accessibilityLabel("Operation")
        Picker("", selection: Binding(get: { ShapeKind(line.shape) }, set: { model.setLineShape($0) })) {
            ForEach(ShapeKind.allCases, id: \.self) { Text($0.title).tag($0) }
        }
        .labelsHidden()
        .frame(width: 110)
        .accessibilityLabel("Shape")
        switch line.shape {
        case .curved(let curve):
            Picker("", selection: Binding(get: { curve }, set: { model.setLineCurve($0) })) {
                ForEach(CurveKind.allCases, id: \.self) { Text($0.title).tag($0.row) }
            }
            .labelsHidden()
            .frame(width: 100)
            .accessibilityLabel("Curve")
        case let .oscillator(wave, period, _):
            Picker("", selection: Binding(get: { wave }, set: { model.setLineWave($0) })) {
                ForEach(WaveKind.allCases, id: \.self) { Text($0.title).tag($0.row) }
            }
            .labelsHidden()
            .frame(width: 90)
            .accessibilityLabel("Wave")
            Picker("", selection: Binding(get: { Self.kind(period) }, set: { model.setLinePeriodKind($0) })) {
                ForEach(PeriodKind.allCases, id: \.self) { Text($0.title).tag($0) }
            }
            .labelsHidden()
            .frame(width: 150)
            .accessibilityLabel("Period Kind")
        case .free(let points):
            Text("Drawn line (\(points.count) points)")
                .foregroundStyle(.secondary)
        default:
            EmptyView()
        }
    }

    static func kind(_ p: PeriodRow) -> PeriodKind {
        switch p {
        case .ticks: .musical
        case .millis: .clock
        case .bars: .bars
        }
    }
}

/// An Automation Target condition's target.
struct AutomationTargetPicker: View {
    let model: SelectModifyModel
    let selection: AutoTarget
    let choose: (AutoTarget) -> Void

    var body: some View {
        Menu(model.automationNames.title(selection)) {
            ForEach(model.automationMenus, id: \.title) { g in
                Menu(g.title) {
                    ForEach(g.lanes, id: \.self) { lane in
                        if case .automation(let t) = lane {
                            Button(model.automationNames.title(t)) { choose(t) }
                        }
                    }
                }
            }
        }
        .frame(width: 220)
        .accessibilityLabel("Automation Target")
    }
}
VIZDAW_EOF
```

In `app/Sources/VizDAW/ModifyPanel.swift`, Find:

```swift
                case .dest(let dest):
                    DestPicker(destinations: model.destinations, selection: dest) { model.setConditionDest($0, at: index) }
```

Replace:

```swift
                case .dest(let dest):
                    DestPicker(destinations: model.destinations, selection: dest) { model.setConditionDest($0, at: index) }
                case .automation(let target):
                    AutomationTargetPicker(model: model, selection: AutoTarget(target)) {
                        model.setConditionTarget($0, at: index)
                    }
```

Find:

```swift
        case .setDest(let dest):
            DestPicker(destinations: model.destinations, selection: dest) { model.setModifyDest($0) }
```

Replace:

```swift
        case let .line(target, lineOp, line):
            LineModifyEditor(model: model, target: target, op: lineOp, line: line)
        case .setDest(let dest):
            DestPicker(destinations: model.destinations, selection: dest) { model.setModifyDest($0) }
```

- [ ] **Step 6: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh test --filter "LinePanelTests|ScaleTimeTests|PianoRollViewTests|ExactGestureTests|SelectModify" 2>&1 | tail -5
```

Expected: PASS. (In the piano roll test the selected notes span 0…2q; dragging the end a quarter right snaps the new end to 3q: 3q / 2q = 3/2 around 0.)

- [ ] **Step 7: Gate and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh build 2>&1 | tail -3 && scripts/swift.sh test 2>&1 | tail -3
git add app/Sources/VizDAW/LineModifyEditor.swift app/Sources/VizDAWEditors/ScaleTimeDrag.swift app/Sources/VizDAWAppModel/SelectModifyModel.swift app/Sources/VizDAWAppModel/ExactMode.swift app/Sources/VizDAW/ModifyPanel.swift app/Sources/VizDAWEditors/PianoRollView.swift app/Tests/VizDAWAppModelTests/LinePanelTests.swift app/Tests/VizDAWAppModelTests/ScaleTimeTests.swift
git commit -m "feat(app): Strip Chart lines and automation targets in the panel; the scale-time drag

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
```

Expected: Swift 524 (518 + 6).

---

### Task 16: Manual checklist for the Strip Chart and automation; the whole-branch gate

**Files:**
- Modify: `app/README.md`

**Interfaces:**
- Consumes: everything above.
- Produces: checklist steps 68–80 under "### Strip Chart and automation (plan 4d)".

Spec §6: a manual checklist per phase (drag feel, playhead smoothness, tear-off windows, VoiceOver).

- [ ] **Step 1: Append the checklist**

Append to `app/README.md` (after step 67):

```markdown

### Strip Chart and automation (plan 4d)

Use the demo project (File > Create Demo Project) unless a step says
otherwise. Its built-in synth plays through the "Built-in Synth" strip; Song
holds Groove and Chords. Turn snap on (the default, quarters) unless a step
says otherwise. "The level" means what you hear and the master meter.

68. Strip Chart: double-click the Chords block and press Cmd-3: the detail
    pane shows "Strip Chart" with one row titled "Velocity ▾" and a bar
    (Mode: Draw, Modify, Erase; Shape; Step; Rows: 1). The chords' 36
    velocities show as bars at their starts; selected notes are accented.
    Rows + makes a second row "Controller 7 (Volume) ▾" showing the
    Controller 7 = 100 event at 1.1.0 as a held step.
69. Draw: in the velocity row drag from bar 1 near the bottom to bar 4 near
    the top: the bars follow the pointer while dragging and the notes of bars
    1–4 rise in a straight line; Edit > Undo "Change Velocity" restores them
    in one step. A drag from right to left works the same way. A single
    click sets the one note at that grid line. A drag that leaves the row
    above or below pins its end at the top or bottom.
70. Shapes: in the controller row choose Shape Curved (Ease In), Flat,
    Random, Free and Oscillator (Sine, Musical 1/4; then Clock 250 ms; then
    Bars 1 with a meter change added in the list) and draw over bars 5–8
    each time: the steps follow the shape; Free follows the pointer; an
    Oscillator's period stays a quarter, 250 ms, or one bar whatever the
    bar's length; Step 1/64 gives a denser line than 1/8. Each drawing
    replaces the controller events in its range ("Draw Controllers").
71. Modify: Mode Modify, Operation Scale: drawing a flat line at the top
    over bars 1–4 doubles those velocities, at the middle leaves them,
    at the bottom zeroes them; Add at the middle changes nothing; Max Limit
    and Min Limit clip; Compress drawn as a rectangle maps the lowest value
    in its range to its bottom and the highest to its top; in a Length row,
    Legato makes each note end at the next; in the controller row Thin
    (Step 1/4) leaves one event per quarter and Thicken fills between.
    Option at the press limits the change to the selected notes.
72. Erase: Mode Erase in the controller row deletes the controller events
    under the drag; in the velocity row the status says "Erase works on
    controller, program and automation rows." and nothing changes. Escape
    during any drag leaves no undo step.
73. Automation: add a lane "Auto" to Song (lane header menu), select it,
    press Cmd-3; on the row title choose Automation > Built-in Synth >
    Fader · Built-in Synth; draw a straight line from bar 1 at the bottom to
    bar 5 at about two thirds up. Play from bar 1: the chords fade in
    smoothly (no zipper noise, no clicks), reaching about unity at bar 5;
    the list (Cmd-2 on Auto) shows two Automation rows "Fader · strip …"
    with "0.0%" and about "63%".
74. Stop and move: stop in the middle of the fade: the level stays where it
    stopped; start an auditioned note (piano keyboard) to hear it. Move the
    mixer's Built-in Synth fader: the level follows the fader. Play again
    from bar 1: automation takes over at once. While playing, moving the
    fader does not hold (automation wins). Delete the Auto lane's points and
    play: the fader's own value plays.
75. Mute, solo, pan and sends: add rows Mute · Built-in Synth and Pan ·
    Built-in Synth to Auto's Strip Chart; draw mute on for bar 3 only (top
    half), pan from left to right over bars 1–4: playback mutes exactly for
    bar 3 (the mute row shows a held block) and pans across; with a bus and
    a send (mixer), Send 1 automation changes the bus level; Solo on a bus
    silences what does not feed it.
76. Plugin parameters: add Apple's AUDelay (or the CLAP test gain if
    installed) as an insert on Built-in Synth; Automation lists it with its
    automatable parameters; draw Dry/Wet Mix (or Gain) from 0 to full over
    bars 1–4 and play: the effect follows; the plugin's own window moves its
    control while playing; no "Set Plugin Parameters" undo steps appear.
    Stop: the plugin keeps the last value.
77. Velocity scale and tempo: a row Automation > Velocity Scale > (the
    chords' destination) drawn low over bars 1–4 makes those chords softer
    on playback (the notes' own velocities in the list do not change). A
    Tempo row drawn as a straight line from 120 down to 60 over bars 5–8 gives
    a smooth slow-down; the list shows two tempo rows (the first "ramp"),
    the transport's BPM falls steadily, and the ruler's bar lines spread.
78. Panel and exact mode: Modify > Change Values… (Ctrl-Cmd-M), Operation
    Strip Chart Line: target, operation and shape pickers and the From, To,
    Start, End and Step fields edit the line numerically; an Automation
    Target condition lists the targets; Apply acts on the selection (one undo
    step); Modify Again repeats it. With Exact Mode on (Ctrl-Cmd-E) a Strip
    Chart drag previews, goes back on release, and opens the panel with the
    same line and a Position condition; Apply then makes it.
79. Scale time: in the piano roll select two chords and Command-drag the end
    of the last selected note one beat right: both chords stretch around the
    first one's start ("Scale Time", one undo step); with Exact Mode on the
    panel opens on Scale Time with the ratio filled in.
80. Windows and VoiceOver: Option-Cmd-3 opens the Strip Chart in its own
    window with the same rows; changing rows in one does not change the
    other; Save, close and reopen the project: rows, bar settings and the
    window come back. With VoiceOver (Cmd-F5) the chart is a group "Strip
    Chart" whose values read like "Velocity 56, bar 1 beat 1" or "Fader ·
    Built-in Synth -6.0 dB, bar 3 beat 1"; the bar's controls read their
    labels (Mode, Operation, Shape, Curve, Wave, Period Kind, Period,
    Phase, Step, Rows).
```

- [ ] **Step 2: Run everything**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace 2>&1 | grep -E "^test result|FAILED|panicked" | tail -80
cargo test --workspace -- --list 2>/dev/null | grep -c ': test$'
scripts/build-ffi.sh && scripts/swift.sh build 2>&1 | tail -3 && scripts/swift.sh test 2>&1 | tail -3
git status --short
```

Expected: every suite passes; 718 Rust tests listed; Swift 524; `git status` shows only `app/README.md` (generated files are ignored).

- [ ] **Step 3: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add app/README.md
git commit -m "docs(app): manual checklist for the Strip Chart and automation

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
```

---

## Self-Review

1. **Spec coverage (§4d, parent §9 Strip Chart, §10):**
   - `EventBody::Automation { target, value }` with the six targets: Task 2 (plus velocity scale, Ruling 2); file compatibility: Task 2 (appended variant, round trip, old projects unchanged).
   - Ramps linear, mute/solo/stepped params jump: Task 6 (`Automation::value`, `mark_stepped`), Task 8 (`a_mute_point_switches_at_its_frame_inside_a_block`).
   - Engine applies automation per block with sub-block splits, minimum 32 frames, to mixer controls and as plugin parameter changes: Tasks 6–9 (`dense_points_never_split_runs_below_32_frames`, `a_fader_ramp_follows_the_lane_between_points`, `a_parameter_ramp_reaches_a_clap_gain_insert`); allocation-free, lock-free, panic-free: `// RT` code only uses preallocated tables and `get`, `rt_alloc` phases in Tasks 8 and 9.
   - Overrides while playing; stop keeps the last value until the user moves it: Tasks 7–8 (Ruling 13 tests).
   - Coexistence with the parameter overlay and gestures: Task 9 (merge after queued values; no gestures from automation; AU writes tracked).
   - Strip Chart lanes for velocity, release velocity, duration, CC, pitch bend, pressure, program, tempo, velocity scale and automation targets: Tasks 10, 12, 13 (row menu).
   - Draw and modify modes; shapes straight/free/curved/flat/random/oscillator with musical, clock or meter-following periods: Tasks 3, 11, 13, 14.
   - Compress rectangle; set, scale, add, limit max/min, legato, thin, thicken; the selected-notes modifier: Tasks 4, 11, 12, 13.
   - Tempo ramps sound linear: Task 5 (`a_straight_tempo_line_is_one_linear_ramp`, closed form).
   - Every operation numerically in the select-and-modify panel: Tasks 4, 5, 15 (`ModifyRow::Line`, fields, pickers); exact mode for Strip Chart lines and scale time: Tasks 11, 12, 15.
   - Spec §6 tests: FFI round trip of every kind (Task 2), edit mapping applies and undoes (Tasks 4–5), golden renders for a fader ramp and a plugin-parameter ramp with the test plugin (Tasks 8, 9), proptest extended (Task 2); manual checklist (Task 16).
   - Carried follow-ups: Task 1.
2. **Placeholder scan:** every code step holds complete code; no "TBD", "similar to", or unnamed handling. Two steps give an explicit fallback for a toolchain detail (`#[uniffi(default)]`, a `ForEach` id type), each with the exact alternative code.
3. **Type consistency:** `LineChange { target, op, line }`, `LineTarget::{Value, Series, Tempo}`, `LineOp` (10 cases) match `LineTargetRow`, `LineOpRow` (Tasks 4–5) and Swift `lineTarget`/`StripOp.row` (Tasks 10–11); `AutoDest`/`StripParam` (Task 6) are used by `Controls` (Task 7) and the renderer (Task 8); `automate(frame: u32, param: ParamId, normalized: f64)` is the same on `EffectNode`, `InstrumentNode` and the adapters (Tasks 8–9); `StripPoint`, `StripStroke`, `StripChartModel.stroke*` (Tasks 11–13) agree; `DetailState.stripRows` (Task 12) is read by the model and the windows; `events::automation_target_row`/`automation_target_of` (Task 2) are used by `core-ffi/src/strip.rs` (Task 4).
4. **Review Focus:** each of the five lines has its named test in its owning task (Tasks 4, 6, 7, 8, 10–13).

Running counts: Rust tests listed 672 → 675 (T1) → 681 (T2) → 687 (T3) → 696 (T4) → 701 (T5) → 705 (T6) → 709 (T7) → 716 (T8) → 718 (T9); Swift `@Test` functions 484 → 486 (T2) → 487 (T4) → 488 (T5) → 493 (T10) → 501 (T11) → 511 (T12) → 517 (T13) → 518 (T14) → 524 (T15).

