# Manual checks

The README's by-eye checks (`app/README.md`, numbered steps) that still need
a person, and where each stands. Claude runs what it can through the control
channel (`vizctl`) and screen control, and leaves here what needs the owner's
hands, ears, hardware or an unlocked screen. Bugs found go in `bugs.md`.

Status: **open** (not run), **owner** (needs the owner; Claude ran what it
could), **pass**, **fail** (see the bug).

## Week of 2026-11-23 (owner)

Test build: dd36914d (26.0.0), `~/Applications/VizDAW Test.app`.

### Co-Producer, time coaching and close prompts (README 395–438)

| Steps | What | Status |
|---|---|---|
| 395–420 | Co-Producer: buses, inserts, sends, new tracks, templates, mix help, automation, asks before touching your sound | open |
| 421–432 | Time records and coaching (Settings › Co-Producer, `Metrics/time.json`, Delete All Time Records) | open |
| 433–438 | Save prompts on close and quit (red close button, ⌘W, ⌘Q, two edited songs) | open |

### Fader groups, VCAs and the transport bar (README 439–456)

Claude ran these on 2026-11-19 through the control channel. Group/VCA
setup was done by command, because the mixer reads ⌘ and ⌥ from the real
keyboard (`NSEvent.modifierFlags`) and synthetic clicks can't hold them.

| Step | What | Status | What's left for you |
|---|---|---|---|
| 439 | Group three strips, drag one, ⌘Z | pass | Also make the group with ⌘-click + Mixer › Group |
| 440 | ⌥-drag a grouped fader moves it alone | owner | All of it (needs a real ⌥) |
| 441 | Group stops when a member hits +12 | pass | — |
| 442 | Group mute/solo links | pass | Turn the links off from the group tag's menu (Claude used a command) |
| 443 | VCA at −6: readout and sound | owner | Listen: the sound drops. The readout passed |
| 444 | VCA mute/solo marks | pass | — |
| 445 | VCA Touch, then Read playback | owner | All of it; Touch did record (export counted 7 points) |
| 446 | Delete a member, undo | pass | — |
| 447 | DAWproject export/import with a VCA and group | pass | — (B-001, the VCA automation track, is in the backlog) |
| 448 | Customize sheet: drag in/out/along, Command button, Reset | owner | The sheet itself (drag, Escape, per-window). Mixer button and Reset passed |
| 449 | Bounce… Command button opens the sheet | owner | All of it; with the screen locked it said "can't run from a button right now" |
| 450 | Narrowest mixer: one row, » lists the rest | owner | Open » and check its list. One row at 459 pt passed |
| 451 | Quit and reopen: same bar in both windows | owner | All of it (the test channel keeps settings in memory only) |
| 452 | Click on every beat, downbeat higher, through tempo/meter changes | owner | All of it (by ear) |
| 453 | Click Record Only, and −24 dB level | owner | All of it (by ear) |
| 454 | Bounce with Click on has no clicks; settings kept after reopen | owner | All of it |
| 455 | P/E/U open » at the field | pass | — |
| 456 | Mixer at 1600 pt shows no » | pass | — |

### Hand checks from the plan's reviews

| What | Status |
|---|---|
| Group tag, VCA strip and "VCA −6.0" offset row look right in each of the 5 themes | open |
| Customize sheet: drag, Escape cancels, each window keeps its own sheet | open |
| Press U then E quickly: » opens at Edit In, nothing typed is lost | open |
| Your own mixer opens at 1600 × 700, not 459 pt (the test copy's narrow size may have been saved) | open |

## `next` branch (after the beta)

These apply to a build from `next`, not the beta test build (dd36914d).

### Small fixes (README 457–464)

| Step | What | Status |
|---|---|---|
| 457 | DAWproject round trip: a VCA's moves add no instrument track | open |
| 458 | VCA labels: "VCA 1 Fader", "Fader of VCA 1", "VCA Strings Fader" | open |
| 459 | A read-only AU parameter's slider is greyed out | open |
| 460 | Destination edit (name, port, channel), one ⌘Z undoes all | open |
| 461 | A quick trim drag snaps without stutter | open |
| 462 | Record, ⌘W asks; Don't Save leaves the file as it was | open |
| 463 | A clean song shows the edited dot while it records | open |
| 464 | Quit with two songs, Discard Changes: the waiting save still happens | open |
| B-001 | Verify the DAWproject VCA fix in the next build from `next` | open |

### File panels, prompts and alerts (GPUI slice 7b)

Linux items need a Linux desktop with xdg-desktop-portal; Claude can't see
either platform's panels.

| Platform | What | Status |
|---|---|---|
| Linux | File › Open is one file chooser; choosing `project.json` inside a `.vizdaw` folder opens that project; MIDI, DAWproject and AAF files open too | open |
| Linux | Open panels show the right filter (label and patterns) and accept label; a folder chooser (Search Folder…, stems Bounce) picks folders only | open |
| Linux | Save panels (Save As, Bounce, MIDI export, interchange report): the suggested name, the folder, the extension added when typed without one | open |
| Linux | Saving over an existing file asks the app's own replace question; Replace saves, Cancel leaves the file as it was | open |
| Linux | The sheet-card prompts (save changes, replace, return prompt): Return presses the default button, Esc the cancel one | open |
| Linux | With no window open, an alert shows in a small dialog window of its own, and OK closes it | open |
| macOS | Each panel opened from a window is attached to it as a sheet; one opened with no window open floats above every window | partly passed 2026-11-24 (Save As: the window dims behind a panel centred on it; Esc closed it — owner; the no-window case still open) |
| macOS | Click into the document window (or another one) while a panel is up: the panel stays in front and the app doesn't look frozen | open |
| macOS | Start playback, then open a panel: the playhead keeps moving behind it | passed 2026-11-24 (controller: Save As over the demo, bar 2 → 13 while the panel was up) |
| macOS | Close the panel's window (or its document) while the panel is up: the panel goes and keys and menus come back | open |
| macOS | Native replace prompt (MIDI clip folder export; any save-panel replace question): Return cancels, Esc cancels, and with Full Keyboard Access on, Space does not replace — Replace must be clicked. If Esc or Space misbehave, raise it (owner ruling on the native button roles) | open |

### Interchange sheets: MIDI, For Another DAW, DAWproject and merge (GPUI slice 7c)

The README steps run again on a GPUI build from `next` (both platforms
where marked). The models, sheets and prompts are tested headless
(`crates/vizdaw-app/tests/{interchange,midi_export,for_daw,dawproject,merge,report}.rs`,
`crates/vizdaw-gpui/tests/{import_midi,export_midi,for_daw,report,merge,alerts}.rs`);
what's left is what a person sees, hears and opens elsewhere.

| Steps | What | Status |
|---|---|---|
| 257–261 | MIDI import: the sheet (where, channel → destination, Split, MPE, tempo map), one Undo, the report after it and from File › Last Import or Export Report…, type 0 split, File › Open… a `.mid` | open |
| 262–264 | MIDI export and MIDI Clip export: "What won't transfer" follows the options; the replace question (Cancel is the default); a place inside the project refused; Cancel leaves no file | open |
| 265–266 | Song info in Project Settings (Undo, save and reopen); typing a region's name | open |
| 267–273 | Export for Another DAW: the sheet and Bounce share the format rows; the folder (numbered on a second export), `About these files.txt`, muted and unticked strips, a pickup, Add the stems, Cancel mid-export, waiting for a running import | open |
| 282–285 | Export DAWproject: Region gain changes "What won't transfer"; the core's name in the save panel; replace asks first; waits for a bounce | open |
| 286–290 | Import a DAWproject from VizDAW (the return prompt, New Project); a missing instrument's placeholder; Cancel closes the new window; Song info round trip | open |
| 291–294 | Merge demo files; the return prompt names the open match; Merge With Existing Project… shows "Merge DAWproject" with "Reading the file…", the counts line and two conflicts; Cancel (and Esc) change nothing | open |
| 295–296 | Merge (Return): the edits arrive, "Drums" keeps its level, "Keys" gets an inactive "Returned <today>" take; the "Merge Report" opens; one Undo restores everything | open |
| 297 | No project open: Merge With Existing Project… asks for the project (panel #12); choosing `Merge Demo.vizdaw` opens it with the sheet | open |
| 298 | Merging the same file again: same conflicts, nothing added; while the merge waits, Convert, the rate change, Project Settings' format change and Relink are off with the tooltip "Finish or cancel the DAWproject merge waiting to be confirmed first." | open |
| 298 | While a merge waits, start each export (For Another DAW, DAWproject, AAF): each says it is waiting, and runs once Merge or Cancel ends the hold | open |
| 299 | `Other.dawproject` (not from VizDAW): no prompt; it opens as a new project | open |
| Linux | Panel #12 (Merge With Existing Project…): choosing `project.json` inside a `.vizdaw` folder merges into that project | open |
| Linux | The return prompt as the sheet card: Return is New Project, Esc is Cancel, Merge With Existing Project… is clickable | open |
| — | A file whose export ID was lost: the sheet lists the project's exports ("Export of <date> at <time>, N tracks", the newest selected); Preview starts the merge with the one chosen | open |

### Plug-in editor windows (host track H2)

A GPUI build from `host` (later `next`). The window models, the header, the
parameter list, the Built-in Synth window and the slot gestures are tested
headless (`crates/vizdaw-app/src/plugin_window.rs`,
`crates/vizdaw-gpui/tests/{plugin_windows,plugin_window_view,mixer_slots}.rs`);
the GPUI end to end opens a CLAP editor, the parameter list and the
Built-in Synth window on the Mac (`gpui_plugin_window_*`,
`gpui_builtin_synth_window_picks_a_sound`) and, in Linux CI under Xvfb,
checks the X11 container, the plug-in's child window and
`WM_TRANSIENT_FOR` (`gpui_x11_*`). What's left is what a person sees and
does with real editors. **Controller** rows Claude runs on the Mac with
screen control; **owner** rows need the owner's Linux desktop.

| Platform | Who | What | Status |
|---|---|---|---|
| macOS | controller | An AU Cocoa editor opens in its window under the 37-pt header, at its size | open |
| macOS | controller | An AU v3 editor: "Loading…" shows, then the editor; no hang or crash | open |
| macOS | controller | A real AU v3 opens Busy, then attaches on a later tick; closing its window while it opens leaves no editor behind; a unit that never answers shows the generic view after 3 s | open |
| macOS | controller | The bundled Multi-Q CLAP's editor (the EQ slot's single click opens it; the EQ's bypass button only bypasses) | open |
| macOS | controller | The SDK VST3 `again` sample's editor | open |
| macOS | controller | Turning a knob in an editor, then Edit › Undo, returns the knob: one undo step per drag ("Change Plugin Parameter") | open |
| macOS | controller | Bypass from the header: lit while bypassed, the slot's dot follows; Learn shows disabled with its help | open |
| macOS | controller | The keys rule (README :70–72): in a plug-in window plain keys go to the plug-in (Space does not play), ⌘ keys go to the menu | open |
| macOS | controller | ⌘W closes the plug-in window while a native editor has the keyboard, and leaves the plug-in on its slot | open |
| macOS | controller | A new plug-in window takes the keyboard when it opens, and gives it to nothing else (the document window keeps its own focus when brought back) | open |
| macOS | controller | A resizable editor follows a window drag and snaps without jitter; a fixed one cannot be resized | open |
| macOS | controller | A self-resizing CLAP editor grows its window | open |
| macOS | controller | Moving the window between a 2× and a 1× display keeps the editor sharp and at its size | open |
| macOS | controller | Closing a window during another editor's open (Busy) closes it once the open ends; nothing is left on screen | open |
| macOS | controller | Quit with two plug-in windows open: no crash, both reopen with the project (README 18) | open |
| macOS | controller | The parameter list: alignment, the thumb at both ends, scrolling with many parameters, the monospaced value column | open |
| macOS | controller | Real double-clicks: an insert opens its window without starting a drag; an instrument slot opens its window; the Built-in Synth slot opens the sound window of the focused track's synth (300×200, radio glyphs, the caption wraps) | open |
| Linux | owner | XWayland in the KDE session and a pure X11 session: editors embed in both | open |
| Linux | owner | The gui test plug-in, a DPF plug-in (timer required) and a heavy open-source instrument, each as CLAP and as VST3: the editor shows, animates and answers the mouse | open |
| Linux | owner | The plug-in window moves and minimizes with its document window (`WM_TRANSIENT_FOR`), also when opened from the Mixer window and at project reopen | open |
| Linux | owner | Typing into an editor's text field reaches the plug-in | open |
| Linux | owner | Ctrl-W closes the plug-in window while a native editor has the keyboard (an X11 editor that grabs focus may swallow it) | open |
| Linux | owner | 2× scale: the container sits under the header and the editor reopens at the new scale | open |
| Linux | owner | Close and quit with editors open: no BadWindow exit (also after a Busy close) | open |
| Linux | owner | No transparency or garbage in the editor area (the 24-bit container inside the 32-bit GPUI window) | open |
| Linux | owner | A plug-in with no editor shows its parameter list, not a black box, and its sliders take the mouse; Loading… is visible while a reopened project's plug-ins load; a plug-in turned off shows its reason, not the old editor's black area | open |
