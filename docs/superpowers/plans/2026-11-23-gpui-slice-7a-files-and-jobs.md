# 27.1.0 Slice 7a: Files and Jobs Underneath Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every File-menu command (new with a format, demo with audio, import audio, project settings, bounce, MIDI import/export, MIDI clip export, export for another DAW, DAWproject import/export/merge, AAF import/export, the interchange report, convert, relink, open a Co-Producer copy) works in the GPUI app headless and over the control channel, with background jobs, progress banners, deferred control replies and open routing by file type — no sheets or panels yet (7b/7c).

**Architecture:** `vizdaw-app` grows the session seam (≈35 core-ffi calls on `DocSession`/`FakeSession`), a jobs model (`jobs`, `start_job`, `poll_jobs`, `JobAnswer`, project banners), channel request parsing ported from Swift (`InterchangeCommands`, `BounceCommands`, `ControlFormats`) and store routing (new project with a format, demo with audio, open by extension, made-project bookkeeping, DAWproject return prompt queue). `vizdaw-gpui` adds pending job/call lists in the control pump (replies deferred up to 600 s via `server.extend`), off-main background calls with generation checks, and the banners above the editors.

**Tech Stack:** Rust 2024; GPUI at the slice 1 pin; `core-ffi`, `core-control`, `vizctl`.

**Spec:** `docs/superpowers/specs/2026-11-20-cross-platform-27-1-design.md` (§2b owner rulings, §4, §8, §10). Parity checklist: File, File panels, Alerts rows. Swift reference with exact strings and file:line: the slice 7 inventory (§1.2 per-command flow, §1.5 banners, §2.2 channel bypass, §3.1 alerts in slice, §4 background work, §5 tests, §6 e2e, §7.3 traps, §7.4 the 7a list).

## Global Constraints

- All slice 6a/6b Global Constraints apply (`vizdaw-app` without GPUI, registry-sourced commands, `result_json` JSON, exact Swift strings, registry roles, naming rules, no GUI for subagents, CI, trailers). The Swift app is retired (owner 2026-11-23): parity with its behaviour is the goal; no compatibility with it.
- Owner rulings (2026-11-23): on Linux, File › Open is one file chooser where choosing a `project.json` inside a `.vizdaw` folder opens that project (7b implements the chooser; 7a's open routing must accept a `…/Song.vizdaw/project.json` path as that project); the Linux label for "Show in Finder" is "Show in Folder" (7c).
- Controller rulings: native panel layer of our own in 7b (design spec line 104); File menu titles and ids switch to Swift's (`file.create_demo` "Create Demo Project", "Close") in 7b; the document alert becomes a modal prompt in 7b (banner stays as the no-window fallback); Mac panels app-modal; the New Project format choice appears before the window exists.
- No synchronous slow core call on the UI thread: DAWproject/AAF dry runs, file info reads and checks run via `background_executor().spawn` with a generation counter so only the newest answer lands.
- Under the control channel no sheet, panel, prompt or offer appears (Swift); job commands reply when the job ends (deadline 600 s; expiry text exactly as Swift), and a reply names a made window only once it is on screen.
- Made projects: an import into a new project closes it again if the job fails or is cancelled and nothing was done in it.

## Review Focus

1. Deferred replies: a job reply is sent exactly once (done, failed, cancelled, expired), never for a closed document's job twice, and a window closed mid-job doesn't leave a dangling pending reply.
2. Off-main calls: no UI-thread freeze on DAWproject/AAF/MIDI checks; an older answer never overwrites a newer one; a document closed while its background call runs drops the answer.
3. Made projects and failures: failed/cancelled imports close an untouched made project and alert on the store; partial-failure messages name the files written (bounce).
4. Channel parsing: every refusal string and status matches Swift; hostile args (paths with `..`, wrong types, both/neither of exclusive params) are refused cleanly.
5. Open routing by extension (`.mid/.midi/.smf/.midi2`, `.dawproject`, `.aaf`, `.vizdaw` folder or its `project.json`): the right flow each time, a refused file creates no project.

---

### Task 1: core-app — the session seam

**Files:** `crates/vizdaw-app/src/{session.rs,testing.rs}`; tests `crates/vizdaw-app/tests/session_files.rs`.
**Produces:** `DocSession`/`FakeSession` calls from inventory §7.1-1 (media, project info/format, convert, import audio, job/cancel_job, bounce, MIDI file info/import/export/checks/clip names, for-DAW, DAWproject (+ snapshots, merge, confirm/cancel merge), AAF, interchange report, song info, recovered files/recover recordings), the free functions and constructors; fake job rows with scripted progress/done/failed/cancelled.
- [ ] Tests (round-trips through the fake; a real-session smoke for a few); implement; commit `core-app: the file and job session calls`.

### Task 2: core-app — jobs and banners

**Files:** `crates/vizdaw-app/src/{jobs.rs,banners.rs,document.rs}`; tests `crates/vizdaw-app/tests/jobs.rs`.
**Produces:** `jobs`, `start_job(kind, title, body)`, `poll_jobs()` from `idle()`, `JobAnswer` (wait / ok(status) / failed(msg)) shared by UI and channel, `ProjectBanner` texts and failure titles, `cancel_job`, Convert and device-rate offers, disk-too-slow and problems banners.
- [ ] Tests ported from `ProjectMediaTests` (12); implement; commit `core-app: jobs and project banners`.

### Task 3: core-app — channel request parsing

**Files:** `crates/vizdaw-app/src/{interchange_commands.rs,bounce_commands.rs,formats.rs}`; tests `crates/vizdaw-app/tests/{interchange_commands.rs,bounce_commands.rs}`.
**Produces:** `ControlFormats`, bounce requests, interchange requests and refusals with Swift's exact strings and statuses; dry runs as pending background calls.
- [ ] Tests ported from `InterchangeCommandsTests` (request/refusal ones) and `BounceCommandsTests`; implement; commit `core-app: file command requests`.

### Task 4: core-app — the store: new, demo with audio, open routing, made projects

**Files:** `crates/vizdaw-app/src/{store.rs,document.rs}`; tests `crates/vizdaw-app/tests/{store_open.rs}`.
**Produces:** `new_project(format)` + default format, `new_demo_audio`, `open_copy`, open routing by extension (MIDI → new project import; DAWproject read off-main → return prompt or import; AAF; `.vizdaw` folder or its `project.json`), made-project bookkeeping, the return prompt queue and its answers (prompt UI is 7b; the channel answers Cancel).
- [ ] Tests from `DocumentStoreTests` and `InterchangeTests` open rows; implement; commit `core-app: new, open routing and made projects`.

### Task 5: GPUI — control: deferred job replies, the commands, state

**Files:** `crates/vizdaw-gpui/src/{control.rs,app_state.rs}`; tests `crates/vizdaw-gpui/tests/control_files.rs`.
**Produces:** `PendingJob` and `PendingCall` lists checked every pump with `server.extend(token, 600 s + 5 s)`, the 16 commands plus `file.create_demo`/`file.create_demo_audio`, `file.new` rate/depth, `state.get media|project`, made-window replies.
- [ ] Tests; implement; commit `gpui: file commands over the channel`.

### Task 6: GPUI — banners and e2e

**Files:** `crates/vizdaw-gpui/src/{banners.rs,shell.rs}`, `crates/vizctl/tests/e2e_gpui.rs`.
**Produces:** banners (job progress + Cancel, Convert, device rate, disk, problems) above the editors with registry labels; GPUI ports `gpui_audio_import_plays_and_reports_media`, `gpui_bounces_the_demo_and_checks_the_file`, `gpui_midi_files_round_trip_over_the_channel`, `gpui_exports_for_another_daw_over_the_channel`, `gpui_dawproject_round_trip_over_the_channel`, `gpui_aaf_round_trip_over_the_channel`, `gpui_dawproject_merge_over_the_channel`.
- [ ] Tests; implement; controller runs e2e on the Mac and Linux, checks banners on screen and ticks the checklist; commit `gpui: project banners; e2e: files over the channel`.
