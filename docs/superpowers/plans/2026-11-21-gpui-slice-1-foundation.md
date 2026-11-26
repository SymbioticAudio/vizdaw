# 27.1.0 Slice 1: App Foundation (core-app + GPUI shell) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The first end-to-end slice of the one-codebase VizDAW: a Rust app-model crate (documents and transport, ported from `VizDAWAppModel` with its tests) and a GPUI app that opens a document window with a transport bar and menus, driven by the control channel so `vizctl` and a first GPUI e2e test run against it on macOS and Linux.

**Architecture:** `crates/vizdaw-app` (core-app) holds app logic with no GPUI dependency. It talks to the core through a `DocSession` trait implemented for `core_ffi::session::Session` (production) and a `FakeSession` (tests), mirroring the Swift app's `SessionProtocol`/`FakeSession` so the Swift tests port one for one. `crates/vizdaw-gpui` is the app binary: GPUI windows and views over core-app, the control channel (`core_control::Server`) pumped on the main thread, and an element registry for `ui.*`.

**Tech Stack:** Rust 2024; GPUI pinned to an exact git rev of its parent repository's main branch (Task 1); existing `core-ffi`, `core-control`, `vizctl`.

**Spec:** `docs/superpowers/specs/2026-11-20-cross-platform-27-1-design.md`. Spike findings that bind this plan: `docs/superpowers/specs/2026-11-21-gpui-spike-report.md` ("What phase 2 must plan for", items 1–5, 7, 8). Swift reference (behaviour and tests to port): `app/Sources/VizDAWAppModel/DocumentModel.swift`, `DocumentStore.swift`, and `app/Tests/VizDAWAppModelTests/{DocumentModelTests,DocumentStoreTests,TransportCommandsTests,TransportReadoutsTests}.swift`, `FakeSession.swift`.

## Global Constraints

- Song format unchanged (v11). A project saved by the Swift test build opens in the GPUI app and vice versa.
- core-app (`vizdaw-app`) never depends on GPUI. Only `vizdaw-gpui` does.
- GPUI: one exact git rev, recorded in `crates/vizdaw-gpui/Cargo.toml` with a comment naming the date and why. Must include the X11 `window_handle` fix (spike report item 1).
- Single-key shortcuts carry `!TextInput` in their key context (spike item 3).
- Every clickable control records role, label and bounds in the element registry while drawing; element paths come from the view tree, not constants (spike item 4, final-review note).
- Control calls handled per frame are capped (at most 16 per pump tick) so a burst cannot stall drawing.
- Element IDs follow `core_control::element` (`"<path>:<fnv1a32(role U+001F label)>"`).
- macOS dev runs launch through an `.app` wrapper (spike item 8); `vizctl launch` must find the GPUI binary inside it.
- Subagents: no GUI, no computer-use tools, never `dangerouslyDisableSandbox`. Visible checks are run by the controller (Mac) or over SSH on the owner's Linux box (192.168.1.192, KDE; XWayland `DISPLAY=:0`, `XAUTHORITY=/run/user/1000/xauth_*`).
- Banned vendor/DAW names stay out of repo text (licence notices exempt). Never commit `.DS_Store`, `.superpowers/`, generated FFI files or brand assets.
- CI: `cargo fmt --all -- --check`, clippy `-D warnings`, tests on macOS and Linux runners.
- Commit trailers: `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>` and `Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P`.

## Review Focus

1. A transport poll that fails or reports busy must not flood alerts or freeze the readout (Swift: `failingPollsRaiseOneAlert`, `aBusyPollKeepsTheLastTransportWithoutAnAlert`) — ported in Task 3.
2. Save on an untitled document must route to Save As, never write to an empty path (Swift: `saveWhenUntitledNeedsSaveAs`) — Task 2.
3. Opening a folder without the `.vizdaw` extension, or a missing project, must fail with the error shown, not a panic (Swift: `openRefusesFoldersWithoutTheExtension`, `openMissingProjectShowsTheError`) — Task 2.
4. Typing in a text field must not trigger single-key transport shortcuts (space, P, E, U) (Swift: `transportKeysAreOffWhileAnyTextIsEdited`) — Task 4 test.
5. `app.quit` with unsaved changes and no `discard` must answer `unsaved_changes`, not quit (next small fixes Task 6, R7) — Task 5 test.

---

### Task 1: Workspace crates and the GPUI pin

**Files:**
- Create: `crates/vizdaw-app/Cargo.toml`, `crates/vizdaw-app/src/lib.rs`
- Create: `crates/vizdaw-gpui/Cargo.toml`, `crates/vizdaw-gpui/src/main.rs`, `crates/vizdaw-gpui/src/lib.rs`, `crates/vizdaw-gpui/tests/boot.rs`
- Modify: `.github/workflows/ci.yml` (Linux runner: GPUI's system packages; macOS: Metal toolchain if the pinned rev needs it)

**Interfaces:**
- Produces: crate `vizdaw_app` (empty lib), crate `vizdaw_gpui` with `pub fn run()` used by `main.rs`.

- [ ] **Step 1: Choose the rev.** Take the parent repository's main branch head (as of the day the task runs). Confirm in that rev: `crates/gpui_linux/src/linux/x11/window.rs` implements `HasWindowHandle for X11Window` without `unimplemented!`; note whether it builds Metal shaders at build time and the feature to avoid it (the spike used `runtime_shaders`). Depend on the `gpui` crate by `git = "...", rev = "<sha>"` (and the platform crate it needs to open windows, e.g. `gpui_platform` if main requires it — read its `crates/gpui/examples` hello-world).
- [ ] **Step 2: Failing test** `crates/vizdaw-gpui/tests/boot.rs`:

```rust
#[gpui::test]
fn opens_an_empty_document_window(cx: &mut gpui::TestAppContext) {
    let w = cx.add_window(|window, cx| vizdaw_gpui::shell::DocumentWindow::empty(window, cx));
    cx.run_until_parked();
    assert!(w.read_with(cx, |v, _| v.title().is_empty()).is_ok());
}
```

- [ ] **Step 3: Run** `cargo test -p vizdaw-gpui --test boot` → FAIL (no `shell`).
- [ ] **Step 4: Implement** `vizdaw_gpui::shell::DocumentWindow` with `empty(window, cx)` and `title(&self) -> &str`; `run()` opens one empty window. If the main-branch rev does not build on either OS, fall back to `gpui = "=0.2.2"` with the spike's two patches (`spikes/gpui-spike/patches/` on branch `spike/gpui`) and record a ruling.
- [ ] **Step 5: Run** the test → PASS on macOS; `cargo build -p vizdaw-gpui` and the test on the Linux box (`ssh 192.168.1.192`) → PASS; `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check` clean; the full workspace test suite still passes.
- [ ] **Step 6: Commit** `vizdaw-gpui: crate skeleton and GPUI pin`.

### Task 2: core-app documents (DocSession, DocumentModel, DocumentStore)

**Files:**
- Create: `crates/vizdaw-app/src/session.rs` (trait + impl for `core_ffi::session::Session`), `crates/vizdaw-app/src/document.rs`, `crates/vizdaw-app/src/store.rs`, `crates/vizdaw-app/src/alerts.rs`
- Create: `crates/vizdaw-app/tests/support/fake_session.rs`, `crates/vizdaw-app/tests/document.rs`, `crates/vizdaw-app/tests/store.rs`

**Interfaces:**
- Produces:
  - `pub trait DocSession: Send + Sync` with exactly the `Session` methods Tasks 2–3 call (signatures copied from `crates/core-ffi/src/session.rs`: `save`, `save_as`, `snapshot`, `transport`, `play`, `stop`, `undo`, `redo`, `can_undo`/titles as the Swift model uses them). `impl DocSession for core_ffi::session::Session`.
  - `pub struct DocumentModel` with `new(session: Arc<dyn DocSession>, path: Option<PathBuf>)`, `title(&self) -> String` (dirty mark as Swift `dirtyTitle`), `save(&mut self) -> SaveAction`, `save_as(&mut self, path: &Path) -> Result<(), String>`, `undo_title()`, `redo_title()`.
  - `pub enum SaveAction { Saved, NeedsSaveAs, Failed(String) }`.
  - `pub struct DocumentStore` with `new_demo()`, `new_empty()`, `open(path) -> Result<DocId, String>`, `close(id)`, `front() -> Option<DocId>`, `document(id) -> Option<&DocumentModel>`, alert queue `post_alert`, `take_alert`.
- Consumes: nothing from earlier tasks.

- [ ] **Step 1: Port the fake.** `FakeSession` mirrors `app/Tests/VizDAWAppModelTests/FakeSession.swift` for the methods in `DocSession` (scripted errors, busy flag, call log).
- [ ] **Step 2: Failing tests** — port these Swift tests one for one, same names in snake_case, same assertions: from `DocumentModelTests`: `undo_and_redo_menu_titles`, `dirty_title`, `save_when_untitled_needs_save_as`, `save_while_untitled_surfaces_use_save_as`, `undo_and_redo_while_busy_do_nothing_silently`; from `DocumentModelTests` store section: `creates_documents_per_window`, `the_front_document_is_the_last_key_one_still_open`, `open_refuses_folders_without_the_extension`, `open_missing_project_shows_the_error`, `take_alert_clears_the_store_alert`, `posted_alerts_queue_behind_the_one_showing`, `open_failures_queue_behind_the_one_showing`. Plus one real-session test: `saved_project_reopens_with_the_same_snapshot` (`Session::new_demo`, `save_as` to a temp `.vizdaw`, `Session::open`, compare `snapshot()` sequences and tracks).
- [ ] **Step 3: Run** `cargo test -p vizdaw-app` → FAIL (types missing).
- [ ] **Step 4: Implement** to the ported behaviour; read the Swift sources for each rule rather than guessing.
- [ ] **Step 5: Run** → PASS; clippy/fmt clean. **Step 6: Commit** `vizdaw-app: documents and store (ported from the Swift model)`.

### Task 3: core-app transport

**Files:**
- Create: `crates/vizdaw-app/src/transport.rs`, `crates/vizdaw-app/tests/transport.rs`

**Interfaces:**
- Consumes: `DocSession`, `DocumentModel`, `DocumentStore` alerts (Task 2).
- Produces: `pub struct TransportModel` with `play_stop(&mut self, doc: &DocumentModel, store: &mut DocumentStore)`, `stop_and_return(...)`, `poll(&mut self, ...) -> PollResult`, `poll_interval(&self) -> Duration`, `readout(&self) -> TransportReadout { bars_beats: String, time: String, playing: bool }`. Formatting matches the Swift readouts exactly.

- [ ] **Step 1: Failing tests** — port: `no_device_on_play_becomes_an_alert_and_stays_stopped`, `play_stop_toggles_and_poll_interval_follows`, `failing_polls_raise_one_alert`, `a_busy_poll_keeps_the_last_transport_without_an_alert`, `a_document_opened_while_busy_starts_stopped` (DocumentModelTests), and the readout-format cases from `TransportReadoutsTests.swift` (every `@Test` there).
- [ ] **Step 2: Run** → FAIL. **Step 3: Implement.** **Step 4: Run** → PASS; clippy/fmt. **Step 5: Commit** `vizdaw-app: transport model and readouts`.

### Task 4: GPUI document window, transport bar, menus

**Files:**
- Create: `crates/vizdaw-gpui/src/shell.rs` (DocumentWindow), `src/transport_bar.rs`, `src/menus.rs`, `src/registry.rs` (element registry; paths from the view tree), `src/text_input.rs` (from the spike, Apache-2.0 notice kept), `crates/vizdaw-gpui/tests/window.rs`
- Modify: `crates/vizdaw-gpui/src/main.rs`; `app`-level licence text list if the licences window reads one (add the GPUI notice)

**Interfaces:**
- Consumes: `DocumentStore`, `DocumentModel`, `TransportModel` (Tasks 2–3).
- Produces: `DocumentWindow::open(store, doc_id, cx)`; actions `NewDocument`, `OpenDocument`, `Save`, `SaveAs`, `CloseWindow`, `Quit`, `PlayStop`, `StopAndReturn`; `Registry` with `record(path, role, label, bounds, enabled)`, `tree()`, `resolve(id)` (spike API) where `path` is built by a `RegistryScope` pushed per container while rendering.

- [ ] **Step 1: Failing tests** (`TestAppContext`): `play_button_toggles_transport` (click `Play` element by registry ID → fake session got `play`), `space_plays_when_no_text_field_is_focused`, `space_types_into_a_focused_text_field` (Review Focus 4), `title_shows_the_dirty_mark`, `registry_paths_follow_the_view_tree` (a reordered container changes the path; old ID → `no_element`).
- [ ] **Step 2: Run** → FAIL. **Step 3: Implement**: transport bar (Play/Stop button, Stop-and-return, bars|beats and time readouts polled at `poll_interval`), File and Transport menus (`set_menus` on macOS; Linux keeps keyboard shortcuts this slice — in-window menu bar is slice 2, ledger it), file dialogs through GPUI's path prompts, window title from `DocumentModel::title`.
- [ ] **Step 4: Run** → PASS; clippy/fmt. **Step 5: Commit** `vizdaw-gpui: document window, transport bar, menus`.
- [ ] **Step 6 (controller, Mac):** build the `.app` wrapper, open the demo, play/stop with the button and space, Save As, reopen in the Swift test build (format check). Record results in the ledger.

### Task 5: Control channel in the GPUI app

**Files:**
- Create: `crates/vizdaw-gpui/src/control.rs`, `crates/vizdaw-gpui/tests/control.rs`
- Modify: `crates/vizdaw-gpui/src/main.rs` (`--control PATH`, `--no-scan`, `-ApplePersistenceIgnoreState YES` accepted and ignored)

**Interfaces:**
- Consumes: Task 4 actions and registry; `core_control::{Server, Config, Command}`.
- Produces: `control::handle(&Command, &mut AppState, cx) -> Result<Value, ControlError>` answering: `app.new_demo`, `app.open {path}`, `app.quit {discard?}`, `command.run` for `file.save`, `file.save_as {path}`, `transport.play_stop`, `transport.stop_and_return` (IDs from `core_control::registry`), `state.get transport` and `state.get windows` in the shapes of `docs/superpowers/specs/2026-10-18-control-channel-design.md` §state, `ui.tree`, `ui.click {element}`. Everything else → `unavailable` with a message naming slice 1.

- [ ] **Step 1: Failing tests:** `new_demo_then_play_reports_playing` (state.get transport `playing: true` after `transport.play_stop`), `quit_with_unsaved_changes_answers_unsaved_changes` (Review Focus 5), `quit_with_discard_quits`, `save_as_then_state_windows_names_the_file`, `pump_handles_at_most_16_calls_per_tick`.
- [ ] **Step 2: Run** → FAIL. **Step 3: Implement** (server pumped on a GPUI timer as in the spike's `control.rs`, capped per tick). **Step 4: Run** → PASS; clippy/fmt. **Step 5: Commit** `vizdaw-gpui: control channel (slice 1 commands)`.

### Task 6: `vizctl` launches the GPUI app; first GPUI e2e test

**Files:**
- Modify: `crates/vizctl/src/launch.rs` (`executable(app)`: read `CFBundleExecutable` from `Contents/Info.plist` instead of assuming `VizDAW`; accept a bare binary path on Linux)
- Create: `scripts/make-gpui-app.sh` (macOS `.app` wrapper for `vizdaw-gpui`, like the spike's `bundle-mac.sh`)
- Create: `crates/vizctl/tests/e2e_gpui.rs`
- Modify: `.github/workflows/ci.yml` (Linux job runs `e2e_gpui` under `xvfb-run` with `WAYLAND_DISPLAY` unset)

**Interfaces:**
- Consumes: Task 5 commands; `vizctl::launch::{launch, LaunchOptions}`.

- [ ] **Step 1: Failing tests:** unit test `executable_reads_cfbundleexecutable` (temp `.app` with `CFBundleExecutable = vizdaw-gpui`); e2e `gpui_demo_plays_saves_and_reopens` (launch with `VIZDAW_E2E_APP` pointing at the GPUI app/binary; `app.new_demo`; `transport.play_stop`; wait `playing`; stop; `file.save_as` temp; `app.quit`; relaunch; `app.open`; `state.get windows` names it). The e2e test is `#[ignore]` unless `VIZDAW_E2E_APP` is set, like the existing e2e tests' gating (read `crates/vizctl/tests/e2e.rs` and follow its pattern).
- [ ] **Step 2: Run** the unit test → FAIL. **Step 3: Implement.** **Step 4: Run** the unit test → PASS; then the controller runs the e2e on the Mac and on the Linux box (XWayland) → PASS. **Step 5: Commit** `vizctl: launch the GPUI app; first GPUI e2e test`.

## Outcome (2026-11-21)

All six tasks done with task reviews; final whole-branch review clean after one fix wave (Save As overwrite guard, 18d4a9e3). GPUI e2e passes on macOS and Linux (XWayland). Not yet verified: the Mac visible check of the window (screen was locked), and the CI xvfb + Vulkan e2e step (needs a GitHub run).

### Rulings

- Ruling P1: GPUI git deps need network; controller pre-fetches the pinned rev into ~/.cargo before T1 so the sandboxed implementer builds offline — cost: none.
- Ruling P2: Linux-box steps (ssh) are run by the controller, not subagents (sandbox network) — implementer reports "Linux pending"; controller verifies before marking complete.
- Task 1: Ruling: fixed review minor #1 (boot test asserted nothing; plan-mandated wording) in controller commit — one-line, covered by running the test
- Task 2: Ruling: round-trip tracks + AlertItem id equality enter round 1 despite Minor grade — tracks is a brief requirement, id identity is load-bearing for Task 3 failingPollsRaiseOneAlert
- Task 4: Ruling: GPUI third-party notices deferred to the packaging task (slice that ships a build) — must be done before any release build — cost: a release without notices would violate Apache-2.0
- Task 4: Ruling: Minor 'quit_now polls closed session' enters round 1 — same function as the Critical fix
- Task 6: Ruling: Minor launch.rs error text enters round 2 with the CI fix (same tiny diff)
- Final: Ruling: I2 is release-gating, not slice-gating — patch GPUI (applicationShouldTerminate hook) before any release build; tracked for the packaging/release slice — cost: a release build could lose unsaved edits on Dock quit/logout (crash markers remain, recovery not yet in GPUI app)
- Final: Ruling: carry to slice 2 — unsaved-state guard reads a cached snapshot; refresh on edits/plugin news or close/quit guards will discard edits

### Deferred minors (backlog)

- Task 1: minor (deferred): clippy.toml large-error-threshold 256 is global; revisit if preserve_order goes away
- Task 1: minor (deferred): lib.rs expect() on open_window — later slices add error reporting
- Task 2: minor (deferred): open() on already-open path returns id without front/activate signal — document for GUI layer
- Task 2: minor (deferred): path() maps a session.path() error (Busy) to untitled → save() says NeedsSaveAs transiently
- Task 3: minor (deferred): no-device test lost 'stays usable' tail (could re-play after clearing error)
- Task 3: minor (deferred): label-case rule TransportReadouts.label("Bar", .flat)=="bar" belongs to theme work
- Task 3: minor (deferred): vacuous s.alert().is_none() in busy-open test
- Task 4: minor (deferred): meter map unused (readout(None)); signature not shown this slice
- Task 4: minor (deferred): no GPUI test for cancelled Save As / failed Save banner
- Task 4: minor (deferred): DocumentWindow::add_field public test hook — doc(hidden) or remove in slice 2
- Task 4: minor (deferred): no app-level alert when no window is open (eprintln only)
- Task 4: minor (deferred): second Quit while quit prompt up opens a second prompt (add quit_prompting flag)
- Task 4: minor (deferred): close-window Save in progress + quit Discard can close the store mid-save (user chose Discard)
- Task 5: minor (deferred): ui.click ignores button/modifiers (right-click acts as left) — answer unavailable
- Task 5: minor (deferred): handler panics not caught as internal; socket left behind on panic
- Task 5: minor (deferred): default target = front document not key window; windows order front-first only
- Task 5: minor (deferred): frame top-left origin, visible always true (disclosed)
- Task 5: minor (deferred): failed-alert detection compares ids only (unrelated alert cleared → false 'failed')
- Task 5: minor (deferred): no test for calls queued behind app.quit in the same tick
- Task 5: minor (deferred): on_app_quit borrow-busy fallback leaks server/socket
- Task 6: minor (deferred): Info.plist scan not comment-aware (theoretical); e2e.rs has the same umask-002 temp-home bug
- Final: fix wave I1 ADDRESSED (re-review Approved); minor (deferred): Save As from the close prompt with Replace untested (shares finish_save_as)
