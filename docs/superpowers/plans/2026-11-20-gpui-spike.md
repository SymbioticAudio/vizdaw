# GPUI Spike (27.1.0 Phase 0) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A throwaway GPUI app that proves (or disproves) the five checks in spec §7 on macOS and Linux, ending in a written verdict: go with GPUI, or bring the Tauri fallback to the owner.

**Architecture:** A standalone crate `spikes/gpui-spike` with its own Cargo workspace (so GPUI's dependency tree never touches the main `Cargo.lock`), depending on the existing `core-host`, `core-control` and (macOS) `platform-macos` crates by path. One window: a fake mixer of 64 meters and a playhead, a button, a text field, a menu bar, and a "Open Plug-in" button that opens a real plug-in editor in a child window. The control-channel server from `core-control` runs inside it and answers `ui.tree` / `ui.click` from a spike-side element registry.

**Tech Stack:** Rust 2024, GPUI (pinned, Task 1), `core-host` CLAP host, `platform-macos` AU host, `core-control` server, `vizctl` client.

**Spec:** `docs/superpowers/specs/2026-11-20-cross-platform-27-1-design.md` (§7 is binding). Control channel: `docs/superpowers/specs/2026-10-18-control-channel-design.md` (`ui.tree` / `ui.click` shapes and element IDs, lines 152, 172, 174).

## Global Constraints

- Throwaway: spike code is not merged into `crates/`. Only the report (Task 7) and, if it proves out, the CLAP X11 change in `core-host` (Task 6) may be kept, each through its own later review.
- Work on branch `spike/gpui` off `next`, in worktree `~/workspace/vizdaw-spike`. No pushes without the owner's OK.
- `spikes/` is outside `members = ["crates/*"]`; `spikes/gpui-spike/Cargo.toml` has its own `[workspace]` table and lockfile, both committed.
- GPUI is pinned to one exact version or git rev, recorded in the report.
- Subagents build and run headless tests only (no GUI, no computer-use, no `dangerouslyDisableSandbox`). Every check that needs a visible window is run by the controller on the Mac, and on Linux by the owner (or the controller if it has a Linux desktop session); steps say which.
- Never run `auval`.
- Banned vendor/DAW names stay out of repo text (licence notices exempt). The reference DAW consulted for ideas is not named in the repo.
- Commit trailers as usual (`Co-Authored-By` and `Claude-Session`).
- `cargo fmt --check` and `cargo clippy -D warnings` pass for the spike crate.

## Review Focus

1. Plug-in editor window outlives or detaches from its parent (closing the GPUI window must close the editor first; no dangling `NSView` / X11 window) — Task 3 and 6 tests close the parent with the editor open.
2. Meter reads on the UI thread taking a lock or allocating per frame — Task 2 asserts the feed is `AtomicU32` reads only and logs per-frame allocations count of zero after warm-up.
3. `ui.click` by element ID hitting a different element after the layout changes — Task 4 test reorders elements and expects `no_element`.
4. Control-channel pump starving the UI (blocking `next_call` on the main thread) — Task 4 pumps with a non-blocking poll on a 50 ms timer, test asserts frame times unchanged with a client spamming `ui.tree`.
5. Linux on a Wayland session: plug-in editors use X11; GPUI must be forced to X11 (XWayland) if embedding is needed — Task 6 runs both and reports which works.

## Preconditions (owner)

- A Linux desktop to run Task 6's visible checks: a Linux machine, or an Ubuntu 24.04 arm64 VM (e.g. UTM) on the Mac with GPU acceleration. If neither exists by Task 6, the controller runs Task 6's build and headless parts and parks the visible checks for the owner.
- The bundled Multi-Q built for macOS as CLAP and AU (`scripts/build-multiq.sh`) and, for Task 6, as a Linux CLAP (same script on Linux, or note why it cannot).

---

### Task 1: Scaffold, pin GPUI, menus, text field, shortcuts (check 5)

**Files:**
- Create: `spikes/gpui-spike/Cargo.toml`, `spikes/gpui-spike/Cargo.lock`, `spikes/gpui-spike/src/main.rs`, `spikes/gpui-spike/src/shell.rs`, `spikes/gpui-spike/README.md`
- Test: `spikes/gpui-spike/tests/shell.rs`

**Interfaces:**
- Produces: `shell::SpikeWindow` (the root view), `shell::actions!` set `Quit`, `FocusNext`, `Bump`; `shell::Counter(u32)` state read by later tasks; `fn run()` in `main.rs`.

- [ ] **Step 1: Pick the pin.** Compare crates.io `gpui 0.2.2` with the parent editor repository's main branch: does main add AccessKit (search its `crates/gpui/Cargo.toml` for `accesskit`)? Pin crates.io 0.2.2 unless main has AccessKit and builds standalone; then pin that exact git rev. Write the choice and reason in `README.md`.
- [ ] **Step 2: Write the failing headless test** using GPUI's `test-support` feature (`TestAppContext`):

```rust
#[gpui::test]
fn bump_action_increments_counter(cx: &mut gpui::TestAppContext) {
    let w = cx.add_window(|_, cx| gpui_spike::shell::SpikeWindow::new(cx));
    w.update(cx, |v, window, cx| window.dispatch_action(Box::new(gpui_spike::shell::Bump), cx)).unwrap();
    assert_eq!(w.read_with(cx, |v, _| v.counter.0).unwrap(), 1);
}
```

- [ ] **Step 3: Run** `cargo test --manifest-path spikes/gpui-spike/Cargo.toml` → FAIL (no `shell`).
- [ ] **Step 4: Implement** `SpikeWindow`: a "Bump" button (label "Bump", shows the counter), a single-line text field, `cx.set_menus` with an app menu (About, Quit ⌘Q) and an Edit menu (Undo, Cut, Copy, Paste, Select All), key bindings `cmd-b` → `Bump` scoped to the focused pane, `tab` → `FocusNext`. Make the crate a lib + bin so tests reach `shell`.
- [ ] **Step 5: Run** the test → PASS; `cargo fmt --check` and `cargo clippy -- -D warnings` clean.
- [ ] **Step 6 (controller, Mac GUI):** run the app; check ⌘Q quits, ⌘B bumps only when the pane has focus, typing, ⌘A/⌘C/⌘V and arrow keys in the text field, Tab moves focus. Note results for the report.
- [ ] **Step 7: Commit** `Spike: GPUI shell, menus, text field, shortcuts`.

### Task 2: 64 meters and a playhead at 60 fps (check 4)

**Files:**
- Create: `spikes/gpui-spike/src/meters.rs`, `spikes/gpui-spike/src/frame_stats.rs`
- Test: `spikes/gpui-spike/tests/meters.rs`

**Interfaces:**
- Produces: `meters::MeterFeed { cells: Arc<[MeterCell]> }` with `fn start_fake_engine(n: usize) -> MeterFeed` (a background thread writing peaks at 1 kHz with the same `AtomicU32` bit-cast pattern as `core-engine`'s `MeterCell`: `f32::to_bits` stored, `Relaxed`), `fn read(&self, i: usize) -> (f32, f32)`; `frame_stats::FrameStats` with `fn record(&mut self, dt: Duration)`, `fn p50()`, `fn p99()`, `fn over_budget(&self) -> f64` (share of frames > 16.7 ms).

- [ ] **Step 1: Failing tests:**

```rust
#[test]
fn feed_reads_what_the_engine_wrote() {
    let f = gpui_spike::meters::MeterFeed::with_cells(2);
    f.cells[1].hold(0.5, 0.25);
    assert_eq!(f.read(1), (0.5, 0.25));
}
#[test]
fn frame_stats_counts_over_budget() {
    let mut s = gpui_spike::frame_stats::FrameStats::default();
    for _ in 0..99 { s.record(Duration::from_millis(10)); }
    s.record(Duration::from_millis(20));
    assert!((s.over_budget() - 0.01).abs() < 1e-9);
}
```

- [ ] **Step 2: Run** → FAIL. **Step 3: Implement**; the mixer view paints 64 strips (meter bar with peak hold, a fader rectangle, a name) and a playhead line moving across a 64-lane arrangement strip, requesting a frame every vsync (`window.request_animation_frame()` or the pinned version's equivalent). Each frame records `dt` into `FrameStats`; every 10 s it prints p50, p99 and over-budget share to stderr. No `Mutex`, `RwLock` or `Vec` allocation in the paint path for meter reads.
- [ ] **Step 4: Run** tests → PASS; fmt/clippy clean. Commit `Spike: 64 meters and playhead with frame stats`.
- [ ] **Step 5 (controller, Mac GUI):** run release build 60 s on the owner's Mac, window full-screen. **Pass:** over-budget ≤ 1%, p99 ≤ 16.7 ms. Record the printed numbers.

### Task 3: Plug-in editor in a child window on macOS (check 1, Mac)

**Files:**
- Create: `spikes/gpui-spike/src/plugin_window.rs` (`#[cfg(target_os = "macos")]` parts inline)
- Test: `spikes/gpui-spike/tests/plugin_window.rs`

**Interfaces:**
- Consumes: `core_host::instance::{PluginInstance, PluginEditor, ParentView}` (`ParentView::new(ns_view)`, `PluginEditor::open/close/resize`), the CLAP host in `core-host` and the AU host in `platform-macos`; Multi-Q IDs `core_host::bundled::{MULTIQ_CLAP_ID, MULTIQ_AU_ID}`.
- Produces: `plugin_window::EditorHost` with `fn open(parent: &gpui::Window, inst: &mut dyn PluginInstance) -> Result<EditorHost, String>` and `fn close(self)`; drop closes.

- [ ] **Step 1: Failing headless test** (no window server needed): `EditorHost::open` with a test instance whose `editor()` is `None` returns `Err` containing `"no editor"`.
- [ ] **Step 2: Implement.** Get the GPUI window's `NSWindow` from `HasWindowHandle` (`RawWindowHandle::AppKit` gives the `ns_view`; its `window`). Create a borderless-titled `NSWindow` sized to the editor, `addChildWindow:ordered:NSWindowAbove` on the GPUI window so it moves and minimizes with it, pass its content view as `ParentView`, call `open`, resize the child to the returned size. Observe the parent closing (GPUI `on_window_should_close` or `on_release`) and close the editor before the parent goes. The "Open Plug-in" button opens Multi-Q CLAP; a second button opens Multi-Q AU.
- [ ] **Step 3: Run** test → PASS; fmt/clippy. Commit `Spike: plug-in editors in child windows on macOS`.
- [ ] **Step 4 (controller, Mac GUI), with `VIZDAW_BUNDLED_PLUGINS` set:** for CLAP and AU each: editor appears; dragging the GPUI window moves the editor; minimizing hides both; closing the GPUI window with the editor open closes both with no crash or console error; editor knobs respond. **Pass:** all four for both formats.

### Task 4: Control channel `ui.tree` / `ui.click` (check 3)

**Files:**
- Create: `spikes/gpui-spike/src/control.rs`, `spikes/gpui-spike/src/registry.rs`
- Test: `spikes/gpui-spike/tests/registry.rs`

**Interfaces:**
- Consumes: `core_control::server::{Server, Config, Call}` (`Server::start(path, config)`, `next_call()`, `reply(token, outcome)`); `vizctl` as the client.
- Produces: `registry::Registry` with `fn record(&mut self, path: &[usize], role: &str, label: &str, bounds: gpui::Bounds<gpui::Pixels>, enabled: bool)`, `fn tree(&self) -> serde_json::Value` (control-channel `ui.tree` shape, spec line 172), `fn resolve(&self, id: &str) -> Result<ElementRef, ControlError>`; element IDs exactly as spec line 152: `"<child index path>:<FNV-1a 32 of role U+001F label>"`, lowercase hex, 8 digits.

- [ ] **Step 1: Failing tests:**

```rust
#[test]
fn element_id_matches_control_channel_rule() {
    let mut r = Registry::default();
    r.record(&[0, 2], "button", "Bump", bounds(10., 10., 60., 24.), true);
    let id = r.tree()["nodes"][0]["id"].as_str().unwrap().to_owned();
    assert_eq!(id, format!("0.2:{:08x}", fnv1a32(b"button\x1fBump")));
}
#[test]
fn stale_id_after_relabel_is_no_element() {
    let mut r = Registry::default();
    r.record(&[0], "button", "Bump", bounds(0., 0., 1., 1.), true);
    let id = r.tree()["nodes"][0]["id"].as_str().unwrap().to_owned();
    r.clear(); r.record(&[0], "button", "Reset", bounds(0., 0., 1., 1.), true);
    assert_eq!(r.resolve(&id).unwrap_err().code(), "no_element");
}
```

- [ ] **Step 2: Run** → FAIL. **Step 3: Implement.** Elements register during paint (cleared each frame). `control.rs` starts `Server` at the path `vizctl` expects for a running app (read `crates/vizctl/src/launch.rs` for the socket location and launch flags), polls `next_call()` on a 50 ms GPUI timer (never blocking), answers `ui.tree` from the registry and `ui.click {element}` by dispatching the element's click handler on the main thread, and `state.get` with `{counter}` under a `spike` key. Other methods reply `unsupported`.
- [ ] **Step 4: Run** tests → PASS; fmt/clippy. Commit `Spike: control channel tree and click`.
- [ ] **Step 5 (controller, Mac GUI):** with the app running, `vizctl ui tree` lists "Bump"; `vizctl ui click <id>` raises the counter (read back with `vizctl state get`); while a shell loop calls `vizctl ui tree` continuously, Task 2's stats stay within the pass line. **Pass:** all three.

### Task 5: Accessibility probe (check 2, reported only)

**Files:**
- Modify: `spikes/gpui-spike/README.md` (findings section)

- [ ] **Step 1:** Record whether the pinned GPUI exposes any accessibility tree (AccessKit or platform-native). If Task 1 pinned a rev with AccessKit, give the Bump button and one fader a role and label through its API.
- [ ] **Step 2 (controller, Mac GUI):** with VoiceOver on, move to the Bump button and a fader; write down exactly what VoiceOver says (or that it says nothing). Not a pass/fail gate (owner: screen readers move to 27.2); the report states the gap and what closing it would take.
- [ ] **Step 3: Commit** `Spike: accessibility findings`.

### Task 6: Linux (checks 1, 3, 4, 5 on Linux)

**Files:**
- Modify: `crates/core-host/src/clap/instance.rs` (`PluginEditor for ClapInstance::open`: accept `GuiApiType::X11` on Linux with the parent X11 window id; Cocoa path unchanged)
- Modify: `crates/core-host/src/instance.rs` (`ParentView` doc: on Linux it carries an X11 window id)
- Modify: `spikes/gpui-spike/src/plugin_window.rs` (Linux: `RawWindowHandle::Xlib`/`Xcb` → X11 window id; Wayland: report unsupported)
- Test: `crates/core-host/tests/` — a headless unit test that the API choice is `X11` on Linux and `COCOA` on macOS (extract the choice into `fn editor_api() -> Option<GuiApiType>`).

- [ ] **Step 1: Failing test** `editor_api_is_platform_native` asserting `COCOA` on macOS and `X11` on Linux (cfg-gated asserts).
- [ ] **Step 2: Run** on the Mac → FAIL (function missing). **Step 3: Implement** `editor_api()` and the X11 branch of `open` (CLAP `set_parent` with `clap_window { api: "x11", x11: window_id }`). **Step 4:** `cargo test -p core-host` passes on the Mac; CI's Linux runner builds it. Commit `core-host: CLAP editors embed into X11 parents on Linux`.
- [ ] **Step 5: Build the spike on Linux** (`cargo build --release --manifest-path spikes/gpui-spike/Cargo.toml`), with Multi-Q Linux CLAP if available, else a free open-source CLAP with a GUI built from source (record which).
- [ ] **Step 6 (owner or controller, Linux desktop):** repeat Task 1 Step 6, Task 2 Step 5, Task 3 Step 4 (CLAP only) and Task 4 Step 5 under an X11 session; then under a Wayland session twice: GPUI native Wayland, and GPUI forced to X11 (unset `WAYLAND_DISPLAY`). Record each result. **Pass for Linux:** all checks under X11, and under Wayland at least one mode where the editor embeds.
- [ ] **Step 7: Commit** Linux findings in the README.

### Task 7: Spike report and verdict

**Files:**
- Create: `docs/superpowers/specs/2026-11-XX-gpui-spike-report.md` (date of writing)
- Modify: `docs/superpowers/specs/2026-11-20-cross-platform-27-1-design.md` (§7: link the report and the verdict)

- [ ] **Step 1:** Write the report: pinned GPUI version and why; a table of checks 1–5 × macOS / Linux X11 / Linux Wayland with pass, fail or reported, and the measured numbers; surprises; what phase 2 must plan for (menus, focus, child windows, registry approach for `ui.*`); the accessibility gap for 27.2.
- [ ] **Step 2: Verdict:** GO if checks 1, 3 and 4 pass on macOS and Linux (X11); otherwise STOP and bring the Tauri fallback to the owner with the failing rows.
- [ ] **Step 3: Commit** `Docs: GPUI spike report and verdict`. Tell the owner the verdict; do not start phase 1 until they accept it.
