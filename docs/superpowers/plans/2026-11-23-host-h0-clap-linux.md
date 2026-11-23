# 27.1.0 Host Track H0: CLAP on Linux to Parity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** CLAP plug-ins on Linux get what their editors need: the host timer and posix-fd extensions, a main-thread run loop the app drives, editor scale set before parenting, and editor resize requests honoured — proven with a GUI test plug-in under Xvfb — so the editor-window milestone (H2) can embed CLAP editors on Linux.

**Architecture:** All in `core-host` (CLAP host via clack-host 0.2) and `core-ffi`. core-host owns a main-thread run loop table (timers and fds registered by plug-ins) exposed as `run_loop_fds()` and `run_loop_tick(now)`; the app drives it from its main-thread poll (the GPUI wiring is H2's). No new threads: CLAP main-thread callbacks run on the main thread.

**Tech Stack:** Rust 2024; `clack-host`/`clack-extensions` 0.2 (enable `timer`, `posix-fd`); `x11rb` for tests; the in-repo `test-plugins` CLAP.

**Spec:** `docs/superpowers/specs/2026-11-20-cross-platform-27-1-design.md` (§2 goals 4–5, §9 phases 3–4). Plug-in hosting spec: `docs/superpowers/specs/2026-10-16-plugin-hosting-design.md`. Research notes with file:line citations: the host-track inventory (controller scratch; §1.2, §1.6, §1.12, §1.13, §5 and §6 H0 copied into briefs).

## Global Constraints

- This track runs in the worktree `~/workspace/vizdaw-host` on branch `host`; it never edits `crates/vizdaw-gpui` or `crates/vizdaw-app` (the mixer track owns them) — GPUI wiring of the run loop is H2.
- No audio-thread allocation or locking introduced (the `rt_alloc_plugins` tests stay green). Plug-in main-thread callbacks run only on the main thread.
- macOS behaviour unchanged (Cocoa editors keep working; timer/fd are no-ops there unless a plug-in asks).
- Song format unchanged.
- Subagents: no GUI, no computer-use, never `dangerouslyDisableSandbox`. Linux runs are done by the controller over SSH (`~/vizdaw-host` on the Linux box) or in CI under Xvfb.
- Banned vendor/DAW names out of repo text (licence notices exempt). Never commit `.DS_Store`, `.superpowers/`, generated FFI files or brand assets.
- CI: fmt, clippy `-D warnings`, tests on macOS and Linux.
- Commit trailers: `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>` and `Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P`.

## Review Focus

1. A plug-in that registers a timer or fd during `gui.create` (DPF, JUCE) finds the table ready; unregistering during a callback is safe; a plug-in destroyed with registrations left behind leaves none.
2. A plug-in that floods timers (period 0 or tiny) cannot stall the main thread beyond one tick's work.
3. `request_resize` honoured only for resizable editors, clamped to the plug-in's `adjust_size`; a refused size leaves the window unchanged.
4. Scale set before parent on X11; points vs pixels consistent across platforms.

---

### Task 1: core-host — CLAP timer and posix-fd support with a main-thread run loop

**Files:** `crates/core-host/Cargo.toml` (clack-extensions features), `crates/core-host/src/clap/*` (host impls), new `crates/core-host/src/run_loop.rs`, `crates/core-host/src/lib.rs`; tests `crates/core-host/tests/run_loop.rs`.
**Produces:** `HostTimerImpl`/`HostPosixFdImpl` on the CLAP host's main-thread state storing registrations in a per-instance table; a process-wide `RunLoop` facade: `run_loop_fds() -> Vec<(RawFd, FdFlags)>`, `run_loop_tick(now: Instant, ready: &[(RawFd, FdFlags)])` firing due timers (bounded work per tick) and fd callbacks; registrations removed when an instance is destroyed.
- [ ] Tests first (a fake CLAP-side caller through the host impls): register/unregister timers and fds; due timers fire once per period; unregister inside a callback; destroy removes leftovers; a period-0 flood fires at most once per tick. Then implement; fmt, clippy, `cargo test -p core-host`. Commit `core-host: CLAP timer and posix-fd with a main-thread run loop`.

### Task 2: core-host — editor scale, resize requests and logical sizes

**Files:** `crates/core-host/src/clap/*`, `crates/core-host/src/instance.rs`; tests in `crates/core-host/tests/`.
**Produces:** `set_scale` before `set_parent` on X11 (CLAP gui API order); `PluginEditor::open` returns logical points on every platform (removes the caveat at `instance.rs:93-95`); `request_resize` honoured for resizable editors via `adjust_size`, reported as `InstanceEvents.editor_resize: Option<(u32, u32)>`.
- [ ] Tests first, then implement; commit `core-host: editor scale and resize requests`.

### Task 3: test plug-in with a GUI, and core-ffi on Linux

**Files:** `crates/test-plugins` (a `gui` kind: X11 editor that registers a timer and an fd and requests a resize), `crates/core-ffi/src/editor.rs` (`on_main_thread` real on Linux; expose run-loop fds/tick and editor resize events), tests `crates/core-host/tests/clap_gui_linux.rs` (`#[cfg(target_os = "linux")]`, `#[ignore]` unless `DISPLAY` is set, run under Xvfb in CI).
- [ ] Tests first; implement; add the Xvfb run of these tests to the Linux CI job; commit `test-plugins: a CLAP GUI test plug-in; core-ffi run loop on Linux`.
- [ ] Controller: run the ignored Linux tests over SSH on the Linux box (XWayland `DISPLAY=:0`) and in CI; merge `host` into `next` after review.

## Outcome (2026-11-23)

All three tasks done (commits 97e26be4..56737b0d on branch `host`). Evidence: macOS and Linux core-host/core-ffi/test-plugins/vizdaw-scan green (Linux 1356 passed), the X11 editor tests pass under XWayland on the owner's Linux box, clippy clean on both; whole-branch review: ready to merge.

### Rulings

- Task 1: Ruling: offer CLAP timer/posix-fd only off macOS until H2 drives the run loop (Swift build and macOS GPUI also use core-host) — avoids starving mac plug-ins — mac plug-ins keep their own timers until H2
- Task 1: Ruling: unix fd type gated behind cfg(unix) so non-unix still builds — none
- Task 1: Ruling: run loop is thread-local (instances are created on the main thread); H2 must tick from the instance-creating thread — correct for the app — an offline renderer creating instances elsewhere would never tick them
- Task 2: Ruling: set_scale is a separate PluginEditor method (default no-op; X11 between create and set_parent) — additive, CLAP order — none
- Task 2: Ruling: IdleReport.editor_resize added so callers see resizes; the headless test editor lives on the timer kind (vizdaw.test.timer now has_editor) — avoids count churn — none
- Task 2: review Approved; Ruling (revises plan Review Focus 3): request_resize accepted from fixed-size editors too (CLAP allows it: zoom menus, panels) — report the size so the window follows, without adjust_size/set_size; resizable editors keep adjust+set — plan was wrong — none
- Task 3: Ruling: exported open_editor keeps its signature (Swift FakeSession conforms to the generated protocol); scale via Rust-only open_editor_scaled — keeps the Swift build compiling — none
- Task 3: Ruling: X11 tests skip at runtime without DISPLAY (not #[ignore]) — CI step needs no --ignored — a missing DISPLAY in CI silently skips (CI step sets it)
- Task 3: Ruling: the Swift build ignores editor_resize, so a macOS CLAP editor that grows itself keeps its old window size there — Swift is personal-only; GPUI H2 follows resizes — owner may see clipped CLAP editors in the Swift build
- Final: Ruling: request_resize acceptance gated off macOS until H2 (Swift build ignores editor_resize; the owner tests plug-ins there) — protects personal testing — macOS editors can't self-resize until H2

### Carried to H2

- Task 2: carry to H2: cap a fixed editor's requested size to the screen at the window layer
- Final: carry to H2: drive run_loop_pump/tick from the instance-creating thread in every Linux binary that loads CLAP plug-ins (vizdaw-gpui; vizdaw-play if it shows editors) before any Linux build ships; back off POLLNVAL fds; ignore editor_resize for closed slots; cap fixed-editor sizes to the screen; lift the macOS resize/timer gates when GPUI follows

### Deferred minors

- Task 1: minor (deferred): Member::drop silently keeps leftovers if the table borrow fails (unreachable); tick allocates two Vecs per tick
- Task 2: minor (deferred): ±1 px rounding drift between to_points (ceil) and from_points (nearest); scale-change test checks only the readout; test size encoding caps at 65535; log a declined set_scale
- Task 2: minor (deferred): no restore target if get_size failed at open
- Task 3: minor (deferred): test plug-in closes its X fd before unregistering it (DPF/JUCE unregister first)
- Task 3: minor (deferred): "reported once" resize assertions weak (pump_until discards events); latest-wins merge untested; a resize for a slot closed before idle is still delivered (document)
- Task 3: minor (deferred): CI could silently skip X11 tests if DISPLAY were missing (add VIZDAW_REQUIRE_DISPLAY); editors_main ignores filters; duplicated poll mapping; possible fd-callback flake; GC failure leaks a window id; libc::gettid needs glibc ≥ 2.30
- Final: minor (deferred): unbounded timer registrations (O(n²) tick) — per-instance cap (e.g. 256)
- Final: minor (deferred): libc::gettid needs glibc ≥ 2.30 (use syscall(SYS_gettid) if older distros matter, H4)
