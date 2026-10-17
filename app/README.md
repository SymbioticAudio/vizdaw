# VizDAW app (SwiftUI shell)

A minimal macOS 14+ app over the Rust core (`crates/core-ffi`, UniFFI).
It builds with the Command Line Tools alone; no Xcode project.

## Build and run

```bash
scripts/build-ffi.sh              # Rust static library + generated Swift bindings
scripts/swift.sh run VizDAW       # opens a window (add --demo to start with the demo)
scripts/swift.sh test             # Swift Testing suite (needs build-ffi.sh first)
scripts/make-app.sh               # dist/VizDAW.app, ad-hoc signed
```

`scripts/swift.sh` adds `--build-system native` and the Swift Testing
framework paths that the Command Line Tools need. With full Xcode installed
it runs plain `swift`.

Launch arguments: `--demo` opens the demo project; a path ending in
`.vizdaw` opens that project; `--no-scan` skips the plugin scan at launch;
`--smoke` plays the demo for 3 s with the mixer open, puts Apple's AUDelay
on the first strip and opens its window, prints the transport, the mixer
and the editor size, and exits 0 when all of them worked. `--smoke` uses a
temporary support folder and never scans.

## Plugins

- The catalog lives in `~/Library/Application Support/VizDAW/plugins.json`
  (`VIZDAW_SUPPORT_DIR` moves the whole folder, crash markers included).
  At launch VizDAW scans new or changed AU and CLAP plugins in the
  background with `vizdaw-scan` (next to the app's executable in the
  bundle; `scripts/swift.sh` points `VIZDAW_SCANNER` at
  `target/ffi/vizdaw-scan`).
- Window > Plugin Manager (⌥⌘P) lists every plugin with its status;
  disable one that crashes VizDAW there. Window > Mixer (⌥⌘M) shows the
  focused document's mixer.
- The built-in synth plays through its strip's constant-power pan, so it
  is 3 dB quieter than in 2c (3a ruling).
- Muting a strip also silences its pre-fader sends (plan 3c ruling).

## Editors (plan 4a)

- The document window shows the arrangement (the selected sequence's
  lanes and their blocks) over a detail pane. The piano roll (plan 4b)
  is built; the other detail editors arrive in plans 4c-4e and until
  then the pane names its target and event count.
  Cmd-1 to Cmd-4 choose piano roll, list, Strip Chart or drum grid;
  Option-Cmd-1 to Option-Cmd-4 open one in its own window.
- Blocks follow the toolbar's rule (silence gap, fixed bars, whole lane);
  a reference is always its own purple block named after its sequence.
  Drag moves, Option-drag copies, dragging onto another lane merges; G
  toggles snap; Escape cancels a drag; a drag in the ruler selects a time
  range, a click there locates.
- Space and Return are Transport menu shortcuts, so every text entry the
  editors add (lane name, sequence name, comment, keyboard slot, reference
  parameters) switches them off while it is open.
- The editor layout (detail pane, editor windows and their frames, the
  sidebar order, block rule, light mode, snap, zoom) is saved in the
  project's `ui.json` when you save; it is not an undo step.

`VIZDAW_AUDIO=offline` renders through the offline backend in real time
(no sound, device label "Offline"). Never set it for normal use.

Generated files (not in git): `app/Sources/VizDAWCore/vizdaw_ffi.swift`,
`app/Sources/vizdaw_ffiFFI/include/`, `target/ffi/`, `dist/`.

## Manual smoke test

1. `scripts/build-ffi.sh && scripts/swift.sh run VizDAW`. A window appears
   in front with a menu bar (File, Edit, Editors, Sequence, Transport).
2. File > Create Demo Project. A second window opens; the sidebar shows
   Song under "Top-level" and Beat (1) and Groove (2) under "Used as
   reference".
3. Song is selected; the arrangement shows the lanes Groove and Chords. Press
   Space: the demo sounds through the speakers, the position display
   advances (bar.beat.unit and m:ss.fff), the tempo slows over the last two
   bars, and playback stops by itself at the end.
4. Press Space again and, while it plays, click M in the Chords lane header: the
   chords drop out within a moment. Edit > Undo Set Lane Mute/Solo/Arm
   brings them back.
5. Double-click Beat in the sidebar, type `Kit`, press Return; the title
   gains " — Edited". Typing a space in the name field types a space and
   does not start playback.
6. File > Save As… `~/Desktop/demo.vizdaw`, close the window (Cmd-W),
   File > Open… and choose `~/Desktop/demo.vizdaw`: the sequence is still
   called Kit. Choosing a folder without `.vizdaw` shows an alert.
7. Unplug or switch the output device during playback: within a moment
   playback continues on the new device from the same position (the UI may
   pause briefly); if no output device is left, an alert appears.
8. `scripts/make-app.sh`, then `open dist/VizDAW.app`; double-click
   `~/Desktop/demo.vizdaw` in Finder: it opens in VizDAW.

### Plugins (plan 3c)

Use `dist/VizDAW.app` from step 8 and your own plugins (for example
Diva, a CLAP effect, a reverb, a compressor with a sidechain input and a
multi-output drum instrument).

9. Launch: within a minute the scan finishes (Window > Plugin Manager
   shows "Scanning n of m…" until then). Every installed plugin is listed
   with name, vendor, format, version, type and status. The search field
   and the AU/CLAP filter narrow the table.
10. Open the demo, then Window > Mixer: strips "Built-in Synth" and
    "Master", each with a fader, pan, M/S, an output menu and a meter that
    moves while playing. The toolbar shows the DSP load.
11. Mixer toolbar > Instruments > "Built-in Piano: MIDI → Plugin…": the
    browser lists instruments grouped by vendor; pick Diva. A "Built-in
    Piano" instrument strip appears; its slot turns green once loaded.
    Play Song: the chords sound through Diva. (Instruments > New Plugin
    Instrument… adds a destination of its own.) Double-click the slot: a window
    "Built-in Piano — Diva" shows Diva's own editor. Turning a knob in it and then
    Edit > Undo returns the knob (one undo step per drag). Closing the
    window does not remove the plugin.
12. On a strip, "+": add a CLAP effect and an AU effect. Drag the second
    above the first: the order changes. Bypass one from its slot menu
    and from its window's header. Remove one, then Undo: it comes back
    with the settings it had.
13. "Add Bus", rename it "Verb" (double-click its name), add a reverb to
    it, add a send from the "Built-in Piano" strip to Verb; drag the send level and
    toggle Pre: the reverb follows. Solo that strip: the bus still
    sounds, other strips go dim and silent.
14. Sidechain: put a compressor with a sidechain input on a pad strip,
    choose the kick's strip in its Sidechain menu, play: the pad ducks on
    the kick.
15. Multi-output: with a multi-output instrument, the strip's name menu
    offers "Add Outputs"; add one: a new strip plays that output.
16. Tempo sync: an AU delay set to a note value follows the song's tempo
    (it slows over the last two bars of Song).
17. Plugin Manager: with the project playing, turn the CLAP effect off.
    At once, in every open project, its slot turns into the dimmed
    placeholder "Disabled in Plugin Manager" (an open plugin window for
    it shows no editor) and audio passes through. Turn it back on: the
    slot reloads with the settings it had. "Rescan All" scans every
    plugin; a crashing plugin shows "Blocklisted: crashed (signal 11)" and
    "Clear Blocklist" scans it again.
18. Save, leave two plugin windows open, close the project and reopen
    it: both windows reopen.
19. Safe mode: open a project with a heavy plugin and quit VizDAW with
    `kill -9` while it loads (Activity Monitor > Force Quit works too).
    Reopen the project: "VizDAW quit unexpectedly while this project was
    open, while loading ‹plugin›" offers Open with Plugins Off / Open
    Normally / Cancel / Disable ‹plugin›. With plugins off, every slot is
    dimmed "Plugins off (safe mode)"; a slot's Load and a strip's "Load
    All" load them. Opening with Shift held asks the same question.
20. Autosave: edit a saved project, wait 2 minutes (or load a plugin),
    then `kill -9` VizDAW. Reopen: "has an autosave newer than its last
    save" offers Restore Autosave / Open Last Saved; restoring shows the
    edit and the title says Edited.

### Editors (plan 4a)

Use the demo project (File > Create Demo Project; its window is titled
"Untitled" until saved) unless a step says otherwise.

21. Song shows two lane rows. Groove holds two purple "Groove" reference
    blocks (bars 1-4 and 5-8); Chords one blue block with small note dashes
    (bars 1-12). The ruler numbers bars 1, 2, 3...; with Cmd-scroll or a
    pinch the view zooms around the pointer and the bar labels thin out
    when zoomed far out.
22. Playhead: press Space. The red playhead glides smoothly (no visible
    steps), slows over the last two bars, and when it reaches the right
    edge the view jumps one page. Turn Follow off: the view stays put.
    Stop: the playhead stays where playback stopped.
23. Ruler: drag in the ruler from bar 2 to bar 4 with any tool chosen: the
    range is shaded (snapped to beats) in the ruler and across both lanes.
    Click in the ruler: the position display jumps there.
24. Drag feel: press on the second Groove block and move 2 points: nothing
    moves. Drag it one bar right: it follows the pointer (and, while
    playing, the music follows within a moment). Press Escape before
    releasing: it snaps back and Edit has no new undo step. Drag again and
    release: Edit > Undo Move Block puts it back in one step. With Snap off
    (G) it moves freely.
25. Option-drag the Chords block one bar right: a copy appears and the
    original stays (Undo Copy Block). Drag the Chords block down onto the
    Groove row: its events join that lane (Undo restores). Drag a block to
    the right edge and hold: the view scrolls and the block keeps following.
26. Lane headers: M, S and R toggle mute, solo and record arm (mute is
    audible while playing); the destination menu lists the instruments;
    double-click a lane name, type, Return renames it; "+ Add Lane" adds
    "Lane 3". Light drawing: Light > On draws outlines only; Automatic
    draws filled blocks for the demo (fewer than 1 500 visible).
27. Blocks menu > 1 Bar: Chords splits into twelve blocks; Whole Lane joins
    them.
28. Sidebar: Groove shows 2 and Beat 1 (uses). Drag Groove above Beat: the
    order sticks after Save and reopen. Context menu > Comment... shows the
    comment under the name; Keyboard Slot... with Option and "a" shows
    Option-A. Duplicate adds "Groove copy". "+" > New from Template > Drum
    Kit adds and selects "Sequence 1" with a drum lane "Drums"; Same Lanes
    as Song copies Song's lanes and tempo without events. Delete... on Beat
    asks first (it is in use) and removes Groove's references to it; Undo
    brings both back.
29. Make a reference: drag in the ruler over bars 1-4, Sequence > Make
    Reference from Selection (Option-Cmd-R): Song gains a new sequence in
    the sidebar and the selected range of every lane becomes one purple
    block. Undo restores the events. A right-click on the Chords block >
    Make Reference does the same for that block.
30. Open a reference: double-click a Groove block: the arrangement shows
    Groove, the sidebar selects it, and "◀ Song" goes back (Cmd-[ too).
31. Reference context menu (right-click a Groove block): Make Private Copy
    makes "Groove copy" and relabels that block only; Flatten replaces the
    block with ordinary events (the drums and bass inline); Re-point To >
    Beat relabels it; Re-point To > Song shows "reference cycle..." in the
    status line and changes nothing; Parameters... (the repeats sheet) with
    Repeats 2 doubles the block, Transpose +5 is audible. Each is one Undo
    step.
32. Detail pane: double-click the Chords block: the pane below shows the
    piano roll (see plan 4b); Cmd-2 switches to List; Option-Cmd-1 opens a "Piano Roll — Untitled" window.
    Option-Cmd-2 also opens a List window. Select Beat in the sidebar, open
    a window on it, then Delete... Beat: the window closes (so does one
    whose lane disappears, for example through Undo). Move and resize one, Save As
    `~/Desktop/editors.vizdaw`, close the project and reopen it: the
    window comes back (now titled "... — editors") at the same place; the
    arrangement keeps its block rule, snap and zoom. Window frames and the
    sidebar order are saved with the project on Save; moving a window alone
    does not mark the project edited.
33. Busy: put an AU v3 instrument with a slow editor on a strip (plan 3c
    steps), open its window and, while it is opening, drag a block and
    release: no alert appears; the block lands (perhaps a moment later) and
    Undo reverts it in one step.
34. Typing never plays: with a project open and stopped, press Space in
    each of these and check that a space is typed and playback does not
    start: the lane name field (double-click a lane name), the sidebar
    rename field, the Comment... editor, the Keyboard Slot... key field,
    and the Parameters... sheet (focus the Repeats stepper, press Space).
    Return in the lane name field commits the name and does not jump to
    the start.
35. VoiceOver (Cmd-F5): the arrangement is a group "Arrangement"; moving
    through it reads blocks such as "Block on Chords, bars 1 to 12, 40
    events" and "Reference to Groove on Groove, bars 1 to 4"; VO-Space on a
    block selects it. The lane headers read "Chords, Built-in Piano"; the
    toolbar controls (tool, Snap, Blocks, Light, Follow, zoom) are labelled.

### Piano roll (plan 4b)

Use the demo project (File > Create Demo Project) unless a step says
otherwise. The audio engine starts on the first Play: until then pressing a
note shows "Notes sound once the audio engine runs: press Play once." once,
so press Space twice (play, stop) before the sound steps.

36. Double-click the Chords block: the detail pane shows the piano roll with
    a toolbar (Arrow/Pencil/Eraser, Keyboard, Lanes, Insert Into, Audio,
    Expression, row-height and zoom buttons), a keyboard on the left with
    its C keys labelled (C4, C5), the ruler, and one three-note chord per
    bar in blue with black-key rows shaded and a line under every C. The
    Keyboard toggle hides the keys (the column stays). Taller Rows /
    Shorter Rows (hover help) and the zoom buttons change the rows and the
    zoom; Cmd-scroll zooms around the pointer; scrolling moves both ways
    and the keyboard follows. Key labels disappear below 8-point rows.
37. Sound: click a note: it is outlined (selected) and sounds while the
    button is held and stops on release. Click a key: it sounds; drag down
    the keyboard: each key plays in turn; release: silence.
38. Cursor zones: over a note's left edge the pointer is an open hand, over
    the middle up-down arrows, over the right edge a resize arrow. Zoom out
    until notes are a few points wide: the zones still work (the middle
    stays reachable, nothing flickers). Drag the middle of a note up two
    rows: it follows in semitones and sounds at each new pitch; release,
    then Edit > Undo Transpose Notes restores it in one step. Drag the left
    edge one quarter right: it moves on the grid (Undo Move Notes). Drag
    the right edge: the length changes on the grid (Undo Resize Notes).
    Option-drag the left edge or the middle: a copy appears (Undo Copy
    Notes). Press Escape during any drag: everything snaps back, the sound
    stops and Edit has no new undo step.
39. Rubber band: with the arrow, drag over empty space around a chord: its
    three notes are selected; drag one of them up: all three move together;
    drag far up: the chord stops when its top note reaches G9 (no
    squashing).
40. Pencil: choose Pencil and click an empty row: a one-grid-step note
    appears and sounds while held; Edit shows Undo Insert Note. Drag right
    from empty space: the new note's length follows the pointer in grid
    steps (one undo step, "Insert Note"). Press G (snap off): the pencil
    paints any length. Eraser: a click deletes a note; dragging across
    notes deletes each one crossed, with one undo step "Undo Erase Notes".
    Select notes and press Delete: they go (one undo step).
41. Several lanes: double-click a Groove block (it opens Groove), then
    double-click its Bass block. Lanes > Drums adds the Drums lane. It has
    no notes of its own (it only references Beat), so use Insert Into >
    Drums and the pencil to add a few notes: they are drawn in a second
    colour (orange) and the Bass notes (blue) are drawn fainter while Drums
    is the active lane. Press a Bass note: "Insert Into" switches back to
    Bass and the Bass notes are drawn on top. Overlapping notes of the two
    lanes: the click picks the one drawn on top. Lanes > Drums again hides
    it (the target's own lane, Bass, cannot be hidden).
42. Playing: Transport > Loop on, Space; drag a chord note up and hold it
    while the loop restarts: the dragged note keeps sounding until you
    release; the music follows the edit; Stop silences everything.
43. Torn window: Option-Cmd-1 opens "Piano Roll — Untitled" on the same
    target. With it in front: Space plays and stops, Cmd-Z undoes (the
    Edit menu names the step), Cmd-2 switches that window to List,
    Option-Cmd-1 opens another window on its target, Cmd-1 to Cmd-4 switch
    that window and Option-Cmd-1 to Option-Cmd-4 open windows from it. Open
    a second project (File > New, then Create Demo Project) with its own
    piano-roll window: while a window of one project is in front, Space,
    Cmd-Z and the shortcuts act on that project only; click the other
    project's window and they move to it. Press and hold a keyboard key in
    a torn window and close it with Cmd-W: no note keeps sounding.
44. Audio: File > Create Demo Project with Audio; press Cmd-1 with nothing
    selected: the piano roll shows Song with the Chords and Loop lanes. Under
    the notes the strip "Audio 1-2" holds two teal regions labelled Loop at
    bars 1 and 5 (silent until sub-project 5). M and S toggle mute and solo
    of the "Audio 1-2" mixer strip (Window > Mixer shows the same, and a
    toggle there shows here); Undo reverts. Drag a region one bar right: it
    moves on the grid (Undo Move Region). The Audio toggle hides the strip.
45. Expression: the pane under the strip shows "Bend ▾" and a centre line.
    Select a chord note and draw across it in the pane: a stepped line
    follows the pointer inside that note only (Undo Draw Expression). "Bend
    ▾" > Registered 74 (Timbre) shows that kind; the eraser removes values
    (Undo Erase Expression). Escape during a stroke removes it. Save As,
    close, reopen: the values and the chosen kind are still there. (The
    built-in instruments are MIDI 1.0 and ignore per-note data; the compile
    report says so.) The Expression toggle hides the pane.
46. Nudge: VizDAW > Settings… (Cmd-,) shows "Nudge in Time" (Step: Edit
    Grid) and "Nudge in Pitch" (Step: Semitones, Amount: 1). Select a chord:
    Option-Right (Editors > Nudge Later) moves it one grid step later,
    Option-Left back, Option-Up a semitone up, Option-Down back. The items
    are disabled with nothing selected. Choose Scale Steps with "Follow the
    sequence's key signature" on: Option-Up turns C-E-G into D-F-A (C
    major). Turn following off, Root D, Scale Dorian: steps follow D
    Dorian. Quit and relaunch: the settings are kept. In a lane name field
    and the Comment... editor Option-arrows move the text cursor, not notes.
47. Busy: with an AU v3 instrument whose editor opens slowly (plan 3c
    steps), start opening it and at once drag a piano-roll note and
    release: no note keeps sounding, and the drag lands (perhaps a moment
    later) as one undo step. If a note-off cannot be delivered the status
    line says "A note may still be sounding — press Stop."; Stop silences it.
48. VoiceOver (Cmd-F5): the piano roll is a group "Piano Roll" whose notes
    read like "Note C4, bar 1 beat 1, velocity 56"; VO-Space selects one.
    The keyboard reads "Keyboard", the strip "Audio Regions" with "Mute
    Audio 1-2" and "Solo Audio 1-2", its regions "Region on Loop, Audio
    1-2", the pane "Expression: Per-Note Pitch Bend".
49. Layout: change Keyboard, row height and zoom, show an extra lane, Save,
    close and reopen: the piano roll and its torn windows come back the
    same.
50. Typing never plays (again): with a project open and stopped, press Space
    in the text fields added or reused by 4b (the lane name field, the
    Comment... editor, the Settings window fields) and check a space is
    typed and playback does not start; Space in the piano roll itself still
    plays and stops.
