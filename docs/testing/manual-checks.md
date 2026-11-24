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
| macOS | Each panel opened from a window is attached to it as a sheet; one opened with no window open floats above every window | open |
| macOS | Click into the document window (or another one) while a panel is up: the panel stays in front and the app doesn't look frozen | open |
| macOS | Start playback, then open a panel: the playhead keeps moving behind it | open |
| macOS | Close the panel's window (or its document) while the panel is up: the panel goes and keys and menus come back | open |
