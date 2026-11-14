# Sub-project 6 — Import and export

Status: draft from the owner's decisions of 2026-11-11, revised the same day with the owner's answers to the builder check (round-trip merge, crossfades, MXF, target DAWs, order), for the owner's review.
Parent spec: `docs/superpowers/specs/2026-10-15-vizdaw-design.md` (§5 model, §8 files and import, §11 step 6). Its requirements are binding here and are not repeated in full. Related: `2026-10-18-recording-audio-design.md` (§4 model, §5.1 `MediaImporter`, §9 bounce), `2026-11-03-ui-polish-design.md` §3 (Multi-Q), `2026-10-18-control-channel-design.md` (every new command).
Order (parent §11, owner 2026-11-11: "let's do 6 next, then 8"): SP6 runs right after 5g-2 and before sub-project 8. It owns project format versions 8 and 9. SP8's approval rule for outward actions does not exist yet, and SP6 does not depend on it. Branch `sp6-import`, based on `sp5g-input-fx`.

Throughout, **(owner)** marks the owner's decision of 2026-11-11, **(controller's call)** a decision made while writing this that the owner may overturn, and anything else is left to the builder.

## 1. Intent

The owner's typical use is **moving a project from VizDAW to another DAW and back. (owner)** Everything should survive the trip: all MIDI and audio tracks, automation, clip details, song metadata, fades, track names and markers, "basically everything in AAF or OMF". No single format carries all of that, so SP6 offers routes of different reach and says plainly what each one carries. After every import or export, the user sees a list of anything that changed or was left out.

Success: the owner exports a song as DAWproject, opens it in another DAW, edits it, exports it back, and **merges** the result into the original project: the other DAW's edits arrive, and everything only VizDAW knows about is still there (§7.8). The tracks, regions, fades, crossfades, notes with per-note expression, tempo, markers, mix and automation all survive, with plug-in settings wherever the same plug-in is installed. For DAWs that read only AAF and MIDI files, or only audio and MIDI files, there is still a route that loses nothing silently.

**The owner's exchange partners (owner, from chat; not named in the repo):** mainstream desktop DAWs. One reads DAWproject and AAF. One reads AAF and MIDI files only. One reads only audio and MIDI files. So the three project routes each have a real user: DAWproject (with merge), AAF with MIDI files beside it, and Export for another DAW. Defaults are chosen for DAW-to-DAW exchange, not video post.

## 2. Goals (owner scope: build all of it)

1. **Standard MIDI File** (types 0 and 1) import and export, and the **MIDI Clip File** import and export. These are the base for every route that carries MIDI.
   - **The MIDI Clip File** is the parent spec's "MIDI 2.0 Clip File" (§8). It is the file format the MIDI Association publishes as the *MIDI Clip File Specification* (M2-116-U): a header `SMF2CLIP`, then one clip of Universal MIDI Packets with delta clockstamps, usually with the extension `.midi2`. It holds one clip, not a song: MIDI 2.0 channel voice messages with 16-bit velocity, 32-bit controllers, per-note controllers and per-note pitch bend, note attributes and 16 groups, plus tempo, meter, key and text as Flex Data. In VizDAW it is the **lossless MIDI format for one track**. **(controller's call)** The builder checks the details against the published specification (midi.org) before coding. A multi-track MIDI 2.0 container file is out of scope; if its specification has been published by the time 6a starts, it goes to the backlog.
2. **DAWproject** import and export: the full-fidelity route. The format is open and stable at version 1.0, MIT-licensed (https://github.com/bitwig/dawproject): a ZIP holding `project.xml`, `metadata.xml`, audio files and plug-in state files. It carries audio clips, fades, MIDI notes with note expressions (mapped to 5g-2's per-note expression), tempo and meter, automation, plug-in state (usable only where the receiving side has the same plug-in) and metadata.
3. **AAF** import and export: audio tracks, clips, fades and crossfades, clip gain, volume and pan automation, track names, markers, metadata, and embedded or external audio. AAF has no MIDI and no musical time, so **MIDI tracks travel beside it as Standard MIDI Files with the tempo map. (owner)**
4. **Export for another DAW**, in one step: stems that all start at bar 1 (from the 5f-2 bounce), one SMF per MIDI track (an idea from Ardour's MIDI stem export), and the tempo map. This is the route for DAWs that read neither AAF nor DAWproject.
5. **OMF import only, last.** It may move to after launch. **(owner)**
5a. **Round trip with merge (owner, 2026-11-11):** "Restore + merge, but with a prompt to the user that asks 'New Project' or 'Merge With Existing Project'". A DAWproject coming back from another DAW can be merged onto the project it came from (§7.8).
5b. **Real crossfades on one track (owner, 2026-11-11):** a VizDAW model, engine and editor feature in its own right, used by the DAWproject, AAF and OMF routes (§7.0).
6. **Bundle Dusk Audio Multi-Q** in every new channel's EQ slot, as the UI-polish spec §3 records. **(owner)**
7. **Ideas from Ardour (owner):** a MIDI file per MIDI track on stem export, and an option to re-import the exported files. Ardour's public docs and code may be read for ideas; code is copied only under a licence compatible with GPL-3.0, with its notices kept.

## 3. Non-goals

- **A project archive with FLAC** (an Ardour idea). It goes to the backlog. **(owner)**
- OMF export. OMF 1.0 import. **(controller's call)** OMF 1.0 is refused with a clear message.
- Reading other DAWs' native project files. The parent's non-goal stands.
- **MXF audio (owner: not now).** AAF clips that point at MXF arrive as offline media, which the user can relink to a WAV or AIFF copy. An MXF reader is a backlog row.
- Time-stretched clips. VizDAW plays audio at its recorded speed (SP5 §1), so a stretched clip is placed unstretched and listed.
- Video, the clip launcher and VCAs. They do not exist in VizDAW. What happens to them on import is listed in §7.
- Standard MIDI File type 2. **(controller's call)** It is refused with a message and goes to the backlog.

## 4. Anti-goals: failure even if it works

- **Losing data silently.** Everything that changes or is left out appears in the report (§6.3). An export also lists those items before it writes anything.
- **Crashing, hanging or filling the disk on a hostile file.** Malformed ZIP, XML, compound-file, OMF or SMF input is refused or partly imported with a report. It never panics, never runs without bound and never writes outside the project's `Media/`.
- **Presenting a lossy route as full fidelity.** Each route's menu item and sheet says what it carries (§9). Only DAWproject is called the most complete.
- **Naming other products in the UI or repo.** Ardour may be named in docs, the backlog and third-party licence notices (and any notices it requires are kept), but never in UI text. **(controller's call)** Otherwise the owner's binding naming rule holds: repo text and UI name no other DAW makers or their products, and none of the hardware or controller brands the themes and MPE work drew on. Platform facts already in the specs ("Apple Silicon", AU, CoreFoundation) are allowed. Say "other DAWs". UI text says "track", not "lane". The DAWproject format's origin appears only in its reference URL. **(owner)**
- **Clean room.** Never read `~/workspace/SVP/research`.

## 5. Constraints and dependencies

- macOS 14+, Apple Silicon, GPL-3.0-only, as the parent. Parsing and writing are platform-neutral Rust. Audio decoding still goes through `MediaImporter` (SP5 §5.1).
- A new crate, **`core-io`**, which the parent spec's architecture already names, holds the format readers and writers and the report. It depends on `core-model` and `core-media`, never on `core-ffi`. **(controller's call)** Jobs, threads and the FFI live in `core-ffi`, as `media.import` does today.
- **Third-party code, checked on 2026-11-11:**

| What | Licence | Version, date | Use |
|---|---|---|---|
| `zip` (crates.io) | MIT | 9.5.1, 2026-11-10 | DAWproject container |
| `quick-xml` (crates.io) | MIT | 0.42.0, 2026-08-22 | DAWproject XML |
| `cfb` (crates.io, github.com/mdsteele/rust-cfb) | MIT | 0.16.0, 2026-10-31 | AAF container: reads and writes the compound-file ("structured storage") format |
| `midly` (crates.io) | Unlicense | 0.5.3, 2023-01-01 | Not used **(controller's call)**: SMF is small, the Clip File needs our own UMP code anyway, and we want our own limits on hostile input |
| `oximedia-aaf` (crates.io) | Apache-2.0 | 0.2.1, 2026-08-12 | Not used: young and part of a large media framework. The builder may evaluate it |
| LibAAF (github.com/agfline/LibAAF) | GPL-2.0-or-later (see below) | `master` | Read as a reference for AAF quirks |
| DAWproject `Project.xsd`, `MetaData.xsd` | MIT | v1.0, no release tags | Vendored into `core-io` test data with its notice, for schema checks |

- **LibAAF's licence.** The repository's label and its `LICENSE` file say GPL-2.0 (https://github.com/agfline/LibAAF/blob/master/LICENSE). The source headers grant **"either version 2 of the License, or (at your option) any later version"**: see `src/AAFIface/AAFIface.c` (https://github.com/agfline/LibAAF/blob/master/src/AAFIface/AAFIface.c) and `src/LibCFB/LibCFB.c` (https://github.com/agfline/LibAAF/blob/master/src/LibCFB/LibCFB.c). Ardour's vendored copy has the same header (https://github.com/Ardour/ardour/blob/master/libs/aaf/AAFIface.c). So LibAAF *may* be used in GPL-3.0 VizDAW under GPL-3.0 terms. LibAAF only reads AAF.
- **The AAF route (controller's call): our own Rust reader and writer on `cfb`,** sharing one object model for the AAF classes we use. The reasons:
  - we must write AAF ourselves anyway, and the writer needs the same class and property dictionary as the reader;
  - a memory-safe parser is easier to keep safe against hostile files and to fuzz;
  - there is no C build in the workspace.

  LibAAF is read for the quirks of files from different vendors (its per-vendor source files show what to expect). Any logic ported from it keeps its copyright notice in the file it lands in. Its `aaftool` may serve as a local check of our exports, but not in CI.
- Published specifications used: the AAF Object Specification v1.1, the AAF Low-Level Container Specification and the AAF Edit Protocol (AMWA); the OMF Interchange Specification 2.1; the SMF 1.0 specification and the MIDI 2.0 UMP and MIDI Clip File specifications (MIDI Association); DAWproject's XSDs and reference documentation.
- Rules carried over: every FFI call returns `Result`, and no panic crosses the boundary. A programmatic call never opens a dialog. Tests never touch real devices, the user's folders or real preferences.

## 6. Shared design

### 6.1 Where imports land, and undo **(controller's call)**

| Import | Default | Other choice | Undo |
|---|---|---|---|
| SMF, MIDI Clip File | Into the current project: new tracks at the playhead (or the drop point) | As a new project | Into the project: one step, "Undo Import MIDI File" |
| DAWproject | A new untitled project | Merge With Existing Project, when the file came from VizDAW (§7.8) | New: none. Merge: one step, "Undo Merge DAWproject" |
| AAF, OMF | A new untitled project | Into the current project: new tracks, timed by the project's tempo map (no merge: §7.8) | New: none. Into the project: one step, "Undo Import AAF" |

- A new project's audio format is the file's majority sample rate (48 kHz if it has no audio) at the depth of the "Default project format" preference. Media is converted to it, as in SP5 §4.1.
- An import into the current project converts media to the project's format. It is applied as one command batch inside one history group. The media it adds follows the existing retention rule.
- Opening a `.mid`, `.midi2`, `.dawproject`, `.aaf` or `.omf` with File › Open, or dropping one on the Dock icon, runs that format's import as a new project. A MIDI file dropped on the arrangement imports at the drop point and track, like a dropped audio file.

### 6.2 Jobs

Every import and export is a background job on the existing `Jobs` (`core-ffi/src/jobs.rs`), with progress and Cancel. `JobKindRow` gains `ImportMidi`, `ExportMidi`, `ImportProject` (DAWproject, AAF, OMF) and `ExportProject` (DAWproject, AAF, Export for another DAW). `JobRow` gains a `report`. **(controller's call)**
- An import parses and maps everything first, then applies in one step. Media is staged in `Media/.incoming/` and renamed into place only at the end, so a cancelled or failed import leaves nothing behind.
- An export writes to a partial path and renames it into place at the end, as `PendingExport` does today. Plug-in state is captured fresh from the running instances, as Save does.
- Import and export are unavailable while recording or bouncing, and while another interchange job runs.

### 6.3 The report

`core-io` returns `Report { summary, items }`. The summary counts what moved (tracks, regions, notes, markers, automation points, plug-ins). Each item is one of:
- **Changed**: carried approximately, e.g. "Velocities rounded to 7 bits (MIDI file)" or "Tempo ramp at bar 17 written as 32 steps";
- **Left out**: not carried, e.g. "Input effects on 3 tracks", "Plug-in 'X' (VST3): VizDAW hosts AU and CLAP only";
- **Note**: carried, but worth knowing, e.g. "Track 'Pad' plays the built-in synth here".

Items of the same kind are grouped with a count and their details (track names and bar positions, up to 50, then "and 120 more"). The wording is plain and user-facing. The report never says "lane".

### 6.4 Model changes: format versions 8 and 9

**Version 8 (plan 6a).** The routes need three things the model lacks. All are optional fields, so the migration from 7 to 8 is a no-op bump, like 6→7:
- **`Settings.info: SongInfo`**: title, artist, album, original artist, composer, songwriter, producer, arranger, year, genre, copyright, website and comment, all optional strings. These are DAWproject's `MetaData` fields, so that route carries them exactly. It is edited in a "Song info" section of File › Project Settings…, with undo (`SettingsCmd::SetInfo`).
- **`EventBody::AudioRegion.name: Option<String>`**: the region's (clip's) own name. Without one, the region shows its media's name, as today. Editable in the inspector, with undo.
- **`Settings.project_id: Option<String>`**: a random UUID, given on the first save (and to a migrated project on its next save). Save As keeps it, and "Save a Copy as New Project" (not in SP6) would make a new one. A merge uses it to find the project a returning file came from (§7.8).

**Version 9 (plan 6c):** crossfades (§7.0). **(controller's call)** It is a separate bump because 6c is its own plan and must be able to land alone.

Nothing else changes in the document format. **(controller's call)** Marker colours and comments, clip colours and track comments are listed as "Left out" rather than added.

### 6.5 Limits on hostile input

These are enforced in `core-io` before anything is allocated to match a declared size. They are constants, so tests can reach them.

| Input | Limits |
|---|---|
| Any file | Must be a regular file (no FIFO, device or directory); symlinks are resolved once. MIDI files up to 64 MiB. The XML in `project.xml` up to 256 MiB. |
| SMF, Clip File | Chunk lengths within the file. Variable-length quantities up to 4 bytes. At most 10 million events. One SysEx up to 1 MiB. Running status only after a status byte. Unknown chunks skipped. |
| ZIP | At most 10 000 entries. Names relative, with no `..`, no absolute paths, no drive letters, no NUL bytes and no duplicates. No symlink entries. Encrypted entries refused. A compression ratio over 200:1 is refused for XML. The total unpacked size is checked against free disk space before extracting. |
| XML | No DTD or entity expansion (any DOCTYPE is refused). Depth up to 256. Numbers must be finite and in range. Every IDREF must resolve, and reference cycles are refused. |
| Compound file (AAF) | FAT and directory chains are walked with a visited set (a cycle is refused). Stream sizes are within the file. Object references are strong-reference trees with a depth limit of 64. Index and count fields are within their arrays. |
| OMF (Bento) | The same rules for its table of contents and value chains. |
| Media | External file references are read only if they resolve to a regular audio file. They are never written, and network URLs are never fetched. Declared essence lengths must fit the stream. |
| Positions | Every computed tick, frame and length is checked arithmetic. Out of range means the item is left out and reported, never a wrapped value. |

Damage that makes the whole file ambiguous (a bad container or bad root XML) refuses the import. Damage to one item (one bad clip, an audio file that will not decode) leaves that item out, reports it, and imports the rest. A parser runs inside `catch_unwind` in its job. A caught panic fails the job with `internal` and is a bug, but it never takes the app down.

## 7. What each route carries

### 7.0 Crossfades on one track (plan 6c, owner)

Today the later of two overlapping regions on a track takes over (SP5 §1). With a crossfade, the earlier region keeps sounding through the later one's fade-in, on the complementary curve, so the two blend. The design below is the **controller's call**.

- **Model (version 9).** `EventBody::AudioRegion` gains `crossfade: bool`, default false. When it is true, the region's `fade_in` is a crossfade with the region it overlaps at its start, on the same take. Validation:
  - a region with `crossfade` must start inside an earlier region on its take, and its `fade_in` length must be at least 1 frame;
  - the fade-in may not run past the end of that earlier region;
  - at most one crossfade per region start.

  A region whose overlap disappears (it moved, or the earlier region was trimmed or deleted) loses `crossfade` in the same command. It never fails validation later.
- **Engine.** The schedule's region segments (SP5 §5.3) change in one place. Inside a crossfading region's fade-in span, the earlier region keeps playing, with a gain of the fade's complement:
  - Linear: 1 − x;
  - EqualPower: cos against sin;
  - Exponential and SCurve: their mirror.

  The earlier region's own fade-out still applies on top. After the span, the later region takes over as before. The real-time rules hold: no allocation, and `rt-check` covers the new path.
- **Commands.** `AudioCmd::Crossfade { event, length_frames, shape }` sets the later region's fade-in and flag together. `AudioCmd::RemoveCrossfade { event }` clears the flag and leaves a plain fade-in. Both are undoable. Moving, trimming or deleting either region keeps or drops the crossfade as the validation above says, inside the same undo step.
- **Editors.** The arrangement and the audio editor (7c) draw a crossfade as one X-shaped handle over the overlap.
  - Dragging the handle changes the length. Typed values work, in milliseconds or frames.
  - The context menu offers the shape.
  - Region › Crossfade (with two overlapping regions selected, or the later one) makes a crossfade over the whole overlap, or at the default length (Settings, 10 ms) when the overlap is longer. Region › Remove Crossfade removes it.
  - **(controller's call)** Recording and editing never make crossfades automatically. SP5's "no automatic crossfades" stands: a crossfade is always the user's or an import's choice.
- **Control channel.** `region.crossfade {seq, lane, event, length_ms?, shape?}` and `region.remove_crossfade {seq, lane, event}`, with hostile-input tests: no overlap, a region on another take, length 0 or past the earlier region's end, NaN or negative lengths, unknown shapes, events that are not regions. Their state appears in the region rows `state.get` already returns.
- **Uses in SP6.**
  - DAWproject has no crossfade element, so export writes overlapping clips with matching fades. Import makes a crossfade wherever two clips on a track overlap and their fades cover the overlap.
  - AAF and OMF transitions map to crossfades both ways (§7.5).

### 7.1 Export matrix

✓ exact · ≈ changed (listed) · — left out (listed) · n/a: the route does not carry that kind of content. "Exact" means exact for playback, within the format's resolution.

| VizDAW | SMF | Clip File | DAWproject | AAF (+ SMF beside) | For another DAW |
|---|---|---|---|---|---|
| Which sequence | The current one, flattened | One track (or the selection) per file | The current one, flattened | The current one, flattened | The current one, flattened |
| References (nested sequences) | ✓ flattened | ✓ flattened | ≈ each use becomes a clip named after its sequence; sharing is lost (re-shared on import, §7.4) | ✓ flattened | ✓ flattened |
| Other sequences, keyboard slots, generator settings | — | — | — | — | — |
| Alternate takes | Active take only; the others — | same | same | same | same |
| Notes, velocity | ≈ 7-bit (MIDI 2.0 translation rules); positions rounded to the file's resolution (960 PPQ by default, 15 360 optional) | ✓ 16-bit, 15 360 ticks per quarter | ✓ normalised doubles | In the SMF beside it | In the SMF |
| Note attributes | — | ✓ | — | — | — |
| Per-note expression (5g-2) | ≈ as MPE when the track or its destination uses MPE (5g-2's encoder, with an MPE Configuration Message); otherwise — | ✓ per-note pitch bend and controllers | ✓ note expressions: pitch bend → `transpose` (semitones), CC 74 → `timbre`, poly pressure → `pressure`, per-note 7 → `gain`, per-note 10 → `pan` | As SMF | As SMF |
| CC, pitch bend, pressure | ≈ 7/14-bit | ✓ 32-bit | ✓ points with a `controller`, `pitchBend` or `channelPressure` target | As SMF | As SMF |
| Program and bank | ✓ CC 0, CC 32, program | ✓ | ✓ `programChange` expression (bank as CC 0/32 points) | As SMF | As SMF |
| SysEx | ✓ 7-bit; 8-bit — | ✓ | — | As SMF | As SMF |
| Text, lyrics, cues | ✓ meta events | ✓ Flex Data text | — | As SMF | As SMF |
| Muted notes and references | — | — | — (a muted reference becomes a disabled clip, ✓) | — | — |
| Destination (group, channel) | ✓ channel; group as a port meta event and its own track | ✓ group and channel | ≈ one track per VizDAW track; a plug-in instrument shared by several tracks is copied to each, with its state | n/a | n/a |
| Destination default patch, MIDI mix (5g-1b) | ✓ program and CC 7/10 at the start | ✓ | ✓ as points at the start | As SMF | As SMF |
| Built-in synth | ✓ as plain MIDI | ✓ | ≈ a note track with no instrument (Note) | As SMF | ✓ MIDI and its audio stem |
| External MIDI destination | ✓ as plain MIDI | ✓ | ≈ a note track with no instrument (Note) | As SMF | MIDI only: the stem is silent and is not written (SP5 §9.1) |
| Track name | ✓ track name meta | ✓ Flex Data | ✓ | ✓ slot name | ✓ file names |
| Track own length and loop, play quantize and shift | ✓ unrolled and applied as played | same | same | same | same |
| Drum layout | ≈ notes kept; per-row destinations become channels; row names — | same | ≈ notes kept; rows — | As SMF | As SMF |
| Input effects, inputs, monitor, arm, `return_of` | — (they shape recording, not playback) | — | — | — | — |
| MPE input settings (`Lane.mpe`) | — | — | — | — | — |
| Audio regions | n/a | n/a | ✓ position, offset, length, mute (`enable`); fades ≈ lengths exact, shape — (the format has none); region gain ≈ applied to the exported audio copy (default) or — | ✓ position (to the sample), offset, length, fades (shape ≈), gain ✓, name ✓; muted regions — | ✓ in stems |
| Region name | n/a | n/a | ✓ | ✓ | n/a |
| Overlapping regions | n/a | n/a | ✓ as clips | ✓ (the later start takes over) | ✓ in stems |
| Crossfades (§7.0) | n/a | n/a | ≈ overlapping clips with matching fades, rebuilt on import; the shape is lost | ✓ transitions (shape as for fades) | ✓ in stems |
| Tempo map | ≈ ramps written as steps (every 1/16 note) | ✓ steps; ramps ≈ | ✓ including linear ramps | In the SMF | In a tempo-map SMF |
| Meter, key | ✓ | ✓ | Meter ✓; key — | In the SMF | In the SMF |
| Markers | ✓ marker meta | ✓ | ✓ name and time | ✓ name and time | In the tempo-map SMF |
| Song info | Title (sequence name) and copyright ✓; the rest — | Title ✓ | ✓ all fields | ≈ title as the composition's name, the rest as tagged comments | In `About these files.txt` |
| Strips: volume, pan, mute, solo | n/a | n/a | ✓ | Volume and pan ✓ as track gain and pan; mute and solo — | Baked into stems |
| Sends, buses, output routing | n/a | n/a | ✓ sends with pre/post and enable; buses as effect or submix channels; outputs as channel destinations | — | Baked into stems |
| Inserts, EQ slot | n/a | n/a | ✓ AU and CLAP with state (usable where the plug-in is installed); bypass ✓; sidechains — | — | Baked into stems |
| Instrument extra outputs | n/a | n/a | — | — | Each strip a stem |
| Strip colour | n/a | n/a | ✓ as a track colour | — | — |
| Automation: fader, pan, mute, send level, plug-in parameter | n/a | n/a | ✓ points (exact at each point; the curve between points may differ slightly, since the format interpolates in its own unit) | Fader and pan ✓; the rest — | Baked into stems |
| Velocity-scale automation | n/a | n/a | — | — | Baked |
| Automation modes, MIDI learn, surfaces | n/a | n/a | — (Note); kept by a merge (§7.8) | — | — |

In the DAWproject column, everything marked — for VizDAW-only state is kept when the file comes back and is merged (§7.8): other sequences, keyboard slots, takes, input effects, inputs, MPE input settings, drum rows, automation modes, MIDI learn and surfaces.

### 7.2 Standard MIDI File (plan 6a)

**Import:**
- Type 0 and type 1. Type 2 is refused (§3).
- PPQ divisions that divide 15 360 are exact. Others round, and the report lists the import as Changed only if a position actually moved. SMPTE divisions convert through seconds at 120 BPM.
- Notes pair first-on, first-off per channel and pitch. A note-on with velocity 0 is a note-off. A note still open at the end of its track ends there (Changed).
- 7-bit values scale up by the MIDI 2.0 translation rules, so a VizDAW → SMF → VizDAW round trip is exact for data that started at 7 bits.
- CC 0 and CC 32 immediately before a program change merge into `Program { bank }`. RPN and NRPN stay as controllers.
- Tempo goes to the tempo map, time signatures and key signatures to the meter map, marker meta events to the sequence's markers, track names to track names, other text kinds to `Text` events, and the port meta event to the group.
- A time signature off a barline moves to the next barline (Changed). Sequencer-specific and SMPTE-offset meta events are left out.
- **Channels and destinations** (parent §5). The sheet lists each used (port, channel) with a destination picker: an existing destination on that channel if the project has one, otherwise a new built-in-synth destination on that channel. **(controller's call)** Events keep their original group and channel.
- Type 1 makes one track per SMF track. Type 0 is split by channel by default. A track with several channels takes its main channel's destination, and its other events carry a destination override.
- A channel-10 track becomes a drum track with rows named from the General MIDI drum map for the notes it uses.
- **MPE.** If a track begins with an MPE Configuration Message, or the sheet's "Treat as MPE" is on, member-channel notes and their bend, CC 74 and pressure become per-note expression through 5g-2's decoder, and the track's `mpe` is set.
- Tempo: into an empty project, or as a new project, the file's tempo and meter maps are used. Into a project with material, "Use the file's tempo map" is offered, and is off by default.

**Export:**
- Type 1 by default, with a conductor track (tempo, meter, key, markers, title, copyright) and one track per VizDAW track. Type 0 is optional.
- Resolution: 960 PPQ by default, or 480 or 15 360.
- The scope is the whole sequence, or the selected tracks. What is written is what plays (§7.1).

### 7.3 MIDI Clip File (plan 6a)

- Import makes one track at the playhead (several dropped files make several tracks), carrying everything VizDAW models, at 15 360 delta clockstamp ticks per quarter (exact). Groups and channels map to destinations as in §7.2.
- Export writes one file per selected track, or the time selection of one track. The tempo, meter and key in force are written as Flex Data at the start, along with any changes inside the range.
- MIDI 1.0 channel voice packets in a clip are accepted and translated up. Unknown UMP types are skipped and counted (Left out).

### 7.4 DAWproject (plan 6d)

**Export:** the current sequence goes to `Arrangement` and VizDAW's mixer to `Structure`, as in §7.1. Specifically:
- Each VizDAW track becomes a `Track` with `contentType` `notes` or `audio`. A track holding both splits into "<name>" and "<name> audio" (Changed).
- MIDI is written as note clips along the arrangement's blocks (the lane's block rule as shown), so the other DAW shows the same clips. Each reference use is one clip named after its sequence.
- Audio files are written inside the ZIP as WAV at the project's rate and depth (float projects as 32-bit float WAV), stored uncompressed. The XML is deflated.
- AU state is written as an `.aupreset` (the `ClassInfo` property list as XML, converted by CoreFoundation behind a `core-io` trait). CLAP state is written as the plug-in's raw state stream.
- `Application` is `VizDAW` with its version. `MetaData` comes from `SongInfo`.
- The return-trip record and the identity tags are written as §7.8 describes. A snapshot stays in the project, so a later merge works even for files exported before 6e lands.
- The output is checked against the vendored XSD in tests (§12).

**Import:**
- `Tracks` and `Channels` become tracks, strips and buses. Group (folder) tracks become a bus that their children output to (Changed: no folders).
- A note track's instrument plug-in is matched in the plug-in catalogue by format and ID. If it is not installed, it becomes the existing offline placeholder that keeps its state (parent §10), so the next export still carries it.
- VST2 and VST3 devices are Left out, by name. A note track with no instrument plays the built-in synth (Note).
- **Re-sharing (controller's call):** note clips that share a name and have identical content become uses of one new sequence, as do looped clips (with their loop count) and clips with a `reference`. Other note clips go straight onto the track. A VizDAW → DAWproject → VizDAW round trip therefore restores shared sequences.
- Times in beats map to ticks. Times in seconds map through the imported tempo map. Fades convert to frames with linear shapes.
- Warps are Left out (the clip is placed unstretched). Clip launcher slots become unplaced sequences named "<scene> – <track>" (Changed). VCA channels and video are Left out.
- The built-in `Equalizer` device maps onto Multi-Q's bands where the band types match, once 6g has landed. Otherwise it, `Compressor`, `NoiseGate` and `Limiter` are Left out with their settings listed.
- External audio (`external="true"`) resolves next to the `.dawproject` file. Missing audio becomes offline regions, with the existing relink.

### 7.5 AAF (plan 6f)

**Export** writes a folder "<Song> AAF" holding:
- `<Song>.aaf`, following the Edit Protocol, with 4 KiB sectors;
- `<Song> MIDI.mid` (type 1, with the tempo map and one track per MIDI track), when the sequence has MIDI;
- `Audio/`, when the audio is external.

**Defaults for DAW-to-DAW exchange (controller's call).** These suit the owner's partners: one reads AAF beside DAWproject, and one reads AAF and MIDI files only. Video post is not the target.

| Setting | Default | Options | Why |
|---|---|---|---|
| Audio | Embedded in the `.aaf` | External WAV in `Audio/` | One file to hand over; DAW AAF importers all read embedded PCM |
| Sample format | WAV PCM at the project rate, 24-bit | 16-bit; float projects are dithered to 24 | Integer PCM is what every AAF reader accepts |
| Stereo | Pairs of mono slots, named "<track> L" and "<track> R" | Stereo slots | Mono slots are the most widely read form. VizDAW's own import joins the pairs back (below) |
| Start | Bar 1 at 00:00:00:00, at 25 fps | 01:00:00:00; 24, 29.97 drop-frame and 30 fps | A DAW session usually starts at zero; 01:00:00:00 is a video-post habit |
| Fades and crossfades | Transitions | — | The form AAF readers expect |
| Clip gain, track volume and pan automation | Included | Each can be turned off | Some readers apply them badly; the option is the way out |
| MIDI | `<Song> MIDI.mid` beside it, type 1 with the tempo map | Off when there is no MIDI | The MIDI file is how bars and MIDI reach the other side |

Details:
- The composition's edit rate is the sample rate, so positions are exact to the sample.
- Fades: linear and equal-power are exact; exponential and S-curve are ≈. Crossfades (§7.0) are transitions between the two clips, with the same shapes. Region gain is a constant `MonoAudioGain`. Fader and pan automation is track-level `MonoAudioGain` and `MonoAudioPan` with varying values, using VizDAW's fader law.
- Markers are descriptive markers.

**Import:**
- Mobs and slots resolve to tracks. Clips, fades and transitions, constant and varying clip gain, and track gain and pan are mapped. Markers, names and comments carry over.
- The composition's start timecode becomes bar 1.
- Seconds map to ticks through the tempo map. If a MIDI file sits beside the AAF with the same stem (or one is chosen in the sheet), its tempo map and MIDI tracks are imported with it, so bars line up.
- **Positions.** VizDAW positions are ticks: 256 × BPM per second. Below about 187 BPM at 48 kHz (172 BPM at 44.1 kHz), a tick is coarser than a sample, so AAF → VizDAW positions are within half a tick, and VizDAW → AAF → VizDAW returns the same ticks. The report says Changed only when a position moved by more than one sample.
- **Crossfades.** Transitions between two clips become crossfades (§7.0): exact for linear and equal-power, other interpolations ≈. A transition that does not fit the crossfade rules (for example, longer than the outgoing clip) becomes plain fades (Changed).
- **Mono pairs.** Two mono slots named "<x> L" and "<x> R" (also ".L"/".R" and "Left"/"Right") with identical clip layouts are joined into one stereo track. Their media is written as one stereo file. "Join L/R pairs" is on by default in the sheet.
- Varying clip gain becomes a constant at its first value (Changed), since regions have no gain envelope.
- Embedded essence (WAVE, AIFC, PCM descriptors) is streamed out to `Media/.incoming/` and converted. External locators resolve relative to the AAF's folder, then by file name in a folder the sheet offers. MXF and unreadable media become offline regions (§3, owner).
- Multichannel essence over two channels splits into mono tracks (SP5 §5.1).

### 7.6 Export for another DAW (plan 6b)

One sheet, one job. It writes a folder "<Song> for another DAW" holding:
- `Audio/NN <track>.wav`: one stem per chosen strip (default: every track's strip and every bus fed only by sends), from the 5f-2 offline bounce, post-fader as heard;
- `MIDI/NN <track>.mid`: one type-1 SMF per MIDI track, each carrying the tempo map, so any one of them lines up on its own;
- `MIDI/All MIDI.mid`;
- `Tempo map.mid`: the conductor track only, with markers;
- `About these files.txt`: tempo, meter, sample rate, start position, the song info, the track list and the report.

Details:
- **Every file starts at the same point. (owner: bar 1)** **(controller's call)** If the sequence has material before bar 1 (its start offset), everything starts at that earlier point instead, and `About these files.txt` and the sheet say so.
- Every stem runs the same length: to the song's end plus the bounce tail.
- Silent stems (external MIDI) are not written, and the report lists them.
- The format choices are the bounce sheet's: WAV, AIFF or CAF; depth; rate; dither.
- **"Add the stems to this project" (Ardour's re-import idea)**, off by default. It places the stems on new tracks at the start point as one undo step, using the bounce's existing add-to-project path.

### 7.7 OMF (plan 6h)

OMF 2.0 import only, from the OMF Interchange Specification 2.1:
- a Bento container reader (§6.5 limits);
- the composition mapped as in §7.5: clips, fades, transitions as crossfades, clip gain, track volume and pan, names, embedded AIFC and WAVE, external media;
- no metadata beyond names.

OMF 1.0 is refused. It shares 6f's mapping from a timeline to tracks, so it follows 6f.

### 7.8 Return trip: merging a DAWproject back (plans 6d and 6e, owner)

The owner wants a project that went out to another DAW to come back without losing what only VizDAW knows. **(owner)** On import of a DAWproject that came from VizDAW, a prompt asks **"New Project"** or **"Merge With Existing Project"**. The rest of this section is the **controller's call**.

**The catch, and the design that answers it.** Another DAW that opens our DAWproject and later exports its own will almost certainly not carry our private files or our XML IDs. A record kept only inside the ZIP would therefore be lost on exactly the trip it is for. So the record lives in three places:
1. **In the ZIP:** `vizdaw/record.json`, a private record holding the project ID, the export ID and time, the VizDAW-only state that the format cannot carry, and a map from every exported XML ID to the VizDAW object it came from (track, strip, sequence use, region, marker, automation target). DAWproject readers ignore files they do not reference. If the file comes straight back (or the other DAW keeps it), matching is exact.
2. **In fields other DAWs tend to keep:** the export ID goes into `metadata.xml`'s `Comment` (as one line, `VizDAW export <id>`, after the user's own comment), and each `Track`, `Channel` and `Clip` `comment` attribute gets a short tag `vz:<object id>`. The import strips these tags again, so they never reach the user's text.
3. **In the project itself:** each DAWproject export writes a snapshot, the **base**, to the package as `Interchange/<export id>.json`. It holds what was exported, in the projection form of §12, plus the ID map. The last 10 exports are kept, and older ones are pruned on save. The snapshot is a package file, not part of the document format.

**When the prompt appears.** It appears when the file has the record, or the metadata tag, or `vz:` tags on most of its tracks. A file with none of these opens as a New Project with no prompt. **(controller's call: a merge needs a base, so a file VizDAW never exported cannot merge.)**

**Which project.**
1. An open document whose `project_id` matches.
2. Else, the prompt's "Merge With Existing Project" offers an Open panel for a `.vizdaw`, which VizDAW opens and checks.
3. A project with a different ID, or without a snapshot for that export ID, is refused with a message ("This file was not exported from that project"). The one exception: if the export ID is missing but the project has snapshots, the user may pick one from a list (time, track count), and matching uses the heuristics below.

**Matching** (three-way: the base, *theirs* = the returning file, *ours* = the project now):
- **Tracks and strips:** by `vz:` tag or record ID. Else by name and content type among the unmatched. Else by order among the unmatched of the same type. A track still unmatched is new.
- **Notes:** within each matched track, by pitch, start and duration, with a tolerance of one tick at 15 360 per quarter (converted from the file's beats).
- **Audio clips:** by tag, else by media (content hash of the decoded audio, or file name) plus position.
- **Markers:** by tag, else by name and position. **Automation:** by target, as whole curves. **Tempo and meter maps:** as whole maps. **Song info:** field by field.
- **Shared sequences:** each exported use is matched to its use. If every use changed the same way, the edit goes into the shared sequence. If the uses now differ, the differing uses become private copies (Make Private Copy), and the report says so.

**Merge rules:**

| Base → theirs | Base → ours | Result |
|---|---|---|
| Changed | Unchanged | Theirs is applied |
| Unchanged | Changed | Ours is kept |
| Changed | Changed, the same way | Applied once |
| Changed | Changed differently | **Conflict.** For a track's notes or clips, ours stays the active take, and theirs is added as a new alternate take, "Returned <date>". For a setting (a level, a name, a tempo map, a curve), ours is kept and theirs is listed with its value. |
| Deleted | Unchanged | Removed (reported; undoable) |
| Deleted | Changed | Kept, listed as a conflict |
| New | — | Added, as an import would add it |

- **What a merge takes from theirs:** notes and per-note expression, audio clips, fades and crossfades, region names and gain, levels, pan, mute, solo, sends, plug-in state of matched plug-ins, automation, markers, the tempo and meter maps, track and strip names and colours, and song info.
- **What a merge always keeps from ours:** everything the format cannot carry, whatever theirs says. That is other sequences, keyboard slots, inactive takes, input effects, inputs and monitoring, `return_of`, MPE input settings, drum rows, automation modes, MIDI learn, surfaces, the project's audio format and templates. A plug-in theirs replaced with a VST plug-in stays ours, listed.
- **The merge is one undo step**, "Undo Merge DAWproject". It is applied as one command batch in one history group. New media is staged as in §6.2.
- **Before applying,** the prompt's second page shows a preview: counts of changes applied, kept, added, removed and in conflict, with the conflict list. Merge or Cancel.
- **The report** (§6.3) gains a "Conflicts" group, alongside Changed, Left out and Notes.
- **AAF does not merge. (controller's call)** AAF has no place another DAW reliably carries for our tags or a private stream, and no musical time for matching notes. AAF imports stay "New Project" or "Add to this project".

## 8. Multi-Q in the EQ slot (plan 6g)

- **Licence and build.** Multi-Q is GPL-3.0 (github.com/dusk-audio/dusk-audio-plugins). **(owner)** Before building, 6g checks the licence of the pinned commit and of the framework it builds on, and records both in the third-party notices. A script, `scripts/build-multiq.sh`, builds a pinned commit from source as CLAP and AU. CI caches the result. VizDAW never downloads it at run time.
- **Where it lives.** The CLAP goes inside `VizDAW.app/Contents/PlugIns/` and is signed with the app. `core-host`'s CLAP search paths put the app's own folder first. The AU is shipped inside the app as well. A Settings › Plug-ins button, "Install Multi-Q for other apps", copies it to `~/Library/Audio/Plug-Ins/Components`, so a project's EQ settings survive a DAWproject trip to another DAW. **(controller's call)** Nothing is installed without that click.
- **New channels.** New audio-track, instrument and built-in-synth strips get Multi-Q in their EQ slot, at its flat default. Buses and master do not, and keep "EQ ＋". A Settings switch, "Put an EQ on new channels", is on by default. **(controller's call)**
- **Imports and the existing slot.** Strips made by an import get Multi-Q too, unless the import fills their EQ slot. The slot's code still assumes no particular EQ (UI-polish §3).
- **Fallback.** If Multi-Q's licence or build fails the check, 6g switches to ZL Equalizer 2 with a CLAP build through clap-juce-extensions, which is the owner's recorded fallback. 6g reports which one it shipped.
- 6g also adds the DAWproject `Equalizer` mapping (§7.4) if 6d has landed. Otherwise 6d adds it.

## 9. UI

**The File menu (controller's call)** keeps Import Audio… (⇧⌘I) and Bounce… and adds:
- **Import MIDI File…** (SMF or MIDI Clip File, by content);
- **Import Project ▸** DAWproject… · AAF… · OMF…;
- **Export ▸**
  - MIDI File…
  - MIDI Clip File…
  - DAWproject… — "Most complete: tracks, mix, plug-ins, automation"
  - AAF… — "Audio, fades and volume; MIDI as MIDI files beside it"
  - For Another DAW… — "Audio stems and MIDI files for any DAW"
- **Last Import or Export Report…**, which reopens the latest report (per document, not saved).

**The Region menu** gains Crossfade and Remove Crossfade (§7.0).

**The return prompt** (§7.8): "This DAWproject came from a VizDAW project." It offers **New Project** and **Merge With Existing Project…** (the matching open project is named when there is one). Merge leads to the preview page, then Merge or Cancel.

**Sheets.** Each format has one options sheet after the Open or Save panel:
- **MIDI import:** where (this project at the playhead, or a new project); the channel → destination table; split by channel; treat as MPE; use the file's tempo map.
- **MIDI export:** type; resolution; tracks; per-note expression as MPE.
- **DAWproject export:** region gain applied to the audio, or left out.
- **AAF export:** embedded or external; depth; mono pairs or stereo; start timecode and rate; clip gain, volume and pan on or off. **AAF and OMF import:** where; the MIDI file beside it; the media folder; join L/R pairs.
- **Export for another DAW:** strips; audio format; add to this project.

Every export sheet shows **"What won't transfer"**: the report from a dry run, updated as options change, before the user clicks Export.

**The report** opens as a sheet when a job finishes:
- a one-line summary ("Imported 12 tracks, 340 regions, 4 210 notes, the tempo map and 8 markers");
- then "Conflicts" (merges only), "Changed", "Left out" and "Notes" groups, each with counts and disclosure rows naming tracks and bars;
- Copy, Save as Text… and, for exports, Show in Finder.

A report with nothing in it says "Everything transferred."

Every new control is accessible-labelled and follows the theme tokens (SP7).

## 10. Control channel

New registry entries, in the existing style (`media.import`, `file.bounce`): **(controller's call)**

| ID | Menu title | Parameters (answering the dialogs) |
|---|---|---|
| `file.import_midi` | Import MIDI File… | `path`, `new_project`, `seq`, `lane`, `tick`, `channel_map` (`[{group, channel, dest}]`), `split_channels`, `mpe`, `use_tempo` |
| `file.export_midi` | Export › MIDI File… | `path`, `replace`, `seq`, `type` (`0`/`1`), `ppq` (`480`/`960`/`15360`), `lanes`, `mpe` |
| `file.export_midi_clip` | Export › MIDI Clip File… | `path` (a file, or a folder for several tracks), `replace`, `seq`, `lanes`, `start`, `end` |
| `file.import_dawproject` | Import Project › DAWproject… | `path`, `mode` (`new`/`merge`; required when the file came from VizDAW, otherwise only `new`), `project` (a `.vizdaw` path, for merge when no open project matches), `snapshot` (an export ID, when the file lost its own), `preview` (`true`: return the merge preview and change nothing) |
| `file.export_dawproject` | Export › DAWproject… | `path`, `replace`, `seq`, `region_gain` (`apply`/`leave_out`) |
| `file.import_aaf` | Import Project › AAF… | `path`, `new_project`, `midi_path`, `media_folder` |
| `file.export_aaf` | Export › AAF… | `path` (folder), `replace`, `seq`, `embed`, `depth`, `stereo` (`mono_pairs`/`stereo`), `start_tc`, `fps`, `clip_gain`, `automation` |
| `file.import_omf` | Import Project › OMF… | `path`, `new_project`, `media_folder` |
| `region.crossfade` | Region › Crossfade | `seq`, `lane`, `event`, `length_ms`, `shape` (§7.0) |
| `region.remove_crossfade` | Region › Remove Crossfade | `seq`, `lane`, `event` |
| `file.export_for_daw` | Export › For Another DAW… | `path` (folder), `seq`, `strips`, `container`, `depth`, `rate`, `dither`, `add_to_project` |
| `file.interchange_report` | Last Import or Export Report… | — (returns the report; never opens the sheet when called programmatically) |

- All of them return a job, as `media.import` does. The job's result carries the report rows, so agents can assert on warnings.
- Export commands also take `dry_run: true`, which returns the "What won't transfer" report without writing.
- A DAWproject from VizDAW without `mode` answers `dialog_required` naming "Return". Merge without a matching open project and without `project` answers `dialog_required` naming "Open".
- Without `path`, a call answers `dialog_required` naming "Open" or "Save". Busy, recording and running-job states answer `busy` or `unavailable`.

**Hostile-input tests for each command**, following the control-channel spec §11:
- the minimal valid form;
- `path` missing, empty, relative, a directory, a FIFO, `/dev/zero`, a symlink loop, over 4 KiB, containing NUL, non-UTF-8, or pointing inside the open project's package (refused for exports);
- an existing file without `replace`;
- unknown enum values, `ppq` 0 and 70 000, ticks at ±2⁴⁰;
- a `channel_map` with duplicates, out-of-range channels or unknown destinations;
- `lanes` and `strips` naming nothing or belonging to another sequence;
- merge with `mode` `merge` on a file without a record, a `project` that is not a package, belongs to another project ID or is the open document under another path, an unknown or malformed `snapshot`, and a merge while the target project is recording;
- crossfade values as listed in §7.0.

2048 random parameter maps per command never panic. Every refusal names the command and the parameter.

## 11. Errors

- **Refused files** say what and why in one sentence, e.g. "This MIDI file can't be read: track 3 ends in the middle of an event." or "This is an OMF 1.0 file; VizDAW reads OMF 2.0 only."
- **Partial imports** finish, and their report lists what was left out and why.
- **Missing media** becomes offline regions with the existing relink. Running out of disk space fails the job and leaves nothing behind.
- **Exports** never leave half files. Overwriting asks first (the save panel's confirmation, or `replace`).
- **Plug-ins** that fail to give their state at export are Left out by name, and the export continues.

## 12. Testing

- **Golden files.** We make them, so they may be redistributed under GPL-3.0: hand-built fixtures and our own exporter's output, checked in under `crates/core-io/tests/data/`, with a README saying how each was made. Third-party sample files (from other DAWs, LibAAF's tests, the DAWproject repo) are committed only if their licence allows redistribution, with the source and licence recorded. Any others are used locally through `VIZDAW_INTERCHANGE_SAMPLES=<dir>` and skipped in CI.
- **Round trips (VizDAW → format → VizDAW).** For each route, a projection function keeps what the route carries (§7.1). Proptest generates documents with the existing model generators, exports, imports, and checks:
  - exact rows equal;
  - approximate rows within the stated tolerance (7-bit values, one sample, the PPQ grid);
  - the report lists exactly the expected kinds.

  Fixed cases cover nested references, takes, MPE, drum tracks, ramps, a pickup before bar 1, shared plug-in instruments, stereo and mono pairs, crossfades and 1 000 tracks.
- **Crossfades (6c).** The engine has golden renders for each shape, and for a crossfade beside a plain fade-out. A null test checks that a crossfade at the overlap equals the two regions mixed by hand. The undo property is extended to `Crossfade` and `RemoveCrossfade`. Moving, trimming and deleting keep or drop the flag as validated. `rt-check` covers the new path. The v8 → v9 migration is tested.
- **Merge (6e).** Three-way tests built from a base document:
  - theirs-only edits are applied;
  - ours-only edits are kept;
  - both-changed cases become conflicts, with an alternate take or a listed value;
  - deletions follow the table;
  - VizDAW-only state is byte-equal after a merge;
  - one undo restores the project exactly.

  The same cases run with the record stripped from the ZIP, with the `vz:` tags stripped, and with both stripped (the heuristic path, with the snapshot chosen). Proptest generates edit scripts applied to theirs and ours, and checks that the merge never panics, always validates, and is one undo step. Hostile records cover a malformed `record.json`, IDs pointing at nothing, IDs pointing at the wrong kind, duplicated IDs and huge maps (§6.5 limits apply).
- **Format checks.**
  - SMF: byte-exact goldens for each writer option.
  - Clip File: goldens and UMP decode/encode properties.
  - DAWproject: every export passes `xmllint --schema` against the vendored XSDs (in CI; `xmllint` ships with macOS); ZIP layout goldens.
  - AAF: our reader reads our writer's files for every option. A local, optional check runs LibAAF's `aaftool` over our exports.
- **Plug-in state.** CLAP state round-trips through DAWproject with the in-repo test plug-ins (`crates/test-plugins`). AU state round-trips with one of the system's built-in Audio Units.
- **Hostile files (never a crash).** A proptest harness, as for damaged projects, with case counts from `VIZDAW_FUZZ_CASES`:
  - byte damage (flip, insert, delete, truncate) on every golden;
  - structure-aware generators: chunk soup for SMF; UMP soup for the Clip File; ZIP with traversal names, duplicates, bombs, encryption flags and ZIP64 edge sizes; XML with DOCTYPE, deep nesting, huge numbers, NaN, dangling and cyclic IDREFs; compound files with cyclic FAT and directory chains, oversize streams and cyclic object references; Bento with cyclic chains.

  Each case is refused or imports with a report. Never a panic, never `VizError::Internal`, and memory and time stay bounded (each case under 2 s and 256 MiB). cargo-fuzz targets are optional, as before.
- **Limits.** One test per row of §6.5, at the limit and one past it.
- **Jobs.** Cancel at every stage leaves `Media/` and the export folder unchanged. Undo of an import into the project restores the document exactly (the existing undo property extended).
- **Control channel.** As in §10, plus one e2e (`crates/vizctl/tests/e2e.rs`, `#[ignore]`) that exports DAWproject, AAF and For Another DAW from the demo project into a temp folder, re-imports each, and checks the track count and report through `state.get`. Offline audio, throwaway HOME, no real devices or user folders.
- **README by-eye checks**: in `app/README.md`, continuing from the last number when each plan starts (256 at the time of writing, so 257 onwards). Each plan adds steps for its sheets and its report. **Owner checks per route**, written without naming the DAWs ("the DAW you use that reads …"), are manual steps only the owner can do:
  - **DAWproject (6d, 6e):** export the demo song; open it in the DAW that reads DAWproject; check tracks, clips, fades, notes with expression, tempo, markers, levels, sends and automation. Move a note, change a level and add a marker there, export a DAWproject back, merge it into the original, and check that the three edits arrived and that keyboard slots, input effects and other sequences are still there. Undo the merge.
  - **AAF (6f):** export with the defaults; open the AAF and the MIDI file beside it in each DAW that reads AAF; check clip positions to the sample (a click track lines up), fades, crossfades, clip gain, volume and pan automation, markers and track names. Export an AAF from that DAW and import it into VizDAW.
  - **Export for another DAW (6b):** drop the folder's stems and MIDI files at bar 1 in the DAW that reads only audio and MIDI files; check they line up and the tempo is right.
  - **Crossfades (6c):** make, drag, type and remove a crossfade, and listen through it.

## 13. Plans **(controller's call)**

Plans run one after another by subagent-driven development, as SP5's and SP7's did, in the order 6a, 6b, 6c, 6d, 6e, 6f, 6g, 6h.

| Plan | Deliverable | Needs |
|---|---|---|
| **6a — MIDI files and the shared base** | `core-io` crate; the report; the limits; format version 8 (`SongInfo`, region names, project ID) with Project Settings › Song info; SMF import and export; MIDI Clip File import and export; the MIDI sheets; `file.import_midi`, `file.export_midi`, `file.export_midi_clip`, `file.interchange_report`; the report sheet; drag-in of MIDI files; File › Open for `.mid` and `.midi2`; README steps. | — |
| **6b — Export for another DAW** | §7.6 on the 5f-2 bounce and 6a's SMF writer; the add-to-project option; `file.export_for_daw`. It is small, and gives the owner a working route early. | 6a |
| **6c — Crossfades** | §7.0: format version 9, commands, the engine's segment change, editor handles and typed values, the Region menu items, `region.crossfade` and `region.remove_crossfade`, golden renders. A VizDAW feature in its own right. | — (it comes before 6d and 6f, which use it) |
| **6d — DAWproject** | ZIP and XML readers and writers; the §7.4 mappings, including re-sharing, crossfades, plug-in state and the offline placeholder; the return-trip record, tags and snapshots (§7.8, write side only); XSD checks; `file.import_dawproject` (New Project), `file.export_dawproject`. | 6a, 6c |
| **6e — DAWproject merge** | §7.8 read side: the prompt, finding the project, matching, the three-way merge, the conflicts take, the preview, the Conflicts group, `mode`/`project`/`snapshot`/`preview` on `file.import_dawproject`. **(controller's call)** It is its own plan: the matching and merge rules are the riskiest code in SP6 and deserve their own review. | 6d |
| **6f — AAF** | The compound-file object model, reader and writer on `cfb`; §7.5 import and export with the DAW-to-DAW defaults, the SMF beside it, transitions as crossfades and L/R joining; `file.import_aaf`, `file.export_aaf`. | 6a, 6c |
| **6g — Multi-Q** | §8: build script, bundling, search path, new-channel default, "Install for other apps", the fallback check, and the DAWproject `Equalizer` mapping if 6d is in. | — (it may run at any point) |
| **6h — OMF import** | The Bento reader and the OMF 2.0 mapping on 6f's timeline code; `file.import_omf`. It may move to after launch. **(owner)** | 6f |

Each plan ends with its control-channel commands, hostile-input tests, README steps and backlog rows for what it found.

## 14. Backlog rows this spec adds

- A project archive with FLAC (Ardour's idea).
- An MXF reader, for AAF clips that point at MXF audio (owner, 2026-11-11: not now).
- SMF type 2.
- The MIDI 2.0 container file, once published.
- OMF 1.0.
- OMF export.
- Clip gain envelopes.
- Marker colours and comments.
- Folder tracks.
- Merging AAF back into a project, if a reliable way to carry IDs through other DAWs turns up.

## 15. Settled and open questions

Settled by the owner on 2026-11-11, after the builder check:
- round trip with a New Project or Merge prompt (§7.8);
- real crossfades on one track (§7.0);
- MXF not now (§3);
- the target DAWs (§1, unnamed);
- SP6 before SP8;
- conflicts in a merge: VizDAW's changes win (owner: "Vizdaw changes win"). A returning take becomes an alternate take, and a returning setting is listed but not applied (§7.8).

No questions are open.
