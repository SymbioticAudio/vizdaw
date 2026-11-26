# Controller checks (Claude, macOS)

Look-and-feel checks of the 27.1 GPUI app that Claude runs on the Mac with
screen control and the control channel (`target/VizDAW-GPUI.app`, offline
device and fake input unless a row says otherwise). They need an unlocked
screen. Rows that need the owner's ears, hands, devices or Linux desktop
are in `manual-checks.md`. Done rows are dropped once recorded in git.

Started 2026-11-25 from the open controller rows of slices 7b, 7c, 8b, 8c
and host track H2.

## Panels and prompts (7b)

- [ ] A panel opened with no window open floats above every window.
- [ ] Clicking into a window while a panel is up: the panel stays in front; the app doesn't look frozen.
- [ ] Closing the panel's window or document while it is up: the panel goes; keys and menus come back.
- [ ] Native replace prompt (MIDI clip folder export, save-panel replace): Return and Esc cancel; with Full Keyboard Access on, Space does not replace.
- [ ] Save prompts: ⌘W on an edited song asks; ⌘Q with two edited songs asks for each; Record then ⌘W asks and Don't Save leaves the file as it was; a clean song shows the edited dot while it records; Discard Changes at quit still does the waiting save.

## Interchange (7c)

- [ ] MIDI import sheet (where, channel → destination, Split, MPE, tempo map), one Undo, the report after it and from File › Last Import or Export Report…, a type 0 split, File › Open… on a `.mid`.
- [ ] MIDI and MIDI Clip export: "What won't transfer" follows the options; the replace question (Cancel default); a place inside the project refused; Cancel leaves no file.
- [ ] Song info in Project Settings (Undo, save and reopen); typing a region's name.
- [ ] Export for Another DAW: shared format rows with Bounce; numbered folder on a second export; `About these files.txt`; muted and unticked strips; a pickup; Add the stems; Cancel mid-export; waiting for a running import.
- [ ] Export DAWproject: region gain in "What won't transfer"; the name in the save panel; replace asks first; waits for a bounce.
- [ ] Import a DAWproject from VizDAW (the return prompt, New Project); a missing instrument's placeholder; Cancel closes the new window; Song info round trip; a VCA's moves add no instrument track.
- [ ] Merge: the return prompt names the open match; "Merge DAWproject" sheet with "Reading the file…", the counts line, two conflicts; Cancel and Esc change nothing; Merge brings the edits ("Drums" keeps its level, "Keys" gets an inactive "Returned <today>" take), opens the Merge Report, one Undo restores all.
- [ ] Merge with no project open asks for the project; merging the same file again adds nothing; while a merge waits, Convert, rate change, format change and Relink are off with "Finish or cancel the DAWproject merge waiting to be confirmed first.", and each export waits.
- [ ] `Other.dawproject` (not from VizDAW) opens as a new project with no prompt; a file whose export ID was lost lists the project's exports.

## Plug-in editor windows (H2)

- [ ] AU Cocoa editor under the 37-pt header at its size; AU v3 "Loading…" then the editor; a slow AU v3 attaches later, closing during open leaves nothing, a silent unit shows the generic view after 3 s.
- [ ] Multi-Q CLAP editor (single click on the EQ slot; bypass only bypasses); the SDK VST3 `again` editor.
- [ ] Knob turn then Undo: one step per drag ("Change Plugin Parameter"); header Bypass lit and the slot dot follows; Learn disabled with its help.
- [ ] Keys: plain keys go to the plug-in (Space does not play), ⌘ keys to the menu; ⌘W closes the window with the editor focused; a new window takes the keyboard.
- [ ] Resizable editor follows a drag without jitter, a fixed one can't resize; a self-resizing CLAP grows its window.
- [ ] Close during another editor's Busy open; quit with two windows open (no crash, both reopen).
- [ ] Parameter list alignment, thumb at both ends, many parameters scroll, monospaced values.
- [ ] Real double-clicks: insert opens without a drag; instrument slot opens; Built-in Synth slot opens the sound window (300×200, radio glyphs, caption wraps).

## Transport bar and recording (8b)

- [ ] Record Options: the caret stays in a field when Record is pressed; a refused Record leaves the popover open; Record from the Mixer window uses the current punch, not the draft.
- [ ] The five field labels fit with the real font; type, Tab through them in order, refusal texts; U opens » at a folded field; the popover keys (open without the mouse, Esc drops the draft).
- [ ] The bar at 1600 pt: nothing folds.
- [ ] Click menu: check mark, Level › and Mode ›; bar and » agree.
- [ ] DSP Load digits change without moving the row; device text shows name and rate.
- [ ] Step menu shows its keys; no key fires twice; with step entry off, `.` is Next Marker, `2` goes to locator 2, Delete deletes; ⌥⇧ digit clears a locator (US layout).
- [ ] MIDI File export with the punch and edit fields in use; Overdub/Replace, MIDI passes as takes, two loop passes.

## Inputs, meters, waveforms, markers (8c)

- [ ] Input row truncation (ellipsis before the badge); the strip's input picker anchoring; the picker in a short window (note unreachable entries; backlog gap).
- [ ] The meter hides on short rows and returns.
- [ ] Gain and fades shape the waveform; the crossfade X; offline hatching and the "Offline" pill, gone after relinking.
- [ ] No blink or hitch scrolling a long region and project.
- [ ] A long hand-recorded take (minutes, several tracks): no UI lag, playhead on time, control channel answers.
- [ ] Markers: "Go to <name>" tooltip; a failed move shows "Go to Marker Failed:" with the reason.

## Settings window (9a)

Checked 2026-11-25 on the Mac (debug bundle, offline device): ⌘, opens one
Settings window; the nine tabs wrap to two rows in 500 pt; the Audio,
Recording and Automation tabs match Swift's forms, nothing clipped, each
window height fits its tab. Still open:

- [ ] The red problem line while recording (change the buffer size during a take).
- [ ] The round-trip caption changes to "Round trip … ms" once a track's input opens.
- [ ] Steppers at their ends (count-in 4, pre-roll 8, return time 2000 ms) and the disabled buttons.

## Themes (9b)

Checked 2026-11-26 on the Mac with the app's offscreen screenshots (screen
locked): the document window (transport bar, track headers, arrangement,
panes) in all six themes and the Mix view in Console and Flat take the
theme, nothing unreadable seen. Still open, on an unlocked screen:

- [ ] The Settings window's Appearance tab: the six rows and their swatches, the switch, the caption; height 300 fits (and with the red problem line).
- [ ] A plug-in window header and the Mixer window in each theme.
- [ ] Every look row above in each of the six themes (monitor tints, waveform colour, region fill, Markers colours, group tag and VCA strip).
- [ ] Menus, popovers (record options, input picker, » overflow) and a sheet card in a dark and a light theme.

## Keys, Editing, Plug-ins, Customize sheet (9c)

Not yet seen on screen. Open, on an unlocked Mac:

- [ ] Settings › Keys at 720×520: rows grouped by menu, changed keys in accent, the recorder's "Record" / "Press a key…", the Replace line, Save… and Load… panels.
- [ ] Recording ⌘W and ⌘Q through the real macOS menu path (the interceptor should take them).
- [ ] Settings › Editing: the height (600, estimated) fits; root and scale disable; the "Choose External Sample Editor" panel title.
- [ ] Settings › Plug-ins with the bundled Multi-Q: the switch, the picker, "(not available)" for a missing choice, the Replace question.
- [ ] The Customize Transport Bar sheet (760 wide): chips in the bar's faces, the palette grid, drag onto, along and off, the inspector's size, command and label, in a dark and a light theme.
