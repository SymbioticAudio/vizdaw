# 27.1.0 Slice 7c: Interchange Sheets Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The remaining File items get their Swift sheets: Import MIDI File, Export MIDI File / MIDI Clip File, the import/export report (Copy, Save as Text…, Show in Finder / Show in Folder), Export for Another DAW, Export DAWproject, Export/Import AAF, and Merge DAWproject (with the return prompt's Merge button) — on the 7a plumbing and the 7b panels, sheet host and alerts. After 7c, `file_menu::LATER` is empty.

**Architecture:** `InterchangeModel` and per-sheet models in `vizdaw-app` ported from Swift (`Interchange`, `ForDawModel`, `DawprojectModel`, `AafModel`, `MergeModel`, `ReportText`), views in `vizdaw-gpui/src/sheets/`. Dry-run checks run off the UI thread with generation counters (7a pattern); jobs and banners from 7a; panels from 7b (`file_panels::choose`); sheets in `sheet_host`; alerts via the presenter.

**Tech Stack:** Rust 2024; GPUI at the slice 1 pin; `core-ffi`, `core-control`, `vizctl`.

**Spec:** `docs/superpowers/specs/2026-11-20-cross-platform-27-1-design.md`. Swift reference with exact strings and file:line: the slice 7 inventory (§1.4.3–1.4.8, §1.4.10, §2.1 panels 9b–9d and 10–17, §3.1 Replace question, §5 tests, §7.3 traps, §7.4 the 7c list). Slices 7a and 7b plans and Outcomes (rulings, carries).

## Global Constraints

- All slice 7a/7b Global Constraints apply (Swift parity, no Swift compatibility; panels/sheets/alerts never under the control channel; nothing slow on the UI thread; tests press `secondary-` keys; Linux CI runs the GPUI tests).
- Owner rulings: Linux says "Show in Folder" where macOS says "Show in Finder" (new string, Linux only); a Linux `…/X.vizdaw/project.json` panel answer means that package (also for the Merge panel, row 12).
- Carried from 7b: every `file_menu::LATER` entry is wired by the end of this slice; the audio format rows (Bounce's picker helpers) are extracted and shared with Export for Another DAW, not copied; a held merge disables Convert and the rate change (7a carry); a queued return prompt that names a document closed since must not be used by Merge (prune on close).
- Exact Swift strings; UI says "track"; report text groups and caps as Swift `ReportText`; the report file name is always `MIDI Report.txt`; MIDI export failure re-opens its sheet with the message, DAWproject/AAF/ForDaw failures go to their sheets, others are alerts titled by `failureTitle`.
- One MIDI import at a time; exports wait for blocking jobs ("Waiting for the running … to finish…") instead of failing.

## Review Focus

1. No export writes over an existing file or folder without the replace question (clip file names, folder exports, `.dawproject`, AAF folder), and a refused write leaves nothing half-written claimed as done.
2. Dry-run checks: an older answer never overwrites a newer one; a closed document drops its answer; the sheet never freezes the UI.
3. Merge: the target is the right project (by id, then snapshot; a project path opens it); a held merge blocks the actions Swift blocks; confirm/cancel run once; a closed target is never used.
4. Import into a new project vs the current one follows Swift defaults (AAF new project by default); a failed/cancelled import into a made project closes it only when untouched (7a rule) and alerts once.
5. The report: text is Swift's grouping and caps; Copy/Save as Text/Show in Finder (Folder on Linux) act on the right report; the menu item's enablement follows "a report this document's UI kept".

---

### Task 1: Import MIDI File

**Files:** `crates/vizdaw-app/src/interchange.rs`, `crates/vizdaw-gpui/src/sheets/import_midi.rs`; tests from Swift `InterchangeTests` import rows.
**Produces:** the Import MIDI File sheet (§1.4.3): file info read off-main, tempo lock, choices, track defaults, new-project import; the File menu item wired (replacing 7b's interim read-only flow).
- [ ] Tests; implement; commit `gpui: import MIDI file`.

### Task 2: Export MIDI File and MIDI Clip File

**Files:** `crates/vizdaw-app/src/interchange.rs`, `crates/vizdaw-gpui/src/sheets/export_midi.rs`; tests from `InterchangeTests` export rows.
**Produces:** both export sheets (§1.4.4) with WontTransfer dry runs, panels #9b/#10, clip names and the Replace question (§3.1 #3), failure back to the sheet.
- [ ] Tests; implement; commit `gpui: export MIDI`.

### Task 3: the report sheet

**Files:** `crates/vizdaw-app/src/report_text.rs`, `crates/vizdaw-gpui/src/sheets/report.rs`; tests from `InterchangeTests` ReportText rows.
**Produces:** the report sheet (§1.4.8): ReportText, Copy (clipboard), Save as Text… (panel #17, `MIDI Report.txt`), Show in Finder / Show in Folder (`reveal_path`), title rules, `showLastReport` status line; Last Import or Export Report… wired.
- [ ] Tests; implement; commit `gpui: the import and export report`.

### Task 4: Export for Another DAW and Export DAWproject

**Files:** `crates/vizdaw-app/src/{for_daw.rs,dawproject.rs}`, `crates/vizdaw-gpui/src/sheets/{for_daw.rs,dawproject.rs}`, shared audio format rows (extracted from the Bounce sheet); tests from `ForDawModelTests` (19), `DawprojectModelTests` (9).
**Produces:** both sheets (§1.4.5, §1.4.6), waiting for blocking jobs, checks off-main, panels #9c/#13.
- [ ] Tests; implement; commit `gpui: export for another DAW and DAWproject`.

### Task 5: Export AAF and Import AAF

**Files:** `crates/vizdaw-app/src/aaf.rs`, `crates/vizdaw-gpui/src/sheets/aaf.rs`; tests from `AafModelTests` (9).
**Produces:** both AAF sheets (§1.4.7), panels #9d/#14–16 (the MIDI file beside the AAF, the media folder), the AAF MIDI path field with the keys-off owner.
- [ ] Tests; implement; commit `gpui: AAF export and import`.

### Task 6: Merge DAWproject

**Files:** `crates/vizdaw-app/src/merge.rs`, `crates/vizdaw-gpui/src/sheets/merge.rs`; tests from `MergeModelTests` (18).
**Produces:** the Merge sheet (§1.4.10), the return prompt's `Merge With Existing Project…` (panel #12, Linux project.json → package), held-merge blocks (Convert, rate change, Relink, format change, exports) with `MergeModel.heldHelp`; prune queued return prompts on close; `file_menu::LATER` empty; manual sweep README 257-273 and the DAWproject/merge headings added to docs/testing/manual-checks.md; the controller ticks the parity checklist File rows.
- [ ] Tests; implement; commit `gpui: merge DAWproject`.

## Outcome (2026-11-24)

All six tasks done (commits on `next` from the plan commit to 3b0dde57). Every File item now has its Swift sheet: Import MIDI File, Export MIDI File and MIDI Clip File, the import/export report (Copy, Save as Text…, Show in Finder / Show in Folder), Export for Another DAW and Export DAWproject (shared audio format rows with Bounce), Export and Import AAF, Merge DAWproject with the held-merge rules; `file_menu::LATER` is empty. Every replace question defaults to Cancel. Evidence: vizdaw-app/vizdaw-gpui/vizctl green at every task on the Mac and Linux (1449 on Linux at the end); GPUI e2e 28/28 on both; final whole-branch review ready after one small fix wave.

**Not yet seen on screen** (Mac locked; in docs/testing/manual-checks.md): the sheets' look, the macOS native replace prompt's Return/Esc/Space behaviour, the DAWproject merge sweep, and every Linux panel and prompt.

### Rulings

- Task 1: Rulings (implementer, accepted): .mid open routing unchanged (Swift opens with defaults, no sheet); one MIDI import at a time is the core's refusal ("MIDI Import Failed" alert), as Swift; the destination list is read once when the sheet opens (reading makes the mixer model if absent); report_shown/hide_report hook for Task 3 (Swift shows the report after an import)
- Task 2: Rulings (implementer): report_text.rs started with groups/line (Task 3 extends it); Swift's `written` for Show in Finder left to Task 3; Replace = Return, Cancel = Esc in the clip Replace question (same as the save-panel question; reviewer checks Swift's default); MIDI export doesn't wait for blocking jobs (Swift same; a clash is the core's refusal in the sheet)
- Task 2: review Needs fixes (conditional). Ruling: Cancel is the Return/default button in EVERY replace question (clip Replace and the 7b save-panel replace question) — Swift's destructive role is guarded by AppKit and the HIG puts the default on the safe choice; unverifiable here — cost: one extra keypress to replace
- Task 3: Rulings (controller): Linux a11y label "Show the exported files in their folder" accepted (Linux has no Finder); the save-failure title "The file “<name>” couldn’t be saved." mirrors the system message Swift showed — accepted; Show in Finder with several files reveals each folder once (GPUI reveal_path takes one path; Swift selects every file) — cost: not every file is selected
- Task 4: Rulings (implementer, accepted): audio format rows shared (audio_format.rs in both crates); no replace question for the for-DAW folder export (the core writes a new numbered folder; Swift asks nothing); a job failing at the first poll goes to its sheet (global constraint; Swift would alert); a failure drops in-flight checks in both sheets; Swift's dropped-MIDI ordering test replaced (no drop holding in the port); Other… shows as plain text while choices are off (no disabled text input)
- Task 5: Rulings (implementer, accepted): AAF export writes a new numbered "<Song> AAF" folder (core-ffi checked), no replace question (Swift none); Import AAF with no window opens as a new project (Swift); "Add to this project" doesn't wait for jobs (core Busy → "AAF Import Failed", Swift same); file_menu::LATER now empty
- Task 6: Rulings (implementer, accepted): the return prompt is the platform prompt on macOS now that no button is disabled (sheet card off macOS); a stale prompt keeps showing but loses its match on close (New Project works, Merge asks for the project); snapshot dates in fixed English format (not locale); confirm runs on the UI thread (Swift same)
- Final: Ruling: declined-to-judge items stand (bounce/imports don't wait on a held merge — Swift same; core-owned partial folders; channel jobs not blocking; fixed-English dates)

### Deferred minors

- Task 1: minor (deferred): a read landing under the channel leaves `shown` true (sheet pops after the channel goes off); a missing suggested destination shows its numeric ID (Swift blank); no test that opening the sheet leaves the document clean; either-or alert assertion; untested channel guard (redundant); dead setup line; destination list freshness relies on the mixer model's version refetch
- Task 2: minor (deferred): a save-panel error shows as a store alert (not the sheet error); untested Esc on the Replace prompt and closed-document drops
- Task 2: minor (deferred): save-panel replace question Esc tested only indirectly; from_prompt treats any first-position Cancel as DefaultCancel
- Task 3: minor (deferred): no sheet-level replace-question test for Save as Text; buttons in one row (Swift: Done alone right); press() dispatches on label strings
- Task 4: minor (deferred): dangling-symlink disagreement between Path::exists (save_target) and lstat (taken) → replace sent for a dangling link without asking (no data lost); save-panel error as a store alert; show_* reset when idle (Swift only false→true); no GPUI replace-question test on panel #13
- Task 5: minor (deferred): no two-document test for the front-window→document mapping; a new aaf read over a shown sheet doesn't resync the focused field's text (unreachable while the sheet is modal); two guard-only surviving mutants
- Task 6: minor (deferred): hold stored twice (Merge.held and merge_held; public setter); press falls back to Cancel for unknown labels; merges_waiting keyed by path survives a channel-answered open / channel turned on while panel #12 is up; test gaps (Cancel after a crash question, merge_into Err/Reading, two-window close during a hold); Merge press close keyed on report_shown
- Final: minor (deferred): merge-target lookup prefers the most recent project-ID match even if only another holds the snapshot (Swift parity; prompt names the match); report press() index→label after a fresh buttons(); merges_waiting by path (Task 6 minor)
