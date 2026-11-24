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

## Outcome (2026-11-24)

All six tasks done (commits on `next` from the plan commit to 4824fb9c). The File menu works from the mouse and keyboard on macOS and Linux: native panels of our own (macOS sheets on their window; Linux portal), the sheet host, modal alerts and questions (one per window, in order), recovery questions before a document opens, the Swift File menu and New Project format dialog, Project Settings, Import Audio, Relink, Open Co-Producer Copy and the Bounce sheet. Evidence: vizdaw-app/vizdaw-gpui/vizctl 1278 passed on the Mac and 1277 on Linux; workspace clippy clean on both; GPUI e2e 28/28 on both; final whole-branch review ready after one fix wave and one targeted follow-up.

**Not yet seen on screen** (the Mac was locked; listed in docs/testing/manual-checks.md): the macOS panels as sheets and their closing with the window, the playhead moving behind a panel, banners for menu-started jobs, the sheets' look, Relink panels over the sheet, Disable <names> in plugins.json; and every Linux panel and sheet-card prompt (owner's manual list).

### Rulings

- Task 1: Ruling (implementer): macOS project rows also allow folders (the GPUI bundle declares no org.vizdaw.project type) — so .vizdaw folders and Co-Producer copies can be chosen — cost: a plain folder can be picked and is refused by the store; Open takes one item (Swift); new Linux filter label strings
- Task 1: fix round 1 (530cf650): beginWithCompletionHandler + oneshot through PanelBackend (no runModal); detached flag clear; one case-insensitive extension rule; off-main Err; row-based wording; Linux MIDI mime types — tests RED→GREEN or mutation-proven. Ruling: keys off while a panel is up (Swift app-modal parity) — added to the round
- Task 1: round 1b (b0837be5): keystroke interceptor stops keys while a panel is up (Esc, Cmd-. pass; key-up passes) — tests RED→GREEN. Ruling: mouse clicks into a window still act while a panel is up — deliberate action, low risk — cost: a click can edit behind a panel (Swift blocked it)
- Task 2: Ruling (implementer, accepted): the no-window alert fallback is a small dialog window; the red banner is removed everywhere (incl. ui.tree dismiss-alert) — a banner can't show without a window — e2e that read dismiss-alert must change
- Task 2: Ruling (accepted): the return prompt is a sheet on both platforms (a native prompt can't disable the Merge button) — none
- Task 2: Ruling: Linux prompts use the sheet-card prompt (GPUI has no platform dialog on X11/Wayland) — controller tests on Linux
- Task 2: controller: two tests pressed Cmd-S/Cmd-W (Ctrl off macOS) — switched to secondary-; Linux alerts+sheets green (commit above). Ruling: implementer-common gains 'use secondary- for platform shortcuts in tests' for Tasks 3–6
- Task 2: fix round 1 (be669d97): presenter serialises all prompts per window (close/quit/replace routed through it; busy = presenter/app question/has_active_prompt/sheet); per-window sheet owner released on close; Disable <names> disables exactly the loading plug-ins (catalog set_enabled) and opens with plug-ins on; answer-mapping tests; found and fixed: an alert queued behind an app question never showed. Ruling (implementer): DocumentWindow.prompting left out of busy (it would block the routed close question) — none
- Task 3: Rulings (accepted): placeholders enabled as Swift but inert until their task (Copy/Relink T5, Project Settings T4, Bounce T6, Export + Report 7c — file_menu::LATER); Import AAF with a project open opens as a new project until 7c; Import MIDI reads the file and shows nothing until 7c (unreadable → Import Failed); New under the channel uses the default format; format panel in a dialog window when the front window has a sheet; copies check throttled to 1/s; Export tooltips dropped (GPUI menus have none)
- Task 4: Ruling (implementer): the Comment field is single-line (Swift multi-line); existing line breaks drawn as spaces — reviewer must confirm they survive an edit+save (else data change); text fields no longer panic on a line break (GPUI debug assert)
- Task 4: Ruling: 5 of Swift's 7 ProjectSettingsModelTests are region-rename tests (region inspector slice) — not ported here — carry to the region slice
- Task 5: Ruling (implementer): coming back to the app rechecks media, inferred from window activation (GPUI desktop has no app-activation event) — none; an alert raised while the Relink sheet is up waits until it closes (presenter rule, SwiftUI same)
- Task 6: Rulings (implementer, accepted): progress bar label "Bounce progress"; toggles as checkbox role; a save-panel error shows "Bounce Failed"; reopening Bounce… during a bounce shows its progress; Swift's bounce-tail test carries to the Settings: Audio slice
- Final: Ruling: declined-to-judge items stand (rulings: clicks behind a panel, alert behind the Relink sheet, single-line Comment, folders as projects; Linux portal look needs a human)
- Final: fix-wave re-review: Needs fixes — a macOS panel sheet whose parent window closes is never ended (panel-up flag may never clear → keys/menus off). Ruling: one targeted follow-up fix (the regression came from the wave itself; small, local) plus a manual-check row

### Carried

- Task 1: carry to Task 6 (controller on screen, Mac locked now): Open/Save panels appear and answer; the playhead keeps moving behind an open panel; Save As with a suggested Song.vizdaw doesn't double the extension on macOS 15+
- Task 2: carry to Task 6 (on screen): crash with a plug-in loading → Disable → that plug-in is off in plugins.json
- Task 3: carry to 7c: every file_menu::LATER entry must be wired by the end of 7c (check before 7c's final review)
- Task 5: carry to Task 6 (on screen): panels #2/#5/#6 over the sheet; recheck on return to the app
- Task 6: carry to 7c: extract the audio format rows (BounceChoices-bound picker helpers) for Export for Another DAW, don't copy
- Final: carry to 7c: Merge panel (row 12) maps a Linux project.json to its package; LATER entries; shared audio format rows

### Deferred minors

- Task 1: minor (deferred): not-main-thread panel returns None silently (should be Err); project check by save_extension (use row); Linux MIDI filter lower-case globs (add mime types); silent refusal of a second panel on Linux; tests for flag clearing on Err and non-project choose_save
- Task 1: minor (deferred): IME text can still reach a focused field in a window clicked into while a panel is up; Esc reaches windows (harmless cancels); with_extension strips a trailing /
- Task 2: minor (deferred): untested answer mappings (Disable, Trash/Later, Cancel role, Shift-held open, prompt re-show on close, cancel_field/end_shuttle); order can change on a failed show; Recovered Recordings offered only for saved projects (Swift on every init); sheet_host.rs 744 lines (split); present clones the queue on every notify
- Task 2: minor (deferred): an ask queued for a window that closes before it shows is orphaned (drop asks whose window is gone); a catalog save failure after Disable is silent (disabled in memory only this run)
- Task 3: minor (deferred): relinkable() clones media rows every frame and retries a failed read every frame; FILE_KEYED intent not pinned by a test; prefs written before new_project can fail (Swift same)
- Task 4: minor (deferred): Import Audio only tested at tick 0 (non-zero tick, no-sequence pool-only); 13 texts cloned per frame
- Task 4: minor (deferred): weak format-read assertion; unread-song GPUI test types into nothing (app test covers it)
- Task 5: minor (deferred): channel offered.retain untested; Unreadable not pinned here; project_path case-sensitive vs is_copy (Linux Take.VIZDAW); button layout vs Swift (Search Folder… left); hard-coded warning colour; Relink Failed alert waits behind the sheet (ruling)
- Task 5: minor (deferred): an app switch made while one of our panels/prompts is up is not rechecked on return (missed recheck, not stuck)
- Task 6: minor (deferred): Other… rate commits per keystroke (Swift on submit/blur); stale warning/length until the next check (and forever if a check panics); Bounce pressable again while its panel is pending on a non-modal backend (Linux) — a second job orphans the first; test gaps (app replace question at the sheet; failure assertion either-or; keys back after close with Other… focused)
- Final: minor (deferred): the format sheet turns menus off but not app keys (Cmd-O/Cmd-Q) — worst case a refused panel; FILE_KEYED lists open_copy/relink (no key in Swift)
