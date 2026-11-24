# 27.1.0 Slice 7b: Panels, Alerts, the File Menu and the Simple Sheets Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The File menu works from the mouse and keyboard on macOS and Linux: native open/save/folder panels of our own, an in-window sheet host, modal alerts (document, store, recovery, Recovered Recordings, the DAWproject return prompt), the Swift File menu titles/ids/submenus/enablement, the New Project format dialog, and the Project Settings, Import Audio, Relink and Bounce sheets — with job banners visible for menu-started jobs.

**Architecture:** A `file_panels` seam in `vizdaw-gpui` (spec constants for each Swift panel live in `vizdaw-app`; macOS via `objc2-app-kit` `NSOpenPanel`/`NSSavePanel` run app-modal from a deferred task; Linux via `ashpd` 0.13 portal requests with filters, accept label and current folder), a sheet host (one modal per window: dim backdrop, centred card, Esc/Return, focus trap, keys off while a sheet field edits, never shown while the control channel is on), prompts on GPUI's `window.prompt` for alerts. Sheet models in `vizdaw-app` (ProjectSettings, Media relink, Bounce), views in `vizdaw-gpui`.

**Tech Stack:** Rust 2024; GPUI at the slice 1 pin; `objc2-app-kit` (macOS), `ashpd` 0.13 (Linux); `core-ffi`, `core-control`, `vizctl`.

**Spec:** `docs/superpowers/specs/2026-11-20-cross-platform-27-1-design.md` (§2b, line 104 native panels). Swift reference with exact strings and file:line: the slice 7 inventory (§1.1 menu, §1.3 New Project format panel, §1.4.1 Project Settings, §1.4.2 Bounce, §1.4.9 Relink, §2 panels, §3.1 alerts, §7.2-7.4). Slice 7a plan and Outcome (rulings, carries to 7b): `docs/superpowers/plans/2026-11-23-gpui-slice-7a-files-and-jobs.md`.

## Global Constraints

- All slice 7a Global Constraints apply (and through them 6a/6b). The Swift app is retired: parity with its behaviour, no compatibility.
- Owner rulings (2026-11-23): Linux File › Open is one file chooser where choosing a `project.json` inside a `.vizdaw` folder opens that project; Linux says "Show in Folder" where macOS says "Show in Finder".
- Controller rulings: our own panel layer; File menu titles and ids switch to Swift's (`file.create_demo` "Create Demo Project", "Close"); the document alert is a modal prompt (the banner stays only as the no-window fallback); Mac panels are app-modal; the New Project format choice appears before the window exists (a modal sheet on the front window when there is one, else a small dialog window).
- Panels without type filters must still give files the right extension (`mid`, `midi2`, `dawproject`, `wav|aif|caf`, `txt`) and ask the replace question themselves when a typed name already exists.
- Under the control channel no panel, sheet, prompt or offer appears (7a behaviour stays).
- Subagents can't see either platform's panels: unit-test the spec table, extension and replace logic, and the portal/panel request construction; the controller checks macOS panels on screen; Linux panels go on the owner's manual-check list (no Linux desktop screenshots).

## Review Focus

1. A panel or sheet never runs re-entrantly inside a GPUI update (deferred tasks only); a window closed while its panel or sheet is up leaves nothing dangling and edits nothing.
2. A typed save name without an extension gets the right one, and an existing file is never overwritten without the replace question.
3. Alerts: one at a time per window, queued in order, never lost when a window closes; store alerts reach the front window or a fallback; recovery questions run before the document exists.
4. Keys: a sheet's text field turns the window's keys off and back on reliably (no stuck keyboard after Esc, close or error).
5. Menu enablement matches Swift for every File item in every state (no document, channel on, held merge, report present, relinkable media, copies present).

---

### Task 1: the file panels seam

**Files:** `crates/vizdaw-app/src/panels.rs` (spec table: rows 1–17 of inventory §2.1 with prompts, messages, types, default names/folders), `crates/vizdaw-gpui/src/file_panels/{mod.rs,macos.rs,linux.rs}`, Cargo features; Open and Save As move onto it; tests `crates/vizdaw-app/tests/panels.rs`, `crates/vizdaw-gpui/tests/file_panels.rs`.
- [ ] Tests (spec table vs Swift strings; extension routing; replace question; Linux `project.json` routing; portal request construction); implement; controller checks macOS Open/Save on screen; commit `gpui: native file panels`.

### Task 2: the sheet host and alerts

**Files:** `crates/vizdaw-gpui/src/{sheet_host.rs,alerts.rs,shell.rs}`, `crates/vizdaw-app/src/{store.rs,document.rs}` as needed; tests `crates/vizdaw-gpui/tests/{sheets.rs,alerts.rs}`.
**Produces:** one modal sheet per window; document and store alerts as prompts (queue, front window, no-window fallback); recovery questions (Open with Plugins Off?, Restore Autosave) wired into store open; Recovered Recordings; the DAWproject return prompt (New Project / Cancel; Merge is 7c). Carried from 7a: a GUI MIDI import failing with no other window open must show its alert; a GUI open of a VizDAW-made `.dawproject` must show the return prompt.
- [ ] Tests; implement; commit `gpui: sheets and alerts`.

### Task 3: the File menu and New Project format

**Files:** `crates/vizdaw-gpui/src/{menus.rs,menu_bar.rs}`, `crates/vizdaw-app/src/commands.rs`; tests `crates/vizdaw-gpui/tests/menus.rs`.
**Produces:** Swift's File menu (titles, ids, `Import Project` and `Export` submenus, keys, enablement per inventory §1.1), the id/title switch for `file.create_demo` and Close, the New Project format dialog (§1.3) with the remembered default.
- [ ] Tests (every item's title, key, enablement across states); implement; commit `gpui: the File menu`.

### Task 4: Project Settings and Import Audio

**Files:** `crates/vizdaw-app/src/project_settings.rs`, `crates/vizdaw-gpui/src/sheets/project_settings.rs`; tests from Swift `ProjectSettingsModelTests` (7).
**Produces:** the Project Settings sheet (§1.4.1: rate, depth, song info, notes, format job, held-merge help); Import Audio through panel #7 into `import_audio` with the import target.
- [ ] Tests; implement; commit `gpui: project settings and import audio`.

### Task 5: Relink and Open Co-Producer Copy

**Files:** `crates/vizdaw-app/src/relink.rs`, `crates/vizdaw-gpui/src/sheets/relink.rs`; tests from Swift `MediaModelTests` relink rows.
**Produces:** the Relink sheet (§1.4.9) and its offer on open/recheck, `Locate…` / `Search Folder…` panels; Open Co-Producer Copy (panel #2, store `open_copy`); e2e `gpui_relinks_imported_media` (the relink/import/save part of Swift `regions_normalize_relink_and_trash`).
- [ ] Tests; implement; commit `gpui: relink and co-producer copies`.

### Task 6: the Bounce sheet

**Files:** `crates/vizdaw-app/src/bounce.rs`, `crates/vizdaw-gpui/src/sheets/bounce.rs`; tests from Swift `BounceModelTests` (9).
**Produces:** the Bounce sheet (§1.4.2) with format rows, the check off the UI thread, progress and Cancel, panels #8/#9a; controller on-screen checks of the sheets and of banners for menu-started jobs (carried from 7a); README 180-184 manual checks.
- [ ] Tests; implement; controller checks on screen and ticks the checklist; commit `gpui: the bounce sheet`.
