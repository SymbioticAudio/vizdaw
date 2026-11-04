# Sub-project 7 — UI polish: one window, main views, panes, themes

Status: draft from the owner brainstorm of 2026-11-03, for the owner's review.
Parent spec: `docs/superpowers/specs/2026-10-15-vizdaw-design.md` (§11 order: SP7 runs right after SP5, then plans 5g-1 and 5g-2, then SP6).
Mockups (gitignored): `.superpowers/brainstorm/11155-1793752858/content/` — `layout.html` (A chosen), `panes.html`, `mixview.html`, `strip.html`.

Throughout, **(owner)** marks a decision the owner made in the brainstorm; **(controller's call)** marks a decision made while writing this that the owner may overturn; anything else is left to the builder.

## 1. Intent

The owner wants to start making music in VizDAW. They come from Logic and last used Studio Vision Pro 4.5 over twenty years ago, so the first obstacle is learning a new visual workflow. **(owner)** The goal is a familiar, modern single-window layout of the kind current DAWs use: the main tracks or mixer always visible, with panes around it that load the editor or tool you need, instead of SVP's many popup windows. Screens are larger and sharper now, so the window can show more at once.

Success: the owner can sit down with a project and record, arrange, edit and mix in one window without hunting for windows. They can switch between an arranging layout and a mixing layout with one key, and the app looks finished in the default theme.

## 2. Goals

1. **One main window per document** with a transport bar across the top and four regions: left, centre, bottom and right. **(owner, layout A)** The left pane runs the full height under the transport bar. The centre (main view) and the bottom and right panes share the rest; the bottom and right panes sit under the centre view, side by side.
2. **Main views.** **(owner)** The centre shows one of several main views, chosen by tabs on the transport bar and by keys:
   - **Arrange view:** the arrangement (tracks and regions) in the centre.
   - **Mix view:** the full mixer fills the window under the transport bar, with no arrangement. **(owner, revised 2026-11-03)** The bottom pane's mixer (§2, item 3) is the resizable alternative: it shows the mixer under the arrangement.
   - **A mixer window:** the mixer can also be opened as its own full window, for example on a second display. **(owner)** It works like today's Mixer window. The pane, the view and the window all show the same mixer state. **The transport stays visible in all three (owner):** in the Arrange view, in the Mix view, and across the top of the mixer window, which has its own copy of the transport bar driving the same transport.
   - Each main view remembers its own pane setup: which panes are shown, what each one shows, and its size. Switching views and back restores both setups. **(owner: "a good start")**
   - The transport, the selection and the playhead are shared by all main views.
3. **Panes choose from their own lists.** **(owner)** Each pane has a view picker at its top and can show only the views on its list. A view on no pane's list stays a floating window.
   - **Bottom pane (owner):** piano roll, list editor, drum grid, audio editor, automation (the Strip Chart), and the mixer.
   - **Left pane:** the selected track's channel strip **(owner)**, the sequence list **(owner, 2026-11-03: "the channel strip is just one thing that could appear there")**, or an inspector for the selected lane or region **(controller's call)**.
   - **Right pane:** controllers (a narrow Strip Chart of the selected lane's controller) **(owner)**. Also the media pool, the plug-in browser and markers **(controller's call)**.
   - **Centre:** a main view only.
   - Toggles on the transport bar and keys show and hide the left, bottom and right panes.
4. **Floating windows stay floating. (owner)** The mixer and the media pool keep their own windows as well as their pane views, so `window.mixer`, `window.media_pool` and the windows the e2e waits on still work. **(controller's call, from the owner's mixer-window answer)** Plug-in windows open as their own windows when you edit a plug-in on the selected track. Sheets stay sheets: Settings, Bounce, Relink, the synth sheets and the alerts. A torn-off editor stays a window, as it is today.
5. **The channel strip. (owner)** The same strip design serves the left pane, the mixer pane and the Mix view. Top to bottom:
   - the colour bar and the name;
   - the input picker;
   - **an EQ slot:** a dedicated insert slot above the others for an EQ plug-in. Empty, it reads "EQ ＋". Loaded, it shows the plug-in's name and bypass, and a click opens the plug-in's window. No curve is drawn on the strip, since a plug-in does not report its curve. **(controller's call)** The EQ slot is a real, saved part of a strip: a small Rust core, FFI and document-format change with undo, so it survives reordering the other inserts. It processes before the other inserts. It cannot be moved among them, and it can be emptied. SP6 fills it in new channels;
   - the other inserts;
   - **aux sends, each a knob (not a slider) with its own level as a typed number;**
   - the output picker: master or a bus. **(controller's call)** VizDAW has no groups or VCAs today, so the owner's "groups/bus" is served by routing the output to a bus. Linked fader groups go to the backlog;
   - **pan, a knob with a typed number (L 24, C, R 10);**
   - **a fader with a much larger cap and a longer throw, with a dB scale and a meter beside it;**
   - **the level as a typed number (−4.2 dB);**
   - mute, solo, record arm and input monitor;
   - the automation mode.

   Every number box can be clicked and typed into. Typed values are hostile-input tested, like every other entry.
6. **Themes. (owner, decided 2026-10-19/20 and confirmed now)** VizDAW adopts the shared `symbiotic-design` package (`SymbioticTokens`, `SymbioticIcons`; `SymbioticBrand` optional) at its released version, with its five themes:
   - **Classic Light (`classic-light`), the default theme (owner, 2026-11-03).** The owner calls it "SVP Light"; that is its design reference, not its name. **(controller's call)** The UI says "Classic Light", because the clean-room rules keep "SVP" and other Opcode names out of the app;
   - Studio Dark (`studio-dark`);
   - Flat (`flat`, Metro-inspired);
   - Console (`console`, realistic, Soundcraft-inspired);
   - SymbioticAudio blue/gold (`symbiotic`).

   The theme is chosen in Settings, applies to the whole app including the mixer, and switches live.

   **Finishing the themes is part of SP7 (controller's call).** In `symbiotic-design` 0.1.0, `flat` and `console` are placeholders, and `classic-light` follows the macOS system colours. 7b therefore designs these palettes in `symbiotic-design` and releases a new tag (0.2.0) that VizDAW pins:
   - `classic-light` gets a palette inspired by SVP 4.5's light look, from memory and the owner's direction. No Opcode assets are used.
   - `flat` and `console` get real palettes.
   - The owner approves each theme by eye from browser mockups before it is coded.
   - Pushing the design repo needs the owner's say-so, as always.

   **Controls follow the family rule in `symbiotic-design/HANDOFF.md`.** Knob and fader design is shared by all SymbioticAudio apps, and the owner prefers flat knobs. **(controller's call)** 7b builds VizDAW's knob, fader and number box as a shared `SymbioticControls` product in `symbiotic-design`, with VizDAW as its first user and EG13 able to adopt it later. Its geometry is agreed with EG13's knob through the HANDOFF.

   **The Console theme's realism is drawn in code** (gradients, shadows, bevels driven by tokens), not from image assets. No photographic or copied textures are used.
7. **Console colouring in every theme. (owner)** The owner is not sold on a realistic Midas mixer, so the mixer follows the theme. Flat themes adopt Midas-style colouring:
   - fader caps coloured by strip type: channel, aux or return, bus, master;
   - knobs coloured by function: gain, EQ bands, sends, pan;
   - an optional colour bar per channel.

   The skeuomorphic look exists only in the Console theme.
8. **Strips and lanes. (controller's call)** Strips belong to destinations and output paths, not lanes. Several lanes can play through one strip, and an external-MIDI lane has none.
   - The left pane shows the strip of the selected lane's path. If two selected lanes share a path, it shows that one strip.
   - A lane without a strip shows the inspector in the left pane.
   - Arm, input and monitor follow plan 5d-2 Ruling 4: the first armed lane edits the strip's input.
   - Strip kinds for colour: a bus fed only by sends is coloured as an aux return, and any other bus as a bus.
9. **Icons:** the app's icons come from `SymbioticIcons`: the owner's glyphs plus the MIT standard set. The app still builds with the fallback icon set when the branded assets are absent, as the design-system spec requires.
10. **Clean-up:** the SP7 rows parked in `docs/feature-backlog.md`:
   - the lane-header meter invalidates only its rect;
   - the meter stays clear of the separator at heights 46–47;
   - the strip's lane picker names its lane;
   - a growing take's waveform follows a tempo change.

11. **The project README keeps up with SP7 (owner, 2026-11-03).** The root `README.md` still says "design phase". Each SP7 plan updates it as part of its last task:
   - **7a** rewrites the status: what works today after SP5. It also corrects the feature list (OMF and AAF import may move after launch; the hosted formats are AU and CLAP; there is no VST3). It adds a short "Getting around" section on the window, the main views, the panes and their keys, and building and running from source.
   - **7b** adds the themes and the default.
   - **7c** adds the audio editor.
   - **7d** gives it a final read for accuracy.

   It stays short: user-facing, plain words, no plan jargon. The disclaimer stays.

## 3. Non-goals

- **No new audio processing.** SP7 is UI. The EQ slot hosts the user's EQ plug-in.
- **Bundling Dusk Audio Multi-Q** is part of SP6, not SP7 **(owner)**. Multi-Q is GPL-3.0 (github.com/dusk-audio/dusk-audio-plugins). It is built from source as CLAP everywhere and AU on macOS, signed, and loaded into every new channel's EQ slot. The fallback is ZL Equalizer 2 with a CLAP build added through clap-juce-extensions. SP7 only makes the EQ slot. Whatever SP6 bundles fills it later, and SP7's code must not assume any particular EQ.
- A built-in VizDAW EQ is for after 1.0, and goes in the backlog.
- No third "Edit" main view in SP7. **(controller's call)** The bottom pane can be made tall instead.
- No change to the document format beyond the EQ slot (§2, item 5) and the per-view pane layout (§6, item 3).
- No new recording, automation or editing features, apart from the audio editor (§6, item 2), which uses only operations the model already has. The existing editors move into panes unchanged.

## 4. Anti-goals: failure even if it works

- **A layout that hides things a user had.** Every window, sheet and editor reachable today must still be reachable, from a pane, a menu or a key.
- **A Logic look-alike.** "Familiar" means the conventions current DAWs share, not Logic's look. No Apple, Logic, Midas, Soundcraft, Steinberg or Opcode artwork, names in the UI, or copied graphics. The clean-room rules hold.
- **A realistic mixer forced on everyone.** Skeuomorphism appears only in the Console theme.
- **Controls too small to use.** Fader caps, knobs and number boxes must be easy to hit with a trackpad on a laptop screen.
- **Hard-coded colours.** Every colour and metric comes from theme tokens, so switching the theme changes everything.
- **Breaking agents and tests.** The control channel, `vizctl` and the e2e keep working. Window and pane state the channel can read or set (`state.get` and the `ui.*` commands) is updated with the layout, not dropped. No GUI or computer-use tools in tests.

## 5. Constraints

- macOS 14 floor for the package, as today. Swift/AppKit plus SwiftUI and Core Animation layers, as the editors use now. CI's macos-15 runner builds with Swift 6.1, so concurrency code must compile there, not only on the newer local toolchain.
- Editor logic stays in Rust where it is cheap (parent spec guideline). New layout state that the control channel reads lives in the app model, with tests.
- Existing keys: ⌘1–⌘4 open the detail editors today, and ⌥⌘1–4 tear them off. The main-view keys must not collide with these. The builder picks the scheme, keeps the current keys working or remaps them deliberately, and records the choice in Settings › Keys. **(controller's call: keep ⌘1–⌘4 for the bottom pane's editors; main views on ⌃1/⌃2 or similar.)**
- **Where things are today** (for the builder's survey):
  - `DocumentView` is a `NavigationSplitView`: the sequence sidebar, then the arrangement over the detail pane in a `VSplitView`, with a transport toolbar.
  - The mixer, the media pool and the Plugin Manager are separate `WindowGroup`s.
  - Plug-in windows, the Modify panel and torn editors are `NSWindow`s.
  - The sheets hang off `DocumentView`.
- Settings and the Plugin Manager stay app-level windows.
- Every new or changed control-channel command gets hostile-input tests. Programmatic and channel calls never open dialogs.
- The theme preference is a user preference, not a project setting. Tests never touch the real preferences.

## 6. Open questions

These go to the owner if the builder check confirms they matter. Otherwise the controller's call above stands.

1. **The sequence list (settled, owner):** it is one of the left pane's views, beside the channel strip and the inspector. A new document's Arrange view opens with the channel strip there.
2. **The audio editor (settled, owner 2026-11-03): built in SP7, plan 7c.** It is a bottom-pane view of the selected audio region:
   - the waveform drawn large, with zoom and a level scale;
   - gain, fade-in and fade-out (with shapes), trim of the start and length, and normalize as gain;
   - each with handles and typed values, using the existing model, commands and undo;
   - **no pan per region (owner)**, which stays on the channel strip;
   - no destructive edits, which stay with the external sample editor.

   It needs no format change.
3. **Where the pane layout is saved.** The controller's call: each main view's layout is saved per document, in the project's UI state with the window size. A new document starts from the default layout of each view.

## 7. Plans

Plans run one after another by subagent-driven development, as SP5's did.

| Plan | Deliverable |
|---|---|
| **7a — Window, main views and panes** | The single-window layout. The transport bar. Arrange and Mix main views (the Mix view is the full mixer; the mixer also sits in the bottom pane and opens as its own window). The left, bottom and right panes with their lists and pickers, and the toggles and keys. Per-view layouts. The selected-track strip in the left pane. The existing editors, the mixer, the media pool and the plug-in browser moved into panes. The control channel updated. README by-eye steps. |
| **7b — Themes, icons and the channel strip** | `symbiotic-design` adopted. The five themes, with SVP Light the default. Settings › Appearance, with live switching. Icons from `SymbioticIcons` with a fallback. The channel strip redesigned as in §2, item 5: EQ slot, send knobs, typed pan, level and sends, the big fader. Console colouring in every theme. |
| **7c — Audio editor** | The bottom pane's audio editor (§6, item 2): the selected region's waveform large, with zoom and a level scale. Gain, fades with shapes, trim and normalize, with handles and typed values, through the existing commands. Control-channel state, hostile-input tests, README steps. |
| **7d — Clean-up** | The SP7 backlog rows (§2, item 10) and the drawing and wording faults that 7a and 7b's own by-eye checks turn up. |
