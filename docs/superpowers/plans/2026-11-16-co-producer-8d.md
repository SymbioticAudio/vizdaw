# Co-Producer Mixer and Plug-ins Implementation Plan (8d)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Co-Producer can build and edit a mix. It can make buses, route strips to them, add and set sends, load plug-ins into inserts, the EQ slot or a new instrument track, set fader, pan, mute and solo, and set a plug-in's parameters. It can list the installed plug-ins it may use and read a loaded plug-in's parameters. Anything that changes the routing or sound of the composer's own strips shows exactly what changes and waits for their yes. Co-Producer's own buses and tracks change freely. A mixer snapshot view shows the strips, their routing, inserts and sends, with a plain-text fallback. This is what 8e's mix recipes need.

**Architecture:**
- **`core-control`.**
  - Thirteen registry commands: twelve `mixer.*` edits (Ruling 1) and `track.new_plugin_instrument`.
  - `mixer.new_bus` with `strip` becomes previewable, with the twelve change and remove commands.
  - Two protocol commands on the Co-Producer socket: `co_producer.plugins` and `co_producer.params`.
- **`core-model`.** `diff` learns the mixer: fader, pan, mute, solo, output, sends, the EQ, inserts and plug-in parameters (Ruling 4).
- **`core-ffi`.**
  - A new `mixer_chores` module:
    - resolves plug-in references by the Plugin Manager's rules and 6g's Multi-Q rules (Ruling 3);
    - builds the core-model commands for each new registry command. The preview and the apply share them, so the two cannot differ.
  - The new `Session` methods live in a child module, `session/co_producer_mixer.rs`, so the release-fixes merge meets one new line in `session.rs`.
  - `state.get mixer` names each slot's plug-in.
- **Swift.**
  - `MixerCommands` runs the thirteen new commands through one Session call.
  - `CoProducerCommands` answers the two new protocol commands.
- **`vizdaw-mcp`.**
  - Three new tools: `list_plugins`, `plugin_params` and `show_mixer`.
  - A fourth MCP Apps view, `ui://vizdaw/mixer`, on 8c's view machinery.

**Tech Stack:**
- Rust 2024: `core-control`, `core-model`, `core-ffi` with UniFFI 0.32.2, `vizdaw-mcp`, `vizctl`.
- Swift 6.1 / SwiftPM with Swift Testing, macOS 14+.
- The view is one self-contained HTML file with plain inline JavaScript, built into `vizdaw-mcp` with `include_str!`, as 8c's are.
- **No new third-party dependency.**

**Spec:** `docs/superpowers/specs/2026-11-14-co-producer-design.md`, approved by the owner on 2026-11-14, is binding. This plan builds its §12 row **8d**:
- §10 gap 4 (mixer editing): buses, sends, inserts, plug-in loading, fader, pan, mute, solo, plug-in parameters;
- §10 gap 5 (the installed plug-in list) and §8's "any installed plug-in";
- §6.3's mixer snapshot view.

The owner's rule of 2026-11-14 (spec §6.5) is binding too. Anything that changes the routing or sound of the composer's tracks needs a yes: pointing them at a bus, or adding an insert, send or automation to them. Co-Producer's own new tracks and buses are free to change.

It builds on these plans, whose rulings stay in force:
- plan 8a, `docs/superpowers/plans/2026-11-14-co-producer-8a.md`: Rulings 1–13 and pre-flight rulings P1–P11 (`.superpowers/sdd/2026-11-14-co-producer-8a/preflight.md`);
- plan 8b, `docs/superpowers/plans/2026-11-16-co-producer-8b.md`: Rulings 1–10, including the marks, `mixer.set_colour` and `ChangeItem.co_producers`;
- plan 8c, `docs/superpowers/plans/2026-11-16-co-producer-8c.md`: Rulings 1–8, including the views and `MAX_SHOWN`.

Executors read all four.

**The release-fixes plan.** It is `docs/superpowers/plans/2026-11-16-release-fixes.md` on branch `release-fixes`, and it merges after this branch. It adds `automation.release_all`, `lane.set_output`, `region.set_fade_shape` and `destination.set_sound`, all class change. 8d adds none of them and reuses no name of theirs:
- **`lane.set_output`** sets an audio track's hardware output path, from `doc.outputs()`. 8d's `mixer.set_output` routes a mixer strip to a bus or master, which is a different thing, so it has a different name and shape. On the Co-Producer socket `lane.set_output` stays "Co-Producer cannot preview lane.set_output yet." (8a Ruling 6). A backlog row covers it.
- **`region.set_fade_shape`** is the fade-shapes gap, which is 8e's. 8e reuses that command's name and shape exactly, and makes it previewable.
- **`destination.set_sound`** picks the Built-in Synth's sound. 8d's `track.new_plugin_instrument {plugin: "built-in-synth"}` makes a track on that synth, as `track.new_instrument` does. Choosing its sound is `destination.set_sound` after the merge, which 8e makes previewable.
- **`automation.release_all`** is not for Co-Producer. It stays unpreviewable.
- **At the merge**, the registry count tests take 8d's count plus the fixes' 4. `session/co_producer_mixer.rs` and the fixes' `session/fixes.rs` are separate files.

**Not in this plan:**
- Automation points, the automation preview view, fade shapes, writing notes, mix recipes and starter templates: **8e**. 8d gives them the commands they use.
- Meter peaks in Co-Producer's reads, for gain staging from levels. 8d shows fader levels only. A backlog row covers it, for 8e.
- Moving an insert to another strip, sidechains, emptying the EQ slot, swapping the plug-in of an existing instrument, loading presets or plug-in state, and renaming strips through Co-Producer: backlog rows.
- Driving plug-in editors' own windows (spec §3).

## Plan Rulings

These are this plan's decisions where the spec, 8a, 8b or 8c leaves a choice. Each one names its cost if wrong.

1. **The mixer commands (spec §10 gap 4).**
   - **Common shape.** All thirteen are `key_only`, with `document: true`. Their menu is "Mixer", except `track.new_plugin_instrument`, whose menu is "Track". Every target is a required ID, so no preview depends on the selection (8a Ruling 6).
   - **Numbers that need decimals are Text** (`gain_db`, `level_db`, `value`), as `automation.gesture`'s `value` is, so no new `ParamKind` is added. `registry::decimal(text, min, max) -> Result<f64, String>` accepts only `-?[0-9]{1,6}(\.[0-9]{1,6})?`. So `"1e3"`, `"NaN"`, `"inf"`, `"+1"`, `"0x10"`, `"1,5"` and `""` are refused, and so is a value out of range. The message names the param and its range.
   - **Indexes are 1-based**, as 8c's markers and `automation.gesture`'s `send:<strip>:<n>` are.
   - **The commands.** The title is the undo label outside a request.

     | id | params | class | title | core-model |
     |---|---|---|---|---|
     | `mixer.set_strip` | `strip`; `gain_db?` (−144..12; −144 is −∞), `pan?` (Int −100..=100, percent, negative is left), `mute?`, `solo?` (Flag); at least one of the four | change | "Set Strip" | `SetGain`, `SetPan`, `SetMute`, `SetSolo` in that order, for those given |
     | `mixer.set_output` | `strip`; `bus` (IntOrNull, required; null is master) | change | "Set Strip Output" | `SetOutput` |
     | `mixer.add_send` | `strip`; `bus` (Int ≥ 1); `level_db?` (default `"0"`); `pre_fader?` (default false) | change | "Add Send" | `AddSend`, enabled |
     | `mixer.set_send` | `strip`; `index` (Int ≥ 1); `level_db?`, `pre_fader?`, `enabled?`; at least one of the three | change | "Set Send" | `EditSend`, with the rest kept |
     | `mixer.remove_send` | `strip`; `index` | remove | "Remove Send" | `RemoveSend` |
     | `mixer.add_insert` | `strip`; `plugin` (Text); `index?` (Int ≥ 1; absent is the end) | change | "Add Insert" | `AddInsert`, an effect |
     | `mixer.remove_insert` | `slot` | remove | "Remove Insert" | `RemoveInsert` |
     | `mixer.move_insert` | `slot`; `index` | change | "Move Insert" | `MoveInsert` within its own strip |
     | `mixer.set_bypass` | `slot`; `bypass` (Flag, required) | change | "Bypass Insert" | `SetBypass` |
     | `mixer.set_eq` | `strip`; `plugin` | change | "Set EQ" | `SetEq`, an effect |
     | `mixer.set_param` | `slot`; `param` (Int ≥ 0); `value` (Text, the plug-in's own range) | change | "Set Plug-in Parameter" | `SetPluginParams` with one pair |
     | `mixer.remove_bus` | `strip` | remove | "Remove Bus" | `RemoveBus` |
     | `track.new_plugin_instrument` | `seq?`, `after?` (as `track.new_instrument`); `plugin` | add | "New Plug-in Instrument Track" | `tracks.rs`'s instrument track, on a new plug-in instrument destination |

     `strip` and `slot` are `id_param`s (Int ≥ 1).
   - **Refusals** found from the document are `VizError::Model`, and a preview reports them as `failed` with nothing changed:
     - master as the `strip` of `set_output`, `add_send` or `remove_bus`: "Master has no output to set.", "Master has no sends." (the existing sentence) and "Master is not a bus.";
     - a `bus` that is not a bus strip: "Strip <n> is not a bus.";
     - a send `index` past the end: "Strip <name> has N sends.";
     - an insert `index` past the end plus one, or a `move_insert` index past the end: "Strip <name> has N inserts.";
     - a `slot` that is an EQ or an instrument's slot, for `remove_insert` and `move_insert`: "Slot <n> is not an insert.";
     - `remove_bus` of a strip that is not a bus: "Strip <n> is not a bus.";
     - a routing cycle: the message `MixerCmd::SetOutput` or `AddSend` gives today, through `mixer_rules::find_routing_cycle`.
   - **`mixer.set_param`** needs the plug-in running, the parameter listed and not hidden, and `value` finite and within the parameter's `min..=max`. A stepped parameter needs a whole value. The refusals:
     - "The plug-in in slot <n> is not running yet; read plugin_params again in a moment.";
     - "Slot <n> has no parameter <p>.";
     - "<param name> is <min> to <max>.";
     - "<param name> takes whole values.".

     It sends the value to the running plug-in, then executes `SetPluginParams`, as the typed-value path (`set_param`, then `commit_params`) does.
   - **`track.new_plugin_instrument`.**
     - It adds a plug-in instrument destination named after the plug-in (cut to 64 characters), then an instrument track playing it, as one step through `tracks.rs`.
     - `plugin: "built-in-synth"` makes the track on the Built-in Synth, exactly as `track.new_instrument` does.
     - Being an add, it runs directly as the `track_new_plugin_instrument` tool (8a Ruling 12). 8b's marks then name the track and strip "… (Co-Producer)".
   - **`mixer.new_bus`.** Its existing `strip` form (8a Ruling 5's `effective_class`: change) becomes previewable, with the preview built as `new_bus_from` builds the step.
   - **Adds first, then changes.** A plan cannot name a strip that does not exist yet. So Claude makes new buses with the `mixer_new_bus` tool (an add, without `strip`), reads their IDs with `show_mixer`, then plans the routing and inserts with `change`. `vizdaw-mcp` leaves `strip` out of `mixer_new_bus`'s `inputSchema`, so the tool can only make a free bus.
   - **Cost if wrong:**
     - a new bus stays when the composer says no to routing to it. It is an add, it shows in the reply, and Undo takes it back;
     - a request that sets up a bus takes three calls instead of one.
2. **The strip decides who must say yes (spec §6.5; the owner's rule of 2026-11-14).**
   - **The rule.** Every mixer item names one strip (Ruling 4). `preview_items` sets `composers_work` true when that strip was in `before` and not marked as Co-Producer's (8b Ruling 1). It is false when the strip was marked in `before`, or is new in `after`, since an acting step made it.
   - **What follows.** This holds even for a send or insert that Co-Producer added to a composer's strip, which 8b marked. Retuning, bypassing or removing it still changes the sound of the composer's track, so it needs a yes. That is stricter than the spec's controller's call on Co-Producer's own additions, because the owner's rule is about the composer's tracks' sound.
   - **Instruments.** A parameter of an instrument's own slot names the instrument's main strip, the `Instrument` strip with output 0 of that destination, so the same rule applies.
   - **Removing a Co-Producer bus.** `RemoveBus` sends the outputs that fed the bus to master and drops their sends. So a composer's strip that fed it gets its own item ("output Drums (Co-Producer) → Master, send to Drums (Co-Producer) deleted"), and the plan needs a yes.
   - **Asking.** `needs_yes` is true when any item is the composer's work, as 8a and 8b compute it. A plan whose items are all on Co-Producer's own strips applies at once, and `change` answers `applied_without_asking` (8c Ruling 8).
   - **Cost if wrong:** one extra question when Co-Producer retunes its own insert on a composer's track.
3. **Plug-in references and the list (spec §8; plan 6g's Multi-Q rules).**
   - **The form** is `plugins.default_eq`'s: `multi-q`, `clap:<id>` or `au:<type/subtype/manufacturer>`, plus `built-in-synth` for `track.new_plugin_instrument`.
     - `registry` checks the shape: one of those prefixes with a non-empty rest, at most 512 characters, with no control character.
     - `core-ffi` resolves it.
   - **Resolution.** `mixer_chores::resolve_plugin(text, want: PluginKind)`:
     - **Multi-Q.** `multi-q`, `clap:<MULTIQ_CLAP_ID>` and `au:<MULTIQ_AU_ID>` all resolve to `bundled::multiq()`, the bundled CLAP that the catalog's `preferred` entry names. CLAP is the built-in default (6g): the AU copy exists only for other apps, and Co-Producer never loads it. When `multiq()` is `None`, the answer is "Multi-Q is not available in this build.". An instrument `want` gets "Multi-Q is an effect, not an instrument.".
     - **Anything else.** `catalog::with(|c| c.preferred(format, id))`, accepted only with status `Ok` or `NotValidated` (never `Disabled`, `Blocklisted` or `Missing`), enabled, and of the wanted kind. The refusals:
       - "No installed plug-in is <text>.";
       - "<name> is turned off in the Plugin Manager.";
       - "<name> cannot be used: it is <missing|blocklisted (<reason>)>.";
       - "<name> is an instrument, not an effect." or "<name> is an effect, not an instrument.".
     - **Names in refusals** pass `diff::shown_name`.
   - **The restart refresh stays the bundled Multi-Q's.** `core-host` and `platform-macos` do not change in this plan. A Co-Producer insert is loaded by the same `AddInsert` the mixer's own menu uses, so 6g's refresh rule (`values_to_refresh`: the bundled Multi-Q's continuous, writable parameters, and none for any other plug-in) applies unchanged. 6g's tests pin it: the CLAP refresh test and `other_aus_name_no_values_to_refresh`.
   - **New strips' EQ.** A bus Co-Producer makes gets the default EQ, as any new bus does (`bundled::arm`). The preview's clone is armed first, so its items name the EQ the apply makes.
   - **The list.** `co_producer_plugins(kind: Option<PluginKindTag>) -> CoProducerPluginsRow` gives the rows Co-Producer may use. Each is `{plugin, name, maker, format, kind, version}`, where `plugin` is the reference text to pass back and `format` is `"au"`, `"clap"` or `"built_in"`.
     - **Which rows:** the usable rows only, by the resolution's rules, with a known kind.
     - **Built-ins first:**
       - `multi-q` ("Multi-Q", effect), when `multiq()` is `Some`;
       - `built-in-synth` ("Built-in Synth", instrument).
     - **Left out:** the catalog's own entries for `MULTIQ_CLAP_ID` and `MULTIQ_AU_ID`, so Multi-Q is listed once.
     - **Order:** after the built-ins, by kind (instruments first), then name ignoring case, then reference.
     - **Cap:** at most 5000 rows, with `truncated`.
   - **Parameters.** `Session::co_producer_params(slot) -> CoProducerParamsRow` answers `{slot, plugin, running, params}` for a slot of this document.
     - `params` holds `[{id, name, module, min, max, default, value, stepped, automatable}]`, without hidden parameters, at most 1000.
     - A slot the document lacks is `Err` "This song has no slot <n>.".
     - A slot not yet running answers `running: false` and no params.
   - **Cost if wrong:** a composer who wants the AU Multi-Q inside VizDAW gets the CLAP, which sounds the same.
4. **What the preview says about the mixer (spec §6.6).** `diff::describe` gains mixer items, after 8b's strip items.
   - **One item per strip** present before and after that changed. Its thing is 8b's "Strip <name>", and its change joins these parts with ", ", in this order:
     1. `volume −2.0 dB → −6.0 dB`: one decimal, U+2212 for minus, and −144 shown as "−∞ dB";
     2. `pan centre → 30% left`: pan × 100 rounded, with 0 "centre", then "30% left" or "30% right";
     3. `muted` or `unmuted`, then `soloed` or `unsoloed`;
     4. `output Master → Drums (Co-Producer)`;
     5. sends, matched by target and then by order among sends to that target:
        - `send to <bus> added (0.0 dB, post-fader)`;
        - `send to <bus> deleted`;
        - `send to <bus> −6.0 dB → −3.0 dB`;
        - `send to <bus> now pre-fader` or `now post-fader`;
        - `send to <bus> off` or `on`;
     6. the EQ: `EQ <old> → <new>`, `EQ <name> added` or `EQ <name> deleted`;
     7. inserts, matched by slot ID:
        - `insert <name> added at 2`;
        - `insert <name> deleted`;
        - `insert <name> moved 3 → 1`, for a move among the slots present both before and after, so a removal shifts nothing;
        - `insert <name> bypassed` or `turned back on`;
        - `insert <name> sidechain <strip> → none`;
     8. parameters, per changed overlay value: `<slot name> <param name> 0.5 → 0.75`. Values show at most 3 decimals, with trailing zeros dropped.
        - The old value is the `before` overlay's, else the lookup's current value.
        - With neither, the part reads `<param name> set to 0.75`.
        - With no name from the lookup, it reads `parameter <id>`.
   - **Caps.** An item holds at most 12 parts, then "and N more changes".
   - **Instruments.** An instrument slot's parameter changes join its main strip's item (Ruling 2).
   - **Parameter names.** `diff::describe_with(before, after, params: &dyn Fn(PluginInstanceId, ParamId) -> Option<(String, f64)>)` takes the name and current value from the caller. `describe` is `describe_with` with `|_, _| None`. `core-ffi` passes the running plug-ins' names and values, read without changing anything.
   - **Safe text.** Every name passes `diff::shown_name`.
   - **Cost if wrong:** a parameter shown as "set to 0.75" does not say what it was. Undo takes it back.
5. **The protocol and the tools (spec §6.1, §6.3, §11.12).**
   - **Two protocol commands**, admitted only on the Co-Producer socket ("only on the Co-Producer socket" elsewhere). Both are read and need no request:
     - `co_producer.plugins {kind?}`, with `kind` one of `instrument effect`;
     - `co_producer.params {slot, window?}`, with `slot` Int ≥ 1.

     The answers are `{plugins, truncated}` and `{doc, slot, plugin, running, params}`.
   - **`list_plugins {kind?}`**, read only.
     - Its text: "Plug-ins you may use (pass the first part as plugin):", then one line per row:
       - "- <plugin>: <name> by <maker>, <kind>"; the "by <maker>" part is left out when the maker is empty;
       - a built-in reads "- <plugin>: <name>, built in, <kind>".
     - The list stops at `MAX_SHOWN` (200) lines, then "- and N more; pass kind to narrow the list".
     - Its description, exactly: "List the installed plug-ins Co-Producer may use: instruments and effects the composer has not turned off in the Plugin Manager, and VizDAW's built-ins (multi-q, the EQ; built-in-synth). Pass a line's first part as plugin to mixer.add_insert, mixer.set_eq or track_new_plugin_instrument. Changes nothing."
   - **`plugin_params {slot}`**, read only.
     - Its text: "Slot <n>, <plugin>:", then one "- <id>: <name> = <value> (<min> to <max>)" line per parameter, with ", whole values" added for a stepped one. It stops at 200 lines, then "- and N more".
     - When the plug-in is not running, the text is "The plug-in in slot <n> is not running yet, so its parameters are not known. Try again in a moment.".
     - Its description, exactly: "List a loaded plug-in's parameters with their current values and ranges, for mixer.set_param. Slots are in show_mixer. Changes nothing."
   - **`show_mixer {}`**, read only. It sends `state.get mixer`.
     - Its text: "Mixer:", then one line per strip in display order: "- <name> (<kind_label>): volume <v>, pan <p>[, muted][, soloed], to <output name>; EQ <name or none>; inserts <slot n name, …, or none>; sends <bus level[ pre][ off], …, or none>". A strip Co-Producer added ends " — added by Co-Producer". Each strip's line starts with its ID in brackets, "[<id>]", so Claude can name it.
     - Its `structuredContent` is `{view: "mixer", strips: [{id, name, kind, colour, gain_db, pan, mute, solo, output, output_name, eq, inserts: [{slot, name, bypass}], sends: [{target, target_name, level_db, pre_fader, enabled}], co_producer}]}`.
     - Both forms stop at 200 strips, then "- and N more".
     - Names have 8a's `disguises` characters replaced by U+FFFD, as 8c's `show_tracks` does.
     - Its description, exactly: "Show the mixer: every strip with its ID, volume, pan, mute and solo, where it goes, its EQ, inserts (with slot IDs) and sends, and which strips Co-Producer added. Changes nothing."
   - **The mixer view** is `ui://vizdaw/mixer`, added to 8c's `VIEWS` for `show_mixer`.
     - It draws one column per strip: name, kind, volume, pan, mute and solo, output, the EQ, inserts and sends, with a "Co-Producer" tag on added strips.
     - It follows 8c Ruling 7: no data of its own, `textContent` only, nothing from the network, and the host's light or dark palette.
     - It has no controls: it is a picture.
   - **`change`'s description** adds, after 8c's examples: "mixer changes: routing a strip to a bus, adding or changing a send or insert, setting an EQ, the volume, pan, mute, solo or a plug-in parameter, and removing a send, insert or bus. Make new buses first with mixer_new_bus and read their IDs with show_mixer; mixer changes on Co-Producer's own buses apply without asking."
   - **The registry tools.** `track_new_plugin_instrument` appears through 8a Ruling 12, being an add. The twelve change and remove commands are run only through `change`.
   - **`state.get mixer`.** Each slot gains `"plugin"`, its reference text: `multi-q` for the bundled Multi-Q, else `clap:<id>` or `au:<id>`. Each strip gains `"output_name"`. 8b's `co_producer` fields stay.
   - **Cost if wrong:** a mixer of more than 200 strips shows the first 200, and says so.

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
  - Swift tests that make a real `Session` set `TestSupportDir.shared`.
  - Tests that load real plug-ins run in a child process whose `HOME` and `CFFIXED_USER_HOME` are a temp folder, through `crates/core-host/tests/support/temp_home.rs`, as `crates/core-ffi/tests/params.rs` does.
  - Linux CI has no AU or CoreFoundation, so any test that needs either is `#[cfg(target_os = "macos")]`. `vizdaw-mcp`, the `core-control` tests and the `core-model` tests are plain Unix and run on Linux.
  - The e2e uses vizctl's throwaway home.
- **Tools.**
  - Never run `auval`.
  - No GUI, computer-use or accessibility-automation tools. Launch VizDAW only through the e2e with `VIZDAW_AUDIO=offline`. Afterwards `pgrep -lf MacOS/VizDAW` prints nothing.
  - Never use `dangerouslyDisableSandbox`.
  - Never read the owner's off-limits research folder outside this repo, which 8b's Global Constraints name. Its path holds a banned name, so this plan does not spell it.
- **Programmatic calls open nothing.** Control-channel and Co-Producer calls, and programmatic calls, never open dialogs, sheets, alerts, panels, menus or windows. Loading a plug-in through Co-Producer never opens its editor.
- **Multi-Q (plan 6g).**
  - CLAP is the built-in default: Co-Producer's `multi-q`, and either Multi-Q ID, load the bundled CLAP (Ruling 3).
  - The restart refresh applies only to the bundled Multi-Q: `crates/core-host` and `crates/platform-macos` do not change in this plan, and 6g's refresh tests stay green.
- **Hostile input.**
  - Every MCP input is hostile: every MCP message, every tool argument, every Co-Producer call, every plug-in name and maker from the catalog, every parameter name from a plug-in, and every name a view draws.
  - Every new FFI export runs inside `ffi_guard` (and so `catch_unwind`). It returns `Result<_, VizError>`, or a safe default for the never-failing ones.
  - `vizdaw-mcp` handles each message inside `std::panic::catch_unwind`. A panic answers `-32603` and the server goes on.
  - Each new command and call gets hostile-input tests: wrong types, unknown params, missing required params, a 70 000-character string, a NUL, IDs out of range or unknown, and IDs of the wrong kind (a slot as a strip, an EQ slot as an insert).
  - Every planned command that does not run gets its line in the reply (8a Ruling 13).
- **The composer's yes.** Every new change or remove command is previewable and runs only through `co_producer.apply` (8a Rulings 6–8; Ruling 2). A direct `command.run` of one on the Co-Producer socket stays `not_allowed`.
- **Counts.** Check every count at the start of Task 1, and report each task's change against it.
  - The registry grows by **13**, from the count measured at the start. It is expected to be 196, after 8c's +6.
  - `COMMAND_NAMES` grows by **2**, expected 22 → 24. `ErrorCode::ALL` stays as measured, expected 16.
- **Names.**
  - The repo and UI never name the vendors and DAWs on the project's banned list. The list lives only in the name scans (`app/Tests/VizDAWAppModelTests/NameScanTests.swift`, `crates/core-control/tests/names.rs`); read them, keep all new text clear of them, and never copy the list.
  - The list includes the feature's old working name, which plan 8a added to the scans (spec, "Name"). Like every banned name, it is never written in the repo, this plan included.
  - Undo names, tool descriptions, refusals and the view's text are covered by the same rule. Multi-Q's maker is named only through `MULTIQ_CLAP_ID` and `MULTIQ_AU_ID`, never in new text.
  - The feature is "Co-Producer" everywhere.
  - Text the app or a view shows says "track", never "lane". Identifiers and JSON keys keep `lane`.
- **Merging with release-fixes.** `session.rs` gains only `mod co_producer_mixer;` and the one-line hooks Tasks 3 and 4 name. The new Session methods live in `crates/core-ffi/src/session/co_producer_mixer.rs`.
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
- **Format.** This plan changes no project format: 8b's marks already cover strips, sends and slots. `core_model::file::VERSION` stays as measured at the start, expected 10.
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
    - HEAD, which is 8c's last commit on `takes-and-co-producer`, or this plan's pre-flight commit if there is one;
    - the registry count (expected 196);
    - `COMMAND_NAMES` (expected 22) and `ErrorCode::ALL` (expected 16);
    - `PREVIEWABLE`'s length (expected 11, after 8c);
    - the last README step (expected 366, after 8c Task 7);
    - the e2e's ignored tests (expected 35);
    - `VERSION` (expected 10).

    If 8a, 8b and 8c are not all fully committed, stop and say so. If a measured value differs from the expected one, use the measured value and report the difference.

## Review Focus

These are the five failure modes a composer is most likely to hit, most likely first. Each is pinned by named tests in its task.

1. **A mix change reaches the composer's strip without a yes.**
   - Examples:
     - a plan holding a free insert on a Co-Producer bus and the composer's Kick routed to that bus;
     - retuning or bypassing an insert Co-Producer itself put on the composer's Bass;
     - removing a Co-Producer bus that the composer's strips feed;
     - `mixer.new_bus` with the composer's strip.
   - Expected: each asks, and the list shows exactly the composer's strips' changes. Only a plan whose every item is on Co-Producer's own strips applies without asking.
   - Pinned by Task 4's `the_strip_decides_who_says_yes` and `removing_a_bus_shows_what_it_fed`, and Task 1's `mixer_changes_only_through_a_preview`.
2. **A plug-in that should not be used.**
   - Examples:
     - one turned off in the Plugin Manager, blocklisted or missing;
     - an instrument given as an insert;
     - `au:` with Multi-Q's AU ID;
     - a user's own copy of Multi-Q elsewhere;
     - a build without Multi-Q.
   - Expected:
     - a clear refusal at preview, with nothing changed;
     - every Multi-Q form loads the bundled CLAP;
     - none of the refused ones is in `list_plugins`.
   - Pinned by Task 3's `plugins_resolve_by_the_plugin_managers_rules`, `multiq_is_the_bundled_clap` and `the_list_offers_only_usable_plugins`.
3. **Odd mixer targets.**
   - Examples:
     - routing bus A to bus B when B feeds A;
     - routing master, or a send to a strip that is not a bus;
     - send index 9 of 2, insert index 99;
     - an EQ slot given to `remove_insert`;
     - a slot of another song.
   - Expected: the refusal of Ruling 1, at preview, with nothing in the plan run and the document unchanged.
   - Pinned by Task 4's `odd_mixer_targets_are_refused_at_preview`.
4. **A plug-in parameter that cannot be set as asked.**
   - Examples:
     - a plug-in still loading;
     - a stepped parameter given 0.5;
     - a value past `max`;
     - a hidden parameter;
     - `value: "NaN"` or `"1e3"`.
   - Expected: Ruling 1's refusals, with no value sent to the plug-in.
   - Pinned by Task 1's `mixer_commands_refuse_hostile_input` and Task 4's `parameters_need_a_running_plugin`.
5. **The mixer view meets hostile or unusual data.**
   - Examples:
     - a strip named `<img src=x onerror=alert(1)>`;
     - a plug-in named with a bidi mark;
     - 500 strips;
     - a host without MCP Apps.
   - Expected:
     - names draw as text, with disguises replaced;
     - lists stop at 200 with "and N more";
     - with no extension, no `_meta.ui` and no `structuredContent`, and the same text.
   - Pinned by Task 6's `mixer_view_draws_names_as_text`, `long_mixers_are_capped` and `the_mixer_view_only_for_hosts_that_ask`.

---

### Task 1: The registry and protocol additions (`core-control`)

**Files:**
- Modify:
  - `crates/core-control/src/registry.rs` (the thirteen entries and their params; `decimal`; the per-id checks beside `destination.set_midi_mix`'s);
  - `crates/core-control/src/request.rs` (`COMMAND_NAMES`; `co_producer.plugins`, `co_producer.params`);
  - `crates/core-control/src/co_producer.rs` (`PREVIEWABLE`; `admit` for the two new commands);
  - `crates/core-control/tests/co_producer.rs`, `tests/commands.rs`, `tests/protocol.rs`;
  - `crates/core-ffi/src/control.rs` (the `ControlRequest` variants and their mapping, so the workspace builds);
  - `app/Sources/VizDAWAppModel/MixerCommands.swift` (the thirteen ids in `ids`, answering `.failed(.of(.unavailable, "Not ready yet."))` until Task 5);
  - `app/Sources/VizDAWAppModel/CoProducerCommands.swift` (the two new commands answer the same until Task 5).

**Interfaces:**
- Consumes: 8a's `co_producer::{admit, effective_class, PREVIEWABLE}` and `registry::validate_args`; 8c's `PREVIEWABLE` list.
- Produces:
  ```rust
  // registry.rs
  /// Ruling 1: `-?[0-9]{1,6}(\.[0-9]{1,6})?` within min..=max; the message names the range.
  pub fn decimal(text: &str, min: f64, max: f64) -> Result<f64, String>;
  pub const PLUGIN_REF_MAX: usize = 512;
  pub const MIXER_IDS: &[&str] = &["mixer.set_strip", "mixer.set_output", "mixer.add_send", "mixer.set_send",
      "mixer.remove_send", "mixer.add_insert", "mixer.remove_insert", "mixer.move_insert", "mixer.set_bypass",
      "mixer.set_eq", "mixer.set_param", "mixer.remove_bus", "track.new_plugin_instrument"];
  // request.rs, Command gains:
  CoProducerPlugins { kind: Option<PluginKindArg> },          // PluginKindArg { Instrument, Effect }
  CoProducerParams { slot: u64, window: Option<WindowRef> },
  // co_producer.rs: PREVIEWABLE gains, after 8c's eleven:
  //   "mixer.new_bus", and every MIXER_IDS entry but "track.new_plugin_instrument".
  ```
  In `core-ffi/src/control.rs`, `ControlRequest` gains `CoProducerPlugins { kind: Option<PluginKindTag> }` and `CoProducerParams { slot: u64, window }`.

- [ ] **Step 1: Measure the baseline** (Global Constraints) and write it at the top of the task report.
- [ ] **Step 2: Write the failing tests.**
  - In `tests/co_producer.rs`:
    - `mixer_commands_have_their_class`:
      - `mixer.remove_send`, `mixer.remove_insert` and `mixer.remove_bus` are `Remove`; `track.new_plugin_instrument` is `Add`; the other nine are `Change`;
      - `list_json()` lists all thirteen, with `previewable: true` for the twelve mixer ids and `false` for `track.new_plugin_instrument`;
      - `mixer.new_bus` is listed `previewable: true`, and `effective_class` still gives `Add` without `strip` and `Change` with it.
    - `mixer_changes_only_through_a_preview`:
      - `command.run` of each of the twelve mixer ids, with valid params, is `not_allowed` with 8a Ruling 6's direct-call sentence, and so is `mixer.new_bus {strip: 2}`;
      - a `co_producer.preview` plan holding each is admitted;
      - `command.run track.new_plugin_instrument {plugin: "clap:vizdaw.test.synth4"}` and `mixer.new_bus {name: "Drums"}` are admitted.
    - `mixer_commands_refuse_hostile_input`. Each is `invalid_params`:
      - `strip` or `slot` missing, `0`, `-1`, `"2"` or `2^53`;
      - `mixer.set_strip` with none of its four params, `pan: 101`, or `gain_db` of `"13"`, `"-145"`, `"1e3"`, `"NaN"`, `"inf"`, `"+1"`, `"0x10"`, `"1,5"`, `""` or 70 000 characters;
      - `mixer.set_output` without `bus`, or with `bus: "master"`;
      - `mixer.set_send` with only `strip` and `index`, or `index: 0`;
      - `plugin` that is `""`, `"clap:"`, `"vst:x"`, `"multi-q\u{0}"`, `"au:a\u{202E}b"` or 513 characters;
      - `mixer.set_param` with `value: "NaN"` or without `param`;
      - an unknown param on any of the thirteen.
    - `plugin_calls_only_on_the_co_producer_socket`: on a developer server, `co_producer.plugins` and `co_producer.params` are `not_allowed` "only on the Co-Producer socket". On the Co-Producer socket they are admitted with no open request.
    - `plugin_calls_refuse_hostile_input`: `co_producer.plugins {kind: "synth"}`, `{kind: 1}` and `{extra: 1}`, and `co_producer.params` with no `slot`, `slot: 0` or `slot: "a"`, are each `invalid_params`.
  - In `tests/commands.rs`:
    - the registry count is the baseline plus 13, and `COMMAND_NAMES.len()` is the baseline plus 2;
    - the minimal-shape list gains `co_producer.plugins`;
    - `decimal_parses_only_plain_decimals`: `"-6"`, `"-6.5"`, `"0"` and `"12"` parse in `-144..=12`, and Step 2's bad forms each give an `Err` naming the range.
  - In `tests/protocol.rs`: the round-trip parse of `co_producer.params {slot: 7}` and `co_producer.plugins {kind: "effect"}`.
- [ ] **Step 3: Run** `cargo test -p core-control`. Expected: FAIL.
- [ ] **Step 4: Implement** Ruling 1's entries and checks, Ruling 3's reference shape, Ruling 5's two protocol commands, and the `PREVIEWABLE` additions.
  - **The cross-param checks** (at least one of a set; `bus` present) go beside `destination.set_midi_mix`'s, as `registry::validate_args` runs them for both sockets.
  - **The two other crates.** `core-ffi/src/control.rs` maps the new commands. Swift gets the temporary arms named in Files, so `everyRegisteredMenuCommandHasAHandler` passes.
- [ ] **Step 5: Run** `cargo test -p core-control -p core-ffi`. Expected: PASS, including `names.rs`.
- [ ] **Step 6: End-of-task checks**, including the Swift build and test. **Commit** `feat(control): Co-Producer mixer commands and plug-in reads (plan 8d Task 1)`.

### Task 2: What the preview says about the mixer (`core-model`)

**Files:**
- Modify:
  - `crates/core-model/src/diff.rs` (Ruling 4; `describe_with`);
  - `crates/core-model/tests/diff.rs`.

**Interfaces:**
- Consumes: 8a's `diff::{describe, shown_name, ChangeItem}`; 8b's strip items, marks and `ChangeItem.co_producers`; `MixerCmd`.
- Produces:
  ```rust
  /// Name and current value of a running plug-in's parameter, if known.
  pub type ParamLookup<'a> = &'a dyn Fn(PluginInstanceId, ParamId) -> Option<(String, f64)>;
  /// `describe`, with parameter names and present values from `params` (Ruling 4).
  pub fn describe_with(before: &Document, after: &Document, params: ParamLookup) -> Vec<ChangeItem>;
  /// The strip an instrument's own slot belongs to: its destination's `Instrument` strip with output 0.
  pub fn instrument_strip(doc: &Document, slot: PluginInstanceId) -> Option<StripId>;
  pub const MAX_PARTS: usize = 12;
  ```

- [ ] **Step 1: Write the failing tests** in `tests/diff.rs`. They use a fixture with strips Kick, Snare and Bass (unmarked), bus "Drums (Co-Producer)" (marked), master, and an instrument destination with its strip. Each test executes the `MixerCmd`s on a `History` and compares `describe(before, after)`.
  - `strip_levels_are_items`:
    - `SetGain` −2 → −6 on Kick gives "Strip Kick" / "volume −2.0 dB → −6.0 dB";
    - −144 shows as "−∞ dB";
    - `SetPan` −0.3 gives "pan centre → 30% left";
    - mute and solo together give "muted, soloed".
  - `routing_is_an_item`: `SetOutput` Kick → Drums gives "output Master → Drums (Co-Producer)", and the item's `co_producers` is false.
  - `sends_are_items`:
    - add gives "send to Drums (Co-Producer) added (0.0 dB, post-fader)";
    - a level edit gives "send to Drums (Co-Producer) 0.0 dB → −3.0 dB";
    - pre-fader and off together give "now pre-fader, off";
    - remove gives "deleted";
    - two sends to one bus, with the second edited, give exactly one part.
  - `inserts_are_items`:
    - an add at 2 gives "insert Test Gain added at 2";
    - a remove of slot 1 of 3 gives only "insert … deleted", with no moves;
    - moving 3 → 1 gives "insert … moved 3 → 1";
    - bypass gives "bypassed";
    - `RemoveBus` of a sidechain source gives "sidechain Drums (Co-Producer) → none".
  - `eq_is_an_item`: `SetEq` from none to Multi-Q gives "EQ Multi-Q added", and a swap gives "EQ A → B".
  - `parameters_are_items`:
    - with a lookup giving ("Gain", 0.5), `SetPluginParams` to 0.75 gives "Test Gain Gain 0.5 → 0.75";
    - with an overlay value before, the old value is the overlay's;
    - with `describe`, it is "parameter 3 set to 0.75";
    - an instrument slot's parameter joins its main strip's item.
  - `removing_a_bus_shows_what_it_fed`: `RemoveBus` of Drums, which Kick outputs to and Snare sends to, gives three items: "Strip Drums (Co-Producer)" / "deleted"; "Strip Kick" / "output Drums (Co-Producer) → Master"; and "Strip Snare" / "send to Drums (Co-Producer) deleted".
  - `long_items_are_capped`: 20 parameter changes on one strip give 12 parts then "and 8 more changes".
  - `mixer_names_are_shown_safely`: a plug-in named `"a\u{202E}b"` shows through `shown_name`.
- [ ] **Step 2: Run** `cargo test -p core-model --test diff`. Expected: FAIL.
- [ ] **Step 3: Implement** Ruling 4 in `diff.rs`, after 8b's strip items, in display order.
- [ ] **Step 4: Run** `cargo test -p core-model`. Expected: PASS, including 8a's `co_producer_requests` proptest and 8c's note and marker items.
- [ ] **Step 5: End-of-task checks. Commit** `feat(model): mixer changes in the Co-Producer preview (plan 8d Task 2)`.

### Task 3: Installed plug-ins, references and parameters (`core-ffi`)

**Files:**
- Create:
  - `crates/core-ffi/src/mixer_chores.rs` (`resolve_plugin`, `plugin_ref_text`, `co_producer_plugins`);
  - `crates/core-ffi/src/session/co_producer_mixer.rs` (`co_producer_params`);
  - `crates/core-ffi/tests/mixer_chores.rs`.
- Modify:
  - `crates/core-ffi/src/session.rs` (`mod co_producer_mixer;` only);
  - `crates/core-ffi/src/lib.rs`;
  - `crates/core-ffi/src/control.rs` (`ControlResult::{CoProducerPlugins, CoProducerParams}` and their JSON; `plugin` on each slot and `output_name` on each strip in `state.get mixer`);
  - `app/Tests/VizDAWAppModelTests/FakeSession.swift` (the stub).

**Interfaces:**
- Consumes: Task 1's ids and requests; `catalog::with`, `Catalog::preferred`, `status_of`; `bundled::multiq`; `MULTIQ_CLAP_ID`, `MULTIQ_AU_ID`; `Session::params`'s reading of the host.
- Produces:
  ```rust
  // mixer_chores.rs
  /// Ruling 3. `want` is the kind the command needs.
  pub fn resolve_plugin(text: &str, want: PluginKind) -> Result<PluginRef, VizError>;
  /// The reference text for `p`: "multi-q" for the bundled Multi-Q, else "clap:<id>" or "au:<id>".
  pub fn plugin_ref_text(p: &PluginRef) -> String;
  #[derive(uniffi::Record)] pub struct CoProducerPluginRow { pub plugin: String, pub name: String, pub maker: String,
      pub format: String, pub kind: PluginKindTag, pub version: String }
  #[derive(uniffi::Record)] pub struct CoProducerPluginsRow { pub plugins: Vec<CoProducerPluginRow>, pub truncated: bool }
  #[uniffi::export] pub fn co_producer_plugins(kind: Option<PluginKindTag>) -> CoProducerPluginsRow; // never fails
  pub const MAX_PLUGINS: usize = 5000;
  pub const MAX_PARAMS: usize = 1000;
  // session/co_producer_mixer.rs
  #[derive(uniffi::Record)] pub struct CoProducerParamsRow { pub slot: u64, pub plugin: String, pub running: bool,
      pub params: Vec<ParamRow> }
  impl Session { pub fn co_producer_params(&self, slot: u64) -> Result<CoProducerParamsRow, VizError>; }
  // control.rs
  // ControlResult::CoProducerPlugins { plugins: Vec<CoProducerPluginRow>, truncated: bool } → {"plugins", "truncated"}
  // ControlResult::CoProducerParams { doc: String, row: CoProducerParamsRow } → {"doc", "slot", "plugin", "running", "params"}
  ```

- [ ] **Step 1: Write the failing tests** in `tests/mixer_chores.rs`, through a `serial` helper. The catalog holds the test bundle (`vizdaw.test.gain`, `vizdaw.test.ducker`, `vizdaw.test.synth4`, …) and a fake Multi-Q set by `debug_set_multiq`.
  - `plugins_resolve_by_the_plugin_managers_rules`:
    - `clap:vizdaw.test.gain` as an effect is its catalog `PluginRef`;
    - `clap:vizdaw.test.synth4` as an effect is `Err` "<its catalog name> is an instrument, not an effect.", and as an instrument resolves;
    - after `PluginCatalog::set_enabled(gain, false)` it is `Err` "… is turned off in the Plugin Manager.";
    - `clap:vizdaw.test.none` is `Err` "No installed plug-in is clap:vizdaw.test.none.";
    - a name with a bidi mark shows through `shown_name` in the message.
  - `multiq_is_the_bundled_clap`:
    - `multi-q`, `clap:<MULTIQ_CLAP_ID>` and `au:<MULTIQ_AU_ID>` each resolve to `bundled::multiq()`, whose format is `Clap`;
    - with `debug_set_multiq(None)`, each is `Err` "Multi-Q is not available in this build.";
    - `multi-q` as an instrument is `Err` "Multi-Q is an effect, not an instrument.".
  - `the_list_offers_only_usable_plugins`:
    - `co_producer_plugins(None)` starts with `multi-q` then `built-in-synth`, both `format: "built_in"`;
    - next come the test bundle's instruments, then its effects, by name;
    - no row has `MULTIQ_CLAP_ID` or `MULTIQ_AU_ID` in its `plugin`;
    - a disabled plug-in is absent;
    - `Some(Effect)` gives effects only, with `multi-q` first.
  - `reference_text_round_trips`: for every row, `resolve_plugin(row.plugin, row.kind)` succeeds, and `plugin_ref_text` of the result equals `row.plugin`.
  - `params_answer_for_this_song_only`:
    - in a session with a test gain insert not yet running, `co_producer_params(slot)` is `running: false` with no params;
    - a slot ID the document lacks is `Err` "This song has no slot 999.";
    - `u64::MAX` is `Err`, with no panic.
  - `mixer_state_names_plugins`: `state.get mixer` JSON gives the insert `"plugin": "clap:vizdaw.test.gain"`, a Multi-Q EQ `"plugin": "multi-q"`, and each strip an `output_name`.
  - `plugin_results_json`: both results serialise to Ruling 5's keys.
  - **The running case.** `params_list_a_running_plugin`, `#[cfg(target_os = "macos")]`, in a child process with the temp home, as `crates/core-ffi/tests/params.rs` does: a loaded test gain answers `running: true`, and its params match `Session::params(slot)` without the hidden ones.
- [ ] **Step 2: Run** `cargo test -p core-ffi --test mixer_chores`. Expected: FAIL.
- [ ] **Step 3: Implement** Ruling 3 and the `state.get mixer` additions. `co_producer_params` reads the host as `Session::params` does, and changes nothing. **Swift:** `FakeSession` gains the stub.
- [ ] **Step 4: Run** `cargo test -p core-ffi`. Expected: PASS, including 6g's `default_eq`, `new_channel_eq` and `install_multiq`.
- [ ] **Step 5: End-of-task checks**, including the Swift build and test. **Commit** `feat(ffi): the plug-ins Co-Producer may use, by the Plugin Manager's and Multi-Q's rules (plan 8d Task 3)`.

### Task 4: Mixer commands, their previews and the yes (`core-ffi`)

**Files:**
- Modify:
  - `crates/core-ffi/src/mixer_chores.rs` (`commands_for`, `PluginLookup`);
  - `crates/core-ffi/src/session/co_producer_mixer.rs` (`run_mixer`; the live `PluginLookup`);
  - `crates/core-ffi/src/tracks.rs` (an instrument track on a given destination);
  - `crates/core-ffi/src/co_producer.rs` (the arms of `preview_items` for the twelve mixer ids and `mixer.new_bus`; `composers_work` by Ruling 2; `describe_with`);
  - `crates/core-ffi/tests/mixer_chores.rs`;
  - `app/Tests/VizDAWAppModelTests/FakeSession.swift` (the stub).

**Interfaces:**
- Consumes: Task 1's ids and params; Task 2's `describe_with`, `instrument_strip`; Task 3's `resolve_plugin`; 8a's `Planned`, `ArgValue`, `preview_items`, `co_producer_take`; 8b's marks.
- Produces:
  ```rust
  // mixer_chores.rs
  /// What `commands_for` asks of the plug-in host; the Session's reads the running plug-ins,
  /// and tests pass a fake.
  pub trait PluginLookup {
      fn resolve(&self, text: &str, want: PluginKind) -> Result<PluginRef, VizError>;
      fn params(&self, slot: PluginInstanceId) -> Option<Vec<ParamRow>>;   // None: not running
  }
  /// The core-model commands for one of MIXER_IDS but `track.new_plugin_instrument`, or for
  /// `mixer.new_bus` with `strip`, in order; Ruling 1's refusals.
  pub fn commands_for(doc: &Document, id: &str, args: &[(String, ArgValue)], host: &dyn PluginLookup)
      -> Result<Vec<Command>, VizError>;
  // session/co_producer_mixer.rs
  impl Session {
      /// Runs one of MIXER_IDS as one step named its title (Ruling 1); `Busy` while an edit is open.
      pub fn run_mixer(&self, id: String, args: Vec<ControlArg>) -> Result<DocSnapshot, VizError>;
  }
  ```

- [ ] **Step 1: Write the failing tests** in `tests/mixer_chores.rs`, on a saved session.
  - **The fixture.** Strips Kick, Snare and Bass (the composer's), with a test gain insert on Bass. A request makes a bus "Drums" by `mixer.new_bus`, which 8b marks "Drums (Co-Producer)". The fake `PluginLookup` resolves through Task 3 and answers params for the Bass insert: "Gain", 0–1, value 0.5.
  - `commands_are_the_mixers_own`:
    - `mixer.set_strip {gain_db: "-6", pan: -30, mute: true}` builds `[SetGain −6.0, SetPan −0.3, SetMute true]`;
    - `mixer.set_output {bus: null}` builds `Output::Master`;
    - `mixer.add_insert {index: 1}` builds `AddInsert` at 0;
    - `mixer.set_send {index: 1, level_db: "-3"}` keeps the send's other fields;
    - `mixer.set_param` builds one `SetPluginParams` pair.
  - `the_strip_decides_who_says_yes`:
    - **Free.** `[mixer.add_insert {strip: Drums, plugin: "multi-q"}]` previews with `needs_yes: false`, and `[mixer.set_strip {strip: Drums, gain_db: "-3"}]` does too.
    - **Asks.** `[mixer.add_insert {strip: Drums, …}, mixer.set_output {strip: Kick, bus: Drums}]` previews `needs_yes: true`. Exactly the Kick item has `composers_work: true`, with "output Master → Drums (Co-Producer)".
    - **Asks, for Co-Producer's own insert.** Co-Producer adds an insert on Bass, with a yes. Then `mixer.set_bypass` and `mixer.set_param` on it each preview `needs_yes: true`.
    - **Asks, for a new bus with a send.** `mixer.new_bus {strip: Snare, name: "Verb"}` previews "Strip Verb (Co-Producer)" / "added" with `composers_work: false`, and "Strip Snare" / "send to Verb (Co-Producer) added (0.0 dB, post-fader)" with `composers_work: true`.
  - `removing_a_bus_shows_what_it_fed`: with Kick routed to Drums, `[mixer.remove_bus {strip: Drums}]` previews `needs_yes: true`, with Task 2's items for Drums and Kick.
  - `mixer_previews_match_what_the_apply_does`: for each of the twelve mixer ids:
    - `co_producer_take` then `run_mixer` with the same args gives a document whose `describe_with` from the start equals the preview's items;
    - a hand `set_gain` between the preview and the take makes the take fail with "The song changed since the preview; preview again.".
  - `odd_mixer_targets_are_refused_at_preview`. Each previews to `Err` with Ruling 1's message, and the plan runs nothing:
    - routing Drums to a bus that feeds Drums gives the cycle message;
    - `mixer.set_output {strip: <master>}` gives "Master has no output to set.";
    - `mixer.add_send {bus: Kick}` gives "Strip <Kick's id> is not a bus.";
    - `mixer.remove_send {index: 9}` gives "Strip Kick has 0 sends.";
    - `mixer.add_insert {index: 99}` gives "Strip Bass has 1 inserts.";
    - `mixer.remove_insert` of Bass's EQ slot gives "Slot <n> is not an insert.";
    - `mixer.remove_bus {strip: Kick}` gives "Strip <id> is not a bus.";
    - a slot ID from another session gives "This song has no slot <n>.".
  - `parameters_need_a_running_plugin`. Each previews to `Err`, with no value sent to the fake:
    - the fake answering `None` gives "The plug-in in slot <n> is not running yet; read plugin_params again in a moment.";
    - `value: "2"` gives "Gain is 0 to 1.";
    - `value: "0.5"` on a stepped parameter gives "<name> takes whole values.";
    - a hidden parameter, or an unknown one, gives "Slot <n> has no parameter <p>.".
  - `run_mixer_is_one_step`:
    - outside a request, `mixer.set_strip` with three params is one undo step "Set Strip";
    - inside a request, it joins "Co-Producer: <Title>".
  - `new_plugin_instruments_are_marked`:
    - in a request, `run_mixer("track.new_plugin_instrument", [plugin: "clap:vizdaw.test.synth4"])` adds one destination named after the plug-in, one instrument track playing it, and its strip, all marked and named "… (Co-Producer)";
    - `plugin: "built-in-synth"` gives the same track `track.new_instrument` makes;
    - `plugin: "multi-q"` is `Err` "Multi-Q is an effect, not an instrument.", and nothing is added.
  - `run_mixer_refuses_hostile_input`: an unknown id, a missing arg, a `plugin` of 70 000 characters, and `u64::MAX` as `strip` are each `Err`, with no panic and no change.
- [ ] **Step 2: Run** `cargo test -p core-ffi --test mixer_chores`. Expected: FAIL.
- [ ] **Step 3: Implement** Rulings 1 and 2.
  - **`commands_for`** reads the args by name, as 8c's `chores::command_for` does. Refusals found from the document come before any command is built.
  - **`preview_items`** gains one arm for the twelve ids and `mixer.new_bus`:
    1. `bundled::arm` the clone;
    2. execute `commands_for(h.doc(), …)` in order;
    3. describe with `describe_with` and the live lookup;
    4. set each item's `composers_work` by Ruling 2.
  - **`run_mixer`** follows `new_bus_from`'s shape:
    1. it refuses with `Busy` while an edit is open;
    2. it arms, begins one transaction named the title, and executes `commands_for`. An error aborts the transaction;
    3. it commits, reloads the host and marks.

    For `mixer.set_param`, the value goes to the running plug-in first, as the typed-value path does. `track.new_plugin_instrument` goes through `tracks.rs` with the new destination.
  - **Swift.** `FakeSession` gains the stub.
- [ ] **Step 4: Run** `cargo test -p core-ffi`. Expected: PASS, including 8a's `co_producer`, 8b's `notes` and 8c's `chores`.
- [ ] **Step 5: End-of-task checks**, including the Swift build and test. **Commit** `feat(ffi): Co-Producer mixer edits with exact previews; the composer's strips need a yes (plan 8d Task 4)`.

### Task 5: Mixer commands in the app (Swift)

**Files:**
- Modify:
  - `app/Sources/VizDAWAppModel/MixerCommands.swift` (the thirteen handlers through `runMixer`, replacing Task 1's arms);
  - `app/Sources/VizDAWAppModel/CoProducerCommands.swift` (`co_producer.plugins`, `co_producer.params`);
  - `app/Tests/VizDAWAppModelTests/MixerCommandsTests.swift` (or the file holding `mixer.new_bus`'s tests), `CoProducerCommandsTests.swift`, `FakeSession.swift`.

**Interfaces:**
- Consumes: Task 3's `coProducerPlugins(kind:)` and `coProducerParams(slot:)`, Task 4's `runMixer(id:args:)`, and `ControlResult.coProducerPlugins` / `.coProducerParams`.
- Produces: the handlers. Each mixer handler answers `.ran(command:doc:window:undoLabel:status:)` with `status` the title, and `track.new_plugin_instrument` with the new track's name.

- [ ] **Step 1: Write the failing Swift tests** with `FakeSession`.
  - `MixerCommandsTests`:
    - `mixerEditsRunThroughOneCall`: each of the thirteen ids calls `runMixer` with its id and args unchanged, then applies the snapshot;
    - a throwing `runMixer` is `.failed` with the session's message, and its `Busy` is `busy`;
    - with an editor gesture open, each is `unavailable` with `EditorModel.gestureOpenMessage`, and `runMixer` is not called.
  - `CoProducerCommandsTests`:
    - `pluginsAnswerTheList`: `co_producer.plugins {kind: "effect"}` calls `coProducerPlugins(kind: .effect)` and answers its rows and `truncated`;
    - `paramsAnswerForTheDocument`: `co_producer.params {slot: 7}` calls the resolved document's `coProducerParams(slot: 7)`, and a throw is `.failed` with its message;
    - `readsEnterNothing`: neither command calls `coProducerEnter`.
    - `noSheetsForMixerEdits`: after a failing `runMixer` reached through `co_producer.apply`, the document has no alert (8a Ruling 13).
- [ ] **Step 2: Run** `scripts/build-ffi.sh && scripts/swift.sh test --filter 'MixerCommands|CoProducerCommands'`. Expected: FAIL.
- [ ] **Step 3: Implement.** The handlers keep `mixer.new_bus`'s guard order: the gesture first, then the session call. Nothing opens a plug-in window.
- [ ] **Step 4: Run** the Step 2 command, then `scripts/swift.sh test`. Expected: PASS, including `NameScanTests`, `noLaneInWhatTheAppShows` and `everyRegisteredMenuCommandHasAHandler`.
- [ ] **Step 5: End-of-task checks**, including `scripts/make-app.sh`. **Commit** `feat(app): Co-Producer mixer edits and plug-in reads in the app (plan 8d Task 5)`.

### Task 6: `vizdaw-mcp`: plug-in tools, `show_mixer` and the mixer view

**Files:**
- Create: `crates/vizdaw-mcp/views/mixer.html`.
- Modify:
  - `crates/vizdaw-mcp/src/lib.rs`, `src/tools.rs`, `src/views.rs` (the fourth entry in `VIEWS`);
  - `crates/vizdaw-mcp/tests/server.rs`, `tests/support/fake_app.rs`.

**Interfaces:**
- Consumes:
  - Task 1's two protocol commands and the thirteen entries in `list_json()`;
  - Task 3's answers and the `state.get mixer` additions;
  - 8c's `MAX_SHOWN`, `views::{VIEWS, wants_views, html}`, `bridge.js` and the `change` tool's states.
- Produces:
  ```rust
  // tools.rs
  pub const LIST_PLUGINS: &str = "list_plugins";
  pub const PLUGIN_PARAMS: &str = "plugin_params";
  pub const SHOW_MIXER: &str = "show_mixer";
  /// Ruling 5's text; `mixer_answer` also gives the view's structuredContent.
  pub fn plugins_answer(result: &Value) -> String;
  pub fn params_answer(result: &Value) -> String;
  pub fn mixer_answer(mixer: &Value) -> (String, Value);
  // views.rs: VIEWS gains ("ui://vizdaw/mixer", "show_mixer").
  ```

- [ ] **Step 1: Write the failing tests** in `tests/server.rs`. The fake app answers `co_producer.plugins`, `co_producer.params` and `state.get mixer` from scripts.
  - `plugin_tools_are_listed`:
    - `list_plugins`, `plugin_params` and `show_mixer` have Ruling 5's descriptions and `readOnlyHint: true`;
    - `list_plugins`' `kind` is an `enum` of `instrument effect`, and `plugin_params`' `slot` is an integer with `minimum` 1;
    - `track_new_plugin_instrument` is listed, with `plugin`'s `maxLength` 512;
    - `mixer_new_bus` has no `strip` property;
    - no tool is named after any of the twelve mixer change and remove ids;
    - `change`'s description holds Ruling 5's added sentence.
  - `plugins_reach_the_app`:
    - `list_plugins {kind: "effect"}` sends `co_producer.plugins {kind: "effect"}`;
    - the text's first line is Ruling 5's, then "- multi-q: Multi-Q, built in, effect" and "- clap:vizdaw.test.gain: Test Gain by <maker>, effect";
    - a row with an empty maker has no " by ".
  - `params_reach_the_app`:
    - `plugin_params {slot: 7}` sends `co_producer.params {slot: 7}`, and its text has "- 3: Gain = 0.5 (0 to 1)";
    - a stepped parameter's line ends ", whole values)";
    - `running: false` gives Ruling 5's not-running sentence.
  - `show_mixer_lists_strips`:
    - the text starts "Mixer:";
    - Kick's line is "- [2] Kick (Audio): volume −6.0 dB, pan centre, to Drums (Co-Producer); EQ Multi-Q; inserts none; sends none";
    - the bus's line ends " — added by Co-Producer".
  - `long_mixers_are_capped`: 500 strips give 200 lines then "- and 300 more", and 300 plug-ins give 200 lines then "- and 100 more; pass kind to narrow the list".
  - `plugin_tools_refuse_hostile_arguments`. Each is `isError: true` before any socket call:
    - `list_plugins` with `kind: "synth"` or an unknown property;
    - `plugin_params` with no `slot`, `slot: 0`, `slot: "7"` or `slot: 1.5`;
    - `show_mixer` with any property.
  - `mixer_names_are_shown_safely`: a strip named `"a\u{202E}b"` and a plug-in named `"x\u{2028}y"` show with U+FFFD in the text and in `structuredContent`.
  - `the_mixer_view_only_for_hosts_that_ask`:
    - with the extension, `show_mixer` has `_meta.ui.resourceUri` "ui://vizdaw/mixer", `resources/list` has it with `MIME`, and the result carries `structuredContent.view` `"mixer"`;
    - without it, and at protocol 2025-03-26, none of these appear, and the text is byte for byte the same.
  - `mixer_view_draws_names_as_text`:
    - `mixer.html` holds none of 8c's forbidden strings (`innerHTML`, `outerHTML`, `insertAdjacentHTML`, `document.write`, `eval(`, `new Function`, `http:`, `https:`, `src=`, `<link`);
    - it holds `textContent` and `<!--BRIDGE-->`;
    - `resources/read ui://vizdaw/mixer` begins `<!DOCTYPE html>` with no `<!--` marker left.
- [ ] **Step 2: Run** `cargo test -p vizdaw-mcp`. Expected: FAIL.
- [ ] **Step 3: Implement** Ruling 5's tools, answers and view. The view draws from `structuredContent`, and with no result or an `isError` result shows the text content as it is (8c Ruling 7).
- [ ] **Step 4: Run** `cargo test -p vizdaw-mcp -p core-control`. Expected: PASS, including `names.rs`'s scan of `vizdaw-mcp/src` and `vizdaw-mcp/views`.
- [ ] **Step 5: End-of-task checks. Commit** `feat(mcp): list_plugins, plugin_params, show_mixer and the mixer view (plan 8d Task 6)`.

### Task 7: README checks, the e2e, the backlog and the gates

**Files:**
- Modify:
  - `app/README.md` (new steps after the last one);
  - `crates/vizctl/tests/e2e.rs`;
  - `docs/feature-backlog.md`.

**Interfaces:**
- Consumes: Tasks 1–6. No production code, unless a step finds a bug. Such a bug is fixed in its owner's module, with a regression test.

- [ ] **Step 1: Add the e2e** `co_producer_mixer_over_mcp`, `#[ignore]`, in the style of 8c's `co_producer_chores_over_mcp` and with 8a's MCP helper. Its client declares `elicitation: {}` and the MCP Apps extension, as 8c's does.
  - **Start.** Launch the app and switch Co-Producer on over the developer channel. Run `file.create_demo`, then `file.save {path: <home>/Song.vizdaw}`. Take two of the demo's channel strips, A and B, from `state.get mixer`.
  - **The plug-in list.**
    - `list_plugins {kind: "effect"}` has the line "- multi-q: Multi-Q, built in, effect";
    - no line names `MULTIQ_CLAP_ID` or `MULTIQ_AU_ID`;
    - `list_plugins {kind: "instrument"}` has "- built-in-synth: Built-in Synth, built in, instrument".
  - **"Set up a drum bus."**
    1. `begin_request {name: "set up a drum bus", asked: "put my drums on a bus"}`;
    2. `mixer_new_bus {name: "Drums"}`, then `show_mixer`: a line "[<id>] Drums (Co-Producer) …" ends " — added by Co-Producer";
    3. `change [mixer.add_insert {strip: <bus>, plugin: "multi-q"}]` applies with no elicitation, and its `structuredContent.state` is `"applied_without_asking"`;
    4. `change [mixer.set_output {strip: A, bus: <bus>}, mixer.set_output {strip: B, bus: <bus>}]` emits `elicitation/create`, whose message has "- Strip <A>: output Master → Drums (Co-Producer)" and the same line for B. Answer `decline`: the result is "The composer said no. Nothing changed.", and `state.get mixer` shows A and B still on master;
    5. the same `change`, answered `accept` with `allow: true`, applies;
    6. `end_request`.

    Then:
    - the undo label is "Co-Producer: Set Up a Drum Bus";
    - `state.get mixer` shows the bus's first insert `"plugin": "multi-q"`, and A and B with `output` the bus;
    - one `edit.undo` over the developer channel removes the bus and puts A and B back on master.
  - **A plug-in parameter.** Redo, then in a request:
    1. poll `plugin_params {slot: <the Multi-Q insert>}` until it is running, for at most 10 s;
    2. take a continuous parameter, then `change [mixer.set_param {slot, param, value: <its min>}]`. It applies without asking, because the bus is Co-Producer's;
    3. `plugin_params` shows that value.
  - **The composer's fader.** `change [mixer.set_strip {strip: A, gain_db: "-6"}]` asks with "volume <v> → −6.0 dB", and an accept applies it.
  - **An instrument track.** `track_new_plugin_instrument {plugin: "built-in-synth"}` adds a track named "… (Co-Producer)".
  - **The mixer view.**
    - `tools/list`'s `show_mixer` has `_meta.ui.resourceUri` "ui://vizdaw/mixer";
    - `resources/read` of it starts `<!DOCTYPE html>`;
    - `show_mixer`'s `structuredContent.strips` has the bus with `co_producer: true`.
  - **The gate.** A raw client on the Co-Producer socket gets:
    - `not_allowed` for `command.run mixer.set_output {strip: A, bus: null}` and for `command.run mixer.new_bus {strip: A}`;
    - `failed` "No installed plug-in is clap:vizdaw.e2e.none." for `co_producer.preview [mixer.add_insert {strip: A, plugin: "clap:vizdaw.e2e.none"}]`.
- [ ] **Step 2: Write the README steps** in `app/README.md`, after the last step, under a heading "Plan 8d, Co-Producer mixer and plug-ins:". Each step states what to do and what is seen, and says "track":
  1. Ask Claude which plug-ins it can use. It lists your installed instruments and effects and the built-in Multi-Q and Built-in Synth. Turn a plug-in off in the Plugin Manager and ask again: it is gone.
  2. Ask Claude to "set up a drum bus". A bus "Drums (Co-Producer)" appears with the badge. Claude then shows exactly which of your tracks' strips will go to it, and waits. Say no: your strips stay where they were, and the bus stays. Ask again and say yes: they go to the bus, as one undo step "Co-Producer: …".
  3. Ask Claude to put an EQ or compressor on its own bus: it happens without a question. Ask it to put one on your vocal track: it asks first.
  4. Ask Claude to add a reverb send from your snare to a new reverb bus. It asks first, and shows the send's level and whether it is pre-fader.
  5. Ask Claude to turn your bass down 3 dB and pan it a little left. It shows "volume … → …" and "pan …", and asks.
  6. Open the plug-in window of an insert Co-Producer added to your track, and ask Claude to change one of its settings. It asks first, because the track is yours. The control moves when you say yes, and Undo puts it back.
  7. Ask Claude to remove its own bus while your tracks feed it. It asks, and lists your strips that go back to the master.
  8. Ask Claude for a new instrument track with a named installed instrument. It appears named "… (Co-Producer)", playing that instrument.
  9. Ask Claude to load a plug-in you turned off, or an instrument as an effect: it says why not, and nothing changes.
  10. In Claude's desktop app or another host that shows MCP Apps views, ask Claude to "show the mixer". A view shows each strip's volume, pan, routing, EQ, inserts and sends, with Co-Producer's strips marked. In a host without views, the same answer comes as text.
  11. Make Multi-Q the bus EQ through Claude, change the audio sample rate, and play: the EQ still shapes the sound, as it does when you load it yourself.
- [ ] **Step 3: Update the backlog** (`docs/feature-backlog.md`).
  - **The "Co-Producer over MCP" row.** Note 8d's delivery:
    - the twelve mixer commands and `track.new_plugin_instrument`;
    - the installed plug-in list and parameter read;
    - the strip-decides yes rule;
    - the mixer snapshot view.
  - **Closes and notes.**
    - Close 8c's row "The mixer snapshot view".
    - Note against "Previews for the remaining change and remove commands" that 8d added thirteen, and that 8e remains.
  - **New rows:**
    - "Meter peaks in Co-Producer's mixer read, for gain staging from levels" — 8e;
    - "Move an insert to another strip, and set a sidechain, through Co-Producer";
    - "Empty a strip's EQ slot through Co-Producer";
    - "Swap the plug-in of an existing instrument through Co-Producer (a change on the composer's sound)";
    - "Load a plug-in preset or saved state through Co-Producer";
    - "Rename a strip through Co-Producer";
    - "After the release-fixes merge: make `lane.set_output` and `destination.set_sound` previewable, and class `automation.release_all` for Co-Producer" — 8e;
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
  - the e2e, `VIZDAW_E2E_APP=dist/VizDAW.app cargo test -p vizctl --test e2e -- --ignored --nocapture --test-threads 1`: the baseline's ignored tests plus one run, and all pass, 6g's `multiq_install_and_switch_over_the_channel` included.

  Then `pgrep -lf MacOS/VizDAW` and `pgrep -lf vizdaw-mcp` print nothing. Report against the baseline: registry +13, `COMMAND_NAMES` +2, `PREVIEWABLE` +13, error codes unchanged, README +11, e2e +1, `VERSION` unchanged, and no change under `crates/core-host` or `crates/platform-macos` (`git diff --stat <baseline HEAD> -- crates/core-host crates/platform-macos` is empty).
- [ ] **Step 5: Commit** `docs(app): Co-Producer mixer README checks, e2e and backlog (plan 8d Task 7)`.

## Self-Review

1. **Spec coverage (the §12 row 8d).**
   - **Mixer editing** (§10 gap 4):
     - new bus: the existing `mixer.new_bus`, now previewable with `strip`;
     - route a track to a bus: `mixer.set_output`;
     - add and set a send: `mixer.add_send`, `mixer.set_send`;
     - add, remove and reorder inserts: `mixer.add_insert`, `mixer.remove_insert`, `mixer.move_insert`, `mixer.set_bypass`;
     - load a plug-in into an insert or as an instrument: `mixer.add_insert`, `mixer.set_eq`, `track.new_plugin_instrument`;
     - fader, pan, mute and solo: `mixer.set_strip`;
     - a plug-in parameter: `mixer.set_param`.

     All are in Ruling 1 and Tasks 1 and 4. `mixer.remove_bus` lets Co-Producer tidy up its own buses.
   - **The installed plug-in list** (§10 gap 5, §8): Ruling 3 and Task 3. Plug-ins turned off in the Plugin Manager are not offered, and the built-ins are listed.
   - **The mixer snapshot view** (§6.3): Ruling 5 and Task 6, with its text fallback (§5).
   - **The owner's rule on routing and sound** (§6.5, 2026-11-14): Ruling 2, Task 4's `the_strip_decides_who_says_yes`, and the e2e's decline then accept.
   - **Multi-Q (6g):** CLAP as the built-in default, and the refresh only for the bundled Multi-Q. These are in Ruling 3, the Global Constraints, Task 3's `multiq_is_the_bundled_clap`, and README step 11.
   - **The gaps the spec names that are not 8d's.** Colours are 8b's `mixer.set_colour`, and markers are 8c's. Automation points and fade shapes are 8e's, with `region.set_fade_shape` reused from release-fixes. 8d duplicates none of them.
2. **Steps.** Every test step names its tests and assertions with the plan's values: ids, ranges, sentences, item texts and caps. Code steps give signatures and point to the ruling. The tool descriptions are written out, because the spec leaves them to the plan (§11.12).
3. **Type consistency.** These names are spelled the same in every task:
   - `registry::{decimal, PLUGIN_REF_MAX, MIXER_IDS}`, `PREVIEWABLE`;
   - `Command::{CoProducerPlugins, CoProducerParams}`, `ControlRequest::{CoProducerPlugins, CoProducerParams}`, `ControlResult::{CoProducerPlugins, CoProducerParams}`;
   - `diff::{describe_with, ParamLookup, instrument_strip, MAX_PARTS}`;
   - `mixer_chores::{resolve_plugin, plugin_ref_text, co_producer_plugins, commands_for, PluginLookup, MAX_PLUGINS, MAX_PARAMS}`, `CoProducerPluginRow`, `CoProducerPluginsRow`, `CoProducerParamsRow`;
   - `Session::{co_producer_params, run_mixer}` in `session/co_producer_mixer.rs`;
   - `tools::{LIST_PLUGINS, PLUGIN_PARAMS, SHOW_MIXER, plugins_answer, params_answer, mixer_answer}`, and the URI `ui://vizdaw/mixer`.
4. **Review Focus.** Each of the five lines names the tests that pin it, in Tasks 1, 3, 4 and 6.
5. **Proportion.** Seven tasks, as the owner asked for 26.0.0 by 2026-11-20. Code appears only as signatures and the exact text the spec leaves to the plan. The plan reuses a lot:
   - every core-model `MixerCmd`, unchanged;
   - `new_bus_from`'s transaction shape;
   - the Plugin Manager's catalog rules and 6g's `multiq()`;
   - 8a's preview and token, 8b's marks, and 8c's view machinery.
6. **Owner decisions.** None are open. Four calls are stated for the owner's review:
   - the strip decides who says yes, so Co-Producer's own insert on a composer's track still asks (Ruling 2);
   - new buses are made first, and routing to them is asked separately, so a "no" leaves the empty bus until Undo (Ruling 1);
   - every Multi-Q reference loads the bundled CLAP, never the AU (Ruling 3);
   - the mixer read shows fader levels, not meter peaks, until 8e (Not in this plan).
