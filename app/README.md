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
`--control PATH` turns on the agent control channel (below); `--smoke`
plays the demo for 3 s with the mixer open, puts Apple's AUDelay
on the first strip and opens its window, prints the transport, the mixer
and the editor size, and exits 0 when all of them worked. `--smoke` uses a
temporary support folder and never scans.

## Plugins

- The catalog lives in `~/Library/Application Support/VizDAW/plugins.json`
  (`VIZDAW_SUPPORT_DIR` moves the whole folder, crash markers included;
  with it set, preferences are kept in memory, not saved).
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
- Keys follow the key bindings (plan 4f): every command, menu item or
  not, has a stable ID and can be bound in VizDAW > Settings > Keys;
  menus show the bound keys. Single unmodified keys drive the transport
  and the modes (Space, S, R, Y, K, Return, comma and period, L, O, I, T,
  G); P, E and U focus the Position, Edit In and Punch In fields; 0-9 go
  to locators, Option-digit sets one and Option-Shift-digit clears it.
  A text field always keeps the keys it types: a letter typed into any
  field is never a shortcut, and while a text field (rename, comment,
  keyboard slot, reference parameters, a panel's field) has the keyboard
  the single-key and Option/Shift transport, locator and mode commands
  are unavailable. A plain key VizDAW does not use goes to the window,
  never to the main menu. In a plugin window plain keys go to the plugin
  (the single-key transport shortcuts do not work while a plugin window
  is in front; Command keys do). While an input method is composing,
  keys go to the text.
- The editor layout (detail pane, editor windows and their frames, the
  sidebar order, block rule, light mode, snap, zoom) is saved in the
  project's `ui.json` when you save; it is not an undo step.

`VIZDAW_AUDIO=offline` renders through the offline backend in real time
(no sound, device label "Offline"). Never set it for normal use.

Generated files (not in git): `app/Sources/VizDAWCore/vizdaw_ffi.swift`,
`app/Sources/vizdaw_ffiFFI/include/`, `target/ffi/`, `dist/`.

## Agent control channel

Off by default. `VizDAW --control PATH` (or `VIZDAW_CONTROL=PATH`) listens
for JSON-lines requests on a Unix socket at PATH (spec
`docs/superpowers/specs/2026-10-18-control-channel-design.md`). The
socket's folder must be private (VizDAW makes it 0700 when missing and
refuses one other users can open); the socket is 0600 and goes when
VizDAW quits. There is no network port. While the channel is on VizDAW
opens no dialog of its own: a call that would needs the answer as a
parameter (`dialog_required` names the dialog and the parameter, for
example `app.open` asks for `restore_autosave` when an autosave exists),
and alerts (app-level and documents' alerts, which open no sheet) are
printed to stderr once each and listed by `state.get windows`; a
document's alert also stays in `state.get document` until it is replaced.
Undo and redo answer `busy` while a plugin call is on the main thread, and
`modify.template` answers `unavailable` when there are no templates. Model
edits and loads refuse tick positions or lengths beyond +-2^40.

`vizctl` drives it from a shell:

```bash
cargo build -p vizctl && scripts/make-app.sh
target/debug/vizctl launch --app dist/VizDAW.app --demo --env VIZDAW_AUDIO=offline
export VIZDAW_CONTROL=/tmp/vizctl-.../ctl/vizdaw.sock   # the socket launch printed
target/debug/vizctl state windows
target/debug/vizctl run transport.play_stop && target/debug/vizctl wait playing
target/debug/vizctl screenshot --out /tmp/play.png
target/debug/vizctl find Play          # element IDs for click, drag and key
target/debug/vizctl key space && target/debug/vizctl wait stopped
target/debug/vizctl quit
```

`launch` runs VizDAW in a throwaway home (its own `HOME`, support folder
(`VIZDAW_SUPPORT_DIR`) and log under `/tmp`, no plugin scan), never in
yours. It passes `-ApplePersistenceIgnoreState YES`, and with
`VIZDAW_SUPPORT_DIR` set the app keeps preferences in memory, so a launched
VizDAW never reads or writes your real preferences. `vizctl --help` lists
every command; `vizctl commands` lists the menu commands by ID. `--timeout`
and `--timeout-ms` must be finite numbers (otherwise exit 2);
`--restore-autosave` takes only `true` or `false`. Exit codes: 0 done, 1
VizDAW answered with an error, 2 bad usage, 3 no connection, launch failure
or timeout. `launch` refuses (exit 3) a `--home` where a VizDAW still
answers, before touching its log, and an existing `--home` that is a link,
belongs to another user or others can write to; without `--home` it makes a
new private folder in `/tmp`. Clicks and keys go into
VizDAW's own windows: VizDAW need not be the active app and the pointer does
not move. A `ui.click` on an element is refused (`no_element`) when the
element's frame is non-finite or empty or its centre lies outside the
window; only mouse events posted by the channel are accepted as first clicks.
A key runs what the current key bindings give it (plan 4f): its command
on the window's document (a menu command's too, with `dialog_required`
for one that would open a dialog), a sequence's keyboard slot, or, typed
into a text field or unbound, the window's own handling.

Agents implementing or reviewing VizDAW may run checklist steps this way
against a throwaway home, read state and attach screenshots; computer-use
tools stay banned. A screenshot does not replace steps that need ears,
VoiceOver or real devices.

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
    piano roll (see plan 4b); Cmd-2 switches to List; Option-Cmd-1 opens a
    "Piano Roll — Untitled" window. Option-Cmd-2 also opens a List window.
    Select Beat in the sidebar, open a window on it, then Delete... Beat:
    the window closes (so does one whose lane disappears, for example
    through Undo). Move and resize one, Save As `~/Desktop/editors.vizdaw`,
    close the project and reopen it: the window comes back (now titled "...
    — editors") at the same place; the arrangement keeps its block rule,
    snap and zoom. Window frames and the sidebar order are saved with the
    project on Save; moving a window alone does not mark the project edited.
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
    bar in orange (Chords is Song's second lane, after Groove; lane colours
    follow the lane's position) with black-key rows shaded and a line under
    every C. The Keyboard toggle hides the keys (the column stays). Taller
    Rows / Shorter Rows (hover help) and the zoom buttons change the rows
    and the zoom; Cmd-scroll zooms around the pointer; scrolling moves both
    ways and the keyboard follows. Key labels disappear below 8-point rows.
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
    Drums and the pencil to add a few notes: they are drawn in blue (Drums
    is Groove's first lane) and the Bass notes (orange, its second) are
    drawn fainter while Drums is the active lane. Press a Bass note: "Insert
    Into" switches back to Bass and the Bass notes are drawn on top.
    Overlapping notes of the two lanes: the click picks the one drawn on
    top. Lanes > Drums again hides it (the target's own lane, Bass, cannot
    be hidden).
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
    Dorian. Quit and relaunch: the settings are kept (not in a `vizctl`
    home or with `VIZDAW_SUPPORT_DIR` set: preferences then stay in
    memory). In a lane name field
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

### List editor and Select and Modify (plan 4c)

Use the demo project (File > Create Demo Project; its window is titled
"Untitled" until saved) unless a step says otherwise. In the demo, Song's
Chords lane holds 36 notes (three per bar, bars 1-12, velocity 56), a
Program and a Controller 7 = 100 event at 1.1.0 and two Pitch Bend events in
bar 12 (the block has 40 events); Song's tempo map is 120 BPM at 1.1.0, a
120 ramp at 11.1.0 and 90 at 13.1.0. The status line is the strip under the
editors (and at the bottom of the panel).

51. List: double-click the Chords block and press Cmd-2: the detail pane
    shows an "Event List" table with the columns Position, Length, Kind,
    Destination, Data 1, Data 2 and Mute (a Lane column appears only when
    the list covers more than one lane) and "40 rows" in its bar. The first
    rows at 1.1.0 are Program (Data 1 0), Controller (7, 100) and three
    "Note" rows (pitch name such as C4 in Data 1, velocity 56 in Data 2).
    Click a row, then use Up and Down: the cursor moves; the selected rows
    are the document's selection (Cmd-1 shows those notes selected in the
    piano roll; Cmd-2 comes back).
52. Typing a position: click a note row in bar 1 and press Return: its
    Position cell edits. Type 3 and press Return: the note moves to 3.1.0,
    the list scrolls to it and keeps it selected (the cursor follows the
    moved event, not the old row) and Edit shows "Undo Move Events". On
    another note press Return, type ".3" and Return: only its beat changes;
    type "..480": only its unit changes. Type "abc": the status line says
    "abc" is not a position (type a bar, bar.beat or bar.beat.unit) and the
    cell shows the old value. Return on the Program row works the same way.
53. Cells and keys: click a note row, press Return (Position edits), then
    Tab: the cursor moves through Length, Destination, Data 1 and Data 2
    (Tab skips read-only cells; Shift-Tab goes back). In Data 1 type D4, in
    Data 2 type 100: each changed cell is one undo step "Edit Event" and the
    list stays on the row; a cell you leave unchanged adds no step. Type
    "H9" in Data 1: the status line explains the refusal. Escape in any cell
    leaves it unchanged with no undo step and the table keeps the keyboard.
    Double-click a cell edits it too; Delete removes the selected rows
    ("Undo Delete Events").
54. Typing never plays: while a cell is edited, Space types a space and the
    transport does not start, and Return commits the cell without the
    position display jumping to the start. With the list focused but not
    editing, Return edits the selected row (it does not return to the
    start) and Space plays and stops. Click the arrangement: Return returns
    to the start again.
55. Insert: with a note row selected click "Insert Note": a C4 quarter note
    (velocity 100) appears at the selected row's position and becomes the
    selection ("Undo Insert Events"). With no row selected it goes at the
    stopped playhead if that lies in the block, else at the block's start.
    Open the arrow of the Insert button, choose Controller: a Controller 7
    = 64 appears and the button now reads "Insert Controller". Press Cmd-I
    (Editors > Insert Event): another controller is added at the new
    selected row's position. Insert's menu offers Note, Controller, Pitch
    Bend, Channel Pressure, Poly Pressure, Program, SysEx and Text and, in
    a block's list, no Reference, Audio Region, Tempo, Meter or Marker.
    While a cell is edited the Cmd-I item is greyed.
56. Optional columns: Columns > turn on End, Release Velocity and SMPTE and
    pick SMPTE Frame Rate 25 fps (Length is on by default and can be turned
    off): End appears after Position, Release before Mute and SMPTE before
    Mute. A note's End is its Position plus its Length. Type 00:00:02:00 in
    the SMPTE cell of a bar-1 note: it moves to 2.1.0 (two seconds at 120
    BPM; "Undo Move Events"). Type 2.1.0 in a note's End cell: its length
    changes ("Undo Edit Event"); an End before the note's Position is
    refused in the status line. Type 64 in a Release cell: it changes.
    Quit and relaunch: the column choices and the frame rate are kept (not
    in a `vizctl` home or with `VIZDAW_SUPPORT_DIR` set).
57. Filters: Show lists every kind (Note, Controller, ... Marker) and the
    destinations Built-in Piano, Built-in Bass and Built-in Drums. Turn off
    Controller: the Controller row disappears and the row count drops; turn
    off Built-in Piano (the Chords lane's destination): every row
    disappears; Show Everything restores them. Option-Cmd-2 opens a second
    list window on the same block: it shows everything (the filter is per
    list). In the first list turn off Controller and choose Apply to All
    Lists: the other list hides Controller too. Choose Save as Default
    (this keeps hidden kinds only, not destinations), then File > Create
    Demo Project and press Cmd-2 on its Chords block: that list starts with
    Controller hidden. Restore with Show Everything and Save as Default.
58. Whole sequence: click Song in the sidebar, click empty arrangement
    space (nothing selected) and press Cmd-2: at 1.1.0 the list shows a
    Tempo row (Data 1 120.00, Data 2 jump) and a Meter and Key row (4/4, C
    major), then every lane's rows with a Lane column (Groove, Chords);
    Tempo rows for 11.1.0 (120.00, ramp) and 13.1.0 (90.00, jump) follow in
    position order. Select a note row in bar 3 and choose Insert > Marker
    (the menu now also offers Tempo, Meter and Marker): "Marker 1" appears
    at 3.1.0 (Data 2 holds its name, editable; Undo Set Markers). Type 100
    in the first Tempo row's Data 1: the tempo changes ("Undo Set Tempo")
    and Play is slower.
59. Tempo, meter and marker refusals: the first Tempo and Meter rows'
    Position cells do not edit (Return does nothing); press Delete on the
    first Tempo row: the status says "The first tempo change stays." On
    the 13.1.0 tempo row type 11 in Position: "A tempo change is already
    at that position." Select the 11.1.0 Tempo row and choose Insert >
    Meter: a Meter row appears at 11.1.0; type 11.2 in its Position: "Meter
    changes must start on a barline."; type 12: it moves to 12.1.0. Select
    the first Tempo row (1.1.0) and choose Insert > Meter: "There is
    already a meter change at the next barline." Clear a marker's Data 2:
    "A marker needs a name."
60. References: Song's list shows two "Reference" rows (Data 1 "Groove",
    Data 2 "×1 0" and "×1 +5"). Type "×2 +12" (or "x2 +12") in the first
    one's Data 2: that use repeats twice an octave up ("Undo Edit Event");
    "abc" is refused with a hint. Select a Reference row and press Cmd-Down
    (or double-click its Kind or Data 1 cell): the list and the
    arrangement show Groove; Sequence > Back to Parent Sequence (Cmd-[)
    returns the arrangement to Song.
61. MIDI In (needs a MIDI keyboard connected): in a Chords list turn on MIDI
    In and select a note row, then play a key: the row takes the played
    pitch and velocity ("Undo Edit Event"). Edit a Data 1 cell (Return,
    Tab until Data 1) and play: the note name appears in the cell, selected,
    and Return commits it; in a Data 2 cell the played velocity (0-127)
    appears. With two projects open, play: only the front project's list
    receives (the other's MIDI In stays on but ignores the notes). Press
    Cmd-1 (the pane becomes the piano roll) and play: nothing is entered;
    Cmd-2 again shows a list whose MIDI In is off. Turn MIDI In off: playing
    changes nothing. If input cannot start the status line says "MIDI input
    couldn't start." With no keyboard, MIDI In does nothing.
62. Modify menu and panel: Modify lists Select by Rule... (Ctrl-Cmd-S),
    Transpose... (Ctrl-Cmd-T), Quantize... (Ctrl-Cmd-U), Change Values...
    (Ctrl-Cmd-M), Move... (Ctrl-Cmd-V), Modify Again (Ctrl-Cmd-R),
    Templates and Exact Mode (Ctrl-Cmd-E). Select the Chords block and
    choose Transpose...: the floating panel "Select and Modify — Untitled"
    opens with Task "Transpose", the Modify switch on and a "semitones"
    field. Type 12 and press Apply: the chords move up an octave and Edit
    shows "Undo Transpose". Choose each other menu item: the panel's Task
    follows it. Choose "To a Copy", Transpose, type 4, Apply: a harmonizing
    copy a third higher appears and the originals stay. The panel floats
    above the document windows; note that it also floats above the windows
    of other open projects (each project has its own panel) and hides when
    VizDAW is not the active app.
63. Select half: select the Chords block, Modify > Select by Rule... (Ctrl-
    Cmd-S): Select is on and Modify off, Kind is Note and Position is inside
    the block. Add Condition > Pitch (it starts inside C4 to C5): set "to"
    to E4 and press Apply: only notes from C4 to E4 are selected, in the
    list and the piano roll, and Edit gains no undo step. Set Result to
    "Refine the Selection", change the Pitch comparison to "is" (C4 to C4)
    and Apply: only the C4 notes stay selected. Set Result to "Replace the
    Selection", click empty arrangement space (nothing selected) and
    Apply: the status says "Select lanes to search first: drag in a ruler
    or select a block." Set Result to "Refine the Selection" and Apply
    again with nothing selected: that message does not appear (Refine skips
    the check). The minus button removes a condition; Match "Any Condition"
    selects notes meeting either of two conditions.
64. Shapes: select the Chords block, Modify > Change Values... (Ctrl-Cmd-M):
    it starts as Velocity, Add, Constant. Choose Operation "Set to", Amount
    "Along a Line", from 20 to 120 and Apply: the list shows velocities
    rising across the selection ("Undo Change Velocity"); the Curve picker
    offers Straight, Ease In, Ease Out and S-Curve. Choose Amount
    "Oscillating", Wave Square, low 30, high 110, period 8.0 and Apply: the
    velocities alternate bar by bar between 110 and 30. Type "abc" in a
    field: it turns red and Apply says "Fix the fields marked in red
    first." Modify... in the list bar opens the panel on Change Values with
    that list's hidden kinds left alone.
65. Templates and Modify Again: choose Transpose..., type 12, type "Octave
    Up" in Template name and Save as Template: the panel's Templates menu
    lists Load "Octave Up" and Delete "Octave Up", and Modify > Templates
    lists "Octave Up". Save As, close and reopen the project: the template
    is still there. Select other notes and choose Modify > Templates >
    Octave Up: it runs at once ("Undo Transpose"). Modify > Modify Again now
    reads "Modify Again: Transpose" (Ctrl-Cmd-R) and repeats the last
    modification on the current selection, in place even after a "To a
    Copy"; the panel's Modify Again button shows "Modify Again changes the
    selection in place." on hover and is disabled in a project with no
    modification yet. Delete "Octave Up" in the panel: Templates in the
    Modify menu is greyed again. Saving a template with no name says "Name
    the template first."
66. Exact mode: open the piano roll (Cmd-1) on Chords and choose Modify >
    Exact Mode (Ctrl-Cmd-E; the toolbar's Exact toggle shows it too). Drag a
    chord note's left edge one beat right: it follows the pointer and, on
    release, goes back while the panel opens on Move with "by" 1.0 and "Apply
    acts on the dragged events."; Apply moves it as one undo step. A
    middle-zone drag straight up three rows opens Transpose with semitones
    3; a diagonal middle drag opens whichever axis is stronger with a
    notice "The vertical part of the drag was left out." or "The horizontal
    part of the drag was left out."; an end-zone drag opens Change Values
    (Length, Add); an Option-drag of a note's start or middle opens
    Move/Transpose with "To a Copy" chosen (end-zone drags resize and ignore
    Option); a rubber band opens Select by Rule with Kind, Position and
    Pitch conditions (Shift-band: Result "Add to the Selection"). Drag a
    Chords block one bar right in the arrangement: Move opens with "by" 4.0;
    Option-drag it one bar right in its own lane: Move opens with "by" 4.0
    and "To a Copy" chosen, and Apply leaves the original and adds the copy
    as one undo step. An Option-drag of a block into another lane copies at
    once, as usual. Region drags (Audio project), a drag into another lane,
    pencil clicks, the eraser and expression strokes still apply as usual.
    Escape during a drag opens nothing and leaves no undo step. Turn Exact
    Mode off: drags apply again.
67. Keyboard and VoiceOver: with the panel in front, Cmd-Z undoes in its
    document, Space in a panel field types a space and playback does not
    start, and Space with no field focused does not play; Space and Return in
    the Template name and condition fields type. Close the panel and reopen
    it with Ctrl-Cmd-M: it comes back where it was. With a torn list or
    piano roll window in front, the Modify menu and Modify Again act on that
    window's document. With VoiceOver (Cmd-F5) the list is a table "Event
    List" with the column names as headers and rows that read like "Note
    C4, bar 1 beat 1, velocity 56", "Controller, bar 1 beat 1, 7, 100" or
    "Tempo, bar 1 beat 1, 120.00, jump" (with the row's own values); the
    panel's controls read their labels (Task, Result, Match, Operation,
    Apply, Comparison, Amount, Curve, Wave, "Pitch from", "Pitch to",
    Remove Condition, Template name).


### Strip Chart and automation (plan 4d)

Use the demo project (File > Create Demo Project) unless a step says
otherwise. Its built-in synth plays through the "Built-in Synth" strip; Song
holds Groove and Chords. Turn snap on (the default, quarters) unless a step
says otherwise. "The level" means what you hear and the master meter.

68. Strip Chart: double-click the Chords block and press Cmd-3: the detail
    pane shows "Strip Chart" with one row titled "Velocity ▾" and a bar
    (Mode: Draw, Modify, Erase; Shape; Step; Rows: 1). The chords' 36
    velocities show as bars at their starts; selected notes are accented.
    Rows + makes a second row "Controller 7 (Volume) ▾" showing the
    Controller 7 = 100 event at 1.1.0 as a held step.
69. Draw: in the velocity row drag from bar 1 near the bottom to bar 4 near
    the top: the bars follow the pointer while dragging and the notes of bars
    1–4 rise in a straight line; Edit > Undo "Change Velocity" restores them
    in one step. A drag from right to left works the same way. A single
    click sets the one note at that grid line. A drag that leaves the row
    above or below pins its end at the top or bottom.
70. Shapes: in the controller row choose Shape Curved (Ease In), Flat,
    Random, Free and Oscillator (Sine, Musical 1/4; then Clock 250 ms; then
    Bars 1 with a meter change added in the list) and draw over bars 5–8
    each time: the steps follow the shape; Free follows the pointer; an
    Oscillator's period stays a quarter, 250 ms, or one bar whatever the
    bar's length; Step 1/64 gives a denser line than 1/8. Each drawing
    replaces the controller events in its range ("Draw Controllers").
71. Modify: Mode Modify, Operation Scale: drawing a flat line at the top
    over bars 1–4 doubles those velocities, at the middle leaves them,
    at the bottom zeroes them; Add at the middle changes nothing; Max Limit
    and Min Limit clip; Compress drawn as a rectangle maps the lowest value
    in its range to its bottom and the highest to its top; in a Length row,
    Legato makes each note end at the next; in the controller row Thin
    (Step 1/4) leaves one event per quarter and Thicken fills between.
    Option at the press limits the change to the selected notes.
72. Erase: Mode Erase in the controller row deletes the controller events
    under the drag; in the velocity row the status says "Erase works on
    controller, program and automation rows." and nothing changes. Escape
    during any drag leaves no undo step.
73. Automation: add a lane "Auto" to Song (lane header menu), select it,
    press Cmd-3; on the row title choose Automation > Built-in Synth >
    Fader · Built-in Synth; draw a straight line from bar 1 at the bottom to
    bar 5 at about two thirds up. Play from bar 1: the chords fade in
    smoothly (no zipper noise, no clicks), reaching about unity at bar 5;
    the list (Cmd-2 on Auto) shows two Automation rows "Fader · strip …"
    with "0.0%" and about "63%".
74. Stop and move: stop in the middle of the fade: the level stays where it
    stopped; start an auditioned note (piano keyboard) to hear it. Move the
    mixer's Built-in Synth fader: the level follows the fader. Play again
    from bar 1: automation takes over at once. While playing, moving the
    fader does not hold (automation wins). Delete the Auto lane's points and
    play: the fader's own value plays.
    With an insert whose parameter is automated, bypass it during playback
    and un-bypass it: the parameter jumps to where the automation is now,
    not to stale values.
75. Mute, solo, pan and sends: add rows Mute · Built-in Synth and Pan ·
    Built-in Synth to Auto's Strip Chart; draw mute on for bar 3 only (top
    half), pan from left to right over bars 1–4: playback mutes exactly for
    bar 3 (the mute row shows a held block) and pans across; with a bus and
    a send (mixer), Send 1 automation changes the bus level; Solo on a bus
    silences what does not feed it.
76. Plugin parameters: add Apple's AUDelay (or the CLAP test gain if
    installed) as an insert on Built-in Synth; Automation lists it with its
    automatable parameters; draw Dry/Wet Mix (or Gain) from 0 to full over
    bars 1–4 and play: the effect follows; the plugin's own window moves its
    control while playing; no "Set Plugin Parameters" undo steps appear.
    Stop: the plugin keeps the last value.
77. Velocity scale and tempo: a row Automation > Velocity Scale > (the
    chords' destination) drawn low over bars 1–4 makes those chords softer
    on playback (the notes' own velocities in the list do not change). A
    Tempo row drawn as a straight line from 120 down to 60 over bars 5–8 gives
    a smooth slow-down; the list shows two tempo rows (the first "ramp"),
    the transport's BPM falls steadily, and the ruler's bar lines spread. Drawing a tempo
    line inside an existing tempo ramp leaves the tempo before and after the
    drawn range unchanged.
78. Panel and exact mode: Modify > Change Values… (Ctrl-Cmd-M), Operation
    Strip Chart Line: target, operation and shape pickers and the From, To,
    Start, End and Step fields edit the line numerically; an Automation
    Target condition lists the targets; Apply acts on the selection (one undo
    step); Modify Again repeats it. With Exact Mode on (Ctrl-Cmd-E) a Strip
    Chart drag previews, goes back on release, and opens the panel with the
    same line and a Position condition; Apply then makes it. The Kind
    condition's menu offers "Automation". With several lanes selected,
    Modify acts on every selected lane even though the chart shows only the
    shown lane.
79. Scale time: in the piano roll select two chords and Command-drag the end
    of the last selected note one beat right: both chords stretch around the
    first one's start ("Scale Time", one undo step); with Exact Mode on the
    panel opens on Scale Time with the ratio filled in.
80. Windows and VoiceOver: Option-Cmd-3 opens the Strip Chart in its own
    window with the same rows; changing rows in one does not change the
    other; Save, close and reopen the project: rows, bar settings and the
    window come back. With VoiceOver (Cmd-F5) the chart is a group "Strip
    Chart" whose values read like "Velocity 56, bar 1 beat 1" or "Fader ·
    Built-in Synth -6.0 dB, bar 3 beat 1"; the bar's controls read their
    labels (Mode, Operation, Shape, Curve, Wave, Period Kind, Period,
    Phase, Step, Rows).

### Control channel (plan CC)

Run these from a shell in the repository, with `V=target/debug/vizctl`.

81. Off by default, in a throwaway home (never your real one; `vizctl
    launch` always turns the channel on, so start the binary directly):
    `HOME=$(mktemp -d) VIZDAW_SUPPORT_DIR=$(mktemp -d)
    dist/VizDAW.app/Contents/MacOS/VizDAW` without `--control` and with
    `VIZDAW_CONTROL` unset prints no "control channel on" line, and no
    socket appears anywhere. Quit it.
82. `$V launch --app dist/VizDAW.app --demo` prints `pid`, `socket`,
    `home` and `log`; `ls -l` of the socket's folder shows `drwx------`
    and the socket `srw-------`; with `VIZDAW_CONTROL` set to it,
    `$V state windows` lists one "document" window titled "Untitled".
    A second `$V launch` with the same `--home` fails: "another VizDAW is
    listening".
83. `$V run transport.play_stop`, `$V wait playing`, `$V screenshot --out
    /tmp/play.png`: the PNG shows the arrangement with Groove's purple
    reference block, the Chords block and the red playhead, even while
    VizDAW's window is behind other windows. `$V run transport.play_stop`
    and `$V wait stopped`.
84. Dialogs: `$V run file.save` answers `dialog_required` with dialog
    "Save" and no panel appears; `$V run file.save '{"path":
    "/tmp/Demo.vizdaw"}'` saves (the window title becomes "Demo").
    `$V open /tmp/Demo.vizdaw` opens a second window.
85. Input: `$V find Play` gives the toolbar button's ID; `$V click <ID>`
    plays without the pointer moving or VizDAW coming to the front;
    `$V key space` stops; `$V drag 600,168 800,168` (points in the
    arrangement's ruler, from `$V find Arrangement`) selects a range
    (`$V state selection` shows `time_range`); with VoiceOver on, its
    outline sits on the block it reads.
86. Hostile input and quitting: `printf 'garbage\n' | nc -U
    $VIZDAW_CONTROL` answers `parse_error`; `$V call nope` answers
    `unknown_command`; `$V quit`: VizDAW quits and the socket file is gone.

### Drum grid (plan 4e)

Use the demo project. To reach Beat, double-click Song's first Groove
block, then Groove's Beat block; its lane "Drums" is a drum lane with
rows Kick, Snare and Closed Hat. Groove's own Drums lane is a plain lane
that references Beat, not a drum lane. Snap stays on (quarters) unless a
step says otherwise.

87. Open: click Beat's block and press Cmd-4. The detail pane shows
    "Drum Grid" with a bar (Paint/Erase, Step Matrix, 1/16, 1/16, 1/32,
    Vel 100, +, "Drums: Off") and row headers Kick C2, Snare D2 and
    Closed Hat F#2 under a "Drums" header. Kick lights the cells at beats
    1 and 3, Snare at 2 and 4, Closed Hat every eighth, brighter on the
    beats (louder hits). The ruler, zoom (Cmd-scroll) and playhead work
    as in the other editors. At the default document window width
    (980 pt) the bar fits on its one line: every control is visible and
    none is clipped at either edge (check by eye; channel screenshots
    cannot show the SwiftUI bar yet).
88. Paint: click an empty Kick cell: it lights; Edit > Undo "Paint Drum
    Hits" takes it back. Click a lit cell: it goes dark ("Erase Drum
    Hits"). Drag along the Snare row: every cell the drag passes lights
    while dragging, and the whole run is one undo step; dragging back
    towards the start shrinks the run; Escape during the drag leaves
    nothing. A drag that wanders into other rows stays on its own row.
89. Settings: Spacing 1/8 makes a drag light every second cell; Length
    1/16 makes new hits longer; Step 1/8 halves the number of cells, and a
    cell lights when any hit starts in it. Timeline shows the hits at
    their true starts and lengths: a click on a hit erases it, a click
    elsewhere paints one at the grid line before the pointer (G toggles
    snap; with snap off, exactly where you click).
90. Erase tool: choose Erase; a drag erases every hit it crosses; a click
    on an empty cell does nothing and adds no undo step.
91. Velocity lane: right-click Snare's header > Show Velocity: the row
    grows a lane with a bar per hit. Painting in that row sets each new
    hit's velocity from the pointer's height (top 127, bottom 1); a run
    dragged up and down gives each hit the velocity where the pointer
    passed it, and the bars and the cells' brightness show it. Vel in the
    bar sets new hits in rows whose velocity lane is hidden.
92. Rows: + adds "Open Hat" (A#2) and selects it. Double-click a row name
    to rename it. Drag a row header up or down to reorder; onto another
    drum lane to move it there; below the last lane ("Drop a row here to
    give it a lane of its own") to give it a new lane named after it,
    its notes going along. Escape during a row-header drag cancels it:
    nothing moves and no undo step is added. The header menu's Split Row
    to Own Lane does the same as the last drop; Remove Row deletes the
    row and its notes. Each is one undo step, and the arrangement shows
    the new lane.
93. Drum lanes: in Song, right-click the Chords lane header > Drum Lane:
    Chords becomes a drum lane with a row per pitch, and Cmd-4 on Song
    shows it; Drum Lane again makes it a normal lane (its notes stay).
    Drum > Drum Lane does the same for the selected lane (click its
    block first). A sequence without drum lanes shows "No Drum Lanes".
94. Play quantize and nudge: play Beat (Space) and choose "Drums: Off" >
    1/8 Swing 66%: the hats' offbeats play late (a shuffle) while the
    list editor still shows them on the grid. Nudge Later a few times:
    the drums play slightly behind Groove's bass (play Groove); the menu
    reads "Nudge: +360 ticks". Off and Nudge Earlier undo it; both are
    saved with the project.
95. Clean starts: play Song from the middle of a beat (click in the
    ruler): the first drum hits heard are the ones after the position,
    never the rest of a hit that began earlier. Play Beat, then mute and
    unmute Beat's Drums lane while it plays (unmute mid-bar): the drums
    come back at their next hit; the same with solo. (Groove's Drums lane
    is a plain lane referencing Beat, not a drum lane, so test the mute
    on Beat itself.)
96. Windows and the Drum menu: Option-Cmd-4 opens the grid in its own
    window; Drum > Add Drum Row, Show Velocity, Split Row to Own Lane and
    Remove Drum Row act on the grid you last clicked in (its selected
    row), Drum > Timeline View switches every grid. Save, close and
    reopen: the bar's settings and the window come back.
97. VoiceOver (Cmd-F5): the grid is a group "Drum Grid" whose cells read
    "Kick, bar 1 beat 1, on, velocity 96" or "Snare, bar 1 beat 1 tick
    3840, off"; the headers are a group "Drum Rows" reading "Kick, C2,
    Drums, Built-in Drums". Every row header shows its destination; one
    inherited from the lane is dimmed, one set on the row itself is
    drawn at full strength. The "Drums" header band reads "Drums ·
    Built-in Drums"; a row with a destination of its own shows it after
    its name.
98. Channel: with `vizctl` (steps 81–86) and VizDAW in the background,
    `$V run editors.drum_grid_window` on Beat (selected in the sidebar),
    then `$V find "Snare, bar 1 beat 1 tick 3840, off"` gives a cell ID;
    `$V click <ID>` paints it (`$V state document` shows undo "Paint Drum
    Hits") without VizDAW coming to the front. `$V run drum.add_row` adds
    and selects a row, `$V run drum.remove_row` removes it again, and
    `$V run drum.timeline '{"on": true}'` switches the view; in a fresh
    launch, `$V run drum.split_row` before any row was clicked answers
    `unavailable`.

### Keys and step entry (plan 4f)

Use the demo project in a throwaway home (VizDAW > Settings changes
preferences). Keys are the defaults unless a step changes them.

99. Menus and defaults: the Transport menu lists Play / Stop (Space),
    Stop (S), Play Selection (Option-Space), Play from Selection
    (Shift-Space), Play from Edit-In (Y), Record (R) and Scrub (K), both
    disabled until sub-project 5, Return to Start (Return), Previous and
    Next Marker (comma, period), Loop (L), Overdub (O), Punch (I), the Go
    to ... Field items (P, E, U) and Arm Previous/Next Lane
    (Option-Shift-Up/Down). The Step menu lists Step Entry (T) and the
    note values 1/1 to 1/32 (1-6), Dotted (.), Triplet (/), Tie (=), Rest
    (0) and Delete Last Step (Delete, shown as ⌫ in the menu); Editors >
    Snap to Grid is G. Check the glyphs by eye: Delete Last Step shows ⌫
    (not ⌦), and the note values show the number row's 1-6. Bind a keypad
    key as a command's first key (Settings > Keys): the menu shows it as
    its digit; pressing the keypad key runs the command.
100. Transport keys: S while Song plays stops and goes back to where
    playback actually began; select a time range in the ruler and press
    Option-Space: only the range plays, loop on or off; Shift-Space plays
    on from its start; with markers in Song (list editor: insert two
    Marker rows), comma and period jump between them. O and I tick
    Overdub and Punch (they record nothing yet); R and K do nothing
    (`$V run transport.record` answers `unavailable` with the reason).
    Open a plugin's window (Plugins) and type letters: they go to the
    plugin, not to VizDAW's commands (single-key transport shortcuts do
    not work while a plugin window is in front; Command keys do). A plain
    key VizDAW does not use never rings through to the main menu.
101. Fields: P selects the Position field: type `5` and Return: the
    playhead jumps to bar 5 and the keys work again; `9:3:480` (colons,
    commas or spaces separate like dots) lands on bar 9 beat 3 and a half.
    E focuses Edit In: typing `3` selects bar 3 in the ruler; Tab to Edit
    Out and type `7` and Return: the range runs to bar 7 (Tab and
    Shift-Tab go round all five fields, so Edit Out and Punch Out, which
    have no key, are reached from the keyboard); an Edit Out
    before Edit In is refused in the status line. U focuses Punch In; Punch Out
    before Punch In is refused. Escape in a field puts its text back.
    Typing letters into any text field (a field, the sidebar rename, the
    list editor's cells) types them and never runs a key; while such a
    field has the keyboard, the single-key and Option/Shift transport,
    locator and mode commands are unavailable (a menu item is dimmed).
    With a Japanese (or other) input method composing in a field, Return
    confirms the text and does not press the sheet's button.
102. Locators: at bar 5 press Option-2; Return goes to bar 1; 2 jumps to
    bar 5; Option-Shift-2 clears it, and 2 then does nothing (`$V run
    locator.go_2` answers `unavailable`: "Locator 2 is not set."). Locators
    are saved with the project.
103. Record arm: Option-Shift-Down arms Song's first lane (the header's
    arm button lights), again moves the arm to the next lane (one lane
    armed), Option-Shift-Up moves it back; past the last lane the status
    line says there is none. Each move is one undo step "Move Record Arm".
104. Step entry: select Chords' block, stop, put the playhead on bar 2
    and press T: the transport bar above the arrangement shows "Step Entry
    · Chords · 1/4". Press 4 (1/8) and play a chord on a MIDI keyboard:
    when the last key is released, the chord is entered at bar 2 as one
    undo step "Step Entry", selected, and the playhead moves on an
    eighth. Press = (tie: the chord grows by an eighth), 0 (a rest: the
    playhead moves on, nothing is entered), . (dotted: the bar shows "1/8
    dotted"), then Delete twice (the rest, then the tie taken back; the
    playhead goes back with them) and once more (the chord goes).
    Clicking in the ruler moves where the next step goes. Number keys
    choose values only while step entry is on; T turns it off and 0–9
    are locators again. Step entry ends with a status line the moment
    the shown sequence changes (select another sequence) or its lane is
    deleted: the step bar goes, Step > Step Entry is unchecked, 2 jumps
    to locator 2 and one T starts step entry in the shown sequence.
    While playing, step keys only say "Stop playing to enter steps." in
    the status line (Delete deletes nothing). Holding T, L, O, I, G or
    Space, or a keyboard slot's key, acts once; holding 0, = or Delete
    repeats. With no MIDI keyboard, `vizctl run step.notes
    '{"pitches": "C4 E4 G4"}'` enters a chord.
105. Reserved number row: Settings > Keys > "Number keys always go to
    locators": during step entry 1-6 go to locators and the keypad's 1-6
    choose values.
106. Keyboard slots: give Groove the slot Option-G (sidebar > Keyboard
    Slot...). Option-G selects Groove and plays it from its start; during
    step entry in Song it enters a reference to Groove at the playhead and
    moves on by Groove's length ("Step Entry"). A slot Cmd-S is refused in
    the sheet: "⌘S is the key of “Save”."; a slot whose key is a step-entry
    command's key (for example Delete) is refused the same way, naming that
    command ("Delete is the key of “Delete Last Step”."); a key that is
    also a global command's (4) names the global one ("Go to Locator 4").
    A slot Cmd-Q is refused: "⌘Q is the key of “Quit VizDAW”." (so are ⌘H,
    ⌥⌘H, ⌘M, ⌘, ⌘W and ⌘Tab). Return and Escape in the keyboard-slot sheet's field
    still Save and Cancel.
107. Keys settings: Settings > Keys lists every command by menu with its
    keys (step entry's marked). Record on Stop, press X: X stops; record
    Space for Stop: "Space is the key of “Play / Stop”. Replace it?" -
    Replace moves it, Cancel keeps both as they were. Reset puts a
    command's keys back; Save... writes a `.vizkeys` file; Reset All, then
    Load... brings the saved keys back; a damaged file is refused with its
    reason and changes nothing. Menus show the new keys at once: after
    recording X for Stop, open the Transport menu: Stop shows X (no
    relaunch, no other menu opened first).
108. Channel: `$V key t`, `$V key 4`, `$V run step.notes '{"pitches":
    "60 64"}'`, `$V state editor` shows `"step": {"value": "eighth", ...}`
    and the transport's locators; `$V run keys.load '{"path":
    "/tmp/x.vizkeys"}'` loads a bindings file and `$V key` follows it;
    `$V state editor` reads the snap division snake_case
    (`thirty_second`).

### Audio playback (plan 5a)

Use a throwaway home (`vizctl launch`) and a few audio files of your own:
a WAV, an AIFF, an MP3 or AAC, and one at 44.1 kHz.

109. New project: File > New opens a "New Project" dialog ("Every audio
     file of the project is stored in this format.") with Sample rate and
     Bit depth pop-ups, pre-filled with 48 kHz / 24-bit; choose 44.1 kHz
     and 16-bit and press Create. The next File > New offers 44.1 kHz /
     16-bit. File > Project Settings… shows "Format: 44.1 kHz, 16-bit.
     The project has no audio files yet."
110. Import Audio: open the demo, select a lane, put the playhead on bar
     3 and choose File > Import Audio… (⇧⌘I) with one stereo WAV. A bar
     shows "Importing audio…" with progress and a Cancel button, then the
     region sits on that lane at bar 3. Undo removes it (one step,
     "Import Audio"). Import two files at once: each gets a new lane
     "Audio n". A text file renamed `.wav` in the batch is listed by a
     bar (Dismiss clears it) and the others import; imported alone, it
     raises an "Import Failed" alert.
111. It plays: Play from bar 1. The region sounds through the output path
     strip "Audio 1" (use the plain Create Demo Project) in the mixer; its fader, pan, mute and solo work; the
     region's sound starts at its bar exactly (record the output or
     listen against the click of a MIDI lane).
112. Loops and locates: loop bars 3–4 over a long region: the loop wraps
     with no gap or click beyond the region's own audio at the seam.
     Click in the ruler in the middle of a region while playing: the
     sound resumes from there at once. No "Disk too slow: some audio
     played as silence." bar shows on a normal disk (if it does, Dismiss
     clears it).
113. Formats: use the project from step 109 (44.1 kHz / 16-bit, with a
     file imported into it; the demo is not in that format). File >
     Project Settings… says how many audio files the change converts
     (one undo step); choose 48 kHz / 24-bit and press
     Change: a "Converting media…" bar, then the regions sit where they
     were and sound the same. Edit > Undo ("Change Project Format")
     restores 44.1 kHz and the old files.
114. Media in another format: in a saved project, replace a file in its
     `Media/` folder (Finder > Show Package Contents) with a CAF at
     another rate, then switch back to VizDAW: a bar says "1 media file
     is not in the project format (…) and stay silent."; that region is
     silent. "Convert 1 File": the bar goes and the region plays.
     `$V state media` lists the item with `"offline": false` afterwards.
115. Missing media: rename a media file in Finder and switch back: its
     region is silent and `$V state media` shows `"offline_reason":
     "missing"`; rename it back and switch again: it plays.
116. Device rate: in Audio MIDI Setup set the output to 44.1 kHz, open a
     48 kHz project and press Play: VizDAW switches the device to 48 kHz
     (Audio MIDI Setup shows it). With a device that cannot run at 48 kHz
     (or set the project to 192 kHz on a 48 kHz-only device), after the
     first Play a bar says "The audio device runs at … ; this project is
     …. Audio regions are muted." while MIDI still plays, and "Change
     Project Rate to …" converts the project to the device's rate.
     Afterwards set the device's rate in Audio MIDI Setup back to what it
     was (the throwaway home does not protect this system setting).
117. Save As during a job: in a saved project, import a long or large
     audio file (a few hundred MB, or several files at once) so the
     "Importing audio…" bar stays up for a while, and at once choose File
     > Save As…: it is refused with "Wait for the import or conversion
     to finish, then save." Once the bar ends, Save As works.

### Regions in the editors (plan 5b)

Use a throwaway home (`vizctl launch`), a few audio files of your own
(one long, one stereo WAV), and the Finder.

118. Waveforms: open the demo and Import Audio… a stereo WAV onto a lane.
     Within a second its region in the arrangement shows the waveform of
     both channels. Zoom in until single samples show (the waveform may
     look blocky for a moment, then sharp) and out until the whole song
     fits: it stays drawn, scrolling never stalls. Open the Piano Roll on
     the sequence: the region strip draws the same waveform.
119. Fades: with the arrow tool, drag the region's top-left corner square
     to the right: the waveform ramps in and a line shows the fade; Edit
     > Undo says "Undo Fade In". Drag the top-right square to the left
     ("Fade Out"). Right-click inside a fade: "Fade In: Linear …
     S-Curve" with the current shape checked; choose S-Curve and listen.
     A region narrower than about 24 points shows no handles.
120. Gain: drag the region's top edge up: a readout shows "+3.0 dB" and
     the waveform grows; release: one step "Region Gain". With ⇧ the
     steps are finer. ⌥-click the top edge: back to "0.0 dB".
121. Region menu: select the region; Region > Normalize… offers -0.3;
     enter -1: the loudest peak reaches -1 dBFS (the waveform touches
     near the top), one step "Normalize". Put the playhead inside the
     region: Region > Fade In to Cursor and Fade Out from Cursor set the
     fades there; Remove Fades clears them. With nothing selected the
     items are disabled; with the playhead outside the region the status
     line says "the cursor is not inside a selected audio region".
122. Drag-in: drag a WAV from the Finder onto a lane at bar 3: a bar
     "Importing audio…", then a region at bar 3 (snapped to the grid).
     Drop two files below the last lane: two new lanes "Audio n". Drop a
     file on the Piano Roll's region strip: it lands on that region's
     lane. A text file renamed `.wav` among them is listed in a Problems
     bar.
123. Media Pool: Window > Media Pool lists Name, Duration, Channels,
     Origin, Uses, Used and Offline. Select Unused selects items no
     region uses; Compact removes them (one undo step). Rename… renames
     (the arrangement's VoiceOver label follows). Reveal in Finder shows
     the file. Import… adds files to the pool only. Delete on an item
     in use first lists its regions ("Song › Audio 3, bar 1"), then
     removes the item and those regions as one step. Double-click a row
     (or Select Regions): its sequence shows with its regions selected.
124. Offline and Relink: save, close, move one media file out of the
     package (Show Package Contents > Media), reopen: the "missing or
     changed" sheet lists it as Missing and its region is hatched and
     says "Offline". Locate… the moved file: the bar "Relinking media…",
     then the region plays again (the file is copied into Media/ as
     "<name> 2.caf"). Leave Offline closes the sheet; File > Relink
     Media… reopens it while something is offline. Search Folder… on the
     folder holding the moved file finds it even renamed. Replace a
     media file with another of the same length and format (copy one in
     under its name) and switch back to VizDAW: after a moment the item
     is "Changed" and silent; put the original back: it plays.
125. Retention: in a saved project, import a file, Undo, Save: the file
     is still in Media/. Make any other edit and Save: the file is now
     in the Finder's Trash (Put Back restores it). Compact an unused
     item and Save: its file stays (Undo could bring it back) until
     another edit and Save. Once, run `cargo test -p platform-macos --
     --ignored a_file_goes_to_the_real_trash`: a
     "vizdaw-trash-check-<pid>.txt" appears in the Trash (delete it).
126. Overview cache: quit, delete the package's `Media/.overviews`
     folder, reopen: waveforms come back within a second or two; a
     garbage file put there in place of a `.vzov` is replaced the same
     way.
127. Channel: with a region selected, `$V run region.normalize
     '{"peak_db": "-0.3"}'` answers with `"undo": "Normalize"`; `$V run
     region.normalize` fails naming the "Normalize" dialog; `$V run
     media.relink '{"media": 1, "path": "/abs/file.wav"}'` waits for the
     job; `$V state media` shows `used_percent` and, for a changed file,
     `"offline_reason": "changed"`; `$V run window.media_pool` opens the
     pool and `$V state windows` lists it as `media_pool`.
128. Look and access: switch macOS between Light and Dark appearance:
     regions, waveforms, handles and the offline hatching follow the
     system colours. With VoiceOver, regions read "Region <name> on
     <lane>, bar n, gain …"; the Relink sheet's buttons read "Locate
     <name>", "Search Folder…" and "Leave Offline".
