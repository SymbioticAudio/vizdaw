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
