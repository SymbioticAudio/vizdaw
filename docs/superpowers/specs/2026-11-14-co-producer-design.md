# Sub-project 8 — Co-Producer

Status: draft from the owner's decisions of 2026-11-11, the brainstorm of 2026-11-14 and the owner's answers the same day, with the builder-check settlements, for the owner's review.
Parent spec: `docs/superpowers/specs/2026-10-15-vizdaw-design.md` (§11 step 8). Builds on `2026-10-18-control-channel-design.md` (the protocol, the command registry, §7 dialogs and Busy, §8 security, the hostile-input rules) and the backlog row "Co-Producer over MCP" in `docs/feature-backlog.md`. Runs after sub-project 6; in the 1.0 release (owner, 2026-11-11).

Throughout, **(owner)** marks the owner's decision, **(controller's call, owner saw it)** a guess the controller showed the owner without objection (it counts as decided), **(controller's call)** a decision made while writing this that the owner may overturn, and anything else is left to the builder (§11).

**Name (owner, 2026-11-14).** The feature is called **Co-Producer** everywhere: menus, Settings, undo names, notes, docs, code. The old working name was another company's product name; it is now on the banned-name list in the name scans (`crates/core-control/tests/names.rs`, `app/Tests/VizDAWAppModelTests/NameScanTests.swift`), which this document never spells out.

## 1. Intent

The owner wants to "lean into 'AI can drive it'": not an AI that composes for the user, but a co-producer that works next to the composer from day 1 (owner, 2026-11-11). The composer talks to Claude in Claude's own app; Claude drives the running VizDAW through a local MCP server (owner).

What it is for, in the owner's order (owner, 2026-11-14):

1. **Chores first.** "Export all as DAWproject", "bounce to stems", set up tracks and routing, name and colour things, tidy MIDI (quantize, velocity, transpose), explain the session.
2. **Creative help second.** Mix advice and bus recipes (a shared reverb, parallel compression, delay into reverb); "help me mix my drums"; starter templates such as "80s synthwave" that load instruments, drums and routing; automation drawn as real points (fades, rides, filter sweeps); and edits on the composer's own material such as turning held chords into arpeggios as real notes, reharmonising, humanising, variations.

**What success looks like (owner):** the composer is more productive and iterates more quickly on new ideas and new songs.

**What the owner winces at (owner):** destructive edits, deletions, and overwriting good creative work. The safety rules in §6 exist to make that impossible by accident.

## 2. Goals

1. A local MCP server, `vizdaw-mcp`, that lets Claude (in Claude's desktop app, Claude Code or any MCP host) drive a running VizDAW through the control channel's command registry and read its state (owner).
2. Every Co-Producer request lands as **one named undo step**, such as "Co-Producer: Set Up Drum Bus", so one Undo takes it all back (owner).
3. Small live **MCP Apps views** in the conversation: a preview of what will change, a track list, a mixer snapshot, an automation curve before it is written (owner).
4. One Settings switch, **Co-Producer**, that fully disables it (owner; §5).
5. Safety: it adds freely; anything that deletes or changes the composer's own work needs a yes in chat that shows exactly what changes; a saved copy of the project is made before each request; files are never replaced (owner; §6).
6. A good **librarian**: a per-song Markdown notes file inside the project package, clear marks on what Co-Producer added, clearly named dated export folders (owner; §7).
7. Creative help from **any installed plug-in** (AU, CLAP, built-in), with web search for ideas done by Claude's own tools (owner; §8).
8. Opt-in **metrics** of where the composer's time goes, counted per song and across all songs, kept on the Mac, with coaching only when asked (owner; §9).

## 3. Non-goals (for 1.0)

- **A chat window inside VizDAW** (owner). The composer talks to Claude in Claude's own app. An in-app Co-Producer pane may come later; nothing in 1.0 depends on it.
- **AI composition**: writing songs for the composer. Creative edits work on the composer's own material or add parts the composer asked for, always as undoable edits (owner, 2026-11-11).
- **Listening.** Co-Producer can see the project (tracks, buses, plug-ins, settings) but cannot hear audio (controller's call, owner saw it). Advice comes from the project's structure, levels VizDAW reports and Claude's own knowledge. Analysing bounced audio is out of scope (controller's call).
- **Network access by VizDAW.** VizDAW itself never goes online for Co-Producer. Web search, if any, is done by Claude's own tools in Claude's app (owner).
- **Clicking through the UI.** Co-Producer acts through registry commands, not by posting clicks, drags or keys into VizDAW's windows (controller's call; §4.3).
- **Driving plug-in editors' own windows.** It sets plug-in parameters through VizDAW, not by working a plug-in's UI.

## 4. Anti-goals (never, in any version)

- **Destructive edits, deletions or overwriting the composer's creative work without a yes** (owner). No hidden or silent route around §6 may exist: not a "trust me" setting, not a batch mode.
- **Replacing a file.** Co-Producer never writes over an existing file, inside or outside the project (owner).
- **Interrupting.** Coaching and suggestions come only when the composer asks (controller's call, owner saw it).
- **Sending the composer's data anywhere by itself.** Metrics and notes stay on the Mac; Claude sees them only when the composer asks through Co-Producer (controller's call, owner saw it).

## 5. Constraints

- **GPL-3.0**, as the whole repo. Any MCP SDK or crate used must be compatible (MIT or Apache-2.0 is fine). The Storytold Rust DAW (MIT/Apache) has an MCP server worth reading for ideas (memory note).
- **Builds on the control channel** and keeps its rules (control-channel spec §3, §7, §8): JSON lines over a private Unix socket, 0600 file permissions, no network port; parsing in Rust; no call opens a dialog; Busy answers `busy`; hostile input returns an error and never crashes the app.
- **The banned-name list** in the name scans applies to everything Co-Producer shows or writes, including undo names, notes and export folder names.
- **UI text says "track", never "lane"** (the name scans check it).
- **Project format.** Co-Producer marks (§7.2) and the notes file are a format change. Sub-project 6 owns versions 8 and 9, so Co-Producer takes the next free version (10, unless SP6 ends with another) (controller's call).
- **MCP Apps** is a young extension (developer preview as of 2026-11). Every view must have a plain-text fallback, so Co-Producer still works in a host that does not show views (controller's call).
- **macOS only**, like the rest of 1.0.

## 6. The decided "how"

### 6.1 The MCP server over the control channel (owner)

```
Claude's app ── MCP (stdio) ──▶ vizdaw-mcp ── JSON lines, Unix socket ──▶ VizDAW (running)
                                 (Rust)          the Co-Producer socket      registry, state, undo
```

- `vizdaw-mcp` is a small Rust binary in the workspace (`crates/vizdaw-mcp`), built on `core-control`'s client, shipped inside the app bundle. The MCP host starts it over stdio (owner, 2026-11-11). It is local only.
- **A separate socket for Co-Producer** (controller's call). Today's control channel is off unless a launch flag or `VIZDAW_CONTROL` names a socket; that stays the developer tool `vizctl` uses, with its own setting. When the Co-Producer switch is on, VizDAW listens on a second socket at a fixed path in VizDAW's support folder (`~/Library/Application Support/VizDAW/co-producer.sock`), with the same security checks (§8 of the control-channel spec). The Co-Producer switch does not cover `vizctl`'s channel (controller's call; this settles the question left open in the backlog row).
- **What Co-Producer may call** is an allow-list, not the whole channel (controller's call): `commands.list`, `state.get`, `wait`, `ui.screenshot` (to see a window, read-only), and `command.run` for registry commands that carry a Co-Producer class (§6.5). `ui.click`, `ui.drag`, `ui.key`, `app.open` of arbitrary files and `app.quit` are refused on the Co-Producer socket.
- **Tools and resources.** The server turns each allowed registry command into an MCP tool (its parameters and help come from the registry, so the two cannot drift), and `state.get` targets into MCP resources (the song, tracks, mixer, automation, media, project). It adds a few Co-Producer tools of its own: begin and end a request (§6.2), preview and apply a change (§6.6), read and append the notes (§7.1), list installed plug-ins (§8), read metrics (§9).
- **Settings mirror.** VizDAW answers on the Co-Producer socket only while the switch is on; `vizdaw-mcp` reports "Co-Producer is turned off in VizDAW's Settings" when it cannot connect, or "VizDAW is not running".

### 6.2 Each Co-Producer edit is one named undo step (owner)

- A **request** is one thing the composer asked for in chat ("set up a drum bus"). Claude opens it with a name, runs its commands, then closes it. Everything in between lands in the undo history as one step named "Co-Producer: <name>", in title case, at most about 40 characters (controller's call).
- **Only Co-Producer's own steps go in that group** (controller's call). If the composer edits by hand while a request is open, the hand edit closes the group and is its own undo step, as always. If Co-Producer then carries on with the same request, it opens a new group named "Co-Producer: <name> (continued)". So undoing a Co-Producer step never takes away the composer's work.
- Undo and Redo of that step work like any other step, from VizDAW's Edit menu or by the composer asking Claude. If Claude is asked to undo a step that is the composer's own, that is a change and needs a yes (§6.6) (controller's call).
- If a request is left open (Claude stops, the connection drops), VizDAW closes it after a short idle time and keeps what was done as one step (controller's call). If a command inside a request fails, the steps already done stay, the request is still one step, and the reply says what failed.
- Commands that do not change the document (exports, bounces to files, reading state) still belong to the request, for the notes, but add nothing to the undo history.
- Only one request is open per document at a time; a second one gets an error naming the open one.

### 6.3 MCP Apps views (owner)

Small web views served by `vizdaw-mcp`, shown in the conversation. The native SwiftUI windows cannot be embedded; these are separate views drawn from VizDAW's state (owner, 2026-11-11).

For 1.0 (controller's call):
- **Change preview**: the exact list of what a request will change or delete (§6.6), with before and after values. This is the most important view.
- **Track list**: tracks and buses, their colours, and which ones Co-Producer added.
- **Mixer snapshot**: strips, levels, routing, inserts and sends.
- **Automation preview**: the curve Co-Producer plans to write, over the existing one, before it is written.
- **Export result**: what was written and where, with a button that reveals the folder in Finder (if the host allows it).

Views are read-only pictures plus, at most, the confirm button of §6.6; they never change the document by themselves. Each has a text fallback. They follow the current theme's colours where practical (controller's call).

### 6.4 The Settings switch "Co-Producer" (owner)

- One switch in VizDAW's Settings, labelled **Co-Producer**. When off: the Co-Producer socket is closed and accepts no connections, any open connection is dropped, no Co-Producer views are served, and nothing reaches the document through it.
- It persists across launches.
- **Off by default** (owner, 2026-11-14). A fresh install opens no Co-Producer socket; the composer turns it on in Settings.
- The metrics have their own switch (§9), off until turned on.

### 6.5 Classes of command

Every registry command that Co-Producer may run carries one class, checked in Rust before the command reaches the app (controller's call):

| Class | Meaning | Needs a yes? |
|---|---|---|
| read | reads state, changes nothing | no |
| add | creates something new and sets it up: a new track or bus with its own inserts, sends, routing and automation; a region on a new track; a marker; a file in a new folder | no |
| change | changes something that exists: names, colours, levels, notes, regions, settings, automation; and on the composer's tracks, routing them to a bus or adding an insert, send or automation (owner) | yes, when the thing is the composer's (§6.6) |
| remove | deletes something | yes, when the thing is the composer's |
| outward | writes files outside the project | no, as long as it only creates new files (§6.7) |
| refused | never run by Co-Producer | always refused |

**The composer's own work** is anything not marked as Co-Producer-added (§7.2), plus anything the composer has taken over. **Routing and sound changes are changes** (owner, 2026-11-14): pointing the composer's existing tracks at a new bus, or adding an insert, send or automation to a track the composer made, needs a yes, with exactly what will change shown. Adding new tracks and buses, and setting up those new things themselves, stays free (owner). Changing or removing Co-Producer's own additions, not yet taken over, needs no yes (controller's call): it is Co-Producer tidying up after itself.

**Refused** (controller's call): commands that delete or overwrite files (`media.compact`, which removes unused media; `region.reload_external`, which replaces a region's audio from an outside file; key-binding writes `keys.save`, `keys.load`, `keys.reset`), recording (`transport.record`, `record.*`; Co-Producer cannot hear and should not record over takes), `file.close`, `file.save_as` (§6.7), `plugins.install_multiq`, and app-wide settings (`plugins.new_channel_eq`, `audio.set`, themes and layout).

### 6.6 A yes in chat that shows exactly what changes (owner)

Before a change or remove on the composer's work runs, Co-Producer shows the composer exactly what will change and waits for a yes.

- Claude first asks VizDAW for a **preview** of the planned commands. VizDAW works it out without changing the document and answers a plain list: each item names the thing ("Track 3, Lead Vox"), what happens to it ("volume −2.0 dB → −6.0 dB", "deleted: 4 notes in bar 12"), and whether it is the composer's work. The change-preview view (§6.3) shows the same list.
- The preview carries a one-time **token** tied to the document's current revision. **Apply** needs that token; it fails if the document changed in between, if the token was used, or if the planned commands differ from the previewed ones.
- **Who says yes** (controller's call): where the MCP host supports MCP elicitation (the server asking the user directly in the host's own UI), `vizdaw-mcp` asks the composer that way, so the yes comes from the composer and not from Claude. Otherwise the token is handed back to Claude, and the tool's description tells Claude it must show the list and get the composer's explicit yes in chat first. The builder checks which hosts support elicitation (§11).
- Adds need no yes, but every request's reply still lists what was added, so nothing arrives unseen.

### 6.7 A saved copy before each request; files never replaced (owner)

- **Saved copy.** When a request opens, before its first command that changes the document, VizDAW writes a copy of the song as it stands, open changes included, without changing which file the window is editing. The copy goes in the package, under `Co-Producer/Copies/` (controller's call), named by date, time and request ("2026-11-14 15-02 before Set Up Drum Bus.vizdaw"). It holds the song document only; media files are shared, which is safe because they are never replaced. File > Open can open a copy as a project of its own. Copies are never deleted by Co-Producer; their total size is shown in Settings (controller's call).
- **Copies stay complete** (owner, 2026-11-14). Save's pruning of unused media and Compact Media never delete plug-in state or media that a Co-Producer copy still uses. In the owner's words, "Compact can ask to delete producer edits": Compact offers to remove old Co-Producer copies and the files only they use, listing each with its size, and asks the composer first. Nothing is removed without that yes. (Co-Producer itself still cannot run Compact, §6.5.)
- **Projects never saved** (controller's call). Co-Producer may work in an untitled project. Its notes and copies are kept in VizDAW's support folder for that untitled document and move into the package on its first save.
- **Saving** (controller's call). Co-Producer may ask to save the project. Save writes over the project file, so it is a change and needs a yes (§6.6). The first save of an untitled project always goes through the composer's own Save dialog: Co-Producer never opens dialogs, so it asks the composer to save, or answers that the project needs saving first. Autosave works as it does today.
- **Files never replaced.** Every outward command writes into a new file or a new folder. Today's exports already refuse or number a taken name (bounce stems, AAF and other-DAW exports number their folders); for the rest, a Co-Producer call with a path that exists is refused in Rust before it reaches the app (controller's call). "File writing goes through the same checks as the menus" (controller's call, owner saw it): Co-Producer never gets a looser path than the composer's own Save dialog would.

## 7. The librarian (owner)

### 7.1 The song notes file

- Each song (one VizDAW project) has a Markdown file inside its package: `Notes.md` at the package's top level (controller's call on the name). Both Co-Producer and the composer write it (owner).
- **Co-Producer logs** (owner): what was asked, what it did, where any files went, and the name of the undo step. One short entry per request, newest last, headed by date and time. **The composer's own edits are not logged**: the project already holds them (owner).
- **The composer** can write anything: ideas, lyrics, reference songs, "never touch the piano part". Co-Producer reads the whole file before its first request in a session ("future sessions read the notes first", owner) and follows what the composer wrote there.
- **Co-Producer only appends** (controller's call). It never rewrites or deletes what is already in the file, the composer's text or its own. Appending goes through VizDAW, which re-reads the file first so an edit made in another editor is not lost.
- **Opening it**: a menu item, Window > Song Notes, opens `Notes.md` in the Mac's default Markdown editor (controller's call). A VizDAW-drawn notes pane is not in 1.0.
- A new project gets a `Notes.md` with a one-line heading (the song's name) the first time Co-Producer or the menu item needs it.
- Exports do not include the notes (controller's call).

Example entry (controller's call on the layout):

```markdown
## 2026-11-14 15:02 — Set up a drum bus
- Asked: "put my drums on a bus with some parallel compression"
- Added (free): bus "Drums (Co-Producer)"; bus "Drum Crush (Co-Producer)" with a compressor; a send from "Drums (Co-Producer)" to it
- Changed (yes in chat): Kick, Snare and Hats now go to "Drums (Co-Producer)" instead of the master
- Undo step: "Co-Producer: Set Up Drum Bus"
- Saved copy: Co-Producer/Copies/2026-11-14 15-02 before Set Up Drum Bus.vizdaw
```

### 7.2 Marks on what Co-Producer added (owner)

- Tracks and buses Co-Producer adds are **named, coloured and marked as Co-Producer-added until the composer takes them over** (owner).
- Name: a plain name with "(Co-Producer)" after it, such as "Drum Crush (Co-Producer)" (controller's call). Colour: one fixed Co-Producer colour from the current theme (controller's call). The mark is saved in the project (§5, format change).
- **Taking over** (controller's call): the mark clears when the composer renames the track or bus, or chooses Track > Make Mine (also on its context menu). From then on it is the composer's work and §6.6 applies to it. Editing its contents by hand does not clear the mark, so the composer can tweak a Co-Producer track and still have Co-Producer tidy it.
- The mark shows as a small Co-Producer badge on the track header and the mixer strip (controller's call; the look is the builder's, in the theme's style).
- Plug-ins, sends, regions and automation that Co-Producer adds to the composer's own tracks are marked the same way inside the project, so §6.5 can tell them apart, but show no badge in 1.0 (controller's call).

### 7.3 Export folders (owner)

Exports go into clearly named, dated folders: `<song> — <what> — <date>` such as "Night Drive — Stems — 2026-11-14", inside a folder the composer chose once (asked in chat the first time, then remembered per song in the notes), or next to the project by default (controller's call). For an untitled project the default is the export location set in VizDAW's Settings; if none is set, Co-Producer asks (controller's call). A second export the same day gets a number, never a replacement.

## 8. Creative help (owner)

- **Any installed plug-in.** Creative work may use any plug-in the composer has installed: AU, CLAP and VizDAW's built-ins (owner). VizDAW exposes the installed plug-in list (name, maker, kind, instrument or effect, enabled or not in the Plugin Manager) through a new read command (§10 gaps). Plug-ins turned off in the Plugin Manager are not offered.
- **Web search by Claude.** For ideas ("which synths fit 80s synthwave?", "a classic delay into reverb setting"), Claude may search the web with its own tools in Claude's app (owner). VizDAW never goes online for it.
- **Mix recipes** (examples, owner): a shared reverb bus fed by sends; parallel compression; delay into reverb; "help me mix my drums" (a drum bus, gain staging from the mixer's reported levels, EQ and compression from installed plug-ins). A recipe usually touches the composer's tracks (routing them to the new bus, adding a send), so it **previews, then asks, then applies** (owner): the new buses and what is on them are adds, and the routing and sends on the composer's tracks are changes shown in the preview.
- **Starter templates** (example, owner: "80s synthwave"): **sounds and routing only** (owner, 2026-11-14): instruments, a drum kit, buses, effects, tempo, meter and markers, using installed plug-ins. A template writes no notes or beats (owner). It makes an untitled project, as File > New does (controller's call), so everything in it is an add and marked Co-Producer-added.
- **Edits on the composer's material** (owner, 2026-11-11): held chords to arpeggios as real notes (the 5g-1 arpeggiator rendered offline), reharmonising, humanising, variations. By default Co-Producer writes the result onto a **new** Co-Producer track beside the original, which needs no yes (controller's call); replacing the original's notes is a change and needs one.
- **Automation as real points**: fades, rides and filter sweeps written as automation points, shown first in the automation preview view.

## 9. Metrics (owner, opt-in)

- **What is counted** (owner): time spent composing, recording, editing and mixing, **both per song and across all songs**.
- **How** (controller's call, owner saw it): broad categories in time blocks, from which window and tool is in use, not clicks or keystrokes. Friction signals: bursts of undo, the same task repeated, long hunts through menus.
- **Off until turned on** in Settings (a switch under the Co-Producer section, labelled plainly, such as "Count where my time goes") (controller's call, owner saw it).
- **Kept local**, in VizDAW's support folder (`~/Library/Application Support/VizDAW/Metrics/`), keyed by the project's ID so a renamed song keeps its history (controller's call, owner saw it, on the folder). Not inside the project package, so sharing a song does not share the composer's habits (controller's call). Settings has a button that deletes it all.
- **Claude sees it only when asked** through Co-Producer, by a read tool that returns totals and friction signals, never raw logs (controller's call, owner saw it).
- **Coaching only on request** (owner): "where does my time go?", "what could you help with?" Claude then suggests where Co-Producer could take a chore off the composer's hands or remove friction. It never brings this up by itself.
- Counting works with the Co-Producer switch off; reading it needs Co-Producer on (controller's call).

## 10. Chores first: what exists and the gaps

The owner's order is chores first, creative second. These chores map onto today's registry (about 180 commands in `crates/core-control/src/registry.rs`):

| Chore | Command today | Class |
|---|---|---|
| Export the song as DAWproject | `file.export_dawproject` | outward |
| Bounce stems | `file.bounce` with `mode: stems` | outward |
| Bounce the mix, or the selection | `file.bounce` with `mode: mix` / `selection` | outward |
| Bounce onto new tracks | `file.bounce` with its "place on new tracks" answer | outward + add |
| Export a folder for another DAW (stems plus MIDI) | `file.export_for_daw` | outward |
| Export AAF | `file.export_aaf` | outward |
| Export MIDI files | `file.export_midi`, `file.export_midi_clip` | outward |
| What survives an export | `file.interchange_report`, and every export's `dry_run` | read |
| Explain the session | `state.get` (document, mixer, automation, media, project, selection) | read |
| New tracks | `track.new_audio`, `track.new_instrument`, `track.new_midi` | add |
| Duplicate a track | `track.duplicate` | add |
| Rename tracks | `track.rename` | change |
| Delete a track | `track.delete` | remove |
| Quantize, transpose, change velocity | `modify.template` (a named template) | change |
| Normalize, fades, crossfades | `region.normalize`, `region.fade_in_to_cursor`, `region.fade_out_from_cursor`, `region.crossfade` | change |
| Remove fades or crossfades | `region.remove_fades`, `region.remove_crossfade` | remove |
| Import audio or MIDI | `media.import`, `file.import_midi` | add |
| Play, stop, loop, locate | `transport.*` (not record) | read (transport is not the document) |

**Gaps** (commands or features that do not exist yet; each needs the control-channel rules, hostile-input tests and a registry entry):

1. **Requests and undo names**: open and close a named request that groups commands into one undo step (§6.2).
2. **Preview and apply** with a token (§6.6), which needs every change and remove command to be able to describe its effect without doing it.
3. **Save a copy** that leaves the window on its own file (§6.7). `file.save_as` moves the document to the new file, so it does not do.
4. **Mixer editing**: new bus, route a track to a bus, add and set a send, add, remove and reorder inserts, load a plug-in into an insert or as an instrument, set fader, pan, mute and solo, set a plug-in parameter. Today the channel reads the mixer (`state.get mixer`) but only changes it through live automation gestures (`automation.gesture`).
5. **Installed plug-in list** as a read command (§8).
6. **Track and strip colours**: also the backlog row "User track colours".
7. **Direct Modify with parameters**: quantize, transpose and change velocity with values, not only through the panel or a saved template.
8. **Write notes and regions**: put MIDI notes into a new region on a track (for arpeggios and variations; templates write none). `midi.inject` is live input, not an edit.
9. **Automation points**: write and remove points on a control over a time range. Today only live gestures write automation.
10. **Markers**: add, rename and remove markers (today only `transport.next_marker` and `transport.previous_marker`).
11. **Fade shapes**: also the backlog row "A control-channel path to set a fade shape".
12. **Notes file and marks**: read and append `Notes.md`; the Co-Producer-added mark and Make Mine (§7).
13. **Metrics**: counting and a read command (§9).
14. **Tempo and meter** changes, for templates (if the model's commands allow; the builder checks).

## 11. Left to the builder

1. Which MCP SDK or crate to use for `vizdaw-mcp` (licence compatible with GPL-3.0), or a hand-written stdio server.
2. The MCP Apps spec's state at build time (developer preview as of 2026-11): how views are served and how a view's confirm button reaches the server; the text fallback for each view.
3. Which MCP hosts support elicitation, and the exact wording of the confirmation shown there (§6.6).
4. How the undo history groups a request into one step (the core-model history), and the idle time after which an open request closes (§6.2).
5. How each change and remove command works out its preview without touching the document, and how the document revision for the token is kept.
6. How each registry command's class is stored (a new field on the registry entry is the obvious way) and the test that every command has one.
7. The saved copy's format inside the package, and how File > Open shows it.
8. The project format change for marks and the notes file: the version number at build time, and loading older projects (no marks, no notes).
9. The look of the Co-Producer badge and colour in each of the five themes.
10. How the notes file is appended safely while another editor has it open.
11. The metrics' time-block length, the exact friction signals and their thresholds, and the storage format.
12. The tool names and descriptions Claude sees, written so Claude follows §6 (show the list, wait for a yes).
13. How a starter template chooses instruments and a drum kit from the installed plug-ins, and what it does when none fits.
14. Whether `ui.screenshot` on the Co-Producer socket needs any extra limit (size, rate).
15. Tests: hostile input on the Co-Producer socket and through `vizdaw-mcp`; a property test that, with no hand edits in between, a request then its one Undo returns exactly the original document; a test that a hand edit during a request closes the group and a later step is named "(continued)"; a test that Save's pruning and Compact keep everything a Co-Producer copy uses; a test that no Co-Producer path can replace an existing file; a test that every change or remove on the composer's work without a valid token is refused; the switch off refuses everything; an end-to-end test that drives `vizdaw-mcp` against the built app, as the control channel's E2E test does.

## 12. Implementation plans

Chores and safety come first (owner's order). Each plan is its own spec check, plan and build cycle; each ends with the app usable.

| Plan | Scope | Depends on |
|---|---|---|
| **8a — Safety and the server** | The Co-Producer socket and Settings switch; `vizdaw-mcp` with tools from the registry and `state.get` resources; command classes and the allow-list; named requests as one undo step; preview, token and apply, with elicitation where the host has it; saved copy before each request; the never-replace check on paths. Read-only and add commands only at first, then change and remove behind the yes. | — |
| **8b — The librarian** | `Notes.md` (read, append, Window > Song Notes); the Co-Producer-added mark, name, colour and badge; Make Mine; the project format change; dated export folders; untitled projects' notes and copies, moved in on first save. Includes the track colours gap. | 8a |
| **8c — Chores** | The existing exports and bounces end to end through Co-Producer with dated folders and notes; direct Modify with parameters; markers; the track list, export result and change-preview views (MCP Apps). This is where the owner's first chores ("export all as DAWproject", "bounce to stems") work. | 8a, 8b |
| **8d — Mixer and plug-ins** | The mixer editing gap (buses, sends, inserts, plug-in loading, fader, pan, plug-in parameters); the installed plug-in list; the mixer snapshot view. Enables mix recipes. | 8a, 8b |
| **8e — Creative** | Automation points and the automation preview view; fade shapes; writing notes and regions; arpeggio, humanise and variation edits on new tracks; mix recipes; starter templates. | 8c, 8d |
| **8f — Metrics** | Opt-in counting per song and across songs, local storage, delete-all, the read tool, coaching on request. | 8a |

Suggested order: **8a → 8b → 8c → 8d → 8e**, with **8f** after 8c (it is independent and can move to wherever it fits). 8a may split in two (server and switch; then preview, token and saved copy) if it grows past about 12 tasks.

## 13. Open questions

None. The owner answered all of them on 2026-11-14: Co-Producer is off by default (§6.4); routing and sound changes on the composer's tracks need a yes (§6.5); copies stay complete and Compact asks before removing them (§6.7); starter projects cover sounds and routing only (§8). Saving, hand edits during a request and untitled projects are settled as controller's calls (§6.2, §6.7).
