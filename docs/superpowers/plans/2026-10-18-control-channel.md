# VizDAW Control Channel Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** An off-by-default agent control channel — a private Unix socket speaking JSON lines (`--control PATH` / `VIZDAW_CONTROL`) — through which a shell tool, `vizctl`, launches VizDAW in a throwaway home, runs every menu command by a stable ID without dialogs, reads transport, document, editor, selection and window state, reads the accessibility tree with element IDs, clicks, drags and types into VizDAW's own windows without moving the pointer or needing focus, takes app-rendered PNG screenshots and waits on conditions; fuzzed for hostile requests and damaged project files, and driven end to end on CI.

**Architecture:** Rust first. A new crate `core-control` owns the wire: request reading and limits, validation of every command's parameters, the menu-command registry (IDs, shortcuts, the dialogs their parameters answer), wait conditions, and the socket server (accept thread, one thread per connection, a call queue with timeouts) plus a client. `core-ffi/src/control.rs` exposes a `ControlServer` object and typed request/result enums through UniFFI and turns results into JSON, so Swift never builds JSON. Swift runs calls on the main thread from a 20 Hz pump: `VizDAWAppModel/ControlCommands` (registry handlers, state rows, opening without dialogs), `VizDAWEditors/ControlUI` (accessibility tree, element IDs, events posted to the app's own queue) and `WindowSnapshot` (layer render plus a redraw of self-drawing views), wired by `VizDAW/ControlChannel`. A new crate `vizctl` is the CLI. Task 1 carries the plan 4d follow-ups.

**Tech Stack:** Rust 2024 (`core-control`, `vizctl`, `core-ffi` with UniFFI 0.32.2, `serde_json`, `proptest`, `tempfile`, std Unix sockets), the in-repo CLAP test plugins, Swift 6 / SwiftPM (AppKit, SwiftUI, Core Animation, Swift Testing), macOS 14+.

**Spec:** `docs/superpowers/specs/2026-10-18-control-channel-design.md` (all of it; §3 is the approved design and binding). Parent `docs/superpowers/specs/2026-10-15-vizdaw-design.md` §4 invariants, §10 faults, §12 testing; `docs/superpowers/specs/2026-10-16-editors-design.md` §3.5 accessibility and §3.8 Busy. Previous plan for conventions: `docs/superpowers/plans/2026-10-17-editors-strip-chart.md`.

## Global Constraints

- Every shell command runs after `export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools` (the Xcode license is not accepted, so `git`, `cc`, `swift` and the linker work only with the Command Line Tools). Every bash block below starts with that line and `cd /Users/lattawb/workspace/vizdaw`.
- Work on branch `sp4-editors` (code head `e59ca95` plus the spec and this plan's commits). Commit at the end of every task, adding only the files the task lists (and `Cargo.lock` when a manifest changed); never push. End every commit message with the attribution lines your harness requires (the blocks below show `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>` and the `Claude-Session:` line).
- Never use GUI or computer-use tools. Never touch `~/workspace/SVP/research`. Launch VizDAW only through `vizctl launch` or the e2e test (a throwaway home under `/tmp`), and quit it when done (`vizctl quit`; `pkill -f "VizDAW --control"` if a test left one).
- Rust stable, edition 2024; manifests keep `edition.workspace = true`, `license.workspace = true`, `rust-version.workspace = true`, `publish = false`. License GPL-3.0-only. New crates: `core-control` and `vizctl` only; new dependencies only those already in `Cargo.lock` (`serde_json`, `proptest`, `tempfile`).
- `core-control` and `vizctl`: `#![forbid(unsafe_code)]`, `#![deny(missing_docs)]`, and `#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic))]`; Unix-portable (the Ubuntu CI job builds and tests them). `core-model`, `core-engine`, `core-host`, `core-ffi` keep their lints. Tests may `unwrap` and index.
- Real-time rules (parent spec §10): code reachable from `Renderer::process` stays allocation-, lock- and panic-free; the counter Task 1 adds is a relaxed atomic.
- No panic crosses the FFI: every exported function that can fail returns `Result<_, VizError>` inside `ffi_guard`. `FakeSession` is unchanged (no `Session` method is added).
- UniFFI: new records and enums only; Swift sees `ControlErrorCode.internalFailure` for the wire code `internal` (`internal` is a Swift keyword).
- Swift: the generated bindings compile in Swift 5 mode; `VizDAWAppModel`, `VizDAWEditors`, `VizDAW` and the tests in Swift 6 mode with no new warnings. No `@State`/`@FocusState` in new views. Models and pure logic are tested with Swift Testing; AppKit window code through off-screen `NSWindow`s.
- Spec values used verbatim: line limit 1 MiB; nesting 32; string id 128 characters; strings 4096 characters, paths 1024 bytes; coordinates ±100 000 points; 4 connections; idle 300 s; write 30 s; app answer 60 s; wait 1–600 000 ms, default 30 000, server grace 15 s; tree depth 64, 5000 nodes; drag steps 1–200, default 16; text 1–256 characters; socket path ≤ 103 bytes; directory 0700; socket 0600; pump every 50 ms, up to 8 calls per tick; new-window answers within 30 s; input answers within 5 s; app-level alerts kept: 20; exit status 2 when the channel cannot start.
- Generated files are never committed: `app/Sources/VizDAWCore/vizdaw_ffi.swift`, `app/Sources/vizdaw_ffiFFI/include/`, `target/ffi/`, `dist/`. Run `scripts/build-ffi.sh` before any `scripts/swift.sh` command after a Rust change.
- Each task ends green: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `scripts/build-ffi.sh`, `scripts/swift.sh build`, `scripts/swift.sh test`. Run `cargo fmt --all` first. Baseline before Task 1: 760 Rust tests listed by `cargo test --workspace -- --list`, 551 Swift `@Test` functions.
- Code in the steps is complete for new files (written whole with `cat > … <<'VIZDAW_EOF'`). Changes to existing files are exact **Find / Replace** excerpts of the tree the previous task left; apply them with an editor (the Edit tool). If an excerpt does not match because a fix landed meanwhile, make the same change by hand and keep going. The code in this plan was built and its tests run (Rust, Swift and the end-to-end test) in a scratch worktree of `e59ca95` while the plan was written.

## Review Focus

The five failure modes a user (here: an agent or a reviewer driving VizDAW) is most likely to hit that the spec implies but no requirement spells out, each pinned by a named test in its owning task:

1. **VizDAW is not the active app.** macOS 14+ never lets a background app take focus, so its windows are never key: a synthesized click on a canvas is spent on "activation", a menu shortcut finds its menu item disabled, and an occluded window's layers stop updating. Expected: clicks land, Space plays, screenshots show the current editor. Pinned: Task 7 `clicksAndDragsLandWhereTheyAreAimed`, `keyStrokesReachTheFirstResponder`; Task 3 `shortcuts_name_their_menu_commands`; Task 8 `aStaleLayerShowsWhatItsViewDrawsNow`; Task 11 `click_play_and_press_space`.
2. **A client that misbehaves** — garbage, a 1 MiB+ line, deep nesting, five connections, a client that never reads or disconnects mid-call, a call the app never answers (a blocked main thread). Expected: every line gets a reply, other clients are unaffected, nothing hangs past its timeout, the app never crashes. Pinned: Task 2 `fuzz::*`; Task 4 `garbage_and_overlong_lines_get_errors_and_the_connection_survives`, `a_call_the_app_never_answers_times_out_and_the_late_answer_is_dropped`, `too_many_connections_are_turned_away`.
3. **An element ID that went stale** (the tree changed between `ui.tree` and `ui.click`), or a flipped canvas whose accessibility frames are mirrored. Expected: `no_element`, never a click on the wrong thing; frames where the items are drawn. Pinned: Task 7 `theTreeListsTheCanvasItemsWithCheckedPaths`; Task 5 `tree_nodes_nest_by_path_and_carry_checked_ids`.
4. **A programmatic call that would open a dialog** (Save of an untitled document, opening a project after a crash, a store alert, a launch-time recovery question) — expected: `dialog_required` naming the dialog or `failed` with the alert's text, and no modal window. Pinned: Task 3 `the_registry_has_unique_ids_and_answers_dialogs_by_parameter`; Task 6 `saveNeedsAPathUntilTheDocumentHasOne`, `openAnswersRecoveryQuestionsFromParameters`, `storeCommandsAnswerWithTheNewDocument`, `busyAndFailedPlaysAreErrorsNotAlerts`; Task 11 `demo_play_screenshot_stop`.
5. **A damaged project** — a byte flipped, a value turned into an extreme number, a field removed — opened through the channel. Expected: refused with an error or loaded and readable; never a panic or `internal` (the harness found event ticks near `i64::MIN` overflowing the block layout). Pinned: Task 11 `an_event_at_the_far_end_of_time_is_refused`, `json_edits_never_crash_and_many_still_load`, `positions_and_lengths_beyond_the_limit_are_refused`.

## Plan Rulings

Decisions the spec leaves open, each implemented and tested as stated (format: Ruling: what — why — cost if wrong):

1. **Ruling:** Task 1 carries the 4d follow-ups: the CLAP test plugins gain a kind `vizdaw.test.bank` (pass-through, 24 automatable parameters, ids 100–123) and `rt_alloc_plugins` puts it on the built-in synth with 24 ramping automation lanes; `core_host::adapt::automation_collapses()` (a relaxed atomic counter) proves the full-queue collapse branch ran under the allocation guard. `a_stop_during_the_rebuild_drain_is_not_lost` waits for `!playing` with the existing 5 s condition wait instead of `ms(60)` — why: the gain plugin has only three parameters, too few to fill the 256-entry queue — cost if wrong: a seventh test plugin in scans (one count updated).
2. **Ruling:** The wire lives in a new platform-neutral crate `core-control` (no UniFFI) used by `core-ffi` and `vizctl`; Swift gets typed requests and returns typed results that Rust turns into JSON — why: the schema and validation in one place, testable on Linux, and `vizctl` needs no FFI — cost if wrong: one more crate.
3. **Ruling:** Swift polls the server (`next_call`) from a 50 ms main-thread timer in the common run-loop modes, up to 8 calls per tick; no UniFFI callback interface — why: the app already runs timers this way; a callback would arrive on a socket thread — cost if wrong: up to 50 ms latency per call.
4. **Ruling:** The server answers `ping` and `commands.list` itself; `ping` is an extra command for readiness checks — cost if wrong: none.
5. **Ruling:** The target document is the `window` parameter's document, else the front-most document window's, else any open document; `window` is a window number, `"key"` or `"main"` — why: VizDAW is usually not the active app, so SwiftUI focus says nothing — cost if wrong: an agent with two documents must pass `window`.
6. **Ruling:** Element IDs are an accessibility child-index path plus an FNV-1a check of role and label (title when no label); resolved again on every use — why: stable enough within a session, and refuses stale targets — cost if wrong: an ID changes when an earlier sibling appears (read the tree again).
7. **Ruling:** Coordinates are window points with a top-left origin, title bar included, the same as screenshot pixels divided by the scale — cost if wrong: none.
8. **Ruling:** Mouse events are posted to VizDAW's own event queue (`NSApp.postEvent`); the call answers once the queue holds no posted mouse or key events (≤ 5 s); the window is ordered to the front without activating; while channel events are queued `ControlUI.firstMouse` is true and the editors' views (`EditorCanvas`, `LaneHeaderView`, `RegionStripView`, `PianoKeyboardView`, `ExpressionPaneView`) accept the first click — why: posting lets tracking loops read the gesture; a background app's first click would otherwise only try to activate — cost if wrong: a user clicking a background VizDAW window while an agent drives it also gets a first click.
9. **Ruling:** A key stroke that is a registry shortcut runs that command on the window's document; when that command is `unavailable` (or the stroke is no shortcut) the key events go to the window with `NSWindow.sendEvent` (its first responder); a `ui.key` `element` is clicked first — why: menu shortcuts of a never-active app are disabled — cost if wrong: a shortcut a view handles before the menu (none today) would run the menu command instead.
10. **Ruling:** The first `ui.tree` or element lookup sets AppKit's enhanced-user-interface attribute (`AXEnhancedUserInterface`) on the application, as VoiceOver does — why: SwiftUI otherwise reports no children (found while planning) — cost if wrong: AppKit's assistive mode stays on for the rest of a control session.
11. **Ruling:** Screenshots render the frame view's layer tree, then draw every self-drawing view again in place with its own sublayers on top (spec §9) — why: AppKit does not redraw occluded windows, so layers hold stale pictures (found while planning: the arrangement was empty) — cost if wrong: redrawn editors paint over the floating sidebar.
12. **Ruling:** The canvases' `CanvasElement` frames are converted to the bottom-left parent space `NSAccessibilityElement` expects; they were mirrored top to bottom in flipped canvases (VoiceOver outlines too) — found while planning — cost if wrong: none (a fix).
13. **Ruling:** Waits are evaluated in Swift every tick with Rust's `control_wait_met`; the deadline is kept in Swift; the server adds 15 s of grace — why: facts live on the main thread; the grace keeps a blocked main thread from hanging a client — cost if wrong: none.
14. **Ruling:** `app.new_demo`, `app.open` and the registry's document-making commands answer once the new document's window is on screen (≤ 30 s), opening it through the `OpenWindowAction` captured from the first document window (else the store's pending queue) — cost if wrong: a slow machine sees `timeout` after 30 s.
15. **Ruling:** While the channel is on, app-level alerts (store, catalog) and launch-time recovery questions are not shown: they go to stderr and `state.get windows` → `alerts` (newest 20); launch-time recovery questions answer Cancel / last save — why: §3 rule 5 — cost if wrong: an agent must read `alerts`.
16. **Ruling:** A failure to start the channel (unsafe directory, live socket, long path) exits the app with status 2 — why: an agent that asked for a channel must not get an app without one — cost if wrong: none.
17. **Ruling:** The damaged-file harness found event ticks near `i64::MIN` overflowing `blocks::bar_span`; the model now refuses positions and lengths beyond ±2^40 ticks (`rules::MAX_TICKS`, about 400 days at 120 BPM) and more than 2^20 reference loops (`MAX_LOOPS`) at insert and at load — why: one bound at the model's door instead of saturating arithmetic everywhere — cost if wrong: a project longer than 400 days is refused.
18. **Ruling:** cargo-fuzz targets are not added; the proptest harnesses scale with `VIZDAW_FUZZ_CASES` — cost if wrong: none.
19. **Ruling:** The e2e test is `#[ignore]`d and run explicitly with `VIZDAW_E2E_APP` on CI in the `app` job (macos-15 runners have a window server) after `scripts/make-app.sh`, one test at a time, offline audio — cost if wrong: a runner image without a window server fails that step (then gate it on `if: runner.os == 'macOS'` plus a `continue-on-error`, and record it).
20. **Ruling:** The process change (spec §12) is documented in the spec and in `app/README.md` ("Agent control channel") — cost if wrong: none.

## Spec gaps and contradictions found while planning

- §3 rule 4 says events go "into VizDAW's own windows"; macOS 14+ never activates a background app, so key windows, SwiftUI focus and menu enabling all fail. Rulings 8–9 route around it without system-wide events.
- §3 rule 4 asks for "the accessibility tree the editors already build"; SwiftUI builds its part only for assistive apps (Ruling 10), and the editors' item frames were mirrored (Ruling 12).
- §3 rule 4 asks for screenshots that "handle CA-layer content"; `CALayer.render(in:)` does, but occluded windows hold stale layers (Ruling 11).
- §3 rule 7's damaged-file fuzzing finds a model gap, not only a parser one (Ruling 17).
- Context menus and pop-up buttons opened by a channel click stay open in their tracking loop; the channel keeps answering in that mode and `ui.key escape` closes them (spec §5.5). Not exercised by a test (no context menu is reachable from the e2e flow).

## File Structure

```
crates/test-plugins/src/{lib,plugin}.rs     Kind::Bank, BANK_PARAMS, PARAM_BANK_FIRST (1)
crates/core-host/src/adapt.rs               automation_collapses() (1)
crates/core-host/tests/rt_alloc_plugins.rs  24 bank lanes, collapse assertion (1)
crates/core-ffi/tests/transport.rs          condition wait (1)
crates/vizdaw-scan/tests/scan.rs            seven test plugins (1)
crates/core-control/
  Cargo.toml, src/lib.rs                    the crate (2; modules added 3, 4)
  src/error.rs, protocol.rs, params.rs,     errors, lines and limits, parameter reads,
      element.rs, keys.rs                   element IDs, key strokes (2)
  src/request.rs, registry.rs, wait.rs      validation, menu commands, wait conditions (3)
  src/server.rs, client.rs                  socket server, client (4)
  tests/{protocol,commands,server}.rs       (2, 3, 4)
crates/core-ffi/src/control.rs, lib.rs      UniFFI surface (5)
crates/core-ffi/tests/control.rs            (5)
app/Sources/VizDAWAppModel/ControlCommands.swift   handlers, open, states, launch flag (6)
app/Sources/VizDAWEditors/ControlUI.swift          tree, IDs, events (7)
app/Sources/VizDAWEditors/{EditorCanvas,LaneHeaderView,RegionStripView,PianoKeyboardView,ExpressionPaneView}.swift
                                                   element frames, first mouse (7)
app/Sources/VizDAWEditors/WindowSnapshot.swift     PNG (8)
app/Sources/VizDAW/ControlChannel.swift            pump, windows, host (9)
app/Sources/VizDAW/{VizDAWApp,AppCommands,DocumentWindow,MixerView,EditorWindows,PluginWindows,ModifyPanel}.swift  wiring (9)
app/Tests/VizDAWAppModelTests/ControlCommandsTests.swift   (6)
app/Tests/VizDAWEditorsTests/{ControlUITests,WindowSnapshotTests}.swift   (7, 8)
crates/vizctl/{Cargo.toml,src/{lib,main,cli,launch}.rs,tests/cli.rs}   (10)
crates/core-model/src/rules.rs, tests/query_flatten.rs   tick and loop limits (11)
crates/core-ffi/tests/{damaged_files,blocks}.rs, Cargo.toml   harness (11)
crates/vizctl/tests/e2e.rs, .github/workflows/ci.yml      end to end (11)
app/README.md                                              docs, checklist 81–86 (12)
```

---

### Task 1: Carried follow-ups — the automation queue's collapse branch runs allocation-free; a condition wait instead of a fixed sleep

**Files:**
- Modify: `crates/test-plugins/src/lib.rs`, `crates/test-plugins/src/plugin.rs`
- Modify: `crates/core-host/src/adapt.rs`
- Test: `crates/core-host/tests/rt_alloc_plugins.rs`, `crates/vizdaw-scan/tests/scan.rs`, `crates/core-ffi/tests/transport.rs`

**Interfaces:**
- Consumes: `core_host::adapt::Core::automate` (its `auto_len >= MAX_PARAM_EVENTS` branch), `rt_check`, the transport tests' `wait_for(&Session, impl Fn(&TransportState) -> bool) -> bool` (5 s condition wait).
- Produces: `test_plugins::Kind::Bank` (`"vizdaw.test.bank"`, "Test Bank"), `test_plugins::PARAM_BANK_FIRST: u32 = 100`, `test_plugins::BANK_PARAMS: u32 = 24`; `core_host::adapt::automation_collapses() -> u64`.

Ruling 1.

- [ ] **Step 1: Write the failing tests**

In `crates/core-host/tests/rt_alloc_plugins.rs`, Find:

```rust
//! parameter changes in, gestures out and a fault bypass.
```

Replace:

```rust
//! parameter changes in, gestures out, a fault bypass, and more ramping
//! automation on one plugin than its queue holds (the collapse branch).
```

Find:

```rust
use core_host::PluginHost;
```

Replace:

```rust
use core_host::PluginHost;
use core_host::adapt::automation_collapses;
```

Find:

```rust
use std::time::Duration;
```

Replace:

```rust
use std::time::Duration;
use test_plugins::{BANK_PARAMS, PARAM_BANK_FIRST};
```

Find:

```rust
    let gain = b.insert(builtin, plugin("vizdaw.test.gain")).unwrap();
```

Replace:

```rust
    let gain = b.insert(builtin, plugin("vizdaw.test.gain")).unwrap();
    let bank = b.insert(builtin, plugin("vizdaw.test.bank")).unwrap();
```

Find:

```rust
    b.events(song, auto, points).unwrap();
    let doc = b.finish();
```

Replace:

```rust
    b.events(song, auto, points).unwrap();
    // 24 ramping lanes on one plugin (plan 4d follow-up): a long run sends
    // each parameter about 17 values, 400 in all, more than the adapter's
    // 256-entry queue, so it collapses to the latest value per parameter.
    for k in 0..BANK_PARAMS {
        let lane = b.lane(song, &format!("Bank {k}"), None).unwrap();
        let ramp = (0..256i64)
            .map(|j| {
                EventSpec::new(
                    Tick(j * Q / 2 + i64::from(k) * 7),
                    EventBody::Automation {
                        target: AutomationTarget::PluginParam(
                            bank,
                            ParamId(u64::from(PARAM_BANK_FIRST + k)),
                        ),
                        value: (j % 2) as f64,
                    },
                )
            })
            .collect();
        b.events(song, lane, ramp).unwrap();
    }
    let doc = b.finish();
```

Find:

```rust
    for id in ["synth4", "ducker", "nan"] {
```

Replace:

```rust
    for id in ["synth4", "ducker", "nan", "bank"] {
```

Find:

```rust
    // Generous for a loaded machine; returns as soon as all four are in.
    let r = host.wait_ready(Duration::from_secs(60));
    assert_eq!(
        r.loaded.len(),
        4,
```

Replace:

```rust
    // Generous for a loaded machine; returns as soon as all five are in.
    let r = host.wait_ready(Duration::from_secs(60));
    assert_eq!(
        r.loaded.len(),
        5,
```

Find:

```rust
    rt_check::set_arm_workers(true);
    let before = rt_check::count();
```

Replace:

```rust
    rt_check::set_arm_workers(true);
    let collapses = automation_collapses();
    let before = rt_check::count();
```

Find:

```rust
    assert!(engine.counters().runs > 2_000, "automation split blocks");
```

Replace:

```rust
    assert!(engine.counters().runs > 2_000, "automation split blocks");
    assert!(
        automation_collapses() > collapses,
        "the bank's queue never filled: the collapse branch did not run"
    );
```

In `crates/vizdaw-scan/tests/scan.rs`, Find `    assert_eq!(list.len(), 6);` Replace `    assert_eq!(list.len(), 7);`.

In `crates/core-ffi/tests/transport.rs`, Find:

```rust
    s.stop().unwrap();
    assert!(poll.join().unwrap().is_ok());
    ms(60);
    assert!(!t(&s).playing, "the Stop was lost to the rebuild");
}
```

Replace:

```rust
    s.stop().unwrap();
    assert!(poll.join().unwrap().is_ok());
    // The engine applies Stop a block late: wait for it (generously, slow
    // CI) instead of a fixed sleep; a lost Stop keeps playing and fails.
    assert!(
        wait_for(&s, |t| !t.playing),
        "the Stop was lost to the rebuild"
    );
    assert!(!t(&s).playing, "the rebuild did not start playing again");
}
```

- [ ] **Step 2: Run them to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-host --test rt_alloc_plugins 2>&1 | grep -E "^error" | head -3
```

Expected: does not compile (`unresolved import core_host::adapt::automation_collapses`, `no BANK_PARAMS in the root`).

- [ ] **Step 3: The bank test plugin**

In `crates/test-plugins/src/lib.rs`, Find:

```rust
//! | `vizdaw.test.nan` | writes NaN into every block |
```

Replace:

```rust
//! | `vizdaw.test.nan` | writes NaN into every block |
//! | `vizdaw.test.bank` | passes audio through; 24 automatable parameters (ids 100–123) that only store their values |
```

Find:

```rust
    /// Hangs on activate.
    Hanger,
}
```

Replace:

```rust
    /// Hangs on activate.
    Hanger,
    /// Pass-through with [`BANK_PARAMS`] automatable parameters.
    Bank,
}
```

Find:

```rust
    pub const ALL: [Kind; 6] = [
        Kind::Gain,
        Kind::Ducker,
        Kind::Synth4,
        Kind::Nan,
        Kind::Crasher,
        Kind::Hanger,
    ];
```

Replace:

```rust
    pub const ALL: [Kind; 7] = [
        Kind::Gain,
        Kind::Ducker,
        Kind::Synth4,
        Kind::Nan,
        Kind::Crasher,
        Kind::Hanger,
        Kind::Bank,
    ];
```

Find `            Kind::Hanger => "vizdaw.test.hanger",` Replace:

```rust
            Kind::Hanger => "vizdaw.test.hanger",
            Kind::Bank => "vizdaw.test.bank",
```

Find `            Kind::Hanger => "Test Hanger",` Replace:

```rust
            Kind::Hanger => "Test Hanger",
            Kind::Bank => "Test Bank",
```

Find:

```rust
pub const PARAM_WIGGLE: u32 = 2;
```

Replace:

```rust
pub const PARAM_WIGGLE: u32 = 2;
/// The bank plugin's first parameter id (0 … 1, default 0).
pub const PARAM_BANK_FIRST: u32 = 100;
/// How many parameters the bank plugin has (ids `PARAM_BANK_FIRST..+24`).
pub const BANK_PARAMS: u32 = 24;
```

Find `    descriptors: [PluginDescriptor; 6],` Replace `    descriptors: [PluginDescriptor; 7],`.

In `crates/test-plugins/src/plugin.rs`, Find:

```rust
use crate::{Kind, PARAM_GAIN, PARAM_LATENCY, PARAM_WIGGLE};
```

Replace:

```rust
use crate::{BANK_PARAMS, Kind, PARAM_BANK_FIRST, PARAM_GAIN, PARAM_LATENCY, PARAM_WIGGLE};
```

Find:

```rust
        if kind.is_none_or(|k| k == Kind::Synth4) {
            builder.register::<PluginNotePorts>();
        }
```

Replace:

```rust
        if kind.is_none_or(|k| k == Kind::Synth4) {
            builder.register::<PluginNotePorts>();
        }
        if kind == Some(Kind::Bank) {
            builder.register::<PluginParams>();
        }
```

Find:

```rust
    wiggle: AtomicU64,
    wiggle_pending: AtomicBool,
}
```

Replace:

```rust
    wiggle: AtomicU64,
    wiggle_pending: AtomicBool,
    /// The bank plugin's parameter values (f64 bits).
    bank: [AtomicU64; BANK_PARAMS as usize],
}

/// The bank slot of parameter `id`.
fn bank_index(id: u32) -> Option<usize> {
    id.checked_sub(PARAM_BANK_FIRST)
        .filter(|i| *i < BANK_PARAMS)
        .map(|i| i as usize)
}
```

Find:

```rust
            wiggle_pending: AtomicBool::new(false),
        }
```

Replace:

```rust
            wiggle_pending: AtomicBool::new(false),
            bank: std::array::from_fn(|_| AtomicU64::new(0)),
        }
```

Find (in `set_param`):

```rust
                self.wiggle_pending.store(true, Ordering::Release);
            }
            _ => {}
```

Replace:

```rust
                self.wiggle_pending.store(true, Ordering::Release);
            }
            id => {
                if let Some(slot) = bank_index(id).and_then(|i| self.bank.get(i)) {
                    slot.store(value.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
                }
            }
```

Find (in `param_value`):

```rust
            PARAM_WIGGLE => Some(f64::from_bits(self.wiggle.load(Ordering::Relaxed))),
            _ => None,
```

Replace:

```rust
            PARAM_WIGGLE => Some(f64::from_bits(self.wiggle.load(Ordering::Relaxed))),
            id => bank_index(id)
                .and_then(|i| self.bank.get(i))
                .map(|v| f64::from_bits(v.load(Ordering::Relaxed))),
```

Find:

```rust
    fn count(&self) -> u32 {
        3
    }

    fn get_info(&self, index: u32, info: &mut ParamInfoWriter) {
```

Replace:

```rust
    fn count(&self) -> u32 {
        match self.shared.kind {
            Kind::Bank => BANK_PARAMS,
            _ => 3,
        }
    }

    fn get_info(&self, index: u32, info: &mut ParamInfoWriter) {
        if self.shared.kind == Kind::Bank {
            if index < BANK_PARAMS {
                info.set(&ParamInfo {
                    id: ClapId::new(PARAM_BANK_FIRST + index),
                    flags: ParamInfoFlags::IS_AUTOMATABLE,
                    cookie: Default::default(),
                    name: b"Bank",
                    module: b"",
                    min_value: 0.0,
                    max_value: 1.0,
                    default_value: 0.0,
                });
            }
            return;
        }
```

The bank's processing is the default pass-through (`factor` 1.0); `read_params` already applies its values through `set_param`.

- [ ] **Step 4: Count collapses**

In `crates/core-host/src/adapt.rs`, Find:

```rust
use std::sync::mpsc::Sender;
```

Replace:

```rust
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Sender;

/// How often a full automation queue collapsed to the latest value per
/// parameter (diagnostics; the `rt_alloc_plugins` test checks the branch
/// ran). A relaxed counter: allocation- and lock-free.
static COLLAPSES: AtomicU64 = AtomicU64::new(0);

/// How often a full automation queue collapsed since the process started.
pub fn automation_collapses() -> u64 {
    COLLAPSES.load(Ordering::Relaxed)
}
```

Find:

```rust
        if self.auto_len >= MAX_PARAM_EVENTS {
            self.collapse();
        }
```

Replace:

```rust
        if self.auto_len >= MAX_PARAM_EVENTS {
            COLLAPSES.fetch_add(1, Ordering::Relaxed);
            self.collapse();
        }
```

- [ ] **Step 5: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-host --test rt_alloc_plugins 2>&1 | tail -3
cargo test -p test-plugins -p vizdaw-scan 2>&1 | grep -E "^test result|FAILED"
cargo test -p core-ffi --test transport a_stop_during 2>&1 | tail -3
```

Expected: PASS (`plugins_render_without_allocating`, `the_bundle_lists_every_test_plugin`, `lists_and_scans_a_plugin`, `a_stop_during_the_rebuild_drain_is_not_lost`).

- [ ] **Step 6: Gate and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace 2>&1 | grep -E "^test result|FAILED|panicked" | tail -60
scripts/build-ffi.sh && scripts/swift.sh build 2>&1 | tail -3 && scripts/swift.sh test 2>&1 | tail -3
git add crates/test-plugins/src/lib.rs crates/test-plugins/src/plugin.rs crates/core-host/src/adapt.rs crates/core-host/tests/rt_alloc_plugins.rs crates/vizdaw-scan/tests/scan.rs crates/core-ffi/tests/transport.rs
git commit -m "test: automation queue collapse runs allocation-free; condition wait after a rebuild

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
```

Expected: every suite passes (760 Rust tests listed, 551 Swift).

---

### Task 2: `core-control` — request lines, limits, errors, parameters, element IDs, key strokes

**Files:**
- Create: `crates/core-control/Cargo.toml`, `crates/core-control/src/lib.rs`, `src/error.rs`, `src/protocol.rs`, `src/params.rs`, `src/element.rs`, `src/keys.rs`
- Test: `crates/core-control/tests/protocol.rs`

**Interfaces:**
- Consumes: nothing (new crate; the workspace's `members = ["crates/*"]` picks it up).
- Produces:
  - `ErrorCode` (15 variants, `ALL`, `as_str() -> &'static str`, `parse(&str) -> Option<ErrorCode>`); `ControlError { code, message: String, dialog: Option<String>, details: Option<Value> }` with `new(code, msg)`, `params(cmd, msg)`, `dialog(name, answered_by: &[&str])`, `with_details(Value)`, `to_json()`, `from_json(&Value)`.
  - `protocol::{MAX_LINE = 1 << 20, MAX_ID_LEN = 128, MAX_CMD_LEN = 64, MAX_DEPTH = 32}`; `RequestId::{Num(i64), Text(String)}`; `RawRequest { id, cmd: String, params: Map<String, Value> }`; `LineError { id: Option<RequestId>, error }`; `read_request(&[u8]) -> Result<RawRequest, LineError>`; `depth(&Value) -> usize`; `truncate(&str, usize) -> String`; `reply_line(Option<&RequestId>, &Result<Value, ControlError>) -> String`; `request_line(&RequestId, &str, &Value) -> String`; `Reply { id, outcome }`; `read_reply(&str) -> Result<Reply, String>`.
  - `params::{MAX_STRING = 4096, MAX_PATH = 1024, MAX_COORD = 100_000.0}`; `WindowRef::{Number(i64), Key, Main}`; `Params::new(cmd, &map)` with `cmd()`, `error(msg)`, `only(&[&str])`, `raw(name)`, `has(name)`, `opt_str/str(name, max)`, `opt_bool`, `opt_int/int(name, min, max)`, `opt_num(name, min, max)`, `opt_path(name, ext) -> Option<PathBuf>`, `window() -> Option<WindowRef>`, `point_of(name, &Value) -> (f64, f64)`.
  - `element::{MAX_PATH_LEN = 64, MAX_INDEX = 100_000}`; `ElementRef { id, path: Vec<u32>, check: u32 }`; `check(role, label) -> u32` (FNV-1a of role, U+001F, label); `element_id(&[u32], role, label) -> String` (`"0.3.1:9f2c01ab"`); `parse(&str) -> Result<ElementRef, String>`.
  - `keys::MAX_TEXT = 256`; `Modifiers { shift, control, option, command }` with `parse(Option<&Value>)`; `KeyStroke { key_code: u16, characters, unmodified, modifiers }`; `named_key(&str, Modifiers) -> Result<KeyStroke, String>`; `text_strokes(&str) -> Result<Vec<KeyStroke>, String>`.

Spec §5.1–5.4.

- [ ] **Step 1: The crate skeleton**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p crates/core-control/src crates/core-control/tests
cat > crates/core-control/Cargo.toml <<'VIZDAW_EOF'
[package]
name = "core-control"
version = "0.1.0"
edition.workspace = true
license.workspace = true
rust-version.workspace = true
publish = false

[dependencies]
serde_json = "1"

[dev-dependencies]
proptest = "1"
tempfile = "3"
VIZDAW_EOF
cat > crates/core-control/src/lib.rs <<'VIZDAW_EOF'
//! `core-control`: the agent control channel (design spec
//! `docs/superpowers/specs/2026-10-18-control-channel-design.md`).
//! JSON-lines requests and replies, their limits and errors, element IDs
//! and key strokes (this task); command validation, the menu-command
//! registry and wait conditions (Task 3); the Unix-socket server and
//! client (Task 4). Platform-neutral Unix code: the app executes what
//! needs its windows.
#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic
    )
)]

pub mod element;
pub mod error;
pub mod keys;
pub mod params;
pub mod protocol;

pub use error::{ControlError, ErrorCode};
pub use protocol::{RawRequest, RequestId};
VIZDAW_EOF
```

- [ ] **Step 2: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-control/tests/protocol.rs <<'VIZDAW_EOF'
//! Request lines, replies, element IDs and key strokes (spec §5).

use core_control::element::{self, check, element_id};
use core_control::keys::{Modifiers, named_key, text_strokes};
use core_control::protocol::{
    MAX_LINE, Reply, RequestId, depth, read_reply, read_request, reply_line, request_line,
};
use core_control::{ControlError, ErrorCode};
use serde_json::{Value, json};

fn code(line: &str) -> ErrorCode {
    read_request(line.as_bytes()).unwrap_err().error.code
}

#[test]
fn a_request_has_an_id_a_command_and_params() {
    let r = read_request(br#"{"id": 7, "cmd": "state.get", "params": {"target": "transport"}}"#)
        .unwrap();
    assert_eq!(r.id, RequestId::Num(7));
    assert_eq!(r.cmd, "state.get");
    assert_eq!(r.params.get("target"), Some(&json!("transport")));
    let r = read_request(br#"{"id": "a-1", "cmd": "commands.list"}"#).unwrap();
    assert_eq!(r.id, RequestId::Text("a-1".into()));
    assert!(r.params.is_empty(), "absent params are empty");
    let r = read_request(br#"{"id":1,"cmd":"app.quit","params":null}"#).unwrap();
    assert!(r.params.is_empty(), "null params are empty");
}

#[test]
fn malformed_lines_are_refused_with_the_right_code() {
    assert_eq!(code("not json"), ErrorCode::ParseError);
    assert_eq!(code(""), ErrorCode::InvalidRequest);
    assert_eq!(code("[1,2]"), ErrorCode::InvalidRequest);
    assert_eq!(code(r#"{"cmd":"app.quit"}"#), ErrorCode::InvalidRequest);
    assert_eq!(
        code(r#"{"id":1.5,"cmd":"app.quit"}"#),
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        code(r#"{"id":{"a":1},"cmd":"app.quit"}"#),
        ErrorCode::InvalidRequest
    );
    assert_eq!(code(r#"{"id":1}"#), ErrorCode::InvalidRequest);
    assert_eq!(code(r#"{"id":1,"cmd":5}"#), ErrorCode::InvalidRequest);
    assert_eq!(
        code(r#"{"id":1,"cmd":"App.Quit"}"#),
        ErrorCode::UnknownCommand
    );
    assert_eq!(
        code(r#"{"id":1,"cmd":"app.quit","params":[1]}"#),
        ErrorCode::InvalidParams
    );
    assert_eq!(
        code(r#"{"id":1,"cmd":"app.quit","extra":true}"#),
        ErrorCode::InvalidRequest
    );
    let long = format!(r#"{{"id":"{}","cmd":"app.quit"}}"#, "x".repeat(129));
    assert_eq!(code(&long), ErrorCode::InvalidRequest);
    assert_eq!(
        read_request(&[0xff, 0xfe, b'{']).unwrap_err().error.code,
        ErrorCode::ParseError
    );
}

#[test]
fn the_id_is_kept_once_it_was_read() {
    let e = read_request(br#"{"id":"k","cmd":"x y"}"#).unwrap_err();
    assert_eq!(e.id, Some(RequestId::Text("k".into())));
    let e = read_request(b"garbage").unwrap_err();
    assert_eq!(e.id, None);
}

#[test]
fn deep_nesting_and_long_lines_are_refused() {
    let deep = format!(
        r#"{{"id":1,"cmd":"state.get","params":{{"a":{}{}}}}}"#,
        "[".repeat(40),
        "]".repeat(40)
    );
    assert_eq!(code(&deep), ErrorCode::InvalidRequest);
    let v: Value = serde_json::from_str(&format!("{}1{}", "[".repeat(10), "]".repeat(10))).unwrap();
    assert_eq!(depth(&v), 10);
    let huge = vec![b' '; MAX_LINE + 1];
    assert_eq!(
        read_request(&huge).unwrap_err().error.code,
        ErrorCode::ParseError
    );
}

#[test]
fn replies_round_trip() {
    let id = RequestId::Num(3);
    let ok = reply_line(Some(&id), &Ok(json!({"playing": true})));
    assert_eq!(
        read_reply(&ok).unwrap(),
        Reply {
            id: Some(id.clone()),
            outcome: Ok(json!({"playing": true}))
        }
    );
    let e = ControlError::dialog("Save", &["path"]);
    let line = reply_line(Some(&id), &Err(e.clone()));
    let v: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(v["error"]["code"], "dialog_required");
    assert_eq!(v["error"]["dialog"], "Save");
    assert_eq!(read_reply(&line).unwrap().outcome, Err(e));
    let none = reply_line(None, &Err(ControlError::new(ErrorCode::ParseError, "x")));
    assert_eq!(read_reply(&none).unwrap().id, None);
    let req = request_line(&RequestId::Text("q".into()), "app.quit", &json!({}));
    assert_eq!(read_request(req.as_bytes()).unwrap().cmd, "app.quit");
}

#[test]
fn every_error_code_has_a_distinct_wire_name() {
    let names: std::collections::BTreeSet<_> = ErrorCode::ALL.iter().map(|c| c.as_str()).collect();
    assert_eq!(names.len(), ErrorCode::ALL.len());
    for c in ErrorCode::ALL {
        assert_eq!(ErrorCode::parse(c.as_str()), Some(c));
    }
}

#[test]
fn element_ids_carry_a_path_and_a_check() {
    let id = element_id(&[0, 3, 12], "AXButton", "Play");
    let r = element::parse(&id).unwrap();
    assert_eq!(r.path, vec![0, 3, 12]);
    assert_eq!(r.check, check("AXButton", "Play"));
    assert_ne!(check("AXButton", "Play"), check("AXButton", "Stop"));
    assert_ne!(
        check("AXButtonP", "lay"),
        check("AXButton", "Play"),
        "role and label are separated"
    );
    for bad in [
        "",
        ":",
        "0.1",
        "0..1:00000000",
        "a:00000000",
        "0:0000000",
        "0:0000000g",
        "1234567:00000000",
    ] {
        assert!(element::parse(bad).is_err(), "{bad:?}");
    }
    let too_deep = format!("{}:00000000", vec!["0"; 65].join("."));
    assert!(element::parse(&too_deep).is_err());
}

#[test]
fn keys_have_codes_and_characters() {
    let space = named_key("space", Modifiers::default()).unwrap();
    assert_eq!((space.key_code, space.characters.as_str()), (49, " "));
    let cmd = Modifiers {
        command: true,
        ..Modifiers::default()
    };
    let s = named_key("s", cmd).unwrap();
    assert_eq!(
        (s.key_code, s.characters.as_str(), s.modifiers.command),
        (1, "s", true)
    );
    let upper = named_key("S", cmd).unwrap();
    assert!(upper.modifiers.shift && upper.modifiers.command);
    assert_eq!(upper.unmodified, "s");
    assert!(named_key("hyper", Modifiers::default()).is_err());
    let typed = text_strokes("Hi!\n").unwrap();
    assert_eq!(typed.len(), 4);
    assert_eq!((typed[0].key_code, typed[0].modifiers.shift), (4, true));
    assert_eq!((typed[2].key_code, typed[2].modifiers.shift), (18, true));
    assert_eq!(typed[3].key_code, 36);
    assert_eq!(
        text_strokes("é").unwrap()[0].key_code,
        0xFFFF,
        "sent with its character"
    );
    assert!(text_strokes("").is_err());
    assert!(text_strokes(&"a".repeat(257)).is_err());
    assert!(text_strokes("a\u{7}").is_err());
    let m = Modifiers::parse(Some(&json!(["cmd", "shift"]))).unwrap();
    assert!(m.command && m.shift && !m.option);
    assert!(Modifiers::parse(Some(&json!(["meta"]))).is_err());
    assert!(Modifiers::parse(Some(&json!("shift"))).is_err());
}

mod fuzz {
    //! Random and mutated lines never panic and always answer (spec §11).
    use super::*;
    use proptest::prelude::*;

    /// JSON values of every kind, a few levels deep.
    pub fn any_json() -> impl Strategy<Value = Value> {
        let leaf = prop_oneof![
            Just(Value::Null),
            any::<bool>().prop_map(Value::from),
            any::<i64>().prop_map(Value::from),
            any::<f64>()
                .prop_filter("finite", |x| x.is_finite())
                .prop_map(Value::from),
            ".{0,40}".prop_map(Value::from),
        ];
        leaf.prop_recursive(4, 64, 8, |inner| {
            prop_oneof![
                prop::collection::vec(inner.clone(), 0..8).prop_map(Value::from),
                prop::collection::btree_map("[a-z_]{0,12}", inner, 0..8)
                    .prop_map(|m| Value::Object(m.into_iter().collect())),
            ]
        })
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(512))]

        #[test]
        fn any_bytes_are_answered(bytes in prop::collection::vec(any::<u8>(), 0..512)) {
            let r = read_request(&bytes);
            if let Err(e) = r {
                // The reply line is always valid JSON.
                let line = reply_line(e.id.as_ref(), &Err(e.error));
                prop_assert!(read_reply(&line).is_ok());
            }
        }

        #[test]
        fn any_json_is_answered(v in any_json()) {
            let line = v.to_string();
            let _ = read_request(line.as_bytes());
        }

        #[test]
        fn requests_with_random_params_are_answered(
            id in any::<i64>(),
            cmd in "[a-z._]{1,20}",
            params in any_json(),
        ) {
            let line = json!({"id": id, "cmd": cmd, "params": params}).to_string();
            match read_request(line.as_bytes()) {
                Ok(r) => prop_assert_eq!(r.id, RequestId::Num(id)),
                Err(e) => prop_assert!(e.id.is_none() || e.id == Some(RequestId::Num(id))),
            }
        }

        #[test]
        fn element_ids_round_trip(path in prop::collection::vec(0u32..1000, 1..20), role in ".{0,20}", label in ".{0,60}") {
            let id = element_id(&path, &role, &label);
            let r = element::parse(&id).unwrap();
            prop_assert_eq!(r.path, path);
            prop_assert_eq!(r.check, check(&role, &label));
        }

        #[test]
        fn any_string_is_an_id_or_refused(s in ".{0,80}") {
            let _ = element::parse(&s);
            let _ = named_key(&s, Modifiers::default());
            let _ = text_strokes(&s);
        }
    }
}
VIZDAW_EOF
cargo test -p core-control --test protocol 2>&1 | grep -E "^error" | head -3
```

Expected: does not compile (`file not found for module element`).

- [ ] **Step 3: Errors**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-control/src/error.rs <<'VIZDAW_EOF'
//! Error codes and the error object a failed reply carries (spec §5.3).

use serde_json::{Map, Value};
use std::fmt;

/// Why a request failed: the `error.code` string on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorCode {
    /// The line is not UTF-8 JSON, or longer than [`crate::protocol::MAX_LINE`].
    ParseError,
    /// JSON, but not a request object (`id`, `cmd`, `params`).
    InvalidRequest,
    /// `cmd` names no command.
    UnknownCommand,
    /// A parameter is missing, unknown, of the wrong type or out of range.
    InvalidParams,
    /// `command.run` names no registered menu command.
    UnknownId,
    /// No document is open (or the given window shows none).
    NoDocument,
    /// The window number names no VizDAW window.
    NoWindow,
    /// The element ID resolves to nothing (or to an element that changed).
    NoElement,
    /// The call would open a dialog; `dialog` names it, the parameters
    /// that answer it are listed in `commands.list`.
    DialogRequired,
    /// A plugin call is on the main thread's stack (a nested run loop);
    /// try again.
    Busy,
    /// The action ran and failed (the message is what the alert would say).
    Failed,
    /// The command is disabled right now (its menu item would be grey).
    Unavailable,
    /// No answer in time (a wait whose condition never held, or an app
    /// that did not answer).
    Timeout,
    /// The channel is stopping (the app is quitting).
    ShuttingDown,
    /// A bug: a caught panic or an impossible state.
    Internal,
}

impl ErrorCode {
    /// Every code, in the spec's order.
    pub const ALL: [ErrorCode; 15] = [
        ErrorCode::ParseError,
        ErrorCode::InvalidRequest,
        ErrorCode::UnknownCommand,
        ErrorCode::InvalidParams,
        ErrorCode::UnknownId,
        ErrorCode::NoDocument,
        ErrorCode::NoWindow,
        ErrorCode::NoElement,
        ErrorCode::DialogRequired,
        ErrorCode::Busy,
        ErrorCode::Failed,
        ErrorCode::Unavailable,
        ErrorCode::Timeout,
        ErrorCode::ShuttingDown,
        ErrorCode::Internal,
    ];

    /// The wire string.
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorCode::ParseError => "parse_error",
            ErrorCode::InvalidRequest => "invalid_request",
            ErrorCode::UnknownCommand => "unknown_command",
            ErrorCode::InvalidParams => "invalid_params",
            ErrorCode::UnknownId => "unknown_id",
            ErrorCode::NoDocument => "no_document",
            ErrorCode::NoWindow => "no_window",
            ErrorCode::NoElement => "no_element",
            ErrorCode::DialogRequired => "dialog_required",
            ErrorCode::Busy => "busy",
            ErrorCode::Failed => "failed",
            ErrorCode::Unavailable => "unavailable",
            ErrorCode::Timeout => "timeout",
            ErrorCode::ShuttingDown => "shutting_down",
            ErrorCode::Internal => "internal",
        }
    }

    /// The code of a wire string.
    pub fn parse(s: &str) -> Option<ErrorCode> {
        Self::ALL.into_iter().find(|c| c.as_str() == s)
    }
}

/// The error of a failed reply.
#[derive(Debug, Clone, PartialEq)]
pub struct ControlError {
    /// What kind of failure.
    pub code: ErrorCode,
    /// One human-readable sentence.
    pub message: String,
    /// The dialog the call would have opened (`dialog_required`).
    pub dialog: Option<String>,
    /// Extra data (a wait's last facts).
    pub details: Option<Value>,
}

impl ControlError {
    /// An error without dialog or details.
    pub fn new(code: ErrorCode, message: impl Into<String>) -> ControlError {
        ControlError {
            code,
            message: message.into(),
            dialog: None,
            details: None,
        }
    }

    /// `invalid_params` for command `cmd`.
    pub fn params(cmd: &str, message: impl fmt::Display) -> ControlError {
        ControlError::new(ErrorCode::InvalidParams, format!("{cmd}: {message}"))
    }

    /// `dialog_required` naming `dialog` and the parameters that answer it.
    pub fn dialog(dialog: &str, answered_by: &[&str]) -> ControlError {
        let params = answered_by
            .iter()
            .map(|p| format!("`{p}`"))
            .collect::<Vec<_>>()
            .join(", ");
        ControlError {
            code: ErrorCode::DialogRequired,
            message: format!("this would open the \"{dialog}\" dialog; pass {params} instead"),
            dialog: Some(dialog.to_string()),
            details: None,
        }
    }

    /// The same error with `details`.
    pub fn with_details(mut self, details: Value) -> ControlError {
        self.details = Some(details);
        self
    }

    /// The wire object: `code`, `message`, then `dialog` and `details`
    /// when present.
    pub fn to_json(&self) -> Value {
        let mut m = Map::new();
        m.insert("code".into(), Value::from(self.code.as_str()));
        m.insert("message".into(), Value::from(self.message.clone()));
        if let Some(d) = &self.dialog {
            m.insert("dialog".into(), Value::from(d.clone()));
        }
        if let Some(d) = &self.details {
            m.insert("details".into(), d.clone());
        }
        Value::Object(m)
    }

    /// The error a reply's `error` object holds; None when it is not one.
    pub fn from_json(v: &Value) -> Option<ControlError> {
        let code = ErrorCode::parse(v.get("code")?.as_str()?)?;
        Some(ControlError {
            code,
            message: v.get("message")?.as_str()?.to_string(),
            dialog: v.get("dialog").and_then(Value::as_str).map(str::to_string),
            details: v.get("details").cloned(),
        })
    }
}

impl fmt::Display for ControlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code.as_str(), self.message)
    }
}

impl std::error::Error for ControlError {}
VIZDAW_EOF
```

- [ ] **Step 4: Lines, ids and replies**

Reading is total: any byte string gives a request or a `LineError`; depth is measured iteratively (serde_json itself stops at 128 levels, so parsing cannot overflow the stack).

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-control/src/protocol.rs <<'VIZDAW_EOF'
//! JSON lines (spec §5.1–5.2): one request object per line in, one reply
//! object per line out. Reading never panics: any byte string becomes a
//! request or a [`LineError`].

use crate::error::{ControlError, ErrorCode};
use serde_json::{Map, Value, json};

/// The longest request line, newline excluded (1 MiB).
pub const MAX_LINE: usize = 1 << 20;
/// The longest string `id`.
pub const MAX_ID_LEN: usize = 128;
/// The longest command name.
pub const MAX_CMD_LEN: usize = 64;
/// The deepest nesting of arrays and objects in a request.
pub const MAX_DEPTH: usize = 32;

/// A request's `id`, echoed in its reply: a string of at most
/// [`MAX_ID_LEN`] characters or an integer.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RequestId {
    /// An integer id.
    Num(i64),
    /// A string id.
    Text(String),
}

impl RequestId {
    /// The JSON value.
    pub fn to_json(&self) -> Value {
        match self {
            RequestId::Num(n) => Value::from(*n),
            RequestId::Text(s) => Value::from(s.clone()),
        }
    }

    /// The id a JSON value holds, if it is a valid one.
    pub fn from_json(v: &Value) -> Option<RequestId> {
        match v {
            Value::Number(n) => n.as_i64().map(RequestId::Num),
            Value::String(s) if s.chars().count() <= MAX_ID_LEN => Some(RequestId::Text(s.clone())),
            _ => None,
        }
    }
}

/// A request as read: id, command name and its unvalidated parameters.
#[derive(Debug, Clone, PartialEq)]
pub struct RawRequest {
    /// Echoed in the reply.
    pub id: RequestId,
    /// The command name (`state.get`, ...).
    pub cmd: String,
    /// The `params` object (empty when absent or null).
    pub params: Map<String, Value>,
}

/// A line that is not a request, with its id when one could be read.
#[derive(Debug, Clone, PartialEq)]
pub struct LineError {
    /// The request's id, if it had a valid one.
    pub id: Option<RequestId>,
    /// Why the line was refused.
    pub error: ControlError,
}

fn refuse(id: Option<RequestId>, code: ErrorCode, message: impl Into<String>) -> LineError {
    LineError {
        id,
        error: ControlError::new(code, message),
    }
}

/// How deeply `v` nests (a scalar is 0). Iterative: a hostile document
/// cannot overflow the stack here (serde_json itself stops at 128).
pub fn depth(v: &Value) -> usize {
    let mut deepest = 0;
    let mut stack = vec![(v, 0usize)];
    while let Some((v, d)) = stack.pop() {
        deepest = deepest.max(d);
        match v {
            Value::Array(a) => stack.extend(a.iter().map(|x| (x, d + 1))),
            Value::Object(o) => stack.extend(o.values().map(|x| (x, d + 1))),
            _ => {}
        }
    }
    deepest
}

/// Reads one request line (without its newline).
pub fn read_request(line: &[u8]) -> Result<RawRequest, LineError> {
    if line.len() > MAX_LINE {
        return Err(refuse(
            None,
            ErrorCode::ParseError,
            format!("request line longer than {MAX_LINE} bytes"),
        ));
    }
    let Ok(text) = std::str::from_utf8(line) else {
        return Err(refuse(
            None,
            ErrorCode::ParseError,
            "request line is not UTF-8",
        ));
    };
    let text = text.trim();
    if text.is_empty() {
        return Err(refuse(
            None,
            ErrorCode::InvalidRequest,
            "empty request line",
        ));
    }
    let v: Value = serde_json::from_str(text)
        .map_err(|e| refuse(None, ErrorCode::ParseError, format!("not JSON: {e}")))?;
    let Value::Object(mut obj) = v else {
        return Err(refuse(
            None,
            ErrorCode::InvalidRequest,
            "a request is a JSON object",
        ));
    };
    let id = match obj.remove("id") {
        None => return Err(refuse(None, ErrorCode::InvalidRequest, "missing `id`")),
        Some(v) => RequestId::from_json(&v).ok_or_else(|| {
            refuse(
                None,
                ErrorCode::InvalidRequest,
                format!("`id` must be an integer or a string of at most {MAX_ID_LEN} characters"),
            )
        })?,
    };
    let fail = |code, message: String| Err(refuse(Some(id.clone()), code, message));
    if depth(&Value::Object(obj.clone())) > MAX_DEPTH {
        return fail(
            ErrorCode::InvalidRequest,
            format!("request nests deeper than {MAX_DEPTH} levels"),
        );
    }
    let cmd = match obj.remove("cmd") {
        Some(Value::String(c)) => c,
        Some(_) => return fail(ErrorCode::InvalidRequest, "`cmd` must be a string".into()),
        None => return fail(ErrorCode::InvalidRequest, "missing `cmd`".into()),
    };
    let well_formed = !cmd.is_empty()
        && cmd.len() <= MAX_CMD_LEN
        && cmd
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'_');
    if !well_formed {
        return fail(
            ErrorCode::UnknownCommand,
            format!("no command {:?}", truncate(&cmd, MAX_CMD_LEN)),
        );
    }
    let params = match obj.remove("params") {
        None | Some(Value::Null) => Map::new(),
        Some(Value::Object(p)) => p,
        Some(_) => {
            return fail(
                ErrorCode::InvalidParams,
                format!("{cmd}: `params` must be an object"),
            );
        }
    };
    if let Some(k) = obj.keys().next() {
        return fail(
            ErrorCode::InvalidRequest,
            format!(
                "unknown field {:?} (a request has `id`, `cmd` and `params`)",
                truncate(k, 64)
            ),
        );
    }
    Ok(RawRequest { id, cmd, params })
}

/// `s` cut to at most `n` characters, for messages that quote input.
pub fn truncate(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// The reply line for `id` (no trailing newline).
pub fn reply_line(id: Option<&RequestId>, outcome: &Result<Value, ControlError>) -> String {
    let id = id.map_or(Value::Null, RequestId::to_json);
    let v = match outcome {
        Ok(result) => json!({ "id": id, "ok": true, "result": result }),
        Err(e) => json!({ "id": id, "ok": false, "error": e.to_json() }),
    };
    v.to_string()
}

/// A request line (no trailing newline), for clients.
pub fn request_line(id: &RequestId, cmd: &str, params: &Value) -> String {
    json!({ "id": id.to_json(), "cmd": cmd, "params": params }).to_string()
}

/// A reply as a client reads it.
#[derive(Debug, Clone, PartialEq)]
pub struct Reply {
    /// The request's id (null for a line the server could not read).
    pub id: Option<RequestId>,
    /// `result` or `error`.
    pub outcome: Result<Value, ControlError>,
}

/// Reads a reply line; Err says why it is not one.
pub fn read_reply(line: &str) -> Result<Reply, String> {
    let v: Value =
        serde_json::from_str(line.trim()).map_err(|e| format!("reply is not JSON: {e}"))?;
    let id = match v.get("id") {
        None | Some(Value::Null) => None,
        Some(i) => Some(RequestId::from_json(i).ok_or("reply has an invalid id")?),
    };
    let outcome = match v.get("ok").and_then(Value::as_bool) {
        Some(true) => Ok(v.get("result").cloned().unwrap_or(Value::Null)),
        Some(false) => Err(v
            .get("error")
            .and_then(ControlError::from_json)
            .ok_or("reply has ok:false without a valid error")?),
        None => return Err("reply has no `ok`".into()),
    };
    Ok(Reply { id, outcome })
}
VIZDAW_EOF
```

- [ ] **Step 5: Parameters, element IDs and key strokes**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-control/src/params.rs <<'VIZDAW_EOF'
//! Typed reads of a request's `params` object. Every failure is an
//! `invalid_params` error naming the command and the parameter.

use crate::error::ControlError;
use crate::protocol::truncate;
use serde_json::{Map, Value};
use std::path::PathBuf;

/// The longest string parameter (characters).
pub const MAX_STRING: usize = 4096;
/// The longest path parameter (bytes).
pub const MAX_PATH: usize = 1024;
/// Window coordinates are within ±this many points.
pub const MAX_COORD: f64 = 100_000.0;

/// A window a request addresses (spec §5.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WindowRef {
    /// `NSWindow.windowNumber`, as `state.get windows` lists it.
    Number(i64),
    /// The key window.
    Key,
    /// The main window.
    Main,
}

/// The `params` of command `cmd`.
#[derive(Debug, Clone, Copy)]
pub struct Params<'a> {
    cmd: &'a str,
    map: &'a Map<String, Value>,
}

impl<'a> Params<'a> {
    /// Wraps `map`, the params of `cmd`.
    pub fn new(cmd: &'a str, map: &'a Map<String, Value>) -> Params<'a> {
        Params { cmd, map }
    }

    /// The command these params belong to.
    pub fn cmd(&self) -> &str {
        self.cmd
    }

    /// An `invalid_params` error for this command.
    pub fn error(&self, message: impl std::fmt::Display) -> ControlError {
        ControlError::params(self.cmd, message)
    }

    /// Refuses any parameter not in `allowed`.
    pub fn only(&self, allowed: &[&str]) -> Result<(), ControlError> {
        match self.map.keys().find(|k| !allowed.contains(&k.as_str())) {
            None => Ok(()),
            Some(k) => Err(self.error(format!(
                "unknown parameter {:?} (allowed: {})",
                truncate(k, 64),
                if allowed.is_empty() {
                    "none".to_string()
                } else {
                    allowed.join(", ")
                }
            ))),
        }
    }

    /// The raw value of `name`; null counts as absent.
    pub fn raw(&self, name: &str) -> Option<&'a Value> {
        self.map.get(name).filter(|v| !v.is_null())
    }

    /// Whether `name` is present (and not null).
    pub fn has(&self, name: &str) -> bool {
        self.raw(name).is_some()
    }

    /// An optional string of at most `max` characters.
    pub fn opt_str(&self, name: &str, max: usize) -> Result<Option<String>, ControlError> {
        match self.raw(name) {
            None => Ok(None),
            Some(Value::String(s)) if s.chars().count() <= max => Ok(Some(s.clone())),
            Some(Value::String(_)) => {
                Err(self.error(format!("`{name}` is longer than {max} characters")))
            }
            Some(_) => Err(self.error(format!("`{name}` must be a string"))),
        }
    }

    /// A required string of at most `max` characters.
    pub fn str(&self, name: &str, max: usize) -> Result<String, ControlError> {
        self.opt_str(name, max)?
            .ok_or_else(|| self.error(format!("missing `{name}`")))
    }

    /// An optional boolean.
    pub fn opt_bool(&self, name: &str) -> Result<Option<bool>, ControlError> {
        match self.raw(name) {
            None => Ok(None),
            Some(Value::Bool(b)) => Ok(Some(*b)),
            Some(_) => Err(self.error(format!("`{name}` must be true or false"))),
        }
    }

    /// An optional integer in `min..=max`.
    pub fn opt_int(&self, name: &str, min: i64, max: i64) -> Result<Option<i64>, ControlError> {
        match self.raw(name) {
            None => Ok(None),
            Some(Value::Number(n)) => match n.as_i64() {
                Some(i) if (min..=max).contains(&i) => Ok(Some(i)),
                _ => Err(self.error(format!("`{name}` must be an integer from {min} to {max}"))),
            },
            Some(_) => Err(self.error(format!("`{name}` must be an integer"))),
        }
    }

    /// A required integer in `min..=max`.
    pub fn int(&self, name: &str, min: i64, max: i64) -> Result<i64, ControlError> {
        self.opt_int(name, min, max)?
            .ok_or_else(|| self.error(format!("missing `{name}`")))
    }

    /// An optional finite number in `min..=max`.
    pub fn opt_num(&self, name: &str, min: f64, max: f64) -> Result<Option<f64>, ControlError> {
        match self.raw(name) {
            None => Ok(None),
            Some(Value::Number(n)) => match n.as_f64() {
                Some(x) if x.is_finite() && (min..=max).contains(&x) => Ok(Some(x)),
                _ => Err(self.error(format!("`{name}` must be a number from {min} to {max}"))),
            },
            Some(_) => Err(self.error(format!("`{name}` must be a number"))),
        }
    }

    /// An optional absolute path of at most [`MAX_PATH`] bytes ending in
    /// `.{extension}`.
    pub fn opt_path(&self, name: &str, extension: &str) -> Result<Option<PathBuf>, ControlError> {
        let Some(s) = self.opt_str(name, MAX_PATH)? else {
            return Ok(None);
        };
        let p = PathBuf::from(&s);
        if s.len() > MAX_PATH || s.contains('\0') || !p.is_absolute() {
            return Err(self.error(format!("`{name}` must be an absolute path")));
        }
        if p.extension().and_then(|e| e.to_str()) != Some(extension) {
            return Err(self.error(format!("`{name}` must end in .{extension}")));
        }
        Ok(Some(p))
    }

    /// The optional `window`: a window number, `"key"` or `"main"`.
    pub fn window(&self) -> Result<Option<WindowRef>, ControlError> {
        match self.raw("window") {
            None => Ok(None),
            Some(Value::Number(n)) => match n.as_i64() {
                Some(i) if i > 0 => Ok(Some(WindowRef::Number(i))),
                _ => Err(self.error("`window` must be a positive window number")),
            },
            Some(Value::String(s)) if s == "key" => Ok(Some(WindowRef::Key)),
            Some(Value::String(s)) if s == "main" => Ok(Some(WindowRef::Main)),
            Some(_) => Err(self.error(
                "`window` must be a window number from `state.get windows`, \"key\" or \"main\"",
            )),
        }
    }

    /// A point object `{x, y}` in window points.
    pub fn point_of(&self, name: &str, v: &Value) -> Result<(f64, f64), ControlError> {
        let coord = |k: &str| {
            v.get(k)
                .and_then(Value::as_f64)
                .filter(|x| x.is_finite() && x.abs() <= MAX_COORD)
        };
        match (v.as_object().map(Map::len), coord("x"), coord("y")) {
            (Some(2), Some(x), Some(y)) => Ok((x, y)),
            _ => Err(self.error(format!(
                "`{name}` must be {{\"x\": number, \"y\": number}} within ±{MAX_COORD}"
            ))),
        }
    }
}
VIZDAW_EOF
cat > crates/core-control/src/element.rs <<'VIZDAW_EOF'
//! Element IDs (spec §5.4): `"<index path>:<check>"`, e.g. `"0.3.1:9f2c01ab"`.
//! The path walks the window's accessibility children; the check is the
//! FNV-1a hash of the element's role and label, so an ID that now points
//! at a different element is refused (`no_element`) instead of clicking
//! the wrong thing.

/// The deepest path an ID may hold.
pub const MAX_PATH_LEN: usize = 64;
/// The largest child index an ID may hold.
pub const MAX_INDEX: u32 = 100_000;

/// An element a request addresses.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ElementRef {
    /// The ID as given.
    pub id: String,
    /// Child indices from the window down.
    pub path: Vec<u32>,
    /// [`check`] of the element's role and label.
    pub check: u32,
}

/// 32-bit FNV-1a of `role`, U+001F and `label`.
pub fn check(role: &str, label: &str) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for b in role.bytes().chain([0x1f]).chain(label.bytes()) {
        h ^= u32::from(b);
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// The ID of the element at `path` with `role` and `label`.
pub fn element_id(path: &[u32], role: &str, label: &str) -> String {
    let p: Vec<String> = path.iter().map(u32::to_string).collect();
    format!("{}:{:08x}", p.join("."), check(role, label))
}

/// Parses an ID; Err says why it is not one.
pub fn parse(id: &str) -> Result<ElementRef, String> {
    let bad = || {
        format!(
            "{:?} is not an element ID (\"0.3.1:9f2c01ab\" from `ui.tree`)",
            crate::protocol::truncate(id, 80)
        )
    };
    let (path, hash) = id.split_once(':').ok_or_else(bad)?;
    if hash.len() != 8 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(bad());
    }
    let check = u32::from_str_radix(hash, 16).map_err(|_| bad())?;
    let mut indices = Vec::new();
    for part in path.split('.') {
        if part.is_empty() || part.len() > 6 || !part.bytes().all(|b| b.is_ascii_digit()) {
            return Err(bad());
        }
        let i: u32 = part.parse().map_err(|_| bad())?;
        if i > MAX_INDEX {
            return Err(bad());
        }
        indices.push(i);
    }
    if indices.len() > MAX_PATH_LEN {
        return Err(bad());
    }
    Ok(ElementRef {
        id: id.to_string(),
        path: indices,
        check,
    })
}
VIZDAW_EOF
cat > crates/core-control/src/keys.rs <<'VIZDAW_EOF'
//! Key strokes for `ui.key` (spec §5.5): named keys and typed text as the
//! macOS virtual key codes of the US ANSI layout plus the characters an
//! `NSEvent` carries. The app sends them into its own key window; menu key
//! equivalents (⌘S, Space for Play) fire as they would from a keyboard.

use serde_json::Value;

/// The most characters one `ui.key` may type.
pub const MAX_TEXT: usize = 256;

/// Modifier keys held during a click, drag or key stroke.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Modifiers {
    /// ⇧
    pub shift: bool,
    /// ⌃
    pub control: bool,
    /// ⌥
    pub option: bool,
    /// ⌘
    pub command: bool,
}

impl Modifiers {
    /// Reads an array of names: `shift`, `control` (`ctrl`), `option`
    /// (`alt`), `command` (`cmd`). Null or absent is none.
    pub fn parse(v: Option<&Value>) -> Result<Modifiers, String> {
        let mut m = Modifiers::default();
        let Some(v) = v.filter(|v| !v.is_null()) else {
            return Ok(m);
        };
        let names = v.as_array().filter(|a| a.len() <= 4).ok_or(
            "`modifiers` must be an array of up to four of shift, control, option, command",
        )?;
        for n in names {
            match n.as_str() {
                Some("shift") => m.shift = true,
                Some("control" | "ctrl") => m.control = true,
                Some("option" | "alt") => m.option = true,
                Some("command" | "cmd") => m.command = true,
                _ => {
                    return Err(
                        "`modifiers` may hold only shift, control, option and command".into(),
                    );
                }
            }
        }
        Ok(m)
    }
}

/// One key press (down and up) as the app sends it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KeyStroke {
    /// The virtual key code (`NSEvent.keyCode`).
    pub key_code: u16,
    /// `NSEvent.characters`.
    pub characters: String,
    /// `NSEvent.charactersIgnoringModifiers`.
    pub unmodified: String,
    /// Modifiers held.
    pub modifiers: Modifiers,
}

/// Named keys: name, key code, the character AppKit reports.
const NAMED: [(&str, u16, char); 26] = [
    ("return", 36, '\r'),
    ("enter", 76, '\u{3}'),
    ("tab", 48, '\t'),
    ("space", 49, ' '),
    ("delete", 51, '\u{7f}'),
    ("escape", 53, '\u{1b}'),
    ("forward_delete", 117, '\u{f728}'),
    ("home", 115, '\u{f729}'),
    ("end", 119, '\u{f72b}'),
    ("page_up", 116, '\u{f72c}'),
    ("page_down", 121, '\u{f72d}'),
    ("left", 123, '\u{f702}'),
    ("right", 124, '\u{f703}'),
    ("down", 125, '\u{f701}'),
    ("up", 126, '\u{f700}'),
    ("f1", 122, '\u{f704}'),
    ("f2", 120, '\u{f705}'),
    ("f3", 99, '\u{f706}'),
    ("f4", 118, '\u{f707}'),
    ("f5", 96, '\u{f708}'),
    ("f6", 97, '\u{f709}'),
    ("f7", 98, '\u{f70a}'),
    ("f8", 100, '\u{f70b}'),
    ("f9", 101, '\u{f70c}'),
    ("f10", 109, '\u{f70d}'),
    ("f11", 103, '\u{f70e}'),
];

/// Unshifted characters of the US ANSI layout and their key codes.
const ANSI: [(char, u16); 47] = [
    ('a', 0),
    ('s', 1),
    ('d', 2),
    ('f', 3),
    ('h', 4),
    ('g', 5),
    ('z', 6),
    ('x', 7),
    ('c', 8),
    ('v', 9),
    ('b', 11),
    ('q', 12),
    ('w', 13),
    ('e', 14),
    ('r', 15),
    ('y', 16),
    ('t', 17),
    ('1', 18),
    ('2', 19),
    ('3', 20),
    ('4', 21),
    ('6', 22),
    ('5', 23),
    ('=', 24),
    ('9', 25),
    ('7', 26),
    ('-', 27),
    ('8', 28),
    ('0', 29),
    (']', 30),
    ('o', 31),
    ('u', 32),
    ('[', 33),
    ('i', 34),
    ('p', 35),
    ('l', 37),
    ('j', 38),
    ('\'', 39),
    ('k', 40),
    (';', 41),
    ('\\', 42),
    (',', 43),
    ('/', 44),
    ('n', 45),
    ('m', 46),
    ('.', 47),
    ('`', 50),
];

/// Shifted characters and the unshifted key they are on.
const SHIFTED: [(char, char); 21] = [
    ('!', '1'),
    ('@', '2'),
    ('#', '3'),
    ('$', '4'),
    ('%', '5'),
    ('^', '6'),
    ('&', '7'),
    ('*', '8'),
    ('(', '9'),
    (')', '0'),
    ('_', '-'),
    ('+', '='),
    ('{', '['),
    ('}', ']'),
    ('|', '\\'),
    (':', ';'),
    ('"', '\''),
    ('<', ','),
    ('>', '.'),
    ('?', '/'),
    ('~', '`'),
];

fn ansi(c: char) -> Option<u16> {
    ANSI.iter().find(|(k, _)| *k == c).map(|(_, code)| *code)
}

/// The key that types `c` and whether it needs shift; None for a
/// character no US key types (it is still sent, with key code 0xFFFF).
fn key_of(c: char) -> Option<(u16, char, bool)> {
    if c == ' ' {
        return Some((49, ' ', false));
    }
    if let Some(code) = ansi(c) {
        return Some((code, c, false));
    }
    if c.is_ascii_uppercase() {
        let lower = c.to_ascii_lowercase();
        return ansi(lower).map(|code| (code, lower, true));
    }
    SHIFTED
        .iter()
        .find(|(s, _)| *s == c)
        .and_then(|(_, base)| ansi(*base).map(|code| (code, *base, true)))
}

/// The stroke for `key` (a name from the table, or one character) with
/// `modifiers`.
pub fn named_key(key: &str, modifiers: Modifiers) -> Result<KeyStroke, String> {
    if let Some((_, code, ch)) = NAMED.iter().find(|(n, _, _)| *n == key) {
        return Ok(KeyStroke {
            key_code: *code,
            characters: ch.to_string(),
            unmodified: ch.to_string(),
            modifiers,
        });
    }
    let mut chars = key.chars();
    let (Some(c), None) = (chars.next(), chars.next()) else {
        return Err(format!(
            "unknown key {:?} (a single character or one of: {})",
            crate::protocol::truncate(key, 32),
            NAMED.iter().map(|n| n.0).collect::<Vec<_>>().join(", ")
        ));
    };
    let (code, base, shift) = key_of(c).unwrap_or((0xFFFF, c, false));
    Ok(KeyStroke {
        key_code: code,
        characters: c.to_string(),
        unmodified: if modifiers.command || modifiers.control {
            base.to_string()
        } else {
            c.to_string()
        },
        modifiers: Modifiers {
            shift: modifiers.shift || shift,
            ..modifiers
        },
    })
}

/// One stroke per character of `text` (at most [`MAX_TEXT`]); newlines
/// are Return, tabs are Tab.
pub fn text_strokes(text: &str) -> Result<Vec<KeyStroke>, String> {
    let n = text.chars().count();
    if n == 0 || n > MAX_TEXT {
        return Err(format!("`text` must hold 1 to {MAX_TEXT} characters"));
    }
    text.chars()
        .map(|c| match c {
            '\n' | '\r' => named_key("return", Modifiers::default()),
            '\t' => named_key("tab", Modifiers::default()),
            c if c.is_control() => Err(format!(
                "`text` holds the control character U+{:04X}",
                c as u32
            )),
            c => {
                let (code, _, shift) = key_of(c).unwrap_or((0xFFFF, c, false));
                Ok(KeyStroke {
                    key_code: code,
                    characters: c.to_string(),
                    unmodified: c.to_string(),
                    modifiers: Modifiers {
                        shift,
                        ..Modifiers::default()
                    },
                })
            }
        })
        .collect()
}
VIZDAW_EOF
```

- [ ] **Step 6: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-control 2>&1 | grep -E "^test result|FAILED|panicked"
```

Expected: PASS, 13 tests (8 plus 5 property tests).

- [ ] **Step 7: Gate and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace 2>&1 | grep -E "^test result|FAILED|panicked" | tail -60
git add crates/core-control Cargo.lock
git commit -m "feat(control): request lines, limits, errors, element IDs and key strokes

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
```

Expected: 773 Rust tests listed. (No Swift change: the Swift gate is unchanged from Task 1.)

---

### Task 3: Validation of every command, the menu-command registry, wait conditions

**Files:**
- Create: `crates/core-control/src/request.rs`, `src/registry.rs`, `src/wait.rs`
- Modify: `crates/core-control/src/lib.rs`
- Test: `crates/core-control/tests/commands.rs`

**Interfaces:**
- Consumes: Task 2's `Params`, `WindowRef`, `element::parse`, `keys::{named_key, text_strokes, Modifiers, KeyStroke}`, `ControlError`, `RawRequest`.
- Produces:
  - `request::COMMAND_NAMES: [&str; 13]`, `MAX_TREE_DEPTH = 64`, `MAX_DRAG_STEPS = 200`, `DEFAULT_DRAG_STEPS = 16`; `StateTarget::{Transport, Document, Editor, Selection, Windows}`; `Pointer::{Element(ElementRef), Point { x, y }}`; `Button::{Left, Right}`; `CrashAnswer::{Normal, Off, Disable}`; `RecoveryAnswers { plugins: Option<CrashAnswer>, restore_autosave: Option<bool> }`; `Command::{CommandsList, Ping, CommandRun { id, args: Vec<(String, ArgValue)>, window }, StateGet { target, window }, UiTree { window, max_depth: u32 }, UiClick { window, at, button, count: u32, modifiers }, UiDrag { window, from, to, steps: u32, modifiers }, UiKey { window, focus: Option<ElementRef>, strokes: Vec<KeyStroke> }, UiScreenshot { window, path: Option<PathBuf>, scale: Option<f64> }, AppNewDemo { audio }, AppOpen { path: PathBuf, answers }, AppQuit, Wait { condition, window, timeout_ms: u64 }}` with `is_local()`; `validate(&RawRequest) -> Result<Command, ControlError>`.
  - `registry::{ParamKind::{Text, Path(ext), Flag, Choice(&[&str])}, ParamSpec { name, kind, required, help }, DialogSpec { name, when, always, answered_by }, CommandSpec { id, menu, title, shortcut: Option<&str>, document, params, dialogs }, ArgValue::{Text(String), Flag(bool)}, COMMANDS: &[CommandSpec] (38 entries), OPEN_DIALOGS}`; `find(id)`, `validate_args(&CommandSpec, &Map) -> Result<Vec<(String, ArgValue)>, ControlError>`, `unknown(id) -> ControlError`, `shortcut_stroke(&str) -> Option<KeyStroke>`, `command_for_stroke(&KeyStroke) -> Option<&'static str>`, `list_json() -> Value`.
  - `wait::{MAX_TIMEOUT_MS = 600_000, DEFAULT_TIMEOUT_MS = 30_000, DETAIL_KINDS}`; `WaitCondition::{Playing, Stopped, TickAtLeast(i64), DocumentsAtLeast(u32), WindowTitled(String), WindowGone(String), DetailKind(String)}` with `name()`, `needs_document()`, `describe()`; `DocFacts { playing, tick, detail_kind }`; `Facts { document: Option<DocFacts>, documents: u32, window_titles: Vec<String> }` with `to_json()`; `met(&WaitCondition, &Facts) -> bool`; `timeout_details(&WaitCondition, timeout_ms, &Facts) -> Value`.
  - Re-exports `core_control::{Command, validate}`.

Spec §5.5, §6, §7 (always-opened dialogs refused here).

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-control/tests/commands.rs <<'VIZDAW_EOF'
//! Command validation, the registry and wait conditions (spec §5.5, §6).

use core_control::ErrorCode;
use core_control::params::WindowRef;
use core_control::protocol::{RawRequest, RequestId};
use core_control::registry::{self, ArgValue, COMMANDS};
use core_control::request::{
    Button, COMMAND_NAMES, Command, CrashAnswer, Pointer, StateTarget, validate,
};
use core_control::wait::{self, DocFacts, Facts, WaitCondition};
use serde_json::{Value, json};

fn req(cmd: &str, params: Value) -> RawRequest {
    RawRequest {
        id: RequestId::Num(1),
        cmd: cmd.into(),
        params: params.as_object().cloned().unwrap_or_default(),
    }
}

fn ok(cmd: &str, params: Value) -> Command {
    validate(&req(cmd, params)).unwrap()
}

fn err(cmd: &str, params: Value) -> (ErrorCode, String) {
    let e = validate(&req(cmd, params)).unwrap_err();
    (e.code, e.message)
}

#[test]
fn every_command_validates_its_minimal_form() {
    let tmp = std::env::temp_dir();
    let shot = tmp.join("x.png");
    let minimal = [
        ("commands.list", json!({})),
        ("command.run", json!({"id": "transport.play_stop"})),
        ("state.get", json!({"target": "windows"})),
        ("ui.tree", json!({})),
        ("ui.click", json!({"point": {"x": 1, "y": 2}})),
        (
            "ui.drag",
            json!({"from": {"x": 1, "y": 2}, "to": "0.1:00000000"}),
        ),
        ("ui.key", json!({"key": "space"})),
        ("ui.screenshot", json!({"path": shot.to_str().unwrap()})),
        ("app.new_demo", json!({})),
        ("app.open", json!({"path": "/tmp/a.vizdaw"})),
        ("app.quit", json!({})),
        ("wait", json!({"condition": "playing"})),
        ("ping", json!({})),
    ];
    assert_eq!(minimal.len(), COMMAND_NAMES.len());
    for (cmd, params) in minimal {
        assert!(
            validate(&req(cmd, params.clone())).is_ok(),
            "{cmd} {params}"
        );
    }
}

#[test]
fn unknown_commands_and_parameters_are_refused() {
    assert_eq!(err("state.set", json!({})).0, ErrorCode::UnknownCommand);
    assert_eq!(
        err("app.quit", json!({"now": true})).0,
        ErrorCode::InvalidParams
    );
    let (code, msg) = err("state.get", json!({"target": "mixer"}));
    assert_eq!(code, ErrorCode::InvalidParams);
    assert!(msg.starts_with("state.get: "), "{msg}");
}

#[test]
fn state_and_tree_read_their_window() {
    assert_eq!(
        ok("state.get", json!({"target": "transport", "window": 42})),
        Command::StateGet {
            target: StateTarget::Transport,
            window: Some(WindowRef::Number(42))
        }
    );
    assert_eq!(
        ok("ui.tree", json!({"window": "key", "max_depth": 3})),
        Command::UiTree {
            window: Some(WindowRef::Key),
            max_depth: 3
        }
    );
    assert_eq!(
        err("state.get", json!({"target": "transport", "window": 0})).0,
        ErrorCode::InvalidParams
    );
    assert_eq!(
        err(
            "state.get",
            json!({"target": "transport", "window": "front"})
        )
        .0,
        ErrorCode::InvalidParams
    );
    assert_eq!(
        err("ui.tree", json!({"max_depth": 0})).0,
        ErrorCode::InvalidParams
    );
}

#[test]
fn clicks_take_an_element_or_a_point() {
    match ok(
        "ui.click",
        json!({"element": "0.2:0000abcd", "button": "right", "count": 2, "modifiers": ["option"]}),
    ) {
        Command::UiClick {
            at: Pointer::Element(e),
            button,
            count,
            modifiers,
            ..
        } => {
            assert_eq!(e.path, vec![0, 2]);
            assert_eq!((button, count, modifiers.option), (Button::Right, 2, true));
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(err("ui.click", json!({})).0, ErrorCode::InvalidParams);
    assert_eq!(
        err(
            "ui.click",
            json!({"element": "0:00000000", "point": {"x": 0, "y": 0}})
        )
        .0,
        ErrorCode::InvalidParams
    );
    assert_eq!(
        err("ui.click", json!({"point": {"x": 1e9, "y": 0}})).0,
        ErrorCode::InvalidParams
    );
    assert_eq!(
        err("ui.click", json!({"point": {"x": 1, "y": 2, "z": 3}})).0,
        ErrorCode::InvalidParams
    );
    assert_eq!(
        err("ui.click", json!({"point": {"x": 1, "y": 2}, "count": 4})).0,
        ErrorCode::InvalidParams
    );
    assert_eq!(
        err("ui.drag", json!({"from": "nope", "to": {"x": 0, "y": 0}})).0,
        ErrorCode::InvalidParams
    );
}

#[test]
fn keys_take_a_key_or_text() {
    match ok(
        "ui.key",
        json!({"key": "z", "modifiers": ["command", "shift"]}),
    ) {
        Command::UiKey { strokes, .. } => {
            assert_eq!(strokes.len(), 1);
            assert!(strokes[0].modifiers.command && strokes[0].modifiers.shift);
        }
        other => panic!("{other:?}"),
    }
    match ok("ui.key", json!({"text": "Kit"})) {
        Command::UiKey { strokes, .. } => assert_eq!(strokes.len(), 3),
        other => panic!("{other:?}"),
    }
    assert_eq!(
        err("ui.key", json!({"text": "a", "modifiers": ["command"]})).0,
        ErrorCode::InvalidParams
    );
    assert_eq!(
        err("ui.key", json!({"key": "a", "text": "b"})).0,
        ErrorCode::InvalidParams
    );
    assert_eq!(err("ui.key", json!({})).0, ErrorCode::InvalidParams);
}

#[test]
fn paths_are_absolute_and_screenshots_need_an_existing_folder() {
    assert_eq!(
        err("app.open", json!({"path": "rel.vizdaw"})).0,
        ErrorCode::InvalidParams
    );
    assert_eq!(
        err("app.open", json!({"path": "/tmp/a.txt"})).0,
        ErrorCode::InvalidParams
    );
    let (code, _) = err("app.open", json!({}));
    assert_eq!(code, ErrorCode::DialogRequired, "no path: the Open dialog");
    match ok(
        "app.open",
        json!({"path": "/tmp/a.vizdaw", "plugins": "off", "restore_autosave": false}),
    ) {
        Command::AppOpen { answers, .. } => {
            assert_eq!(answers.plugins, Some(CrashAnswer::Off));
            assert_eq!(answers.restore_autosave, Some(false));
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        err("ui.screenshot", json!({"path": "/no/such/folder/x.png"})).0,
        ErrorCode::InvalidParams
    );
    assert_eq!(
        err("ui.screenshot", json!({"scale": 9})).0,
        ErrorCode::InvalidParams
    );
}

#[test]
fn waits_take_their_own_parameters_and_a_bounded_timeout() {
    assert_eq!(
        ok(
            "wait",
            json!({"condition": "tick_at_least", "tick": 15360, "timeout_ms": 60000})
        ),
        Command::Wait {
            condition: WaitCondition::TickAtLeast(15360),
            window: None,
            timeout_ms: 60_000
        }
    );
    match ok("wait", json!({"condition": "stopped"})) {
        Command::Wait { timeout_ms, .. } => assert_eq!(timeout_ms, wait::DEFAULT_TIMEOUT_MS),
        other => panic!("{other:?}"),
    }
    assert_eq!(
        err("wait", json!({"condition": "playing", "tick": 3})).0,
        ErrorCode::InvalidParams
    );
    assert_eq!(
        err("wait", json!({"condition": "tick_at_least"})).0,
        ErrorCode::InvalidParams
    );
    assert_eq!(
        err(
            "wait",
            json!({"condition": "playing", "timeout_ms": 600_001})
        )
        .0,
        ErrorCode::InvalidParams
    );
    assert_eq!(
        err("wait", json!({"condition": "detail_kind", "kind": "mixer"})).0,
        ErrorCode::InvalidParams
    );
    assert_eq!(
        err("wait", json!({"condition": "window_titled", "title": ""})).0,
        ErrorCode::InvalidParams
    );
}

#[test]
fn the_registry_has_unique_ids_and_answers_dialogs_by_parameter() {
    let ids: std::collections::BTreeSet<_> = COMMANDS.iter().map(|c| c.id).collect();
    assert_eq!(ids.len(), COMMANDS.len(), "IDs are unique");
    assert!(COMMANDS.len() >= 38);
    for c in COMMANDS {
        assert!(
            c.id.contains('.')
                && c.id
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b == b'.' || b == b'_'),
            "{}",
            c.id
        );
        for d in c.dialogs {
            for a in d.answered_by {
                assert!(
                    c.params.iter().any(|p| p.name == *a),
                    "{}: {} answers nothing",
                    c.id,
                    a
                );
            }
        }
    }
    assert_eq!(
        err("command.run", json!({"id": "file.print"})).0,
        ErrorCode::UnknownId
    );
    let (code, msg) = err("command.run", json!({"id": "file.save_as"}));
    assert_eq!(code, ErrorCode::DialogRequired);
    assert!(msg.contains("\"Save\"") && msg.contains("`path`"), "{msg}");
    assert_eq!(
        ok(
            "command.run",
            json!({"id": "file.save_as", "params": {"path": "/tmp/a.vizdaw"}, "window": 7})
        ),
        Command::CommandRun {
            id: "file.save_as".into(),
            args: vec![("path".into(), ArgValue::Text("/tmp/a.vizdaw".into()))],
            window: Some(WindowRef::Number(7)),
        }
    );
    assert_eq!(
        err(
            "command.run",
            json!({"id": "transport.loop", "params": {"on": "yes"}})
        )
        .0,
        ErrorCode::InvalidParams
    );
    assert_eq!(
        err("command.run", json!({"id": "modify.template"})).0,
        ErrorCode::InvalidParams
    );
    assert_eq!(
        err(
            "command.run",
            json!({"id": "edit.undo", "params": {"count": 2}})
        )
        .0,
        ErrorCode::InvalidParams
    );
    let list = registry::list_json();
    let first = &list["commands"][0];
    assert_eq!(first["id"], "file.new");
    assert_eq!(list["commands"].as_array().unwrap().len(), COMMANDS.len());
    let open = list["commands"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "file.open")
        .unwrap();
    assert_eq!(open["dialogs"][0]["answered_by"][0], "path");
    assert_eq!(open["params"][1]["values"][1], "off");
}

#[test]
fn shortcuts_name_their_menu_commands() {
    use core_control::keys::{Modifiers, named_key};
    let cmd = Modifiers {
        command: true,
        ..Modifiers::default()
    };
    let stroke = |k: &str, m: Modifiers| registry::command_for_stroke(&named_key(k, m).unwrap());
    assert_eq!(
        stroke("space", Modifiers::default()),
        Some("transport.play_stop")
    );
    assert_eq!(
        stroke("return", Modifiers::default()),
        Some("transport.return_to_start")
    );
    assert_eq!(stroke("s", cmd), Some("file.save"));
    assert_eq!(stroke("S", cmd), Some("file.save_as"), "shift");
    assert_eq!(
        stroke(
            "1",
            Modifiers {
                option: true,
                ..cmd
            }
        ),
        Some("editors.piano_roll_window")
    );
    assert_eq!(
        stroke(
            "left",
            Modifiers {
                option: true,
                ..Modifiers::default()
            }
        ),
        Some("editors.nudge_earlier")
    );
    assert_eq!(
        stroke("x", Modifiers::default()),
        None,
        "a plain letter is typing"
    );
    for c in COMMANDS {
        if let Some(s) = c.shortcut {
            let k = registry::shortcut_stroke(s).unwrap_or_else(|| panic!("{}: {s}", c.id));
            assert_eq!(
                registry::command_for_stroke(&k),
                Some(c.id),
                "{s} is ambiguous"
            );
        }
    }
}

#[test]
fn wait_conditions_read_the_facts() {
    let doc = |playing, tick, kind: &str| Facts {
        document: Some(DocFacts {
            playing,
            tick,
            detail_kind: kind.into(),
        }),
        documents: 1,
        window_titles: vec!["Untitled".into(), "Mixer — Untitled".into()],
    };
    assert!(wait::met(&WaitCondition::Playing, &doc(true, 0, "none")));
    assert!(
        !wait::met(&WaitCondition::Playing, &Facts::default()),
        "no document, never met"
    );
    assert!(!wait::met(&WaitCondition::Stopped, &Facts::default()));
    assert!(wait::met(
        &WaitCondition::TickAtLeast(10),
        &doc(false, 10, "none")
    ));
    assert!(!wait::met(
        &WaitCondition::TickAtLeast(11),
        &doc(true, 10, "none")
    ));
    assert!(wait::met(
        &WaitCondition::DocumentsAtLeast(1),
        &doc(false, 0, "none")
    ));
    assert!(wait::met(
        &WaitCondition::WindowTitled("Mixer".into()),
        &doc(false, 0, "none")
    ));
    assert!(!wait::met(
        &WaitCondition::WindowGone("Mixer".into()),
        &doc(false, 0, "none")
    ));
    assert!(wait::met(
        &WaitCondition::DetailKind("list".into()),
        &doc(false, 0, "list")
    ));
    let d = wait::timeout_details(&WaitCondition::Playing, 500, &doc(false, 7, "none"));
    assert_eq!(d["facts"]["document"]["tick"], 7);
    assert_eq!(d["condition"], "playing");
}

mod fuzz {
    //! Random parameters for every command and every registry entry never
    //! panic; valid ones stay valid (spec §11).
    use super::*;
    use proptest::prelude::*;

    fn any_json() -> impl Strategy<Value = Value> {
        let leaf = prop_oneof![
            Just(Value::Null),
            any::<bool>().prop_map(Value::from),
            any::<i64>().prop_map(Value::from),
            (-1e12f64..1e12).prop_map(Value::from),
            ".{0,24}".prop_map(Value::from),
            Just(Value::from("0.1:00000000")),
            Just(Value::from("/tmp/a.vizdaw")),
            Just(Value::from("space")),
            Just(json!({"x": 1, "y": 2})),
            Just(json!(["command"])),
        ];
        leaf.prop_recursive(3, 32, 6, |inner| {
            prop_oneof![
                prop::collection::vec(inner.clone(), 0..6).prop_map(Value::from),
                prop::collection::btree_map("[a-z_]{0,10}", inner, 0..6)
                    .prop_map(|m| Value::Object(m.into_iter().collect())),
            ]
        })
    }

    /// Parameter names any command uses, so random maps hit real keys.
    fn key() -> impl Strategy<Value = String> {
        prop_oneof![
            Just("id"),
            Just("params"),
            Just("window"),
            Just("target"),
            Just("element"),
            Just("point"),
            Just("from"),
            Just("to"),
            Just("steps"),
            Just("button"),
            Just("count"),
            Just("modifiers"),
            Just("key"),
            Just("text"),
            Just("path"),
            Just("scale"),
            Just("audio"),
            Just("plugins"),
            Just("restore_autosave"),
            Just("condition"),
            Just("tick"),
            Just("title"),
            Just("kind"),
            Just("timeout_ms"),
            Just("max_depth"),
            Just("on"),
            Just("name"),
        ]
        .prop_map(str::to_string)
    }

    fn params() -> impl Strategy<Value = Value> {
        prop::collection::btree_map(key(), any_json(), 0..6)
            .prop_map(|m| Value::Object(m.into_iter().collect()))
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(2048))]

        #[test]
        fn every_command_survives_random_params(i in 0..COMMAND_NAMES.len(), p in params()) {
            let cmd = COMMAND_NAMES[i];
            let _ = validate(&req(cmd, p));
        }

        #[test]
        fn every_registry_entry_survives_random_params(i in 0..COMMANDS.len(), p in params(), w in any_json()) {
            let id = COMMANDS[i].id;
            let r = validate(&req("command.run", json!({"id": id, "params": p, "window": w})));
            if let Ok(Command::CommandRun { id: got, .. }) = r {
                prop_assert_eq!(got, id);
            }
        }
    }
}
VIZDAW_EOF
cargo test -p core-control --test commands 2>&1 | grep -E "^error" | head -3
```

Expected: does not compile (`could not find registry in core_control`).

- [ ] **Step 2: Wait conditions**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-control/src/wait.rs <<'VIZDAW_EOF'
//! `wait` conditions (spec §5.5) and the facts the app reports for them.
//! The app evaluates [`met`] on every pump tick until it holds or the
//! wait's timeout passes; a timeout reports the last facts seen.

use serde_json::{Value, json};

/// The longest wait.
pub const MAX_TIMEOUT_MS: u64 = 600_000;
/// The wait when `timeout_ms` is absent.
pub const DEFAULT_TIMEOUT_MS: u64 = 30_000;

/// A detail editor kind as the protocol names it.
pub const DETAIL_KINDS: [&str; 5] = ["none", "piano_roll", "list", "strip_chart", "drum_grid"];

/// What a `wait` waits for.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum WaitCondition {
    /// The target document's transport plays.
    Playing,
    /// It is stopped.
    Stopped,
    /// It is at or past `tick` (playing or not).
    TickAtLeast(i64),
    /// At least this many documents are open.
    DocumentsAtLeast(u32),
    /// A visible VizDAW window's title contains this text.
    WindowTitled(String),
    /// No visible window's title contains this text.
    WindowGone(String),
    /// The target document's detail pane shows this kind (`none`: empty).
    DetailKind(String),
}

impl WaitCondition {
    /// The condition's wire name.
    pub fn name(&self) -> &'static str {
        match self {
            WaitCondition::Playing => "playing",
            WaitCondition::Stopped => "stopped",
            WaitCondition::TickAtLeast(_) => "tick_at_least",
            WaitCondition::DocumentsAtLeast(_) => "documents_at_least",
            WaitCondition::WindowTitled(_) => "window_titled",
            WaitCondition::WindowGone(_) => "window_gone",
            WaitCondition::DetailKind(_) => "detail_kind",
        }
    }

    /// Whether it needs a target document.
    pub fn needs_document(&self) -> bool {
        matches!(
            self,
            WaitCondition::Playing
                | WaitCondition::Stopped
                | WaitCondition::TickAtLeast(_)
                | WaitCondition::DetailKind(_)
        )
    }

    /// One phrase for messages: `playing`, `tick ≥ 15360`, ...
    pub fn describe(&self) -> String {
        match self {
            WaitCondition::Playing => "playing".into(),
            WaitCondition::Stopped => "stopped".into(),
            WaitCondition::TickAtLeast(t) => format!("tick >= {t}"),
            WaitCondition::DocumentsAtLeast(n) => format!("at least {n} open documents"),
            WaitCondition::WindowTitled(t) => format!("a window titled {t:?}"),
            WaitCondition::WindowGone(t) => format!("no window titled {t:?}"),
            WaitCondition::DetailKind(k) => format!("detail pane {k}"),
        }
    }
}

/// The target document's state, as one pump tick saw it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DocFacts {
    /// Its transport plays.
    pub playing: bool,
    /// Its transport's tick.
    pub tick: i64,
    /// Its detail pane's kind (`none` when empty), as in [`DETAIL_KINDS`].
    pub detail_kind: String,
}

/// What a pump tick saw.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Facts {
    /// The target document; None when there is none.
    pub document: Option<DocFacts>,
    /// Open documents.
    pub documents: u32,
    /// Titles of visible VizDAW windows.
    pub window_titles: Vec<String>,
}

impl Facts {
    /// The facts as JSON (a timeout's `details.facts`).
    pub fn to_json(&self) -> Value {
        json!({
            "document": self.document.as_ref().map(|d| json!({
                "playing": d.playing, "tick": d.tick, "detail_kind": d.detail_kind,
            })),
            "documents": self.documents,
            "window_titles": self.window_titles,
        })
    }
}

/// Whether `c` holds for `f`.
pub fn met(c: &WaitCondition, f: &Facts) -> bool {
    let titled = |t: &str| f.window_titles.iter().any(|w| w.contains(t));
    match c {
        WaitCondition::Playing => f.document.as_ref().is_some_and(|d| d.playing),
        WaitCondition::Stopped => f.document.as_ref().is_some_and(|d| !d.playing),
        WaitCondition::TickAtLeast(t) => f.document.as_ref().is_some_and(|d| d.tick >= *t),
        WaitCondition::DocumentsAtLeast(n) => f.documents >= *n,
        WaitCondition::WindowTitled(t) => titled(t),
        WaitCondition::WindowGone(t) => !titled(t),
        WaitCondition::DetailKind(k) => f.document.as_ref().is_some_and(|d| d.detail_kind == *k),
    }
}

/// The `timeout` error's details for a wait that gave up.
pub fn timeout_details(c: &WaitCondition, timeout_ms: u64, last: &Facts) -> Value {
    json!({ "condition": c.name(), "timeout_ms": timeout_ms, "facts": last.to_json() })
}
VIZDAW_EOF
```

- [ ] **Step 3: The registry**

Every menu item of `app/Sources/VizDAW/AppCommands.swift`, by stable ID, with its shortcut written the way the menu shows it; `shortcut_stroke` turns that text back into a key stroke so `ui.key` can find the command (Ruling 9).

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-control/src/registry.rs <<'VIZDAW_EOF'
//! The menu-command registry (spec §6): every menu command of the app by
//! a stable ID, with its menu, title, shortcut, parameters and the dialogs
//! its parameters answer. `command.run` runs these; plan 4f extends the
//! registry to every command (menu or not) and drives key bindings from it.

use crate::error::{ControlError, ErrorCode};
use crate::keys::{KeyStroke, Modifiers, named_key};
use crate::params::{MAX_PATH, MAX_STRING, Params};
use serde_json::{Map, Value, json};

/// A parameter's type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamKind {
    /// A string of at most [`MAX_STRING`] characters.
    Text,
    /// An absolute path ending in `.{0}`.
    Path(&'static str),
    /// true or false.
    Flag,
    /// One of these strings.
    Choice(&'static [&'static str]),
}

/// One parameter of a command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParamSpec {
    /// Its name in `params`.
    pub name: &'static str,
    /// Its type.
    pub kind: ParamKind,
    /// It must be given.
    pub required: bool,
    /// One line for `commands.list`.
    pub help: &'static str,
}

/// A dialog a command opens from the menu; a programmatic call takes its
/// answers as parameters instead (spec §7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DialogSpec {
    /// The dialog's title, as errors name it.
    pub name: &'static str,
    /// When the menu command opens it.
    pub when: &'static str,
    /// The menu command always opens it: without one of `answered_by`
    /// the call fails before it reaches the app.
    pub always: bool,
    /// The parameters that answer it.
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
    pub params: &'static [ParamSpec],
    /// The dialogs its parameters answer.
    pub dialogs: &'static [DialogSpec],
}

/// A validated `command.run` argument.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ArgValue {
    /// A string (paths too).
    Text(String),
    /// A flag.
    Flag(bool),
}

const fn cmd(
    id: &'static str,
    menu: &'static str,
    title: &'static str,
    shortcut: Option<&'static str>,
    document: bool,
) -> CommandSpec {
    CommandSpec {
        id,
        menu,
        title,
        shortcut,
        document,
        params: &[],
        dialogs: &[],
    }
}

const OPEN_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        name: "path",
        kind: ParamKind::Path("vizdaw"),
        required: false,
        help: "The .vizdaw package to open (answers the Open dialog).",
    },
    ParamSpec {
        name: "plugins",
        kind: ParamKind::Choice(&["normal", "off", "disable"]),
        required: false,
        help: "After a crash: open normally, with plugins off, or disable the plugins that were loading.",
    },
    ParamSpec {
        name: "restore_autosave",
        kind: ParamKind::Flag,
        required: false,
        help: "A newer autosave exists: true restores it, false opens the last save.",
    },
];

/// The dialogs `file.open` and `app.open` answer.
pub const OPEN_DIALOGS: &[DialogSpec] = &[
    DialogSpec {
        name: "Open",
        when: "always",
        always: true,
        answered_by: &["path"],
    },
    DialogSpec {
        name: "Open with Plugins Off?",
        when: "VizDAW quit unexpectedly while the project was open, or Shift is held",
        always: false,
        answered_by: &["plugins"],
    },
    DialogSpec {
        name: "Restore Autosave",
        when: "the project has an autosave newer than its last save",
        always: false,
        answered_by: &["restore_autosave"],
    },
];

const SAVE_PATH: &[ParamSpec] = &[ParamSpec {
    name: "path",
    kind: ParamKind::Path("vizdaw"),
    required: false,
    help: "Where to save (answers the Save dialog).",
}];

const ON: &[ParamSpec] = &[ParamSpec {
    name: "on",
    kind: ParamKind::Flag,
    required: false,
    help: "Set on or off; absent toggles.",
}];

/// Every menu command, in menu order.
pub const COMMANDS: &[CommandSpec] = &[
    cmd("file.new", "File", "New", Some("⌘N"), false),
    CommandSpec {
        params: OPEN_PARAMS,
        dialogs: OPEN_DIALOGS,
        ..cmd("file.open", "File", "Open…", Some("⌘O"), false)
    },
    cmd(
        "file.create_demo",
        "File",
        "Create Demo Project",
        None,
        false,
    ),
    cmd(
        "file.create_demo_audio",
        "File",
        "Create Demo Project with Audio",
        None,
        false,
    ),
    cmd("file.close", "File", "Close", Some("⌘W"), true),
    CommandSpec {
        params: SAVE_PATH,
        dialogs: &[DialogSpec {
            name: "Save",
            when: "the document was never saved",
            always: false,
            answered_by: &["path"],
        }],
        ..cmd("file.save", "File", "Save", Some("⌘S"), true)
    },
    CommandSpec {
        params: SAVE_PATH,
        dialogs: &[DialogSpec {
            name: "Save",
            when: "always",
            always: true,
            answered_by: &["path"],
        }],
        ..cmd("file.save_as", "File", "Save As…", Some("⇧⌘S"), true)
    },
    cmd("edit.undo", "Edit", "Undo", Some("⌘Z"), true),
    cmd("edit.redo", "Edit", "Redo", Some("⇧⌘Z"), true),
    cmd("window.mixer", "Window", "Mixer", Some("⌥⌘M"), true),
    cmd(
        "window.plugin_manager",
        "Window",
        "Plugin Manager",
        Some("⌥⌘P"),
        false,
    ),
    cmd(
        "editors.piano_roll",
        "Editors",
        "Piano Roll",
        Some("⌘1"),
        true,
    ),
    cmd("editors.list", "Editors", "List", Some("⌘2"), true),
    cmd(
        "editors.strip_chart",
        "Editors",
        "Strip Chart",
        Some("⌘3"),
        true,
    ),
    cmd(
        "editors.drum_grid",
        "Editors",
        "Drum Grid",
        Some("⌘4"),
        true,
    ),
    cmd(
        "editors.piano_roll_window",
        "Editors",
        "Open Piano Roll in Window",
        Some("⌥⌘1"),
        true,
    ),
    cmd(
        "editors.list_window",
        "Editors",
        "Open List in Window",
        Some("⌥⌘2"),
        true,
    ),
    cmd(
        "editors.strip_chart_window",
        "Editors",
        "Open Strip Chart in Window",
        Some("⌥⌘3"),
        true,
    ),
    cmd(
        "editors.drum_grid_window",
        "Editors",
        "Open Drum Grid in Window",
        Some("⌥⌘4"),
        true,
    ),
    cmd(
        "editors.close_detail",
        "Editors",
        "Close Detail Editor",
        None,
        true,
    ),
    cmd(
        "editors.insert_event",
        "Editors",
        "Insert Event",
        Some("⌘I"),
        true,
    ),
    cmd(
        "editors.nudge_earlier",
        "Editors",
        "Nudge Earlier",
        Some("⌥←"),
        true,
    ),
    cmd(
        "editors.nudge_later",
        "Editors",
        "Nudge Later",
        Some("⌥→"),
        true,
    ),
    cmd("editors.nudge_up", "Editors", "Nudge Up", Some("⌥↑"), true),
    cmd(
        "editors.nudge_down",
        "Editors",
        "Nudge Down",
        Some("⌥↓"),
        true,
    ),
    cmd(
        "modify.select_by_rule",
        "Modify",
        "Select by Rule…",
        Some("⌃⌘S"),
        true,
    ),
    cmd(
        "modify.transpose",
        "Modify",
        "Transpose…",
        Some("⌃⌘T"),
        true,
    ),
    cmd("modify.quantize", "Modify", "Quantize…", Some("⌃⌘U"), true),
    cmd(
        "modify.change",
        "Modify",
        "Change Values…",
        Some("⌃⌘M"),
        true,
    ),
    cmd("modify.move", "Modify", "Move…", Some("⌃⌘V"), true),
    cmd("modify.again", "Modify", "Modify Again", Some("⌃⌘R"), true),
    CommandSpec {
        params: &[ParamSpec {
            name: "name",
            kind: ParamKind::Text,
            required: true,
            help: "The template to run (Modify > Templates).",
        }],
        ..cmd("modify.template", "Modify", "Templates", None, true)
    },
    CommandSpec {
        params: ON,
        ..cmd(
            "modify.exact_mode",
            "Modify",
            "Exact Mode",
            Some("⌃⌘E"),
            true,
        )
    },
    cmd(
        "sequence.make_reference",
        "Sequence",
        "Make Reference from Selection",
        Some("⌥⌘R"),
        true,
    ),
    cmd(
        "sequence.back",
        "Sequence",
        "Back to Parent Sequence",
        Some("⌘["),
        true,
    ),
    cmd(
        "transport.play_stop",
        "Transport",
        "Play / Stop",
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
        "Return to Start",
        Some("Return"),
        true,
    ),
];

/// The command with `id`.
pub fn find(id: &str) -> Option<&'static CommandSpec> {
    COMMANDS.iter().find(|c| c.id == id)
}

/// Checks `params` against `spec`: no unknown or missing parameter, every
/// value of its type, and an always-opened dialog answered.
pub fn validate_args(
    spec: &CommandSpec,
    params: &Map<String, Value>,
) -> Result<Vec<(String, ArgValue)>, ControlError> {
    let p = Params::new(spec.id, params);
    let names: Vec<&str> = spec.params.iter().map(|s| s.name).collect();
    p.only(&names)?;
    let mut out = Vec::new();
    for s in spec.params {
        let v = match s.kind {
            ParamKind::Text => p.opt_str(s.name, MAX_STRING)?.map(ArgValue::Text),
            ParamKind::Path(ext) => p
                .opt_path(s.name, ext)?
                .map(|path| ArgValue::Text(path.to_string_lossy().into_owned())),
            ParamKind::Flag => p.opt_bool(s.name)?.map(ArgValue::Flag),
            ParamKind::Choice(values) => match p.opt_str(s.name, MAX_PATH)? {
                None => None,
                Some(v) if values.contains(&v.as_str()) => Some(ArgValue::Text(v)),
                Some(_) => {
                    return Err(p.error(format!(
                        "`{}` must be one of {}",
                        s.name,
                        values.join(", ")
                    )));
                }
            },
        };
        match v {
            Some(v) => out.push((s.name.to_string(), v)),
            None if s.required => return Err(p.error(format!("missing `{}`", s.name))),
            None => {}
        }
    }
    for d in spec.dialogs.iter().filter(|d| d.always) {
        if !d.answered_by.iter().any(|a| p.has(a)) {
            return Err(ControlError::dialog(d.name, d.answered_by));
        }
    }
    Ok(out)
}

/// `unknown_id` for a `command.run` id.
pub fn unknown(id: &str) -> ControlError {
    ControlError::new(
        ErrorCode::UnknownId,
        format!(
            "no menu command {:?} (see commands.list)",
            crate::protocol::truncate(id, 64)
        ),
    )
}

/// The key and modifiers of a shortcut as the registry writes it
/// (`⇧⌘S`, `⌥←`, `Space`).
pub fn shortcut_stroke(shortcut: &str) -> Option<KeyStroke> {
    let mut m = Modifiers::default();
    let mut rest = shortcut;
    loop {
        let mut chars = rest.chars();
        match chars.next() {
            Some('⇧') => m.shift = true,
            Some('⌃') => m.control = true,
            Some('⌥') => m.option = true,
            Some('⌘') => m.command = true,
            _ => break,
        }
        rest = chars.as_str();
    }
    let key = match rest {
        "Space" => "space".to_string(),
        "Return" => "return".to_string(),
        "←" => "left".to_string(),
        "→" => "right".to_string(),
        "↑" => "up".to_string(),
        "↓" => "down".to_string(),
        other => other.to_lowercase(),
    };
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
    match k {
        ParamKind::Text => json!({"type": "string"}),
        ParamKind::Path(ext) => json!({"type": "path", "extension": ext}),
        ParamKind::Flag => json!({"type": "bool"}),
        ParamKind::Choice(values) => json!({"type": "choice", "values": values}),
    }
}

/// The `commands.list` result.
pub fn list_json() -> Value {
    let commands: Vec<Value> = COMMANDS
        .iter()
        .map(|c| {
            let params: Vec<Value> = c
                .params
                .iter()
                .map(|p| {
                    let mut v = kind_json(p.kind);
                    if let Some(o) = v.as_object_mut() {
                        o.insert("name".into(), json!(p.name));
                        o.insert("required".into(), json!(p.required));
                        o.insert("help".into(), json!(p.help));
                    }
                    v
                })
                .collect();
            let dialogs: Vec<Value> = c
                .dialogs
                .iter()
                .map(|d| json!({"name": d.name, "when": d.when, "answered_by": d.answered_by}))
                .collect();
            json!({
                "id": c.id, "menu": c.menu, "title": c.title, "shortcut": c.shortcut,
                "document": c.document, "params": params, "dialogs": dialogs,
            })
        })
        .collect();
    json!({ "commands": commands })
}
VIZDAW_EOF
```

- [ ] **Step 4: Validation**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-control/src/request.rs <<'VIZDAW_EOF'
//! Validated commands (spec §5.5): every parameter of every command is
//! checked here, in Rust, before anything reaches the app. Validation is
//! total: any [`RawRequest`] gives a [`Command`] or an error, never a panic.

use crate::element::{self, ElementRef};
use crate::error::{ControlError, ErrorCode};
use crate::keys::{KeyStroke, Modifiers, named_key, text_strokes};
use crate::params::{MAX_STRING, Params, WindowRef};
use crate::protocol::{RawRequest, truncate};
use crate::registry::{self, ArgValue, OPEN_DIALOGS};
use crate::wait::{DEFAULT_TIMEOUT_MS, DETAIL_KINDS, MAX_TIMEOUT_MS, WaitCondition};
use serde_json::{Map, Value};
use std::path::PathBuf;

/// Every command name.
pub const COMMAND_NAMES: [&str; 13] = [
    "commands.list",
    "command.run",
    "state.get",
    "ui.tree",
    "ui.click",
    "ui.drag",
    "ui.key",
    "ui.screenshot",
    "app.new_demo",
    "app.open",
    "app.quit",
    "wait",
    "ping",
];

/// The deepest `ui.tree` (and the default).
pub const MAX_TREE_DEPTH: u32 = 64;
/// The most intermediate points of a drag.
pub const MAX_DRAG_STEPS: u32 = 200;
/// Intermediate points of a drag when `steps` is absent.
pub const DEFAULT_DRAG_STEPS: u32 = 16;

/// What `state.get` reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StateTarget {
    /// The target document's transport.
    Transport,
    /// Its snapshot, sequences, lanes and pending alert.
    Document,
    /// Its editors: detail pane, torn-off windows, tool, snap, modes.
    Editor,
    /// Its selection.
    Selection,
    /// Every VizDAW window, plus app-level alerts.
    Windows,
}

/// Where a click or drag lands.
#[derive(Debug, Clone, PartialEq)]
pub enum Pointer {
    /// The centre of an element from `ui.tree`.
    Element(ElementRef),
    /// A point in window points, top-left origin (as in screenshots).
    Point {
        /// From the window's left edge.
        x: f64,
        /// From the window's top edge.
        y: f64,
    },
}

/// A mouse button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Button {
    /// Primary.
    Left,
    /// Secondary (context menus).
    Right,
}

/// An answer to the after-crash question.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CrashAnswer {
    /// Open Normally.
    Normal,
    /// Open with Plugins Off.
    Off,
    /// Disable the plugins that were loading, then open.
    Disable,
}

/// The answers to the dialogs opening a project may ask (spec §7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct RecoveryAnswers {
    /// "Open with Plugins Off?"
    pub plugins: Option<CrashAnswer>,
    /// "Restore Autosave"
    pub restore_autosave: Option<bool>,
}

/// A validated request.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Answered by the server: the registry.
    CommandsList,
    /// Answered by the server: `{ "pong": true }`.
    Ping,
    /// Runs a registered menu command.
    CommandRun {
        /// Registry ID.
        id: String,
        /// Validated arguments.
        args: Vec<(String, ArgValue)>,
        /// The window whose document it acts on.
        window: Option<WindowRef>,
    },
    /// Reads state.
    StateGet {
        /// What.
        target: StateTarget,
        /// Whose (a document window); absent: the front document.
        window: Option<WindowRef>,
    },
    /// A window's accessibility tree.
    UiTree {
        /// Which window; absent: the front document window.
        window: Option<WindowRef>,
        /// How deep.
        max_depth: u32,
    },
    /// A click.
    UiClick {
        /// Which window.
        window: Option<WindowRef>,
        /// Where.
        at: Pointer,
        /// Which button.
        button: Button,
        /// 1 to 3 (double, triple click).
        count: u32,
        /// Modifiers held.
        modifiers: Modifiers,
    },
    /// A drag.
    UiDrag {
        /// Which window.
        window: Option<WindowRef>,
        /// Press here.
        from: Pointer,
        /// Release here.
        to: Pointer,
        /// Intermediate points.
        steps: u32,
        /// Modifiers held.
        modifiers: Modifiers,
    },
    /// Key strokes.
    UiKey {
        /// Which window.
        window: Option<WindowRef>,
        /// Clicked first, to give it the keyboard.
        focus: Option<ElementRef>,
        /// In order.
        strokes: Vec<KeyStroke>,
    },
    /// A PNG of a window.
    UiScreenshot {
        /// Which window.
        window: Option<WindowRef>,
        /// Where; absent: a new file in the socket's directory.
        path: Option<PathBuf>,
        /// Pixels per point; absent: the window's backing scale.
        scale: Option<f64>,
    },
    /// Creates the demo project in a new window.
    AppNewDemo {
        /// The demo with an audio lane.
        audio: bool,
    },
    /// Opens a project in a new window.
    AppOpen {
        /// The package.
        path: PathBuf,
        /// Answers to the recovery dialogs.
        answers: RecoveryAnswers,
    },
    /// Quits the app (after replying).
    AppQuit,
    /// Waits for a condition.
    Wait {
        /// What.
        condition: WaitCondition,
        /// Whose (for document conditions).
        window: Option<WindowRef>,
        /// At most this long.
        timeout_ms: u64,
    },
}

impl Command {
    /// Whether the server answers it without the app.
    pub fn is_local(&self) -> bool {
        matches!(self, Command::CommandsList | Command::Ping)
    }
}

/// Validates `raw` into a command.
pub fn validate(raw: &RawRequest) -> Result<Command, ControlError> {
    let p = Params::new(&raw.cmd, &raw.params);
    match raw.cmd.as_str() {
        "commands.list" => {
            p.only(&[])?;
            Ok(Command::CommandsList)
        }
        "ping" => {
            p.only(&[])?;
            Ok(Command::Ping)
        }
        "command.run" => command_run(&p, &raw.params),
        "state.get" => {
            p.only(&["target", "window"])?;
            let target = match p.str("target", 32)?.as_str() {
                "transport" => StateTarget::Transport,
                "document" => StateTarget::Document,
                "editor" => StateTarget::Editor,
                "selection" => StateTarget::Selection,
                "windows" => StateTarget::Windows,
                _ => {
                    return Err(p.error(
                        "`target` must be transport, document, editor, selection or windows",
                    ));
                }
            };
            Ok(Command::StateGet {
                target,
                window: p.window()?,
            })
        }
        "ui.tree" => {
            p.only(&["window", "max_depth"])?;
            Ok(Command::UiTree {
                window: p.window()?,
                max_depth: p
                    .opt_int("max_depth", 1, i64::from(MAX_TREE_DEPTH))?
                    .map_or(MAX_TREE_DEPTH, |d| d as u32),
            })
        }
        "ui.click" => {
            p.only(&["window", "element", "point", "button", "count", "modifiers"])?;
            let at = match (p.raw("element"), p.raw("point")) {
                (Some(e), None) => pointer(&p, "element", e)?,
                (None, Some(pt)) => {
                    let (x, y) = p.point_of("point", pt)?;
                    Pointer::Point { x, y }
                }
                _ => return Err(p.error("give exactly one of `element` and `point`")),
            };
            let button = match p.opt_str("button", 8)?.as_deref() {
                None | Some("left") => Button::Left,
                Some("right") => Button::Right,
                Some(_) => return Err(p.error("`button` must be left or right")),
            };
            Ok(Command::UiClick {
                window: p.window()?,
                at,
                button,
                count: p.opt_int("count", 1, 3)?.map_or(1, |c| c as u32),
                modifiers: modifiers(&p)?,
            })
        }
        "ui.drag" => {
            p.only(&["window", "from", "to", "steps", "modifiers"])?;
            let from = p.raw("from").ok_or_else(|| p.error("missing `from`"))?;
            let to = p.raw("to").ok_or_else(|| p.error("missing `to`"))?;
            Ok(Command::UiDrag {
                window: p.window()?,
                from: pointer(&p, "from", from)?,
                to: pointer(&p, "to", to)?,
                steps: p
                    .opt_int("steps", 1, i64::from(MAX_DRAG_STEPS))?
                    .map_or(DEFAULT_DRAG_STEPS, |s| s as u32),
                modifiers: modifiers(&p)?,
            })
        }
        "ui.key" => {
            p.only(&["window", "key", "text", "modifiers", "element"])?;
            let m = modifiers(&p)?;
            let strokes = match (p.opt_str("key", 32)?, p.opt_str("text", MAX_STRING)?) {
                (Some(k), None) => vec![named_key(&k, m).map_err(|e| p.error(e))?],
                (None, Some(t)) if !p.has("modifiers") => {
                    text_strokes(&t).map_err(|e| p.error(e))?
                }
                (None, Some(_)) => return Err(p.error("`modifiers` go with `key`, not `text`")),
                _ => return Err(p.error("give exactly one of `key` and `text`")),
            };
            let focus = match p.opt_str("element", 512)? {
                None => None,
                Some(e) => Some(element::parse(&e).map_err(|m| p.error(m))?),
            };
            Ok(Command::UiKey {
                window: p.window()?,
                focus,
                strokes,
            })
        }
        "ui.screenshot" => {
            p.only(&["window", "path", "scale"])?;
            let path = p.opt_path("path", "png")?;
            if let Some(dir) = path.as_ref().and_then(|p| p.parent()) {
                if !dir.is_dir() {
                    return Err(
                        p.error(format!("`path`'s folder {} does not exist", dir.display()))
                    );
                }
            }
            Ok(Command::UiScreenshot {
                window: p.window()?,
                path,
                scale: p.opt_num("scale", 0.25, 4.0)?,
            })
        }
        "app.new_demo" => {
            p.only(&["audio"])?;
            Ok(Command::AppNewDemo {
                audio: p.opt_bool("audio")?.unwrap_or(false),
            })
        }
        "app.open" => {
            p.only(&["path", "plugins", "restore_autosave"])?;
            let Some(path) = p.opt_path("path", "vizdaw")? else {
                return match OPEN_DIALOGS.first() {
                    Some(d) => Err(ControlError::dialog(d.name, d.answered_by)),
                    None => Err(p.error("missing `path`")),
                };
            };
            let plugins = match p.opt_str("plugins", 16)?.as_deref() {
                None => None,
                Some("normal") => Some(CrashAnswer::Normal),
                Some("off") => Some(CrashAnswer::Off),
                Some("disable") => Some(CrashAnswer::Disable),
                Some(_) => return Err(p.error("`plugins` must be normal, off or disable")),
            };
            Ok(Command::AppOpen {
                path,
                answers: RecoveryAnswers {
                    plugins,
                    restore_autosave: p.opt_bool("restore_autosave")?,
                },
            })
        }
        "app.quit" => {
            p.only(&[])?;
            Ok(Command::AppQuit)
        }
        "wait" => wait(&p),
        other => Err(ControlError::new(
            ErrorCode::UnknownCommand,
            format!(
                "no command {:?} (commands: {})",
                truncate(other, 64),
                COMMAND_NAMES.join(", ")
            ),
        )),
    }
}

fn modifiers(p: &Params<'_>) -> Result<Modifiers, ControlError> {
    Modifiers::parse(p.raw("modifiers")).map_err(|e| p.error(e))
}

/// An element ID (a string) or a point (`{x, y}`).
fn pointer(p: &Params<'_>, name: &str, v: &Value) -> Result<Pointer, ControlError> {
    match v {
        Value::String(s) => element::parse(s)
            .map(Pointer::Element)
            .map_err(|m| p.error(format!("`{name}`: {m}"))),
        _ => {
            let (x, y) = p.point_of(name, v)?;
            Ok(Pointer::Point { x, y })
        }
    }
}

fn command_run(p: &Params<'_>, all: &Map<String, Value>) -> Result<Command, ControlError> {
    p.only(&["id", "params", "window"])?;
    let id = p.str("id", 64)?;
    let spec = registry::find(&id).ok_or_else(|| registry::unknown(&id))?;
    let empty = Map::new();
    let args = match all.get("params") {
        None | Some(Value::Null) => &empty,
        Some(Value::Object(m)) => m,
        Some(_) => return Err(p.error("`params` must be an object")),
    };
    Ok(Command::CommandRun {
        id,
        args: registry::validate_args(spec, args)?,
        window: p.window()?,
    })
}

fn wait(p: &Params<'_>) -> Result<Command, ControlError> {
    p.only(&[
        "condition",
        "tick",
        "count",
        "title",
        "kind",
        "window",
        "timeout_ms",
    ])?;
    let name = p.str("condition", 32)?;
    let (condition, extra): (WaitCondition, &[&str]) = match name.as_str() {
        "playing" => (WaitCondition::Playing, &[]),
        "stopped" => (WaitCondition::Stopped, &[]),
        "tick_at_least" => (
            WaitCondition::TickAtLeast(p.int("tick", 0, i64::MAX / 2)?),
            &["tick"],
        ),
        "documents_at_least" => (
            WaitCondition::DocumentsAtLeast(p.int("count", 0, 1000)? as u32),
            &["count"],
        ),
        "window_titled" | "window_gone" => {
            let t = p.str("title", 256)?;
            if t.is_empty() {
                return Err(p.error("`title` must not be empty"));
            }
            let c = if name == "window_titled" {
                WaitCondition::WindowTitled(t)
            } else {
                WaitCondition::WindowGone(t)
            };
            (c, &["title"])
        }
        "detail_kind" => {
            let k = p.str("kind", 32)?;
            if !DETAIL_KINDS.contains(&k.as_str()) {
                return Err(p.error(format!("`kind` must be one of {}", DETAIL_KINDS.join(", "))));
            }
            (WaitCondition::DetailKind(k), &["kind"])
        }
        _ => {
            return Err(p.error(
                "`condition` must be playing, stopped, tick_at_least, documents_at_least, window_titled, window_gone or detail_kind",
            ));
        }
    };
    for k in ["tick", "count", "title", "kind"] {
        if p.has(k) && !extra.contains(&k) {
            return Err(p.error(format!("`{k}` does not go with condition {name}")));
        }
    }
    Ok(Command::Wait {
        condition,
        window: p.window()?,
        timeout_ms: p
            .opt_int("timeout_ms", 1, MAX_TIMEOUT_MS as i64)?
            .map_or(DEFAULT_TIMEOUT_MS, |t| t as u64),
    })
}
VIZDAW_EOF
```

In `crates/core-control/src/lib.rs`, Find:

```rust
pub mod protocol;

pub use error::{ControlError, ErrorCode};
pub use protocol::{RawRequest, RequestId};
```

Replace:

```rust
pub mod protocol;
pub mod registry;
pub mod request;
pub mod wait;

pub use error::{ControlError, ErrorCode};
pub use protocol::{RawRequest, RequestId};
pub use request::{Command, validate};
```

- [ ] **Step 5: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-control 2>&1 | grep -E "^test result|FAILED|panicked"
```

Expected: PASS (protocol 13; commands 12, of which 2 property tests of 2048 cases each, a few seconds).

- [ ] **Step 6: Gate and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace 2>&1 | grep -E "^test result|FAILED|panicked" | tail -60
git add crates/core-control
git commit -m "feat(control): validate every command; menu-command registry; wait conditions

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
```

Expected: 785 Rust tests listed.

---

### Task 4: The control socket — private directory, call queue, timeouts — and a client

**Files:**
- Create: `crates/core-control/src/server.rs`, `crates/core-control/src/client.rs`
- Modify: `crates/core-control/src/lib.rs`
- Test: `crates/core-control/tests/server.rs`

**Interfaces:**
- Consumes: `read_request`, `reply_line`, `request_line`, `read_reply`, `validate`, `registry::list_json`, `ControlError`.
- Produces:
  - `server::MAX_SOCKET_PATH = 103`; `Config { max_connections: 4, app_timeout: 60 s, wait_grace: 15 s, idle_timeout: 300 s, write_timeout: 30 s }` (`Default`); `Call { token: u64, command: Command }`; `StartError(pub String)`; `Server::start(&Path, Config) -> Result<Server, StartError>`, `path()`, `dir()`, `next_call() -> Option<Call>`, `reply(token, Result<Value, ControlError>) -> bool`, `is_waiting(token) -> bool`, `screenshot_path() -> PathBuf` (`<dir>/screenshot-<n>.png`), `stop()` (idempotent; also on drop).
  - `client::{Client, CallError::{Io, Reply(ControlError), Protocol(String)}}`: `Client::connect(&Path)`, `call(cmd, Value, Duration) -> Result<Value, CallError>`, `send_line(&str)`, `read_reply_for(&RequestId, Duration)`, `read_line()`.
  - Re-exports `core_control::{Call, Config, Server, StartError}`.

Spec §4.2, §4.4, §5.1, §8.

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-control/tests/server.rs <<'VIZDAW_EOF'
//! The control socket: private directory, stale sockets, calls, limits,
//! timeouts and shutdown (spec §4.2, §8). Success paths wait generously
//! (slow CI); only deliberate timeouts are short.

use core_control::client::{CallError, Client};
use core_control::protocol::MAX_LINE;
use core_control::server::{Config, Server};
use core_control::{Command, ErrorCode};
use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

const LONG: Duration = Duration::from_secs(20);

/// A short private directory (socket paths are limited to 103 bytes).
fn private_dir() -> tempfile::TempDir {
    let d = tempfile::Builder::new()
        .prefix("vzc")
        .tempdir_in("/tmp")
        .unwrap();
    std::fs::set_permissions(d.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    d
}

fn sock(d: &tempfile::TempDir) -> PathBuf {
    d.path().join("ctl").join("s.sock")
}

/// Answers every call with `answer(command)` until `stop` is set.
fn fake_app(
    server: Arc<std::sync::Mutex<Server>>,
    stop: Arc<AtomicBool>,
    answer: impl Fn(&Command) -> Option<Value> + Send + 'static,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        while !stop.load(Ordering::SeqCst) {
            let call = server.lock().unwrap().next_call();
            match call {
                Some(c) => {
                    if let Some(v) = answer(&c.command) {
                        server.lock().unwrap().reply(c.token, Ok(v));
                    }
                }
                None => thread::sleep(Duration::from_millis(1)),
            }
        }
    })
}

fn mode(p: &Path) -> u32 {
    std::fs::metadata(p).unwrap().permissions().mode() & 0o777
}

#[test]
fn it_listens_in_a_private_directory_and_cleans_up() {
    let d = private_dir();
    let path = sock(&d);
    let mut s = Server::start(&path, Config::default()).unwrap();
    assert_eq!(
        mode(path.parent().unwrap()),
        0o700,
        "the directory it made is private"
    );
    assert_eq!(mode(&path), 0o600, "the socket is the user's only");
    assert_eq!(s.dir(), path.parent().unwrap());
    assert!(s.screenshot_path().starts_with(path.parent().unwrap()));
    s.stop();
    assert!(!path.exists(), "the socket file goes on stop");
}

#[test]
fn unsafe_places_are_refused() {
    let d = private_dir();
    let open = d.path().join("open");
    std::fs::create_dir(&open).unwrap();
    std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o755)).unwrap();
    let e = Server::start(&open.join("s.sock"), Config::default())
        .err()
        .unwrap();
    assert!(e.0.contains("open to other users"), "{e}");
    assert!(Server::start(Path::new("rel/s.sock"), Config::default()).is_err());
    let long = d.path().join("x".repeat(120)).join("s.sock");
    assert!(
        Server::start(&long, Config::default())
            .err()
            .unwrap()
            .0
            .contains("at most 103")
    );
    let file = d.path().join("f.sock");
    std::fs::write(&file, b"").unwrap();
    assert!(
        Server::start(&file, Config::default())
            .err()
            .unwrap()
            .0
            .contains("not a socket")
    );
}

#[test]
fn a_stale_socket_is_replaced_but_a_live_one_is_not() {
    let d = private_dir();
    let path = sock(&d);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::set_permissions(
        path.parent().unwrap(),
        std::fs::Permissions::from_mode(0o700),
    )
    .unwrap();
    drop(UnixListener::bind(&path).unwrap()); // leaves a dead socket file
    let live = Server::start(&path, Config::default()).unwrap();
    let e = Server::start(&path, Config::default()).err().unwrap();
    assert!(e.0.contains("another VizDAW"), "{e}");
    drop(live);
}

#[test]
fn calls_reach_the_app_and_local_commands_do_not() {
    let d = private_dir();
    let server = Arc::new(std::sync::Mutex::new(
        Server::start(&sock(&d), Config::default()).unwrap(),
    ));
    let stop = Arc::new(AtomicBool::new(false));
    let app = fake_app(Arc::clone(&server), Arc::clone(&stop), |c| match c {
        Command::StateGet { .. } => Some(json!({"playing": false})),
        other => panic!("the app got {other:?}"),
    });
    let mut c = Client::connect(&sock(&d)).unwrap();
    assert_eq!(
        c.call("state.get", json!({"target": "transport"}), LONG)
            .unwrap(),
        json!({"playing": false})
    );
    let list = c.call("commands.list", json!({}), LONG).unwrap();
    assert!(list["commands"].as_array().unwrap().len() >= 38);
    assert_eq!(
        c.call("ping", json!({}), LONG).unwrap(),
        json!({"pong": true})
    );
    match c.call("state.get", json!({"target": "nope"}), LONG) {
        Err(CallError::Reply(e)) => assert_eq!(e.code, ErrorCode::InvalidParams),
        other => panic!("{other:?}"),
    }
    stop.store(true, Ordering::SeqCst);
    app.join().unwrap();
}

#[test]
fn garbage_and_overlong_lines_get_errors_and_the_connection_survives() {
    let d = private_dir();
    let _s = Server::start(&sock(&d), Config::default()).unwrap();
    let mut c = Client::connect(&sock(&d)).unwrap();
    c.send_line("this is not json").unwrap();
    let v: Value = serde_json::from_str(&c.read_line().unwrap()).unwrap();
    assert_eq!(v["error"]["code"], "parse_error");
    assert_eq!(v["id"], Value::Null);
    c.send_line(&"x".repeat(MAX_LINE + 10)).unwrap();
    let v: Value = serde_json::from_str(&c.read_line().unwrap()).unwrap();
    assert_eq!(v["error"]["code"], "parse_error");
    c.send_line("").unwrap(); // blank lines are skipped
    assert_eq!(
        c.call("ping", json!({}), LONG).unwrap(),
        json!({"pong": true})
    );
}

#[test]
fn a_call_the_app_never_answers_times_out_and_the_late_answer_is_dropped() {
    let d = private_dir();
    let config = Config {
        app_timeout: Duration::from_millis(200),
        wait_grace: Duration::from_millis(100),
        ..Config::default()
    };
    let s = Server::start(&sock(&d), config).unwrap();
    let mut c = Client::connect(&sock(&d)).unwrap();
    let t0 = Instant::now();
    match c.call("app.quit", json!({}), LONG) {
        Err(CallError::Reply(e)) => assert_eq!(e.code, ErrorCode::Timeout),
        other => panic!("{other:?}"),
    }
    assert!(t0.elapsed() < Duration::from_secs(10));
    // The app takes the call late: nobody waits for it any more.
    assert!(
        s.next_call().is_none(),
        "a call nobody waits for is skipped"
    );
    // A wait gets its own timeout plus the grace.
    let t0 = Instant::now();
    match c.call(
        "wait",
        json!({"condition": "playing", "timeout_ms": 300}),
        LONG,
    ) {
        Err(CallError::Reply(e)) => assert_eq!(e.code, ErrorCode::Timeout),
        other => panic!("{other:?}"),
    }
    assert!(
        t0.elapsed() >= Duration::from_millis(400),
        "{:?}",
        t0.elapsed()
    );
}

#[test]
fn too_many_connections_are_turned_away() {
    let d = private_dir();
    let config = Config {
        max_connections: 2,
        ..Config::default()
    };
    let _s = Server::start(&sock(&d), config).unwrap();
    let mut a = Client::connect(&sock(&d)).unwrap();
    let mut b = Client::connect(&sock(&d)).unwrap();
    // Both are served (each answers a ping), so both count.
    a.call("ping", json!({}), LONG).unwrap();
    b.call("ping", json!({}), LONG).unwrap();
    let mut c = Client::connect(&sock(&d)).unwrap();
    let v: Value = serde_json::from_str(&c.read_line().unwrap()).unwrap();
    assert_eq!(v["error"]["code"], "unavailable");
    drop(a);
    // Once a connection closes another is served (its thread ends first).
    let deadline = Instant::now() + LONG;
    loop {
        let mut d2 = Client::connect(&sock(&d)).unwrap();
        if d2.call("ping", json!({}), Duration::from_secs(5)).is_ok() {
            break;
        }
        assert!(Instant::now() < deadline, "a freed slot was never reused");
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn stopping_answers_pending_calls() {
    let d = private_dir();
    let mut s = Server::start(&sock(&d), Config::default()).unwrap();
    let path = sock(&d);
    let caller = thread::spawn(move || {
        let mut c = Client::connect(&path).unwrap();
        c.call("app.quit", json!({}), LONG)
    });
    let deadline = Instant::now() + LONG;
    let call = loop {
        if let Some(c) = s.next_call() {
            break c;
        }
        assert!(Instant::now() < deadline, "the call never arrived");
        thread::sleep(Duration::from_millis(1));
    };
    assert_eq!(call.command, Command::AppQuit);
    s.stop();
    match caller.join().unwrap() {
        Err(CallError::Reply(e)) => assert_eq!(e.code, ErrorCode::ShuttingDown),
        other => panic!("{other:?}"),
    }
    assert!(!s.reply(call.token, Ok(json!({}))), "too late");
}

#[test]
fn parallel_clients_get_their_own_replies() {
    let d = private_dir();
    let server = Arc::new(std::sync::Mutex::new(
        Server::start(&sock(&d), Config::default()).unwrap(),
    ));
    let stop = Arc::new(AtomicBool::new(false));
    let app = fake_app(Arc::clone(&server), Arc::clone(&stop), |c| match c {
        Command::Wait { timeout_ms, .. } => Some(json!({"elapsed_ms": timeout_ms})),
        _ => None,
    });
    let handles: Vec<_> = (1..=3u64)
        .map(|k| {
            let path = sock(&d);
            thread::spawn(move || {
                let mut c = Client::connect(&path).unwrap();
                for _ in 0..20 {
                    let v = c
                        .call(
                            "wait",
                            json!({"condition": "playing", "timeout_ms": k}),
                            LONG,
                        )
                        .unwrap();
                    assert_eq!(v["elapsed_ms"], k);
                }
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    stop.store(true, Ordering::SeqCst);
    app.join().unwrap();
}
VIZDAW_EOF
cargo test -p core-control --test server 2>&1 | grep -E "^error" | head -3
```

Expected: does not compile (`could not find client in core_control`).

- [ ] **Step 2: The server**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-control/src/server.rs <<'VIZDAW_EOF'
//! The control socket (spec §4.2, §8): a Unix domain socket in a private
//! directory, one thread accepting and one thread per connection. A
//! connection reads a line, validates it, answers `commands.list` and
//! `ping` itself and queues anything else as a [`Call`] for the app,
//! which takes calls with [`Server::next_call`] on its main thread and
//! answers with [`Server::reply`]. A call the app does not answer in time
//! is answered `timeout`; the app's late answer is dropped.
//!
//! Nothing here panics on input: every line becomes a reply.

use crate::error::{ControlError, ErrorCode};
use crate::protocol::{MAX_LINE, RequestId, read_request, reply_line};
use crate::registry;
use crate::request::{Command, validate};
use serde_json::{Value, json};
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

/// The longest socket path macOS accepts (`sun_path` is 104 bytes with
/// its terminating NUL).
pub const MAX_SOCKET_PATH: usize = 103;

/// Limits and timeouts.
#[derive(Debug, Clone)]
pub struct Config {
    /// Connections served at once; more get one `unavailable` line.
    pub max_connections: usize,
    /// How long a call may wait for the app.
    pub app_timeout: Duration,
    /// Added to a wait's own timeout before the server gives up on it.
    pub wait_grace: Duration,
    /// A connection idle this long (mid-line or between lines) is closed.
    pub idle_timeout: Duration,
    /// A reply the client does not read within this is dropped with
    /// the connection.
    pub write_timeout: Duration,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            max_connections: 4,
            app_timeout: Duration::from_secs(60),
            wait_grace: Duration::from_secs(15),
            idle_timeout: Duration::from_secs(300),
            write_timeout: Duration::from_secs(30),
        }
    }
}

/// A request for the app.
#[derive(Debug, Clone, PartialEq)]
pub struct Call {
    /// Answer with this.
    pub token: u64,
    /// What to do.
    pub command: Command,
}

type Outcome = Result<Value, ControlError>;

struct Shared {
    config: Config,
    path: PathBuf,
    dir: PathBuf,
    queue: Mutex<VecDeque<Call>>,
    waiting: Mutex<HashMap<u64, Sender<Outcome>>>,
    streams: Mutex<HashMap<u64, UnixStream>>,
    next_token: AtomicU64,
    next_conn: AtomicU64,
    shots: AtomicU64,
    connections: AtomicUsize,
    stopping: AtomicBool,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Why the server did not start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartError(pub String);

impl std::fmt::Display for StartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "control channel: {}", self.0)
    }
}

impl std::error::Error for StartError {}

/// The listening control channel. Dropping it stops it.
pub struct Server {
    shared: Arc<Shared>,
    acceptor: Option<JoinHandle<()>>,
}

/// Checks (or makes) the socket's private directory and clears a stale
/// socket; returns the directory.
fn prepare(path: &Path) -> Result<PathBuf, StartError> {
    let fail = |m: String| Err(StartError(m));
    if !path.is_absolute() {
        return fail(format!("{} is not an absolute path", path.display()));
    }
    let len = path.as_os_str().len();
    if len > MAX_SOCKET_PATH {
        return fail(format!(
            "{} is {len} bytes long; a socket path may have at most {MAX_SOCKET_PATH}",
            path.display()
        ));
    }
    let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) else {
        return fail(format!("{} has no directory", path.display()));
    };
    match fs::symlink_metadata(dir) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(dir)
                .map_err(|e| StartError(format!("cannot create {}: {e}", dir.display())))?;
        }
        Err(e) => return fail(format!("cannot read {}: {e}", dir.display())),
        Ok(m) if m.file_type().is_symlink() || !m.is_dir() => {
            return fail(format!("{} is not a directory", dir.display()));
        }
        Ok(_) => {}
    }
    let mode = fs::metadata(dir)
        .map_err(|e| StartError(format!("cannot read {}: {e}", dir.display())))?
        .permissions()
        .mode();
    if mode & 0o077 != 0 {
        return fail(format!(
            "{} is open to other users (mode {:o}); the socket needs a private directory (chmod 700)",
            dir.display(),
            mode & 0o777
        ));
    }
    match fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_socket() => {
            if UnixStream::connect(path).is_ok() {
                return fail(format!("another VizDAW is listening on {}", path.display()));
            }
            fs::remove_file(path).map_err(|e| {
                StartError(format!("cannot remove the stale {}: {e}", path.display()))
            })?;
        }
        Ok(_) => return fail(format!("{} exists and is not a socket", path.display())),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return fail(format!("cannot read {}: {e}", path.display())),
    }
    Ok(dir.to_path_buf())
}

impl Server {
    /// Listens on `path` (spec §8): the directory is created 0700 or must
    /// already be private and owned by this user; a stale socket is
    /// removed; the socket is made 0600.
    pub fn start(path: &Path, config: Config) -> Result<Server, StartError> {
        let dir = prepare(path)?;
        let listener = UnixListener::bind(path)
            .map_err(|e| StartError(format!("cannot listen on {}: {e}", path.display())))?;
        let fail = |m: String| {
            let _ = fs::remove_file(path);
            Err(StartError(m))
        };
        if let Err(e) = fs::set_permissions(path, fs::Permissions::from_mode(0o600)) {
            return fail(format!("cannot make {} private: {e}", path.display()));
        }
        // The socket was made by this process: its owner is this user.
        match (fs::metadata(path), fs::metadata(&dir)) {
            (Ok(s), Ok(d)) if s.uid() == d.uid() => {}
            (Ok(_), Ok(_)) => return fail(format!("{} belongs to another user", dir.display())),
            (Err(e), _) | (_, Err(e)) => {
                return fail(format!("cannot read {}: {e}", dir.display()));
            }
        }
        let shared = Arc::new(Shared {
            config,
            path: path.to_path_buf(),
            dir,
            queue: Mutex::default(),
            waiting: Mutex::default(),
            streams: Mutex::default(),
            next_token: AtomicU64::new(1),
            next_conn: AtomicU64::new(1),
            shots: AtomicU64::new(0),
            connections: AtomicUsize::new(0),
            stopping: AtomicBool::new(false),
        });
        let s = Arc::clone(&shared);
        let acceptor = std::thread::Builder::new()
            .name("vizdaw-control".into())
            .spawn(move || accept(&s, &listener))
            .map_err(|e| StartError(format!("cannot start the control thread: {e}")))?;
        Ok(Server {
            shared,
            acceptor: Some(acceptor),
        })
    }

    /// The socket's path.
    pub fn path(&self) -> &Path {
        &self.shared.path
    }

    /// The socket's (private) directory.
    pub fn dir(&self) -> &Path {
        &self.shared.dir
    }

    /// The oldest call nobody answered yet (calls whose client gave up are
    /// skipped).
    pub fn next_call(&self) -> Option<Call> {
        let mut q = lock(&self.shared.queue);
        while let Some(c) = q.pop_front() {
            if self.is_waiting(c.token) {
                return Some(c);
            }
        }
        None
    }

    /// Answers call `token`; false when nobody waits for it any more (it
    /// timed out or its client left).
    pub fn reply(&self, token: u64, outcome: Result<Value, ControlError>) -> bool {
        let Some(tx) = lock(&self.shared.waiting).remove(&token) else {
            return false;
        };
        tx.send(outcome).is_ok()
    }

    /// Whether a client still waits for call `token`.
    pub fn is_waiting(&self, token: u64) -> bool {
        lock(&self.shared.waiting).contains_key(&token)
    }

    /// A new screenshot path in the socket's directory.
    pub fn screenshot_path(&self) -> PathBuf {
        let n = self.shared.shots.fetch_add(1, Ordering::Relaxed) + 1;
        self.shared.dir.join(format!("screenshot-{n}.png"))
    }

    /// Stops: pending calls are answered `shutting_down`, connections
    /// close, the socket file goes. Idempotent.
    pub fn stop(&mut self) {
        let s = &self.shared;
        if s.stopping.swap(true, Ordering::SeqCst) {
            return;
        }
        let waiting: Vec<_> = lock(&s.waiting).drain().collect();
        for (_, tx) in waiting {
            let _ = tx.send(Err(ControlError::new(
                ErrorCode::ShuttingDown,
                "VizDAW is quitting",
            )));
        }
        lock(&s.queue).clear();
        // Wake the acceptor, then end every connection's read.
        let _ = UnixStream::connect(&s.path);
        if let Some(h) = self.acceptor.take() {
            let _ = h.join();
        }
        for (_, st) in lock(&s.streams).drain() {
            let _ = st.shutdown(std::net::Shutdown::Read);
        }
        let _ = fs::remove_file(&s.path);
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop();
    }
}

fn accept(s: &Arc<Shared>, listener: &UnixListener) {
    for stream in listener.incoming() {
        if s.stopping.load(Ordering::SeqCst) {
            return;
        }
        let Ok(stream) = stream else { continue };
        let n = s.connections.fetch_add(1, Ordering::SeqCst);
        if n >= s.config.max_connections {
            s.connections.fetch_sub(1, Ordering::SeqCst);
            let e = ControlError::new(
                ErrorCode::Unavailable,
                format!(
                    "too many control connections (at most {})",
                    s.config.max_connections
                ),
            );
            let mut w = stream;
            let _ = w.set_write_timeout(Some(Duration::from_secs(1)));
            let _ = writeln!(w, "{}", reply_line(None, &Err(e)));
            continue;
        }
        let conn = s.next_conn.fetch_add(1, Ordering::Relaxed);
        if let Ok(c) = stream.try_clone() {
            lock(&s.streams).insert(conn, c);
        }
        let s2 = Arc::clone(s);
        let spawned = std::thread::Builder::new()
            .name("vizdaw-control-conn".into())
            .spawn(move || {
                serve(&s2, stream);
                lock(&s2.streams).remove(&conn);
                s2.connections.fetch_sub(1, Ordering::SeqCst);
            });
        if spawned.is_err() {
            lock(&s.streams).remove(&conn);
            s.connections.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

/// A line read with a length cap.
enum Line {
    Full(Vec<u8>),
    TooLong,
}

/// Reads up to a newline; a line over `max` bytes is consumed and
/// reported as [`Line::TooLong`]. None at end of input.
fn read_line_capped(r: &mut impl BufRead, max: usize) -> io::Result<Option<Line>> {
    let mut buf = Vec::new();
    let mut too_long = false;
    loop {
        let (used, done, eof) = {
            let available = match r.fill_buf() {
                Ok(b) => b,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            };
            if available.is_empty() {
                (0, true, true)
            } else {
                match available.iter().position(|&b| b == b'\n') {
                    Some(i) => {
                        if !too_long {
                            buf.extend_from_slice(available.get(..i).unwrap_or(&[]));
                        }
                        (i + 1, true, false)
                    }
                    None => {
                        if !too_long {
                            buf.extend_from_slice(available);
                        }
                        (available.len(), false, false)
                    }
                }
            }
        };
        r.consume(used);
        if buf.len() > max {
            too_long = true;
            buf = Vec::new();
        }
        if done {
            if eof && buf.is_empty() && !too_long {
                return Ok(None);
            }
            return Ok(Some(if too_long {
                Line::TooLong
            } else {
                Line::Full(buf)
            }));
        }
    }
}

fn serve(s: &Arc<Shared>, stream: UnixStream) {
    let _ = stream.set_read_timeout(Some(s.config.idle_timeout));
    let _ = stream.set_write_timeout(Some(s.config.write_timeout));
    let Ok(read_half) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::with_capacity(64 * 1024, read_half);
    let mut writer = stream;
    loop {
        let (id, outcome) = match read_line_capped(&mut reader, MAX_LINE) {
            Ok(None) | Err(_) => return,
            Ok(Some(Line::TooLong)) => (
                None,
                Err(ControlError::new(
                    ErrorCode::ParseError,
                    format!("request line longer than {MAX_LINE} bytes"),
                )),
            ),
            Ok(Some(Line::Full(line))) => {
                if line.iter().all(u8::is_ascii_whitespace) {
                    continue;
                }
                handle(s, &line)
            }
        };
        let text = reply_line(id.as_ref(), &outcome);
        if writer
            .write_all(text.as_bytes())
            .and_then(|()| writer.write_all(b"\n"))
            .and_then(|()| writer.flush())
            .is_err()
        {
            return;
        }
        if s.stopping.load(Ordering::SeqCst) {
            return;
        }
    }
}

fn handle(s: &Arc<Shared>, line: &[u8]) -> (Option<RequestId>, Result<Value, ControlError>) {
    let raw = match read_request(line) {
        Ok(r) => r,
        Err(e) => return (e.id, Err(e.error)),
    };
    let id = Some(raw.id.clone());
    if s.stopping.load(Ordering::SeqCst) {
        return (
            id,
            Err(ControlError::new(
                ErrorCode::ShuttingDown,
                "VizDAW is quitting",
            )),
        );
    }
    let command = match validate(&raw) {
        Ok(c) => c,
        Err(e) => return (id, Err(e)),
    };
    match command {
        Command::CommandsList => return (id, Ok(registry::list_json())),
        Command::Ping => return (id, Ok(json!({ "pong": true }))),
        _ => {}
    }
    let timeout = match &command {
        Command::Wait { timeout_ms, .. } => {
            Duration::from_millis(*timeout_ms) + s.config.wait_grace
        }
        _ => s.config.app_timeout,
    };
    let token = s.next_token.fetch_add(1, Ordering::Relaxed);
    let (tx, rx) = mpsc::channel();
    lock(&s.waiting).insert(token, tx);
    if s.stopping.load(Ordering::SeqCst) {
        // `stop` drained the waiters before this one was added.
        lock(&s.waiting).remove(&token);
        return (
            id,
            Err(ControlError::new(
                ErrorCode::ShuttingDown,
                "VizDAW is quitting",
            )),
        );
    }
    lock(&s.queue).push_back(Call { token, command });
    let outcome = rx.recv_timeout(timeout).unwrap_or_else(|_| {
        Err(ControlError::new(
            ErrorCode::Timeout,
            format!(
                "VizDAW did not answer within {} s (is its main thread blocked?)",
                timeout.as_secs()
            ),
        ))
    });
    lock(&s.waiting).remove(&token);
    (id, outcome)
}
VIZDAW_EOF
```

- [ ] **Step 3: The client**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-control/src/client.rs <<'VIZDAW_EOF'
//! A blocking client of the control socket (`vizctl`, tests).

use crate::error::ControlError;
use crate::protocol::{RequestId, read_reply, request_line};
use serde_json::Value;
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

/// Why a call failed.
#[derive(Debug)]
pub enum CallError {
    /// The socket failed (no VizDAW listening, closed, timed out).
    Io(io::Error),
    /// VizDAW answered with an error.
    Reply(ControlError),
    /// The answer was not a reply.
    Protocol(String),
}

impl std::fmt::Display for CallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CallError::Io(e) => write!(f, "control socket: {e}"),
            CallError::Reply(e) => write!(f, "{e}"),
            CallError::Protocol(m) => write!(f, "control protocol: {m}"),
        }
    }
}

impl std::error::Error for CallError {}

/// One connection.
pub struct Client {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
    next: i64,
}

impl Client {
    /// Connects to the socket at `path`.
    pub fn connect(path: &Path) -> io::Result<Client> {
        let writer = UnixStream::connect(path)?;
        let reader = BufReader::new(writer.try_clone()?);
        Ok(Client {
            reader,
            writer,
            next: 1,
        })
    }

    /// Sends `cmd` with `params` and waits up to `timeout` for its reply.
    pub fn call(
        &mut self,
        cmd: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, CallError> {
        let id = RequestId::Num(self.next);
        self.next += 1;
        self.send_line(&request_line(&id, cmd, &params))?;
        self.read_reply_for(&id, timeout)
    }

    /// Sends a raw line (tests send malformed ones).
    pub fn send_line(&mut self, line: &str) -> Result<(), CallError> {
        self.writer
            .write_all(line.as_bytes())
            .and_then(|()| self.writer.write_all(b"\n"))
            .and_then(|()| self.writer.flush())
            .map_err(CallError::Io)
    }

    /// Reads replies until the one for `id` (or a reply without id, which
    /// is a connection-level error).
    pub fn read_reply_for(
        &mut self,
        id: &RequestId,
        timeout: Duration,
    ) -> Result<Value, CallError> {
        self.writer
            .set_read_timeout(Some(timeout.max(Duration::from_millis(1))))
            .map_err(CallError::Io)?;
        loop {
            let line = self.read_line()?;
            let reply = read_reply(&line).map_err(CallError::Protocol)?;
            if reply.id.is_none() || reply.id.as_ref() == Some(id) {
                return reply.outcome.map_err(CallError::Reply);
            }
        }
    }

    /// The next raw line.
    pub fn read_line(&mut self) -> Result<String, CallError> {
        let mut line = String::new();
        match self.reader.read_line(&mut line) {
            Ok(0) => Err(CallError::Io(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "VizDAW closed the connection",
            ))),
            Ok(_) => Ok(line),
            Err(e) => Err(CallError::Io(e)),
        }
    }
}
VIZDAW_EOF
```

In `crates/core-control/src/lib.rs`, Find `pub mod element;` Replace:

```rust
pub mod client;
pub mod element;
```

Find `pub mod request;` Replace:

```rust
pub mod request;
pub mod server;
```

Find `pub use request::{Command, validate};` Replace:

```rust
pub use request::{Command, validate};
pub use server::{Call, Config, Server, StartError};
```

- [ ] **Step 4: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-control --test server 2>&1 | grep -E "^test |test result"
```

Expected: PASS, 9 tests, about a second.

- [ ] **Step 5: Gate and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace 2>&1 | grep -E "^test result|FAILED|panicked" | tail -60
git add crates/core-control
git commit -m "feat(control): private Unix socket server with a call queue and timeouts; client

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
```

Expected: 794 Rust tests listed.

---

### Task 5: The FFI surface — `ControlServer`, typed requests and results, results as JSON

**Files:**
- Create: `crates/core-ffi/src/control.rs`
- Modify: `crates/core-ffi/Cargo.toml`, `crates/core-ffi/src/lib.rs`
- Test: `crates/core-ffi/tests/control.rs`

**Interfaces:**
- Consumes: `core_control::{Server, Config, Command, ControlError, ErrorCode, registry, wait, element}`; `crate::snapshot::{DocSnapshot, LaneRow, TransportState}`, `crate::selection::SelectionRow`, `crate::plugins::is_busy`, `ffi_guard`, `lock`.
- Produces (UniFFI; Swift names in brackets):
  - Records: `ElementRefRow { id, path: Vec<u32>, check: u32 }`, `ModifiersRow { shift, control, option, command }`, `KeyStrokeRow { key_code: u16, characters, unmodified, modifiers, command: Option<String> }`, `RecoveryAnswersRow { plugins: Option<CrashAnswerRow>, restore_autosave: Option<bool> }`, `ControlArg { name, value: ControlValue }`, `DocFactsRow { playing, tick: i64, detail_kind }`, `WaitFactsRow { document: Option<DocFactsRow>, documents: u32, window_titles }`, `ControlCall { token: u64, request }`, `AlertRow { title, message }`, `DetailRow { kind, seq: u64, lane: Option<u64>, start: Option<i64>, end: Option<i64> }`, `TornRow { id, detail }`, `EditorStateRow { detail, windows: Vec<TornRow>, tool, snap_enabled, snap_division, exact_mode, gesture, status }`, `WindowRow { number: i64, title, kind, doc: Option<String>, key, main, visible, x, y, width, height }`, `UiNodeRow { path, role, subrole, label, title, value, x, y, width, height, enabled, selected, focused, actions }`, `ControlFailure { code, message, dialog }`.
  - Enums: `WindowRefRow::{Number { number }, Key, Main}`, `PointerRow::{Element { element }, Point { x, y }}`, `MouseButtonRow`, `StateTargetRow`, `CrashAnswerRow::{Normal, PluginsOff, Disable}`, `ControlValue::{Text { value }, Flag { value }}`, `WaitConditionRow::{Playing, Stopped, TickAtLeast { tick }, DocumentsAtLeast { count }, WindowTitled { title }, WindowGone { title }, DetailKind { kind }}`, `WindowKindRow::{Document, Mixer, Editor, Plugin, ModifyPanel, PluginManager, Settings, Other}`, `ControlErrorCode` (15, `InternalFailure` for `internal`), `ControlRequest::{RunCommand { id, args, window }, State { target, window }, Tree { window, max_depth }, Click { window, at, button, count, modifiers }, Drag { window, from, to, steps, modifiers }, Key { window, focus, strokes }, Screenshot { window, path: String, scale }, NewDemo { audio }, Open { path, answers }, Quit, Wait { condition, window, timeout_ms }}`, `ControlResult::{Ran { command, doc, window, undo_label, status }, Transport { doc, state }, Document { doc, path, snapshot, selected_seq, lanes, alert }, Editor { doc, editor }, Selection { doc, selection }, Windows { windows, alerts }, Tree { window, nodes, truncated }, Input { window, events, x, y }, Screenshot { window, path, width, height }, Opened { doc, window }, Quitting, Waited { condition, elapsed_ms }}`.
  - Object `ControlServer`: constructor `start(path: String, app_timeout_ms: u64)` [`ControlServer.start(path:appTimeoutMs:)`], `path()`, `next_call() -> Option<ControlCall>` [`nextCall()`], `reply(token, result) -> bool`, `fail(token, failure) -> bool`, `wait_timed_out(token, condition, facts, timeout_ms) -> bool` [`waitTimedOut(token:condition:facts:timeoutMs:)`], `is_waiting(token) -> bool`, `stop()`.
  - Functions: `control_wait_met(condition, facts) -> bool` [`controlWaitMet`], `control_element_check(role, label: Option<String>, title: Option<String>) -> u32` [`controlElementCheck`], `control_command_ids() -> Vec<String>`, `control_host_busy() -> bool`.
  - Rust-only: `control::request_of(Command, screenshot: impl FnOnce() -> PathBuf) -> Option<ControlRequest>`, `control::result_json(&ControlResult) -> Value`, `control::condition_row(WaitCondition) -> WaitConditionRow`.

Spec §4.1, §5.5 (result schemas), Rulings 2, 9.

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-ffi/tests/control.rs <<'VIZDAW_EOF'
//! The control channel through the FFI (control spec §4, §5.5): calls
//! reach the app as typed requests and typed results become the reply's
//! JSON. Success paths wait generously (slow CI).

use core_control::client::{CallError, Client};
use core_control::request::{Command, CrashAnswer, RecoveryAnswers};
use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use vizdaw_ffi::control::{request_of, result_json};
use vizdaw_ffi::{
    AlertRow, ControlCall, ControlErrorCode, ControlFailure, ControlRequest, ControlResult,
    ControlServer, CrashAnswerRow, DocFactsRow, Session, UiNodeRow, WaitConditionRow, WaitFactsRow,
    control_command_ids, control_element_check, control_wait_met,
};

const LONG: Duration = Duration::from_secs(20);

fn private_dir() -> tempfile::TempDir {
    let d = tempfile::Builder::new()
        .prefix("vzf")
        .tempdir_in("/tmp")
        .unwrap();
    std::fs::set_permissions(d.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    d
}

fn socket(d: &tempfile::TempDir) -> PathBuf {
    d.path().join("s.sock")
}

/// The next call, waiting for it (generously).
fn next(s: &ControlServer) -> ControlCall {
    let deadline = Instant::now() + LONG;
    loop {
        if let Some(c) = s.next_call().unwrap() {
            return c;
        }
        assert!(Instant::now() < deadline, "no call arrived");
        thread::sleep(Duration::from_millis(1));
    }
}

/// Sends `cmd` from another thread; returns its outcome's handle.
fn send(
    path: &Path,
    cmd: &'static str,
    params: Value,
) -> thread::JoinHandle<Result<Value, CallError>> {
    let path = path.to_path_buf();
    thread::spawn(move || Client::connect(&path).unwrap().call(cmd, params, LONG))
}

#[test]
fn a_state_call_round_trips_as_typed_rows() {
    let d = private_dir();
    let s = ControlServer::start(socket(&d).to_string_lossy().into_owned(), 60_000).unwrap();
    let h = send(
        &socket(&d),
        "state.get",
        json!({"target": "document", "window": 9}),
    );
    let call = next(&s);
    assert!(matches!(
        call.request,
        ControlRequest::State {
            target: vizdaw_ffi::StateTargetRow::Document,
            window: Some(vizdaw_ffi::WindowRefRow::Number { number: 9 })
        }
    ));
    let session = Session::new_demo().unwrap();
    let snapshot = session.snapshot().unwrap();
    let r = ControlResult::Document {
        doc: "D1".into(),
        path: None,
        snapshot,
        selected_seq: None,
        lanes: vec![],
        alert: Some(AlertRow {
            title: "Play Failed".into(),
            message: "no device".into(),
        }),
    };
    assert!(s.reply(call.token, r).unwrap());
    let v = h.join().unwrap().unwrap();
    assert_eq!(v["doc"], "D1");
    let names: Vec<&str> = v["sequences"]
        .as_array()
        .unwrap()
        .iter()
        .map(|q| q["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"Song"), "{names:?}");
    assert_eq!(v["alert"]["title"], "Play Failed");
    assert!(!s.is_waiting(call.token));
    s.stop().unwrap();
    assert!(!socket(&d).exists());
}

#[test]
fn failures_carry_their_code_and_dialog() {
    let d = private_dir();
    let s = ControlServer::start(socket(&d).to_string_lossy().into_owned(), 60_000).unwrap();
    let h = send(&socket(&d), "command.run", json!({"id": "file.save"}));
    let call = next(&s);
    s.fail(
        call.token,
        ControlFailure {
            code: ControlErrorCode::DialogRequired,
            message: "this would open the \"Save\" dialog; pass `path` instead".into(),
            dialog: Some("Save".into()),
        },
    )
    .unwrap();
    match h.join().unwrap() {
        Err(CallError::Reply(e)) => {
            assert_eq!(e.code.as_str(), "dialog_required");
            assert_eq!(e.dialog.as_deref(), Some("Save"));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_wait_that_times_out_reports_its_last_facts() {
    let d = private_dir();
    let s = ControlServer::start(socket(&d).to_string_lossy().into_owned(), 60_000).unwrap();
    let h = send(
        &socket(&d),
        "wait",
        json!({"condition": "playing", "timeout_ms": 50}),
    );
    let call = next(&s);
    let ControlRequest::Wait {
        condition,
        timeout_ms,
        ..
    } = call.request
    else {
        panic!("{:?}", call.request)
    };
    let facts = WaitFactsRow {
        document: Some(DocFactsRow {
            playing: false,
            tick: 120,
            detail_kind: "none".into(),
        }),
        documents: 1,
        window_titles: vec!["Untitled".into()],
    };
    assert!(!control_wait_met(condition.clone(), facts.clone()));
    s.wait_timed_out(call.token, condition, facts, timeout_ms)
        .unwrap();
    match h.join().unwrap() {
        Err(CallError::Reply(e)) => {
            assert_eq!(e.code.as_str(), "timeout");
            let d = e.details.unwrap();
            assert_eq!(d["facts"]["document"]["tick"], 120);
            assert_eq!(d["timeout_ms"], 50);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn screenshots_default_to_the_socket_folder_and_answers_map() {
    let d = private_dir();
    let s =
        Arc::new(ControlServer::start(socket(&d).to_string_lossy().into_owned(), 60_000).unwrap());
    let h = send(&socket(&d), "ui.screenshot", json!({}));
    let call = next(&s);
    let ControlRequest::Screenshot { path, .. } = &call.request else {
        panic!("{:?}", call.request)
    };
    assert!(
        path.starts_with(&d.path().to_string_lossy().into_owned()) && path.ends_with(".png"),
        "{path}"
    );
    s.reply(
        call.token,
        ControlResult::Screenshot {
            window: 3,
            path: path.clone(),
            width: 20,
            height: 10,
        },
    )
    .unwrap();
    assert_eq!(h.join().unwrap().unwrap()["width"], 20);
    let open = request_of(
        Command::AppOpen {
            path: "/tmp/a.vizdaw".into(),
            answers: RecoveryAnswers {
                plugins: Some(CrashAnswer::Off),
                restore_autosave: Some(true),
            },
        },
        || unreachable!(),
    );
    match open {
        Some(ControlRequest::Open { answers, .. }) => {
            assert_eq!(answers.plugins, Some(CrashAnswerRow::PluginsOff));
            assert_eq!(answers.restore_autosave, Some(true));
        }
        other => panic!("{other:?}"),
    }
    assert!(
        request_of(Command::Ping, || unreachable!()).is_none(),
        "the server answers it"
    );
}

#[test]
fn tree_nodes_nest_by_path_and_carry_checked_ids() {
    let node = |path: Vec<u32>, role: &str, label: Option<&str>| UiNodeRow {
        path,
        role: role.into(),
        subrole: None,
        label: label.map(str::to_string),
        title: None,
        value: None,
        x: 1.0,
        y: 2.0,
        width: 3.0,
        height: 4.0,
        enabled: true,
        selected: false,
        focused: false,
        actions: vec![],
    };
    let r = ControlResult::Tree {
        window: 5,
        nodes: vec![
            node(vec![0], "AXGroup", Some("Arrangement")),
            node(vec![0, 0], "AXButton", Some("Block Chords")),
            node(vec![0, 1], "AXButton", Some("Block Bass")),
            node(vec![1], "AXToolbar", None),
            node(vec![0, 10], "AXButton", Some("Block Late")),
        ],
        truncated: false,
    };
    let v = result_json(&r);
    let roots = v["nodes"].as_array().unwrap();
    assert_eq!(roots.len(), 2);
    let kids: Vec<&str> = roots[0]["children"]
        .as_array()
        .unwrap()
        .iter()
        .map(|k| k["label"].as_str().unwrap())
        .collect();
    assert_eq!(
        kids,
        ["Block Chords", "Block Bass", "Block Late"],
        "siblings in index order"
    );
    let id = roots[0]["children"][1]["id"].as_str().unwrap();
    let e = core_control::element::parse(id).unwrap();
    assert_eq!(e.path, vec![0, 1]);
    assert_eq!(
        e.check,
        control_element_check("AXButton".into(), Some("Block Bass".into()), None)
    );
    assert_eq!(
        control_element_check("AXWindow".into(), None, Some("Untitled".into())),
        control_element_check(
            "AXWindow".into(),
            Some(String::new()),
            Some("Untitled".into())
        ),
        "no label: the title counts"
    );
    assert!(
        roots[1].get("label").is_none(),
        "absent fields are left out"
    );
}

#[test]
fn the_registry_ids_reach_swift() {
    let ids = control_command_ids();
    assert!(ids.contains(&"transport.play_stop".to_string()));
    assert_eq!(ids.len(), core_control::registry::COMMANDS.len());
    assert!(control_wait_met(
        WaitConditionRow::DocumentsAtLeast { count: 2 },
        WaitFactsRow {
            document: None,
            documents: 2,
            window_titles: vec![]
        }
    ));
}
VIZDAW_EOF
cargo test -p core-ffi --test control 2>&1 | grep -E "^error" | head -3
```

Expected: does not compile (`unresolved import core_control`).

- [ ] **Step 2: The FFI module**

In `crates/core-ffi/Cargo.toml`, Find:

```toml
[dependencies]
core-model = { path = "../core-model" }
```

Replace:

```toml
[dependencies]
core-control = { path = "../core-control" }
core-model = { path = "../core-model" }
```

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-ffi/src/control.rs <<'VIZDAW_EOF'
//! The control channel's FFI surface (control spec §4): `core-control`'s
//! server validates and queues requests; Swift takes each [`ControlCall`]
//! on the main thread, executes it and answers with a typed
//! [`ControlResult`] or a [`ControlFailure`], which this module turns into
//! the reply's JSON. The wire schema lives here and in `core-control`;
//! Swift never builds JSON.

use crate::error::VizError;
use crate::guard::{ffi_guard, lock};
use crate::selection::SelectionRow;
use crate::snapshot::{DocSnapshot, LaneRow, TransportState};
use core_control::element::{self as el, ElementRef};
use core_control::keys::{KeyStroke, Modifiers};
use core_control::params::WindowRef;
use core_control::registry::{self, ArgValue};
use core_control::request::{Button, Command, CrashAnswer, Pointer, StateTarget};
use core_control::server::{Config, Server};
use core_control::wait::{self, DocFacts, Facts, WaitCondition};
use core_control::{ControlError, ErrorCode};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// A window a call addresses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum WindowRefRow {
    /// `NSWindow.windowNumber`.
    Number {
        /// The number.
        number: i64,
    },
    /// The key window.
    Key,
    /// The main window.
    Main,
}

/// An element from `ui.tree`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Record)]
pub struct ElementRefRow {
    /// The ID as given.
    pub id: String,
    /// Child indices from the window down.
    pub path: Vec<u32>,
    /// [`control_element_check`] of the element.
    pub check: u32,
}

/// Where a click or drag lands.
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum PointerRow {
    /// An element's centre.
    Element {
        /// Which.
        element: ElementRefRow,
    },
    /// Window points, top-left origin.
    Point {
        /// From the left edge.
        x: f64,
        /// From the top edge.
        y: f64,
    },
}

/// Modifier keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, uniffi::Record)]
pub struct ModifiersRow {
    /// ⇧
    pub shift: bool,
    /// ⌃
    pub control: bool,
    /// ⌥
    pub option: bool,
    /// ⌘
    pub command: bool,
}

/// One key press.
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Record)]
pub struct KeyStrokeRow {
    /// `NSEvent.keyCode`.
    pub key_code: u16,
    /// `NSEvent.characters`.
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum MouseButtonRow {
    /// Primary.
    Left,
    /// Secondary.
    Right,
}

/// What `state.get` reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum StateTargetRow {
    /// Transport.
    Transport,
    /// Document.
    Document,
    /// Editors.
    Editor,
    /// Selection.
    Selection,
    /// Windows and app alerts.
    Windows,
}

/// The after-crash answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum CrashAnswerRow {
    /// Open Normally.
    Normal,
    /// Open with Plugins Off.
    PluginsOff,
    /// Disable the plugins that were loading.
    Disable,
}

/// Answers to the dialogs opening a project may ask.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, uniffi::Record)]
pub struct RecoveryAnswersRow {
    /// "Open with Plugins Off?"
    pub plugins: Option<CrashAnswerRow>,
    /// "Restore Autosave"
    pub restore_autosave: Option<bool>,
}

/// A `command.run` argument's value.
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum ControlValue {
    /// A string or path.
    Text {
        /// The string.
        value: String,
    },
    /// A flag.
    Flag {
        /// The flag.
        value: bool,
    },
}

/// A `command.run` argument.
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Record)]
pub struct ControlArg {
    /// Its name.
    pub name: String,
    /// Its value.
    pub value: ControlValue,
}

/// What a `wait` waits for.
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum WaitConditionRow {
    /// Playing.
    Playing,
    /// Stopped.
    Stopped,
    /// At or past a tick.
    TickAtLeast {
        /// The tick.
        tick: i64,
    },
    /// At least this many documents.
    DocumentsAtLeast {
        /// How many.
        count: u32,
    },
    /// A visible window titled like this.
    WindowTitled {
        /// Part of the title.
        title: String,
    },
    /// No visible window titled like this.
    WindowGone {
        /// Part of the title.
        title: String,
    },
    /// The detail pane shows this kind (`none`, `piano_roll`, ...).
    DetailKind {
        /// The kind.
        kind: String,
    },
}

/// The target document's state at one pump tick.
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Record)]
pub struct DocFactsRow {
    /// Its transport plays.
    pub playing: bool,
    /// Its transport's tick.
    pub tick: i64,
    /// Its detail pane's kind.
    pub detail_kind: String,
}

/// What one pump tick saw.
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Record)]
pub struct WaitFactsRow {
    /// The target document, if any.
    pub document: Option<DocFactsRow>,
    /// Open documents.
    pub documents: u32,
    /// Visible window titles.
    pub window_titles: Vec<String>,
}

/// A validated request for the app.
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum ControlRequest {
    /// `command.run`.
    RunCommand {
        /// Registry ID.
        id: String,
        /// Validated arguments.
        args: Vec<ControlArg>,
        /// Whose document.
        window: Option<WindowRefRow>,
    },
    /// `state.get`.
    State {
        /// What.
        target: StateTargetRow,
        /// Whose.
        window: Option<WindowRefRow>,
    },
    /// `ui.tree`.
    Tree {
        /// Which window.
        window: Option<WindowRefRow>,
        /// How deep.
        max_depth: u32,
    },
    /// `ui.click`.
    Click {
        /// Which window.
        window: Option<WindowRefRow>,
        /// Where.
        at: PointerRow,
        /// Which button.
        button: MouseButtonRow,
        /// 1–3.
        count: u32,
        /// Held modifiers.
        modifiers: ModifiersRow,
    },
    /// `ui.drag`.
    Drag {
        /// Which window.
        window: Option<WindowRefRow>,
        /// Press.
        from: PointerRow,
        /// Release.
        to: PointerRow,
        /// Intermediate points.
        steps: u32,
        /// Held modifiers.
        modifiers: ModifiersRow,
    },
    /// `ui.key`.
    Key {
        /// Which window.
        window: Option<WindowRefRow>,
        /// Clicked first.
        focus: Option<ElementRefRow>,
        /// In order.
        strokes: Vec<KeyStrokeRow>,
    },
    /// `ui.screenshot`.
    Screenshot {
        /// Which window.
        window: Option<WindowRefRow>,
        /// Where to write the PNG.
        path: String,
        /// Pixels per point (None: the window's).
        scale: Option<f64>,
    },
    /// `app.new_demo`.
    NewDemo {
        /// With an audio lane.
        audio: bool,
    },
    /// `app.open`.
    Open {
        /// The package.
        path: String,
        /// Dialog answers.
        answers: RecoveryAnswersRow,
    },
    /// `app.quit`.
    Quit,
    /// `wait`.
    Wait {
        /// What.
        condition: WaitConditionRow,
        /// Whose.
        window: Option<WindowRefRow>,
        /// At most this long.
        timeout_ms: u64,
    },
}

/// A request and the token to answer it with.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct ControlCall {
    /// Answer with this.
    pub token: u64,
    /// The request.
    pub request: ControlRequest,
}

/// An alert the app would have shown.
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Record)]
pub struct AlertRow {
    /// Its title.
    pub title: String,
    /// Its message.
    pub message: String,
}

/// What a detail pane or torn-off editor shows.
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Record)]
pub struct DetailRow {
    /// `piano_roll`, `list`, `strip_chart` or `drum_grid`.
    pub kind: String,
    /// The target's sequence.
    pub seq: u64,
    /// Its lane (lane and block targets).
    pub lane: Option<u64>,
    /// A block target's start tick.
    pub start: Option<i64>,
    /// A block target's end tick.
    pub end: Option<i64>,
}

/// A torn-off editor window.
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Record)]
pub struct TornRow {
    /// Its layout ID.
    pub id: String,
    /// What it shows.
    pub detail: DetailRow,
}

/// A document's editors.
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Record)]
pub struct EditorStateRow {
    /// The detail pane (None: empty).
    pub detail: Option<DetailRow>,
    /// Torn-off editor windows.
    pub windows: Vec<TornRow>,
    /// `arrow`, `pencil` or `eraser`.
    pub tool: String,
    /// Snap on.
    pub snap_enabled: bool,
    /// Snap division (`quarter`, ...).
    pub snap_division: String,
    /// Exact mode on.
    pub exact_mode: bool,
    /// A drag is open.
    pub gesture: bool,
    /// The editors' status line.
    pub status: Option<String>,
}

/// Kinds of VizDAW window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum WindowKindRow {
    /// A document window.
    Document,
    /// A mixer.
    Mixer,
    /// A torn-off editor.
    Editor,
    /// A plugin window.
    Plugin,
    /// A select-and-modify panel.
    ModifyPanel,
    /// The Plugin Manager.
    PluginManager,
    /// Settings.
    Settings,
    /// Anything else (menus' windows are not listed).
    Other,
}

/// A VizDAW window.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct WindowRow {
    /// `NSWindow.windowNumber`.
    pub number: i64,
    /// Its title.
    pub title: String,
    /// Its kind.
    pub kind: WindowKindRow,
    /// Its document's ID, if it has one.
    pub doc: Option<String>,
    /// Key window.
    pub key: bool,
    /// Main window.
    pub main: bool,
    /// On screen.
    pub visible: bool,
    /// Frame in screen points (AppKit, bottom-left origin).
    pub x: f64,
    /// Frame origin y.
    pub y: f64,
    /// Width in points.
    pub width: f64,
    /// Height in points.
    pub height: f64,
}

/// One accessibility element, in pre-order; `path` places it.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct UiNodeRow {
    /// Child indices from the window down.
    pub path: Vec<u32>,
    /// `AXButton`, `AXGroup`, ...
    pub role: String,
    /// Subrole, if any.
    pub subrole: Option<String>,
    /// Accessibility label.
    pub label: Option<String>,
    /// Title.
    pub title: Option<String>,
    /// Value as text.
    pub value: Option<String>,
    /// Frame in window points, top-left origin.
    pub x: f64,
    /// Frame y.
    pub y: f64,
    /// Width.
    pub width: f64,
    /// Height.
    pub height: f64,
    /// Enabled.
    pub enabled: bool,
    /// Selected.
    pub selected: bool,
    /// Has keyboard focus.
    pub focused: bool,
    /// Actions it supports (`press`, ...).
    pub actions: Vec<String>,
}

/// What a call came to.
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum ControlResult {
    /// `command.run`.
    Ran {
        /// Registry ID.
        command: String,
        /// The document it acted on or made.
        doc: Option<String>,
        /// The window it opened.
        window: Option<i64>,
        /// The undo label afterwards.
        undo_label: Option<String>,
        /// The editors' status line afterwards.
        status: Option<String>,
    },
    /// `state.get transport`.
    Transport {
        /// Document ID.
        doc: String,
        /// Its transport.
        state: TransportState,
    },
    /// `state.get document`.
    Document {
        /// Document ID.
        doc: String,
        /// Its package, once saved.
        path: Option<String>,
        /// Its snapshot.
        snapshot: DocSnapshot,
        /// The sidebar's sequence.
        selected_seq: Option<u64>,
        /// That sequence's lanes.
        lanes: Vec<LaneRow>,
        /// The alert its window would show.
        alert: Option<AlertRow>,
    },
    /// `state.get editor`.
    Editor {
        /// Document ID.
        doc: String,
        /// Its editors.
        editor: EditorStateRow,
    },
    /// `state.get selection`.
    Selection {
        /// Document ID.
        doc: String,
        /// Its selection.
        selection: SelectionRow,
    },
    /// `state.get windows`.
    Windows {
        /// Every VizDAW window, front to back.
        windows: Vec<WindowRow>,
        /// App-level alerts not shown (the channel suppresses them).
        alerts: Vec<AlertRow>,
    },
    /// `ui.tree`.
    Tree {
        /// The window.
        window: i64,
        /// Pre-order nodes.
        nodes: Vec<UiNodeRow>,
        /// The node cap cut the tree.
        truncated: bool,
    },
    /// `ui.click`, `ui.drag`, `ui.key`.
    Input {
        /// The window.
        window: i64,
        /// Events sent.
        events: u32,
        /// Where the last mouse event went (0 for keys).
        x: f64,
        /// Its y.
        y: f64,
    },
    /// `ui.screenshot`.
    Screenshot {
        /// The window.
        window: i64,
        /// The PNG.
        path: String,
        /// Pixels.
        width: u32,
        /// Pixels.
        height: u32,
    },
    /// `app.new_demo`, `app.open`.
    Opened {
        /// The new document.
        doc: String,
        /// Its window.
        window: i64,
    },
    /// `app.quit`.
    Quitting,
    /// `wait`.
    Waited {
        /// The condition's name.
        condition: String,
        /// How long it took.
        elapsed_ms: u64,
    },
}

/// Error codes (control spec §5.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum ControlErrorCode {
    /// `parse_error`
    ParseError,
    /// `invalid_request`
    InvalidRequest,
    /// `unknown_command`
    UnknownCommand,
    /// `invalid_params`
    InvalidParams,
    /// `unknown_id`
    UnknownId,
    /// `no_document`
    NoDocument,
    /// `no_window`
    NoWindow,
    /// `no_element`
    NoElement,
    /// `dialog_required`
    DialogRequired,
    /// `busy`
    Busy,
    /// `failed`
    Failed,
    /// `unavailable`
    Unavailable,
    /// `timeout`
    Timeout,
    /// `shutting_down`
    ShuttingDown,
    /// `internal` (`Internal` is a Swift keyword when lower-cased).
    InternalFailure,
}

/// A failed call.
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Record)]
pub struct ControlFailure {
    /// The code.
    pub code: ControlErrorCode,
    /// One sentence.
    pub message: String,
    /// The dialog it would have opened.
    pub dialog: Option<String>,
}

fn code_of(c: ControlErrorCode) -> ErrorCode {
    match c {
        ControlErrorCode::ParseError => ErrorCode::ParseError,
        ControlErrorCode::InvalidRequest => ErrorCode::InvalidRequest,
        ControlErrorCode::UnknownCommand => ErrorCode::UnknownCommand,
        ControlErrorCode::InvalidParams => ErrorCode::InvalidParams,
        ControlErrorCode::UnknownId => ErrorCode::UnknownId,
        ControlErrorCode::NoDocument => ErrorCode::NoDocument,
        ControlErrorCode::NoWindow => ErrorCode::NoWindow,
        ControlErrorCode::NoElement => ErrorCode::NoElement,
        ControlErrorCode::DialogRequired => ErrorCode::DialogRequired,
        ControlErrorCode::Busy => ErrorCode::Busy,
        ControlErrorCode::Failed => ErrorCode::Failed,
        ControlErrorCode::Unavailable => ErrorCode::Unavailable,
        ControlErrorCode::Timeout => ErrorCode::Timeout,
        ControlErrorCode::ShuttingDown => ErrorCode::ShuttingDown,
        ControlErrorCode::InternalFailure => ErrorCode::Internal,
    }
}

fn window_row(w: Option<WindowRef>) -> Option<WindowRefRow> {
    w.map(|w| match w {
        WindowRef::Number(number) => WindowRefRow::Number { number },
        WindowRef::Key => WindowRefRow::Key,
        WindowRef::Main => WindowRefRow::Main,
    })
}

fn element_row(e: ElementRef) -> ElementRefRow {
    ElementRefRow {
        id: e.id,
        path: e.path,
        check: e.check,
    }
}

fn pointer_row(p: Pointer) -> PointerRow {
    match p {
        Pointer::Element(e) => PointerRow::Element {
            element: element_row(e),
        },
        Pointer::Point { x, y } => PointerRow::Point { x, y },
    }
}

fn modifiers_row(m: Modifiers) -> ModifiersRow {
    ModifiersRow {
        shift: m.shift,
        control: m.control,
        option: m.option,
        command: m.command,
    }
}

fn stroke_row(k: KeyStroke) -> KeyStrokeRow {
    KeyStrokeRow {
        command: registry::command_for_stroke(&k).map(str::to_string),
        key_code: k.key_code,
        characters: k.characters,
        unmodified: k.unmodified,
        modifiers: modifiers_row(k.modifiers),
    }
}

/// The FFI form of a wait condition.
pub fn condition_row(c: WaitCondition) -> WaitConditionRow {
    match c {
        WaitCondition::Playing => WaitConditionRow::Playing,
        WaitCondition::Stopped => WaitConditionRow::Stopped,
        WaitCondition::TickAtLeast(tick) => WaitConditionRow::TickAtLeast { tick },
        WaitCondition::DocumentsAtLeast(count) => WaitConditionRow::DocumentsAtLeast { count },
        WaitCondition::WindowTitled(title) => WaitConditionRow::WindowTitled { title },
        WaitCondition::WindowGone(title) => WaitConditionRow::WindowGone { title },
        WaitCondition::DetailKind(kind) => WaitConditionRow::DetailKind { kind },
    }
}

fn condition_of(c: WaitConditionRow) -> WaitCondition {
    match c {
        WaitConditionRow::Playing => WaitCondition::Playing,
        WaitConditionRow::Stopped => WaitCondition::Stopped,
        WaitConditionRow::TickAtLeast { tick } => WaitCondition::TickAtLeast(tick),
        WaitConditionRow::DocumentsAtLeast { count } => WaitCondition::DocumentsAtLeast(count),
        WaitConditionRow::WindowTitled { title } => WaitCondition::WindowTitled(title),
        WaitConditionRow::WindowGone { title } => WaitCondition::WindowGone(title),
        WaitConditionRow::DetailKind { kind } => WaitCondition::DetailKind(kind),
    }
}

fn facts_of(f: WaitFactsRow) -> Facts {
    Facts {
        document: f.document.map(|d| DocFacts {
            playing: d.playing,
            tick: d.tick,
            detail_kind: d.detail_kind,
        }),
        documents: f.documents,
        window_titles: f.window_titles,
    }
}

/// The request for the app, or None for a command the server answers.
pub fn request_of(c: Command, screenshot: impl FnOnce() -> PathBuf) -> Option<ControlRequest> {
    Some(match c {
        Command::CommandsList | Command::Ping => return None,
        Command::CommandRun { id, args, window } => ControlRequest::RunCommand {
            id,
            args: args
                .into_iter()
                .map(|(name, v)| ControlArg {
                    name,
                    value: match v {
                        ArgValue::Text(value) => ControlValue::Text { value },
                        ArgValue::Flag(value) => ControlValue::Flag { value },
                    },
                })
                .collect(),
            window: window_row(window),
        },
        Command::StateGet { target, window } => ControlRequest::State {
            target: match target {
                StateTarget::Transport => StateTargetRow::Transport,
                StateTarget::Document => StateTargetRow::Document,
                StateTarget::Editor => StateTargetRow::Editor,
                StateTarget::Selection => StateTargetRow::Selection,
                StateTarget::Windows => StateTargetRow::Windows,
            },
            window: window_row(window),
        },
        Command::UiTree { window, max_depth } => ControlRequest::Tree {
            window: window_row(window),
            max_depth,
        },
        Command::UiClick {
            window,
            at,
            button,
            count,
            modifiers,
        } => ControlRequest::Click {
            window: window_row(window),
            at: pointer_row(at),
            button: match button {
                Button::Left => MouseButtonRow::Left,
                Button::Right => MouseButtonRow::Right,
            },
            count,
            modifiers: modifiers_row(modifiers),
        },
        Command::UiDrag {
            window,
            from,
            to,
            steps,
            modifiers,
        } => ControlRequest::Drag {
            window: window_row(window),
            from: pointer_row(from),
            to: pointer_row(to),
            steps,
            modifiers: modifiers_row(modifiers),
        },
        Command::UiKey {
            window,
            focus,
            strokes,
        } => ControlRequest::Key {
            window: window_row(window),
            focus: focus.map(element_row),
            strokes: strokes.into_iter().map(stroke_row).collect(),
        },
        Command::UiScreenshot {
            window,
            path,
            scale,
        } => ControlRequest::Screenshot {
            window: window_row(window),
            path: path
                .unwrap_or_else(screenshot)
                .to_string_lossy()
                .into_owned(),
            scale,
        },
        Command::AppNewDemo { audio } => ControlRequest::NewDemo { audio },
        Command::AppOpen { path, answers } => ControlRequest::Open {
            path: path.to_string_lossy().into_owned(),
            answers: RecoveryAnswersRow {
                plugins: answers.plugins.map(|a| match a {
                    CrashAnswer::Normal => CrashAnswerRow::Normal,
                    CrashAnswer::Off => CrashAnswerRow::PluginsOff,
                    CrashAnswer::Disable => CrashAnswerRow::Disable,
                }),
                restore_autosave: answers.restore_autosave,
            },
        },
        Command::AppQuit => ControlRequest::Quit,
        Command::Wait {
            condition,
            window,
            timeout_ms,
        } => ControlRequest::Wait {
            condition: condition_row(condition),
            window: window_row(window),
            timeout_ms,
        },
    })
}

fn frame(x: f64, y: f64, width: f64, height: f64) -> Value {
    json!({ "x": x, "y": y, "width": width, "height": height })
}

fn alert_json(a: &AlertRow) -> Value {
    json!({ "title": a.title, "message": a.message })
}

fn detail_json(d: &DetailRow) -> Value {
    json!({ "kind": d.kind, "seq": d.seq, "lane": d.lane, "start": d.start, "end": d.end })
}

fn node_json(n: &UiNodeRow) -> Value {
    let mut o = Map::new();
    let name = n.label.as_deref().or(n.title.as_deref()).unwrap_or("");
    o.insert("id".into(), json!(el::element_id(&n.path, &n.role, name)));
    o.insert("role".into(), json!(n.role));
    for (k, v) in [
        ("subrole", &n.subrole),
        ("label", &n.label),
        ("title", &n.title),
        ("value", &n.value),
    ] {
        if let Some(v) = v.as_ref().filter(|v| !v.is_empty()) {
            o.insert(k.into(), json!(v));
        }
    }
    o.insert("frame".into(), frame(n.x, n.y, n.width, n.height));
    o.insert("enabled".into(), json!(n.enabled));
    if n.selected {
        o.insert("selected".into(), json!(true));
    }
    if n.focused {
        o.insert("focused".into(), json!(true));
    }
    if !n.actions.is_empty() {
        o.insert("actions".into(), json!(n.actions));
    }
    Value::Object(o)
}

/// Nests pre-order nodes by their paths (a node whose parent is missing
/// stays at the top).
fn tree_json(nodes: &[UiNodeRow]) -> Vec<Value> {
    let mut built: BTreeMap<Vec<u32>, Value> = nodes
        .iter()
        .map(|n| (n.path.clone(), node_json(n)))
        .collect();
    let mut paths: Vec<Vec<u32>> = built.keys().cloned().collect();
    // Deepest first, siblings in index order (the sort is stable).
    paths.sort_by_key(|p| std::cmp::Reverse(p.len()));
    let mut roots = Vec::new();
    for p in paths {
        let Some(v) = built.remove(&p) else { continue };
        let parent = p.split_last().map(|(_, rest)| rest).unwrap_or(&[]);
        match built.get_mut(parent).and_then(Value::as_object_mut) {
            Some(o) if !parent.is_empty() => {
                if let Some(a) = o
                    .entry("children")
                    .or_insert_with(|| json!([]))
                    .as_array_mut()
                {
                    a.push(v);
                }
            }
            _ => roots.push(v),
        }
    }
    // Children were added deepest-first; sibling order holds per parent,
    // roots come out in index order.
    roots
}

fn transport_json(doc: &str, t: &TransportState) -> Value {
    json!({
        "doc": doc, "playing": t.playing, "seq": t.seq, "tick": t.tick,
        "position": t.position_bbu, "seconds": t.seconds, "sample_rate": t.sample_rate,
        "device": t.device, "faults": t.faults, "report": t.report,
    })
}

/// The reply's `result` for `r` (control spec §5.5).
pub fn result_json(r: &ControlResult) -> Value {
    match r {
        ControlResult::Ran {
            command,
            doc,
            window,
            undo_label,
            status,
        } => json!({
            "command": command, "doc": doc, "window": window, "undo": undo_label, "status": status,
        }),
        ControlResult::Transport { doc, state } => transport_json(doc, state),
        ControlResult::Document {
            doc,
            path,
            snapshot: s,
            selected_seq,
            lanes,
            alert,
        } => json!({
            "doc": doc, "title": s.title, "path": path, "dirty": s.dirty, "version": s.version,
            "undo": s.undo_label, "redo": s.redo_label, "selected_seq": selected_seq,
            "sequences": s.sequences.iter().map(|q| json!({
                "id": q.id, "name": q.name, "top_level": q.top_level, "uses": q.use_count,
                "length": q.length_ticks, "lanes": q.lane_count,
            })).collect::<Vec<_>>(),
            "lanes": lanes.iter().map(|l| json!({
                "id": l.id, "name": l.name, "destination": l.destination,
                "events": l.midi_event_count, "regions": l.region_count,
                "references": l.reference_count, "mute": l.mute, "solo": l.solo, "arm": l.arm,
            })).collect::<Vec<_>>(),
            "warnings": s.warnings,
            "alert": alert.as_ref().map(alert_json),
        }),
        ControlResult::Editor { doc, editor: e } => json!({
            "doc": doc,
            "detail": e.detail.as_ref().map(detail_json),
            "windows": e.windows.iter().map(|w| json!({
                "id": w.id, "detail": detail_json(&w.detail),
            })).collect::<Vec<_>>(),
            "tool": e.tool,
            "snap": { "enabled": e.snap_enabled, "division": e.snap_division },
            "exact_mode": e.exact_mode, "gesture": e.gesture, "status": e.status,
        }),
        ControlResult::Selection { doc, selection: s } => json!({
            "doc": doc,
            "time_range": s.time_range.map(|t| json!({ "start": t.start, "end": t.end })),
            "lanes": s.lanes.iter().map(|l| json!({ "seq": l.seq, "lane": l.lane })).collect::<Vec<_>>(),
            "events": s.events,
        }),
        ControlResult::Windows { windows, alerts } => json!({
            "windows": windows.iter().map(|w| json!({
                "number": w.number, "title": w.title, "kind": window_kind(w.kind), "doc": w.doc,
                "key": w.key, "main": w.main, "visible": w.visible,
                "frame": frame(w.x, w.y, w.width, w.height),
            })).collect::<Vec<_>>(),
            "alerts": alerts.iter().map(alert_json).collect::<Vec<_>>(),
        }),
        ControlResult::Tree {
            window,
            nodes,
            truncated,
        } => json!({ "window": window, "truncated": truncated, "nodes": tree_json(nodes) }),
        ControlResult::Input {
            window,
            events,
            x,
            y,
        } => {
            json!({ "window": window, "events": events, "x": x, "y": y })
        }
        ControlResult::Screenshot {
            window,
            path,
            width,
            height,
        } => json!({ "window": window, "path": path, "width": width, "height": height }),
        ControlResult::Opened { doc, window } => json!({ "doc": doc, "window": window }),
        ControlResult::Quitting => json!({ "quitting": true }),
        ControlResult::Waited {
            condition,
            elapsed_ms,
        } => json!({ "condition": condition, "elapsed_ms": elapsed_ms }),
    }
}

fn window_kind(k: WindowKindRow) -> &'static str {
    match k {
        WindowKindRow::Document => "document",
        WindowKindRow::Mixer => "mixer",
        WindowKindRow::Editor => "editor",
        WindowKindRow::Plugin => "plugin",
        WindowKindRow::ModifyPanel => "modify_panel",
        WindowKindRow::PluginManager => "plugin_manager",
        WindowKindRow::Settings => "settings",
        WindowKindRow::Other => "other",
    }
}

/// The control socket the app listens on (`--control` / `VIZDAW_CONTROL`).
#[derive(uniffi::Object)]
pub struct ControlServer {
    server: Mutex<Server>,
    path: String,
}

#[uniffi::export]
impl ControlServer {
    /// Listens on `path` (control spec §8); a call the app does not answer
    /// within `app_timeout_ms` is answered `timeout`.
    #[uniffi::constructor]
    pub fn start(path: String, app_timeout_ms: u64) -> Result<Arc<ControlServer>, VizError> {
        ffi_guard(|| {
            let config = Config {
                app_timeout: Duration::from_millis(app_timeout_ms.max(1)),
                ..Config::default()
            };
            let server =
                Server::start(std::path::Path::new(&path), config).map_err(|e| VizError::Io {
                    message: e.to_string(),
                })?;
            Ok(Arc::new(ControlServer {
                server: Mutex::new(server),
                path,
            }))
        })
    }

    /// The socket's path.
    pub fn path(&self) -> String {
        self.path.clone()
    }

    /// The oldest call for the app, if any.
    pub fn next_call(&self) -> Result<Option<ControlCall>, VizError> {
        ffi_guard(|| {
            let server = lock(&self.server);
            while let Some(call) = server.next_call() {
                let token = call.token;
                if let Some(request) = request_of(call.command, || server.screenshot_path()) {
                    return Ok(Some(ControlCall { token, request }));
                }
            }
            Ok(None)
        })
    }

    /// Answers `token` with `result`; false when nobody waits any more.
    pub fn reply(&self, token: u64, result: ControlResult) -> Result<bool, VizError> {
        ffi_guard(|| Ok(lock(&self.server).reply(token, Ok(result_json(&result)))))
    }

    /// Answers `token` with `failure`.
    pub fn fail(&self, token: u64, failure: ControlFailure) -> Result<bool, VizError> {
        ffi_guard(|| {
            let mut e = ControlError::new(code_of(failure.code), failure.message);
            e.dialog = failure.dialog;
            Ok(lock(&self.server).reply(token, Err(e)))
        })
    }

    /// Answers wait `token` with `timeout`, the condition and the last facts.
    pub fn wait_timed_out(
        &self,
        token: u64,
        condition: WaitConditionRow,
        facts: WaitFactsRow,
        timeout_ms: u64,
    ) -> Result<bool, VizError> {
        ffi_guard(|| {
            let c = condition_of(condition);
            let details = wait::timeout_details(&c, timeout_ms, &facts_of(facts));
            let e = ControlError::new(
                ErrorCode::Timeout,
                format!("{} did not happen within {timeout_ms} ms", c.describe()),
            )
            .with_details(details);
            Ok(lock(&self.server).reply(token, Err(e)))
        })
    }

    /// Whether a client still waits for `token`.
    pub fn is_waiting(&self, token: u64) -> bool {
        lock(&self.server).is_waiting(token)
    }

    /// Stops listening (pending calls get `shutting_down`). Idempotent.
    pub fn stop(&self) -> Result<(), VizError> {
        ffi_guard(|| {
            lock(&self.server).stop();
            Ok(())
        })
    }
}

/// Whether `condition` holds for `facts`.
#[uniffi::export]
pub fn control_wait_met(condition: WaitConditionRow, facts: WaitFactsRow) -> bool {
    wait::met(&condition_of(condition), &facts_of(facts))
}

/// The check an element ID carries: of its role and its label (its title
/// when it has no label).
#[uniffi::export]
pub fn control_element_check(role: String, label: Option<String>, title: Option<String>) -> u32 {
    let name = label
        .filter(|l| !l.is_empty())
        .or(title)
        .unwrap_or_default();
    el::check(&role, &name)
}

/// Every registered menu command ID (the app checks it handles each).
#[uniffi::export]
pub fn control_command_ids() -> Vec<String> {
    registry::COMMANDS
        .iter()
        .map(|c| c.id.to_string())
        .collect()
}

/// Whether a plugin call is on this (the main) thread's stack: a control
/// call now must answer `busy` (control spec §7).
#[uniffi::export]
pub fn control_host_busy() -> bool {
    crate::plugins::is_busy()
}
VIZDAW_EOF
```

In `crates/core-ffi/src/lib.rs`, Find `pub mod clock;` Replace:

```rust
pub mod clock;
pub mod control;
```

Find `pub use edit::{` Replace:

```rust
pub use control::{
    AlertRow, ControlArg, ControlCall, ControlErrorCode, ControlFailure, ControlRequest,
    ControlResult, ControlServer, ControlValue, CrashAnswerRow, DetailRow, DocFactsRow,
    EditorStateRow, ElementRefRow, KeyStrokeRow, ModifiersRow, MouseButtonRow, PointerRow,
    RecoveryAnswersRow, StateTargetRow, TornRow, UiNodeRow, WaitConditionRow, WaitFactsRow,
    WindowKindRow, WindowRefRow, WindowRow, control_command_ids, control_element_check,
    control_host_busy, control_wait_met,
};
pub use edit::{
```

- [ ] **Step 3: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-ffi --test control 2>&1 | grep -E "^test |test result"
```

Expected: PASS, 6 tests.

- [ ] **Step 4: Bindings**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/build-ffi.sh 2>&1 | tail -1
grep -c "public func controlWaitMet\|public static func start(path: String, appTimeoutMs: UInt64)\|case internalFailure" app/Sources/VizDAWCore/vizdaw_ffi.swift
```

Expected: `FFI ready (release)`; the grep counts 3.

- [ ] **Step 5: Gate and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace 2>&1 | grep -E "^test result|FAILED|panicked" | tail -60
scripts/build-ffi.sh && scripts/swift.sh build 2>&1 | tail -3 && scripts/swift.sh test 2>&1 | tail -3
git add crates/core-ffi/Cargo.toml crates/core-ffi/src/control.rs crates/core-ffi/src/lib.rs crates/core-ffi/tests/control.rs Cargo.lock
git commit -m "feat(ffi): control server, typed requests and results

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
```

Expected: 800 Rust tests listed; Swift 551 (unchanged).

---

### Task 6: Swift — menu commands on a document model, state rows, opening without dialogs, the launch flag

**Files:**
- Create: `app/Sources/VizDAWAppModel/ControlCommands.swift`
- Test: `app/Tests/VizDAWAppModelTests/ControlCommandsTests.swift`

**Interfaces:**
- Consumes: Task 5's generated types (`ControlResult`, `ControlFailure`, `ControlErrorCode`, `ControlArg`, `ControlValue`, `RecoveryAnswersRow`, `CrashAnswerRow`, `DetailRow`, `TornRow`, `EditorStateRow`, `AlertRow`, `DocFactsRow`, `StateTargetRow`, `controlCommandIds()`); `DocumentStore` (`newEmpty/newDemo/newDemoAudio/open(path:)/takeAlert/close/recovery`), `DocumentModel` (intents, `session`, `alert`, `poll()`), `EditorModel` (`showDetail`, `closeDetail`, `nudge`, `makeReferenceFromSelection`, `back`, `exactMode`, `layout`, `tool`, `selection`, `status`, `gesture`), `SelectModifyModel` (`open`, `run`, `modifyAgain`, `canModifyAgain`), `RecoveryQuestions`, `CrashChoice`, `NudgeSettings`, `AlertItem`.
- Produces: `ControlOutcome::{done(ControlResult), failed(ControlFailure), opened(command: String?, doc: UUID)}`; `ControlFailure.of(_:_:dialog:)`, `.thrown(_:_:)`, `.alert(_:)`, `.dialog(_:_:)`; `[ControlArg].text(_:)`, `.flag(_:)`; `protocol ControlHost` (`showMixer`, `showPluginManager`, `tearOff(_:kind:)`, `closeDocument`, `nudge`); `ControlCommands.handled: Set<String>`, `ControlCommands.run(_:args:doc:store:host:) -> ControlOutcome`; `ControlOpen.answers(_:)`, `ControlOpen.open(path:answers:store:command:)`; `ControlStates.kindName(_:)`, `.detailRow(_:)`, `.state(_:_:_:) -> ControlResult?`, `.facts(_:) -> DocFactsRow`; `ControlLaunch.socketPath(arguments:environment:)`, `ControlLaunch.strip(_:)`.

Spec §6, §7; Rulings 5, 14.

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Tests/VizDAWAppModelTests/ControlCommandsTests.swift <<'VIZDAW_EOF'
import Foundation
import Testing
@testable import VizDAWAppModel
import VizDAWCore

@MainActor
final class FakeControlHost: ControlHost {
    var calls: [String] = []
    var nudge = NudgeSettings()
    func showMixer(_ doc: UUID) { calls.append("mixer") }
    func showPluginManager() { calls.append("plugins") }
    func tearOff(_ doc: UUID, kind: DetailEditorKind) { calls.append("tear \(kind.rawValue)") }
    func closeDocument(_ doc: UUID) { calls.append("close") }
}

@MainActor
@Suite struct ControlCommandsTests {
    let host = FakeControlHost()

    /// A store with one demo document (Song selected) and its fake session.
    func demo() throws -> (DocumentStore, UUID, DocumentModel, FakeSession) {
        let f = demoFake()
        let s = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { f }, open: { _ in demoFake() })
        let id = try #require(s.newDemo())
        let m = try #require(s.model(id))
        return (s, id, m, f)
    }

    func run(_ id: String, _ args: [ControlArg], _ s: DocumentStore, _ doc: UUID?) -> ControlOutcome {
        ControlCommands.run(id, args: args, doc: doc.flatMap { d in s.model(d).map { (d, $0) } }, store: s, host: host)
    }

    func run(_ id: String, _ s: DocumentStore, _ doc: UUID?) -> ControlOutcome { run(id, [], s, doc) }

    func failure(_ o: ControlOutcome) -> ControlFailure? {
        if case .failed(let f) = o { return f }
        return nil
    }

    @Test func everyRegisteredMenuCommandHasAHandler() {
        #expect(Set(controlCommandIds()) == ControlCommands.handled)
    }

    @Test func playStopPlaysAndStopsTheSelectedSequence() throws {
        let (s, id, m, f) = try demo()
        #expect(m.selectedSeq == 3)
        guard case .done(.ran(let command, let doc, _, _, _)) = run("transport.play_stop", s, id) else {
            Issue.record("not done")
            return
        }
        #expect(command == "transport.play_stop" && doc == id.uuidString)
        #expect(f.state.playing && f.state.seq == 3 && m.isPlaying)
        _ = run("transport.play_stop", s, id)
        #expect(!f.state.playing)
    }

    @Test func busyAndFailedPlaysAreErrorsNotAlerts() throws {
        let (s, id, m, f) = try demo()
        f.playError = .Busy(message: "plugin host is busy")
        #expect(failure(run("transport.play_stop", s, id))?.code == .busy)
        f.playError = .NoDevice(message: "none")
        let e = try #require(failure(run("transport.play_stop", s, id)))
        #expect(e.code == .failed && e.message.hasPrefix("Play Failed: "))
        #expect(m.alert == nil, "no sheet appears")
    }

    @Test func saveNeedsAPathUntilTheDocumentHasOne() throws {
        let (s, id, _, f) = try demo()
        let e = try #require(failure(run("file.save", s, id)))
        #expect(e.code == .dialogRequired && e.dialog == "Save")
        let path = ControlArg(name: "path", value: .text(value: "/tmp/Song.vizdaw"))
        guard case .done = run("file.save", [path], s, id) else {
            Issue.record("save with a path failed")
            return
        }
        #expect(f.savedPath == "/tmp/Song.vizdaw")
        guard case .done = run("file.save", s, id) else {
            Issue.record("a saved document saves without a path")
            return
        }
        #expect(failure(run("file.save_as", s, id))?.dialog == "Save")
    }

    @Test func disabledCommandsAreUnavailable() throws {
        let (s, id, _, _) = try demo()
        #expect(failure(run("edit.undo", s, id))?.code == .unavailable)
        #expect(failure(run("editors.close_detail", s, id))?.code == .unavailable)
        #expect(failure(run("sequence.back", s, id))?.code == .unavailable)
        #expect(failure(run("editors.nudge_later", s, id))?.code == .unavailable)
        #expect(failure(run("modify.again", s, id))?.code == .unavailable)
        #expect(failure(run("editors.insert_event", s, id))?.code == .unavailable)
    }

    @Test func documentCommandsNeedADocumentButAppCommandsDoNot() {
        let s = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { demoFake() }, open: { _ in demoFake() })
        #expect(failure(run("edit.undo", s, nil))?.code == .noDocument)
        guard case .done = run("window.plugin_manager", s, nil) else {
            Issue.record("plugin manager needs no document")
            return
        }
        #expect(host.calls == ["plugins"])
        #expect(failure(run("file.print", s, nil))?.code == .unknownId)
    }

    @Test func editorsAndWindowsGoWhereTheMenuSendsThem() throws {
        let (s, id, m, _) = try demo()
        _ = run("editors.list", s, id)
        #expect(m.editor.layout.detail?.kind == .list)
        _ = run("editors.strip_chart_window", s, id)
        _ = run("window.mixer", s, id)
        _ = run("file.close", s, id)
        #expect(host.calls == ["tear stripChart", "mixer", "close"])
        _ = run("modify.exact_mode", [ControlArg(name: "on", value: .flag(value: true))], s, id)
        #expect(m.editor.exactMode)
        _ = run("modify.exact_mode", s, id)
        #expect(!m.editor.exactMode, "no `on` toggles")
        let r = ControlStates.state(.editor, id, m)
        guard case .editor(_, let e)? = r else {
            Issue.record("no editor state")
            return
        }
        #expect(e.detail?.kind == "list" && e.detail?.seq == 3 && e.tool == "arrow")
        #expect(ControlStates.facts(m).detailKind == "list")
        _ = run("editors.close_detail", s, id)
        #expect(ControlStates.facts(m).detailKind == "none")
    }

    @Test func storeCommandsAnswerWithTheNewDocument() {
        let s = DocumentStore(
            makeEmpty: { throw VizError.Io(message: "disk full") }, makeDemo: { demoFake() },
            open: { _ in demoFake() })
        guard case .opened(let command, let doc) = run("file.create_demo", s, nil) else {
            Issue.record("no document")
            return
        }
        #expect(command == "file.create_demo" && s.model(doc) != nil)
        let e = failure(run("file.new", s, nil))
        #expect(e?.code == .failed && e?.message == "New Document Failed: disk full")
        #expect(s.alert == nil, "the store's alert was taken, not shown")
    }

    @Test func openAnswersRecoveryQuestionsFromParameters() {
        let q = FakeQuestions()
        var opened: [OpenOptions] = []
        let s = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { demoFake() }, open: { _ in demoFake() })
        s.recovery = Recovery(
            check: { _ in RecoveryInfo(crashed: true, loading: [], autosave: nil) },
            openWith: { _, o in
                opened.append(o)
                return demoFake()
            },
            disable: { _ in true }, shiftHeld: { false }, questions: q)
        let none = RecoveryAnswersRow(plugins: nil, restoreAutosave: nil)
        guard case .failed(let e) = ControlOpen.open(path: "/p/Song.vizdaw", answers: none, store: s) else {
            Issue.record("opened without an answer")
            return
        }
        #expect(e.code == .dialogRequired && e.dialog == "Open with Plugins Off?")
        #expect(s.documents.isEmpty && q.asked.isEmpty, "no dialog, no document")
        #expect(s.recovery?.questions === q, "the app's questions are back")
        let off = RecoveryAnswersRow(plugins: .pluginsOff, restoreAutosave: nil)
        guard case .opened = ControlOpen.open(path: "/p/Song.vizdaw", answers: off, store: s) else {
            Issue.record("not opened")
            return
        }
        #expect(opened == [OpenOptions(pluginsOff: true, autosave: nil)])
    }

    @Test func theSocketComesFromTheFlagThenTheEnvironment() {
        #expect(ControlLaunch.socketPath(arguments: ["VizDAW", "--control", "/a/s"], environment: ["VIZDAW_CONTROL": "/b/s"]) == "/a/s")
        #expect(ControlLaunch.socketPath(arguments: ["VizDAW"], environment: ["VIZDAW_CONTROL": "/b/s"]) == "/b/s")
        #expect(ControlLaunch.socketPath(arguments: ["VizDAW"], environment: ["VIZDAW_CONTROL": ""]) == nil)
        #expect(ControlLaunch.socketPath(arguments: ["VizDAW", "--control"], environment: [:]) == nil)
        #expect(ControlLaunch.strip(["VizDAW", "--control", "/a/x.vizdaw", "--demo"]) == ["VizDAW", "--demo"])
    }
}
VIZDAW_EOF
scripts/swift.sh build --build-tests 2>&1 | grep -E "error:" | head -3
```

Expected: does not compile (`cannot find type 'ControlHost' in scope`).

- [ ] **Step 2: The handlers**

Each case does what the menu item's action in `AppCommands.swift` does, guarded by the item's `.disabled` condition (→ `unavailable`). Transport calls the session directly so a Busy or failed Play becomes the call's error, not an alert sheet; any other alert a command raises is taken from the model and returned as `failed`.

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Sources/VizDAWAppModel/ControlCommands.swift <<'VIZDAW_EOF'
import Foundation
import VizDAWCore

/// What a control call came to (control spec §4.3).
public enum ControlOutcome {
    /// Answered.
    case done(ControlResult)
    /// Failed.
    case failed(ControlFailure)
    /// Document `doc` was made or opened; the app answers once its window
    /// is up. `command` is the registry ID (nil for `app.*`).
    case opened(command: String?, doc: UUID)
}

extension ControlFailure {
    public static func of(_ code: ControlErrorCode, _ message: String, dialog: String? = nil) -> ControlFailure {
        ControlFailure(code: code, message: message, dialog: dialog)
    }

    /// A thrown error: Busy is `busy`, any other is `failed`.
    public static func thrown(_ error: any Error, _ title: String) -> ControlFailure {
        if case VizError.Busy(let message) = error { return .of(.busy, message) }
        return .alert(AlertItem(error, title: title))
    }

    /// An alert the call raised, taken instead of shown.
    public static func alert(_ a: AlertItem) -> ControlFailure { .of(.failed, "\(a.title): \(a.message)") }

    /// `dialog_required` for `dialog`, answered by `param`.
    public static func dialog(_ dialog: String, _ param: String) -> ControlFailure {
        .of(.dialogRequired, "this would open the \"\(dialog)\" dialog; pass `\(param)` instead", dialog: dialog)
    }
}

extension Array where Element == ControlArg {
    /// The text argument `name`.
    public func text(_ name: String) -> String? {
        for a in self where a.name == name {
            if case .text(let v) = a.value { return v }
        }
        return nil
    }

    /// The flag argument `name`.
    public func flag(_ name: String) -> Bool? {
        for a in self where a.name == name {
            if case .flag(let v) = a.value { return v }
        }
        return nil
    }
}

/// The windows and app-wide settings commands need; the app gives the
/// real one, tests a fake.
@MainActor
public protocol ControlHost: AnyObject {
    func showMixer(_ doc: UUID)
    func showPluginManager()
    func tearOff(_ doc: UUID, kind: DetailEditorKind)
    func closeDocument(_ doc: UUID)
    var nudge: NudgeSettings { get }
}

/// Runs registered menu commands (control spec §6) on a document model
/// the way the menu items do, minus their dialogs (spec §7).
@MainActor
public enum ControlCommands {
    static let detail: [String: DetailEditorKind] = [
        "editors.piano_roll": .pianoRoll, "editors.list": .list,
        "editors.strip_chart": .stripChart, "editors.drum_grid": .drumGrid,
    ]
    static let torn: [String: DetailEditorKind] = [
        "editors.piano_roll_window": .pianoRoll, "editors.list_window": .list,
        "editors.strip_chart_window": .stripChart, "editors.drum_grid_window": .drumGrid,
    ]
    static let nudges: [String: NudgeDirection] = [
        "editors.nudge_earlier": .earlier, "editors.nudge_later": .later,
        "editors.nudge_up": .up, "editors.nudge_down": .down,
    ]
    static let panels: [String: PanelTask] = [
        "modify.select_by_rule": .selectByRule, "modify.transpose": .transpose,
        "modify.quantize": .quantize, "modify.change": .change, "modify.move": .move,
    ]
    static let other: Set<String> = [
        "file.new", "file.open", "file.create_demo", "file.create_demo_audio", "file.close",
        "file.save", "file.save_as", "edit.undo", "edit.redo", "window.mixer",
        "window.plugin_manager", "editors.close_detail", "editors.insert_event", "modify.again",
        "modify.template", "modify.exact_mode", "sequence.make_reference", "sequence.back",
        "transport.play_stop", "transport.loop", "transport.return_to_start",
    ]

    /// Every registry ID `run` handles (a test checks it is all of them).
    public static var handled: Set<String> {
        other.union(detail.keys).union(torn.keys).union(nudges.keys).union(panels.keys)
    }

    /// Runs command `id` with `args` on `doc` (the target window's document,
    /// else the front one's; nil when none is open).
    public static func run(
        _ id: String, args: [ControlArg], doc: (id: UUID, model: DocumentModel)?,
        store: DocumentStore, host: any ControlHost
    ) -> ControlOutcome {
        guard handled.contains(id) else { return .failed(.of(.unknownId, "VizDAW has no handler for \(id).")) }
        switch id {
        case "file.new": return made(id, store.newEmpty(), store)
        case "file.create_demo": return made(id, store.newDemo(), store)
        case "file.create_demo_audio": return made(id, store.newDemoAudio(), store)
        case "file.open":
            guard let path = args.text("path") else { return .failed(.dialog("Open", "path")) }
            return ControlOpen.open(path: path, answers: ControlOpen.answers(args), store: store, command: id)
        case "window.plugin_manager":
            host.showPluginManager()
            return .done(ran(id, nil))
        default: break
        }
        guard let (docID, m) = doc else { return .failed(.of(.noDocument, "No document is open.")) }
        let before = m.alert?.id
        switch id {
        case "file.close":
            host.closeDocument(docID)
            return .done(ran(id, docID))
        case "file.save":
            if m.needsSaveAs {
                guard let path = args.text("path") else { return .failed(.dialog("Save", "path")) }
                m.saveAs(path: path)
            } else {
                m.save()
            }
        case "file.save_as":
            guard let path = args.text("path") else { return .failed(.dialog("Save", "path")) }
            m.saveAs(path: path)
        case "edit.undo", "edit.redo":
            let undo = id == "edit.undo"
            guard undo ? m.canUndo : m.canRedo else {
                return unavailable(undo ? "Nothing to undo." : "Nothing to redo.")
            }
            guard m.editor.gesture == nil else { return unavailable(EditorModel.gestureOpenMessage) }
            undo ? m.undo() : m.redo()
        case "window.mixer":
            host.showMixer(docID)
        case _ where detail[id] != nil:
            m.editor.showDetail(detail[id] ?? .pianoRoll, shown: m.selectedSeq)
        case _ where torn[id] != nil:
            host.tearOff(docID, kind: torn[id] ?? .pianoRoll)
        case "editors.close_detail":
            guard m.editor.layout.detail != nil else { return unavailable("No detail editor is open.") }
            m.editor.closeDetail()
        case "editors.insert_event":
            guard let list = m.listForCommands(), !m.editor.textEditing else {
                return unavailable("Insert Event needs a list editor that has the keyboard.")
            }
            list.insert()
        case _ where nudges[id] != nil:
            guard m.transportKeysEnabled, !m.editor.selection.events.isEmpty else {
                return unavailable("Nudge needs selected events and no text being edited.")
            }
            if !m.editor.nudge(nudges[id] ?? .later, settings: host.nudge) {
                return .failed(.of(.failed, m.editor.status ?? "The nudge was refused."))
            }
        case _ where panels[id] != nil:
            m.selectModify.open(panels[id] ?? .change)
        case "modify.again":
            guard m.selectModify.canModifyAgain else { return unavailable("There is no modification to repeat.") }
            if !m.selectModify.modifyAgain() {
                return .failed(.of(.failed, m.editor.status ?? "Modify Again was refused."))
            }
        case "modify.template":
            let name = args.text("name") ?? ""
            guard let t = m.editor.modifyTemplates().first(where: { $0.name == name }) else {
                return .failed(.of(.invalidParams, "modify.template: no template named \"\(name)\""))
            }
            if !m.selectModify.run(t) {
                return .failed(.of(.failed, m.editor.status ?? "The template was refused."))
            }
        case "modify.exact_mode":
            m.editor.exactMode = args.flag("on") ?? !m.editor.exactMode
        case "sequence.make_reference":
            guard let seq = m.selectedSeq, m.editor.gesture == nil else {
                return unavailable("Select a sequence and finish the drag first.")
            }
            if !m.editor.makeReferenceFromSelection(seq: seq) {
                return .failed(.of(.failed, m.editor.status ?? "Make Reference was refused."))
            }
        case "sequence.back":
            guard !m.editor.navigation.isEmpty else { return unavailable("There is no parent sequence to go back to.") }
            m.editor.back()
        case "transport.play_stop":
            guard m.canPlay else { return unavailable("Select a sequence to play.") }
            guard m.transportKeysEnabled else { return unavailable("A text field has the keyboard.") }
            let playing = m.isPlaying
            do {
                if playing {
                    try m.session.stop()
                } else if let seq = m.selectedSeq {
                    try m.session.play(seq: seq)
                }
            } catch {
                return .failed(.thrown(error, playing ? "Stop Failed" : "Play Failed"))
            }
            m.poll()
        case "transport.loop":
            if (args.flag("on") ?? !m.looping) != m.looping { m.toggleLoop() }
        case "transport.return_to_start":
            guard m.returnKeyEnabled else { return unavailable("A text field or list has the keyboard.") }
            m.returnToStart()
        default:
            return .failed(.of(.unknownId, "VizDAW has no handler for \(id)."))
        }
        // An alert the command raised is the call's failure, not a sheet.
        if let a = m.alert, a.id != before {
            m.alert = nil
            return .failed(.alert(a))
        }
        return .done(ran(id, docID, m))
    }

    static func unavailable(_ message: String) -> ControlOutcome { .failed(.of(.unavailable, message)) }

    static func ran(_ id: String, _ doc: UUID?, _ m: DocumentModel? = nil) -> ControlResult {
        .ran(
            command: id, doc: doc?.uuidString, window: nil, undoLabel: m?.snapshot.undoLabel,
            status: m?.editor.status)
    }

    /// A store command that makes a document: its window opens next.
    static func made(_ id: String, _ doc: UUID?, _ store: DocumentStore) -> ControlOutcome {
        if let doc { return .opened(command: id, doc: doc) }
        if let a = store.takeAlert() { return .failed(.alert(a)) }
        return .failed(.of(.failed, "\(id) made no document."))
    }
}

/// Opening a project without dialogs (control spec §7): the recovery
/// questions are answered from the call's parameters; a question without
/// its answer fails the call naming the dialog.
@MainActor
public enum ControlOpen {
    /// The answers in `command.run file.open` arguments.
    public static func answers(_ args: [ControlArg]) -> RecoveryAnswersRow {
        let plugins: CrashAnswerRow? =
            switch args.text("plugins") {
            case "normal": .normal
            case "off": .pluginsOff
            case "disable": .disable
            default: nil
            }
        return RecoveryAnswersRow(plugins: plugins, restoreAutosave: args.flag("restore_autosave"))
    }

    public static func open(
        path: String, answers: RecoveryAnswersRow, store: DocumentStore, command: String? = nil
    ) -> ControlOutcome {
        let asked = ControlAnswers(answers)
        let saved = store.recovery?.questions
        store.recovery?.questions = asked
        defer { if let saved { store.recovery?.questions = saved } }
        let alertBefore = store.alert?.id
        let id = store.open(path: path)
        if let missing = asked.missing {
            if let id { store.close(id) }
            return .failed(.dialog(missing.dialog, missing.param))
        }
        guard let id else {
            if let a = store.alert, a.id != alertBefore {
                _ = store.takeAlert()
                return .failed(.alert(a))
            }
            return .failed(.of(.failed, "Opening \(path) was cancelled."))
        }
        return .opened(command: command, doc: id)
    }
}

/// Recovery questions answered from a control call.
@MainActor
final class ControlAnswers: RecoveryQuestions {
    let answers: RecoveryAnswersRow
    /// The first question asked without an answer.
    private(set) var missing: (dialog: String, param: String)?

    init(_ answers: RecoveryAnswersRow) { self.answers = answers }

    func afterCrash(project: String, loading: [PluginRefRow], crashed: Bool) -> CrashChoice {
        switch answers.plugins {
        case .normal: return .normally
        case .pluginsOff: return .pluginsOff
        case .disable: return .disable
        case nil:
            if missing == nil { missing = ("Open with Plugins Off?", "plugins") }
            return .cancel
        }
    }

    func restoreAutosave(project: String) -> Bool {
        guard let r = answers.restoreAutosave else {
            if missing == nil { missing = ("Restore Autosave", "restore_autosave") }
            return false
        }
        return r
    }
}

/// `state.get` for a document, and the facts a `wait` reads.
@MainActor
public enum ControlStates {
    /// The protocol's name of a detail editor kind.
    public static func kindName(_ k: DetailEditorKind) -> String {
        switch k {
        case .pianoRoll: "piano_roll"
        case .list: "list"
        case .stripChart: "strip_chart"
        case .drumGrid: "drum_grid"
        }
    }

    public static func detailRow(_ d: DetailState) -> DetailRow {
        let kind = kindName(d.kind)
        switch d.target {
        case .block(let seq, let lane, let start, let end):
            return DetailRow(kind: kind, seq: seq, lane: lane, start: start, end: end)
        case .lane(let seq, let lane):
            return DetailRow(kind: kind, seq: seq, lane: lane, start: nil, end: nil)
        case .sequence(let seq):
            return DetailRow(kind: kind, seq: seq, lane: nil, start: nil, end: nil)
        }
    }

    /// `target` of document `id`; the transport is read fresh.
    public static func state(_ target: StateTargetRow, _ id: UUID, _ m: DocumentModel) -> ControlResult? {
        let doc = id.uuidString
        switch target {
        case .transport:
            m.poll()
            return .transport(doc: doc, state: m.transport)
        case .document:
            return .document(
                doc: doc, path: (try? m.session.path()) ?? nil, snapshot: m.snapshot,
                selectedSeq: m.selectedSeq, lanes: m.lanes,
                alert: m.alert.map { AlertRow(title: $0.title, message: $0.message) })
        case .editor:
            let e = m.editor
            return .editor(
                doc: doc,
                editor: EditorStateRow(
                    detail: e.layout.detail.map(detailRow),
                    windows: e.layout.windows.map { TornRow(id: $0.id.uuidString, detail: detailRow($0.detail)) },
                    tool: e.tool.rawValue, snapEnabled: e.layout.snap.enabled,
                    snapDivision: e.layout.snap.division.rawValue, exactMode: e.exactMode,
                    gesture: e.gesture != nil, status: e.status))
        case .selection:
            return .selection(doc: doc, selection: m.editor.selection)
        case .windows:
            return nil
        }
    }

    /// What a wait reads of document `m` (its transport read fresh: a
    /// window in the background does not poll).
    public static func facts(_ m: DocumentModel) -> DocFactsRow {
        m.poll()
        return DocFactsRow(
            playing: m.transport.playing, tick: m.transport.tick,
            detailKind: m.editor.layout.detail.map { kindName($0.kind) } ?? "none")
    }
}

/// Where the control socket goes (control spec §8): `--control PATH`
/// wins over `VIZDAW_CONTROL`; neither, or an empty one, is off.
public enum ControlLaunch {
    public static func socketPath(arguments: [String], environment: [String: String]) -> String? {
        if let i = arguments.firstIndex(of: "--control") {
            let next = arguments.index(after: i)
            return next < arguments.endIndex && !arguments[next].isEmpty ? arguments[next] : nil
        }
        if let e = environment["VIZDAW_CONTROL"], !e.isEmpty { return e }
        return nil
    }

    /// `arguments` without `--control` and its path (so the path is never
    /// taken for a project).
    public static func strip(_ arguments: [String]) -> [String] {
        guard let i = arguments.firstIndex(of: "--control") else { return arguments }
        var a = arguments
        a.remove(at: i)
        if i < a.endIndex { a.remove(at: i) }
        return a
    }
}
VIZDAW_EOF
```

- [ ] **Step 3: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh test --filter ControlCommandsTests 2>&1 | grep -E "error:|✘|Test run"
```

Expected: `Test run with 10 tests in 1 suite passed`.

- [ ] **Step 4: Gate and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all -- --check && scripts/build-ffi.sh && scripts/swift.sh build 2>&1 | tail -3 && scripts/swift.sh test 2>&1 | tail -3
git add app/Sources/VizDAWAppModel/ControlCommands.swift app/Tests/VizDAWAppModelTests/ControlCommandsTests.swift
git commit -m "feat(app): run registered menu commands without dialogs; control state rows

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
```

Expected: Swift 561 tests pass.

---

### Task 7: Swift — the accessibility tree with element IDs, mouse and key events; canvases' element frames and first clicks

**Files:**
- Create: `app/Sources/VizDAWEditors/ControlUI.swift`
- Modify: `app/Sources/VizDAWEditors/EditorCanvas.swift`, `LaneHeaderView.swift`, `RegionStripView.swift`, `PianoKeyboardView.swift`, `ExpressionPaneView.swift`
- Test: `app/Tests/VizDAWEditorsTests/ControlUITests.swift`

**Interfaces:**
- Consumes: `UiNodeRow`, `ElementRefRow`, `PointerRow`, `MouseButtonRow`, `ModifiersRow`, `KeyStrokeRow`, `controlElementCheck(role:label:title:)`; `CanvasElement` and `ItemsCanvas` (the test canvas in `EditorCanvasTests.swift`).
- Produces: `ControlUI.maxNodes = 5000`, `enableAccessibility()`, `tree(of:maxDepth:) -> (nodes: [UiNodeRow], truncated: Bool)`, `windowRect(_:in:) -> CGRect`, `element(_:in:) -> (any NSAccessibilityProtocol)?`, `point(_:in:) -> CGPoint?`, `clickEvents(_:at:button:count:modifiers:) -> [NSEvent]`, `dragEvents(_:from:to:steps:modifiers:) -> [NSEvent]`, `keyEvents(_:_:) -> [NSEvent]`, `firstMouse: Bool`, `post(_:to:)`, `deliver(_:to:)`, `eventsPending: Bool`; `CanvasElement.parentSpace(_:in:) -> CGRect`.

Spec §5.4, §5.5; Rulings 6–10, 12.

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Tests/VizDAWEditorsTests/ControlUITests.swift <<'VIZDAW_EOF'
import AppKit
import Testing
import VizDAWCore
@testable import VizDAWEditors

/// Records the events it gets, in its own (flipped) coordinates.
@MainActor
final class RecordingView: NSView {
    var got: [String] = []
    override var isFlipped: Bool { true }
    override var acceptsFirstResponder: Bool { true }
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }
    private func note(_ name: String, _ e: NSEvent) {
        let p = convert(e.locationInWindow, from: nil)
        got.append("\(name) \(Int(p.x.rounded())),\(Int(p.y.rounded()))")
    }
    override func mouseDown(with e: NSEvent) { note("down", e) }
    override func mouseDragged(with e: NSEvent) { note("drag", e) }
    override func mouseUp(with e: NSEvent) { note("up", e) }
    override func rightMouseDown(with e: NSEvent) { note("rdown", e) }
    override func keyDown(with e: NSEvent) {
        got.append("key \(e.keyCode) \(e.characters ?? "") \(e.modifierFlags.contains(.command) ? "cmd" : "")")
    }
}

let none = ModifiersRow(shift: false, control: false, option: false, command: false)

@MainActor
@Suite struct ControlUITests {
    /// A 400×300 window: a canvas with two items over the top half, a
    /// recording view over the bottom half.
    func window() -> (NSWindow, ItemsCanvas, RecordingView) {
        let w = NSWindow(
            contentRect: NSRect(x: 100, y: 100, width: 400, height: 300), styleMask: [.titled],
            backing: .buffered, defer: false)
        w.isReleasedWhenClosed = false
        let c = ItemsCanvas(frame: NSRect(x: 0, y: 150, width: 400, height: 150))
        let r = RecordingView(frame: NSRect(x: 0, y: 0, width: 400, height: 150))
        w.contentView?.addSubview(c)
        w.contentView?.addSubview(r)
        c.layout()
        return (w, c, r)
    }

    @Test func theTreeListsTheCanvasItemsWithCheckedPaths() throws {
        let (w, _, _) = window()
        let (nodes, truncated) = ControlUI.tree(of: w, maxDepth: 64)
        #expect(!truncated)
        let chords = try #require(nodes.first { $0.label == "Block Chords, bars 1 to 2" })
        #expect(chords.role == "AXButton" && chords.selected && chords.actions == ["press"])
        let check = controlElementCheck(role: chords.role, label: chords.label, title: chords.title)
        let ref = ElementRefRow(id: "", path: chords.path, check: check)
        #expect(ControlUI.element(ref, in: w) != nil)
        let stale = ElementRefRow(id: "", path: chords.path, check: check ^ 1)
        #expect(ControlUI.element(stale, in: w) == nil, "a changed element is not clicked")
        let gone = ElementRefRow(id: "", path: chords.path + [99], check: check)
        #expect(ControlUI.element(gone, in: w) == nil)
        // The item's frame is in window points, top-left origin: the canvas
        // fills the top half of the content, under the title bar.
        let titleBar = w.frame.height - w.contentLayoutRect.height
        #expect(abs(chords.y - (titleBar + EditorCanvas.rulerHeight)) < 1, "\(chords.y)")
        let p = try #require(ControlUI.point(.element(element: ref), in: w))
        #expect(abs(p.x - 25) < 1)
        #expect(ControlUI.tree(of: w, maxDepth: 1).nodes.allSatisfy { $0.path.count == 1 })
    }

    /// Where `events` land in `view`, as "type x,y" (the window routes a
    /// mouse event by its location; `ControlUI.post` queues them).
    func landing(_ events: [NSEvent], in view: NSView) -> [String] {
        events.map { e in
            let p = view.convert(e.locationInWindow, from: nil)
            let name: String =
                switch e.type {
                case .leftMouseDown: "down"
                case .leftMouseUp: "up"
                case .leftMouseDragged: "drag"
                case .rightMouseDown: "rdown"
                case .rightMouseUp: "rup"
                default: "?"
                }
            return "\(name) \(Int(p.x.rounded())),\(Int(p.y.rounded()))"
        }
    }

    @Test func clicksAndDragsLandWhereTheyAreAimed() {
        let (w, _, r) = window()
        let titleBar = w.frame.height - w.contentLayoutRect.height
        let top = titleBar + 150  // the recording view's top edge, window points
        let clicks = ControlUI.clickEvents(w, at: CGPoint(x: 30, y: top + 20), button: .left, count: 2, modifiers: none)
        #expect(landing(clicks, in: r) == ["down 30,20", "up 30,20", "down 30,20", "up 30,20"])
        #expect(clicks.map(\.clickCount) == [1, 1, 2, 2], "the second press is a double click")
        #expect(clicks.allSatisfy { $0.windowNumber == w.windowNumber })
        let right = ControlUI.clickEvents(w, at: CGPoint(x: 5, y: top + 5), button: .right, count: 1, modifiers: none)
        #expect(landing(right, in: r) == ["rdown 5,5", "rup 5,5"])
        let drag = ControlUI.dragEvents(
            w, from: CGPoint(x: 10, y: top + 10), to: CGPoint(x: 50, y: top + 30), steps: 2,
            modifiers: ModifiersRow(shift: false, control: false, option: true, command: false))
        #expect(drag.allSatisfy { $0.modifierFlags.contains(.option) })
        #expect(landing(drag, in: r) == ["down 10,10", "drag 30,20", "drag 50,30", "up 50,30"])
        // Delivered, the recording view gets them where they were aimed.
        w.makeFirstResponder(r)
        r.mouseDown(with: drag[0])
        #expect(r.got == ["down 10,10"])
    }

    @Test func keyStrokesReachTheFirstResponder() {
        let (w, _, r) = window()
        w.makeFirstResponder(r)
        let cmdS = KeyStrokeRow(
            keyCode: 1, characters: "s", unmodified: "s",
            modifiers: ModifiersRow(shift: false, control: false, option: false, command: true), command: "file.save")
        let space = KeyStrokeRow(keyCode: 49, characters: " ", unmodified: " ", modifiers: none, command: nil)
        let events = ControlUI.keyEvents(w, [space, cmdS])
        #expect(events.map(\.type) == [NSEvent.EventType.keyDown, .keyUp, .keyDown, .keyUp])
        ControlUI.deliver(events, to: w)
        #expect(r.got == ["key 49   ", "key 1 s cmd"])
    }
}
VIZDAW_EOF
scripts/swift.sh build --build-tests 2>&1 | grep -E "error:" | head -3
```

Expected: does not compile (`cannot find 'ControlUI' in scope`).

- [ ] **Step 2: Tree, IDs and events**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Sources/VizDAWEditors/ControlUI.swift <<'VIZDAW_EOF'
import AppKit
import VizDAWCore

/// The control channel's window-side work (control spec §9, §10): the
/// accessibility tree with element IDs, and mouse and key events for
/// VizDAW's own windows. Events are posted to the app's own event queue
/// (`NSApp.postEvent`), never through Quartz: no system-wide events, no
/// real cursor movement, no Accessibility or Screen Recording permission.
/// Posting (not `sendEvent`) lets controls that track the mouse in a loop
/// (buttons, sliders, menus) read the rest of the gesture from the queue.
/// Coordinates are window points with a top-left origin, as in screenshots.
@MainActor
public enum ControlUI {
    /// The most nodes one tree holds.
    public static let maxNodes = 5000

    // MARK: Tree

    private static var accessibilityOn = false

    /// SwiftUI builds its accessibility tree only while an assistive app
    /// asks for it: the channel asks the way VoiceOver does, by setting
    /// AppKit's enhanced-user-interface attribute on the application.
    public static func enableAccessibility() {
        guard !accessibilityOn else { return }
        accessibilityOn = true
        _ = NSApplication.shared.perform(
            NSSelectorFromString("accessibilitySetValue:forAttribute:"), with: NSNumber(value: true),
            with: "AXEnhancedUserInterface")
    }

    /// `window`'s accessibility tree in pre-order, at most `maxDepth` deep.
    public static func tree(of window: NSWindow, maxDepth: Int) -> (nodes: [UiNodeRow], truncated: Bool) {
        enableAccessibility()
        var nodes: [UiNodeRow] = []
        var truncated = false
        var seen: Set<ObjectIdentifier> = [ObjectIdentifier(window)]
        func walk(_ e: any NSAccessibilityProtocol, _ path: [UInt32]) {
            guard path.count < maxDepth else { return }
            for (i, child) in (e.accessibilityChildren() ?? []).enumerated() {
                guard nodes.count < maxNodes else {
                    truncated = true
                    return
                }
                guard let c = child as? (any NSAccessibilityProtocol & NSObjectProtocol),
                    seen.insert(ObjectIdentifier(c)).inserted
                else { continue }
                let p = path + [UInt32(i)]
                nodes.append(row(c, path: p, in: window))
                walk(c, p)
            }
        }
        walk(window, [])
        return (nodes, truncated)
    }

    static func text(_ v: Any?) -> String? {
        switch v {
        case let s as String: s
        case let n as NSNumber: n.stringValue
        case let a as NSAttributedString: a.string
        default: nil
        }
    }

    static let pressable: Set<String> = [
        "AXButton", "AXCheckBox", "AXRadioButton", "AXMenuItem", "AXPopUpButton", "AXMenuButton",
        "AXDisclosureTriangle", "AXCell", "AXRow", "AXLink",
    ]

    static func row(_ e: any NSAccessibilityProtocol, path: [UInt32], in window: NSWindow) -> UiNodeRow {
        let role = e.accessibilityRole()?.rawValue ?? "AXUnknown"
        let f = windowRect(e.accessibilityFrame(), in: window)
        return UiNodeRow(
            path: path, role: role, subrole: e.accessibilitySubrole()?.rawValue,
            label: e.accessibilityLabel(), title: e.accessibilityTitle(), value: text(e.accessibilityValue()),
            x: f.minX, y: f.minY, width: f.width, height: f.height,
            enabled: e.isAccessibilityEnabled(), selected: e.isAccessibilitySelected(),
            focused: e.isAccessibilityFocused(), actions: pressable.contains(role) ? ["press"] : [])
    }

    /// A screen rect (AppKit, bottom-left origin) in window points with a
    /// top-left origin.
    public static func windowRect(_ screen: NSRect, in window: NSWindow) -> CGRect {
        let r = window.convertFromScreen(screen)
        return CGRect(x: r.minX, y: window.frame.height - r.maxY, width: r.width, height: r.height)
    }

    /// The element `ref` names, if it is still there with the same role
    /// and label.
    public static func element(_ ref: ElementRefRow, in window: NSWindow) -> (any NSAccessibilityProtocol)? {
        enableAccessibility()
        var e: any NSAccessibilityProtocol = window
        for i in ref.path {
            let kids = e.accessibilityChildren() ?? []
            guard Int(i) < kids.count, let next = kids[Int(i)] as? any NSAccessibilityProtocol else { return nil }
            e = next
        }
        let role = e.accessibilityRole()?.rawValue ?? "AXUnknown"
        let check = controlElementCheck(role: role, label: e.accessibilityLabel(), title: e.accessibilityTitle())
        return check == ref.check ? e : nil
    }

    /// Where `p` lands in `window` (an element's centre); nil when the
    /// element is gone.
    public static func point(_ p: PointerRow, in window: NSWindow) -> CGPoint? {
        switch p {
        case .point(let x, let y):
            return CGPoint(x: x, y: y)
        case .element(let ref):
            guard let e = element(ref, in: window) else { return nil }
            let f = windowRect(e.accessibilityFrame(), in: window)
            return CGPoint(x: f.midX, y: f.midY)
        }
    }

    // MARK: Events

    static func flags(_ m: ModifiersRow) -> NSEvent.ModifierFlags {
        var f: NSEvent.ModifierFlags = []
        if m.shift { f.insert(.shift) }
        if m.control { f.insert(.control) }
        if m.option { f.insert(.option) }
        if m.command { f.insert(.command) }
        return f
    }

    private static var eventNumber = 0

    static func mouse(
        _ type: NSEvent.EventType, _ window: NSWindow, _ p: CGPoint, _ flags: NSEvent.ModifierFlags, clicks: Int
    ) -> NSEvent? {
        eventNumber += 1
        let pressure: Float = (type == .leftMouseUp || type == .rightMouseUp) ? 0 : 1
        return NSEvent.mouseEvent(
            with: type, location: NSPoint(x: p.x, y: window.frame.height - p.y), modifierFlags: flags,
            timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber, context: nil,
            eventNumber: eventNumber, clickCount: clicks, pressure: pressure)
    }

    /// A click (`count` presses, each down and up) at `p`.
    public static func clickEvents(
        _ window: NSWindow, at p: CGPoint, button: MouseButtonRow, count: Int, modifiers: ModifiersRow
    ) -> [NSEvent] {
        let (down, up): (NSEvent.EventType, NSEvent.EventType) =
            button == .right ? (.rightMouseDown, .rightMouseUp) : (.leftMouseDown, .leftMouseUp)
        let f = flags(modifiers)
        return (1...max(1, count)).flatMap { n in
            [mouse(down, window, p, f, clicks: n), mouse(up, window, p, f, clicks: n)].compactMap { $0 }
        }
    }

    /// A press at `from`, `steps` drags along the line, a release at `to`.
    public static func dragEvents(
        _ window: NSWindow, from: CGPoint, to: CGPoint, steps: Int, modifiers: ModifiersRow
    ) -> [NSEvent] {
        let f = flags(modifiers)
        let n = max(1, steps)
        var out = [mouse(.leftMouseDown, window, from, f, clicks: 1)]
        for k in 1...n {
            let t = CGFloat(k) / CGFloat(n)
            let p = CGPoint(x: from.x + (to.x - from.x) * t, y: from.y + (to.y - from.y) * t)
            out.append(mouse(.leftMouseDragged, window, p, f, clicks: 1))
        }
        out.append(mouse(.leftMouseUp, window, to, f, clicks: 1))
        return out.compactMap { $0 }
    }

    /// Down and up for every stroke.
    public static func keyEvents(_ window: NSWindow, _ strokes: [KeyStrokeRow]) -> [NSEvent] {
        strokes.flatMap { s -> [NSEvent] in
            [NSEvent.EventType.keyDown, .keyUp].compactMap { type in
                NSEvent.keyEvent(
                    with: type, location: .zero, modifierFlags: flags(s.modifiers),
                    timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber,
                    context: nil, characters: s.characters, charactersIgnoringModifiers: s.unmodified,
                    isARepeat: false, keyCode: s.keyCode)
            }
        }
    }

    /// True while channel mouse events are queued: the editors accept them
    /// as first clicks, since VizDAW may not be the active app (macOS does
    /// not let a background app take focus) and AppKit would otherwise
    /// spend the click on activating the window.
    public static var firstMouse = false

    /// Orders `window` to the front (without stealing focus from the
    /// active app) and queues `events` behind anything already queued.
    public static func post(_ events: [NSEvent], to window: NSWindow) {
        window.orderFront(nil)
        firstMouse = true
        for e in events { NSApp.postEvent(e, atStart: false) }
    }

    /// Sends key `events` straight to `window` (its first responder), which
    /// works whether or not VizDAW is active.
    public static func deliver(_ events: [NSEvent], to window: NSWindow) {
        for e in events { window.sendEvent(e) }
    }

    /// Whether the app's queue still holds posted mouse or key events.
    public static var eventsPending: Bool {
        let mask: NSEvent.EventTypeMask = [
            .leftMouseDown, .leftMouseUp, .leftMouseDragged, .rightMouseDown, .rightMouseUp, .keyDown, .keyUp,
        ]
        return NSApp.nextEvent(matching: mask, until: .distantPast, inMode: .default, dequeue: false) != nil
    }
}
VIZDAW_EOF
scripts/swift.sh test --filter ControlUITests 2>&1 | grep -E "✘|Test run|↳   " | head -8
```

Expected: `theTreeListsTheCanvasItemsWithCheckedPaths` fails on the `chords.y` expectation (the item's frame is mirrored: `NSAccessibilityElement` reads frames in a bottom-left parent space although the canvas is flipped); the other two pass.

- [ ] **Step 3: Element frames the right way up; first clicks from the channel**

In `app/Sources/VizDAWEditors/EditorCanvas.swift`, Find:

```swift
    nonisolated(unsafe) private var press: (@MainActor () -> Void)?

    init(_ item: CanvasAccessibilityItem, parent: NSView, frame: CGRect) {
        super.init()
        setAccessibilityRole(.button)
        setAccessibilityParent(parent)
        update(item, frame: frame)
    }

    func update(_ item: CanvasAccessibilityItem, frame: CGRect) {
        press = item.press
        setAccessibilityLabel(item.label)
        setAccessibilityFrameInParentSpace(frame)
        setAccessibilitySelected(item.selected)
    }
```

Replace:

```swift
    nonisolated(unsafe) private var press: (@MainActor () -> Void)?
    /// The view whose coordinates item frames are given in.
    private weak var host: NSView?

    init(_ item: CanvasAccessibilityItem, parent: NSView, frame: CGRect) {
        super.init()
        host = parent
        setAccessibilityRole(.button)
        setAccessibilityParent(parent)
        update(item, frame: frame)
    }

    func update(_ item: CanvasAccessibilityItem, frame: CGRect) {
        press = item.press
        setAccessibilityLabel(item.label)
        setAccessibilityFrameInParentSpace(Self.parentSpace(frame, in: host))
        setAccessibilitySelected(item.selected)
    }

    /// `frame` (in the parent's own coordinates) in the bottom-left space
    /// `NSAccessibilityElement` reads whatever the parent's `isFlipped`.
    /// The canvases are flipped: without this, VoiceOver's outline and the
    /// control channel's element frames were mirrored top to bottom.
    static func parentSpace(_ frame: CGRect, in parent: NSView?) -> CGRect {
        guard let p = parent, p.isFlipped else { return frame }
        return CGRect(x: frame.minX, y: p.bounds.height - frame.maxY, width: frame.width, height: frame.height)
    }
```

Find:

```swift
    public override var isFlipped: Bool { true }
    public override var acceptsFirstResponder: Bool { true }

    private func setUp() {
```

Replace:

```swift
    public override var isFlipped: Bool { true }
    public override var acceptsFirstResponder: Bool { true }

    /// A click the control channel sends is never a window-activating
    /// first click (VizDAW may not be the active app; control spec §5.5).
    open override func acceptsFirstMouse(for event: NSEvent?) -> Bool {
        ControlUI.firstMouse || super.acceptsFirstMouse(for: event)
    }

    private func setUp() {
```

Find:

```swift
        verticalOffset = clampVertical(verticalOffset)
        movePlayhead(now: Self.hostNow())
    }
```

Replace:

```swift
        verticalOffset = clampVertical(verticalOffset)
        movePlayhead(now: Self.hostNow())
        // Element frames depend on the height (see `CanvasElement.parentSpace`).
        accessibilityStale = true
    }
```

In each of `LaneHeaderView.swift` and `PianoKeyboardView.swift`, Find the first `    public override var isFlipped: Bool { true }` and in each of `RegionStripView.swift` and `ExpressionPaneView.swift` Find `    public override var acceptsFirstResponder: Bool { true }`; Replace each with itself followed by:

```swift

    /// A click the control channel sends is never a window-activating
    /// first click (VizDAW may not be the active app; control spec §5.5).
    public override func acceptsFirstMouse(for event: NSEvent?) -> Bool {
        ControlUI.firstMouse || super.acceptsFirstMouse(for: event)
    }
```

- [ ] **Step 4: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh test --filter "ControlUITests|EditorCanvasTests" 2>&1 | grep -E "error:|✘|Test run"
```

Expected: `Test run with 9 tests in 2 suites passed` (3 new, 6 existing canvas tests, including `accessibilityElementsKeepTheirIdentityAndFollowTheAxis`).

- [ ] **Step 5: Gate and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh build 2>&1 | tail -3 && scripts/swift.sh test 2>&1 | tail -3
git add app/Sources/VizDAWEditors/ControlUI.swift app/Sources/VizDAWEditors/EditorCanvas.swift app/Sources/VizDAWEditors/LaneHeaderView.swift app/Sources/VizDAWEditors/RegionStripView.swift app/Sources/VizDAWEditors/PianoKeyboardView.swift app/Sources/VizDAWEditors/ExpressionPaneView.swift app/Tests/VizDAWEditorsTests/ControlUITests.swift
git commit -m "feat(editors): accessibility tree with element IDs and in-app input events; fix mirrored element frames

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
```

Expected: Swift 564 tests pass.

---

### Task 8: Swift — window snapshots rendered by the app

**Files:**
- Create: `app/Sources/VizDAWEditors/WindowSnapshot.swift`
- Test: `app/Tests/VizDAWEditorsTests/WindowSnapshotTests.swift`

**Interfaces:**
- Consumes: AppKit, Core Animation, the Objective-C runtime (`class_getMethodImplementation`).
- Produces: `WindowSnapshot.root(of:) -> NSView?`, `png(of view: NSView, scale: CGFloat) -> (data: Data, width: Int, height: Int)?`, `png(of window: NSWindow, scale: CGFloat?)`, `drawsItself(_:)`, `redrawSelfDrawn(under:ctx:)`, `redraw(_:root:ctx:)`.

Spec §9; Ruling 11.

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Tests/VizDAWEditorsTests/WindowSnapshotTests.swift <<'VIZDAW_EOF'
import AppKit
import QuartzCore
import Testing
@testable import VizDAWEditors

/// A flipped view that fills itself green in `draw(_:)`, with a blue strip
/// along its bottom edge.
@MainActor
final class FilledView: NSView {
    var fill = NSColor(srgbRed: 0, green: 1, blue: 0, alpha: 1)
    override var isFlipped: Bool { true }
    override func draw(_ dirtyRect: NSRect) {
        fill.setFill()
        bounds.fill()
        NSColor(srgbRed: 0, green: 0, blue: 1, alpha: 1).setFill()
        NSRect(x: 0, y: bounds.height - 5, width: bounds.width, height: 5).fill()
    }
}

@MainActor
@Suite struct WindowSnapshotTests {
    /// The dominant channel at `x`, `y` (row 0 is the top): "r", "g", "b"
    /// or "?" (colour management shifts exact values a little).
    func pixel(_ data: Data, _ x: Int, _ y: Int) -> String {
        guard let rep = NSBitmapImageRep(data: data), let c = rep.colorAt(x: x, y: y)?.usingColorSpace(.sRGB) else {
            return "none"
        }
        let (r, g, b) = (c.redComponent, c.greenComponent, c.blueComponent)
        if r > 0.8 && g < 0.4 && b < 0.4 { return "r" }
        if g > 0.8 && r < 0.4 && b < 0.4 { return "g" }
        if b > 0.8 && r < 0.4 && g < 0.4 { return "b" }
        return "?"
    }

    @Test func layerContentIsRenderedTheRightWayUp() throws {
        let v = NSView(frame: NSRect(x: 0, y: 0, width: 40, height: 20))
        v.wantsLayer = true
        let top = CALayer()
        top.frame = CGRect(x: 0, y: 10, width: 40, height: 10)  // y up: the top half
        top.backgroundColor = NSColor(srgbRed: 1, green: 0, blue: 0, alpha: 1).cgColor
        let bottom = CALayer()
        bottom.frame = CGRect(x: 0, y: 0, width: 40, height: 10)
        bottom.backgroundColor = NSColor(srgbRed: 0, green: 0, blue: 1, alpha: 1).cgColor
        v.layer?.addSublayer(top)
        v.layer?.addSublayer(bottom)
        let png = try #require(WindowSnapshot.png(of: v, scale: 2))
        #expect(png.width == 80 && png.height == 40)
        #expect(pixel(png.data, 5, 5) == "r", "row 0 is the top")
        #expect(pixel(png.data, 5, 35) == "b")
    }

    @Test func drawnFlippedViewsAndTheirSublayersAreIncluded() throws {
        let v = FilledView(frame: NSRect(x: 0, y: 0, width: 30, height: 30))
        v.wantsLayer = true
        let mark = CALayer()
        mark.frame = CGRect(x: 0, y: 0, width: 30, height: 5)  // flipped: the top strip
        mark.backgroundColor = NSColor(srgbRed: 1, green: 0, blue: 0, alpha: 1).cgColor
        v.layer?.addSublayer(mark)
        let png = try #require(WindowSnapshot.png(of: v, scale: 1))
        #expect(pixel(png.data, 15, 15) == "g", "draw(_:) content")
        #expect(pixel(png.data, 15, 28) == "b", "drawn at the bottom, shown at the bottom")
        #expect(pixel(png.data, 15, 1) == "r", "a flipped view's sublayer at y 0 is at its top")
    }

    @Test func aWindowIsRenderedWholeAtItsScale() throws {
        let w = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 200, height: 100), styleMask: [.titled],
            backing: .buffered, defer: false)
        w.isReleasedWhenClosed = false
        let content = FilledView(frame: NSRect(x: 0, y: 0, width: 200, height: 100))
        w.contentView = content
        let png = try #require(WindowSnapshot.png(of: w, scale: 1))
        #expect(png.width == Int(w.frame.width) && png.height == Int(w.frame.height), "title bar included")
        #expect(pixel(png.data, 100, png.height - 50) == "g", "the content is in it")
    }

    @Test func aStaleLayerShowsWhatItsViewDrawsNow() throws {
        // AppKit does not redraw occluded windows: a layer can hold an old
        // picture. The snapshot draws self-drawing views again.
        let parent = NSView(frame: NSRect(x: 0, y: 0, width: 40, height: 40))
        parent.wantsLayer = true
        let v = FilledView(frame: NSRect(x: 0, y: 0, width: 40, height: 40))
        v.wantsLayer = true
        parent.addSubview(v)
        v.layer?.display()
        #expect(v.layer?.contents != nil, "drawn once into its layer")
        v.fill = NSColor(srgbRed: 1, green: 0, blue: 0, alpha: 1)  // no needsDisplay: the layer is stale
        let png = try #require(WindowSnapshot.png(of: parent, scale: 1))
        #expect(pixel(png.data, 20, 10) == "r", "what the view draws now")
        #expect(pixel(png.data, 20, 38) == "b", "the right way up")
    }
}
VIZDAW_EOF
scripts/swift.sh build --build-tests 2>&1 | grep -E "error:" | head -3
```

Expected: does not compile (`cannot find 'WindowSnapshot' in scope`).

- [ ] **Step 2: The renderer**

Step 1 of the spec's §9 alone (`CALayer.render(in:)`) passes the first three tests; `aStaleLayerShowsWhatItsViewDrawsNow` needs the redraw pass, and `drawnFlippedViewsAndTheirSublayersAreIncluded` the flip of a geometry-flipped root.

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Sources/VizDAWEditors/WindowSnapshot.swift <<'VIZDAW_EOF'
import AppKit
import ObjectiveC
import QuartzCore

/// A window as a PNG, rendered by the app itself (control spec §9), with
/// no Screen Recording permission:
///
/// 1. the window's layer tree is drawn with `CALayer.render(in:)`, which
///    brings in SwiftUI and Core Animation content;
/// 2. every view that draws itself (`draw(_:)` into its layer) is drawn
///    again, in place, front to back, with its own Core Animation
///    sublayers (the playhead) on top. AppKit does not redraw an occluded
///    window (behind the agent's terminal, or on a locked screen), so its
///    layers can hold a stale picture; drawing the views shows what they
///    hold now.
///
/// Content the window server composites alone (vibrancy materials, Metal
/// or video layers, plugin views of other processes) renders flat or empty.
@MainActor
public enum WindowSnapshot {
    /// The view whose layer covers the whole window, title bar included.
    public static func root(of window: NSWindow) -> NSView? {
        window.contentView?.superview ?? window.contentView
    }

    /// `view` as PNG data at `scale` pixels per point, with its size in pixels.
    public static func png(of view: NSView, scale: CGFloat) -> (data: Data, width: Int, height: Int)? {
        view.layoutSubtreeIfNeeded()
        let size = view.bounds.size
        let w = max(1, Int((size.width * scale).rounded()))
        let h = max(1, Int((size.height * scale).rounded()))
        guard w <= 16_384, h <= 16_384, let space = CGColorSpace(name: CGColorSpace.sRGB),
            let ctx = CGContext(
                data: nil, width: w, height: h, bitsPerComponent: 8, bytesPerRow: 0, space: space,
                bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)
        else { return nil }
        ctx.scaleBy(x: CGFloat(w) / max(size.width, 1), y: CGFloat(h) / max(size.height, 1))
        if let layer = view.layer {
            ctx.saveGState()
            // A flipped view's own layer is geometry-flipped: rendered on
            // its own (not inside its window) it would come out upside down.
            if layer.isGeometryFlipped {
                ctx.translateBy(x: 0, y: size.height)
                ctx.scaleBy(x: 1, y: -1)
            }
            layer.render(in: ctx)
            ctx.restoreGState()
            redrawSelfDrawn(under: view, ctx: ctx)
        } else {
            let g = NSGraphicsContext(cgContext: ctx, flipped: false)
            NSGraphicsContext.saveGraphicsState()
            NSGraphicsContext.current = g
            view.displayIgnoringOpacity(view.bounds, in: g)
            NSGraphicsContext.restoreGraphicsState()
        }
        guard let image = ctx.makeImage(),
            let data = NSBitmapImageRep(cgImage: image).representation(using: .png, properties: [:])
        else { return nil }
        return (data, w, h)
    }

    /// `window` as PNG data; `scale` nil uses the window's backing scale.
    public static func png(of window: NSWindow, scale: CGFloat?) -> (data: Data, width: Int, height: Int)? {
        guard let v = root(of: window) else { return nil }
        return png(of: v, scale: scale ?? window.backingScaleFactor)
    }

    /// Whether `v`'s class draws in `draw(_:)`.
    static func drawsItself(_ v: NSView) -> Bool {
        let sel = #selector(NSView.draw(_:))
        return class_getMethodImplementation(type(of: v), sel) != class_getMethodImplementation(NSView.self, sel)
    }

    /// Step 2: the self-drawn views under `root`, front to back.
    static func redrawSelfDrawn(under root: NSView, ctx: CGContext) {
        func walk(_ v: NSView) {
            for s in v.subviews where !s.isHidden && s.alphaValue > 0 {
                if s.layer?.contents != nil, drawsItself(s), !(s is NSVisualEffectView) {
                    redraw(s, root: root, ctx: ctx)
                }
                walk(s)
            }
        }
        walk(root)
    }

    /// Draws `s` where it is in `root` (bottom-left based context).
    static func redraw(_ s: NSView, root: NSView, ctx: CGContext) {
        let r = s.convert(s.bounds, to: root)
        let rect = root.isFlipped ? CGRect(x: r.minX, y: root.bounds.height - r.maxY, width: r.width, height: r.height) : r
        ctx.saveGState()
        defer { ctx.restoreGState() }
        ctx.clip(to: rect)
        ctx.translateBy(x: rect.minX, y: rect.minY)
        if s.isFlipped {
            ctx.translateBy(x: 0, y: rect.height)
            ctx.scaleBy(x: 1, y: -1)
        }
        ctx.translateBy(x: -s.bounds.minX, y: -s.bounds.minY)
        NSGraphicsContext.saveGraphicsState()
        NSGraphicsContext.current = NSGraphicsContext(cgContext: ctx, flipped: s.isFlipped)
        s.draw(s.bounds)
        NSGraphicsContext.restoreGraphicsState()
        // Its own Core Animation sublayers (not its subviews') on top.
        guard let layer = s.layer else { return }
        let views = Set(s.subviews.compactMap { $0.layer.map(ObjectIdentifier.init) })
        for sub in layer.sublayers ?? [] where !sub.isHidden && !views.contains(ObjectIdentifier(sub)) {
            ctx.saveGState()
            ctx.translateBy(x: sub.frame.minX, y: sub.frame.minY)
            sub.render(in: ctx)
            ctx.restoreGState()
        }
    }
}
VIZDAW_EOF
```

- [ ] **Step 3: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh test --filter WindowSnapshotTests 2>&1 | grep -E "error:|✘|Test run"
```

Expected: `Test run with 4 tests in 1 suite passed`. (Colours are compared by dominant channel: colour management shifts exact values by a few units.)

- [ ] **Step 4: Gate and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh build 2>&1 | tail -3 && scripts/swift.sh test 2>&1 | tail -3
git add app/Sources/VizDAWEditors/WindowSnapshot.swift app/Tests/VizDAWEditorsTests/WindowSnapshotTests.swift
git commit -m "feat(editors): window snapshots rendered in-app, fresh for occluded windows

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
```

Expected: Swift 568 tests pass.

---

### Task 9: Swift — the channel in the app: pump, windows, deferred answers, dialogs kept off screen

**Files:**
- Create: `app/Sources/VizDAW/ControlChannel.swift`
- Modify: `app/Sources/VizDAW/VizDAWApp.swift`, `AppCommands.swift`, `DocumentWindow.swift`, `MixerView.swift`, `EditorWindows.swift`, `PluginWindows.swift`, `ModifyPanel.swift`

**Interfaces:**
- Consumes: everything from Tasks 5–8; `appStore`, `editingPreferences`, `EditorWindows.tearOff`, SwiftUI `OpenWindowAction`.
- Produces: `ControlChannel.{server, active, alerts, start(path:), stop(), note(_:), pump()}`; `ControlWindows.{tag(_:_:doc:), list(), window(_:), window(of:), document(_:), openDocument(_:), openWindow}`; `WindowTag` (SwiftUI marker view); `AppControlHost`.

Spec §4.2–4.4, §5.5, §7; Rulings 3, 5, 13–16. The app layer has no unit tests (as before: AppKit wiring); the e2e test (Task 11) covers it, and Step 3 here smoke-tests it.

- [ ] **Step 1: The channel**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Sources/VizDAW/ControlChannel.swift <<'VIZDAW_EOF'
import AppKit
import SwiftUI
import VizDAWAppModel
import VizDAWCore
import VizDAWEditors

/// The agent control channel (control spec §4): off unless `--control
/// PATH` or `VIZDAW_CONTROL` names a socket. A 20 Hz main-thread timer
/// takes the calls Rust validated and queued, runs them like the user's
/// menu items, clicks and keys would, and answers. Calls that must wait
/// (a window to appear, posted events to be handled, a `wait`) are kept
/// as deferred answers and checked on every tick.
@MainActor
enum ControlChannel {
    private(set) static var server: ControlServer?
    private static var timer: Timer?
    /// App-level alerts the channel kept from showing (newest last).
    private(set) static var alerts: [AlertItem] = []
    private static var deferred: [Deferred] = []
    private static let clock = ContinuousClock()

    /// A call answered later: `step(expired)` answers and returns true
    /// when done (`expired`: its time is up, it must answer now).
    struct Deferred {
        let token: UInt64
        let deadline: ContinuousClock.Instant
        let step: @MainActor (_ expired: Bool) -> Bool
    }

    static var active: Bool { server != nil }

    /// Starts listening on `path`; a socket that cannot be made ends the
    /// app (exit status 2), so a launching agent sees why at once.
    static func start(path: String) {
        do {
            server = try ControlServer.start(path: path, appTimeoutMs: 60_000)
        } catch {
            FileHandle.standardError.write(Data("vizdaw: \(AlertItem(error, title: "Control").message)\n".utf8))
            exit(2)
        }
        FileHandle.standardError.write(Data("vizdaw: control channel on \(path)\n".utf8))
        let t = Timer(timeInterval: 0.05, repeats: true) { _ in MainActor.assumeIsolated { pump() } }
        // Common modes: answers keep coming while a menu or a drag tracks.
        RunLoop.main.add(t, forMode: .common)
        timer = t
    }

    /// The app quits: pending calls get `shutting_down`, the socket goes.
    static func stop() {
        timer?.invalidate()
        timer = nil
        try? server?.stop()
        server = nil
    }

    /// Keeps an alert the app would have shown (control spec §7).
    static func note(_ item: AlertItem) {
        FileHandle.standardError.write(Data("vizdaw: alert \(item.title): \(item.message)\n".utf8))
        alerts.append(item)
        if alerts.count > 20 { alerts.removeFirst(alerts.count - 20) }
    }

    static func pump() {
        guard let server else { return }
        let now = clock.now
        deferred = deferred.filter { d in
            guard server.isWaiting(token: d.token) else { return false }
            return !d.step(now >= d.deadline)
        }
        for _ in 0..<8 {
            guard let call = (try? server.nextCall()) ?? nil else { break }
            execute(call, server)
        }
    }

    static func answer(_ server: ControlServer, _ token: UInt64, _ outcome: ControlOutcome) {
        switch outcome {
        case .done(let r): _ = try? server.reply(token: token, result: r)
        case .failed(let f): _ = try? server.fail(token: token, failure: f)
        case .opened(let command, let doc): whenShown(server, token, command: command, doc: doc)
        }
    }

    static func later(_ token: UInt64, seconds: Double, _ step: @escaping @MainActor (Bool) -> Bool) {
        if !step(false) {
            deferred.append(Deferred(token: token, deadline: clock.now + .seconds(seconds), step: step))
        }
    }

    /// Opens `doc`'s window and answers once it is on screen.
    static func whenShown(_ server: ControlServer, _ token: UInt64, command: String?, doc: UUID) {
        ControlWindows.openDocument(doc)
        later(token, seconds: 30) { expired in
            if let w = ControlWindows.window(of: doc) {
                let r: ControlResult =
                    command.map {
                        .ran(command: $0, doc: doc.uuidString, window: Int64(w.windowNumber), undoLabel: nil, status: nil)
                    } ?? .opened(doc: doc.uuidString, window: Int64(w.windowNumber))
                _ = try? server.reply(token: token, result: r)
                return true
            }
            guard expired else { return false }
            _ = try? server.fail(
                token: token, failure: .of(.timeout, "the new document's window did not appear within 30 s"))
            return true
        }
    }

    static func execute(_ call: ControlCall, _ server: ControlServer) {
        let token = call.token
        if controlHostBusy() {
            answer(server, token, .failed(.of(.busy, "a plugin call is running on the main thread; try again")))
            return
        }
        switch call.request {
        case .runCommand(let id, let args, let window):
            answer(server, token, ControlCommands.run(
                id, args: args, doc: ControlWindows.document(window), store: appStore, host: AppControlHost.shared))
        case .state(let target, let window):
            if target == .windows {
                answer(server, token, .done(.windows(windows: ControlWindows.list(), alerts: alerts.map {
                    AlertRow(title: $0.title, message: $0.message)
                })))
            } else if let (id, m) = ControlWindows.document(window),
                let r = ControlStates.state(target, id, m)
            {
                answer(server, token, .done(r))
            } else {
                answer(server, token, .failed(.of(.noDocument, "No document is open in that window.")))
            }
        case .tree(let window, let maxDepth):
            guard let w = ControlWindows.window(window) else { return answer(server, token, noWindow) }
            let (nodes, truncated) = ControlUI.tree(of: w, maxDepth: Int(maxDepth))
            answer(server, token, .done(.tree(window: Int64(w.windowNumber), nodes: nodes, truncated: truncated)))
        case .click(let window, let at, let button, let count, let modifiers):
            guard let w = ControlWindows.window(window) else { return answer(server, token, noWindow) }
            guard let p = ControlUI.point(at, in: w) else { return answer(server, token, noElement) }
            let events = ControlUI.clickEvents(w, at: p, button: button, count: Int(count), modifiers: modifiers)
            send(server, token, events, w, p)
        case .drag(let window, let from, let to, let steps, let modifiers):
            guard let w = ControlWindows.window(window) else { return answer(server, token, noWindow) }
            guard let a = ControlUI.point(from, in: w), let b = ControlUI.point(to, in: w) else {
                return answer(server, token, noElement)
            }
            send(server, token, ControlUI.dragEvents(w, from: a, to: b, steps: Int(steps), modifiers: modifiers), w, b)
        case .key(let window, let focus, let strokes):
            guard let w = ControlWindows.window(window) else { return answer(server, token, noWindow) }
            var click: [NSEvent] = []
            if let focus {
                guard let p = ControlUI.point(.element(element: focus), in: w) else {
                    return answer(server, token, noElement)
                }
                click = ControlUI.clickEvents(
                    w, at: p, button: .left, count: 1,
                    modifiers: ModifiersRow(shift: false, control: false, option: false, command: false))
            }
            // The focusing click first; the keys once it was handled.
            send(server, token, click, w, .zero) { keys(server, token, strokes, w) }
        case .screenshot(let window, let path, let scale):
            guard let w = ControlWindows.window(window) else { return answer(server, token, noWindow) }
            guard let png = WindowSnapshot.png(of: w, scale: scale.map { CGFloat($0) }) else {
                return answer(server, token, .failed(.of(.failed, "the window could not be rendered")))
            }
            do {
                try png.data.write(to: URL(fileURLWithPath: path), options: .atomic)
                answer(server, token, .done(.screenshot(
                    window: Int64(w.windowNumber), path: path, width: UInt32(png.width), height: UInt32(png.height))))
            } catch {
                answer(server, token, .failed(.of(.failed, "cannot write \(path): \(error.localizedDescription)")))
            }
        case .newDemo(let audio):
            let id = audio ? appStore.newDemoAudio() : appStore.newDemo()
            answer(server, token, id.map { .opened(command: nil, doc: $0) }
                ?? .failed(appStore.takeAlert().map(ControlFailure.alert) ?? .of(.failed, "no demo was made")))
        case .open(let path, let answers):
            answer(server, token, ControlOpen.open(path: path, answers: answers, store: appStore))
        case .quit:
            _ = try? server.reply(token: token, result: .quitting)
            // Next turn: the reply is on its way before the app tears down.
            DispatchQueue.main.async { NSApp.terminate(nil) }
        case .wait(let condition, let window, let timeoutMs):
            let start = clock.now
            var last = facts(window)
            later(token, seconds: Double(timeoutMs) / 1000) { expired in
                last = facts(window)
                if controlWaitMet(condition: condition, facts: last) {
                    let ms = (clock.now - start).components
                    let elapsed = UInt64(ms.seconds * 1000) + UInt64(ms.attoseconds / 1_000_000_000_000_000)
                    _ = try? server.reply(
                        token: token, result: .waited(condition: ControlWindows.name(condition), elapsedMs: elapsed))
                    return true
                }
                guard expired else { return false }
                _ = try? server.waitTimedOut(token: token, condition: condition, facts: last, timeoutMs: timeoutMs)
                return true
            }
        }
    }

    static let noWindow = ControlOutcome.failed(.of(.noWindow, "No such VizDAW window (see state.get windows)."))
    static let noElement = ControlOutcome.failed(
        .of(.noElement, "The element is gone or changed (read ui.tree again)."))

    /// Posts `events` and, once the app has handled them, answers (or runs
    /// `then`, which answers).
    static func send(
        _ server: ControlServer, _ token: UInt64, _ events: [NSEvent], _ w: NSWindow, _ p: CGPoint,
        then: (@MainActor () -> Void)? = nil
    ) {
        ControlUI.post(events, to: w)
        later(token, seconds: 5) { expired in
            guard expired || !ControlUI.eventsPending else { return false }
            ControlUI.firstMouse = false
            if let then {
                then()
            } else {
                _ = try? server.reply(
                    token: token,
                    result: .input(window: Int64(w.windowNumber), events: UInt32(events.count), x: p.x, y: p.y))
            }
            return true
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
                case .failed(let f):
                    _ = try? server.fail(token: token, failure: f)
                    return
                case .done, .opened:
                    if case .opened(_, let doc) = outcome { ControlWindows.openDocument(doc) }
                    sent += 1
                    continue
                }
            }
            let events = ControlUI.keyEvents(w, [s])
            ControlUI.deliver(events, to: w)
            sent += UInt32(events.count)
        }
        _ = try? server.reply(token: token, result: .input(window: Int64(w.windowNumber), events: sent, x: 0, y: 0))
    }

    /// What a wait reads now.
    static func facts(_ window: WindowRefRow?) -> WaitFactsRow {
        WaitFactsRow(
            document: ControlWindows.document(window).map { ControlStates.facts($0.model) },
            documents: UInt32(appStore.documents.count),
            windowTitles: ControlWindows.list().filter(\.visible).map(\.title))
    }
}

/// VizDAW's windows for the channel: what each shows, which one a call
/// means, and opening a document's window from outside SwiftUI.
@MainActor
enum ControlWindows {
    final class Tag {
        let kind: WindowKindRow
        let doc: UUID?
        init(_ kind: WindowKindRow, _ doc: UUID?) {
            self.kind = kind
            self.doc = doc
        }
    }

    private static let tags = NSMapTable<NSWindow, Tag>.weakToStrongObjects()
    /// SwiftUI's window opener, taken from the first document window.
    static var openWindow: OpenWindowAction?

    static func tag(_ w: NSWindow, _ kind: WindowKindRow, doc: UUID?) {
        tags.setObject(Tag(kind, doc), forKey: w)
    }

    static func tagOf(_ w: NSWindow) -> Tag {
        if let t = tags.object(forKey: w) { return t }
        if w.title == "Plugin Manager" { return Tag(.pluginManager, nil) }
        if w.identifier?.rawValue.contains("Settings") == true { return Tag(.settings, nil) }
        return Tag(.other, nil)
    }

    /// VizDAW's titled windows, front to back.
    static func windows() -> [NSWindow] {
        NSApp.windows
            .filter { $0.styleMask.contains(.titled) && ($0.isVisible || $0.isMiniaturized) }
            .sorted { $0.orderedIndex < $1.orderedIndex }
    }

    static func list() -> [WindowRow] {
        windows().map { w in
            let t = tagOf(w)
            return WindowRow(
                number: Int64(w.windowNumber), title: w.title, kind: t.kind, doc: t.doc?.uuidString,
                key: w.isKeyWindow, main: w.isMainWindow, visible: w.isVisible, x: w.frame.minX,
                y: w.frame.minY, width: w.frame.width, height: w.frame.height)
        }
    }

    /// The window `ref` names; none: the front document window.
    static func window(_ ref: WindowRefRow?) -> NSWindow? {
        let all = windows()
        switch ref {
        case nil: return all.first { tagOf($0).kind == .document } ?? all.first
        case .key: return all.first { $0.isKeyWindow }
        case .main: return all.first { $0.isMainWindow }
        case .number(let n): return all.first { $0.windowNumber == Int(n) }
        }
    }

    /// The document `ref`'s window shows; none: the front document
    /// window's, else any open document.
    static func document(_ ref: WindowRefRow?) -> (id: UUID, model: DocumentModel)? {
        let id: UUID? =
            if let ref { window(ref).flatMap { tagOf($0).doc } } else {
                window(nil).flatMap { tagOf($0).doc } ?? appStore.documents.keys.sorted { $0.uuidString < $1.uuidString }.first
            }
        return id.flatMap { i in appStore.model(i).map { (i, $0) } }
    }

    /// Document `doc`'s own window, once it is on screen.
    static func window(of doc: UUID) -> NSWindow? {
        windows().first { w in
            let t = tagOf(w)
            return t.kind == .document && t.doc == doc && w.isVisible
        }
    }

    static func openDocument(_ doc: UUID) {
        guard window(of: doc) == nil else { return }
        if let openWindow {
            openWindow(value: doc)
        } else {
            appStore.enqueue(doc)
        }
    }

    static func name(_ c: WaitConditionRow) -> String {
        switch c {
        case .playing: "playing"
        case .stopped: "stopped"
        case .tickAtLeast: "tick_at_least"
        case .documentsAtLeast: "documents_at_least"
        case .windowTitled: "window_titled"
        case .windowGone: "window_gone"
        case .detailKind: "detail_kind"
        }
    }
}

/// Marks the window a SwiftUI view is in (document and mixer windows).
struct WindowTag: NSViewRepresentable {
    let kind: WindowKindRow
    let doc: UUID?

    final class Marker: NSView {
        var kind: WindowKindRow = .other
        var doc: UUID?
        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            if let w = window { ControlWindows.tag(w, kind, doc: doc) }
        }
    }

    func makeNSView(context: Context) -> Marker {
        let m = Marker()
        m.kind = kind
        m.doc = doc
        return m
    }

    func updateNSView(_ m: Marker, context: Context) {
        m.kind = kind
        m.doc = doc
        if let w = m.window { ControlWindows.tag(w, kind, doc: doc) }
    }
}

/// The app's windows and settings for `ControlCommands`.
@MainActor
final class AppControlHost: ControlHost {
    static let shared = AppControlHost()

    func showMixer(_ doc: UUID) { ControlWindows.openWindow?(id: "mixer", value: doc) }
    func showPluginManager() { ControlWindows.openWindow?(id: "plugins") }
    func tearOff(_ doc: UUID, kind: DetailEditorKind) { EditorWindows.tearOff(appStore, doc: doc, kind: kind) }

    func closeDocument(_ doc: UUID) {
        if let w = ControlWindows.window(of: doc) {
            w.performClose(nil)
        } else {
            appStore.close(doc)
        }
    }

    var nudge: NudgeSettings { editingPreferences.nudge }
}
VIZDAW_EOF
```

- [ ] **Step 2: Wire it in**

In `app/Sources/VizDAW/VizDAWApp.swift`, Find:

```swift
        LaunchArguments.parseFlags(CommandLine.arguments)
        AppSetup.openCatalog()
        LaunchArguments.apply(CommandLine.arguments, to: appStore)
```

Replace:

```swift
        LaunchArguments.parseFlags(CommandLine.arguments)
        // The control channel (control spec §8) starts before any document
        // opens, so launch-time dialogs already go to it instead of the screen.
        if let path = ControlLaunch.socketPath(
            arguments: CommandLine.arguments, environment: ProcessInfo.processInfo.environment)
        {
            ControlChannel.start(path: path)
        }
        AppSetup.openCatalog()
        LaunchArguments.apply(ControlLaunch.strip(CommandLine.arguments), to: appStore)
```

Find:

```swift
        MainActor.assumeIsolated {
            appStore.closeAll()
```

Replace:

```swift
        MainActor.assumeIsolated {
            // Pending control calls are answered before the documents go.
            ControlChannel.stop()
            appStore.closeAll()
```

Find:

```swift
            guard let item = plugins.alert else { return }
            plugins.alert = nil
            let alert = NSAlert()
```

Replace:

```swift
            guard let item = plugins.alert else { return }
            plugins.alert = nil
            // Under the control channel no dialog opens (control spec §7).
            if ControlChannel.active {
                ControlChannel.note(item)
                return
            }
            let alert = NSAlert()
```

Find:

```swift
    func afterCrash(project: String, loading: [PluginRefRow], crashed: Bool) -> CrashChoice {
        let alert = NSAlert()
```

Replace:

```swift
    func afterCrash(project: String, loading: [PluginRefRow], crashed: Bool) -> CrashChoice {
        // Under the control channel no dialog opens; app.open answers this
        // with `plugins` (control spec §7).
        if ControlChannel.active {
            ControlChannel.note(AlertItem(
                title: "Open with Plugins Off?",
                message: "\(project) was not opened; open it with app.open and `plugins`."))
            return .cancel
        }
        let alert = NSAlert()
```

Find:

```swift
    func restoreAutosave(project: String) -> Bool {
        let alert = NSAlert()
```

Replace:

```swift
    func restoreAutosave(project: String) -> Bool {
        if ControlChannel.active {
            ControlChannel.note(AlertItem(
                title: "Restore Autosave",
                message: "\(project) opened its last save; app.open with `restore_autosave` chooses."))
            return false
        }
        let alert = NSAlert()
```

In `app/Sources/VizDAW/AppCommands.swift`, Find:

```swift
        while let item = store.takeAlert() {
            let alert = NSAlert()
```

Replace:

```swift
        while let item = store.takeAlert() {
            // Under the control channel no dialog opens (control spec §7).
            if ControlChannel.active {
                ControlChannel.note(item)
                continue
            }
            let alert = NSAlert()
```

In `app/Sources/VizDAW/DocumentWindow.swift`, Find:

```swift
                DocumentView(model: model, docID: id)
                    .focusedSceneValue(\.documentModel, model)
```

Replace:

```swift
                DocumentView(model: model, docID: id)
                    .background(WindowTag(kind: .document, doc: id))
                    .focusedSceneValue(\.documentModel, model)
```

Find:

```swift
        .onAppear {
            adopt()
            openQueued()
        }
```

Replace:

```swift
        .onAppear {
            // The control channel opens document windows with it.
            ControlWindows.openWindow = openWindow
            adopt()
            openQueued()
        }
```

In `app/Sources/VizDAW/MixerView.swift`, Find:

```swift
    var body: some View {
        content
            .onChange(of: KeyDoc(id: id, key: activeState == .key), initial: true) { old, new in
```

Replace:

```swift
    var body: some View {
        content
            .background(WindowTag(kind: .mixer, doc: id))
            .onChange(of: KeyDoc(id: id, key: activeState == .key), initial: true) { old, new in
```

In `app/Sources/VizDAW/EditorWindows.swift`, Find:

```swift
        win.title = "\(w.detail.kind.title) — \(model.snapshot.title)"
        super.init(window: win)
```

Replace:

```swift
        win.title = "\(w.detail.kind.title) — \(model.snapshot.title)"
        super.init(window: win)
        ControlWindows.tag(win, .editor, doc: doc)
```

In `app/Sources/VizDAW/PluginWindows.swift`, Find:

```swift
        window.isReleasedWhenClosed = false
        super.init(window: window)
        window.delegate = self
```

Replace:

```swift
        window.isReleasedWhenClosed = false
        super.init(window: window)
        ControlWindows.tag(window, .plugin, doc: docID)
        window.delegate = self
```

In `app/Sources/VizDAW/ModifyPanel.swift`, Find `        super.init(window: panel)` Replace:

```swift
        super.init(window: panel)
        ControlWindows.tag(panel, .modifyPanel, doc: doc)
```

- [ ] **Step 3: Build and smoke-test through the socket**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh build 2>&1 | grep -E "error|warning: " | grep -v build-system
BIN="$(scripts/swift.sh build --show-bin-path 2>/dev/null | tail -1)/VizDAW"
rm -rf /tmp/vzcc && mkdir -m 700 /tmp/vzcc
HOME=/tmp/vzcc VIZDAW_SUPPORT_DIR=/tmp/vzcc/support VIZDAW_AUDIO=offline "$BIN" --control /tmp/vzcc/ctl/s.sock --no-scan --demo >/tmp/vzcc/log 2>&1 &
for i in $(seq 1 600); do [ -S /tmp/vzcc/ctl/s.sock ] && break; sleep 0.2; done
ls -ld /tmp/vzcc/ctl; ls -l /tmp/vzcc/ctl/s.sock
printf '%s\n' '{"id":1,"cmd":"ping"}' '{"id":2,"cmd":"wait","params":{"condition":"documents_at_least","count":1,"timeout_ms":60000}}' '{"id":3,"cmd":"command.run","params":{"id":"transport.play_stop"}}' '{"id":4,"cmd":"wait","params":{"condition":"playing","timeout_ms":60000}}' '{"id":5,"cmd":"command.run","params":{"id":"file.save"}}' '{"id":6,"cmd":"app.quit"}' | nc -U -w 90 /tmp/vzcc/ctl/s.sock
sleep 2; ls /tmp/vzcc/ctl
```

(This step's polling loops wait for the socket; the `sleep 2` only lets the process finish exiting before the last `ls`.)

Expected: no build errors or new warnings; `drwx------` and `srw-------`; six reply lines: `{"id":1,"ok":true,"result":{"pong":true}}`, a `waited` result, a `ran` result for `transport.play_stop`, `{"condition":"playing",…}`, an error with `"code":"dialog_required"` and `"dialog":"Save"`, `{"quitting":true}`; the final `ls` shows an empty folder (the socket is gone).

- [ ] **Step 4: Gate and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh build 2>&1 | tail -3 && scripts/swift.sh test 2>&1 | tail -3
git add app/Sources/VizDAW/ControlChannel.swift app/Sources/VizDAW/VizDAWApp.swift app/Sources/VizDAW/AppCommands.swift app/Sources/VizDAW/DocumentWindow.swift app/Sources/VizDAW/MixerView.swift app/Sources/VizDAW/EditorWindows.swift app/Sources/VizDAW/PluginWindows.swift app/Sources/VizDAW/ModifyPanel.swift
git commit -m "feat(app): agent control channel (--control / VIZDAW_CONTROL)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
```

Expected: Swift 568 tests pass.

---

### Task 10: `vizctl`

**Files:**
- Create: `crates/vizctl/Cargo.toml`, `crates/vizctl/src/lib.rs`, `src/main.rs`, `src/cli.rs`, `src/launch.rs`
- Test: `crates/vizctl/tests/cli.rs`

**Interfaces:**
- Consumes: `core_control::{client::{Client, CallError}, server::{Server, Config, MAX_SOCKET_PATH}, Command, ControlError, ErrorCode}`.
- Produces: `vizctl::cli::{USAGE, Request { cmd, params, timeout, find }, Plan::{Help, Launch(LaunchOptions), Send { socket, request }}, DEFAULT_TIMEOUT = 120 s, parse(&[String], Option<String>) -> Result<Plan, String>, find_in_tree(&Value, &str) -> Value, run(&[String]) -> i32}`; `vizctl::launch::{LaunchOptions { app, home, demo, timeout, env }, Launched { child, socket, home, log } (to_json), executable(&Path) -> PathBuf, log_tail(&Path, usize) -> String, launch(&LaunchOptions) -> Result<Launched, String>}`; the `vizctl` binary (exit 0 done, 1 error reply, 2 usage, 3 no connection).

Spec §10, §12.

- [ ] **Step 1: The crate and its failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p crates/vizctl/src crates/vizctl/tests
cat > crates/vizctl/Cargo.toml <<'VIZDAW_EOF'
[package]
name = "vizctl"
version = "0.1.0"
edition.workspace = true
license.workspace = true
rust-version.workspace = true
publish = false

[dependencies]
core-control = { path = "../core-control" }
serde_json = "1"

[dev-dependencies]
tempfile = "3"
VIZDAW_EOF
cat > crates/vizctl/src/lib.rs <<'VIZDAW_EOF'
//! `vizctl`: drives VizDAW through its control channel from a shell
//! (control spec §4, §10). [`cli`] turns arguments into requests;
//! [`launch`] starts VizDAW with the channel on in a throwaway home.
#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic
    )
)]

pub mod cli;
pub mod launch;
VIZDAW_EOF
cat > crates/vizctl/src/main.rs <<'VIZDAW_EOF'
//! The `vizctl` command (see `vizctl help`).

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(vizctl::cli::run(&args));
}
VIZDAW_EOF
cat > crates/vizctl/tests/cli.rs <<'VIZDAW_EOF'
//! `vizctl` arguments, exit codes and output against a fake VizDAW (a
//! `core-control` server answered by a test thread).

use core_control::Command;
use core_control::server::{Config, Server};
use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt;
use std::process::Command as Process;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use vizctl::cli::{Plan, find_in_tree, parse};
use vizctl::launch::{LaunchOptions, launch};

fn args(s: &str) -> Vec<String> {
    s.split_whitespace().map(str::to_string).collect()
}

fn request(s: &str) -> (String, Value, Duration) {
    match parse(&args(s), Some("/tmp/x.sock".into())).unwrap() {
        Plan::Send { request, .. } => (request.cmd, request.params, request.timeout),
        other => panic!("{other:?}"),
    }
}

#[test]
fn arguments_become_requests() {
    assert_eq!(
        request("click 10 20 --right --count 2 --window 5").1,
        json!({"point": {"x": 10.0, "y": 20.0}, "button": "right", "count": 2, "window": 5})
    );
    assert_eq!(
        request("drag 0.1:0000abcd 30,40 --steps 4 --mod option").1,
        json!({"from": "0.1:0000abcd", "to": {"x": 30.0, "y": 40.0}, "steps": 4, "modifiers": ["option"]})
    );
    assert_eq!(
        request("key s --mod command,shift").1,
        json!({"key": "s", "modifiers": ["command", "shift"]})
    );
    assert_eq!(
        request("type Hello there").1,
        json!({"text": "Hello there"})
    );
    assert_eq!(
        request(r#"run file.save_as {"path":"/tmp/a.vizdaw"} --window 3"#).1,
        json!({"id": "file.save_as", "params": {"path": "/tmp/a.vizdaw"}, "window": 3})
    );
    let (cmd, p, timeout) = request("wait tick_at_least --tick 100 --timeout-ms 90000");
    assert_eq!(cmd, "wait");
    assert_eq!(
        p,
        json!({"condition": "tick_at_least", "tick": 100, "timeout_ms": 90000})
    );
    assert!(
        timeout >= Duration::from_secs(120),
        "room for the wait's own timeout"
    );
    assert_eq!(
        request("new-demo --audio --window 2").1,
        json!({"audio": true}),
        "no window for app.*"
    );
    assert_eq!(request("state windows").0, "state.get");
    assert!(matches!(parse(&args("help"), None).unwrap(), Plan::Help));
    assert!(
        parse(&args("state transport"), None)
            .unwrap_err()
            .contains("VIZDAW_CONTROL")
    );
    assert!(parse(&args("frobnicate"), Some("/tmp/x".into())).is_err());
    assert!(
        parse(&args("click 1"), Some("/tmp/x".into())).is_ok(),
        "one argument is an element ID"
    );
    assert!(parse(&args("click 1 2 3"), Some("/tmp/x".into())).is_err());
    match parse(
        &args("launch --app /x/VizDAW.app --env VIZDAW_AUDIO=offline --demo"),
        None,
    )
    .unwrap()
    {
        Plan::Launch(o) => {
            assert!(o.demo);
            assert_eq!(
                o.env,
                vec![("VIZDAW_AUDIO".to_string(), "offline".to_string())]
            );
            assert_eq!(
                vizctl::launch::executable(&o.app).to_str().unwrap(),
                "/x/VizDAW.app/Contents/MacOS/VizDAW"
            );
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn find_flattens_matching_tree_nodes() {
    let tree = json!({"window": 3, "nodes": [
        {"id": "0:00000001", "role": "AXGroup", "label": "Arrangement", "children": [
            {"id": "0.0:00000002", "role": "AXButton", "label": "Block Chords", "frame": {"x": 1}},
        ]},
        {"id": "1:00000003", "role": "AXButton", "title": "Play"},
    ]});
    let found = find_in_tree(&tree, "Chords");
    assert_eq!(found["matches"].as_array().unwrap().len(), 1);
    assert_eq!(found["matches"][0]["id"], "0.0:00000002");
    assert_eq!(
        find_in_tree(&tree, "Play")["matches"][0]["role"],
        "AXButton"
    );
}

/// A fake VizDAW on a socket in a private folder.
struct Fake {
    _dir: tempfile::TempDir,
    socket: std::path::PathBuf,
    stop: Arc<AtomicBool>,
    app: Option<thread::JoinHandle<()>>,
}

impl Fake {
    fn new() -> Fake {
        let dir = tempfile::Builder::new()
            .prefix("vzt")
            .tempdir_in("/tmp")
            .unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let socket = dir.path().join("s.sock");
        let server = Arc::new(Mutex::new(
            Server::start(&socket, Config::default()).unwrap(),
        ));
        let stop = Arc::new(AtomicBool::new(false));
        let (s, st) = (Arc::clone(&server), Arc::clone(&stop));
        let app = thread::spawn(move || {
            while !st.load(Ordering::SeqCst) {
                let call = s.lock().unwrap().next_call();
                let Some(c) = call else {
                    thread::sleep(Duration::from_millis(1));
                    continue;
                };
                let outcome = match c.command {
                    Command::StateGet { .. } => Ok(json!({"playing": true, "tick": 7})),
                    _ => Err(core_control::ControlError::new(
                        core_control::ErrorCode::Unavailable,
                        "Nothing to undo",
                    )),
                };
                s.lock().unwrap().reply(c.token, outcome);
            }
        });
        Fake {
            _dir: dir,
            socket,
            stop,
            app: Some(app),
        }
    }

    fn vizctl(&self, a: &str) -> (i32, String) {
        let out = Process::new(env!("CARGO_BIN_EXE_vizctl"))
            .arg("--socket")
            .arg(&self.socket)
            .args(args(a))
            .output()
            .unwrap();
        (
            out.status.code().unwrap(),
            String::from_utf8(out.stdout).unwrap(),
        )
    }
}

impl Drop for Fake {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(a) = self.app.take() {
            let _ = a.join();
        }
    }
}

#[test]
fn the_binary_prints_results_and_exits_by_outcome() {
    let f = Fake::new();
    let (code, out) = f.vizctl("state transport");
    assert_eq!(code, 0);
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["tick"], 7);
    let (code, out) = f.vizctl("run edit.undo");
    assert_eq!(code, 1, "VizDAW answered with an error");
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["error"]["code"], "unavailable");
    let (code, out) = f.vizctl("commands");
    assert_eq!(code, 0);
    assert!(out.contains("transport.play_stop"));
    assert_eq!(f.vizctl("frobnicate").0, 2, "bad usage");
    let gone = Process::new(env!("CARGO_BIN_EXE_vizctl"))
        .args(["--socket", "/tmp/no-such-vizdaw.sock", "ping"])
        .output()
        .unwrap();
    assert_eq!(gone.status.code(), Some(3), "no connection");
}

#[test]
fn launch_reports_an_app_that_exits_early() {
    let home = tempfile::Builder::new()
        .prefix("vzl")
        .tempdir_in("/tmp")
        .unwrap();
    let e = launch(&LaunchOptions {
        app: "/usr/bin/true".into(),
        home: Some(home.path().join("h")),
        demo: false,
        timeout: Duration::from_secs(60),
        env: vec![],
    })
    .unwrap_err();
    assert!(e.contains("exited"), "{e}");
    let mode = std::fs::metadata(home.path().join("h/ctl"))
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o700, "the socket folder is private");
    assert!(
        launch(&LaunchOptions {
            app: "/no/such/VizDAW.app".into(),
            home: None,
            demo: false,
            timeout: Duration::from_secs(1),
            env: vec![],
        })
        .unwrap_err()
        .contains("no VizDAW executable")
    );
}
VIZDAW_EOF
cargo test -p vizctl 2>&1 | grep -E "^error" | head -3
```

Expected: does not compile (`file not found for module cli`).

- [ ] **Step 2: Launching**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/vizctl/src/launch.rs <<'VIZDAW_EOF'
//! `vizctl launch`: starts VizDAW with the control channel on, in a
//! throwaway home (control spec §12): its own `HOME`, support folder
//! (`VIZDAW_SUPPORT_DIR`), private socket directory and log, no plugin
//! scan. Waits until the channel answers.

use core_control::client::Client;
use core_control::server::MAX_SOCKET_PATH;
use serde_json::json;
use std::fs;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// What to launch and where.
#[derive(Debug, Clone)]
pub struct LaunchOptions {
    /// `VizDAW.app` or its executable.
    pub app: PathBuf,
    /// The throwaway home (made if missing); None: a new one in `/tmp`.
    pub home: Option<PathBuf>,
    /// Open the demo project at launch.
    pub demo: bool,
    /// How long to wait for the channel (slow CI: be generous).
    pub timeout: Duration,
    /// Extra environment (`VIZDAW_AUDIO=offline` on CI).
    pub env: Vec<(String, String)>,
}

/// A running VizDAW.
#[derive(Debug)]
pub struct Launched {
    /// Its process.
    pub child: Child,
    /// Its control socket.
    pub socket: PathBuf,
    /// Its throwaway home.
    pub home: PathBuf,
    /// Its stdout and stderr.
    pub log: PathBuf,
}

impl Launched {
    /// What `vizctl launch` prints.
    pub fn to_json(&self) -> serde_json::Value {
        json!({
            "pid": self.child.id(),
            "socket": self.socket,
            "home": self.home,
            "log": self.log,
        })
    }
}

/// The executable inside `app` when it is a bundle, else `app`.
pub fn executable(app: &Path) -> PathBuf {
    if app.extension().is_some_and(|e| e == "app") {
        app.join("Contents/MacOS/VizDAW")
    } else {
        app.to_path_buf()
    }
}

fn private_dir(p: &Path) -> Result<(), String> {
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(p)
        .map_err(|e| format!("cannot create {}: {e}", p.display()))
}

/// The last `n` lines of the log, for errors.
pub fn log_tail(log: &Path, n: usize) -> String {
    let text = fs::read_to_string(log).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    lines
        .get(lines.len().saturating_sub(n)..)
        .unwrap_or(&[])
        .join("\n")
}

/// Launches VizDAW and waits until its channel answers `ping`.
pub fn launch(o: &LaunchOptions) -> Result<Launched, String> {
    let exe = executable(&o.app);
    if !exe.is_file() {
        return Err(format!(
            "no VizDAW executable at {} (build it with scripts/make-app.sh, or pass --app)",
            exe.display()
        ));
    }
    let home = match &o.home {
        Some(h) => h.clone(),
        None => {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.subsec_nanos())
                .unwrap_or(0);
            PathBuf::from(format!("/tmp/vizctl-{}-{nanos}", std::process::id()))
        }
    };
    private_dir(&home)?;
    let ctl = home.join("ctl");
    private_dir(&ctl)?;
    let support = home.join("support");
    private_dir(&support)?;
    let socket = ctl.join("vizdaw.sock");
    if socket.as_os_str().len() > MAX_SOCKET_PATH {
        return Err(format!(
            "{} is longer than {MAX_SOCKET_PATH} bytes; pass a shorter --home",
            socket.display()
        ));
    }
    let log = home.join("vizdaw.log");
    let out =
        fs::File::create(&log).map_err(|e| format!("cannot create {}: {e}", log.display()))?;
    let err = out
        .try_clone()
        .map_err(|e| format!("cannot open {}: {e}", log.display()))?;
    let mut cmd = Command::new(&exe);
    cmd.arg("--control")
        .arg(&socket)
        .arg("--no-scan")
        .env("HOME", &home)
        .env("VIZDAW_SUPPORT_DIR", &support)
        .env_remove("VIZDAW_CONTROL")
        .stdin(Stdio::null())
        .stdout(out)
        .stderr(err);
    if o.demo {
        cmd.arg("--demo");
    }
    for (k, v) in &o.env {
        cmd.env(k, v);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("cannot start {}: {e}", exe.display()))?;
    let deadline = Instant::now() + o.timeout;
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            return Err(format!(
                "VizDAW exited ({status}) before its control channel answered; log:\n{}",
                log_tail(&log, 20)
            ));
        }
        let ready = Client::connect(&socket)
            .ok()
            .and_then(|mut c| c.call("ping", json!({}), Duration::from_secs(5)).ok());
        if ready.is_some() {
            return Ok(Launched {
                child,
                socket,
                home,
                log,
            });
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            return Err(format!(
                "VizDAW's control channel did not answer within {} s; log:\n{}",
                o.timeout.as_secs(),
                log_tail(&log, 20)
            ));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}
VIZDAW_EOF
```

- [ ] **Step 3: Arguments and output**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/vizctl/src/cli.rs <<'VIZDAW_EOF'
//! Arguments to requests, and the exit codes: 0 done, 1 VizDAW answered
//! with an error, 2 bad usage, 3 no connection (control spec §10).

use crate::launch::{LaunchOptions, launch};
use core_control::client::{CallError, Client};
use serde_json::{Map, Value, json};
use std::path::PathBuf;
use std::time::Duration;

/// `vizctl help`.
pub const USAGE: &str = "\
usage: vizctl [--socket PATH] [--timeout SECONDS] COMMAND [ARGS]

The socket defaults to $VIZDAW_CONTROL. Results print as JSON on stdout.

  launch [--app PATH] [--home DIR] [--demo] [--env K=V]...
                               start VizDAW in a throwaway home; prints pid, socket, home, log
  ping                         check the channel
  commands                     the menu-command registry
  run ID [PARAMS-JSON]         run a menu command (e.g. run transport.play_stop)
  state TARGET                 transport | document | editor | selection | windows
  tree [--depth N]             the window's accessibility tree with element IDs
  find TEXT                    tree elements whose label, title or value contains TEXT
  click ID | click X Y         click an element or a point (window points, top-left origin)
        [--right] [--count N] [--mod shift,control,option,command]
  drag FROM TO [--steps N] [--mod ...]   FROM and TO: an element ID or X,Y
  key KEY [--mod ...]          one key: space, return, escape, left, a, 1, ...
  type TEXT                    type characters
  screenshot [--out PATH] [--scale S]
  wait CONDITION [--tick N | --count N | --title T | --kind K] [--timeout-ms N]
                               playing | stopped | tick_at_least | documents_at_least |
                               window_titled | window_gone | detail_kind
  new-demo [--audio]           the demo project in a new window
  open PATH [--plugins normal|off|disable] [--restore-autosave true|false]
  quit                         quit VizDAW
  call CMD [PARAMS-JSON]       send any request

Every command except launch takes --window N (a number from `state windows`).
Exit codes: 0 done, 1 VizDAW answered with an error, 2 bad usage, 3 no connection.";

/// A request to send.
#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    /// Command name.
    pub cmd: String,
    /// Its params.
    pub params: Value,
    /// Client-side reply timeout.
    pub timeout: Duration,
    /// `find`: filter the tree for this text.
    pub find: Option<String>,
}

/// What the arguments ask for.
#[derive(Debug, Clone)]
pub enum Plan {
    /// Print usage.
    Help,
    /// Launch VizDAW.
    Launch(LaunchOptions),
    /// Send a request to `socket`.
    Send {
        /// The socket.
        socket: PathBuf,
        /// The request.
        request: Request,
    },
}

/// The default client timeout: longer than the server's 60 s app timeout.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);

struct Args {
    items: Vec<String>,
}

impl Args {
    /// Removes `--name VALUE`; Err when the value is missing.
    fn value(&mut self, name: &str) -> Result<Option<String>, String> {
        let Some(i) = self.items.iter().position(|a| a == name) else {
            return Ok(None);
        };
        if i + 1 >= self.items.len() {
            return Err(format!("{name} needs a value"));
        }
        let v = self.items.remove(i + 1);
        self.items.remove(i);
        Ok(Some(v))
    }

    /// Removes every `--name VALUE`.
    fn values(&mut self, name: &str) -> Result<Vec<String>, String> {
        let mut out = Vec::new();
        while let Some(v) = self.value(name)? {
            out.push(v);
        }
        Ok(out)
    }

    /// Removes `--name`; whether it was there.
    fn flag(&mut self, name: &str) -> bool {
        match self.items.iter().position(|a| a == name) {
            Some(i) => {
                self.items.remove(i);
                true
            }
            None => false,
        }
    }

    fn positional(&self) -> &[String] {
        &self.items
    }
}

fn number(s: &str, what: &str) -> Result<i64, String> {
    s.parse::<i64>()
        .map_err(|_| format!("{what} must be an integer, not {s:?}"))
}

fn float(s: &str, what: &str) -> Result<f64, String> {
    s.parse::<f64>()
        .map_err(|_| format!("{what} must be a number, not {s:?}"))
}

fn json_arg(s: Option<&String>) -> Result<Value, String> {
    match s {
        None => Ok(json!({})),
        Some(t) => serde_json::from_str(t).map_err(|e| format!("not JSON: {e}")),
    }
}

/// `X,Y` as a point, anything else as an element ID.
fn target(s: &str) -> Result<Value, String> {
    match s.split_once(',') {
        Some((x, y)) if !s.contains(':') => Ok(json!({"x": float(x, "x")?, "y": float(y, "y")?})),
        _ => Ok(json!(s)),
    }
}

/// Sets `key` in the object `p`.
fn set(p: &mut Value, key: &str, v: Value) {
    if let Some(o) = p.as_object_mut() {
        o.insert(key.into(), v);
    }
}

fn modifiers(a: &mut Args) -> Result<Option<Value>, String> {
    Ok(a.value("--mod")?
        .map(|m| json!(m.split(',').map(str::trim).collect::<Vec<_>>())))
}

/// Reads the arguments (without the program name).
pub fn parse(args: &[String], env_socket: Option<String>) -> Result<Plan, String> {
    let mut a = Args {
        items: args.to_vec(),
    };
    let socket = a.value("--socket")?.or(env_socket);
    let timeout = match a.value("--timeout")? {
        Some(t) => Duration::from_secs_f64(float(&t, "--timeout")?.clamp(1.0, 3600.0)),
        None => DEFAULT_TIMEOUT,
    };
    let window = match a.value("--window")? {
        Some(w) if w == "key" || w == "main" => Some(json!(w)),
        Some(w) => Some(json!(number(&w, "--window")?)),
        None => None,
    };
    let Some(cmd) = a.items.first().cloned() else {
        return Ok(Plan::Help);
    };
    a.items.remove(0);
    if cmd == "help" || cmd == "--help" || cmd == "-h" {
        return Ok(Plan::Help);
    }
    if cmd == "launch" {
        let env = a
            .values("--env")?
            .into_iter()
            .map(|kv| {
                kv.split_once('=')
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .ok_or(format!("--env takes K=V, not {kv:?}"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let app = a
            .value("--app")?
            .or_else(|| std::env::var("VIZDAW_APP").ok())
            .unwrap_or_else(|| "dist/VizDAW.app".into());
        let home = a.value("--home")?.map(PathBuf::from);
        let demo = a.flag("--demo");
        if let Some(extra) = a.positional().first() {
            return Err(format!("launch: unexpected {extra:?}"));
        }
        return Ok(Plan::Launch(LaunchOptions {
            app: PathBuf::from(app),
            home,
            demo,
            timeout: timeout.max(Duration::from_secs(120)),
            env,
        }));
    }
    let mut find = None;
    let mut wait_ms = None;
    let (name, mut params): (String, Value) = match cmd.as_str() {
        "ping" => ("ping".to_string(), json!({})),
        "commands" => ("commands.list".to_string(), json!({})),
        "call" => {
            let c = a
                .positional()
                .first()
                .cloned()
                .ok_or("call: give a command")?;
            let p = json_arg(a.positional().get(1))?;
            (c, p)
        }
        "run" => {
            let id = a
                .positional()
                .first()
                .cloned()
                .ok_or("run: give a command ID")?;
            let p = json_arg(a.positional().get(1))?;
            ("command.run".to_string(), json!({"id": id, "params": p}))
        }
        "state" => {
            let t = a
                .positional()
                .first()
                .cloned()
                .ok_or("state: give a target")?;
            ("state.get".to_string(), json!({"target": t}))
        }
        "tree" | "find" => {
            let mut p = json!({});
            if let Some(d) = a.value("--depth")? {
                set(&mut p, "max_depth", json!(number(&d, "--depth")?));
            }
            if cmd == "find" {
                find = Some(
                    a.positional()
                        .first()
                        .cloned()
                        .ok_or("find: give some text")?,
                );
            }
            ("ui.tree".to_string(), p)
        }
        "click" => {
            let right = a.flag("--right");
            let count = a.value("--count")?;
            let m = modifiers(&mut a)?;
            let mut p = match a.positional() {
                [id] => json!({"element": id}),
                [x, y] => json!({"point": {"x": float(x, "x")?, "y": float(y, "y")?}}),
                _ => return Err("click: give an element ID or X Y".into()),
            };
            if right {
                set(&mut p, "button", json!("right"));
            }
            if let Some(c) = count {
                set(&mut p, "count", json!(number(&c, "--count")?));
            }
            if let Some(m) = m {
                set(&mut p, "modifiers", m);
            }
            ("ui.click".to_string(), p)
        }
        "drag" => {
            let steps = a.value("--steps")?;
            let m = modifiers(&mut a)?;
            let [from, to] = a.positional() else {
                return Err("drag: give FROM and TO".into());
            };
            let mut p = json!({"from": target(from)?, "to": target(to)?});
            if let Some(s) = steps {
                set(&mut p, "steps", json!(number(&s, "--steps")?));
            }
            if let Some(m) = m {
                set(&mut p, "modifiers", m);
            }
            ("ui.drag".to_string(), p)
        }
        "key" => {
            let m = modifiers(&mut a)?;
            let k = a.positional().first().cloned().ok_or("key: give a key")?;
            let mut p = json!({"key": k});
            if let Some(m) = m {
                set(&mut p, "modifiers", m);
            }
            ("ui.key".to_string(), p)
        }
        "type" => {
            let t = a.positional().join(" ");
            if t.is_empty() {
                return Err("type: give some text".into());
            }
            ("ui.key".to_string(), json!({"text": t}))
        }
        "screenshot" => {
            let mut p = json!({});
            if let Some(o) = a.value("--out")? {
                let path = std::path::absolute(&o).map_err(|e| format!("--out: {e}"))?;
                set(&mut p, "path", json!(path));
            }
            if let Some(s) = a.value("--scale")? {
                set(&mut p, "scale", json!(float(&s, "--scale")?));
            }
            ("ui.screenshot".to_string(), p)
        }
        "wait" => {
            let c = a
                .positional()
                .first()
                .cloned()
                .ok_or("wait: give a condition")?;
            let mut p = json!({"condition": c});
            for (flag, key, num) in [
                ("--tick", "tick", true),
                ("--count", "count", true),
                ("--title", "title", false),
                ("--kind", "kind", false),
            ] {
                if let Some(v) = a.value(flag)? {
                    let v = if num {
                        json!(number(&v, flag)?)
                    } else {
                        json!(v)
                    };
                    set(&mut p, key, v);
                }
            }
            if let Some(t) = a.value("--timeout-ms")? {
                let ms = number(&t, "--timeout-ms")?;
                wait_ms = Some(ms.max(0) as u64);
                set(&mut p, "timeout_ms", json!(ms));
            }
            ("wait".to_string(), p)
        }
        "new-demo" => (
            "app.new_demo".to_string(),
            json!({"audio": a.flag("--audio")}),
        ),
        "open" => {
            let plugins = a.value("--plugins")?;
            let restore = a.value("--restore-autosave")?;
            let path = a
                .positional()
                .first()
                .cloned()
                .ok_or("open: give a .vizdaw path")?;
            let path = std::path::absolute(&path).map_err(|e| format!("open: {e}"))?;
            let mut p = json!({"path": path});
            if let Some(pl) = plugins {
                set(&mut p, "plugins", json!(pl));
            }
            if let Some(r) = restore {
                set(&mut p, "restore_autosave", json!(r == "true"));
            }
            ("app.open".to_string(), p)
        }
        "quit" => ("app.quit".to_string(), json!({})),
        other => return Err(format!("unknown command {other:?} (see vizctl help)")),
    };
    if let (Some(w), Some(o)) = (window, params.as_object_mut()) {
        if !matches!(
            name.as_str(),
            "commands.list" | "ping" | "app.new_demo" | "app.open" | "app.quit"
        ) {
            o.insert("window".into(), w);
        }
    }
    let socket = socket
        .map(PathBuf::from)
        .ok_or("no socket: pass --socket or set VIZDAW_CONTROL (vizctl launch prints it)")?;
    // A wait answers within its own timeout; leave room for the reply.
    let timeout = match wait_ms {
        Some(ms) => timeout.max(Duration::from_millis(ms) + Duration::from_secs(30)),
        None if name == "wait" => timeout.max(Duration::from_secs(60)),
        None => timeout,
    };
    Ok(Plan::Send {
        socket,
        request: Request {
            cmd: name,
            params,
            timeout,
            find,
        },
    })
}

/// Tree nodes whose label, title or value contains `text`, flattened.
pub fn find_in_tree(tree: &Value, text: &str) -> Value {
    fn walk(nodes: &Value, text: &str, out: &mut Vec<Value>) {
        for n in nodes.as_array().into_iter().flatten() {
            let hit = ["label", "title", "value"].iter().any(|k| {
                n.get(*k)
                    .and_then(Value::as_str)
                    .is_some_and(|s| s.contains(text))
            });
            if hit {
                let mut m = Map::new();
                for k in ["id", "role", "label", "title", "value", "frame"] {
                    if let Some(v) = n.get(k) {
                        m.insert(k.into(), v.clone());
                    }
                }
                out.push(Value::Object(m));
            }
            if let Some(c) = n.get("children") {
                walk(c, text, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(tree.get("nodes").unwrap_or(&Value::Null), text, &mut out);
    json!({ "window": tree.get("window"), "matches": out })
}

/// Runs `vizctl` with `args`; returns the exit code.
pub fn run(args: &[String]) -> i32 {
    let plan = match parse(args, std::env::var("VIZDAW_CONTROL").ok()) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("vizctl: {e}");
            return 2;
        }
    };
    match plan {
        Plan::Help => {
            println!("{USAGE}");
            0
        }
        Plan::Launch(o) => match launch(&o) {
            Ok(l) => {
                println!("{}", l.to_json());
                eprintln!("export VIZDAW_CONTROL={}", l.socket.display());
                0
            }
            Err(e) => {
                eprintln!("vizctl: {e}");
                3
            }
        },
        Plan::Send { socket, request } => {
            let mut client = match Client::connect(&socket) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("vizctl: cannot connect to {}: {e}", socket.display());
                    return 3;
                }
            };
            match client.call(&request.cmd, request.params, request.timeout) {
                Ok(v) => {
                    let v = match &request.find {
                        Some(t) => find_in_tree(&v, t),
                        None => v,
                    };
                    println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
                    0
                }
                Err(CallError::Reply(e)) => {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&json!({ "error": e.to_json() }))
                            .unwrap_or_default()
                    );
                    eprintln!("vizctl: {e}");
                    1
                }
                Err(e) => {
                    eprintln!("vizctl: {e}");
                    3
                }
            }
        }
    }
}
VIZDAW_EOF
```

- [ ] **Step 4: Run the tests and try it**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p vizctl 2>&1 | grep -E "^test |test result"
BIN="$(scripts/swift.sh build --show-bin-path 2>/dev/null | tail -1)/VizDAW"
V=target/debug/vizctl
$V launch --app "$BIN" --home /tmp/vzct --env VIZDAW_AUDIO=offline --demo
export VIZDAW_CONTROL=/tmp/vzct/ctl/vizdaw.sock
$V wait documents_at_least --count 1 && $V find Arrangement && $V find Play
$V run transport.play_stop && $V wait playing && $V screenshot --out /tmp/vzct/play.png
$V key space && $V wait stopped && $V quit
```

Expected: 4 tests pass; `launch` prints `{"home":"/tmp/vzct",…,"pid":…,"socket":"/tmp/vzct/ctl/vizdaw.sock"}`; `find Arrangement` lists an `AXGroup` labelled "Arrangement"; `find Play` lists the toolbar's `AXButton` "Play"; the waits answer `{"condition":…,"elapsed_ms":…}`; open `/tmp/vzct/play.png` with the Read tool: Groove's purple reference block, the Chords block and the red playhead are in it (the sidebar may be blank: vibrancy, spec §9); `quit` answers `{"quitting":true}`.

- [ ] **Step 5: Gate and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace 2>&1 | grep -E "^test result|FAILED|panicked" | tail -60
git add crates/vizctl Cargo.lock
git commit -m "feat(vizctl): drive VizDAW's control channel from a shell

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
```

Expected: 804 Rust tests listed.

---

### Task 11: Damaged project files never crash (and the limits that needs); the end-to-end test on CI

**Files:**
- Modify: `crates/core-model/src/rules.rs`
- Test: `crates/core-model/tests/query_flatten.rs`, `crates/core-ffi/tests/blocks.rs`
- Create: `crates/core-ffi/tests/damaged_files.rs`, `crates/vizctl/tests/e2e.rs`
- Modify: `crates/core-ffi/Cargo.toml`, `.github/workflows/ci.yml`

**Interfaces:**
- Consumes: `core_model::file::from_json`, `core_model::rules::{check_event_fields, validate}`, `Session::{from_document, open, snapshot, lanes, lane_events, sequence_info, seconds_at, blocks, selection, mixer, destinations, modify_templates, close}`, `BlockRule`; `vizctl::launch`, `vizctl::cli::find_in_tree`, `core_control::client`.
- Produces: `core_model::rules::{MAX_TICKS: i64 = 1 << 40, MAX_LOOPS: u32 = 1 << 20, check_ticks(field, t) -> Result<()>}`; the harness (`VIZDAW_FUZZ_CASES` scales it); the e2e tests `demo_play_screenshot_stop`, `click_play_and_press_space`; the CI step.

Spec §11; Rulings 17–19.

- [ ] **Step 1: Write the failing tests**

In `crates/core-ffi/Cargo.toml`, Find:

```toml
[dev-dependencies]
core-ffi = { path = ".", features = ["test-hooks"] }
```

Replace:

```toml
[dev-dependencies]
proptest = "1"
core-ffi = { path = ".", features = ["test-hooks"] }
```

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-ffi/tests/damaged_files.rs <<'VIZDAW_EOF'
//! Damaged project files never crash VizDAW (control spec §11): random
//! byte damage and random JSON edits of the demo project either load and
//! answer every read the editors make, or are refused with an error.
//! Never a panic, never `VizError::Internal` (a caught panic).

use proptest::prelude::*;
use serde_json::{Value, json};
use vizdaw_ffi::{BlockRule, Session, VizError};

/// Cases per property: `VIZDAW_FUZZ_CASES` (a long local run), else `n`.
fn cases(n: u32) -> u32 {
    std::env::var("VIZDAW_FUZZ_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(n)
}

fn demo_text() -> String {
    let p = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/demo.vizdaw/project.json"
    );
    std::fs::read_to_string(p).unwrap()
}

fn no_bug<T>(what: &str, r: Result<T, VizError>) -> Result<Option<T>, TestCaseError> {
    match r {
        Ok(v) => Ok(Some(v)),
        Err(VizError::Internal { message }) => Err(TestCaseError::fail(format!(
            "{what}: internal error {message}"
        ))),
        Err(_) => Ok(None),
    }
}

/// Loads `text` like Open does and makes every read an editor makes;
/// Ok(true) when it loaded.
fn exercise(text: &str) -> Result<bool, TestCaseError> {
    let Ok(doc) = core_model::file::from_json(text) else {
        return Ok(false); // refused with an error: fine
    };
    let Some(s) = no_bug("from_document", Session::from_document(doc))? else {
        return Ok(false);
    };
    let Some(snap) = no_bug("snapshot", s.snapshot())? else {
        return Ok(true);
    };
    for q in &snap.sequences {
        no_bug("sequence_info", s.sequence_info(q.id))?;
        no_bug("seconds_at", s.seconds_at(q.id, vec![0, 15_360, 1 << 40]))?;
        let Some(lanes) = no_bug("lanes", s.lanes(q.id))? else {
            continue;
        };
        for l in &lanes {
            no_bug("lane_events", s.lane_events(q.id, l.id))?;
            for rule in [
                BlockRule::SilenceGap { ticks: 15_360 },
                BlockRule::FixedBars { bars: 1 },
                BlockRule::WholeLane,
            ] {
                no_bug("blocks", s.blocks(q.id, l.id, rule))?;
            }
        }
    }
    no_bug("selection", s.selection())?;
    no_bug("mixer", s.mixer())?;
    no_bug("destinations", s.destinations())?;
    no_bug("modify_templates", s.modify_templates())?;
    let _ = s.close();
    Ok(true)
}

/// Byte damage: flip, insert, delete or truncate.
#[derive(Debug, Clone)]
enum Damage {
    Flip(usize, u8),
    Insert(usize, u8),
    Delete(usize),
    Truncate(usize),
}

fn damage() -> impl Strategy<Value = Damage> {
    prop_oneof![
        (any::<usize>(), any::<u8>()).prop_map(|(i, b)| Damage::Flip(i, b)),
        (any::<usize>(), any::<u8>()).prop_map(|(i, b)| Damage::Insert(i, b)),
        any::<usize>().prop_map(Damage::Delete),
        any::<usize>().prop_map(Damage::Truncate),
    ]
}

fn apply(bytes: &mut Vec<u8>, d: &Damage) {
    let n = bytes.len().max(1);
    match *d {
        Damage::Flip(i, b) => {
            if let Some(x) = bytes.get_mut(i % n) {
                *x ^= b | 1;
            }
        }
        Damage::Insert(i, b) => bytes.insert(i % (bytes.len() + 1), b),
        Damage::Delete(i) => {
            if !bytes.is_empty() {
                bytes.remove(i % n);
            }
        }
        Damage::Truncate(i) => bytes.truncate(i % n),
    }
}

/// Values that break assumptions: wrong types, extremes, empties.
fn nasty() -> impl Strategy<Value = Value> {
    prop_oneof![
        Just(Value::Null),
        Just(json!(true)),
        Just(json!(-1)),
        Just(json!(0)),
        Just(json!(i64::MAX)),
        Just(json!(i64::MIN)),
        Just(json!(u64::MAX)),
        Just(json!(1e308)),
        Just(json!(-0.5)),
        Just(json!("")),
        Just(json!("x")),
        Just(json!([])),
        Just(json!({})),
        Just(json!([[[[[[[[1]]]]]]]])),
        any::<i32>().prop_map(Value::from),
    ]
}

/// Every JSON pointer in `v`.
fn pointers(v: &Value, at: String, out: &mut Vec<String>) {
    match v {
        Value::Object(o) => {
            for (k, x) in o {
                let p = format!("{at}/{}", k.replace('~', "~0").replace('/', "~1"));
                out.push(p.clone());
                pointers(x, p, out);
            }
        }
        Value::Array(a) => {
            for (i, x) in a.iter().enumerate() {
                let p = format!("{at}/{i}");
                out.push(p.clone());
                pointers(x, p, out);
            }
        }
        _ => {}
    }
}

/// A JSON edit at the `i`th pointer: replace with any value, remove, or
/// tweak it keeping its type (numbers to extremes, strings emptied or
/// grown, flags flipped, arrays shortened or an element duplicated), so
/// many damaged documents still deserialize and reach the reads.
#[derive(Debug, Clone)]
enum Edit {
    Replace(usize, Value),
    Remove(usize),
    Tweak(usize, i64),
}

fn edit() -> impl Strategy<Value = Edit> {
    prop_oneof![
        1 => (any::<usize>(), nasty()).prop_map(|(i, v)| Edit::Replace(i, v)),
        1 => any::<usize>().prop_map(Edit::Remove),
        6 => (any::<usize>(), prop_oneof![
            Just(-1i64), Just(0), Just(1), Just(i64::from(i32::MAX)), Just(i64::MAX),
            Just(i64::MIN), Just(1 << 40), -100_000i64..100_000,
        ]).prop_map(|(i, k)| Edit::Tweak(i, k)),
    ]
}

fn tweak(v: &mut Value, k: i64) {
    match v {
        Value::Number(n) if n.is_f64() => *v = json!(k as f64 * 1.5),
        Value::Number(_) => *v = json!(k),
        Value::String(s) => {
            if k % 2 == 0 {
                s.clear();
            } else {
                s.push_str(&"x".repeat((k.unsigned_abs() % 300) as usize));
            }
        }
        Value::Bool(b) => *b = !*b,
        Value::Array(a) => {
            if k % 2 == 0 {
                a.pop();
            } else if let Some(first) = a.first().cloned() {
                a.push(first);
            }
        }
        Value::Object(o) => {
            o.clear();
        }
        Value::Null => *v = json!(k),
    }
}

fn apply_edit(doc: &mut Value, e: &Edit) {
    let mut all = Vec::new();
    pointers(doc, String::new(), &mut all);
    if all.is_empty() {
        return;
    }
    let (i, replacement) = match e {
        Edit::Replace(i, v) => (*i, Some(v.clone())),
        Edit::Remove(i) => (*i, None),
        Edit::Tweak(i, k) => {
            let Some(p) = all.get(i % all.len()).cloned() else {
                return;
            };
            if let Some(slot) = doc.pointer_mut(&p) {
                tweak(slot, *k);
            }
            return;
        }
    };
    let Some(p) = all.get(i % all.len()).cloned() else {
        return;
    };
    match replacement {
        Some(v) => {
            if let Some(slot) = doc.pointer_mut(&p) {
                *slot = v;
            }
        }
        None => {
            let (parent, key) = p.rsplit_once('/').unwrap_or(("", ""));
            let key = key.replace("~1", "/").replace("~0", "~");
            match doc.pointer_mut(parent) {
                Some(Value::Object(o)) => {
                    o.remove(&key);
                }
                Some(Value::Array(a)) => {
                    if let Ok(k) = key.parse::<usize>() {
                        if k < a.len() {
                            a.remove(k);
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

#[test]
fn the_undamaged_demo_loads_and_answers() {
    assert!(exercise(&demo_text()).unwrap());
}

#[test]
fn an_event_at_the_far_end_of_time_is_refused() {
    // Found by `json_edits_never_crash_and_many_still_load`: a tick near
    // i64::MIN loaded, and arranging it into blocks overflowed.
    for tick in [i64::MIN, i64::MAX, (1i64 << 40) + 1] {
        let mut doc: Value = serde_json::from_str(&demo_text()).unwrap();
        *doc.pointer_mut("/document/sequences/0/lanes/0/takes/0/events/0/tick")
            .unwrap() = json!(tick);
        assert!(
            core_model::file::from_json(&doc.to_string()).is_err(),
            "tick {tick} loaded"
        );
        assert!(!exercise(&doc.to_string()).unwrap());
    }
}

#[test]
fn damaged_packages_on_disk_are_refused_with_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let pkg = dir.path().join("Bad.vizdaw");
    std::fs::create_dir(&pkg).unwrap();
    let cases: [(&str, Option<&[u8]>); 4] = [
        ("missing project.json", None),
        ("empty file", Some(b"")),
        ("binary noise", Some(&[0, 159, 146, 150, 255, 0, 1])),
        (
            "valid JSON, wrong shape",
            Some(b"{\"format\":\"vizdaw-project\",\"version\":2}"),
        ),
    ];
    for (what, bytes) in cases {
        let file = pkg.join("project.json");
        let _ = std::fs::remove_file(&file);
        if let Some(b) = bytes {
            std::fs::write(&file, b).unwrap();
        }
        match Session::open(pkg.to_string_lossy().into_owned()) {
            Ok(_) => panic!("{what}: opened"),
            Err(VizError::Internal { message }) => panic!("{what}: internal error {message}"),
            Err(_) => {}
        }
    }
    // A directory where the file should be.
    std::fs::remove_file(pkg.join("project.json")).unwrap();
    std::fs::create_dir(pkg.join("project.json")).unwrap();
    assert!(!matches!(
        Session::open(pkg.to_string_lossy().into_owned()),
        Ok(_) | Err(VizError::Internal { .. })
    ));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(cases(128)))]

    #[test]
    fn byte_damage_never_crashes(damages in prop::collection::vec(damage(), 1..8)) {
        let mut bytes = demo_text().into_bytes();
        for d in &damages {
            apply(&mut bytes, d);
        }
        // Non-UTF-8 is refused before parsing, as `fs::read_to_string` does.
        if let Ok(text) = String::from_utf8(bytes) {
            exercise(&text)?;
        }
    }
}

#[test]
fn json_edits_never_crash_and_many_still_load() {
    use proptest::test_runner::{Config, TestRunner};
    let base: Value = serde_json::from_str(&demo_text()).unwrap();
    let loaded = std::cell::Cell::new(0u32);
    let mut runner = TestRunner::new(Config {
        cases: cases(256),
        failure_persistence: None,
        ..Config::default()
    });
    runner
        .run(&prop::collection::vec(edit(), 1..4), |edits| {
            let mut doc = base.clone();
            for e in &edits {
                apply_edit(&mut doc, e);
            }
            if exercise(&doc.to_string())? {
                loaded.set(loaded.get() + 1);
            }
            Ok(())
        })
        .unwrap();
    // Most single edits hit values validation accepts: the reads, not just
    // the parser, get damaged documents.
    assert!(
        loaded.get() >= 25,
        "only {} of 256 edited documents loaded",
        loaded.get()
    );
}
VIZDAW_EOF
```

Append to `crates/core-model/tests/query_flatten.rs`:

```rust
#[test]
fn positions_and_lengths_beyond_the_limit_are_refused() {
    use core_model::rules::MAX_TICKS;
    let d = base();
    let far = EventSpec::new(Tick(MAX_TICKS + 1), EventBody::note(60, 100, Tick(Q)));
    let long = EventSpec::new(Tick(0), EventBody::note(60, 100, Tick(MAX_TICKS + 1)));
    let before = EventSpec::new(Tick(-MAX_TICKS - 1), EventBody::note(60, 100, Tick(Q)));
    for spec in [far, long, before] {
        let r = apply(
            &d,
            &Command::Event(EventCmd::Insert {
                seq: SeqId(1),
                lane: LaneId(1),
                events: vec![spec],
            }),
        );
        assert!(r.is_err(), "accepted a tick beyond ±2^40");
    }
    let mut r = reference(
        0,
        None,
        LoopCount::Count(core_model::rules::MAX_LOOPS + 1),
        0,
    );
    r.tick = Tick(0);
    let refused = apply(
        &d,
        &Command::Event(EventCmd::Insert {
            seq: SeqId(0),
            lane: LaneId(0),
            events: vec![r],
        }),
    );
    assert!(refused.is_err(), "accepted 2^20 + 1 loops");
}
```

- [ ] **Step 2: Run them to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-ffi --test damaged_files an_event_at 2>&1 | grep -E "panicked|test result" -A1 | head -4
VIZDAW_FUZZ_CASES=3000 cargo test -p core-ffi --test damaged_files json_edits 2>&1 | grep -E "panicked at|internal error" | head -3
cargo test -p core-model --test query_flatten positions_and 2>&1 | grep -E "^error|panicked" | head -2
```

Expected: `an_event_at_the_far_end_of_time_is_refused` fails ("tick -9223372036854775808 loaded"); the long JSON run fails within its 3000 cases with `blocks: internal error attempt to subtract with overflow` (`crates/core-ffi/src/blocks.rs`, `bar_span`; if a run happens to pass, run it once more); the model test does not compile (`could not find MAX_TICKS in rules`).

- [ ] **Step 3: Positions and lengths within ±2^40 ticks, loops within 2^20**

In `crates/core-model/src/rules.rs`, Find:

```rust
fn need(cond: bool, field: &'static str, value: impl std::fmt::Display) -> Result<()> {
```

Replace:

```rust
/// The furthest position and the longest length a document may hold:
/// 2^40 ticks (about 400 days at 120 BPM). Beyond it the editors' and the
/// engine's time arithmetic could overflow, so a damaged file or a
/// hostile edit holding such a value is refused (control spec §11).
pub const MAX_TICKS: i64 = 1 << 40;

/// The most loops a reference may play.
pub const MAX_LOOPS: u32 = 1 << 20;

/// Checks that `t` is a position or length within `±MAX_TICKS`.
pub fn check_ticks(field: &'static str, t: i64) -> Result<()> {
    need((-MAX_TICKS..=MAX_TICKS).contains(&t), field, t)
}

fn need(cond: bool, field: &'static str, value: impl std::fmt::Display) -> Result<()> {
```

Find:

```rust
    check_group_channel(ev.group_channel)?;
    match &ev.body {
        EventBody::Note {
            pitch, duration, ..
        } => {
            need(*pitch <= 127, "pitch", pitch)?;
            need(duration.0 >= 0, "duration", duration.0)?;
        }
```

Replace:

```rust
    check_group_channel(ev.group_channel)?;
    check_ticks("tick", ev.tick.0)?;
    match &ev.body {
        EventBody::Note {
            pitch, duration, ..
        } => {
            need(*pitch <= 127, "pitch", pitch)?;
            need(duration.0 >= 0, "duration", duration.0)?;
            check_ticks("duration", duration.0)?;
        }
```

Find:

```rust
            if let Some(l) = length {
                need(l.0 > 0, "reference.length", l.0)?;
            }
            if let LoopCount::Count(n) = loop_count {
                need(*n >= 1, "reference.loop_count", n)?;
            }
```

Replace:

```rust
            if let Some(l) = length {
                need(l.0 > 0, "reference.length", l.0)?;
                check_ticks("reference.length", l.0)?;
            }
            if let LoopCount::Count(n) = loop_count {
                need((1..=MAX_LOOPS).contains(n), "reference.loop_count", n)?;
            }
```

Find:

```rust
            need(fade_in.length.0 >= 0, "fade_in.length", fade_in.length.0)?;
            need(fade_out.length.0 >= 0, "fade_out.length", fade_out.length.0)?;
```

Replace:

```rust
            need(fade_in.length.0 >= 0, "fade_in.length", fade_in.length.0)?;
            need(fade_out.length.0 >= 0, "fade_out.length", fade_out.length.0)?;
            check_ticks("fade_in.length", fade_in.length.0)?;
            check_ticks("fade_out.length", fade_out.length.0)?;
```

Find:

```rust
            need(s.start_offset.0 >= 0, "start_offset", s.start_offset.0),
            &ctx,
        );
        if let crate::sequence::SeqLength::Locked(t) = s.length {
            push(check_positive("length", t), &ctx);
        }
```

Replace:

```rust
            need(s.start_offset.0 >= 0, "start_offset", s.start_offset.0),
            &ctx,
        );
        push(check_ticks("start_offset", s.start_offset.0), &ctx);
        if let crate::sequence::SeqLength::Locked(t) = s.length {
            push(check_positive("length", t), &ctx);
            push(check_ticks("length", t.0), &ctx);
        }
```

Find:

```rust
            if let Some(len) = l.length {
                push(check_positive("lane.length", len), &lctx);
            }
```

Replace:

```rust
            if let Some(len) = l.length {
                push(check_positive("lane.length", len), &lctx);
                push(check_ticks("lane.length", len.0), &lctx);
            }
```

Two existing tests used ticks beyond the new limit as "huge" values; they now use the largest allowed one. In `crates/core-ffi/tests/blocks.rs`, Find:

```rust
    let big = i64::MAX - Q;
```

Replace:

```rust
    // The furthest a document may hold (the model refuses beyond it).
    let big = core_model::rules::MAX_TICKS - Q;
```

In `crates/core-model/tests/query_flatten.rs`, Find:

```rust
        *length = Some(Tick(i64::MAX - 1));
```

Replace:

```rust
        // The longest a document may hold (the model refuses beyond it).
        *length = Some(Tick(core_model::rules::MAX_TICKS - 1));
```

- [ ] **Step 4: Run the tests (and a long fuzz run)**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test query_flatten 2>&1 | tail -2
cargo test -p core-ffi --test damaged_files --test blocks 2>&1 | grep -E "^test result|FAILED"
for i in 1 2 3; do VIZDAW_FUZZ_CASES=3000 cargo test -p core-ffi --test damaged_files 2>&1 | grep -E "^test result|panicked at"; done
```

Expected: PASS; the three long runs pass too. A new failure in a long run is a real bug: fix it at its source (in the model's checks when a value is absurd, else in the arithmetic), add its case as a deterministic test beside `an_event_at_the_far_end_of_time_is_refused`, and record it in the task's report.

- [ ] **Step 5: The end-to-end test**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/vizctl/tests/e2e.rs <<'VIZDAW_EOF'
//! End to end (control spec §11): launches the built VizDAW with the
//! control channel in a throwaway home and drives README steps through
//! the socket. Ignored by default: it needs a window server and a built
//! app. Run it with
//!
//! ```text
//! VIZDAW_E2E_APP=dist/VizDAW.app cargo test -p vizctl --test e2e -- --ignored --nocapture
//! ```
//!
//! Every wait is a condition with a generous timeout (slow CI runners);
//! there is no fixed sleep.

use core_control::client::{CallError, Client};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use vizctl::launch::{LaunchOptions, Launched, launch, log_tail};

/// Generous: macos-15 runners are slow.
const WAIT_MS: u64 = 60_000;

struct App {
    launched: Launched,
    client: Client,
}

impl App {
    fn start() -> App {
        let app = std::env::var("VIZDAW_E2E_APP")
            .expect("set VIZDAW_E2E_APP to VizDAW.app or its executable");
        let home = tempfile::Builder::new()
            .prefix("vze")
            .tempdir_in("/tmp")
            .unwrap()
            .keep();
        let launched = launch(&LaunchOptions {
            app: PathBuf::from(app),
            home: Some(home),
            demo: false,
            timeout: Duration::from_secs(180),
            env: vec![("VIZDAW_AUDIO".into(), "offline".into())],
        })
        .unwrap_or_else(|e| panic!("launch: {e}"));
        let client = Client::connect(&launched.socket).unwrap();
        App { launched, client }
    }

    /// Calls `cmd`; panics with the app's log on an error.
    fn call(&mut self, cmd: &str, params: Value) -> Value {
        let timeout = Duration::from_millis(WAIT_MS) + Duration::from_secs(60);
        match self.client.call(cmd, params.clone(), timeout) {
            Ok(v) => v,
            Err(e) => panic!(
                "{cmd} {params}: {e}\nVizDAW log:\n{}",
                log_tail(&self.launched.log, 40)
            ),
        }
    }

    fn call_err(&mut self, cmd: &str, params: Value) -> core_control::ControlError {
        match self.client.call(cmd, params, Duration::from_secs(120)) {
            Err(CallError::Reply(e)) => e,
            other => panic!("{cmd}: expected an error, got {other:?}"),
        }
    }

    fn wait(&mut self, window: &Value, condition: Value) -> Value {
        let mut p = condition;
        p["timeout_ms"] = json!(WAIT_MS);
        p["window"] = window.clone();
        self.call("wait", p)
    }

    /// Quits and waits (generously) for the process to end.
    fn quit(mut self) {
        assert_eq!(self.call("app.quit", json!({}))["quitting"], true);
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            if let Ok(Some(status)) = self.launched.child.try_wait() {
                assert!(status.success(), "VizDAW exited with {status}");
                break;
            }
            if Instant::now() > deadline {
                let _ = self.launched.child.kill();
                panic!(
                    "VizDAW did not quit; log:\n{}",
                    log_tail(&self.launched.log, 40)
                );
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        assert!(!self.launched.socket.exists(), "the socket file is gone");
    }
}

/// README smoke steps 2–3 and 32 through the channel: create the demo,
/// play, check the transport moves, screenshot, stop, open the list.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn demo_play_screenshot_stop() {
    let mut app = App::start();
    let opened = app.call("app.new_demo", json!({}));
    let window = opened["window"].clone();
    assert!(window.as_i64().is_some_and(|w| w > 0), "{opened}");
    app.wait(
        &window,
        json!({"condition": "documents_at_least", "count": 1}),
    );

    let doc = app.call("state.get", json!({"target": "document", "window": window}));
    let song = doc["sequences"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "Song")
        .unwrap_or_else(|| panic!("no Song in {doc}"))["id"]
        .clone();
    assert_eq!(doc["selected_seq"], song, "Song is selected");

    // A command that would open a dialog fails instead, naming it.
    let e = app.call_err("command.run", json!({"id": "file.save", "window": window}));
    assert_eq!(e.dialog.as_deref(), Some("Save"), "{e}");

    app.call(
        "command.run",
        json!({"id": "transport.play_stop", "window": window}),
    );
    app.wait(&window, json!({"condition": "playing"}));
    let t0 = app.call(
        "state.get",
        json!({"target": "transport", "window": window}),
    )["tick"]
        .as_i64()
        .unwrap();
    app.wait(
        &window,
        json!({"condition": "tick_at_least", "tick": t0 + 1}),
    );

    let shot = app.launched.home.join("playing.png");
    let r = app.call("ui.screenshot", json!({"window": window, "path": shot}));
    let png = std::fs::read(&shot).unwrap();
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n", "a PNG");
    assert!(r["width"].as_u64().unwrap() >= 400, "{r}");

    app.call(
        "command.run",
        json!({"id": "transport.play_stop", "window": window}),
    );
    app.wait(&window, json!({"condition": "stopped"}));

    let tree = app.call("ui.tree", json!({"window": window}));
    let found = vizctl::cli::find_in_tree(&tree, "Arrangement");
    assert!(
        !found["matches"].as_array().unwrap().is_empty(),
        "no Arrangement in the tree"
    );

    app.call(
        "command.run",
        json!({"id": "editors.list", "window": window}),
    );
    app.wait(&window, json!({"condition": "detail_kind", "kind": "list"}));
    app.quit();
}

/// Events into VizDAW's own window: a click on the toolbar's Play button
/// plays, Space (the Transport menu's key) stops.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn click_play_and_press_space() {
    let mut app = App::start();
    let window = app.call("app.new_demo", json!({}))["window"].clone();
    let tree = app.call("ui.tree", json!({"window": window}));
    let play = vizctl::cli::find_in_tree(&tree, "Play");
    let id = play["matches"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["role"] == "AXButton")
        .unwrap_or_else(|| panic!("no Play button in {play}"))["id"]
        .clone();
    app.call("ui.click", json!({"window": window, "element": id}));
    app.wait(&window, json!({"condition": "playing"}));
    app.call("ui.key", json!({"window": window, "key": "space"}));
    app.wait(&window, json!({"condition": "stopped"}));
    app.quit();
}
VIZDAW_EOF
scripts/make-app.sh 2>&1 | tail -1
VIZDAW_E2E_APP=dist/VizDAW.app cargo test -p vizctl --test e2e -- --ignored --nocapture --test-threads 1 2>&1 | grep -E "^test |test result|panicked"
pgrep -fl "VizDAW --control" || echo "no VizDAW left running"
```

Expected: `built dist/VizDAW.app`; both tests pass in a few seconds; no VizDAW left running.

In `.github/workflows/ci.yml`, Find:

```yaml
      - name: App bundle
        run: scripts/make-app.sh
```

Replace:

```yaml
      - name: App bundle
        run: scripts/make-app.sh
      - name: End to end through the control channel (needs the runner's window server)
        run: cargo test -p vizctl --test e2e -- --ignored --nocapture --test-threads 1
        env:
          VIZDAW_E2E_APP: dist/VizDAW.app
```

- [ ] **Step 6: Gate and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace 2>&1 | grep -E "^test result|FAILED|panicked" | tail -60
scripts/build-ffi.sh && scripts/swift.sh build 2>&1 | tail -3 && scripts/swift.sh test 2>&1 | tail -3
git add crates/core-model/src/rules.rs crates/core-model/tests/query_flatten.rs crates/core-ffi/Cargo.toml crates/core-ffi/tests/blocks.rs crates/core-ffi/tests/damaged_files.rs crates/vizctl/tests/e2e.rs .github/workflows/ci.yml Cargo.lock
git commit -m "test: damaged project files never crash (ticks within 2^40); end to end through the control channel

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
```

Expected: every suite passes; 812 Rust tests listed (2 of them ignored e2e tests); Swift 568.

---

### Task 12: README — the channel, `vizctl`, the process change; checklist; the whole-branch gate

**Files:**
- Modify: `app/README.md`

**Interfaces:**
- Consumes: everything above.
- Produces: the "Agent control channel" section and checklist steps 81–86 under "### Control channel (plan CC)".

Spec §10, §12.

- [ ] **Step 1: Document it**

In `app/README.md`, Find:

```markdown
`--smoke` plays the demo for 3 s with the mixer open, puts Apple's AUDelay
```

Replace:

```markdown
`--control PATH` turns on the agent control channel (below); `--smoke`
plays the demo for 3 s with the mixer open, puts Apple's AUDelay
```

Find:

```markdown
## Manual smoke test
```

Replace:

````markdown
## Agent control channel

Off by default. `VizDAW --control PATH` (or `VIZDAW_CONTROL=PATH`) listens
for JSON-lines requests on a Unix socket at PATH (spec
`docs/superpowers/specs/2026-10-18-control-channel-design.md`). The
socket's folder must be private (VizDAW makes it 0700 when missing and
refuses one other users can open); the socket is 0600 and goes when
VizDAW quits. There is no network port. While the channel is on VizDAW
opens no dialog of its own: a call that would needs the answer as a
parameter, and app-level alerts are printed to stderr and listed by
`state.get windows`.

`vizctl` drives it from a shell:

```bash
cargo build -p vizctl && scripts/make-app.sh
target/debug/vizctl launch --app dist/VizDAW.app --demo --env VIZDAW_AUDIO=offline
export VIZDAW_CONTROL=/tmp/vizctl-…/ctl/vizdaw.sock   # the socket launch printed
target/debug/vizctl state windows
target/debug/vizctl run transport.play_stop && target/debug/vizctl wait playing
target/debug/vizctl screenshot --out /tmp/play.png
target/debug/vizctl find Play          # element IDs for click, drag and key
target/debug/vizctl key space && target/debug/vizctl wait stopped
target/debug/vizctl quit
```

`launch` runs VizDAW in a throwaway home (its own `HOME`, support folder
and log under `/tmp`, no plugin scan), never in yours. `vizctl help`
lists every command; `vizctl commands` lists the menu commands by ID.
Clicks and keys go into VizDAW's own windows: VizDAW need not be the
active app and the pointer does not move. A key that is a menu shortcut
runs that menu command on the window's document.

Agents implementing or reviewing VizDAW may run checklist steps this way
against a throwaway home, read state and attach screenshots; computer-use
tools stay banned. A screenshot does not replace steps that need ears,
VoiceOver or real devices.

## Manual smoke test
````

Append to `app/README.md` (after step 80):

```markdown

### Control channel (plan CC)

Run these from a shell in the repository, with `V=target/debug/vizctl`.

81. Off by default: `scripts/swift.sh run VizDAW` prints no "control
    channel on" line, and no socket appears anywhere.
82. `$V launch --app dist/VizDAW.app --demo` prints `pid`, `socket`,
    `home` and `log`; `ls -l` of the socket's folder shows `drwx------`
    and the socket `srw-------`; with `VIZDAW_CONTROL` set to it,
    `$V state windows` lists one "document" window titled "Untitled".
    A second `$V launch` with the same `--home` fails: "another VizDAW is
    listening".
83. `$V run transport.play_stop`, `$V wait playing`, `$V screenshot --out
    /tmp/play.png`: the PNG shows the arrangement with Groove's purple
    reference block, the Chords block and the red playhead, even while
    VizDAW's window is behind other windows. `$V run transport.play_stop`
    and `$V wait stopped`.
84. Dialogs: `$V run file.save` answers `dialog_required` with dialog
    "Save" and no panel appears; `$V run file.save '{"path":
    "/tmp/Demo.vizdaw"}'` saves (the window title becomes "Demo").
    `$V open /tmp/Demo.vizdaw` opens a second window.
85. Input: `$V find Play` gives the toolbar button's ID; `$V click <ID>`
    plays without the pointer moving or VizDAW coming to the front;
    `$V key space` stops; `$V drag 600,168 800,168` (points in the
    arrangement's ruler, from `$V find Arrangement`) selects a range
    (`$V state selection` shows `time_range`); with VoiceOver on, its
    outline sits on the block it reads.
86. Hostile input and quitting: `printf 'garbage\n' | nc -U
    $VIZDAW_CONTROL` answers `parse_error`; `$V call nope` answers
    `unknown_command`; `$V quit`: VizDAW quits and the socket file is gone.
```

- [ ] **Step 2: Run everything**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace 2>&1 | grep -E "^test result|FAILED|panicked" | tail -80
cargo test --workspace -- --list 2>/dev/null | grep -c ': test$'
scripts/build-ffi.sh && scripts/swift.sh build 2>&1 | tail -3 && scripts/swift.sh test 2>&1 | tail -3
scripts/make-app.sh 2>&1 | tail -1 && VIZDAW_E2E_APP=dist/VizDAW.app cargo test -p vizctl --test e2e -- --ignored --test-threads 1 2>&1 | grep -E "test result"
git status --short
```

Expected: every suite passes; 812 Rust tests listed; Swift 568; the e2e tests pass; `git status` shows only `app/README.md`.

- [ ] **Step 3: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add app/README.md
git commit -m "docs(app): agent control channel, vizctl and their checklist

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
```

---

## Self-Review

1. **Spec coverage:**
   - §3.1 purpose and §1 success: Tasks 9–11 (`demo_play_screenshot_stop` is the success scenario).
   - §3.2 off by default, private socket, 0600, no port: Task 4 (`it_listens_in_a_private_directory_and_cleans_up`, `unsafe_places_are_refused`), Task 6 (`theSocketComesFromTheFlagThenTheEnvironment`), Task 9 (start before documents; exit 2), checklist 81–82.
   - §3.3 JSON lines, Rust parsing/validation/dispatch types, Swift main thread: Tasks 2–5, 9.
   - §3.4 commands: `commands.list` / `command.run` + registry (Tasks 3, 6), `state.get` (Tasks 5, 6, 9), `ui.tree` (Task 7), `ui.click/drag/key` without system events (Tasks 7, 9; Rulings 8–9), `ui.screenshot` with CA content (Task 8), `app.*` (Tasks 6, 9), `wait` (Tasks 3, 5, 9).
   - §3.5 rules: dialogs (Tasks 3, 6, 9; spec §7), Busy (Tasks 6, 9), hostile requests (Tasks 2–4 fuzz and server tests).
   - §3.6 `vizctl`: Task 10.
   - §3.7 tests: parser and every command's params fuzzed (Tasks 2, 3), damaged files fuzzed in `cargo test` (Task 11), e2e with condition waits and CI step (Task 11).
   - §3.8 process change: spec §12 and README (Task 12).
   - §3.9 size and order: 12 tasks in the approved order (follow-ups first, as asked).
   - 4d follow-ups: Task 1.
2. **Placeholder scan:** every code step holds complete code (new files whole, changes as Find/Replace); the one open-ended instruction (Task 11 Step 4: a new fuzz finding) states exactly what to do with it.
3. **Type consistency:** `ControlRequest`/`ControlResult` cases (Task 5) are the ones Swift switches over (Task 9: `.runCommand`, `.state`, `.tree`, `.click`, `.drag`, `.key`, `.screenshot`, `.newDemo`, `.open`, `.quit`, `.wait`); `KeyStrokeRow.command` (Task 5) is set from `registry::command_for_stroke` (Task 3) and read by `ControlChannel.keys` (Task 9); `ControlOutcome` (Task 6) is answered by `ControlChannel.answer` (Task 9); `ControlUI.firstMouse` (Task 7) is read by the editors' `acceptsFirstMouse` (Task 7) and cleared by `ControlChannel.send` (Task 9); `controlElementCheck(role:label:title:)` (Task 5) matches `node_json`'s label-else-title rule (Task 5) and `ControlUI.element` (Task 7); `Facts`/`WaitFactsRow` fields agree (Tasks 3, 5, 6, 9); `vizctl::cli::find_in_tree` and `vizctl::launch::{launch, log_tail}` (Task 10) are what `e2e.rs` uses (Task 11).
4. **Review Focus:** each line has its named test in its owning task (Tasks 2–8, 11).

Running counts: Rust tests listed 760 → 760 (T1) → 773 (T2) → 785 (T3) → 794 (T4) → 800 (T5) → 804 (T10) → 812 (T11); Swift `@Test` functions 551 → 561 (T6) → 564 (T7) → 568 (T8).
