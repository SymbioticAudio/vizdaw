# Co-Producer Creative Help and Starter Projects Implementation Plan (8e)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Co-Producer can help creatively. "Set up a new template project for me, I'd like it to be in a sort of 80's Synthwave style" makes a new untitled project in one call. It sets up sounds and routing only: instruments, a drum kit, buses, effects, tempo, meter and markers. It uses the composer's installed plug-ins and falls back to the Built-in Synth and Multi-Q. It writes no notes or beats. "Help me figure out how to mix my drums" and the bus recipes (a shared reverb, parallel compression, delay into reverb) are written-down recipes that Claude reads and then follows with 8d's mixer commands. Automation is written as real points: fades, rides and filter sweeps. Claude sees the curve in a view before it is written. New parts, such as an arpeggio of held chords, a reharmonised line or a variation, go onto a new Co-Producer track beside the original. Anything that adds notes or automation to the composer's own tracks shows exactly what changes and waits for their yes.

**Architecture:**
- **`core-control`.**
  - Three registry commands:
    - `automation.write` (change, previewable);
    - `midi.add_notes` (change, previewable);
    - `midi.new_part` (add).
  - Three protocol commands on the Co-Producer socket: `co_producer.starter`, `co_producer.midi_notes` and `co_producer.automation`.
  - A new `starter` module parses and checks a starter project's spec. It is the only parser; `core-ffi` calls it again.
- **`core-model`.**
  - `Command::WriteAutomation`, on the recording pass's existing `write_automation`.
  - `diff` learns automation and added notes.
- **`core-ffi`.**
  - A new `creative` module builds the core-model commands for the three registry commands. The preview and the apply share them.
  - A new `starter` module resolves a starter's plug-ins with the fallback and builds the project as one step.
  - The new `Session` methods live in a child module, `session/co_producer_creative.rs`, so the release-fixes merge meets one new line in `session.rs`.
- **Swift.**
  - `CreativeCommands` runs the three registry commands through one Session call.
  - `CoProducerCommands` answers the three protocol commands. `co_producer.starter` opens a new untitled document, as File › New does.
- **`vizdaw-mcp`.**
  - Four new tools: `starter_project`, `read_midi_notes`, `preview_automation` and `recipes`.
  - The recipes are also served as MCP prompts (Ruling 6).
  - A fifth MCP Apps view, `ui://vizdaw/automation`, on 8c's view machinery.

**Tech Stack:**
- Rust 2024: `core-control`, `core-model`, `core-ffi` with UniFFI 0.32.2, `vizdaw-mcp`, `vizctl`.
- Swift 6.1 / SwiftPM with Swift Testing, macOS 14+.
- The view is one self-contained HTML file with plain inline JavaScript, built into `vizdaw-mcp` with `include_str!`, as 8c's and 8d's are.
- **No new third-party dependency.**

**Spec:** `docs/superpowers/specs/2026-11-14-co-producer-design.md`, approved by the owner on 2026-11-14, is binding. This plan builds its §12 row **8e**:
- §8, creative help: any installed plug-in, web search by Claude, mix recipes, starter templates, edits on the composer's material, and automation as real points;
- §10's gaps 8 (write notes), 9 (automation points) and 14 (tempo and meter, for templates);
- §6.3's automation preview view.

**The owner's answers are binding too** (2026-11-14):
- Starter projects set up sounds and routing only: instruments, a drum kit, buses, effects, tempo, meter and markers.
- They write no notes or beats.
- Sounds come from any plug-in the composer has installed.
- Claude does any web search with its own tools. VizDAW only exposes the installed plug-in list (8d's `list_plugins`) and never goes online.
- Changes to the routing or sound of the composer's tracks need a yes. Co-Producer's own new tracks and buses are free.

It builds on these plans, whose rulings stay in force:
- plan 8a, `docs/superpowers/plans/2026-11-14-co-producer-8a.md`: Rulings 1–13 and pre-flight rulings P1–P11 (`.superpowers/sdd/2026-11-14-co-producer-8a/preflight.md`);
- plan 8b, `docs/superpowers/plans/2026-11-16-co-producer-8b.md`: Rulings 1–10, including the marks and `ChangeItem.co_producers`;
- plan 8c, `docs/superpowers/plans/2026-11-16-co-producer-8c.md`: Rulings 1–8, including the markers, the views and `MAX_SHOWN`;
- plan 8d, `docs/superpowers/plans/2026-11-16-co-producer-8d.md`: Rulings 1–5, including `decimal`, `resolve_plugin`, `PluginLookup`, `describe_with` and the strip-decides rule.

Executors read all five.

**The release-fixes plan.** It is `docs/superpowers/plans/2026-11-16-release-fixes.md` on branch `release-fixes`, and it merges after this branch. 8e depends on two of its commands, and adds neither of them:
- **`region.set_fade_shape`** is the fade-shapes gap (spec §10 gap 11). 8d's plan said 8e reuses its name and shape exactly and makes it previewable. That command does not exist on this branch, so the work is in "At the release-fixes merge" below. It is not a task of this plan. Until then, Co-Producer draws fades as fader automation points (Ruling 2).
- **`destination.set_sound`** gives the Built-in Synth five General MIDI sounds. Until it merges, a starter's fallback instrument is the existing Built-in Synth with its present sound (Ruling 5). Every instrument track shares that synth's one destination (`tracks.rs` `synth_dest`), and the starter's answer says so. Choosing a sound per role needs a Built-in Synth per track, which release-fixes moves to 27.1.0.
- **At the merge**, the registry count tests take 8e's count plus the fixes' 4. `session/co_producer_creative.rs` and the fixes' `session/fixes.rs` are separate files.

**Not in this plan:**
- **The feel recipe.** The backlog row "Feel: per-track timing lean, and a humanize that isn't random" names a Co-Producer recipe, "give this a live feel". The feature is placed in 27.1.0, so 8e does not build it, and humanising is not one of 8e's recipes. When that row ships, its recipe joins Ruling 6's table. Task 7 notes this on the row.
- **Rendering the input-FX arpeggiator offline.** The spec's parenthesis names the 5g-1 arpeggiator. No offline render exists, so Claude works out the arpeggio's notes itself and writes them with `midi.new_part` (Ruling 3). A backlog row covers rendering the arpeggiator into notes. (Controller's call.)
- **Meter peaks for gain staging.** 8d's backlog row gave this to 8e. The engine's strip peaks are read and cleared by the mixer window at 30 Hz, so a second reader needs its own peak hold in `core-engine`. That is too much for 26.0.0. The drum recipe gain-stages from the fader levels `show_mixer` reports and asks the composer to play the song and read the meters. The row moves to 27.1.0. (Controller's call.)
- **Marks on notes and automation.** 8b left these to 8e. 8e does not need them. A write onto the composer's track always asks (Ruling 4), and a write onto Co-Producer's own track is decided by the track's or strip's mark. So the project format does not change. A backlog row keeps the per-event marks.
- **Deleting or replacing the composer's notes.** Co-Producer adds notes. It never removes the original's notes in 8e: the composer mutes the original if they like the new part better. A backlog row covers "replace the original's notes".
- **Tempo and meter changes on an existing song.** A starter sets them as it makes the song. Changing them later is a backlog row.
- **Plug-in categories** (reverb, delay, compressor) in `list_plugins`. Claude picks by name and maker, using its own knowledge and web search. A backlog row covers reading the CLAP features and AU types.

## Plan Rulings

These are this plan's decisions where the spec or 8a–8d leave a choice. Each one names its cost if wrong.

1. **The creative commands (spec §10 gaps 8 and 9).**
   - **Common shape.** All three are `key_only`, with `document: true`. Every target is a required ID, so no preview depends on the selection (8a Ruling 6). Ticks are at `PPQ` 15 360, and the tool descriptions say so.
   - **The commands.** The title is the undo label outside a request.

     | id | params | class | title | menu | core-model |
     |---|---|---|---|---|---|
     | `automation.write` | `seq` (Int ≥ 1); `target` (Text, `automation.gesture`'s grammar); `start`, `end` (Int ≥ 0, `end > start`); `points` (Text, a point list, 0–1024 points) | change | "Write Automation" | Mixer | `Command::WriteAutomation` |
     | `midi.add_notes` | `seq`; `lane` (Int ≥ 1); `notes` (Text, a note list, 1–1024 notes) | change | "Add Notes" | Track | `EventCmd::Insert` into the track's active take |
     | `midi.new_part` | `seq`; `lane`; `name` (Text, 1–64 characters after trimming, with 8a P11's character rules); `notes` (1–1024 notes) | add | "New Part" | Track | a new track below `lane`, playing `lane`'s destination, then `EventCmd::Insert` |

   - **A note list** is entries separated by one or more ASCII spaces. Each entry is `<start>:<pitch>:<length>:<velocity>`, in ASCII digits only:
     - `start` in ticks from the song's start, at most 1 000 000 000;
     - `pitch` 0–127;
     - `length` 1 to 983 040 ticks (64 quarter notes);
     - `velocity` 1–127, mapped as 8c's velocity `set` maps it (`input::upscale_velocity`).

     The text is at most 32 768 bytes. `registry::note_list` refuses anything else with "Note <n>: <what> is <range>." or "Note <n> is not start:pitch:length:velocity.".
   - **A point list** is entries separated by one or more ASCII spaces. Each entry is `<tick>:<value>`:
     - `tick` is ASCII digits, within `start..=end`, and strictly after the point before it;
     - `value` passes `registry::decimal(text, -1_000_000, 1_000_000)`.

     An empty list clears `start..=end`. The text is at most 32 768 bytes. `registry::point_list` refuses with "Point <n> is not tick:value.", "Point <n> is outside start to end." or "Point <n> is not after point <n−1>.".
   - **Values are in the target's own units**, so Claude never works in the hidden 0–1 scale:
     - `fader` and `send`: dB, −144..=12, where −144 is −∞;
     - `pan`: percent, −100..=100, where negative is left;
     - `mute` and `solo`: 0 or 1;
     - `param`: the plug-in's own `min..=max`, whole values for a stepped parameter.

     `core-ffi` converts them as `automation.gesture`'s targets are mapped: `fader_norm` for fader and send levels, `pan_norm(p / 100)` for pan, and `(v − min) / (max − min)` for a parameter. A value out of range is refused with 8d's sentences: "<target> is <min> to <max>." and "<param name> takes whole values.". A parameter needs its plug-in running, as 8d's `mixer.set_param` does.
   - **Refusals found from the document** are `VizError::Model`, and a preview reports them as `failed` with nothing changed:
     - a target that does not exist: "This song has no <target>.";
     - a `seq` that is not top-level, or unknown: "This song has no sequence <seq>.";
     - a `lane` not in `seq`: "Track <id> is not in sequence <seq>.";
     - an audio or drum track for `midi.add_notes` or `midi.new_part`: "Track <name> is not a MIDI or instrument track.". Drum tracks are left for a backlog row because their rows map pitches.
   - **Cost if wrong:** 1024 notes or points per call is about 64 bars of sixteenths. A longer part takes several calls in one request.
2. **Automation as real points (spec §8, §6.3).**
   - **`Command::WriteAutomation(AutomationWrite)`** is new in `core-model`. It runs the recording pass's own `check_write` and `write_automation` (`command/record.rs`), so the points land as a live pass's do. Its `name()` is "Write Automation". `lane_name` is "<strip name> <fader|pan|mute|solo|send n|param name> Automation", cut to 64 characters.
   - **The range is replaced.** Points already in `start..=end` on that target go, and the new ones take their place, as a Write pass does. An empty list clears the range.
   - **Fades, rides and sweeps.** A fade is fader points, a ride is fader points across a phrase, and a filter sweep is a `param` target on a filter's cutoff. These are recipes (Ruling 6), not commands.
   - **Cost if wrong:** a write over a range with hand-drawn points replaces them. The preview says how many points go (Ruling 4), and a yes is needed on the composer's strips.
3. **New parts beside the original (spec §8, "edits on the composer's material").**
   - **Reading.** `co_producer.midi_notes {seq, lane, start?, end?, window?}` is read. It answers `{doc, seq, lane, name, notes: [[start, pitch, length, velocity]], truncated}` for the active take's notes, at most 4096, by start then pitch. `start` and `end` are both given or neither, with `end > start`. Velocities are 7-bit, through the inverse of `upscale_velocity`.
   - **Writing.** By default Claude writes the result with `midi.new_part`, as a new track below the original that plays the same destination. Being an add, it needs no yes, and 8b's marks name it "<name> (Co-Producer)". Arpeggios, reharmonised lines and variations are all worked out by Claude and written this way.
   - **Onto an existing track.** `midi.add_notes` adds notes to a track. On a Co-Producer track it applies without asking; on the composer's it asks (Ruling 4). It never removes or changes a note already there.
   - **Cost if wrong:** a composer who wanted the original replaced mutes it by hand, or asks Claude to mute its strip (8d's `mixer.set_strip`, with a yes).
4. **Who must say yes (spec §6.5; the owner's rule; 8d Ruling 2).**
   - **`automation.write`.** The target's strip decides, by 8d Ruling 2: `automation::strip_of(target)`, or for a parameter the slot's strip, or an instrument slot's main strip (`diff::instrument_strip`). So automation on a Co-Producer bus is free. Automation on the composer's Kick asks. A parameter of a Co-Producer insert on the composer's Bass asks too, because it changes the sound of the composer's track.
   - **`midi.add_notes`.** The track decides. `composers_work` is true when the lane was in `before` and is not marked as Co-Producer's (8b Ruling 1). This narrows 8c Ruling 3 for this command alone: 8c's quantize, transpose and velocity chores still always ask.
   - **New automation tracks.** A track that `WriteAutomation` makes is new in `after`, so its own item is not the composer's. The strip's item decides.
   - **Asking.** `needs_yes` is true when any item is the composer's work, as 8a–8d compute it. A plan whose items are all Co-Producer's applies at once, and `change` answers `applied_without_asking` (8c Ruling 8).
   - **What the preview says.** `diff::describe_with` gains two parts:
     - On the strip's item (8d Ruling 4), after the parameters: `<label> automation <n> points → <m> points, bars <a>–<b>`. `<label>` is `fader`, `pan`, `mute`, `solo`, `send to <bus>` or `<slot name> <param name>`. The bars are 1-based, from the sequence's meter, as the transport counts them. With `m == 0` it reads `<label> automation cleared, bars <a>–<b>`.
     - On the track's item, when notes were only added: `<n> notes added, bars <a>–<b>`. This replaces 8c's `"12 notes → 76 notes"` part for that case. Any other note change keeps 8c's parts.
   - **Cost if wrong:** one extra question when Co-Producer automates its own insert on a composer's track.
5. **Starter projects (spec §8; the owner's answers).**
   - **One call.** `co_producer.starter {spec}` is class add and needs no open request. It:
     1. checks the whole spec in `core-control` (`starter::parse`) before anything happens, so a bad spec opens no window;
     2. opens a new untitled document, as File › New does with the default project format. This is the one window a Co-Producer call may open, because the spec says a template makes an untitled project (§8);
     3. in that document, opens the request "start <title>", builds the project as one step, and ends the request. The undo step is "Co-Producer: Start <Title>". 8b's notes entry for an untitled song goes to its support folder.
   - **The spec** is a JSON object with no other keys:
     - `title`: 1–64 characters after trimming, with P11's rules;
     - `tempo`: Text, `decimal` 20–300 bpm, stored as `Bpm` thousandths;
     - `meter`: `"<num>/<den>"`, with `num` 1–32 and `den` one of 1, 2, 4, 8, 16, 32;
     - `markers`: 0–32 of `{bar: Int 1–999, name}`, with 8c's marker-name rules;
     - `buses`: 0–16 of `{name, effects?, output?}`;
     - `tracks`: 1–32 of `{name, kind, plugin?, effects?, output?, sends?}`, where:
       - `kind` is `instrument` or `audio`;
       - `plugin` is only for `instrument` ("plugin is only for instrument tracks."). Without it, the track plays the Built-in Synth;
       - `effects` is 0–8 plug-in references, in insert order;
       - `output` is the name of a bus in `buses`; without it, the track goes to master;
       - `sends` is 0–8 of `{bus, level_db?}`, with `level_db` by `decimal` −144..=12, default `"0"`.
   - **Spec checks** (each `invalid_params`):
     - names are 1–64 characters, with P11's rules, and unique across buses and tracks ignoring case: "Two strips are named <name>.";
     - every `output` and send `bus` names a bus in the spec: "No bus <name> in the starter project.";
     - bus outputs make no cycle: "Buses <a> and <b> feed each other.", naming the first two in the cycle;
     - plug-in references have 8d Ruling 3's shape;
     - an unknown key anywhere: "Unknown key <key> in the starter project.".
   - **Using what is installed.** `starter::resolve` resolves every reference by 8d Ruling 3's `resolve_plugin`, and never fails the starter over a plug-in. When an instrument is not usable, the track plays the Built-in Synth. When an effect is not usable, Multi-Q takes its place if the strip has no Multi-Q insert yet; otherwise it is left out. With no Multi-Q in the build, it is left out. Each fallback adds a line to the answer:
     - "<track>: <8d's refusal>; it plays the Built-in Synth instead.";
     - "<strip>: <8d's refusal>; Multi-Q is in its place.";
     - "<strip>: <8d's refusal>; left out.".

     When any track plays the Built-in Synth, the answer adds "Tracks on the Built-in Synth share its one sound for now.".
   - **The build.** It is one transaction, inside `acting`, so 8b's marks name every track and bus "… (Co-Producer)". In order:
     1. `SetTempo` with `TempoMap::constant` and `SetMeter` on the document's sequence;
     2. `SetMarkers`, with each bar's tick from the new meter;
     3. the buses, as `mixer.new_bus` makes them (each gets the default EQ, as 8d says);
     4. the tracks: an instrument track by 8d's `track.new_plugin_instrument` path, or an audio track by `track.new_audio`'s;
     5. the inserts, outputs and sends, through 8d's `mixer_chores::commands_for` with the IDs the earlier steps made.

     **It writes no notes, regions or automation.** Task 4's test checks that every track has no events.
   - **The answer:** `{doc, window, title, undo_label, tracks: [{name, plays}], buses: [{name, effects}], lines}`.
     - `plays` is the instrument's catalog name, `"Built-in Synth"` or `"audio"`;
     - `effects` names each insert as loaded.
   - **One at a time.** A second `co_producer.starter` while one is building is `busy` "A starter project is being made.".
   - **Cost if wrong:** a starter with nothing fitting installed is all Built-in Synth tracks sharing one sound, with Multi-Q on the buses. It says so, and Claude can suggest instruments to install.
6. **Recipes are a tool and MCP prompts, from one table (spec §8, §11.12).**
   - **The call.** Recipes are written-down steps Claude follows with 8d's and 8e's commands. They are served two ways, both from `vizdaw-mcp`'s `recipes::RECIPES`:
     - the tool `recipes {name?}`, read only, because a tool is what Claude itself can reach when the composer simply types "help me mix my drums";
     - MCP prompts (`prompts/list`, `prompts/get`), because a host may offer prompts to the composer as menu items or slash commands.

     They are not resources. A host shows resources to the composer to attach, so Claude cannot count on reading one. They are not only tool descriptions either, because the full steps would bloat every `tools/list`. (Controller's call.)
   - **The table.** Each recipe has a `name`, a `title`, and steps written as plain text. Each one starts "Start a request with begin_request." and ends "End the request with end_request, and tell the composer what was added and what changed.". The steps hold these points, in this order:

     | name | title | steps |
     |---|---|---|
     | `starter_project` | Set up a starter project in a style | Read list_plugins for instruments and for effects. Choose roles that suit the style (for 80s synthwave: drums, bass, pad, lead and arpeggio parts, a gated reverb and a delay bus). Pick installed plug-ins by name; search the web with your own tools if unsure which installed plug-ins fit. Choose a typical tempo and meter, and markers for the sections (Intro, Verse, Chorus, Bridge, Outro). Call starter_project once. Never write notes or beats. Read its lines aloud: they say which roles fell back to the Built-in Synth or Multi-Q. |
     | `mix_drums` | Help me mix my drums | Find the drum tracks with show_tracks and show_mixer, and ask the composer which they are if unsure. Make a bus "Drums" with mixer_new_bus. Plan routing each drum strip to it in one change: the composer sees the list and says yes. Gain-stage from the fader levels show_mixer reports: aim the loudest drum near −6 dB on its fader, and ask the composer to play the song and say what the meters show. Put an EQ (multi-q) and a compressor from list_plugins on the bus; on its own bus this applies without asking. Offer parallel compression next (parallel_compression). |
     | `shared_reverb` | A shared reverb bus | Make a bus "Reverb", put a reverb from list_plugins on it, set its mix parameter fully wet with plugin_params and mixer.set_param, and plan one send from each chosen track at about −12 dB, post-fader, in one change. |
     | `parallel_compression` | Parallel compression | Make a bus "Crush", put a compressor on it set hard (fast attack, high ratio), and plan a send from the source track or bus at 0 dB, post-fader. Bring the bus's fader up from −∞ until it thickens the sound; start near −10 dB. |
     | `delay_into_reverb` | Delay into reverb | Make or reuse the "Reverb" bus, make a bus "Delay" with a delay plug-in fully wet, route "Delay" to "Reverb" with mixer.set_output (both are Co-Producer's, so no question), and plan sends to "Delay" from the chosen tracks. |
     | `automation_moves` | Fades, rides and filter sweeps | Read the curve first with preview_automation, and show it. Then write it with change and automation.write. A fade is fader points from the level to −144 dB; a ride is a few fader points across a phrase, a dB or two up or down; a sweep is points on a filter's cutoff parameter from plugin_params. Use the target's own units. |
     | `new_part` | A new part from the composer's own | Read the notes with read_midi_notes. Work out the new part yourself: an arpeggio of the held chords, a reharmonised line, or a variation. Write it with midi_new_part as a new track below the original, which needs no yes. Never change the original's notes; if the composer wants to hear only the new part, offer to mute the original's strip. |

   - **Plug-ins that are not there.** Every mix recipe adds: "If list_plugins has no fitting effect, say so, set up the bus with multi-q only, and suggest the kind of plug-in to install."
   - **No banned names.** Recipe text is scanned like every other text in `vizdaw-mcp/src` (`names.rs`). It names no plug-in, maker or product.
   - **The tool.** `recipes {}` answers "Recipes:" and one "- <name>: <title>" line per recipe. `recipes {name}` answers "<title>:" and its steps, one "- <step>" line each. An unknown name is `isError: true` "No recipe <name>. Call recipes with no name for the list.", with the name passed through `rpc::truncate` and 8a's `disguises`.
     - Its description, exactly: "Read Co-Producer's recipes for creative help: starter projects, mixing drums, a shared reverb, parallel compression, delay into reverb, automation moves, and new parts from the composer's own. Call it with no name for the list, then with a name for its steps, before you start. Changes nothing."
   - **The prompts.** `capabilities` gains `prompts: {listChanged: false}`.
     - `prompts/list` gives one prompt per recipe, with its `name` and `title` and the title as its `description`. `starter_project` has one optional argument, `style`; the others have none.
     - `prompts/get {name, arguments?}` gives one `user` message whose text is "<title>." plus the steps, and for `starter_project` with a style, "The style: <style>.".
     - `style` is at most 200 characters, with no control character, and passes `disguises`. Anything else is `-32602` "style is up to 200 characters of plain text.". An unknown prompt is `-32602` "No prompt <name>.".
   - **The server's instructions** gain: "For creative help (a starter project, mixing, automation, a new part), read the matching recipe with the recipes tool first."
   - **Cost if wrong:** a host that never shows prompts still has the tool. A recipe that reads badly is a text change with no code risk.
7. **The tools and the automation view (spec §6.1, §6.3, §11.12).**
   - **`starter_project {spec}`** sends `co_producer.starter`. Its `inputSchema` gives the spec's shape, with `additionalProperties: false` at each level and Ruling 5's limits as `maxItems`, `maxLength`, `minimum` and `maximum`.
     - Its text: "Made a new song, "<title>", in window <window>. Undo step: "<undo_label>"." Then "Tracks:" with one "- <name>: <plays>" line each, "Buses:" with one "- <name>: <effects joined by ", " or none>" line each, and then each fallback line.
     - Its description, exactly: "Make a new untitled song set up in a style: tempo, meter, section markers, instrument and audio tracks, buses, effects, outputs and sends. It writes no notes or beats. Use plug-ins from list_plugins; an instrument or effect that is not usable falls back to the Built-in Synth or Multi-Q, and the answer says which. Pass window from the answer to later tools to work in the new song. Read the starter_project recipe first."
   - **`read_midi_notes {seq, lane, start?, end?}`**, read only, sends `co_producer.midi_notes`.
     - Its text: "Track <lane>, <name>: N notes (start:pitch:length:velocity, 15360 ticks per quarter note):", then the notes as one note list, wrapped at 80 notes a line. If `truncated`, it adds "- and more; read a shorter range".
     - Its description, exactly: "Read a track's notes as start:pitch:length:velocity, in ticks at 15360 per quarter note, for working out a new part. Changes nothing."
   - **`preview_automation {seq, target, start, end, points}`**, read only.
     - It checks `points` with `registry::point_list` itself, then sends `co_producer.automation {seq, target, start, end}`. That answers `{doc, target, target_name, unit, min, max, start_bar, end_bar, points: [[tick, value]], truncated}`, with values in the target's units, at most 4096 points.
     - Its text: "<target_name>, bars <start_bar>–<end_bar>: now N points, planned M points." Then "Planned:" with one "- tick <t>: <value> <unit>" line each, at most 40, then "- and N more".
     - Its `structuredContent` is `{view: "automation", target_name, unit, min, max, start, end, existing, planned}`.
     - Its description, exactly: "Show the automation curve you plan to write over the one that is there now, before writing it with automation.write through change. Values are in the target's own units: dB for fader and send, percent for pan (negative is left), 0 or 1 for mute and solo, and the plug-in's own range for a param. Changes nothing."
   - **The automation view** is `ui://vizdaw/automation`, added to 8c's `VIEWS` for `preview_automation`.
     - It draws one SVG chart: the existing points as one line and the planned points as a second, over `start..end` and `min..max`. It labels the target and the unit.
     - The SVG is built with `createElementNS` and `setAttribute` of numbers only. Every name is set with `textContent`. Points that are not finite are skipped.
     - It follows 8c Ruling 7: no data of its own, nothing from the network, and the host's light or dark palette.
     - It has no controls: it is a picture.
   - **`change`'s description** adds, after 8d's sentence: "creative writes: automation points with automation.write, and notes added to an existing track with midi.add_notes. On Co-Producer's own tracks and buses these apply without asking."
   - **The registry tools.** `midi_new_part` appears through 8a Ruling 12, being an add. Its `notes` has `maxLength` 32 768. `automation.write` and `midi.add_notes` are run only through `change`.
   - **Safe text.** Every name in a text or `structuredContent` has 8a's `disguises` characters replaced by U+FFFD, as 8c's `show_tracks` does.
   - **Cost if wrong:** a curve of more than 4096 existing points shows the first 4096, and the text says "truncated".

## Global Constraints

Every task's requirements include these.

- **Shell.** Every command runs after `export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools` and `cd /Users/lattawb/workspace/vizdaw`.
- **Swift.**
  - Build only through `scripts/build-ffi.sh` (after any FFI change) and `scripts/swift.sh build|test`, plus `scripts/make-app.sh` in Tasks 5 and 7. Never run `swift`, `xcodebuild` or `xcrun` directly.
  - Swift 6.1:
    - no `[weak self]`, and no non-Sendable value captured inside `MainActor.assumeIsolated`;
    - no `@State` or `@FocusState`: view state lives in the `@Observable` models.
  - Every new control has an accessibility label, and colours, fonts and spacing come from theme tokens (`theme.text…`, `theme.type…`, `theme.spacing…`).
- **Tests never touch the machine.**
  - They never open real audio or MIDI, and never trigger the microphone or TCC prompt.
  - They never touch the real `~/Library`: Application Support/VizDAW, preferences, the Trash, `Audio/Plug-Ins`. Use temp folders, `MemoryDefaults()`, the e2e's throwaway home, and fakes. Trash moves in tests go through `debug_set_trash_dir`.
  - Socket tests use a private temp folder under `/tmp`, as `server.rs`'s `private_tmp` does. `vizdaw-mcp` tests set `VIZDAW_SUPPORT_DIR` on the child they spawn, never in their own process.
  - Every `core-ffi` test file calls `force_offline()` through its `serial` helper, and points the support folder at a temp folder as `co_producer_copies.rs` does.
  - The catalog in `core-ffi` tests comes from `catalog::add_test_bundle(&test_plugins::bundle())`, and Multi-Q from `bundled::debug_set_multiq`, as `default_eq.rs` does. Nothing scans the real plug-in folders.
  - Parameter values in `core-ffi` tests come from 8d's fake `PluginLookup`. This plan loads no real plug-in in a test.
  - Swift tests that make a real `Session` set `TestSupportDir.shared`.
  - Linux CI has no AU or CoreFoundation, so any test that needs either is `#[cfg(target_os = "macos")]`. `vizdaw-mcp`, the `core-control` tests and the `core-model` tests are plain Unix and run on Linux.
  - The e2e uses vizctl's throwaway home.
- **Tools.**
  - Never run `auval`.
  - No GUI, computer-use or accessibility-automation tools. Launch VizDAW only through the e2e with `VIZDAW_AUDIO=offline`. Afterwards `pgrep -lf MacOS/VizDAW` prints nothing.
  - Never use `dangerouslyDisableSandbox`.
  - Never read the owner's off-limits research folder outside this repo, which 8b's Global Constraints name. Its path holds a banned name, so this plan does not spell it.
- **Programmatic calls open nothing**, with one exception. Control-channel and Co-Producer calls, and programmatic calls, never open dialogs, sheets, alerts, panels, menus or windows. Loading a plug-in through Co-Producer never opens its editor. The exception is `co_producer.starter`, which opens one new document window, as File › New does (Ruling 5). It opens no dialog, and a refused spec opens nothing.
- **Multi-Q (plan 6g).**
  - CLAP is the built-in default: Co-Producer's `multi-q`, and either Multi-Q ID, load the bundled CLAP (8d Ruling 3).
  - The restart refresh applies only to the bundled Multi-Q: `crates/core-host` and `crates/platform-macos` do not change in this plan, and 6g's refresh tests stay green.
- **Hostile input.**
  - Every MCP input is hostile: every MCP message, every tool and prompt argument, every Co-Producer call, every starter spec, every plug-in name and maker from the catalog, every parameter name from a plug-in, every note and point list, and every name a view draws.
  - Every new FFI export runs inside `ffi_guard` (and so `catch_unwind`). It returns `Result<_, VizError>`, or a safe default for the never-failing ones.
  - `vizdaw-mcp` handles each message inside `std::panic::catch_unwind`. A panic answers `-32603` and the server goes on.
  - Each new command and call gets hostile-input tests: wrong types, unknown params or keys, missing required params, a 70 000-character string, a NUL, IDs out of range or unknown, and IDs of the wrong kind (an audio track for notes, a slot as a strip).
  - Every planned command that does not run gets its line in the reply (8a Ruling 13).
- **The composer's yes.** `automation.write` and `midi.add_notes` are previewable and run only through `co_producer.apply` (8a Rulings 6–8; Ruling 4). A direct `command.run` of either on the Co-Producer socket stays `not_allowed`.
- **No notes or beats in a starter.** A starter project writes no events of any kind (Ruling 5).
- **Counts.** Check every count at the start of Task 1, and report each task's change against it.
  - The registry grows by **3**, from the count measured at the start. It is expected to be 209, after 8d's +13.
  - `COMMAND_NAMES` grows by **3**, expected 24 → 27. `ErrorCode::ALL` stays as measured, expected 16.
  - `PREVIEWABLE` grows by **2**, expected 24 → 26.
- **Names.**
  - The repo and UI never name the vendors and DAWs on the project's banned list. The list lives only in the name scans (`app/Tests/VizDAWAppModelTests/NameScanTests.swift`, `crates/core-control/tests/names.rs`); read them, keep all new text clear of them, and never copy the list.
  - The list includes the feature's old working name, which plan 8a added to the scans (spec, "Name"). Like every banned name, it is never written in the repo, this plan included.
  - Undo names, tool descriptions, recipes, prompts, refusals, starter lines and the view's text are covered by the same rule. Recipes and examples name styles and kinds of plug-in, never a product or maker. Multi-Q's maker is named only through `MULTIQ_CLAP_ID` and `MULTIQ_AU_ID`, never in new text.
  - The feature is "Co-Producer" everywhere.
  - Text the app or a view shows says "track", never "lane". Identifiers and JSON keys keep `lane`.
- **Merging with release-fixes.** `session.rs` gains only `mod co_producer_creative;` and the one-line hooks Tasks 3 and 4 name. The new Session methods live in `crates/core-ffi/src/session/co_producer_creative.rs`. This plan adds no command named in the release-fixes plan.
- **Commits.**
  - Stay on branch `takes-and-co-producer`. Never switch branches. Never push: the owner is asked before every push.
  - Other agents work in this worktree. Commit at the end of every task by explicit paths, never `git add -A`, and retry when the index is locked. Never commit `.superpowers/`, `.DS_Store`, `target/`, generated FFI (`app/Sources/VizDAWCore/vizdaw_ffi.swift`, `app/Sources/vizdaw_ffiFFI/include/`, `target/ffi/`) or `dist/`.
  - Every message ends with exactly these two lines:
    ```
    Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
    Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P
    ```
  - Check each commit with `git log -1 --oneline && git show --stat HEAD`.
  - `Cargo.lock` does not change.
- **Brand.** Brand assets are never in the repo. `VIZDAW_NO_BRAND=1` builds without them.
- **Lints.**
  - Every crate keeps `#![deny(missing_docs)]` and the `clippy::{unwrap_used, expect_used, indexing_slicing, panic}` denials outside tests. `vizdaw-mcp` keeps `#![forbid(unsafe_code)]`.
  - `core-model` stays free of OS APIs, apart from the `std::fs` that `file.rs` already uses. Plug-in resolution is `core-ffi`'s.
- **Format.** This plan changes no project format: the track and strip marks of 8b decide every yes (Ruling 4). `core_model::file::VERSION` stays as measured at the start, expected 10.
- **TDD.** Every new or changed test is run and seen to fail before the code that makes it pass.
- **Each task ends green.**
  - Always:
    - `cargo fmt --all -- --check`;
    - `cargo clippy --workspace --all-targets -- -D warnings`;
    - the Linux clippy: `cargo clippy --workspace --all-targets --target x86_64-unknown-linux-gnu --target-dir target/linux-check -- -D warnings`;
    - `cargo test` for the crates the task touches.
  - Tasks 1, 3, 4 and 5 also run `cargo clippy -p core-ffi --lib -- -D warnings`, `scripts/build-ffi.sh`, `scripts/swift.sh build` and `scripts/swift.sh test`.
  - Known flaky: the `damaged_files` and `dawproject_fuzz` proptests, `platform-macos/tests/midi.rs`, `core-ffi` catalog tests under load, and the latency e2e timing. Re-run once before investigating, and say so in the report.
  - **Baseline: check at start.** At the start of Task 1, measure and record:
    - HEAD, which is 8d's last commit on `takes-and-co-producer`, or this plan's pre-flight commit if there is one;
    - the registry count (expected 209);
    - `COMMAND_NAMES` (expected 24) and `ErrorCode::ALL` (expected 16);
    - `PREVIEWABLE`'s length (expected 24, after 8d);
    - the last README step (expected 377, after 8d Task 7);
    - the e2e's ignored tests (expected 36);
    - `VERSION` (expected 10);
    - whether `region.set_fade_shape` is in the registry (expected: no, since release-fixes merges later).

    If 8a, 8b, 8c and 8d are not all fully committed, stop and say so. If a measured value differs from the expected one, use the measured value and report the difference. If release-fixes has already merged, say so: the "At the release-fixes merge" steps then run after Task 7.

## Review Focus

These are the five failure modes a composer is most likely to hit, most likely first. Each is pinned by named tests in its task.

1. **A creative write reaches the composer's track without a yes.**
   - Examples:
     - fader automation on the composer's Kick;
     - a filter sweep on a Co-Producer insert that sits on the composer's Bass;
     - notes added to the composer's Piano track;
     - a plan holding free automation on a Co-Producer bus and notes on the composer's track.
   - Expected: each asks, and the list shows exactly the composer's items. Only a plan whose every item is Co-Producer's applies without asking.
   - Pinned by Task 3's `creative_writes_ask_for_the_composers_work` and Task 1's `creative_changes_only_through_a_preview`.
2. **A starter project meets an install with nothing fitting.**
   - Examples:
     - an instrument turned off in the Plugin Manager, blocklisted or missing;
     - an effect given where an instrument is wanted;
     - every effect unusable;
     - a build without Multi-Q.
   - Expected: the project is still made, every fallback has its line, and no starter fails over a plug-in.
   - Pinned by Task 4's `starters_fall_back_to_built_ins` and `starters_never_fail_over_a_plugin`.
3. **A starter writes notes, or a bad spec opens a window.**
   - Examples:
     - any track with an event after the build;
     - a spec with a bus cycle, two strips named "Bass", 33 tracks, `bar: 0`, `tempo: "1e3"`, `meter: "4/3"` or an unknown key.
   - Expected: every starter track has no events, and a bad spec is refused before any window opens.
   - Pinned by Task 4's `starters_write_no_notes`, Task 1's `starter_specs_refuse_hostile_input` and Task 5's `badStarterOpensNoWindow`.
4. **Odd notes and points.**
   - Examples:
     - a point outside `start..end`, two points out of order, or 1025 points;
     - a fader value of 20 dB, or a stepped parameter given 0.5;
     - a parameter of a plug-in still loading;
     - a note of pitch 128 or length 0;
     - notes for an audio track, or for a track of another sequence.
   - Expected: Ruling 1's refusal, at preview, with the document unchanged.
   - Pinned by Task 1's `note_and_point_lists_parse_strictly` and Task 3's `odd_creative_targets_are_refused_at_preview`.
5. **The automation view, recipes and prompts meet hostile or unusual data.**
   - Examples:
     - a parameter named `<img src=x onerror=alert(1)>`;
     - 4096 existing points and 1024 planned;
     - a host without MCP Apps;
     - a prompt `style` holding a bidi mark or 10 000 characters;
     - a recipe name of 70 000 characters.
   - Expected:
     - names draw as text, with disguises replaced;
     - lists stop where Ruling 7 says, and say so;
     - with no extension, no `_meta.ui` and no `structuredContent`, and the same text;
     - prompt and recipe refusals as Ruling 6 says.
   - Pinned by Task 6's `automation_view_draws_names_as_text`, `the_automation_view_only_for_hosts_that_ask` and `recipes_and_prompts_refuse_hostile_input`.

---

### Task 1: The registry, protocol and starter spec (`core-control`)

**Files:**
- Create:
  - `crates/core-control/src/starter.rs` (`StarterSpec` and `parse`);
  - `crates/core-control/tests/starter.rs`.
- Modify:
  - `crates/core-control/src/registry.rs` (the three entries and their params; `note_list`, `point_list`);
  - `crates/core-control/src/request.rs` (`COMMAND_NAMES`; `co_producer.starter`, `co_producer.midi_notes`, `co_producer.automation`);
  - `crates/core-control/src/co_producer.rs` (`PREVIEWABLE`; `admit` for the three new commands);
  - `crates/core-control/src/lib.rs`;
  - `crates/core-control/tests/co_producer.rs`, `tests/commands.rs`, `tests/protocol.rs`;
  - `crates/core-ffi/src/control.rs` (the `ControlRequest` variants and their mapping, so the workspace builds);
  - `app/Sources/VizDAWAppModel/CreativeCommands.swift` (new: the three ids, answering `.failed(.of(.unavailable, "Not ready yet."))` until Task 5);
  - `app/Sources/VizDAWAppModel/CoProducerCommands.swift` (the three new commands answer the same until Task 5).

**Interfaces:**
- Consumes: 8a's `co_producer::{admit, PREVIEWABLE}` and `registry::validate_args`; 8d's `registry::{decimal, PLUGIN_REF_MAX}` and its plug-in reference shape check; 8a P11's name rules; 8c's marker-name rules.
- Produces:
  ```rust
  // registry.rs
  pub const CREATIVE_IDS: &[&str] = &["automation.write", "midi.add_notes", "midi.new_part"];
  pub const MAX_NOTES: usize = 1024;
  pub const MAX_POINTS: usize = 1024;
  pub const LIST_TEXT_MAX: usize = 32_768;
  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  pub struct NoteSpec { pub start: u64, pub pitch: u8, pub length: u64, pub velocity: u8 }
  /// Ruling 1's note list: 1..=MAX_NOTES notes.
  pub fn note_list(text: &str) -> Result<Vec<NoteSpec>, String>;
  /// Ruling 1's point list: 0..=MAX_POINTS points, ticks in start..=end, strictly rising.
  pub fn point_list(text: &str, start: u64, end: u64) -> Result<Vec<(u64, f64)>, String>;
  // starter.rs
  pub enum StarterKind { Instrument, Audio }
  pub struct StarterSend { pub bus: String, pub level_db: f64 }
  pub struct StarterBus { pub name: String, pub effects: Vec<String>, pub output: Option<String> }
  pub struct StarterTrack { pub name: String, pub kind: StarterKind, pub plugin: Option<String>,
      pub effects: Vec<String>, pub output: Option<String>, pub sends: Vec<StarterSend> }
  pub struct StarterSpec { pub title: String, pub tempo_milli: u32, pub meter: (u8, u8),
      pub markers: Vec<(u32, String)>, pub buses: Vec<StarterBus>, pub tracks: Vec<StarterTrack> }
  /// Ruling 5's checks, in full.
  pub fn parse(v: &serde_json::Value) -> Result<StarterSpec, String>;
  pub const MAX_TRACKS: usize = 32; pub const MAX_BUSES: usize = 16; pub const MAX_MARKERS: usize = 32;
  // request.rs, Command gains:
  CoProducerStarter { spec: serde_json::Value },   // checked by starter::parse in admit
  CoProducerMidiNotes { seq: u64, lane: u64, range: Option<(u64, u64)>, window: Option<WindowRef> },
  CoProducerAutomation { seq: u64, target: String, start: u64, end: u64, window: Option<WindowRef> },
  // co_producer.rs: PREVIEWABLE gains "automation.write" and "midi.add_notes", after 8d's.
  ```
  In `core-ffi/src/control.rs`, `ControlRequest` gains `CoProducerStarter { spec: String }` (the spec's JSON text), `CoProducerMidiNotes { seq, lane, start, end, window }` and `CoProducerAutomation { seq, target, start, end, window }`.

- [ ] **Step 1: Measure the baseline** (Global Constraints) and write it at the top of the task report.
- [ ] **Step 2: Write the failing tests.**
  - In `tests/co_producer.rs`:
    - `creative_commands_have_their_class`:
      - `automation.write` and `midi.add_notes` are `Change`, and `midi.new_part` is `Add`;
      - `list_json()` lists all three, with `previewable: true` for the first two and `false` for `midi.new_part`.
    - `creative_changes_only_through_a_preview`:
      - `command.run` of `automation.write` or `midi.add_notes`, with valid params, is `not_allowed` with 8a Ruling 6's direct-call sentence;
      - a `co_producer.preview` plan holding each is admitted;
      - `command.run midi.new_part {seq: 1, lane: 2, name: "Arp", notes: "0:60:3840:100"}` is admitted.
    - `creative_commands_refuse_hostile_input`. Each is `invalid_params`:
      - `seq` or `lane` missing, `0`, `-1`, `"2"` or `2^53`;
      - `automation.write` with `end` equal to `start`, a `target` of `"fader:x"`, `"vel:1"` or 70 000 characters, or `points` of 32 769 bytes;
      - `midi.new_part` with `name` of `""`, `"   "`, 65 characters or `"a\u{0}b"`;
      - an unknown param on any of the three.
    - `starter_and_reads_only_on_the_co_producer_socket`: on a developer server, all three protocol commands are `not_allowed` "only on the Co-Producer socket". On the Co-Producer socket they are admitted with no open request.
    - `reads_refuse_hostile_input`: `co_producer.midi_notes` with only `start`, or `end <= start`; `co_producer.automation` with no `target`, `end <= start`, or `{extra: 1}`. Each is `invalid_params`.
  - In `tests/starter.rs`:
    - `a_full_spec_parses`: the synthwave example below gives `tempo_milli` 118 000, `meter` (4, 4), 5 markers, 2 buses and 5 tracks, in order. The example is:
      - title "Synthwave";
      - tempo "118", meter "4/4";
      - markers Intro 1, Verse 9, Chorus 17, Bridge 33, Outro 41;
      - buses "Reverb" with `["clap:vizdaw.test.gain"]`, and "Delay" with output "Reverb";
      - tracks "Drums", "Bass", "Pad", "Lead" and "Arp", all `instrument`, with "Pad" sending to "Reverb" at "-12".
    - `starter_specs_refuse_hostile_input`. Each is `Err` with Ruling 5's sentence:
      - a missing `title` or `tracks`, or `tracks: []`;
      - 33 tracks, 17 buses or 33 markers;
      - `tempo` of `"19"`, `"301"`, `"1e3"`, `"NaN"` or `120` as a number;
      - `meter` of `"4/3"`, `"0/4"`, `"33/4"`, `"4/64"` or `"4"`;
      - a marker `bar` of 0 or 1000;
      - two strips named "Bass" and "bass";
      - an `output` or send to "Nowhere";
      - buses A → B → A;
      - `plugin` on an `audio` track;
      - 9 effects, 9 sends, a `level_db` of `"13"`;
      - a plug-in reference of `"vst:x"` or 513 characters;
      - an unknown key at the top, in a bus, in a track or in a send;
      - a name with a NUL, or a title of 70 000 characters.
  - In `tests/commands.rs`:
    - the registry count is the baseline plus 3, and `COMMAND_NAMES.len()` is the baseline plus 3;
    - the minimal-shape list gains `co_producer.starter`;
    - `note_and_point_lists_parse_strictly`:
      - `"0:60:3840:100 3840:64:3840:90"` gives two `NoteSpec`s;
      - pitch 128, length 0, length 983 041, velocity 0, velocity 128, `"0:60:3840"`, `"0:60:3840:100:1"`, `"-1:60:1:1"`, `"0:+60:1:1"`, 1025 notes and `""` are each `Err` with Ruling 1's message;
      - `point_list("0:-6 3840:0", 0, 3840)` gives two points;
      - `""` gives no points;
      - a tick outside `start..=end`, two equal ticks, a falling tick, `"0:1e3"`, `"0:NaN"`, `"0:"` and 1025 points are each `Err`.
  - In `tests/protocol.rs`: the round-trip parse of `co_producer.midi_notes {seq: 1, lane: 2, start: 0, end: 61440}`, `co_producer.automation {seq: 1, target: "fader:3", start: 0, end: 61440}` and `co_producer.starter {spec: <the example>}`.
- [ ] **Step 3: Run** `cargo test -p core-control`. Expected: FAIL.
- [ ] **Step 4: Implement** Ruling 1's entries and lists, Ruling 5's `starter::parse`, the three protocol commands and the `PREVIEWABLE` additions.
  - **The cross-param checks** (`end > start`; `start` and `end` together) go beside 8d's, as `registry::validate_args` runs them for both sockets. `point_list` runs there with the call's own `start` and `end`.
  - **`admit`** runs `starter::parse` on `co_producer.starter`'s spec, so the app never sees a bad one.
  - **The two other crates.** `core-ffi/src/control.rs` maps the new commands. Swift gets the temporary arms named in Files, so `everyRegisteredMenuCommandHasAHandler` passes.
- [ ] **Step 5: Run** `cargo test -p core-control -p core-ffi`. Expected: PASS, including `names.rs`.
- [ ] **Step 6: End-of-task checks**, including the Swift build and test. **Commit** `feat(control): Co-Producer creative commands, reads and the starter spec (plan 8e Task 1)`.

### Task 2: Automation writes and what the preview says (`core-model`)

**Files:**
- Modify:
  - `crates/core-model/src/command/mod.rs` (`Command::WriteAutomation`);
  - `crates/core-model/src/command/record.rs` (`check_write` and `write_automation` made `pub(crate)`);
  - `crates/core-model/src/diff.rs` (Ruling 4's two parts);
  - `crates/core-model/tests/diff.rs`;
  - `crates/core-model/tests/automation_write.rs` (new).

**Interfaces:**
- Consumes: `AutomationWrite`, `MAX_WRITE_POINTS`, `automation::strip_of`; 8d's `describe_with`, `ParamLookup`, `instrument_strip`, `MAX_PARTS`; 8c's note parts.
- Produces:
  ```rust
  // command/mod.rs
  /// Writes points over a range of one target, as a Write pass does (Ruling 2). Name "Write Automation".
  Command::WriteAutomation(AutomationWrite),
  // diff.rs
  /// The 1-based bar a tick falls in, from `seq`'s meter.
  pub fn bar_of(doc: &Document, seq: SeqId, tick: Tick) -> u32;
  ```

- [ ] **Step 1: Write the failing tests.**
  - In `tests/automation_write.rs`, on a `History` with strips Kick (unmarked) and "Drums (Co-Producer)" (marked):
    - `write_replaces_the_range`: three fader points in bars 1–2 on Kick, then a write of two points over bar 2 only, leave bar 1's point and the two new ones;
    - `an_empty_write_clears_the_range`: no points in the range after, and points outside it stay;
    - `write_makes_its_track_once`: the first write on a target with no automation track makes one named "Kick fader Automation", and a second write reuses it;
    - `write_is_one_undo_step`: one `undo` brings back the points as they were, named "Write Automation";
    - `write_refuses_what_a_pass_refuses`: a target that does not exist, or more than `MAX_WRITE_POINTS` points, is `Err`, and the document is unchanged.
  - In `tests/diff.rs`:
    - `automation_is_a_strip_part`:
      - 12 new fader points on Kick in bars 9–12 give "Strip Kick" / "fader automation 0 points → 12 points, bars 9–12";
      - a clear gives "fader automation cleared, bars 9–12";
      - a send target gives "send to Drums (Co-Producer) automation …";
      - with a lookup giving ("Cutoff", 0.5), a parameter target gives "Test Gain Cutoff automation …";
      - an instrument slot's parameter joins its main strip's item.
    - `bars_follow_the_meter`: in 3/4, tick 3 × 15 360 is bar 2.
    - `added_notes_say_so`: inserting 64 notes on a track with 12 gives "64 notes added, bars 1–4", and no "12 notes → 76 notes" part. A quantize still gives 8c's parts.
    - `automation_parts_count_toward_the_cap`: 20 parameter writes on one strip give 12 parts, then "and 8 more changes".
- [ ] **Step 2: Run** `cargo test -p core-model --test automation_write --test diff`. Expected: FAIL.
- [ ] **Step 3: Implement** Ruling 2's command and Ruling 4's parts, after 8d's parameter parts.
- [ ] **Step 4: Run** `cargo test -p core-model`. Expected: PASS, including 8a's `co_producer_requests` proptest, 8c's note and marker items and 8d's mixer items.
- [ ] **Step 5: End-of-task checks. Commit** `feat(model): automation writes and creative changes in the Co-Producer preview (plan 8e Task 2)`.

### Task 3: Creative commands, their previews and the yes (`core-ffi`)

**Files:**
- Create:
  - `crates/core-ffi/src/creative.rs` (`commands_for`, the unit conversions, the reads);
  - `crates/core-ffi/src/session/co_producer_creative.rs` (`run_creative`, `co_producer_midi_notes`, `co_producer_automation`);
  - `crates/core-ffi/tests/creative.rs`.
- Modify:
  - `crates/core-ffi/src/session.rs` (`mod co_producer_creative;` only);
  - `crates/core-ffi/src/lib.rs`;
  - `crates/core-ffi/src/co_producer.rs` (the arms of `preview_items` for `automation.write` and `midi.add_notes`; `composers_work` by Ruling 4);
  - `crates/core-ffi/src/tracks.rs` (a MIDI track below a given one, on its destination, for `midi.new_part`);
  - `crates/core-ffi/src/control.rs` (`ControlResult::{CoProducerMidiNotes, CoProducerAutomation}` and their JSON);
  - `app/Tests/VizDAWAppModelTests/FakeSession.swift` (the stubs).

**Interfaces:**
- Consumes: Task 1's ids, lists and requests; Task 2's `WriteAutomation`, `bar_of` and parts; 8d's `PluginLookup`, `describe_with`, `instrument_strip` and its Ruling 2; `automation::{fader_norm, fader_db, pan_norm, pan_of, param_plain}`; `parse_automation_target`; `input::upscale_velocity`; 8b's marks.
- Produces:
  ```rust
  // creative.rs
  /// The core-model commands for one of CREATIVE_IDS, in order; Ruling 1's refusals.
  pub fn commands_for(doc: &Document, id: &str, args: &[(String, ArgValue)], host: &dyn PluginLookup)
      -> Result<Vec<Command>, VizError>;
  /// A value in the target's units to the stored 0–1, or Ruling 1's refusal.
  pub fn to_stored(doc: &Document, target: AutomationTarget, value: f64, host: &dyn PluginLookup)
      -> Result<f64, VizError>;
  pub const MAX_READ: usize = 4096;
  #[derive(uniffi::Record)] pub struct NoteRow { pub start: u64, pub pitch: u8, pub length: u64, pub velocity: u8 }
  #[derive(uniffi::Record)] pub struct CoProducerNotesRow { pub seq: u64, pub lane: u64, pub name: String,
      pub notes: Vec<NoteRow>, pub truncated: bool }
  #[derive(uniffi::Record)] pub struct CurvePointRow { pub tick: u64, pub value: f64 }
  #[derive(uniffi::Record)] pub struct CoProducerCurveRow { pub target: String, pub target_name: String,
      pub unit: String, pub min: f64, pub max: f64, pub start_bar: u32, pub end_bar: u32,
      pub points: Vec<CurvePointRow>, pub truncated: bool }
  // session/co_producer_creative.rs
  impl Session {
      /// Runs one of CREATIVE_IDS as one step named its title; `Busy` while an edit is open.
      pub fn run_creative(&self, id: String, args: Vec<ControlArg>) -> Result<DocSnapshot, VizError>;
      pub fn co_producer_midi_notes(&self, seq: u64, lane: u64, start: Option<u64>, end: Option<u64>)
          -> Result<CoProducerNotesRow, VizError>;
      pub fn co_producer_automation(&self, seq: u64, target: String, start: u64, end: u64)
          -> Result<CoProducerCurveRow, VizError>;
  }
  // control.rs
  // ControlResult::CoProducerMidiNotes { doc, row } → {"doc", "seq", "lane", "name", "notes": [[s, p, l, v]], "truncated"}
  // ControlResult::CoProducerAutomation { doc, row } → {"doc", "target", "target_name", "unit", "min", "max",
  //     "start_bar", "end_bar", "points": [[tick, value]], "truncated"}
  ```
  `unit` is `"dB"`, `"%"`, `"on/off"` or `""` for a parameter.

- [ ] **Step 1: Write the failing tests** in `tests/creative.rs`, on a saved session, through a `serial` helper.
  - **The fixture.** The composer's tracks Piano (instrument, 12 notes in bars 1–4) and Vox (audio). Their strips are Piano and Vox, with a test gain insert on Vox. A request makes bus "Drums" by `mixer.new_bus`, which 8b marks "Drums (Co-Producer)". It also adds a Co-Producer test gain insert on Vox, with a yes. 8d's fake `PluginLookup` answers that insert's params: "Cutoff", 0–1, value 0.5; "Mode", 0–3, stepped.
  - `creative_commands_build_the_models_own`:
    - `automation.write {target: "fader:<Drums>", points: "0:-6 61440:0"}` builds one `WriteAutomation` with `fader_norm(-6.0)` and `fader_norm(0.0)`;
    - `pan` −30 is stored as `pan_norm(-0.3)`;
    - `param` 0.25 on Cutoff is stored as 0.25;
    - `midi.add_notes` builds one `EventCmd::Insert` with velocities through `upscale_velocity`;
    - `midi.new_part {lane: Piano, name: "Arp"}` builds a new track below Piano, on Piano's destination, then the insert.
  - `creative_writes_ask_for_the_composers_work`:
    - **Free.** `[automation.write {target: "fader:<Drums>", …}]` previews `needs_yes: false`.
    - **Asks.** `[automation.write {target: "fader:<Piano>", …}]` previews `needs_yes: true`, with "Strip Piano" / "fader automation 0 points → 2 points, bars 1–2" `composers_work: true`.
    - **Asks, for Co-Producer's insert on the composer's strip.** `automation.write {target: "param:<Vox insert>:<Cutoff>"}` previews `needs_yes: true`.
    - **Asks, for notes on the composer's track.** `midi.add_notes {lane: Piano}` previews `needs_yes: true` with "64 notes added, bars 1–4".
    - **Free, for notes on Co-Producer's track.** After `midi.new_part` makes "Arp (Co-Producer)", `midi.add_notes {lane: Arp}` previews `needs_yes: false`.
    - **A mixed plan asks.** Free automation on Drums plus notes on Piano previews `needs_yes: true`, and exactly the Piano item has `composers_work: true`.
  - `creative_previews_match_what_the_apply_does`: for each of the two previewable ids:
    - `co_producer_take`, then `run_creative` with the same args, gives a document whose `describe_with` from the start equals the preview's items;
    - a hand edit between the preview and the take makes the take fail with "The song changed since the preview; preview again.".
  - `odd_creative_targets_are_refused_at_preview`. Each previews to `Err` with Ruling 1's message, and the plan runs nothing:
    - `fader` 20 dB gives "fader is −144 to 12.";
    - pan 101 gives "pan is −100 to 100.";
    - mute 0.5 gives "mute is 0 to 1.";
    - Mode 0.5 gives "Mode takes whole values.";
    - the fake answering `None` gives 8d's not-running sentence;
    - `fader:999` gives "This song has no fader:999.";
    - `seq: 99` gives "This song has no sequence 99.";
    - `midi.add_notes {lane: Vox}` gives "Track Vox is not a MIDI or instrument track.";
    - a lane of another sequence gives "Track <id> is not in sequence <seq>.".
  - `new_parts_are_marked`:
    - in a request, `run_creative("midi.new_part", …)` adds one track "Arp (Co-Producer)" below Piano, playing Piano's destination, with exactly the given notes;
    - Piano's notes are unchanged;
    - it is one step "Co-Producer: <Title>";
    - outside a request, the undo label is "New Part".
  - `reads_change_nothing`:
    - `co_producer_midi_notes(Piano)` gives its 12 notes, 7-bit, by start then pitch, and a range gives only those inside it;
    - `co_producer_automation("fader:<Piano>", …)` after an applied write gives the points in dB, with `unit` "dB", `min` −144, `max` 12 and the bars;
    - neither changes the document's revision;
    - a track of 5000 notes gives 4096 with `truncated: true`;
    - an unknown lane, an unknown target, or `u64::MAX` is `Err`, with no panic.
  - `run_creative_refuses_hostile_input`: an unknown id, a missing arg, `notes` of 70 000 characters, and `u64::MAX` as `lane` are each `Err`, with no panic and no change.
  - `creative_results_json`: both read results serialise to the keys in Interfaces.
- [ ] **Step 2: Run** `cargo test -p core-ffi --test creative`. Expected: FAIL.
- [ ] **Step 3: Implement** Rulings 1–4.
  - **`commands_for`** reads the args by name, as 8d's does. Refusals found from the document come before any command is built.
  - **`preview_items`** gains one arm for the two ids. It executes `commands_for(h.doc(), …)` on the clone, describes with `describe_with` and the live lookup, and sets `composers_work` by Ruling 4.
  - **`run_creative`** follows 8d's `run_mixer`: it refuses with `Busy` while an edit is open, runs one transaction named the title, aborts on an error, then commits and marks.
  - **The reads** take the document as it stands and change nothing.
  - **Swift.** `FakeSession` gains the stubs.
- [ ] **Step 4: Run** `cargo test -p core-ffi`. Expected: PASS, including 8a's `co_producer`, 8b's `notes`, 8c's `chores` and 8d's `mixer_chores`.
- [ ] **Step 5: End-of-task checks**, including the Swift build and test. **Commit** `feat(ffi): Co-Producer automation points and new parts, with exact previews (plan 8e Task 3)`.

### Task 4: Starter projects (`core-ffi`)

**Files:**
- Create:
  - `crates/core-ffi/src/starter.rs` (`resolve`, `build`);
  - `crates/core-ffi/tests/starter.rs`.
- Modify:
  - `crates/core-ffi/src/session/co_producer_creative.rs` (`co_producer_starter`);
  - `crates/core-ffi/src/lib.rs`;
  - `crates/core-ffi/src/control.rs` (`ControlResult::CoProducerStarter` and its JSON);
  - `app/Tests/VizDAWAppModelTests/FakeSession.swift` (the stub).

**Interfaces:**
- Consumes: Task 1's `core_control::starter::{parse, StarterSpec}`; 8d's `resolve_plugin`, `mixer_chores::commands_for`, `track.new_plugin_instrument`'s path in `tracks.rs`, and `bundled::{arm, multiq}`; `mixer.new_bus`'s `new_bus_from`; `TempoMap::constant`; `SequenceCmd::{SetTempo, SetMeter, SetMarkers}`; 8b's marks.
- Produces:
  ```rust
  // starter.rs
  /// Each spec plug-in resolved, or its fallback, with Ruling 5's lines.
  pub struct Resolved { pub instruments: Vec<PluginChoice>, pub effects: Vec<Vec<PluginRef>>, pub lines: Vec<String> }
  pub enum PluginChoice { Plugin(PluginRef), BuiltinSynth, Audio }
  pub fn resolve(spec: &StarterSpec, host: &dyn PluginLookup) -> Resolved;   // never fails
  #[derive(uniffi::Record)] pub struct StarterStripRow { pub name: String, pub plays: String, pub effects: Vec<String> }
  #[derive(uniffi::Record)] pub struct CoProducerStarterRow { pub title: String, pub undo_label: String,
      pub tracks: Vec<StarterStripRow>, pub buses: Vec<StarterStripRow>, pub lines: Vec<String> }
  // session/co_producer_creative.rs
  impl Session {
      /// Ruling 5's build, as one acting step in this (new, untitled) document. `spec` is the JSON
      /// text, parsed again by core_control::starter::parse. Refuses unless the document has no
      /// events, tracks or buses of its own beyond File › New's.
      pub fn co_producer_starter(&self, spec: String) -> Result<CoProducerStarterRow, VizError>;
  }
  // control.rs
  // ControlResult::CoProducerStarter { doc, window, row } → {"doc", "window", "title", "undo_label",
  //     "tracks": [{"name", "plays"}], "buses": [{"name", "effects"}], "lines"}
  ```

- [ ] **Step 1: Write the failing tests** in `tests/starter.rs`, through a `serial` helper, each on a fresh session as File › New makes it, inside a request "start synthwave". The catalog holds the test bundle, and Multi-Q is set by `debug_set_multiq`.
  - `a_starter_sets_up_sounds_and_routing`, with Task 1's synthwave example, its instruments `clap:vizdaw.test.synth4`:
    - the sequence's tempo is 118 bpm and its meter 4/4;
    - its markers are Intro at bar 1 (tick 0), Verse at bar 9 (tick 8 × 61 440), and so on;
    - buses "Reverb (Co-Producer)" and "Delay (Co-Producer)" exist, "Delay" goes to "Reverb", and "Reverb" has the test gain insert;
    - five instrument tracks "Drums (Co-Producer)" … "Arp (Co-Producer)" each play a test synth destination;
    - "Pad"'s strip has a send to "Reverb" at −12 dB, post-fader;
    - everything is marked as Co-Producer's;
    - one `undo` returns the document to File › New's.
  - `starters_write_no_notes`: after the build, every track of every sequence has no events of any kind, and no automation track exists.
  - `starters_fall_back_to_built_ins`:
    - with "Bass"'s plugin `clap:vizdaw.test.none`, "Bass" plays the Built-in Synth, and `lines` holds "Bass: No installed plug-in is clap:vizdaw.test.none.; it plays the Built-in Synth instead." and "Tracks on the Built-in Synth share its one sound for now.";
    - with the test gain turned off in the Plugin Manager, "Reverb"'s effect becomes Multi-Q, with "Reverb: … is turned off in the Plugin Manager.; Multi-Q is in its place.";
    - two unusable effects on one strip give one Multi-Q and one "left out." line;
    - with `debug_set_multiq(None)`, the unusable effect is "left out.";
    - an effect given as an instrument (`clap:vizdaw.test.gain` on "Lead") falls back to the Built-in Synth.
  - `starters_never_fail_over_a_plugin`: a spec whose every reference is `clap:vizdaw.test.none` still builds, with one line per reference.
  - `a_starter_answers_what_it_made`: `tracks[0]` is `{name: "Drums (Co-Producer)", plays: <the synth's catalog name>}`; an audio track's `plays` is "audio"; `undo_label` is "Co-Producer: Start Synthwave".
  - `a_starter_needs_a_new_song`: on a session with a track of the composer's, `co_producer_starter` is `Err` "A starter project needs a new song." and changes nothing.
  - `starter_refuses_hostile_input`: a spec text that is not JSON, or fails `parse`, is `Err` with `parse`'s message, and changes nothing.
  - `starter_result_json`: the result serialises to the keys in Interfaces.
- [ ] **Step 2: Run** `cargo test -p core-ffi --test starter`. Expected: FAIL.
- [ ] **Step 3: Implement** Ruling 5's `resolve` and build in `starter.rs`, and `co_producer_starter`. The build arms the document first (`bundled::arm`), runs one transaction in Ruling 5's order, and aborts on any error, so a failed build leaves File › New's document. Inserts, outputs and sends go through 8d's `mixer_chores::commands_for` with the new strips' IDs. **Swift:** `FakeSession` gains the stub.
- [ ] **Step 4: Run** `cargo test -p core-ffi`. Expected: PASS, including 6g's `default_eq` and 8d's `mixer_chores`.
- [ ] **Step 5: End-of-task checks**, including the Swift build and test. **Commit** `feat(ffi): Co-Producer starter projects from installed plug-ins, with built-in fallbacks (plan 8e Task 4)`.

### Task 5: Creative commands and starters in the app (Swift)

**Files:**
- Modify:
  - `app/Sources/VizDAWAppModel/CreativeCommands.swift` (the three handlers through `runCreative`, replacing Task 1's arms);
  - `app/Sources/VizDAWAppModel/CoProducerCommands.swift` (`co_producer.starter`, `co_producer.midi_notes`, `co_producer.automation`);
  - `app/Tests/VizDAWAppModelTests/CreativeCommandsTests.swift` (new), `CoProducerCommandsTests.swift`, `FakeSession.swift`.

**Interfaces:**
- Consumes: Task 3's `runCreative(id:args:)`, `coProducerMidiNotes(…)` and `coProducerAutomation(…)`; Task 4's `coProducerStarter(spec:)`; `DocumentStore.newProject(_:)` and `store.defaultFormat`, as `file.new`'s handler uses them; 8a's request begin and end and `coProducerEnter(stamp:)` / `coProducerLeave()`.
- Produces: the handlers. Each creative handler answers `.ran(command:doc:window:undoLabel:status:)` with `status` the title, and `midi.new_part` with the new track's name. `co_producer.starter` answers its result with the new document's `window`.

- [ ] **Step 1: Write the failing Swift tests** with `FakeSession`.
  - `CreativeCommandsTests`:
    - `creativeEditsRunThroughOneCall`: each of the three ids calls `runCreative` with its id and args unchanged, then applies the snapshot;
    - a throwing `runCreative` is `.failed` with the session's message, and its `Busy` is `busy`;
    - with an editor gesture open, each is `unavailable` with `EditorModel.gestureOpenMessage`, and `runCreative` is not called.
  - `CoProducerCommandsTests`:
    - `starterOpensOneNewSong`: `co_producer.starter {spec}` makes exactly one new document with the default format. In it, the request is begun as "start <title>", then entered, then `coProducerStarter` is called with the spec's JSON, then left, then ended, in that order. The answer carries that document's `window`. The composer's own window and its request are untouched.
    - `badStarterOpensNoWindow`: when `coProducerStarter` throws, the new document is closed without a sheet or a save question, no document is left, and the answer is `.failed` with the message.
    - `oneStarterAtATime`: a second `co_producer.starter` while the first is building is `busy` "A starter project is being made.".
    - `readsAnswerForTheDocument`: `co_producer.midi_notes` and `co_producer.automation` call the resolved document's methods with their args, a throw is `.failed` with its message, and neither calls `coProducerEnter`.
    - `noSheetsForCreativeEdits`: after a failing `runCreative` reached through `co_producer.apply`, the document has no alert (8a Ruling 13).
- [ ] **Step 2: Run** `scripts/build-ffi.sh && scripts/swift.sh test --filter 'CreativeCommands|CoProducerCommands'`. Expected: FAIL.
- [ ] **Step 3: Implement.**
  - The creative handlers keep 8d's guard order: the gesture first, then the session call.
  - `co_producer.starter` makes the document as `file.new`'s handler does, and runs Ruling 5's steps 2–3. Nothing else opens: no panel, no sheet, no plug-in window.
- [ ] **Step 4: Run** the Step 2 command, then `scripts/swift.sh test`. Expected: PASS, including `NameScanTests`, `noLaneInWhatTheAppShows` and `everyRegisteredMenuCommandHasAHandler`.
- [ ] **Step 5: End-of-task checks**, including `scripts/make-app.sh`. **Commit** `feat(app): Co-Producer creative edits and starter projects in the app (plan 8e Task 5)`.

### Task 6: `vizdaw-mcp`: creative tools, recipes, prompts and the automation view

**Files:**
- Create:
  - `crates/vizdaw-mcp/src/recipes.rs` (`RECIPES`, `recipes_answer`, the prompts);
  - `crates/vizdaw-mcp/views/automation.html`.
- Modify:
  - `crates/vizdaw-mcp/src/lib.rs` (`prompts/list`, `prompts/get`, the `prompts` capability, the four tools);
  - `crates/vizdaw-mcp/src/tools.rs` (the tools, their answers, `INSTRUCTIONS`, `change`'s description);
  - `crates/vizdaw-mcp/src/views.rs` (the fifth entry in `VIEWS`);
  - `crates/vizdaw-mcp/tests/server.rs`, `tests/support/fake_app.rs`.

**Interfaces:**
- Consumes:
  - Task 1's three protocol commands, `registry::point_list` and the three entries in `list_json()`;
  - the answers of Tasks 3 and 4;
  - 8c's `MAX_SHOWN`, `views::{VIEWS, wants_views, html}`, `bridge.js` and the `change` tool's states; 8a's `disguises` and `rpc::truncate`.
- Produces:
  ```rust
  // tools.rs
  pub const STARTER_PROJECT: &str = "starter_project";
  pub const READ_MIDI_NOTES: &str = "read_midi_notes";
  pub const PREVIEW_AUTOMATION: &str = "preview_automation";
  pub const RECIPES: &str = "recipes";
  /// Ruling 7's texts; `automation_answer` also gives the view's structuredContent.
  pub fn starter_answer(result: &Value) -> String;
  pub fn notes_answer(result: &Value) -> String;
  pub fn automation_answer(current: &Value, planned: &[(u64, f64)]) -> (String, Value);
  // recipes.rs
  pub struct Recipe { pub name: &'static str, pub title: &'static str, pub steps: &'static [&'static str] }
  pub const RECIPES: &[Recipe];   // Ruling 6's seven, in its order
  pub fn recipes_answer(name: Option<&str>) -> Result<String, String>;
  pub fn prompts_list() -> Value;
  pub fn prompt_get(params: &Map<String, Value>) -> Result<Value, RpcError>;
  // views.rs: VIEWS gains ("ui://vizdaw/automation", "preview_automation").
  ```

- [ ] **Step 1: Write the failing tests** in `tests/server.rs`. The fake app answers the three protocol commands from scripts.
  - `creative_tools_are_listed`:
    - `starter_project`, `read_midi_notes`, `preview_automation` and `recipes` have Ruling 6's and 7's descriptions;
    - the last three have `readOnlyHint: true`, and `starter_project` has `destructiveHint: false`;
    - `starter_project`'s schema has `additionalProperties: false` at every level, `tracks` with `minItems` 1 and `maxItems` 32, and `kind` an `enum` of `instrument audio`;
    - `midi_new_part` is listed, with `notes`' `maxLength` 32 768;
    - no tool is named `automation_write` or `midi_add_notes`;
    - `change`'s description holds Ruling 7's added sentence, and the instructions hold Ruling 6's.
  - `starters_reach_the_app`:
    - `starter_project {spec}` sends `co_producer.starter` with the spec unchanged;
    - its text starts "Made a new song, "Synthwave", in window <n>.", lists "Tracks:" and "Buses:", and ends with the fallback lines.
  - `notes_reach_the_app`: `read_midi_notes {seq: 1, lane: 2}` sends `co_producer.midi_notes`, and its text holds the header and "0:60:3840:100 …". 200 notes wrap onto three lines.
  - `automation_previews_both_curves`:
    - `preview_automation` sends `co_producer.automation` without `points`;
    - its text has "now 3 points, planned 2 points";
    - its `structuredContent` has `existing` and `planned`;
    - a bad `points` is `isError: true` with `point_list`'s message, before any socket call.
  - `recipes_are_a_tool_and_prompts`:
    - `recipes {}` lists the seven names in Ruling 6's order;
    - `recipes {name: "mix_drums"}` starts "Help me mix my drums:", its first step "- Start a request with begin_request." and its last "- End the request with end_request, …";
    - every mix recipe holds the "If list_plugins has no fitting effect" step;
    - `starter_project`'s steps hold "Never write notes or beats.";
    - `initialize`'s `capabilities` has `prompts: {listChanged: false}`;
    - `prompts/list` gives the seven, and only `starter_project` has the argument `style`;
    - `prompts/get {name: "starter_project", arguments: {style: "80s synthwave"}}` gives one `user` message ending "The style: 80s synthwave.".
  - `recipes_and_prompts_refuse_hostile_input`:
    - `recipes {name: <70 000 characters>}` is `isError: true` with the name truncated;
    - `recipes {name: 1}` and `recipes {extra: 1}` are `isError: true`;
    - `prompts/get` with `style` of 201 characters, `"a\u{0}b"` or `"a\u{202E}b"`, an unknown name, or `arguments: 3` is `-32602`;
    - no answer holds a disguise character.
  - `creative_tools_refuse_hostile_arguments`. Each is `isError: true` before any socket call:
    - `starter_project` with no `spec`, a spec that is not an object, or a spec over the schema's limits;
    - `read_midi_notes` with `lane: 0` or `start` alone;
    - `preview_automation` with `end` equal to `start`.
  - `long_curves_are_capped`: 4096 existing points with `truncated` and 1024 planned give 40 "Planned:" lines then "- and 984 more", and the text says "truncated".
  - `creative_names_are_shown_safely`: a target name `"a\u{202E}b"` and a track named `"x\u{2028}y"` show with U+FFFD in the text and in `structuredContent`.
  - `the_automation_view_only_for_hosts_that_ask`:
    - with the extension, `preview_automation` has `_meta.ui.resourceUri` "ui://vizdaw/automation", `resources/list` has it with `MIME`, and the result carries `structuredContent.view` `"automation"`;
    - without it, and at protocol 2025-03-26, none of these appear, and the text is byte for byte the same.
  - `automation_view_draws_names_as_text`:
    - `automation.html` holds none of 8c's forbidden strings (`innerHTML`, `outerHTML`, `insertAdjacentHTML`, `document.write`, `eval(`, `new Function`, `http:`, `https:`, `src=`, `<link`);
    - it holds `textContent`, `createElementNS`, `Number.isFinite` and `<!--BRIDGE-->`;
    - `resources/read ui://vizdaw/automation` begins `<!DOCTYPE html>` with no `<!--` marker left.
- [ ] **Step 2: Run** `cargo test -p vizdaw-mcp`. Expected: FAIL.
- [ ] **Step 3: Implement** Rulings 6 and 7. The view draws from `structuredContent`, and with no result or an `isError` result shows the text content as it is (8c Ruling 7).
- [ ] **Step 4: Run** `cargo test -p vizdaw-mcp -p core-control`. Expected: PASS, including `names.rs`'s scan of `vizdaw-mcp/src` and `vizdaw-mcp/views`.
- [ ] **Step 5: End-of-task checks. Commit** `feat(mcp): starter_project, read_midi_notes, preview_automation, recipes and prompts, and the automation view (plan 8e Task 6)`.

### Task 7: README checks, the e2e, the backlog and the gates

**Files:**
- Modify:
  - `app/README.md` (new steps after the last one);
  - `crates/vizctl/tests/e2e.rs`;
  - `docs/feature-backlog.md`.

**Interfaces:**
- Consumes: Tasks 1–6. No production code, unless a step finds a bug. Such a bug is fixed in its owner's module, with a regression test.

- [ ] **Step 1: Add the e2e** `co_producer_creative_over_mcp`, `#[ignore]`, in the style of 8d's `co_producer_mixer_over_mcp` and with 8a's MCP helper. Its client declares `elicitation: {}` and the MCP Apps extension.
  - **Start.** Launch the app and switch Co-Producer on over the developer channel. Run `file.create_demo`, then `file.save {path: <home>/Song.vizdaw}`. Take an instrument track P of the demo's with notes, and its strip.
  - **Recipes.** `recipes {}` lists seven, and `prompts/list` has `starter_project` with `style`.
  - **A starter project.**
    1. `starter_project` with Task 1's synthwave example, where "Bass" asks for `clap:vizdaw.e2e.none` and "Reverb" asks for `clap:vizdaw.e2e.none` too;
    2. the text names a new window W, and holds the Built-in Synth line for "Bass" and the Multi-Q line for "Reverb";
    3. in W, `state.get document` shows tempo 118, meter 4/4, the five markers, five tracks named "… (Co-Producer)", and every track with `"events": 0`;
    4. `state.get mixer` in W shows "Delay (Co-Producer)" going to "Reverb (Co-Producer)", and "Reverb"'s first insert `"plugin": "multi-q"`;
    5. the undo label in W is "Co-Producer: Start Synthwave", and the demo window's undo history is unchanged.
  - **Automation on Co-Producer's bus, in W.** In a request, `preview_automation {target: "fader:<Reverb>", points: "0:-144 61440:0"}` gives "planned 2 points", with `structuredContent.view` "automation". Then `change [automation.write …]` with the same args applies with no elicitation, and its state is `"applied_without_asking"`.
  - **Automation on the composer's strip, in the demo window.** `change [automation.write {target: "fader:<P's strip>", …}]` emits `elicitation/create` with "fader automation 0 points → 2 points". Answer `decline`: nothing changed. The same `change`, answered `accept` with `allow: true`, applies.
  - **A new part.**
    1. `read_midi_notes` on P gives its notes;
    2. `midi_new_part {lane: P, name: "Arp", notes: <16 notes>}` adds "Arp (Co-Producer)" below P, with 16 events, and P's event count is unchanged;
    3. `change [midi.add_notes {lane: <Arp>, …}]` applies without asking;
    4. `change [midi.add_notes {lane: P, …}]` asks with "notes added"; `decline` leaves P unchanged.
  - **The gate.** A raw client on the Co-Producer socket gets:
    - `not_allowed` for `command.run automation.write` and `command.run midi.add_notes`;
    - `invalid_params` for `co_producer.starter` with a bus cycle, and `state.get windows` shows no new window.
  - **Undo.** One `edit.undo` in the demo window takes back the request that held the new part.
- [ ] **Step 2: Write the README steps** in `app/README.md`, after the last step, under a heading "Plan 8e, Co-Producer creative help and starter projects:". Each step states what to do and what is seen, and says "track":
  1. Ask Claude to "set up a new template project in a sort of 80s synthwave style". A new untitled song opens with a tempo, meter, section markers, instrument tracks for drums, bass, pad, lead and arpeggio, and reverb and delay buses, all with the Co-Producer badge. No track holds any notes.
  2. Look at which instruments and effects it chose: they are plug-ins you have installed. Claude tells you which roles fell back to the Built-in Synth or Multi-Q, and that tracks on the Built-in Synth share one sound for now.
  3. Turn off every instrument in the Plugin Manager and ask for another starter project: it still opens, on the Built-in Synth, and says so.
  4. Undo once in the new song: it goes back to an empty new song.
  5. Ask Claude to "help me figure out how to mix my drums". It makes a drum bus, shows exactly which of your drum tracks' strips will go to it, and waits for your yes. It talks about levels from the faders, and asks you to play and read the meters.
  6. Ask for a shared reverb, then for parallel compression on the drums, then for a delay into the reverb. Each new bus appears without a question. Sends from your tracks are listed first and wait for your yes.
  7. Ask Claude to fade out the song's last four bars. In a host that shows views, a curve appears first. It asks before writing on your master or track strips, and the fade plays as real automation points you can edit.
  8. Ask Claude for a filter sweep on a filter plug-in Co-Producer added to one of your tracks. It asks first, because the track is yours.
  9. Ask Claude to turn the held chords on your piano track into an arpeggio. A new track "… (Co-Producer)" appears below the piano, playing the same instrument, with real notes. Your piano's notes are unchanged.
  10. Ask Claude to add those notes to your piano track itself: it asks first, and says how many notes and which bars.
  11. In a host that offers prompts, open the prompt list: the recipes are there, and the starter project prompt takes a style.
  12. In a host without MCP Apps views, ask for the automation curve: the same answer comes as text.
- [ ] **Step 3: Update the backlog** (`docs/feature-backlog.md`).
  - **The "Co-Producer over MCP" row.** Note 8e's delivery:
    - starter projects from installed plug-ins, with built-in fallbacks, and no notes;
    - the seven recipes, as a tool and as MCP prompts;
    - automation points and the automation preview view;
    - new parts and added notes, with the track-decides yes rule.
  - **Closes and notes.**
    - Close 8c's row "The automation preview view".
    - Note against "Previews for the remaining change and remove commands" that 8e added two, and that the release-fixes commands remain for the merge.
    - Note against "Feel: per-track timing lean, and a humanize that isn't random" that 8e leaves it, and that its recipe "give this a live feel" joins the recipes table when the row ships.
    - Move "Meter peaks in Co-Producer's mixer read, for gain staging from levels" to 27.1.0, with the reason: a second peak reader needs its own hold in `core-engine`.
    - Re-target 8b's "Marks on regions, notes and automation Co-Producer writes onto the composer's tracks": 8e decides the yes by the track and strip mark, so the row is now about tidying such writes without a question.
  - **New rows:**
    - "Render the input-FX arpeggiator offline into notes, for Co-Producer's arpeggio recipe";
    - "Replace the original's notes with a new part through Co-Producer (a remove on the composer's notes)";
    - "Write notes onto drum tracks through Co-Producer";
    - "Change the tempo or meter of an existing song through Co-Producer";
    - "Plug-in categories (reverb, delay, compressor) in list_plugins, from CLAP features and AU types";
    - "Starter projects give each fallback role its own Built-in Synth sound" — with 27.1.0's Built-in Synth per track;
    - any narrow edge cases the task reports listed, at the owner's review pace.
- [ ] **Step 4: Run the gates:**
  - `cargo test --workspace`;
  - both clippies;
  - `cargo clippy -p core-ffi --lib -- -D warnings`;
  - `scripts/build-ffi.sh`;
  - `scripts/swift.sh build`;
  - `scripts/swift.sh test`;
  - the no-brand build, `VIZDAW_NO_BRAND=1 scripts/swift.sh build --scratch-path <fresh temp dir>`;
  - `scripts/make-app.sh`;
  - the e2e, `VIZDAW_E2E_APP=dist/VizDAW.app cargo test -p vizctl --test e2e -- --ignored --nocapture --test-threads 1`: the baseline's ignored tests plus one run, and all pass, 6g's `multiq_install_and_switch_over_the_channel` and 8d's `co_producer_mixer_over_mcp` included.

  Then `pgrep -lf MacOS/VizDAW` and `pgrep -lf vizdaw-mcp` print nothing. Report against the baseline: registry +3, `COMMAND_NAMES` +3, `PREVIEWABLE` +2, error codes unchanged, README +12, e2e +1, `VERSION` unchanged, and no change under `crates/core-host`, `crates/core-engine` or `crates/platform-macos` (`git diff --stat <baseline HEAD> -- crates/core-host crates/core-engine crates/platform-macos` is empty).
- [ ] **Step 5: Commit** `docs(app): Co-Producer creative README checks, e2e and backlog (plan 8e Task 7)`.

## At the release-fixes merge

These steps are not a task of this branch. They run when `release-fixes` merges, by whoever merges it, as part of the merge's own review. They close 8d's row "After the release-fixes merge: make `lane.set_output` and `destination.set_sound` previewable, and class `automation.release_all` for Co-Producer", and the fade-shapes gap.

1. **Counts.** The registry count tests take this branch's count plus 4. `COMMAND_NAMES` does not change.
2. **Classes.** `automation.release_all` is refused on the Co-Producer socket, with the other app-wide commands of 8a Ruling 5's refused list.
3. **Previews.** `region.set_fade_shape`, `destination.set_sound` and `lane.set_output` join `PREVIEWABLE`. Each gets a `preview_items` arm that executes its own command on the clone.
   - **`region.set_fade_shape`.** The track decides, as `midi.add_notes` does (Ruling 4). The region's track item gains `fade in linear → s-curve`, with the shapes' menu titles.
   - **`destination.set_sound`.** It always asks: the sound is shared by every track that plays the Built-in Synth (release-fixes Scope 1). The item is "Built-in Synth" / "sound Piano → Pad".
   - **`lane.set_output`.** The track decides. The item is "output <old> → <new>", with the output names from `doc.outputs()`.
4. **Tests.** Each command gets 8a's preview-matches-apply test, a hostile-input test on the Co-Producer socket, and a direct `command.run` that is `not_allowed`.
5. **Recipes.** `automation_moves` gains: "For a region's own fade, set its shape with region.set_fade_shape through change.". `starter_project` gains: "To choose the Built-in Synth's one sound, use destination.set_sound through change.".
6. **Backlog.** Close 8d's row and the fade-shapes gap (spec §10 gap 11), and the backlog row "A control-channel path to set a fade shape" if release-fixes left it open.

## Self-Review

1. **Spec coverage (the §12 row 8e).**
   - **Automation points** (§10 gap 9; §8 "automation as real points"): Rulings 1 and 2, Tasks 2 and 3, with fades, rides and sweeps as Ruling 6's `automation_moves`.
   - **The automation preview view** (§6.3): Ruling 7 and Task 6, with its text fallback (§5).
   - **Fade shapes** (§10 gap 11): reused from release-fixes, not duplicated, in "At the release-fixes merge". Until then, fades are fader points.
   - **Writing notes** (§10 gap 8): `midi.new_part` and `midi.add_notes` (Rulings 1 and 3), with `read_midi_notes` so Claude can see the composer's material. "Regions" here are the notes of a track's active take, as the model holds MIDI.
   - **Arpeggio and variation edits on new tracks** (§8): the `new_part` recipe and `midi.new_part`. Humanising is the Feel row's, in 27.1.0, by the owner's backlog row.
   - **Mix recipes** (§8; the owner's examples): Ruling 6's `mix_drums`, `shared_reverb`, `parallel_compression` and `delay_into_reverb`, on 8d's commands, with the composer's routing and sends asked for.
   - **Starter templates** (§8; the owner's answers): Ruling 5 and Task 4. They cover sounds and routing only, write no notes or beats, use installed plug-ins, and fall back to the Built-in Synth and Multi-Q. They make an untitled project, as File › New does.
   - **Tempo and meter** (§10 gap 14): set by a starter. Changing them later is a backlog row.
   - **Web search by Claude** (§8): the `starter_project` recipe tells Claude to use its own tools. VizDAW goes online for nothing.
   - **The owner's rule on routing and sound**: Ruling 4, Task 3's `creative_writes_ask_for_the_composers_work`, and the e2e's decline then accept.
   - **8d's hand-offs**: meter peaks (moved to 27.1.0, with its reason); `region.set_fade_shape` and `destination.set_sound` (the merge section); the release-fixes rows (the merge section).
2. **Steps.** Every test step names its tests and assertions with the plan's values: ids, ranges, grammars, sentences, item texts and caps. Code steps give signatures and point to the ruling. The tool descriptions and the recipes' steps are written out, because the spec leaves them to the plan (§11.12).
3. **Type consistency.** These names are spelled the same in every task:
   - `registry::{CREATIVE_IDS, MAX_NOTES, MAX_POINTS, LIST_TEXT_MAX, NoteSpec, note_list, point_list}`, `PREVIEWABLE`;
   - `starter::{StarterSpec, StarterBus, StarterTrack, StarterSend, StarterKind, parse, MAX_TRACKS, MAX_BUSES, MAX_MARKERS}` in `core-control`;
   - `Command::{CoProducerStarter, CoProducerMidiNotes, CoProducerAutomation}`, the matching `ControlRequest` variants, and `ControlResult::{CoProducerStarter, CoProducerMidiNotes, CoProducerAutomation}`;
   - `core_model::Command::WriteAutomation`, `diff::bar_of`;
   - `creative::{commands_for, to_stored, MAX_READ}`, `NoteRow`, `CoProducerNotesRow`, `CurvePointRow`, `CoProducerCurveRow`;
   - `starter::{resolve, Resolved, PluginChoice}`, `StarterStripRow`, `CoProducerStarterRow` in `core-ffi`;
   - `Session::{run_creative, co_producer_midi_notes, co_producer_automation, co_producer_starter}` in `session/co_producer_creative.rs`;
   - `tools::{STARTER_PROJECT, READ_MIDI_NOTES, PREVIEW_AUTOMATION, RECIPES, starter_answer, notes_answer, automation_answer}`, `recipes::{Recipe, RECIPES, recipes_answer, prompts_list, prompt_get}`, and the URI `ui://vizdaw/automation`.
4. **Review Focus.** Each of the five lines names the tests that pin it, in Tasks 1, 3, 4, 5 and 6.
5. **Proportion.** Seven tasks, as the owner asked for 26.0.0 by 2026-11-20. Code appears only as signatures and the exact text the spec leaves to the plan. The plan reuses a lot:
   - the recording pass's `write_automation`, unchanged;
   - 8d's plug-in resolution, mixer commands, `PluginLookup`, `describe_with` and its strip rule;
   - 8a's preview and token, 8b's marks, 8c's markers and view machinery;
   - File › New for the starter's document.
6. **Owner decisions.** None are open. These calls are stated for the owner's review:
   - recipes are a read tool plus MCP prompts, not resources (Ruling 6);
   - Claude works out arpeggios itself; rendering the input-FX arpeggiator offline is a backlog row (Not in this plan);
   - gain staging reads fader levels; meter peaks move to 27.1.0 (Not in this plan);
   - a starter's fallback is the existing Built-in Synth, one shared sound, until release-fixes and 27.1.0 (Ruling 5);
   - an unusable effect becomes Multi-Q, once per strip, by the owner's fallback rule (Ruling 5);
   - Co-Producer adds notes but never removes the original's in 8e (Ruling 3).
