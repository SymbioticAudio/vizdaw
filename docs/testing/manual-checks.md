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
| 447 | DAWproject export/import with a VCA and group | pass | Note B-001 (VCA automation track exports as a synth track) |
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
