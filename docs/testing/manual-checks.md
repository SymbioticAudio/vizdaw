# Manual checks (owner)

What the owner checks by hand in the 27.1 app (the GPUI build from `next`),
on macOS and Linux. Only checks that need your ears, your hands on a real
key or device, a second display or your Linux desktop are here. Claude's
own screen-control checks are in `controller-checks.md`; bugs go in
`bugs.md`. The Swift 26.x beta's checks were dropped on 2026-11-25 (the
Swift app is retired; see git history for the old list).

Status: **open**, **pass**, **fail** (with the bug's number).

## Getting the build

- **macOS:** in `~/workspace/vizdaw-next`, run `scripts/make-gpui-app.sh`
  (or `scripts/make-gpui-app.sh release`), then open
  `target/VizDAW-GPUI.app`.
- **Linux:** in `~/vizdaw-next` on the test box, `git pull`, then
  `cargo run -p vizdaw-gpui --release`.

Not in the app yet, so not on this list: Settings › Surfaces and Co-Producer,
user themes, Learn MIDI, the Strip Chart, Drum
Grid and Audio editors, torn-off editor windows, the Plugin Manager and
Media Pool. Their checks come with their slices.

## macOS

| # | What to do | What should happen | Status |
|---|---|---|---|
| M1 | Mixer: group three strips, hold Option and drag one grouped fader | Only that fader moves | open |
| M2 | Make a VCA, put a playing track on it, set the VCA to −6 dB | You hear the track drop by about 6 dB | open |
| M3 | Turn Click on and play over a tempo change and a meter change | A click on every beat, the downbeat higher, following both changes | open |
| M4 | Click › Mode › Record Only, then play, then record | Silent while playing, clicking while recording | open |
| M5 | Click › Level › −24 dB | The click is clearly quieter | open |
| M6 | Bounce with Click on, play the bounced file | No clicks in the file | open |
| M7 | Play something loud into a track's meter, then stop the sound | The meter falls smoothly; the clip mark holds about 2 s, then clears | open |
| M8 | Plug in an audio interface and open a track's input picker | Its channel names are listed | open |
| M9 | Choose one of its inputs | "Opening the audio input… If macOS asks for microphone access, answer its prompt." shows, and macOS asks | open |
| M10 | Answer Allow, then make sound | The track's meter moves | open |
| M11 | On another try (reset the permission in System Settings › Privacy › Microphone), answer Don't Allow | The track says it has no access; nothing crashes | open |
| M12 | Monitor that input (the A / I / – button) and record a take with it | You hear yourself when the mode says so; the take records and plays back | open |
| M13 | Unplug the interface while recording | Recording stops; Faults (⚠) shows "Recording stopped: the audio device went away; what was recorded is kept."; the take is kept | open |
| M14 | Plug in a MIDI keyboard and open a MIDI track's input picker | The keyboard is listed | open |
| M15 | Choose it, play, record | Notes show on the track and record into a take | open |
| M16 | Unplug the keyboard | The track keeps its choice and shows it as offline | open |
| M17 | With a second display at a different scale (2× and 1×), drag a plug-in window from one to the other | The editor stays sharp and the same size | open |
| M18 | Export For Another DAW and Export DAWproject, then open the results in another DAW | The tracks, stems and positions arrive as "What won't transfer" said | open |
| M19 | Settings › Audio with your interface plugged in, then unplugged | Its name is in the Input device list; after unplugging, "<name> (not connected)" stays chosen | open |
| M20 | Change I/O Buffer Size while playing | The new size is heard without a glitch or a stop | open |
| M21 | Settings › Keys: record ⌘W for Go to Locator 2, then ⌘Q for another command | Each key is recorded on its row (no window closes, the app does not quit); the menus show the new keys; Reset All puts them back | open |
| M22 | Settings › Plug-ins › Install Multi-Q for other apps, then install again | It lands in ~/Library/Audio/Plug-Ins/Components and another app sees it; the second time asks to replace | open |
| M23 | Settings › Surfaces: add a real MCU-compatible unit, move fader 1 and press Mute 1, then unplug it | The strip follows, the LEDs light, and unplugging reads "Disconnected" within a second | open |
| M24 | Plug in a host-integration keyboard and choose it in Add Surface | Both ports fill with its "DAW" port; its transport, knob and tempo work | open |
| M25 | Learn a knob to a fader from the mixer's menu, and a pad to Undo from Settings › Keys | The banner shows, then goes; the knob rides the fader; the pad undoes; the MIDI Learn Maps sheet lists the knob, changes its mode, and Undo reverts it | open |
| M26 | Launch with AU, CLAP and VST3 plug-ins installed, then open an insert slot's browser and Window › Plugin Manager | The launch scan finds them (a format shows as AU, CLAP or VST3); the browser lists them and adds one; the Manager lists every plug-in with its status; Reveal in Finder selects the plug-in file | open |
| M27 | Modify › Transpose… over a selected block (README 62-66), then use the panel by hand: every picker with the real mouse, including the same-label ones in two condition rows (each row's Comparison, both Kinds menus, both Destination pickers) and the two Operation pickers; type in a field and press Space and Return; Apply To a Copy +4; Change Values with each shape; Save as Template, Save As, reopen and load it; turn on Exact Mode and drag a block in the arrangement, notes in the Piano Roll and a rubber band; ⌘Z and ⌘S with the panel key; look at the menu bar with the panel key; switch to another app; ⌥←→↑↓ on a selection in the document window | The panel floats above the document windows; each picker opens its own menu and changes only its own row; Space does not play and Return applies only; To a Copy +4 makes a harmonizing copy; the Change shapes change the values as drawn; the template is listed after reopen and runs; each Exact Mode drag snaps back and opens the panel with its amounts, and Apply makes the move; ⌘Z undoes and ⌘S saves the panel's document; the menu bar's items are enabled as for the document; note whether the panel stays on screen in the other app (Swift hid it); the ⌥-arrows nudge the selection | open |

## Linux (your desktop)

| # | What to do | What should happen | Status |
|---|---|---|---|
| L1 | File › Open: pick `project.json` inside a `.vizdaw` folder; also a MIDI, a DAWproject and an AAF file | Each opens | open |
| L2 | Look at the open panels' filters; use Search Folder… and a stems Bounce | The filter label and patterns are right; folder choosers pick only folders | open |
| L3 | Save As, Bounce, MIDI export, interchange report | The suggested name, folder, and the extension added when you type none | open |
| L4 | Save over an existing file | The app asks its own replace question; Replace saves, Cancel leaves the file alone | open |
| L5 | The save-changes, replace and return prompts | Return presses the default button, Esc the cancel one | open |
| L6 | Close every window, then trigger an alert (for example, open a file that fails to open) | The alert shows in its own small window; OK closes it | open |
| L7 | Merge With Existing Project…: pick `project.json` inside a `.vizdaw` folder | It merges into that project | open |
| L8 | The DAWproject return prompt | Return is New Project, Esc is Cancel, Merge With Existing Project… is clickable | open |
| L9 | Open a plug-in editor in the KDE session (XWayland) and in a plain X11 session | It embeds under the header in both | open |
| L10 | The gui test plug-in, a DPF plug-in and a heavy open-source instrument, each as CLAP and VST3 | The editor shows, animates and answers the mouse | open |
| L11 | Move and minimize the document window with a plug-in window open (also opened from the Mixer, and after reopening the project) | The plug-in window goes with it | open |
| L12 | Type into an editor's text field | The plug-in gets the text | open |
| L13 | Ctrl-W while the editor has the keyboard | The plug-in window closes and the plug-in stays on its slot | open |
| L14 | At 2× scale, open an editor; change scale and reopen | It sits under the header and reopens at the new scale | open |
| L15 | Close and quit with editors open (also right after opening one) | No BadWindow exit, no crash | open |
| L16 | Look at the editor area | No transparency or garbage | open |
| L17 | A plug-in with no editor; reopen a project with plug-ins; turn a plug-in off | The parameter list with working sliders; "Loading…" while loading; the reason it's off, not a black area | open |
| L18 | The transport bar at 1440 and 1600 px | The Record button's colours, the Faults tint, DSP and device text; no clipped label | open |
| L19 | The Record Options popover (the chevron beside Record) | It opens under the chevron, takes the keyboard, closes on Esc and on a click outside | open |
| L20 | The Step and Transport menus | Check marks and keys shown; Ctrl-digit and digit keys do what the menu says | open |
| L21 | A track's monitor button, input row, picker and meter | Nothing clipped; tints readable | open |
| L22 | Audio regions in a long project: stereo, an offline file, a zoom, scrolling | Two bands for stereo; the hatching and "Offline" pill; a brief blank after a zoom, then the waveform; no hitch scrolling | open |
| L23 | Record a few minutes with the offline device; compare just before and after Stop | The take looks the same; no lag while it grows | open |
| L24 | The Markers pane with many markers | Rows and digits aligned; "Go to <name>" on hover; a click moves the playhead; it scrolls to the last | open |
| L25 | (After host track H3) a real audio interface and MIDI keyboard | As M8–M16 | waits for H3 |
| L26 | VizDAW › Settings… (Ctrl-,) in the in-window menu bar | The Settings window opens; the Audio, Recording and Automation tabs draw with the system font, nothing clipped | open |
| L27 | Settings › Appearance: try each of the six themes, then turn colour bars off | Every window changes at once; text stays readable with the system font; the colour bars go | open |
| L28 | Settings › Keys: record a new key for a command, then Save… and Load… a keys file | The in-window menu shows the new key and it runs the command; the save and open panels appear; a loaded file's keys take effect | open |
| L29 | View › Customize Transport Bar…: drag an item onto the bar, one along it, one off, then Done | The bar changes as dragged; Cancel instead leaves it as it was | open |
| L30 | Settings › Surfaces on Linux | The empty lines draw; Add Surface… shows "No MIDI sources" and "No MIDI destinations", and Add is disabled | open |
| L31 | The MIDI Learn banner and Maps sheet on Linux | They draw with the system font; Esc cancels | open |
| L32 | The plug-in browser and Plugin Manager on Linux (XWayland) | They draw with the system font and open in a free window; the format filter offers All, CLAP and VST3; Show in Files opens the plug-in's folder | open |
| L33 | Modify › Transpose… on Linux (XWayland, X11 window manager): open the panel, click a field and type with Space and Return, Ctrl+Z from the panel, click the document window and back, then close the project | The panel opens as a free window over its document and stays above it, draws with the system font and takes the keyboard without the transport reacting; Ctrl+Z undoes the panel's document; focus moves between the two windows on click; closing the project closes the panel | open |
