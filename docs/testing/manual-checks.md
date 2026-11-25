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

### Transport bar and recording views (GPUI slice 8b)

A GPUI build from `next`. The bar's pieces, the Record Options popover, the
» entries, the Faults popover and the Step and Transport menus are tested
headless and by the control channel (`transport_jumps…` step 7 and
`gpui_transport_bar_click_and_command_buttons`). What's left is how it looks
and feels with the real font and a real keyboard. **Controller** rows Claude
runs on the Mac with screen control; **owner** rows need the owner's Linux
desktop or a real interface. Screenshots go to the owner only when the bar
resembles the approved look.

| Platform | Who | What | Status |
|---|---|---|---|
| macOS | controller | The Record button: red when armed to record, its label changes to Stop Recording while recording, and the readouts after it take their colours | done 2026-11-25 (controller, offline device + fake input) |
| macOS | controller | The Record cluster's summary pill, the separator, the disabled » entries while recording, and the tooltips | done 2026-11-25 (controller, offline device + fake input) |
| macOS | controller | The Record Options popover: anchored under the chevron at its width, layout, steppers, and every field disabled while recording; the caret stays in a field when Record is pressed (and the refused Record leaves the popover open) | done 2026-11-25 (controller, offline device + fake input); layout and greyed punch fields checked, the caret-in-field part still open |
| macOS | controller | Record pressed in the Mixer window while the document window's popover holds a draft records at the current punch, not the draft (Swift does the same) | open |
| macOS | controller | The five field labels (Position, Edit In, Edit Out, Punch In, Punch Out) fit with the real font, and the measured widths leave no clipped or loose label | open |
| macOS | controller | The standard bar at 1440 pt with the real Mac font: nothing folds except possibly Pane Toggles; at 1600 pt nothing folds | done 2026-11-25 (controller, offline device + fake input) at 1440; 1600 not run |
| macOS | controller | Punch In and Out fields: type a value, Tab through the five fields in order, U opens » at a folded field | open |
| macOS | controller | The Click menu: check mark, Level › and Mode › (they open on a click here, not on hover; see the backlog), the bar and » forms agree | open |
| macOS | controller | DSP Load digits change without moving the row, and the Audio Device text shows the device name and rate | open |
| macOS | controller | Command button faces (Mixer, Piano Roll, List, Drum Grid, Loop, Record): label only, greyed when the app cannot run the command yet | done 2026-11-25 (controller, offline device + fake input) |
| macOS | controller | The Faults ⚠ glyph and tint appear when a problem arrives; the popover lists the lines and closes on Esc, a click outside or the button; the lines stay until the document closes (a failed plug-in, an autosave failure) | done 2026-11-25 (controller, offline device + fake input); the line reads the raw engine name ("clipped"), as in Swift (backlog) |
| macOS | controller | The stop-reason line in Faults: unplug an interface mid-recording and read "Recording stopped: the audio device went away; what was recorded is kept." (README 136, at 1218) | open |
| macOS | controller | Transport menu check marks: Overdub, Punch and Click show a mark when on, and the mark follows the bar | done 2026-11-25 (controller, offline device + fake input) (Overdub mark, keys shown) |
| macOS | controller | The native Step menu shows its keys (T, 1–6, ., /, =, 0, ⌫) | open |
| macOS | controller | Step menu: disabled items show no key; no key fires twice (one press, one step) | done 2026-11-25 (controller, offline device + fake input) (disabled items show no key; the double-fire part open) |
| macOS | controller | With step entry off, `.` is Next Marker, `2` goes to locator 2 and Delete deletes the selection (not the Step commands) | open |
| macOS | controller | ⌥⇧ and a digit clears that locator on a US layout (`alt-@` aliases ⌥⇧2); other layouts have no key (from slice 8a) | open |
| macOS | controller | README 130–142: record and monitor, loopback calibration, a separate input device, a MIDI keyboard, loop passes, crash recovery, unplug mid-recording, `state record`, input only while needed, synth measure and record to the end, as far as the bar shows them | open |
| macOS | controller | README 167: the record popover keys (open without the mouse, Esc drops the draft) | open |
| macOS | controller | README 228: the five fields, typing, Tab order and the refusal texts | open |
| macOS | controller | README 262: File › Export › MIDI File… with the bar's punch and edit fields in use | open |
| macOS | controller | README 326–331: Overdub/Replace beside Record, the Record ⌄ popover, MIDI passes as takes, Takes rename and delete, two loop passes | open |
| Linux | owner | The bar on the owner's desktop at 1440 and 1600 px: the Record button colours, the Faults tint, DSP and device text, no clipped label with the system font | open |
| Linux | owner | The Record Options popover opens under its chevron, takes the keyboard, closes on a press outside and on Esc | open |
| Linux | owner | The Step and Transport menus: check marks and keys shown as the platform writes them; Ctrl-digit and plain digit keys do what the Step menu says | open |
| Linux | owner | Record with a real input device (needs host track H3; until then the offline device) and the stop-reason line in Faults | open |

### Inputs, meters, waveforms and markers (GPUI slice 8c)

A GPUI build from `next`. The input model, the header and strip pickers, the
meters, the region waveforms, the growing take and the Markers pane are tested
headless and by the control channel (`gpui_arms_meters_counts_in_and_draws_the_take`,
`gpui_records_audio_loop_takes_and_midi`, `window_views_and_panes_over_the_channel`
step for markers). What's left is how it looks and feels with the real font,
real screen and real devices. **Controller** rows Claude runs on the Mac with
screen control; **owner** rows need the owner's Linux desktop. Linux real
input is host track H3. Screenshots go to the owner only when the header and
the take resemble the approved look.

| Platform | Who | What | Status |
|---|---|---|---|
| macOS | controller | The monitor button's letters and tints (off, on, auto), in the header and in the strip; both strip buttons agree; the tint is readable in every theme | done 2026-11-25 (controller, offline device + fake input) in the header on the dark theme; other themes wait for Settings |
| macOS | controller | The input row's truncation (a long channel or port name ends in an ellipsis, not over the badge) and its badge | open |
| macOS | controller | The input picker: its sections (audio channels, MIDI sources, none), the checked entry, and its anchoring under the header's input row and under the strip's input control; Escape and a click outside close it | done 2026-11-25 (controller, offline device + fake input) (header picker; Escape closes) |
| macOS | controller | The input picker's list cannot scroll in a short window: shrink the window and note which entries cannot be reached (known gap, see the backlog) | open |
| macOS | controller | The meter's fall rate after a loud signal, and the 2 s clip mark (it holds, then clears; a click or a new peak behaves as in Swift) | open |
| macOS | controller | The meter hides on short track rows and returns when the row grows | open |
| macOS | controller | Region waveform colour against each theme's region fill (SVP Light, Cubase Dark, Metro UI, Realistic, SymbioticAudio), selected and muted regions included | open |
| macOS | controller | Region waveforms: mono is one band, stereo is two bands; gain and fades shape the drawing; the crossfade X draws and the earlier region's tail is as the backlog says | done 2026-11-25 (controller, offline device + fake input) for mono and stereo; gain, fades and crossfades open |
| macOS | controller | Zoom in close: `Coarse` rows sharpen into `Ready` ones; after a zoom the waveform is briefly blank, then fills in; `Pending` regions fill in without a flicker | done 2026-11-25 (controller, offline device + fake input); found and fixed a zoom that left waveforms blank until another frame (ffd246d2) |
| macOS | controller | No blink or hitch while scrolling a long region and a long project, vertically and horizontally | open |
| macOS | controller | An offline region: the hatching (0xf0883e at 0.55, a stand-in for Swift's offline colour; the owner may pick another) and the "Offline" pill read clearly on every theme; relinking removes both | open |
| macOS | controller | The take while recording: its colour, the peaks growing in step with the playhead, and no lag in the UI while it grows | done 2026-11-25 (controller, offline device + fake input) |
| macOS | controller | At Stop the take turns into the same waveform on the placed region: compare a screenshot just before and just after Stop; no flash, shift or blank | done 2026-11-25 (controller, offline device + fake input) |
| macOS | controller | Recording speed: rerun `gpui_records_audio_loop_takes_and_midi` and `gpui_arms_meters_counts_in_and_draws_the_take` on an UNLOCKED Mac and note both times (earlier runs were made with the screen locked, so the drawing cost is unproven); record the times here | done 2026-11-25: unlocked, `gpui_records_audio_loop_takes_and_midi` 36.2 s (35.8 s locked; the time is the takes playing in real time: record to the end, three loop passes, a MIDI take), `gpui_arms_meters_counts_in_and_draws_the_take` 4.2 s |
| macOS | controller | Record a long take by hand (several minutes, a few tracks armed) and watch for UI lag, a late playhead or a control channel that times out | open |
| macOS | controller | The Markers pane: row spacing and colours, the name label and the position digits aligned in a column (Swift monospaced), the selected or current row, and the pane's empty state | done 2026-11-25 (controller, offline device + fake input) (ellipsis, right-aligned positions); colours per theme wait for Settings |
| macOS | controller | The Markers pane: hovering a row shows the "Go to <name>" tooltip; a click moves the playhead to the marker (also while playing) | done 2026-11-25 (controller, offline device + fake input) for the click; found and fixed a locate that landed short while a track was armed (84ca0056); tooltip open |
| macOS | controller | The Markers pane with many markers (50 or more): the list scrolls to the last one; a failed move shows "Go to Marker Failed:" with the reason | done 2026-11-25 (controller, offline device + fake input) (61 markers scroll to the last) |
| macOS | controller | A real input device on the Mac: its channel names fill the picker; choosing it while closed shows "Opening the audio input… If macOS asks…" and the microphone prompt appears; after Allow, the meter moves with sound; after Don't Allow, the track says so and nothing crashes | open |
| macOS | controller | A real MIDI source (a keyboard) listed in the picker, chosen on a track; notes light the meter or the track and record; unplugging it leaves the track's choice and shows it offline | open |
| Linux | owner | The monitor button, input row, picker and meter on the owner's desktop with the system font: nothing clipped, tints readable | open |
| Linux | owner | Region waveforms on the owner's desktop: colour against each theme's fill, stereo bands, offline hatching with its pill, no hitch scrolling a long project, the brief blank after a zoom | open |
| Linux | owner | The growing take and its hand-over at Stop look the same just before and after; no UI lag during a long take | open |
| Linux | owner | The Markers pane on the owner's desktop: rows, position digits, the "Go to <name>" tooltip, scrolling many markers | open |
| Linux | owner | The GPUI e2e `gpui_arms_meters_counts_in_and_draws_the_take` and the markers step run green in Linux CI under Xvfb | done 2026-11-25 (CI run 45553218877) |
| Linux | owner | A real input device and a real MIDI source in the picker, with the audio permission text where the system asks (needs host track H3; until then the offline device) | open |
